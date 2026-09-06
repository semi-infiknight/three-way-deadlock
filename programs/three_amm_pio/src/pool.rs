//! Pool layer: weighted quotes only. No SPL Transfer / MintTo / Burn.

use pinocchio::error::ProgramError;
use three_amm_pool::WeightedPool;

use crate::Pool;

fn reserves(pool: &Pool) -> [u64; 3] {
    [pool.reserve_a, pool.reserve_b, pool.reserve_c]
}

pub fn weighted(pool: &Pool) -> Result<WeightedPool, ProgramError> {
    let wa = pool.weight_a;
    let wb = pool.weight_b;
    let wc = pool.weight_c;
    let fee = pool.fee_bps;
    WeightedPool::new([wa, wb, wc], fee).ok_or(ProgramError::InvalidArgument)
}

pub fn quote_swap_exact_in(
    pool: &Pool,
    token_in: usize,
    token_out: usize,
    amount_in: u64,
) -> Result<([u64; 3], u64), ProgramError> {
    let wp = weighted(pool)?;
    wp.quote_swap_exact_in(reserves(pool), token_in, token_out, amount_in)
        .ok_or(ProgramError::InvalidArgument)
}

pub fn quote_join_proportional(
    pool: &Pool,
    amounts: [u64; 3],
) -> Result<([u64; 3], u64), ProgramError> {
    let wp = weighted(pool)?;
    let supply = pool.lp_supply;
    wp.quote_join_proportional(reserves(pool), amounts, supply)
        .ok_or(ProgramError::InvalidArgument)
}

pub fn quote_exit_proportional(pool: &Pool, lp_burn: u64) -> Result<[u64; 3], ProgramError> {
    let wp = weighted(pool)?;
    let supply = pool.lp_supply;
    wp.quote_exit_proportional(reserves(pool), lp_burn, supply)
        .ok_or(ProgramError::InvalidArgument)
}
