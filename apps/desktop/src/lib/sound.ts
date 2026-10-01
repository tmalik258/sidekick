// Sound cues, synthesized with the Web Audio API so the repo ships no audio
// files. Soft sine partials through a gentle low-pass and a short room
// reflection, tuned to sit under the user's music rather than over it.
// Replace with original recorded sounds before the public release.

import type { Cue, Settings } from "./types";

interface Note {
  /** Start frequency, Hz. */
  freq: number;
  /** Optional glide target, Hz. */
  to?: number;
  /** Seconds after the cue starts. */
  at?: number;
  /** Seconds until silent. */
  decay: number;
  gain: number;
  type?: OscillatorType;
}

// Pitches from a pentatonic set so cues never clash with each other.
const CUES: Record<Cue, Note[]> = {
  // Something caught its eye: two soft rising notes.
  chirp: [
    { freq: 880, decay: 0.12, gain: 0.22 },
    { freq: 1175, at: 0.07, decay: 0.16, gain: 0.18 },
  ],
  // A suggestion arrived: a rounded glass tap.
  pop: [
    { freq: 1318, to: 1240, decay: 0.18, gain: 0.26 },
    { freq: 2637, decay: 0.08, gain: 0.05 },
  ],
  // Listening: an open, upward pair.
  open: [
    { freq: 659, decay: 0.14, gain: 0.2 },
    { freq: 988, at: 0.08, decay: 0.2, gain: 0.18 },
  ],
  // Done: a bell with a long, quiet tail.
  ding: [
    { freq: 1568, decay: 0.9, gain: 0.2 },
    { freq: 2349, at: 0.005, decay: 0.55, gain: 0.07 },
    { freq: 3136, at: 0.01, decay: 0.3, gain: 0.03 },
  ],
  // Failed: a low, falling pair. Calm, not alarming.
  boop: [
    { freq: 523, to: 494, decay: 0.16, gain: 0.22, type: "triangle" },
    { freq: 392, at: 0.11, decay: 0.24, gain: 0.2, type: "triangle" },
  ],
  // Going to rest: a slow, quiet sigh downward.
  yawn: [{ freq: 440, to: 294, decay: 0.7, gain: 0.12 }],
};

let ctx: AudioContext | null = null;
let bus: AudioNode | null = null;

/** Lazily builds context, low-pass and a short room echo. */
function output(): { ac: AudioContext; bus: AudioNode } {
  if (!ctx || !bus) {
    ctx = new AudioContext();
    const lowpass = ctx.createBiquadFilter();
    lowpass.type = "lowpass";
    lowpass.frequency.value = 4200;
    lowpass.Q.value = 0.3;

    const room = ctx.createDelay();
    room.delayTime.value = 0.09;
    const roomGain = ctx.createGain();
    roomGain.gain.value = 0.16;

    lowpass.connect(ctx.destination);
    lowpass.connect(room).connect(roomGain).connect(ctx.destination);
    bus = lowpass;
  }
  if (ctx.state === "suspended") void ctx.resume();
  return { ac: ctx, bus };
}

export function cueVolume(settings: Settings, cue: Cue): number {
  if (settings.muted) return 0;
  return settings.masterVolume * (settings.cueVolumes[cue] ?? 1);
}

export function playCue(cue: Cue, volume: number): void {
  if (volume <= 0 || typeof window === "undefined") return;
  const { ac, bus } = output();
  const now = ac.currentTime + 0.005;

  for (const n of CUES[cue]) {
    const start = now + (n.at ?? 0);
    const end = start + n.decay;
    const osc = ac.createOscillator();
    const amp = ac.createGain();

    osc.type = n.type ?? "sine";
    osc.frequency.setValueAtTime(n.freq, start);
    if (n.to) osc.frequency.exponentialRampToValueAtTime(n.to, end);

    // 4 ms attack avoids clicks; exponential release sounds natural.
    amp.gain.setValueAtTime(0.0001, start);
    amp.gain.exponentialRampToValueAtTime(Math.max(n.gain * volume, 0.0002), start + 0.004);
    amp.gain.exponentialRampToValueAtTime(0.0001, end);

    osc.connect(amp).connect(bus);
    osc.start(start);
    osc.stop(end + 0.05);
  }
}
