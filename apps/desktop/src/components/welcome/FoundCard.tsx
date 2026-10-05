"use client";

// What Sidekick found on this PC — pick rows, one tap to apply.

import { useMemo, useState } from "react";
import { api } from "@/lib/bridge";
import { SETUP_STATUS_CACHE_KEY, useCached } from "@/lib/cache";
import { friendlyError } from "@/lib/friendly";
import { useSidekick } from "@/lib/store";
import type { Settings, SetupFound } from "@/lib/types";
import { SetupSpinner } from "../SetupChecklistRow";
import { WelcomeChoice, WelcomeChoiceSkeleton } from "./WelcomeChoice";

interface Pick {
  id: string;
  title: string;
  hint: string;
  on: boolean;
}

/** Only things not already applied — remount after Hide must not re-offer them. */
function picksFrom(f: SetupFound, off: Set<string>, settings: Settings): Pick[] {
  const list: Omit<Pick, "on">[] = [];
  const repos = f.codeFolders.reduce((n, c) => n + c.repos, 0);
  if (f.codeFolders.length > 0 && settings.codeFolders.length === 0) {
    const labels = f.codeFolders.map((c) => c.label).join(", ");
    list.push({
      id: "code",
      title: "Code",
      hint: repos > 0 ? `${labels} · ${repos} repos` : labels,
    });
  }
  if (f.searchFolders.length > 0 && settings.indexFolders.length === 0) {
    list.push({
      id: "search",
      title: "Search",
      hint: f.searchFolders.map((c) => c.label).join(", "),
    });
  }
  // Local chat / Ollama / vision stay in the checklist below — not here.
  if (f.claudeInstalled && !f.claudeHooks) {
    list.push({ id: "hooks", title: "Claude hooks", hint: "Hear when it finishes or asks" });
  }
  if (f.claudeInstalled && !f.claudeMcp) {
    list.push({ id: "mcp", title: "Claude tools", hint: "Let it use Sidekick" });
  }
  for (const item of f.installable.filter((i) => i.recommended && !i.done && i.runnable)) {
    list.push({ id: `install:${item.id}`, title: item.title, hint: item.why });
  }
  return list.map((p) => ({ ...p, on: !off.has(p.id) }));
}

function ctaLabel(selected: number, busy: boolean): string {
  if (busy) return "Setting up…";
  if (selected <= 1) return "Set up";
  return `Set up · ${selected}`;
}

export function FoundCard({ onDone }: { onDone: () => void }) {
  const settings = useSidekick((s) => s.settings);
  const { data: found, refresh } = useCached<SetupFound>("setup-detect", api.setupDetect);
  const { refresh: refreshStatus } = useCached(SETUP_STATUS_CACHE_KEY, api.setupStatus);
  // Unticked rows; everything else found is on. Kept apart from the data so a
  // refresh in the background never resets your choices.
  const [off, setOff] = useState<Set<string>>(() => new Set());
  const [busy, setBusy] = useState(false);
  const [success, setSuccess] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const picks = useMemo(
    () => (found ? picksFrom(found, off, settings) : []),
    [found, off, settings],
  );

  if (!found) {
    return (
      <div className="flex flex-col gap-2.5 text-[13px]">
        <p className="font-medium text-white">On this PC</p>
        <WelcomeChoiceSkeleton />
        <WelcomeChoiceSkeleton />
        <WelcomeChoiceSkeleton />
      </div>
    );
  }

  if (!success && picks.length === 0) return null;

  const selected = picks.filter((p) => p.on).length;
  const on = (id: string) => picks.some((p) => p.id === id && p.on);
  const showClaudeNote = on("hooks") || on("mcp");

  const toggle = (id: string, next: boolean) => {
    setOff((prev) => {
      const set = new Set(prev);
      if (next) set.delete(id);
      else set.add(id);
      return set;
    });
  };

  const apply = () => {
    setBusy(true);
    setError(null);
    void api
      .setupApply({
        codeFolders: on("code") ? found.codeFolders.map((f) => f.path) : [],
        searchFolders: on("search") ? found.searchFolders.map((f) => f.path) : [],
        chatModel: null,
        claudeHooks: on("hooks"),
        claudeMcp: on("mcp"),
        install: picks.filter((p) => p.on && p.id.startsWith("install:")).map((p) => p.id.slice(8)),
        voice: false,
        launchAtLogin: settings.launchAtLogin,
      })
      .then(async () => {
        setSuccess(true);
        // So Hide → open does not re-offer what we just applied.
        await Promise.all([refresh().catch(() => undefined), refreshStatus().catch(() => undefined)]);
        setTimeout(onDone, 1400);
      })
      .catch((e) => setError(friendlyError(e)))
      .finally(() => setBusy(false));
  };

  return (
    <div className="flex flex-col gap-2.5 text-[13px]">
      <p className="font-medium text-white">On this PC</p>
      {success ? (
        <div className="flex items-center gap-3 rounded-2xl bg-[#30d158]/12 px-3.5 py-3 ring-1 ring-inset ring-[#30d158]/35">
          <span className="grid size-7 shrink-0 place-items-center rounded-full bg-[#30d158] text-[13px] font-bold text-black">
            ✓
          </span>
          <div className="min-w-0">
            <p className="font-medium text-white">All set</p>
            <p className="text-[11.5px] text-[rgb(235_235_245/0.55)]">Ready on this PC.</p>
          </div>
        </div>
      ) : (
        <>
          <div className="flex flex-col gap-2">
            {picks.map((p) => (
              <WelcomeChoice
                key={p.id}
                title={p.title}
                hint={p.hint}
                on={p.on}
                onChange={(next) => toggle(p.id, next)}
              />
            ))}
          </div>
          <button
            type="button"
            disabled={busy || selected === 0}
            onClick={apply}
            className="chip inline-flex items-center gap-1.5 self-start rounded-full bg-white px-3.5 py-1.5 text-[13px] font-medium text-black hover:bg-white/90 disabled:opacity-50"
          >
            {busy && <SetupSpinner className="text-black/50" />}
            {ctaLabel(selected, busy)}
          </button>
          {error && <p className="text-[12px] text-[#ff453a]">{error}</p>}
          {showClaudeNote && (
            <p className="text-[11.5px] text-[rgb(235_235_245/0.5)]">Claude settings are backed up first.</p>
          )}
        </>
      )}
    </div>
  );
}
