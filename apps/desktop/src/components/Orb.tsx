"use client";

// Sidekick's face: a glass orb with a slowly turning halo and two eyes that
// follow the cursor anywhere on screen. Near the cursor the orb is pulled
// toward it like a magnet and tilts in 3D. All motion is spring driven so it
// can be interrupted at any instant (Apple: interruptibility, velocity carry).

import {
  animate,
  type MotionValue,
  motion,
  useMotionValue,
  useReducedMotion,
  useSpring,
  useTransform,
} from "motion/react";
import { type CSSProperties, useEffect, useRef } from "react";
import { subscribeCursor } from "@/lib/cursor";
import type { MascotState } from "@/lib/types";

interface Look {
  body: [string, string];
  halo: [string, string, string];
  halo_opacity: number;
  /** Halo rotation speed, relative to the idle speed. */
  spin: number;
  /** Eye openness, 0 closed to about 1.15 wide. */
  open: number;
  happy?: boolean;
  worried?: boolean;
  /** Eyes follow the cursor in this state. */
  gaze: boolean;
}

const INTELLIGENCE: [string, string, string] = ["#ff7a45", "#e14bff", "#4d8bff"];

const LOOKS: Record<MascotState, Look> = {
  idle: {
    body: ["#4b49c9", "#9e9cff"],
    halo: ["#5e5ce6", "#bf5af2", "#64d2ff"],
    halo_opacity: 0.5,
    spin: 1,
    open: 1,
    gaze: true,
  },
  sleeping: {
    body: ["#24242c", "#4a4a58"],
    halo: ["#3a3a48", "#2c2c38", "#3a3a48"],
    halo_opacity: 0,
    spin: 0,
    open: 0.1,
    gaze: false,
  },
  noticing: {
    body: ["#e07b00", "#ffd60a"],
    halo: ["#ff9f0a", "#ff375f", "#ffd60a"],
    halo_opacity: 0.85,
    spin: 2.6,
    open: 1.15,
    gaze: true,
  },
  suggesting: {
    body: ["#0068d6", "#7fdcff"],
    halo: ["#0a84ff", "#5e5ce6", "#64d2ff"],
    halo_opacity: 0.7,
    spin: 1.4,
    open: 1,
    gaze: true,
  },
  listening: { body: ["#5b47ff", "#ff8ad8"], halo: INTELLIGENCE, halo_opacity: 0.95, spin: 4, open: 1.1, gaze: true },
  working: { body: ["#5b47ff", "#ff8ad8"], halo: INTELLIGENCE, halo_opacity: 0.9, spin: 6, open: 0.8, gaze: false },
  success: {
    body: ["#1f9e48", "#a6f4b5"],
    halo: ["#30d158", "#64d2ff", "#a6f4b5"],
    halo_opacity: 0.75,
    spin: 2,
    open: 1,
    happy: true,
    gaze: true,
  },
  error: {
    body: ["#c9241b", "#ff9f8a"],
    halo: ["#ff453a", "#ff9f0a", "#ff375f"],
    halo_opacity: 0.7,
    spin: 1,
    open: 0.75,
    worried: true,
    gaze: true,
  },
};

/** Idle halo turn: one revolution every 14 s. */
const IDLE_SPIN_MS = 14_000;
/** Gaze returns to center after the cursor rests this long. */
const GAZE_RELEASE_MS = 4000;

