// Execute the production startup guards with deferred backend responses.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import ts from "typescript";

const source = readFileSync(new URL("../src/lib/store.ts", import.meta.url), "utf8");
const init = source.slice(
  source.indexOf("  let disposed = false;", source.indexOf("export function connect")),
  source.indexOf("  void api.focusStatus()", source.indexOf("export function connect")),
);
const snapshots = source.slice(
  source.indexOf("    const progressSnapshotRevision"),
  source.indexOf("    const settingsSnapshotRevision"),
);
for (const change of ["none", "event", "local"]) {
  let state = { update: null, updateProgress: null };
  const subscriptions = [];
  const deferred = {};
  const useDuskie = {
    subscribe(fn) {
      subscriptions.push(fn);
      return () => {};
    },
    setState(patch) {
      const previous = state;
      state = { ...state, ...patch };
      subscriptions.forEach((fn) => {
        fn(state, previous);
      });
    },
  };
  const api = Object.fromEntries(
    ["updateProgress", "updateStatus"].map((key) => [
      key,
      () =>
        new Promise((resolve) => {
          deferred[key] = resolve;
        }),
    ]),
  );
  const script = init + snapshots + (change === "event" ? "progressRevision++; updateRevision++;" : "");
  vm.runInNewContext(ts.transpileModule(script, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, {
    useDuskie,
    api,
    EVENTS: {},
    acceptActivities() {},
    listen: () => new Promise(() => {}),
  });
  const live = { phase: "downloading", done: 50, total: 100 };
  if (change !== "none") useDuskie.setState({ updateProgress: live, update: { version: "new" } });
  deferred.updateProgress(null);
  deferred.updateStatus({ version: "old" });
  await Promise.resolve();
  assert.equal(state.updateProgress, change === "none" ? null : live);
  assert.equal(state.update.version, change === "none" ? "old" : "new");
}
const confirm = source.slice(
  source.indexOf("let confirmedCodeFolders"),
  source.indexOf("export function updateSettings"),
);
let state = {};
const calls = [];
const context = {
  useDuskie: {
    setState(patch) {
      state = { ...state, ...patch };
    },
  },
  revalidate(key) {
    calls.push(key);
  },
  api: {},
};
vm.runInNewContext(
  ts.transpileModule(confirm, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText +
    "\nthis.confirm = confirmSettings;",
  context,
);
context.confirm({ codeFolders: ["a"], savedProjects: [] });
state.settings = { codeFolders: ["a", "b"], savedProjects: [] }; // Optimistic same-window edit.
context.confirm({ codeFolders: ["a", "b"], savedProjects: [] });
context.confirm({ codeFolders: ["a", "b"], savedProjects: [] }); // Command response after event.
context.confirm({ codeFolders: ["a"], savedProjects: [] });
assert.deepEqual(calls, ["projects", "repos", "projects", "repos"]);
console.log("Startup snapshot ordering and confirmed folder refresh checks passed");
