// Run against the browser preview: next dev --port 3002, then node scripts/update-check.mjs.
import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.DUSKIE_PREVIEW_URL ?? "http://localhost:3002";
const browser = await chromium.launch(process.env.CHROMIUM ? { executablePath: process.env.CHROMIUM } : {});
const errors = [];

async function preview(query, test) {
  const page = await browser.newPage({ viewport: { width: 760, height: 760 }, reducedMotion: "reduce" });
  page.on("pageerror", (error) => errors.push(error.message));
  try {
    await page.goto(`${base}/island/${query}`);
    await test(page);
  } finally {
    await page.close();
  }
}

async function hover(page) {
  await page.mouse.move(380, 25);
}

async function downloading(page) {
  const bar = page.getByRole("progressbar", { name: "Update download" });
  await bar.waitFor();
  await page.waitForFunction(
    () => Number(document.querySelector('[aria-label="Update download"]')?.getAttribute("aria-valuenow")) > 0,
  );
  return bar;
}

try {
  await preview("?onboarded&update", async (page) => {
    await hover(page);
    await page.getByRole("button", { name: "Update to 0.2.0" }).click();
    await page.getByText("Preparing update...", { exact: true }).waitFor();
    await downloading(page);
    await page.mouse.move(20, 600);
    await page.getByText(/Downloading update · \d+%/).waitFor();
    await hover(page);
    await page.getByRole("progressbar", { name: "Update download" }).waitFor();
    await page.getByText("Opening installer...", { exact: true }).waitFor();
    console.log("PASS: island acknowledges click and shows download and installer progress");
  });

  await preview("?onboarded&update", async (page) => {
    await hover(page);
    await page.getByRole("button", { name: "Settings", exact: true }).click();
    await page.getByText("Startup and updates", { exact: true }).click();
    const section = page
      .locator("section")
      .filter({ has: page.getByRole("heading", { name: "Startup and updates", exact: true }) });
    await section.getByRole("button", { name: "Install", exact: true }).click();
    await page.getByText("Preparing update...", { exact: true }).waitFor();
    assert.equal(await page.getByRole("button", { name: "Updating...", exact: true }).isDisabled(), true);
    await downloading(page);
    await page.getByText("Verifying download...", { exact: true }).waitFor();
    await page.getByText("Opening installer...", { exact: true }).waitFor();
    console.log("PASS: Settings shows real progress and prevents duplicate clicks");
  });

  await preview("?onboarded&update&updateerror", async (page) => {
    await hover(page);
    await page.getByRole("button", { name: "Update to 0.2.0" }).click();
    await page.getByRole("button", { name: "Retry update", exact: true }).waitFor();
    await page.getByText(/The download did not match its checksum/).waitFor();
    await page.getByRole("button", { name: "Retry update", exact: true }).click();
    await page.getByText("Preparing update...", { exact: true }).waitFor();
    await page.getByRole("button", { name: "Retry update", exact: true }).waitFor();
    console.log("PASS: update errors are visible and can be retried");
  });

  await preview("?approval", async (page) => {
    // Welcome exposes Hide after its spoken introduction finishes.
    await page.getByRole("button", { name: "Hide", exact: true }).click({ timeout: 90_000 });
    await page.mouse.move(20, 600);
    await page.getByText("Allow this connection?", { exact: true }).waitFor();
    await hover(page);
    await page.waitForTimeout(800);
    assert.equal(await page.getByRole("button", { name: "Hide", exact: true }).count(), 0);
    await page.getByRole("button", { name: /Accept/ }).click();
    await page.getByText("Allow this connection?", { exact: true }).waitFor({ state: "hidden" });
    console.log("PASS: pending approval takes priority over Welcome and remains clickable");
  });

  assert.deepEqual(errors, [], "Browser runtime errors");
} finally {
  await browser.close();
}
