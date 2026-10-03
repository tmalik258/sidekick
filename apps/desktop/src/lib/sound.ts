// UI sounds from SND (https://snd.dev/), professionally designed interface
// sound kits by Dentsu Inc. and Starryworks Inc. Free to use; copyright of
// the audio stays with the credited sound designers. Each kit is one audio
// sprite (copied into public/sounds by scripts/sync-sounds.mjs) decoded once
// and sliced per sound. If a kit cannot load, a quiet synthesized tone is
// used instead so feedback never disappears.

import type { Cue, Settings } from "./types";

/** Sound names inside an SND sprite. */
export type SndSound =
  | "button"
  | "caution"
  | "celebration"
  | "disabled"
  | "notification"
  | "select"
  | "toggle_on"
  | "toggle_off"
  | "transition_up"
  | "transition_down";

/** Which designed sound plays for each mascot cue. */
const CUE_SOUND: Record<Cue, SndSound> = {
  chirp: "notification",
  pop: "transition_up",
  open: "toggle_on",
  ding: "celebration",
  boop: "caution",
  yawn: "transition_down",
  settle: "button", // unused: settle is synthesized, see powerUp
};

interface Kit {
  buffer: AudioBuffer;
  map: Record<string, { start: number; end: number }>;
}

let ctx: AudioContext | null = null;
let out: GainNode | null = null;
const kits = new Map<string, Promise<Kit | null>>();

function audio(): { ac: AudioContext; out: GainNode } {
  if (!ctx || !out) {
    ctx = new AudioContext();
    out = ctx.createGain();
    out.connect(ctx.destination);
  }
  if (ctx.state === "suspended") void ctx.resume();
  return { ac: ctx, out };
}

/** Last load error per kit, shown in settings so failures are never silent. */
export const kitErrors = new Map<string, string>();

async function decodeFirst(ac: AudioContext, id: string): Promise<AudioBuffer> {
  let lastError: unknown;
  for (const ext of ["m4a", "mp3", "ogg"]) {
    try {
      const res = await fetch(`/sounds/${id}/sprite.${ext}`);
      if (!res.ok) throw new Error(`HTTP ${res.status} for sprite.${ext}`);
      return await ac.decodeAudioData(await res.arrayBuffer());
    } catch (err) {
      lastError = err;
    }
  }
  throw lastError;
}

async function fetchSpriteMap(id: string): Promise<Record<string, { start: number; end: number }>> {
  const res = await fetch(`/sounds/${id}/sprite.json`);
  if (!res.ok) {
    throw new Error(
      `HTTP ${res.status} for /sounds/${id}/sprite.json (run pnpm install or pnpm --filter desktop dev to sync kits)`,
    );
  }
  const json: { spritemap: Record<string, { start: number; end: number }> } = await res.json();
  return json.spritemap;
}

function loadKit(id: string): Promise<Kit | null> {
  let kit = kits.get(id);
  if (!kit) {
    const { ac } = audio();
    kit = Promise.all([fetchSpriteMap(id), decodeFirst(ac, id)])
      .then(([map, buffer]) => {
        kitErrors.delete(id);
        return { buffer, map };
      })
      .catch((err) => {
        kitErrors.set(id, String(err));
        console.warn(`sound kit ${id} unavailable, using fallback tone`, err);
        kits.delete(id); // retry on the next play
        return null;
      });
    kits.set(id, kit);
  }
  return kit;
}

/** Loads a kit and reports whether it decoded, for the settings screen. */
export async function checkKit(id: string): Promise<string | null> {
  const kit = await loadKit(id);
  return kit ? null : (kitErrors.get(id) ?? "unknown error");
}

/** Starts decoding a kit ahead of the first cue so playback is instant. */
export function preloadSounds(kitId: string): void {
  if (typeof window !== "undefined") void loadKit(kitId);
}

export function cueVolume(settings: Settings, cue: Cue): number {
  if (settings.muted) return 0;
  return settings.masterVolume * (settings.cueVolumes[cue] ?? 1);
}

export function playCue(cue: Cue, volume: number, kitId: string): void {
  if (cue === "settle") {
    if (volume > 0 && typeof window !== "undefined") {
      const { ac, out } = audio();
      powerUp(ac, out, volume);
    }
    return;
  }
  playSound(CUE_SOUND[cue], volume, kitId);
}

