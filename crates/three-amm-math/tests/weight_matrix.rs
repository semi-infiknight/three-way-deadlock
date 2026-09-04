//! Full weighted-pool / swap matrix. Calls shipped functions only.

use three_amm_math::{
    apply_swap, apply_swap_weighted, equal_weights, invariant, invariant_ge, ln_invariant,
    ln_invariant_ge, swap_out_given_in, swap_out_given_in_weighted, validate_weights,
    weighted_spot_e9, FEE_DENOM, MIN_WEIGHT, WEIGHT_DENOM,
};

const PAIRS: [(usize, usize); 6] = [(0, 1), (0, 2), (1, 0), (1, 2), (2, 0), (2, 1)];

struct Mix {
    name: &'static str,
    w: [u16; 3],
    r: [u64; 3],
}

/// Reserves proportional to weights so spots start near 1.
const MIXES: &[Mix] = &[
    Mix {
        name: "equal-3333",
        w: [3333, 3333, 3334],
        r: [3_333_000, 3_333_000, 3_334_000],
    },
    Mix {
        name: "50/30/20",
        w: [5000, 3000, 2000],
        r: [5_000_000, 3_000_000, 2_000_000],
    },
    Mix {
        name: "30/50/20",
        w: [3000, 5000, 2000],
        r: [3_000_000, 5_000_000, 2_000_000],
    },
    Mix {
        name: "20/30/50",
        w: [2000, 3000, 5000],
        r: [2_000_000, 3_000_000, 5_000_000],
    },
    Mix {
        name: "80/10/10",
        w: [8000, 1000, 1000],
        r: [8_000_000, 1_000_000, 1_000_000],
    },
    Mix {
        name: "10/80/10",
        w: [1000, 8000, 1000],
        r: [1_000_000, 8_000_000, 1_000_000],
    },
    Mix {
        name: "10/10/80",
        w: [1000, 1000, 8000],
        r: [1_000_000, 1_000_000, 8_000_000],
    },
    Mix {
        name: "40/40/20",
        w: [4000, 4000, 2000],
        r: [4_000_000, 4_000_000, 2_000_000],
    },
    Mix {
        name: "70/20/10",
        w: [7000, 2000, 1000],
        r: [7_000_000, 2_000_000, 1_000_000],
    },
    Mix {
        name: "60/25/15",
        w: [6000, 2500, 1500],
        r: [6_000_000, 2_500_000, 1_500_000],
    },
    Mix {
        name: "min-A 1/1/98",
        w: [100, 100, 9800],
        r: [1_000_000, 1_000_000, 98_000_000],
    },
    Mix {
        name: "min-B 1/98/1",
        w: [100, 9800, 100],
        r: [1_000_000, 98_000_000, 1_000_000],
    },
    Mix {
        name: "min-C 98/1/1",
        w: [9800, 100, 100],
        r: [98_000_000, 1_000_000, 1_000_000],
    },
];

fn third(i: usize, o: usize) -> usize {
    3 - i - o
}

fn dx_for(rin: u64) -> u64 {
    (rin / 500).max(2_000)
}

#[test]
fn equal_weights_are_valid_and_sum_to_denom() {
    let w = equal_weights();
    assert_eq!(w, [3333, 3333, 3334]);
    assert!(validate_weights(w));
    assert_eq!(
        w[0] as u32 + w[1] as u32 + w[2] as u32,
        WEIGHT_DENOM as u32
    );
    assert!(w.iter().all(|&x| x >= MIN_WEIGHT));
}

#[test]
fn validate_weights_rejects_invalid_mixes() {
    let bad: &[([u16; 3], &str)] = &[
        ([3333, 3333, 3333], "sum 9999"),
        ([3334, 3333, 3334], "sum 10001"),
        ([0, 5000, 5000], "zero"),
        ([99, 4951, 4950], "below MIN_WEIGHT"),
        ([50, 50, 9900], "below floor even if sum ok"),
        ([10000, 0, 0], "100% one asset"),
        ([1, 1, 9998], "tiny weights"),
        ([5000, 5000, 0], "zero C"),
        ([WEIGHT_DENOM, 0, 0], "denom on A"),
    ];
    for (w, why) in bad {
        assert!(!validate_weights(*w), "{why}: {w:?} should be invalid");
    }
}

