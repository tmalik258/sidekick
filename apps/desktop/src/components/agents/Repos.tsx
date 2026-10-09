"use client";

// The Repos tab: every repo with its branch, and only what needs you
// coloured (behind, failing checks, reviews). A row opens to its actions:
// a failed check says what failed and offers the fix right there.

import { useEffect, useState } from "react";
import { setTab, startHere, startSession } from "@/lib/agents";
import { api } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import { useNow } from "@/lib/hooks";
import type { RepoRow, ReposOverview } from "@/lib/types";

const CI_COLOR = { pass: "#30d158", fail: "#ff453a", pending: "#ff9f0a", none: "rgb(255 255 255 / 0.25)" } as const;
const CI_LABEL = { pass: "Checks pass", fail: "Checks failing", pending: "Checks running", none: "No checks" } as const;

function since(t: number, now: number): string {
  const s = Math.max(0, Math.round((now - t) / 1000));
  if (s < 60) return `${s} s ago`;
  return `${Math.round(s / 60)} min ago`;
}

function Tags({ r }: { r: RepoRow }) {
  return (
    <span className="ak-rtags">
      {r.behind > 0 && (
        <span className="ak-rtag" data-k="behind">
          {r.behind} behind
        </span>
      )}
      {r.ahead > 0 && (
        <span className="ak-rtag" data-k="ahead">
          {r.ahead} ahead
        </span>
      )}
      {r.prs.length > 0 && (
        <span className="ak-rtag">
          {r.prs.length} {r.prs.length === 1 ? "PR" : "PRs"}
        </span>
      )}
      {r.ci === "pending" && <span className="ak-rtag">CI running</span>}
      {r.reviews > 0 && (
        <span className="ak-rtag" data-k="behind">
          {r.reviews} to review
        </span>
      )}
    </span>
  );
}

// Placeholder rows shaped like the real ones, so the list does not jump
// when the repos arrive.
const SKELETON = [
  [46, 30, 2],
  [62, 22, 1],
  [38, 26, 0],
] as const;

function ReposLoading() {
  return (
    <section className="ak-repos" aria-label="Repos" aria-busy="true">
      <div className="ak-rh">
        <span className="shimmer-text">Finding your repos and checking GitHub</span>
      </div>
      {SKELETON.map(([name, branch, tags], i) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: fixed placeholder rows
        <div key={i} className="ak-rrow ak-rskel" aria-hidden="true" style={{ animationDelay: `${i * 120}ms` }}>
          <i className="ak-rdot" />
          <span className="min-w-0">
            <i className="ak-rbar" style={{ width: `${name}%` }} />
            <i className="ak-rbar ak-rbar-s" style={{ width: `${branch}%` }} />
          </span>
          <span className="ak-rtags">
            {Array.from({ length: tags }, (_, t) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: fixed placeholder tags
              <i key={t} className="ak-rbar ak-rbar-t" />
            ))}
          </span>
        </div>
      ))}
    </section>
  );
}

