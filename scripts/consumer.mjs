import { swapOutGivenIn, proportionalAdd } from "./devnet-e2e.mjs";

const reserves = [1_000_000n, 2_000_000n, 3_000_000n];
const quote = swapOutGivenIn(reserves[0], reserves[1], 12_345n, 0n);
if (quote !== 24388n) {
  throw new Error(`quote mismatch ${quote}`);
}
const add = proportionalAdd(reserves, [10_000n, 20_000n, 30_000n], 100_000n);
if (add.lp !== 1000n) throw new Error(`lp mismatch ${add.lp}`);
console.log("quote_out=" + quote.toString());
console.log("lp_minted=" + add.lp.toString());
console.log("JS_CONSUMER_OK");