#[test]
fn every_catalogued_mix_is_valid() {
    for m in MIXES {
        assert!(validate_weights(m.w), "{}", m.name);
        assert_eq!(
            m.w[0] as u32 + m.w[1] as u32 + m.w[2] as u32,
            WEIGHT_DENOM as u32,
            "{}",
            m.name
        );
    }
}

#[test]
fn matrix_all_mixes_all_pairs_fee_0_and_30() {
    let mut cases = 0u32;
    for m in MIXES {
        let v0 = ln_invariant(m.r, m.w).expect(m.name);
        for fee in [0u64, 30] {
            for (tin, tout) in PAIRS {
                let dx = dx_for(m.r[tin]);
                let (next, out) = apply_swap_weighted(m.r, m.w, tin, tout, dx, fee)
                    .unwrap_or_else(|| panic!("{} {}→{} fee={fee} dx={dx}", m.name, tin, tout));
                assert!(out > 0, "{} out=0", m.name);
                assert!(out < m.r[tout], "{} drained out", m.name);
                assert_eq!(
                    next[third(tin, tout)],
                    m.r[third(tin, tout)],
                    "{} third reserve moved",
                    m.name
                );
                assert_eq!(next[tin], m.r[tin] + dx, "{}", m.name);
                assert_eq!(next[tout], m.r[tout] - out, "{}", m.name);
                let v1 = ln_invariant(next, m.w).expect(m.name);
                assert!(
                    ln_invariant_ge(v1, v0),
                    "{} ln-invariant dropped v0={v0} v1={v1} {}→{} fee={fee}",
                    m.name,
                    tin,
                    tout
                );
                if fee > 0 {
                    assert!(
                        v1 + 1_000_000 > v0,
                        "{} fee should not shrink V",
                        m.name
                    );
                }
                if m.w[tin] == m.w[tout] {
                    let cpmm = swap_out_given_in(m.r[tin], m.r[tout], dx, fee).unwrap();
                    assert_eq!(out, cpmm, "{} equal-weight pair must match product", m.name);
                }
                let quote = swap_out_given_in_weighted(
                    m.r[tin], m.w[tin], m.r[tout], m.w[tout], dx, fee,
                )
                .unwrap();
                assert_eq!(out, quote, "{} quote != apply", m.name);
                cases += 1;
            }
        }
    }
    // 13 mixes × 6 pairs × 2 fees
    assert_eq!(cases, 13 * 6 * 2);
}

#[test]
fn unequal_pair_on_near_equal_weights_uses_pow_not_blind_cpmm() {
    let w = equal_weights(); // 3333 vs 3334 on A-C and B-C
    let r = [3_000_000u64, 3_000_000, 3_000_000];
    let dx = 10_000u64;
    let weighted = swap_out_given_in_weighted(r[0], w[0], r[2], w[2], dx, 0).unwrap();
    let cpmm = swap_out_given_in(r[0], r[2], dx, 0).unwrap();
    // w_in != w_out so the product shortcut is not taken; values stay close.
    assert_ne!(w[0], w[2]);
    let err = (weighted as i128 - cpmm as i128).abs();
    assert!(err <= 5, "near-equal weights should be near CPMM err={err}");
}

#[test]
fn apply_swap_weighted_rejects_bad_indices_and_zero_weights() {
    let r = [1_000_000u64, 2_000_000, 3_000_000];
    let w = [5000u16, 3000, 2000];
    assert!(apply_swap_weighted(r, w, 0, 0, 1_000, 0).is_none());
    assert!(apply_swap_weighted(r, w, 0, 3, 1_000, 0).is_none());
    assert!(apply_swap_weighted(r, w, 3, 1, 1_000, 0).is_none());
    assert!(apply_swap_weighted(r, w, 1, 1, 1_000, 0).is_none());
    assert!(apply_swap_weighted(r, [0, 5000, 5000], 0, 1, 1_000, 0).is_none());
    assert!(apply_swap_weighted(r, [4000, 3000, 2000], 0, 1, 1_000, 0).is_none()); // sum 9000
    assert!(apply_swap_weighted(r, w, 0, 1, 0, 0).is_none());
}

