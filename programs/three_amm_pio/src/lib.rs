//! Pinocchio Router entry. Vault custodies tokens; Pool only quotes.
//! Instruction tag is 1 byte (not Anchor's 8-byte sha256).
#![no_std]

use pinocchio::{
    cpi::{Seed, Signer},
    default_allocator, error::ProgramError, nostd_panic_handler, program_entrypoint,
    AccountView, Address, ProgramResult,
};
use pinocchio_system::instructions::CreateAccount;
use pinocchio_token::{
    instructions::{Burn, InitializeAccount3, InitializeMint2, MintTo, Transfer},
    state::{Account as SplAccount, Mint},
};
use three_amm_math::{equal_weights, validate_weights};

pub mod handlers;
pub mod pool;

program_entrypoint!(process_instruction);
default_allocator!();
nostd_panic_handler!();

pub const ID: Address = Address::new_from_array([
    110, 128, 34, 37, 232, 37, 210, 135, 97, 69, 199, 223, 164, 235, 209, 99, 17, 191, 122, 197,
    254, 193, 237, 217, 181, 36, 57, 240, 3, 10, 37, 78,
]);

pub const POOL_SEED: &[u8] = b"pool";
pub const VAULT_SEED: &[u8] = b"vault";
pub const VAULT_STATE_SEED: &[u8] = b"vault-state";
pub const LP_SEED: &[u8] = b"lp";
pub const IX_INIT: u8 = 0;
pub const IX_ADD: u8 = 1;
pub const IX_REMOVE: u8 = 2;
pub const IX_SWAP: u8 = 3;
pub const IX_PAUSE: u8 = 4;
pub const POOL_DISC: u8 = 1;
pub const VAULT_DISC: u8 = 2;
/// Client must attach a compute-budget ix above the 200k default (weighted ln/exp).
pub const SWAP_COMPUTE_UNITS: u32 = 1_000_000;

/// Packed: no implicit padding (a leading `u8` disc would otherwise shift `fee_bps`).
#[repr(C, packed)]
pub struct Pool {
    pub disc: u8,
    pub paused: u8,
    pub bump: u8,
    pub lp_bump: u8,
    pub weight_a: u16,
    pub weight_b: u16,
    pub weight_c: u16,
    pub fee_bps: u64,
    pub authority: [u8; 32],
    pub mint_a: [u8; 32],
    pub mint_b: [u8; 32],
    pub mint_c: [u8; 32],
    pub vault_a: [u8; 32],
    pub vault_b: [u8; 32],
    pub vault_c: [u8; 32],
    pub lp_mint: [u8; 32],
    pub vault_bump: u8,
    pub _pad: [u8; 7],
    pub reserve_a: u64,
    pub reserve_b: u64,
    pub reserve_c: u64,
    pub lp_supply: u64,
}

pub const POOL_LEN: usize = core::mem::size_of::<Pool>();

#[repr(C, packed)]
pub struct VaultState {
    pub disc: u8,
    pub bump: u8,
    pub unlocked: u8,
    pub authority: [u8; 32],
}

pub const VAULT_STATE_LEN: usize = core::mem::size_of::<VaultState>();

fn addr_bytes(a: &Address) -> [u8; 32] {
    let s = a.as_ref();
    let mut out = [0u8; 32];
    out.copy_from_slice(s);
    out
}

