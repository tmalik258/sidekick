"use client";
import { motion } from "motion/react";
import { type RefObject, useEffect, useRef, useState } from "react";
import { api } from "@/lib/bridge";
import { runProposal, sendChat, useSidekick } from "@/lib/store";
import type { Proposal } from "@/lib/types";

/** Buttons still waiting for a tap; they take Alt 1, Alt 2... first. */
export function pendingCount(items: Proposal[] | undefined): number {
  return (items ?? []).filter((p) => !p.ran).length;
}

/** Alt + a digit, from the island's own keys (it has focus in Ask). */
export function useAltDigits(count: number, start: number, run: (n: number) => void) {
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
export function Proposals({ items, keys }: { items: Proposal[]; keys: boolean }) {
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
  // Do all: one tap runs every waiting action in order, stopping at the
  // first that fails so later steps never run on a broken one.
  const [busy, setBusy] = useState(false);
  const doAll = async () => {
    if (busy) return;
    setBusy(true);
    try {
      for (const p of pending) {
        await runProposal(p.id);
        const ran = useSidekick
          .getState()
          .turns.flatMap((t) => t.proposals ?? [])
          .find((x) => x.id === p.id)?.ran;
        if (!ran?.ok) break;
      }
    } finally {
      setBusy(false);
    }
  };
  const allRef = useRef(doAll);
  allRef.current = doAll;
  const many = pending.length >= 2;
  useEffect(() => {
    if (!keys || !many) return;
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.altKey && !e.ctrlKey && e.key.toLowerCase() === "a") {
        e.preventDefault();
        void allRef.current();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [keys, many]);
  return (
    <div className="mt-2 flex flex-col gap-1.5">
      {many && (
        <button
          type="button"
          disabled={busy}
          onClick={() => void doAll()}
          className="chip flex min-h-8 items-center gap-2 self-start rounded-full bg-[#0a84ff] px-3.5 py-1.5 text-[13px] font-medium text-white hover:bg-[#0a84ff]/90 disabled:opacity-60"
        >
          {busy ? "Working..." : `Do all ${pending.length}`}
          {keys && !busy && <kbd className="shrink-0 font-sans text-[11px] text-white/60">Alt A</kbd>}
        </button>
      )}
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
          <motion.button
            key={p.id}
            type="button"
            initial={{ opacity: 0, y: 4 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.2, delay: pending.indexOf(p) * 0.04, ease: [0.23, 1, 0.32, 1] }}
            onClick={() => void runProposal(p.id)}
            className={`chip flex min-h-8 items-center gap-2 self-start rounded-full px-3.5 py-1.5 text-[13px] font-medium ${
              pending.indexOf(p) === 0 && !many
                ? "bg-white text-black hover:bg-white/90"
                : "bg-white/[0.12] text-white hover:bg-white/[0.2]"
            }`}
          >
            {p.label}
            {keys && pending.indexOf(p) < 9 && (
              <kbd
                className={`shrink-0 font-sans text-[11px] ${pending.indexOf(p) === 0 && !many ? "text-black/40" : "text-white/35"}`}
              >
                Alt {pending.indexOf(p) + 1}
              </kbd>
            )}
          </motion.button>
        ),
      )}
    </div>
  );
}

export function UndoProposal({
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
export function AnswerOptions({ options: all, start }: { options: string[]; start: number }) {
  // Three actions at most with the buttons above, so the next step is a
  // glance, not a menu. Only the first action of all is solid.
  const options = all.slice(0, Math.max(1, 3 - start));
  const shown = Math.max(0, Math.min(options.length, 9 - start));
  useAltDigits(shown, start, (n) => sendChat(options[n]));
  return (
    <div className="mt-2 flex flex-wrap gap-1.5">
      {options.map((o, i) => (
        <motion.button
          key={o}
          type="button"
          initial={{ opacity: 0, y: 4 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.2, delay: i * 0.04, ease: [0.23, 1, 0.32, 1] }}
          onClick={() => sendChat(o)}
          className={`chip flex min-h-8 max-w-full items-center gap-2 rounded-full px-3 py-1.5 text-left text-[13px] font-medium ${
            i === 0 && start === 0
              ? "bg-white text-black hover:bg-white/90"
              : "bg-white/[0.12] text-white hover:bg-white/[0.2]"
          }`}
        >
          <span className="leading-snug">{o}</span>
          <kbd
            className={`shrink-0 font-sans text-[11px] ${i === 0 && start === 0 ? "text-black/40" : "text-white/35"}`}
          >
            {i < shown ? `Alt ${start + i + 1}` : ""}
          </kbd>
        </motion.button>
      ))}
    </div>
  );
}
