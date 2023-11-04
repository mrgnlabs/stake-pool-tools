use {
    super::{LamportsAllocation, Rewards, StakePoolMetaApi},
    crate::{
        get_inflation_rewards_per_validator_stake_account,
        vendors::{
            jito::JitoRewardsLookup,
            marinade::{MarinadeState, StakeRecord, MARINADE_STATE_ADDRESS},
        },
    },
    borsh::BorshDeserialize,
    itertools::Itertools,
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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MarinadeStakePoolMeta {
    /// Pool address
    pub address: String,
    /// LST mint
    pub mint: String,
    /// Pool manager
    pub manager: String,
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

impl StakePoolMetaApi for MarinadeStakePoolMeta {
    fn address(&self) -> String {
        self.address.to_string()
    }

    fn manager(&self) -> String {
        self.manager.to_string()
    }

    fn mint(&self) -> String {
        self.mint.to_string()
    }

    fn provider(&self) -> String {
        "Marinade".to_string()
    }

    fn lamports_allocation(&self) -> LamportsAllocation {
        self.stake_accounts.iter().fold(
            LamportsAllocation {
                active: 0,
                activating: 0,
                deactivating: 0,
                undelegated: self.reserve,
            },
            |mut acc, validator_meta| {
                acc.active = acc.active + validator_meta.active_stake;
                acc.activating = acc.activating + validator_meta.activating_stake;
                acc.deactivating = acc.deactivating + validator_meta.deactivating_stake;
                acc.undelegated = acc.undelegated + validator_meta.undelegated_stake;

                acc
            },
        )
    }

    fn rewards(&self) -> Rewards {
        self.stake_accounts.iter().fold(
            Rewards {
                inflation: 0,
                jito: 0,
            },
            |mut acc, validator_meta| {
                acc.inflation = acc.inflation + validator_meta.inflation_rewards;
                acc.jito = acc.jito + validator_meta.jito_rewards;

                acc
            },
        )
    }

    fn is_valid(&self) -> bool {
        true
    }

    fn lst_price(&self) -> f64 {
        if self.pool_token_supply == 0 {
            0.0
        } else {
            self.total_lamports as f64 / self.pool_token_supply as f64
        }
    }

    fn lst_supply(&self) -> u64 {
        self.pool_token_supply
    }

    fn management_fee(&self) -> f64 {
        self.management_fee
    }

    fn fetch_live_lst_price(&self, rpc_client: &RpcClient) -> f64 {
        let marinade_state_account = rpc_client
            .get_account(&Pubkey::from_str(&self.address).unwrap())
            .unwrap();
        let marinade_state =
            try_from_slice_unchecked::<MarinadeState>(&marinade_state_account.data()[8..]).unwrap();

        if marinade_state.msol_supply == 0 {
            return 0.0;
        }

        marinade_state.total_virtual_staked_lamports() as f64 / marinade_state.msol_supply as f64
    }

    fn staked_validator_count(&self) -> u64 {
        self.stake_accounts
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
            .count() as u64
    }
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

        let lst_price = marinade_state.total_virtual_staked_lamports() as f64
            / marinade_state.msol_supply as f64;

        let management_fee = marinade_state.reward_fee.basis_points as f64 / 10_000.0;

        Self {
            address: marinade_state_address.to_string(),
            manager: marinade_state.admin_authority.to_string(),
            mint: marinade_state.msol_mint.to_string(),
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
