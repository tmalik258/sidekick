"use client";

// The command palette (FR-UI-07): run a command, or ask Sidekick with the
// current app and clipboard attached on request (FR-AI-06). Opens with the
// global shortcut (Alt+Space by default) and hides when it loses focus.

import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { type KeyboardEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, EVENTS, listen } from "@/lib/bridge";
import { Markdown } from "@/lib/markdown";
import { connect, updateSettings, useSidekick } from "@/lib/store";
import {
  type ChatMessage,
  isPaused,
  type PaletteContext,
  type PaletteOpen,
  PROVIDER_LABELS,
  type ProviderStatus,
} from "@/lib/types";
import { Icon, type IconName } from "./Icon";
import { Orb } from "./Orb";

interface Command {
  id: string;
  label: string;
  hint?: string;
  icon: IconName;
  run: () => void | Promise<void>;
}

interface Turn extends ChatMessage {
  provider?: string | null;
  error?: string | null;
  streaming?: boolean;
}

const EMPTY_CONTEXT: PaletteContext = {
  app: null,
  title: null,
  clipboardKind: null,
  clipboardPreview: null,
  clipboardSecret: false,
};

const ease = [0.23, 1, 0.32, 1] as const;

export function Palette() {
  const { settings, ready } = useSidekick();
  const reduced = useReducedMotion() ?? false;
  const [text, setText] = useState("");
  const [selected, setSelected] = useState(0);
  const [turns, setTurns] = useState<Turn[]>([]);
  const [chatId, setChatId] = useState<string | null>(null);
  const [context, setContext] = useState<PaletteContext>(EMPTY_CONTEXT);
  const [attachWindow, setAttachWindow] = useState(false);
  const [attachClip, setAttachClip] = useState(false);
  const [localOnly, setLocalOnly] = useState(false);
  const [providers, setProviders] = useState<ProviderStatus[]>([]);
  const [openCount, setOpenCount] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const backdropRef = useRef<HTMLDivElement>(null);
  const turnsRef = useRef<Turn[]>([]);
  turnsRef.current = turns;

  useEffect(() => connect({ sounds: false }), []);

  const streaming = chatId !== null;

  const send = useCallback(
    (prompt: string, opts?: { clipboard?: boolean }) => {
      const q = prompt.trim();
      if (!q || chatId) return;
      const id = crypto.randomUUID();
      const history: ChatMessage[] = [
        ...turnsRef.current.filter((t) => !t.error).map(({ role, content }) => ({ role, content })),
        { role: "user", content: q },
      ];
      setTurns((t) => [...t, { role: "user", content: q }, { role: "assistant", content: "", streaming: true }]);
      setChatId(id);
      setText("");
      void api.aiChat(id, history, { window: attachWindow, clipboard: opts?.clipboard ?? attachClip }, localOnly);
    },
    [chatId, attachWindow, attachClip, localOnly],
  );

  // Stream answers into the last turn.
  useEffect(() => {
    const offs = [
      listen(EVENTS.aiDelta, ({ id, text }) => {
        if (id !== chatId) return;
        setTurns((t) => {
          const last = t[t.length - 1];
          if (last?.role !== "assistant") return t;
          return [...t.slice(0, -1), { ...last, content: last.content + text }];
        });
      }),
      listen(EVENTS.aiDone, ({ id, provider, error }) => {
        if (id !== chatId) return;
        setChatId(null);
        setTurns((t) => {
          const last = t[t.length - 1];
          if (last?.role !== "assistant") return t;
          return [...t.slice(0, -1), { ...last, provider, error, streaming: false }];
        });
      }),
    ];
    return () => {
      for (const o of offs) void o.then((f) => f());
    };
  }, [chatId]);

  // Each open: fresh context, focus, and an optional prompt to ask at once.
  useEffect(() => {
    const off = listen(EVENTS.paletteOpen, (open: PaletteOpen) => {
      setContext(open.context);
      setAttachClip(false);
      setAttachWindow(false);
      setOpenCount((n) => n + 1);
      if (open.prompt) {
        if (open.ask) {
          setAttachClip(!open.context.clipboardSecret);
          send(open.prompt, { clipboard: !open.context.clipboardSecret });
        } else setText(open.prompt);
      }
      void api.aiStatus().then(setProviders);
      requestAnimationFrame(() => inputRef.current?.focus());
    });
    return () => void off.then((f) => f());
  }, [send]);

  useEffect(() => {
    void api.aiStatus().then(setProviders);
    inputRef.current?.focus();
  }, []);

  // The window is larger than the panel; a click on the clear part closes it.
  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (e.target === backdropRef.current) void api.paletteHide();
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, []);

  // Keep the newest text in view while streaming.
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
      { id: "settings", label: "Open settings", icon: "settings", run: () => void api.openSettings() },
      ...(turns.length
        ? [
            {
              id: "new",
              label: "New chat",
              hint: "Clears this conversation",
              icon: "close" as const,
              run: () => setTurns([]),
            },
          ]
        : []),
    ];
    const q = text.trim().toLowerCase();
    return q ? all.filter((c) => c.label.toLowerCase().includes(q)) : all;
  }, [paused, settings.muted, turns.length, text]);

  // Row 0 is "Ask" whenever there is text; commands follow.
  const asking = text.trim().length > 0;
  const rows = asking ? commands.length + 1 : commands.length;

  const runRow = (i: number) => {
    if (asking && i === 0) return send(text);
    const cmd = commands[asking ? i - 1 : i];
    if (!cmd) return;
    void cmd.run();
    if (cmd.id !== "new") void api.paletteHide();
    setText("");
  };

  const onKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelected((s) => (rows ? (s + 1) % rows : 0));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelected((s) => (rows ? (s - 1 + rows) % rows : 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      runRow(selected);
    } else if (e.key === "Escape") {
      e.preventDefault();
      if (chatId) void api.aiCancel(chatId);
      else if (text) setText("");
      else void api.paletteHide();
    }
  };

  if (!ready) return null;

  const showChat = turns.length > 0 && !asking;
  const best = providers.find((p) => p.available && (!localOnly || p.local) && p.id !== "semif");

  return (
    <div ref={backdropRef} className="flex h-screen w-screen items-start justify-center p-3">
      <motion.div
        key={openCount}
        initial={reduced ? false : { opacity: 0, scale: 0.97, y: -6 }}
        animate={{ opacity: 1, scale: 1, y: 0 }}
        transition={{ duration: 0.22, ease }}
        className="palette-shell flex max-h-full w-full flex-col overflow-hidden text-white"
      >
        <div className="flex items-center gap-3 px-4 pt-3.5 pb-3">
          <Orb state={streaming ? "working" : "idle"} size={26} theme={settings.theme} magnetic={false} />
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
            className="min-w-0 flex-1 bg-transparent font-display text-[19px] tracking-[-0.01em] text-white outline-none placeholder:text-white/35"
          />
          {streaming && (
            <button
              type="button"
              onClick={() => chatId && void api.aiCancel(chatId)}
              className="rounded-full bg-white/10 px-2.5 py-1 text-[12px] text-white/75 transition-colors duration-150 hover:bg-white/16"
            >
              Stop
            </button>
          )}
        </div>

        <ContextChips
          context={context}
          attachWindow={attachWindow}
          attachClip={attachClip}
          localOnly={localOnly}
          onWindow={() => setAttachWindow((v) => !v)}
          onClip={() => setAttachClip((v) => !v)}
          onLocal={() => setLocalOnly((v) => !v)}
        />

        <div className="h-px bg-white/8" />

        <div ref={scrollRef} className="palette-scroll min-h-0 flex-1 overflow-y-auto px-2 py-2">
          {showChat ? (
            <Chat turns={turns} />
          ) : (
            <ul>
              {asking && (
                <Row active={selected === 0} onHover={() => setSelected(0)} onClick={() => runRow(0)}>
                  <span className="grid size-7 place-items-center rounded-lg bg-(--accent)/25 text-[13px]">?</span>
                  <span className="min-w-0 flex-1 truncate">
                    Ask <span className="text-white/55">"{text.trim()}"</span>
                  </span>
                  <Kbd>Enter</Kbd>
                </Row>
              )}
              {commands.map((c, i) => {
                const row = asking ? i + 1 : i;
                return (
                  <Row
                    key={c.id}
                    active={selected === row}
                    onHover={() => setSelected(row)}
                    onClick={() => runRow(row)}
                  >
                    <span className="grid size-7 place-items-center rounded-lg bg-white/8 text-white/80">
                      <Icon name={c.icon} size={15} />
                    </span>
                    <span className="flex-1 truncate">{c.label}</span>
                    {c.hint && <span className="text-[12px] text-white/40">{c.hint}</span>}
                  </Row>
                );
              })}
            </ul>
          )}
        </div>

        <div className="flex items-center gap-2 border-t border-white/8 px-4 py-2 text-[11.5px] text-white/45">
          <span className={`size-1.5 rounded-full ${best ? "bg-emerald-400" : "bg-amber-400"}`} aria-hidden="true" />
          <span className="truncate">
            {best
              ? `${PROVIDER_LABELS[best.id] ?? best.id}${best.local ? ", on this PC" : ""}`
              : localOnly
                ? "No local model running"
                : "No AI set up. See Settings > AI"}
          </span>
          <span className="ml-auto flex items-center gap-3">
            <span>
              <Kbd>Enter</Kbd> {asking ? "ask" : "run"}
            </span>
            <span>
              <Kbd>Esc</Kbd> {streaming ? "stop" : "close"}
            </span>
          </span>
        </div>
      </motion.div>
    </div>
  );
}

