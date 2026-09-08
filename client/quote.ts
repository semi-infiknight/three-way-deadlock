/**
 * Off-chain quote client — same formulas as `three-amm-math`.
 * Used by the consumer check and the devnet e2e script.
 */
export const FEE_DENOM = 10_000n;

export function amountAfterFee(amountIn: bigint, feeBps: bigint): bigint {
  if (feeBps >= FEE_DENOM) throw new Error("bad fee");
  return (amountIn * (FEE_DENOM - feeBps)) / FEE_DENOM;
}

export function swapOutGivenIn(
  reserveIn: bigint,
  reserveOut: bigint,
  amountIn: bigint,
  feeBps: bigint
): bigint {
  if (reserveIn === 0n || reserveOut === 0n || amountIn === 0n) {
    throw new Error("empty");
  }
  const dx = amountAfterFee(amountIn, feeBps);
  if (dx === 0n) throw new Error("dust");
  const out = (reserveOut * dx) / (reserveIn + dx);
  if (out === 0n || out >= reserveOut) throw new Error("zero out");
  return out;
}

export function proportionalAdd(
  reserves: [bigint, bigint, bigint],
  amounts: [bigint, bigint, bigint],
  supply: bigint
): { used: [bigint, bigint, bigint]; lp: bigint } {
  if (supply === 0n) {
    throw new Error("use on-chain initial LP (cube root)");
  }
  let lp = amounts[0] * supply / reserves[0];
  for (let i = 1; i < 3; i++) {
    const s = amounts[i] * supply / reserves[i];
    if (s < lp) lp = s;
  }
  const used: [bigint, bigint, bigint] = [
    (lp * reserves[0]) / supply,
    (lp * reserves[1]) / supply,
    (lp * reserves[2]) / supply,
  ];
  return { used, lp };
}
