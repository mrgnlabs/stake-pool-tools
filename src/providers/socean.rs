use {
    super::SECONDS_PER_DAY,
    crate::{
        apr_to_apy,
        commands::generate_normalized_stats::EpochStakePoolStats,
        compute_effective_stake_pool_apr, get_epoch_duration,
        get_inflation_rewards_per_validator_stake_account,
        vendors::{
            jito::JitoRewardsLookup,
            socean::{
                find_stake_program_address, find_transient_stake_program_address,
                minimum_reserve_lamports, Fee, StakePool, StakeStatus, ValidatorList,
                ValidatorStakeInfo, SOCEAN_PROGRAM_ID,
            },
        },
    },
    itertools::Itertools,
    num_traits::cast::ToPrimitive,
    serde::{Deserialize, Serialize},
    solana_accounts_db::accounts_index::{IndexKey, ScanConfig},
    solana_client::rpc_client::RpcClient,
    solana_program::{borsh0_10::try_from_slice_unchecked, pubkey::Pubkey, stake},
    solana_runtime::bank::Bank,
    solana_sdk::account::{AccountSharedData, ReadableAccount},
    std::{collections::HashMap, fmt::Debug, str::FromStr, sync::Arc},
};

pub fn generate_socean_stake_pool_metas(
    bank: &Arc<Bank>,
    rpc_client: &RpcClient,
    jito_rewards_lookup: &JitoRewardsLookup,
) -> Vec<SoceanStakePoolMeta> {
    let socean_stake_pools_raw = bank
        .get_filtered_indexed_accounts(
            &IndexKey::ProgramId(SOCEAN_PROGRAM_ID),
            |account| account.data().len() == 529,
            &ScanConfig {
                collect_all_unsorted: true,
                ..ScanConfig::default()
            },
            bank.byte_limit_for_scans(),
        )
        .unwrap();

    let socean_stake_pools = socean_stake_pools_raw
        .into_iter()
        .map(|(address, account_data)| {
            SoceanStakePoolMeta::build(
                &rpc_client,
                bank,
                address,
                account_data,
                jito_rewards_lookup,
            )
        })
        .collect_vec();

    socean_stake_pools
}

