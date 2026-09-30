# three-way-deadlock

True **3-asset** weighted AMM on Solana. One pool, three SPL reserves, constant-mean invariant

\[
V = \prod_i R_i^{w_i}
\]

Any pair is one hop. The untraded reserve **amount** does not change; implied prices versus that asset still move.

Not two independent 2-pools. Not an Ethereum Balancer port. Native SPL custody with a Balancer V3-style Router → Vault → Pool split in crates (combined Pinocchio ELF on-chain).

## Why this shape

Two 2-pools (A–B and A–C) leave the third leg’s price untouched when you trade A–B. A single weighted 3-pool couples all three spots on every swap, so liquidity is shared across every pair without a hop tax.

Weights are portfolio basis points summing to `10_000` (min 1% each). Equal-weight legs use the exact product path; unequal legs use floored Balancer `_calcOutGivenIn`.

## Architecture

| Role | Crate / on-chain | Holds tokens? |
|------|------------------|---------------|
| **Router** | `crates/three-amm-router` · Pinocchio ix tags | Calls Vault only |
| **Vault** | `crates/three-amm-vault` · PDA `["vault-state"]` | Yes — custody + per-pool reserve index |
| **Pool** | `crates/three-amm-pool` · `pool.rs` quotes | No — amounts only |
| **Math** | `crates/three-amm-math` | Framework-free \(V=\prod R^w\) |

Unlock → take/send → settle → lock is one Rust call stack (Solana has no EIP-1153). Bind checks reject a fake vault trio or fake LP mint before settle. Operating pool A leaves pool B reserves bitwise unchanged.

### Fees (on-chain)

Each pool stores a **base** `fee_bps` (fee on input). The Pinocchio swap path **measures** stress from trade size vs the input reserve (`fee_bps_for_trade`) by default. Clients may append an explicit `volatility_bps` on the swap ix to override via `fee_bps_for_volatility`. Higher measured/explicit volatility → weakly higher fee; override `0` keeps the base fee.

### Virtual-balance softening (on-chain)

After an exact-in swap, the pool seeds Mooniswap-style **virtual balances** at the pre-swap reserve vector. The **reverse** of that swap quotes against virtual reserves for a few discrete steps, then converges toward real. Same-direction flow still quotes real. Custody always updates **real** reserves; the untraded third reserve amount is unchanged on the real path.

## Release

See **[`docs/release.md`](docs/release.md)** for the plain-language explainer: what AMM research this helps with (LVR / stress fees, reverse-path arb softening), who benefits, and how to talk about the upgrade. Short version:

> True 3-asset weighted pool — any pair one hop. This release puts stress-sensitive fees and Mooniswap-style reverse softening on the settle path so LPs keep more value under size and immediate round-trips. The invariant is still constant-mean.

**Pool layout v2** (virt + softening fields) is **breaking** for pre-upgrade pool accounts — re-init after program upgrade. Record new pools in `docs/upgrade.md` once deployed.

## On-chain (Pinocchio, devnet)

