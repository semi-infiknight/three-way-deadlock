# Balancer V3-style stack (Solana)

## What maps

| V3 | This repo | Role |
|---|---|---|
| Router | `crates/three-amm-router` | Only user-facing entry: `initialize_weighted_pool`, `swap_exact_in`, `add_liquidity_proportional`, `remove_liquidity_proportional`. |
| Vault | `crates/three-amm-vault` | Token custody + **per-pool reserve index**. `unlock` → `take`/`send` → `settle` → `lock`. |
| Weighted Pool | `crates/three-amm-pool` | Amounts only (`quote_swap_exact_in`, join/exit). Calls `three-amm-math`. **Does not** hold or transfer tokens. |
| Weighted math | `crates/three-amm-math` | \(V=\prod R_i^{w_i}\), fee on the way in, equal-weight product path. |

Unlock session is one Rust call stack (Solana has no EIP-1153 transient storage). That is the honest analogue of V3 `unlock` / settle, not a bit-identical Vault.

Bind: `Vault::vaults_match_pool` / `Vault::lp_matches_pool` reject a fake vault-in and a fake LP mint. Router returns `BindVault` / `BindLp` before settle.

BPT: join `mint_bpt`s to the caller; exit `burn_bpt`s from the caller **before** sending reserves. BPT is not vault pool inventory (`mint_totals` untouched). A user with 0 BPT cannot exit another user's join.

## Evidence

- Two pools, settle/swap on A, B reserves bitwise-unchanged (`cargo test -p three-amm-vault`, `operate_pool_a_does_not_move_pool_b_through_router`).
- Router 50/30/20 given-in out equals `swap_out_given_in_weighted`; third reserve amount unchanged.
- Equal-weight pair (40/40/20 A→B) equals `swap_out_given_in`.
- Consumer `v3_out=9943` twice (50/30/20, 10_000 in, 30 bps) — same as `apply_swap_weighted`.

## Deferred (not this goal)

BatchRouter / multi-hop, CompositeLiquidityRouter, BufferRouter, ERC4626 buffers, rate providers, Stable/Gyro/LBP/hooks, unbalanced join/exit, N>3, Permit2/WETH, gauges/veBAL, factory UX, Jupiter, mainnet, Token-2022, balancer.fi UI.

On-chain Pinocchio `programs/three_amm_pio` is still a combined binary (devnet staging). The **protocol** split is the crates above; a future deploy can CPI Router → Vault with Pool as a quote program.
