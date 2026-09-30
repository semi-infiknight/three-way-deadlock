//! Drive shipped fee + virtual-balance APIs (no re-implementation).

use three_amm_math::{
    apply_swap_weighted, converge_virtual_balances, fee_bps_for_volatility,
    quote_out_given_in_real, quote_out_given_in_virtual, seed_virtual_after_swap,
    swap_out_given_in_weighted, DEFAULT_SOFTENING_STEPS,
};

#[test]
fn high_vol_fee_ge_calm_baseline_same_swap_inputs() {
    let base = 30u64;
    let calm_fee = fee_bps_for_volatility(base, 0).unwrap();
    let stress_fee = fee_bps_for_volatility(base, 2_000).unwrap();
    assert!(stress_fee >= calm_fee);
    assert!(stress_fee > calm_fee);

    let ri = 5_000_000u64;
    let ro = 3_000_000u64;
    let wi = 5000u16;
    let wo = 3000u16;
    let amount_in = 10_000u64;

    let calm_out = swap_out_given_in_weighted(ri, wi, ro, wo, amount_in, calm_fee).unwrap();
    let stress_out = swap_out_given_in_weighted(ri, wi, ro, wo, amount_in, stress_fee).unwrap();
    // Higher fee on input → weakly less out for the trader.
    assert!(stress_out <= calm_out);
}

#[test]
fn post_swap_virtual_reverse_weakly_worse_then_converges() {
    let weights = [5000u16, 3000, 2000];
    let pre = [5_000_000u64, 3_000_000, 2_000_000];
    let amount_in = 10_000u64;
    let fee = 30u64;
    let (post, out) = apply_swap_weighted(pre, weights, 0, 1, amount_in, fee).unwrap();
    assert_eq!(post[2], pre[2]);

    let mut state = seed_virtual_after_swap(pre, post, DEFAULT_SOFTENING_STEPS);
    let virt_rev = quote_out_given_in_virtual(&state, weights, 1, 0, out, fee).unwrap();
    let real_rev = quote_out_given_in_real(&state, weights, 1, 0, out, fee).unwrap();
    assert!(virt_rev <= real_rev);
    assert!(virt_rev < real_rev);

    converge_virtual_balances(&mut state, DEFAULT_SOFTENING_STEPS + 1);
    let virt_after = quote_out_given_in_virtual(&state, weights, 1, 0, out, fee).unwrap();
    let real_after = quote_out_given_in_real(&state, weights, 1, 0, out, fee).unwrap();
    assert_eq!(virt_after, real_after);
    assert_eq!(state.real[2], pre[2]);
}
