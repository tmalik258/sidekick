"use client";

// An update of your branch from main that stopped on conflicts, solved
// right in the island: each file shows both sides, and you keep one, keep
// both, open it in your editor or hand it to Claude Code. Finish commits
// the merge and puts your changes back; Undo leaves things as before.

import { useState } from "react";
import { setTab, startSession } from "@/lib/agents";
import { api } from "@/lib/bridge";
import { openMerge, useSidekick } from "@/lib/store";

type Side = "mine" | "theirs" | "both";

const ICON: Record<string, [string, string]> = {
  ts: ["TS", "#3178c6"],
  tsx: ["TSX", "#3178c6"],
  js: ["JS", "#b59a00"],
  jsx: ["JSX", "#b59a00"],
  rs: ["RS", "#ce422b"],
  py: ["PY", "#3572a5"],
  css: ["CSS", "#663399"],
  json: ["{ }", "#6e6e73"],
  md: ["MD", "#6e6e73"],
};

function split(file: string): [string, string] {
  const at = file.lastIndexOf("/");
  return at < 0 ? [file, ""] : [file.slice(at + 1), file.slice(0, at)];
}

export function MergeCard({ path }: { path: string }) {
  const state = useSidekick((s) => s.merge?.state);
  const [kept, setKept] = useState<Record<string, string>>({});
  const [open, setOpen] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (!state) return null;
  const files = state.files;
  const all = files.length;
  const left = state.files.filter((f) => !kept[f.file]).length;
  const shown = open ?? state.files.find((f) => !kept[f.file])?.file ?? null;

  const keep = async (file: string, side: Side) => {
    setError(null);
    try {
      await api.mergeKeep(path, file, side);
      setKept((k) => ({ ...k, [file]: side === "mine" ? "yours" : side === "theirs" ? `${state.main}'s` : "both" }));
      setOpen(null);
    } catch (e) {
      setError(String(e));
    }
  };
  const end = async (finish: boolean) => {
    setBusy(true);
    setError(null);
    try {
      const lastResult = await api.mergeEnd(path, finish);
      useSidekick.setState({ merge: null, lastResult, mascot: "success" });
    } catch (e) {
      setError(String(e));
      // The files may have changed outside the island: look again.
      void openMerge(path);
    } finally {
      setBusy(false);
    }
  };
  const askClaude = () =>
    void startSession(
      "claude_code",
      path,
      `Updating ${state.branch} from ${state.main} stopped on conflicts in ${state.files
        .filter((f) => !kept[f.file])
        .map((f) => f.file)
        .join(", ")}. Resolve them keeping the intent of both sides, then stage the files. Do not commit.`,
      "edit",
    ).then(() => {
      setTab("agents");
      useSidekick.setState({ merge: null });
    });

  return (
    <div className="flex flex-col">
      <p className="truncate font-display text-[15px] leading-5 font-semibold tracking-[-0.015em] text-white">
        {left ? `${left} of ${all} ${all === 1 ? "file needs" : "files need"} you` : "All conflicts solved"}
      </p>
      <p className="mt-0.5 truncate text-[13px] leading-4.5 text-[rgb(235_235_245/0.6)]">
        Updating {state.branch} from {state.main}
        {state.stashed ? " · your changes are put aside" : ""}
      </p>
      <div className="nk-meter" aria-hidden="true">
        {Array.from({ length: all }, (_, i) => (
          // biome-ignore lint/suspicious/noArrayIndexKey: fixed segments
          <i key={i} data-on={i < all - left} />
        ))}
      </div>
      <div className="nk-files">
        {files.map((f) => {
          const done = kept[f.file];
          const expanded = !done && shown === f.file;
          const [name, dir] = split(f.file);
          const [label, color] = ICON[name.split(".").pop()?.toLowerCase() ?? ""] ?? ["", "#48484a"];
          return (
            <div key={f.file} className="nk-file" data-done={!!done}>
              <button
                type="button"
                className="nk-fh"
                aria-expanded={expanded}
                disabled={!!done}
                onClick={() => setOpen(expanded ? "" : f.file)}
              >
                <span className="nk-fi" style={{ background: color }}>
                  {label}
                </span>
                <span className="min-w-0">
                  <span className="nk-fn">{name}</span>
                  {dir && <span className="nk-fd">{dir}</span>}
                </span>
                <span className="nk-st" data-ok={!!done}>
                  {done ? `Kept ${done}` : `${f.spots} spot${f.spots > 1 ? "s" : ""}`}
                </span>
              </button>
              {expanded && (
                <>
                  <div className="nk-sides">
                    <div className="nk-side">
                      <header>
                        <i style={{ background: "#64d2ff" }} />
                        <span>Yours · {state.branch}</span>
                        <button type="button" className="nk-keep" onClick={() => void keep(f.file, "mine")}>
                          Keep yours
                        </button>
                      </header>
                      <pre>{f.mine}</pre>
                    </div>
                    <div className="nk-side">
                      <header>
                        <i style={{ background: "#ff9f0a" }} />
                        <span>{state.main}</span>
                        <button type="button" className="nk-keep" onClick={() => void keep(f.file, "theirs")}>
                          Keep {state.main}'s
                        </button>
                      </header>
                      <pre>{f.theirs}</pre>
                    </div>
                  </div>
                  <div className="nk-more">
                    <button type="button" className="nk-b" onClick={() => void keep(f.file, "both")}>
                      Keep both
                    </button>
                    <button type="button" className="nk-b" onClick={() => void api.repoOpen(`${path}/${f.file}`)}>
                      Open in editor
                    </button>
                  </div>
                </>
              )}
            </div>
          );
        })}
      </div>
      {error && <p className="mt-2 text-[12px] text-[#ff9f8a]">{error}</p>}
      <div className="mt-3.5 flex flex-wrap items-center gap-1.5">
        {left > 0 && (
          <button
            type="button"
            className="chip rounded-full bg-white px-3 py-1.5 text-[13px] font-medium text-black hover:bg-white/90"
            onClick={askClaude}
          >
            Ask Claude Code to fix
          </button>
        )}
        <button
          type="button"
          disabled={left > 0 || busy}
          className={`chip rounded-full px-3 py-1.5 text-[13px] font-medium disabled:opacity-35 ${
            left ? "bg-white/12 text-white" : "bg-[#30d158] text-black hover:bg-[#30d158]/90"
          }`}
          onClick={() => void end(true)}
        >
          Finish update
        </button>
        <button
          type="button"
          disabled={busy}
          className="chip rounded-full bg-white/12 px-3 py-1.5 text-[13px] font-medium text-white hover:bg-white/20"
          onClick={() => void end(false)}
        >
          Undo update
        </button>
      </div>
    </div>
  );
}
