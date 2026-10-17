"use client";

// Settings mode: the island grows into the settings page, under the same
// tab row as Ask. Esc or clicking elsewhere closes it; Ctrl Tab or a tab
// goes back to Ask, Agents, Repos or History.

import { useEffect } from "react";
import { type AskTab, activeCount, setTab, useAgents } from "@/lib/agents";
import { api } from "@/lib/bridge";
import { useAltHeld } from "@/lib/hooks";
import { setAsk, useDuskie } from "@/lib/store";
import { SHELL_SETTINGS } from "./island-layout";
import { nextTab, type PanelTab, PanelTabs } from "./PanelTabs";
import { SettingsPanel } from "./SettingsPanel";

function leave(tab: PanelTab) {
  if (tab === "settings") return;
  setTab(tab as AskTab);
  setAsk({ view: "ask" });
}

export function IslandSettings() {
  const coder = useDuskie((s) => s.settings.codes !== false);
  const working = useAgents((s) => activeCount(s.sessions));
  const alt = useAltHeld();

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key === "Tab") {
        e.preventDefault();
        leave(nextTab("settings", coder, e.shiftKey));
        return;
      }
      if (e.altKey && !e.ctrlKey && e.key.toLowerCase() === "h") {
        e.preventDefault();
        leave("history");
        return;
      }
      if (e.key !== "Escape") return;
      const el = e.target as HTMLElement | null;
      // Esc in a text field undoes the edit instead.
      if (el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT")) return;
      void api.askClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [coder]);

  return (
    <div
      className="ak !flex min-h-0 flex-col"
      style={{
        maxHeight: `${SHELL_SETTINGS.height}px`,
        width: SHELL_SETTINGS.width - 24,
      }}
    >
      <PanelTabs current="settings" onPick={leave} coder={coder} working={working} alt={alt} />
      <SettingsPanel />
    </div>
  );
}
