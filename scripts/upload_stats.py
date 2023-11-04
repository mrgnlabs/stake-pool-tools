#!/usr/bin/env python3

import os
import json
import re
from google.cloud import storage


MRGN_GCP_BUCKET = "mrgn-public"
STAKE_POOL_STATS_DIR = "stake_pool_data"
STAKE_POOL_FILENAME_TEMPLATE = "stats_{}.json"


def main():
    client = storage.Client()
    bucket = client.bucket(MRGN_GCP_BUCKET)

    script_dir = os.path.dirname(os.path.abspath(__file__))
    stats_dir = os.path.realpath(os.path.join(script_dir, "..", "output"))
    stats_files = os.listdir(stats_dir)
    stats_files.sort()
    stats_file_regex = "stats_([0-9]+).json"

    manifest = {"latest": None, "epochs": []}

    for stats_file in stats_files:
        match = re.match(stats_file_regex, stats_file)
        if match:
            epoch = int(match.groups()[0])

            blob_name = f"{STAKE_POOL_STATS_DIR}/{stats_file}"
            blob = bucket.blob(blob_name)

            stats_path = os.path.join(stats_dir, stats_file)

            print("Uploading {} to {}".format(stats_path, blob_name))
            blob.upload_from_filename(stats_path)
            blob.acl.all().grant_read()
            blob.acl.save()

            if manifest["latest"] is None or epoch > manifest["latest"]:
                manifest["latest"] = epoch
            manifest["epochs"].append(epoch)

    manifest_blob_name = f"{STAKE_POOL_STATS_DIR}/manifest.json"
    manifest_blob = bucket.blob(manifest_blob_name)
    print("Uploading manifest to {}".format(manifest_blob_name))
    manifest_blob.upload_from_string(json.dumps(manifest))
    blob.acl.all().grant_read()
    blob.acl.save()


if __name__ == "__main__":
    main()
