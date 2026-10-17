"use client";

// The Git dock: Changes (checkbox staging, one commit message with a
// sparkle button that writes it from the staged diff, Commit and Amend),
// History, the branch's pull request, and Stashes. Esc closes it.

import { useCallback, useEffect, useState } from "react";
import { api } from "@/lib/bridge";
import { baseName, countChanges, groupByFolder } from "@/lib/changes";
import type { CommitRow, GitFile, RepoRow, StashRow } from "@/lib/types";
import { Ic, Icon } from "../Icon";
import { Tip } from "../Tip";
import { Editor } from "./Editor";

const KIND: Record<string, string> = {
  M: "changed",
  A: "added",
  D: "deleted",
  R: "renamed",
  "?": "new",
  U: "conflict",
};

type GitTab = "changes" | "history" | "pr" | "stashes";

export function GitDock({ path, repo, onClose }: { path: string; repo?: RepoRow | null; onClose: () => void }) {
  const [tab, setTab] = useState<GitTab>("changes");
  const [stashes, setStashes] = useState<StashRow[]>([]);
  const [changed, setChanged] = useState(0);
  const [full, setFull] = useState(false);
  const [at, setAt] = useState<{ path: string; atChange?: boolean } | null>(null);
  const loadStashes = useCallback(
    () =>
      void api
        .gitStashes(path)
        .then(setStashes)
        .catch(() => {}),
    [path],
  );
  useEffect(loadStashes, [loadStashes]);

  // Esc closes the dock (the editor handles its own Esc).
  useEffect(() => {
    if (full) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, full]);

  const pr = repo?.prs.find((p) => p.branch === repo.branch) ?? null;
  const tabs: [GitTab, string, number | null][] = [
    ["changes", "Changes", changed],
    ["history", "History", null],
    ["pr", "Pull request", null],
    ["stashes", "Stashes", stashes.length],
  ];
  if (full)
    return (
      <Editor
        root={path}
        initial={at}
        onClose={() => {
          setFull(false);
          setAt(null);
        }}
      />
    );
  return (
    <section className="gd flex min-h-0 flex-1 flex-col" aria-label="Git">
      <div className="gd-bar">
        <span className="gd-pick">
          <Ic name="folder" size={14} />
          <span className="truncate">{repo?.name ?? baseName(path)}</span>
        </span>
        {repo?.branch && (
          <span className="gd-pick">
            <Ic name="branch" size={14} />
            <span className="truncate mono">{repo.branch}</span>
          </span>
        )}
        <button
          type="button"
          className="rd-btn ghost chip-s ml-auto"
          onClick={() => {
            setAt(null);
            setFull(true);
          }}
        >
          Full screen
        </button>
        <button type="button" className="rd-btn ghost chip-s ic-only" aria-label="Close Git" onClick={onClose}>
          <Ic name="x" size={12} />
        </button>
      </div>
      <div className="gd-tabs" role="tablist" aria-label="Git views">
        {tabs.map(([k, label, n]) => (
          <button
            key={k}
            type="button"
            role="tab"
            className="gd-tab"
            aria-selected={tab === k}
            onClick={() => setTab(k)}
          >
            {label}
            {n !== null && n > 0 && <span className="n">{n}</span>}
          </button>
        ))}
      </div>
      <div className="gd-body min-h-0 flex-1">
        {tab === "changes" && (
          <Changes
            path={path}
            onCount={setChanged}
            onOpen={(file) => {
              setAt({ path: file, atChange: true });
              setFull(true);
            }}
          />
        )}
        {tab === "history" && <History path={path} />}
        {tab === "pr" && <PullRequest pr={pr} branch={repo?.branch ?? null} />}
        {tab === "stashes" && <Stashes path={path} stashes={stashes} reload={loadStashes} />}
      </div>
    </section>
  );
}

function History({ path }: { path: string }) {
  const [rows, setRows] = useState<CommitRow[] | null>(null);
  useEffect(
    () =>
      void api
        .gitLog(path)
        .then(setRows)
        .catch(() => setRows([])),
    [path],
  );
  if (!rows) return <p className="gd-empty shimmer-text">Reading the history</p>;
  if (rows.length === 0) return <p className="gd-empty">No commits yet.</p>;
  return (
    <div className="flex flex-col">
      {rows.map((c) => (
        <div key={c.hash} className="gd-row">
          <span className="mono gd-hash">{c.hash}</span>
          <span className="min-w-0 flex-1 truncate">{c.subject}</span>
          <span className="gd-meta">
            {c.author}, {c.when}
          </span>
        </div>
      ))}
    </div>
  );
}

