use {
    crate::{
        providers::{
            marinade::{generate_marinade_stake_pool_meta, MarinadeStakePoolMeta},
            socean::{generate_socean_stake_pool_metas, SoceanStakePoolMeta},
            spl::{generate_spl_stake_pool_metas, SplStakePoolMeta},
        },
        vendors::{
            jito::{
                generate_stake_accout_jito_rewards_lookup, TIP_DISTRIBUTION_PROGRAM_ID,
                TIP_PAYMENT_PROGRAM_ID,
            },
            marinade::MARINADE_PROGRAM_ID,
            socean::SOCEAN_PROGRAM_ID,
            spl::STAKE_POOL_PROGRAM_ID,
        },
    },
    log::info,
    serde::{Deserialize, Serialize},
    solana_accounts_db::{
        accounts_index::{
            AccountIndex, AccountSecondaryIndexes, AccountSecondaryIndexesIncludeExclude,
        },
        hardened_unpack::{open_genesis_config, MAX_GENESIS_ARCHIVE_UNPACKED_SIZE},
    },
    solana_client::rpc_client::RpcClient,
    solana_ledger::{
        bank_forks_utils,
        blockstore::Blockstore,
        blockstore_options::{AccessType, BlockstoreOptions, LedgerColumnOptions},
        blockstore_processor::ProcessOptions,
    },
    solana_runtime::{bank::Bank, snapshot_config::SnapshotConfig},
    solana_sdk::clock::Slot,
    solana_tip_distributor::stake_meta_generator_workflow::generate_stake_meta_collection,
    std::{
        collections::HashSet,
        fmt::{Debug, Display, Formatter},
        fs::File,
        io::{BufWriter, Write},
        path::{Path, PathBuf},
        sync::{atomic::AtomicBool, Arc},
    },
    thiserror::Error,
};

#[derive(Error, Debug)]
pub enum GenerateMetasError {
    EpochMetasNotFound(String),
}

impl Display for GenerateMetasError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&self, f)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StakePoolsMetas {
    pub stake_pools: Vec<StakePoolMeta>,
    pub bank_hash: String,
    pub epoch: u64,
    pub slot: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum StakePoolMeta {
    Spl(SplStakePoolMeta),
    Marinade(MarinadeStakePoolMeta),
    Socean(SoceanStakePoolMeta),
}

pub fn process_generate_stake_pool_metas(ledger_path: &Path, snapshot_slot: &Slot, out_path: &str) {
    info!("Creating bank from ledger path...");
    let bank = create_bank_from_snapshot(ledger_path, snapshot_slot);

    info!("Generating stake pools data...");
    let stake_pool_metas = generate_stake_pool_metas(&bank);

    info!("Writing stake pools data to JSON {}...", out_path);
    write_to_json_file(&stake_pool_metas, out_path);
}

fn create_bank_from_snapshot(ledger_path: &Path, snapshot_slot: &Slot) -> Arc<Bank> {
    let genesis_config = open_genesis_config(ledger_path, MAX_GENESIS_ARCHIVE_UNPACKED_SIZE);
    let snapshot_config = SnapshotConfig {
        full_snapshot_archive_interval_slots: Slot::MAX,
        incremental_snapshot_archive_interval_slots: Slot::MAX,
        full_snapshot_archives_dir: PathBuf::from(ledger_path),
        incremental_snapshot_archives_dir: PathBuf::from(ledger_path),
        bank_snapshots_dir: PathBuf::from(ledger_path),
        ..SnapshotConfig::default()
    };
    let blockstore = Blockstore::open_with_options(
        ledger_path,
        BlockstoreOptions {
            access_type: AccessType::PrimaryForMaintenance,
            recovery_mode: None,
            enforce_ulimit_nofile: false,
            column_options: LedgerColumnOptions::default(),
        },
    )
    .unwrap();

    let (bank_forks, _, _) = bank_forks_utils::load_bank_forks(
        &genesis_config,
        &blockstore,
        vec![PathBuf::from(ledger_path).join(Path::new("stake-pools.accounts"))],
        None,
        Some(&snapshot_config),
        &ProcessOptions {
            account_indexes: AccountSecondaryIndexes {
                indexes: [AccountIndex::ProgramId]
                    .iter()
                    .cloned()
                    .collect::<HashSet<_>>(),
                keys: Some(AccountSecondaryIndexesIncludeExclude {
                    keys: [
                        solana_stake_program::id(),
                        STAKE_POOL_PROGRAM_ID,
                        MARINADE_PROGRAM_ID,
                        SOCEAN_PROGRAM_ID,
                    ]
                    .iter()
                    .cloned()
                    .collect::<HashSet<_>>(),
                    exclude: false,
                }),
            },
            ..ProcessOptions::default()
        },
        None,
        None,
        None,
        Arc::new(AtomicBool::new(false)),
        false,
    );

    let working_bank = bank_forks.read().unwrap().working_bank();
    assert_eq!(
        working_bank.slot(),
        *snapshot_slot,
        "expected working bank slot {}, found {}",
        snapshot_slot,
        working_bank.slot()
    );

    working_bank
}

pub fn generate_stake_pool_metas(bank: &Arc<Bank>) -> StakePoolsMetas {
    assert!(bank.is_frozen());

    let rpc_client = RpcClient::new(
        std::env::var("RPC_ENDPOINT").unwrap_or("https://api.mainnet-beta.solana.com".to_string()),
    );

    let jito_stake_meta_collection = generate_stake_meta_collection(
        &bank,
        &TIP_DISTRIBUTION_PROGRAM_ID,
        &TIP_PAYMENT_PROGRAM_ID,
    )
    .unwrap();
    let jito_rewards_lookup =
        generate_stake_accout_jito_rewards_lookup(&jito_stake_meta_collection);

    let mut stake_pools: Vec<StakePoolMeta> = vec![];

    let spl_stake_pools = generate_spl_stake_pool_metas(bank, &rpc_client, &jito_rewards_lookup);
    stake_pools.extend(spl_stake_pools.into_iter().map(StakePoolMeta::Spl));

    let marinade_stake_pool =
        generate_marinade_stake_pool_meta(bank, &rpc_client, &jito_rewards_lookup);
    stake_pools.push(StakePoolMeta::Marinade(marinade_stake_pool));

    let socean_stake_pools =
        generate_socean_stake_pool_metas(bank, &rpc_client, &jito_rewards_lookup);
    stake_pools.extend(socean_stake_pools.into_iter().map(StakePoolMeta::Socean));

    StakePoolsMetas {
        stake_pools,
        bank_hash: bank.hash().to_string(),
        epoch: bank.epoch(),
        slot: bank.slot(),
    }
}

fn write_to_json_file(stake_pools_metas: &StakePoolsMetas, out_path: &str) {
    let file = File::create(out_path).unwrap();
    let mut writer = BufWriter::new(file);
    let json = serde_json::to_string_pretty(&stake_pools_metas).unwrap();
    writer.write_all(json.as_bytes()).unwrap();
    writer.flush().unwrap();
}

pub fn read_from_json_file(path: &Path) -> Result<StakePoolsMetas, GenerateMetasError> {
    let file = File::open(path)
        .map_err(|_| GenerateMetasError::EpochMetasNotFound(path.to_str().unwrap().to_string()))?;
    let metas = serde_json::from_reader(file).unwrap();

    Ok(metas)
}
