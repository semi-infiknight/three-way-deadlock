# On-chain Router / Vault / Pool (one ELF)

## Layout
One Pinocchio program `8SMAn5rTDFCaLKAdNd1fXckxm1eHdZGDkZP7JqhLUpbP` (not three program ids).

| Role | On-chain | Transfers? |
|---|---|---|
| Router | Only `process_instruction` tags: `0` init, `1` join, `2` exit, `3` swap exact-in | Calls Vault CPIs |
| Vault | PDA `["vault-state"]` is token/LP mint authority. Per-pool reserve index on the pool account. `unlock`/`settle`/`lock` in one ix (RAM session; not EIP-1153). BPT mint/burn on the **caller**. | Yes (`Transfer`/`MintTo`/`Burn`) |
| Pool | `programs/three_amm_pio/src/pool.rs` + `three-amm-pool` quotes \(V=\prod R^w\) | **No** |

Client talks only to Router tags. Compute-budget ix `SWAP_COMPUTE_UNITS = 1_000_000` (> 200k) is prepended on every Router tx.

## Devnet (public `api.devnet.solana.com`)
- Upgrade slot **505925428** (layout v2 — vol fee + softening; same program id)
- Vault state: `22MGcjwTaKHbnhM8vn2z66w8qQbgWHVSxkYd2rAYURau`
- v2 pool 50/30/20: `3iwdZpU3N7fQLZ4mLQvk6BnB7EsrkVLhtQam2iEcv76m`
- Swap A→B `3v9bkBjQCpyJhoAR9XibsKLkRt8gGsEj3i4imXRnTcVAQSo7H6Y7ZNdmoDGWgiq5GiyNHmkVpFKkgWVcmZjQMcZF`
  - quote = actualOut = **9943** (vol override 0)
  - C unchanged; `getTransaction` `err: null`; cu_limit 1_000_000; cu ~68k
- Full receipts: [`upgrade.md`](upgrade.md)

## Layout v2 (vol fee + softening)

Pool account grows after `lp_supply` with virtual reserves + softening metadata. Swap may carry optional `volatility_bps`. See [`release.md`](release.md). Pre-v2 pools are layout-incompatible (re-init).

## Deferred
Three program ids + CPI, BatchRouter, N>3, gauges, Jupiter, mainnet. Pre-split / pre-v2 pools are layout-incompatible.
