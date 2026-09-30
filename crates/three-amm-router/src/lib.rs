//! Router: the only user-facing entry. Vault unlock → Pool quote → Vault settle.
//! Analogous to Balancer V3 Router (not BatchRouter / BufferRouter).

use std::collections::BTreeMap;

use three_amm_math::{
    converge_virtual_balances, quote_out_given_in_real, quote_out_given_in_virtual,
    seed_virtual_after_swap, step_virtual_balances, VirtualBalanceState, DEFAULT_SOFTENING_STEPS,
};
use three_amm_pool::WeightedPool;
use three_amm_vault::{AccountId, MintId, PoolId, UserId, Vault, VaultError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouterError {
    Vault(VaultError),
    BindVault,
    BindLp,
    BadPool,
    Quote,
    Slippage,
}

impl From<VaultError> for RouterError {
    fn from(e: VaultError) -> Self {
        RouterError::Vault(e)
    }
}

/// Per-pool Mooniswap-style virtual balances. Custody stays on Vault `real` reserves.
#[derive(Clone, Debug, Default)]
pub struct SofteningBook {
    states: BTreeMap<PoolId, VirtualBalanceState>,
}

impl SofteningBook {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, pool_id: PoolId) -> Option<&VirtualBalanceState> {
        self.states.get(&pool_id)
    }

    pub fn step(&mut self, pool_id: PoolId) {
        if let Some(s) = self.states.get_mut(&pool_id) {
            step_virtual_balances(s);
        }
    }

    pub fn converge(&mut self, pool_id: PoolId) {
        if let Some(s) = self.states.get_mut(&pool_id) {
            let n = s.steps_remaining.saturating_add(1);
            converge_virtual_balances(s, n);
        }
    }
}

pub struct Router;

impl Router {
    /// Register a weighted pool in the Vault. Pool object is math-only (weights/fee).
    pub fn initialize_weighted_pool(
        vault: &mut Vault,
        pool_id: PoolId,
        mints: [MintId; 3],
        vault_accounts: [AccountId; 3],
        lp_mint: AccountId,
        weights: [u16; 3],
        fee_bps: u64,
    ) -> Result<WeightedPool, RouterError> {
        let pool = WeightedPool::new(weights, fee_bps).ok_or(RouterError::BadPool)?;
        vault.register_pool(pool_id, mints, vault_accounts, lp_mint)?;
        Ok(pool)
    }

    /// Aggregator-facing exact-in quote (no custody). Uses volatility-aware fee.
    /// Returns the same out amount the softened/vol swap path would settle at
    /// `volatility_bps` against current vault reserves (real balances).
    pub fn quote_exact_in(
        vault: &Vault,
        pool_id: PoolId,
        pool: &WeightedPool,
        token_in: usize,
        token_out: usize,
        amount_in: u64,
        volatility_bps: u64,
    ) -> Result<u64, RouterError> {
        let reserves = vault.reserves(pool_id)?;
        pool.quote_exact_in_for_aggregator(reserves, token_in, token_out, amount_in, volatility_bps)
            .ok_or(RouterError::Quote)
    }

    pub fn swap_exact_in(
        vault: &mut Vault,
        pool_id: PoolId,
        pool: &WeightedPool,
        user: UserId,
        presented_vaults: [AccountId; 3],
        token_in: usize,
        token_out: usize,
        amount_in: u64,
        min_out: u64,
    ) -> Result<u64, RouterError> {
        Self::swap_exact_in_vol(
            vault,
            pool_id,
            pool,
            user,
            presented_vaults,
            token_in,
            token_out,
            amount_in,
            min_out,
            0,
            None,
            DEFAULT_SOFTENING_STEPS,
        )
    }

    /// Exact-in swap with volatility-aware fee and optional virtual-balance seeding.
    /// Vault bind → unlock → pool quote → take/send → settle(**real**) → lock.
    pub fn swap_exact_in_vol(
        vault: &mut Vault,
        pool_id: PoolId,
        pool: &WeightedPool,
        user: UserId,
        presented_vaults: [AccountId; 3],
        token_in: usize,
        token_out: usize,
        amount_in: u64,
        min_out: u64,
        volatility_bps: u64,
        softening: Option<&mut SofteningBook>,
        softening_steps: u32,
    ) -> Result<u64, RouterError> {
        if !vault.vaults_match_pool(
            pool_id,
            &presented_vaults[0],
            &presented_vaults[1],
            &presented_vaults[2],
        ) {
            return Err(RouterError::BindVault);
        }
        let rec = vault.pool(pool_id)?;
        let mints = rec.mints;
        let reserves = rec.reserves;
        vault.unlock()?;
        let (next, out) = pool
            .quote_swap_exact_in_vol(reserves, token_in, token_out, amount_in, volatility_bps)
            .ok_or(RouterError::Quote)?;
        if out < min_out {
            vault.lock();
            return Err(RouterError::Slippage);
        }
        let supply = vault.lp_supply(pool_id)?;
        vault.take(user, mints[token_in], amount_in)?;
        vault.send(user, mints[token_out], out)?;
        vault.settle(pool_id, next, supply)?;
        vault.lock();
        if let Some(book) = softening {
            book.states
                .insert(pool_id, seed_virtual_after_swap(reserves, next, softening_steps));
        }
        Ok(out)
    }

