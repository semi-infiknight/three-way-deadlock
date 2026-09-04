//! 3-asset Balancer CFMM: `V = ∏ R_i^{w_i}` (Martinelli & Mushegian 2019).
//! Equal weights `w_i = 1/3` reduce to `k = x * y * z`.
#![cfg_attr(not(test), no_std)]
//!
//! Any pair swap is one-hop. The untouched reserve stays in the product
//! (its *amount* is unchanged; implied prices vs the third asset still move).

pub mod pow;
pub mod weighted;

pub use weighted::{
    apply_swap_weighted, equal_weights, ln_invariant, ln_invariant_ge, swap_out_given_in_weighted,
    validate_weights, weighted_spot_e9, MIN_WEIGHT, WEIGHT_DENOM,
};

use core::cmp::Ordering;

pub const FEE_DENOM: u64 = 10_000;

/// 192-bit unsigned product (fits three `u64` factors).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct U192 {
    pub hi: u64,
    pub lo: u128,
}

impl PartialOrd for U192 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for U192 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.hi.cmp(&other.hi).then(self.lo.cmp(&other.lo))
    }
}

fn mul_u128_u64(a: u128, b: u64) -> U192 {
    let b = b as u128;
    let a_lo = a as u64 as u128;
    let a_hi = a >> 64;
    let p0 = a_lo * b;
    let p1 = a_hi * b;
    let p0_lo = p0 as u64 as u128;
    let p0_hi = p0 >> 64;
    let mid = p0_hi + (p1 & 0xffff_ffff_ffff_ffff);
    let lo = p0_lo + ((mid & 0xffff_ffff_ffff_ffff) << 64);
    let hi = (p1 >> 64) + (mid >> 64);
    U192 {
        hi: hi as u64,
        lo,
    }
}

/// `k = x * y * z` as 192-bit integer.
pub fn invariant(x: u64, y: u64, z: u64) -> U192 {
    let xy = (x as u128)
        .checked_mul(y as u128)
        .expect("xy overflow");
    mul_u128_u64(xy, z)
}

pub fn invariant_ge(after: U192, before: U192) -> bool {
    after >= before
}

/// Apply fee to input: `floor(amount * (denom - fee_bps) / denom)`.
pub fn amount_after_fee(amount_in: u64, fee_bps: u64) -> Option<u64> {
    if fee_bps >= FEE_DENOM {
        return None;
    }
    let num = (amount_in as u128).checked_mul((FEE_DENOM - fee_bps) as u128)?;
    Some((num / FEE_DENOM as u128) as u64)
}

/// One-hop swap `token_in -> token_out` on equal-weight product.
/// Third reserve is unused in the *amount* formula but remains in `k`.
/// Output is floored (favors the pool).
pub fn swap_out_given_in(
    reserve_in: u64,
    reserve_out: u64,
    amount_in: u64,
    fee_bps: u64,
) -> Option<u64> {
    if reserve_in == 0 || reserve_out == 0 || amount_in == 0 {
        return None;
    }
    let dx = amount_after_fee(amount_in, fee_bps)?;
    if dx == 0 {
        return None;
    }
    let new_in = (reserve_in as u128).checked_add(dx as u128)?;
    let num = (reserve_out as u128).checked_mul(dx as u128)?;
    let out = num / new_in;
    if out == 0 || out >= reserve_out as u128 {
        return None;
    }
    Some(out as u64)
}

/// Inverse: input required for a desired output (ceil, favors pool).
pub fn swap_in_given_out(
    reserve_in: u64,
    reserve_out: u64,
    amount_out: u64,
    fee_bps: u64,
) -> Option<u64> {
    if amount_out == 0 || amount_out >= reserve_out || reserve_in == 0 {
        return None;
    }
    if fee_bps >= FEE_DENOM {
        return None;
    }
    // dx_eff = r_in * dy / (r_out - dy)  (ceil)
    let denom = (reserve_out - amount_out) as u128;
    let num = (reserve_in as u128)
        .checked_mul(amount_out as u128)?;
    let dx_eff = (num + denom - 1) / denom;
    // invert fee: amount_in = ceil(dx_eff * denom / (denom - fee))
    let fee_keep = (FEE_DENOM - fee_bps) as u128;
    let gross = (dx_eff
        .checked_mul(FEE_DENOM as u128)?
        + fee_keep
        - 1)
        / fee_keep;
    if gross > u64::MAX as u128 {
        return None;
    }
    Some(gross as u64)
}

/// Spot price of `quote` in units of `base` (1e9 scale): `quote/base * 1e9`.
pub fn spot_price_e9(reserve_base: u64, reserve_quote: u64) -> Option<u128> {
    if reserve_base == 0 {
        return None;
    }
    Some((reserve_quote as u128) * 1_000_000_000u128 / reserve_base as u128)
}

