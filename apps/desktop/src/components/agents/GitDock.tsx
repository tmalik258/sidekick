"use client";

// The Git dock: changed files with checkbox staging, one commit message
// with a sparkle button that writes it from the staged diff, Commit, and
// Amend. It shows exactly what will be committed.

import { useCallback, useEffect, useState } from "react";
import { api } from "@/lib/bridge";
import { baseName, countChanges, groupByFolder } from "@/lib/changes";
import type { GitFile } from "@/lib/types";
import { Icon } from "../Icon";
import { Tip } from "../Tip";

const KIND: Record<string, string> = {
  M: "changed",
  A: "added",
  D: "deleted",
  R: "renamed",
  "?": "new",
  U: "conflict",
};

export function GitDock({ path, onClose }: { path: string; onClose: () => void }) {
  const [files, setFiles] = useState<GitFile[]>([]);
  const [message, setMessage] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [warnAmend, setWarnAmend] = useState(false);

  const refresh = useCallback(() => void api.gitStatus(path).then(setFiles), [path]);
  useEffect(refresh, [refresh]);

  // Esc closes the dock.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const counts = countChanges(files);
  const act = async (work: () => Promise<string>) => {
    setBusy(true);
    setNote(null);
    try {
      setNote(await work());
    } catch (e) {
      setNote(String(e));
    } finally {
      setBusy(false);
      refresh();
    }
  };
  const toggle = (f: GitFile) =>
    act(() => (f.staged && !f.unstaged ? api.gitUnstage(path, [f.path]) : api.gitStage(path, [f.path])));
  const write = () =>
    act(async () => {
      const text = await api.gitWriteMessage(path);
      setMessage(text);
      return "Message written from the staged changes. Edit it if you like.";
    });
  const commit = () =>
    act(async () => {
      const done = await api.gitCommit(path, message);
      setMessage("");
      return done;
    });
  const amend = async () => {
    // Amending a commit that is already on the remote needs a force push.
    if (!warnAmend && (await api.gitLastCommitPushed(path))) {
      setWarnAmend(true);
      return;
    }
    setWarnAmend(false);
    await act(() => api.gitAmend(path, message.trim() || undefined));
  };

  return (
    <section className="ak-body flex flex-col gap-2 text-[13px]" aria-label="Git changes">
      <div className="flex items-center justify-between">
        <span className="font-medium">
          Changes{" "}
          <span className="text-white/55">
            {counts.staged} of {counts.total} staged
          </span>
        </span>
        <button
          type="button"
          className="ak-chip chip"
          disabled={busy || counts.total === 0}
          onClick={() => void act(() => (counts.all ? api.gitUnstage(path, []) : api.gitStage(path, [])))}
        >
          {counts.all ? "Unstage all" : "Stage all"}
        </button>
      </div>

      {files.length === 0 ? (
        <p className="text-white/55">Nothing has changed.</p>
      ) : (
        groupByFolder(files).map((g) => (
          <div key={g.folder}>
            <p className="mono text-[11px] text-white/45">{g.folder}</p>
            {g.files.map((f) => (
              <label key={f.path} className="flex cursor-pointer items-center gap-2 py-0.5">
                <input
                  type="checkbox"
                  checked={f.staged}
                  disabled={busy || f.kind === "U"}
                  onChange={() => void toggle(f)}
                  aria-label={`Stage ${f.path}`}
                />
                <span className="min-w-0 flex-1 truncate">{baseName(f.path)}</span>
                <span className="text-[11px] text-white/45">{KIND[f.kind] ?? f.kind}</span>
              </label>
            ))}
          </div>
        ))
      )}

      <div className="relative">
        <textarea
          value={message}
          onChange={(e) => setMessage(e.target.value)}
          rows={3}
          aria-label="Commit message"
          placeholder="Commit message"
          className="w-full resize-none rounded-lg bg-white/[0.06] p-2 pr-9 outline-none focus-visible:outline-2 focus-visible:outline-(--accent)"
        />
        <Tip label="Write it from the staged changes">
          <button
            type="button"
            aria-label="Write the message"
            disabled={busy || counts.total === 0}
            onClick={() => void write()}
            className="absolute top-1.5 right-1.5 grid size-6 place-items-center rounded-md hover:bg-white/10 disabled:opacity-40"
          >
            <Icon name="sparkle" size={14} />
          </button>
        </Tip>
      </div>

      {warnAmend && (
        <p className="text-(--warn)" role="alert">
          That commit is already on the remote. Amending it means a force push. Press Amend again to go on.
        </p>
      )}
      <div className="ak-chips">
        <button
          type="button"
          className="ak-chip primary chip"
          disabled={busy || counts.staged === 0 || message.trim() === ""}
          onClick={() => void commit()}
        >
          Commit
        </button>
        <button
          type="button"
          className="ak-chip chip"
          disabled={busy || counts.staged === 0}
          onClick={() => void amend()}
        >
          Amend last commit
        </button>
      </div>
      {note && (
        <p className="text-white/60" role="status">
          {note}
        </p>
      )}
    </section>
  );
}
