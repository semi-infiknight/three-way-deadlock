//! Drive init / swap exact-in / proportional join / exit through the Router.

use three_amm_math::{
    fee_bps_for_volatility, proportional_add, proportional_remove, swap_out_given_in,
    swap_out_given_in_weighted, DEFAULT_SOFTENING_STEPS,
};
use three_amm_router::{Router, RouterError, SofteningBook};
use three_amm_vault::{AccountId, PoolId, UserId, Vault};

fn id(n: u8) -> [u8; 32] {
    [n; 32]
}

struct Fixture {
    vault: Vault,
    pool_id: PoolId,
    vault_acc: [AccountId; 3],
    lp: AccountId,
    user: UserId,
    pool: three_amm_pool::WeightedPool,
}

fn setup(weights: [u16; 3], fee_bps: u64) -> Fixture {
    let mut vault = Vault::new();
    let pool_id = id(1);
    let mints = [id(10), id(11), id(12)];
    let vault_acc = [id(20), id(21), id(22)];
    let lp = id(30);
    let user = id(9);
    let pool = Router::initialize_weighted_pool(
        &mut vault, pool_id, mints, vault_acc, lp, weights, fee_bps,
    )
    .unwrap();
    for m in mints {
        vault.credit_user(user, m, 1_000_000_000).unwrap();
    }
    Fixture {
        vault,
        pool_id,
        vault_acc,
        lp,
        user,
        pool,
    }
}

#[test]
fn swap_exact_in_matches_shipped_weighted_quote_50_30_20() {
    let mut f = setup([5000, 3000, 2000], 30);
    let lp = Router::add_liquidity_proportional(
        &mut f.vault,
        f.pool_id,
        &f.pool,
        f.user,
        f.vault_acc,
        f.lp,
        [5_000_000, 3_000_000, 2_000_000],
        1,
    )
    .unwrap();
    assert!(lp > 0);

    let reserves = f.vault.reserves(f.pool_id).unwrap();
    let amount_in = 10_000u64;
    let quote = f
        .pool
        .quote_out_given_in(reserves[0], reserves[1], 0, 1, amount_in)
        .unwrap();
    let math = swap_out_given_in_weighted(
        reserves[0],
        5000,
        reserves[1],
        3000,
        amount_in,
        30,
    )
    .unwrap();
    assert_eq!(quote, math);

    let b_before = reserves[2];
    let out = Router::swap_exact_in(
        &mut f.vault,
        f.pool_id,
        &f.pool,
        f.user,
        f.vault_acc,
        0,
        1,
        amount_in,
        quote,
    )
    .unwrap();
    assert_eq!(out, quote);
    let after = f.vault.reserves(f.pool_id).unwrap();
    assert_eq!(after[2], b_before, "third reserve amount unchanged");
    assert_eq!(after[0], reserves[0] + amount_in);
    assert_eq!(after[1], reserves[1] - out);
}

#[test]
fn equal_weight_pair_matches_product_formula() {
    // A and B both 40%; C 20%. A→B uses the equal-weight product path.
    let mut f = setup([4000, 4000, 2000], 0);
    Router::add_liquidity_proportional(
        &mut f.vault,
        f.pool_id,
        &f.pool,
        f.user,
        f.vault_acc,
        f.lp,
        [4_000_000, 4_000_000, 2_000_000],
        1,
    )
    .unwrap();
    let r = f.vault.reserves(f.pool_id).unwrap();
    let dx = 10_000u64;
    let product = swap_out_given_in(r[0], r[1], dx, 0).unwrap();
    let out = Router::swap_exact_in(
        &mut f.vault,
        f.pool_id,
        &f.pool,
        f.user,
        f.vault_acc,
        0,
        1,
        dx,
        product,
    )
    .unwrap();
    assert_eq!(out, product);
    assert_eq!(f.vault.reserves(f.pool_id).unwrap()[2], r[2]);
}

#[test]
fn proportional_add_then_remove_matches_shipped_math() {
    let mut f = setup([5000, 3000, 2000], 30);
    let amounts = [5_000_000u64, 3_000_000, 2_000_000];
    let (used0, lp0) = proportional_add([0, 0, 0], amounts, 0).unwrap();
    let minted = Router::add_liquidity_proportional(
        &mut f.vault,
        f.pool_id,
        &f.pool,
        f.user,
        f.vault_acc,
        f.lp,
        amounts,
        1,
    )
    .unwrap();
    assert_eq!(minted, lp0);
    assert_eq!(f.vault.reserves(f.pool_id).unwrap(), used0);
    assert_eq!(f.vault.user_balance(f.user, f.lp), minted);

    let r = f.vault.reserves(f.pool_id).unwrap();
    let supply = f.vault.lp_supply(f.pool_id).unwrap();
    let burn = minted / 2;
    let expect = proportional_remove(r, burn, supply).unwrap();
    let got = Router::remove_liquidity_proportional(
        &mut f.vault,
        f.pool_id,
        &f.pool,
        f.user,
        f.vault_acc,
        f.lp,
        burn,
        [0, 0, 0],
    )
    .unwrap();
    assert_eq!(got, expect);
    assert_eq!(f.vault.user_balance(f.user, f.lp), minted - burn);
}

