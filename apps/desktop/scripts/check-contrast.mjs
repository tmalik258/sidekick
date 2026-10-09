// Checks that text on the island is readable on every island colour: each
// text colour the components use, over each background in globals.css, laid
// over a white and a black desktop (the glass is not fully opaque).
// Text at 50% white or more is body text and needs 4.5:1 (WCAG AA); fainter
// text is for hints and key badges and needs 3:1.

import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dirname, "..", "src");
const css = readFileSync(join(root, "app", "globals.css"), "utf8");

/** "rgb(10 10 12 / 0.94)" or "#000" to [r, g, b, a]. */
function parse(c) {
  c = c.trim();
  if (c === "transparent") return null;
  if (c.startsWith("#")) {
    const h = c.slice(1);
    const full = h.length === 3 ? [...h].map((x) => x + x).join("") : h;
    return [0, 2, 4].map((i) => Number.parseInt(full.slice(i, i + 2), 16)).concat(1);
  }
  const m = c.match(/rgba?\(([\d.]+)[ ,_]+([\d.]+)[ ,_]+([\d.]+)(?:[ ,_]*\/?[ ,_]*([\d.]+))?\)/);
  if (!m) return null;
  return [Number(m[1]), Number(m[2]), Number(m[3]), m[4] === undefined ? 1 : Number(m[4])];
}

const over = ([r, g, b, a], [R, G, B]) => [r * a + R * (1 - a), g * a + G * (1 - a), b * a + B * (1 - a)];

function luminance(rgb) {
  const [r, g, b] = rgb.map((v) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

const ratio = (a, b) => {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
};

// Island backgrounds: the default and each [data-color].
const backgrounds = {};
const base = css.match(/\.island-shell\s*\{[^}]*--isl-bg:\s*([^;]+);/);
if (base) backgrounds.default = base[1];
for (const m of css.matchAll(/\.island-shell\[data-color="([^"]+)"\]\s*\{[^}]*--isl-bg:\s*([^;]+);/g)) {
  backgrounds[m[1]] = m[2];
}

// Text colours used in components and the island's styles.
function files(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
    e.isDirectory() ? files(join(dir, e.name)) : e.name.endsWith(".tsx") ? [join(dir, e.name)] : [],
  );
}
const texts = new Set();
for (const f of files(join(root, "components"))) {
  const src = readFileSync(f, "utf8");
  for (const m of src.matchAll(/text-\[(rgb\(235_235_245\/[\d.]+\))\]/g)) texts.add(m[1].replaceAll("_", " "));
  for (const m of src.matchAll(/text-white\/(\d+)/g)) texts.add(`rgb(255 255 255 / ${Number(m[1]) / 100})`);
}
const akBlock = css.slice(css.indexOf(".ak {"));
for (const m of akBlock.matchAll(/(?<![-\w])color:\s*(rgb\([^)]+\))/g)) texts.add(m[1]);
// Text tokens the island's styles use (--i2, --i3, --muted...).
for (const m of css.matchAll(/--(i\d|muted):\s*(rgb\(235 235 245[^)]*\))/g)) texts.add(m[2]);
texts.add("rgb(255 255 255 / 1)");

const desktops = { "white desktop": [255, 255, 255], "black desktop": [0, 0, 0] };
let failed = 0;
const rows = [];
for (const [name, bgCss] of Object.entries(backgrounds)) {
  const bg = parse(bgCss);
  if (!bg) continue;
  for (const [deskName, desk] of Object.entries(desktops)) {
    const surface = over(bg, desk);
    for (const t of texts) {
      const fg = parse(t);
      if (!fg) continue;
      const need = fg[3] >= 0.5 ? 4.5 : 3;
      const r = ratio(over(fg, surface), surface);
      if (r < need) {
        failed++;
        rows.push(`  ${name} (${deskName}): ${t} is ${r.toFixed(2)}:1, needs ${need}:1`);
      }
    }
  }
}
console.log(`Checked ${texts.size} text colours on ${Object.keys(backgrounds).length} island colours.`);
if (failed) {
  console.error(`${failed} too faint:\n${rows.join("\n")}`);
  process.exit(1);
}
