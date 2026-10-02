// Prefetches the pinned sherpa-onnx / Kokoro voice models into
// src-tauri/resources/voice-models so first paint never waits on GitHub.
// Mirrors crates/voice/src/models.rs (URLs, SHA-256, dirs, required files).
// Skips work when every model is already unpacked. Gitignored; not committed.
// Kokoro first: the welcome greeting depends on it.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { createWriteStream, existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const OUT = join(dirname(fileURLToPath(import.meta.url)), "..", "src-tauri", "resources", "voice-models");

const MODELS = [
  {
    id: "wake",
    label: "Wake word",
    url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/kws-models/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01.tar.bz2",
    sha256: "f170013b4716e41b62b9bfd809687c207cef798ef9bc6534d524e17af9b6561a",
    dir: "sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01",
    files: [
      "encoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx",
      "decoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx",
      "joiner-epoch-12-avg-2-chunk-16-left-64.int8.onnx",
      "tokens.txt",
    ],
  },
  {
    id: "speech",
    label: "Speech to text",
    url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-streaming-zipformer-en-kroko-2025-08-06.tar.bz2",
    sha256: "c8676e5ff9ac2a85296e53ee0fd4d5fb1db6770e7a7647166eeafe349ade6834",
    dir: "sherpa-onnx-streaming-zipformer-en-kroko-2025-08-06",
    files: ["encoder.onnx", "decoder.onnx", "joiner.onnx", "tokens.txt"],
  },
  {
    id: "kokoro",
    label: "Kokoro voice",
    url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/kokoro-multi-lang-v1_0.tar.bz2",
    sha256: "c5f7e2d2caf082bc1d20fb70334a61d99d20b484500aad32e7cf84c128ea3298",
    dir: "kokoro-multi-lang-v1_0",
    files: [
      "model.onnx",
      "voices.bin",
      "tokens.txt",
      "lexicon-us-en.txt",
      "lexicon-gb-en.txt",
      "espeak-ng-data",
      "dict",
    ],
    // Chinese only; models.rs skips the same.
    exclude: ["*lexicon-zh.txt", "*-zh.fst"],
  },
];

function installed(model) {
  const root = join(OUT, model.dir);
  return model.files.every((f) => existsSync(join(root, f)));
}

function sha256File(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

async function download(url, dest) {
  const res = await fetch(url, { headers: { "User-Agent": "Sidekick" } });
  if (!res.ok) throw new Error(`${url}: HTTP ${res.status}`);
  const total = Number(res.headers.get("content-length") ?? 0);
  if (!res.body) throw new Error(`${url}: empty body`);
  mkdirSync(dirname(dest), { recursive: true });
  const file = createWriteStream(dest);
  const reader = res.body.getReader();
  let done = 0;
  let lastLog = 0;
  for (;;) {
    const { value, done: end } = await reader.read();
    if (end) break;
    file.write(Buffer.from(value));
    done += value.byteLength;
    if (total && done - lastLog >= 1024 * 1024) {
      lastLog = done;
      const pct = ((100 * done) / total).toFixed(0);
      process.stdout.write(`\r  ${pct}% (${(done / 1e6).toFixed(1)} / ${(total / 1e6).toFixed(1)} MB)`);
    }
  }
  await new Promise((resolve, reject) => {
    file.end(resolve);
    file.on("error", reject);
  });
  if (total) process.stdout.write("\n");
}

// Replaced models, removed so they are not bundled (models.rs RETIRED).
const RETIRED = ["kokoro-int8-en-v0_19"];

function unpackBz2(archive, destRoot, dirName, exclude = []) {
  const staging = join(destRoot, `.unpack-${dirName}`);
  rmSync(staging, { recursive: true, force: true });
  mkdirSync(staging, { recursive: true });
  // Windows 10+ ships bsdtar as tar.exe; it reads .tar.bz2.
  const skip = exclude.flatMap((p) => ["--exclude", p]);
  execFileSync("tar", ["-xjf", archive, "-C", staging, ...skip], { stdio: "inherit" });
  const unpacked = join(staging, dirName);
  if (!existsSync(unpacked)) {
    throw new Error(`archive did not contain ${dirName}`);
  }
  const target = join(destRoot, dirName);
  rmSync(target, { recursive: true, force: true });
  renameSync(unpacked, target);
  rmSync(staging, { recursive: true, force: true });
}

async function ensure(model) {
  if (installed(model)) {
    console.log(`ok  ${model.label}`);
    return;
  }
  console.log(`get ${model.label}`);
  mkdirSync(OUT, { recursive: true });
  const part = join(OUT, `${model.id}.part`);
  await download(model.url, part);
  const hash = sha256File(part);
  if (hash !== model.sha256) {
    rmSync(part, { force: true });
    throw new Error(`${model.label} checksum mismatch (got ${hash})`);
  }
  unpackBz2(part, OUT, model.dir, model.exclude);
  rmSync(part, { force: true });
  if (!installed(model)) {
    throw new Error(`${model.label} unpack incomplete`);
  }
  console.log(`ok  ${model.label}`);
}

async function main() {
  mkdirSync(OUT, { recursive: true });
  writeFileSync(join(OUT, ".keep"), "");
  for (const id of ["kokoro", "wake", "speech"]) {
    await ensure(MODELS.find((m) => m.id === id));
  }
  for (const dir of RETIRED) rmSync(join(OUT, dir), { recursive: true, force: true });
  console.log("voice models ready");
}

main().catch((err) => {
  console.error(err);
  // Offline / CI without network: Rust still downloads at runtime if needed.
  console.warn("voice model prefetch failed; runtime will download if needed");
  process.exit(0);
});