const CI_TEXT = { pass: "Checks pass", fail: "Checks failing", pending: "Checks running", none: "No checks" } as const;

function PullRequest({ pr, branch }: { pr: RepoRow["prs"][number] | null; branch: string | null }) {
  if (!pr) {
    return <p className="gd-empty">{branch ? `No open pull request for ${branch}.` : "No open pull request."}</p>;
  }
  return (
    <div className="flex flex-col gap-2.5">
      <div>
        <p className="gd-meta mono">#{pr.number}</p>
        <p className="font-medium">{pr.title}</p>
      </div>
      <div className="gd-row">
        <span
          className="rd-dot"
          style={{
            background:
              pr.ci === "pass"
                ? "var(--ok)"
                : pr.ci === "fail"
                  ? "var(--err)"
                  : pr.ci === "pending"
                    ? "var(--warn)"
                    : "var(--ink-3)",
          }}
        />
        <span className="flex-1">{CI_TEXT[pr.ci]}</span>
        {pr.failing && (
          <button type="button" className="rd-btn ghost" onClick={() => void api.aiOpenLink(pr.failing?.[1] ?? pr.url)}>
            {pr.failing[0]}
          </button>
        )}
      </div>
      {pr.reviewRequested && <p className="gd-meta">Your review is requested.</p>}
      <div className="flex gap-1.5">
        <button type="button" className="rd-btn primary" onClick={() => void api.aiOpenLink(pr.url)}>
          Open on GitHub
        </button>
      </div>
    </div>
  );
}

function Stashes({ path, stashes, reload }: { path: string; stashes: StashRow[]; reload: () => void }) {
  const [note, setNote] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<string | null>(null);
  const run = (work: Promise<string>) =>
    void work
      .then(setNote)
      .catch((e) => setNote(String(e)))
      .finally(reload);
  if (stashes.length === 0) return <p className="gd-empty">No stashes.</p>;
  return (
    <div className="flex flex-col gap-1">
      {stashes.map((s) => (
        <div key={s.name} className="gd-row">
          <Ic name="box" size={14} />
          <span className="min-w-0 flex-1">
            <span className="block truncate">{s.message}</span>
            <span className="gd-meta mono">
              {s.name}, {s.when}
            </span>
          </span>
          <button type="button" className="rd-btn primary" onClick={() => run(api.gitStashApply(path, s.name, true))}>
            Pop
          </button>
          <button type="button" className="rd-btn ghost" onClick={() => run(api.gitStashApply(path, s.name, false))}>
            Apply
          </button>
          {confirm === s.name ? (
            <button
              type="button"
              className="rd-btn ghost"
              style={{ color: "var(--err)" }}
              onClick={() => {
                setConfirm(null);
                run(api.gitStashDrop(path, s.name));
              }}
            >
              Drop for good
            </button>
          ) : (
            <button
              type="button"
              className="rd-btn ghost chip-s ic-only"
              aria-label={`Drop ${s.name}`}
              onClick={() => setConfirm(s.name)}
            >
              <Ic name="trash" size={12} />
            </button>
          )}
        </div>
      ))}
      {note && (
        <p className="gd-meta" role="status">
          {note}
        </p>
      )}
    </div>
  );
}

function Changes({
  path,
  onCount,
  onOpen,
}: {
  path: string;
  onCount: (n: number) => void;
  onOpen: (file: string) => void;
}) {
  const [files, setFiles] = useState<GitFile[]>([]);
  const [message, setMessage] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [warnAmend, setWarnAmend] = useState(false);

  const refresh = useCallback(() => void api.gitStatus(path).then(setFiles), [path]);
  useEffect(refresh, [refresh]);
  useEffect(() => onCount(files.length), [files, onCount]);

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
          <span className="text-white/65">
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
        <p className="text-white/65">Nothing has changed.</p>
      ) : (
        groupByFolder(files).map((g) => (
          <div key={g.folder}>
            <p className="mono text-[11px] text-white/65">{g.folder}</p>
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
                <span className="text-[11px] text-white/65">{KIND[f.kind] ?? f.kind}</span>
                {f.kind !== "D" && (
                  <button
                    type="button"
                    className="rd-btn ghost chip-s"
                    aria-label={`Open ${f.path} at the change`}
                    onClick={(e) => {
                      e.preventDefault();
                      onOpen(f.path);
                    }}
                  >
                    Open
                  </button>
                )}
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
            <Ic name="spark" size={14} />
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
