# Memepairs, memestocks, SV151 — GS-Ball context

Research snapshot: **2026-09-06**. Programs and PDAs are mainnet unless noted.

## What a memepair is

Not “a stock that became a meme” (GME/AMC 2021). A **hybrid**:

- **Base** = new joke coin
- **Quote** = another token people already treat as money or as a story (tokenized stock, SV151 packs, SOL, USDC, a meme)
- Buying the meme **sells the quote into the pool vault**
- That **locks on-chain float** of the quote (often thin), **prints volume on the quote**, and supports a “we’re locking the float / squeeze” narrative

Price is in **quote units**, not dollars. If the quote rips and the meme is flat in USD, the pair price falls.

## Analogues

### PONS (Robinhood Chain)

- Bonding curve priced in an **approved** ERC-20 (`pairToken`): ETH, USDG, tokenized NVDA/AAPL/HOOD/TSLA/SPY, etc.
- Not permissionless any-token; factory allowlist so you cannot pair vs a junk mint to fake price
- Whole lifecycle in that quote: buys, sells, graduation threshold, Uniswap v4 pool, creator payouts
- No silent ETH conversion (v2 docs)
- Docs: https://docs.ponsfamily.com/v2

### StonkFun / @LaunchOnSF (Raydium)

- `stonkfun.xyz` — “create coins paired with anything”
- Raydium **LaunchLab**: `quoteMint` on initialize; they publish one template per pair (xStocks, PreStocks, Sunrise/TAO, custom)
- STONK itself launched **STONK/SPYx** on Raydium CLMM
- Devs: https://www.stonkfun.xyz/developers

GS-Ball is the **Meteora** version of this product, not a Raydium fork.

## Meteora: two programs, two quote rules

| | DBC | DAMM v2 |
|--|-----|---------|
| Role | Bonding curve launch | Live AMM (and DBC graduation target) |
| Program | `dbcij3LWUppWqq96dh6gJWwBifmcGfLSB5D4DuSMaqN` | `cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG` |
| SPL quote (USDC, BONK, SV151) | Yes | Yes |
| Token-2022 quote, metadata only | Yes | Yes |
| Token-2022 quote with extra extensions (xStocks) | **No** (`InvalidQuoteMint`) | **Only with operator token badge** |
| Badge escape hatch | None (unused `token_badge` constant) | `create_token_badge` (Meteora only) |

DBC `is_supported_quote_mint`: SPL Token program → allow. Token-2022 → every extension must be `MetadataPointer` or `TokenMetadata`. Else false.

DAMM `is_supported_mint`: SPL → allow. T22 permissionless: transfer fee, metadata pointer, token metadata, transfer hook **only if program id and authority are both unset**. Else need badge remaining account.

DBC migrate CPI to DAMM does **not** pass token-badge accounts. Even if DBC accepted NVDAx, graduation would still fail today.

### Token badge (what it is)

A small PDA: seeds `token_badge` + mint, owned by DAMM v2, stores the mint. Means “Meteora reviewed this dangerous T22 mint; pools may use it.” Not an LP NFT. You cannot create one; operators do (form + Discord).

## On-chain checks (2026-09-06)

DBC `getProgramAccounts`, memcmp `quote_mint` at offset 8:

| Quote | Accounts |
|-------|----------|
| WSOL | 437,231 |
| USDC | 44,909 |
| JUP | 201 |
| BONK | 17 |
| NVDAx | **0** |
| SV151 | **0** (valid SPL quote, unused as DBC quote yet) |

### xStocks / NVDAx

Mint: `Xsc9qvGR1efVDFGLrVsmkzv3qi45LTBjeUKSPmx9qEh`  
Program: Token-2022. Decimals 8. Freeze + mint authorities set.

Extensions include: metadata pointer, token metadata, **permanent delegate**, **default account state**, **confidential transfer**, **transfer hook** (program disabled, **authority still set**). That fails both DBC quotes and DAMM permissionless mint checks.

DAMM badge **exists**: `HoeLtVnW6oWdhJCQrWDTwLSitWmUaR3bszz79ExYQPEM`

Example DAMM v2 pools with NVDAx:

