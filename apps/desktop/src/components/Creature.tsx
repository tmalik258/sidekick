"use client";

// Duskie's face: a small fur creature with the app's eyes, drawn by
// creature/creature.ts. Without WebGL 2 it shows a still silhouette.

import { useReducedMotion } from "motion/react";
import { useEffect, useRef, useState } from "react";
import { subscribeCursor } from "@/lib/cursor";
import type { MascotState, Theme } from "@/lib/types";
import { createCreature, creatureSupported } from "./creature/creature";
import { type Expression, FOR_STATE } from "./creature/expressions";

interface Live {
  show: (expr: string) => void;
  setTheme: (t: string) => void;
  setAlive: (on: boolean) => void;
  setActive: (on: boolean) => void;
  gaze: (x: number, y: number) => void;
  pull: (x: number, y: number) => void;
  destroy: () => void;
  expr: string | null;
}

/** Gaze returns to center after the cursor rests this long. */
const GAZE_RELEASE_MS = 4000;

export function Creature(props: {
  state: MascotState;
  /** A mood that overrides the state's own face for a while. */
  face?: Expression | null;
  size: number;
  theme?: Theme;
  /** Pull toward a nearby cursor. Gaze tracking stays on either way. */
  magnetic?: boolean;
  /** Idle on its own: glances, blinks, the odd smile. */
  alive?: boolean;
  /** Battery saver: no motion beyond what a state needs. */
  simple?: boolean;
  /** False while nothing is on screen: the creature stops drawing. */
  active?: boolean;
  /**
   * Full-body vs bust crop. Prototype island always uses full body and CSS
   * scale; pass false on the redesign path so Ask (small scale) keeps eyes.
   */
  bust?: boolean;
}) {
  const {
    state,
    face,
    size,
    theme = "atelier",
    magnetic = true,
    alive = false,
    simple = false,
    active = true,
    bust,
  } = props;
  // Decided after mount, so the server's markup and the first client
  // render match; null means not decided yet.
  const [supported, setSupported] = useState<boolean | null>(null);
  useEffect(() => setSupported(creatureSupported()), []);
  const reduced = (useReducedMotion() ?? false) || simple;
  const expression = face ?? FOR_STATE[state];
  const root = useRef<HTMLDivElement>(null);
  const live = useRef<Live | null>(null);
  const latest = useRef({ expression, theme, alive, active });
  latest.current = { expression, theme, alive, active };

  useEffect(() => {
    if (supported !== true || !root.current) return;
    const now = latest.current;
    const c = createCreature(root.current, {
      size,
      expr: now.expression,
      theme: now.theme,
      alive: now.alive,
      active: now.active,
      blink: !reduced,
      ...(bust !== undefined ? { bust } : {}),
    }) as Live | null;
    live.current = c;
    return () => {
      c?.destroy();
      live.current = null;
    };
  }, [supported, size, reduced, bust]);

  useEffect(() => {
    const c = live.current;
    if (c && c.expr !== expression) c.show(expression);
  }, [expression]);
  useEffect(() => {
    live.current?.setTheme(theme);
  }, [theme]);
  useEffect(() => {
    live.current?.setAlive(alive);
  }, [alive]);
  useEffect(() => {
    live.current?.setActive(active);
  }, [active]);

  // Cursor: gaze anywhere on screen, magnetic pull only nearby.
  useEffect(() => {
    if (supported !== true) return;
    let release: ReturnType<typeof setTimeout> | undefined;
    const unsubscribe = subscribeCursor((x, y) => {
      const el = root.current;
      const c = live.current;
      if (!el || !c) return;
      const r = el.getBoundingClientRect();
      const dx = x - (r.left + r.width / 2);
      const dy = y - (r.top + r.height * 0.42);
      const dist = Math.hypot(dx, dy) || 1;
      const reach = 1 - Math.exp(-dist / 160);
      c.gaze((dx / dist) * reach, (dy / dist) * reach);
      const radius = size * 1.6 + 48;
      if (magnetic && !reduced && dist < radius) {
        const k = (1 - dist / radius) ** 1.4;
        const max = size * 0.12;
        const clamp = (v: number) => Math.max(-max, Math.min(max, v * 0.3 * k));
        c.pull(clamp(dx), clamp(dy));
      } else c.pull(0, 0);
      clearTimeout(release);
      release = setTimeout(() => live.current?.gaze(0, 0), GAZE_RELEASE_MS);
    });
    return () => {
      clearTimeout(release);
      unsubscribe();
    };
  }, [supported, size, magnetic, reduced]);

  // No WebGL 2 on this machine: a still silhouette, no animation.
  if (supported === false) {
    return (
      <div
        className="creature-still"
        style={{ width: size, height: size }}
        role="img"
        aria-label={`Duskie is ${state}`}
      />
    );
  }
  return (
    <div
      ref={root}
      className="creature"
      style={{ width: size, height: size }}
      role="img"
      aria-label={`Duskie is ${state}`}
    />
  );
}