    /// Reverse-direction quote using virtual balances when present (arb softening).
    /// Falls back to real vault reserves when the book has no state for `pool_id`.
    pub fn quote_reverse(
        vault: &Vault,
        pool_id: PoolId,
        pool: &WeightedPool,
        book: &SofteningBook,
        token_in: usize,
        token_out: usize,
        amount_in: u64,
        fee_bps: u64,
    ) -> Result<u64, RouterError> {
        if let Some(state) = book.get(pool_id) {
            quote_out_given_in_virtual(state, pool.weights, token_in, token_out, amount_in, fee_bps)
                .ok_or(RouterError::Quote)
        } else {
            let r = vault.reserves(pool_id)?;
            let state = VirtualBalanceState::from_real(r);
            quote_out_given_in_real(&state, pool.weights, token_in, token_out, amount_in, fee_bps)
                .ok_or(RouterError::Quote)
        }
    }

    /// Real-reserve reverse quote (no virtual overlay) — for arb comparison tests.
    pub fn quote_reverse_real(
        vault: &Vault,
        pool_id: PoolId,
        pool: &WeightedPool,
        token_in: usize,
        token_out: usize,
        amount_in: u64,
        fee_bps: u64,
    ) -> Result<u64, RouterError> {
        let r = vault.reserves(pool_id)?;
        let state = VirtualBalanceState::from_real(r);
        quote_out_given_in_real(&state, pool.weights, token_in, token_out, amount_in, fee_bps)
            .ok_or(RouterError::Quote)
    }

    pub fn add_liquidity_proportional(
        vault: &mut Vault,
        pool_id: PoolId,
        pool: &WeightedPool,
        user: UserId,
        presented_vaults: [AccountId; 3],
        presented_lp: AccountId,
        amounts: [u64; 3],
        min_lp: u64,
    ) -> Result<u64, RouterError> {
        if !vault.vaults_match_pool(
            pool_id,
            &presented_vaults[0],
            &presented_vaults[1],
            &presented_vaults[2],
        ) {
            return Err(RouterError::BindVault);
        }
        if !vault.lp_matches_pool(pool_id, &presented_lp) {
            return Err(RouterError::BindLp);
        }
        let rec = vault.pool(pool_id)?;
        let mints = rec.mints;
        let reserves = rec.reserves;
        let supply = rec.lp_supply;
        vault.unlock()?;
        let (used, lp) = pool
            .quote_join_proportional(reserves, amounts, supply)
            .ok_or(RouterError::Quote)?;
        if lp < min_lp {
            vault.lock();
            return Err(RouterError::Slippage);
        }
        let mut next = reserves;
        for i in 0..3 {
            vault.take(user, mints[i], used[i])?;
            next[i] = next[i]
                .checked_add(used[i])
                .ok_or(VaultError::Overflow)?;
        }
        let new_supply = supply.checked_add(lp).ok_or(VaultError::Overflow)?;
        vault.mint_bpt(pool_id, user, lp)?;
        vault.settle(pool_id, next, new_supply)?;
        vault.lock();
        Ok(lp)
    }

    pub fn remove_liquidity_proportional(
        vault: &mut Vault,
        pool_id: PoolId,
        pool: &WeightedPool,
        user: UserId,
        presented_vaults: [AccountId; 3],
        presented_lp: AccountId,
        lp_burn: u64,
        min_out: [u64; 3],
    ) -> Result<[u64; 3], RouterError> {
        if !vault.vaults_match_pool(
            pool_id,
            &presented_vaults[0],
            &presented_vaults[1],
            &presented_vaults[2],
        ) {
            return Err(RouterError::BindVault);
        }
        if !vault.lp_matches_pool(pool_id, &presented_lp) {
            return Err(RouterError::BindLp);
        }
        let rec = vault.pool(pool_id)?;
        let mints = rec.mints;
        let reserves = rec.reserves;
        let supply = rec.lp_supply;
        vault.unlock()?;
        let out = pool
            .quote_exit_proportional(reserves, lp_burn, supply)
            .ok_or(RouterError::Quote)?;
        if out[0] < min_out[0] || out[1] < min_out[1] || out[2] < min_out[2] {
            vault.lock();
            return Err(RouterError::Slippage);
        }
        // Burn the caller's BPT before sending reserves so a 0-BPT user cannot drain.
        if let Err(e) = vault.burn_bpt(pool_id, user, lp_burn) {
            vault.lock();
            return Err(e.into());
        }
        let mut next = reserves;
        for i in 0..3 {
            next[i] = next[i]
                .checked_sub(out[i])
                .ok_or(VaultError::InsufficientPool)?;
            vault.send(user, mints[i], out[i])?;
        }
        let new_supply = supply
            .checked_sub(lp_burn)
            .ok_or(VaultError::InsufficientPool)?;
        vault.settle(pool_id, next, new_supply)?;
        vault.lock();
        Ok(out)
    }
}
