//! Geometric-mean / volatility-aware fee selection.
//!
//! Base fee remains well-defined at zero volatility. Higher measured (or
//! explicit) volatility weakly increases the fee charged on the input.

use crate::FEE_DENOM;

/// Maximum fee bump from volatility (bps), keeping headroom under [`FEE_DENOM`].
pub const MAX_VOL_FEE_BUMP_BPS: u64 = 500;

/// Scale: each `VOL_FEE_SCALE` bps of volatility adds 1 bps of fee (floored).
pub const VOL_FEE_SCALE: u64 = 10;

/// Select the fee charged on a swap given a pool base fee and volatility.
///
/// - `volatility_bps == 0` → returns `base_fee_bps` unchanged.
/// - Higher `volatility_bps` → weakly higher fee (never lower than base).
/// - Result is always `< FEE_DENOM` when `base_fee_bps < FEE_DENOM`.
pub fn fee_bps_for_volatility(base_fee_bps: u64, volatility_bps: u64) -> Option<u64> {
    if base_fee_bps >= FEE_DENOM {
        return None;
    }
    let bump = (volatility_bps / VOL_FEE_SCALE).min(MAX_VOL_FEE_BUMP_BPS);
    let fee = base_fee_bps.saturating_add(bump);
    if fee >= FEE_DENOM {
        Some(FEE_DENOM - 1)
    } else {
        Some(fee)
    }
}

/// Proxy “measured” volatility from trade size vs the input reserve, in bps.
/// `floor(amount_in * 10_000 / reserve_in)`. Zero when either side is zero.
pub fn measured_volatility_bps(reserve_in: u64, amount_in: u64) -> u64 {
    if reserve_in == 0 || amount_in == 0 {
        return 0;
    }
    let v = (amount_in as u128)
        .saturating_mul(FEE_DENOM as u128)
        / reserve_in as u128;
    if v > u64::MAX as u128 {
        u64::MAX
    } else {
        v as u64
    }
}

/// Convenience: base fee + measured vol from the trade itself.
pub fn fee_bps_for_trade(base_fee_bps: u64, reserve_in: u64, amount_in: u64) -> Option<u64> {
    fee_bps_for_volatility(base_fee_bps, measured_volatility_bps(reserve_in, amount_in))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_vol_returns_base() {
        assert_eq!(fee_bps_for_volatility(30, 0), Some(30));
        assert_eq!(fee_bps_for_volatility(0, 0), Some(0));
    }

    #[test]
    fn higher_vol_weakly_higher_fee() {
        let calm = fee_bps_for_volatility(30, 0).unwrap();
        let mid = fee_bps_for_volatility(30, 100).unwrap();
        let high = fee_bps_for_volatility(30, 1_000).unwrap();
        assert!(mid >= calm);
        assert!(high >= mid);
        assert!(high > calm);
    }

    #[test]
    fn rejects_invalid_base() {
        assert!(fee_bps_for_volatility(FEE_DENOM, 0).is_none());
    }

    #[test]
    fn measured_vol_scales_with_size() {
        let small = measured_volatility_bps(1_000_000, 1_000);
        let large = measured_volatility_bps(1_000_000, 100_000);
        assert_eq!(small, 10);
        assert_eq!(large, 1_000);
        assert!(large > small);
    }
}
