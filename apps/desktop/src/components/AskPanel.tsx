"use client";

// Ask mode: the island itself becomes the place to run a
// command or ask Sidekick, with the app you were in and the clipboard
// attachable on request. Rendered inside the island shell; the island owns
// the morph, this owns the content.

import { AnimatePresence, motion } from "motion/react";
import { type KeyboardEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
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
import { Kbd, Pill, Row } from "./ask/parts";
import type { Command } from "./ask/Starters";
import { ContextChips, contextStarters, soonestMeeting } from "./ask/Starters";
import { Icon } from "./Icon";
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
    return [...all.filter((c) => c.label.toLowerCase().includes(q)), ...launch];
  }, [paused, settings.muted, turns.length, text, projects, starters, resetChat]);

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
  // While typing, the first rows are "Ask", "Search" and "Teach a skill".
  const lead = asking ? 3 : 0;
  const rows =
    hearing !== null || showChat || showHits || showClips || showHistory
      ? 0
      : asking
        ? commands.length + lead
        : commands.length;
  // Models that can answer now; the picked one (if still there) goes first.
  const choices = providers.filter((p) => p.available && (!ask.localOnly || p.local) && p.id !== "semif");
  const pickedModel = choices.find((p) => p.id === askModel) ?? null;
  const best = pickedModel ?? choices[0];

  const runRow = (i: number) => {
    if (asking && i === 0) {
      sendChat(text);
      setText("");
      return;
    }
    if (asking && i === 1) {
      const query = text.trim();
      void api.search(query).then((items) => setHits({ query, items }));
      setText("");
      return;
    }
    if (asking && i === 2) {
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
    if (e.altKey && !e.ctrlKey && e.key.toLowerCase() === "h" && !streaming) {
      // Alt H: chat history.
      e.preventDefault();
      openHistory();
      return;
    }
    if (e.altKey && e.key.toLowerCase() === "m" && choices.length > 1) {
      // Alt M: next model (Auto, then each one that can answer).
      e.preventDefault();
      const ids = [null, ...choices.map((c) => c.id)];
      setAskModel(ids[(ids.indexOf(pickedModel?.id ?? null) + 1) % ids.length]);
      return;
    }
    if (listLen && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
      e.preventDefault();
      const step = e.key === "ArrowDown" ? 1 : -1;
      setPick((p) => (Math.min(p, listLen - 1) + step + listLen) % listLen);
    } else if (listLen && e.key === "Enter" && !e.ctrlKey && !e.metaKey) {
      e.preventDefault();
      openListItem(Math.min(pick, listLen - 1));
    } else if (e.key === "ArrowDown" && rows) {
      e.preventDefault();
      setSelected((s) => (s + 1) % rows);
    } else if (e.key === "ArrowUp" && rows) {
      e.preventDefault();
      setSelected((s) => (s - 1 + rows) % rows);
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
      else if (rows) runRow(selected);
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
              setSelected(0);
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
            {voiceReady && !streaming && (
              <button
                type="button"
                aria-label="Talk (or say Hey Sidekick)"
                title="Talk (or say Hey Sidekick)"
                onClick={startListening}
                className="chip grid size-7 shrink-0 place-items-center rounded-full bg-white/[0.12] text-white/85 hover:bg-white/[0.2]"
              >
                <Icon name="mic" size={14} />
              </button>
            )}
            {!streaming && (
              <button
                type="button"
                aria-label="Chat history"
                title="Chat history (Alt H)"
                aria-pressed={historyOpen}
                onClick={openHistory}
                className={`chip grid size-7 shrink-0 place-items-center rounded-full text-white/85 ${
                  historyOpen ? "bg-white/[0.22]" : "bg-white/[0.12] hover:bg-white/[0.2]"
                }`}
              >
                <Icon name="history" size={14} />
              </button>
            )}
            {streaming ? (
              <Pill onClick={cancelChat}>Stop</Pill>
            ) : (
              turns.length > 0 && (
                <button
                  type="button"
                  aria-label="New chat"
                  title="New chat (Esc)"
                  onClick={resetChat}
                  className="chip grid size-7 shrink-0 place-items-center rounded-full bg-white/[0.12] text-white/85 hover:bg-white/[0.2]"
                >
                  <Icon name="plus" size={16} />
                </button>
              )
            )}
          </>
        )}
      </div>

      <ContextChips />

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
                <Row active={selected === 0} onHover={() => setSelected(0)} onClick={() => runRow(0)}>
                  <span className="grid size-6 place-items-center rounded-full bg-white text-[12px] font-semibold text-black">
                    ?
                  </span>
                  <span className="min-w-0 flex-1 truncate">
                    Ask <span className="text-[rgb(235_235_245/0.6)]">{text.trim()}</span>
                  </span>
                  <Kbd>Enter</Kbd>
                </Row>
              )}
              {asking && (
                <Row active={selected === 1} onHover={() => setSelected(1)} onClick={() => runRow(1)}>
                  <span className="grid size-6 place-items-center rounded-full bg-white/[0.12] text-white/85">
                    <Icon name="ask" size={13} />
                  </span>
                  <span className="min-w-0 flex-1 truncate">
                    Search my stuff for <span className="text-[rgb(235_235_245/0.6)]">{text.trim()}</span>
                  </span>
                </Row>
              )}
              {asking && (
                <Row active={selected === 2} onHover={() => setSelected(2)} onClick={() => runRow(2)}>
                  <span className="grid size-6 place-items-center rounded-full bg-white/[0.12] text-white/85">
                    <Icon name="settings" size={13} />
                  </span>
                  <span className="min-w-0 flex-1 truncate">
                    Teach Sidekick a skill <span className="text-[rgb(235_235_245/0.6)]">{text.trim()}</span>
                  </span>
                </Row>
              )}
              {commands.map((c, i) => {
                const row = i + lead;
                return (
                  <Row
                    key={c.id}
                    active={selected === row}
                    onHover={() => setSelected(row)}
                    onClick={() => runRow(row)}
                  >
                    <span className="grid size-6 place-items-center rounded-full bg-white/12 text-white/85">
                      <Icon name={c.icon} size={13} />
                    </span>
                    <span className="flex-1 truncate">{c.label}</span>
                    {c.hint && <span className="text-[12px] text-[rgb(235_235_245/0.4)]">{c.hint}</span>}
                  </Row>
                );
              })}
            </motion.ul>
          )
        )}
      </AnimatePresence>

      {handoffError && <p className="mt-1.5 text-[12px] text-[#ffb4ae]">{handoffError}</p>}
      <div className="mt-2 flex items-center gap-2 text-[11px] text-[rgb(235_235_245/0.45)]">
        {providersData === null && !best ? (
          <SetupSpinner className="text-white/50" />
        ) : (
          <span className={`size-1.5 rounded-full ${best ? "bg-[#30d158]" : "bg-[#ffd60a]"}`} aria-hidden="true" />
        )}
        {best ? (
          <ModelPicker choices={choices} best={best} picked={pickedModel} />
        ) : providersData === null ? (
          <span className="truncate" aria-live="polite">
            Checking AI…
          </span>
        ) : (
          <span className="truncate">
            {ask.localOnly ? "No local model running" : "No AI set up yet. See Settings > AI"}
          </span>
        )}
        <span className="ml-auto flex shrink-0 items-center gap-2.5">
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
        </span>
      </div>
    </div>
  );
}

/** Live transcript while listening, with a breathing level bar. */
export function Hearing({ text }: { text: string }) {
  return (
    <div className="flex min-w-0 flex-1 items-center gap-2.5" aria-live="polite">
      <span className="flex h-4 items-center gap-[3px]" role="img" aria-label="Listening">
        {[0, 1, 2, 3].map((i) => (
          <motion.span
            key={i}
            className="w-[3px] rounded-full bg-[#30d158]"
            animate={{ height: [4, 14, 4] }}
            transition={{ duration: 0.8, repeat: Number.POSITIVE_INFINITY, delay: i * 0.12, ease: "easeInOut" }}
          />
        ))}
      </span>
      <span
        className={`min-w-0 flex-1 truncate font-display text-[17px] tracking-[-0.015em] ${
          text ? "text-white" : "text-[rgb(235_235_245/0.4)]"
        }`}
      >
        {text || "Listening..."}
      </span>
    </div>
  );
}
