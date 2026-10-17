"use client";

// One library of skills for Claude Code, Codex and the local model. Skills
// are copied, never run. Skills with scripts are marked and left unticked,
// and a link or an update shows what would change before anything does.

import { useCallback, useEffect, useState } from "react";
import { api } from "@/lib/bridge";
import type { FoundSkill, LibrarySkill, LinkPreview, SkillOrigin, SkillTarget } from "@/lib/types";
import { Button, Segmented } from "./ui";

const ORIGIN: Record<SkillOrigin, string> = {
  claude: "Claude Code",
  claude_plugin: "Claude plugin",
  codex: "Codex",
  duskie: "Duskie",
  url: "Link",
};

const TARGETS: [SkillTarget, string][] = [
  ["claude", "Claude Code"],
  ["codex", "Codex"],
  ["local", "Local model"],
];

function Badge({ children }: { children: string }) {
  return <span className="rounded-full border border-(--border) px-1.5 text-[11px] text-(--muted)">{children}</span>;
}

/** Ticked by default: everything without scripts. */
function preselect<T extends { name: string; hasScripts: boolean }>(list: T[], keep: (t: T) => boolean = () => true) {
  return new Set(list.filter((s) => !s.hasScripts && keep(s)).map((s) => s.name));
}

function Pick({
  name,
  description,
  badges,
  checked,
  onChange,
  children,
}: {
  name: string;
  description: string;
  badges: string[];
  checked: boolean;
  onChange: (on: boolean) => void;
  children?: React.ReactNode;
}) {
  return (
    <li className="py-2">
      <label className="flex cursor-pointer items-start gap-3">
        <input
          type="checkbox"
          className="mt-1"
          checked={checked}
          onChange={(e) => onChange(e.target.checked)}
          aria-label={`Add ${name}`}
        />
        <span className="min-w-0 text-[13px]">
          <span className="flex flex-wrap items-center gap-2 font-medium">
            {name}
            {badges.map((b) => (
              <Badge key={b}>{b}</Badge>
            ))}
          </span>
          <span className="block truncate text-[12px] text-(--muted)">{description}</span>
          {children}
        </span>
      </label>
    </li>
  );
}

