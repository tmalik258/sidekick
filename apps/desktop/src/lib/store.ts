import { create } from "zustand";
import type { Expression } from "@/components/orb/expressions";
import { api, EVENTS, listen } from "./bridge";
import { putCached, SETUP_STATUS_CACHE_KEY } from "./cache";
import { firstToday, isThanks, type Mood, moodForSkill, SUGGESTION_MOOD_MS } from "./mood";
import { type NetNotice, watchNet } from "./net";
import { cueVolume, playCue, playMood, playSound, preloadSounds, setDndQuiet } from "./sound";
import type { SynthSound } from "./synth";
import { timings } from "./timings";
import { toolStatus } from "./tools";
import {
  type ActionResult,
  type AskContext,
  type ChatMessage,
  DEFAULT_SETTINGS,
  type ExtensionGuide,
  type HitRect,
  type MascotState,
  type Proposal,
  type Settings,
  type Suggestion,
  type Turn,
  type UpdateInfo,
  type VoiceStatus,
} from "./types";

interface SidekickState {
  mascot: MascotState;
  settings: Settings;
  suggestion: Suggestion | null;
  /** A newer release, until it is installed. */
  update: UpdateInfo | null;
  /** A short-lived face on top of the mascot's state (see mood.ts). */
  mood: Mood | null;
  /** The internet is reachable. */
  online: boolean;
  /** When the connection dropped, while offline. */
  offlineSince: number | null;
  /** A question to ask again once the internet is back. */
  askWhenOnline: string | null;
  /** A short note that the connection dropped or came back. */
  netNotice: NetNotice | null;
  /** Cursor is over the island's interactive area (reported by Rust). */
  hovered: boolean;
  /** Extra interactive area while a floating menu is open (unioned into the hit rect). */
  overlayHit: HitRect | null;
  /** False while a fullscreen app is in front; the island fades away. */
  visible: boolean;
  /** A fullscreen app is in front on the island's screen. */
  fullscreen: boolean;
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
  /** The model picked in Ask mode; null lets Sidekick choose. */
  askModel: string | null;
  /** A spoken question asked with Ask closed: the island stays compact
   * ("Thinking...") until the answer starts, then opens to show it. */
  voiceQuestion: string | null;
  /** A short "done" line in the compact island ("Hotspot is on"). */
  donePill: string | null;
  /** Where the conversation is saved, so it can be picked up later. */
  conversation: string;
  /** Sidekick's voice is playing; the mascot talks along. */
  speaking: boolean;
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
  /** Shrunk to the pill; hovering brings the guide back unless background. */
  minimized?: boolean;
  /**
   * Wait quietly: compact pill only, hover does not reopen the guide.
   * Still finishes with the usual success when the step is done.
   */
  background?: boolean;
  /** Browser id when waiting on a specific extension (chrome, edge, zen, …). */
  target?: string;
  /** Said when it finishes, instead of "… is installed" or "… is connected". */
  doneLine?: string;
}

export interface WaitOptions {
  resumeTab?: string;
  steps?: string[];
  copies?: { label: string; text: string }[];
  again?: () => void;
  /** Browser id for per-browser extension waiting. */
  target?: string;
  /** When true, skip the initial expanded guide (steps only). */
  minimized?: boolean;
  doneLine?: string;
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
  localOnly: boolean;
  /** Settings tab to show, when something asked for a particular one. */
  settingsTab?: string;
  /** A tool a shortcut asked to start with. */
  tool?: "screen" | "clipboard" | null;
}

const MODEL_KEY = "sidekick.askModel";

function savedModel(): string | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage.getItem(MODEL_KEY);
  } catch {
    return null;
  }
}

/** Picks the model for Ask mode (null: Sidekick chooses), kept for next time. */
export function setAskModel(id: string | null) {
  useSidekick.setState({ askModel: id });
  try {
    if (id) localStorage.setItem(MODEL_KEY, id);
    else localStorage.removeItem(MODEL_KEY);
  } catch {
    // Storage off: the pick lasts until restart.
  }
}