function ContextChips(props: {
  context: PaletteContext;
  attachWindow: boolean;
  attachClip: boolean;
  localOnly: boolean;
  onWindow: () => void;
  onClip: () => void;
  onLocal: () => void;
}) {
  const { context } = props;
  return (
    <div className="flex flex-wrap items-center gap-1.5 px-4 pb-3">
      {context.app && (
        <Chip on={props.attachWindow} onClick={props.onWindow} title={context.title ?? undefined}>
          {context.app}
        </Chip>
      )}
      {context.clipboardSecret ? (
        <span className="rounded-full border border-white/8 px-2.5 py-1 text-[11.5px] text-white/35">
          Clipboard hidden (looks like a secret)
        </span>
      ) : (
        context.clipboardKind && (
          <Chip on={props.attachClip} onClick={props.onClip} title={context.clipboardPreview ?? undefined}>
            Clipboard: {context.clipboardKind.replace("_", " ")}
          </Chip>
        )
      )}
      <Chip on={props.localOnly} onClick={props.onLocal} title="Only use a model on this PC">
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
  title?: string;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-pressed={on}
      title={title}
      onClick={onClick}
      className={`max-w-[220px] truncate rounded-full border px-2.5 py-1 text-[11.5px] transition-[background-color,border-color,color,transform] duration-150 ease-out active:scale-[0.97] ${
        on
          ? "border-(--accent)/60 bg-(--accent)/22 text-white"
          : "border-white/10 bg-white/4 text-white/60 hover:bg-white/8 hover:text-white/85"
      }`}
    >
      {on ? "+ " : ""}
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
  children: React.ReactNode;
}) {
  return (
    <li>
      <button
        type="button"
        onMouseMove={onHover}
        onClick={onClick}
        className={`flex w-full items-center gap-3 rounded-xl px-2.5 py-2 text-left text-[14px] transition-colors duration-100 ${
          active ? "bg-white/10 text-white" : "text-white/80"
        }`}
      >
        {children}
      </button>
    </li>
  );
}

