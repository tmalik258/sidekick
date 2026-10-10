"use client";

// Ask mode: the island itself becomes the place to run a
// command or ask Sidekick, with the app you were in and the clipboard
// attachable on request. Rendered inside the island shell; the island owns
// the morph, this owns the content.

import { type KeyboardEvent, type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  type AskTab,
  activeCount,
  handOff,
  interruptSession,
  listenToAgents,
  type Session,
  setLayout,
  setTab,
  useAgents,
} from "@/lib/agents";
import { api } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import { useAltHeld } from "@/lib/hooks";
import {
  cancelChat,
  newChat,
  openChat,
  runOpen,
  sendChat,
  setAsk,
  setAskModel,
  showDone,
  startListening,
  startSkill,
  stopListening,
  updateSettings,
  useAssistantName,
  useSidekick,
} from "@/lib/store";
import {
  type CalendarToday,
  type ChatSummary,
  type EditorList,
  type InstantResults,
  isPaused,
  type ProviderStatus,
  type SearchHit,
} from "@/lib/types";
import { AgentsTab } from "./agents/AgentsTab";
import { Repos } from "./agents/Repos";
import { Chat, useAgentName } from "./ask/Chat";
import { Clips, Results } from "./ask/Lists";
import { ModelPicker } from "./ask/ModelPicker";
import { ago, KeyHint, scrollIfActive } from "./ask/parts";
import type { Command } from "./ask/Starters";
import { ContextLine, contextStarters, soonestMeeting } from "./ask/Starters";
import { FirstRun } from "./ask/States";
import { Timings } from "./ask/Timings";
import { Icon } from "./Icon";
import { markSeen } from "./IslandAgents";
import { nextTab, type PanelTab, PanelTabs } from "./PanelTabs";
import { SETTINGS_TABS } from "./SettingsPanel";
import { SetupSpinner } from "./SetupChecklistRow";
import { Tip } from "./Tip";

/** Space the island's orb takes at the top-left in Settings and the welcome. */
export const ASK_ORB = 30;
/** The mascot in Ask: 36 px, on the input line under the tabs. */
export const ASK_MASCOT = { size: 36, x: 10, y: 40 } as const;
/** Ask's padding inside the island: top, sides, bottom. */
export const ASK_PAD = { top: 8, x: 10, bottom: 10 } as const;

/** The "Hold Alt" hint shows in the first five sessions only. */
const EARLY_SESSIONS = (() => {
  try {
    const n = Number(localStorage.getItem("sk-sessions") ?? "0") + 1;
    localStorage.setItem("sk-sessions", String(n));
    return n <= 5;
  } catch {
    return true;
  }
})();

/** Input + chips + footer + gaps; scroll area keeps the rest under the Ask cap. */
const ASK_CHROME = 118;
/** Two board tile rows at full size (196 + gap + 196); island window is 640 tall. */
const ASK_SCROLL_CAP = 400;
/** Island window is ~640 tall (tauri.conf); leave room for chrome + pad. */
const ASK_SCROLL_FLOOR = 120;

function askScrollMax(): number {
  const available = window.innerHeight - ASK_CHROME - 40;
  return Math.min(ASK_SCROLL_CAP, Math.max(ASK_SCROLL_FLOOR, available));
}

/** Windows Settings pages Ask can open by name, with the words people type. */
const WINDOWS_PAGES: { page: string; label: string; words: string[]; switch?: string }[] = [
  { page: "display", label: "Display", words: ["display", "screen", "resolution", "brightness", "scale"] },
  {
    page: "nightlight",
    label: "Night light",
    words: ["night", "nightlight", "night light", "blue light"],
    switch: "night_light",
  },
  { page: "sound", label: "Sound", words: ["sound", "audio", "speaker", "microphone", "volume"] },
  { page: "notifications", label: "Notifications", words: ["notifications"] },
  { page: "focus", label: "Do Not Disturb", words: ["focus", "do not disturb", "dnd"], switch: "dnd" },
  { page: "bluetooth", label: "Bluetooth", words: ["bluetooth", "devices", "headphones"], switch: "bluetooth" },
  { page: "wifi", label: "Wi-Fi", words: ["wifi", "wi-fi", "wireless"], switch: "wifi" },
  {
    page: "hotspot",
    label: "Mobile hotspot",
    words: ["hotspot", "mobile hotspot", "tethering", "share internet"],
    switch: "hotspot",
  },
  { page: "airplane", label: "Airplane mode", words: ["airplane", "aeroplane", "flight mode"], switch: "airplane" },
  { page: "network", label: "Network & internet", words: ["network", "internet", "ethernet", "vpn", "proxy"] },
  { page: "battery", label: "Battery saver", words: ["battery", "saver"] },
  { page: "power", label: "Power & sleep", words: ["power", "sleep"] },
  { page: "storage", label: "Storage", words: ["storage", "disk", "space"] },
  { page: "apps", label: "Installed apps", words: ["apps", "uninstall", "programs"] },
  { page: "default_apps", label: "Default apps", words: ["default"] },
  { page: "startup_apps", label: "Startup apps", words: ["startup"] },
  {
    page: "colors",
    label: "Dark mode",
    words: ["dark mode", "dark", "light mode", "colors", "colours", "theme"],
    switch: "dark_mode",
  },
  { page: "background", label: "Background", words: ["background", "wallpaper"] },
  { page: "mouse", label: "Mouse & touchpad", words: ["mouse", "touchpad", "trackpad"] },
  { page: "keyboard", label: "Keyboard", words: ["keyboard"] },
  { page: "printers", label: "Printers & scanners", words: ["printer", "scanner"] },
  { page: "updates", label: "Windows Update", words: ["update", "updates"] },
  { page: "privacy", label: "Privacy & security", words: ["privacy", "security", "permissions"] },
  { page: "accounts", label: "Your account", words: ["account", "profile"] },
  { page: "time", label: "Date & time", words: ["date", "time", "clock", "timezone"] },
  { page: "language", label: "Language & region", words: ["language", "region"] },
  { page: "about", label: "About this PC", words: ["about", "specs", "system info"] },
];

