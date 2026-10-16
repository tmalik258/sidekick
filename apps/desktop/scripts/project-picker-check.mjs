// Run against next dev --port 3002: node apps/desktop/scripts/project-picker-check.mjs
import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.DUSKIE_PREVIEW_URL ?? "http://localhost:3002";
const browser = await chromium.launch(process.env.CHROMIUM ? { executablePath: process.env.CHROMIUM } : {});
const errors = [];
try {
  for (const empty of [false, true]) {
    const page = await browser.newPage({ viewport: { width: 760, height: 900 }, reducedMotion: "reduce" });
    page.on("pageerror", (error) => errors.push(error.message));
    await page.goto(`${base}/island/?onboarded${empty ? "&noprojects" : ""}`);
    await page.mouse.move(380, 25);
    await page.getByRole("button", { name: /Ask Duskie/ }).click();
    await page.keyboard.press("Control+Tab");
    const project = page.getByRole("button", { name: "Project", exact: true });
    await project.click();
    await page.getByRole("textbox", { name: "Search Project" }).fill("no-matching-project");
    await page.getByRole("option", { name: /Choose another folder/ }).click();
    await project.getByText("Other project", { exact: true }).waitFor();
    await project.click();
    await page.getByRole("option", { name: /Other project/ }).waitFor();
    await page.keyboard.press("Escape");
    await page.getByRole("button", { name: "Board", exact: true }).click();
    await page.getByRole("button", { name: /Start or bring back/ }).click();
    await project.getByText("Other project", { exact: true }).waitFor();
    await project.focus();
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("ArrowDown");
    await project.getByText("Other project", { exact: true }).waitFor();
    await page.keyboard.press("Escape");
    await project.getByText("Other project", { exact: true }).waitFor();
    await page.reload();
    await page.mouse.move(380, 25);
    await page.getByRole("button", { name: /Ask Duskie/ }).click();
    await page.keyboard.press("Control+Tab");
    await page.getByRole("button", { name: "One", exact: true }).click();
    await project.click();
    await page.getByRole("option", { name: /Other project/ }).waitFor();
    console.log(`PASS: One and Board choose custom folders with ${empty ? "empty" : "populated"} project lists`);
    await page.close();
  }
  const page = await browser.newPage({ viewport: { width: 760, height: 900 }, reducedMotion: "reduce" });
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto(`${base}/island/?onboarded&foldercancel`);
  await page.mouse.move(380, 25);
  await page.getByRole("button", { name: /Ask Duskie/ }).click();
  await page.keyboard.press("Control+Tab");
  const project = page.getByRole("button", { name: "Project", exact: true });
  await project.getByText("sidekick", { exact: true }).waitFor();
  await project.click();
  await page.getByRole("option", { name: /Choose another folder/ }).click();
  await project.getByText("sidekick", { exact: true }).waitFor();
  assert.equal(await page.locator('.ak-err[role="alert"]').count(), 0);
  console.log("PASS: cancelling the folder dialog preserves the current project");
  await page.close();
  const failed = await browser.newPage({ viewport: { width: 760, height: 900 }, reducedMotion: "reduce" });
  await failed.goto(`${base}/island/?onboarded&folderfail`);
  await failed.mouse.move(380, 25);
  await failed.getByRole("button", { name: /Ask Duskie/ }).click();
  await failed.keyboard.press("Control+Tab");
  const failedProject = failed.getByRole("button", { name: "Project", exact: true });
  await failedProject.getByText("sidekick", { exact: true }).waitFor();
  await failedProject.click();
  await failed.getByRole("option", { name: /Choose another folder/ }).click();
  await failed.getByRole("alert").getByText("Could not save chosen folder", { exact: false }).waitFor();
  await failedProject.getByText("sidekick", { exact: true }).waitFor();
  await failedProject.click();
  assert.equal(await failed.getByRole("option", { name: /Other project/ }).count(), 0);
  await failed.close();
  console.log("PASS: folder save failure preserves selection and saved folders");
  assert.deepEqual(errors, [], "Browser runtime errors");
} finally {
  await browser.close();
}
