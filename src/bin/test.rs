use {clap::Parser, dotenv::dotenv, stake_pool_tools::commands::test::process_test};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {}

fn main() {
    env_logger::init();
    dotenv().ok();
    process_test();
}
