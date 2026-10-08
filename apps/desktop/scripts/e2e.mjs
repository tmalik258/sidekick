// End-to-end regression tests on the real app (Windows): starts the test
// build, whose WebView2 opens a debugging port (tauri.e2e.conf.json), connects Playwright to it (the
// way Microsoft documents for automating WebView2), drives the island the way
// a user does, and fails on a broken flow, a page error, a slow step or a
// frozen UI thread.
//
//   pnpm --filter desktop tauri build --debug --no-bundle --config src-tauri/tauri.e2e.conf.json
//   pnpm --filter desktop e2e

import { spawn } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";
import { chromium } from "playwright";

const APP =
  process.env.SIDEKICK_EXE ?? join(import.meta.dirname, "..", "..", "..", "target", "debug", "sidekick-desktop.exe");
/** Set in tauri.e2e.conf.json; WebView2 ignores the environment variable once the app passes its own arguments. */
const PORT = 9229;
const SHOTS = join(import.meta.dirname, "..", "e2e");
/** Budgets, in ms: a step slower than this fails. */
const BUDGET = { open: 1500, instant: 1500, freeze: 250 };

// Start onboarded with voice off, so no welcome or model download gets in
// the way. Your own settings are put back when the run ends.
const settingsFile = process.env.APPDATA ? join(process.env.APPDATA, "app.sidekick.desktop", "settings.json") : null;
const backup = settingsFile ? `${settingsFile}.e2e-backup` : null;
if (settingsFile && backup) {
  mkdirSync(join(settingsFile, ".."), { recursive: true });
  if (existsSync(settingsFile) && !existsSync(backup)) copyFileSync(settingsFile, backup);
  writeFileSync(settingsFile, JSON.stringify({ onboarded: true, voice: { enabled: false } }));
}
function restoreSettings() {
  if (!settingsFile || !backup) return;
  if (existsSync(backup)) renameSync(backup, settingsFile);
  else rmSync(settingsFile, { force: true });
}
mkdirSync(SHOTS, { recursive: true });

/** The end of Sidekick's own log, for a failed start. */
function appLog() {
  const dir = process.env.LOCALAPPDATA ? join(process.env.LOCALAPPDATA, "app.sidekick.desktop", "logs") : null;
  if (!dir || !existsSync(dir)) return "(no log)";
  return readdirSync(dir)
    .map((f) => readFileSync(join(dir, f), "utf8").split("\n").slice(-30).join("\n"))
    .join("\n");
}

const app = spawn(APP, [], {
  env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT}` },
  stdio: "ignore",
});
let browser;
async function stop() {
  await browser?.close().catch(() => undefined);
  if (process.platform === "win32") spawn("taskkill", ["/pid", String(app.pid), "/T", "/F"], { stdio: "ignore" });
  else app.kill();
  restoreSettings();
}

// The island's page, once WebView2 opens its debugging port.
async function connect() {
  const started = Date.now();
  while (Date.now() - started < 60_000) {
    if (app.exitCode !== null) throw new Error(`Sidekick exited with code ${app.exitCode}\n${appLog()}`);
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
      throw new Error("no island page");
    } catch {
      await new Promise((r) => setTimeout(r, 500));
    }
  }
  throw new Error(`could not connect to Sidekick's WebView2 on port ${PORT}\n${appLog()}`);
}

let page;
try {
  page = await connect();
} catch (e) {
  console.log(`FAIL  start: ${e.message ?? e}`);
  await stop();
  process.exit(1);
}

/** Calls a Tauri command the way the UI does. */
const invoke = (cmd, args = {}) =>
  page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a).catch((e) => ({ error: String(e) })), [cmd, args]);
