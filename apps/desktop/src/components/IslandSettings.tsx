"use client";

// Settings mode: the island grows into the settings page, like Ask mode.
// Esc or clicking elsewhere closes it; Back returns to Ask.

import { useEffect } from "react";
import { api } from "@/lib/bridge";
import { PANEL_MAX_HEIGHT } from "@/lib/islandSize";
import { setAsk } from "@/lib/store";
import { ASK_ORB } from "./AskPanel";
import { SettingsPanel } from "./SettingsPanel";

export function IslandSettings() {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      const el = e.target as HTMLElement | null;
      // Esc in a text field undoes the edit instead.
      if (el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT")) return;
      void api.askClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="flex flex-col" style={{ maxHeight: PANEL_MAX_HEIGHT }}>
      <div className="mb-2.5 flex h-[30px] shrink-0 items-center gap-2" style={{ paddingLeft: ASK_ORB + 10 }}>
        <h1 className="flex-1 font-display text-[17px] font-semibold tracking-[-0.015em]">Settings</h1>
        <button
          type="button"
          onClick={() => setAsk({ view: "ask" })}
          className="chip rounded-full bg-white/[0.12] px-3 py-1 text-[12px] font-medium text-white/90 hover:bg-white/[0.2]"
        >
          Ask
        </button>
      </div>
      <SettingsPanel />
    </div>
  );
}
