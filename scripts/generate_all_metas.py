#!/usr/bin/env python3

import os
import re
import sys
import subprocess
from solana.rpc.api import Client as SolanaClient
import shutil


GENERATOR_BINARY = "generate-metas"


def main():
    if len(sys.argv) < 1:
        print(f"Usage: {sys.argv[0]} [<rpc_endpoint>]")
        sys.exit(1)

    rpc_endpoint = (
        sys.argv[2] if len(sys.argv) > 2 else "https://api.mainnet-beta.solana.com"
    )

    print(f"Generating stake pool metas for all epochs")

    script_dir = os.path.dirname(os.path.abspath(__file__))

    epochs_dir = os.path.realpath(os.path.join(script_dir, "..", "data"))

    epoch_dirs = os.listdir(epochs_dir)
    epoch_dirs.sort()
    epochs_dir_regex = "epoch_([0-9]+)"
    for dir_name in epoch_dirs:
        match = re.match(epochs_dir_regex, dir_name)
        if match:
            target_epoch = int(match.groups()[0])
            if target_epoch < 516:
                print(f"Skipping epoch {target_epoch} - not supported")
                continue

            epoch_dir = os.path.join(epochs_dir, dir_name)

            # Check that there is already a folder for this epoch
            if not os.path.exists(epoch_dir):
                print(f"Warning: epoch {target_epoch} not prepped. Skipping.")
                continue

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
                print(
                    f"Error: binary {GENERATOR_BINARY} not found. Have you built the crate?"
                )
                sys.exit(1)

            output_dir = os.path.realpath(os.path.join(script_dir, "..", "output"))
            os.makedirs(epoch_dir, exist_ok=True)

            solana_client = SolanaClient(rpc_endpoint)
            epoch_schedule = solana_client.get_epoch_schedule()
            target_slot = epoch_schedule.value.get_last_slot_in_epoch(target_epoch)

            output_path = os.path.join(
                output_dir, f"stake_pool_metas_{target_epoch}.json"
            )

            print(f"Processing epoch {target_epoch} (slot: {target_slot})...")
            env = os.environ.copy()
            env["RUST_LOG"] = "error"
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
            print(f"Done processing epoch {target_epoch}")


if __name__ == "__main__":
    main()
