/** One A→B swap against an existing pool. Slow RPC polling for public devnet. */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import {
  Connection,
  Keypair,
  PublicKey,
  Transaction,
  TransactionInstruction,
} from "@solana/web3.js";
import { TOKEN_PROGRAM_ID, getAccount } from "@solana/spl-token";
import { swapOutGivenIn } from "./devnet-e2e.mjs";

const PROGRAM_ID = new PublicKey("8SMAn5rTDFCaLKAdNd1fXckxm1eHdZGDkZP7JqhLUpbP");
const POOL_SEED = Buffer.from("pool");
const VAULT_SEED = Buffer.from("vault");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function u64le(n) {
  const b = Buffer.alloc(8);
  b.writeBigUInt64LE(BigInt(n));
  return b;
}

function loadKeypair(p) {
  return Keypair.fromSecretKey(Uint8Array.from(JSON.parse(fs.readFileSync(p, "utf8"))));
}

async function confirm(connection, sig) {
  for (let i = 0; i < 30; i++) {
    await sleep(3000);
    try {
      const st = await connection.getSignatureStatus(sig, { searchTransactionHistory: true });
      const v = st?.value;
      if (v?.err) throw new Error("tx err " + JSON.stringify(v.err));
      if (v?.confirmationStatus === "confirmed" || v?.confirmationStatus === "finalized") {
        return;
      }
    } catch (e) {
      if (String(e.message || e).includes("429")) continue;
      throw e;
    }
  }
  throw new Error("confirm timeout " + sig);
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
  async function userAta(mint) {
    const r = await connection.getParsedTokenAccountsByOwner(payer.publicKey, { mint });
    if (!r.value.length) throw new Error("no token account for " + mint.toBase58());
    return r.value[0].pubkey;
  }
  await sleep(2000);
  const userA = await userAta(mintA);
  await sleep(1500);
  const userB = await userAta(mintB);
  await sleep(1500);
  const userC = await userAta(mintC);
  console.log("user_accounts", userA.toBase58(), userB.toBase58(), userC.toBase58());

  await sleep(4000);
  const va = await getAccount(connection, vaultA);
  await sleep(1500);
  const vb = await getAccount(connection, vaultB);
  await sleep(1500);
  const vc = await getAccount(connection, vaultC);
  const amountIn = 10_000n;
  const quote = swapOutGivenIn(va.amount, vb.amount, amountIn, 30n);
  const swapData = Buffer.concat([Buffer.from([3, 0, 1]), u64le(amountIn), u64le(quote)]);
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
  const tx = new Transaction().add(ix);
  tx.feePayer = payer.publicKey;
  tx.recentBlockhash = (await connection.getLatestBlockhash("confirmed")).blockhash;
  tx.sign(payer);
  const sig = await connection.sendRawTransaction(tx.serialize(), {
    skipPreflight: false,
  });
  console.log("sent", sig, "quote", quote.toString());
  await confirm(connection, sig);
  await sleep(3000);
  const va2 = await getAccount(connection, vaultA);
  await sleep(1500);
  const vb2 = await getAccount(connection, vaultB);
  await sleep(1500);
  const vc2 = await getAccount(connection, vaultC);
  const actualOut = vb.amount - vb2.amount;
  console.log("swap2", {
    sig,
    pool: pool.toBase58(),
    quote: quote.toString(),
    actualOut: actualOut.toString(),
    before: { a: va.amount.toString(), b: vb.amount.toString(), c: vc.amount.toString() },
    after: { a: va2.amount.toString(), b: vb2.amount.toString(), c: vc2.amount.toString() },
    thirdUnchanged: vc.amount === vc2.amount,
    match: actualOut === quote,
  });
  if (actualOut !== quote) throw new Error("quote mismatch");
  if (vc.amount !== vc2.amount) throw new Error("third reserve moved");
  console.log("SWAP2_OK");
}

main().catch((e) => {
  console.error("FAIL", e);
  process.exit(1);
});
