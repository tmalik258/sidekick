// Shell geometry: registry size → width/height/radius, Take A mascot placement,
// GROW/SETTLE springs.

import type { IslandStateId } from "@/lib/island-states";
import { stateDef } from "@/lib/island-states";
import { motionFor } from "@/lib/spring";
import type { WashEvent } from "@/lib/wash";
import {
  GROW,
  SETTLE,
  SHELL_AGENTS,
  SHELL_ASK,
  SHELL_CARD,
  SHELL_COMPACT,
  SHELL_GLANCE,
  SHELL_IDLE,
  SHELL_NOTIFY,
  SHELL_OS,
  SHELL_SETTINGS,
  TAKE_A,
  TAKE_A_ASK,
  TAKE_A_CARD,
} from "./island-layout";

export interface ShellGeom {
  width: number;
  height: number;
  radius: number;
  bare: boolean;
  /** Compact Take A perch: shell plate should carve under the mascot. */
  perch: boolean;
  wash: WashEvent;
  announce: string | null;
  mascotX: number;
  mascotY: number;
  /** Always SHELL_OS (48); visual size comes from mascotScale. */
  mascotSize: number;
  /** CSS scale on the creature (Take A / Ask), not canvas resize. */
  mascotScale: number;
  contentPadLeft: number;
}

export function shellGeom(opts: {
  id: IslandStateId;
  contentHeight: number;
  /** Compact pill width from the live label (voice / working / unread). */
  pillWidth?: number;
  askTab: string | null;
  agentsWide: boolean;
  boardWide: boolean;
  /** Measured Settings height; falls back to 460. */
  settingsHeight?: number;
  reduced: boolean;
  settled: boolean;
}): { geom: ShellGeom; transition: ReturnType<typeof motionFor> } {
  const def = stateDef(opts.id);
  const panel = def.size === "panel";
  const card = def.size === "card";
  const compact = def.size === "compact" || def.size === "pill";
  const bare = opts.id === "idle";

  let width: number = SHELL_IDLE.width;
  let height: number = SHELL_IDLE.height;
  let radius: number = SHELL_IDLE.radius;
  let take: { readonly x: number; readonly y: number; readonly scale: number } = TAKE_A;

  if (opts.id === "settings") {
    width = SHELL_SETTINGS.width;
    height = opts.settingsHeight ?? SHELL_SETTINGS.height;
    radius = SHELL_SETTINGS.radius;
    take = TAKE_A_ASK;
  } else if (opts.id === "agents") {
    width = opts.boardWide ? 680 : opts.agentsWide ? SHELL_AGENTS.width : SHELL_ASK.width;
    height = Math.max(SHELL_COMPACT.height, opts.contentHeight + 18);
    radius = SHELL_AGENTS.radius;
    take = TAKE_A_ASK;
  } else if (opts.id === "ask") {
    width = SHELL_ASK.width;
    height = Math.max(SHELL_COMPACT.height, opts.contentHeight + 18);
    radius = SHELL_ASK.radius;
    take = TAKE_A_ASK;
  } else if (card || (panel && !bare)) {
    width =
      opts.id === "suggestion" || opts.id === "waiting" || opts.id === "merge-conflict"
        ? SHELL_CARD.width
        : opts.id === "new-message"
          ? SHELL_NOTIFY.width
          : opts.id === "glance" || opts.id === "quiet-glance" || opts.id === "activities"
            ? SHELL_GLANCE.width
            : SHELL_CARD.width;
    height = Math.max(78, opts.contentHeight + 16);
    radius = height > 40 ? 26 : 18;
    take = TAKE_A_CARD;
  } else if (compact) {
    width = Math.max(SHELL_IDLE.width, opts.pillWidth ?? 109);
    height = SHELL_COMPACT.height;
    radius = SHELL_COMPACT.radius;
    take = TAKE_A;
  }

  if (bare) {
    width = SHELL_IDLE.width;
    height = SHELL_IDLE.height;
    radius = SHELL_IDLE.radius;
    take = TAKE_A;
  }

  // Internal size stays --os (48); Take A / Ask only CSS-scale.
  const mascotSize = SHELL_OS;
  const visual = mascotSize * take.scale;
  const perch = !bare && take === TAKE_A;
  const geom: ShellGeom = {
    width,
    height,
    radius,
    bare,
    perch,
    wash: def.wash,
    announce: def.announce,
    mascotX: take.x,
    mascotY: take.y,
    mascotSize,
    mascotScale: take.scale,
    contentPadLeft: panel || card ? take.x + visual + 10 : SHELL_COMPACT.padLeft,
  };

  const transition = motionFor(opts.settled ? SETTLE : GROW, opts.reduced);
  return { geom, transition };
}
