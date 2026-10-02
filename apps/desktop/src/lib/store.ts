import { create } from "zustand";
import { api, EVENTS, listen } from "./bridge";
import { cueVolume, playCue, preloadSounds } from "./sound";
import {
  type ActionResult,
  type AskContext,
  type ChatMessage,
  DEFAULT_SETTINGS,
  type ExtensionGuide,
  type MascotState,
  type Settings,
  type Suggestion,
  type Turn,
  type VoiceStatus,
} from "./types";
import { welcomeHeard } from "./welcomeVoice";

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
  /** The option being carried out, shown while the island works on it. */
  running: string | null;
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
  /** Where the conversation is saved, so it can be picked up later. */
  conversation: string;
  /** Voice: what is being heard right now, while listening. */
  hearing: string | null;
  voiceStatus: VoiceStatus | null;
  /** Suggestions held while you were busy or in a meeting. */
  later: number;
  /** A setup step finishing outside Sidekick (browser, PowerShell). */
  waiting: Waiting | null;
  /** The step that just finished while waited for, shown highlighted. */
  justDone: string | null;
}

export interface Waiting {
  /** The setup item (e.g. "composio"). */
  id: string;
  /** What the island says, e.g. "Composio". */
  label: string;
  since: number;
  /** Settings tab to reopen when this finishes after onboarding. */
  resumeTab?: string;
  /** What to do meanwhile; stays on the island until done or cancelled. */
  steps?: string[];
  /** Copy buttons for things to paste (an address, a folder path). */
  copies?: { label: string; text: string }[];
  /** Starts the step again (reopens the browser page, reruns the install). */
  again?: () => void;
  /** Shrunk to the pill; hovering brings the guide back. */
  minimized?: boolean;
}

export interface WaitOptions {
  shrink?: boolean;
  resumeTab?: string;
  steps?: string[];
  copies?: { label: string; text: string }[];
  again?: () => void;
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
  /** A tool a shortcut asked to start with. */
  tool?: "screen" | "clipboard" | null;
}

export const useSidekick = create<SidekickState>(() => ({
  mascot: "idle",
  settings: DEFAULT_SETTINGS,
  suggestion: null,
  hovered: false,
  visible: true,
  ready: false,
  lastResult: null,
  running: null,
  ask: null,
  turns: [],
  chatId: null,
  chatPage: null,
  chatSkill: false,
  conversation: crypto.randomUUID(),
  hearing: null,
  voiceStatus: null,
  later: 0,
  waiting: null,
  justDone: null,
}));

/** Give up waiting after this long. */
const WAIT_LIMIT_MS = 10 * 60_000;
const WAIT_POLL_MS = 3000;

/** Settings tab to open once a waited-for step finishes (post-onboarding). */
let resumeSettingsTab: string | null = null;

/**
 * Shrinks to a "Waiting for …" pill while a step is finished elsewhere
 * (browser sign-in, install), then brings Settings or welcome back when done.
 */
export function startWaiting(
  id: string,
  label: string,
  { shrink = true, resumeTab, steps, copies, again }: WaitOptions = {},
) {
  useSidekick.setState({ waiting: { id, label, since: Date.now(), resumeTab, steps, copies, again } });
  // ask_defer_welcome parks welcome, or closes Settings/Ask when already onboarded.
  if (shrink) void api.askDeferWelcome();
}

export function stopWaiting() {
  useSidekick.setState({ waiting: null });
}

/**
 * Opens a browser's extensions page and keeps the steps on the island until
 * the extension connects. Shared by the welcome and Settings.
 */
export async function installExtension(id: string, name: string, { shrink = true } = {}): Promise<ExtensionGuide> {
  const guide = await api.extensionInstall(id);
  const firefox = guide.page.startsWith("about:");
  startWaiting("browser", `the ${name} extension`, {
    shrink,
    resumeTab: "connections",
    steps: guide.steps,
    copies: [
      { label: "Copy extensions address", text: guide.page },
      { label: firefox ? "Copy file path" : "Copy folder path", text: guide.copied },
    ],
    again: () => void installExtension(id, name, { shrink }).catch(() => undefined),
  });
  return guide;
}

/** Shrinks the guide to the pill, or brings it back. */
export function minimizeWaiting(minimized: boolean) {
  const waiting = useSidekick.getState().waiting;
  if (waiting) useSidekick.setState({ waiting: { ...waiting, minimized } });
}

/** Marks a waited-for step done: speak, highlight, reopen welcome or Settings. */
export function finishWaiting(waiting: Waiting, line: string) {
  if (useSidekick.getState().waiting?.id !== waiting.id) return;
  stopWaiting();
  useSidekick.setState({ justDone: waiting.id });
  setTimeout(() => {
    if (useSidekick.getState().justDone === waiting.id) useSidekick.setState({ justDone: null });
  }, 6000);
  const { settings } = useSidekick.getState();
  playCue("ding", cueVolume(settings, "ding"), settings.soundKit);
  void api.voiceSay(line);
  if (settings.onboarded) {
    resumeSettingsTab = waiting.resumeTab ?? "home";
    void api.openSettings();
  } else {
    void api.askResumeWelcome();
  }
}

