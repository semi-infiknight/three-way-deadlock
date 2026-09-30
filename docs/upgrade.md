# Weighted 3-asset AMM upgrade (public Solana devnet)

## Layout v2 deploy (vol fee + virtual softening) — live

Narrative: [`docs/release.md`](release.md).

- Swap ix: **19 bytes** → measured fee; **27 bytes** → optional `volatility_bps` override (e2e uses override `0` for classic base-fee receipts).
- Pool stores `virt_*`, softening steps, last direction. Reverse of last swap quotes on virtual; settle updates **real** only. Account length **354** bytes.
- Build: Agave `cargo-build-sbf` **3.1.10** + platform-tools **v1.52** (rustc 1.89); pin `zeroize = "=1.8.1"` for older toolchains.
- ELF: `target/deploy/three_amm_pio.so` — **59344** bytes — sha256 `a0e4e3543bf52a4d122dccc2826c4fa4709f6fc5e886517912e45fa5d0f4b53e`

### Program (same id, upgraded)
- Program id: `8SMAn5rTDFCaLKAdNd1fXckxm1eHdZGDkZP7JqhLUpbP`
- Owner: `BPFLoaderUpgradeab1e11111111111111111111111`
- ProgramData: `GhsPGKqYp3PAZi9aXTxiUUZexqTPfS5fN98PJQyJDnp8`
- Upgrade authority: `5bMjKkWCnTv1TDeSn24j2rL9zuw7CLEou1XHUD3e7PGX`
- Last deployed slot **before v2**: `494949810`
- Last deployed slot **after v2**: **`505925428`**
- Data length: `80000` bytes (ELF 59344; no extend needed)

Explorer: https://explorer.solana.com/address/8SMAn5rTDFCaLKAdNd1fXckxm1eHdZGDkZP7JqhLUpbP?cluster=devnet

### New v2 pool (do not reuse `HNmZvQL…` / `Gjk93Df…`)
- Pool: **`3iwdZpU3N7fQLZ4mLQvk6BnB7EsrkVLhtQam2iEcv76m`**
- Vault state: `22MGcjwTaKHbnhM8vn2z66w8qQbgWHVSxkYd2rAYURau`
- Weights on-chain: **5000 / 3000 / 2000**
- Base fee: 30 bps
- Mints:
  - A `Ai8UPi6hAkav1M9exqsEixSmkizCd9fNmXroV3Whfoo`
  - B `3Ty1aajpT425Sf5JJ8k3GGXYwJYjD49zhgSbYtzzoK3f`
  - C `FLaBu7QNYG8iLLE7pK1NpvQgcLyW3wxeYTkFZ26CMF5n`
- Init: `3N8gN5SGHpEs8zB93N1qTb4V2FiCzaCfWGimkRHTyUQ6xM1y384UfmaeNL14oC7axUuAYVZuYdzs23eTFbmZWKkw`
- Add liquidity (5e6 / 3e6 / 2e6): `5XVEeTLhGVFCt9Gdo1PzFtZNqvu1g8ZsjrYhSQL1tcF2c9tXgT6tgmJd3ni27UB5Q5z14UYDJyWDDozDB2hjyWFd`

Explorer: https://explorer.solana.com/address/3iwdZpU3N7fQLZ4mLQvk6BnB7EsrkVLhtQam2iEcv76m?cluster=devnet

### Swaps (vol override `0` → base 30 bps; quote = `consumer quote`)
CU limit 1_000_000. Consumed ~68k–72k on these paths.

#### Swap 1 — A→B (C unchanged)
- Sig: `3v9bkBjQCpyJhoAR9XibsKLkRt8gGsEj3i4imXRnTcVAQSo7H6Y7ZNdmoDGWgiq5GiyNHmkVpFKkgWVcmZjQMcZF`
- Slot: `505925592`
- Quote = actualOut = **9943**
- Reserves: `{a: 5000000, b: 3000000, c: 2000000}` → `{a: 5010000, b: 2990057, c: 2000000}`
- `thirdUnchanged`: true · `match`: true

https://explorer.solana.com/tx/3v9bkBjQCpyJhoAR9XibsKLkRt8gGsEj3i4imXRnTcVAQSo7H6Y7ZNdmoDGWgiq5GiyNHmkVpFKkgWVcmZjQMcZF?cluster=devnet

#### Swap 2 — B→C (A unchanged)
- Sig: `4BjGNCLQN84yeCvXJtyoiv3BPcUGVgQwE6m9c8mygJ3oVDpENipPKdxBhjPGmT3AqUgj7UVaYnjUbuoNttvYoudM`
- Slot: `505925613`
- Quote = actualOut = **9961**
- Reserves: `{a: 5010000, b: 2990057, c: 2000000}` → `{a: 5010000, b: 3000057, c: 1990039}`
- `thirdUnchanged`: true · `match`: true

https://explorer.solana.com/tx/4BjGNCLQN84yeCvXJtyoiv3BPcUGVgQwE6m9c8mygJ3oVDpENipPKdxBhjPGmT3AqUgj7UVaYnjUbuoNttvYoudM?cluster=devnet

## Cluster
- RPC: `https://api.devnet.solana.com`
- Not mainnet-beta. Not localhost.

## Prior weighted pool (stale — pre-v2 layout)
- Pool: `HNmZvQLbw7DgSTHTnGYpb4yzgMieNko6mz5myHaAaprN` — **do not use** after layout v2
- Earlier equal-weight pool `Gjk93DfMNXFfSYAi5Q8FGkwarwUUqAWRKjWnq47Dr98m` — also incompatible
- Prior program slots: `494895366` / `494949810` (pre-v2 ELF ~57344 bytes)

## Tests (local, this tree)
- `cargo test -p three-amm-math` / vault / router — green
- `cargo test -p three_amm_pio --lib` — 16 passed (measured fee, vol override, reverse softening, bind)
- `node scripts/devnet-e2e.mjs` — `SWAP_OK` after slot `505925428`

## Remaining gaps
- Not an audit. Upgrade authority is a single keypair.
- Public RPC 429s; confirm via `getTransaction`.
- No Jupiter adapter, Token-2022, factory UX, gauges, or mainnet.
