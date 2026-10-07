// End-to-end regression tests on the real app (Windows): starts the debug
// build through tauri-driver, drives the island the way a user does, and
// fails on a broken flow, a page error, a slow step or a frozen UI thread.
// No dependencies: it speaks WebDriver over HTTP.
//
//   pnpm --filter desktop tauri build --debug --no-bundle --config src-tauri/tauri.e2e.conf.json
//   tauri-driver --native-driver .\msedgedriver.exe   (in another window)
//   pnpm --filter desktop e2e

import { copyFileSync, existsSync, mkdirSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const DRIVER = process.env.TAURI_DRIVER ?? "http://127.0.0.1:4444";
const APP =
  process.env.SIDEKICK_EXE ?? join(import.meta.dirname, "..", "..", "..", "target", "debug", "sidekick-desktop.exe");
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

async function wd(method, path, body) {
  const res = await fetch(`${DRIVER}${path}`, {
    method,
    headers: { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const json = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(`${method} ${path}: ${JSON.stringify(json.value ?? json).slice(0, 400)}`);
  return json.value;
}

const session = await wd("POST", "/session", {
  capabilities: { alwaysMatch: { browserName: "wry", "tauri:options": { application: APP } } },
});
const sid = session.sessionId;
const s = (p) => `/session/${sid}${p}`;

/** Runs `fn` (a function body) in the page with `args`. */
const run = (body, ...args) => wd("POST", s("/execute/sync"), { script: body, args });
/** Calls a Tauri command the way the UI does. */
const invoke = (cmd, args = {}) =>
  wd("POST", s("/execute/async"), {
    script:
      "const [c, a, done] = arguments; window.__TAURI_INTERNALS__.invoke(c, a).then(done, (e) => done({ error: String(e) }));",
    args: [cmd, args],
  });
/** Waits until `body` returns something truthy; returns it and how long it took. */
async function waitFor(what, body, ms = 5000, ...args) {
  const started = Date.now();
  for (;;) {
    const v = await run(body, ...args).catch(() => null);
    if (v) return { value: v, ms: Date.now() - started };
    if (Date.now() - started > ms) throw new Error(`timed out after ${ms} ms waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 50));
  }
}
const active = () => wd("POST", s("/element/active"));
const type = async (text) => {
  const el = await active();
  const id = Object.values(el)[0];
  await wd("POST", s(`/element/${id}/value`), { text });
};
const KEY = { enter: "", esc: "", ctrl: "", tab: "", backspace: "" };
const clearInput = () =>
  run("const i = document.querySelector('.ak input'); if (i) { i.select(); }").then(() => type(KEY.backspace));
const clickText = (text, scope = "body") =>
  run(
    `const [t, scope] = arguments;
     const el = [...document.querySelectorAll(scope + ' button, ' + scope + ' [role=tab]')].find((b) => b.textContent.trim() === t);
     if (!el) return false; el.click(); return true;`,
    text,
    scope,
  );
async function shot(name) {
  const png = await wd("GET", s("/screenshot")).catch(() => null);
  if (png) writeFileSync(join(SHOTS, `${name}.png`), Buffer.from(png, "base64"));
}

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

try {
  await test("island loads", async () => {
    await waitFor("the island", "return !!document.querySelector('.island-shell')", 20_000);
    await run(
      "window.__e2eErrors = []; window.addEventListener('error', (e) => window.__e2eErrors.push(String(e.message))); window.addEventListener('unhandledrejection', (e) => window.__e2eErrors.push(String(e.reason)));",
    );
  });

  await test("Ask opens with the input focused", async () => {
    await invoke("ask_open", { prompt: null, ask: false });
    const { ms } = await waitFor(
      "the Ask input to have focus",
      "const i = document.querySelector('.ak input'); return !!i && document.activeElement === i && document.querySelectorAll('.ak-tabs [role=tab]').length === 3",
      BUDGET.open,
    );
    await shot("ask-open");
    return `${ms} ms`;
  });

  await test("instant results find an app", async () => {
    await type("powershell");
    const { ms } = await waitFor(
      "an app row for PowerShell",
      "return [...document.querySelectorAll('.ak-row')].some((r) => /powershell/i.test(r.textContent))",
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
      "return [...document.querySelectorAll('.ak-row')].some((r) => /Turn Mobile hotspot on/.test(r.textContent))",
      BUDGET.instant,
    );
    await clearInput();
  });

  await test("This PC only stays on after sending", async () => {
    const pressed = "return document.querySelector('.ak-pc')?.getAttribute('aria-pressed')";
    if ((await run(pressed)) !== "true") await clickText("This PC only");
    if ((await run(pressed)) !== "true") throw new Error("This PC only did not switch on");
    await type("hello");
    await type(KEY.enter);
    // No local model on the runner: the answer fails, which is fine here.
    await waitFor(
      "the question in the chat",
      "return [...document.querySelectorAll('.ak-um')].some((m) => m.textContent.includes('hello'))",
      5000,
    );
    if ((await run("return document.querySelector('.ak input')?.value")) !== "")
      throw new Error("the input kept the question");
    await waitFor(
      "an answer or a failure card",
      "return !!document.querySelector('.ak-ans, .ak-err, [role=alert], .ak-fail')",
      30_000,
    );
    if ((await run(pressed)) !== "true") throw new Error("This PC only switched off after sending");
    await shot("after-send");
  });

  await test("Esc starts a new chat", async () => {
    await type(KEY.esc);
    await waitFor("an empty chat", "return document.querySelectorAll('.ak-um').length === 0", 3000);
  });

  await test("Ctrl Tab moves through Agents and History", async () => {
    await type(KEY.ctrl + KEY.tab + KEY.ctrl);
    await waitFor(
      "the Agents tab",
      "return /^Agents/.test(document.querySelector('.ak-tabs [aria-selected=true]')?.textContent.trim() ?? '')",
      3000,
    );
    await shot("agents");
    await clickText("History", ".ak-tabs");
    await waitFor(
      "the History tab",
      "return /History/.test(document.querySelector('.ak-tabs [aria-selected=true]')?.textContent ?? '')",
      3000,
    );
    const overflow = await run(
      "const s = document.querySelector('.island-shell').getBoundingClientRect(); return [...document.querySelectorAll('.island-shell *')].filter((e) => e.getBoundingClientRect().right > s.right + 1).length",
    );
    if (overflow > 0) throw new Error(`${overflow} elements run past the island`);
    await shot("history");
    await clickText("Ask", ".ak-tabs");
  });

  await test("every Settings tab opens", async () => {
    await invoke("open_settings");
    await waitFor(
      "Settings",
      "return [...document.querySelectorAll('button')].some((b) => b.textContent.trim() === 'Appearance')",
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
      "return /No key|Not installed|Not running/.test(document.body.textContent)",
      5000,
    );
  });

  await test("no page errors", async () => {
    const errors = await run("return window.__e2eErrors ?? []");
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
  await wd("DELETE", s("")).catch(() => undefined);
  restoreSettings();
}

writeFileSync(join(SHOTS, "results.json"), JSON.stringify(results, null, 2));
const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length} passed, ${failed.length} failed`);
process.exit(failed.length ? 1 : 0);
