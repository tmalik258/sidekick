import { create } from "zustand";
import { api, EVENTS, listen } from "./bridge";
import { cueVolume, playCue, preloadSounds } from "./sound";
import {
  type ActionResult,
  type AskContext,
  type ChatMessage,
  DEFAULT_SETTINGS,
  type MascotState,
  type Settings,
  type Suggestion,
  type Turn,
} from "./types";

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
  /** Ask mode: the island is a panel for commands and chat. */
  ask: AskState | null;
  /** The Ask conversation. It outlives Ask mode closing. */
  turns: Turn[];
  /** The chat request in flight, if any. */
  chatId: string | null;
}

export interface AskState {
  context: AskContext;
  /** Text to start the input with. */
  prompt: string;
  /** Bumped on every open, so the panel refocuses. */
  seq: number;
  attachWindow: boolean;
  attachClip: boolean;
  localOnly: boolean;
}

export const useSidekick = create<SidekickState>(() => ({
  mascot: "idle",
  settings: DEFAULT_SETTINGS,
  suggestion: null,
  hovered: false,
  visible: true,
  ready: false,
  lastResult: null,
  ask: null,
  turns: [],
  chatId: null,
}));

export const setAsk = (patch: Partial<AskState>) => {
  const ask = useSidekick.getState().ask;
  if (ask) useSidekick.setState({ ask: { ...ask, ...patch } });
};

/** Sends a message in the Ask conversation; answers stream into the last turn. */
export function sendChat(prompt: string, attach?: { clipboard?: boolean }) {
  const { ask, turns, chatId } = useSidekick.getState();
  const q = prompt.trim();
  if (!q || chatId) return;
  const id = crypto.randomUUID();
  const history: ChatMessage[] = [
    ...turns.filter((t) => !t.error).map(({ role, content }) => ({ role, content })),
    { role: "user", content: q },
  ];
  useSidekick.setState({
    chatId: id,
    turns: [...turns, { role: "user", content: q }, { role: "assistant", content: "", streaming: true }],
  });
  void api.aiChat(
    id,
    history,
    { window: ask?.attachWindow ?? false, clipboard: attach?.clipboard ?? ask?.attachClip ?? false },
    ask?.localOnly ?? false,
  );
}

export function cancelChat() {
  const { chatId } = useSidekick.getState();
  if (chatId) void api.aiCancel(chatId);
}

export function newChat() {
  cancelChat();
  useSidekick.setState({ turns: [], chatId: null });
}

function updateLastTurn(id: string, fn: (t: Turn) => Turn) {
  const { chatId, turns } = useSidekick.getState();
  const last = turns[turns.length - 1];
  if (id !== chatId || last?.role !== "assistant") return;
  useSidekick.setState({ turns: [...turns.slice(0, -1), fn(last)] });
}

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
    listen(EVENTS.askOpen, (open) => {
      const seq = (useSidekick.getState().ask?.seq ?? 0) + 1;
      const clip = open.ask && !open.context.clipboardSecret;
      useSidekick.setState({
        ask: {
          context: open.context,
          prompt: open.ask ? "" : (open.prompt ?? ""),
          seq,
          attachWindow: false,
          attachClip: clip,
          localOnly: false,
        },
      });
      if (open.ask && open.prompt) sendChat(open.prompt, { clipboard: clip });
    }),
    listen(EVENTS.askClose, () => useSidekick.setState({ ask: null })),
    listen(EVENTS.aiDelta, ({ id, text }) => updateLastTurn(id, (t) => ({ ...t, content: t.content + text }))),
    listen(EVENTS.aiDone, ({ id, provider, error }) => {
      updateLastTurn(id, (t) => ({ ...t, provider, error, streaming: false }));
      if (useSidekick.getState().chatId === id) useSidekick.setState({ chatId: null });
    }),
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