export function Orb({
  state,
  size,
  magnetic = true,
}: {
  state: MascotState;
  size: number;
  /** Pull toward a nearby cursor. Gaze tracking stays on either way. */
  magnetic?: boolean;
}) {
  const reduced = useReducedMotion() ?? false;
  const look = LOOKS[state];
  const anchorRef = useRef<HTMLDivElement>(null);
  const haloRef = useRef<HTMLDivElement>(null);
  const sheenRef = useRef<HTMLDivElement>(null);
  const spinRef = useRef<Animation[]>([]);
  const lookRef = useRef(look);
  lookRef.current = look;

  // Gaze (-1..1) and magnetic offset (px), each axis its own spring.
  const gazeX = useSpring(0, { stiffness: 210, damping: 24, mass: 0.7 });
  const gazeY = useSpring(0, { stiffness: 210, damping: 24, mass: 0.7 });
  const pullX = useSpring(0, { stiffness: 170, damping: 13, mass: 0.6 });
  const pullY = useSpring(0, { stiffness: 170, damping: 13, mass: 0.6 });
  const hop = useMotionValue(0);

  const open = useSpring(look.open, { stiffness: 380, damping: 30 });
  const blink = useMotionValue(1);
  const eyeScaleY = useTransform(() => open.get() * blink.get());

  const eyeX = useTransform(gazeX, (v) => v * size * 0.13);
  const eyeY = useTransform(gazeY, (v) => v * size * 0.1);
  const rotateY = useTransform(gazeX, (v) => v * 16);
  const rotateX = useTransform(gazeY, (v) => -v * 16);
  const specX = useTransform(gazeX, (v) => -v * size * 0.07);
  const specY = useTransform(gazeY, (v) => -v * size * 0.06);
  const bodyY = useTransform(() => pullY.get() + hop.get());

  // Cursor tracking: gaze anywhere on screen, magnetic pull only nearby.
  useEffect(() => {
    let release: ReturnType<typeof setTimeout> | undefined;
    const unsubscribe = subscribeCursor((x, y) => {
      const el = anchorRef.current;
      if (!el) return;
      const r = el.getBoundingClientRect();
      const dx = x - (r.left + r.width / 2);
      const dy = y - (r.top + r.height / 2);
      const dist = Math.hypot(dx, dy) || 1;

      if (lookRef.current.gaze) {
        // Ease toward full gaze over ~260 px so near movements stay readable.
        const reach = 1 - Math.exp(-dist / 140);
        gazeX.set((dx / dist) * reach);
        gazeY.set((dy / dist) * reach);
      }

      const radius = size * 1.6 + 48;
      if (magnetic && !reduced && dist < radius) {
        const strength = (1 - dist / radius) ** 1.4;
        const max = size * 0.28;
        pullX.set(Math.max(-max, Math.min(max, dx * 0.45 * strength)));
        pullY.set(Math.max(-max, Math.min(max, dy * 0.45 * strength)));
      } else {
        pullX.set(0);
        pullY.set(0);
      }

      clearTimeout(release);
      release = setTimeout(() => {
        gazeX.set(0);
        gazeY.set(0);
      }, GAZE_RELEASE_MS);
    });
    return () => {
      clearTimeout(release);
      unsubscribe();
    };
  }, [size, magnetic, reduced, gazeX, gazeY, pullX, pullY]);

  // State changes: eye openness, gaze reset, a small hop on attention.
  useEffect(() => {
    open.set(look.open);
    if (!look.gaze) {
      gazeX.set(0);
      gazeY.set(state === "working" ? -0.35 : 0);
    }
    if (!reduced && (state === "noticing" || state === "success")) {
      // A quick hop up, then a bouncy settle, like something got its attention.
      void animate(hop, -size * 0.12, { type: "spring", stiffness: 700, damping: 26 }).then(() =>
        animate(hop, 0, { type: "spring", bounce: 0.45, duration: 0.5 }),
      );
    }
  }, [state, look, open, gazeX, gazeY, hop, reduced, size]);

  // Blinking: irregular, sometimes a double blink, never while asleep.
  useEffect(() => {
    if (reduced) return;
    let timer: ReturnType<typeof setTimeout>;
    const schedule = () => {
      timer = setTimeout(
        () => {
          if (lookRef.current.open > 0.3) {
            const double = Math.random() < 0.2;
            void animate(blink, double ? [1, 0.06, 1, 0.06, 1] : [1, 0.06, 1], {
              duration: double ? 0.34 : 0.16,
              ease: "easeInOut",
            });
          }
          schedule();
        },
        2600 + Math.random() * 3800,
      );
    };
    schedule();
    return () => clearTimeout(timer);
  }, [blink, reduced]);

  // Halo rotation on the compositor (WAAPI). Speed changes go through the
  // playback rate, so the halo accelerates smoothly instead of jumping.
  useEffect(() => {
    const targets = [haloRef.current, sheenRef.current].filter(Boolean) as HTMLElement[];
    spinRef.current = targets.map((el, i) =>
      el.animate([{ transform: "rotate(0turn)" }, { transform: `rotate(${i === 0 ? 1 : -1}turn)` }], {
        duration: IDLE_SPIN_MS,
        iterations: Number.POSITIVE_INFINITY,
      }),
    );
    return () => {
      for (const a of spinRef.current) a.cancel();
    };
  }, []);

  useEffect(() => {
    for (const a of spinRef.current) {
      if (reduced || look.spin === 0) a.pause();
      else {
        a.updatePlaybackRate(look.spin);
        a.play();
      }
    }
  }, [look.spin, reduced]);

  const vars = {
    "--size": `${size}px`,
    "--o1": look.body[0],
    "--o2": look.body[1],
    "--g1": look.halo[0],
    "--g2": look.halo[1],
    "--g3": look.halo[2],
    "--halo": look.halo_opacity,
    width: size,
    height: size,
  } as CSSProperties;

  return (
    <div ref={anchorRef} className="orb" style={vars} role="img" aria-label={`Sidekick is ${state}`}>
      <motion.div className="orb-float" style={{ x: pullX, y: bodyY }}>
        <div ref={haloRef} className="orb-halo" />
        <motion.div className="orb-body" style={{ rotateX, rotateY }}>
          <div ref={sheenRef} className="orb-sheen" />
          <motion.div className="orb-spec" style={{ x: specX, y: specY }} />
          <motion.div className="orb-face" style={{ x: eyeX, y: eyeY }}>
            <Eyes scaleY={eyeScaleY} happy={!!look.happy} worried={!!look.worried} />
          </motion.div>
        </motion.div>
      </motion.div>
    </div>
  );
}

function Eyes({ scaleY, happy, worried }: { scaleY: MotionValue<number>; happy: boolean; worried: boolean }) {
  const fade = { type: "spring", bounce: 0, duration: 0.3 } as const;
  return (
    <>
      <motion.div className="orb-eyes" animate={{ opacity: happy ? 0 : 1, scale: happy ? 0.8 : 1 }} transition={fade}>
        <motion.span className="orb-eye" style={{ scaleY }} animate={{ rotate: worried ? 16 : 0 }} transition={fade} />
        <motion.span className="orb-eye" style={{ scaleY }} animate={{ rotate: worried ? -16 : 0 }} transition={fade} />
      </motion.div>
      <motion.div
        className="orb-eyes"
        initial={false}
        animate={{ opacity: happy ? 1 : 0, scale: happy ? 1 : 0.8, y: happy ? 0 : 2 }}
        transition={fade}
        aria-hidden
      >
        <span className="orb-eye-happy" />
        <span className="orb-eye-happy" />
      </motion.div>
    </>
  );
}