#[test]
fn router_rejects_fake_vault_in_and_fake_lp() {
    let mut f = setup([5000, 3000, 2000], 30);
    Router::add_liquidity_proportional(
        &mut f.vault,
        f.pool_id,
        &f.pool,
        f.user,
        f.vault_acc,
        f.lp,
        [5_000_000, 3_000_000, 2_000_000],
        1,
    )
    .unwrap();
    let fake = [id(99), f.vault_acc[1], f.vault_acc[2]];
    let err = Router::swap_exact_in(
        &mut f.vault,
        f.pool_id,
        &f.pool,
        f.user,
        fake,
        0,
        1,
        10_000,
        1,
    )
    .unwrap_err();
    assert_eq!(err, RouterError::BindVault);

    let err = Router::remove_liquidity_proportional(
        &mut f.vault,
        f.pool_id,
        &f.pool,
        f.user,
        f.vault_acc,
        id(0xff),
        1,
        [0, 0, 0],
    )
    .unwrap_err();
    assert_eq!(err, RouterError::BindLp);
}

#[test]
fn operate_pool_a_does_not_move_pool_b_through_router() {
    let mut vault = Vault::new();
    let pa = id(1);
    let pb = id(2);
    let user = id(9);
    let pool_a = Router::initialize_weighted_pool(
        &mut vault,
        pa,
        [id(10), id(11), id(12)],
        [id(20), id(21), id(22)],
        id(30),
        [5000, 3000, 2000],
        30,
    )
    .unwrap();
    let _pool_b = Router::initialize_weighted_pool(
        &mut vault,
        pb,
        [id(13), id(14), id(15)],
        [id(23), id(24), id(25)],
        id(31),
        [8000, 1000, 1000],
        0,
    )
    .unwrap();
    for m in [10u8, 11, 12, 13, 14, 15] {
        vault.credit_user(user, id(m), 100_000_000).unwrap();
    }
    Router::add_liquidity_proportional(
        &mut vault,
        pa,
        &pool_a,
        user,
        [id(20), id(21), id(22)],
        id(30),
        [5_000_000, 3_000_000, 2_000_000],
        1,
    )
    .unwrap();
    Router::add_liquidity_proportional(
        &mut vault,
        pb,
        &_pool_b,
        user,
        [id(23), id(24), id(25)],
        id(31),
        [8_000_000, 1_000_000, 1_000_000],
        1,
    )
    .unwrap();
    let b_before = vault.reserves(pb).unwrap();
    Router::swap_exact_in(
        &mut vault,
        pa,
        &pool_a,
        user,
        [id(20), id(21), id(22)],
        0,
        1,
        10_000,
        1,
    )
    .unwrap();
    assert_eq!(vault.reserves(pb).unwrap(), b_before);
}

#[test]
fn zero_bpt_user_cannot_remove() {
    let mut f = setup([5000, 3000, 2000], 30);
    Router::add_liquidity_proportional(
        &mut f.vault,
        f.pool_id,
        &f.pool,
        f.user,
        f.vault_acc,
        f.lp,
        [5_000_000, 3_000_000, 2_000_000],
        1,
    )
    .unwrap();
    let stranger = id(8);
    assert_eq!(f.vault.user_balance(stranger, f.lp), 0);
    let reserves_before = f.vault.reserves(f.pool_id).unwrap();
    let err = Router::remove_liquidity_proportional(
        &mut f.vault,
        f.pool_id,
        &f.pool,
        stranger,
        f.vault_acc,
        f.lp,
        1,
        [0, 0, 0],
    )
    .unwrap_err();
    assert_eq!(err, RouterError::Vault(three_amm_vault::VaultError::InsufficientUser));
    assert_eq!(f.vault.reserves(f.pool_id).unwrap(), reserves_before);
    assert_eq!(f.vault.user_balance(f.user, f.lp), f.vault.lp_supply(f.pool_id).unwrap());
}

