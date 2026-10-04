// The "Sidekick" sound kit: short, soft tones synthesized on the fly, so the
// mascot has its own voice and nothing needs downloading. Each sound is a
// couple of sine or triangle notes with a quick fade, kept quiet.

export type SynthSound =
  | "chirp"
  | "pop"
  | "coo"
  | "cooSoft"
  | "sparkle"
  | "mwah"
  | "hello"
  | "hmm"
  | "hmmUp"
  | "gasp"
  | "tada"
  | "fanfare"
  | "sad"
  | "uhoh"
  | "yawn"
  | "snore"
  | "focus"
  | "open"
  | "disconnect"
  | "reconnect"
  | "buzz"
  | "click"
  | "tap"
  | "down";

type Voice = (n: Notes) => void;

interface Notes {
  tone(f1: number, f2: number, dur: number, type?: OscillatorType, vol?: number, at?: number): void;
  noise(dur: number, f1: number, vol?: number, at?: number, f2?: number, q?: number): void;
}

const SOUNDS: Record<SynthSound, Voice> = {
  chirp: (n) => {
    n.tone(900, 1500, 0.08, "sine", 0.14);
    n.tone(1300, 1900, 0.07, "sine", 0.12, 0.09);
  },
  // Pop, click and tap are short, so they sit higher and a little louder to
  // be heard on laptop speakers.
  pop: (n) => {
    n.tone(520, 1180, 0.09, "sine", 0.26);
    n.noise(0.03, 2600, 0.05);
  },
  coo: (n) => {
    n.tone(540, 760, 0.16, "sine", 0.14);
    n.tone(760, 700, 0.18, "sine", 0.11, 0.15);
  },
  cooSoft: (n) => n.tone(620, 760, 0.22, "sine", 0.1),
  sparkle: (n) => {
    for (const [i, f] of [1568, 2093, 2637].entries()) n.tone(f, f * 1.005, 0.14, "sine", 0.08, i * 0.06);
  },
  mwah: (n) => {
    n.noise(0.04, 2400, 0.12, 0, 1800, 2);
    n.tone(660, 420, 0.14, "sine", 0.12, 0.03);
  },
  hello: (n) => {
    n.tone(587, 622, 0.11, "sine", 0.14);
    n.tone(784, 830, 0.18, "sine", 0.14, 0.13);
  },
  hmm: (n) => {
    n.tone(240, 252, 0.26, "sine", 0.12);
    n.tone(252, 226, 0.26, "sine", 0.1, 0.26);
  },
  hmmUp: (n) => n.tone(320, 520, 0.26, "sine", 0.12),
  gasp: (n) => {
    n.noise(0.14, 2000, 0.1, 0, 3800, 1.5);
    n.tone(520, 880, 0.12, "sine", 0.1);
  },
  tada: (n) => {
    for (const [i, f] of [523, 659, 784].entries()) n.tone(f, f, i === 2 ? 0.38 : 0.1, "sine", 0.12, i * 0.09);
  },
  fanfare: (n) => {
    for (const [i, f] of [523, 659, 784].entries()) n.tone(f, f, 0.1, "triangle", 0.09, i * 0.09);
    n.tone(1047, 1047, 0.45, "sine", 0.1, 0.27);
  },
  sad: (n) => {
    n.tone(440, 420, 0.2, "sine", 0.1);
    n.tone(392, 370, 0.2, "sine", 0.1, 0.22);
    n.tone(330, 300, 0.4, "sine", 0.1, 0.44);
  },
  uhoh: (n) => {
    n.tone(620, 610, 0.1, "sine", 0.12);
    n.tone(460, 440, 0.2, "sine", 0.12, 0.13);
  },
  yawn: (n) => {
    n.tone(300, 520, 0.35, "sine", 0.1);
    n.tone(520, 240, 0.6, "sine", 0.1, 0.33);
  },
  snore: (n) => n.noise(0.9, 380, 0.08, 0, 240, 3),
  focus: (n) => {
    n.tone(330, 330, 0.5, "sine", 0.07);
    n.tone(495, 495, 0.5, "sine", 0.05, 0.05);
  },
  open: (n) => {
    n.tone(520, 880, 0.09, "sine", 0.12);
    n.tone(880, 1180, 0.09, "sine", 0.1, 0.1);
  },
  disconnect: (n) => {
    n.tone(740, 494, 0.14, "sine", 0.11);
    n.tone(494, 330, 0.22, "sine", 0.11, 0.15);
  },
  reconnect: (n) => {
    n.tone(494, 740, 0.1, "sine", 0.11);
    n.tone(740, 988, 0.18, "sine", 0.11, 0.11);
  },
  buzz: (n) => n.tone(300, 200, 0.2, "triangle", 0.1),
  click: (n) => {
    n.tone(2200, 1500, 0.05, "sine", 0.16);
    n.noise(0.02, 3500, 0.05);
  },
  tap: (n) => n.noise(0.06, 2200, 0.16, 0, 1600, 1.2),
  down: (n) => n.tone(880, 560, 0.08, "sine", 0.12),
};

/** Plays a synthesized sound into `out`. */
export function playSynth(ac: AudioContext, out: AudioNode, sound: SynthSound, volume: number) {
  if (volume <= 0) return;
  const voice = SOUNDS[sound];
  if (!voice) return;
  const notes: Notes = {
    tone(f1, f2, dur, type = "sine", vol = 0.14, at = 0) {
      const t = ac.currentTime + at;
      const o = ac.createOscillator();
      const g = ac.createGain();
      o.type = type;
      o.frequency.setValueAtTime(f1, t);
      o.frequency.exponentialRampToValueAtTime(Math.max(20, f2), t + dur);
      g.gain.setValueAtTime(0.0001, t);
      g.gain.exponentialRampToValueAtTime(Math.max(vol * volume, 0.0002), t + Math.min(0.02, dur / 3));
      g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
      o.connect(g).connect(out);
      o.start(t);
      o.stop(t + dur + 0.02);
    },
    noise(dur, f1, vol = 0.12, at = 0, f2 = f1, q = 1) {
      const t = ac.currentTime + at;
      const len = Math.ceil(ac.sampleRate * dur);
      const buf = ac.createBuffer(1, len, ac.sampleRate);
      const d = buf.getChannelData(0);
      for (let i = 0; i < len; i++) d[i] = Math.random() * 2 - 1;
      const s = ac.createBufferSource();
      const f = ac.createBiquadFilter();
      const g = ac.createGain();
      s.buffer = buf;
      f.type = "bandpass";
      f.Q.value = q;
      f.frequency.setValueAtTime(f1, t);
      f.frequency.exponentialRampToValueAtTime(f2, t + dur);
      g.gain.setValueAtTime(0.0001, t);
      g.gain.exponentialRampToValueAtTime(Math.max(vol * volume, 0.0002), t + 0.01);
      g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
      s.connect(f).connect(g).connect(out);
      s.start(t);
      s.stop(t + dur);
    },
  };
  voice(notes);
}
