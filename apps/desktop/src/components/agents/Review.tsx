"use client";

// Keep or undo what an agent changed: every change shown, each with Keep and
// Undo, a whole file at once, or everything. Nothing is final until Done.
// Keys: arrows move between changes, K keeps, U undoes.

import { useCallback, useEffect, useState } from "react";
import type { Session } from "@/lib/agents";
import { api } from "@/lib/bridge";
import type { FileChange } from "@/lib/types";

type Hunk = FileChange["hunks"][number];

/** Lines of a hunk with the line number each one has in its file. */
function numbered(h: Hunk): { n: number; kind: "add" | "del" | "ctxl"; text: string }[] {
  const m = h.header.match(/-(\d+)(?:,\d+)? \+(\d+)/);
  let old = m ? Number(m[1]) : 1;
  let now = m ? Number(m[2]) : 1;
  const out: { n: number; kind: "add" | "del" | "ctxl"; text: string }[] = [];
  for (const l of h.lines) {
    if (l.startsWith("\\")) continue;
    if (l.startsWith("+")) out.push({ n: now++, kind: "add", text: l });
    else if (l.startsWith("-")) out.push({ n: old++, kind: "del", text: l });
    else {
      out.push({ n: now, kind: "ctxl", text: l });
      old++;
      now++;
    }
  }
  return out;
}

export function Review({ session, maxHeight, onDone }: { session: Session; maxHeight: number; onDone: () => void }) {
  const [files, setFiles] = useState<FileChange[] | null>(null);
  const [active, setActive] = useState(0);
  // Changes kept, by "path:header". Undone ones leave the diff.
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
  // Every change still waiting for a decision, in order.
  const open = list.flatMap((f) =>
    f.hunks.map((h, n) => ({ f, h, n, key: `${f.path}:${h.header}` })).filter((x) => !kept.has(x.key)),
  );
  const sel = open[Math.min(active, open.length - 1)];
  const keepAll = useCallback(() => {
    setKept(new Set(list.flatMap((f) => f.hunks.map((h) => `${f.path}:${h.header}`))));
    onDone();
  }, [list, onDone]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement)?.tagName === "INPUT") return;
      if (e.altKey && !e.ctrlKey && (e.key === "1" || e.key === "2")) {
        e.preventDefault();
        if (e.key === "1") keepAll();
        else undo(null, null);
        return;
      }
      if (e.ctrlKey || e.metaKey || e.altKey || !sel) return;
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        setActive((a) => (a + (e.key === "ArrowDown" ? 1 : -1) + open.length) % open.length);
      } else if (e.key.toLowerCase() === "u") {
        e.preventDefault();
        undo(sel.f.path, sel.n);
      } else if (e.key.toLowerCase() === "k") {
        e.preventDefault();
        setKept((k) => new Set(k).add(sel.key));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [sel, open.length, undo, keepAll]);

  const changes = list.reduce((n, f) => n + f.hunks.length, 0);
  return (
    <div className="ak-tl ak-in">
      <div className="ak-rv-head">
        <b>Review changes</b>
        <span className="sub2">
          {files === null
            ? "Loading..."
            : `${list.length} ${list.length === 1 ? "file" : "files"} · ${changes} ${changes === 1 ? "change" : "changes"}`}
        </span>
        <span className="keys mono">↑↓ move · K keep · U undo</span>
      </div>
      {error && <p className="ak-err">{error}</p>}
      <div className="ak-tl ak-scroll" style={{ maxHeight: maxHeight - 50 }}>
        {list.map((f) => (
          <div key={f.path} className="ak-rv-file">
            <div className="ak-rv-fh">
              <span className="mono">{f.path}</span>
              {f.status !== "modified" && <span>{f.status}</span>}
              {f.added > 0 && <span className="add-n mono">+{f.added}</span>}
              {f.removed > 0 && <span className="del-n mono">−{f.removed}</span>}
              <span className="acts">
                <button
                  type="button"
                  onClick={() => setKept((k) => new Set([...k, ...f.hunks.map((h) => `${f.path}:${h.header}`)]))}
                  className="ak-rv-btn chip"
                >
                  Keep file
                </button>
                <button type="button" onClick={() => undo(f.path, null)} className="ak-rv-btn chip">
                  Undo file
                </button>
              </span>
            </div>
            {f.hunks.map((h, n) => {
              const key = `${f.path}:${h.header}`;
              if (kept.has(key)) {
                return (
                  <div key={key} className="ak-rv-hunk">
                    <p className="ak-rv-res">
                      <span className="k">✓ Kept</span>
                      <span className="mono truncate">
                        {h.lines
                          .find((l) => l.startsWith("+"))
                          ?.slice(1)
                          .trim()}
                      </span>
                    </p>
                  </div>
                );
              }
              return (
                // biome-ignore lint/a11y/noStaticElementInteractions: a click picks the change; keys act on it
                // biome-ignore lint/a11y/useKeyWithClickEvents: arrows move between changes
                <div
                  key={key}
                  className="ak-rv-hunk"
                  data-sel={sel?.key === key}
                  onClick={() => setActive(open.findIndex((x) => x.key === key))}
                >
                  <pre className="mono">
                    {numbered(h)
                      .slice(0, 60)
                      .map((l, k) => (
                        // biome-ignore lint/suspicious/noArrayIndexKey: lines of one hunk
                        <div key={k}>
                          <span className="ln">{l.n}</span>
                          <span className={l.kind}>{l.text}</span>
                        </div>
                      ))}
                  </pre>
                  <div className="ha">
                    <button
                      type="button"
                      onClick={() => setKept((k) => new Set(k).add(key))}
                      className="ak-rv-btn chip"
                    >
                      Keep <kbd>K</kbd>
                    </button>
                    <button type="button" onClick={() => undo(f.path, n)} className="ak-rv-btn chip">
                      Undo <kbd>U</kbd>
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        ))}
        {files !== null && list.length === 0 && <p className="ak-done">No changes left.</p>}
      </div>
      <div className="ak-chips">
        <button type="button" onClick={keepAll} className="ak-chip primary chip">
          {open.length === 0 ? "Done" : "Keep all"} <kbd>Alt 1</kbd>
        </button>
        {list.length > 0 && (
          <button type="button" onClick={() => undo(null, null)} className="ak-chip chip">
            Undo all <kbd>Alt 2</kbd>
          </button>
        )}
      </div>
    </div>
  );
}