#[test]
fn vol_aware_swap_through_router_matches_aggregator_quote() {
    let mut f = setup([5000, 3000, 2000], 30);
    Router::add_liquidity_proportional(
        &mut f.vault,
        f.pool_id,
        &f.pool,
        f.user,
        f.vault_acc,
        f.lp,
        [5_000_000, 3_000_000, 2_000_000],
        1,
    )
    .unwrap();
    let amount_in = 10_000u64;
    let vol = 500u64;
    let quote = Router::quote_exact_in(
        &f.vault, f.pool_id, &f.pool, 0, 1, amount_in, vol,
    )
    .unwrap();
    let fee = fee_bps_for_volatility(30, vol).unwrap();
    let r = f.vault.reserves(f.pool_id).unwrap();
    let math = swap_out_given_in_weighted(r[0], 5000, r[1], 3000, amount_in, fee).unwrap();
    assert_eq!(quote, math);

    let out = Router::swap_exact_in_vol(
        &mut f.vault,
        f.pool_id,
        &f.pool,
        f.user,
        f.vault_acc,
        0,
        1,
        amount_in,
        quote,
        vol,
        None,
        0,
    )
    .unwrap();
    assert_eq!(out, quote);
    assert_eq!(f.vault.reserves(f.pool_id).unwrap()[2], r[2]);
}

#[test]
fn virtual_softening_worsens_reverse_then_converges_via_router() {
    let mut f = setup([5000, 3000, 2000], 30);
    Router::add_liquidity_proportional(
        &mut f.vault,
        f.pool_id,
        &f.pool,
        f.user,
        f.vault_acc,
        f.lp,
        [5_000_000, 3_000_000, 2_000_000],
        1,
    )
    .unwrap();
    let mut book = SofteningBook::new();
    let amount_in = 10_000u64;
    let out = Router::swap_exact_in_vol(
        &mut f.vault,
        f.pool_id,
        &f.pool,
        f.user,
        f.vault_acc,
        0,
        1,
        amount_in,
        1,
        0,
        Some(&mut book),
        DEFAULT_SOFTENING_STEPS,
    )
    .unwrap();
    assert_eq!(f.vault.reserves(f.pool_id).unwrap()[2], 2_000_000);

    let virt_rev = Router::quote_reverse(
        &f.vault, f.pool_id, &f.pool, &book, 1, 0, out, 30,
    )
    .unwrap();
    let real_rev = Router::quote_reverse_real(
        &f.vault, f.pool_id, &f.pool, 1, 0, out, 30,
    )
    .unwrap();
    assert!(virt_rev <= real_rev);
    assert!(virt_rev < real_rev);

    book.converge(f.pool_id);
    let virt_after = Router::quote_reverse(
        &f.vault, f.pool_id, &f.pool, &book, 1, 0, out, 30,
    )
    .unwrap();
    assert_eq!(virt_after, real_rev);
}

#[test]
fn user_b_with_zero_lp_cannot_exit_user_a_liquidity() {
    let mut vault = Vault::new();
    let pool_id = id(1);
    let mints = [id(10), id(11), id(12)];
    let vault_acc = [id(20), id(21), id(22)];
    let lp = id(30);
    let user_a = id(9);
    let user_b = id(8);
    let pool = Router::initialize_weighted_pool(
        &mut vault, pool_id, mints, vault_acc, lp, [5000, 3000, 2000], 30,
    )
    .unwrap();
    for m in mints {
        vault.credit_user(user_a, m, 1_000_000_000).unwrap();
        vault.credit_user(user_b, m, 1_000_000_000).unwrap();
    }
    let minted = Router::add_liquidity_proportional(
        &mut vault,
        pool_id,
        &pool,
        user_a,
        vault_acc,
        lp,
        [5_000_000, 3_000_000, 2_000_000],
        1,
    )
    .unwrap();
    assert_eq!(vault.user_balance(user_a, lp), minted);
    assert_eq!(vault.user_balance(user_b, lp), 0);
    let reserves_before = vault.reserves(pool_id).unwrap();
    let a_tokens_before = [
        vault.user_balance(user_a, mints[0]),
        vault.user_balance(user_a, mints[1]),
        vault.user_balance(user_a, mints[2]),
    ];
    let err = Router::remove_liquidity_proportional(
        &mut vault,
        pool_id,
        &pool,
        user_b,
        vault_acc,
        lp,
        minted,
        [0, 0, 0],
    )
    .unwrap_err();
    assert_eq!(err, RouterError::Vault(three_amm_vault::VaultError::InsufficientUser));
    assert_eq!(vault.reserves(pool_id).unwrap(), reserves_before);
    assert_eq!(vault.user_balance(user_a, lp), minted);
    assert_eq!(vault.user_balance(user_b, lp), 0);
    assert_eq!(
        [
            vault.user_balance(user_a, mints[0]),
            vault.user_balance(user_a, mints[1]),
            vault.user_balance(user_a, mints[2]),
        ],
        a_tokens_before
    );
}