export const useSidekick = create<SidekickState>(() => ({
  mascot: "idle",
  settings: DEFAULT_SETTINGS,
  suggestion: null,
  mood: null,
  update: null,
  online: true,
  askWhenOnline: null,
  offlineSince: null,
  netNotice: null,
  hovered: false,
  overlayHit: null,
  visible: true,
  fullscreen: false,
  ready: false,
  lastResult: null,
  running: null,
  ask: null,
  turns: [],
  chatId: null,
  chatPage: null,
  chatSkill: false,
  askModel: savedModel(),
  voiceQuestion: null,
  donePill: null,
  conversation: crypto.randomUUID(),
  hearing: null,
  speaking: false,
  voiceStatus: null,
  later: 0,
  waiting: null,
  justDone: null,
}));

/** Give up waiting after this long. */
const WAIT_LIMIT_MS = 10 * 60_000;
const WAIT_POLL_MS = 3000;
/** After Set up, keep the step guide open before shrinking to the pill. */
const GUIDE_HOLD_MS = 5000;

/** Settings tab to open once a waited-for step finishes (post-onboarding). */
let resumeSettingsTab: string | null = null;
let guideHoldTimer: ReturnType<typeof setTimeout> | null = null;

/**
 * Shrinks to a "Waiting for …" pill while a step is finished elsewhere
 * (browser sign-in, install), then brings Settings or welcome back when done.
 */
export function startWaiting(
  id: string,
  label: string,
  { resumeTab, steps, copies, again, target, minimized = false, doneLine }: WaitOptions = {},
) {
  if (guideHoldTimer) {
    clearTimeout(guideHoldTimer);
    guideHoldTimer = null;
  }
  const since = Date.now();
  const hasSteps = (steps?.length ?? 0) > 0;
  const startMinimized = hasSteps ? minimized : true;
  useSidekick.setState({
    waiting: {
      id,
      label,
      since,
      resumeTab,
      steps,
      copies,
      again,
      target,
      minimized: startMinimized,
      doneLine,
    },
  });
  if (hasSteps && !startMinimized) {
    guideHoldTimer = setTimeout(() => {
      guideHoldTimer = null;
      const waiting = useSidekick.getState().waiting;
      if (waiting?.id === id && waiting.since === since) minimizeWaiting(true);
    }, GUIDE_HOLD_MS);
  }
  // ask_defer_welcome parks welcome, or closes Settings/Ask when already onboarded.
  void api.askDeferWelcome();
}

export function stopWaiting() {
  if (guideHoldTimer) {
    clearTimeout(guideHoldTimer);
    guideHoldTimer = null;
  }
  useSidekick.setState({ waiting: null });
}

/** The guide's Cancel: stop waiting, and carry on with the welcome if unfinished. */
export function cancelWaiting() {
  stopWaiting();
  const { settings, ask } = useSidekick.getState();
  if (!settings.onboarded && !ask) void api.askResumeWelcome();
}

/**
 * Opens a browser's extensions page and keeps the steps on the island until
 * the extension connects. Shared by the welcome and Settings.
 */
export async function installExtension(id: string, name: string): Promise<ExtensionGuide> {
  const guide = await api.extensionInstall(id);
  const firefox = guide.page.startsWith("about:");
  startWaiting("browser", `the ${name} extension`, {
    resumeTab: "connections",
    target: id,
    steps: guide.steps,
    copies: [
      { label: "Copy extensions address", text: guide.page },
      { label: firefox ? "Copy file path" : "Copy folder path", text: guide.copied },
    ],
    again: () => void installExtension(id, name).catch(() => undefined),
  });
  return guide;
}

/** Shrinks the guide to the pill, or brings it back. */
export function minimizeWaiting(minimized: boolean) {
  const waiting = useSidekick.getState().waiting;
  if (waiting)
    useSidekick.setState({ waiting: { ...waiting, minimized, background: minimized ? waiting.background : false } });
}

/** Wait as a pill only: hover no longer opens the guide; success still finishes normally. */
export function backgroundWaiting() {
  const waiting = useSidekick.getState().waiting;
  if (waiting) useSidekick.setState({ waiting: { ...waiting, minimized: true, background: true } });
}