#[test]
fn apply_swap_weighted_does_not_enforce_min_weight_floor() {
    // Init uses validate_weights; swap only checks nonzero + sum. Pin the split.
    let r = [1_000_000u64, 1_000_000, 98_000_000];
    let w = [50u16, 50, 9900];
    assert!(!validate_weights(w));
    assert!(apply_swap_weighted(r, w, 0, 1, 10_000, 0).is_some());
}

#[test]
fn swap_out_rejects_zero_and_invalid_fee() {
    assert!(swap_out_given_in_weighted(0, 5000, 1_000_000, 3000, 10, 0).is_none());
    assert!(swap_out_given_in_weighted(1_000_000, 5000, 0, 3000, 10, 0).is_none());
    assert!(swap_out_given_in_weighted(1_000_000, 5000, 1_000_000, 3000, 0, 0).is_none());
    assert!(swap_out_given_in_weighted(1_000_000, 0, 1_000_000, 5000, 10, 0).is_none());
    assert!(swap_out_given_in_weighted(1_000_000, 5000, 1_000_000, 0, 10, 0).is_none());
    assert!(swap_out_given_in_weighted(1_000_000, 5000, 1_000_000, 3000, 10, FEE_DENOM).is_none());
    assert!(swap_out_given_in_weighted(1_000_000, 5000, 1_000_000, 3000, 1, FEE_DENOM - 1).is_none());
}

#[test]
fn weighted_spot_matches_balancer_ratio() {
    let r = [8_000_000u64, 1_000_000, 1_000_000];
    let w = [8000u16, 1000, 1000];
    // (Rq/wq) / (Rb/wb) * 1e9
    let p = weighted_spot_e9(r[0], w[0], r[1], w[1]).unwrap();
    assert_eq!(p, 1_000_000_000);
    assert!(weighted_spot_e9(0, w[0], r[1], w[1]).is_none());
    assert!(weighted_spot_e9(r[0], 0, r[1], w[1]).is_none());
    assert!(weighted_spot_e9(r[0], w[0], r[1], 0).is_none());
}

#[test]
fn swap_moves_all_three_weighted_spots() {
    for m in MIXES {
        let (next, _) = apply_swap_weighted(m.r, m.w, 0, 1, dx_for(m.r[0]), 0).unwrap();
        let p01 = weighted_spot_e9(m.r[0], m.w[0], m.r[1], m.w[1]).unwrap();
        let p02 = weighted_spot_e9(m.r[0], m.w[0], m.r[2], m.w[2]).unwrap();
        let p12 = weighted_spot_e9(m.r[1], m.w[1], m.r[2], m.w[2]).unwrap();
        let q01 = weighted_spot_e9(next[0], m.w[0], next[1], m.w[1]).unwrap();
        let q02 = weighted_spot_e9(next[0], m.w[0], next[2], m.w[2]).unwrap();
        let q12 = weighted_spot_e9(next[1], m.w[1], next[2], m.w[2]).unwrap();
        assert_ne!(p01, q01, "{} A/B", m.name);
        assert_ne!(p02, q02, "{} A/C must move (C amount fixed)", m.name);
        assert_ne!(p12, q12, "{} B/C must move (C amount fixed)", m.name);
        assert_eq!(next[2], m.r[2], "{}", m.name);
    }
}

#[test]
fn two_pool_router_does_not_couple_weighted_third() {
    let w = [5000u16, 3000, 2000];
    let r = [5_000_000u64, 3_000_000, 2_000_000];
    let p_ac = weighted_spot_e9(r[0], w[0], r[2], w[2]).unwrap();
    // Fake two 2-pools: A-B swap leaves A-C pool untouched.
    assert_eq!(
        p_ac,
        weighted_spot_e9(r[0], w[0], r[2], w[2]).unwrap(),
        "standalone A-C pool"
    );
    let (next, _) = apply_swap_weighted(r, w, 0, 1, 50_000, 0).unwrap();
    assert_ne!(
        p_ac,
        weighted_spot_e9(next[0], w[0], next[2], w[2]).unwrap()
    );
}

