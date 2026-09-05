//! Two pools in one Vault: mutating A cannot change B's attributed reserves.

use three_amm_vault::Vault;

fn id(n: u8) -> [u8; 32] {
    [n; 32]
}

#[test]
fn two_pools_settle_a_leaves_b_bitwise_unchanged() {
    let mut v = Vault::new();
    let pa = id(1);
    let pb = id(2);
    v.register_pool(pa, [id(10), id(11), id(12)], [id(20), id(21), id(22)], id(30))
        .unwrap();
    v.register_pool(pb, [id(13), id(14), id(15)], [id(23), id(24), id(25)], id(31))
        .unwrap();

    v.unlock().unwrap();
    v.settle(pa, [5_000_000, 3_000_000, 2_000_000], 1_000).unwrap();
    v.settle(pb, [8_000_000, 1_000_000, 1_000_000], 2_000).unwrap();
    v.lock();

    let b_before = v.reserves(pb).unwrap();
    let b_lp_before = v.lp_supply(pb).unwrap();
    let b_bytes_before = b_before;

    v.unlock().unwrap();
    v.settle(pa, [5_010_000, 2_990_057, 2_000_000], 1_000).unwrap();
    v.lock();

    let b_after = v.reserves(pb).unwrap();
    assert_eq!(b_after, b_bytes_before, "pool B reserves must be bitwise unchanged");
    assert_eq!(v.lp_supply(pb).unwrap(), b_lp_before);
    assert_eq!(v.reserves(pa).unwrap(), [5_010_000, 2_990_057, 2_000_000]);
}

#[test]
fn shared_mint_index_still_isolates_pool_b() {
    let mut v = Vault::new();
    let pa = id(1);
    let pb = id(2);
    // Same USDC mint (id 10) in both pools — Vault totals combine; indexes do not.
    v.register_pool(pa, [id(10), id(11), id(12)], [id(20), id(21), id(22)], id(30))
        .unwrap();
    v.register_pool(pb, [id(10), id(16), id(17)], [id(26), id(27), id(28)], id(31))
        .unwrap();
    v.unlock().unwrap();
    v.settle(pa, [1_000, 2_000, 3_000], 10).unwrap();
    v.settle(pb, [9_000, 4_000, 5_000], 20).unwrap();
    v.lock();
    let b_before = v.reserves(pb).unwrap();
    v.unlock().unwrap();
    v.settle(pa, [1_500, 1_800, 3_000], 10).unwrap();
    v.lock();
    assert_eq!(v.reserves(pb).unwrap(), b_before);
}

#[test]
fn fake_vault_in_fails_shipped_bind() {
    let mut v = Vault::new();
    let p = id(1);
    v.register_pool(p, [id(10), id(11), id(12)], [id(20), id(21), id(22)], id(30))
        .unwrap();
    let fake_in = id(99);
    assert!(
        !v.vaults_match_pool(p, &fake_in, &id(21), &id(22)),
        "attacker vault_in must not bind"
    );
    assert!(v.vaults_match_pool(p, &id(20), &id(21), &id(22)));
}

#[test]
fn fake_lp_mint_fails_shipped_bind() {
    let mut v = Vault::new();
    let p = id(1);
    v.register_pool(p, [id(10), id(11), id(12)], [id(20), id(21), id(22)], id(30))
        .unwrap();
    let fake_lp = id(0xff);
    assert!(!v.lp_matches_pool(p, &fake_lp));
    assert!(v.lp_matches_pool(p, &id(30)));
}

#[test]
fn mint_burn_bpt_do_not_touch_mint_totals() {
    let mut v = Vault::new();
    let p = id(1);
    let user = id(9);
    v.register_pool(p, [id(10), id(11), id(12)], [id(20), id(21), id(22)], id(30))
        .unwrap();
    v.unlock().unwrap();
    v.mint_bpt(p, user, 100).unwrap();
    assert_eq!(v.user_balance(user, id(30)), 100);
    assert_eq!(v.mint_total(id(30)), 0, "BPT is not vault-custodied pool inventory");
    v.burn_bpt(p, user, 40).unwrap();
    assert_eq!(v.user_balance(user, id(30)), 60);
    assert!(v.burn_bpt(p, id(8), 1).is_err());
    v.lock();
}

#[test]
fn settle_rejected_while_locked() {
    let mut v = Vault::new();
    let p = id(1);
    v.register_pool(p, [id(10), id(11), id(12)], [id(20), id(21), id(22)], id(30))
        .unwrap();
    assert!(v.settle(p, [1, 1, 1], 1).is_err());
}
