//! Router instruction handlers. Same tags the client sends.
//! Quote + per-pool index + BPT mint/burn. No SPL Transfer in this module.

use pinocchio::error::ProgramError;

use crate::pool;
use crate::{lp_matches_pool, u64_at, vaults_match_pool, Pool, IX_ADD, IX_REMOVE, IX_SWAP};

#[derive(Clone, Copy, Debug)]
pub struct VaultSession {
    pub unlocked: u8,
}

impl VaultSession {
    pub fn new() -> Self {
        Self { unlocked: 0 }
    }

    pub fn unlock(&mut self) -> Result<(), ProgramError> {
        if self.unlocked != 0 {
            return Err(ProgramError::InvalidAccountData);
        }
        self.unlocked = 1;
        Ok(())
    }

    pub fn lock(&mut self) {
        self.unlocked = 0;
    }
}

fn require_unlocked(session: &VaultSession) -> Result<(), ProgramError> {
    if session.unlocked == 0 {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(())
}

/// Swap exact-in. Full payload: tag `3`, tin, tout, amount, min_out [, volatility_bps].
///
/// Fee: if `volatility_bps` present (27-byte ix), `fee_bps_for_volatility(base, vol)`;
/// else measure from trade size via `fee_bps_for_trade`. Reverse of the last swap
/// quotes against virtual balances; custody updates **real** reserves only.
pub fn handle_swap(
    data: &[u8],
    pool: &mut Pool,
    session: &mut VaultSession,
    presented_vaults: [&[u8; 32]; 3],
) -> Result<u64, ProgramError> {
    if data.first().copied() != Some(IX_SWAP) {
        return Err(ProgramError::InvalidInstructionData);
    }
    let rest = &data[1..];
    if rest.len() < 18 {
        return Err(ProgramError::InvalidInstructionData);
    }
    if !vaults_match_pool(pool, presented_vaults[0], presented_vaults[1], presented_vaults[2]) {
        return Err(ProgramError::InvalidAccountData);
    }
    if pool.paused != 0 {
        return Err(ProgramError::Custom(1));
    }
    let token_in = rest[0] as usize;
    let token_out = rest[1] as usize;
    let amount_in = u64_at(rest, 2)?;
    let min_out = u64_at(rest, 10)?;
    let vol_override = if rest.len() >= 26 {
        Some(u64_at(rest, 18)?)
    } else {
        None
    };
    session.unlock()?;
    // Step prior softening window so reverse quotes converge over activity.
    if pool.softening_steps_remaining > 0 && !pool::is_reverse_of_last(pool, token_in, token_out) {
        pool::step_softening(pool);
    }
    let pre = [pool.reserve_a, pool.reserve_b, pool.reserve_c];
    let (next, out, _fee) =
        pool::quote_swap_exact_in(pool, token_in, token_out, amount_in, vol_override)?;
    if out < min_out {
        session.lock();
        return Err(ProgramError::Custom(2));
    }
    require_unlocked(session)?;
    pool.reserve_a = next[0];
    pool.reserve_b = next[1];
    pool.reserve_c = next[2];
    pool::seed_softening_after_swap(pool, pre, next, token_in, token_out);
    session.lock();
    Ok(out)
}

/// Proportional join. Full payload: tag `1` + 4×u64. Credits `user_bpt` (BPT mint).
pub fn handle_join(
    data: &[u8],
    pool: &mut Pool,
    session: &mut VaultSession,
    presented_vaults: [&[u8; 32]; 3],
    presented_lp: &[u8; 32],
    user_bpt: &mut u64,
) -> Result<([u64; 3], u64), ProgramError> {
    if data.first().copied() != Some(IX_ADD) {
        return Err(ProgramError::InvalidInstructionData);
    }
    let rest = &data[1..];
    if !vaults_match_pool(pool, presented_vaults[0], presented_vaults[1], presented_vaults[2]) {
        return Err(ProgramError::InvalidAccountData);
    }
    if !lp_matches_pool(pool, presented_lp) {
        return Err(ProgramError::InvalidAccountData);
    }
    if pool.paused != 0 {
        return Err(ProgramError::Custom(1));
    }
    let amounts = [u64_at(rest, 0)?, u64_at(rest, 8)?, u64_at(rest, 16)?];
    let min_lp = u64_at(rest, 24)?;
    session.unlock()?;
    let (used, lp) = pool::quote_join_proportional(pool, amounts)?;
    if lp < min_lp {
        session.lock();
        return Err(ProgramError::Custom(2));
    }
    *user_bpt = user_bpt
        .checked_add(lp)
        .ok_or(ProgramError::InvalidArgument)?;
    pool.reserve_a = pool
        .reserve_a
        .checked_add(used[0])
        .ok_or(ProgramError::InvalidArgument)?;
    pool.reserve_b = pool
        .reserve_b
        .checked_add(used[1])
        .ok_or(ProgramError::InvalidArgument)?;
    pool.reserve_c = pool
        .reserve_c
        .checked_add(used[2])
        .ok_or(ProgramError::InvalidArgument)?;
    pool.lp_supply = pool
        .lp_supply
        .checked_add(lp)
        .ok_or(ProgramError::InvalidArgument)?;
    session.lock();
    Ok((used, lp))
}

/// Proportional exit. Full payload: tag `2` + lp_burn + 3×min. Burns `user_bpt` first.
pub fn handle_exit(
    data: &[u8],
    pool: &mut Pool,
    session: &mut VaultSession,
    presented_vaults: [&[u8; 32]; 3],
    presented_lp: &[u8; 32],
    user_bpt: &mut u64,
) -> Result<[u64; 3], ProgramError> {
    if data.first().copied() != Some(IX_REMOVE) {
        return Err(ProgramError::InvalidInstructionData);
    }
    let rest = &data[1..];
    if !vaults_match_pool(pool, presented_vaults[0], presented_vaults[1], presented_vaults[2]) {
        return Err(ProgramError::InvalidAccountData);
    }
    if !lp_matches_pool(pool, presented_lp) {
        return Err(ProgramError::InvalidAccountData);
    }
    if pool.paused != 0 {
        return Err(ProgramError::Custom(1));
    }
    let lp_burn = u64_at(rest, 0)?;
    let min_out = [u64_at(rest, 8)?, u64_at(rest, 16)?, u64_at(rest, 24)?];
    session.unlock()?;
    let out = pool::quote_exit_proportional(pool, lp_burn)?;
    if out[0] < min_out[0] || out[1] < min_out[1] || out[2] < min_out[2] {
        session.lock();
        return Err(ProgramError::Custom(2));
    }
    if *user_bpt < lp_burn {
        session.lock();
        return Err(ProgramError::InsufficientFunds);
    }
    *user_bpt -= lp_burn;
    pool.reserve_a = pool
        .reserve_a
        .checked_sub(out[0])
        .ok_or(ProgramError::InvalidArgument)?;
    pool.reserve_b = pool
        .reserve_b
        .checked_sub(out[1])
        .ok_or(ProgramError::InvalidArgument)?;
    pool.reserve_c = pool
        .reserve_c
        .checked_sub(out[2])
        .ok_or(ProgramError::InvalidArgument)?;
    pool.lp_supply = pool
        .lp_supply
        .checked_sub(lp_burn)
        .ok_or(ProgramError::InvalidArgument)?;
    session.lock();
    Ok(out)
}

pub fn encode_swap(token_in: u8, token_out: u8, amount_in: u64, min_out: u64) -> [u8; 19] {
    let mut d = [0u8; 19];
    d[0] = IX_SWAP;
    d[1] = token_in;
    d[2] = token_out;
    d[3..11].copy_from_slice(&amount_in.to_le_bytes());
    d[11..19].copy_from_slice(&min_out.to_le_bytes());
    d
}

/// Swap with explicit volatility override (measured path skipped).
pub fn encode_swap_vol(
    token_in: u8,
    token_out: u8,
    amount_in: u64,
    min_out: u64,
    volatility_bps: u64,
) -> [u8; 27] {
    let mut d = [0u8; 27];
    d[..19].copy_from_slice(&encode_swap(token_in, token_out, amount_in, min_out));
    d[19..27].copy_from_slice(&volatility_bps.to_le_bytes());
    d
}

pub fn encode_join(amounts: [u64; 3], min_lp: u64) -> [u8; 33] {
    let mut d = [0u8; 33];
    d[0] = IX_ADD;
    d[1..9].copy_from_slice(&amounts[0].to_le_bytes());
    d[9..17].copy_from_slice(&amounts[1].to_le_bytes());
    d[17..25].copy_from_slice(&amounts[2].to_le_bytes());
    d[25..33].copy_from_slice(&min_lp.to_le_bytes());
    d
}

pub fn encode_exit(lp_burn: u64, min_out: [u64; 3]) -> [u8; 33] {
    let mut d = [0u8; 33];
    d[0] = IX_REMOVE;
    d[1..9].copy_from_slice(&lp_burn.to_le_bytes());
    d[9..17].copy_from_slice(&min_out[0].to_le_bytes());
    d[17..25].copy_from_slice(&min_out[1].to_le_bytes());
    d[25..33].copy_from_slice(&min_out[2].to_le_bytes());
    d
}
