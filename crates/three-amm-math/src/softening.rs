//! Mooniswap-style virtual-balance softening on the reverse path.
//!
//! After an exact-in swap, virtual reserves start at the **pre-swap** state and
//! converge toward **post-swap** real reserves over discrete steps. Quotes that
//! use virtual balances make an immediate reverse trade weakly worse for an
//! arb than quoting on raw post-swap reserves. Real custody amounts are always
//! the `real` field; the untraded third reserve amount is unchanged on the
//! real path.

use crate::weighted::swap_out_given_in_weighted;

/// Default number of discrete convergence steps after a swap.
pub const DEFAULT_SOFTENING_STEPS: u32 = 4;

/// Virtual overlay over real reserves. Settle / custody always use `real`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VirtualBalanceState {
    pub real: [u64; 3],
    pub virt: [u64; 3],
    pub steps_remaining: u32,
}

impl VirtualBalanceState {
    pub fn from_real(real: [u64; 3]) -> Self {
        Self {
            real,
            virt: real,
            steps_remaining: 0,
        }
    }

    pub fn is_converged(&self) -> bool {
        self.steps_remaining == 0 && self.virt == self.real
    }
}

/// Seed softening after a real swap from `pre` → `post` reserves.
///
/// When `steps == 0`, virtual equals real immediately (no softening).
pub fn seed_virtual_after_swap(pre: [u64; 3], post: [u64; 3], steps: u32) -> VirtualBalanceState {
    if steps == 0 {
        return VirtualBalanceState::from_real(post);
    }
    VirtualBalanceState {
        real: post,
        virt: pre,
        steps_remaining: steps,
    }
}

/// Move virtual balances one discrete step toward real reserves.
/// When steps hit zero, virtual snaps to real.
pub fn step_virtual_balances(state: &mut VirtualBalanceState) {
    if state.steps_remaining == 0 {
        state.virt = state.real;
        return;
    }
    let rem = state.steps_remaining as i128;
    for i in 0..3 {
        let gap = state.real[i] as i128 - state.virt[i] as i128;
        let delta = gap / rem;
        state.virt[i] = (state.virt[i] as i128 + delta) as u64;
    }
    state.steps_remaining -= 1;
    if state.steps_remaining == 0 {
        state.virt = state.real;
    }
}

/// Run `n` convergence steps (or until converged).
pub fn converge_virtual_balances(state: &mut VirtualBalanceState, n: u32) {
    for _ in 0..n {
        if state.steps_remaining == 0 {
            state.virt = state.real;
            break;
        }
        step_virtual_balances(state);
    }
}

/// Exact-in out-amount quoted against **virtual** reserves (softened path).
pub fn quote_out_given_in_virtual(
    state: &VirtualBalanceState,
    weights: [u16; 3],
    token_in: usize,
    token_out: usize,
    amount_in: u64,
    fee_bps: u64,
) -> Option<u64> {
    if token_in > 2 || token_out > 2 || token_in == token_out {
        return None;
    }
    swap_out_given_in_weighted(
        state.virt[token_in],
        weights[token_in],
        state.virt[token_out],
        weights[token_out],
        amount_in,
        fee_bps,
    )
}

/// Exact-in out-amount quoted against **real** reserves (post-swap custody).
pub fn quote_out_given_in_real(
    state: &VirtualBalanceState,
    weights: [u16; 3],
    token_in: usize,
    token_out: usize,
    amount_in: u64,
    fee_bps: u64,
) -> Option<u64> {
    if token_in > 2 || token_out > 2 || token_in == token_out {
        return None;
    }
    swap_out_given_in_weighted(
        state.real[token_in],
        weights[token_in],
        state.real[token_out],
        weights[token_out],
        amount_in,
        fee_bps,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weighted::apply_swap_weighted;

    #[test]
    fn reverse_quote_virtual_weakly_worse_than_real() {
        let weights = [5000u16, 3000, 2000];
        let pre = [5_000_000u64, 3_000_000, 2_000_000];
        let amount_in = 10_000u64;
        let fee = 30u64;
        let (post, out) = apply_swap_weighted(pre, weights, 0, 1, amount_in, fee).unwrap();
        assert_eq!(post[2], pre[2], "third reserve amount unchanged");
        let state = seed_virtual_after_swap(pre, post, DEFAULT_SOFTENING_STEPS);
        // Reverse: 1 → 0 with the amount just received.
        let virt_rev = quote_out_given_in_virtual(&state, weights, 1, 0, out, fee).unwrap();
        let real_rev = quote_out_given_in_real(&state, weights, 1, 0, out, fee).unwrap();
        assert!(
            virt_rev <= real_rev,
            "virtual reverse {virt_rev} must be ≤ real reverse {real_rev}"
        );
        assert!(virt_rev < real_rev, "softening must bite on a non-trivial swap");
    }

    #[test]
    fn full_convergence_matches_real_quote() {
        let weights = [5000u16, 3000, 2000];
        let pre = [5_000_000u64, 3_000_000, 2_000_000];
        let (post, out) = apply_swap_weighted(pre, weights, 0, 1, 10_000, 30).unwrap();
        let mut state = seed_virtual_after_swap(pre, post, DEFAULT_SOFTENING_STEPS);
        converge_virtual_balances(&mut state, DEFAULT_SOFTENING_STEPS + 2);
        assert!(state.is_converged());
        let virt_rev = quote_out_given_in_virtual(&state, weights, 1, 0, out, 30).unwrap();
        let real_rev = quote_out_given_in_real(&state, weights, 1, 0, out, 30).unwrap();
        assert_eq!(virt_rev, real_rev);
        assert_eq!(state.real[2], pre[2]);
    }

    #[test]
    fn zero_steps_is_already_converged() {
        let pre = [1u64, 2, 3];
        let post = [4u64, 5, 6];
        let state = seed_virtual_after_swap(pre, post, 0);
        assert!(state.is_converged());
        assert_eq!(state.virt, post);
    }
}
