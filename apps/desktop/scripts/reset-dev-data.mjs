// Wipes the dev build's data so the next `pnpm dev` starts as a new user:
// its settings, database and webview storage, and its Credential Manager
// entries. Reads the dev identity from tauri.dev.conf.json and refuses to
// run for any other identity, so the installed app's data is never touched.
import { execFileSync } from "node:child_process";
import { readFileSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const devConf = JSON.parse(readFileSync(join(here, "..", "src-tauri", "tauri.dev.conf.json"), "utf8"));
const prodConf = JSON.parse(readFileSync(join(here, "..", "src-tauri", "tauri.conf.json"), "utf8"));
const { identifier, productName } = devConf;

if (!identifier || !productName) {
  throw new Error(`tauri.dev.conf.json must set identifier and productName, got ${JSON.stringify(devConf)}`);
}
if (identifier === prodConf.identifier || productName === prodConf.productName) {
  throw new Error(`Dev identity matches production (${identifier}, ${productName}); refusing to wipe it`);
}
if (process.platform !== "win32") {
  throw new Error(`reset-dev-data only knows Windows paths, running on ${process.platform}`);
}

function requiredEnv(name) {
  const value = process.env[name];
  if (!value) throw new Error(`Required environment variable ${name} is not set`);
  return value;
}

for (const root of [requiredEnv("APPDATA"), requiredEnv("LOCALAPPDATA")]) {
  const dir = join(root, identifier);
  rmSync(dir, { recursive: true, force: true });
  console.log(`removed ${dir}`);
}

// keyring stores each secret as "<name>.<service>"; the service is the product name.
const suffix = `.${productName}`;
const targets = execFileSync("cmdkey", ["/list"], { encoding: "utf8" })
  .split(/\r?\n/)
  .map((line) => line.match(/^\s*Target:\s*(.+?)\s*$/)?.[1])
  .filter((target) => target?.endsWith(suffix));
for (const target of targets) {
  execFileSync("cmdkey", [`/delete:${target}`], { stdio: "ignore" });
  console.log(`removed credential ${target}`);
}
console.log(`${productName} will start fresh.`);
