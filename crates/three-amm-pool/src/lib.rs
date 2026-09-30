//! Weighted Pool: amounts only. Does not hold or transfer tokens.
//! Balancer V3 split — Vault owns balances; this type is the strategy.
#![cfg_attr(not(test), no_std)]

use three_amm_math::{
    apply_swap_weighted, fee_bps_for_trade, fee_bps_for_volatility, proportional_add,
    proportional_remove, swap_out_given_in_weighted, validate_weights,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WeightedPool {
    pub weights: [u16; 3],
    /// Base fee in bps. Volatility-aware paths bump from this floor.
    pub fee_bps: u64,
}

impl WeightedPool {
    pub fn new(weights: [u16; 3], fee_bps: u64) -> Option<Self> {
        if !validate_weights(weights) || fee_bps >= 10_000 {
            return None;
        }
        Some(Self { weights, fee_bps })
    }

    /// Effective fee for an explicit volatility input (0 → base fee).
    pub fn effective_fee_bps(&self, volatility_bps: u64) -> Option<u64> {
        fee_bps_for_volatility(self.fee_bps, volatility_bps)
    }

    /// Effective fee measured from trade size vs the input reserve.
    pub fn effective_fee_bps_for_trade(&self, reserve_in: u64, amount_in: u64) -> Option<u64> {
        fee_bps_for_trade(self.fee_bps, reserve_in, amount_in)
    }

    /// Given-in quote at the pool base fee. Returns next reserves and amount out.
    pub fn quote_swap_exact_in(
        &self,
        reserves: [u64; 3],
        token_in: usize,
        token_out: usize,
        amount_in: u64,
    ) -> Option<([u64; 3], u64)> {
        self.quote_swap_exact_in_with_fee(reserves, token_in, token_out, amount_in, self.fee_bps)
    }

    /// Given-in quote with an explicit fee (e.g. volatility-selected).
    pub fn quote_swap_exact_in_with_fee(
        &self,
        reserves: [u64; 3],
        token_in: usize,
        token_out: usize,
        amount_in: u64,
        fee_bps: u64,
    ) -> Option<([u64; 3], u64)> {
        apply_swap_weighted(
            reserves,
            self.weights,
            token_in,
            token_out,
            amount_in,
            fee_bps,
        )
    }

    /// Given-in quote using volatility-aware fee selection.
    pub fn quote_swap_exact_in_vol(
        &self,
        reserves: [u64; 3],
        token_in: usize,
        token_out: usize,
        amount_in: u64,
        volatility_bps: u64,
    ) -> Option<([u64; 3], u64)> {
        let fee = self.effective_fee_bps(volatility_bps)?;
        self.quote_swap_exact_in_with_fee(reserves, token_in, token_out, amount_in, fee)
    }

    pub fn quote_out_given_in(
        &self,
        reserve_in: u64,
        reserve_out: u64,
        token_in: usize,
        token_out: usize,
        amount_in: u64,
    ) -> Option<u64> {
        self.quote_out_given_in_with_fee(
            reserve_in,
            reserve_out,
            token_in,
            token_out,
            amount_in,
            self.fee_bps,
        )
    }

    pub fn quote_out_given_in_with_fee(
        &self,
        reserve_in: u64,
        reserve_out: u64,
        token_in: usize,
        token_out: usize,
        amount_in: u64,
        fee_bps: u64,
    ) -> Option<u64> {
        swap_out_given_in_weighted(
            reserve_in,
            self.weights[token_in],
            reserve_out,
            self.weights[token_out],
            amount_in,
            fee_bps,
        )
    }

    /// Aggregator-facing exact-in quote: same math the router settles.
    pub fn quote_exact_in_for_aggregator(
        &self,
        reserves: [u64; 3],
        token_in: usize,
        token_out: usize,
        amount_in: u64,
        volatility_bps: u64,
    ) -> Option<u64> {
        let (_next, out) =
            self.quote_swap_exact_in_vol(reserves, token_in, token_out, amount_in, volatility_bps)?;
        Some(out)
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