impl Pool {
    fn load(data: &[u8]) -> Result<&Self, ProgramError> {
        if data.len() < POOL_LEN || data[0] != POOL_DISC {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok(unsafe { &*(data.as_ptr() as *const Self) })
    }
    fn load_mut(data: &mut [u8]) -> Result<&mut Self, ProgramError> {
        if data.len() < POOL_LEN || data[0] != POOL_DISC {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok(unsafe { &mut *(data.as_mut_ptr() as *mut Self) })
    }
}

fn u64_at(data: &[u8], off: usize) -> Result<u64, ProgramError> {
    let s = data
        .get(off..off + 8)
        .ok_or(ProgramError::InvalidInstructionData)?;
    Ok(u64::from_le_bytes(s.try_into().unwrap()))
}

fn u16_at(data: &[u8], off: usize) -> Result<u16, ProgramError> {
    let s = data
        .get(off..off + 2)
        .ok_or(ProgramError::InvalidInstructionData)?;
    Ok(u16::from_le_bytes(s.try_into().unwrap()))
}

fn read_weights(data: &[u8]) -> Result<[u16; 3], ProgramError> {
    if data.len() < 14 {
        return Ok(equal_weights());
    }
    let w = [u16_at(data, 8)?, u16_at(data, 10)?, u16_at(data, 12)?];
    if !validate_weights(w) {
        return Err(ProgramError::InvalidArgument);
    }
    Ok(w)
}

pub fn process_instruction(
    program_id: &Address,
    accounts: &mut [AccountView],
    data: &[u8],
) -> ProgramResult {
    let (tag, rest) = data
        .split_first()
        .ok_or(ProgramError::InvalidInstructionData)?;
    match *tag {
        IX_INIT => router_init(program_id, accounts, rest),
        IX_ADD => router_join(accounts, data),
        IX_REMOVE => router_exit(accounts, data),
        IX_SWAP => router_swap(accounts, data),
        IX_PAUSE => router_pause(accounts, rest),
        _ => Err(ProgramError::InvalidInstructionData),
    }
}

fn rent_exempt(space: u64) -> u64 {
    (space + 128).saturating_mul(3480).saturating_mul(2)
}

fn create_pda(
    payer: &AccountView,
    pda: &AccountView,
    space: u64,
    owner: &Address,
    seeds: &[Seed],
) -> ProgramResult {
    let signers = [Signer::from(seeds)];
    CreateAccount {
        from: payer,
        to: pda,
        lamports: rent_exempt(space),
        space,
        owner,
    }
    .invoke_signed(&signers)
}

fn router_init(program_id: &Address, accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    let fee_bps = u64_at(data, 0)?;
    if fee_bps >= 10_000 {
        return Err(ProgramError::InvalidArgument);
    }
    let weights = read_weights(data)?;
    let [payer, authority, mint_a, mint_b, mint_c, vault_state, pool, vault_a, vault_b, vault_c, lp_mint, token_program, _system] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    if !payer.is_signer() || !authority.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if token_program.address() != &pinocchio_token::ID {
        return Err(ProgramError::IncorrectProgramId);
    }

    let (vault_addr, vault_bump) =
        Address::find_program_address(&[VAULT_STATE_SEED], program_id);
    if vault_state.address() != &vault_addr {
        return Err(ProgramError::InvalidSeeds);
    }
    let vbump_arr = [vault_bump];
    let vault_seeds = [
        Seed::from(VAULT_STATE_SEED),
        Seed::from(vbump_arr.as_ref()),
    ];
    if vault_state.lamports() == 0 {
        create_pda(
            payer,
            vault_state,
            VAULT_STATE_LEN as u64,
            program_id,
            &vault_seeds,
        )?;
        let mut buf = vault_state.try_borrow_mut()?;
        if buf.len() < VAULT_STATE_LEN {
            return Err(ProgramError::AccountDataTooSmall);
        }
        let v = unsafe { &mut *(buf.as_mut_ptr() as *mut VaultState) };
        v.disc = VAULT_DISC;
        v.bump = vault_bump;
        v.unlocked = 0;
        v.authority = addr_bytes(authority.address());
    } else {
        let buf = vault_state.try_borrow()?;
        if buf.len() < VAULT_STATE_LEN || buf[0] != VAULT_DISC {
            return Err(ProgramError::InvalidAccountData);
        }
    }

    let authority_b = addr_bytes(authority.address());
    let mint_a_b = addr_bytes(mint_a.address());
    let mint_b_b = addr_bytes(mint_b.address());
    let mint_c_b = addr_bytes(mint_c.address());
    let vault_a_b = addr_bytes(vault_a.address());
    let vault_b_b = addr_bytes(vault_b.address());
    let vault_c_b = addr_bytes(vault_c.address());
    let lp_mint_b = addr_bytes(lp_mint.address());

    let (pool_addr, pool_bump) = Address::find_program_address(
        &[
            POOL_SEED,
            mint_a.address().as_ref(),
            mint_b.address().as_ref(),
            mint_c.address().as_ref(),
        ],
        program_id,
    );
    if pool.address() != &pool_addr {
        return Err(ProgramError::InvalidSeeds);
    }
    let bump_arr = [pool_bump];
    let pool_seeds = [
        Seed::from(POOL_SEED),
        Seed::from(mint_a.address().as_ref()),
        Seed::from(mint_b.address().as_ref()),
        Seed::from(mint_c.address().as_ref()),
        Seed::from(&bump_arr),
    ];
    create_pda(payer, pool, POOL_LEN as u64, program_id, &pool_seeds)?;

    fn init_vault_ata(
        payer: &AccountView,
        pool: &AccountView,
        vault_ata: &AccountView,
        mint: &AccountView,
        vault_owner: &Address,
        program_id: &Address,
    ) -> ProgramResult {
        let (expected, vbump) = Address::find_program_address(
            &[VAULT_SEED, pool.address().as_ref(), mint.address().as_ref()],
            program_id,
        );
        if vault_ata.address() != &expected {
            return Err(ProgramError::InvalidSeeds);
        }
        let vb = [vbump];
        let seeds = [
            Seed::from(VAULT_SEED),
            Seed::from(pool.address().as_ref()),
            Seed::from(mint.address().as_ref()),
            Seed::from(&vb),
        ];
        create_pda(
            payer,
            vault_ata,
            SplAccount::LEN as u64,
            &pinocchio_token::ID,
            &seeds,
        )?;
        InitializeAccount3::new(vault_ata, mint, vault_owner).invoke()
    }
    let vault_owner = vault_state.address();
    init_vault_ata(payer, pool, vault_a, mint_a, vault_owner, program_id)?;
    init_vault_ata(payer, pool, vault_b, mint_b, vault_owner, program_id)?;
    init_vault_ata(payer, pool, vault_c, mint_c, vault_owner, program_id)?;

    let (lp_addr, lp_bump) =
        Address::find_program_address(&[LP_SEED, pool.address().as_ref()], program_id);
    if lp_mint.address() != &lp_addr {
        return Err(ProgramError::InvalidSeeds);
    }
    let lb = [lp_bump];
    let lp_seeds = [
        Seed::from(LP_SEED),
        Seed::from(pool.address().as_ref()),
        Seed::from(&lb),
    ];
    create_pda(payer, lp_mint, Mint::LEN as u64, &pinocchio_token::ID, &lp_seeds)?;
    InitializeMint2::new(lp_mint, 6, vault_owner, None).invoke()?;

    {
        let mut buf = pool.try_borrow_mut()?;
        if buf.len() < POOL_LEN {
            return Err(ProgramError::AccountDataTooSmall);
        }
        let p = unsafe { &mut *(buf.as_mut_ptr() as *mut Pool) };
        p.disc = POOL_DISC;
        p.authority = authority_b;
        p.mint_a = mint_a_b;
        p.mint_b = mint_b_b;
        p.mint_c = mint_c_b;
        p.vault_a = vault_a_b;
        p.vault_b = vault_b_b;
        p.vault_c = vault_c_b;
        p.lp_mint = lp_mint_b;
        p.fee_bps = fee_bps;
        p.weight_a = weights[0];
        p.weight_b = weights[1];
        p.weight_c = weights[2];
        p.paused = 0;
        p.bump = pool_bump;
        p.lp_bump = lp_bump;
        p.vault_bump = vault_bump;
        p._pad = [0; 7];
        p.reserve_a = 0;
        p.reserve_b = 0;
        p.reserve_c = 0;
        p.lp_supply = 0;
    }
    Ok(())
}

/// Vault pubkeys must be exactly the three PDA vaults stored on `pool`.
pub fn vaults_match_pool(
    pool: &Pool,
    vault_a: &[u8; 32],
    vault_b: &[u8; 32],
    vault_c: &[u8; 32],
) -> bool {
    let a = pool.vault_a;
    let b = pool.vault_b;
    let c = pool.vault_c;
    &a == vault_a && &b == vault_b && &c == vault_c
}

pub fn lp_matches_pool(pool: &Pool, lp_mint: &[u8; 32]) -> bool {
    let expected = pool.lp_mint;
    &expected == lp_mint
}

fn token_amt(acc: &AccountView) -> Result<u64, ProgramError> {
    Ok(SplAccount::from_account_view(acc)?.amount())
}

fn router_join(accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    let [user, vault_state, pool, vault_a, vault_b, vault_c, lp_mint, user_a, user_b, user_c, user_lp, _token] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    if !user.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if pool.owner() != &ID || vault_state.owner() != &ID {
        return Err(ProgramError::IncorrectProgramId);
    }
    let (used, lp, vault_bump) = {
        let mut pbuf = pool.try_borrow_mut()?;
        let p = Pool::load_mut(&mut pbuf)?;
        let va = addr_bytes(vault_a.address());
        let vb = addr_bytes(vault_b.address());
        let vc = addr_bytes(vault_c.address());
        let lp_b = addr_bytes(lp_mint.address());
        let mut session = handlers::VaultSession { unlocked: 0 };
        let mut user_bpt = 0u64;
        let (used, lp) = handlers::handle_join(
            data,
            p,
            &mut session,
            [&va, &vb, &vc],
            &lp_b,
            &mut user_bpt,
        )?;
        (used, lp, p.vault_bump)
    };
    Transfer::new(user_a, vault_a, user, used[0]).invoke()?;
    Transfer::new(user_b, vault_b, user, used[1]).invoke()?;
    Transfer::new(user_c, vault_c, user, used[2]).invoke()?;
    let bump = [vault_bump];
    let seeds = [
        Seed::from(VAULT_STATE_SEED),
        Seed::from(bump.as_ref()),
    ];
    let signers = [Signer::from(&seeds)];
    MintTo::new(lp_mint, user_lp, vault_state, lp).invoke_signed(&signers)?;
    Ok(())
}

fn router_exit(accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    let [user, vault_state, pool, vault_a, vault_b, vault_c, lp_mint, user_a, user_b, user_c, user_lp, _token] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    if !user.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if pool.owner() != &ID || vault_state.owner() != &ID {
        return Err(ProgramError::IncorrectProgramId);
    }
    let mut user_bpt = token_amt(user_lp)?;
    let (out, vault_bump) = {
        let mut pbuf = pool.try_borrow_mut()?;
        let p = Pool::load_mut(&mut pbuf)?;
        let va = addr_bytes(vault_a.address());
        let vb = addr_bytes(vault_b.address());
        let vc = addr_bytes(vault_c.address());
        let lp_b = addr_bytes(lp_mint.address());
        let mut session = handlers::VaultSession { unlocked: 0 };
        let out = handlers::handle_exit(
            data,
            p,
            &mut session,
            [&va, &vb, &vc],
            &lp_b,
            &mut user_bpt,
        )?;
        (out, p.vault_bump)
    };
    Burn::new(user_lp, lp_mint, user, data_lp_burn(data)?).invoke()?;
    let bump = [vault_bump];
    let seeds = [
        Seed::from(VAULT_STATE_SEED),
        Seed::from(bump.as_ref()),
    ];
    let signers = [Signer::from(&seeds)];
    Transfer::new(vault_a, user_a, vault_state, out[0]).invoke_signed(&signers)?;
    Transfer::new(vault_b, user_b, vault_state, out[1]).invoke_signed(&signers)?;
    Transfer::new(vault_c, user_c, vault_state, out[2]).invoke_signed(&signers)?;
    Ok(())
}

fn data_lp_burn(data: &[u8]) -> Result<u64, ProgramError> {
    if data.first().copied() != Some(IX_REMOVE) {
        return Err(ProgramError::InvalidInstructionData);
    }
    u64_at(&data[1..], 0)
}

fn router_swap(accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    let [user, vault_state, pool, vault_a, vault_b, vault_c, user_a, user_b, user_c, _token] = accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    if !user.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if pool.owner() != &ID || vault_state.owner() != &ID {
        return Err(ProgramError::IncorrectProgramId);
    }
    let (token_in, token_out, amount_in, amount_out, vault_bump) = {
        let mut pbuf = pool.try_borrow_mut()?;
        let p = Pool::load_mut(&mut pbuf)?;
        let va = addr_bytes(vault_a.address());
        let vb = addr_bytes(vault_b.address());
        let vc = addr_bytes(vault_c.address());
        let mut session = handlers::VaultSession { unlocked: 0 };
        let out = handlers::handle_swap(data, p, &mut session, [&va, &vb, &vc])?;
        let tin = data.get(1).copied().ok_or(ProgramError::InvalidInstructionData)? as usize;
        let tout = data.get(2).copied().ok_or(ProgramError::InvalidInstructionData)? as usize;
        let ain = u64_at(&data[1..], 2)?;
        (tin, tout, ain, out, p.vault_bump)
    };
    match token_in {
        0 => Transfer::new(user_a, vault_a, user, amount_in).invoke()?,
        1 => Transfer::new(user_b, vault_b, user, amount_in).invoke()?,
        _ => Transfer::new(user_c, vault_c, user, amount_in).invoke()?,
    }
    let bump = [vault_bump];
    let seeds = [
        Seed::from(VAULT_STATE_SEED),
        Seed::from(bump.as_ref()),
    ];
    let signers = [Signer::from(&seeds)];
    match token_out {
        0 => Transfer::new(vault_a, user_a, vault_state, amount_out).invoke_signed(&signers)?,
        1 => Transfer::new(vault_b, user_b, vault_state, amount_out).invoke_signed(&signers)?,
        _ => Transfer::new(vault_c, user_c, vault_state, amount_out).invoke_signed(&signers)?,
    }
    Ok(())
}

fn router_pause(accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    let paused = *data.first().ok_or(ProgramError::InvalidInstructionData)?;
    let [authority, pool] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    if !authority.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let mut buf = pool.try_borrow_mut()?;
    let p = Pool::load_mut(&mut buf)?;
    if p.authority != addr_bytes(authority.address()) {
        return Err(ProgramError::MissingRequiredSignature);
    }
    p.paused = paused;
    Ok(())
}

#[cfg(test)]
mod bind_tests {
    use super::*;

    fn sample_pool() -> Pool {
        sample_pool_weights(3333, 3333, 3334)
    }

    fn sample_pool_weights(wa: u16, wb: u16, wc: u16) -> Pool {
        Pool {
            disc: POOL_DISC,
            paused: 0,
            bump: 255,
            lp_bump: 254,
            weight_a: wa,
            weight_b: wb,
            weight_c: wc,
            fee_bps: 30,
            authority: [1u8; 32],
            mint_a: [2u8; 32],
            mint_b: [3u8; 32],
            mint_c: [4u8; 32],
            vault_a: [10u8; 32],
            vault_b: [11u8; 32],
            vault_c: [12u8; 32],
            lp_mint: [13u8; 32],
            vault_bump: 253,
            _pad: [0; 7],
            reserve_a: 0,
            reserve_b: 0,
            reserve_c: 0,
            lp_supply: 0,
        }
    }

    #[test]
    fn honest_vaults_and_lp_bind() {
        let p = sample_pool();
        let va = p.vault_a;
        let vb = p.vault_b;
        let vc = p.vault_c;
        let lp = p.lp_mint;
        assert!(vaults_match_pool(&p, &va, &vb, &vc));
        assert!(lp_matches_pool(&p, &lp));
    }

    #[test]
    fn swap_rejects_tiny_self_owned_vault_in() {
        let p = sample_pool();
        let fake_in = [99u8; 32];
        let vb = p.vault_b;
        let vc = p.vault_c;
        assert!(
            !vaults_match_pool(&p, &fake_in, &vb, &vc),
            "attacker vault_in must not bind"
        );
    }

    #[test]
    fn remove_rejects_self_minted_fake_lp() {
        let p = sample_pool();
        let fake_lp = [0xffu8; 32];
        assert!(!lp_matches_pool(&p, &fake_lp));
        let va = p.vault_a;
        let vb = p.vault_b;
        let vc = p.vault_c;
        assert!(
            vaults_match_pool(&p, &va, &vb, &vc),
            "real vaults alone are not enough without lp bind"
        );
    }

    #[test]
    fn packed_layout_puts_weights_at_offset_4() {
        assert_eq!(core::mem::size_of::<Pool>(), POOL_LEN);
        let p = sample_pool_weights(5000, 3000, 2000);
        let bytes: [u8; POOL_LEN] = unsafe { core::mem::transmute_copy(&p) };
        assert_eq!(bytes[0], POOL_DISC);
        assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), 5000);
        assert_eq!(u16::from_le_bytes([bytes[6], bytes[7]]), 3000);
        assert_eq!(u16::from_le_bytes([bytes[8], bytes[9]]), 2000);
    }

