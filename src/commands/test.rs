use {crate::vendors::socean::SOCEAN_PROGRAM_ID, log::info, solana_client::rpc_client::RpcClient};

pub fn process_test() {
    info!("Fetching marinade data...");

    let rpc_client = RpcClient::new(
        std::env::var("RPC_ENDPOINT").unwrap_or("https://api.mainnet-beta.solana.com".to_string()),
    );

    // let marinade_state = rpc_client.get_account(&MARINADE_STATE_ADDRESS).unwrap();
    // let marinade_state: MarinadeState =
    //     MarinadeState::try_from_slice(&marinade_state.data[8..(MarinadeState::serialized_len())])
    //         .unwrap();

    // println!("Marinade state: {:?}", marinade_state);

    // let stake_list = rpc_client
    //     .get_account(&marinade_state.stake_system.stake_list.account)
    //     .unwrap();

    // let stake_account_records = (0..marinade_state.stake_system.stake_list.count)
    //     .map(|index| marinade_state.stake_system.get(&stake_list.data, index))
    //     .collect::<Vec<StakeRecord>>();

    // println!("Stake records: {:?}", stake_account_records.len());

    let accounts = rpc_client.get_program_accounts(&SOCEAN_PROGRAM_ID).unwrap();
    for (pk, _) in accounts {
        println!("account: {:?}", pk);
    }
}
