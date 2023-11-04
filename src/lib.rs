#![allow(deprecated)]

pub mod commands;
mod providers;
mod vendors;

use {
    log::error,
    solana_client::rpc_client::RpcClient,
    solana_program::{epoch_schedule::EpochSchedule, pubkey::Pubkey},
    std::{collections::HashMap, fs::canonicalize, path::PathBuf, process::exit},
};

pub fn path_parser(ledger_path: &str) -> Result<PathBuf, &'static str> {
    Ok(canonicalize(ledger_path).unwrap_or_else(|err| {
        error!("Unable to access ledger path '{}': {}", ledger_path, err);
        exit(1);
    }))
}

pub fn get_inflation_rewards_per_validator_stake_account(
    rpc_client: &RpcClient,
    epoch: u64,
    stake_accounts: &[Pubkey],
) -> HashMap<Pubkey, u64> {
    rpc_client
        .get_inflation_reward(stake_accounts, Some(epoch))
        .unwrap()
        .into_iter()
        .zip(stake_accounts.iter().cloned())
        .map(|(maybe_rewards, stake_account_address)| {
            (
                stake_account_address,
                maybe_rewards.map_or(0, |rewards| rewards.amount),
            )
        })
        .collect()
}

fn get_epoch_duration(rpc_client: &RpcClient, epoch_schedule: &EpochSchedule, epoch: u64) -> i64 {
    let mut epoch_end_slot = epoch_schedule.get_last_slot_in_epoch(epoch - 1);
    let epoch_end_time: i64;

    loop {
        match rpc_client.get_block_time(epoch_end_slot) {
            Ok(_epoch_end_time) => {
                epoch_end_time = _epoch_end_time;
                break;
            }
            Err(_) => {
                epoch_end_slot += 1;
            }
        }
    }

    let mut epoch_start_slot = epoch_schedule.get_first_slot_in_epoch(epoch - 1);
    let epoch_start_time: i64;

    loop {
        match rpc_client.get_block_time(epoch_start_slot) {
            Ok(_epoch_start_time) => {
                epoch_start_time = _epoch_start_time;
                break;
            }
            Err(_) => {
                epoch_start_slot += 1;
            }
        }
    }

    epoch_end_time.checked_sub(epoch_start_time).unwrap()
}

fn apr_to_apy(apr: f64, compounding_frequency: f64) -> f64 {
    (1.0 + apr / compounding_frequency).powf(compounding_frequency) - 1.0
}

fn compute_effective_stake_pool_apr(
    current_epoch_lst_price: f64,
    next_epoch_lst_price: f64,
    epochs_per_year: f64,
) -> f64 {
    let epoch_rate = next_epoch_lst_price / current_epoch_lst_price - 1.0;

    epoch_rate * epochs_per_year
}