    #[test]
    fn bind_holds_for_unequal_weight_pools() {
        for w in [[5000u16, 3000, 2000], [8000, 1000, 1000], [100, 100, 9800]] {
            let p = sample_pool_weights(w[0], w[1], w[2]);
            let va = p.vault_a;
            let vb = p.vault_b;
            let vc = p.vault_c;
            let lp = p.lp_mint;
            assert!(vaults_match_pool(&p, &va, &vb, &vc), "{w:?}");
            assert!(lp_matches_pool(&p, &lp), "{w:?}");
            assert!(!vaults_match_pool(&p, &[9u8; 32], &vb, &vc), "{w:?}");
        }
    }
}

#[cfg(test)]
mod handler_tests {
    use super::*;
    use crate::handlers::{
        encode_exit, encode_join, encode_swap, handle_exit, handle_join, handle_swap, VaultSession,
    };
    use three_amm_math::swap_out_given_in_weighted;

    fn pool_50_30_20() -> Pool {
        Pool {
            disc: POOL_DISC,
            paused: 0,
            bump: 255,
            lp_bump: 254,
            weight_a: 5000,
            weight_b: 3000,
            weight_c: 2000,
            fee_bps: 30,
            authority: [1u8; 32],
            mint_a: [2u8; 32],
            mint_b: [3u8; 32],
            mint_c: [4u8; 32],
            vault_a: [10u8; 32],
            vault_b: [11u8; 32],
            vault_c: [12u8; 32],
            lp_mint: [13u8; 32],
            vault_bump: 253,
            _pad: [0; 7],
            reserve_a: 0,
            reserve_b: 0,
            reserve_c: 0,
            lp_supply: 0,
        }
    }

