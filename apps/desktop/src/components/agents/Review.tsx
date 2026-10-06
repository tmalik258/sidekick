"use client";

// Keep or undo what an agent changed: every file with its +/- counts; open
// one to see each change and undo just that change, the file, or all of it.
// Nothing is final until Done. Keys: arrows move, U undoes, K keeps.

import { useCallback, useEffect, useState } from "react";
import type { Session } from "@/lib/agents";
import { api } from "@/lib/bridge";
import type { FileChange } from "@/lib/types";
import { Kbd } from "../ask/parts";

export function Review({
  session,
  keys,
  maxHeight,
  onDone,
}: {
  session: Session;
  keys: boolean;
  maxHeight: number;
  onDone: () => void;
}) {
  const [files, setFiles] = useState<FileChange[] | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [active, setActive] = useState(0);
  const [kept, setKept] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    void api
      .agentChanges(session.id)
      .then((f) => {
        setFiles(f);
        setError(null);
      })
      .catch((e: unknown) => setError(String(e)));
  }, [session.id]);
  useEffect(load, [load]);

  const undo = useCallback(
    (path: string | null, hunk: number | null) =>
      void api
        .agentUndo(session.id, path, hunk)
        .then(load)
        .catch((e: unknown) => setError(String(e))),
    [session.id, load],
  );

  const list = files ?? [];
  const shown = list.filter((f) => !kept.has(f.path));
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey || e.metaKey || e.altKey || (e.target as HTMLElement)?.tagName === "INPUT") return;
      const f = shown[Math.min(active, shown.length - 1)];
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        setActive((a) => (shown.length ? (a + (e.key === "ArrowDown" ? 1 : -1) + shown.length) % shown.length : 0));
      } else if (e.key === "Enter" && f) {
        e.preventDefault();
        setOpen((o) => (o === f.path ? null : f.path));
      } else if (e.key.toLowerCase() === "u" && f) {
        e.preventDefault();
        undo(f.path, null);
      } else if (e.key.toLowerCase() === "k" && f) {
        e.preventDefault();
        setKept((k) => new Set(k).add(f.path));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [shown, active, undo]);

  const added = list.reduce((n, f) => n + f.added, 0);
  const removed = list.reduce((n, f) => n + f.removed, 0);
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-2 text-[12.5px]">
        <span className="font-medium text-white">
          {files === null ? "Loading changes..." : `${list.length} ${list.length === 1 ? "file" : "files"} changed`}
        </span>
        {files !== null && (
          <span className="text-[rgb(235_235_245/0.5)]">
            <span className="text-[#30d158]">+{added}</span> <span className="text-[#ff6961]">-{removed}</span>
          </span>
        )}
        <span className="ml-auto flex gap-1.5">
          {list.length > 0 && (
            <button
              type="button"
              onClick={() => undo(null, null)}
              className="chip rounded-full bg-white/[0.12] px-3 py-1 text-[12px] text-white/90"
            >
              Undo all
            </button>
          )}
          <button
            type="button"
            onClick={onDone}
            className="chip rounded-full bg-white px-3 py-1 text-[12px] font-medium text-black"
          >
            Done
          </button>
        </span>
      </div>
      {error && <p className="text-[12px] text-[#ffb4ae]">{error}</p>}
      <ul className="ask-scroll flex flex-col gap-1 overflow-y-auto pr-1" style={{ maxHeight }}>
        {shown.map((f, i) => (
          <li key={f.path} className={`rounded-xl ${i === active ? "bg-white/[0.08]" : "bg-white/[0.03]"}`}>
            <div className="flex items-center gap-2 px-2.5 py-1.5 text-[12.5px]">
              <button
                type="button"
                onClick={() => {
                  setActive(i);
                  setOpen((o) => (o === f.path ? null : f.path));
                }}
                className="flex min-w-0 flex-1 items-center gap-2 text-left"
                aria-expanded={open === f.path}
              >
                <span className="truncate font-mono text-[12px] text-white/90">{f.path}</span>
                {f.status !== "modified" && (
                  <span className="shrink-0 text-[11px] text-[rgb(235_235_245/0.45)]">{f.status}</span>
                )}
                <span className="shrink-0 text-[11px]">
                  <span className="text-[#30d158]">+{f.added}</span>{" "}
                  <span className="text-[#ff6961]">-{f.removed}</span>
                </span>
              </button>
              <button
                type="button"
                onClick={() => setKept((k) => new Set(k).add(f.path))}
                className="chip rounded-full bg-white/[0.1] px-2 py-0.5 text-[11.5px] text-white/85"
              >
                Keep
              </button>
              <button
                type="button"
                onClick={() => undo(f.path, null)}
                className="chip rounded-full bg-white/[0.1] px-2 py-0.5 text-[11.5px] text-white/85"
              >
                Undo
              </button>
            </div>
            {open === f.path && (
              <div className="flex flex-col gap-1.5 px-2.5 pb-2">
                {f.hunks.map((h, n) => (
                  <div key={h.header} className="overflow-hidden rounded-lg bg-black/40">
                    <div className="flex items-center justify-between px-2 py-1 text-[11px] text-[rgb(235_235_245/0.45)]">
                      <span className="font-mono">{h.header}</span>
                      {f.hunks.length > 1 && (
                        <button
                          type="button"
                          onClick={() => undo(f.path, n)}
                          className="chip rounded-full bg-white/[0.1] px-2 py-0.5 text-white/85"
                        >
                          Undo this change
                        </button>
                      )}
                    </div>
                    <pre className="overflow-x-auto px-2 pb-1.5 font-mono text-[11px] leading-[1.45]">
                      {h.lines.slice(0, 80).map((l, k) => (
                        <div
                          // biome-ignore lint/suspicious/noArrayIndexKey: lines of one hunk
                          key={k}
                          className={
                            l.startsWith("+")
                              ? "bg-[#30d158]/10 text-[#9cf0b0]"
                              : l.startsWith("-")
                                ? "bg-[#ff453a]/10 text-[#ffb4ae]"
                                : "text-white/55"
                          }
                        >
                          {l}
                        </div>
                      ))}
                    </pre>
                  </div>
                ))}
              </div>
            )}
          </li>
        ))}
        {files !== null && shown.length === 0 && (
          <li className="py-2 text-[12.5px] text-[rgb(235_235_245/0.55)]">
            {list.length ? "Everything kept." : "No changes left."}
          </li>
        )}
      </ul>
      {keys && (
        <p className="flex gap-3 text-[11px] text-[rgb(235_235_245/0.5)]">
          <span>
            <Kbd>↑↓</Kbd> move
          </span>
          <span>
            <Kbd>Enter</Kbd> show
          </span>
          <span>
            <Kbd>K</Kbd> keep
          </span>
          <span>
            <Kbd>U</Kbd> undo
          </span>
        </p>
      )}
    </div>
  );
}