- Program: [`8SMAn5rTDFCaLKAdNd1fXckxm1eHdZGDkZP7JqhLUpbP`](https://explorer.solana.com/address/8SMAn5rTDFCaLKAdNd1fXckxm1eHdZGDkZP7JqhLUpbP?cluster=devnet)
- Instructions: `0` init · `1` join · `2` exit · `3` swap exact-in (19-byte measured fee, or 27-byte + `volatility_bps`) · `4` pause
- Vault PDA `["vault-state"]` is token / LP mint authority; the pool account quotes only
- Weighted swaps need a ~1M CU compute-budget ix (path uses ~200k+)

Stack choice (Pinocchio now; Anchor v1 as shape reference): see [`docs/stack.md`](docs/stack.md). Layout receipts: [`docs/onchain-v3.md`](docs/onchain-v3.md), [`docs/upgrade.md`](docs/upgrade.md). Release narrative: [`docs/release.md`](docs/release.md).

## Build and test

```bash
cargo test -p three-amm-math
cargo test -p three-amm-vault
cargo test -p three-amm-router
cargo test -p three_amm_pio --lib
cargo build-sbf --manifest-path programs/three_amm_pio/Cargo.toml
```

Off-chain smoke through the shipped Router / Vault / Pool path:

```bash
cargo run -p consumer
# ends with CONSUMER_OK and prints v3_out=…
```

## Aggregator quote (exact-in)

Integrators (Jupiter-class routers) should call the same weighted math the vault settles — never a parallel formula.

**Library**

- `three_amm_math::swap_out_given_in_weighted(rin, win, rout, wout, amount_in, fee_bps)`
- `three_amm_math::fee_bps_for_volatility(base_fee_bps, volatility_bps)` when using the vol-aware path
- `three_amm_pool::WeightedPool::quote_exact_in_for_aggregator(...)`
- `three_amm_router::Router::quote_exact_in(vault, pool_id, pool, token_in, token_out, amount_in, volatility_bps)` — reads live vault reserves, no custody change

**CLI** (single `u64` line on stdout):

```bash
# base fee only (vol = 0 path)
cargo run -p consumer -- quote <rin> <win> <rout> <wout> <amount_in> <fee_bps>

# volatility-aware fee: effective fee = fee_bps_for_volatility(fee_bps, volatility_bps)
cargo run -p consumer -- quote <rin> <win> <rout> <wout> <amount_in> <fee_bps> <volatility_bps>
```

Representative check (50/30/20, 5M/3M/2M reserves, 10_000 in, 30 bps, calm vol):

```bash
cargo run -p consumer -- quote 5000000 5000 3000000 3000 10000 30
# → 9943
```

Wire that out-amount as `min_out` (or the router’s expected out) on the on-chain swap ix. Multi-hop / BatchRouter is deferred; this pool is already any-pair one-hop inside one 3-reserve vault.

## LP risk (weighted IL / LVR)

Providing liquidity is not the same as holding the three assets outside the pool.

- **Weighted impermanent loss** — as relative prices move, the pool rebalances toward the asset that fell. With unequal weights (e.g. 50/30/20), IL is skewed: the heavy leg dominates inventory risk. Fees compensate only if volume covers that path dependency.
- **Loss vs rebalancing (LVR-style)** — when the pool’s mid lags the external market, arbs trade against LPs and capture the gap. Fee bumps under stress and virtual-balance softening on the reverse path reduce (do not eliminate) how quickly that value leaves the pool.
- **Third-asset coupling** — a swap that never touches asset C still moves A/C and B/C spots. LPs are exposed to all three pairwise moves, which is the product feature and the risk.

This protocol does **not** use an oracle as the primary pricing mechanism (unlike proactive MMs). Spot comes from balances and weights.

## Security and status

- **Not audited.** Do not deposit mainnet funds you cannot lose.
- Upgrade authority is a single keypair on the current BPF-upgradeable program; treat that as trusted admin risk.
- Core math lives in `three-amm-math` and is shared with the Pinocchio program so quotes cannot silently diverge from settle.
- Active development / not audited. Vol fee + virtual softening are wired into `three_amm_pio` handlers; a BPF upgrade + new pools are still required before devnet matches this tree. Proceed with caution.
- Deferred (not this repo’s near goal): three program-id CPI split, Token-2022, Jupiter listing process, mainnet, gauges, N>3, unbalanced join/exit, TWAMM, factory UX.

## Docs

| Doc | Topic |
|-----|--------|
| [`docs/release.md`](docs/release.md) | **Release explainer** — research → LP value |
| [`docs/balancer-on-solana.md`](docs/balancer-on-solana.md) | Paper math on SPL vaults |
| [`docs/v3-stack.md`](docs/v3-stack.md) | Router / Vault / Pool mapping |
| [`docs/onchain-v3.md`](docs/onchain-v3.md) | One-ELF layout + devnet ids |
| [`docs/upgrade.md`](docs/upgrade.md) | Deploy / upgrade receipts |
| [`docs/stack.md`](docs/stack.md) | Pinocchio vs Anchor / Quasar |
| [`docs/memepairs.md`](docs/memepairs.md) | Adjacent product research (not this AMM) |