    fn pool_80_10_10() -> Pool {
        let mut p = pool_50_30_20();
        p.weight_a = 8000;
        p.weight_b = 1000;
        p.weight_c = 1000;
        p.fee_bps = 0;
        p.vault_a = [20u8; 32];
        p.vault_b = [21u8; 32];
        p.vault_c = [22u8; 32];
        p.lp_mint = [23u8; 32];
        p
    }

    #[test]
    fn two_pools_swap_a_leaves_b_bitwise_unchanged() {
        let mut a = pool_50_30_20();
        let mut b = pool_80_10_10();
        let mut sa = VaultSession::new();
        let mut sb = VaultSession::new();
        let mut bpt_a = 0u64;
        let mut bpt_b = 0u64;
        let va = a.vault_a;
        let vb = a.vault_b;
        let vc = a.vault_c;
        let lp_a = a.lp_mint;
        handle_join(
            &encode_join([5_000_000, 3_000_000, 2_000_000], 1),
            &mut a,
            &mut sa,
            [&va, &vb, &vc],
            &lp_a,
            &mut bpt_a,
        )
        .unwrap();
        let bva = b.vault_a;
        let bvb = b.vault_b;
        let bvc = b.vault_c;
        let lp_b = b.lp_mint;
        handle_join(
            &encode_join([8_000_000, 1_000_000, 1_000_000], 1),
            &mut b,
            &mut sb,
            [&bva, &bvb, &bvc],
            &lp_b,
            &mut bpt_b,
        )
        .unwrap();
        let b_before = [b.reserve_a, b.reserve_b, b.reserve_c];
        let b_lp = b.lp_supply;
        let quote = swap_out_given_in_weighted(a.reserve_a, 5000, a.reserve_b, 3000, 10_000, 30)
            .unwrap();
        handle_swap(
            &encode_swap(0, 1, 10_000, quote),
            &mut a,
            &mut sa,
            [&va, &vb, &vc],
        )
        .unwrap();
        assert_eq!([b.reserve_a, b.reserve_b, b.reserve_c], b_before);
        let b_lp_after = b.lp_supply;
        assert_eq!(b_lp_after, b_lp);
    }

