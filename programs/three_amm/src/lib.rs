use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, MintTo, Burn, Token, TokenAccount, Transfer};
use three_amm_math::{
    apply_swap, invariant, invariant_ge, proportional_add, proportional_remove,
};

declare_id!("Etmngw3iSqFvj65cfrhfexMXtZAoGX3VFf2Xs3AzgbBx");

pub const POOL_SEED: &[u8] = b"pool";
pub const VAULT_SEED: &[u8] = b"vault";
pub const LP_SEED: &[u8] = b"lp";

#[program]
pub mod three_amm {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>, fee_bps: u64) -> Result<()> {
        require!(fee_bps < 10_000, AmmError::BadFee);
        let pool = &mut ctx.accounts.pool;
        pool.authority = ctx.accounts.authority.key();
        pool.mint_a = ctx.accounts.mint_a.key();
        pool.mint_b = ctx.accounts.mint_b.key();
        pool.mint_c = ctx.accounts.mint_c.key();
        pool.vault_a = ctx.accounts.vault_a.key();
        pool.vault_b = ctx.accounts.vault_b.key();
        pool.vault_c = ctx.accounts.vault_c.key();
        pool.lp_mint = ctx.accounts.lp_mint.key();
        pool.fee_bps = fee_bps;
        pool.paused = false;
        pool.bump = ctx.bumps.pool;
        pool.lp_bump = ctx.bumps.lp_mint;
        Ok(())
    }

    pub fn set_paused(ctx: Context<Admin>, paused: bool) -> Result<()> {
        ctx.accounts.pool.paused = paused;
        Ok(())
    }

    pub fn add_liquidity(
        ctx: Context<ModifyLiq>,
        amount_a: u64,
        amount_b: u64,
        amount_c: u64,
        min_lp: u64,
    ) -> Result<()> {
        require!(!ctx.accounts.pool.paused, AmmError::Paused);
        let supply = ctx.accounts.lp_mint.supply;
        let reserves = [
            ctx.accounts.vault_a.amount,
            ctx.accounts.vault_b.amount,
            ctx.accounts.vault_c.amount,
        ];
        let (used, lp) = proportional_add(reserves, [amount_a, amount_b, amount_c], supply)
            .ok_or(AmmError::Math)?;
        require!(lp >= min_lp, AmmError::Slippage);

        token::transfer(ctx.accounts.xfer_user_a(), used[0])?;
        token::transfer(ctx.accounts.xfer_user_b(), used[1])?;
        token::transfer(ctx.accounts.xfer_user_c(), used[2])?;

        let bump = ctx.accounts.pool.bump;
        let ma = ctx.accounts.pool.mint_a;
        let mb = ctx.accounts.pool.mint_b;
        let mc = ctx.accounts.pool.mint_c;
        let bump_arr = [bump];
        let seeds: &[&[u8]] = &[POOL_SEED, ma.as_ref(), mb.as_ref(), mc.as_ref(), &bump_arr];
        let signer = &[seeds];
        token::mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                MintTo {
                    mint: ctx.accounts.lp_mint.to_account_info(),
                    to: ctx.accounts.user_lp.to_account_info(),
                    authority: ctx.accounts.pool.to_account_info(),
                },
                signer,
            ),
            lp,
        )?;
        Ok(())
    }

    pub fn remove_liquidity(ctx: Context<ModifyLiq>, lp_burn: u64, min_out: [u64; 3]) -> Result<()> {
        require!(!ctx.accounts.pool.paused, AmmError::Paused);
        let supply = ctx.accounts.lp_mint.supply;
        let reserves = [
            ctx.accounts.vault_a.amount,
            ctx.accounts.vault_b.amount,
            ctx.accounts.vault_c.amount,
        ];
        let out = proportional_remove(reserves, lp_burn, supply).ok_or(AmmError::Math)?;
        require!(
            out[0] >= min_out[0] && out[1] >= min_out[1] && out[2] >= min_out[2],
            AmmError::Slippage
        );

        token::burn(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Burn {
                    mint: ctx.accounts.lp_mint.to_account_info(),
                    from: ctx.accounts.user_lp.to_account_info(),
                    authority: ctx.accounts.user.to_account_info(),
                },
            ),
            lp_burn,
        )?;

        let bump = ctx.accounts.pool.bump;
        let ma = ctx.accounts.pool.mint_a;
        let mb = ctx.accounts.pool.mint_b;
        let mc = ctx.accounts.pool.mint_c;
        let bump_arr = [bump];
        let seeds: &[&[u8]] = &[POOL_SEED, ma.as_ref(), mb.as_ref(), mc.as_ref(), &bump_arr];
        let signer = &[seeds];

        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.vault_a.to_account_info(),
                    to: ctx.accounts.user_a.to_account_info(),
                    authority: ctx.accounts.pool.to_account_info(),
                },
                signer,
            ),
            out[0],
        )?;
        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.vault_b.to_account_info(),
                    to: ctx.accounts.user_b.to_account_info(),
                    authority: ctx.accounts.pool.to_account_info(),
                },
                signer,
            ),
            out[1],
        )?;
        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.vault_c.to_account_info(),
                    to: ctx.accounts.user_c.to_account_info(),
                    authority: ctx.accounts.pool.to_account_info(),
                },
                signer,
            ),
            out[2],
        )?;
        Ok(())
    }

    pub fn swap(
        ctx: Context<Swap>,
        token_in: u8,
        token_out: u8,
        amount_in: u64,
        min_out: u64,
    ) -> Result<()> {
        require!(!ctx.accounts.pool.paused, AmmError::Paused);
        require!(token_in < 3 && token_out < 3 && token_in != token_out, AmmError::BadPair);

        let reserves = [
            ctx.accounts.vault_a.amount,
            ctx.accounts.vault_b.amount,
            ctx.accounts.vault_c.amount,
        ];
        let k_before = invariant(reserves[0], reserves[1], reserves[2]);
        let (next, amount_out) = apply_swap(
            reserves,
            token_in as usize,
            token_out as usize,
            amount_in,
            ctx.accounts.pool.fee_bps,
        )
        .ok_or(AmmError::Math)?;
        require!(amount_out >= min_out, AmmError::Slippage);
        let k_after = invariant(next[0], next[1], next[2]);
        require!(invariant_ge(k_after, k_before), AmmError::InvariantDrop);

        // Transfer in
        let (user_in, vault_in) = match token_in {
            0 => (
                ctx.accounts.user_a.to_account_info(),
                ctx.accounts.vault_a.to_account_info(),
            ),
            1 => (
                ctx.accounts.user_b.to_account_info(),
                ctx.accounts.vault_b.to_account_info(),
            ),
            _ => (
                ctx.accounts.user_c.to_account_info(),
                ctx.accounts.vault_c.to_account_info(),
            ),
        };
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: user_in,
                    to: vault_in,
                    authority: ctx.accounts.user.to_account_info(),
                },
            ),
            amount_in,
        )?;

        let bump = ctx.accounts.pool.bump;
        let ma = ctx.accounts.pool.mint_a;
        let mb = ctx.accounts.pool.mint_b;
        let mc = ctx.accounts.pool.mint_c;
        let bump_arr = [bump];
        let seeds: &[&[u8]] = &[POOL_SEED, ma.as_ref(), mb.as_ref(), mc.as_ref(), &bump_arr];
        let signer = &[seeds];

        let (vault_out, user_out) = match token_out {
            0 => (
                ctx.accounts.vault_a.to_account_info(),
                ctx.accounts.user_a.to_account_info(),
            ),
            1 => (
                ctx.accounts.vault_b.to_account_info(),
                ctx.accounts.user_b.to_account_info(),
            ),
            _ => (
                ctx.accounts.vault_c.to_account_info(),
                ctx.accounts.user_c.to_account_info(),
            ),
        };
        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: vault_out,
                    to: user_out,
                    authority: ctx.accounts.pool.to_account_info(),
                },
                signer,
            ),
            amount_out,
        )?;
        Ok(())
    }
}