/** Checks the step being waited for; done or too long brings the panel back. */
export function watchWaiting(): () => void {
  let busy = false;
  const id = setInterval(() => {
    const waiting = useSidekick.getState().waiting;
    if (!waiting || busy) return;
    if (Date.now() - waiting.since > WAIT_LIMIT_MS) {
      stopWaiting();
      return;
    }
    // Composio apps finish via composio://changed, not setup status.
    if (waiting.id.startsWith("app:")) return;
    busy = true;
    api
      .setupStatus()
      .then((status) => {
        if (useSidekick.getState().waiting?.id !== waiting.id) return;
        const item = status.items.find((i) => i.id === waiting.id);
        if (item?.done) {
          const name = waiting.label.charAt(0).toUpperCase() + waiting.label.slice(1);
          finishWaiting(
            waiting,
            item.group === "connect" ? `Done. ${name} is connected.` : `All set. ${name} is installed.`,
          );
        }
      })
      .catch(() => undefined)
      .finally(() => {
        busy = false;
      });
  }, WAIT_POLL_MS);
  return () => clearInterval(id);
}

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
  const auto = autoContext(q, ask);
  useSidekick.setState({
    chatId: id,
    turns: [...turns, { role: "user", content: q, screen }, { role: "assistant", content: "", streaming: true }],
  });
  if (ask?.attachScreen) setAsk({ attachScreen: false });
  void api.aiChat(
    id,
    history,
    {
      window: (ask?.attachWindow ?? false) || auto.window,
      clipboard: attach?.clipboard ?? ((ask?.attachClip ?? false) || auto.clipboard),
      page: chatPage,
      skill: chatSkill,
      screen,
      speak: attach?.speak ?? false,
    },
    ask?.localOnly ?? false,
  );
}

/**
 * Context a question plainly points at, so you rarely tick the chips: "this
 * error" or "here" means the window in front, "what I copied" the clipboard.
 * Secrets on the clipboard are never added this way.
 */
export function autoContext(q: string, ask: AskState | null): { window: boolean; clipboard: boolean } {
  const text = q.toLowerCase();
  const win = Boolean(ask?.context.app) && /\b(this|here|current(ly)?|in front)\b/.test(text);
  const clipboard =
    Boolean(ask?.context.clipboardKind) &&
    !ask?.context.clipboardSecret &&
    /\b(copied|clipboard|pasted|i just copied|this (error|trace|stack ?trace|snippet|link|url))\b/.test(text);
  return { window: win, clipboard };
}

/** Saves the conversation so Ask mode can list it later. */
function saveChat() {
  const { turns, conversation } = useSidekick.getState();
  const first = turns.find((t) => t.role === "user");
  if (!first) return;
  const title = first.content.length > 60 ? `${first.content.slice(0, 57)}...` : first.content;
  const clean = turns.filter((t) => !t.streaming).map(({ streaming: _, tool: __, ...t }) => t);
  void api.chatSave(conversation, title, clean).catch(() => undefined);
}

