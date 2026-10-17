"use client";

// An update of your branch from main that stopped on conflicts, solved
// right in the island: each file shows both sides, and you keep one, keep
// both, open it in your editor or hand it to Claude Code. Finish commits
// the merge and puts your changes back; Undo leaves things as before.

import { useState } from "react";
import { api } from "@/lib/bridge";
import { openMerge, useDuskie } from "@/lib/store";
import { Ic } from "./Icon";

/** One row per file, pick a side, and nothing is written until Finish merge
 * applies every pick at once. */
export function MergeCard({ path }: { path: string }) {
  const state = useDuskie((s) => s.merge?.state);
  const [picks, setPicks] = useState<Record<string, "mine" | "theirs">>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (!state) return null;
  const files = state.files;
  const chosen = files.filter((f) => picks[f.file]).length;
  const repo = path.split(/[\\/]/).filter(Boolean).pop() ?? path;
  const end = async (finish: boolean) => {
    setBusy(true);
    setError(null);
    try {
      if (finish) for (const f of files) await api.mergeKeep(path, f.file, picks[f.file]);
      const lastResult = await api.mergeEnd(path, finish);
      useDuskie.setState({ merge: null, lastResult, mascot: "success" });
    } catch (e) {
      setError(String(e));
      void openMerge(path);
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="rd-glance flex flex-col gap-2.5">
      <div className="rd-card-head">
        <p className="rd-eyebrow flex items-center gap-1.5">
          <span className="rd-dot" style={{ background: "var(--err)" }} />
          {repo}, pull from {state.main}
        </p>
        <p className="rd-title">
          {files.length} {files.length === 1 ? "file needs" : "files need"} a decision
        </p>
        <p className="rd-detail">Pick a side for each, or open it in the review.</p>
      </div>
      <div className="flex flex-col gap-1" style={{ marginLeft: "calc(var(--orb-indent, 0px) * -1)" }}>
        {files.map((f) => (
          <div key={f.file} className="rd-list-row mg-row">
            <span className="t mono">{f.file}</span>
            <span className="r">
              {f.spots} {f.spots === 1 ? "conflict" : "conflicts"}
            </span>
            <span className="inline-flex gap-0.5">
              {(["mine", "theirs"] as const).map((side) => (
                <button
                  key={side}
                  type="button"
                  className="chip-s"
                  aria-pressed={picks[f.file] === side}
                  onClick={() => setPicks((p) => ({ ...p, [f.file]: side }))}
                >
                  {side === "mine" ? "Mine" : "Theirs"}
                </button>
              ))}
              <button
                type="button"
                className="chip-s ic-only"
                aria-label="Open in review"
                onClick={() => void api.repoOpen(`${path}/${f.file}`)}
              >
                <Ic name="file" size={12} />
              </button>
            </span>
          </div>
        ))}
        {error && <p className="px-1 text-[12px] text-[#ff9f8a]">{error}</p>}
        <div className="mt-1.5 flex items-center gap-1.5">
          <button
            type="button"
            className="rd-btn primary"
            disabled={chosen < files.length || busy}
            onClick={() => void end(true)}
          >
            Finish merge
          </button>
          <span className="text-[12px] text-[var(--ink-3)]">
            {chosen} of {files.length} chosen
          </span>
          <button type="button" className="rd-btn ghost ml-auto" disabled={busy} onClick={() => void end(false)}>
            Abort
          </button>
        </div>
      </div>
    </div>
  );
}