function Chat({ turns }: { turns: Turn[] }) {
  return (
    <div className="space-y-3 px-2 py-1">
      <AnimatePresence initial={false}>
        {turns.map((t, i) => (
          <motion.div
            // biome-ignore lint/suspicious/noArrayIndexKey: turns only ever append
            key={i}
            initial={{ opacity: 0, y: 4 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.18, ease }}
            className={t.role === "user" ? "flex justify-end" : ""}
          >
            {t.role === "user" ? (
              <div className="max-w-[85%] rounded-2xl rounded-br-md bg-(--accent)/30 px-3 py-2 text-[13.5px] whitespace-pre-wrap">
                {t.content}
              </div>
            ) : (
              <div className="text-[13.5px] leading-relaxed text-white/90">
                {t.content ? <Markdown text={t.content} /> : t.streaming ? <Thinking /> : null}
                {t.error && (
                  <p className="mt-1 rounded-xl bg-red-500/12 px-3 py-2 text-[12.5px] text-red-200">{t.error}</p>
                )}
                {!t.streaming && t.provider && (
                  <p className="mt-1 text-[11px] text-white/35">{PROVIDER_LABELS[t.provider] ?? t.provider}</p>
                )}
              </div>
            )}
          </motion.div>
        ))}
      </AnimatePresence>
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

function Kbd({ children }: { children: React.ReactNode }) {
  return (
    <kbd className="rounded-[5px] border border-white/12 bg-white/6 px-1.5 py-px font-sans text-[10.5px] text-white/60">
      {children}
    </kbd>
  );
}
