// Compare redesign island preview to the prototype HTML.
// Run with the browser preview on port 3002 (or DUSKIE_PREVIEW_URL).
// Writes PNGs under apps/desktop/.parity/ (gitignored).
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "playwright";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const out = join(root, ".parity");
mkdirSync(out, { recursive: true });

const base = process.env.DUSKIE_PREVIEW_URL ?? "http://localhost:3002";
const protoPath = join(root, "../../docs/internal/Duskie Island Redesign.html");
const protoUrl = pathToFileURL(protoPath).href;
const browser = await chromium.launch(process.env.CHROMIUM ? { executablePath: process.env.CHROMIUM } : {});
const failures = [];

async function shot(page, name) {
  const path = join(out, `${name}.png`);
  await page.screenshot({ path, fullPage: false });
  return path;
}

async function cropCreature(page, name) {
  const el = page.locator(".creature").first();
  if ((await el.count()) === 0) return;
  await el.screenshot({ path: join(out, `creature-${name}.png`) });
}

async function live(query, name, test) {
  const page = await browser.newPage({ viewport: { width: 900, height: 700 }, reducedMotion: "reduce" });
  try {
    const q = query ? `&${query}` : "";
    await page.goto(`${base}/island/?onboarded&redesign&color=atelier${q}`, {
      waitUntil: "domcontentloaded",
      timeout: 60_000,
    });
    await page.locator("[data-redesign]").first().waitFor({ state: "attached", timeout: 60_000 });
    await page.waitForTimeout(800);
    await test(page);
    await shot(page, `live-${name}`);
    await cropCreature(page, name);
  } finally {
    await page.close();
  }
}

async function prototype(name, clickLabel) {
  const page = await browser.newPage({ viewport: { width: 1100, height: 800 }, reducedMotion: "reduce" });
  try {
    await page.goto(protoUrl, { waitUntil: "domcontentloaded", timeout: 45_000 });
    await page.waitForTimeout(400);
    if (clickLabel) {
      const btn = page.getByRole("button", { name: clickLabel }).first();
      if ((await btn.count()) > 0) await btn.click();
      await page.waitForTimeout(300);
    }
    await shot(page, `proto-${name}`);
  } catch (err) {
    failures.push(`proto ${name}: ${err instanceof Error ? err.message : String(err)}`);
  } finally {
    await page.close();
  }
}

try {
  await live("", "idle", async (page) => {
    // Creature must be full-body (no bust mask) at redesign size 48.
    const bust = await page.evaluate(() => {
      const c = document.querySelector(".creature");
      if (!c) return "no-creature";
      const style = getComputedStyle(c);
      return style.maskImage !== "none" && style.maskImage !== "" ? "bust-mask" : "ok";
    });
    if (bust === "bust-mask") failures.push("live idle: creature has bust mask");
    if (bust === "no-creature") failures.push("live idle: no creature");
    const size = await page.evaluate(() => {
      const c = document.querySelector(".creature");
      return c ? { w: c.offsetWidth, h: c.offsetHeight } : null;
    });
    if (size && (size.w < 40 || size.h < 40)) {
      failures.push(`live idle: creature too small (${size.w}x${size.h}); bust risk`);
    }
  });

  await live("", "glance", async (page) => {
    await page.mouse.move(450, 25);
    await page.waitForTimeout(600);
    const glass = await page.evaluate(() => {
      const shell = document.querySelector("[data-redesign]");
      if (!shell) return -1;
      const hits = [...shell.querySelectorAll("*")].filter((el) => {
        const cls = typeof el.className === "string" ? el.className : "";
        return /\bbg-white(?:\/|\b)/.test(cls);
      });
      return hits.length;
    });
    writeFileSync(join(out, "glance-glass-count.txt"), String(glass));
    if (glass > 0) failures.push(`live glance: ${glass} glass bg-white class markers under redesign`);
    if (!(await page.locator(".rd-glance, .rd-card-head").count())) {
      failures.push("live glance: missing rd-glance / rd-card-head structure");
    }
  });

  await live("ask", "ask", async (page) => {
    await page.keyboard.press("Control+Space").catch(() => {});
    await page.waitForTimeout(500);
    const ask = await page.locator(".ak, [data-redesign]").count();
    assert.ok(ask > 0, "Ask / redesign shell missing");
    const size = await page.evaluate(() => {
      const c = document.querySelector(".creature");
      return c ? { w: c.offsetWidth, h: c.offsetHeight } : null;
    });
    if (size && (size.w < 40 || size.h < 40)) {
      failures.push(`live ask: creature canvas too small (${size.w}x${size.h}); bust risk`);
    }
  });

  await prototype("idle", "Idle");
  await prototype("glance", "Glance");
  await prototype("ask", "Ask mode");

  const report = [
    "# Redesign parity",
    "",
    `Base: ${base}`,
    `Failures: ${failures.length}`,
    ...failures.map((f) => `- ${f}`),
    "",
    "PNGs written next to this report.",
  ].join("\n");
  writeFileSync(join(out, "report.md"), report);
  console.log(report);
  if (failures.length) {
    console.error("FAIL: redesign parity");
    process.exit(1);
  }
  console.log("PASS: redesign parity harness (screenshots + bust gate)");
} finally {
  await browser.close();
}