export function AskPanel() {
  const assistant = useAssistantName();
  const ask = useSidekick((s) => s.ask);
  const turns = useSidekick((s) => s.turns);
  const chatId = useSidekick((s) => s.chatId);
  const hearing = useSidekick((s) => s.hearing);
  const voiceReady = useSidekick((s) => s.settings.voice.enabled && (s.voiceStatus?.listening ?? false));
  const settings = useSidekick((s) => s.settings);
  const [text, setText] = useState(ask?.prompt ?? "");
  const alt = useAltHeld();
  const tab = useAgents((s) => s.tab);
  const layout = useAgents((s) => s.layout);
  const working = useAgents((s) => activeCount(s.sessions));
  // "Do you work with code?" No hides Agents and Repos; unanswered counts as yes.
  const coder = useSidekick((s) => s.settings.codes !== false);
  useEffect(listenToAgents, []);
  const speak = useSidekick((s) => s.settings.voice.speakAnswers);
  const toggleSpeak = useCallback(() => {
    const voice = useSidekick.getState().settings.voice;
    void updateSettings({ voice: { ...voice, speakAnswers: !voice.speakAnswers } });
  }, []);
  // Alt shortcuts work wherever focus is in Ask. What they act on changes
  // every render, so they read it from here.
  const altKeys = useRef<Record<string, (back?: boolean) => void>>({});
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      // Ctrl Tab / Ctrl Shift Tab move between Ask, Agents and History.
      if (e.ctrlKey && e.key === "Tab") {
        e.preventDefault();
        altKeys.current.tab?.(e.shiftKey);
        return;
      }
      if (!e.altKey || e.ctrlKey || e.metaKey || e.repeat) return;
      const run = altKeys.current[e.key.toLowerCase()];
      if (!run) return;
      e.preventDefault();
      run();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  const [selected, setSelected] = useState(0);
  const { data: providersData, refresh: refreshProviders } = useCached<ProviderStatus[]>("ai-status", api.aiStatus);
  const providers = providersData ?? [];
  // While nothing is set up yet, keep checking so FirstRun goes away once a
  // provider is configured (e.g. This PC install finished writing settings).
  const anySetUp = providers.some((p) => p.id !== "semif" && p.configured);
  useEffect(() => {
    if (anySetUp || providersData === null) return;
    const id = setInterval(() => void refreshProviders().catch(() => undefined), 3000);
    return () => clearInterval(id);
  }, [anySetUp, providersData, refreshProviders]);
  const [handoffError, setHandoffError] = useState<string | null>(null);
  /** The chat's name when renamed; its first question otherwise. */
  const [chatName, setChatName] = useState<string | null>(null);
  const [hits, setHits] = useState<{ query: string; items: SearchHit[] } | null>(null);
  const [clips, setClips] = useState<{ text: string; ts: string }[] | null>(null);
  /** File with no default app: Ask shows where to open it. */
  const [openWhere, setOpenWhere] = useState<{ name: string; path: string } | null>(null);
  const [historyKind, setHistoryKind] = useState<HistoryKind>("all");
  const agentSessions = useAgents((s) => s.sessions);
  const { data: editors } = useCached<EditorList>("editors", api.editorsList);
  const editorName = editors?.current ?? editors?.editors[0]?.name ?? null;
  // The highlighted clip, search result, or history row, moved with the arrow keys.
  const [pick, setPick] = useState(0);
  // biome-ignore lint/correctness/useExhaustiveDependencies: a new list starts at its top
  useEffect(() => setPick(0), [clips, hits, tab, historyKind, openWhere]);
  // Esc works wherever focus is in Ask (after clicking a button or chip);
  // the input handles it itself.
  const escRef = useRef<() => void>(() => undefined);
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key !== "Escape" || e.repeat || e.defaultPrevented || e.target === inputRef.current) return;
      e.preventDefault();
      escRef.current();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  const [projects, setProjects] = useState<{ name: string; path: string }[]>([]);
  // Apps and files named like what is typed: no model, answers in a blink.
  const [instant, setInstant] = useState<InstantResults>({ apps: [], files: [] });
  useEffect(() => {
    const q = text.trim();
    if (q.length < 2 || q.startsWith("/") || useSidekick.getState().turns.length > 0) {
      setInstant({ apps: [], files: [] });
      return;
    }
    let live = true;
    const id = setTimeout(() => {
      void api
        .instantFind(q)
        .then((r) => live && setInstant(r))
        .catch(() => undefined);
    }, 60);
    return () => {
      live = false;
      clearTimeout(id);
    };
  }, [text]);
  const [chats, setChats] = useState<ChatSummary[]>([]);
  const inputRef = useRef<HTMLInputElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const nearBottom = useRef(true);
  const seq = ask?.seq;
  const [scrollMax, setScrollMax] = useState(ASK_SCROLL_CAP);

  useEffect(() => {
    const sync = () => setScrollMax(askScrollMax());
    sync();
    window.addEventListener("resize", sync);
    return () => window.removeEventListener("resize", sync);
  }, []);

  // Every open: focus the input and refresh which AI is reachable.
  useEffect(() => {
    if (seq === undefined) return;
    setText(useSidekick.getState().ask?.prompt ?? "");
    markSeen();
    setSelected(0);
    setClips(null);
    setOpenWhere(null);
    setHandoffError(null);
    void refreshProviders().catch(() => undefined);
    void api.projectsList().then(setProjects);
    void api
      .chatsList()
      .then(setChats)
      .catch(() => setChats([]));
    // A shortcut can open Ask mode straight into a tool.
    const tool = useSidekick.getState().ask?.tool;
    if (tool) setAsk({ tool: null });
    if (tool === "clipboard") void api.clipboardHistory().then(setClips);
    if (tool === "screen") {
      sendChat("What's on my screen? Explain it briefly and point out anything I should act on.", { screen: true });
    }
    const id = requestAnimationFrame(() => inputRef.current?.focus({ preventScroll: true }));
    return () => cancelAnimationFrame(id);
  }, [seq, refreshProviders]);

  // A new question always jumps to the bottom, even after scrolling up.
  const userTurns = turns.filter((t) => t.role === "user").length;
  // biome-ignore lint/correctness/useExhaustiveDependencies: only when a question is added
  useEffect(() => {
    nearBottom.current = true;
  }, [userTurns]);

  // The newest message stays in view: on open, while an answer streams, and
  // when buttons or links appear under it, unless the user scrolled up to read.
  const [chatEl, setChatEl] = useState<HTMLDivElement | null>(null);
  const chatRef = useCallback((el: HTMLDivElement | null) => {
    scrollRef.current = el;
    setChatEl(el);
    if (el) {
      nearBottom.current = true;
      el.scrollTop = el.scrollHeight;
    }
  }, []);
  useEffect(() => {
    if (!chatEl) return;
    const follow = () => {
      if (nearBottom.current) chatEl.scrollTop = chatEl.scrollHeight;
    };
    const watch = new ResizeObserver(follow);
    watch.observe(chatEl);
    for (const child of Array.from(chatEl.children)) watch.observe(child);
    follow();
    return () => watch.disconnect();
  }, [chatEl]);

  const paused = isPaused(settings.pause);
  const chatPage = useSidekick((s) => s.chatPage);
  const { data: calendar } = useCached<CalendarToday>("calendar-today", api.calendarToday);
  const agent = useAgentName();
  const askModel = useSidekick((s) => s.askModel);
  const starters = useMemo(
    () =>
      contextStarters({
        context: ask?.context ?? null,
        page: chatPage,
        meeting: soonestMeeting(calendar),
        coder,
        // After the row runs (which clears the input), type the prefix in.
        focusInput: (prefix) =>
          requestAnimationFrame(() => {
            setText(prefix);
            inputRef.current?.focus({ preventScroll: true });
          }),
      }),
    [ask?.context, chatPage, calendar, coder],
  );
  const resetChat = useCallback(() => {
    if (useSidekick.getState().hearing !== null) stopListening();
    newChat();
    setChatName(null);
    setText("");
    setSelected(0);
    setClips(null);
    setHits(null);
    setOpenWhere(null);
    setPick(0);
    nearBottom.current = true;
    requestAnimationFrame(() => inputRef.current?.focus({ preventScroll: true }));
  }, []);

  const commands = useMemo<Command[]>(() => {
    const all: Command[] = [
      paused
        ? { id: "resume", label: "Resume Sidekick", icon: "play", run: () => void api.sensorsResume() }
        : { id: "pause15", label: "Pause for 15 minutes", icon: "pause", run: () => void api.sensorsPause(15) },
      ...(paused
        ? []
        : [{ id: "pause60", label: "Pause for 1 hour", icon: "pause" as const, run: () => void api.sensorsPause(60) }]),
      {
        id: "mute",
        label: settings.muted ? "Turn sounds on" : "Mute sounds",
        icon: settings.muted ? "play" : "pause",
        run: () => void updateSettings({ muted: !settings.muted }),
      },
      {
        id: "screen",
        label: "What's on my screen?",
        hint: "Reads the window you were in",
        icon: "screen",
        run: () =>
          sendChat("What's on my screen? Explain it briefly and point out anything I should act on.", { screen: true }),
        stay: true,
      },
      {
        id: "clipboard",
        label: "Clipboard history",
        hint: "Copy something again",
        icon: "undo",
        run: () => {
          void api.clipboardHistory().then(setClips);
        },
        stay: true,
      },
      { id: "settings", label: "Open settings", icon: "settings", run: () => setAsk({ view: "settings" }), stay: true },
      ...(turns.length
        ? [
            {
              id: "new",
              label: "New chat",
              hint: "Clears this conversation",
              icon: "plus" as const,
              run: resetChat,
              stay: true,
            },
          ]
        : []),
    ];
    return all;
  }, [paused, settings.muted, turns.length, resetChat]);

  if (!ask) return null;

  const streaming = chatId !== null;
  const inHistory = tab === "history";
  // "/" lists every command; what follows filters them.
  const slash = !inHistory && clips === null && text.startsWith("/");
  const q = (slash ? text.slice(1) : text).trim().toLowerCase();
  // While clipboard history is open, typing filters it instead of asking.
  const clipFilter = clips !== null ? q : "";
  const asking = q.length > 0 && !slash && clips === null && !inHistory;
  const shownClips = clips?.filter((c) => !clipFilter || c.text.toLowerCase().includes(clipFilter)) ?? [];
  const history = historyRows(chats, agentSessions, historyKind, inHistory ? q : "");
  // With a conversation going, the body keeps showing it while you type
  // the next question; commands show only before the first one.
  const inChat = turns.length > 0;
  const showClips = clips !== null && !inHistory;
  const showHits = hits !== null && !asking && !showClips && !inHistory && !slash;
  const showChat = inChat && !showHits && !showClips && !inHistory && !slash && !(asking && !inChat);

  const openFile = (path: string, how?: "editor" | "reveal" | "default") => {
    const name = path.split(/[\\/]/).pop() ?? path;
    // No how yet: probe without Closing Ask so the "Open with" picker can show.
    if (!how) {
      void api.fileOpen(path).then(
        (r) => {
          if (!r.opened) {
            setHandoffError(null);
            setOpenWhere({ name, path });
            setSelected(0);
            return;
          }
          setOpenWhere(null);
          setText("");
          void api.askClose();
          showDone(`Opened ${name}`);
        },
        (e: unknown) => setHandoffError(String(e)),
      );
      return;
    }
    setOpenWhere(null);
    setText("");
    void runOpen(async () => {
      const r = await api.fileOpen(path, how);
      return r.opened;
    }, `Opened ${name}`);
  };

  const openHit = (source: string, reference: string, title?: string) => {
    const label =
      title?.trim() ||
      (source === "page" ? reference.replace(/^https?:\/\//, "").split("/")[0] : undefined) ||
      reference.split(/[\\/]/).pop() ||
      reference;
    void runOpen(async () => {
      await api.openReference(source, reference);
      return true;
    }, `Opened ${label}`);
  };

  // The rows under the input: starters, then everything that matches by
  // name (instant, no AI), then Search and Teach at the end.
  const items: Item[] = [];
  if (openWhere) {
    const { name, path } = openWhere;
    const group = `Open ${name}`;
    if (editorName) {
      items.push({
        id: "open-editor",
        group,
        icon: <Icon name="file" size={12} />,
        label: `Open in ${editorName}`,
        hint: "No default app",
        run: () => openFile(path, "editor"),
        stay: true,
        keepText: true,
      });
    }
    items.push(
      {
        id: "open-reveal",
        group: editorName ? undefined : group,
        icon: <Icon name="folder" size={12} />,
        label: "Show in folder",
        hint: editorName ? undefined : "No default app",
        run: () => openFile(path, "reveal"),
        stay: true,
        keepText: true,
      },
      {
        id: "open-default",
        icon: <Icon name="file" size={12} />,
        label: "Open with…",
        hint: "Windows",
        run: () => openFile(path, "default"),
        stay: true,
        keepText: true,
      },
      {
        id: "open-back",
        icon: <span className="text-[11px]">←</span>,
        label: "Back",
        run: () => setOpenWhere(null),
        stay: true,
        keepText: true,
      },
    );
  } else if (!inHistory && !showClips && !showHits && hearing === null) {
    // Math answers itself: Enter copies the result.
    if (asking && !inChat && instant.calc) {
      const result = instant.calc;
      items.push({
        id: "calc",
        group: "Calculator",
        icon: <span className="text-[12px] font-bold">=</span>,
        label: result,
        hint: "Enter copies",
        run: () => void navigator.clipboard.writeText(result).catch(() => undefined),
      });
    }
    if (asking) {
      items.push({
        id: "ask",
        icon: <span className="text-[12px]">✦</span>,
        label: (
          <>
            Ask {assistant}: <span className="text-[rgb(235_235_245/0.6)]">“{text.trim()}”</span>
          </>
        ),
        run: () => {
          sendChat(text);
          setText("");
        },
        stay: true,
        keepText: true,
      });
      if (!inChat) {
        const named = (group: string, list: Command[]) => {
          for (const [n, c] of list.entries()) items.push({ ...commandItem(c), group: n === 0 ? group : undefined });
        };
        // Browsers get a separate private/incognito row (like Zen's Start-menu
        // "Private Browsing"), not a Ctrl Enter shortcut on the main app.
        const privateBrowsers = new Set(
          instant.apps.filter((a) => a.browser && isPrivateAppName(a.name)).map((a) => a.browser as string),
        );
        let appsGrouped = false;
        for (const a of instant.apps) {
          const browser = a.browser ?? null;
          const privateNamed = !!(browser && isPrivateAppName(a.name));
          items.push({
            id: `app:${a.id}`,
            group: !appsGrouped ? "Apps" : undefined,
            icon: <span className="text-[11px] font-bold text-white">{a.name.slice(0, 1).toUpperCase()}</span>,
            label: a.name,
            hint: a.minutes >= 60 ? `${Math.round(a.minutes / 60)} h this week` : undefined,
            run: () =>
              void runOpen(async () => {
                await api.appLaunch(
                  a.id,
                  privateNamed && browser ? { private: true, browser } : undefined,
                );
                return true;
              }, `Opened ${a.name}`),
          });
          appsGrouped = true;
          if (browser && !privateNamed && !privateBrowsers.has(browser)) {
            privateBrowsers.add(browser);
            const label = privateBrowserLabel(browser);
            items.push({
              id: `app-private:${browser}`,
              icon: <span className="text-[11px] font-bold text-white">{label.slice(0, 1).toUpperCase()}</span>,
              label,
              hint: "Private window",
              run: () =>
                void runOpen(async () => {
                  await api.appLaunch(a.id, { private: true, browser });
                  return true;
                }, `Opened ${label}`),
            });
          }
        }
        named(
          "Commands",
          commands.filter((c) => c.label.toLowerCase().includes(q)),
        );
        named(
          "Projects",
          projects
            .filter((p) => p.name.toLowerCase().includes(q.replace(/^open\s+/, "")))
            .slice(0, 3)
            .map((p) => ({
              id: `project:${p.path}`,
              label: p.name,
              hint: "Editor and terminal",
              icon: "folder" as const,
              run: () =>
                void runOpen(async () => {
                  await api.projectLaunch(p.path);
                  return true;
                }, `Opened ${p.name}`),
            })),
        );
        for (const [n, f] of instant.files.entries()) {
          items.push({
            id: `file:${f.path}`,
            group: n === 0 ? "Files" : undefined,
            icon: <Icon name={f.folder ? "folder" : "file"} size={12} />,
            label: f.name,
            hint: f.place,
            // Enter opens the file; Ctrl Enter shows its folder.
            run: () => openFile(f.path),
            ctrlRun: () => openFile(f.path, "reveal"),
            ctrlHint: "folder",
            stay: true,
            keepText: true,
          });
        }
        // "turn on hotspot", "hotspot off", "open bluetooth settings".
        const want = /\b(on|enable|start)\b/.test(q) ? true : /\b(off|disable|stop)\b/.test(q) ? false : null;
        const wq = q
          .replace(/^(open\s+)?(windows\s+)?settings?\s*/, "")
          .replace(/\b(turn|switch|set|please|the|my|on|off|enable|disable|start|stop)\b/g, " ")
          .replace(/\s+/g, " ")
          .trim();
        const pages = (
          wq.length >= 3 ? WINDOWS_PAGES.filter((p) => p.words.some((w) => w.startsWith(wq) || wq.startsWith(w))) : []
        ).slice(0, 2);
        named(
          "Windows settings",
          pages.flatMap((p) => {
            const open: Command = {
              id: `winset:${p.page}`,
              label: `${p.label} settings`,
              hint: "Windows Settings",
              icon: "settings" as const,
              run: () =>
                void runOpen(async () => {
                  await api.windowsSettingsOpen(p.page);
                  return true;
                }, `Opened ${p.label} settings`),
            };
            const name = p.switch;
            if (!name) return [open];
            const flip = (on: boolean): Command => ({
              id: `switch:${name}:${on}`,
              label: `Turn ${p.label.replace(/^Do Not/, "do not")} ${on ? "on" : "off"}`,
              hint: "This PC",
              icon: "settings" as const,
              stay: true,
              run: () => {
                const asked = `Turn ${p.label} ${on ? "on" : "off"}`;
                const say = (content: string, error?: string) =>
                  useSidekick.setState((st) => ({
                    turns: [...st.turns, { role: "user", content: asked }, { role: "assistant", content, error }],
                  }));
                void api.pcSwitch(name, on).then(
                  (done) => say(`${done}.`),
                  (e: unknown) => say("", String(e)),
                );
              },
            });
            const flips = want === null ? [flip(true), flip(false)] : [flip(want)];
            return [...flips, open];
          }),
        );
        named(
          "Settings",
          SETTINGS_TABS.filter((t) => t.label.toLowerCase().includes(q.replace(/^settings?\s*/, "")))
            .slice(0, 2)
            .map((t) => ({
              id: `settings:${t.id}`,
              label: t.label,
              hint: "Settings",
              icon: "settings" as const,
              run: () => setAsk({ view: "settings", settingsTab: t.id }),
              stay: true,
            })),
        );
        named(
          "Chats",
          chats
            .filter((c) => c.title.toLowerCase().includes(q))
            .slice(0, 2)
            .map((c) => ({
              id: `chat:${c.id}`,
              label: c.title,
              hint: "Past chat",
              icon: "history" as const,
              run: () => void openChat(c.id),
              stay: true,
            })),
        );
        items.push(
          {
            id: "search",
            group: "More",
            icon: <Icon name="ask" size={12} />,
            label: "Search my stuff",
            hint: "Files, pages, chats",
            run: () => {
              const query = text.trim();
              void api.search(query).then((found) => setHits({ query, items: found }));
              setText("");
            },
            stay: true,
            keepText: true,
          },
          {
            id: "skill",
            icon: <Icon name="settings" size={12} />,
            label: "Teach Sidekick a skill",
            run: () => {
              startSkill(text.trim());
              setText("");
            },
            stay: true,
            keepText: true,
          },
        );
      }
    } else if (slash) {
      for (const c of commands.filter((c) => c.label.toLowerCase().includes(q))) items.push(commandItem(c));
    } else if (!inChat) {
      for (const c of starters) items.push(commandItem(c));
      const pause = commands.find((c) => c.id === "pause60" || c.id === "resume");
      if (pause)
        items.push(commandItem(pause.id === "pause60" ? { ...pause, label: "Pause suggestions for an hour" } : pause));
      items.push({
        id: "all",
        icon: <span className="text-[11px]">···</span>,
        label: "All commands",
        hint: "Screen, clipboard, mute, settings",
        key: "/",
        run: () => {
          setText("/");
          setSelected(0);
          inputRef.current?.focus({ preventScroll: true });
        },
        stay: true,
        keepText: true,
      });
    }
  }
  const rows = items.length;
  // A short name ("slack", "settings") picks its match; a question picks Ask.
  // "turn on hotspot" picks the switch itself.
  const switchRow = /\b(on|off|enable|disable)\b/i.test(text) ? items.findIndex((i) => i.id.startsWith("switch:")) : -1;
  const intentRow = openWhere
    ? 0
    : asking && switchRow > 0
      ? switchRow
      : asking && rows > 3 && looksLikeName(text)
        ? 1
        : 0;
  const active = Math.min(selected === INTENT_PENDING ? intentRow : selected, Math.max(rows - 1, 0));
  // Models that can answer now; the picked one (if still there) goes first.
  const choices = providers.filter((p) => p.available && (!ask.localOnly || p.local) && p.id !== "semif");
  const pickedModel = choices.find((p) => p.id === askModel) ?? null;
  const best = pickedModel ?? choices[0];

  const goTab = (next: AskTab) => {
    setTab(next);
    setText("");
    setPick(0);
    setOpenWhere(null);
    if (next === "history") {
      setClips(null);
      setHits(null);
      void api
        .chatsList()
        .then(setChats)
        .catch(() => setChats([]));
    }
    // Agents focuses its own composer; Ask/History use this input.
    if (next !== "agents") {
      requestAnimationFrame(() => inputRef.current?.focus({ preventScroll: true }));
    }
  };

  const pickTab = (next: PanelTab) => (next === "settings" ? setAsk({ view: "settings" }) : goTab(next));

  altKeys.current = {
    s: toggleSpeak,
    v: () => (hearing !== null ? stopListening() : voiceReady && !streaming && startListening()),
    p: () => setAsk({ localOnly: !ask.localOnly }),
    h: () => !streaming && goTab(tab === "history" ? "ask" : "history"),
    o: () => tab === "agents" && setLayout("one"),
    b: () => tab === "agents" && setLayout("board"),
    tab: (back) => pickTab(nextTab(tab, coder, !!back)),
    m: () => {
      if (choices.length < 2) return;
      // Auto, then each model that can answer now.
      const ids = [null, ...choices.map((c) => c.id)];
      setAskModel(ids[(ids.indexOf(pickedModel?.id ?? null) + 1) % ids.length]);
    },
  };

  const runRow = (i: number) => {
    const item = items[i];
    if (!item) return;
    item.run();
    if (!item.keepText) setText("");
    if (!item.stay) void api.askClose();
  };

  // Clipboard, search results, and history: arrows move, Enter uses it.
  const listLen = showClips ? shownClips.length : inHistory ? history.length : showHits && hits ? hits.items.length : 0;
  const openListItem = (i: number) => {
    if (showClips) {
      const c = shownClips[i];
      if (c) void api.clipboardCopy(c.text).then(() => api.askClose());
    } else if (inHistory) {
      history[i]?.open();
      setText("");
    } else if (showHits && hits) {
      const h = hits.items[i];
      if (h) openHit(h.source, h.reference, h.title);
    }
  };

  escRef.current = () => {
    if (tab === "agents") {
      // Esc interrupts the agent that is working; otherwise it closes Ask.
      const { sessions, current } = useAgents.getState();
      const s = sessions.find((x) => x.id === current);
      if (s && (s.status === "working" || s.status === "waiting")) interruptSession(s.id);
      else void api.askClose();
      return;
    }
    if (openWhere) {
      setOpenWhere(null);
      return;
    }
    if (inHistory || slash) {
      if (inHistory) setTab("ask");
      setText("");
      return;
    }
    const current = useSidekick.getState();
    // Listening: Esc only stops the mic and leaves the box ready to type in.
    if (current.hearing !== null || current.mascot === "listening") {
      stopListening();
      requestAnimationFrame(() => inputRef.current?.focus({ preventScroll: true }));
      return;
    }
    if (current.turns.length > 0 || current.chatId !== null) {
      resetChat();
    } else {
      void api.askClose();
    }
  };

  const onKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (listLen && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
      e.preventDefault();
      const step = e.key === "ArrowDown" ? 1 : -1;
      setPick((p) => (Math.min(p, listLen - 1) + step + listLen) % listLen);
    } else if (listLen && e.key === "Enter" && !e.ctrlKey && !e.metaKey) {
      e.preventDefault();
      openListItem(Math.min(pick, listLen - 1));
    } else if (e.key === "ArrowDown" && rows) {
      e.preventDefault();
      setSelected((active + 1) % rows);
    } else if (e.key === "ArrowUp" && rows) {
      e.preventDefault();
      setSelected((active - 1 + rows) % rows);
    } else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      const item = rows ? items[active] : undefined;
      if (item?.ctrlRun) {
        item.ctrlRun();
        return;
      }
      // Otherwise: this conversation (and what is typed) goes to the coding agent.
      if (!agent) return;
      const messages = turns
        .filter((t) => !t.error && t.content.trim())
        .map(({ role, content }) => ({ role, content }));
      if (text.trim()) messages.push({ role: "user", content: text.trim() });
      if (messages.length === 0) return;
      setText("");
      setHandoffError(null);
      // It carries on in the Agents tab, step by step.
      void handOff(messages, null).catch((err: unknown) => setHandoffError(String(err)));
    } else if (e.key === "Enter") {
      e.preventDefault();
      // In a conversation Enter sends the follow-up.
      if (asking && inChat) runRow(0);
      else if (rows) runRow(active);
    } else if (e.key === "Escape") {
      e.preventDefault();
      if (!e.repeat) escRef.current();
    }
  };

  const onScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    nearBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 48;
  };

  const tabs = (
    <PanelTabs
      current={tab}
      onPick={pickTab}
      coder={coder}
      working={working}
      alt={alt}
      extra={
        tab === "agents" && (
          <fieldset aria-label="Layout" className="ak-lay ak-seg border-0">
            {(
              [
                ["one", "One", "Alt O"],
                ["board", "Board", "Alt B"],
              ] as const
            ).map(([id, label, key]) => (
              <button
                key={id}
                type="button"
                aria-pressed={layout === id}
                aria-label={label}
                title={`${label} (${key})`}
                onClick={() => setLayout(id)}
                className="chip relative"
              >
                <svg viewBox="0 0 16 16" aria-hidden="true" className="size-3.5">
                  {id === "one" ? (
                    <rect
                      x="3"
                      y="3"
                      width="10"
                      height="10"
                      rx="2"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="1.4"
                    />
                  ) : (
                    <path
                      d="M3 3h4v4H3zM9 3h4v4H9zM3 9h4v4H3zM9 9h4v4H9z"
                      fill="currentColor"
                      stroke="currentColor"
                      strokeWidth="0.6"
                      strokeLinejoin="round"
                    />
                  )}
                </svg>
                <KeyHint show={alt}>{key}</KeyHint>
              </button>
            ))}
          </fieldset>
        )
      }
    />
  );

  if (tab === "repos" && coder) {
    return (
      <div className="ak">
        {tabs}
        <Repos maxHeight={scrollMax} />
      </div>
    );
  }

  if (tab === "agents" && coder) {
    return (
      <div className="ak">
        {tabs}
        <AgentsTab keys={alt} maxHeight={scrollMax} />
      </div>
    );
  }

  // First-run card only when nothing is set up yet. If a provider is
  // configured but not running, the footer says so — not this picker.
  const firstRun = !anySetUp && providersData !== null && !inChat && !asking && !slash && !inHistory && !showClips;
  const footer =
    !best && !firstRun ? (
      <div className="ak-foot items-center">
        {providersData === null ? (
          <SetupSpinner className="text-white/62" />
        ) : (
          <span className="size-1.5 self-center rounded-full bg-[#ffd60a]" aria-hidden="true" />
        )}
        <span className="truncate" aria-live="polite">
          {providersData === null
            ? "Checking AI…"
            : ask.localOnly
              ? "No local model running"
              : "No AI set up yet. See Settings > AI"}
        </span>
      </div>
    ) : alt ? (
      <div className="ak-foot">
        <span>
          <kbd>Enter</kbd>{" "}
          {items[active]?.ctrlRun
            ? "open"
            : asking
              ? "ask"
              : showClips
                ? "copy"
                : showHits || inHistory
                  ? "open"
                  : "run"}
        </span>
        {items[active]?.ctrlRun ? (
          <span>
            <kbd>Ctrl Enter</kbd> {items[active]?.ctrlHint ?? "open"}
          </span>
        ) : (
          agent &&
          (asking || inChat) && (
            <span>
              <kbd>Ctrl Enter</kbd> continue in {agent}
            </span>
          )
        )}
        <span>
          <kbd>Ctrl Tab</kbd> switch tab
        </span>
        <span>
          <kbd>Esc</kbd> {hearing !== null ? "stop mic" : inChat || streaming ? "new chat" : "close"}
        </span>
      </div>
    ) : !inChat && !asking && !inHistory && EARLY_SESSIONS ? (
      // Only where you are about to type, for your first few sessions.
      <div className="ak-foot">
        <span>
          Hold <kbd>Alt</kbd> for shortcuts · <kbd>/</kbd> for all commands
        </span>
      </div>
    ) : null;

  const field = (
    <div className="ak-bar">
      {hearing !== null ? (
        <Hearing text={hearing} />
      ) : (
        <input
          ref={inputRef}
          value={text}
          onChange={(e) => {
            setText(e.target.value);
            setOpenWhere(null);
            setSelected(INTENT_PENDING);
            setPick(0);
          }}
          onKeyDown={onKey}
          placeholder={inHistory ? "Search history" : inChat ? "Ask a follow-up" : "Ask anything"}
          spellCheck={false}
          className="ak-q"
        />
      )}
      <div className="ak-right">
        {inHistory ? (
          <fieldset className="ak-seg border-0" aria-label="Show">
            {(["all", "chats", "agents"] as const).map((k) => (
              <button
                key={k}
                type="button"
                aria-pressed={historyKind === k}
                onClick={() => setHistoryKind(k)}
                className="chip capitalize"
              >
                {k}
              </button>
            ))}
          </fieldset>
        ) : hearing !== null ? (
          <button type="button" onClick={stopListening} className="ak-stop chip">
            Stop <kbd>Esc</kbd>
          </button>
        ) : (
          <>
            <IconButton
              label={speak ? "Speak replies: on (Alt S)" : "Speak replies: off (Alt S)"}
              pressed={speak}
              keys={alt}
              hint="Alt S"
              onClick={toggleSpeak}
            >
              <Icon name={speak ? "speaker" : "speakerOff"} size={14} />
            </IconButton>
            {voiceReady && !streaming && (
              <IconButton
                label={`Talk, or say Hey ${assistant} (Alt V)`}
                keys={alt}
                hint="Alt V"
                onClick={startListening}
              >
                <Icon name="mic" size={14} />
              </IconButton>
            )}
            {streaming ? (
              <button type="button" onClick={cancelChat} className="ak-stop chip">
                Stop <kbd>Esc</kbd>
              </button>
            ) : (
              <>
                {best && <ModelPicker choices={choices} best={best} picked={pickedModel} keys={alt} />}
                {inChat && !showChat && (
                  <IconButton label="New chat (Esc)" keys={alt} hint="Esc" onClick={resetChat}>
                    <Icon name="plus" size={15} />
                  </IconButton>
                )}
              </>
            )}
          </>
        )}
      </div>
    </div>
  );

  return (
    <div className="ak" data-chat={showChat || undefined}>
      {tabs}
      {showChat && (
        <div className="ak-head">
          <input
            aria-label="Chat name"
            value={chatName ?? turns.find((t) => t.role === "user")?.content ?? ""}
            onChange={(e) => setChatName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === "Escape") {
                e.preventDefault();
                inputRef.current?.focus();
              }
            }}
            spellCheck={false}
            className="ak-title ak-cname"
          />
          <IconButton label="New chat (Esc)" keys={alt} hint="Esc" onClick={resetChat}>
            <Icon name="plus" size={15} />
          </IconButton>
        </div>
      )}
      {/* In a chat the field is a box at the bottom: what goes with the
          question on top, then the field, then the model and voice. The
          same wrapper either way, so the input keeps focus on send. */}
      <div className={showChat ? "ak-askc" : "contents"}>
        {field}
        {!inHistory && <ContextLine keys={alt} />}
      </div>

      {showClips && clips ? (
        <div key="clips" className="ak-scroll ak-in" style={{ maxHeight: scrollMax }}>
          <Clips
            items={shownClips}
            filtered={clipFilter.length > 0}
            active={Math.min(pick, shownClips.length - 1)}
            onHover={setPick}
          />
        </div>
      ) : inHistory ? (
        <div key="history" className="ak-scroll ak-in" style={{ maxHeight: scrollMax }}>
          <HistoryList rows={history} active={Math.min(pick, Math.max(history.length - 1, 0))} onHover={setPick} />
        </div>
      ) : showHits && hits ? (
        <div key="hits" className="ak-scroll ak-in" style={{ maxHeight: scrollMax }}>
          <Results
            query={hits.query}
            items={hits.items}
            active={Math.min(pick, hits.items.length - 1)}
            onHover={setPick}
            onOpen={(h) => openHit(h.source, h.reference, h.title)}
          />
        </div>
      ) : showChat && !(asking && rows > 0 && !inChat) ? (
        <div key="chat" ref={chatRef} onScroll={onScroll} className="ak-scroll" style={{ maxHeight: scrollMax }}>
          <Chat turns={turns} />
        </div>
      ) : firstRun ? (
        <FirstRun />
      ) : (
        rows > 0 && (
          <ul
            key={slash ? "slash" : asking ? "typing" : "start"}
            className="ak-list ak-scroll"
            style={{ maxHeight: scrollMax }}
          >
            {items.map((it, i) => (
              <li key={it.id} ref={scrollIfActive(active === i)}>
                {it.group && <p className="ak-group">{it.group}</p>}
                <button
                  type="button"
                  data-sel={active === i}
                  onMouseMove={() => setSelected(i)}
                  onClick={() => runRow(i)}
                  className="ak-row"
                >
                  <span className="ak-ico">{it.icon}</span>
                  <span className="min-w-0 flex-1 truncate">{it.label}</span>
                  {it.hint && <span className="ak-hint">{it.hint}</span>}
                  {it.key ? (
                    <kbd className="ak-key">{it.key}</kbd>
                  ) : (
                    asking &&
                    active === i &&
                    (it.ctrlRun ? (
                      <span className="ak-keys">
                        <kbd className="ak-key">Enter</kbd>
                        <kbd className="ak-key">Ctrl Enter</kbd>
                      </span>
                    ) : (
                      <kbd className="ak-key">Enter</kbd>
                    ))
                  )}
                </button>
              </li>
            ))}
          </ul>
        )
      )}

      {handoffError && <p className="ak-err">{handoffError}</p>}
      {footer}
      <Timings />
    </div>
  );
}