#[account]
#[derive(InitSpace)]
pub struct Pool {
    pub authority: Pubkey,
    pub mint_a: Pubkey,
    pub mint_b: Pubkey,
    pub mint_c: Pubkey,
    pub vault_a: Pubkey,
    pub vault_b: Pubkey,
    pub vault_c: Pubkey,
    pub lp_mint: Pubkey,
    pub fee_bps: u64,
    pub paused: bool,
    pub bump: u8,
    pub lp_bump: u8,
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    pub authority: Signer<'info>,
    pub mint_a: Account<'info, Mint>,
    pub mint_b: Account<'info, Mint>,
    pub mint_c: Account<'info, Mint>,
    #[account(
        init,
        payer = payer,
        space = 8 + Pool::INIT_SPACE,
        seeds = [POOL_SEED, mint_a.key().as_ref(), mint_b.key().as_ref(), mint_c.key().as_ref()],
        bump
    )]
    pub pool: Account<'info, Pool>,
    #[account(
        init,
        payer = payer,
        token::mint = mint_a,
        token::authority = pool,
        seeds = [VAULT_SEED, pool.key().as_ref(), mint_a.key().as_ref()],
        bump
    )]
    pub vault_a: Account<'info, TokenAccount>,
    #[account(
        init,
        payer = payer,
        token::mint = mint_b,
        token::authority = pool,
        seeds = [VAULT_SEED, pool.key().as_ref(), mint_b.key().as_ref()],
        bump
    )]
    pub vault_b: Account<'info, TokenAccount>,
    #[account(
        init,
        payer = payer,
        token::mint = mint_c,
        token::authority = pool,
        seeds = [VAULT_SEED, pool.key().as_ref(), mint_c.key().as_ref()],
        bump
    )]
    pub vault_c: Account<'info, TokenAccount>,
    #[account(
        init,
        payer = payer,
        mint::decimals = 6,
        mint::authority = pool,
        seeds = [LP_SEED, pool.key().as_ref()],
        bump
    )]
    pub lp_mint: Account<'info, Mint>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Admin<'info> {
    pub authority: Signer<'info>,
    #[account(mut, has_one = authority)]
    pub pool: Account<'info, Pool>,
}

