"use client";

// Ask mode (FR-UI-07, FR-AI-06): the island itself becomes the place to run a
// command or ask Sidekick, with the app you were in and the clipboard
// attachable on request. Rendered inside the island shell; the island owns
// the morph, this owns the content.

import { AnimatePresence, motion } from "motion/react";
import {
  type KeyboardEvent,
  type ReactNode,
  type RefObject,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { api } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import { Markdown } from "@/lib/markdown";
import { splitOptions } from "@/lib/options";
import {
  cancelChat,
  newChat,
  openChat,
  runProposal,
  sendChat,
  setAsk,
  setAskModel,
  startListening,
  startSkill,
  stopListening,
  updateSettings,
  useSidekick,
} from "@/lib/store";
import { toolStatus } from "@/lib/tools";
import {
  type Agents,
  type AskContext,
  type CalendarToday,
  type ChatSummary,
  isPaused,
  PROVIDER_LABELS,
  type Proposal,
  type ProviderStatus,
  type SearchHit,
  type Turn,
} from "@/lib/types";
import { Icon, type IconName } from "./Icon";

interface Command {
  id: string;
  label: string;
  hint?: string;
  icon: IconName;
  run: () => void;
  /** Keep Ask mode open after running. */
  stay?: boolean;
}

const ease = [0.23, 1, 0.32, 1] as const;

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
  const [providers, setProviders] = useState<ProviderStatus[]>([]);
  const [hits, setHits] = useState<{ query: string; items: SearchHit[] } | null>(null);
  const [clips, setClips] = useState<{ text: string; ts: string }[] | null>(null);
  // The highlighted clip or search result, moved with the arrow keys.
  const [pick, setPick] = useState(0);
  // biome-ignore lint/correctness/useExhaustiveDependencies: a new list starts at its top
  useEffect(() => setPick(0), [clips, hits]);
  // Esc works wherever focus is in Ask (after clicking a button or chip);
  // the input handles it itself.
  const escRef = useRef<() => void>(() => undefined);
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented || e.target === inputRef.current) return;
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
    void api.aiStatus().then(setProviders);
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
  }, [seq]);

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
        run: () => void api.clipboardHistory().then(setClips),
        stay: true,
      },
      { id: "settings", label: "Open settings", icon: "settings", run: () => setAsk({ view: "settings" }), stay: true },
      ...(turns.length
        ? [
            {
              id: "new",
              label: "New chat",
              hint: "Clears this conversation",
              icon: "close" as const,
              run: newChat,
              stay: true,
            },
          ]
        : []),
    ];
    // Recent conversations to pick up again.
    const recent: Command[] = chats.slice(0, 20).map((c) => ({
      id: `chat:${c.id}`,
      label: c.title,
      hint: `Continue · ${ago(c.updated)}`,
      icon: "ask",
      run: () => void openChat(c.id),
      stay: true,
    }));
    const q = text.trim().toLowerCase();
    if (!q) return turns.length ? all : [...starters, ...all, ...recent.slice(0, 3)];
    // Typing a project's name offers to open it (FR-DEV-10).
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
    const pickUp = recent.filter((c) => c.label.toLowerCase().includes(q)).slice(0, 3);
    return [...all.filter((c) => c.label.toLowerCase().includes(q)), ...launch, ...pickUp];
  }, [paused, settings.muted, turns.length, text, projects, chats, starters]);

  if (!ask) return null;

  const streaming = chatId !== null;
  // While the clipboard history is open, typing filters it instead of asking.
  const clipFilter = clips !== null ? text.trim().toLowerCase() : "";
  const asking = text.trim().length > 0 && clips === null;
  const shownClips = clips?.filter((c) => !clipFilter || c.text.toLowerCase().includes(clipFilter)) ?? [];
  // With a conversation going, the body keeps showing it while you type
  // the next question; commands show only before the first one.
  const inChat = turns.length > 0;
  const showClips = clips !== null;
  const showHits = hits !== null && !asking && !showClips;
  const showChat = inChat && !showHits && !showClips;
  // While typing, the first rows are "Ask", "Search" and "Teach a skill".
  const lead = asking ? 3 : 0;
  const rows =
    hearing !== null || showChat || showHits || showClips ? 0 : asking ? commands.length + lead : commands.length;
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

  // Clipboard history and search results: arrows move, Enter uses it.
  const listLen = showClips ? shownClips.length : showHits && hits ? hits.items.length : 0;
  const openListItem = (i: number) => {
    if (showClips) {
      const c = shownClips[i];
      if (c) void api.clipboardCopy(c.text).then(() => api.askClose());
    } else if (showHits && hits) {
      const h = hits.items[i];
      if (h) void api.openReference(h.source, h.reference);
    }
  };

  escRef.current = () => {
    inputRef.current?.focus();
    if (hearing !== null) stopListening();
    else if (streaming) cancelChat();
    else void api.askClose();
  };

  const onKey = (e: KeyboardEvent<HTMLInputElement>) => {
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
      const messages = turns
        .filter((t) => !t.error && t.content.trim())
        .map(({ role, content }) => ({ role, content }));
      if (text.trim()) messages.push({ role: "user", content: text.trim() });
      if (messages.length === 0) return;
      setText("");
      void api.aiHandoff(messages, null).then(() => api.askClose());
    } else if (e.key === "Enter") {
      e.preventDefault();
      // In a conversation Enter sends the follow-up.
      if (asking && showChat) runRow(0);
      else if (rows) runRow(selected);
    } else if (e.key === "Escape") {
      e.preventDefault();
      if (hearing !== null) stopListening();
      else if (streaming) cancelChat();
      else if (text) setText("");
      else if (clips) setClips(null);
      else if (hits) setHits(null);
      else void api.askClose();
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
            placeholder={turns.length ? "Ask a follow-up" : "Ask Sidekick or type a command"}
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
            {streaming ? (
              <Pill onClick={cancelChat}>Stop</Pill>
            ) : (
              turns.length > 0 && (
                <Pill
                  onClick={() => {
                    newChat();
                    inputRef.current?.focus();
                  }}
                >
                  New
                </Pill>
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
                    <span className="grid size-6 place-items-center rounded-full bg-white/[0.12] text-white/85">
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

      <div className="mt-2 flex items-center gap-2 text-[11px] text-[rgb(235_235_245/0.45)]">
        <span className={`size-1.5 rounded-full ${best ? "bg-[#30d158]" : "bg-[#ffd60a]"}`} aria-hidden="true" />
        {best ? (
          <ModelPicker choices={choices} best={best} picked={pickedModel} />
        ) : (
          <span className="truncate">
            {ask.localOnly ? "No local model running" : "No AI set up yet. See Settings > AI"}
          </span>
        )}
        <span className="ml-auto flex shrink-0 items-center gap-2.5">
          <span>
            <Kbd>Enter</Kbd> {asking ? "ask" : showClips ? "copy" : showHits ? "open" : "run"}
          </span>
          {(asking || turns.length > 0) && (
            <span>
              <Kbd>Ctrl Enter</Kbd> {agent}
            </span>
          )}
          <span>
            <Kbd>Esc</Kbd> {streaming ? "stop" : "close"}
          </span>
        </span>
      </div>
    </div>
  );
}

function ContextChips() {
  const ask = useSidekick((s) => s.ask);
  if (!ask) return null;
  const { context } = ask;
  return (
    <div className="mt-2.5 flex flex-wrap items-center gap-1.5">
      {context.app && (
        <Chip on={ask.attachWindow} onClick={() => setAsk({ attachWindow: !ask.attachWindow })} title={context.title}>
          {context.app}
        </Chip>
      )}
      {context.clipboardSecret ? (
        <span className="rounded-full px-2.5 py-1 text-[11.5px] text-[rgb(235_235_245/0.35)]">
          Clipboard hidden (looks like a secret)
        </span>
      ) : (
        context.clipboardKind && (
          <Chip
            on={ask.attachClip}
            onClick={() => setAsk({ attachClip: !ask.attachClip })}
            title={context.clipboardPreview}
          >
            Clipboard: {context.clipboardKind.replace("_", " ")}
          </Chip>
        )
      )}
      <Chip
        on={ask.localOnly}
        onClick={() => setAsk({ localOnly: !ask.localOnly })}
        title="Only use a model on this PC"
      >
        This PC only
      </Chip>
    </div>
  );
}

function Chip({
  on,
  onClick,
  title,
  children,
}: {
  on: boolean;
  onClick: () => void;
  title?: string | null;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-pressed={on}
      title={title ?? undefined}
      onClick={onClick}
      className={`chip max-w-[200px] truncate rounded-full px-2.5 py-1 text-[11.5px] font-medium ${
        on ? "bg-white text-black" : "bg-white/[0.1] text-[rgb(235_235_245/0.7)] hover:bg-white/[0.16]"
      }`}
    >
      {children}
    </button>
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

function Pill({ onClick, children }: { onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="chip h-7 shrink-0 rounded-full bg-white/[0.12] px-3 text-[12px] font-medium text-white/85 hover:bg-white/[0.2]"
    >
      {children}
    </button>
  );
}

function Row({
  active,
  onHover,
  onClick,
  children,
}: {
  active: boolean;
  onHover: () => void;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <li>
      <button
        type="button"
        onMouseMove={onHover}
        onClick={onClick}
        className={`flex w-full items-center gap-2.5 rounded-[14px] px-1.5 py-1.5 text-left text-[13.5px] tracking-[-0.01em] transition-colors duration-100 ${
          active ? "bg-white/[0.14] text-white ring-1 ring-inset ring-white/25" : "text-white/80"
        }`}
      >
        {children}
      </button>
    </li>
  );
}

const SOURCE_LABELS: Record<string, string> = {
  file: "File",
  download: "Download",
  screenshot: "Screenshot",
  clipboard: "Copied",
  page: "Web page",
  claude: "Claude Code",
  action: "Action",
  chat: "Ask",
};

/** Matches come back between [ and ]; show them bold. */
function Snippet({ text }: { text: string }) {
  const parts = text.split(/(\[[^\]]*\])/g);
  return (
    <>
      {parts.map((p, i) =>
        p.startsWith("[") && p.endsWith("]") ? (
          // biome-ignore lint/suspicious/noArrayIndexKey: parts have no identity beyond order
          <strong key={i} className="font-semibold text-white">
            {p.slice(1, -1)}
          </strong>
        ) : (
          p
        ),
      )}
    </>
  );
}

/** Keeps the highlighted row of a list in view as the arrows move it. */
function scrollIfActive(active: boolean) {
  return (el: HTMLElement | null) => {
    if (active && el) el.scrollIntoView({ block: "nearest" });
  };
}

/** Clipboard history: arrows or the mouse pick, Enter or a click copies
 * again (FR-CLIP-01). Typing filters the list. */
function Clips({
  items,
  filtered,
  active,
  onHover,
}: {
  items: { text: string; ts: string }[];
  filtered: boolean;
  active: number;
  onHover: (i: number) => void;
}) {
  if (items.length === 0)
    return (
      <p className="py-2 text-[13px] text-[rgb(235_235_245/0.6)]">
        {filtered ? "No copied text matches." : "Nothing copied yet. Secrets are never kept."}
      </p>
    );
  return (
    <ul className="py-1" aria-label="Clipboard history">
      {items.map((c, i) => (
        <li key={`${c.ts}${c.text.slice(0, 40)}`} ref={scrollIfActive(i === active)}>
          <button
            type="button"
            aria-current={i === active}
            onMouseMove={() => onHover(i)}
            onClick={() => void api.clipboardCopy(c.text).then(() => api.askClose())}
            className={`flex w-full items-center gap-2 rounded-[14px] px-1.5 py-1.5 text-left transition-colors duration-100 ${
              i === active ? "bg-white/[0.14] ring-1 ring-inset ring-white/25" : "hover:bg-white/[0.08]"
            }`}
          >
            <span className="line-clamp-2 min-w-0 flex-1 font-mono text-[12px] break-all text-white/85">{c.text}</span>
            <span className="shrink-0 text-[11px] text-[rgb(235_235_245/0.4)] tabular-nums">
              {new Date(c.ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
            </span>
            {i === active && <Kbd>Enter</Kbd>}
          </button>
        </li>
      ))}
    </ul>
  );
}

function Results({
  query,
  items,
  active,
  onHover,
}: {
  query: string;
  items: SearchHit[];
  active: number;
  onHover: (i: number) => void;
}) {
  if (items.length === 0)
    return (
      <p className="py-2 text-[13px] text-[rgb(235_235_245/0.6)]">
        Nothing found for &quot;{query}&quot;. Add folders under Settings &gt; Search to find your files.
      </p>
    );
  return (
    <ul className="py-1">
      {items.map((h, i) => {
        const opens = ["file", "download", "screenshot", "page"].includes(h.source);
        return (
          <li key={`${h.source}|${h.reference}`} ref={scrollIfActive(i === active)}>
            <button
              type="button"
              disabled={!opens}
              aria-current={i === active}
              onMouseMove={() => onHover(i)}
              onClick={() => void api.openReference(h.source, h.reference)}
              className={`flex w-full flex-col gap-0.5 rounded-[14px] px-1.5 py-1.5 text-left transition-colors duration-100 ${
                i === active ? "bg-white/[0.14] ring-1 ring-inset ring-white/25" : "enabled:hover:bg-white/[0.08]"
              }`}
            >
              <span className="flex items-center gap-2 text-[13px]">
                <span className="rounded-full bg-white/[0.12] px-1.5 py-px text-[10.5px] text-white/70">
                  {SOURCE_LABELS[h.source] ?? h.source}
                </span>
                <span className="truncate text-white/90">{h.title || h.reference}</span>
              </span>
              <span className="line-clamp-2 text-[12px] text-[rgb(235_235_245/0.55)]">
                <Snippet text={h.snippet} />
              </span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}

/** The YAML block of an answer, if it has one. */
function yamlBlock(text: string): string | null {
  const m = text.match(/```ya?ml\s*\n([\s\S]*?)```/);
  return m ? m[1].trim() : null;
}

function AddSkill({ yaml }: { yaml: string }) {
  const [state, setState] = useState<{ ok: boolean; message: string } | null>(null);
  const install = async () => {
    try {
      const name = await api.skillInstall(yaml);
      setState({ ok: true, message: `Added "${name}". Manage it in Settings > Skills.` });
    } catch (err) {
      setState({ ok: false, message: String(err) });
    }
  };
  if (state)
    return <p className={`mt-1.5 text-[12.5px] ${state.ok ? "text-[#30d158]" : "text-[#ffb4ae]"}`}>{state.message}</p>;
  return (
    <button
      type="button"
      onClick={() => void install()}
      className="chip mt-1.5 h-8 rounded-full bg-white px-3.5 text-[13px] font-medium text-black hover:bg-white/90"
    >
      Add skill
    </button>
  );
}

function Chat({ turns }: { turns: Turn[] }) {
  const skillMode = useSidekick((s) => s.chatSkill);
  const last = turns.at(-1);
  const options = last?.role === "assistant" && !last.streaming ? splitOptions(last.content).options : [];
  return (
    <div className="space-y-2.5 py-1">
      {turns.map((t, i) => (
        <motion.div
          // biome-ignore lint/suspicious/noArrayIndexKey: turns only ever append
          key={i}
          initial={{ opacity: 0, y: 4 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.2, ease }}
          className={t.role === "user" ? "flex justify-end" : ""}
        >
          {t.role === "user" ? (
            <div className="max-w-[85%] rounded-[18px] rounded-br-md bg-white/[0.14] px-3 py-1.5 text-[13.5px] whitespace-pre-wrap">
              {t.content}
              {t.screen && (
                <span className="mt-0.5 block text-[11px] text-[rgb(235_235_245/0.5)]">with screenshot</span>
              )}
            </div>
          ) : (
            <div className="text-[13.5px] leading-relaxed text-white/90">
              {t.content ? (
                <Markdown text={splitOptions(t.content, t.streaming).body} />
              ) : t.streaming ? (
                <Thinking />
              ) : null}
              {(t.steps?.length ?? 0) > 1 ? (
                <Steps steps={t.steps ?? []} running={!!t.streaming} />
              ) : (
                t.streaming &&
                t.tool && <p className="mt-0.5 text-[11.5px] text-[rgb(235_235_245/0.5)]">{toolStatus(t.tool)}</p>
              )}
              {t.error && (
                <p className="mt-1 rounded-xl bg-[#ff453a]/15 px-3 py-2 text-[12.5px] text-[#ffb4ae]">{t.error}</p>
              )}
              {skillMode && !t.streaming && yamlBlock(t.content) && <AddSkill yaml={yamlBlock(t.content) ?? ""} />}
              {!t.streaming && t.provider && (
                <p className="mt-0.5 text-[11px] text-[rgb(235_235_245/0.35)]">
                  {PROVIDER_LABELS[t.provider] ?? t.provider}
                </p>
              )}
              {t.proposals && t.proposals.length > 0 && <Proposals items={t.proposals} keys={i === turns.length - 1} />}
              {i === turns.length - 1 && options.length > 0 && (
                <AnswerOptions options={options} start={pendingCount(t.proposals)} />
              )}
              {/* Offered when the local model gives up; Ctrl Enter works any time. */}
              {!t.streaming && i === turns.length - 1 && (t.handoff || t.error) && (
                <Handoff turns={turns} reason={t.handoff ?? null} />
              )}
            </div>
          )}
        </motion.div>
      ))}
    </div>
  );
}

/** A meeting starting within the hour, from today's "HH:MM" list. */
function soonestMeeting(calendar: CalendarToday | null): { title: string; start: string } | null {
  const now = new Date();
  for (const m of calendar?.meetings ?? []) {
    const [h, min] = m.start.split(":").map(Number);
    const at = new Date(now);
    at.setHours(h ?? 0, min ?? 0, 0, 0);
    const mins = (at.getTime() - now.getTime()) / 60_000;
    if (mins >= -5 && mins <= 60) return m;
  }
  return null;
}

/**
 * What Ask offers before you type, from what you are doing: the error you
 * copied, the page you are on, the meeting coming up. Enter runs the first.
 */
function contextStarters({
  context,
  page,
  meeting,
  focusInput,
}: {
  context: AskContext | null;
  page: string | null;
  meeting: { title: string; start: string } | null;
  focusInput: (prefix: string) => void;
}): Command[] {
  const out: Command[] = [];
  const clip = context?.clipboardSecret ? null : context?.clipboardKind;
  if (clip === "stack_trace") {
    out.push({
      id: "starter:error",
      label: "Explain the error I copied",
      hint: "Two ways to fix it",
      icon: "ask",
      run: () => sendChat("Explain the error I copied and give me the two most likely fixes.", { clipboard: true }),
      stay: true,
    });
  }
  if (page) {
    out.push({
      id: "starter:page",
      label: "Summarize this page",
      hint: "In three lines",
      icon: "ask",
      run: () => sendChat("Summarize this page in three short lines."),
      stay: true,
    });
  }
  if (meeting) {
    out.push({
      id: "starter:meeting",
      label: `Prepare for ${meeting.title}`,
      hint: `At ${meeting.start}`,
      icon: "ask",
      run: () =>
        sendChat(`Help me prepare for "${meeting.title}" at ${meeting.start}: related notes, files and open items.`),
      stay: true,
    });
  }
  out.push(
    {
      id: "starter:find",
      label: "Find a file...",
      hint: "Searches your PC",
      icon: "folder",
      run: () => focusInput("Find "),
      stay: true,
    },
    {
      id: "starter:today",
      label: "What did I work on today?",
      icon: "ask",
      run: () => sendChat("What did I work on today? Two lines."),
      stay: true,
    },
  );
  return out.slice(0, 3);
}

/** The steps of a multi-step task: done ones ticked, the current one live.
 * Folds to one line once the answer is in. */
function Steps({ steps, running }: { steps: string[]; running: boolean }) {
  const [open, setOpen] = useState(false);
  if (!running && !open) {
    return (
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="chip mt-1 text-[11.5px] text-[rgb(235_235_245/0.45)] hover:text-white"
      >
        {steps.length} steps
      </button>
    );
  }
  // Steps only grow, in order, so their position is a stable id.
  const keyed = steps.map((s, n) => ({ s, id: `${n}:${s}` }));
  return (
    <ol className="mt-1 flex flex-col gap-0.5 text-[11.5px]">
      {keyed.map(({ s, id }, i) => {
        const live = running && i === steps.length - 1;
        return (
          <li
            key={id}
            className={`flex items-center gap-1.5 ${live ? "text-white/80" : "text-[rgb(235_235_245/0.45)]"}`}
          >
            <span
              className={`grid size-3 shrink-0 place-items-center rounded-full text-[8px] ${
                live ? "animate-pulse bg-[#0a84ff]/60" : "bg-[#30d158]/70 text-black"
              }`}
            >
              {live ? "" : "✓"}
            </span>
            {toolStatus(s).replace(/\.\.\.$/, "")}
          </li>
        );
      })}
    </ol>
  );
}

/** Buttons still waiting for a tap; they take Alt 1, Alt 2... first. */
function pendingCount(items: Proposal[] | undefined): number {
  return (items ?? []).filter((p) => !p.ran).length;
}

/** Alt + a digit, from the island's own keys (it has focus in Ask). */
function useAltDigits(count: number, start: number, run: (n: number) => void) {
  const runRef = useRef(run);
  runRef.current = run;
  useEffect(() => {
    if (count === 0) return;
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (!e.altKey || e.ctrlKey || e.metaKey) return;
      const n = Number(e.key) - start;
      if (Number.isInteger(n) && n >= 1 && n <= count) {
        e.preventDefault();
        runRef.current(n - 1);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [count, start]);
}

/** Actions the answer offers: nothing runs until a tap, and Undo follows. */
function Proposals({ items, keys }: { items: Proposal[]; keys: boolean }) {
  const pending = items.filter((p) => !p.ran);
  useAltDigits(keys ? Math.min(pending.length, 9) : 0, 0, (n) => void runProposal(pending[n].id));
  // Alt U undoes the newest action that can be undone.
  const undoable = [...items].reverse().find((p) => p.ran?.ok && p.ran.undoId != null && !p.ran.undone);
  const undoRef = useRef<HTMLButtonElement | null>(null);
  useEffect(() => {
    if (!keys || !undoable) return;
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.altKey && !e.ctrlKey && e.key.toLowerCase() === "u") {
        e.preventDefault();
        undoRef.current?.click();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [keys, undoable]);
  return (
    <div className="mt-2 flex flex-col gap-1.5">
      {items.map((p) =>
        p.ran ? (
          <div key={p.id} className="flex items-center gap-2 text-[12.5px]">
            <span className={`size-1.5 shrink-0 rounded-full ${p.ran.ok ? "bg-[#30d158]" : "bg-[#ff453a]"}`} />
            <span className="min-w-0 flex-1 truncate text-[rgb(235_235_245/0.75)]">{p.ran.message}</span>
            {p.ran.ok && p.ran.undoId != null && !p.ran.undone && (
              <UndoProposal proposal={p} buttonRef={p === undoable && keys ? undoRef : undefined} />
            )}
            {p.ran.ok && p.ran.path && (
              <button
                type="button"
                onClick={() => void api.revealPath(p.ran?.path ?? "")}
                className="chip shrink-0 rounded-full bg-white/[0.12] px-2.5 py-1 text-[12px] text-white/90 hover:bg-white/[0.2]"
              >
                Show
              </button>
            )}
          </div>
        ) : (
          <button
            key={p.id}
            type="button"
            onClick={() => void runProposal(p.id)}
            className="chip flex min-h-8 items-center gap-2 self-start rounded-full bg-white px-3.5 py-1.5 text-[13px] font-medium text-black hover:bg-white/90"
          >
            {p.label}
            {keys && pending.indexOf(p) < 9 && (
              <kbd className="shrink-0 font-sans text-[11px] text-black/40">Alt {pending.indexOf(p) + 1}</kbd>
            )}
          </button>
        ),
      )}
    </div>
  );
}

function UndoProposal({
  proposal,
  buttonRef,
}: {
  proposal: Proposal;
  buttonRef?: RefObject<HTMLButtonElement | null>;
}) {
  const [state, setState] = useState<string | null>(null);
  if (state) return <span className="shrink-0 text-[12px] text-[rgb(235_235_245/0.55)]">{state}</span>;
  return (
    <button
      ref={buttonRef}
      type="button"
      onClick={() =>
        void api
          .actionUndo(proposal.ran?.undoId ?? 0)
          .then((m) => setState(m))
          .catch((e) => setState(String(e)))
      }
      className="chip shrink-0 rounded-full bg-white/[0.12] px-2.5 py-1 text-[12px] text-white/90 hover:bg-white/[0.2]"
    >
      Undo
      {buttonRef && <kbd className="ml-1.5 font-sans text-[11px] text-white/35">Alt U</kbd>}
    </button>
  );
}

/** Next steps the answer offers: click one or press its Alt number to ask it. */
function AnswerOptions({ options, start }: { options: string[]; start: number }) {
  const shown = Math.max(0, Math.min(options.length, 9 - start));
  useAltDigits(shown, start, (n) => sendChat(options[n]));
  return (
    <div className="mt-2 flex flex-wrap gap-1.5">
      {options.map((o, i) => (
        <button
          key={o}
          type="button"
          onClick={() => sendChat(o)}
          className={`chip flex min-h-8 max-w-full items-center gap-2 rounded-full px-3 py-1.5 text-left text-[13px] font-medium ${
            i === 0 ? "bg-white text-black hover:bg-white/90" : "bg-white/[0.12] text-white hover:bg-white/[0.2]"
          }`}
        >
          <span className="leading-snug">{o}</span>
          <kbd className={`shrink-0 font-sans text-[11px] ${i === 0 ? "text-black/40" : "text-white/35"}`}>
            {i < shown ? `Alt ${start + i + 1}` : ""}
          </kbd>
        </button>
      ))}
    </div>
  );
}

/** The model that answers in Ask mode: Auto (Sidekick picks) or one of
 * those that can answer now. Kept for next time. */
function ModelPicker({
  choices,
  best,
  picked,
}: {
  choices: ProviderStatus[];
  best: ProviderStatus;
  picked: ProviderStatus | null;
}) {
  const [open, setOpen] = useState(false);
  const listRef = useRef<HTMLDivElement>(null);
  const label = (p: ProviderStatus) => `${PROVIDER_LABELS[p.id] ?? p.id}${p.local ? ", on this PC" : ""}`;
  const items: { id: string | null; text: string }[] = [
    { id: null, text: `Auto (${PROVIDER_LABELS[choices[0]?.id] ?? "best"})` },
    ...choices.map((p) => ({ id: p.id, text: label(p) })),
  ];
  useEffect(() => {
    if (open) listRef.current?.querySelector<HTMLButtonElement>("[aria-checked=true]")?.focus();
  }, [open]);
  const choose = (id: string | null) => {
    setAskModel(id);
    setOpen(false);
  };
  const onListKey = (e: KeyboardEvent<HTMLDivElement>) => {
    const buttons = [...(listRef.current?.querySelectorAll<HTMLButtonElement>("button") ?? [])];
    const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      buttons[(at + (e.key === "ArrowDown" ? 1 : -1) + buttons.length) % buttons.length]?.focus();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      setOpen(false);
    }
  };
  return (
    <span className="relative min-w-0">
      <button
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        title="Change the model (Alt M)"
        onClick={() => setOpen((o) => !o)}
        className="flex max-w-full items-center gap-1 truncate rounded-md px-1 py-0.5 hover:bg-white/[0.08] hover:text-white/80"
      >
        <span className="truncate">{picked ? label(picked) : `Auto: ${label(best)}`}</span>
        <span aria-hidden="true">▾</span>
      </button>
      {open && (
        <div
          ref={listRef}
          role="menu"
          aria-label="Model"
          onKeyDown={onListKey}
          className="absolute bottom-full left-0 z-20 mb-1.5 flex min-w-52 flex-col gap-0.5 rounded-xl bg-[#1c1c1e] p-1 text-[12.5px] shadow-xl ring-1 ring-white/10"
        >
          {items.map((it) => {
            const on = (picked?.id ?? null) === it.id;
            return (
              <button
                key={it.id ?? "auto"}
                type="button"
                role="menuitemradio"
                aria-checked={on}
                onClick={() => choose(it.id)}
                className={`flex w-full items-center justify-between gap-3 rounded-lg px-2.5 py-1.5 text-left ${
                  on ? "bg-white/[0.12] text-white" : "text-white/75 hover:bg-white/[0.08]"
                }`}
              >
                {it.text}
                {on && <span aria-hidden="true">✓</span>}
              </button>
            );
          })}
        </div>
      )}
    </span>
  );
}

/** The coding agent that gets handoffs: Claude Code or Codex. */
function useAgentName(): string {
  const { data } = useCached<Agents>("agents", api.agentsStatus);
  return data?.handoff ?? "Claude Code";
}

/** Continue this conversation in the coding agent, which can make changes. */
function Handoff({ turns, reason }: { turns: Turn[]; reason: string | null }) {
  const [state, setState] = useState<string>("idle");
  const agent = useAgentName();
  const go = () => {
    setState("opening");
    const messages = turns.filter((t) => !t.error && t.content.trim()).map(({ role, content }) => ({ role, content }));
    api
      .aiHandoff(messages, reason)
      .then(() => setState("opened"))
      .catch((e) => setState(String(e)));
  };
  if (state === "opened") {
    return <p className="mt-1.5 text-[12px] text-[rgb(235_235_245/0.55)]">Opened in {agent} with this conversation.</p>;
  }
  return (
    <div className="mt-1.5 flex flex-col gap-1">
      {reason && <p className="text-[12px] text-[rgb(235_235_245/0.6)]">Too much for the local model: {reason}.</p>}
      <button
        type="button"
        disabled={state === "opening"}
        onClick={go}
        className={`chip h-8 self-start rounded-full px-3.5 text-[13px] font-medium disabled:opacity-50 ${
          reason ? "bg-white text-black hover:bg-white/90" : "bg-white/[0.12] text-white/90 hover:bg-white/[0.2]"
        }`}
      >
        {state === "opening" ? "Opening..." : `Continue in ${agent}`}
      </button>
      {state !== "idle" && state !== "opening" && <p className="text-[12px] text-[#ffb4ae]">{state}</p>}
    </div>
  );
}

function Thinking() {
  return (
    <span className="inline-flex gap-1 py-2" role="status" aria-label="Thinking">
      <span className="thinking-dot" />
      <span className="thinking-dot" />
      <span className="thinking-dot" />
    </span>
  );
}

/** "5 min ago", "yesterday", or a date. */
function ago(iso: string): string {
  const mins = Math.round((Date.now() - Date.parse(iso)) / 60_000);
  if (!Number.isFinite(mins)) return "";
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins} min ago`;
  if (mins < 24 * 60) return `${Math.round(mins / 60)} h ago`;
  if (mins < 48 * 60) return "yesterday";
  return new Date(iso).toLocaleDateString();
}

function Kbd({ children }: { children: ReactNode }) {
  return (
    <kbd className="rounded-[5px] bg-white/[0.1] px-1.5 py-px font-sans text-[10px] text-[rgb(235_235_245/0.6)]">
      {children}
    </kbd>
  );
}