/// Integer cube root via binary search (for initial LP shares ≈ ∛(xyz)).
pub fn integer_cbrt(n: u128) -> u64 {
    if n == 0 {
        return 0;
    }
    let mut lo = 1u64;
    let mut hi = 1u64 << 42;
    while lo < hi {
        let mid = lo + (hi - lo + 1) / 2;
        let m = mid as u128;
        match m.checked_mul(m).and_then(|sq| sq.checked_mul(m)) {
            Some(cube) if cube <= n => lo = mid,
            _ => hi = mid - 1,
        }
    }
    lo
}

/// Initial LP minted from first three-sided deposit.
pub fn initial_lp(x: u64, y: u64, z: u64) -> Option<u64> {
    if x == 0 || y == 0 || z == 0 {
        return None;
    }
    let xy = (x as u128).checked_mul(y as u128)?;
    // cbrt(x*y*z) — if xyz doesn't fit u128, use cbrt(x)*cbrt(y)*cbrt(z) bound
    let shares = if let Some(xyz) = xy.checked_mul(z as u128) {
        integer_cbrt(xyz)
    } else {
        let a = integer_cbrt(x as u128) as u128;
        let b = integer_cbrt(y as u128) as u128;
        let c = integer_cbrt(z as u128) as u128;
        a.checked_mul(b)?.checked_mul(c)? as u64
    };
    if shares == 0 {
        None
    } else {
        Some(shares)
    }
}

/// Proportional add: mint `min_i(amount_i * supply / reserve_i)` and consume
/// matching amounts (floor). Does not change spot prices beyond 1-unit rounding.
pub fn proportional_add(
    reserves: [u64; 3],
    amounts: [u64; 3],
    supply: u64,
) -> Option<([u64; 3], u64)> {
    if supply == 0 {
        let lp = initial_lp(amounts[0], amounts[1], amounts[2])?;
        return Some((amounts, lp));
    }
    let mut lp = u64::MAX;
    for i in 0..3 {
        if reserves[i] == 0 || amounts[i] == 0 {
            return None;
        }
        let s = (amounts[i] as u128)
            .checked_mul(supply as u128)?
            / reserves[i] as u128;
        if s == 0 || s > u64::MAX as u128 {
            return None;
        }
        lp = lp.min(s as u64);
    }
    let mut used = [0u64; 3];
    for i in 0..3 {
        // ceil would pull extra; floor used amounts from minted shares
        used[i] = ((lp as u128)
            .checked_mul(reserves[i] as u128)?
            / supply as u128) as u64;
        if used[i] == 0 {
            return None;
        }
    }
    Some((used, lp))
}

pub fn proportional_remove(
    reserves: [u64; 3],
    lp_burn: u64,
    supply: u64,
) -> Option<[u64; 3]> {
    if lp_burn == 0 || supply == 0 || lp_burn > supply {
        return None;
    }
    let mut out = [0u64; 3];
    for i in 0..3 {
        out[i] = ((lp_burn as u128)
            .checked_mul(reserves[i] as u128)?
            / supply as u128) as u64;
    }
    Some(out)
}

