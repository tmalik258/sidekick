import assert from "node:assert/strict";
import { webcrypto } from "node:crypto";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import ts from "typescript";

let now = 1_000_000;
class Clock extends Date {
  static now() {
    return now;
  }
}
function store(initializer) {
  let state = initializer();
  return {
    getState: () => state,
    setState: (patch) => {
      state = { ...state, ...(typeof patch === "function" ? patch(state) : patch) };
    },
  };
}
const text = (name) => readFileSync(new URL(`../src/lib/${name}.ts`, import.meta.url), "utf8");
const context = {
  exports: {},
  require: (name) => {
    assert.equal(name, "zustand");
    return { create: store };
  },
  Date: Clock,
  crypto: webcrypto,
};
vm.runInNewContext(
  ts.transpileModule(text("activities"), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText,
  context,
);
const a = context.exports;
const activity = {
  id: "one",
  source: "action",
  parentId: null,
  label: "Opening sidekick in Cursor",
  phase: "running",
  startedAt: now,
  lastProgressAt: now,
  transferred: null,
  total: null,
  outcome: null,
  dismissed: false,
  revision: 1,
};
now += 119_000;
assert.match(a.activityLabel(activity, now), /119s$/);
now += 1000;
assert.match(a.activityLabel(activity, now), /no confirmed progress for 2m$/);
assert.match(a.activityLabel({ ...activity, lastProgressAt: now }, now), /120s$/);
const downloading = {
  ...activity,
  source: "browser",
  phase: "downloading",
  label: "Downloading report.zip in Chrome",
  transferred: 18_000_000,
};
assert.match(a.activityLabel(downloading, now), /no new data for 2m$/);
assert.match(a.activityLabel({ ...downloading, lastProgressAt: now }, now), /18 MB received$/);
assert.match(a.activityLabel({ ...downloading, total: 36_000_000, lastProgressAt: now }, now), /50% · 18 MB of 36 MB$/);
assert.match(a.activityLabel({ ...downloading, phase: "awaitingUser" }, now), /^Download paused/);
assert.match(a.activityLabel({ ...downloading, phase: "failed", outcome: "NETWORK_FAILED" }, now), /NETWORK_FAILED/);
assert.equal(a.ownsWorking(downloading), false, "unrelated downloads do not own the mascot");
assert.equal(a.ownsWorking({ ...activity, phase: "completed" }), false);
assert.equal(
  a.latestActivity(
    [
      { ...activity, id: "update-old", source: "update", phase: "failed", dismissed: true, startedAt: 1 },
      { ...activity, id: "update-new", source: "update", startedAt: 2 },
    ],
    "update",
  ).id,
  "update-new",
  "a failed or dismissed attempt cannot replace the new retry",
);
a.acceptActivities({ revision: 2, activities: [{ ...activity, phase: "completed" }] });
a.acceptActivities({ revision: 1, activities: [activity] });
assert.equal(a.useActivities.getState().activities[0].phase, "completed", "late snapshot cannot restore running state");
a.acceptActivities({ revision: 2, activities: [activity] });
assert.equal(a.useActivities.getState().activities[0].phase, "completed");
const local = a.beginActivity("Opening project");
a.dismissLocal(local);
assert.equal(a.useActivities.getState().local[0].dismissed, true);
a.finishActivity(local, true, "Launch requested");
assert.equal(a.useActivities.getState().local[0].dismissed, false, "terminal result restores dismissed operation");
assert.equal(a.visibleActivities([{ ...downloading, dismissed: true }], now).length, 0);

let state;
const outcomes = [];
const useDuskie = {
  getState: () => state,
  setState: (patch) => {
    state = { ...state, ...(typeof patch === "function" ? patch(state) : patch) };
  },
};
const source = text("store");
const runSource = source.slice(
  source.indexOf("let openGeneration"),
  source.indexOf("/** How long the island keeps the Done card"),
);
const openContext = {
  ...context,
  useDuskie,
  beginActivity: a.beginActivity,
  finishActivity: a.finishActivity,
  ownsWorking: a.ownsWorking,
  useActivities: a.useActivities,
  api: { askClose() {}, activityOpenStart: async () => {}, activityOpenFinish: async () => {} },
  showDone(message) {
    outcomes.push(message);
    useDuskie.setState({ mascot: "success" });
  },
};
vm.runInNewContext(
  `${ts
    .transpileModule(runSource, { compilerOptions: { target: ts.ScriptTarget.ES2022 } })
    .outputText.replace("export async function", "async function")}\nthis.runOpen = runOpen;`,
  openContext,
);
for (const result of [true, false, new Error("Launch failed")]) {
  state = { mascot: "idle", running: null };
  const ok = await openContext.runOpen(async () => {
    if (result instanceof Error) throw result;
    return result;
  }, "Opened sidekick in Cursor");
  assert.equal(ok, result === true);
  assert.equal(state.running, null);
  assert.notEqual(state.mascot, "working");
  if (result instanceof Error) assert.equal(state.mascot, "error");
}
assert.equal(outcomes[0], "Launch requested for sidekick in Cursor", "launch does not prove a window opened");
for (const olderResult of [true, false, new Error("Old failure")]) {
  let oldResolve, newResolve;
  state = { mascot: "idle", running: null };
  const older = openContext.runOpen(
    () =>
      new Promise((resolve, reject) => {
        oldResolve = () => (olderResult instanceof Error ? reject(olderResult) : resolve(olderResult));
      }),
    "Opened old",
  );
  await new Promise(setImmediate);
  const newer = openContext.runOpen(
    () =>
      new Promise((resolve) => {
        newResolve = resolve;
      }),
    "Opened new",
  );
  await new Promise(setImmediate);
  oldResolve();
  await older;
  assert.equal(state.running, "Opening new");
  assert.equal(state.mascot, "working");
  newResolve(false);
  await newer;
  assert.equal(state.running, null);
  assert.equal(state.mascot, "idle");
}
console.log(
  "PASS: inactivity boundary, byte labels, pause/failure, snapshots, dismissal, runOpen exits/concurrency, mascot ownership",
);
