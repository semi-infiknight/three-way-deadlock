//! Balancer weighted 3-asset CFMM (Martinelli & Mushegian 2019).
//! `V = ∏ R_i^{w_i}`, weights in bps summing to [`WEIGHT_DENOM`].

use crate::pow::{ln_raw, pow_ratio_down, ONE};
use crate::{amount_after_fee, swap_out_given_in};

/// ∑ w_i ln(R_i) (1e18 ln). Used to check V = ∏ R^w did not drop.
pub fn ln_invariant(reserves: [u64; 3], weights: [u16; 3]) -> Option<i128> {
    let a = ln_raw(reserves[0] as u128)?;
    let b = ln_raw(reserves[1] as u128)?;
    let c = ln_raw(reserves[2] as u128)?;
    a.checked_mul(weights[0] as i128)?
        .checked_add(b.checked_mul(weights[1] as i128)?)?
        .checked_add(c.checked_mul(weights[2] as i128)?)
}

/// True if weighted invariant did not drop beyond series noise.
pub fn ln_invariant_ge(after: i128, before: i128) -> bool {
    after + 1_000_000 >= before
}

pub const WEIGHT_DENOM: u16 = 10_000;
pub const MIN_WEIGHT: u16 = 100; // 1% — Balancer-style floor

pub fn validate_weights(w: [u16; 3]) -> bool {
    w.iter().all(|&x| x >= MIN_WEIGHT)
        && (w[0] as u32) + (w[1] as u32) + (w[2] as u32) == WEIGHT_DENOM as u32
}

pub fn equal_weights() -> [u16; 3] {
    [3333, 3333, 3334]
}

/// Spot `quote` per `base`: `(R_q / w_q) / (R_b / w_b)` at 1e9 scale.
pub fn weighted_spot_e9(
    reserve_base: u64,
    weight_base: u16,
    reserve_quote: u64,
    weight_quote: u16,
) -> Option<u128> {
    if reserve_base == 0 || weight_base == 0 || weight_quote == 0 {
        return None;
    }
    Some(
        (reserve_quote as u128)
            .checked_mul(weight_base as u128)?
            .checked_mul(1_000_000_000)?
            / (reserve_base as u128 * weight_quote as u128),
    )
}

/// Balancer `_calcOutGivenIn` (floor). Equal weights use exact CPMM.
pub fn swap_out_given_in_weighted(
    reserve_in: u64,
    weight_in: u16,
    reserve_out: u64,
    weight_out: u16,
    amount_in: u64,
    fee_bps: u64,
) -> Option<u64> {
    if reserve_in == 0 || reserve_out == 0 || amount_in == 0 {
        return None;
    }
    if weight_in == 0 || weight_out == 0 {
        return None;
    }
    if weight_in == weight_out {
        return swap_out_given_in(reserve_in, reserve_out, amount_in, fee_bps);
    }
    let dx = amount_after_fee(amount_in, fee_bps)?;
    if dx == 0 {
        return None;
    }
    let new_in = (reserve_in as u128).checked_add(dx as u128)?;
    // power = (R_in / (R_in+dx))^(w_in/w_out)  in 1e18
    let power = pow_ratio_down(reserve_in as u128, new_in, weight_in as u64, weight_out as u64)?;
    if power >= ONE {
        return None;
    }
    let one_minus = ONE - power;
    let out = (reserve_out as u128).checked_mul(one_minus)? / ONE;
    if out == 0 || out >= reserve_out as u128 {
        return None;
    }
    Some(out as u64)
}

pub fn apply_swap_weighted(
    reserves: [u64; 3],
    weights: [u16; 3],
    token_in: usize,
    token_out: usize,
    amount_in: u64,
    fee_bps: u64,
) -> Option<([u64; 3], u64)> {
    if token_in > 2 || token_out > 2 || token_in == token_out {
        return None;
    }
    if weights.iter().any(|&w| w == 0) {
        return None;
    }
    if (weights[0] as u32) + (weights[1] as u32) + (weights[2] as u32) != WEIGHT_DENOM as u32 {
        return None;
    }
    let dy = swap_out_given_in_weighted(
        reserves[token_in],
        weights[token_in],
        reserves[token_out],
        weights[token_out],
        amount_in,
        fee_bps,
    )?;
    let mut next = reserves;
    next[token_in] = reserves[token_in].checked_add(amount_in)?;
    next[token_out] = reserves[token_out].checked_sub(dy)?;
    Some((next, dy))
}
