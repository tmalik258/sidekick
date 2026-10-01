// Copies the SND UI sound kits from node_modules into public/sounds so the
// app serves them offline. Runs before dev and build; the copies are not
// committed.
//
// Sounds: SND (https://snd.dev/) by Dentsu Inc. and Starryworks Inc. Free to
// use; copyright of the audio stays with the credited sound designers.
import { copyFileSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const pkg = dirname(require.resolve("snd-lib/package.json"));
const out = join(dirname(fileURLToPath(import.meta.url)), "..", "public", "sounds");

for (const kit of ["01", "02", "03"]) {
  const src = join(pkg, "assets", "sounds", "sprite", kit);
  const dest = join(out, kit);
  mkdirSync(dest, { recursive: true });
  copyFileSync(join(src, "audioSprite.ogg"), join(dest, "sprite.ogg"));
  copyFileSync(join(src, "audioSprite.json"), join(dest, "sprite.json"));
}
console.log("sounds synced");
