pub mod marinade;
pub mod socean;
pub mod spl;

use {
    self::{marinade::MarinadeStakePoolMeta, socean::SoceanStakePoolMeta, spl::SplStakePoolMeta},
    enum_dispatch::enum_dispatch,
    serde::{Deserialize, Serialize},
    solana_client::rpc_client::RpcClient,
};

pub const SECONDS_PER_DAY: u64 = 24 * 60 * 60;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StakePoolsMetas {
    pub stake_pools: Vec<StakePoolMeta>,
    pub bank_hash: String,
    pub total_sol_supply: u64,
    pub total_native_stake: u64,
    pub total_liquid_stake: u64,
    pub total_undelegated_lamports: u64,
    pub epoch: u64,
    pub epoch_duration: u64,
    pub slot: u64,
}

pub struct LamportsAllocation {
    pub active: u64,
    pub activating: u64,
    pub deactivating: u64,
    pub undelegated: u64,
}

pub struct Rewards {
    pub inflation: u64,
    pub jito: u64,
}

#[enum_dispatch(StakePoolMetaApi)]
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum StakePoolMeta {
    Spl(SplStakePoolMeta),
    Marinade(MarinadeStakePoolMeta),
    Socean(SoceanStakePoolMeta),
}

#[enum_dispatch]
pub trait StakePoolMetaApi {
    fn address(&self) -> String;
    fn manager(&self) -> String;
    fn mint(&self) -> String;
    fn total_lamports(&self) -> u64 {
        self.delegated_lamports() + self.undelegated_lamports()
    }
    fn delegated_lamports(&self) -> u64 {
        let LamportsAllocation {
            active,
            activating,
            deactivating,
            ..
        } = self.lamports_allocation();

        active + activating + deactivating
    }
    fn undelegated_lamports(&self) -> u64 {
        self.lamports_allocation().undelegated
    }
    fn yielding_lamports(&self) -> u64 {
        let LamportsAllocation {
            active,
            deactivating,
            ..
        } = self.lamports_allocation();

        active + deactivating
    }
    fn lamports_allocation(&self) -> LamportsAllocation;
    fn rewards(&self) -> Rewards;
    fn total_rewards(&self) -> u64 {
        self.rewards().inflation + self.rewards().jito
    }
    fn provider(&self) -> String;
    fn is_valid(&self) -> bool;
    fn lst_price(&self) -> f64;
    fn lst_supply(&self) -> u64;
    fn management_fee(&self) -> f64;
    fn fetch_live_lst_price(&self, rpc_client: &RpcClient) -> f64;
    fn staked_validator_count(&self) -> u64;
}
