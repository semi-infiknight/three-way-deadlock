# Release: vol-aware fees + reverse-path softening

## What this is

A **3-asset weighted AMM** on Solana (\(V=\prod R_i^{w_i}\)): one pool, any pair one hop, untraded reserve **amount** fixed. This release upgrades the Pinocchio program so the research-backed LP protections are **on the settle path**, not only in off-chain crates.

## What the research helps with (plain language)

AMMs lose value for LPs in two main ways that papers and prior designs keep circling:

1. **Stale mid vs the market (LVR / arb extract)**  
   When the pool’s price lags the outside world, arbs trade against LPs and capture the gap. Fees are how LPs get paid for that risk. A **flat** fee undercharges calm flow and overcharges (or still underprotects) stressed flow.

2. **Immediate round-trip arb after a large swap (Mooniswap insight)**  
   Right after a big trade, the reverse direction is often a free(ish) arb. Softening the reverse quote for a short window leaves more of that value with LPs.

**What we shipped from that research**

| Idea | Source lineage | What we did |
|------|----------------|-------------|
| Geometric-mean / stress-sensitive fees | G3M fee literature + vol-adjusted fee practice (Lifinity/Hydraswap-class) | Effective fee rises when trade size vs reserve is large, or when a client passes an explicit volatility input |
| Virtual-balance reverse softening | Mooniswap | After a swap, the **opposite** direction quotes against pre-swap virtual reserves for a few discrete steps, then converges to real |
| Constant-mean 3-asset pool | Balancer paper | Already the core product — this release does not change the invariant |

This is **not** an oracle proactive market maker (Lifinity-style). Spot still comes from balances and weights. We borrow the *LP protection* ideas, not the pricing engine.

## Who benefits

- **LPs** — large or stressful flow pays a bit more fee; immediate reverse arbs get a worse quote for a short window.
- **Integrators / aggregators** — same weighted math; calm path with `volatility_bps = 0` still matches the classic 50/30/20 → `9943` receipt; measured path is the default when the short swap ix is used.
- **Traders** — still one-hop any pair; third asset amount unchanged on the real path.

## On-chain behavior (v2 pool layout)

- **Fee (default):** `fee_bps_for_trade(base_fee, reserve_in, amount_in)` — larger size vs reserve → weakly higher fee.
- **Fee (optional):** append `volatility_bps` (u64 LE) to the swap ix → `fee_bps_for_volatility(base, vol)`.
- **Softening:** pool stores `virt_*`, step counters, last direction. Reverse of last swap quotes on virtual; settle always updates **real** reserves.
- **Breaking:** pre-v2 pool accounts fail the new `POOL_LEN` check — re-init pools after upgrade (same class of break as the weighted upgrade).

### Swap ix

| Bytes | Meaning |
|-------|---------|
| 19 | tag `3` + tin + tout + amount_in + min_out → **measured** fee |
| 27 | same + `volatility_bps` → **override** fee |

## How to talk about it (short)

> We ship a true 3-asset weighted pool on Solana — any pair one hop. This release adds two LP protections from AMM research: fees that rise under stress, and Mooniswap-style softening so the reverse trade right after a swap is less of a free arb. The curve is still constant-mean; we did not switch to an oracle PMM.

## Verify locally

```bash
cargo test -p three-amm-math
cargo test -p three_amm_pio --lib
cargo run -p consumer -- quote 5000000 5000 3000000 3000 10000 30
# → 9943  (base / vol-0 path)
```

### Live (devnet)
- Program slot: **`505925428`** (id `8SMAn5rTDFCaLKAdNd1fXckxm1eHdZGDkZP7JqhLUpbP`)
- Pool: `3iwdZpU3N7fQLZ4mLQvk6BnB7EsrkVLhtQam2iEcv76m` — A→B out **9943** (vol override 0)
- Full receipts: [`upgrade.md`](upgrade.md)

Build note: use Agave `cargo-build-sbf` ≥ 3.1 with platform-tools **v1.52** (rustc 1.89), and keep `zeroize = "=1.8.1"` pinned for older SBF cargos.
