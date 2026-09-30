//! Pool layer: weighted quotes only. No SPL Transfer / MintTo / Burn.

use pinocchio::error::ProgramError;
use three_amm_math::{
    fee_bps_for_trade, fee_bps_for_volatility, swap_out_given_in_weighted, DEFAULT_SOFTENING_STEPS,
};
use three_amm_pool::WeightedPool;

use crate::Pool;

pub const NO_LAST_TOKEN: u8 = 0xff;

fn real_reserves(pool: &Pool) -> [u64; 3] {
    [pool.reserve_a, pool.reserve_b, pool.reserve_c]
}

fn virt_reserves(pool: &Pool) -> [u64; 3] {
    [pool.virt_a, pool.virt_b, pool.virt_c]
}

pub fn weighted(pool: &Pool) -> Result<WeightedPool, ProgramError> {
    let wa = pool.weight_a;
    let wb = pool.weight_b;
    let wc = pool.weight_c;
    let fee = pool.fee_bps;
    WeightedPool::new([wa, wb, wc], fee).ok_or(ProgramError::InvalidArgument)
}

/// Select fee: optional explicit vol override, else measure from trade size.
pub fn select_swap_fee(
    pool: &Pool,
    reserve_in: u64,
    amount_in: u64,
    vol_override: Option<u64>,
) -> Result<u64, ProgramError> {
    match vol_override {
        Some(vol) => fee_bps_for_volatility(pool.fee_bps, vol).ok_or(ProgramError::InvalidArgument),
        None => fee_bps_for_trade(pool.fee_bps, reserve_in, amount_in)
            .ok_or(ProgramError::InvalidArgument),
    }
}

/// True when this swap is the reverse of the last softened trade.
pub fn is_reverse_of_last(pool: &Pool, token_in: usize, token_out: usize) -> bool {
    pool.softening_steps_remaining > 0
        && pool.last_token_in != NO_LAST_TOKEN
        && pool.last_token_out != NO_LAST_TOKEN
        && token_in == pool.last_token_out as usize
        && token_out == pool.last_token_in as usize
}

fn quote_reserves(pool: &Pool, token_in: usize, token_out: usize) -> [u64; 3] {
    if is_reverse_of_last(pool, token_in, token_out) {
        virt_reserves(pool)
    } else {
        real_reserves(pool)
    }
}

/// Exact-in quote + next **real** reserves. Uses virt only for reverse softening.
pub fn quote_swap_exact_in(
    pool: &Pool,
    token_in: usize,
    token_out: usize,
    amount_in: u64,
    vol_override: Option<u64>,
) -> Result<([u64; 3], u64, u64), ProgramError> {
    if token_in > 2 || token_out > 2 || token_in == token_out {
        return Err(ProgramError::InvalidArgument);
    }
    let real = real_reserves(pool);
    let fee = select_swap_fee(pool, real[token_in], amount_in, vol_override)?;
    let q = quote_reserves(pool, token_in, token_out);
    let weights = [pool.weight_a, pool.weight_b, pool.weight_c];
    let out = swap_out_given_in_weighted(
        q[token_in],
        weights[token_in],
        q[token_out],
        weights[token_out],
        amount_in,
        fee,
    )
    .ok_or(ProgramError::InvalidArgument)?;
    if out >= real[token_out] {
        return Err(ProgramError::InvalidArgument);
    }
    let mut next = real;
    next[token_in] = real[token_in]
        .checked_add(amount_in)
        .ok_or(ProgramError::InvalidArgument)?;
    next[token_out] = real[token_out]
        .checked_sub(out)
        .ok_or(ProgramError::InvalidArgument)?;
    Ok((next, out, fee))
}

/// Seed Mooniswap-style virtual balances after a real swap.
pub fn seed_softening_after_swap(pool: &mut Pool, pre: [u64; 3], post: [u64; 3], token_in: usize, token_out: usize) {
    let steps = if pool.softening_steps_max == 0 {
        DEFAULT_SOFTENING_STEPS
    } else {
        pool.softening_steps_max
    };
    pool.virt_a = pre[0];
    pool.virt_b = pre[1];
    pool.virt_c = pre[2];
    pool.softening_steps_remaining = steps;
    pool.softening_steps_max = steps;
    pool.last_token_in = token_in as u8;
    pool.last_token_out = token_out as u8;
    // Real already written by caller to post.
    let _ = post;
}

/// One discrete convergence step of virtual → real.
pub fn step_softening(pool: &mut Pool) {
    if pool.softening_steps_remaining == 0 {
        pool.virt_a = pool.reserve_a;
        pool.virt_b = pool.reserve_b;
        pool.virt_c = pool.reserve_c;
        return;
    }
    let rem = pool.softening_steps_remaining as i128;
    let real = [pool.reserve_a, pool.reserve_b, pool.reserve_c];
    let mut virt = [pool.virt_a, pool.virt_b, pool.virt_c];
    for i in 0..3 {
        let gap = real[i] as i128 - virt[i] as i128;
        virt[i] = (virt[i] as i128 + gap / rem) as u64;
    }
    pool.virt_a = virt[0];
    pool.virt_b = virt[1];
    pool.virt_c = virt[2];
    pool.softening_steps_remaining -= 1;
    if pool.softening_steps_remaining == 0 {
        pool.virt_a = pool.reserve_a;
        pool.virt_b = pool.reserve_b;
        pool.virt_c = pool.reserve_c;
    }
}

pub fn quote_join_proportional(
    pool: &Pool,
    amounts: [u64; 3],
) -> Result<([u64; 3], u64), ProgramError> {
    let wp = weighted(pool)?;
    let supply = pool.lp_supply;
    wp.quote_join_proportional(real_reserves(pool), amounts, supply)
        .ok_or(ProgramError::InvalidArgument)
}

pub fn quote_exit_proportional(pool: &Pool, lp_burn: u64) -> Result<[u64; 3], ProgramError> {
    let wp = weighted(pool)?;
    let supply = pool.lp_supply;
    wp.quote_exit_proportional(real_reserves(pool), lp_burn, supply)
        .ok_or(ProgramError::InvalidArgument)
}
