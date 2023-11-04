#!/usr/bin/env python3

import os
import sys
import subprocess
from solana.rpc.api import Client as SolanaClient
import shutil


GENERATOR_BINARY = "generate-metas"


def main():
    if len(sys.argv) < 2:
        print(f"Usage: {sys.argv[0]} <epoch> [<rpc_endpoint>]")
        sys.exit(1)

    target_epoch = int(sys.argv[1])
    rpc_endpoint = (
        sys.argv[2] if len(sys.argv) > 2 else "https://api.mainnet-beta.solana.com"
    )

    print(f"Generating stake pool metas for epoch {target_epoch}")

    script_dir = os.path.dirname(os.path.abspath(__file__))

    epoch_dir = os.path.realpath(
        os.path.join(script_dir, "..", "data", f"epoch_{target_epoch}")
    )
    # Check that there is already a folder for this epoch
    if not os.path.exists(epoch_dir):
        print(f"Error: epoch {target_epoch} not prepped")
        sys.exit(1)

    # Delete previous artifacts
    rocksdb_dir = os.path.join(epoch_dir, "rocksdb")
    if os.path.exists(rocksdb_dir) and os.path.isdir(rocksdb_dir):
        shutil.rmtree(rocksdb_dir)
    accounts_dir = os.path.join(epoch_dir, "stake-pools.accounts")
    if os.path.exists(accounts_dir) and os.path.isdir(accounts_dir):
        shutil.rmtree(accounts_dir)
    genesis_unpacked_dir = os.path.join(epoch_dir, "genesis.bin")
    if os.path.isfile(genesis_unpacked_dir):
        os.remove(genesis_unpacked_dir)

    bin_path = os.path.realpath(
        os.path.join(script_dir, "..", "target/release", GENERATOR_BINARY)
    )
    if not os.path.exists(bin_path):
        print(f"Error: binary {GENERATOR_BINARY} not found. Have you built the crate?")
        sys.exit(1)

    output_dir = os.path.realpath(os.path.join(script_dir, "..", "output"))
    os.makedirs(epoch_dir, exist_ok=True)

    solana_client = SolanaClient(rpc_endpoint)
    epoch_schedule = solana_client.get_epoch_schedule()
    target_slot = epoch_schedule.value.get_last_slot_in_epoch(target_epoch)
    print(f"Target slot: {target_slot}")

    output_path = os.path.join(output_dir, f"stake_pool_metas_{target_epoch}.json")

    env = os.environ.copy()
    subprocess.run(
        [
            str(bin_path),
            "--ledger-path",
            str(epoch_dir),
            "--out-path",
            str(output_path),
            "--snapshot-slot",
            str(target_slot),
        ],
        env=env,
    )


if __name__ == "__main__":
    main()
