"use client";

// Ask mode: the island itself becomes the place to run a
// command or ask Sidekick, with the app you were in and the clipboard
// attachable on request. Rendered inside the island shell; the island owns
// the morph, this owns the content.

import { AnimatePresence, motion } from "motion/react";
import { type KeyboardEvent, type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from "react";
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
import { type CalendarToday, type ChatSummary, isPaused, type ProviderStatus, type SearchHit } from "@/lib/types";
import { Chat, useAgentName } from "./ask/Chat";
import { ChatHistory, Clips, Results } from "./ask/Lists";
import { ModelPicker } from "./ask/ModelPicker";
import { Kbd, KeyHint, Pill, Row } from "./ask/parts";
import type { Command } from "./ask/Starters";
import { ContextLine, contextStarters, soonestMeeting } from "./ask/Starters";
import { Timings } from "./ask/Timings";
import { Icon } from "./Icon";
import { SETTINGS_TABS } from "./SettingsPanel";
import { SetupSpinner } from "./SetupChecklistRow";

/** Space the island's orb takes at the top-left in Ask mode. */
export const ASK_ORB = 30;

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
  const [historyOpen, setHistoryOpen] = useState(false);
  // The highlighted clip, search result, or history row, moved with the arrow keys.
  const [pick, setPick] = useState(0);
  // biome-ignore lint/correctness/useExhaustiveDependencies: a new list starts at its top
  useEffect(() => setPick(0), [clips, hits, historyOpen]);
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
    setSelected(0);
    setClips(null);
    setHistoryOpen(false);
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
    setHistoryOpen(false);
    setPick(0);
    nearBottom.current = true;
    requestAnimationFrame(() => inputRef.current?.focus());
  }, []);

  const openHistory = useCallback(() => {
    setHistoryOpen((open) => {
      if (open) return false;
      setClips(null);
      setHits(null);
      setPick(0);
      void api
        .chatsList()
        .then(setChats)
        .catch(() => setChats([]));
      return true;
    });
  }, []);

  const removeChat = useCallback(
    (id: string) => {
      void api.chatDelete(id).then(() => {
        setChats((list) => list.filter((c) => c.id !== id));
        if (useSidekick.getState().conversation === id) resetChat();
      });
    },
    [resetChat],
  );

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
          setHistoryOpen(false);
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
    const q = text.trim().toLowerCase();
    if (!q) return turns.length ? all : [...starters, ...all];
    // Typing a project's name offers to open it.
    const launch: Command[] = projects
      .filter((p) => p.name.toLowerCase().includes(q.replace(/^open\s+/, "")))
      .slice(0, 4)
      .map((p) => ({
        id: `project:${p.path}`,
        label: `Open ${p.name}`,
        hint: "Editor and terminal",
        icon: "folder",
        run: () => void api.projectLaunch(p.path),
      }));
    // Settings screens and past chats show up by name too.
    const screens: Command[] = SETTINGS_TABS.filter((t) =>
      t.label.toLowerCase().includes(q.replace(/^settings?\s*/, "")),
    )
      .slice(0, 2)
      .map((t) => ({
        id: `settings:${t.id}`,
        label: `${t.label} settings`,
        hint: "Settings",
        icon: "settings",
        run: () => setAsk({ view: "settings", settingsTab: t.id }),
        stay: true,
      }));
    const past: Command[] = chats
      .filter((c) => c.title.toLowerCase().includes(q))
      .slice(0, 2)
      .map((c) => ({
        id: `chat:${c.id}`,
        label: c.title,
        hint: "Past chat",
        icon: "history",
        run: () => void openChat(c.id),
        stay: true,
      }));
    return [...all.filter((c) => c.label.toLowerCase().includes(q)), ...launch, ...screens, ...past];
  }, [paused, settings.muted, turns.length, text, projects, starters, resetChat, chats]);

  if (!ask) return null;

  const streaming = chatId !== null;
  // While clipboard or chat history is open, typing filters it instead of asking.
  const clipFilter = clips !== null ? text.trim().toLowerCase() : "";
  const historyFilter = historyOpen ? text.trim().toLowerCase() : "";
  const asking = text.trim().length > 0 && clips === null && !historyOpen;
  const shownClips = clips?.filter((c) => !clipFilter || c.text.toLowerCase().includes(clipFilter)) ?? [];
  const shownChats = chats.filter((c) => !historyFilter || c.title.toLowerCase().includes(historyFilter));
  // With a conversation going, the body keeps showing it while you type
  // the next question; commands show only before the first one.
  const inChat = turns.length > 0;
  const showClips = clips !== null;
  const showHistory = historyOpen && !showClips;
  const showHits = hits !== null && !asking && !showClips && !showHistory;
  const showChat = inChat && !showHits && !showClips && !showHistory;
  // While typing: Ask on top, then what matches by name (instant, no AI),
  // then Search and Teach a skill at the end.
  const lead = asking ? 1 : 0;
  const tail = asking ? 2 : 0;
  const rows = hearing !== null || showChat || showHits || showClips || showHistory ? 0 : lead + commands.length + tail;
  const searchRow = lead + commands.length;
  const skillRow = searchRow + 1;
  // A short name ("slack", "settings") picks its match; a question picks Ask.
  const intentRow = asking && commands.length > 0 && looksLikeName(text) ? lead : 0;
  const active = selected === INTENT_PENDING ? intentRow : selected;
  // Models that can answer now; the picked one (if still there) goes first.
  const choices = providers.filter((p) => p.available && (!ask.localOnly || p.local) && p.id !== "semif");
  const pickedModel = choices.find((p) => p.id === askModel) ?? null;
  const best = pickedModel ?? choices[0];

  altKeys.current = {
    s: toggleSpeak,
    v: () => (hearing !== null ? stopListening() : voiceReady && !streaming && startListening()),
    p: () => setAsk({ localOnly: !ask.localOnly }),
    h: () => !streaming && openHistory(),
    m: () => {
      if (choices.length < 2) return;
      // Auto, then each model that can answer now.
      const ids = [null, ...choices.map((c) => c.id)];
      setAskModel(ids[(ids.indexOf(pickedModel?.id ?? null) + 1) % ids.length]);
    },
  };

  const runRow = (i: number) => {
    if (asking && i === 0) {
      sendChat(text);
      setText("");
      return;
    }
    if (asking && i === searchRow) {
      const query = text.trim();
      void api.search(query).then((items) => setHits({ query, items }));
      setText("");
      return;
    }
    if (asking && i === skillRow) {
      startSkill(text.trim());
      setText("");
      return;
    }
    const cmd = commands[i - lead];
    if (!cmd) return;
    cmd.run();
    setText("");
    if (!cmd.stay) void api.askClose();
  };

  // Clipboard, search results, and chat history: arrows move, Enter uses it.
  const listLen = showClips
    ? shownClips.length
    : showHistory
      ? shownChats.length
      : showHits && hits
        ? hits.items.length
        : 0;
  const openListItem = (i: number) => {
    if (showClips) {
      const c = shownClips[i];
      if (c) void api.clipboardCopy(c.text).then(() => api.askClose());
    } else if (showHistory) {
      const c = shownChats[i];
      if (c) {
        setHistoryOpen(false);
        setText("");
        void openChat(c.id);
      }
    } else if (showHits && hits) {
      const h = hits.items[i];
      if (h) void api.openReference(h.source, h.reference);
    }
  };

  escRef.current = () => {
    if (historyOpen) {
      setHistoryOpen(false);
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
      void api
        .aiHandoff(messages, null)
        .then(() => api.askClose())
        .catch((err: unknown) => setHandoffError(String(err)));
    } else if (e.key === "Enter") {
      e.preventDefault();
      // In a conversation Enter sends the follow-up.
      if (asking && showChat) runRow(0);
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

  return (
    <div className="flex flex-col">
      <div className="flex h-[30px] items-center gap-2" style={{ paddingLeft: ASK_ORB + 10 }}>
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
            placeholder={
              historyOpen ? "Filter chats" : turns.length ? "Ask a follow-up" : "Ask Sidekick or type a command"
            }
            spellCheck={false}
            className="min-w-0 flex-1 bg-transparent font-display text-[17px] tracking-[-0.015em] text-white outline-none placeholder:text-[rgb(235_235_245/0.4)]"
          />
        )}
        {hearing !== null ? (
          <Pill onClick={stopListening}>Stop</Pill>
        ) : (
          <>
            <IconButton
              label={speak ? "Speak replies: on (Alt S)" : "Speak replies: off (Alt S)"}
              pressed={speak}
              keys={alt}
              hint="Alt S"
              onClick={toggleSpeak}
            >
              <Icon name={speak ? "speaker" : "speakerOff"} size={15} />
            </IconButton>
            {voiceReady && !streaming && (
              <IconButton label="Talk, or say Hey Sidekick (Alt V)" keys={alt} hint="Alt V" onClick={startListening}>
                <Icon name="mic" size={14} />
              </IconButton>
            )}
            {!streaming && (
              <IconButton
                label="Chat history (Alt H)"
                pressed={historyOpen}
                keys={alt}
                hint="Alt H"
                onClick={openHistory}
              >
                <Icon name="history" size={14} />
              </IconButton>
            )}
            {best && <ModelPicker choices={choices} best={best} picked={pickedModel} keys={alt} />}
            {streaming ? (
              <Pill onClick={cancelChat}>Stop</Pill>
            ) : (
              turns.length > 0 && (
                <IconButton label="New chat (Esc)" keys={alt} hint="Esc" onClick={resetChat}>
                  <Icon name="plus" size={16} />
                </IconButton>
              )
            )}
          </>
        )}
      </div>

      <ContextLine keys={alt} />

      <AnimatePresence initial={false} mode="popLayout">
        {showClips && clips ? (
          <motion.div
            key="clips"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0, transition: { duration: 0.08 } }}
            className="ask-scroll mt-2 overflow-y-auto pr-1"
            style={{ maxHeight: scrollMax }}
          >
            <Clips
              items={shownClips}
              filtered={clipFilter.length > 0}
              active={Math.min(pick, shownClips.length - 1)}
              onHover={setPick}
            />
          </motion.div>
        ) : showHistory ? (
          <motion.div
            key="history"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0, transition: { duration: 0.08 } }}
            className="ask-scroll mt-2 overflow-y-auto pr-1"
            style={{ maxHeight: scrollMax }}
          >
            <ChatHistory
              items={shownChats}
              filtered={historyFilter.length > 0}
              active={Math.min(pick, Math.max(shownChats.length - 1, 0))}
              onHover={setPick}
              onOpen={(id) => {
                setHistoryOpen(false);
                setText("");
                void openChat(id);
              }}
              onDelete={removeChat}
            />
          </motion.div>
        ) : showHits && hits ? (
          <motion.div
            key="hits"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0, transition: { duration: 0.08 } }}
            className="ask-scroll mt-2 overflow-y-auto pr-1"
            style={{ maxHeight: scrollMax }}
          >
            <Results
              query={hits.query}
              items={hits.items}
              active={Math.min(pick, hits.items.length - 1)}
              onHover={setPick}
            />
          </motion.div>
        ) : showChat ? (
          <motion.div
            key="chat"
            ref={chatRef}
            onScroll={onScroll}
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0, transition: { duration: 0.08 } }}
            className="ask-scroll mt-2 overflow-y-auto pr-1"
            style={{ maxHeight: scrollMax }}
          >
            <div>
              <Chat turns={turns} />
            </div>
          </motion.div>
        ) : (
          rows > 0 && (
            <motion.ul
              key="rows"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0, transition: { duration: 0.08 } }}
              className="mt-2 -mx-1.5"
            >
              {asking && (
                <Row active={active === 0} onHover={() => setSelected(0)} onClick={() => runRow(0)}>
                  <span className="grid size-6 place-items-center rounded-full bg-white text-[12px] font-semibold text-black">
                    ?
                  </span>
                  <span className="min-w-0 flex-1 truncate">
                    Ask Sidekick <span className="text-[rgb(235_235_245/0.6)]">{text.trim()}</span>
                  </span>
                  {active === 0 && <Kbd>Enter</Kbd>}
                </Row>
              )}
              {commands.map((c, i) => {
                const row = i + lead;
                return (
                  <Row key={c.id} active={active === row} onHover={() => setSelected(row)} onClick={() => runRow(row)}>
                    <span className="grid size-6 place-items-center rounded-full bg-white/12 text-white/85">
                      <Icon name={c.icon} size={13} />
                    </span>
                    <span className="flex-1 truncate">{c.label}</span>
                    {c.hint && <span className="text-[12px] text-[rgb(235_235_245/0.4)]">{c.hint}</span>}
                    {asking && active === row && <Kbd>Enter</Kbd>}
                  </Row>
                );
              })}
              {asking && (
                <Row
                  active={active === searchRow}
                  onHover={() => setSelected(searchRow)}
                  onClick={() => runRow(searchRow)}
                >
                  <span className="grid size-6 place-items-center rounded-full bg-white/[0.12] text-white/85">
                    <Icon name="ask" size={13} />
                  </span>
                  <span className="min-w-0 flex-1 truncate">
                    Search my stuff for <span className="text-[rgb(235_235_245/0.6)]">{text.trim()}</span>
                  </span>
                </Row>
              )}
              {asking && (
                <Row
                  active={active === skillRow}
                  onHover={() => setSelected(skillRow)}
                  onClick={() => runRow(skillRow)}
                >
                  <span className="grid size-6 place-items-center rounded-full bg-white/[0.12] text-white/85">
                    <Icon name="settings" size={13} />
                  </span>
                  <span className="min-w-0 flex-1 truncate">
                    Teach Sidekick a skill <span className="text-[rgb(235_235_245/0.6)]">{text.trim()}</span>
                  </span>
                </Row>
              )}
            </motion.ul>
          )
        )}
      </AnimatePresence>

      {handoffError && <p className="mt-1.5 text-[12px] text-[#ffb4ae]">{handoffError}</p>}
      {/* No model to answer: say so. Otherwise the key legend shows only while Alt is held. */}
      {!best ? (
        <div className="mt-2 flex items-center gap-2 text-[11px] text-[rgb(235_235_245/0.45)]">
          {providersData === null ? (
            <SetupSpinner className="text-white/50" />
          ) : (
            <span className="size-1.5 rounded-full bg-[#ffd60a]" aria-hidden="true" />
          )}
          <span className="truncate" aria-live="polite">
            {providersData === null
              ? "Checking AI…"
              : ask.localOnly
                ? "No local model running"
                : "No AI set up yet. See Settings > AI"}
          </span>
        </div>
      ) : (
        alt && (
          <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-[rgb(235_235_245/0.5)]">
            <span>
              <Kbd>Enter</Kbd> {asking ? "ask" : showClips ? "copy" : showHits ? "open" : "run"}
            </span>
            {agent && (asking || turns.length > 0) && (
              <span>
                <Kbd>Ctrl Enter</Kbd> continue in {agent}
              </span>
            )}
            <span>
              <Kbd>Esc</Kbd> {hearing !== null ? "stop mic" : inChat || streaming ? "new chat" : "close"}
            </span>
          </div>
        )
      )}
      <Timings />
    </div>
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
        className={`chip grid size-7 place-items-center rounded-full text-white/85 ${
          pressed ? "bg-white/[0.22]" : "bg-white/[0.1] hover:bg-white/[0.18]"
        }`}
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
