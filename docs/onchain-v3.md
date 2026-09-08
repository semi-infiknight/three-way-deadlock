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
- Upgrade slot **494949810** (same program id)
- Vault state: `22MGcjwTaKHbnhM8vn2z66w8qQbgWHVSxkYd2rAYURau`
- New pool 50/30/20: `E49rz2ZdBqcpdbqcw4Pz5ptrtEQhF2BdwTMTHoXTWCeR`
- Swap A→B `2pPmtpnBEuKTP5NsaGNy7xaqd4yvvFG5osuajJq8b6jXuQqpsC9Gs368yam6csom6K9CRJhA74rpNCsSvccsDWbW`
  - quote = actualOut = **9943**
  - C unchanged; `getTransaction` `err: null`; cu_limit 1_000_000

## Deferred
Three program ids + CPI, BatchRouter, N>3, gauges, Jupiter, mainnet. Pre-split pools are layout-incompatible.
