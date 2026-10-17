"use client";

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { friendlyError } from "@/lib/friendly";
import { useScrollEdge } from "@/lib/hooks";
import { useDuskie } from "@/lib/store";
import { Ic, type IcName } from "./Icon";
import { AiTab } from "./settings/AiTab";
import { AppearanceTab } from "./settings/AppearanceTab";
import { ConnectionsTab } from "./settings/ConnectionsTab";
import { HomeTab } from "./settings/HomeTab";
import { MemoryTab } from "./settings/MemoryTab";
import { PrivacyTab } from "./settings/PrivacyTab";
import { ShortcutsTab } from "./settings/ShortcutsTab";
import { SkillsTab } from "./settings/SkillsTab";
import { StorageTab } from "./settings/StorageTab";
import { SettingsQuery } from "./settings/ui";

/** Design layout: the sections grouped in a sidebar, with a short line on each. */
const GROUPS: { label: string; items: { id: SettingsTab; label: string; icon: IcName; blurb: string }[] }[] = [
  {
    label: "Duskie",
    items: [
      {
        id: "home",
        label: "General",
        icon: "home",
        blurb: "How Duskie starts, how your day is shaped, and when it stays quiet.",
      },
      {
        id: "appearance",
        label: "Appearance",
        icon: "palette",
        blurb: "The look of Duskie, the island and the wash behind it.",
      },
    ],
  },
  {
    label: "Intelligence",
    items: [
      { id: "ai", label: "AI models", icon: "cpu", blurb: "Which brain answers, and which one writes code." },
      { id: "memory", label: "Memory", icon: "brain", blurb: "What Duskie knows about you. All of it is editable." },
      {
        id: "skills",
        label: "Skills and automations",
        icon: "bolt",
        blurb: "What Duskie offers, and what it now does on its own.",
      },
    ],
  },
  {
    label: "Connections",
    items: [{ id: "connections", label: "Apps", icon: "plug", blurb: "Accounts Duskie can read from and act in." }],
  },
  {
    label: "System",
    items: [
      { id: "shortcuts", label: "Shortcuts", icon: "key", blurb: "Click a shortcut, then press the new keys." },
      { id: "privacy", label: "Privacy", icon: "shield", blurb: "What Duskie notices, and what it never looks at." },
      { id: "storage", label: "Storage", icon: "disk", blurb: "What takes space on this PC." },
    ],
  },
];

export const SETTINGS_TABS = [
  { id: "home", label: "Home" },
  { id: "appearance", label: "Appearance" },
  { id: "ai", label: "AI" },
  { id: "connections", label: "Apps" },
  { id: "memory", label: "Memory" },
  { id: "shortcuts", label: "Shortcuts" },
  { id: "storage", label: "Storage" },
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

/** Settings, shown inside the island: tabs and a search over all of them. */
export function SettingsPanel() {
  const ready = useDuskie((s) => s.ready);
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
  const wantedTab = useDuskie((s) => s.ask?.settingsTab);
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

  const body = (
    <SettingsQuery.Provider value={query.trim()}>
      <div
        ref={scrollerRef}
        onScroll={(e) => {
          scrolled.current = e.currentTarget.scrollTop;
        }}
        className="settings-scroll flex min-h-0 flex-auto flex-col gap-5 overflow-y-auto px-0.5 pb-3"
      >
        {show("home") && <HomeTab onError={report} onOpenTab={open} />}
        {show("appearance") && <AppearanceTab onError={report} />}
        {show("ai") && <AiTab onError={report} />}
        {show("connections") && <ConnectionsTab onError={report} />}
        {show("privacy") && <PrivacyTab onError={report} />}
        {show("memory") && <MemoryTab onError={report} />}
        {show("shortcuts") && <ShortcutsTab onError={report} />}
        {show("storage") && <StorageTab onError={report} />}
        {show("skills") && <SkillsTab onError={report} />}
      </div>
    </SettingsQuery.Provider>
  );
  const alert = error && (
    <p
      role="alert"
      className="mb-2 flex shrink-0 items-start justify-between gap-3 rounded-lg border border-red-500/40 bg-red-500/10 px-3 py-2 text-sm text-red-300"
    >
      <span className="min-w-0 break-words">{error}</span>
      <button type="button" onClick={() => setError(null)} aria-label="Hide" className="shrink-0 opacity-70">
        ×
      </button>
    </p>
  );

  const current = GROUPS.flatMap((g) => g.items).find((i) => i.id === tab);
  return (
    <div className="island-settings st-split flex min-h-0 flex-auto">
      <nav aria-label="Settings sections" className="st-side">
        <label className="st-search">
          <Ic name="search" size={14} />
          <input
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search settings"
            aria-label="Search settings"
            spellCheck={false}
          />
          <kbd>/</kbd>
        </label>
        {GROUPS.map((g) => (
          <div key={g.label} className="flex flex-col">
            <p className="st-group">{g.label}</p>
            {g.items.map((i) => (
              <button
                key={i.id}
                type="button"
                className="st-item"
                aria-current={!searching && tab === i.id ? "page" : undefined}
                onClick={() => {
                  setQuery("");
                  pick(i.id);
                }}
              >
                <Ic name={i.icon} size={14} />
                {i.label}
              </button>
            ))}
          </div>
        ))}
      </nav>
      <div className="flex min-h-0 min-w-0 flex-auto flex-col">
        <div className="shrink-0 pb-3">
          <p className="st-title">{searching ? "Search" : current?.label}</p>
          <p className="st-blurb">{searching ? `Settings matching "${query.trim()}"` : current?.blurb}</p>
        </div>
        {alert}
        {body}
      </div>
    </div>
  );
}
