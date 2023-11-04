use {
    super::{LamportsAllocation, Rewards, StakePoolMetaApi},
    crate::{
        get_inflation_rewards_per_validator_stake_account,
        vendors::{
            jito::JitoRewardsLookup,
            spl::{
                find_stake_program_address, find_transient_stake_program_address,
                minimum_reserve_lamports, Fee, StakePool, StakeStatus, ValidatorList,
                ValidatorStakeInfo, STAKE_POOL_PROGRAM_ID,
            },
        },
    },
    itertools::Itertools,
    serde::{Deserialize, Serialize},
    solana_accounts_db::accounts_index::{IndexKey, ScanConfig},
    solana_client::rpc_client::RpcClient,
    solana_program::{borsh0_10::try_from_slice_unchecked, pubkey::Pubkey, stake},
    solana_runtime::bank::Bank,
    solana_sdk::account::{AccountSharedData, ReadableAccount},
    std::{collections::HashMap, fmt::Debug, num::NonZeroU32, str::FromStr, sync::Arc},
};

pub fn generate_spl_stake_pool_metas(
    bank: &Arc<Bank>,
    rpc_client: &RpcClient,
    jito_rewards_lookup: &JitoRewardsLookup,
) -> Vec<SplStakePoolMeta> {
    let spl_stake_pools_raw = bank
        .get_filtered_indexed_accounts(
            &IndexKey::ProgramId(STAKE_POOL_PROGRAM_ID),
            |account| account.data().len() == 611,
            &ScanConfig {
                collect_all_unsorted: true,
                ..ScanConfig::default()
            },
            bank.byte_limit_for_scans(),
        )
        .unwrap();

    let spl_stake_pools = spl_stake_pools_raw
        .into_iter()
        .map(|(address, account_data)| {
            SplStakePoolMeta::build(
                &rpc_client,
                bank,
                address,
                account_data,
                jito_rewards_lookup,
            )
        })
        .collect_vec();

    spl_stake_pools
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SplStakePoolMeta {
    /// Pool address
    pub address: String,
    /// LST mint
    pub mint: String,
    /// Pool manager
    pub manager: String,
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
    /// Total lamports locked at the end of the previous epoch, according to the stake pool program accounting
    pub last_epoch_total_lamports: u64,
    /// LST supply at the end of the previous epoch, according to the stake pool program accounting
    pub last_epoch_pool_token_supply: u64,
    /// Pool fees
    pub fees: SplStakePoolFees,
    /// Amount of lamports in the reserve stake account - calculated from the reserve stake account directly
    pub reserve_stake: u64,
    /// Relevant datapoints for each validator in the pool
    pub validators: Vec<SplStakePoolValidator>,
}

impl StakePoolMetaApi for SplStakePoolMeta {
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
        "SPL".to_string()
    }

    fn lamports_allocation(&self) -> LamportsAllocation {
        self.validators.iter().fold(
            LamportsAllocation {
                active: 0,
                activating: 0,
                deactivating: 0,
                undelegated: self.reserve_stake,
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
        self.validators.iter().fold(
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
        self.is_valid && !self.needs_update
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
        if self.fees.epoch.denominator == 0 {
            0.0
        } else {
            self.fees.epoch.numerator as f64 / self.fees.epoch.denominator as f64
        }
    }

    fn fetch_live_lst_price(&self, rpc_client: &RpcClient) -> f64 {
        let stake_pool_account = rpc_client
            .get_account(&Pubkey::from_str(&self.address).unwrap())
            .unwrap();
        let stake_pool = try_from_slice_unchecked::<StakePool>(stake_pool_account.data()).unwrap();

        if stake_pool.pool_token_supply == 0 {
            return 0f64;
        }

        stake_pool.total_lamports as f64 / stake_pool.pool_token_supply as f64
    }

    fn staked_validator_count(&self) -> u64 {
        self.validators
            .iter()
            .filter(|v| {
                v.activating_stake + v.deactivating_stake + v.active_stake + v.undelegated_stake
                    > 1_000_000_000
                    && v.status == "Active"
            })
            .count() as u64
    }
}

impl SplStakePoolMeta {
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
                    &STAKE_POOL_PROGRAM_ID,
                    &validator_info.vote_account_address,
                    &stake_pool_address,
                    NonZeroU32::new(validator_info.validator_seed_suffix.into()),
                );

                let (transient_stake_account_address, _) = find_transient_stake_program_address(
                    &STAKE_POOL_PROGRAM_ID,
                    &validator_info.vote_account_address,
                    &stake_pool_address,
                    validator_info.transient_seed_suffix.into(),
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

        let lst_price = if stake_pool.pool_token_supply == 0 {
            0.0
        } else {
            stake_pool.total_lamports as f64 / stake_pool.pool_token_supply as f64
        };

        Self {
            address: stake_pool_address.to_string(),
            mint: stake_pool.pool_mint.to_string(),
            manager: stake_pool.manager.to_string(),
            is_valid: stake_pool.is_valid(),
            needs_update: stake_pool.last_update_epoch < bank.epoch(),
            total_lamports: stake_pool.total_lamports,
            pool_token_supply: stake_pool.pool_token_supply,
            lst_price,
            last_epoch_total_lamports: stake_pool.last_epoch_total_lamports,
            last_epoch_pool_token_supply: stake_pool.last_epoch_pool_token_supply,
            fees: SplStakePoolFees {
                epoch: Fee {
                    numerator: stake_pool.epoch_fee.numerator,
                    denominator: stake_pool.epoch_fee.denominator,
                },
                deposit_sol: Fee {
                    numerator: stake_pool.sol_deposit_fee.numerator,
                    denominator: stake_pool.sol_deposit_fee.denominator,
                },
                withdrawal_sol: Fee {
                    numerator: stake_pool.sol_withdrawal_fee.numerator,
                    denominator: stake_pool.sol_withdrawal_fee.denominator,
                },
                deposit_stake: Fee {
                    numerator: stake_pool.stake_deposit_fee.numerator,
                    denominator: stake_pool.stake_deposit_fee.denominator,
                },
                withdrawal_stake: Fee {
                    numerator: stake_pool.stake_withdrawal_fee.numerator,
                    denominator: stake_pool.stake_withdrawal_fee.denominator,
                },
            },
            reserve_stake,
            validators,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SplStakePoolFees {
    epoch: Fee,
    deposit_sol: Fee,
    withdrawal_sol: Fee,
    deposit_stake: Fee,
    withdrawal_stake: Fee,
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
            &STAKE_POOL_PROGRAM_ID,
            &validator_info.vote_account_address,
            stake_pool_address,
            NonZeroU32::new(validator_info.validator_seed_suffix.into()),
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
            &STAKE_POOL_PROGRAM_ID,
            &validator_info.vote_account_address,
            stake_pool_address,
            validator_info.transient_seed_suffix.into(),
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