    #[test]
    fn zero_bpt_user_cannot_exit() {
        let mut p = pool_50_30_20();
        let mut session = VaultSession::new();
        let mut owner_bpt = 0u64;
        let va = p.vault_a;
        let vb = p.vault_b;
        let vc = p.vault_c;
        let lp = p.lp_mint;
        handle_join(
            &encode_join([5_000_000, 3_000_000, 2_000_000], 1),
            &mut p,
            &mut session,
            [&va, &vb, &vc],
            &lp,
            &mut owner_bpt,
        )
        .unwrap();
        let reserves_before = [p.reserve_a, p.reserve_b, p.reserve_c];
        let mut stranger = 0u64;
        let err = handle_exit(
            &encode_exit(1, [0, 0, 0]),
            &mut p,
            &mut session,
            [&va, &vb, &vc],
            &lp,
            &mut stranger,
        )
        .unwrap_err();
        assert_eq!(err, ProgramError::InsufficientFunds);
        assert_eq!([p.reserve_a, p.reserve_b, p.reserve_c], reserves_before);
        let supply = p.lp_supply;
        assert_eq!(owner_bpt, supply);
    }

    #[test]
    fn user_b_zero_lp_cannot_exit_user_a() {
        let mut p = pool_50_30_20();
        let mut session = VaultSession::new();
        let mut a_bpt = 0u64;
        let va = p.vault_a;
        let vb = p.vault_b;
        let vc = p.vault_c;
        let lp = p.lp_mint;
        handle_join(
            &encode_join([5_000_000, 3_000_000, 2_000_000], 1),
            &mut p,
            &mut session,
            [&va, &vb, &vc],
            &lp,
            &mut a_bpt,
        )
        .unwrap();
        let before = [p.reserve_a, p.reserve_b, p.reserve_c];
        let mut b_bpt = 0u64;
        assert!(handle_exit(
            &encode_exit(a_bpt, [0, 0, 0]),
            &mut p,
            &mut session,
            [&va, &vb, &vc],
            &lp,
            &mut b_bpt,
        )
        .is_err());
        assert_eq!([p.reserve_a, p.reserve_b, p.reserve_c], before);
        let supply = p.lp_supply;
        assert_eq!(a_bpt, supply);
        assert_eq!(b_bpt, 0);
    }

