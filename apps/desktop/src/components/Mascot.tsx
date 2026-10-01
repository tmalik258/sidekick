"use client";

// Placeholder mascot drawn on Canvas 2D at 60 fps. Shapes are math, not
// images, so the look can change without new assets. Original design; do not
// copy Coucou's Mochi (its art and sounds are proprietary).

import { useEffect, useRef } from "react";
import type { MascotState } from "@/lib/types";

interface Look {
  color: [number, number, number];
  eyeOpen: number; // 0 closed, 1 open
  eyeScale: number;
  mouth: number; // -1 frown, 0 flat, 1 smile
  antenna: number; // 0 droop, 1 perked
  breathe: number; // breathing amplitude
  breatheSpeed: number;
  lookX: number; // pupils, -1..1
  lookY: number;
}

const LOOKS: Record<MascotState, Look> = {
  idle: {
    color: [139, 124, 246],
    eyeOpen: 1,
    eyeScale: 1,
    mouth: 0.6,
    antenna: 0.6,
    breathe: 0.03,
    breatheSpeed: 2,
    lookX: 0,
    lookY: 0,
  },
  sleeping: {
    color: [107, 111, 138],
    eyeOpen: 0,
    eyeScale: 1,
    mouth: 0,
    antenna: 0,
    breathe: 0.045,
    breatheSpeed: 1,
    lookX: 0,
    lookY: 0.4,
  },
  noticing: {
    color: [245, 181, 68],
    eyeOpen: 1,
    eyeScale: 1.25,
    mouth: 0.2,
    antenna: 1,
    breathe: 0.02,
    breatheSpeed: 3,
    lookX: 0.7,
    lookY: -0.4,
  },
  suggesting: {
    color: [56, 189, 248],
    eyeOpen: 1,
    eyeScale: 1.1,
    mouth: 1,
    antenna: 0.9,
    breathe: 0.025,
    breatheSpeed: 2.5,
    lookX: 0.5,
    lookY: 0.5,
  },
  listening: {
    color: [96, 165, 250],
    eyeOpen: 1,
    eyeScale: 1.15,
    mouth: 0.3,
    antenna: 1,
    breathe: 0.02,
    breatheSpeed: 2,
    lookX: 0,
    lookY: -0.2,
  },
  working: {
    color: [167, 139, 250],
    eyeOpen: 0.75,
    eyeScale: 0.95,
    mouth: 0,
    antenna: 0.8,
    breathe: 0.015,
    breatheSpeed: 4,
    lookX: -0.3,
    lookY: 0.3,
  },
  success: {
    color: [74, 222, 128],
    eyeOpen: 1,
    eyeScale: 1,
    mouth: 1,
    antenna: 1,
    breathe: 0.03,
    breatheSpeed: 3,
    lookX: 0,
    lookY: 0,
  },
  error: {
    color: [248, 113, 113],
    eyeOpen: 0.85,
    eyeScale: 0.95,
    mouth: -0.8,
    antenna: 0.2,
    breathe: 0.02,
    breatheSpeed: 2,
    lookX: 0,
    lookY: 0.3,
  },
};

/** Damped spring toward a target, one value at a time. */
class Spring {
  value: number;
  velocity = 0;
  constructor(
    value: number,
    private stiffness = 170,
    private damping = 14,
  ) {
    this.value = value;
  }
  step(target: number, dt: number) {
    const force = this.stiffness * (target - this.value) - this.damping * this.velocity;
    this.velocity += force * dt;
    this.value += this.velocity * dt;
  }
}

const lerp = (a: number, b: number, t: number) => a + (b - a) * t;

