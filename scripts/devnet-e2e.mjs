/**
 * Deploy-target client: Pinocchio three_amm_pio (1-byte discriminators).
 * Seeds three mints, LP, swap twice. Same quote math as three-amm-math.
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import {
  ComputeBudgetProgram,
  Connection,
  Keypair,
  PublicKey,
  SystemProgram,
  Transaction,
  sendAndConfirmTransaction,
  TransactionInstruction,
} from "@solana/web3.js";
import {
  TOKEN_PROGRAM_ID,
  createMint,
  createAccount,
  mintTo,
  getAccount,
} from "@solana/spl-token";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "..");
const PROGRAM_ID = new PublicKey("8SMAn5rTDFCaLKAdNd1fXckxm1eHdZGDkZP7JqhLUpbP");
const POOL_SEED = Buffer.from("pool");
const VAULT_SEED = Buffer.from("vault");
const VAULT_STATE_SEED = Buffer.from("vault-state");
const LP_SEED = Buffer.from("lp");
const SWAP_COMPUTE_UNITS = 1_000_000;

function u64le(n) {
  const b = Buffer.alloc(8);
  b.writeBigUInt64LE(BigInt(n));
  return b;
}

function loadKeypair(p) {
  const raw = JSON.parse(fs.readFileSync(p, "utf8"));
  return Keypair.fromSecretKey(Uint8Array.from(raw));
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function sendTx(connection, tx, signers) {
  tx.instructions.unshift(
    ComputeBudgetProgram.setComputeUnitLimit({ units: SWAP_COMPUTE_UNITS })
  );
  let last;
  for (let i = 0; i < 8; i++) {
    try {
      return await sendAndConfirmTransaction(connection, tx, signers, {
        commitment: "confirmed",
        skipPreflight: false,
      });
    } catch (e) {
      last = e;
      const msg = String(e?.message || e);
      if (!msg.includes("429") && !msg.includes("rate")) throw e;
      const wait = 1500 * (i + 1);
      console.log("rpc 429, retry in", wait, "ms");
      await sleep(wait);
    }
  }
  throw last;
}

export function swapOutGivenIn(rin, rout, amountIn, feeBps) {
  const FEE_DENOM = 10000n;
  const dx = (amountIn * (FEE_DENOM - feeBps)) / FEE_DENOM;
  return (rout * dx) / (rin + dx);
}

/** Quote via the shipped `three-amm-math` crate (same fn the program calls). */
export function swapOutGivenInWeighted(rin, win, rout, wout, amountIn, feeBps) {
  const bin = path.join(ROOT, "target/release/consumer");
  const out = execFileSync(
    bin,
    [
      "quote",
      String(rin),
      String(win),
      String(rout),
      String(wout),
      String(amountIn),
      String(feeBps),
    ],
    { encoding: "utf8" }
  ).trim();
  return BigInt(out);
}

async function confirmGetTransaction(connection, sig) {
  let last;
  for (let i = 0; i < 20; i++) {
    try {
      const tx = await connection.getTransaction(sig, {
        commitment: "confirmed",
        maxSupportedTransactionVersion: 0,
      });
      if (tx) {
        if (tx.meta?.err) throw new Error("tx err " + JSON.stringify(tx.meta.err));
        return tx;
      }
    } catch (e) {
      last = e;
      const msg = String(e?.message || e);
      if (msg.includes("tx err")) throw e;
      if (!msg.includes("429") && !msg.includes("rate")) throw e;
    }
    await sleep(1500 * (i + 1));
  }
  throw last || new Error("getTransaction missing " + sig);
}

export function proportionalAdd(reserves, amounts, supply) {
  let lp = (amounts[0] * supply) / reserves[0];
  for (let i = 1; i < 3; i++) {
    const s = (amounts[i] * supply) / reserves[i];
    if (s < lp) lp = s;
  }
  return {
    used: [
      (lp * reserves[0]) / supply,
      (lp * reserves[1]) / supply,
      (lp * reserves[2]) / supply,
    ],
    lp,
  };
}