    #[test]
    fn fake_vault_in_fails_handle_swap() {
        let mut p = pool_50_30_20();
        let mut session = VaultSession::new();
        let mut bpt = 0u64;
        let va = p.vault_a;
        let vb = p.vault_b;
        let vc = p.vault_c;
        let lp = p.lp_mint;
        handle_join(
            &encode_join([5_000_000, 3_000_000, 2_000_000], 1),
            &mut p,
            &mut session,
            [&va, &vb, &vc],
            &lp,
            &mut bpt,
        )
        .unwrap();
        let fake = [99u8; 32];
        assert!(handle_swap(
            &encode_swap(0, 1, 10_000, 1),
            &mut p,
            &mut session,
            [&fake, &vb, &vc],
        )
        .is_err());
    }

    #[test]
    fn fake_lp_fails_handle_exit() {
        let mut p = pool_50_30_20();
        let mut session = VaultSession::new();
        let mut bpt = 0u64;
        let va = p.vault_a;
        let vb = p.vault_b;
        let vc = p.vault_c;
        let lp = p.lp_mint;
        handle_join(
            &encode_join([5_000_000, 3_000_000, 2_000_000], 1),
            &mut p,
            &mut session,
            [&va, &vb, &vc],
            &lp,
            &mut bpt,
        )
        .unwrap();
        let fake_lp = [0xffu8; 32];
        assert!(handle_exit(
            &encode_exit(1, [0, 0, 0]),
            &mut p,
            &mut session,
            [&va, &vb, &vc],
            &fake_lp,
            &mut bpt,
        )
        .is_err());
    }

