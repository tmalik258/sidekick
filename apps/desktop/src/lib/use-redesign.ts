// Whether the island uses the redesign look from the prototype. Off by
// default until Live mode (ADR-08). One place so every surface agrees.

import { useDuskie } from "./store";

/** True when Settings > redesign is on. */
export function useRedesign(): boolean {
  return useDuskie((s) => s.settings.redesign.enabled);
}

/** Read the flag outside React (effects, one-shot helpers). */
export function redesignOn(): boolean {
  return useDuskie.getState().settings.redesign.enabled;
}
