"use client";

// Ask mode (FR-UI-07, FR-AI-06): the island itself becomes the place to run a
// command or ask Sidekick, with the app you were in and the clipboard
// attachable on request. Rendered inside the island shell; the island owns
// the morph, this owns the content.

import { AnimatePresence, motion } from "motion/react";
import { type KeyboardEvent, type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { api } from "@/lib/bridge";
import { Markdown } from "@/lib/markdown";
import {
  cancelChat,
  newChat,
  sendChat,
  setAsk,
  startListening,
  startSkill,
  stopListening,
  updateSettings,
  useSidekick,
} from "@/lib/store";
import { isPaused, PROVIDER_LABELS, type ProviderStatus, type SearchHit, type Turn } from "@/lib/types";
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
  const [projects, setProjects] = useState<{ name: string; path: string }[]>([]);
  const inputRef = useRef<HTMLInputElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const seq = ask?.seq;

  // Every open: focus the input and refresh which AI is reachable.
  useEffect(() => {
    if (seq === undefined) return;
    setText(useSidekick.getState().ask?.prompt ?? "");
    setSelected(0);
    setClips(null);
    void api.aiStatus().then(setProviders);
    void api.projectsList().then(setProjects);
    const id = requestAnimationFrame(() => inputRef.current?.focus());
    return () => cancelAnimationFrame(id);
  }, [seq]);

  // Keep the newest text in view while an answer streams in.
  useEffect(() => {
    const el = scrollRef.current;
    if (el && turns.length) el.scrollTop = el.scrollHeight;
  }, [turns]);

  const paused = isPaused(settings.pause);
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
        hint: "Sends a screenshot to your AI",
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
    const q = text.trim().toLowerCase();
    if (!q) return all;
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
    return [...all.filter((c) => c.label.toLowerCase().includes(q)), ...launch];
  }, [paused, settings.muted, turns.length, text, projects]);

  if (!ask) return null;

  const streaming = chatId !== null;
  const asking = text.trim().length > 0;
  // With a conversation going, the body shows it; commands show only while typing.
  const showClips = clips !== null && !asking;
  const showHits = hits !== null && !asking && !showClips;
  const showChat = turns.length > 0 && !asking && !showHits && !showClips;
  // While typing, the first rows are "Ask", "Search" and "Teach a skill".
  const lead = asking ? 3 : 0;
  const rows =
    hearing !== null ? 0 : asking ? commands.length + lead : showChat || showHits || showClips ? 0 : commands.length;
  const best = providers.find((p) => p.available && (!ask.localOnly || p.local) && p.id !== "semif");

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

  const onKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown" && rows) {
      e.preventDefault();
      setSelected((s) => (s + 1) % rows);
    } else if (e.key === "ArrowUp" && rows) {
      e.preventDefault();
      setSelected((s) => (s - 1 + rows) % rows);
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (rows) runRow(selected);
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
              turns.length > 0 && <Pill onClick={newChat}>New</Pill>
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
            className="ask-scroll mt-2 max-h-[330px] overflow-y-auto pr-1"
          >
            <Clips items={clips} />
          </motion.div>
        ) : showHits && hits ? (
          <motion.div
            key="hits"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0, transition: { duration: 0.08 } }}
            className="ask-scroll mt-2 max-h-[330px] overflow-y-auto pr-1"
          >
            <Results query={hits.query} items={hits.items} />
          </motion.div>
        ) : showChat ? (
          <motion.div
            key="chat"
            ref={scrollRef}
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0, transition: { duration: 0.08 } }}
            className="ask-scroll mt-2 max-h-[330px] overflow-y-auto pr-1"
          >
            <Chat turns={turns} />
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
        <span className="truncate">
          {best
            ? `${PROVIDER_LABELS[best.id] ?? best.id}${best.local ? ", on this PC" : ""}`
            : ask.localOnly
              ? "No local model running"
              : "No AI set up yet. See Settings > AI"}
        </span>
        <span className="ml-auto flex shrink-0 items-center gap-2.5">
          <span>
            <Kbd>Enter</Kbd> {asking ? "ask" : "run"}
          </span>
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
        on={ask.attachScreen}
        onClick={() => setAsk({ attachScreen: !ask.attachScreen })}
        title={`Send a screenshot of ${context.app ?? "the screen"} with the next question`}
      >
        Screenshot
      </Chip>
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
function Hearing({ text }: { text: string }) {
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
          active ? "bg-white/[0.1] text-white" : "text-white/80"
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

/** Clipboard history: click to copy again (FR-CLIP-01). */
function Clips({ items }: { items: { text: string; ts: string }[] }) {
  if (items.length === 0)
    return <p className="py-2 text-[13px] text-[rgb(235_235_245/0.6)]">Nothing copied yet. Secrets are never kept.</p>;
  return (
    <ul className="-mx-1.5 py-1">
      {items.map((c) => (
        <li key={`${c.ts}${c.text.slice(0, 40)}`}>
          <button
            type="button"
            onClick={() => void api.clipboardCopy(c.text).then(() => api.askClose())}
            className="flex w-full items-center gap-2 rounded-[14px] px-1.5 py-1.5 text-left transition-colors duration-100 hover:bg-white/[0.1]"
          >
            <span className="line-clamp-2 min-w-0 flex-1 font-mono text-[12px] break-all text-white/85">{c.text}</span>
            <span className="shrink-0 text-[11px] text-[rgb(235_235_245/0.4)] tabular-nums">
              {new Date(c.ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
            </span>
          </button>
        </li>
      ))}
    </ul>
  );
}

function Results({ query, items }: { query: string; items: SearchHit[] }) {
  if (items.length === 0)
    return (
      <p className="py-2 text-[13px] text-[rgb(235_235_245/0.6)]">
        Nothing found for &quot;{query}&quot;. Add folders under Settings &gt; Search to find your files.
      </p>
    );
  return (
    <ul className="-mx-1.5 py-1">
      {items.map((h) => {
        const opens = ["file", "download", "screenshot", "page"].includes(h.source);
        return (
          <li key={`${h.source}|${h.reference}`}>
            <button
              type="button"
              disabled={!opens}
              onClick={() => void api.openReference(h.source, h.reference)}
              className="flex w-full flex-col gap-0.5 rounded-[14px] px-1.5 py-1.5 text-left transition-colors duration-100 enabled:hover:bg-white/[0.1]"
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
              {t.content ? <Markdown text={t.content} /> : t.streaming ? <Thinking /> : null}
              {t.error && (
                <p className="mt-1 rounded-xl bg-[#ff453a]/15 px-3 py-2 text-[12.5px] text-[#ffb4ae]">{t.error}</p>
              )}
              {skillMode && !t.streaming && yamlBlock(t.content) && <AddSkill yaml={yamlBlock(t.content) ?? ""} />}
              {!t.streaming && t.provider && (
                <p className="mt-0.5 text-[11px] text-[rgb(235_235_245/0.35)]">
                  {PROVIDER_LABELS[t.provider] ?? t.provider}
                </p>
              )}
            </div>
          )}
        </motion.div>
      ))}
    </div>
  );
}

function Thinking() {
  return (
    <span className="inline-flex gap-1 py-2" role="status" aria-label="Thinking">
      {[0, 1, 2].map((i) => (
        <motion.span
          key={i}
          className="size-1.5 rounded-full bg-white/50"
          animate={{ opacity: [0.25, 1, 0.25] }}
          transition={{ duration: 1.1, repeat: Number.POSITIVE_INFINITY, delay: i * 0.15 }}
        />
      ))}
    </span>
  );
}

function Kbd({ children }: { children: ReactNode }) {
  return (
    <kbd className="rounded-[5px] bg-white/[0.1] px-1.5 py-px font-sans text-[10px] text-[rgb(235_235_245/0.6)]">
      {children}
    </kbd>
  );
}