/// Apply a swap to a 3-reserve vector. `token_in`/`token_out` are 0..2.
pub fn apply_swap(
    reserves: [u64; 3],
    token_in: usize,
    token_out: usize,
    amount_in: u64,
    fee_bps: u64,
) -> Option<([u64; 3], u64)> {
    if token_in > 2 || token_out > 2 || token_in == token_out {
        return None;
    }
    let dy = swap_out_given_in(reserves[token_in], reserves[token_out], amount_in, fee_bps)?;
    let mut next = reserves;
    next[token_in] = reserves[token_in].checked_add(amount_in)?;
    next[token_out] = reserves[token_out].checked_sub(dy)?;
    Some((next, dy))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(r: [u64; 3]) -> U192 {
        invariant(r[0], r[1], r[2])
    }

    #[test]
    fn fee_free_swap_preserves_k() {
        let r = [1_000_000u64, 2_000_000, 3_000_000];
        let k0 = k(r);
        let (r1, out) = apply_swap(r, 0, 1, 10_000, 0).unwrap();
        assert!(out > 0);
        // Flooring can increase k by <1 ulp of product; never decrease.
        assert!(k(r1) >= k0, "k dropped: {:?} -> {:?}", k0, k(r1));
        // Third reserve amount unchanged
        assert_eq!(r1[2], r[2]);
    }

    #[test]
    fn swap_moves_all_three_implied_prices() {
        let r = [1_000_000u64, 2_000_000, 3_000_000];
        let p01 = spot_price_e9(r[0], r[1]).unwrap();
        let p02 = spot_price_e9(r[0], r[2]).unwrap();
        let p12 = spot_price_e9(r[1], r[2]).unwrap();
        let (r1, _) = apply_swap(r, 0, 1, 50_000, 0).unwrap();
        let q01 = spot_price_e9(r1[0], r1[1]).unwrap();
        let q02 = spot_price_e9(r1[0], r1[2]).unwrap();
        let q12 = spot_price_e9(r1[1], r1[2]).unwrap();
        assert_ne!(p01, q01);
        assert_ne!(p02, q02, "A/C price must move even though C amount is fixed");
        assert_ne!(p12, q12, "B/C price must move even though C amount is fixed");
        assert_eq!(r1[2], r[2]);
    }

    #[test]
    fn two_independent_pools_do_not_satisfy_3way_price_coupling() {
        // Stand-in: two CPMM pools (A-B) and (A-C). Swap on A-B only.
        let ab = (1_000_000u64, 2_000_000u64);
        let ac = (1_000_000u64, 3_000_000u64);
        let p_ac_before = spot_price_e9(ac.0, ac.1).unwrap();
        let dx = 50_000u64;
        let _dy = swap_out_given_in(ab.0, ab.1, dx, 0).unwrap();
        // A-C pool is independent; its reserves (and price) do not change.
        let p_ac_after = spot_price_e9(ac.0, ac.1).unwrap();
        assert_eq!(
            p_ac_before, p_ac_after,
            "2-pool router leaves A/C pool price unchanged — not a 3-way pool"
        );
        // The 3-way pool on the same start state MUST move A/C.
        let r = [1_000_000u64, 2_000_000, 3_000_000];
        let (r1, _) = apply_swap(r, 0, 1, 50_000, 0).unwrap();
        assert_ne!(
            spot_price_e9(r[0], r[2]).unwrap(),
            spot_price_e9(r1[0], r1[2]).unwrap()
        );
    }

    #[test]
    fn fees_grow_k() {
        let r = [5_000_000u64, 5_000_000, 5_000_000];
        let k0 = k(r);
        let (r1, _) = apply_swap(r, 1, 2, 100_000, 30).unwrap(); // 30 bps
        assert!(k(r1) > k0);
    }

    #[test]
    fn invariant_must_not_drop() {
        let r = [9_000_000u64, 8_000_000, 7_000_000];
        let k0 = k(r);
        let (r1, _) = apply_swap(r, 2, 0, 1, 0).unwrap();
        assert!(invariant_ge(k(r1), k0));
    }

    #[test]
    fn proportional_add_is_price_neutral() {
        let r = [1000u64, 2000, 3000];
        let supply = 1000u64;
        let (used, lp) = proportional_add(r, [100, 200, 300], supply).unwrap();
        assert_eq!(used, [100, 200, 300]);
        assert_eq!(lp, 100);
        let r2 = [r[0] + used[0], r[1] + used[1], r[2] + used[2]];
        assert_eq!(spot_price_e9(r[0], r[1]), spot_price_e9(r2[0], r2[1]));
        assert_eq!(spot_price_e9(r[0], r[2]), spot_price_e9(r2[0], r2[2]));
    }

    #[test]
    fn proportional_remove_inverse() {
        let r = [1000u64, 2000, 3000];
        let supply = 1000u64;
        let out = proportional_remove(r, 100, supply).unwrap();
        assert_eq!(out, [100, 200, 300]);
    }

    #[test]
    fn drain_resistance_cannot_empty_out_reserve() {
        let r = [1_000u64, 1_000, 1_000];
        assert!(apply_swap(r, 0, 1, u64::MAX / 2, 0).is_none() || {
            let (n, dy) = apply_swap(r, 0, 1, 1_000_000_000, 0).unwrap();
            dy < r[1] && n[1] > 0
        });
        let (n, dy) = apply_swap(r, 0, 1, 1_000_000, 0).unwrap();
        assert!(dy < 1000);
        assert!(n[1] > 0);
    }

    #[test]
    fn rounding_favors_pool_on_tiny_swaps() {
        let r = [1_000_000u64, 1_000_000, 1_000_000];
        let k0 = k(r);
        // floor(1e6 / (1e6+1)) = 0 → rejected (dust cannot extract value)
        assert!(swap_out_given_in(1_000_000, 1_000_000, 1, 0).is_none());
        let out = swap_out_given_in(1_000_000, 1_000_000, 2, 0).unwrap();
        assert_eq!(out, 1); // floor(2e6/(1e6+2))=1
        let (r2, _) = apply_swap(r, 0, 1, 2, 0).unwrap();
        assert!(k(r2) >= k0);
    }

    #[test]
    fn triangle_fee_free_is_rounding_only() {
        let start = [10_000_000u64, 10_000_000, 10_000_000];
        let dx = 10_000u64;
        // A->B then B->C
        let (s1, db) = apply_swap(start, 0, 1, dx, 0).unwrap();
        let (s2, dc_path) = apply_swap(s1, 1, 2, db, 0).unwrap();
        // A->C direct from same start
        let (_d, dc_direct) = apply_swap(start, 0, 2, dx, 0).unwrap();
        let residual = (dc_direct as i128 - dc_path as i128).abs();
        // Sequential hops compound floor rounding; residual is tiny vs size.
        assert!(residual <= 5, "residual {residual} path={dc_path} direct={dc_direct}");
        assert_eq!(s2[0], start[0] + dx);
        let _ = s2;
    }

    #[test]
    fn fake_implementation_that_drops_k_is_rejected() {
        let r = [1_000_000u64, 1_000_000, 1_000_000];
        let k0 = k(r);
        // Attack: steal from the untouched reserve (2-pool + skim).
        let stolen = [1_000_010, 990_000, 999_000];
        assert!(
            !invariant_ge(k(stolen), k0),
            "a k-dropping stand-in must fail the conservation check"
        );
        let (honest, _) = apply_swap(r, 0, 1, 10_000, 0).unwrap();
        assert!(invariant_ge(k(honest), k0));
    }

    #[test]
    fn in_given_out_round_trips_at_least_requested() {
        let rin = 4_000_000u64;
        let rout = 6_000_000u64;
        let want = 1_000u64;
        let din = swap_in_given_out(rin, rout, want, 25).unwrap();
        let got = swap_out_given_in(rin, rout, din, 25).unwrap();
        assert!(got >= want, "got {got} want {want} din {din}");
    }

    #[test]
    fn equal_weights_match_product_formula() {
        let r = [1_000_000u64, 2_000_000, 3_000_000];
        let w = equal_weights();
        let dx = 10_000u64;
        let a = swap_out_given_in(r[0], r[1], dx, 0).unwrap();
        let b = swap_out_given_in_weighted(r[0], w[0], r[1], w[1], dx, 0).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn weighted_80_10_10_spot_and_swap() {
        // 80% A / 10% B / 10% C — Balancer-style index (mostly A).
        let w = [8000u16, 1000, 1000];
        assert!(validate_weights(w));
        let r = [8_000_000u64, 1_000_000, 1_000_000];
        let p_ba = weighted_spot_e9(r[0], w[0], r[1], w[1]).unwrap();
        // P_B per A = (Rb/wb)/(Ra/wa) = (1e6/1000)/(8e6/8000) = 1000/1000 = 1
        assert_eq!(p_ba, 1_000_000_000);
        let (r1, out) = apply_swap_weighted(r, w, 0, 1, 80_000, 0).unwrap();
        assert!(out > 0);
        assert_eq!(r1[2], r[2], "third reserve amount unchanged");
        let q_ba = weighted_spot_e9(r1[0], w[0], r1[1], w[1]).unwrap();
        let q_ca = weighted_spot_e9(r1[0], w[0], r1[2], w[2]).unwrap();
        let p_ca = weighted_spot_e9(r[0], w[0], r[2], w[2]).unwrap();
        assert_ne!(p_ba, q_ba);
        assert_ne!(p_ca, q_ca, "A/C weighted spot must move");
    }

    #[test]
    fn weighted_out_less_than_f64_plus_one() {
        let ri = 5_000_000u64;
        let ro = 2_000_000u64;
        let dx = 25_000u64;
        let wi = 2000u16;
        let wo = 5000u16;
        let got = swap_out_given_in_weighted(ri, wi, ro, wo, dx, 0).unwrap();
        let base = ri as f64 / (ri + dx) as f64;
        let expect = ro as f64 * (1.0 - base.powf(wi as f64 / wo as f64));
        let err = (got as f64 - expect).abs();
        assert!(err < 2.0, "got {got} expect {expect} err {err}");
        assert!((got as f64) <= expect + 1.0, "must not overpay vs real curve");
    }

    #[test]
    fn weighted_ln_invariant_does_not_drop() {
        let w = [5000u16, 3000, 2000];
        let r = [5_000_000u64, 3_000_000, 2_000_000];
        let v0 = ln_invariant(r, w).unwrap();
        let (r1, _) = apply_swap_weighted(r, w, 0, 2, 10_000, 0).unwrap();
        let v1 = ln_invariant(r1, w).unwrap();
        assert!(ln_invariant_ge(v1, v0), "v0={v0} v1={v1}");
        assert_eq!(r1[1], r[1]);
    }
}
