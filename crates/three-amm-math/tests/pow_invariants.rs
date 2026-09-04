//! Direct tests of the 1e18 ln/exp/pow used by weighted out-given-in.

use three_amm_math::pow::{exp_1e18, ln_raw, ln_ratio, pow_ratio_down, ONE};

#[test]
fn ln_one_is_near_zero() {
    let v = ln_raw(1).unwrap();
    assert!(v.abs() < 2_000, "ln(1)={v}");
}

#[test]
fn ln_e_ish() {
    // e ≈ 2.718… ; ln(3) ≈ 1.0986e18
    let ln3 = ln_raw(3).unwrap();
    assert!((ln3 - 1_098_612_288_668_109_691).abs() < 1_000_000_000_000);
}

#[test]
fn ln_rejects_zero() {
    assert!(ln_raw(0).is_none());
}

#[test]
fn ln_ratio_one_is_zero() {
    assert_eq!(ln_ratio(100, 100).unwrap(), 0);
    assert!(ln_ratio(0, 1).is_none());
    assert!(ln_ratio(2, 1).is_none());
}

#[test]
fn exp_zero_is_one() {
    assert_eq!(exp_1e18(0).unwrap(), ONE);
}

#[test]
fn exp_ln_roundtrip_small() {
    for x in [1u128, 2, 3, 4, 8, 10, 16, 100, 1_000, 1_000_000] {
        let ln = ln_raw(x).unwrap();
        let back = exp_1e18(ln).unwrap();
        let err = (back as i128 - (x * ONE) as i128).unsigned_abs();
        // series + floor: allow 50 bps relative
        let tol = (x * ONE) / 200 + 1_000_000_000_000;
        assert!(err < tol, "x={x} back={back} want={} err={err}", x * ONE);
    }
}

#[test]
fn pow_ratio_down_one_and_zero_exp() {
    assert_eq!(pow_ratio_down(5, 5, 3, 7).unwrap(), ONE);
    assert_eq!(pow_ratio_down(3, 9, 0, 5).unwrap(), ONE);
    assert!(pow_ratio_down(3, 9, 1, 0).is_none());
    assert!(pow_ratio_down(0, 9, 1, 2).is_none());
}

#[test]
fn pow_ratio_down_sqrt_quarter() {
    // (1/4)^(1/2) = 1/2
    let p = pow_ratio_down(1, 4, 1, 2).unwrap();
    let half = ONE / 2;
    let err = p.abs_diff(half);
    assert!(err < ONE / 1_000, "sqrt(1/4)={p} want {half} err={err}");
}

#[test]
fn pow_ratio_down_close_to_f64() {
    let numer = 5_000_000u128;
    let denom = 5_025_000u128;
    let p = pow_ratio_down(numer, denom, 5000, 3000).unwrap();
    let expect = (numer as f64 / denom as f64).powf(5000.0 / 3000.0) * (ONE as f64);
    let rel = (p as f64 - expect).abs() / expect;
    // f64 is not an oracle (it can sit either side of the 1e18 integer).
    assert!(rel < 1e-10, "p={p} expect={expect} rel={rel}");
}