    #[test]
    fn router_swap_50_30_20_matches_shipped_quote() {
        let mut p = pool_50_30_20();
        let mut session = VaultSession::new();
        let mut bpt = 0u64;
        let va = p.vault_a;
        let vb = p.vault_b;
        let vc = p.vault_c;
        let lp = p.lp_mint;
        handle_join(
            &encode_join([5_000_000, 3_000_000, 2_000_000], 1),
            &mut p,
            &mut session,
            [&va, &vb, &vc],
            &lp,
            &mut bpt,
        )
        .unwrap();
        let c_before = p.reserve_c;
        let quote = swap_out_given_in_weighted(p.reserve_a, 5000, p.reserve_b, 3000, 10_000, 30)
            .unwrap();
        let out = handle_swap(
            &encode_swap(0, 1, 10_000, quote),
            &mut p,
            &mut session,
            [&va, &vb, &vc],
        )
        .unwrap();
        assert_eq!(out, quote);
        let ra = p.reserve_a;
        let rb = p.reserve_b;
        let rc = p.reserve_c;
        assert_eq!(rc, c_before);
        assert_eq!(ra, 5_000_000 + 10_000);
        assert_eq!(rb, 3_000_000 - out);
    }

    #[test]
    fn swap_compute_budget_exceeds_200k_default() {
        assert!(SWAP_COMPUTE_UNITS > 200_000);
    }

    #[test]
    fn pool_module_has_no_token_transfers() {
        let src = include_str!("pool.rs");
        assert!(!src.contains("Transfer::"));
        assert!(!src.contains("MintTo::"));
        assert!(!src.contains("Burn::"));
        assert!(!src.contains("pinocchio_token"));
    }
}
