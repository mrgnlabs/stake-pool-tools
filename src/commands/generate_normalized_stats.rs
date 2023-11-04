use {
    super::generate_metas::read_from_json_file,
    crate::{
        apr_to_apy, compute_effective_stake_pool_apr,
        providers::{
            LamportsAllocation, Rewards, StakePoolMeta, StakePoolMetaApi, SECONDS_PER_DAY,
        },
    },
    log::{debug, info, warn},
    serde::{Deserialize, Serialize},
    solana_client::rpc_client::RpcClient,
    solana_program::stake_history::Epoch,
    std::{fs::File, io::BufWriter, io::Write, path::Path},
};

#[derive(Debug, Deserialize, Serialize)]
pub struct EpochStakePoolStatsCollection {
    pub epoch: u64,
    pub total_sol_supply: u64,
    pub total_native_stake: u64,
    pub total_liquid_stake: u64,
    pub total_undelegated_lamports: u64,
    pub stake_pools: Vec<EpochStakePoolStats>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct EpochStakePoolStats {
    pub address: String,
    pub manager: String,
    pub management_fee: f64,
    pub provider: String,
    pub is_valid: bool,
    pub mint: String,
    pub lst_price: f64,
    pub lst_supply: u64,
    pub staked_validator_count: u64,
    pub undelegated_lamports: u64,
    pub total_lamports_locked: u64,
    pub active_lamports: u64,
    pub activating_lamports: u64,
    pub deactivating_lamports: u64,
    pub inflation_rewards: u64,
    pub jito_rewards: u64,
    pub apr_baseline: f64,
    pub apy_baseline: f64,
    pub apr_effective: f64,
    pub apy_effective: f64,
    pub liquidity_delta: i128,
}

pub fn process_generate_normalized_stats(metas_dir: &Path, epoch: &Epoch, out_path: &str) {
    let target_epoch_metas_path = metas_dir.join(format!("stake_pool_metas_{}.json", epoch));
    let target_epoch_metas = read_from_json_file(&target_epoch_metas_path).unwrap();

    let rpc_client = RpcClient::new(
        std::env::var("RPC_ENDPOINT").unwrap_or("https://api.mainnet-beta.solana.com".to_string()),
    );

    info!(
        "Processing data for {:?} stake pools",
        target_epoch_metas.stake_pools.len()
    );

    let next_epoch_metas_path = metas_dir.join(format!("stake_pool_metas_{}.json", epoch + 1));
    let next_epoch_metas = read_from_json_file(&next_epoch_metas_path).ok();
    if next_epoch_metas.is_none() {
        warn!(
            "Stake pool metas for following epoch ({}) not found, effective APY will be based off live data",
            epoch
        );
    }

    let prev_epoch_metas_path = metas_dir.join(format!("stake_pool_metas_{}.json", epoch - 1));
    let prev_epoch_metas = read_from_json_file(&prev_epoch_metas_path).ok();
    if prev_epoch_metas.is_none() {
        warn!(
            "Stake pool metas for previous epoch ({}) not found, liquidity change will default to 0",
            epoch
        );
    }

    let stats: Vec<EpochStakePoolStats> = target_epoch_metas
        .stake_pools
        .iter()
        .map(|target_pool_meta| {
            debug!("stake_pool: {:?}", target_pool_meta.address());

            let next_epoch_meta = if let Some(next_epoch_metas) = &next_epoch_metas {
                next_epoch_metas
                    .stake_pools
                    .iter()
                    .find(|next_epoch_meta| {
                        if let StakePoolMeta::Spl(next_epoch_meta) = next_epoch_meta {
                            next_epoch_meta.address == target_pool_meta.address()
                        } else {
                            false
                        }
                    })
                    .cloned()
            } else {
                None
            };

            let prev_epoch_meta = if let Some(prev_epoch_metas) = &prev_epoch_metas {
                prev_epoch_metas
                    .stake_pools
                    .iter()
                    .find(|prev_epoch_meta| {
                        if let StakePoolMeta::Spl(prev_epoch_meta) = prev_epoch_meta {
                            prev_epoch_meta.address == target_pool_meta.address()
                        } else {
                            false
                        }
                    })
                    .cloned()
            } else {
                None
            };

            generate_stake_pool_stats(
                &rpc_client,
                target_pool_meta,
                target_epoch_metas.epoch_duration,
                prev_epoch_meta,
                next_epoch_meta,
            )
        })
        .collect();

    write_to_json_file(
        &EpochStakePoolStatsCollection {
            epoch: epoch.clone(),
            total_sol_supply: target_epoch_metas.total_sol_supply,
            total_native_stake: target_epoch_metas.total_native_stake,
            total_liquid_stake: target_epoch_metas.total_liquid_stake,
            total_undelegated_lamports: target_epoch_metas.total_undelegated_lamports,
            stake_pools: stats,
        },
        out_path,
    );
}

fn write_to_json_file(stake_pools_metas: &EpochStakePoolStatsCollection, out_path: &str) {
    let file = File::create(out_path).unwrap();
    let mut writer = BufWriter::new(file);
    let json = serde_json::to_string_pretty(&stake_pools_metas).unwrap();
    writer.write_all(json.as_bytes()).unwrap();
    writer.flush().unwrap();
}

pub fn generate_stake_pool_stats(
    rpc_client: &RpcClient,
    target_epoch_meta: &StakePoolMeta,
    target_epoch_duration: u64,
    prev_epoch_meta: Option<StakePoolMeta>,
    next_epoch_meta: Option<StakePoolMeta>,
) -> EpochStakePoolStats {
    let LamportsAllocation {
        active: active_lamports,
        activating: activating_lamports,
        deactivating: deactivating_lamports,
        ..
    } = target_epoch_meta.lamports_allocation();

    let Rewards {
        jito: jito_rewards,
        inflation: inflation_rewards,
    } = target_epoch_meta.rewards();

    let epochs_per_year = (SECONDS_PER_DAY as f64 * 365.0) / target_epoch_duration as f64;

    let apr_potential = target_epoch_meta.total_rewards() as f64
        / target_epoch_meta.yielding_lamports() as f64
        * (epochs_per_year as f64);

    let next_epoch_lst_price = if let Some(next_epoch_meta) = next_epoch_meta {
        next_epoch_meta.lst_price()
    } else {
        target_epoch_meta.fetch_live_lst_price(&rpc_client)
    };

    let total_lamports_locked = target_epoch_meta.total_lamports();

    let liquidity_delta = prev_epoch_meta.map_or(0, |prev_epoch_meta| {
        total_lamports_locked as i128 - prev_epoch_meta.total_lamports() as i128
    });

    let apr_effective = compute_effective_stake_pool_apr(
        target_epoch_meta.lst_price(),
        next_epoch_lst_price,
        epochs_per_year as f64,
    );

    EpochStakePoolStats {
        address: target_epoch_meta.address(),
        manager: target_epoch_meta.manager(),
        provider: target_epoch_meta.provider(),
        management_fee: target_epoch_meta.management_fee(),
        is_valid: target_epoch_meta.is_valid(),
        mint: target_epoch_meta.mint(),
        lst_price: target_epoch_meta.lst_price(),
        staked_validator_count: target_epoch_meta.staked_validator_count(),
        lst_supply: target_epoch_meta.lst_supply(),
        undelegated_lamports: target_epoch_meta.undelegated_lamports(),
        total_lamports_locked,
        active_lamports,
        activating_lamports,
        deactivating_lamports,
        liquidity_delta,
        inflation_rewards,
        jito_rewards,
        apr_baseline: apr_potential,
        apy_baseline: apr_to_apy(apr_potential, epochs_per_year),
        apr_effective,
        apy_effective: apr_to_apy(apr_effective, epochs_per_year),
    }
}
