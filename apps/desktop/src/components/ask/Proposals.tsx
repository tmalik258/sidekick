"use client";
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
  // One action is a button; several are a checklist that ticks as each runs.
  if (items.length < 2) {
    return (
      <div className="ak-chips">
        {items.map((p) =>
          p.ran ? (
            <Ran key={p.id} p={p} undoRef={p === undoable && keys ? undoRef : undefined} />
          ) : (
            <button
              key={p.id}
              type="button"
              onClick={() => void runProposal(p.id)}
              className="ak-chip primary chip ak-in"
            >
              {p.label}
              {keys && <kbd>Alt 1</kbd>}
            </button>
          ),
        )}
      </div>
    );
  }
  return (
    <div className="ak-body">
      <div className="ak-prs">
        {items.map((p, i) => (
          <button
            key={p.id}
            type="button"
            disabled={!!p.ran || busy}
            onClick={() => void runProposal(p.id)}
            data-state={p.ran ? (p.ran.ok ? "done" : "failed") : "waiting"}
            title={p.ran?.message}
            className="ak-pr ak-in"
            style={{ animationDelay: `${i * 40}ms` }}
          >
            <span className="ak-cb" aria-hidden="true" />
            <span className="min-w-0 flex-1 truncate">{p.ran && !p.ran.ok ? p.ran.message : p.label}</span>
            {!p.ran && keys && pending.indexOf(p) < 9 && <kbd>Alt {pending.indexOf(p) + 1}</kbd>}
          </button>
        ))}
      </div>
      <div className="ak-chips">
        {pending.length > 0 && (
          <button type="button" disabled={busy} onClick={() => void doAll()} className="ak-chip primary chip">
            {busy ? "Working..." : `Do all ${pending.length}`}
            {keys && !busy && <kbd>Alt A</kbd>}
          </button>
        )}
        {undoable && <UndoProposal proposal={undoable} buttonRef={keys ? undoRef : undefined} />}
      </div>
    </div>
  );
}

/** What one action did, with Undo and Show when they apply. */
function Ran({ p, undoRef }: { p: Proposal; undoRef?: RefObject<HTMLButtonElement | null> }) {
  if (!p.ran) return null;
  return (
    <span className="flex min-w-0 basis-full items-center gap-2 text-[12.5px]">
      <span className={`size-1.5 shrink-0 rounded-full ${p.ran.ok ? "bg-[#30d158]" : "bg-[#ff453a]"}`} />
      <span className="min-w-0 flex-1 truncate text-[rgb(235_235_245/0.75)]">{p.ran.message}</span>
      {p.ran.ok && p.ran.undoId != null && !p.ran.undone && <UndoProposal proposal={p} buttonRef={undoRef} />}
      {p.ran.ok && p.ran.path && (
        <button type="button" onClick={() => void api.revealPath(p.ran?.path ?? "")} className="ak-chip chip">
          Show
        </button>
      )}
    </span>
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
      className="ak-chip chip"
    >
      Undo
      {buttonRef && <kbd>Alt U</kbd>}
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
    <div className="ak-chips">
      {options.map((o, i) => (
        <button
          key={o}
          type="button"
          onClick={() => sendChat(o)}
          className={`ak-chip chip ak-in max-w-full text-left ${i === 0 && start === 0 ? "primary" : ""}`}
          style={{ animationDelay: `${i * 40}ms` }}
        >
          <span className="leading-snug">{o}</span>
          {i < shown && <kbd>Alt {start + i + 1}</kbd>}
        </button>
      ))}
    </div>
  );
}
