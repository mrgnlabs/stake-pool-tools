use {
    clap::Parser,
    dotenv::dotenv,
    solana_program::stake_history::Epoch,
    stake_pool_tools::{
        commands::generate_normalized_stats::process_generate_normalized_stats, path_parser,
    },
    std::path::PathBuf,
};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to required metas JSON files.
    #[arg(long, env, value_parser = path_parser)]
    metas_dir: PathBuf,

    /// Path to output JSON.
    #[arg(long, env)]
    out_path: String,

    /// The target epoch.
    #[arg(long, env)]
    epoch: Epoch,

    /// Use live price fallback (use for latest epoch).
    #[arg(long, env)]
    use_live_price_fallback: bool,
}

fn main() {
    env_logger::init();
    dotenv().ok();
    let args: Args = Args::parse();
    process_generate_normalized_stats(
        &args.metas_dir,
        &args.epoch,
        &args.out_path,
        args.use_live_price_fallback,
    );
}
