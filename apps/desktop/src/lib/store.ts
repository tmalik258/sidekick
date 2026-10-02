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
  type VoiceStatus,
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
  /** Web page text the conversation is about; sent with every turn. */
  chatPage: string | null;
  /** The conversation is about writing a new skill. */
  chatSkill: boolean;
  /** Voice: what is being heard right now, while listening. */
  hearing: string | null;
  voiceStatus: VoiceStatus | null;
}

export interface AskState {
  /** Ask (commands and chat) or Settings. */
  view: "ask" | "settings" | "welcome";
  context: AskContext;
  /** Text to start the input with. */
  prompt: string;
  /** Bumped on every open, so the panel refocuses. */
  seq: number;
  attachWindow: boolean;
  attachClip: boolean;
  /** Send a screenshot with the next question only. */
  attachScreen: boolean;
  localOnly: boolean;
  /** Settings tab to show, when something asked for a particular one. */
  settingsTab?: string;
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
  chatPage: null,
  chatSkill: false,
  hearing: null,
  voiceStatus: null,
}));

export const setAsk = (patch: Partial<AskState>) => {
  const ask = useSidekick.getState().ask;
  if (ask) useSidekick.setState({ ask: { ...ask, ...patch } });
};

/** Sends a message in the Ask conversation; answers stream into the last turn. */
export function sendChat(prompt: string, attach?: { clipboard?: boolean; screen?: boolean; speak?: boolean }) {
  const { ask, turns, chatId, chatPage, chatSkill } = useSidekick.getState();
  const q = prompt.trim();
  if (!q || chatId) return;
  const id = crypto.randomUUID();
  const history: ChatMessage[] = [
    ...turns.filter((t) => !t.error).map(({ role, content }) => ({ role, content })),
    { role: "user", content: q },
  ];
  const screen = attach?.screen ?? ask?.attachScreen ?? false;
  useSidekick.setState({
    chatId: id,
    turns: [...turns, { role: "user", content: q, screen }, { role: "assistant", content: "", streaming: true }],
  });
  if (ask?.attachScreen) setAsk({ attachScreen: false });
  void api.aiChat(
    id,
    history,
    {
      window: ask?.attachWindow ?? false,
      clipboard: attach?.clipboard ?? ask?.attachClip ?? false,
      page: chatPage,
      skill: chatSkill,
      screen,
      speak: attach?.speak ?? false,
    },
    ask?.localOnly ?? false,
  );
}

/** Starts a conversation in which AI drafts a new skill (FR-SKL-08). */
export function startSkill(description: string) {
  newChat();
  useSidekick.setState({ chatSkill: true });
  sendChat(`Write a skill: ${description}`);
}

export function cancelChat() {
  const { chatId } = useSidekick.getState();
  if (chatId) void api.aiCancel(chatId);
  void api.voiceStop();
}

/** Push to talk: listen now, no wake word needed. */
export function startListening() {
  useSidekick.setState({ hearing: "" });
  api.voiceListen().catch((e) => {
    const { turns } = useSidekick.getState();
    useSidekick.setState({ hearing: null, turns: [...turns, { role: "assistant", content: "", error: String(e) }] });
  });
}

export function stopListening() {
  useSidekick.setState({ hearing: null });
  void api.voiceStop();
}

export function newChat() {
  cancelChat();
  useSidekick.setState({ turns: [], chatId: null, chatPage: null, chatSkill: false });
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
  void api.voiceStatus().then((voiceStatus) => !disposed && useSidekick.setState({ voiceStatus }));

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
      const clip = open.clipboard && !open.context.clipboardSecret;
      // A question about a web page starts a fresh conversation about it.
      if (open.page) {
        newChat();
        useSidekick.setState({ chatPage: open.page });
      }
      useSidekick.setState({
        ask: {
          view: open.view ?? "ask",
          context: open.context,
          prompt: open.ask ? "" : (open.prompt ?? ""),
          seq,
          attachWindow: false,
          attachClip: clip,
          attachScreen: false,
          localOnly: false,
        },
      });
      if (open.ask && open.prompt) sendChat(open.prompt, { clipboard: clip });
    }),
    listen(EVENTS.askClose, () => {
      if (useSidekick.getState().hearing !== null) stopListening();
      useSidekick.setState({ ask: null });
    }),
    listen(EVENTS.voiceState, (voiceStatus) => useSidekick.setState({ voiceStatus })),
    listen(EVENTS.voiceHeard, ({ text, final, byVoice }) => {
      if (!final) {
        useSidekick.setState({ hearing: text });
        return;
      }
      useSidekick.setState({ hearing: null });
      const { settings, turns } = useSidekick.getState();
      if (text.trim()) {
        sendChat(text, { speak: settings.voice.speakAnswers });
      } else if (byVoice && turns.length === 0) {
        void api.askClose();
      }
    }),
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
