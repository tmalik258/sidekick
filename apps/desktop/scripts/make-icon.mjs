// Draws the placeholder app icon (the mascot's face) as a 1024x1024 PNG with
// no dependencies. Regenerate all icon sizes with:
//   node scripts/make-icon.mjs && pnpm tauri icon app-icon.png
import { writeFileSync } from "node:fs";
import { deflateSync } from "node:zlib";

const S = 1024;
const px = Buffer.alloc(S * S * 4);

const inEllipse = (x, y, cx, cy, rx, ry) => ((x - cx) / rx) ** 2 + ((y - cy) / ry) ** 2 <= 1;

for (let y = 0; y < S; y++) {
  for (let x = 0; x < S; x++) {
    let c = null;
    if (inEllipse(x, y, 512, 560, 400, 380)) c = [139, 124, 246];
    if (inEllipse(x, y, 512, 120, 60, 60)) c = [139, 124, 246];
    if (Math.abs(x - 512) < 18 && y > 120 && y < 200) c = [139, 124, 246];
    for (const ex of [352, 672]) {
      if (inEllipse(x, y, ex, 500, 84, 84)) c = [255, 255, 255];
      if (inEllipse(x, y, ex + 16, 520, 42, 42)) c = [27, 24, 48];
    }
    const my = 680 + 0.0016 * (x - 512) ** 2 * -1 + 40;
    if (x > 420 && x < 604 && Math.abs(y - my) < 14) c = [27, 24, 48];
    if (c) px.set([...c, 255], (y * S + x) * 4);
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
    chunk("IDAT", deflateSync(raw)),
    chunk("IEND", Buffer.alloc(0)),
  ]),
);
console.log("wrote app-icon.png");