/** One row under the input. */
interface Item {
  id: string;
  /** Starts a named group ("Projects"). */
  group?: string;
  icon: ReactNode;
  label: ReactNode;
  hint?: string;
  /** A key shown at the end ("/"). */
  key?: string;
  run: () => void;
  /** Ctrl Enter: e.g. show a file's folder, or browser incognito. */
  ctrlRun?: () => void;
  /** Footer label for Ctrl Enter ("folder", "incognito"). */
  ctrlHint?: string;
  /** Keep Ask open after running. */
  stay?: boolean;
  /** The row clears the input itself (or keeps it). */
  keepText?: boolean;
}

function commandItem(c: Command): Item {
  return {
    id: c.id,
    icon:
      c.id === "starter:error" ? (
        <span className="text-[11px] font-semibold">!</span>
      ) : (
        <Icon name={c.icon} size={12} />
      ),
    label: c.label,
    hint: c.hint,
    run: c.run,
    stay: c.stay,
  };
}

type HistoryKind = "all" | "chats" | "agents";

interface HistoryRow {
  id: string;
  group: string;
  agent: boolean;
  title: string;
  meta: string;
  open: () => void;
  remove?: () => void;
}

/** Past chats and agent sessions, newest first, under Today / Yesterday / Earlier. */
function historyRows(chats: ChatSummary[], sessions: Session[], kind: HistoryKind, q: string): HistoryRow[] {
  const day = (ms: number) => {
    const d = new Date(ms);
    const today = new Date();
    const start = new Date(today.getFullYear(), today.getMonth(), today.getDate()).getTime();
    return ms >= start ? "Today" : ms >= start - 86_400_000 ? "Yesterday" : d.toLocaleDateString();
  };
  const all: (HistoryRow & { at: number })[] = [];
  if (kind !== "agents") {
    for (const c of chats) {
      const at = Date.parse(c.updated) || 0;
      all.push({
        id: `chat:${c.id}`,
        group: day(at),
        agent: false,
        title: c.title,
        meta: ago(c.updated),
        at,
        open: () => {
          setTab("ask");
          void openChat(c.id);
        },
      });
    }
  }
  if (kind !== "chats") {
    for (const s of sessions) {
      all.push({
        id: `agent:${s.id}`,
        group: day(s.startedAt),
        agent: true,
        title: s.title,
        meta: [
          s.agent,
          s.project.split(/[\\/]/).filter(Boolean).pop(),
          s.changes ? `${s.changes} ${s.changes === 1 ? "change" : "changes"}` : "",
        ]
          .filter(Boolean)
          .join(" · "),
        at: s.startedAt,
        open: () => useAgents.setState({ current: s.id, tab: "agents" }),
      });
    }
  }
  return all
    .filter((r) => !q || r.title.toLowerCase().includes(q))
    .sort((a, b) => b.at - a.at)
    .map(({ at: _, ...r }) => r);
}

