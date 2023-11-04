use {
    clap::Parser,
    dotenv::dotenv,
    log::*,
    solana_program::slot_history::Slot,
    stake_pool_tools::commands::generate_metas::process_generate_stake_pool_metas,
    std::{fs, path::PathBuf, process::exit},
};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Ledger path, where genesis and tar'd snapshot are located.
    #[arg(long, env, value_parser = Args::ledger_path_parser)]
    ledger_path: PathBuf,

    /// Path to output JSON.
    #[arg(long, env)]
    out_path: String,

    /// The expected snapshot slot.
    #[arg(long, env)]
    snapshot_slot: Slot,
}

impl Args {
    fn ledger_path_parser(ledger_path: &str) -> Result<PathBuf, &'static str> {
        Ok(fs::canonicalize(ledger_path).unwrap_or_else(|err| {
            error!("Unable to access ledger path '{}': {}", ledger_path, err);
            exit(1);
        }))
    }
}

fn main() {
    env_logger::init();
    dotenv().ok();
    let args: Args = Args::parse();
    process_generate_stake_pool_metas(&args.ledger_path, &args.snapshot_slot, &args.out_path);
}
