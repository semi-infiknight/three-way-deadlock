# Weighted 3-asset AMM upgrade (public Solana devnet)

## Pending: layout v2 (vol fee + virtual softening)

Code in this repo extends `Pool` with `virt_*`, softening step counters, and last-swap direction. Swap ix: **19 bytes** → measured fee; **27 bytes** → optional `volatility_bps` override. Reverse of last swap quotes on virtual reserves.

- **Status:** implemented + unit-tested in `three_amm_pio`; **not yet** recorded as a new BPF upgrade slot below.
- **Action after `cargo build-sbf` + `solana program deploy/upgrade`:** re-init pools (old `POOL_LEN` fails), run a vol-0 A→B (expect out **9943** on 5M/3M/2M @ 30 bps), then a reverse and confirm out ≤ real-reserve reverse quote; append slot + pool ids here.
- Narrative for sharing: [`docs/release.md`](release.md).

## Cluster
- RPC: `https://api.devnet.solana.com`
- Not mainnet-beta. Not localhost.

## Program (same id, upgraded)
- Program id: `8SMAn5rTDFCaLKAdNd1fXckxm1eHdZGDkZP7JqhLUpbP`
- Owner: `BPFLoaderUpgradeab1e11111111111111111111111`
- ProgramData: `GhsPGKqYp3PAZi9aXTxiUUZexqTPfS5fN98PJQyJDnp8`
- Upgrade authority: `5bMjKkWCnTv1TDeSn24j2rL9zuw7CLEou1XHUD3e7PGX`
- Last deployed slot **before**: `494517504`
- Last deployed slot **after**: `494895366` (stable on second `program show`)
- Data length: `80000` bytes (ELF 57344; no extend needed)
- `getAccountInfo`: executable `true`, owner BPF upgradeable loader
- Math: Balancer constant-mean `V = ∏ R_i^{w_i}` (Martinelli & Mushegian 2019). Weights u16 bps, ∑ = 10000, min 1%. Equal weights still use exact `x*y*z` product path.

Explorer: https://explorer.solana.com/address/8SMAn5rTDFCaLKAdNd1fXckxm1eHdZGDkZP7JqhLUpbP?cluster=devnet

## New weighted pool (do not reuse `Gjk93Df…`)
- Pool: `HNmZvQLbw7DgSTHTnGYpb4yzgMieNko6mz5myHaAaprN`
- Weights on-chain: **5000 / 3000 / 2000** (50% / 30% / 20%)
- Fee: 30 bps
- Mints:
  - A `3DD6epG5NksjGvXEpfPc8gkX5TvKifUT5XmcTW5praNA`
  - B `7fgxiUUWEY7xJD5JqZnCFvB25inPiA2w3PiYxuWkCGji`
  - C `GoHb13rxornUjD2rLnUwqvZLWAzvmF2AznyJQkz3kk7u`
- Init: `3hThYFMC886AMHoM98wLFn37L3E6aEPiJJYCxTbvVharFmpqftNv4rVWC5qCFzkHfXM2LBiCHfarui8YNMo6tMy3`
- Add liquidity (5e6 / 3e6 / 2e6): `5SfLXUfLRGmCtLvvtqs2WP37zz3gPngmdBMBKHrsfPkkjXLEmuuQEV9nezQVxGWr5CfwUHLzUeepN72TjBMBYZ19`

Explorer: https://explorer.solana.com/address/HNmZvQLbw7DgSTHTnGYpb4yzgMieNko6mz5myHaAaprN?cluster=devnet

## Swaps (quoted with shipped `swap_out_given_in_weighted`)
Quotes came from `target/release/consumer quote` → `three_amm_math::swap_out_given_in_weighted` (same crate the program calls). Client CU budget 1_000_000 (weighted `ln`/`exp` exceeds the 200k default; measured ~212k–216k CU).

### Swap 1 — A→B (C unchanged)
- Sig: `2UtrQHMLrGcw8qbBTV4WNp1RCdXjQHXTZqQ8Fv9kRkMjoJAZp7LvoTfexjrsAbHp3j6XJALwVt4LHhcPeFEZFW3p`
- Slot: `494896297`
- Quote = actualOut = **9943**
- Reserves: `{a: 5000000, b: 3000000, c: 2000000}` → `{a: 5010000, b: 2990057, c: 2000000}`
- `getTransaction` `err`: null

https://explorer.solana.com/tx/2UtrQHMLrGcw8qbBTV4WNp1RCdXjQHXTZqQ8Fv9kRkMjoJAZp7LvoTfexjrsAbHp3j6XJALwVt4LHhcPeFEZFW3p?cluster=devnet

### Swap 2 — B→C (A unchanged)
- Sig: `2AnpKzYxhfbzP4RaMP4mx5mSMfekjbyEQGixmojy6dLqEvhChuLhmJ7Ea8pw1jDFqnneiHRMz2PFsPmKsC8QFGYe`
- Slot: `494896390`
- Quote = actualOut = **9961**
- Reserves: `{a: 5010000, b: 2990057, c: 2000000}` → `{a: 5010000, b: 3000057, c: 1990039}`
- `getTransaction` `err`: null

https://explorer.solana.com/tx/2AnpKzYxhfbzP4RaMP4mx5mSMfekjbyEQGixmojy6dLqEvhChuLhmJ7Ea8pw1jDFqnneiHRMz2PFsPmKsC8QFGYe?cluster=devnet

## Tests
- `cargo test -p three-amm-math`: 16 passed (equal-weight = product; 80/10/10 spot; ln-invariant does not drop)
- `cargo test -p three_amm_pio --lib`: 3 passed (honest bind; fake vault_in rejected; fake LP rejected)

## Remaining gaps
- Not an audit. Upgrade authority is a single keypair.
- Pre-upgrade pool `Gjk93DfMNXFfSYAi5Q8FGkwarwUUqAWRKjWnq47Dr98m` is layout-incompatible; do not use it.
- Weighted swap needs a compute-budget ix (~212k CU). Default 200k fails.
- Public RPC 429s; confirm via `getTransaction`, do not treat send-without-confirm as success.
- No Jupiter adapter, Token-2022, factory UX, gauges, or mainnet.
