//! Fixed-point `ln` / `exp` / `pow` (1e18 scale) for Balancer
//! `out = R_out * (1 - (R_in/(R_in+dx))^(w_in/w_out))`.
//! Rounding: `pow_down` floors so swaps favor the pool.

#![allow(clippy::arithmetic_side_effects)]

pub const ONE: u128 = 1_000_000_000_000_000_000;
const LN2: i128 = 693_147_180_559_945_309; // ln(2) * 1e18

/// ln(x) for x in (0, 2^64] as 1e18-scaled signed. `x` is *not* 1e18-scaled
/// (raw positive integer, e.g. a reserve or a ratio numerator).
pub fn ln_raw(x: u128) -> Option<i128> {
    if x == 0 {
        return None;
    }
    // x = 2^k * m, m in [1, 2)
    let k = 127i32 - x.leading_zeros() as i32;
    let m_num = x;
    let m_den = 1u128 << k.max(0) as u32;
    // Work with m in 1e18: m_fp = x * ONE / 2^k
    let m_fp = if k >= 0 {
        (m_num.saturating_mul(ONE)) / m_den
    } else {
        m_num.saturating_mul(ONE) * (1u128 << (-k) as u32)
    };
    // z = (m-1)/(m+1), atanh series: ln(m) = 2(z + z^3/3 + z^5/5 + ...)
    let mp = m_fp as i128;
    let one = ONE as i128;
    let z = ((mp - one) * one) / (mp + one);
    let mut z_pow = z;
    let mut sum = z;
    for n in 1..16u32 {
        z_pow = z_pow.checked_mul(z)? / one;
        z_pow = z_pow.checked_mul(z)? / one;
        let term = z_pow / (2 * n as i128 + 1);
        sum = sum.checked_add(term)?;
    }
    let ln_m = sum.checked_mul(2)?;
    Some(ln_m + (k as i128) * LN2)
}

/// ln(a/b) * 1e18 for a < b (ratio in (0,1)).
pub fn ln_ratio(numer: u128, denom: u128) -> Option<i128> {
    if numer == 0 || denom == 0 || numer > denom {
        return None;
    }
    if numer == denom {
        return Some(0);
    }
    let ln_n = ln_raw(numer)?;
    let ln_d = ln_raw(denom)?;
    Some(ln_n - ln_d)
}

/// exp(x) as 1e18-scaled, x is 1e18-scaled (can be negative). Floor.
pub fn exp_1e18(x: i128) -> Option<u128> {
    // exp(x) = 2^(x/ln2) = 2^n * exp(r) with r in (-ln2/2, ln2/2) roughly
    if x == 0 {
        return Some(ONE);
    }
    // Clamp: exp of very negative → 0; very positive → overflow
    if x < -40 * (ONE as i128) {
        return Some(0);
    }
    if x > 40 * (ONE as i128) {
        return None;
    }
    let one = ONE as i128;
    // n = floor(x / ln2)
    // Truncation toward zero is wrong for negative x (need floor).
    let n = if x >= 0 {
        x / LN2
    } else {
        let q = x / LN2;
        if x % LN2 == 0 {
            q
        } else {
            q - 1
        }
    };
    let r = x - n * LN2;
    // exp(r) series, r typically in (-ln2, ln2)
    let mut term = one;
    let mut sum = one;
    for k in 1..24i128 {
        term = term.checked_mul(r)? / one;
        term /= k;
        sum = sum.checked_add(term)?;
    }
    if sum <= 0 {
        return Some(0);
    }
    let exp_r = sum as u128;
    if n >= 0 {
        exp_r.checked_shl(n as u32)
    } else {
        Some(exp_r >> ((-n) as u32))
    }
}

/// floor( (numer/denom)^(exp_n/exp_d) * ONE ) with numer <= denom.
pub fn pow_ratio_down(numer: u128, denom: u128, exp_n: u64, exp_d: u64) -> Option<u128> {
    if exp_d == 0 || numer == 0 {
        return None;
    }
    if numer == denom || exp_n == 0 {
        return Some(ONE);
    }
    let ln = ln_ratio(numer, denom)?; // negative
    let scaled = ln
        .checked_mul(exp_n as i128)?
        .checked_div(exp_d as i128)?;
    exp_1e18(scaled)
}
