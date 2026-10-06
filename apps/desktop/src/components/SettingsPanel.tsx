"use client";

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { friendlyError } from "@/lib/friendly";
import { useScrollEdge } from "@/lib/hooks";
import { useSidekick } from "@/lib/store";
import { AiTab } from "./settings/AiTab";
import { AppearanceTab } from "./settings/AppearanceTab";
import { ConnectionsTab } from "./settings/ConnectionsTab";
import { HomeTab } from "./settings/HomeTab";
import { PrivacyTab } from "./settings/PrivacyTab";
import { SkillsTab } from "./settings/SkillsTab";
import { SettingsQuery } from "./settings/ui";

export const SETTINGS_TABS = [
  { id: "home", label: "Home" },
  { id: "appearance", label: "Appearance" },
  { id: "ai", label: "AI" },
  { id: "connections", label: "Apps" },
  { id: "privacy", label: "Privacy" },
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
  apps: "connections",
  calendar: "connections",
  search: "privacy",
  sensors: "privacy",
};

function tabFor(id: string | undefined): SettingsTab | null {
  if (!id) return null;
  if (SETTINGS_TABS.some((t) => t.id === id)) return id as SettingsTab;
  return MOVED[id] ?? null;
}

/** How long Settings reopens where it was left (tab and scroll). */
const REMEMBER_MS = 60_000;
/** Where Settings was when it last closed. Lives outside the component,
 * which unmounts when the island closes. */
let leftAt: { tab: SettingsTab; scroll: number; at: number } | null = null;

function recent() {
  return leftAt && Date.now() - leftAt.at < REMEMBER_MS ? leftAt : null;
}

/** Settings, shown inside the island: five tabs and a search over all of them. */
export function SettingsPanel() {
  const ready = useSidekick((s) => s.ready);
  const [tab, setTab] = useState<SettingsTab>(() => recent()?.tab ?? "home");
  const [query, setQuery] = useState("");
  const scroller = useRef<HTMLDivElement | null>(null);
  const edges = useScrollEdge();
  const scrollerRef = useCallback(
    (el: HTMLDivElement | null) => {
      scroller.current = el;
      edges(el);
    },
    [edges],
  );
  const tabRef = useRef(tab);
  tabRef.current = tab;
  // Kept as it scrolls: the element is already gone when unmounting.
  const scrolled = useRef(0);
  const wantedTab = useSidekick((s) => s.ask?.settingsTab);
  // A new tab starts at its top.
  const pick = useCallback((t: SettingsTab) => {
    setTab(t);
    scroller.current?.scrollTo({ top: 0 });
  }, []);

  // Opened at a given tab (back from connecting an app): that tab, at its top.
  useEffect(() => {
    const t = tabFor(wantedTab);
    if (t) pick(t);
  }, [wantedTab, pick]);

  // Reopened within a minute: back to the same spot. The tab's content
  // loads in pieces, so keep trying for a moment until it is tall enough.
  useLayoutEffect(() => {
    if (!ready) return;
    const back = recent();
    if (!back || back.tab !== tabRef.current || back.scroll <= 0) return;
    let frames = 0;
    let raf = 0;
    const restore = () => {
      const el = scroller.current;
      if (!el) return;
      el.scrollTop = back.scroll;
      if (Math.abs(el.scrollTop - back.scroll) > 2 && frames++ < 60) raf = requestAnimationFrame(restore);
    };
    restore();
    return () => cancelAnimationFrame(raf);
  }, [ready]);

  // Remember where it was on close.
  useEffect(
    () => () => {
      leftAt = { tab: tabRef.current, scroll: scrolled.current, at: Date.now() };
    },
    [],
  );
  const [error, setError] = useState<string | null>(null);
  // Errors read as what to do next, not raw messages.
  const report = useCallback((e: string) => setError(friendlyError(e)), []);
  const open = useCallback(
    (id: string) => {
      const t = tabFor(id);
      if (t) {
        setQuery("");
        pick(t);
      }
    },
    [pick],
  );

  if (!ready) return null;
  const searching = query.trim() !== "";
  const show = (id: SettingsTab) => searching || tab === id;

  return (
    <div className="island-settings flex min-h-0 flex-auto flex-col">
      <div className="flex shrink-0 flex-col gap-2 pb-2.5">
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
                onClick={() => pick(t.id)}
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
          className="mb-2 flex shrink-0 items-start justify-between gap-3 rounded-lg border border-red-500/40 bg-red-500/10 px-3 py-2 text-sm text-red-300"
        >
          <span className="min-w-0 break-words">{error}</span>
          <button type="button" onClick={() => setError(null)} aria-label="Hide" className="shrink-0 opacity-70">
            ×
          </button>
        </p>
      )}

      <SettingsQuery.Provider value={query.trim()}>
        <div
          ref={scrollerRef}
          onScroll={(e) => {
            scrolled.current = e.currentTarget.scrollTop;
          }}
          className="settings-scroll -mr-3 flex min-h-0 flex-auto flex-col gap-5 overflow-y-auto pr-3 pl-0.5 pb-3"
        >
          {show("home") && <HomeTab onError={report} onOpenTab={open} />}
          {show("appearance") && <AppearanceTab onError={report} />}
          {show("ai") && <AiTab onError={report} />}
          {show("connections") && <ConnectionsTab onError={report} />}
          {show("privacy") && <PrivacyTab onError={report} />}
          {show("skills") && <SkillsTab onError={report} />}
        </div>
      </SettingsQuery.Provider>
    </div>
  );
}
