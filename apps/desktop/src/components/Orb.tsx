"use client";

// Sidekick's face: a glass orb with a slowly turning halo and two eyes that
// follow the cursor anywhere on screen. Near the cursor it is pulled toward
// it like a magnet. Each state (and each short mood on top of it) has its own
// expression; the motion lives in orb/engine.ts and runs on springs, so it can
// be interrupted at any instant.

import { useReducedMotion } from "motion/react";
import { type CSSProperties, useEffect, useRef } from "react";
import { subscribeCursor } from "@/lib/cursor";
import type { MascotState, Theme as ThemeName } from "@/lib/types";
import { type Material, OrbEngine } from "./orb/engine";
import { type Expression, FOR_STATE } from "./orb/expressions";

export const THEME_STYLES: Record<ThemeName, Material & { label: string }> = {
  // Pearl white with pastel light around it.
  pearl: {
    label: "Pearl",
    body: ["#a9adb9", "#ffffff"],
    halo: ["#ffc8dd", "#bde0fe", "#e2d1ff"],
    haloOpacity: 0.5,
    eye: "#1c1c1e",
    eyeGlow: "rgb(0 0 0 / 0)",
  },
  // Pearl with a soft rainbow sheen, like the voice halo.
  aurora: {
    label: "Aurora",
    body: ["#a99fd6", "#ffffff"],
    halo: ["#ffb3d9", "#b9c8ff", "#b3f0e0"],
    haloOpacity: 0.55,
    eye: "#1b1630",
    eyeGlow: "rgb(0 0 0 / 0)",
    sheen:
      "conic-gradient(from 210deg at 58% 62%, rgb(255 185 222 / 0.5), rgb(198 182 255 / 0.5), rgb(168 214 255 / 0.5), rgb(182 245 226 / 0.5), rgb(255 226 186 / 0.5), rgb(255 185 222 / 0.5))",
  },
  // Liquid metal.
  chrome: {
    label: "Chrome",
    body: ["#6f7584", "#f4f6fa"],
    halo: ["#c9d2e3", "#ffffff", "#9fb4d8"],
    haloOpacity: 0.4,
    eye: "#0f1116",
    eyeGlow: "rgb(0 0 0 / 0)",
    sheen:
      "linear-gradient(168deg, rgb(255 255 255 / 0.45) 0%, rgb(255 255 255 / 0) 30%, rgb(36 40 52 / 0.32) 48%, rgb(255 255 255 / 0.22) 62%, rgb(255 255 255 / 0) 80%)",
  },
  peach: {
    label: "Peach",
    body: ["#e8767c", "#fff3ea"],
    halo: ["#ffb199", "#ffd6a5", "#ff9fb2"],
    haloOpacity: 0.5,
    eye: "#3a1716",
    eyeGlow: "rgb(0 0 0 / 0)",
  },
  mint: {
    label: "Mint",
    body: ["#4eb29b", "#f2fffb"],
    halo: ["#9ff0d4", "#b9f6ff", "#d4ffb8"],
    haloOpacity: 0.48,
    eye: "#0e2a23",
    eyeGlow: "rgb(0 0 0 / 0)",
  },
  lilac: {
    label: "Lilac",
    body: ["#8a71df", "#fbf8ff"],
    halo: ["#cdb8ff", "#ffc8f0", "#b8d0ff"],
    haloOpacity: 0.5,
    eye: "#1d1440",
    eyeGlow: "rgb(0 0 0 / 0)",
  },
  // Glossy black with white eyes and a cool rim.
  onyx: {
    label: "Onyx",
    body: ["#060608", "#3c3c48"],
    halo: ["#8fa3c8", "#d9c8ee", "#9fd4e4"],
    haloOpacity: 0.34,
    eye: "#ffffff",
    eyeGlow: "rgb(255 255 255 / 0.5)",
  },
};

/** Gaze returns to center after the cursor rests this long. */
const GAZE_RELEASE_MS = 4000;