/** Waits until `fn` (run in the page) returns something truthy; returns how long it took. */
async function waitFor(what, fn, ms = 5000, arg) {
  const started = Date.now();
  for (;;) {
    const v = await page.evaluate(fn, arg).catch(() => null);
    if (v) return Date.now() - started;
    if (Date.now() - started > ms) throw new Error(`timed out after ${ms} ms waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 50));
  }
}
const type = (text) => page.keyboard.type(text);
const clearInput = async () => {
  await page.evaluate(() => document.querySelector(".ak input")?.select());
  await page.keyboard.press("Backspace");
};
const clickText = (text, scope = "body") =>
  page.evaluate(
    ([t, s]) => {
      const el = [...document.querySelectorAll(`${s} button, ${s} [role=tab]`)].find((b) => b.textContent.trim() === t);
      if (!el) return false;
      el.click();
      return true;
    },
    [text, scope],
  );
const shot = (name) => page.screenshot({ path: join(SHOTS, `${name}.png`) }).catch(() => undefined);
const errors = [];
page.on("pageerror", (e) => errors.push(e.message));

const results = [];
async function test(name, fn) {
  const started = Date.now();
  try {
    const note = await fn();
    results.push({ name, ok: true, ms: Date.now() - started, note });
    console.log(`ok    ${name}${note ? ` (${note})` : ""}`);
  } catch (e) {
    results.push({ name, ok: false, ms: Date.now() - started, note: String(e.message ?? e) });
    console.log(`FAIL  ${name}: ${e.message ?? e}`);
    await shot(`fail-${name.replace(/\W+/g, "-")}`);
  }
}
const pcPressed = () => page.evaluate(() => document.querySelector(".ak-pc")?.getAttribute("aria-pressed"));

try {
  await test("island loads", async () => {
    await waitFor("the island", () => !!document.querySelector(".island-shell"), 20_000);
  });

  await test("Ask opens with the input focused", async () => {
    await invoke("ask_open", { prompt: null, ask: false });
    const ms = await waitFor(
      "the Ask input to have focus",
      () => {
        const i = document.querySelector(".ak input");
        return !!i && document.activeElement === i && document.querySelectorAll(".ak-tabs [role=tab]").length === 3;
      },
      BUDGET.open,
    );
    await shot("ask-open");
    return `${ms} ms`;
  });

  await test("instant results find an app", async () => {
    await type("powershell");
    const ms = await waitFor(
      "an app row for PowerShell",
      () => [...document.querySelectorAll(".ak-row")].some((r) => /powershell/i.test(r.textContent)),
      BUDGET.instant,
    );
    await shot("instant-app");
    return `${ms} ms`;
  });

  await test("a Windows switch shows as an instant result", async () => {
    await clearInput();
    await type("turn on hotspot");
    await waitFor(
      "Turn Mobile hotspot on",
      () => [...document.querySelectorAll(".ak-row")].some((r) => /Turn Mobile hotspot on/.test(r.textContent)),
      BUDGET.instant,
    );
    await clearInput();
  });

  await test("This PC only stays on after sending", async () => {
    // Off, the chip is just a lock icon: click it by its label.
    if ((await pcPressed()) !== "true") await page.click('.ak-pc[aria-label="This PC only"]');
    if ((await pcPressed()) !== "true") throw new Error("This PC only did not switch on");
    await page.evaluate(() => document.querySelector(".ak input")?.focus());
    await type("hello");
    await page.keyboard.press("Enter");
    // No local model on the runner: the answer fails, which is fine here.
    await waitFor(
      "the question in the chat",
      () => [...document.querySelectorAll(".ak-um")].some((m) => m.textContent.includes("hello")),
      5000,
    );
    if ((await page.evaluate(() => document.querySelector(".ak input")?.value)) !== "")
      throw new Error("the input kept the question");
    await waitFor("an answer or a failure card", () => !!document.querySelector(".ak-ans, .ak-err"), 30_000);
    if ((await pcPressed()) !== "true") throw new Error("This PC only switched off after sending");
    await shot("after-send");
  });

  await test("Esc starts a new chat", async () => {
    await page.keyboard.press("Escape");
    await waitFor("an empty chat", () => document.querySelectorAll(".ak-um").length === 0, 3000);
  });

  await test("Ctrl Tab moves through Agents and History", async () => {
    await page.keyboard.press("Control+Tab");
    await waitFor(
      "the Agents tab",
      () => /^Agents/.test(document.querySelector(".ak-tabs [aria-selected=true]")?.textContent.trim() ?? ""),
      3000,
    );
    await shot("agents");
    await clickText("History", ".ak-tabs");
    await waitFor(
      "the History tab",
      () => /History/.test(document.querySelector(".ak-tabs [aria-selected=true]")?.textContent ?? ""),
      3000,
    );
    const overflow = await page.evaluate(() => {
      const s = document.querySelector(".island-shell").getBoundingClientRect();
      return [...document.querySelectorAll(".island-shell *")].filter(
        (e) => e.getBoundingClientRect().right > s.right + 1,
      ).length;
    });
    if (overflow > 0) throw new Error(`${overflow} elements run past the island`);
    await shot("history");
    await clickText("Ask", ".ak-tabs");
  });

  await test("every Settings tab opens", async () => {
    await invoke("open_settings");
    await waitFor(
      "Settings",
      () => [...document.querySelectorAll("button")].some((b) => b.textContent.trim() === "Appearance"),
      5000,
    );
    const missing = [];
    for (const label of ["Home", "Appearance", "AI", "Apps", "Privacy", "Skills"]) {
      if (!(await clickText(label))) {
        missing.push(label);
        continue;
      }
      await new Promise((r) => setTimeout(r, 400));
      await shot(`settings-${label}`);
    }
    if (missing.length) throw new Error(`no tab named ${missing.join(", ")}`);
  });

  await test("AI settings say why a model cannot be used", async () => {
    await clickText("AI");
    await waitFor(
      "a reason instead of a switch",
      () => /No key|Not installed|Not running/.test(document.body.textContent),
      5000,
    );
  });

  await test("no page errors", async () => {
    if (errors.length) throw new Error(errors.join(" | "));
  });

  await test("the UI thread never froze", async () => {
    const freezes = (await invoke("freeze_report")) ?? [];
    const worst = freezes.reduce((m, f) => Math.max(m, f.ms), 0);
    const list = freezes.map((f) => `${f.what} ${f.ms} ms`).join(", ");
    if (worst > BUDGET.freeze) throw new Error(`held for ${worst} ms: ${list}`);
    return freezes.length ? `slowest ${worst} ms: ${list}` : "nothing over 50 ms";
  });
} finally {
  await stop();
}

writeFileSync(join(SHOTS, "results.json"), JSON.stringify(results, null, 2));
const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length} passed, ${failed.length} failed`);
process.exit(failed.length ? 1 : 0);
