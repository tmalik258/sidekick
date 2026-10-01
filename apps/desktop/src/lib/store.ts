import { create } from "zustand";
import { api, EVENTS, listen } from "./bridge";
import { cueVolume, playCue, preloadSounds } from "./sound";
import { type ActionResult, DEFAULT_SETTINGS, type MascotState, type Settings, type Suggestion } from "./types";

interface SidekickState {
  mascot: MascotState;
  settings: Settings;
  suggestion: Suggestion | null;
  /** Cursor is over the island's interactive area (reported by Rust). */
  hovered: boolean;
  /** False while a fullscreen app is in front; the island fades away. */
  visible: boolean;
  ready: boolean;
  /** Outcome of the last action, shown while the mascot reports it. */
  lastResult: ActionResult | null;
}

export const useSidekick = create<SidekickState>(() => ({
  mascot: "idle",
  settings: DEFAULT_SETTINGS,
  suggestion: null,
  hovered: false,
  visible: true,
  ready: false,
  lastResult: null,
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
      if (disposed) return;
      useSidekick.setState({ settings, mascot, suggestion, ready: true });
      if (sounds) preloadSounds(settings.soundKit);
    },
  );

  unlisteners.push(
    listen(EVENTS.mascotState, (t) => {
      useSidekick.setState({ mascot: t.state });
      const { settings } = useSidekick.getState();
      if (sounds && t.cue) playCue(t.cue, cueVolume(settings, t.cue), settings.soundKit);
    }),
    listen(EVENTS.settingsChanged, (settings) => {
      useSidekick.setState({ settings });
      if (sounds) preloadSounds(settings.soundKit);
    }),
    listen(EVENTS.suggestionNew, (suggestion) => useSidekick.setState({ suggestion, lastResult: null })),
    listen(EVENTS.actionResult, (lastResult) => useSidekick.setState({ lastResult })),
    listen(EVENTS.suggestionClear, (id) => {
      if (useSidekick.getState().suggestion?.id === id) useSidekick.setState({ suggestion: null });
    }),
    listen(EVENTS.islandHover, setHovered),
    listen(EVENTS.islandVisible, (visible) => useSidekick.setState({ visible })),
  );

  return () => {
    disposed = true;
    for (const u of unlisteners) void u.then((fn) => fn());
  };
}

/** Volume for interface sounds (chip presses), following the master volume. */
export function uiVolume(): number {
  const { settings } = useSidekick.getState();
  return settings.muted ? 0 : settings.masterVolume * 0.7;
}
