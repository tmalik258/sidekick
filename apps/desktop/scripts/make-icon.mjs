// Draws the app icon to match the live pearl orb: dark circular field,
// white/silver face, two solid black vertical pill eyes (no pupils/mouth).
// Proportions follow .orb-eye in globals.css (~11% × 26%, ~15% gap).
// Regenerate all Tauri icon sizes with:
//   node scripts/make-icon.mjs && pnpm tauri icon app-icon.png
import { writeFileSync } from "node:fs";
import { deflateSync } from "node:zlib";

const S = 1024;
const px = Buffer.alloc(S * S * 4);

const clamp = (v, a, b) => Math.max(a, Math.min(b, v));
const mix = (a, b, t) => a + (b - a) * t;
const inCircle = (x, y, cx, cy, r) => (x - cx) ** 2 + (y - cy) ** 2 <= r * r;
const inEllipse = (x, y, cx, cy, rx, ry) => ((x - cx) / rx) ** 2 + ((y - cy) / ry) ** 2 <= 1;

// Pearl body (THEME_STYLES.pearl) + dark field like the island behind the orb.
const FIELD = [8, 8, 10];
const BODY_DARK = [169, 173, 185];
const BODY_LIGHT = [255, 255, 255];
const EYE = [28, 28, 30];

const CX = 512;
const CY = 512;
const FIELD_R = 508;
const FACE_R = 390;

for (let y = 0; y < S; y++) {
  for (let x = 0; x < S; x++) {
    if (!inCircle(x, y, CX, CY, FIELD_R)) continue;

    let c = FIELD;
    let a = 255;

    const dx = x - CX;
    const dy = y - CY;
    const dist = Math.hypot(dx, dy);

    if (dist <= FACE_R) {
      const nx = dx / FACE_R;
      const ny = dy / FACE_R;
      // Highlight upper-left, soft gray lower-right.
      const lit = clamp(0.62 - 0.45 * nx - 0.7 * ny, 0, 1);
      const shade = lit ** 0.9;
      c = [
        Math.round(mix(BODY_DARK[0], BODY_LIGHT[0], shade)),
        Math.round(mix(BODY_DARK[1], BODY_LIGHT[1], shade)),
        Math.round(mix(BODY_DARK[2], BODY_LIGHT[2], shade)),
      ];

      // Specular sheen.
      const sx = (x - (CX - FACE_R * 0.28)) / (FACE_R * 0.5);
      const sy = (y - (CY - FACE_R * 0.4)) / (FACE_R * 0.38);
      const spec = Math.exp(-(sx * sx + sy * sy) * 2.4);
      c = [
        Math.round(mix(c[0], 255, spec * 0.45)),
        Math.round(mix(c[1], 255, spec * 0.45)),
        Math.round(mix(c[2], 255, spec * 0.5)),
      ];
    }

    // Solid black pill eyes (.orb-eye: ~11% wide, ~26% tall of face).
    const eyeW = FACE_R * 2 * 0.11;
    const eyeH = FACE_R * 2 * 0.26;
    const gap = FACE_R * 2 * 0.15;
    const eyeY = CY - FACE_R * 0.02;
    for (const ex of [CX - gap / 2 - eyeW / 2, CX + gap / 2 + eyeW / 2]) {
      if (inEllipse(x, y, ex, eyeY, eyeW / 2, eyeH / 2)) {
        c = EYE;
      }
    }

    px.set([c[0], c[1], c[2], a], (y * S + x) * 4);
  }
}

const crcTable = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
const crc32 = (buf) => {
  let c = 0xffffffff;
  for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
};
const chunk = (type, data) => {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const td = Buffer.concat([Buffer.from(type), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(td));
  return Buffer.concat([len, td, crc]);
};

const raw = Buffer.alloc(S * (S * 4 + 1));
for (let y = 0; y < S; y++) px.copy(raw, y * (S * 4 + 1) + 1, y * S * 4, (y + 1) * S * 4);
const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(S, 0);
ihdr.writeUInt32BE(S, 4);
ihdr.set([8, 6, 0, 0, 0], 8);

writeFileSync(
  new URL("../app-icon.png", import.meta.url),
  Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]),
);
console.log("wrote app-icon.png");
