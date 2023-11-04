use im::HashMap;
use solana_program::{pubkey, pubkey::Pubkey};
use solana_tip_distributor::{StakeMeta, StakeMetaCollection};

pub const TIP_DISTRIBUTION_PROGRAM_ID: Pubkey =
    pubkey!("4R3gSG8BpU4t19KYj8CfnbtRpnT8gtk4dvTHxVRwc2r7");
pub const TIP_PAYMENT_PROGRAM_ID: Pubkey = pubkey!("T1pyyaTNZsKv2WcRAB8oVnk93mLJw2XzjtVYqCsaHqt");

#[derive(Clone, Eq, Debug, Hash, PartialEq)]
pub struct JitoReward {
    pub stake_account: Pubkey,
    pub staker_pubkey: Pubkey,
    pub amount: u64,
}

pub type JitoRewardsLookup = HashMap<Pubkey, u64>;

pub fn generate_stake_accout_jito_rewards_lookup(
    stake_meta_collection: &StakeMetaCollection,
) -> JitoRewardsLookup {
    stake_meta_collection
        .stake_metas
        .iter()
        .flat_map(|stake_meta| {
            let stake_meta = generate_stake_accout_jito_rewards_lookup_for_validator(stake_meta);

            stake_meta
                .into_iter()
                .map(move |(_, jito_reward)| (jito_reward.stake_account, jito_reward.amount))
        })
        .collect()
}

pub type ValidatorJitoRewardsLookup = HashMap<Pubkey, JitoReward>;

pub fn generate_stake_accout_jito_rewards_lookup_for_validator(
    stake_meta: &StakeMeta,
) -> ValidatorJitoRewardsLookup {
    if let Some(tip_distribution_meta) = stake_meta.maybe_tip_distribution_meta.as_ref() {
        let validator_amount = (tip_distribution_meta.total_tips as u128)
            .checked_mul(tip_distribution_meta.validator_fee_bps as u128)
            .unwrap()
            .checked_div(10_000)
            .unwrap() as u64;

        let remaining_total_rewards = tip_distribution_meta
            .total_tips
            .checked_sub(validator_amount)
            .unwrap() as u128;

        let total_delegated = stake_meta.total_delegated as u128;

        stake_meta
            .delegations
            .iter()
            .map(|delegation| {
                let amount_delegated = delegation.lamports_delegated as u128;
                let reward_amount = (amount_delegated.checked_mul(remaining_total_rewards))
                    .unwrap()
                    .checked_div(total_delegated)
                    .unwrap();

                (
                    delegation.stake_account_pubkey,
                    JitoReward {
                        stake_account: delegation.stake_account_pubkey,
                        staker_pubkey: delegation.staker_pubkey,
                        amount: reward_amount as u64,
                    },
                )
            })
            .collect()
    } else {
        HashMap::new()
    }
}
