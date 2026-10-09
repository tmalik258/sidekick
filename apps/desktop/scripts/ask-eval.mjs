// Ask mode eval on the real app (Windows): asks every question in
// ask-cases.json the way you would, reads what the island shows, and
// checks it against the rules in ask-check.mjs plus each case's own
// expect/forbid patterns. Uses your own settings and models, so answers
// are real. Nothing here sends, posts or deletes anything.
//
//   pnpm --filter desktop tauri build --debug --no-bundle --config src-tauri/tauri.e2e.conf.json
//   pnpm --filter desktop ask-eval                 (all cases)
//   pnpm --filter desktop ask-eval -- --only files (cases whose group or question matches)
//
// Writes e2e/ask-report.md with every question, answer and problem.

import { spawn } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { chromium } from "playwright";
import { checkAnswer, checkCase } from "./ask-check.mjs";

const APP =
  process.env.SIDEKICK_EXE ?? join(import.meta.dirname, "..", "..", "..", "target", "debug", "sidekick-desktop.exe");
const PORT = 9229;
const OUT = join(import.meta.dirname, "..", "e2e");
const ANSWER_MS = 180_000;
const only = process.argv.includes("--only") ? process.argv[process.argv.indexOf("--only") + 1].toLowerCase() : null;
const cases = JSON.parse(readFileSync(join(import.meta.dirname, "ask-cases.json"), "utf8")).filter(
  (c) => !only || c.group.includes(only) || c.q.toLowerCase().includes(only),
);
mkdirSync(OUT, { recursive: true });

console.log(`Starting ${APP}`);
// A build older than the last pull tests old code.
try {
  const { statSync } = await import("node:fs");
  const { execSync } = await import("node:child_process");
  const head = Number(execSync("git log -1 --format=%ct").toString().trim()) * 1000;
  if (statSync(APP).mtimeMs < head) {
    console.error("The test build is older than your last pull. Rebuild it first:");
    console.error("  pnpm --filter desktop tauri build --debug --no-bundle --config src-tauri/tauri.e2e.conf.json");
    process.exit(1);
  }
} catch {}
const app = spawn(APP, [], {
  env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT}` },
  stdio: "ignore",
});
let browser;
async function stop() {
  await browser?.close().catch(() => undefined);
  if (process.platform === "win32") spawn("taskkill", ["/pid", String(app.pid), "/T", "/F"], { stdio: "ignore" });
  else app.kill();
}

async function connect() {
  const started = Date.now();
  while (Date.now() - started < 60_000) {
    if (app.exitCode !== null)
      throw new Error(
        `the test build closed at once (code ${app.exitCode}). Is your normal Sidekick still running? Quit it from the tray and run this again.`,
      );
    try {
      browser = await chromium.connectOverCDP(`http://127.0.0.1:${PORT}`);
      for (let i = 0; i < 100; i++) {
        const page = browser
          .contexts()
          .flatMap((c) => c.pages())
          .find((p) => p.url().includes("island"));
        if (page) return page;
        await new Promise((r) => setTimeout(r, 200));
      }
    } catch {
      await new Promise((r) => setTimeout(r, 500));
    }
  }
  throw new Error(`could not connect to Sidekick on port ${PORT}. Is the e2e build running elsewhere?`);
}

const page = await connect().catch(async (e) => {
  console.log(`FAIL  start: ${e.message}`);
  await stop();
  process.exit(1);
});
console.log(`Connected. Asking ${cases.length} questions, hands off until it is done.`);
const invoke = (cmd, args = {}) =>
  page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a).catch((e) => ({ error: String(e) })), [cmd, args]);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** The last answer as the island shows it. */
const read = () =>
  page.evaluate(() => {
    const body = [...document.querySelectorAll(".ak-ans")].at(-1)?.innerText ?? "";
    const chips = [...document.querySelectorAll(".ak-chips")].at(-1);
    const options = chips ? [...chips.querySelectorAll(".ak-chip .leading-snug")].map((o) => o.textContent.trim()) : [];
    const stats = [...document.querySelectorAll(".ak-stat")].map((s) => ({
      label: s.childNodes[0]?.textContent?.trim() ?? "",
      value: s.querySelector("b")?.textContent?.trim() ?? "",
    }));
    const err = document.querySelector(".ak-err");
    return { body, options, stats, error: err ? err.innerText.trim() : undefined };
  });

const results = [];
try {
  await invoke("ask_open", { prompt: null, ask: false });
  for (const c of cases) {
    // A fresh chat per question: the + button, then wait for an empty chat.
    for (let i = 0; i < 3; i++) {
      await invoke("ask_open", { prompt: null, ask: false });
      if (await page.waitForSelector(".ak-q", { timeout: 5000 }).catch(() => null)) break;
    }
    const plus = page.locator('button[aria-label="New chat (Esc)"]').first();
    if (await plus.isVisible().catch(() => false)) await plus.click();
    await page
      .waitForFunction(() => !document.querySelector(".ak-ans, .ak-acts, .ak-err"), null, { timeout: 5000 })
      .catch(() => undefined);
    await page.click(".ak-q");
    await page.keyboard.type(c.q);
    const started = Date.now();
    await page.keyboard.press("Enter");
    let done = false;
    while (Date.now() - started < ANSWER_MS) {
      done = await page.evaluate(() => !!document.querySelector(".ak-acts, .ak-err"));
      if (done) break;
      await sleep(250);
    }
    if (!done) {
      // Stop it so the next question does not queue behind it.
      await page
        .locator("button", { hasText: "Stop" })
        .first()
        .click({ timeout: 2000 })
        .catch(() => undefined);
      await sleep(500);
    }
    const ms = Date.now() - started;
    const a = done ? await read() : { body: "", options: [], stats: [], error: `no answer in ${ANSWER_MS / 1000} s` };
    const problems = [...checkAnswer({ question: c.q, ...a }), ...checkCase(c, a)];
    results.push({ ...c, ...a, ms, problems });
    console.log(`${problems.length ? "FAIL" : "ok  "}  [${c.group}] ${c.q} (${(ms / 1000).toFixed(1)} s)`);
    for (const p of problems) console.log(`        - ${p}`);
    if (problems.length) await page.screenshot({ path: join(OUT, `ask-${results.length}.png`) }).catch(() => undefined);
  }
} finally {
  await stop();
}

const failed = results.filter((r) => r.problems.length);
const md = [
  `# Ask eval, ${new Date().toLocaleString()}`,
  "",
  `${results.length - failed.length} of ${results.length} passed.`,
  "",
  ...results.flatMap((r, i) => [
    `## ${i + 1}. ${r.problems.length ? "FAIL" : "ok"}: ${r.q}`,
    "",
    `Group: ${r.group}. Took ${(r.ms / 1000).toFixed(1)} s.${r.problems.length ? ` Screenshot: ask-${i + 1}.png` : ""}`,
    "",
    ...(r.problems.length ? ["Problems:", ...r.problems.map((p) => `- ${p}`), ""] : []),
    "Answer:",
    "",
    "```",
    r.error ? `(error) ${r.error}` : r.body || "(empty)",
    "```",
    ...(r.stats.length ? ["", `Stats: ${r.stats.map((s) => `${s.label} ${s.value}`).join(", ")}`] : []),
    ...(r.options.length ? ["", `Options: ${r.options.join(" | ")}`] : []),
    "",
  ]),
].join("\n");
writeFileSync(join(OUT, "ask-report.md"), md);
console.log(`\n${results.length - failed.length} passed, ${failed.length} failed. Report: e2e/ask-report.md`);
process.exit(failed.length ? 1 : 0);
