"use client";

import { useCallback, useEffect, useState } from "react";
import { useSidekick } from "@/lib/store";
import { AiTab } from "./settings/AiTab";
import { ConnectionsTab } from "./settings/ConnectionsTab";
import { HomeTab } from "./settings/HomeTab";
import { PrivacyTab } from "./settings/PrivacyTab";
import { SkillsTab } from "./settings/SkillsTab";
import { SettingsQuery } from "./settings/ui";

export const SETTINGS_TABS = [
  { id: "home", label: "Home" },
  { id: "ai", label: "AI" },
  { id: "connections", label: "Connections" },
  { id: "privacy", label: "Privacy and data" },
  { id: "skills", label: "Skills" },
] as const;
export type SettingsTab = (typeof SETTINGS_TABS)[number]["id"];

/** Old tab ids (links from setup items and older builds) to the new ones. */
const MOVED: Record<string, SettingsTab> = {
  general: "home",
  setup: "home",
  today: "home",
  about: "home",
  voice: "ai",
  browser: "connections",
  search: "privacy",
  sensors: "privacy",
};

function tabFor(id: string | undefined): SettingsTab | null {
  if (!id) return null;
  if (SETTINGS_TABS.some((t) => t.id === id)) return id as SettingsTab;
  return MOVED[id] ?? null;
}

/** Settings, shown inside the island: five tabs and a search over all of them. */
export function SettingsPanel() {
  const ready = useSidekick((s) => s.ready);
  const [tab, setTab] = useState<SettingsTab>("home");
  const [query, setQuery] = useState("");
  const wantedTab = useSidekick((s) => s.ask?.settingsTab);
  useEffect(() => {
    const t = tabFor(wantedTab);
    if (t) setTab(t);
  }, [wantedTab]);
  const [error, setError] = useState<string | null>(null);
  const open = useCallback((id: string) => {
    const t = tabFor(id);
    if (t) {
      setQuery("");
      setTab(t);
    }
  }, []);

  if (!ready) return null;
  const searching = query.trim() !== "";
  const show = (id: SettingsTab) => searching || tab === id;

  return (
    <div className="island-settings flex flex-col">
      <div className="flex flex-col gap-2 pb-2.5">
        <input
          type="search"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search settings"
          aria-label="Search settings"
          spellCheck={false}
          className="w-full rounded-lg bg-white/[0.08] px-3 py-1.5 text-[13px] text-white placeholder:text-white/40 outline-none focus:bg-white/[0.12]"
        />
        {!searching && (
          <nav aria-label="Settings sections" className="no-scrollbar -mx-1 flex gap-1 overflow-x-auto px-1">
            {SETTINGS_TABS.map((t) => (
              <button
                key={t.id}
                type="button"
                aria-pressed={tab === t.id}
                onClick={() => setTab(t.id)}
                className={`chip shrink-0 rounded-full px-3 py-1 text-[12.5px] font-medium ${
                  tab === t.id
                    ? "bg-white text-black"
                    : "bg-white/[0.08] text-[rgb(235_235_245/0.7)] hover:bg-white/[0.14]"
                }`}
              >
                {t.label}
              </button>
            ))}
          </nav>
        )}
      </div>

      {error && (
        <p
          role="alert"
          className="mb-2 flex items-start justify-between gap-3 rounded-lg border border-red-500/40 bg-red-500/10 px-3 py-2 text-sm text-red-300"
        >
          <span className="min-w-0 break-words">{error}</span>
          <button type="button" onClick={() => setError(null)} aria-label="Hide" className="shrink-0 opacity-70">
            ×
          </button>
        </p>
      )}

      <SettingsQuery.Provider value={query.trim()}>
        <div className="settings-scroll -mr-3 flex max-h-[430px] flex-col gap-5 overflow-y-auto pr-3 pl-0.5 pb-3">
          {show("home") && <HomeTab onError={setError} onOpenTab={open} />}
          {show("ai") && <AiTab onError={setError} />}
          {show("connections") && <ConnectionsTab onError={setError} />}
          {show("privacy") && <PrivacyTab onError={setError} />}
          {show("skills") && <SkillsTab onError={setError} />}
        </div>
      </SettingsQuery.Provider>
    </div>
  );
}
