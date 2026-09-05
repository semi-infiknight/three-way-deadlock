//! Balancer V3-style Vault: token custody + per-pool reserve index.
//!
//! The Vault holds tokens. Pools never own or transfer user/pool tokens.
//! Solana has no EIP-1153 transient storage; `unlock` / `settle` / `lock`
//! in one call stack is the honest analogue of a V3 unlock session.

use std::collections::BTreeMap;

pub type PoolId = [u8; 32];
pub type UserId = [u8; 32];
pub type MintId = [u8; 32];
pub type AccountId = [u8; 32];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VaultError {
    Locked,
    AlreadyUnlocked,
    PoolExists,
    UnknownPool,
    DuplicateMint,
    AccountBound,
    InsufficientUser,
    InsufficientPool,
    Overflow,
}

#[derive(Clone, Debug)]
pub struct PoolRecord {
    pub mints: [MintId; 3],
    pub vault_accounts: [AccountId; 3],
    pub lp_mint: AccountId,
    pub reserves: [u64; 3],
    pub lp_supply: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Vault {
    unlocked: bool,
    pools: BTreeMap<PoolId, PoolRecord>,
    /// Tokens physically in the Vault, keyed by mint (sum of per-pool indexes).
    mint_totals: BTreeMap<MintId, u64>,
    users: BTreeMap<(UserId, MintId), u64>,
}

impl Vault {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_unlocked(&self) -> bool {
        self.unlocked
    }

    pub fn unlock(&mut self) -> Result<(), VaultError> {
        if self.unlocked {
            return Err(VaultError::AlreadyUnlocked);
        }
        self.unlocked = true;
        Ok(())
    }

    pub fn lock(&mut self) {
        self.unlocked = false;
    }

    pub fn register_pool(
        &mut self,
        id: PoolId,
        mints: [MintId; 3],
        vault_accounts: [AccountId; 3],
        lp_mint: AccountId,
    ) -> Result<(), VaultError> {
        if self.pools.contains_key(&id) {
            return Err(VaultError::PoolExists);
        }
        if mints[0] == mints[1] || mints[0] == mints[2] || mints[1] == mints[2] {
            return Err(VaultError::DuplicateMint);
        }
        for acc in vault_accounts {
            for p in self.pools.values() {
                if p.vault_accounts.contains(&acc) {
                    return Err(VaultError::AccountBound);
                }
            }
        }
        for p in self.pools.values() {
            if p.lp_mint == lp_mint {
                return Err(VaultError::AccountBound);
            }
        }
        self.pools.insert(
            id,
            PoolRecord {
                mints,
                vault_accounts,
                lp_mint,
                reserves: [0, 0, 0],
                lp_supply: 0,
            },
        );
        Ok(())
    }

    pub fn pool(&self, id: PoolId) -> Result<&PoolRecord, VaultError> {
        self.pools.get(&id).ok_or(VaultError::UnknownPool)
    }

    pub fn reserves(&self, id: PoolId) -> Result<[u64; 3], VaultError> {
        Ok(self.pool(id)?.reserves)
    }

    pub fn lp_supply(&self, id: PoolId) -> Result<u64, VaultError> {
        Ok(self.pool(id)?.lp_supply)
    }

    /// Bind: presented token accounts must be exactly the Vault-registered trio.
    pub fn vaults_match_pool(
        &self,
        id: PoolId,
        vault_a: &AccountId,
        vault_b: &AccountId,
        vault_c: &AccountId,
    ) -> bool {
        match self.pools.get(&id) {
            Some(p) => {
                &p.vault_accounts[0] == vault_a
                    && &p.vault_accounts[1] == vault_b
                    && &p.vault_accounts[2] == vault_c
            }
            None => false,
        }
    }

    /// Bind: presented LP mint must be the pool's registered LP mint.
    pub fn lp_matches_pool(&self, id: PoolId, lp_mint: &AccountId) -> bool {
        match self.pools.get(&id) {
            Some(p) => &p.lp_mint == lp_mint,
            None => false,
        }
    }

    pub fn credit_user(&mut self, user: UserId, mint: MintId, amount: u64) -> Result<(), VaultError> {
        let e = self.users.entry((user, mint)).or_insert(0);
        *e = e.checked_add(amount).ok_or(VaultError::Overflow)?;
        Ok(())
    }

    pub fn user_balance(&self, user: UserId, mint: MintId) -> u64 {
        self.users.get(&(user, mint)).copied().unwrap_or(0)
    }

    pub fn mint_total(&self, mint: MintId) -> u64 {
        self.mint_totals.get(&mint).copied().unwrap_or(0)
    }

    /// Pull tokens from the user into the Vault. Requires an open unlock session.
    pub fn take(&mut self, user: UserId, mint: MintId, amount: u64) -> Result<(), VaultError> {
        if !self.unlocked {
            return Err(VaultError::Locked);
        }
        let ub = self.users.get_mut(&(user, mint)).ok_or(VaultError::InsufficientUser)?;
        if *ub < amount {
            return Err(VaultError::InsufficientUser);
        }
        *ub -= amount;
        let tot = self.mint_totals.entry(mint).or_insert(0);
        *tot = tot.checked_add(amount).ok_or(VaultError::Overflow)?;
        Ok(())
    }

    /// Send tokens from the Vault to the user. Requires an open unlock session.
    pub fn send(&mut self, user: UserId, mint: MintId, amount: u64) -> Result<(), VaultError> {
        if !self.unlocked {
            return Err(VaultError::Locked);
        }
        let tot = self.mint_totals.entry(mint).or_insert(0);
        if *tot < amount {
            return Err(VaultError::InsufficientPool);
        }
        *tot -= amount;
        let ub = self.users.entry((user, mint)).or_insert(0);
        *ub = ub.checked_add(amount).ok_or(VaultError::Overflow)?;
        Ok(())
    }

    /// Credit BPT to the user. Not a pool reserve — does **not** touch `mint_totals`.
    pub fn mint_bpt(&mut self, id: PoolId, user: UserId, amount: u64) -> Result<(), VaultError> {
        if !self.unlocked {
            return Err(VaultError::Locked);
        }
        let lp = self.pool(id)?.lp_mint;
        let e = self.users.entry((user, lp)).or_insert(0);
        *e = e.checked_add(amount).ok_or(VaultError::Overflow)?;
        Ok(())
    }

    /// Burn BPT from the user. Fails if they hold none / too little. Not `take`/`send`.
    pub fn burn_bpt(&mut self, id: PoolId, user: UserId, amount: u64) -> Result<(), VaultError> {
        if !self.unlocked {
            return Err(VaultError::Locked);
        }
        let lp = self.pool(id)?.lp_mint;
        let e = self
            .users
            .get_mut(&(user, lp))
            .ok_or(VaultError::InsufficientUser)?;
        if *e < amount {
            return Err(VaultError::InsufficientUser);
        }
        *e -= amount;
        Ok(())
    }

    /// Write one pool's attributed reserves + LP supply. Other pools are untouched.
    pub fn settle(
        &mut self,
        id: PoolId,
        new_reserves: [u64; 3],
        new_lp: u64,
    ) -> Result<(), VaultError> {
        if !self.unlocked {
            return Err(VaultError::Locked);
        }
        let rec = self.pools.get_mut(&id).ok_or(VaultError::UnknownPool)?;
        rec.reserves = new_reserves;
        rec.lp_supply = new_lp;
        Ok(())
    }
}
