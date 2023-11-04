#!/usr/bin/env python3

import os
import sys
import re
import requests
from tqdm import tqdm
from google.cloud import storage
from solana.rpc.api import Client as SolanaClient


JITO_GCP_BUCKET = "jito-mainnet"
JITO_PREFERRED_WAREHOUSE = "ny-mainnet-warehouse-1"


def main():
    if len(sys.argv) < 2:
        print(f"Usage: {sys.argv[0]} <epoch> [<rpc_endpoint>]")
        sys.exit(1)

    target_epoch = int(sys.argv[1])
    rpc_endpoint = (
        sys.argv[2] if len(sys.argv) > 2 else "https://api.mainnet-beta.solana.com"
    )

    print(f"Preparing epoch {target_epoch} folder")

    script_dir = os.path.dirname(os.path.abspath(__file__))
    epoch_dir = os.path.realpath(
        os.path.join(script_dir, "..", "data", f"epoch_{target_epoch}")
    )

    if os.path.exists(epoch_dir):
        print(f"Error: epoch {target_epoch} already exists")
        sys.exit(1)

    os.makedirs(epoch_dir)

    genesis_url = "https://mainnet.rpc.jito.wtf/genesis.tar.bz2"
    output_file = os.path.join(epoch_dir, "genesis.tar.bz2")
    download_file(genesis_url, output_file, "genesis")

    solana_client = SolanaClient(rpc_endpoint)
    epoch_schedule = solana_client.get_epoch_schedule()
    target_slot = epoch_schedule.value.get_last_slot_in_epoch(target_epoch)
    print(f"Target slot: {target_slot}")

    snapshot_name, snapshot_url = find_jito_snapshot(target_epoch, target_slot)
    output_path = os.path.join(epoch_dir, snapshot_name)

    download_file(snapshot_url, output_path, "snapshot")


def find_jito_snapshot(target_epoch: int, target_slot: int) -> (str, str):
    snapshot_regex = f"{target_epoch}/[^/]+/snapshot-{target_slot}-.+\.tar\.zst"

    snapshots_found = find_blob_in_bucket(JITO_GCP_BUCKET, snapshot_regex)
    snapshots_found = list(
        filter(lambda blob: blob.size > 10_000_000_000, snapshots_found)
    )
    if len(snapshots_found) == 0:
        print(f"No snapshot found for epoch {target_epoch} at slot {target_slot}")
        sys.exit(1)

    selected_snapshot_blob = snapshots_found[0]
    found_preferred = False
    for snapshot in snapshots_found:
        if JITO_PREFERRED_WAREHOUSE in snapshot.name:
            selected_snapshot_blob = snapshot
            found_preferred = True
            break
    selected_snapshot_name = selected_snapshot_blob.name.split("/")[-1]
    print(f"Selected snapshot: {selected_snapshot_name} (preferred: {found_preferred})")

    return (selected_snapshot_name, selected_snapshot_blob.media_link)


def find_blob_in_bucket(bucket_name, regex_pattern):
    client = storage.Client.create_anonymous_client()
    bucket = client.bucket(bucket_name)
    blobs = bucket.list_blobs()

    matching_files = []
    for blob in blobs:
        if re.search(regex_pattern, blob.name):
            matching_files.append(blob)

    return matching_files


def download_file(url: str, destination: str, label: str = None) -> None:
    response = requests.get(url, stream=True)
    snapshot_size = int(response.headers.get("content-length", 0))
    block_size = 1024

    with open(destination, "wb") as file, tqdm(
        desc="Downloading" if label is None else f"Downloading {label}",
        total=snapshot_size,
        unit="B",
        unit_scale=True,
        unit_divisor=1024,
        miniters=1,
    ) as bar:
        for data in response.iter_content(block_size):
            file.write(data)
            bar.update(len(data))


if __name__ == "__main__":
    main()