function HistoryList({ rows, active, onHover }: { rows: HistoryRow[]; active: number; onHover: (i: number) => void }) {
  if (rows.length === 0) return <p className="ak-group py-2">Nothing here yet.</p>;
  return (
    <ul className="ak-list" aria-label="History">
      {rows.map((r, i) => (
        <li key={r.id} ref={scrollIfActive(i === active)}>
          {(i === 0 || rows[i - 1].group !== r.group) && <p className="ak-group">{r.group}</p>}
          <button
            type="button"
            data-sel={i === active}
            onMouseMove={() => onHover(i)}
            onClick={r.open}
            className="ak-row"
          >
            <span className="ak-ico">
              <Icon name={r.agent ? "terminal" : "ask"} size={12} />
            </span>
            <span className="min-w-0 flex-1 truncate">{r.title}</span>
            <span className="ak-hint">{r.meta}</span>
          </button>
        </li>
      ))}
    </ul>
  );
}

/** A round header button with its key badge while Alt is held. */
function IconButton({
  label,
  hint,
  keys,
  pressed,
  onClick,
  children,
}: {
  label: string;
  hint: string;
  keys: boolean;
  pressed?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <span className="relative shrink-0">
      <Tip label={label}>
        <button type="button" aria-label={label} aria-pressed={pressed} onClick={onClick} className="ak-ibtn chip">
          {children}
        </button>
      </Tip>
      <KeyHint show={keys}>{hint}</KeyHint>
    </span>
  );
}

