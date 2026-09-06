//! Fresh consumer of the shipped Router / Vault / Pool stack (not crate unit tests).
use std::env;
use three_amm_math::{
    apply_swap, apply_swap_weighted, invariant, invariant_ge, proportional_add, swap_out_given_in,
    swap_out_given_in_weighted,
};
use three_amm_router::Router;
use three_amm_vault::Vault;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("quote") {
        // quote <rin> <win> <rout> <wout> <amount_in> <fee_bps>
        let rin: u64 = args[2].parse().expect("rin");
        let win: u16 = args[3].parse().expect("win");
        let rout: u64 = args[4].parse().expect("rout");
        let wout: u16 = args[5].parse().expect("wout");
        let amount_in: u64 = args[6].parse().expect("amount_in");
        let fee_bps: u64 = args[7].parse().expect("fee_bps");
        let q = swap_out_given_in_weighted(rin, win, rout, wout, amount_in, fee_bps)
            .expect("weighted quote");
        println!("{q}");
        return;
    }

    let reserves = [1_000_000u64, 2_000_000, 3_000_000];
    let amount_in = 12_345u64;
    let fee_bps = 0u64;

    let quote = swap_out_given_in(reserves[0], reserves[1], amount_in, fee_bps)
        .expect("quote");
    let (after_swap, out) = apply_swap(reserves, 0, 1, amount_in, fee_bps).unwrap();
    assert_eq!(quote, out, "quote must match apply_swap");

    let k0 = invariant(reserves[0], reserves[1], reserves[2]);
    let k1 = invariant(after_swap[0], after_swap[1], after_swap[2]);
    assert!(
        invariant_ge(k1, k0),
        "conservation must not drop after quoted swap"
    );
    assert_eq!(after_swap[2], reserves[2], "third reserve amount unchanged");

    let (used, lp) = proportional_add(reserves, [10_000, 20_000, 30_000], 100_000)
        .expect("add");
    assert_eq!(used, [10_000, 20_000, 30_000]);
    assert!(lp > 0);

    println!("quote_out={quote}");
    println!("lp_minted={lp}");
    println!("k_before_hi={} k_before_lo={}", k0.hi, k0.lo);
    println!("k_after_hi={} k_after_lo={}", k1.hi, k1.lo);
    println!("third_reserve={}", after_swap[2]);

    let w = [8000u16, 1000, 1000];
    let rw = [8_000_000u64, 1_000_000, 1_000_000];
    let (after_w, wout) = apply_swap_weighted(rw, w, 0, 1, 8_000, 0).unwrap();
    assert_eq!(after_w[2], rw[2]);
    println!("weighted_80_10_10_out={wout}");

    v3_fifty_thirty_twenty();
    println!("CONSUMER_OK");
}

fn id(n: u8) -> [u8; 32] {
    [n; 32]
}

/// Representative 50/30/20 given-in through the shipped Router (Vault settle + Pool quote).
fn v3_fifty_thirty_twenty() {
    let mut vault = Vault::new();
    let pool_id = id(1);
    let mints = [id(10), id(11), id(12)];
    let vault_acc = [id(20), id(21), id(22)];
    let lp = id(30);
    let user = id(9);
    let weights = [5000u16, 3000, 2000];
    let fee = 30u64;
    let pool = Router::initialize_weighted_pool(
        &mut vault, pool_id, mints, vault_acc, lp, weights, fee,
    )
    .expect("init");
    for m in mints {
        vault.credit_user(user, m, 1_000_000_000).unwrap();
    }
    Router::add_liquidity_proportional(
        &mut vault,
        pool_id,
        &pool,
        user,
        vault_acc,
        lp,
        [5_000_000, 3_000_000, 2_000_000],
        1,
    )
    .unwrap();
    let r = vault.reserves(pool_id).unwrap();
    let amount_in = 10_000u64;
    let math = swap_out_given_in_weighted(r[0], weights[0], r[1], weights[1], amount_in, fee)
        .unwrap();
    let (applied, applied_out) =
        apply_swap_weighted(r, weights, 0, 1, amount_in, fee).unwrap();
    assert_eq!(math, applied_out);
    let out = Router::swap_exact_in(
        &mut vault, pool_id, &pool, user, vault_acc, 0, 1, amount_in, math,
    )
    .unwrap();
    assert_eq!(out, math, "router out must equal shipped weighted quote");
    assert_eq!(vault.reserves(pool_id).unwrap()[2], applied[2]);
    println!("v3_out={out}");
}