/** How long a done pill stays before the island settles. */
const DONE_PILL_MS = 2600;
let doneTimer: ReturnType<typeof setTimeout> | undefined;
/** Shows a compact done pill for a moment: nothing needs you. */
export function showDone(text: string) {
  clearTimeout(doneTimer);
  useSidekick.setState({ donePill: text, voiceQuestion: null });
  doneTimer = setTimeout(() => useSidekick.setState({ donePill: null }), DONE_PILL_MS);
}

/** How long the island keeps the Done card after a waited step. */
const JUST_DONE_MS = 3000;

/** Marks a waited-for step done: speak, show island Done, then reopen welcome or Settings. */
export function finishWaiting(waiting: Waiting, line: string) {
  if (useSidekick.getState().waiting?.id !== waiting.id) return;
  stopWaiting();
  const { settings } = useSidekick.getState();
  playCue("ding", cueVolume(settings, "ding"), settings.soundKit);
  void api.voiceSay(line);

  // Island Done / All set (mascot success) — not only the orb celebrate face.
  // Welcome/Settings reopen after this so the card is actually visible.
  const detail = line.replace(/^Done\.\s*/i, "").trim() || `${waiting.label} is connected.`;
  useSidekick.setState({
    justDone: waiting.id,
    mascot: "success",
    lastResult: { ok: true, message: detail, path: null, auto: false, undoId: null },
  });
  setMood("celebrate", JUST_DONE_MS);

  const openPanel = () => {
    if (useSidekick.getState().justDone === waiting.id) {
      useSidekick.setState({ justDone: null });
    }
    if (useSidekick.getState().mascot === "success") {
      useSidekick.setState({ mascot: "idle", lastResult: null });
    }
    if (settings.onboarded) {
      resumeSettingsTab = waiting.resumeTab ?? "home";
      void api.openSettings();
    } else {
      void api.askResumeWelcome();
    }
  };

  // Prime caches while Done shows; do not open the panel until the hold is up.
  const caches = Promise.all([
    api.setupStatus().then((s) => putCached(SETUP_STATUS_CACHE_KEY, s)),
    api.browsersStatus().then((b) => putCached("browsers", b)),
    api.composioStatus().then((s) => {
      if (waiting.id.startsWith("app:")) {
        const slug = waiting.id.slice("app:".length);
        putCached("composio-status", {
          ...s,
          apps: s.apps.map((a) => (a.slug === slug ? { ...a, connected: true } : a)),
        });
      } else {
        putCached("composio-status", s);
      }
    }),
  ]).catch(() => undefined);

  void Promise.all([caches, new Promise<void>((r) => setTimeout(r, JUST_DONE_MS))]).then(openPanel);
}