#[derive(Accounts)]
pub struct ModifyLiq<'info> {
    pub user: Signer<'info>,
    #[account(
        mut,
        has_one = vault_a,
        has_one = vault_b,
        has_one = vault_c,
        has_one = lp_mint,
    )]
    pub pool: Account<'info, Pool>,
    #[account(mut)]
    pub vault_a: Account<'info, TokenAccount>,
    #[account(mut)]
    pub vault_b: Account<'info, TokenAccount>,
    #[account(mut)]
    pub vault_c: Account<'info, TokenAccount>,
    #[account(mut)]
    pub lp_mint: Account<'info, Mint>,
    #[account(mut, token::mint = pool.mint_a, token::authority = user)]
    pub user_a: Account<'info, TokenAccount>,
    #[account(mut, token::mint = pool.mint_b, token::authority = user)]
    pub user_b: Account<'info, TokenAccount>,
    #[account(mut, token::mint = pool.mint_c, token::authority = user)]
    pub user_c: Account<'info, TokenAccount>,
    #[account(mut, token::mint = lp_mint, token::authority = user)]
    pub user_lp: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

impl<'info> ModifyLiq<'info> {
    fn xfer_user_a(&self) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        CpiContext::new(
            self.token_program.to_account_info(),
            Transfer {
                from: self.user_a.to_account_info(),
                to: self.vault_a.to_account_info(),
                authority: self.user.to_account_info(),
            },
        )
    }
    fn xfer_user_b(&self) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        CpiContext::new(
            self.token_program.to_account_info(),
            Transfer {
                from: self.user_b.to_account_info(),
                to: self.vault_b.to_account_info(),
                authority: self.user.to_account_info(),
            },
        )
    }
    fn xfer_user_c(&self) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        CpiContext::new(
            self.token_program.to_account_info(),
            Transfer {
                from: self.user_c.to_account_info(),
                to: self.vault_c.to_account_info(),
                authority: self.user.to_account_info(),
            },
        )
    }
}

#[derive(Accounts)]
pub struct Swap<'info> {
    pub user: Signer<'info>,
    #[account(
        has_one = vault_a,
        has_one = vault_b,
        has_one = vault_c,
    )]
    pub pool: Account<'info, Pool>,
    #[account(mut)]
    pub vault_a: Account<'info, TokenAccount>,
    #[account(mut)]
    pub vault_b: Account<'info, TokenAccount>,
    #[account(mut)]
    pub vault_c: Account<'info, TokenAccount>,
    #[account(mut, token::mint = pool.mint_a, token::authority = user)]
    pub user_a: Account<'info, TokenAccount>,
    #[account(mut, token::mint = pool.mint_b, token::authority = user)]
    pub user_b: Account<'info, TokenAccount>,
    #[account(mut, token::mint = pool.mint_c, token::authority = user)]
    pub user_c: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[error_code]
pub enum AmmError {
    #[msg("fee_bps must be < 10000")]
    BadFee,
    #[msg("pool paused")]
    Paused,
    #[msg("invalid token pair")]
    BadPair,
    #[msg("math / empty output")]
    Math,
    #[msg("slippage")]
    Slippage,
    #[msg("invariant decreased")]
    InvariantDrop,
}