export function Mascot({
  state,
  size,
  reducedMotion = false,
}: {
  state: MascotState;
  size: number;
  reducedMotion?: boolean;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const stateRef = useRef(state);
  const bounce = useRef(new Spring(0, 260, 12));

  useEffect(() => {
    if (stateRef.current !== state && !reducedMotion) bounce.current.velocity -= 4; // hop on every change
    stateRef.current = state;
  }, [state, reducedMotion]);

  useEffect(() => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx) return;

    const dpr = window.devicePixelRatio || 1;
    canvas.width = size * dpr;
    canvas.height = size * dpr;
    ctx.scale(dpr, dpr);

    const motion = reducedMotion ? 0.2 : 1;
    const start = LOOKS[stateRef.current];
    const s = {
      r: new Spring(start.color[0]),
      g: new Spring(start.color[1]),
      b: new Spring(start.color[2]),
      eyeOpen: new Spring(start.eyeOpen, 300, 22),
      eyeScale: new Spring(start.eyeScale),
      mouth: new Spring(start.mouth),
      antenna: new Spring(start.antenna, 200, 10),
      lookX: new Spring(start.lookX),
      lookY: new Spring(start.lookY),
    };

    let frame = 0;
    let last = performance.now();
    let stateSince = last;
    let lastState = stateRef.current;

    const draw = (now: number) => {
      const dt = Math.min((now - last) / 1000, 1 / 30);
      last = now;
      const current = stateRef.current;
      if (current !== lastState) {
        lastState = current;
        stateSince = now;
      }
      const look = LOOKS[current];
      const t = now / 1000;
      const inState = (now - stateSince) / 1000;

      s.r.step(look.color[0], dt);
      s.g.step(look.color[1], dt);
      s.b.step(look.color[2], dt);
      s.eyeScale.step(look.eyeScale, dt);
      s.mouth.step(look.mouth, dt);
      s.antenna.step(look.antenna, dt);
      s.lookX.step(look.lookX, dt);
      s.lookY.step(look.lookY, dt);
      bounce.current.step(0, dt);

      // Blink every few seconds unless asleep.
      const blinking = look.eyeOpen > 0 && t % 4.2 < 0.12;
      s.eyeOpen.step(blinking ? 0.05 : look.eyeOpen, dt);

      const color = `rgb(${s.r.value | 0} ${s.g.value | 0} ${s.b.value | 0})`;
      const cx = size / 2;
      const r = size * 0.3;
      const breathe = 1 + Math.sin(t * look.breatheSpeed) * look.breathe * motion;
      const cy = size * 0.58 + bounce.current.value * size * 0.05 * motion;

      ctx.clearRect(0, 0, size, size);

      // Listening: pulse ring.
      if (current === "listening") {
        const p = (t * 1.2) % 1;
        ctx.strokeStyle = `rgb(96 165 250 / ${0.6 * (1 - p)})`;
        ctx.lineWidth = size * 0.03;
        ctx.beginPath();
        ctx.arc(cx, cy, r * (1.05 + p * 0.5 * motion), 0, Math.PI * 2);
        ctx.stroke();
      }

      // Antenna.
      const tipX = cx + Math.sin(t * 2) * size * 0.02 * motion + (1 - s.antenna.value) * size * 0.12;
      const tipY = cy - r * breathe - size * (0.04 + 0.12 * s.antenna.value);
      ctx.strokeStyle = color;
      ctx.lineWidth = size * 0.035;
      ctx.lineCap = "round";
      ctx.beginPath();
      ctx.moveTo(cx, cy - r * breathe * 0.9);
      ctx.quadraticCurveTo(cx, tipY + size * 0.04, tipX, tipY);
      ctx.stroke();
      ctx.fillStyle = color;
      ctx.beginPath();
      ctx.arc(tipX, tipY, size * 0.045, 0, Math.PI * 2);
      ctx.fill();

      // Body.
      ctx.beginPath();
      ctx.ellipse(cx, cy, r * (2 - breathe), r * breathe, 0, 0, Math.PI * 2);
      ctx.fill();

      // Eyes.
      const eyeDx = r * 0.42;
      const eyeY = cy - r * 0.12;
      const eyeR = r * 0.2 * s.eyeScale.value;
      for (const side of [-1, 1]) {
        const ex = cx + side * eyeDx;
        if (current === "success") {
          // Happy arcs.
          ctx.strokeStyle = "white";
          ctx.lineWidth = size * 0.035;
          ctx.beginPath();
          ctx.arc(ex, eyeY + eyeR * 0.4, eyeR * 0.8, Math.PI * 1.15, Math.PI * 1.85);
          ctx.stroke();
          continue;
        }
        const open = Math.max(s.eyeOpen.value, 0.06);
        ctx.fillStyle = "white";
        ctx.beginPath();
        ctx.ellipse(ex, eyeY, eyeR, eyeR * open, 0, 0, Math.PI * 2);
        ctx.fill();
        if (open > 0.3) {
          ctx.fillStyle = "#1b1830";
          ctx.beginPath();
          ctx.arc(
            ex + s.lookX.value * eyeR * 0.4,
            eyeY + s.lookY.value * eyeR * 0.4 * open,
            eyeR * 0.5,
            0,
            Math.PI * 2,
          );
          ctx.fill();
        }
      }

      // Mouth: smile, flat, or frown from one curve.
      const mouthY = cy + r * 0.32;
      ctx.strokeStyle = "#1b1830";
      ctx.lineWidth = size * 0.03;
      ctx.beginPath();
      ctx.moveTo(cx - r * 0.22, mouthY);
      ctx.quadraticCurveTo(cx, mouthY + s.mouth.value * r * 0.22, cx + r * 0.22, mouthY);
      ctx.stroke();

      // Working: orbiting dots.
      if (current === "working") {
        for (let i = 0; i < 3; i++) {
          const a = t * 3 * motion + (i * Math.PI * 2) / 3;
          ctx.fillStyle = `rgb(167 139 250 / ${0.5 + i * 0.2})`;
          ctx.beginPath();
          ctx.arc(cx + Math.cos(a) * r * 1.35, cy + Math.sin(a) * r * 0.9, size * 0.03, 0, Math.PI * 2);
          ctx.fill();
        }
      }

      // Sleeping: drifting z.
      if (current === "sleeping") {
        ctx.fillStyle = "rgb(200 200 220 / 0.85)";
        ctx.font = `600 ${size * 0.16}px system-ui, sans-serif`;
        const p = (t * 0.35) % 1;
        ctx.globalAlpha = 1 - p;
        ctx.fillText("z", cx + r * 0.9 + p * size * 0.1 * motion, cy - r - p * size * 0.25 * motion);
        ctx.globalAlpha = 1;
      }

      // Success: sparkles for the first second.
      if (current === "success" && inState < 1.2) {
        const p = Math.min(inState / 1.2, 1);
        ctx.fillStyle = `rgb(250 250 210 / ${1 - p})`;
        for (let i = 0; i < 6; i++) {
          const a = (i * Math.PI * 2) / 6;
          const d = r * lerp(1.1, 1.7, p);
          ctx.beginPath();
          ctx.arc(cx + Math.cos(a) * d, cy + Math.sin(a) * d * 0.8, size * 0.025, 0, Math.PI * 2);
          ctx.fill();
        }
      }

      frame = requestAnimationFrame(draw);
    };

    frame = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(frame);
  }, [size, reducedMotion]);

  return (
    <canvas ref={canvasRef} style={{ width: size, height: size }} aria-label={`Sidekick is ${state}`} role="img" />
  );
}