/** Picks up a saved conversation. */
export async function openChat(id: string) {
  cancelChat();
  const turns = await api.chatGet(id);
  useSidekick.setState({ turns, chatId: null, chatPage: null, chatSkill: false, conversation: id });
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
  useSidekick.setState({
    turns: [],
    chatId: null,
    chatPage: null,
    chatSkill: false,
    conversation: crypto.randomUUID(),
  });
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

function voiceReady(voice: VoiceStatus | null): boolean {
  return voice?.models.some((m) => m.id === "voice" && m.installed) ?? false;
}

/**
 * Loads initial state and subscribes to core events. Only the island window
 * passes `sounds: true`, so cues never play twice.
 *
 * Listeners are registered before askEnsureWelcome so a cold-start emit is not
 * dropped (welcome lock).
 */
export function connect({ sounds }: { sounds: boolean }): () => void {
  let disposed = false;
  const unlisteners: Array<() => void> = [];

  void (async () => {
    const offs = await Promise.all([
      listen(EVENTS.mascotState, (t) => {
        useSidekick.setState({ mascot: t.state });
        const { settings } = useSidekick.getState();
        if (sounds && t.cue) playCue(t.cue, cueVolume(settings, t.cue), settings.soundKit);
      }),
      listen(EVENTS.settingsChanged, (settings) => {
        useSidekick.setState({ settings });
        if (sounds) preloadSounds(settings.soundKit);
        if (!settings.onboarded && !useSidekick.getState().ask) void api.askEnsureWelcome();
      }),
      listen(EVENTS.suggestionNew, (suggestion) => useSidekick.setState({ suggestion, lastResult: null })),
      listen(EVENTS.actionResult, (lastResult) => useSidekick.setState({ lastResult, running: null })),
      listen(EVENTS.suggestionClear, (id) => {
        if (useSidekick.getState().suggestion?.id === id) useSidekick.setState({ suggestion: null });
      }),
      listen(EVENTS.suggestionLater, (later) => useSidekick.setState({ later })),
      listen(EVENTS.islandHover, setHovered),
      listen(EVENTS.islandVisible, (visible) => useSidekick.setState({ visible })),
      listen(EVENTS.composioChanged, ({ ok, message }) => {
        const waiting = useSidekick.getState().waiting;
        if (!ok || !waiting) return;
        if (waiting.id === "composio") {
          finishWaiting(waiting, message.trim() || "Done. Composio is connected.");
          return;
        }
        if (waiting.id.startsWith("app:")) {
          // Only finish when this app connected (message is e.g. "Gmail connected").
          if (!message.toLowerCase().includes(waiting.label.toLowerCase())) return;
          finishWaiting(waiting, message.trim() || `Done. ${waiting.label} is connected.`);
        }
      }),
      listen(EVENTS.askOpen, (open) => {
        const seq = (useSidekick.getState().ask?.seq ?? 0) + 1;
        const clip = open.clipboard && !open.context.clipboardSecret;
        if (open.page) {
          newChat();
          useSidekick.setState({ chatPage: open.page });
        }
        const settingsTab = resumeSettingsTab ?? undefined;
        resumeSettingsTab = null;
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
            tool: open.tool ?? null,
            settingsTab,
          },
        });
        if (open.ask && open.prompt) sendChat(open.prompt, { clipboard: clip });
      }),
      listen(EVENTS.askClose, (payload) => {
        if (useSidekick.getState().hearing !== null) stopListening();
        useSidekick.setState({ ask: null });
        // Hide parks welcome; do not fight the park with ensure_welcome.
        if (payload.reason === "defer") return;
        if (!useSidekick.getState().settings.onboarded) {
          void api.askEnsureWelcome();
        }
      }),
      listen(EVENTS.voiceState, (voiceStatus) => {
        useSidekick.setState({ voiceStatus });
        const { settings, ask } = useSidekick.getState();
        if (!settings.onboarded && !ask && voiceReady(voiceStatus)) {
          void api.askEnsureWelcome();
        }
      }),
      listen(EVENTS.voiceHeard, ({ text, final, byVoice }) => {
        if (!final) {
          useSidekick.setState({ hearing: text });
          return;
        }
        useSidekick.setState({ hearing: null });
        const { settings, turns, ask } = useSidekick.getState();
        // During the welcome, speech moves between steps instead of chatting.
        if (ask?.view === "welcome" && welcomeHeard(text)) return;
        if (text.trim()) {
          sendChat(text, { speak: settings.voice.speakAnswers });
        } else if (byVoice && turns.length === 0) {
          void api.askClose();
        }
      }),
      listen(EVENTS.aiDelta, ({ id, text }) => updateLastTurn(id, (t) => ({ ...t, content: t.content + text }))),
      listen(EVENTS.aiTool, ({ id, name }) => updateLastTurn(id, (t) => ({ ...t, tool: name }))),
      listen(EVENTS.aiDone, ({ id, provider, error, handoff }) => {
        updateLastTurn(id, (t) => ({ ...t, provider, error, handoff, tool: null, streaming: false }));
        if (useSidekick.getState().chatId === id) {
          useSidekick.setState({ chatId: null });
          if (sounds && !useSidekick.getState().chatSkill) saveChat();
          // Ask was closed while the answer came in: bring it back with the answer.
          if (sounds && !useSidekick.getState().ask && useSidekick.getState().turns.length > 0) {
            void api.askOpen();
          }
        }
      }),
    ]);
    if (disposed) {
      for (const off of offs) off();
      return;
    }
    unlisteners.push(...offs);

    const [settings, mascot, suggestion, voiceStatus, later] = await Promise.all([
      api.settingsGet(),
      api.mascotGet(),
      api.suggestionCurrent(),
      api.voiceStatus(),
      api.laterList().catch(() => []),
    ]);
    if (disposed) return;
    useSidekick.setState({ settings, mascot, suggestion, voiceStatus, later: later.length, ready: true });
    if (sounds) preloadSounds(settings.soundKit);
    // Listeners are live; re-emit welcome if still locked (repairs missed emit).
    if (!settings.onboarded) void api.askEnsureWelcome();
  })();

  return () => {
    disposed = true;
    for (const off of unlisteners) off();
  };
}

/** Volume for interface sounds (chip presses), following the master volume. */
export function uiVolume(): number {
  const { settings } = useSidekick.getState();
  return settings.muted ? 0 : settings.masterVolume * 0.7;
}