export function Repos({ maxHeight }: { maxHeight?: number }) {
  const { data, refresh } = useCached<ReposOverview>("repos", () => api.reposOverview(false));
  const [checked, setChecked] = useState(() => Date.now());
  const now = useNow(10_000);
  const [open, setOpen] = useState<string | null>(null);
  const [note, setNote] = useState<Record<string, string>>({});
  // biome-ignore lint/correctness/useExhaustiveDependencies: a new answer means a fresh check
  useEffect(() => setChecked(Date.now()), [data]);
  if (!data) return <ReposLoading />;
  if (data.repos.length === 0) {
    return <p className="ak-rempty">No repos yet. Clone one or open a folder with git, and it shows here.</p>;
  }
  const say = (path: string, text: string) => setNote((n) => ({ ...n, [path]: text }));
  const pull = (r: RepoRow, stash: boolean) =>
    void api
      .repoPull(r.path, stash)
      .then((m) => {
        say(r.path, m);
        void refresh?.();
      })
      .catch((e: unknown) => say(r.path, String(e)));
  const via =
    data.via === "gh"
      ? "GitHub connected"
      : data.via === "composio"
        ? "GitHub through Composio"
        : "GitHub not connected";
  return (
    <section className="ak-repos" aria-label="Repos">
      <div className="ak-rh">
        <span>
          {data.repos.length} repo{data.repos.length === 1 ? "" : "s"} · {via}
        </span>
        <button
          type="button"
          className="ak-rcheck"
          title="Check now: fetch every repo and look for new commits and PRs"
          onClick={() => void api.reposOverview(true).then(() => refresh?.())}
        >
          checked {since(checked, now)}
        </button>
      </div>
      {data.via === "none" && <p className="ak-rnote">Sign in with gh auth login to see PRs and checks.</p>}
      <div className="ak-scroll ak-rlist" style={{ maxHeight: maxHeight ? maxHeight - 48 : undefined }}>
        {data.repos.map((r) => {
          const expanded = open === r.path;
          const failing = r.prs.find((p) => p.failing && p.mine) ?? r.prs.find((p) => p.failing);
          const review = r.prs.find((p) => p.reviewRequested);
          return (
            <div key={r.path} className="ak-repo" data-open={expanded}>
              <button
                type="button"
                className="ak-rrow"
                aria-expanded={expanded}
                onClick={() => setOpen(expanded ? null : r.path)}
              >
                <span
                  className="ak-rdot"
                  style={{ background: CI_COLOR[r.ci] }}
                  role="img"
                  aria-label={CI_LABEL[r.ci]}
                  title={CI_LABEL[r.ci]}
                />
                <span className="min-w-0">
                  <span className="ak-rn">{r.name}</span>
                  <span className="ak-rb">⎇ {r.branch}</span>
                </span>
                <Tags r={r} />
              </button>
              {expanded && (
                <div className="ak-rx ak-in">
                  {failing?.failing && (
                    <div className="ak-rci">
                      <span>
                        <b>CI failed</b> on PR #{failing.number} · {failing.failing[0]}
                      </span>
                      <div className="ak-rbtns">
                        <button
                          type="button"
                          className="ak-chip chip"
                          onClick={() => void api.aiOpenLink(failing.failing?.[1] ?? failing.url)}
                        >
                          Open logs
                        </button>
                        <button
                          type="button"
                          className="ak-chip primary chip"
                          onClick={() =>
                            void startSession(
                              "claude_code",
                              r.path,
                              `The "${failing.failing?.[0]}" check failed on PR #${failing.number} (${failing.branch}). Find out why from ${failing.failing?.[1] ?? failing.url} and fix it.`,
                              "edit",
                            ).then(() => setTab("agents"))
                          }
                        >
                          Ask Claude Code to fix
                        </button>
                      </div>
                    </div>
                  )}
                  {review && (
                    <button type="button" className="ak-rline" onClick={() => void api.aiOpenLink(review.url)}>
                      Review requested on #{review.number} · {review.title}
                    </button>
                  )}
                  {r.prs
                    .filter((p) => p !== failing && p !== review)
                    .map((p) => (
                      <button
                        key={p.number}
                        type="button"
                        className="ak-rline"
                        onClick={() => void api.aiOpenLink(p.url)}
                      >
                        #{p.number} {p.title}
                      </button>
                    ))}
                  <div className="ak-rbtns">
                    {r.behind > 0 && (
                      <button type="button" className="ak-chip chip" onClick={() => pull(r, r.changed > 0)}>
                        {r.changed > 0 ? "Stash, pull, put back" : "Pull"}
                      </button>
                    )}
                    <button type="button" className="ak-chip chip" onClick={() => void api.repoOpen(r.path)}>
                      Open in editor
                    </button>
                    {r.slug && (
                      <button
                        type="button"
                        className="ak-chip chip"
                        onClick={() => void api.aiOpenLink(`https://github.com/${r.slug}`)}
                      >
                        Open on GitHub
                      </button>
                    )}
                    <button type="button" className="ak-chip chip" onClick={() => startHere(r.path)}>
                      Start an agent here
                    </button>
                  </div>
                  {note[r.path] && <p className="ak-rnote">{note[r.path]}</p>}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </section>
  );
}
