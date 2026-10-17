// Accessibility audit with axe-core on the static build and the browser
// mock: labels, roles, names and contrast on every page, plus the Settings
// window and the editor. Fails on serious or critical issues.
//
//   pnpm --filter desktop build && pnpm --filter desktop check:a11y
import { createReadStream, existsSync, readFileSync, statSync } from "node:fs";
import http from "node:http";
import { createRequire } from "node:module";
import { extname, join } from "node:path";
import { chromium } from "playwright";

const OUT = join(import.meta.dirname, "..", "out");
const axe = readFileSync(createRequire(import.meta.url).resolve("axe-core/axe.min.js"), "utf8");
const TYPES = {
  ".js": "text/javascript",
  ".css": "text/css",
  ".html": "text/html",
  ".woff2": "font/woff2",
  ".svg": "image/svg+xml",
};
const PAGES = ["/", "/island/", "/dev/git/", "/dev/editor/", "/dev/states/", "/dev/skills/"];

const srv = http
  .createServer((q, r) => {
    let f = join(OUT, decodeURIComponent(q.url.split("?")[0]));
    if (existsSync(f) && statSync(f).isDirectory()) f = join(f, "index.html");
    if (!existsSync(f)) {
      r.writeHead(404);
      return r.end();
    }
    r.writeHead(200, { "content-type": TYPES[extname(f)] ?? "application/octet-stream" });
    createReadStream(f).pipe(r);
  })
  .listen(0);
const base = `http://localhost:${srv.address().port}`;
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM || undefined });
let failed = 0;
for (const path of PAGES) {
  const page = await browser.newPage({ viewport: { width: 1100, height: 760 } });
  await page.goto(base + path);
  await page.waitForTimeout(1200);
  await page.addScriptTag({ content: axe });
  const res = await page.evaluate(() =>
    // biome-ignore lint: axe is injected above.
    window.axe.run(document, { resultTypes: ["violations"] }),
  );
  const bad = res.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  for (const v of bad) {
    failed++;
    console.log(`${path}  ${v.impact}  ${v.id}: ${v.help}`);
    for (const n of v.nodes.slice(0, 4)) console.log(`    ${n.target.join(" ")}`);
  }
  if (!bad.length) console.log(`${path}  ok (${res.violations.length} minor)`);
  await page.close();
}
await browser.close();
srv.close();
if (failed) {
  console.error(`${failed} serious accessibility issues`);
  process.exit(1);
}