/** Selected row not chosen yet: the panel picks by what was typed. */
const INTENT_PENDING = -1;

/** Start-menu private browsing shortcuts (Zen ships one; we synthesize the rest). */
function isPrivateAppName(name: string): boolean {
  return /\b(private|incognito|inprivate)\b/i.test(name);
}

/** Label for a synthesized private browser row. */
function privateBrowserLabel(browser: string): string {
  switch (browser) {
    case "edge":
      return "Edge InPrivate";
    case "firefox":
      return "Firefox Private";
    case "zen":
      return "Zen Private";
    case "brave":
      return "Brave Incognito";
    case "samsung":
      return "Samsung Internet Private";
    default:
      return "Chrome Incognito";
  }
}

/** A short name ("slack", "dark mode", "settings"), not a question. */
export function looksLikeName(text: string): boolean {
  const t = text.trim();
  if (!t || /[?]/.test(t)) return false;
  if (/^(what|why|how|who|when|where|which|can|could|should|is|are|do|does|explain|tell|write|summari[sz]e)\b/i.test(t))
    return false;
  return t.split(/\s+/).length <= 3;
}

/** The voice's rainbow bars. */
export function VoiceBars() {
  return (
    <span className="voice-bars shrink-0" role="img" aria-label="Listening">
      <i />
      <i />
      <i />
      <i />
      <i />
      <i />
    </span>
  );
}

/** Live transcript while listening: the bars, then your words a size
 * larger, or "Listening..." until the first one. */
export function Hearing({ text }: { text: string }) {
  return (
    <div className="flex min-w-0 flex-1 items-center gap-2.5" aria-live="polite">
      <VoiceBars />
      <span
        className={`min-w-0 flex-1 truncate font-display text-[17px] tracking-[-0.015em] ${
          text ? "text-white" : "text-[rgb(235_235_245/0.45)]"
        }`}
      >
        {text || "Listening..."}
      </span>
    </div>
  );
}