- token A: `6tKMbxufBSa6x5V2Cv6tA2YLmgng5eKKQ8E8zBQ7jqxL`
- token B: `6WQav5gEHHRKM4UyZMFhtviMp8z4VKEhSUjZUTXeZhpk`, `7vsUne4nTqPjtk8r32h5bQVmbixdBqKziiXj1GfDpTwu`, `DhrqCc5xSQNSVzdXtewhB3oNzQfuKcg6vMCUJerA11gZ`, `DjhSmVxawbqsucrtdEdFFrGmm8TKUZYnWMzYkyjXQoCL`, `EQv5w5CJFxMGST8F1b3f2bK9jEJepQtk97z7pXGhJd7J`

Squeeze story caveats: you lock **on-chain wrapper float**, not NYSE shares. Issuer can mint vs custody, pause, or seize (permanent delegate). Scaled UI (splits) can desync “shares locked” copy from raw vault amounts. Pool quotes 24/7; cash equity does not.

### SV151 memepairs

**SV151** = Sunrise / Meteora “Dynamic Asset”: fractional claim on custodied Pokémon Scarlet & Violet 151 sealed packs (Bedrock / Dynamic Assets), not an equity.

Mint: `SV151D5pjygAKA8aJJcKzm4wFnRX5G92Fye94jQJk7g`  
Program: **SPL Token** (`Tokenkeg…`). Decimals 6. Mint + freeze **revoked**.

- DAMM badge PDA `BqHLxzzRKXviaMLJzYvfHdM6pksLRPQcP2sHguYzizCx` → **AccountNotFound** (not needed)
- DAMM v2: **27** pools as token A, **4** as token B (e.g. SV151/USDC `KKyUyWncRfakBZh2M318BFfdR6332WWu1NePd9amQtj`)
- Launched via **DBC as base vs USDC**, then DAMM — the inverse of using it as quote
- Using SV151 **as quote** for new joke coins: DBC **allows** it (SPL). No config does that yet. GS-Ball can be first.

Badge is **not** “RWA vs meme.” It is **T22 extra extensions vs vanilla SPL**. SV151 is vanilla SPL.

## GS-Ball quote router

```
quote mint
  ├─ owner == Tokenkeg          → DBC path (SV151, USDC, WSOL, BONK, JUP, …)
  ├─ T22 metadata-only          → DBC path
  ├─ T22 extra extensions
  │     └─ DAMM badge exists    → DAMM path (xStocks)
  └─ else                       → reject (or wait for Meteora badge)
```

Allowlist anyway (PONS-style): junk quote mints fake market cap.

`migrationQuoteThreshold` is **raw** smallest units. SV151 is 6 decimals; NVDAx is 8. Do not reuse SOL lamports constants.

Meteora keepers auto-migrate some SOL/USDC/JUP thresholds. SV151-as-quote needs **GS-Ball migrator**.

Non-SOL quotes: Jupiter/Photon/Axiom often hide them. Product must include **our** swap + charts.

## Suggested v1 quote set

1. USDC — DBC, proven
2. WSOL — DBC, proven
3. **SV151** — DBC, first memepair narrative for GS-Ball (pack float lock)
4. **NVDAx** (optional same release) — DAMM + existing badge, no curve

## References

- DBC: https://docs.meteora.ag/core-products/dbc/what-is-dbc
- DBC T22 / quotes: https://docs.meteora.ag/core-products/dbc/token-2022-support
- DAMM T22 / badges: https://docs.meteora.ag/core-products/damm-v2/token-2022-support
- DBC source `is_supported_quote_mint`: https://github.com/MeteoraAg/dynamic-bonding-curve
- DAMM source `is_supported_mint` + `create_token_badge`: https://github.com/MeteoraAg/cp-amm
- PONS custom pairs: https://docs.ponsfamily.com/v2
- StonkFun: https://www.stonkfun.xyz/developers
- SV151 mint: `SV151D5pjygAKA8aJJcKzm4wFnRX5G92Fye94jQJk7g`
- NVDAx mint: `Xsc9qvGR1efVDFGLrVsmkzv3qi45LTBjeUKSPmx9qEh`
