# Goal

Generate a reasonable & transparent benchmark for all major Solana stake pool flavours / providers.

# Methodology

The data is generated from end-of-epoch ledger snapshots. The tool currently makes use of snapshots courtesy of Jito.

The process is simple:

1. Unpack the snapshot, reconstruct the bank, and use that bank (together with RPC to a lesser extent) to build a provider-specific set of metrics for all detected pools.
1. Normalize all provider-specific pool metrics into a common set of metrics.
1. Upload those metrics to static storage, where they can be pulled from and displayed at [coming soon]()

# Usage

```bash
./scripts/setup.sh
./scripts/prepare_epoch.py 516
./scripts/generate_metas.py 516
```

# Notes

- Assumes all jito rewards were collected prior to the stake pool validator list being updated
- Does not distinguish jito rewards from potential "donations" to stake accounts
- Rather than mirror the logic, inflation rewards are currently fetched through a RPC call
- The effective APY is based on the LST price appreciation, and therefore obtained from the snapshot at epoch N+1 when available, or through the live pool state if N+1 is the current epoch. A consequence of this is that the pools' effective APY for the latest complete epoch will likely change once the following epoch completes.
