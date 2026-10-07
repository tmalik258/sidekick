// Screenshots of the island in each island colour, over a light and a dark
// desktop: hover card, Ask, Agents, and solid (transparency off). Runs on the
// static build with the browser mock and fails on any page error. CI keeps
// the pictures as an artifact for review.
//
//   pnpm --filter desktop build && pnpm --filter desktop visual

import { mkdirSync, readFileSync, statSync } from "node:fs";
import { createServer } from "node:http";
import { extname, join, normalize } from "node:path";
import { chromium } from "playwright";

const dir = import.meta.dirname;
const out = join(dir, "..", "out");
const shots = join(dir, "..", "visual");
mkdirSync(shots, { recursive: true });

const css = readFileSync(join(dir, "..", "src", "app", "globals.css"), "utf8");
const orb = readFileSync(join(dir, "..", "src", "components", "Orb.tsx"), "utf8");
const themes = [...orb.matchAll(/^ {2}(\w+): \{\n {4}label:/gm)].map((m) => m[1]);
const colors = ["glass", ...[...css.matchAll(/\.island-shell\[data-color="([^"]+)"\]/g)].map((m) => m[1])];

const TYPES = {
  ".html": "text/html",
  ".js": "text/javascript",
  ".css": "text/css",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".woff2": "font/woff2",
  ".json": "application/json",
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
const base = `http://localhost:${server.address().port}/island/`;

const DESKTOPS = {
  light: "linear-gradient(180deg, #f3f4f8, #dfe3ee)",
  dark: "linear-gradient(180deg, #14162c, #2a2150)",
};

const browser = await chromium.launch(process.env.CHROMIUM ? { executablePath: process.env.CHROMIUM } : {});
const errors = [];
let count = 0;
async function shoot(name, query, desktop, act) {
  const page = await browser.newPage({
    viewport: { width: 760, height: 560 },
    colorScheme: desktop === "light" ? "light" : "dark",
    reducedMotion: "reduce",
  });
  page.on("pageerror", (e) => errors.push(`${name}: ${e.message}`));
  await page.goto(base + query);
  await page.addStyleTag({ content: `html,body{background:${DESKTOPS[desktop]} !important}` });
  await page.waitForTimeout(1500);
  await act(page);
  await page.waitForTimeout(900);
  await page.screenshot({ path: join(shots, `${name}-${desktop}.png`) });
  await page.close();
  count++;
}

const hover = (p) => p.mouse.move(380, 25, { steps: 4 });
const ask = async (p) => {
  await hover(p);
  await p.waitForTimeout(800);
  await p.getByRole("button", { name: /Ask Sidekick/ }).click();
};
const agents = async (p) => {
  await ask(p);
  await p.waitForTimeout(600);
  await p.keyboard.press("Control+Tab");
};

for (const color of colors) {
  const q = `?onboarded${color === "glass" ? "" : `&color=${color}`}`;
  for (const desktop of Object.keys(DESKTOPS)) await shoot(`${color}-hover`, q, desktop, hover);
}
for (const theme of themes) {
  for (const desktop of Object.keys(DESKTOPS))
    await shoot(`mascot-${theme}`, `?onboarded&theme=${theme}`, desktop, hover);
}
for (const desktop of Object.keys(DESKTOPS)) {
  await shoot("ask", "?onboarded", desktop, ask);
  await shoot("agents", "?onboarded", desktop, agents);
  await shoot("solid-hover", "?onboarded&solid", desktop, hover);
  await shoot("first-run", "?onboarded&nomodel", desktop, ask);
}

await browser.close();
server.close();
console.log(`${count} screenshots in ${shots}`);
if (errors.length) {
  console.error(errors.join("\n"));
  process.exit(1);
}