export function SkillLibrary({ onError }: { onError: (e: string) => void }) {
  const [found, setFound] = useState<FoundSkill[]>([]);
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [lib, setLib] = useState<LibrarySkill[]>([]);
  const [note, setNote] = useState("");
  const [link, setLink] = useState("");
  const [busy, setBusy] = useState(false);
  const [preview, setPreview] = useState<LinkPreview | null>(null);
  const [linkChosen, setLinkChosen] = useState<Set<string>>(new Set());
  const fail = useCallback((e: unknown) => onError(String(e)), [onError]);

  const review = useCallback(
    () =>
      api
        .libraryReview()
        .then((f) => {
          setFound(f);
          setChosen(preselect(f));
        })
        .catch(fail),
    [fail],
  );
  useEffect(() => {
    api.libraryList().then(setLib).catch(fail);
    void review();
  }, [fail, review]);

  const toggle = (set: Set<string>, name: string, on: boolean) => {
    const next = new Set(set);
    if (on) next.add(name);
    else next.delete(name);
    return next;
  };

  const addFound = () =>
    api
      .libraryImport([...chosen])
      .then((l) => {
        setLib(l);
        return review();
      })
      .catch(fail);

  const fetchLink = (url: string) => {
    setBusy(true);
    api
      .libraryFetchUrl(url)
      .then((p) => {
        setPreview(p);
        setLinkChosen(preselect(p.skills, (s) => s.status !== "same"));
      })
      .catch(fail)
      .finally(() => setBusy(false));
  };

  const addLinked = () => {
    if (!preview) return;
    api
      .libraryInstallFetched(preview.url, preview.commit, [...linkChosen])
      .then((l) => {
        setLib(l);
        setPreview(null);
        setLink("");
      })
      .catch(fail);
  };

  const sync = () =>
    api
      .librarySync()
      .then((r) => {
        const parts = [`${r.written.length} written`];
        if (r.conflicts.length) parts.push(`${r.conflicts.length} changed by hand, left alone`);
        if (r.backups.length) parts.push(`${r.backups.length} backed up`);
        setNote(parts.join(", "));
      })
      .catch(fail);

  return (
    <div className="flex flex-col gap-3">
      <form
        className="flex items-center gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (link.trim()) fetchLink(link.trim());
        }}
      >
        <input
          aria-label="GitHub link to a repo or skill folder"
          className="min-w-0 flex-1 rounded-md border border-(--border) bg-transparent px-2 py-1 text-[13px]"
          placeholder="Paste a GitHub link to a repo or skill folder"
          value={link}
          onChange={(e) => setLink(e.target.value)}
        />
        <Button small disabled={busy || !link.trim()} onClick={() => fetchLink(link.trim())}>
          {busy ? "Looking" : "Look at link"}
        </Button>
      </form>

      {preview && (
        <div className="flex flex-col gap-2 rounded-lg border border-(--border) p-3">
          <span className="text-[13px]">
            At commit <span className="font-mono">{preview.commit.slice(0, 7)}</span>. Nothing runs when you add.
          </span>
          <ul className="flex flex-col divide-y divide-(--border)">
            {preview.skills.map((s) => (
              <Pick
                key={s.name}
                name={s.name}
                description={s.description}
                badges={[
                  s.status === "new" ? "New" : s.status === "changed" ? "Update" : "Same as yours",
                  `${s.files} file${s.files === 1 ? "" : "s"}, ${Math.max(1, Math.round(s.bytes / 1024))} KB`,
                  ...(s.hasScripts ? ["Has scripts"] : []),
                ]}
                checked={linkChosen.has(s.name)}
                onChange={(on) => setLinkChosen((c) => toggle(c, s.name, on))}
              >
                {s.diff.length > 0 && (
                  <span className="mt-1 block font-mono text-[11px] text-(--muted)">{s.diff.join(", ")}</span>
                )}
              </Pick>
            ))}
          </ul>
          <div className="flex gap-2">
            <Button small primary disabled={linkChosen.size === 0} onClick={addLinked}>
              Add selected
            </Button>
            <Button small onClick={() => setPreview(null)}>
              Cancel
            </Button>
          </div>
        </div>
      )}

      {found.length > 0 && (
        <div className="flex flex-col gap-2">
          <div className="flex items-center justify-between gap-3">
            <span className="text-[13px]">
              {found.length} skill{found.length === 1 ? "" : "s"} found on this PC
            </span>
            <Button small disabled={chosen.size === 0} onClick={() => void addFound()}>
              Add selected
            </Button>
          </div>
          <ul className="flex flex-col divide-y divide-(--border)">
            {found.map((f) => (
              <Pick
                key={f.name}
                name={f.name}
                description={f.description}
                badges={[ORIGIN[f.origin], ...(f.hasScripts ? ["Has scripts"] : [])]}
                checked={chosen.has(f.name)}
                onChange={(on) => setChosen((c) => toggle(c, f.name, on))}
              />
            ))}
          </ul>
        </div>
      )}

      {lib.length === 0 ? (
        <p className="text-[13px] text-(--muted)">No skills in your library yet.</p>
      ) : (
        <ul className="flex flex-col divide-y divide-(--border)">
          {lib.map((s) => (
            <li key={s.name} className="flex flex-col gap-2 py-2">
              <div className="flex items-center justify-between gap-4">
                <span className="min-w-0 text-[13px]">
                  <span className="flex items-center gap-2 font-medium">
                    {s.name} <Badge>{ORIGIN[s.origin]}</Badge>
                    {s.hasScripts && <Badge>Has scripts</Badge>}
                  </span>
                  <span className="block truncate text-[12px] text-(--muted)">
                    {s.description}
                    {s.commit ? ` (pinned at ${s.commit.slice(0, 7)})` : ""}
                  </span>
                </span>
                <span className="flex gap-2">
                  {s.url && (
                    <Button small disabled={busy} onClick={() => fetchLink(s.url ?? "")}>
                      Check for update
                    </Button>
                  )}
                  <Button small onClick={() => api.libraryRemove(s.name).then(setLib).catch(fail)}>
                    Remove
                  </Button>
                </span>
              </div>
              <div className="flex flex-wrap gap-3">
                {TARGETS.map(([t, label]) => (
                  <Segmented
                    key={t}
                    label={label}
                    value={s.targets[t] === false ? "off" : "on"}
                    options={[
                      ["on", `${label} on`],
                      ["off", "Off"],
                    ]}
                    onChange={(v) =>
                      api
                        .librarySetTarget(s.name, t, v === "on")
                        .then(setLib)
                        .catch(fail)
                    }
                  />
                ))}
              </div>
            </li>
          ))}
        </ul>
      )}
      {lib.length > 0 && (
        <div className="flex items-center gap-3">
          <Button small onClick={sync}>
            Sync now
          </Button>
          {note && <span className="text-[12px] text-(--muted)">{note}</span>}
        </div>
      )}
    </div>
  );
}
