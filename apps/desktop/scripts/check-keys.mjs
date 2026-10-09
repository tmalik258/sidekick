// Fails when an island shortcut uses a key another app owns system-wide:
// NVIDIA and AMD overlays take Alt R and Alt Z, Windows takes Alt Tab,
// Alt F4 and Alt Space. Pressing those would trigger the other app.

import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const RESERVED = ["r", "z"];
const root = join(import.meta.dirname, "..", "src");
const files = (dir) =>
  readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
    e.isDirectory() ? files(join(dir, e.name)) : /\.tsx?$/.test(e.name) ? [join(dir, e.name)] : [],
  );
const bad = [];
for (const f of files(root)) {
  const src = readFileSync(f, "utf8");
  for (const k of RESERVED) {
    const handler = new RegExp(`altKey[^\\n]*key(?:\\.toLowerCase\\(\\))? === "${k}"`, "i");
    const label = new RegExp(`alt-pre">Alt </i>${k}\\b`, "i");
    if (handler.test(src) || label.test(src)) bad.push(`${f.replace(root, "src")}: Alt ${k.toUpperCase()}`);
  }
}
if (bad.length) {
  console.log(`Shortcuts on keys GPU overlays own:\n  ${bad.join("\n  ")}`);
  process.exit(1);
}
console.log("No island shortcut uses Alt R or Alt Z.");
