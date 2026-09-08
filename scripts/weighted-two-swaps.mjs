/** Two swaps on an existing weighted 3-asset pool (public devnet). */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  ComputeBudgetProgram,
  Connection,
  Keypair,
  PublicKey,
  Transaction,
  TransactionInstruction,
} from "@solana/web3.js";
import { TOKEN_PROGRAM_ID, getAccount } from "@solana/spl-token";
import { swapOutGivenInWeighted } from "./devnet-e2e.mjs";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const PROGRAM_ID = new PublicKey("8SMAn5rTDFCaLKAdNd1fXckxm1eHdZGDkZP7JqhLUpbP");
const POOL_SEED = Buffer.from("pool");
const VAULT_SEED = Buffer.from("vault");
const WEIGHTS = [5000, 3000, 2000];
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function u64le(n) {
  const b = Buffer.alloc(8);
  b.writeBigUInt64LE(BigInt(n));
  return b;
}

function loadKeypair(p) {
  return Keypair.fromSecretKey(Uint8Array.from(JSON.parse(fs.readFileSync(p, "utf8"))));
}

async function rpcRetry(fn, label) {
  let last;
  for (let i = 0; i < 12; i++) {
    try {
      return await fn();
    } catch (e) {
      last = e;
      const msg = String(e?.message || e);
      if (!msg.includes("429") && !msg.includes("Too many") && !msg.includes("rate")) {
        throw e;
      }
      const wait = 2500 * (i + 1);
      console.log("rpc 429", label, "retry in", wait, "ms");
      await sleep(wait);
    }
  }
  throw last;
}

async function confirmGetTransaction(connection, sig) {
  let last;
  for (let i = 0; i < 24; i++) {
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
    }
    await sleep(2500 * (i + 1 > 6 ? 6 : i + 1));
  }
  throw last || new Error("getTransaction missing " + sig);
}

async function main() {
  const rpc = process.env.RPC_URL || "https://api.devnet.solana.com";
  const mintA = new PublicKey(process.env.MINT_A);
  const mintB = new PublicKey(process.env.MINT_B);
  const mintC = new PublicKey(process.env.MINT_C);
  const payer = loadKeypair(
    process.env.WALLET || path.join(os.homedir(), ".config/solana/id.json")
  );
  const connection = new Connection(rpc, {
    commitment: "confirmed",
    disableRetryOnRateLimit: true,
  });

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
  console.log("rpc", rpc);
  console.log("pool", pool.toBase58());
  console.log("weights", WEIGHTS.join("/"));

  async function userAta(mint) {
    const r = await rpcRetry(
      () => connection.getParsedTokenAccountsByOwner(payer.publicKey, { mint }),
      "ata " + mint.toBase58()
    );
    if (!r.value.length) throw new Error("no token account for " + mint.toBase58());
    return r.value[0].pubkey;
  }

  const userA = await userAta(mintA);
  await sleep(2000);
  const userB = await userAta(mintB);
  await sleep(2000);
  const userC = await userAta(mintC);

  async function readVaults() {
    const va = await rpcRetry(() => getAccount(connection, vaultA), "vaultA");
    await sleep(1500);
    const vb = await rpcRetry(() => getAccount(connection, vaultB), "vaultB");
    await sleep(1500);
    const vc = await rpcRetry(() => getAccount(connection, vaultC), "vaultC");
    return { va, vb, vc };
  }

  async function doSwap(label, tokenIn, tokenOut) {
    const { va, vb, vc } = await readVaults();
    const amounts = [va.amount, vb.amount, vc.amount];
    const amountIn = 10_000n;
    const feeBps = 30n;
    const quote = swapOutGivenInWeighted(
      amounts[tokenIn],
      WEIGHTS[tokenIn],
      amounts[tokenOut],
      WEIGHTS[tokenOut],
      amountIn,
      feeBps
    );
    const swapData = Buffer.concat([
      Buffer.from([3, tokenIn, tokenOut]),
      u64le(amountIn),
      u64le(quote),
    ]);
    const ix = new TransactionInstruction({
      programId: PROGRAM_ID,
      keys: [
        { pubkey: payer.publicKey, isSigner: true, isWritable: false },
        { pubkey: pool, isSigner: false, isWritable: false },
        { pubkey: vaultA, isSigner: false, isWritable: true },
        { pubkey: vaultB, isSigner: false, isWritable: true },
        { pubkey: vaultC, isSigner: false, isWritable: true },
        { pubkey: userA, isSigner: false, isWritable: true },
        { pubkey: userB, isSigner: false, isWritable: true },
        { pubkey: userC, isSigner: false, isWritable: true },
        { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
      ],
      data: swapData,
    });
    const tx = new Transaction().add(
      ComputeBudgetProgram.setComputeUnitLimit({ units: 1_000_000 }),
      ix
    );
    tx.feePayer = payer.publicKey;
    tx.recentBlockhash = (
      await rpcRetry(() => connection.getLatestBlockhash("confirmed"), "blockhash")
    ).blockhash;
    tx.sign(payer);
    const sig = await rpcRetry(
      () =>
        connection.sendRawTransaction(tx.serialize(), {
          skipPreflight: false,
          maxRetries: 0,
        }),
      "send " + label
    );
    console.log("sent", label, sig, "quote", quote.toString());
    const confirmed = await confirmGetTransaction(connection, sig);
    await sleep(2500);
    const after = await readVaults();
    const afterAmt = [after.va.amount, after.vb.amount, after.vc.amount];
    const actualOut = amounts[tokenOut] - afterAmt[tokenOut];
    const third = [0, 1, 2].find((i) => i !== tokenIn && i !== tokenOut);
    const thirdUnchanged = amounts[third] === afterAmt[third];
    const row = {
      label,
      sig,
      slot: confirmed.slot,
      tokenIn,
      tokenOut,
      weights: WEIGHTS,
      quote: quote.toString(),
      actualOut: actualOut.toString(),
      before: {
        a: amounts[0].toString(),
        b: amounts[1].toString(),
        c: amounts[2].toString(),
      },
      after: {
        a: afterAmt[0].toString(),
        b: afterAmt[1].toString(),
        c: afterAmt[2].toString(),
      },
      thirdUnchanged,
      match: actualOut === quote,
      cu: confirmed.meta?.computeUnitsConsumed ?? null,
    };
    console.log(label, row);
    if (actualOut !== quote) throw new Error("quote mismatch");
    if (!thirdUnchanged) throw new Error("third reserve moved");
    return row;
  }

  const s1 = await doSwap("swap1", 0, 1);
  await sleep(4000);
  const s2 = await doSwap("swap2", 1, 2);
  console.log("SWAP_OK", s1.sig, s2.sig);
}

main().catch((e) => {
  console.error("FAIL", e);
  process.exit(1);
});
