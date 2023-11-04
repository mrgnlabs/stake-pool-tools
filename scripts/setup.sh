#!/usr/bin/env bash
set -e

ROOT=$(git rev-parse --show-toplevel)

pip3 install -r ${ROOT}/requirements.txt
cargo build --release
