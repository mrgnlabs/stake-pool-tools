use {
    super::SECONDS_PER_DAY,
    crate::{
        apr_to_apy,
        commands::generate_normalized_stats::EpochStakePoolStats,
        compute_effective_stake_pool_apr, get_epoch_duration,
        get_inflation_rewards_per_validator_stake_account,
        vendors::{
            jito::JitoRewardsLookup,
            marinade::{MarinadeState, StakeRecord, MARINADE_STATE_ADDRESS},
        },
    },
    borsh::BorshDeserialize,
    itertools::Itertools,
    num_traits::ToPrimitive,
    serde::{Deserialize, Serialize},
    solana_client::rpc_client::RpcClient,
    solana_program::{
        borsh0_10::try_from_slice_unchecked,
        pubkey::Pubkey,
        stake::{self},
        stake_history::StakeHistoryEntry,
    },
    solana_runtime::bank::Bank,
    solana_sdk::account::{AccountSharedData, ReadableAccount},
    solana_stake_program::stake_state::StakeState,
    std::{collections::HashMap, fmt::Debug, str::FromStr, sync::Arc},
};

pub fn generate_marinade_stake_pool_meta(
    bank: &Arc<Bank>,
    rpc_client: &RpcClient,
    jito_rewards_lookup: &JitoRewardsLookup,
) -> MarinadeStakePoolMeta {
    let marinade_state_account = bank.get_account(&MARINADE_STATE_ADDRESS).unwrap();

    MarinadeStakePoolMeta::build(
        &rpc_client,
        bank,
        MARINADE_STATE_ADDRESS,
        marinade_state_account,
        jito_rewards_lookup,
    )
}