pub fn generate_socean_stake_pool_stats(
    rpc_client: &RpcClient,
    target_epoch_meta: &SoceanStakePoolMeta,
    next_epoch_meta: Option<SoceanStakePoolMeta>,
) -> EpochStakePoolStats {
    let (
        active_lamports,
        validator_undelegated_lamports,
        activating_lamports,
        deactivating_lamports,
        jito_rewards,
        inflation_rewards,
    ) = target_epoch_meta
        .validators
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
        fetch_current_socean_stake_pool_price(
            &rpc_client,
            Pubkey::from_str(&target_epoch_meta.address).unwrap(),
        )
    };

    let apr_effective = compute_effective_stake_pool_apr(
        target_epoch_meta.lst_price,
        next_epoch_lst_price,
        epochs_per_year as f64,
    );

    let undelegated_lamports = target_epoch_meta.reserve_stake + validator_undelegated_lamports;

    let management_fee = if target_epoch_meta.fees.epoch.denominator == 0 {
        0.0
    } else {
        target_epoch_meta.fees.epoch.numerator as f64
            / target_epoch_meta.fees.epoch.denominator as f64
    };

    EpochStakePoolStats {
        address: target_epoch_meta.address.clone(),
        manager: target_epoch_meta.manager.clone(),
        provider: "Socean".to_string(),
        management_fee,
        is_valid: target_epoch_meta.is_valid && !target_epoch_meta.needs_update,
        mint: target_epoch_meta.mint.clone(),
        lst_price: target_epoch_meta.lst_price,
        staked_validator_count: target_epoch_meta
            .validators
            .iter()
            .filter(|v| {
                v.activating_stake + v.deactivating_stake + v.active_stake + v.undelegated_stake
                    > 1_000_000_000
                    && v.status == "Active"
            })
            .count() as u64,
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
pub struct SoceanStakePoolMeta {
    /// Pool address
    pub address: String,
    /// LST mint
    pub mint: String,
    /// Pool manager
    pub manager: String,
    /// Epoch duration in seconds - calculated from first and last slot times
    pub epoch_duration: u64,
    /// Flag indicating whether the pool is in a valid state, i.e. initialized
    pub is_valid: bool,
    /// Flag indicating whether the pool still needs to be updated this epoch
    pub needs_update: bool,
    /// Total lamports locked according to the stake pool program accounting
    pub total_lamports: u64,
    /// LST supply according to the stake pool program accounting
    pub pool_token_supply: u64,
    /// LST price according to the stake pool program accounting
    pub lst_price: f64,
    /// Pool fees
    pub fees: SoceanStakePoolFees,
    /// Amount of lamports in the reserve stake account - calculated from the reserve stake account directly
    pub reserve_stake: u64,
    /// Relevant datapoints for each validator in the pool
    pub validators: Vec<SplStakePoolValidator>,
}

impl SoceanStakePoolMeta {
    pub fn build(
        rpc_client: &RpcClient,
        bank: &Arc<Bank>,
        stake_pool_address: Pubkey,
        account_data: AccountSharedData,
        jito_rewards_lookup: &JitoRewardsLookup,
    ) -> Self {
        let stake_pool = try_from_slice_unchecked::<StakePool>(account_data.data()).unwrap();
        let validator_list = bank.get_account(&stake_pool.validator_list).unwrap();
        let validator_list =
            try_from_slice_unchecked::<ValidatorList>(validator_list.data()).unwrap();

        let all_stake_accounts = validator_list
            .validators
            .iter()
            .flat_map(|validator_info| {
                let (active_stake_account_address, _) = find_stake_program_address(
                    &SOCEAN_PROGRAM_ID,
                    &validator_info.vote_account_address,
                    &stake_pool_address,
                );

                let (transient_stake_account_address, _) = find_transient_stake_program_address(
                    &SOCEAN_PROGRAM_ID,
                    &validator_info.vote_account_address,
                    &stake_pool_address,
                );

                vec![
                    active_stake_account_address,
                    transient_stake_account_address,
                ]
            })
            .collect_vec();

        let inflation_rewards = get_inflation_rewards_per_validator_stake_account(
            rpc_client,
            bank.epoch(),
            &all_stake_accounts,
        );

        let validators = validator_list
            .validators
            .into_iter()
            .map(|validator_info| {
                SplStakePoolValidator::from_validator_info(
                    &bank,
                    &stake_pool_address,
                    validator_info,
                    &inflation_rewards,
                    &jito_rewards_lookup,
                )
            })
            .collect_vec();
        let reserve_stake_ai = bank.get_account(&stake_pool.reserve_stake).unwrap();
        let reserve_stake =
            try_from_slice_unchecked::<stake::state::StakeState>(reserve_stake_ai.data()).unwrap();

        let reserve_stake = if let stake::state::StakeState::Initialized(meta) = reserve_stake {
            reserve_stake_ai
                .lamports()
                .checked_sub(minimum_reserve_lamports(&meta))
                .unwrap()
        } else {
            unreachable!()
        };

        let epoch_schedule = bank.epoch_schedule();
        let epoch_duration = get_epoch_duration(rpc_client, epoch_schedule, bank.epoch())
            .to_u64()
            .unwrap();

        let lst_price = if stake_pool.pool_token_supply == 0 {
            0.0
        } else {
            stake_pool.total_stake_lamports as f64 / stake_pool.pool_token_supply as f64
        };

        Self {
            address: stake_pool_address.to_string(),
            mint: stake_pool.pool_mint.to_string(),
            manager: stake_pool.manager.to_string(),
            epoch_duration,
            is_valid: stake_pool.is_valid(),
            needs_update: stake_pool.last_update_epoch < bank.epoch(),
            total_lamports: stake_pool.total_stake_lamports,
            pool_token_supply: stake_pool.pool_token_supply,
            lst_price,
            fees: SoceanStakePoolFees {
                epoch: Fee {
                    numerator: stake_pool.fee.numerator,
                    denominator: stake_pool.fee.denominator,
                },
                deposit_sol: Fee {
                    numerator: stake_pool.sol_deposit_fee.numerator,
                    denominator: stake_pool.sol_deposit_fee.denominator,
                },
                withdrawal: Fee {
                    numerator: stake_pool.withdrawal_fee.numerator,
                    denominator: stake_pool.withdrawal_fee.denominator,
                },
                deposit_stake: Fee {
                    numerator: stake_pool.stake_deposit_fee.numerator,
                    denominator: stake_pool.stake_deposit_fee.denominator,
                },
            },
            reserve_stake,
            validators,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SoceanStakePoolFees {
    epoch: Fee,
    deposit_sol: Fee,
    withdrawal: Fee,
    deposit_stake: Fee,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SplStakePoolValidator {
    /// Validator vote account address
    pub vote_account_address: String,
    /// Active stake account address
    pub active_stake_account_address: String,
    /// Transient stake account address
    pub transient_stake_account_address: String,
    /// Status of the validator wrt the stake pool
    pub status: String,
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

impl SplStakePoolValidator {
    pub fn from_validator_info(
        bank: &Bank,
        stake_pool_address: &Pubkey,
        validator_info: ValidatorStakeInfo,
        inflation_rewards: &HashMap<Pubkey, u64>,
        jito_rewards_lookup: &JitoRewardsLookup,
    ) -> Self {
        let status = format!(
            "{:?}",
            StakeStatus::try_from(validator_info.status).unwrap()
        );

        // Check active stake account
        let mut active_stake = 0;
        let mut undelegated_stake = 0;
        let (active_stake_account_address, _) = find_stake_program_address(
            &SOCEAN_PROGRAM_ID,
            &validator_info.vote_account_address,
            stake_pool_address,
        );
        let active_stake_account = bank.get_account(&active_stake_account_address);
        if let Some(active_stake_account) = active_stake_account {
            if active_stake_account.lamports() > 0 && !active_stake_account.data().is_empty() {
                let active_stake_state = try_from_slice_unchecked::<stake::state::StakeState>(
                    active_stake_account.data(),
                )
                .unwrap();
                let stake::state::StakeState::Stake(meta, active_stake_state) = active_stake_state
                else {
                    unreachable!()
                };

                if StakeStatus::try_from(validator_info.status).unwrap() == StakeStatus::Active {
                    active_stake = active_stake_state.delegation.stake;

                    undelegated_stake = active_stake_account
                        .lamports()
                        .checked_sub(active_stake)
                        .unwrap()
                        .checked_sub(minimum_reserve_lamports(&meta))
                        .unwrap();
                }
            }
        }

        // Check transient stake account
        let (transient_stake_account_address, _) = find_transient_stake_program_address(
            &SOCEAN_PROGRAM_ID,
            &validator_info.vote_account_address,
            stake_pool_address,
        );

        let mut activating_stake = 0;
        let mut deactivating_stake = 0;
        let transient_stake_account = bank.get_account(&transient_stake_account_address);
        if let Some(transient_stake_account) = transient_stake_account {
            if transient_stake_account.lamports() > 0 && !transient_stake_account.data().is_empty()
            {
                let transient_stake_state = try_from_slice_unchecked::<stake::state::StakeState>(
                    transient_stake_account.data(),
                )
                .unwrap();
                let stake::state::StakeState::Stake(meta, transient_stake_state) =
                    transient_stake_state
                else {
                    unreachable!()
                };
                let is_transient_stake_activating = bank.epoch()
                    >= transient_stake_state.delegation.activation_epoch
                    && transient_stake_state.delegation.deactivation_epoch == std::u64::MAX;
                let is_transient_stake_deactivating = bank.epoch()
                    >= transient_stake_state.delegation.deactivation_epoch
                    && transient_stake_state.delegation.deactivation_epoch != std::u64::MAX;

                if is_transient_stake_activating && is_transient_stake_deactivating {
                    panic!(
                        "Stake account {} is both activating and deactivating",
                        transient_stake_account_address
                    );
                } else if !is_transient_stake_activating && !is_transient_stake_deactivating {
                    panic!(
                        "Stake account {} is neither activating nor deactivating",
                        transient_stake_account_address
                    );
                }

                let amount = meta
                    .rent_exempt_reserve
                    .saturating_add(transient_stake_state.delegation.stake);
                if is_transient_stake_activating {
                    activating_stake = amount;
                } else {
                    deactivating_stake = amount;
                };
            }
        }

        // Fetch inflationrewards among previously fetched inflation rewards
        let inflation_rewards = inflation_rewards
            .get(&active_stake_account_address)
            .copied()
            .unwrap_or(0)
            + inflation_rewards
                .get(&transient_stake_account_address)
                .copied()
                .unwrap_or(0);

        let jito_rewards = jito_rewards_lookup
            .get(&active_stake_account_address)
            .copied()
            .unwrap_or(0)
            + jito_rewards_lookup
                .get(&transient_stake_account_address)
                .copied()
                .unwrap_or(0);

        Self {
            vote_account_address: validator_info.vote_account_address.to_string(),
            active_stake_account_address: active_stake_account_address.to_string(),
            transient_stake_account_address: transient_stake_account_address.to_string(),
            status,
            active_stake,
            undelegated_stake,
            activating_stake,
            deactivating_stake,
            inflation_rewards,
            jito_rewards,
        }
    }
}

fn fetch_current_socean_stake_pool_price(
    rpc_client: &RpcClient,
    stake_pool_address: Pubkey,
) -> f64 {
    let stake_pool_account = rpc_client.get_account(&stake_pool_address).unwrap();
    let stake_pool = try_from_slice_unchecked::<StakePool>(stake_pool_account.data()).unwrap();

    if stake_pool.pool_token_supply == 0 {
        return 0f64;
    }

    stake_pool.total_stake_lamports as f64 / stake_pool.pool_token_supply as f64
}