#[test]
fn skim_third_reserve_fails_ln_invariant() {
    let w = [5000u16, 3000, 2000];
    let r = [5_000_000u64, 3_000_000, 2_000_000];
    let v0 = ln_invariant(r, w).unwrap();
    let stolen = [5_000_100u64, 2_990_000, 1_990_000];
    let vs = ln_invariant(stolen, w).unwrap();
    assert!(!ln_invariant_ge(vs, v0));
    let (honest, _) = apply_swap_weighted(r, w, 0, 1, 10_000, 0).unwrap();
    assert!(ln_invariant_ge(ln_invariant(honest, w).unwrap(), v0));
}

#[test]
fn fees_grow_ln_invariant_on_unequal_weights() {
    let w = [8000u16, 1000, 1000];
    let r = [8_000_000u64, 1_000_000, 1_000_000];
    let v0 = ln_invariant(r, w).unwrap();
    let (next, _) = apply_swap_weighted(r, w, 0, 1, 80_000, 30).unwrap();
    let v1 = ln_invariant(next, w).unwrap();
    assert!(v1 > v0, "v0={v0} v1={v1}");
}

#[test]
fn weighted_cannot_empty_out_reserve() {
    let w = [5000u16, 3000, 2000];
    let r = [1_000u64, 1_000, 1_000];
    let huge = apply_swap_weighted(r, w, 0, 1, 1_000_000_000, 0);
    if let Some((n, dy)) = huge {
        assert!(dy < r[1]);
        assert!(n[1] > 0);
    }
    let (n, dy) = apply_swap_weighted([1_000_000, 1_000_000, 1_000_000], w, 0, 1, 1_000_000, 0)
        .unwrap();
    assert!(dy < 1_000_000);
    assert!(n[1] > 0);
}

#[test]
fn triangle_unequal_weights_direct_beats_or_matches_path() {
    // With fees=0, A→C should be at least as good as A→B→C up to floor dust.
    let w = [5000u16, 3000, 2000];
    let start = [5_000_000u64, 3_000_000, 2_000_000];
    let dx = 10_000u64;
    let (s1, db) = apply_swap_weighted(start, w, 0, 1, dx, 0).unwrap();
    let (_s2, dc_path) = apply_swap_weighted(s1, w, 1, 2, db, 0).unwrap();
    let (_d, dc_direct) = apply_swap_weighted(start, w, 0, 2, dx, 0).unwrap();
    assert!(
        dc_direct + 3 >= dc_path,
        "direct {dc_direct} path {dc_path}"
    );
}

#[test]
fn equal_weight_apply_matches_product_k_and_weighted() {
    let r = [1_000_000u64, 2_000_000, 3_000_000];
    let w = [4000u16, 4000, 2000]; // A/B equal, C different
    let (prod, out_p) = apply_swap(r, 0, 1, 10_000, 0).unwrap();
    let (wgt, out_w) = apply_swap_weighted(r, w, 0, 1, 10_000, 0).unwrap();
    assert_eq!(out_p, out_w);
    assert_eq!(prod, wgt);
    assert!(invariant_ge(
        invariant(prod[0], prod[1], prod[2]),
        invariant(r[0], r[1], r[2])
    ));
}

#[test]
fn reverse_swap_does_not_restore_exactly_after_fees() {
    let w = [5000u16, 3000, 2000];
    let r = [5_000_000u64, 3_000_000, 2_000_000];
    let dx = 20_000u64;
    let (mid, out) = apply_swap_weighted(r, w, 0, 1, dx, 30).unwrap();
    let (back, recovered) = apply_swap_weighted(mid, w, 1, 0, out, 30).unwrap();
    assert!(
        recovered < dx,
        "round-trip with 30 bps must return less A than spent recovered={recovered} dx={dx}"
    );
    assert!(back[0] > r[0], "fee residue stays in the pool");
    assert_eq!(back[2], r[2]);
}

#[test]
fn all_six_directions_on_80_10_10_and_50_30_20() {
    for m in MIXES.iter().filter(|m| m.name == "80/10/10" || m.name == "50/30/20") {
        for (tin, tout) in PAIRS {
            let (next, out) =
                apply_swap_weighted(m.r, m.w, tin, tout, 8_000, 0).expect(m.name);
            assert!(out > 0);
            assert_eq!(next[third(tin, tout)], m.r[third(tin, tout)]);
        }
    }
}
