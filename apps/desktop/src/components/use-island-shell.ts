// Shell geometry for the redesign path: registry size → width/height/radius,
// Take A mascot placement, GROW/SETTLE springs.

import type { IslandStateId } from "@/lib/island-states";
import { stateDef } from "@/lib/island-states";
import {
  GROW,
  REDESIGN_AGENTS,
  REDESIGN_ASK,
  REDESIGN_CARD,
  REDESIGN_COMPACT,
  REDESIGN_GLANCE,
  REDESIGN_IDLE,
  REDESIGN_NOTIFY,
  REDESIGN_OS,
  REDESIGN_SETTINGS,
  SETTLE,
  TAKE_A,
  TAKE_A_ASK,
  TAKE_A_CARD,
} from "@/lib/redesign-layout";
import { motionFor } from "@/lib/spring";
import type { WashEvent } from "@/lib/wash";

export interface ShellGeom {
  width: number;
  height: number;
  radius: number;
  bare: boolean;
  wash: WashEvent;
  announce: string | null;
  mascotX: number;
  mascotY: number;
  /** Always REDESIGN_OS (48); visual size comes from mascotScale (prototype orbAt). */
  mascotSize: number;
  /** CSS scale on the creature (Take A / Ask), not canvas resize. */
  mascotScale: number;
  contentPadLeft: number;
}

export function redesignShell(opts: {
  id: IslandStateId;
  contentHeight: number;
  /** Compact pill width from the live label (voice / working / unread). */
  pillWidth?: number;
  askTab: string | null;
  agentsWide: boolean;
  boardWide: boolean;
  /** Measured Settings height; falls back to prototype 460. */
  settingsHeight?: number;
  reduced: boolean;
  settled: boolean;
}): { geom: ShellGeom; transition: ReturnType<typeof motionFor> } {
  const def = stateDef(opts.id);
  const panel = def.size === "panel";
  const card = def.size === "card";
  const compact = def.size === "compact" || def.size === "pill";
  const bare = opts.id === "idle";

  let width: number = REDESIGN_IDLE.width;
  let height: number = REDESIGN_IDLE.height;
  let radius: number = REDESIGN_IDLE.radius;
  let take: { readonly x: number; readonly y: number; readonly scale: number } = TAKE_A;

  if (opts.id === "settings") {
    width = REDESIGN_SETTINGS.width;
    height = opts.settingsHeight ?? REDESIGN_SETTINGS.height;
    radius = REDESIGN_SETTINGS.radius;
    take = TAKE_A_ASK;
  } else if (opts.id === "agents") {
    width = opts.boardWide ? 680 : opts.agentsWide ? REDESIGN_AGENTS.width : REDESIGN_ASK.width;
    height = Math.max(REDESIGN_COMPACT.height, opts.contentHeight + 18);
    radius = REDESIGN_AGENTS.radius;
    take = TAKE_A_ASK;
  } else if (opts.id === "ask" || opts.id === "welcome" || opts.id === "setup-guide") {
    width = opts.id === "ask" ? REDESIGN_ASK.width : opts.id === "welcome" ? 440 : 452;
    height = Math.max(REDESIGN_COMPACT.height, opts.contentHeight + 18);
    radius = REDESIGN_ASK.radius;
    take = TAKE_A_ASK;
  } else if (card || (panel && !bare)) {
    width =
      opts.id === "suggestion" || opts.id === "waiting" || opts.id === "merge-conflict"
        ? REDESIGN_CARD.width
        : opts.id === "new-message"
          ? REDESIGN_NOTIFY.width
          : opts.id === "glance" || opts.id === "quiet-glance" || opts.id === "activities"
            ? REDESIGN_GLANCE.width
            : REDESIGN_CARD.width;
    height = Math.max(78, opts.contentHeight + 16);
    radius = height > 40 ? 26 : 18;
    take = TAKE_A_CARD;
  } else if (compact) {
    width = Math.max(REDESIGN_IDLE.width, opts.pillWidth ?? 109);
    height = REDESIGN_COMPACT.height;
    radius = REDESIGN_COMPACT.radius;
    take = TAKE_A;
  }

  if (bare) {
    width = REDESIGN_IDLE.width;
    height = REDESIGN_IDLE.height;
    radius = REDESIGN_IDLE.radius;
    take = TAKE_A;
  }

  // Prototype: internal size stays --os (48); Take A / Ask only CSS-scale.
  const mascotSize = REDESIGN_OS;
  const visual = mascotSize * take.scale;
  const geom: ShellGeom = {
    width,
    height,
    radius,
    bare,
    wash: def.wash,
    announce: def.announce,
    mascotX: take.x,
    mascotY: take.y,
    mascotSize,
    mascotScale: take.scale,
    contentPadLeft: panel || card ? take.x + visual + 10 : REDESIGN_COMPACT.padLeft,
  };

  const transition = motionFor(opts.settled ? SETTLE : GROW, opts.reduced);
  return { geom, transition };
}
