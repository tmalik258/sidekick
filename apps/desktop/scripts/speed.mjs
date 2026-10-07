// Speed targets as a test, on the static build with the browser mock: Ask
// open to ready, and typing to instant results. Each runs several times and
// the median must meet its target. Targets have headroom for CI machines;
// the real ones (Ask's timing panel, Ctrl+Alt+Shift+T) are stricter.
//
//   pnpm --filter desktop build && pnpm --filter desktop speed

import { readFileSync, statSync } from "node:fs";
import { createServer } from "node:http";
import { extname, join, normalize } from "node:path";
import { chromium } from "playwright";

const TARGETS = { open_to_ready: 250, instant_results: 300 };
const RUNS = 5;

const out = join(import.meta.dirname, "..", "out");
const TYPES = {
  ".html": "text/html",
  ".js": "text/javascript",
  ".css": "text/css",
  ".svg": "image/svg+xml",
  ".woff2": "font/woff2",
};
const server = createServer((req, res) => {
  const path = normalize(decodeURIComponent(new URL(req.url, "http://x").pathname)).replace(/^(\.\.[/\\])+/, "");
  let file = join(out, path);
  try {
    if (statSync(file).isDirectory()) file = join(file, "index.html");
    res.writeHead(200, { "content-type": TYPES[extname(file)] ?? "application/octet-stream" });
    res.end(readFileSync(file));
  } catch {
    res.writeHead(404);
    res.end();
  }
});
await new Promise((r) => server.listen(0, r));
const url = `http://localhost:${server.address().port}/island/?onboarded`;

const browser = await chromium.launch(process.env.CHROMIUM ? { executablePath: process.env.CHROMIUM } : {});
const page = await browser.newPage({ viewport: { width: 760, height: 560 } });
await page.goto(url);
await page.waitForTimeout(1500);

const median = (v) => [...v].sort((a, b) => a - b)[Math.floor(v.length / 2)];
const results = { open_to_ready: [], instant_results: [] };
for (let i = 0; i < RUNS; i++) {
  await page.evaluate(() => {
    window.__timings = [];
  });
  await page.mouse.move(380, 25, { steps: 4 });
  await page.waitForTimeout(900);
  await page.getByRole("button", { name: /Ask Sidekick/ }).click({ timeout: 4000 });
  await page.waitForFunction(() => window.__timings?.some((t) => t.name === "open_to_ready"), null, { timeout: 5000 });
  results.open_to_ready.push(await page.evaluate(() => window.__timings.find((t) => t.name === "open_to_ready").ms));

  // Typing shows apps by name before any model runs.
  const input = page.locator(".ak-q");
  const start = await page.evaluate(() => performance.now());
  await input.fill("slack");
  await page.getByText("Slack", { exact: true }).first().waitFor({ timeout: 5000 });
  results.instant_results.push(Math.round((await page.evaluate(() => performance.now())) - start));

  await input.fill("");
  await input.press("Escape");
  await page.mouse.move(380, 400, { steps: 4 });
  await page.waitForTimeout(1600);
}
await browser.close();
server.close();

let failed = false;
for (const [name, values] of Object.entries(results)) {
  const m = median(values);
  const ok = m <= TARGETS[name];
  failed ||= !ok;
  console.log(
    `${ok ? "ok  " : "SLOW"} ${name}: median ${m} ms (target ${TARGETS[name]} ms; runs ${values.join(", ")})`,
  );
}
process.exit(failed ? 1 : 0);