async function main() {
  const rpc = process.env.RPC_URL || "https://api.devnet.solana.com";
  const walletPath =
    process.env.WALLET || path.join(os.homedir(), ".config/solana/id.json");
  const payer = loadKeypair(walletPath);
  const connection = new Connection(rpc, "confirmed");
  const soPath = path.join(ROOT, "target/deploy/three_amm_pio.so");
  if (!fs.existsSync(soPath)) {
    throw new Error(`missing ${soPath}; run cargo build-sbf`);
  }

  const bal = await connection.getBalance(payer.publicKey);
  console.log("rpc", rpc);
  console.log("payer", payer.publicKey.toBase58(), "lamports", bal);
  console.log("program", PROGRAM_ID.toBase58());

  const mintA = await createMint(connection, payer, payer.publicKey, null, 6);
  const mintB = await createMint(connection, payer, payer.publicKey, null, 6);
  const mintC = await createMint(connection, payer, payer.publicKey, null, 6);
  console.log("mints", mintA.toBase58(), mintB.toBase58(), mintC.toBase58());

  const [vaultState] = PublicKey.findProgramAddressSync(
    [VAULT_STATE_SEED],
    PROGRAM_ID
  );
  const [pool] = PublicKey.findProgramAddressSync(
    [POOL_SEED, mintA.toBuffer(), mintB.toBuffer(), mintC.toBuffer()],
    PROGRAM_ID
  );
  const [vaultA] = PublicKey.findProgramAddressSync(
    [VAULT_SEED, pool.toBuffer(), mintA.toBuffer()],
    PROGRAM_ID
  );
  const [vaultB] = PublicKey.findProgramAddressSync(
    [VAULT_SEED, pool.toBuffer(), mintB.toBuffer()],
    PROGRAM_ID
  );
  const [vaultC] = PublicKey.findProgramAddressSync(
    [VAULT_SEED, pool.toBuffer(), mintC.toBuffer()],
    PROGRAM_ID
  );
  const [lpMint] = PublicKey.findProgramAddressSync(
    [LP_SEED, pool.toBuffer()],
    PROGRAM_ID
  );
  console.log("vault_state", vaultState.toBase58());
  console.log("pool", pool.toBase58());
  console.log("cu_limit", SWAP_COMPUTE_UNITS);

  const userA = await createAccount(connection, payer, mintA, payer.publicKey);
  const userB = await createAccount(connection, payer, mintB, payer.publicKey);
  const userC = await createAccount(connection, payer, mintC, payer.publicKey);
  await mintTo(connection, payer, mintA, userA, payer, 1_000_000_000n);
  await mintTo(connection, payer, mintB, userB, payer, 1_000_000_000n);
  await mintTo(connection, payer, mintC, userC, payer, 1_000_000_000n);

  function u16le(n) {
    const b = Buffer.alloc(2);
    b.writeUInt16LE(n);
    return b;
  }
  // tag 0 + fee 30 bps + Balancer weights 50/30/20 (explicit, unequal)
  const WEIGHTS = [5000, 3000, 2000];
  const initData = Buffer.concat([
    Buffer.from([0]),
    u64le(30),
    u16le(WEIGHTS[0]),
    u16le(WEIGHTS[1]),
    u16le(WEIGHTS[2]),
  ]);
  const initIx = new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [
      { pubkey: payer.publicKey, isSigner: true, isWritable: true },
      { pubkey: payer.publicKey, isSigner: true, isWritable: false },
      { pubkey: mintA, isSigner: false, isWritable: false },
      { pubkey: mintB, isSigner: false, isWritable: false },
      { pubkey: mintC, isSigner: false, isWritable: false },
      { pubkey: vaultState, isSigner: false, isWritable: true },
      { pubkey: pool, isSigner: false, isWritable: true },
      { pubkey: vaultA, isSigner: false, isWritable: true },
      { pubkey: vaultB, isSigner: false, isWritable: true },
      { pubkey: vaultC, isSigner: false, isWritable: true },
      { pubkey: lpMint, isSigner: false, isWritable: true },
      { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data: initData,
  });
  const initSig = await sendTx(
    connection,
    new Transaction().add(initIx),
    [payer]
  );
  console.log("init", initSig);
  await confirmGetTransaction(connection, initSig);
  const poolInfo = await connection.getAccountInfo(pool, "confirmed");
  const wOnChain = [
    poolInfo.data.readUInt16LE(4),
    poolInfo.data.readUInt16LE(6),
    poolInfo.data.readUInt16LE(8),
  ];
  console.log("weights_on_chain", wOnChain);

  const userLp = await createAccount(connection, payer, lpMint, payer.publicKey);

  // Reserves proportional to weights so spots start near 1:1.
  const addData = Buffer.concat([
    Buffer.from([1]),
    u64le(5_000_000),
    u64le(3_000_000),
    u64le(2_000_000),
    u64le(1),
  ]);
  const liqKeys = [
    { pubkey: payer.publicKey, isSigner: true, isWritable: false },
    { pubkey: vaultState, isSigner: false, isWritable: true },
    { pubkey: pool, isSigner: false, isWritable: true },
    { pubkey: vaultA, isSigner: false, isWritable: true },
    { pubkey: vaultB, isSigner: false, isWritable: true },
    { pubkey: vaultC, isSigner: false, isWritable: true },
    { pubkey: lpMint, isSigner: false, isWritable: true },
    { pubkey: userA, isSigner: false, isWritable: true },
    { pubkey: userB, isSigner: false, isWritable: true },
    { pubkey: userC, isSigner: false, isWritable: true },
    { pubkey: userLp, isSigner: false, isWritable: true },
    { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
  ];
  await sleep(1500);
  const addSig = await sendTx(
    connection,
    new Transaction().add(
      new TransactionInstruction({ programId: PROGRAM_ID, keys: liqKeys, data: addData })
    ),
    [payer]
  );
  console.log("add_liquidity", addSig);

  const amountIn = 10_000n;
  const feeBps = 30n;

  async function readVaults() {
    let last;
    for (let i = 0; i < 8; i++) {
      try {
        return {
          va: await getAccount(connection, vaultA),
          vb: await getAccount(connection, vaultB),
          vc: await getAccount(connection, vaultC),
        };
      } catch (e) {
        last = e;
        const msg = String(e?.message || e);
        if (!msg.includes("429") && !msg.includes("rate")) throw e;
        await sleep(1500 * (i + 1));
      }
    }
    throw last;
  }

  async function doSwap(label, tokenIn, tokenOut, { volOverride = 0n } = {}) {
    const { va, vb, vc } = await readVaults();
    const amounts = [va.amount, vb.amount, vc.amount];
    // volOverride 0 → base fee path (matches classic 9943 receipt). Omit override
    // only when intentionally testing measured fees.
    const quote = swapOutGivenInWeighted(
      amounts[tokenIn],
      WEIGHTS[tokenIn],
      amounts[tokenOut],
      WEIGHTS[tokenOut],
      amountIn,
      feeBps
    );
    const before = { a: va.amount.toString(), b: vb.amount.toString(), c: vc.amount.toString() };
    const swapData = Buffer.concat([
      Buffer.from([3, tokenIn, tokenOut]),
      u64le(amountIn),
      u64le(quote),
      u64le(volOverride), // 27-byte ix: explicit volatility_bps override
    ]);
    const swapKeys = [
      { pubkey: payer.publicKey, isSigner: true, isWritable: false },
      { pubkey: vaultState, isSigner: false, isWritable: true },
      { pubkey: pool, isSigner: false, isWritable: true },
      { pubkey: vaultA, isSigner: false, isWritable: true },
      { pubkey: vaultB, isSigner: false, isWritable: true },
      { pubkey: vaultC, isSigner: false, isWritable: true },
      { pubkey: userA, isSigner: false, isWritable: true },
      { pubkey: userB, isSigner: false, isWritable: true },
      { pubkey: userC, isSigner: false, isWritable: true },
      { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
    ];
    await sleep(2000);
    const sig = await sendTx(
      connection,
      new Transaction().add(
        new TransactionInstruction({ programId: PROGRAM_ID, keys: swapKeys, data: swapData })
      ),
      [payer]
    );
    const confirmed = await confirmGetTransaction(connection, sig);
    await sleep(1500);
    const { va: va2, vb: vb2, vc: vc2 } = await readVaults();
    const afterAmt = [va2.amount, vb2.amount, vc2.amount];
    const actualOut = amounts[tokenOut] - afterAmt[tokenOut];
    const third = [0, 1, 2].find((i) => i !== tokenIn && i !== tokenOut);
    const thirdUnchanged = amounts[third] === afterAmt[third];
    console.log(label, {
      sig,
      slot: confirmed.slot,
      tokenIn,
      tokenOut,
      weights: WEIGHTS,
      quote: quote.toString(),
      actualOut: actualOut.toString(),
      cu_limit: SWAP_COMPUTE_UNITS,
      cu_consumed: confirmed.meta?.computeUnitsConsumed ?? null,
      before,
      after: { a: va2.amount.toString(), b: vb2.amount.toString(), c: vc2.amount.toString() },
      thirdUnchanged,
      match: actualOut === quote,
    });
    if (actualOut !== quote) throw new Error("quote mismatch");
    if (!thirdUnchanged) throw new Error("third reserve moved");
    return sig;
  }

  const s1 = await doSwap("swap1", 0, 1); // A→B, C unchanged
  const s2 = await doSwap("swap2", 1, 2); // B→C, A unchanged
  console.log("pool", pool.toBase58());
  console.log("weights", WEIGHTS.join("/"));
  console.log("SWAP_OK", s1, s2);
}

const isMain = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  main().catch((e) => {
    console.error("E2E_FAIL", e);
    process.exit(1);
  });
}