/** Finish a browser wait the moment pairing is allowed (no 3s poll lag). */
function onBrowsersChanged() {
  void api
    .setupStatus()
    .then((s) => putCached(SETUP_STATUS_CACHE_KEY, s))
    .catch(() => undefined);

  const waiting = useSidekick.getState().waiting;
  if (waiting?.id !== "browser" || !waiting.target) return;
  const target = waiting.target;
  const name = waiting.label.charAt(0).toUpperCase() + waiting.label.slice(1);
  void api
    .browsersStatus()
    .then((browsers) => {
      putCached("browsers", browsers);
      if (useSidekick.getState().waiting?.id !== waiting.id) return;
      if (browsers.some((b) => b.id === target && b.connected)) {
        finishWaiting(waiting, `Done. ${name} is connected.`);
      }
    })
    .catch(() => undefined);
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
    const name = waiting.label.charAt(0).toUpperCase() + waiting.label.slice(1);
    // Extension install: finish only when this browser connects, not any paired one.
    if (waiting.id === "browser" && waiting.target) {
      const target = waiting.target;
      api
        .browsersStatus()
        .then((browsers) => {
          if (useSidekick.getState().waiting?.id !== waiting.id) return;
          if (browsers.some((b) => b.id === target && b.connected)) {
            finishWaiting(waiting, `Done. ${name} is connected.`);
          }
        })
        .catch(() => undefined)
        .finally(() => {
          busy = false;
        });
      return;
    }
    api
      .setupStatus()
      .then((status) => {
        if (useSidekick.getState().waiting?.id !== waiting.id) return;
        const item = status.items.find((i) => i.id === waiting.id);
        if (item?.done) {
          finishWaiting(
            waiting,
            waiting.doneLine ??
              (item.group === "connect" ? `Done. ${name} is connected.` : `All set. ${name} is installed.`),
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

/** Runs an action Ask offered, and records what happened on its button. */
export async function runProposal(id: string) {
  const mark = (ran: NonNullable<Proposal["ran"]>) =>
    useSidekick.setState({
      turns: useSidekick
        .getState()
        .turns.map((t) =>
          t.proposals?.some((p) => p.id === id)
            ? { ...t, proposals: t.proposals.map((p) => (p.id === id ? { ...p, ran } : p)) }
            : t,
        ),
    });
  try {
    mark(await api.aiRunProposal(id));
  } catch (e) {
    mark({ ok: false, message: String(e), undoId: null, path: null });
  }
}

/** Undoes what a tapped action did; marks it so Undo does not show again. */
export async function undoProposal(id: string): Promise<string> {
  const p = useSidekick
    .getState()
    .turns.flatMap((t) => t.proposals ?? [])
    .find((x) => x.id === id);
  if (!p?.ran?.undoId) return "Nothing to undo.";
  const message = await api.actionUndo(p.ran.undoId);
  useSidekick.setState({
    turns: useSidekick.getState().turns.map((t) =>
      t.proposals?.some((x) => x.id === id)
        ? {
            ...t,
            proposals: t.proposals.map((x) => (x.id === id && x.ran ? { ...x, ran: { ...x.ran, undone: true } } : x)),
          }
        : t,
    ),
  });
  return message;
}

export const setAsk = (patch: Partial<AskState>) => {
  const ask = useSidekick.getState().ask;
  if (ask) useSidekick.setState({ ask: { ...ask, ...patch } });
};

/** Sends a message in the Ask conversation; answers stream into the last turn. */
/** Asks the last question again after an answer failed. */
/** Asks the question of the last answer again when the internet is back. */
export function askWhenOnline() {
  const { turns } = useSidekick.getState();
  const question = turns.findLast((t) => t.role === "user");
  if (question) useSidekick.setState({ askWhenOnline: question.content });
}

// Back online with a question waiting: open Ask and ask it.
useSidekick.subscribe((s, prev) => {
  if (!prev.online && s.online && s.askWhenOnline) {
    const q = s.askWhenOnline;
    useSidekick.setState({ askWhenOnline: null });
    void api.askOpen().then(() => setTimeout(() => sendChat(q), 400));
  }
});

export function retryLast(): boolean {
  const { turns, chatId } = useSidekick.getState();
  const last = turns[turns.length - 1];
  const question = turns[turns.length - 2];
  if (chatId || !last?.error || question?.role !== "user") return false;
  useSidekick.setState({ turns: turns.slice(0, -2) });
  return sendChat(question.content);
}

/** Asks the last question again; `think` lets the local model reason first. */
export function askAgain(think = false): boolean {
  const { turns, chatId } = useSidekick.getState();
  const question = turns[turns.length - 2];
  if (chatId || question?.role !== "user") return false;
  useSidekick.setState({ turns: turns.slice(0, -2) });
  return sendChat(question.content, { think });
}

/** Asks the last question again, letting the local model think first. */
export function thinkHarder(): boolean {
  return askAgain(true);
}

/** Sends a question; false when it could not start (empty, or one running). */
let moodTimer: ReturnType<typeof setTimeout> | undefined;
/** Shows a mood on the mascot for `ms`, with its sound when given. */
export function setMood(id: Expression, ms: number, sound?: SynthSound) {
  clearTimeout(moodTimer);
  const until = Date.now() + ms;
  useSidekick.setState({ mood: { id, until } });
  moodTimer = setTimeout(() => {
    if (useSidekick.getState().mood?.until === until) useSidekick.setState({ mood: null });
  }, ms);
  if (sound) playMood(sound, uiVolume(), useSidekick.getState().settings.soundKit);
}

/** Two failures this close together make the mascot sad, not just "oops". */
const SAD_WITHIN_MS = 10 * 60_000;
let failures = 0;
let lastFailure = 0;
/** The option the user picked last, to know what an action result was for. */
let lastPick: { skillId: string; label: string } | null = null;

export function notePick(skillId: string, label: string) {
  lastPick = { skillId, label };
}

/** How an action ended: a second failure in a row is sad; finishing the
 * whole morning routine is worth a small celebration. */
function reactToResult(result: ActionResult, sound: boolean) {
  const pick = lastPick;
  lastPick = null;
  if (!result.ok) {
    const now = Date.now();
    failures = now - lastFailure < SAD_WITHIN_MS ? failures + 1 : 1;
    lastFailure = now;
    if (failures >= 2) {
      failures = 0;
      setMood("sad", 3500, sound ? "sad" : undefined);
    }
    return;
  }
  failures = 0;
  if (pick?.skillId === "system.morning-brief" && pick.label === "Open all") {
    setMood("celebrate", 3000, sound ? "fanfare" : undefined);
  }
}

/** "Thanks" in Ask: a little shy, then warm. */
function thanked() {
  setMood("shy", 1400, "cooSoft");
  setTimeout(() => setMood("love", 2400, "mwah"), 1400);
}

/** The first time Sidekick is seen each day, it says hello. */
function helloOncePerDay() {
  if (firstToday()) setTimeout(() => setMood("hello", 2600, "hello"), 900);
}

type ChatAttach = { clipboard?: boolean; screen?: boolean; speak?: boolean; voice?: boolean; think?: boolean };

/** What `api.aiChat` needs for question `q` in the chat as it is now. */
function chatRequest(q: string, attach?: ChatAttach) {
  const { ask, turns, chatPage, chatSkill, askModel, settings } = useSidekick.getState();
  const history: ChatMessage[] = [
    ...turns.filter((t) => !t.error).map(({ role, content }) => ({ role, content })),
    { role: "user", content: q },
  ];
  const auto = autoContext(q, ask);
  return {
    history,
    attach: {
      window: (ask?.attachWindow ?? false) || auto.window,
      clipboard: attach?.clipboard ?? ((ask?.attachClip ?? false) || auto.clipboard),
      page: chatPage,
      skill: chatSkill,
      screen: attach?.screen ?? false,
      // Every answer follows the speaker button, typed or spoken.
      speak: attach?.speak ?? settings.voice.speakAnswers,
      voice: attach?.voice ?? false,
      think: attach?.think ?? false,
      prefer: askModel,
    },
    localOnly: ask?.localOnly ?? false,
  };
}

/**
 * An answer started at a pause in speech, before the question was final.
 * Rust holds it unseen and unsaid (and lets it only look things up) until
 * the question ends the same way; then it is shown at once.
 */
interface EarlyChat {
  id: string;
  q: string;
  tools: { name: string; label?: string }[];
  proposals: { id: string; label: string; step?: boolean }[];
}
let early: EarlyChat | null = null;

function dropEarly() {
  if (!early) return;
  void api.aiCancel(early.id);
  early = null;
}

function startEarly(q: string, speak: boolean) {
  if (!q || useSidekick.getState().chatId || early?.q === q) return;
  dropEarly();
  const id = crypto.randomUUID();
  const req = chatRequest(q, { speak, voice: true });
  early = { id, q, tools: [], proposals: [] };
  timings.sent(id);
  void api.aiChat(id, req.history, { ...req.attach, hold: true }, req.localOnly);
}

export function sendChat(prompt: string, attach?: ChatAttach): boolean {
  const { turns, chatId } = useSidekick.getState();
  const q = prompt.trim();
  if (!q || chatId) {
    dropEarly();
    return false;
  }
  if (isThanks(q)) thanked();
  const adopted = early?.q === q && (attach?.screen ?? false) === false ? early : null;
  if (!adopted) dropEarly();
  early = null;
  const id = adopted?.id ?? crypto.randomUUID();
  const screen = attach?.screen ?? false;
  const req = chatRequest(q, attach);
  useSidekick.setState({
    chatId: id,
    turns: [
      ...turns,
      { role: "user", content: q, screen },
      {
        role: "assistant",
        content: "",
        streaming: true,
        startedAt: Date.now(),
        offline: !useSidekick.getState().online,
        steps: adopted?.tools.map((t) => t.label || toolStatus(t.name).replace(/\.\.\.$/, "")),
        proposals: adopted?.proposals,
      },
    ],
  });
  if (adopted) {
    void api.aiRelease(id);
    return true;
  }
  timings.sent(id);
  void api.aiChat(id, req.history, req.attach, req.localOnly);
  return true;
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

/** Starts a conversation in which AI drafts a new skill. */
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

/** Ends an in-flight chat so a new voice turn can start (clears chatId). */
function clearInFlightChat() {
  cancelChat();
  if (useSidekick.getState().chatId) useSidekick.setState({ chatId: null });
}

/** Push to talk: listen now, no wake word needed. */
export function startListening() {
  useSidekick.setState({ hearing: "" });
  api.voiceListen().catch((e) => {
    const { turns } = useSidekick.getState();
    useSidekick.setState({ hearing: null, turns: [...turns, { role: "assistant", content: "", error: String(e) }] });
  });
}

/** How long the voice pill may sit unchanged before it is cleared. */
const VOICE_STUCK_MS = { hearing: 30_000, question: 120_000 };
let voiceTimer: ReturnType<typeof setTimeout> | null = null;

/**
 * A safety net under the voice pill: if what is heard, or the question
 * waiting for an answer, stays the same too long, the pill clears and
 * listening stops, so the island never stays stuck.
 */
function watchVoice() {
  if (voiceTimer) clearTimeout(voiceTimer);
  const { hearing, voiceQuestion } = useSidekick.getState();
  const ms = voiceQuestion !== null ? VOICE_STUCK_MS.question : VOICE_STUCK_MS.hearing;
  voiceTimer = setTimeout(() => {
    voiceTimer = null;
    const now = useSidekick.getState();
    if (now.hearing !== null && now.hearing === hearing) stopListening();
    if (now.voiceQuestion !== null && now.voiceQuestion === voiceQuestion) {
      useSidekick.setState({ voiceQuestion: null });
    }
  }, ms);
}

export function stopListening() {
  useSidekick.setState({ hearing: null });
  void api.voiceStop();
}

/** Chirp/Pop while listening or waiting on a voice answer would talk over the user. */
function muteSuggestionCue(state: MascotState, previous: MascotState | null): boolean {
  const { hearing, voiceQuestion } = useSidekick.getState();
  if (hearing !== null || voiceQuestion !== null) return true;
  return state === "listening" || previous === "listening";
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

/** Streamed words waiting for the next frame, by chat. */
const pendingText = new Map<string, string>();
let pendingFrame = 0;
let pendingTimer = 0;

/** Flush on the next frame; a hidden window paints no frames, so on a timer. */
function scheduleText() {
  if (pendingFrame || pendingTimer) return;
  if (document.hidden) pendingTimer = window.setTimeout(flushText, 50);
  else pendingFrame = requestAnimationFrame(flushText);
}

function flushText() {
  if (pendingFrame) cancelAnimationFrame(pendingFrame);
  if (pendingTimer) clearTimeout(pendingTimer);
  pendingFrame = 0;
  pendingTimer = 0;
  for (const [id, text] of pendingText)
    updateLastTurn(id, (t) => ({
      ...t,
      content: t.content + text,
      firstMs: t.firstMs ?? (t.startedAt ? Date.now() - t.startedAt : undefined),
    }));
  pendingText.clear();
}

/** Reopening Ask within this long keeps the last chat. */
const RESUME_MS = 10 * 60_000;
/** This PC only of the chat that was open, for when Ask comes back to it. */
let keptLocalOnly = false;
let askClosedAt = Date.now();

function updateLastTurn(id: string, fn: (t: Turn) => Turn) {
  const { chatId, turns } = useSidekick.getState();
  const last = turns[turns.length - 1];
  if (id !== chatId || last?.role !== "assistant") return;
  useSidekick.setState({ turns: [...turns.slice(0, -1), fn(last)] });
}

export const setHovered = (hovered: boolean) => useSidekick.setState({ hovered });

/** Grow the clickable area around a floating menu so it is not click-through. */
export const setOverlayHit = (overlayHit: HitRect | null) => useSidekick.setState({ overlayHit });

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
        const suggestionCue = t.cue === "chirp" || t.cue === "pop";
        const skipCue = suggestionCue && muteSuggestionCue(t.state, t.previous);
        if (sounds && t.cue && !skipCue) playCue(t.cue, cueVolume(settings, t.cue), settings.soundKit);
        if (sounds && t.previous === "sleeping" && t.state === "idle") helloOncePerDay();
      }),
      listen(EVENTS.settingsChanged, (settings) => {
        useSidekick.setState({ settings });
        if (sounds) preloadSounds(settings.soundKit);
        void api.updateStatus().then(
          (update) => !disposed && useSidekick.setState({ update }),
          () => {},
        );
        if (!settings.onboarded && !useSidekick.getState().ask) void api.askEnsureWelcome();
      }),
      listen(EVENTS.updateAvailable, (update) => useSidekick.setState({ update })),
      listen(EVENTS.suggestionNew, (suggestion) => {
        useSidekick.setState({ suggestion, lastResult: null });
        // The cue already plays for a new suggestion, so the mood is silent.
        const mood = moodForSkill(suggestion.skillId);
        if (mood) setMood(mood, SUGGESTION_MOOD_MS);
      }),
      listen(EVENTS.actionResult, (lastResult) => {
        useSidekick.setState({ lastResult, running: null });
        // Done with nothing to undo or open: a compact pill, not a card.
        if (lastResult.ok && !lastResult.undoId && !lastResult.path && !useSidekick.getState().ask) {
          showDone(lastResult.message);
        }
        reactToResult(lastResult, sounds);
      }),
      listen(EVENTS.suggestionClear, (id) => {
        if (useSidekick.getState().suggestion?.id === id) useSidekick.setState({ suggestion: null });
      }),
      listen(EVENTS.suggestionLater, (later) => {
        // Everything that was waiting is dealt with: a small celebration.
        const before = useSidekick.getState().later;
        useSidekick.setState({ later });
        if (before >= 2 && later === 0) setMood("celebrate", 3000, sounds ? "tada" : undefined);
      }),
      listen(EVENTS.islandHover, setHovered),
      listen(EVENTS.islandVisible, (visible) => useSidekick.setState({ visible })),
      listen(EVENTS.islandFullscreen, (fullscreen) => useSidekick.setState({ fullscreen })),
      listen(EVENTS.browsersChanged, () => onBrowsersChanged()),
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
        timings.opened(open.sentAt);
        const seq = (useSidekick.getState().ask?.seq ?? 0) + 1;
        // Back within 10 minutes: the last chat is still there. Later, a new one.
        const { turns, chatId } = useSidekick.getState();
        if (turns.length > 0 && chatId === null && Date.now() - askClosedAt > RESUME_MS) newChat();
        const clip = open.clipboard && !open.context.clipboardSecret;
        if (open.page) {
          newChat();
          useSidekick.setState({ chatPage: open.page });
        }
        const settingsTab = resumeSettingsTab ?? undefined;
        resumeSettingsTab = null;
        // This PC only belongs to the chat: it stays while the chat goes on.
        const prev = useSidekick.getState();
        const sameChat = prev.turns.length > 0;
        const localOnly = sameChat ? (prev.ask?.localOnly ?? keptLocalOnly) : false;
        useSidekick.setState({
          ask: {
            view: open.view ?? "ask",
            context: open.context,
            prompt: open.ask ? "" : (open.prompt ?? ""),
            seq,
            attachWindow: false,
            attachClip: clip,
            localOnly,
            tool: open.tool ?? null,
            settingsTab,
          },
        });
        if (open.ask && open.prompt) sendChat(open.prompt, { clipboard: clip });
      }),
      listen(EVENTS.askClose, (payload) => {
        askClosedAt = Date.now();
        keptLocalOnly = useSidekick.getState().ask?.localOnly ?? false;
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
      listen(EVENTS.voiceSpeaking, (speaking) => useSidekick.setState({ speaking })),
      listen(EVENTS.voiceHeard, ({ text, final, pause, byVoice }) => {
        if (!final) {
          if (pause) {
            startEarly(text.trim(), useSidekick.getState().settings.voice.speakAnswers);
            return;
          }
          // More words came: the early answer was for a different question.
          if (early && text.trim() !== early.q) dropEarly();
          useSidekick.setState({ hearing: text });
          watchVoice();
          return;
        }
        const q = text.trim();
        if (!q) dropEarly();
        const { settings, turns, ask, chatId } = useSidekick.getState();
        if (q) {
          // A stale chatId makes sendChat no-op and drops the Thinking pill,
          // leaving a bare idle island (tiny hit rect / lockout).
          if (chatId) clearInFlightChat();
          // Asked with Ask closed: Thinking pill first so the hit rect stays live.
          if (!ask) {
            useSidekick.setState({ hearing: null, voiceQuestion: q });
            watchVoice();
          } else {
            useSidekick.setState({ hearing: null });
          }
          const started = sendChat(q, { speak: settings.voice.speakAnswers, voice: true });
          if (!started && !ask) useSidekick.setState({ voiceQuestion: null });
        } else {
          useSidekick.setState({ hearing: null });
          if (byVoice && turns.length === 0 && ask) void api.askClose();
        }
      }),
      listen(EVENTS.aiDelta, ({ id, text }) => {
        // Words arrive faster than the screen paints: one update per frame.
        pendingText.set(id, (pendingText.get(id) ?? "") + text);
        scheduleText();
        timings.firstWord(id);
        // The first words of a spoken question's answer: open to show it.
        if (sounds && useSidekick.getState().voiceQuestion !== null) {
          useSidekick.setState({ voiceQuestion: null });
          if (!useSidekick.getState().ask) void api.askOpen();
        }
      }),
      listen(EVENTS.aiTool, ({ id, name, label }) => {
        if (early?.id === id) {
          early.tools.push({ name, label });
          return;
        }
        flushText();
        // Steps read as what they do ("Searching the web for ..."), in words.
        const step = label || toolStatus(name).replace(/\.\.\.$/, "");
        updateLastTurn(id, (t) => ({ ...t, tool: step, steps: [...(t.steps ?? []), step] }));
      }),
      listen(EVENTS.aiProposal, ({ chatId, id, label, step }) => {
        if (early?.id === chatId) {
          early.proposals.push({ id, label, step });
          return;
        }
        updateLastTurn(chatId, (t) => ({ ...t, proposals: [...(t.proposals ?? []), { id, label, step }] }));
      }),
      listen(EVENTS.islandDone, ({ id, text }) => {
        updateLastTurn(id, (t) => ({ ...t, content: text }));
        showDone(text);
      }),
      listen(EVENTS.aiDone, ({ id, provider, error, handoff, cost }) => {
        flushText();
        timings.done(id);
        if (useSidekick.getState().voiceQuestion !== null) useSidekick.setState({ voiceQuestion: null });
        updateLastTurn(id, (t) => ({
          ...t,
          provider,
          error,
          handoff,
          cost: cost ?? undefined,
          tool: null,
          streaming: false,
          tookMs: t.startedAt ? Date.now() - t.startedAt : undefined,
        }));
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
    if (sounds && settings.onboarded) helloOncePerDay();
    // Listeners are live; re-emit welcome if still locked (repairs missed emit).
    if (!settings.onboarded) void api.askEnsureWelcome();
  })();

  const stopNet = watchNet(
    useSidekick.getState,
    (s) => useSidekick.setState(s),
    sounds ? (sound) => playSound(sound, uiVolume(), useSidekick.getState().settings.soundKit) : null,
    () => setMood("happy", 2500),
  );

  // Quiet in Do Not Disturb: checked now and every minute.
  const checkDnd = () =>
    void api
      .dndGet()
      .then((on) => setDndQuiet(on === true))
      .catch(() => undefined);
  checkDnd();
  const dndTimer = setInterval(checkDnd, 60_000);

  return () => {
    disposed = true;
    stopNet();
    clearInterval(dndTimer);
    for (const off of unlisteners) off();
  };
}

/** Volume for interface sounds (chip presses), following the master volume. */
export function uiVolume(): number {
  const { settings } = useSidekick.getState();
  return settings.muted ? 0 : settings.masterVolume * 0.7;
}

/** The assistant's name, "Sidekick" unless the user renamed it. */
export const useAssistantName = () => useSidekick((s) => s.settings.assistantName || "Sidekick");
