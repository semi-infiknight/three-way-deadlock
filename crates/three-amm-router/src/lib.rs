//! Router: the only user-facing entry. Vault unlock → Pool quote → Vault settle.
//! Analogous to Balancer V3 Router (not BatchRouter / BufferRouter).

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
            .quote_swap_exact_in(reserves, token_in, token_out, amount_in)
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
        Ok(out)
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