export function Orb({
  state,
  face,
  size,
  theme = "pearl",
  magnetic = true,
  alive = false,
  simple = false,
}: {
  state: MascotState;
  /** A mood that overrides the state's own face for a while. */
  face?: Expression | null;
  size: number;
  theme?: ThemeName;
  /** Pull toward a nearby cursor. Gaze tracking stays on either way. */
  magnetic?: boolean;
  /** Idle on its own: glances, blinks, the odd smile. */
  alive?: boolean;
  /** Battery saver: no motion beyond what a state needs. */
  simple?: boolean;
}) {
  const reduced = (useReducedMotion() ?? false) || simple;
  const expression = face ?? FOR_STATE[state];
  const root = useRef<HTMLDivElement>(null);
  const halo = useRef<HTMLDivElement>(null);
  const haloPos = useRef<HTMLDivElement>(null);
  const float = useRef<HTMLDivElement>(null);
  const life = useRef<HTMLDivElement>(null);
  const body = useRef<HTMLDivElement>(null);
  const faceEl = useRef<HTMLDivElement>(null);
  const eyes = useRef<HTMLDivElement>(null);
  const eyeL = useRef<SVGSVGElement>(null);
  const eyeR = useRef<SVGSVGElement>(null);
  const mouth = useRef<SVGSVGElement>(null);
  const cheekL = useRef<HTMLDivElement>(null);
  const cheekR = useRef<HTMLDivElement>(null);
  const fx = useRef<HTMLDivElement>(null);
  const engine = useRef<OrbEngine | null>(null);
  // Latest props, so a rebuilt engine starts from them.
  const latest = useRef({ theme, expression, alive });
  latest.current = { theme, expression, alive };

  useEffect(() => {
    const e = new OrbEngine(
      {
        root: root.current as HTMLElement,
        halo: halo.current as HTMLElement,
        haloPos: haloPos.current as HTMLElement,
        float: float.current as HTMLElement,
        life: life.current as HTMLElement,
        body: body.current as HTMLElement,
        face: faceEl.current as HTMLElement,
        eyes: eyes.current as HTMLElement,
        eyeL: eyeL.current as SVGSVGElement,
        eyeR: eyeR.current as SVGSVGElement,
        mouth: mouth.current as SVGSVGElement,
        cheeks: [cheekL.current as HTMLElement, cheekR.current as HTMLElement],
        fx: fx.current as HTMLElement,
      },
      size,
      reduced,
    );
    const now = latest.current;
    e.setMaterial(THEME_STYLES[now.theme] ?? THEME_STYLES.pearl);
    e.show(now.expression);
    e.setAlive(now.alive);
    engine.current = e;
    return () => {
      e.destroy();
      engine.current = null;
    };
  }, [size, reduced]);

  useEffect(() => {
    engine.current?.setMaterial(THEME_STYLES[theme] ?? THEME_STYLES.pearl);
  }, [theme]);
  useEffect(() => {
    engine.current?.show(expression);
  }, [expression]);
  useEffect(() => {
    engine.current?.setAlive(alive);
  }, [alive]);

  // Cursor: gaze anywhere on screen, magnetic pull only nearby.
  useEffect(() => {
    let release: ReturnType<typeof setTimeout> | undefined;
    const unsubscribe = subscribeCursor((x, y) => {
      const el = root.current;
      const e = engine.current;
      if (!el || !e) return;
      const r = el.getBoundingClientRect();
      const dx = x - (r.left + r.width / 2);
      const dy = y - (r.top + r.height / 2);
      const dist = Math.hypot(dx, dy) || 1;
      // Ease toward full gaze over ~260 px so near movements stay readable.
      const reach = 1 - Math.exp(-dist / 140);
      e.gaze((dx / dist) * reach, (dy / dist) * reach);
      const radius = size * 1.6 + 48;
      if (magnetic && !reduced && dist < radius) {
        const strength = (1 - dist / radius) ** 1.4;
        const max = size * 0.28;
        const clamp = (v: number) => Math.max(-max, Math.min(max, v * 0.45 * strength));
        e.pull(clamp(dx), clamp(dy));
      } else e.pull(0, 0);
      clearTimeout(release);
      release = setTimeout(() => engine.current?.gaze(0, 0), GAZE_RELEASE_MS);
    });
    return () => {
      clearTimeout(release);
      unsubscribe();
    };
  }, [size, magnetic, reduced]);

  return (
    <div
      ref={root}
      className="orb"
      style={{ "--size": `${size}px`, width: size, height: size } as CSSProperties}
      role="img"
      aria-label={`Sidekick is ${state}`}
    >
      {/* The glow sits behind the body and only slides with it: no tilt or
          squash, and always under the face. */}
      <div ref={haloPos} className="orb-halo-pos">
        <div ref={halo} className="orb-halo" />
      </div>
      <div ref={float} className="orb-float">
        <div ref={life} className="orb-life">
          <div ref={body} className="orb-body">
            <div className="orb-spec" />
            <div ref={faceEl} className="orb-face">
              <div ref={eyes} className="orb-eyes">
                <svg ref={eyeL} className="orb-eye" viewBox="0 0 20 30" aria-hidden="true" />
                <svg ref={eyeR} className="orb-eye" viewBox="0 0 20 30" aria-hidden="true" />
              </div>
              <svg ref={mouth} className="orb-mouth" viewBox="0 0 40 20" aria-hidden="true" />
              <div ref={cheekL} className="orb-cheek orb-cheek-l" />
              <div ref={cheekR} className="orb-cheek orb-cheek-r" />
            </div>
          </div>
          <div ref={fx} className="orb-fx" />
        </div>
      </div>
    </div>
  );
}
