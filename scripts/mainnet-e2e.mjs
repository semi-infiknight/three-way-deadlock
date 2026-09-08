/**
 * Production client — same quote + ix path as scripts/devnet-e2e.mjs.
 * Cluster: Solana mainnet-beta.
 */
import { spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
process.env.RPC_URL =
  process.env.RPC_URL || "https://api.mainnet-beta.solana.com";

const child = spawn(
  process.execPath,
  [path.join(__dirname, "devnet-e2e.mjs")],
  { stdio: "inherit", env: process.env }
);
child.on("exit", (code) => process.exit(code ?? 1));
