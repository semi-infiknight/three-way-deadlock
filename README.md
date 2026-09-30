# three-way-deadlock

True **3-asset** weighted AMM on Solana. One pool, three reserves, \(V=\prod R_i^{w_i}\).

Any pair one hop. The untraded reserve **amount** does not change; implied prices still move.

## On-chain (Pinocchio)

- Program: [`8SMAn5rTDFCaLKAdNd1fXckxm1eHdZGDkZP7JqhLUpbP`](https://explorer.solana.com/address/8SMAn5rTDFCaLKAdNd1fXckxm1eHdZGDkZP7JqhLUpbP?cluster=devnet)
- Ixs: `0` init, `1` join, `2` exit, `3` swap exact-in, `4` pause
- Vault PDA `["vault-state"]` is token authority; pool quotes only

```bash
cargo test -p three-amm-math
cargo build-sbf --manifest-path programs/three_amm_pio/Cargo.toml
```
