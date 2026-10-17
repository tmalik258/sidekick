// Bundles Monaco's workers into public/monaco so the static export serves
// them as plain files. The editor starts them from there on first open.
import { build } from "esbuild";

const workers = {
  editor: "monaco-editor/esm/vs/editor/editor.worker.js",
  json: "monaco-editor/esm/vs/language/json/json.worker.js",
  css: "monaco-editor/esm/vs/language/css/css.worker.js",
  html: "monaco-editor/esm/vs/language/html/html.worker.js",
  ts: "monaco-editor/esm/vs/language/typescript/ts.worker.js",
};

await build({
  entryPoints: Object.fromEntries(Object.entries(workers).map(([k, v]) => [`${k}.worker`, v])),
  bundle: true,
  minify: true,
  format: "iife",
  outdir: "public/monaco",
  logLevel: "warning",
});
