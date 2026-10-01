import { create } from "zustand";
import { api, EVENTS, listen } from "./bridge";
import { cueVolume, playCue } from "./sound";
import { DEFAULT_SETTINGS, type MascotState, type Settings, type Suggestion } from "./types";

interface SidekickState {
  mascot: MascotState;
  settings: Settings;
  suggestion: Suggestion | null;
  /** Cursor is over the island's interactive area (reported by Rust). */
  hovered: boolean;
  ready: boolean;
}

export const useSidekick = create<SidekickState>(() => ({
  mascot: "idle",
  settings: DEFAULT_SETTINGS,
  suggestion: null,
  hovered: false,
  ready: false,
}));

export const setHovered = (hovered: boolean) => useSidekick.setState({ hovered });

export function updateSettings(patch: Partial<Settings>): Promise<Settings> {
  const next = { ...useSidekick.getState().settings, ...patch };
  useSidekick.setState({ settings: next });
  return api.settingsSet(next);
}

/**
 * Loads initial state and subscribes to core events. Only the island window
 * passes `sounds: true`, so cues never play twice.
 */
export function connect({ sounds }: { sounds: boolean }): () => void {
  let disposed = false;
  const unlisteners: Array<Promise<() => void>> = [];

  void Promise.all([api.settingsGet(), api.mascotGet(), api.suggestionCurrent()]).then(
    ([settings, mascot, suggestion]) => {
      if (!disposed) useSidekick.setState({ settings, mascot, suggestion, ready: true });
    },
  );

  unlisteners.push(
    listen(EVENTS.mascotState, (t) => {
      useSidekick.setState({ mascot: t.state });
      if (sounds && t.cue) playCue(t.cue, cueVolume(useSidekick.getState().settings, t.cue));
    }),
    listen(EVENTS.settingsChanged, (settings) => useSidekick.setState({ settings })),
    listen(EVENTS.suggestionNew, (suggestion) => useSidekick.setState({ suggestion })),
    listen(EVENTS.suggestionClear, (id) => {
      if (useSidekick.getState().suggestion?.id === id) useSidekick.setState({ suggestion: null });
    }),
    listen(EVENTS.islandHover, setHovered),
  );

  return () => {
    disposed = true;
    for (const u of unlisteners) void u.then((fn) => fn());
  };
}
