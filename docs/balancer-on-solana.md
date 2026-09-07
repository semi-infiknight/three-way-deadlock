# Balancer weighted CFMM on Solana (SPL)

Martinelli & Mushegian, 2019: a pool is a **self-balancing weighted index** that makes a market in every pair.

\[
V = \prod_{i} R_i^{w_i} = \text{const},\qquad \sum w_i = 1
\]

Spot:

\[
P_{i\to j} = \frac{R_j/w_j}{R_i/w_i}
\]

Out-given-in (Balancer V2, fee taken from input):

\[
\Delta_o = R_o\left(1 - \left(\frac{R_i}{R_i+\Delta_i^{\text{fee}}}\right)^{w_i/w_o}\right)
\]

Equal weights \(w_i=1/3\) reduce to \(xyz=k\) and \(\Delta_o = R_o\Delta_i/(R_i+\Delta_i)\).

**Weights** are integers summing to `10_000` (portfolio bps), e.g. `5000/3000/2000` = 50/30/20.

This is **not** Balancer’s Ethereum Vault. It is the **paper’s math** on SPL vaults (`programs/three_amm_pio`).
