// Sound cues, synthesized with the Web Audio API so the repo ships no audio
// files. Replace with original recorded sounds before the public release.

import type { Cue, Settings } from "./types";

interface Voice {
  type: OscillatorType;
  from: number;
  to: number;
  duration: number;
  gain: number;
  delay?: number;
  vibratoHz?: number;
}

const VOICES: Record<Cue, Voice[]> = {
  pop: [{ type: "sine", from: 620, to: 980, duration: 0.09, gain: 0.5 }],
  ding: [
    { type: "sine", from: 1318, to: 1318, duration: 0.7, gain: 0.32 },
    { type: "sine", from: 1976, to: 1976, duration: 0.45, gain: 0.12, delay: 0.01 },
  ],
  chirp: [{ type: "triangle", from: 440, to: 560, duration: 0.12, gain: 0.35 }],
  boop: [{ type: "triangle", from: 330, to: 200, duration: 0.24, gain: 0.45 }],
  yawn: [{ type: "sine", from: 320, to: 170, duration: 0.65, gain: 0.22, vibratoHz: 6 }],
  open: [
    { type: "sine", from: 520, to: 780, duration: 0.12, gain: 0.32 },
    { type: "sine", from: 780, to: 1040, duration: 0.1, gain: 0.22, delay: 0.09 },
  ],
};

let ctx: AudioContext | null = null;

function context(): AudioContext {
  ctx ??= new AudioContext();
  if (ctx.state === "suspended") void ctx.resume();
  return ctx;
}

export function cueVolume(settings: Settings, cue: Cue): number {
  if (settings.muted) return 0;
  return settings.masterVolume * (settings.cueVolumes[cue] ?? 1);
}

export function playCue(cue: Cue, volume: number): void {
  if (volume <= 0 || typeof window === "undefined") return;
  const ac = context();
  const now = ac.currentTime;

  for (const v of VOICES[cue]) {
    const start = now + (v.delay ?? 0);
    const end = start + v.duration;
    const osc = ac.createOscillator();
    const amp = ac.createGain();

    osc.type = v.type;
    osc.frequency.setValueAtTime(v.from, start);
    osc.frequency.exponentialRampToValueAtTime(v.to, end);
    amp.gain.setValueAtTime(0.0001, start);
    amp.gain.exponentialRampToValueAtTime(Math.max(v.gain * volume, 0.0002), start + 0.012);
    amp.gain.exponentialRampToValueAtTime(0.0001, end);

    if (v.vibratoHz) {
      const lfo = ac.createOscillator();
      const depth = ac.createGain();
      lfo.frequency.value = v.vibratoHz;
      depth.gain.value = 8;
      lfo.connect(depth).connect(osc.frequency);
      lfo.start(start);
      lfo.stop(end);
    }

    osc.connect(amp).connect(ac.destination);
    osc.start(start);
    osc.stop(end + 0.02);
  }
}
