# Stack decision (researched 2026-09)

## Myths

| Claim | Reality |
|---|---|
| Pinocchio is more secure than Anchor | **No.** Pinocchio is *more explicit* (every owner/signer check is visible). Missing a check is *more* likely. Security of this AMM is the invariant + vault PDA authority, not the framework brand. Pinocchio is audited (Zellic) and powers p-token; that is a *library* audit, not your program audit. |
| Quasar is better in every way | **No.** Blueshift Quasar is **beta, unaudited**, APIs can change. Field tests show it can beat naive Pinocchio on PDA CU (`create_program_address` vs `find`). That is not a reason to custody funds in unaudited macros. |
| Anchor is always too fat to ship DeFi | **Partly.** Anchor v1 Borsh-copy + 8-byte discs + logs cost CU and ~100KB+ binaries. This pool’s hot path is three token CPIs + a few muls — well under 1.4M CU either way. Deploy *rent* scales with `.so` size; Pinocchio wins on cost. |
| Anchor v2 already replaces this choice | **Not for this ship.** Anchor **2.0.0-rc.1** (Aug 2026) *is* Pinocchio-based and claims ~90% smaller binaries. It is an **RC**. We do not put a custody program on an RC for the first deploy. Revisit when 2.0 is stable. |

## Choice for gs-ball 3-way AMM

**On-chain (deployed):** Pinocchio `0.11` + `pinocchio-token` + `pinocchio-system`. Same `three-amm-math` crate as the reference.

**Kept:** `programs/three_amm` (Anchor 1.1.2) — instruction shape, PDA seeds, and CPI order learned from it. Not the deploy target.

**Rejected for now:** Quasar (unaudited beta). Anchor v2 RC (real option later).

Math stays framework-free so tests cannot diverge from the program.

## X / community (sampled 2026-09)

- Performance-minded Solana builders (order books, multisigs, p-token rewrite) **ship Pinocchio** in the open: zero-copy, no allocator, explicit CPI. Helius still points people at it as the `solana-program` replacement.
- **Anchor v2** is the other live conversation: OtterSec/Anchor 2.0.0-rc.1 is Pinocchio-backed (~90% smaller binaries, 2.8–50× CU in benches). Devs are excited about deploy rent dropping with smaller `.so` *plus* cheaper account rent. It is still an **RC**.
- **Quasar** is the Blueshift camp: Anchor-shaped macros, Pinocchio-class CU, unaudited beta. Friends who “lean Quasar” want the DX; they are not claiming a funds-custody audit.
- Official onboarding (Solana DevRel / bootcamp) is still **Anchor v1 + Kit** for 2026 beginners — that is pedagogy, not the CU frontier.

So: friends leaning Pinocchio are aligned with p-token / Helius. Friends leaning Quasar want macros on that same substrate. Anchor v2 is the eventual merge of those paths. This repo deploys **Pinocchio 0.11 now**, keeps the Anchor v1 reference, and does not put LP vaults on Quasar or Anchor RC.
