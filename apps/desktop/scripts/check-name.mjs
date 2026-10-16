// Fails when the app's old name, Sidekick, shows up where users can see it.
// Code that moves data from the old app and old release history may say it.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

const root = execFileSync("git", ["rev-parse", "--show-toplevel"], { encoding: "utf8" }).trim();
const files = execFileSync("git", ["ls-files", "apps", "crates", "docs", "skills", "README.md"], {
  cwd: root,
  encoding: "utf8",
})
  .split("\n")
  .filter((f) => /\.(rs|ts|tsx|js|mjs|json|md|html|yaml|yml|css|nsh)$/.test(f));

const allowed = [
  "apps/desktop/src-tauri/src/rename.rs",
  "apps/desktop/src-tauri/src/updates.rs",
  "apps/desktop/src-tauri/src/find.rs",
  "apps/desktop/src-tauri/windows/hooks.nsh",
  "apps/desktop/scripts/check-name.mjs",
  "crates/core/src/settings.rs",
  "crates/voice/src/text.rs",
];

const bad = [];
for (const file of files) {
  if (allowed.includes(file)) continue;
  const lines = readFileSync(`${root}/${file}`, "utf8").split("\n");
  lines.forEach((line, i) => {
    if (/Sidekick/.test(line)) bad.push(`${file}:${i + 1}: ${line.trim().slice(0, 120)}`);
  });
}

if (bad.length) {
  console.error(`The old name still shows in ${bad.length} places:\n${bad.join("\n")}`);
  process.exit(1);
}
console.log(`check:name ok (${files.length} files)`);
