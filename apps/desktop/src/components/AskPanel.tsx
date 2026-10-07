"use client";

// Ask mode: the island itself becomes the place to run a
// command or ask Sidekick, with the app you were in and the clipboard
// attachable on request. Rendered inside the island shell; the island owns
// the morph, this owns the content.

import { type KeyboardEvent, type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { type AskTab, activeCount, handOff, listenToAgents, type Session, setTab, useAgents } from "@/lib/agents";
import { api } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import { useAltHeld } from "@/lib/hooks";
import {
  cancelChat,
  newChat,
  openChat,
  sendChat,
  setAsk,
  setAskModel,
  startListening,
  startSkill,
  stopListening,
  updateSettings,
  useSidekick,
} from "@/lib/store";
import {
  type CalendarToday,
  type ChatSummary,
  type InstantResults,
  isPaused,
  type ProviderStatus,
  type SearchHit,
} from "@/lib/types";
import { AgentsTab } from "./agents/AgentsTab";
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
import { SETTINGS_TABS } from "./SettingsPanel";
import { SetupSpinner } from "./SetupChecklistRow";

/** Space the island's orb takes at the top-left in Settings and the welcome. */
export const ASK_ORB = 30;
/** The mascot in Ask: 36 px, on the input line under the tabs. */
export const ASK_MASCOT = { size: 36, x: 10, y: 40 } as const;
/** Ask's padding inside the island: top, sides, bottom. */
export const ASK_PAD = { top: 8, x: 10, bottom: 10 } as const;

/** Input + chips + footer + gaps; scroll area keeps the rest under the Ask cap. */
const ASK_CHROME = 118;
const ASK_SCROLL_CAP = 330;
/** Island window is ~560 tall (tauri.conf); leave room for chrome + pad. */
const ASK_SCROLL_FLOOR = 120;

function askScrollMax(): number {
  const available = window.innerHeight - ASK_CHROME - 40;
  return Math.min(ASK_SCROLL_CAP, Math.max(ASK_SCROLL_FLOOR, available));
}

export function AskPanel() {
  const ask = useSidekick((s) => s.ask);
  const turns = useSidekick((s) => s.turns);
  const chatId = useSidekick((s) => s.chatId);
  const hearing = useSidekick((s) => s.hearing);
  const voiceReady = useSidekick((s) => s.settings.voice.enabled && (s.voiceStatus?.listening ?? false));
  const settings = useSidekick((s) => s.settings);
  const [text, setText] = useState(ask?.prompt ?? "");
  const alt = useAltHeld();
  const tab = useAgents((s) => s.tab);
  const working = useAgents((s) => activeCount(s.sessions));
  useEffect(listenToAgents, []);
  const speak = useSidekick((s) => s.settings.voice.speakAnswers);
  const toggleSpeak = useCallback(() => {
    const voice = useSidekick.getState().settings.voice;
    void updateSettings({ voice: { ...voice, speakAnswers: !voice.speakAnswers } });
  }, []);
  // Alt shortcuts work wherever focus is in Ask. What they act on changes
  // every render, so they read it from here.
  const altKeys = useRef<Record<string, () => void>>({});
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      // Ctrl Tab moves between Ask, Agents and History.
      if (e.ctrlKey && e.key === "Tab") {
        e.preventDefault();
        altKeys.current.tab?.();
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
  const [handoffError, setHandoffError] = useState<string | null>(null);
  const [hits, setHits] = useState<{ query: string; items: SearchHit[] } | null>(null);
  const [clips, setClips] = useState<{ text: string; ts: string }[] | null>(null);
  const [historyKind, setHistoryKind] = useState<HistoryKind>("all");
  const agentSessions = useAgents((s) => s.sessions);
  // The highlighted clip, search result, or history row, moved with the arrow keys.
  const [pick, setPick] = useState(0);
  // biome-ignore lint/correctness/useExhaustiveDependencies: a new list starts at its top
  useEffect(() => setPick(0), [clips, hits, tab, historyKind]);
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
    const id = requestAnimationFrame(() => inputRef.current?.focus());
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
        // After the row runs (which clears the input), type the prefix in.
        focusInput: (prefix) =>
          requestAnimationFrame(() => {
            setText(prefix);
            inputRef.current?.focus();
          }),
      }),
    [ask?.context, chatPage, calendar],
  );
  const resetChat = useCallback(() => {
    if (useSidekick.getState().hearing !== null) stopListening();
    newChat();
    setText("");
    setSelected(0);
    setClips(null);
    setHits(null);
    setPick(0);
    nearBottom.current = true;
    requestAnimationFrame(() => inputRef.current?.focus());
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

  // The rows under the input: starters, then everything that matches by
  // name (instant, no AI), then Search and Teach at the end.
  const items: Item[] = [];
  if (!inHistory && !showClips && !showHits && hearing === null) {
    if (asking) {
      items.push({
        id: "ask",
        icon: <span className="text-[12px]">✦</span>,
        label: (
          <>
            Ask Sidekick: <span className="text-[rgb(235_235_245/0.6)]">“{text.trim()}”</span>
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
        for (const [n, a] of instant.apps.entries()) {
          items.push({
            id: `app:${a.id}`,
            group: n === 0 ? "Apps" : undefined,
            icon: <span className="text-[11px] font-bold text-white">{a.name.slice(0, 1).toUpperCase()}</span>,
            label: a.name,
            hint: a.minutes >= 60 ? `${Math.round(a.minutes / 60)} h this week` : undefined,
            run: () => void api.appLaunch(a.id),
          });
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
              run: () => void api.projectLaunch(p.path),
            })),
        );
        for (const [n, f] of instant.files.entries()) {
          items.push({
            id: `file:${f.path}`,
            group: n === 0 ? "Files" : undefined,
            icon: <Icon name={f.folder ? "folder" : "file"} size={12} />,
            label: f.name,
            hint: f.place,
            run: () => void api.fileOpen(f.path),
          });
        }
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
          inputRef.current?.focus();
        },
        stay: true,
        keepText: true,
      });
    }
  }
  const rows = items.length;
  // A short name ("slack", "settings") picks its match; a question picks Ask.
  const intentRow = asking && rows > 3 && looksLikeName(text) ? 1 : 0;
  const active = Math.min(selected === INTENT_PENDING ? intentRow : selected, Math.max(rows - 1, 0));
  // Models that can answer now; the picked one (if still there) goes first.
  const choices = providers.filter((p) => p.available && (!ask.localOnly || p.local) && p.id !== "semif");
  const pickedModel = choices.find((p) => p.id === askModel) ?? null;
  const best = pickedModel ?? choices[0];

  const goTab = (next: AskTab) => {
    setTab(next);
    setText("");
    setPick(0);
    if (next === "history") {
      setClips(null);
      setHits(null);
      void api
        .chatsList()
        .then(setChats)
        .catch(() => setChats([]));
    }
    requestAnimationFrame(() => inputRef.current?.focus());
  };

  altKeys.current = {
    s: toggleSpeak,
    v: () => (hearing !== null ? stopListening() : voiceReady && !streaming && startListening()),
    p: () => setAsk({ localOnly: !ask.localOnly }),
    h: () => !streaming && goTab(tab === "history" ? "ask" : "history"),
    tab: () => {
      const order: AskTab[] = ["ask", "agents", "history"];
      goTab(order[(order.indexOf(tab) + 1) % order.length]);
    },
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
      if (h) void api.openReference(h.source, h.reference);
    }
  };

  escRef.current = () => {
    if (tab === "agents") {
      // Esc interrupts the agent that is working; otherwise it closes Ask.
      const { sessions, current } = useAgents.getState();
      const s = sessions.find((x) => x.id === current);
      if (s && (s.status === "working" || s.status === "waiting")) void api.agentStop(s.id);
      else void api.askClose();
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
      requestAnimationFrame(() => inputRef.current?.focus());
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
      // Ctrl Enter: this conversation (and what is typed) goes to the coding agent.
      e.preventDefault();
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
    <div className="ak-tabs" role="tablist">
      {(
        [
          ["ask", "Ask", null],
          ["agents", "Agents", working],
          ["history", "History", "Alt H"],
        ] as const
      ).map(([id, label, extra]) => (
        <span key={id} className="relative">
          <button type="button" role="tab" aria-selected={tab === id} onClick={() => goTab(id)} className="ak-tab chip">
            {label}
            {typeof extra === "number" && extra > 0 && <i className="n not-italic">{extra}</i>}
          </button>
          {typeof extra === "string" && <KeyHint show={alt}>{extra}</KeyHint>}
        </span>
      ))}
      <span className="ak-tabkey mono">Ctrl Tab</span>
    </div>
  );

  if (tab === "agents") {
    return (
      <div className="ak">
        {tabs}
        <AgentsTab keys={alt} maxHeight={scrollMax} />
      </div>
    );
  }

  const lastQuestion = turns.findLast((t) => t.role === "user")?.content ?? "";
  const askedInBar = inChat && !text && !inHistory;
  // No model yet: the first-run card says how to add one, so the footer
  // keeps its usual line.
  const firstRun = !best && providersData !== null && !inChat && !asking && !slash && !inHistory && !showClips;
  const footer =
    !best && !firstRun ? (
      <div className="ak-foot items-center">
        {providersData === null ? (
          <SetupSpinner className="text-white/50" />
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
          <kbd>Enter</kbd> {asking ? "ask" : showClips ? "copy" : showHits || inHistory ? "open" : "run"}
        </span>
        {agent && (asking || inChat) && (
          <span>
            <kbd>Ctrl Enter</kbd> continue in {agent}
          </span>
        )}
        <span>
          <kbd>Ctrl Tab</kbd> switch tab
        </span>
        <span>
          <kbd>Esc</kbd> {hearing !== null ? "stop mic" : inChat || streaming ? "new chat" : "close"}
        </span>
      </div>
    ) : (
      <div className="ak-foot">
        <span>
          Hold <kbd>Alt</kbd> for shortcuts · <kbd>/</kbd> for all commands
        </span>
      </div>
    );

  return (
    <div className="ak">
      {tabs}
      <div className="ak-bar">
        {hearing !== null ? (
          <Hearing text={hearing} />
        ) : (
          <input
            ref={inputRef}
            value={text}
            onChange={(e) => {
              setText(e.target.value);
              setSelected(INTENT_PENDING);
              setPick(0);
            }}
            onKeyDown={onKey}
            data-asked={askedInBar}
            placeholder={inHistory ? "Search history" : askedInBar ? lastQuestion : "Ask anything"}
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
                <IconButton label="Talk, or say Hey Sidekick (Alt V)" keys={alt} hint="Alt V" onClick={startListening}>
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
                  {inChat && (
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

      {!inHistory && <ContextLine keys={alt} />}

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
          />
        </div>
      ) : showChat && !(asking && rows > 0 && !inChat) ? (
        <div key="chat" ref={chatRef} onScroll={onScroll} className="ak-scroll" style={{ maxHeight: scrollMax }}>
          <Chat turns={turns} askedInBar={askedInBar} />
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
              <li key={it.id}>
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
                    asking && active === i && <kbd className="ak-key">Enter</kbd>
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
        meta: [s.agent, s.project, s.changes ? `${s.changes} ${s.changes === 1 ? "change" : "changes"}` : ""]
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
      <button
        type="button"
        aria-label={label}
        title={label}
        aria-pressed={pressed}
        onClick={onClick}
        className="ak-ibtn chip"
      >
        {children}
      </button>
      <KeyHint show={keys}>{hint}</KeyHint>
    </span>
  );
}

/** Selected row not chosen yet: the panel picks by what was typed. */
const INTENT_PENDING = -1;

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
