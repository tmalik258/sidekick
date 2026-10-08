"use client";

// The Repos list in Agents: each repo's branch, ahead and behind, open
// PRs, CI and review requests. A row opens to the PR titles, the failing
// check, Pull and Open in editor.

import { useState } from "react";
import { startHere } from "@/lib/agents";
import { api } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import type { RepoRow, ReposOverview } from "@/lib/types";

const CI_LABEL = { pass: "Checks pass", fail: "Checks failing", pending: "Checks running", none: "" } as const;

function CiDot({ ci }: { ci: RepoRow["ci"] }) {
  if (ci === "none") return null;
  const s = ci === "fail" ? "failed" : ci === "pending" ? "working" : "idle";
  return <span className="ak-sd" data-s={s} role="img" aria-label={CI_LABEL[ci]} title={CI_LABEL[ci]} />;
}

export function Repos() {
  const { data, refresh } = useCached<ReposOverview>("repos", () => api.reposOverview(false));
  const [open, setOpen] = useState<string | null>(null);
  const [note, setNote] = useState<Record<string, string>>({});
  if (!data || data.repos.length === 0) return null;
  const say = (path: string, text: string) => setNote((n) => ({ ...n, [path]: text }));
  const pull = (r: RepoRow, stash: boolean) =>
    void api
      .repoPull(r.path, stash)
      .then((m) => {
        say(r.path, m);
        void refresh?.();
      })
      .catch((e: unknown) => say(r.path, String(e)));
  return (
    <section className="ak-repos" aria-label="Repos">
      <div className="ak-repos-h">
        <span>Repos</span>
        {data.via === "none" && <em>Sign in with gh auth login to see PRs and checks</em>}
      </div>
      {data.repos.map((r) => {
        const expanded = open === r.path;
        return (
          <div key={r.path} className="ak-repo" data-open={expanded}>
            <button
              type="button"
              className="ak-repo-row"
              aria-expanded={expanded}
              onClick={() => setOpen(expanded ? null : r.path)}
            >
              <span className="ak-repo-name truncate">{r.name}</span>
              <span className="ak-repo-branch mono truncate">{r.branch}</span>
              {r.behind > 0 && <span className="ak-repo-n">{r.behind} behind</span>}
              {r.ahead > 0 && <span className="ak-repo-n">{r.ahead} ahead</span>}
              {r.prs.length > 0 && (
                <span className="ak-repo-n">
                  {r.prs.length} {r.prs.length === 1 ? "PR" : "PRs"}
                </span>
              )}
              {r.reviews > 0 && <span className="ak-repo-n ak-repo-hot">{r.reviews} to review</span>}
              <CiDot ci={r.ci} />
            </button>
            {expanded && (
              <div className="ak-repo-more ak-in">
                {r.prs.map((p) => (
                  <div key={p.number} className="ak-repo-pr">
                    <CiDot ci={p.ci} />
                    <button type="button" className="truncate text-left" onClick={() => void api.aiOpenLink(p.url)}>
                      #{p.number} {p.title}
                    </button>
                    {p.reviewRequested && <em>review</em>}
                    {p.failing && (
                      <button
                        type="button"
                        className="ak-chip chip"
                        onClick={() => void api.aiOpenLink(p.failing?.[1] ?? p.url)}
                      >
                        {p.failing[0]} failed
                      </button>
                    )}
                  </div>
                ))}
                <div className="ak-repo-acts">
                  {r.behind > 0 && (
                    <button type="button" className="ak-chip chip" onClick={() => pull(r, r.changed > 0)}>
                      {r.changed > 0 ? "Stash, pull, put back" : "Pull"}
                    </button>
                  )}
                  <button type="button" className="ak-chip chip" onClick={() => void api.repoOpen(r.path)}>
                    Open in editor
                  </button>
                  <button type="button" className="ak-chip chip" onClick={() => startHere(r.path)}>
                    Start an agent here
                  </button>
                </div>
                {note[r.path] && <p className="ak-note">{note[r.path]}</p>}
              </div>
            )}
          </div>
        );
      })}
    </section>
  );
}
