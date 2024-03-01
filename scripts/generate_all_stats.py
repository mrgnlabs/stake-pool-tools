#!/usr/bin/env python3

import os
import re
import sys
import subprocess
from solana.rpc.api import Client as SolanaClient
import shutil


GENERATOR_BINARY = "generate-normalized-stats"


def main():
    if len(sys.argv) < 1:
        print(f"Usage: {sys.argv[0]} [<rpc_endpoint>]")
        sys.exit(1)

    rpc_endpoint = (
        sys.argv[2] if len(sys.argv) > 2 else "https://api.mainnet-beta.solana.com"
    )

    print(f"Generating stake pool stats for all epochs")

    script_dir = os.path.dirname(os.path.abspath(__file__))

    metas_dir = os.path.realpath(os.path.join(script_dir, "..", "output"))

    solana_client = SolanaClient(rpc_endpoint)
    live_epoch = solana_client.get_epoch_info().value.epoch

    metas_files = os.listdir(metas_dir)
    metas_files.sort()
    metas_file_regex = "stake_pool_metas_([0-9]+).json"
    for file_name in metas_files:
        match = re.match(metas_file_regex, file_name)
        if match:
            target_epoch = int(match.groups()[0])

            bin_path = os.path.realpath(
                os.path.join(script_dir, "..", "target/release", GENERATOR_BINARY)
            )
            if not os.path.exists(bin_path):
                print(
                    f"Error: binary {GENERATOR_BINARY} not found. Have you built the crate?"
                )
                sys.exit(1)

            output_path = os.path.realpath(
                os.path.join(metas_dir, f"stats_{target_epoch}.json")
            )

            is_latest = target_epoch == (live_epoch - 1)

            print(f"Generating stats for epoch {target_epoch}...")
            env = os.environ.copy()
            env["RUST_LOG"] = "error"
            cmd = [
                str(bin_path),
                "--metas-dir",
                str(metas_dir),
                "--out-path",
                str(output_path),
                "--epoch",
                str(target_epoch),
            ]
            if is_latest:
                cmd.append("--use-live-price-fallback")

            subprocess.run(
                cmd,
                env=env,
            )
            print(f"Done processing epoch {target_epoch}")


if __name__ == "__main__":
    main()
