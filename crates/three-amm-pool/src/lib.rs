//! Weighted Pool: amounts only. Does not hold or transfer tokens.
//! Balancer V3 split — Vault owns balances; this type is the strategy.
#![cfg_attr(not(test), no_std)]

use three_amm_math::{
    apply_swap_weighted, proportional_add, proportional_remove, swap_out_given_in_weighted,
    validate_weights,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WeightedPool {
    pub weights: [u16; 3],
    pub fee_bps: u64,
}

impl WeightedPool {
    pub fn new(weights: [u16; 3], fee_bps: u64) -> Option<Self> {
        if !validate_weights(weights) || fee_bps >= 10_000 {
            return None;
        }
        Some(Self { weights, fee_bps })
    }

    /// Given-in quote. Returns next reserves and amount out. No custody change.
    pub fn quote_swap_exact_in(
        &self,
        reserves: [u64; 3],
        token_in: usize,
        token_out: usize,
        amount_in: u64,
    ) -> Option<([u64; 3], u64)> {
        apply_swap_weighted(
            reserves,
            self.weights,
            token_in,
            token_out,
            amount_in,
            self.fee_bps,
        )
    }

    pub fn quote_out_given_in(
        &self,
        reserve_in: u64,
        reserve_out: u64,
        token_in: usize,
        token_out: usize,
        amount_in: u64,
    ) -> Option<u64> {
        swap_out_given_in_weighted(
            reserve_in,
            self.weights[token_in],
            reserve_out,
            self.weights[token_out],
            amount_in,
            self.fee_bps,
        )
    }

    pub fn quote_join_proportional(
        &self,
        reserves: [u64; 3],
        amounts: [u64; 3],
        supply: u64,
    ) -> Option<([u64; 3], u64)> {
        proportional_add(reserves, amounts, supply)
    }

    pub fn quote_exit_proportional(
        &self,
        reserves: [u64; 3],
        lp_burn: u64,
        supply: u64,
    ) -> Option<[u64; 3]> {
        proportional_remove(reserves, lp_burn, supply)
    }
}
