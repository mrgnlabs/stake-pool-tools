use {
    super::generate_metas::read_from_json_file,
    crate::{
        commands::generate_metas::StakePoolMeta,
        providers::{
            marinade::{generate_marinade_stake_pool_stats, MarinadeStakePoolMeta},
            socean::{generate_socean_stake_pool_stats, SoceanStakePoolMeta},
            spl::{generate_spl_stake_pool_stats, SplStakePoolMeta},
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
    pub staked_validator_count: u64,
    pub pool_token_supply: u64,
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

    let mut stats: Vec<EpochStakePoolStats> = vec![];

    // SPL

    let spl_pools: Vec<&SplStakePoolMeta> = target_epoch_metas
        .stake_pools
        .iter()
        .filter_map(|stake_pool| match stake_pool {
            StakePoolMeta::Spl(target_epoch_meta) => Some(target_epoch_meta),
            _ => None,
        })
        .collect::<Vec<&SplStakePoolMeta>>();

    if !spl_pools.is_empty() {
        stats = spl_pools
            .iter()
            .map(|target_epoch_meta| {
                debug!("stake_pool: {:?}", target_epoch_meta.address);

                let next_epoch_meta = if let Some(next_epoch_metas) = &next_epoch_metas {
                    next_epoch_metas
                        .stake_pools
                        .iter()
                        .find_map(|next_epoch_meta| {
                            if let StakePoolMeta::Spl(next_epoch_meta) = next_epoch_meta {
                                if next_epoch_meta.address == target_epoch_meta.address {
                                    Some(next_epoch_meta.clone())
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        })
                } else {
                    None
                };

                generate_spl_stake_pool_stats(&rpc_client, target_epoch_meta, next_epoch_meta)
            })
            .collect();
    }

    // Marinade

    let marinade_pools: Vec<&MarinadeStakePoolMeta> = target_epoch_metas
        .stake_pools
        .iter()
        .filter_map(|stake_pool| match stake_pool {
            StakePoolMeta::Marinade(target_epoch_meta) => Some(target_epoch_meta),
            _ => None,
        })
        .collect::<Vec<&MarinadeStakePoolMeta>>();

    if !spl_pools.is_empty() {
        stats.extend(marinade_pools.iter().map(|target_epoch_meta| {
            let next_epoch_meta = if let Some(next_epoch_metas) = &next_epoch_metas {
                next_epoch_metas
                    .stake_pools
                    .iter()
                    .find_map(|next_epoch_meta| {
                        if let StakePoolMeta::Marinade(next_epoch_meta) = next_epoch_meta {
                            if next_epoch_meta.address == target_epoch_meta.address {
                                Some(next_epoch_meta.clone())
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
            } else {
                None
            };

            generate_marinade_stake_pool_stats(&rpc_client, target_epoch_meta, next_epoch_meta)
        }));
    }

    // Socean

    let socean_pools: Vec<&SoceanStakePoolMeta> = target_epoch_metas
        .stake_pools
        .iter()
        .filter_map(|stake_pool| match stake_pool {
            StakePoolMeta::Socean(target_epoch_meta) => Some(target_epoch_meta),
            _ => None,
        })
        .collect::<Vec<&SoceanStakePoolMeta>>();

    if !socean_pools.is_empty() {
        stats.extend(
            socean_pools
                .iter()
                .map(|target_epoch_meta| {
                    debug!("stake_pool: {:?}", target_epoch_meta.address);

                    let next_epoch_meta = if let Some(next_epoch_metas) = &next_epoch_metas {
                        next_epoch_metas
                            .stake_pools
                            .iter()
                            .find_map(|next_epoch_meta| {
                                if let StakePoolMeta::Socean(next_epoch_meta) = next_epoch_meta {
                                    if next_epoch_meta.address == target_epoch_meta.address {
                                        Some(next_epoch_meta.clone())
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            })
                    } else {
                        None
                    };

                    generate_socean_stake_pool_stats(
                        &rpc_client,
                        target_epoch_meta,
                        next_epoch_meta,
                    )
                })
                .collect::<Vec<EpochStakePoolStats>>(),
        );
    }

    write_to_json_file(
        &EpochStakePoolStatsCollection {
            epoch: epoch.clone(),
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