pub fn generate_marinade_stake_pool_stats(
    rpc_client: &RpcClient,
    target_epoch_meta: &MarinadeStakePoolMeta,
    next_epoch_meta: Option<MarinadeStakePoolMeta>,
) -> EpochStakePoolStats {
    let (
        active_lamports,
        validator_undelegated_lamports,
        activating_lamports,
        deactivating_lamports,
        jito_rewards,
        inflation_rewards,
    ) = target_epoch_meta
        .stake_accounts
        .iter()
        .fold((0, 0, 0, 0, 0, 0), |acc, validator_meta| {
            (
                acc.0 + validator_meta.active_stake,
                acc.1 + validator_meta.undelegated_stake,
                acc.2 + validator_meta.activating_stake,
                acc.3 + validator_meta.deactivating_stake,
                acc.4 + validator_meta.jito_rewards,
                acc.5 + validator_meta.inflation_rewards,
            )
        });

    let epochs_per_year =
        (SECONDS_PER_DAY as f64 * 365.0) / (target_epoch_meta.epoch_duration as f64);

    let apr_potential = (inflation_rewards as f64 + jito_rewards as f64)
        / (active_lamports as f64 + deactivating_lamports as f64)
        * (epochs_per_year as f64);

    let next_epoch_lst_price = if let Some(next_epoch_meta) = next_epoch_meta {
        next_epoch_meta.lst_price
    } else {
        fetch_current_marinade_stake_pool_price(
            &rpc_client,
            Pubkey::from_str(&target_epoch_meta.address).unwrap(),
        )
    };

    let apr_effective = compute_effective_stake_pool_apr(
        target_epoch_meta.lst_price,
        next_epoch_lst_price,
        epochs_per_year as f64,
    );

    let undelegated_lamports = target_epoch_meta.reserve + validator_undelegated_lamports;

    let staked_validator_count = target_epoch_meta
        .stake_accounts
        .iter()
        .fold(HashMap::new(), |mut acc, stake_account| {
            let validator_lamports = acc.get_mut(&stake_account.vote_account_address);
            let stake_account_lamports = stake_account.active_stake
                + stake_account.undelegated_stake
                + stake_account.activating_stake
                + stake_account.deactivating_stake;

            if let Some(validator_lamports) = validator_lamports {
                *validator_lamports += stake_account_lamports;
            } else {
                acc.insert(
                    stake_account.vote_account_address.clone(),
                    stake_account_lamports,
                );
            }

            acc
        })
        .iter()
        .filter(|(_, stake)| **stake >= 1_000_000_000)
        .count() as u64;

    EpochStakePoolStats {
        address: target_epoch_meta.address.clone(),
        manager: target_epoch_meta.manager.clone(),
        provider: "Marinade".to_string(),
        management_fee: target_epoch_meta.management_fee,
        is_valid: true,
        mint: target_epoch_meta.mint.clone(),
        lst_price: target_epoch_meta.lst_price,
        staked_validator_count,
        pool_token_supply: target_epoch_meta.pool_token_supply,
        undelegated_lamports,
        total_lamports_locked: undelegated_lamports
            + active_lamports
            + activating_lamports
            + deactivating_lamports,
        active_lamports,
        activating_lamports,
        deactivating_lamports,
        inflation_rewards,
        jito_rewards,
        apr_baseline: apr_potential,
        apy_baseline: apr_to_apy(apr_potential, epochs_per_year),
        apr_effective,
        apy_effective: apr_to_apy(apr_effective, epochs_per_year),
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MarinadeStakePoolMeta {
    /// Pool address
    pub address: String,
    /// LST mint
    pub mint: String,
    /// Pool manager
    pub manager: String,
    /// Epoch duration in seconds - calculated from first and last slot times
    pub epoch_duration: u64,
    /// Total lamports locked according to the stake pool program accounting
    pub total_lamports: u64,
    /// LST supply according to the stake pool program accounting
    pub pool_token_supply: u64,
    /// LST price
    pub lst_price: f64,
    /// Management fees
    pub management_fee: f64,
    /// Amount of lamports in the reserve - calculated from the reserve account directly
    pub reserve: u64,
    /// Relevant datapoints for each validator in the pool
    pub stake_accounts: Vec<MarinadeStakePoolStakeAccount>,
}

impl MarinadeStakePoolMeta {
    pub fn build(
        rpc_client: &RpcClient,
        bank: &Arc<Bank>,
        marinade_state_address: Pubkey,
        marinade_state_data: AccountSharedData,
        jito_rewards_lookup: &JitoRewardsLookup,
    ) -> Self {
        let marinade_state: MarinadeState = MarinadeState::try_from_slice(
            &marinade_state_data.data()[8..(MarinadeState::serialized_len())],
        )
        .unwrap();

        let stake_list = rpc_client
            .get_account(&marinade_state.stake_system.stake_list.account)
            .unwrap();

        let stake_records = (0..marinade_state.stake_system.stake_list.count)
            .map(|index| marinade_state.stake_system.get(&stake_list.data, index))
            .collect::<Vec<StakeRecord>>();

        let stake_accounts = stake_records
            .into_iter()
            .filter_map(|stake_record| {
                bank.get_account(&stake_record.stake_account)
                    .map(|account| {
                        let StakeState::Stake(_, stake_state) =
                            try_from_slice_unchecked::<StakeState>(account.data()).unwrap()
                        else {
                            unreachable!()
                        };

                        (
                            stake_record.stake_account,
                            (stake_state.delegation.voter_pubkey, account),
                        )
                    })
            })
            .collect::<HashMap<Pubkey, (Pubkey, AccountSharedData)>>();

        let inflation_rewards = get_inflation_rewards_per_validator_stake_account(
            rpc_client,
            bank.epoch(),
            &stake_accounts.keys().cloned().collect_vec(),
        );

        let stake_accounts = stake_accounts
            .iter()
            .map(|(stake_account_address, (vote_account, stake_account))| {
                MarinadeStakePoolStakeAccount::build(
                    &bank,
                    &vote_account,
                    stake_account_address,
                    stake_account,
                    &inflation_rewards,
                    &jito_rewards_lookup,
                )
            })
            .collect_vec();

        let (reserve_pda, _) = MarinadeState::find_reserve_address(&marinade_state_address);
        let reserve_pda = bank.get_account(&reserve_pda).unwrap();
        let reserve = reserve_pda.lamports() - marinade_state.rent_exempt_for_token_acc;

        let epoch_schedule = bank.epoch_schedule();
        let epoch_duration = get_epoch_duration(rpc_client, epoch_schedule, bank.epoch())
            .to_u64()
            .unwrap();

        let lst_price = marinade_state.total_virtual_staked_lamports() as f64
            / marinade_state.msol_supply as f64;

        let management_fee = marinade_state.reward_fee.basis_points as f64 / 10_000.0;

        Self {
            address: marinade_state_address.to_string(),
            manager: marinade_state.admin_authority.to_string(),
            mint: marinade_state.msol_mint.to_string(),
            epoch_duration,
            total_lamports: marinade_state.total_virtual_staked_lamports(),
            pool_token_supply: marinade_state.msol_supply,
            lst_price,
            management_fee,
            reserve,
            stake_accounts,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MarinadeStakePoolStakeAccount {
    /// Validator vote account address
    pub vote_account_address: String,
    /// Stake account address
    pub stake_account_address: String,
    /// Amount of active stake delegated to the validator - calculated from the stake pool's active and transient stake accounts directly
    pub active_stake: u64,
    /// Amount of lamports in the active stake account that are not delegated and not rent-related - calculated from the stake pool's active stake accounts directly
    pub undelegated_stake: u64,
    /// Amount of activating stake delegated to the validator - calculated from the stake pool's active and transient stake accounts directly
    pub activating_stake: u64,
    /// Amount of deactivating stake delegated to the validator - calculated from the stake pool's active and transient stake accounts directly
    pub deactivating_stake: u64,
    /// Inflation rewards
    pub inflation_rewards: u64,
    /// Jito rewards for current epoch
    pub jito_rewards: u64,
    // TODO: add donations
}

impl MarinadeStakePoolStakeAccount {
    pub fn build(
        bank: &Bank,
        validator_vote_account: &Pubkey,
        stake_account_address: &Pubkey,
        stake_account: &AccountSharedData,
        inflation_rewards: &HashMap<Pubkey, u64>,
        jito_rewards_lookup: &JitoRewardsLookup,
    ) -> Self {
        let mut undelegated_stake = 0;
        let mut active_stake = 0;
        let mut activating_stake = 0;
        let mut deactivating_stake = 0;

        if stake_account.lamports() > 0 {
            let stake_state =
                try_from_slice_unchecked::<stake::state::StakeState>(stake_account.data()).unwrap();
            let stake::state::StakeState::Stake(meta, stake_state) = stake_state else {
                unreachable!()
            };

            let StakeHistoryEntry {
                effective,
                activating,
                deactivating,
            } = stake_state
                .delegation
                .stake_activating_and_deactivating(bank.epoch(), None, None);

            active_stake += effective;
            activating_stake += activating;
            deactivating_stake += deactivating;

            undelegated_stake += stake_account
                .lamports()
                .checked_sub(stake_state.delegation.stake)
                .unwrap()
                .checked_sub(meta.rent_exempt_reserve)
                .unwrap();
        }

        // Fetch inflationrewards among previously fetched inflation rewards
        let inflation_rewards = inflation_rewards
            .get(&stake_account_address)
            .copied()
            .unwrap_or(0);

        // Get jito rewards for current epoch using the Jito's StakeMetaCollection
        let jito_rewards = jito_rewards_lookup
            .get(&stake_account_address)
            .copied()
            .unwrap_or(0);

        Self {
            vote_account_address: validator_vote_account.to_string(),
            stake_account_address: stake_account_address.to_string(),
            active_stake,
            undelegated_stake,
            activating_stake,
            deactivating_stake,
            inflation_rewards,
            jito_rewards,
        }
    }
}

fn fetch_current_marinade_stake_pool_price(
    rpc_client: &RpcClient,
    stake_pool_address: Pubkey,
) -> f64 {
    let marinade_state_account = rpc_client.get_account(&stake_pool_address).unwrap();
    let marinade_state =
        try_from_slice_unchecked::<MarinadeState>(&marinade_state_account.data()[8..]).unwrap();

    if marinade_state.msol_supply == 0 {
        return 0.0;
    }

    marinade_state.total_virtual_staked_lamports() as f64 / marinade_state.msol_supply as f64
}