/** Plays one sound from a kit. Used for cues and for chip presses. */
export function playSound(sound: SndSound, volume: number, kitId: string): void {
  if (volume <= 0 || typeof window === "undefined") return;
  void loadKit(kitId).then((kit) => {
    const { ac, out } = audio();
    const slice = kit?.map[sound];
    if (!kit || !slice) return fallbackTone(ac, out, volume);
    const src = ac.createBufferSource();
    const gain = ac.createGain();
    src.buffer = kit.buffer;
    gain.gain.value = volume;
    src.connect(gain).connect(out);
    src.start(0, slice.start, slice.end - slice.start);
  });
}

function fallbackTone(ac: AudioContext, out: AudioNode, volume: number) {
  const now = ac.currentTime;
  const osc = ac.createOscillator();
  const amp = ac.createGain();
  osc.frequency.value = 880;
  amp.gain.setValueAtTime(0.0001, now);
  amp.gain.exponentialRampToValueAtTime(Math.max(0.15 * volume, 0.0002), now + 0.005);
  amp.gain.exponentialRampToValueAtTime(0.0001, now + 0.18);
  osc.connect(amp).connect(out);
  osc.start(now);
  osc.stop(now + 0.2);
}

/**
 * A machine powering on, kept short and quiet: a low hum spins up through an
 * opening filter, a faint FM shimmer climbs over it, and one soft ping lands
 * at the end. Fully synthesized, so it needs no kit and stays original.
 */
function powerUp(ac: AudioContext, out: AudioNode, volume: number) {
  const t = ac.currentTime;
  const peak = Math.max(0.1 * volume, 0.0002);
  const master = ac.createGain();
  master.gain.setValueAtTime(0.0001, t);
  master.gain.exponentialRampToValueAtTime(peak, t + 0.05);
  master.gain.setValueAtTime(peak, t + 0.2);
  master.gain.exponentialRampToValueAtTime(0.0001, t + 0.38);
  master.connect(out);

  // Hum: two detuned saws sweeping up, the filter opening as they rise.
  const filter = ac.createBiquadFilter();
  filter.type = "lowpass";
  filter.Q.value = 5;
  filter.frequency.setValueAtTime(260, t);
  filter.frequency.exponentialRampToValueAtTime(2200, t + 0.24);
  filter.connect(master);
  for (const detune of [-9, 9]) {
    const saw = ac.createOscillator();
    saw.type = "sawtooth";
    saw.detune.value = detune;
    saw.frequency.setValueAtTime(90, t);
    saw.frequency.exponentialRampToValueAtTime(260, t + 0.24);
    saw.connect(filter);
    saw.start(t);
    saw.stop(t + 0.4);
  }

  // Shimmer: FM with a fast modulator gives the metallic, alien edge.
  const carrier = ac.createOscillator();
  const mod = ac.createOscillator();
  const depth = ac.createGain();
  const shimmer = ac.createGain();
  carrier.type = "sine";
  carrier.frequency.setValueAtTime(520, t);
  carrier.frequency.exponentialRampToValueAtTime(1320, t + 0.24);
  mod.type = "sine";
  mod.frequency.setValueAtTime(110, t);
  mod.frequency.exponentialRampToValueAtTime(340, t + 0.24);
  depth.gain.setValueAtTime(100, t);
  depth.gain.linearRampToValueAtTime(320, t + 0.24);
  shimmer.gain.value = 0.25;
  mod.connect(depth).connect(carrier.frequency);
  carrier.connect(shimmer).connect(master);
  for (const o of [carrier, mod]) {
    o.start(t);
    o.stop(t + 0.4);
  }

  // Ready ping once the machine is up.
  const ping = ac.createOscillator();
  const amp = ac.createGain();
  ping.type = "triangle";
  ping.frequency.value = 1760;
  amp.gain.setValueAtTime(0.0001, t + 0.2);
  amp.gain.exponentialRampToValueAtTime(0.35, t + 0.21);
  amp.gain.exponentialRampToValueAtTime(0.0001, t + 0.34);
  ping.connect(amp).connect(master);
  ping.start(t + 0.2);
  ping.stop(t + 0.38);
}
