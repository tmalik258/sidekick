// Run with the browser preview on port 3002.
import assert from "node:assert/strict";
import { chromium } from "playwright";

const browser = await chromium.launch(process.env.CHROMIUM ? { executablePath: process.env.CHROMIUM } : {});
const errors = [];
const base = process.env.DUSKIE_PREVIEW_URL ?? "http://localhost:3002";
async function preview(query, test) {
  const page = await browser.newPage({ viewport: { width: 900, height: 800 }, reducedMotion: "reduce" });
  page.on("pageerror", (e) => errors.push(e.message));
  try {
    await page.goto(`${base}/island/?onboarded&${query}`);
    await test(page);
  } finally {
    await page.close();
  }
}
async function details(page) {
  await page.mouse.move(450, 25);
}
try {
  for (const query of ["orphan", "orphan&activity=download"]) {
    await preview(query, async (page) => {
      await page.getByRole("img", { name: "Duskie is idle", exact: true }).waitFor();
      assert.equal(await page.getByRole("img", { name: "Duskie is working", exact: true }).count(), 0);
    });
  }
  await preview("activity=action", async (page) => {
    await page
      .getByText(/Opening sidekick in Cursor.*\d+s/)
      .first()
      .waitFor();
    await details(page);
    await page.getByRole("button", { name: "Check again", exact: true }).waitFor();
    await page.getByRole("button", { name: "Dismiss", exact: true }).click();
    await page.getByText(/Opening sidekick in Cursor/).waitFor({ state: "hidden" });
  });
  await preview("activity=download", async (page) => {
    await page
      .getByText(/Downloading report.zip in Chrome · 18 MB received/)
      .first()
      .waitFor();
    assert.equal(await page.getByText(/18 MB of/).count(), 0);
  });
  await preview("activity=download&knownTotal", async (page) => {
    await page
      .getByText(/50% · 18 MB of 36 MB/)
      .first()
      .waitFor();
  });
  await preview("activity=stale&activityrecover", async (page) => {
    await page
      .getByText(/no new data for 2m/)
      .first()
      .waitFor();
    await page.getByRole("img", { name: "Waiting for confirmed progress" }).waitFor();
    await details(page);
    await page.getByRole("button", { name: "Check again", exact: true }).click();
    await page
      .getByText(/19 MB received/)
      .first()
      .waitFor();
    assert.equal(await page.getByText(/no new data/).count(), 0);
  });
  await preview("activity=paused", async (page) => {
    await page
      .getByText(/Download paused/)
      .first()
      .waitFor();
  });
  await preview("activity=group", async (page) => {
    await page.getByText("2 downloads in progress", { exact: true }).waitFor();
    await details(page);
    await page
      .getByText(/Downloading report.zip/)
      .first()
      .waitFor();
    await page
      .getByText(/Downloading photo.zip/)
      .first()
      .waitFor();
    assert.equal(await page.getByRole("button", { name: "Check again", exact: true }).count(), 2);
  });
  assert.deepEqual(errors, []);
  console.log(
    "PASS: browser labels/elapsed, totals, waiting/recovery, pause, Check again, Dismiss, grouped download details",
  );
} finally {
  await browser.close();
}
