"use client";

// The last welcome step: voice, quiet Windows, and launch on login.

import { useState } from "react";
import { api } from "@/lib/bridge";
import { useDnd } from "@/lib/hooks";
import { updateSettings, useAssistantName, useSidekick } from "@/lib/store";
import { WelcomeChoice } from "./WelcomeChoice";

export function Extras() {
  const settings = useSidekick((s) => s.settings);
  const dnd = useDnd({ autoEnable: true });
  const name = useAssistantName();
  return (
    <div className="flex flex-col gap-2.5 text-[13px]">
      <WelcomeChoice
        title="Talk to me"
        hint={`Say "Hey ${name}". Downloads about 205 MB of speech models once; all on this PC.`}
        on={settings.voice.enabled}
        onChange={(enabled) => {
          void updateSettings({ voice: { ...settings.voice, enabled } });
          if (enabled) void api.voiceDownload();
        }}
      />
      <WelcomeChoice
        title="Keep Windows quiet"
        hint={
          dnd.busy ? "Switching..." : "Do Not Disturb is on — I show what matters up here, not two alerts for one ping."
        }
        on={dnd.on}
        onChange={(want) => {
          if (!dnd.busy) dnd.set(want);
        }}
      />
      <WelcomeChoice
        title="Launch on login"
        hint="On by default — so I'm here when you sit down."
        on={settings.launchAtLogin}
        onChange={(launchAtLogin) => void updateSettings({ launchAtLogin })}
      />
      <Rename />
      <p className="px-1 pt-1 text-[12px] leading-relaxed text-[rgb(235_235_245/0.62)]">
        Come back in Settings anytime — for these, or what's left to set up.
      </p>
    </div>
  );
}

/** Call me something else: the name, and "Hey <name>" to talk. */
function Rename() {
  const saved = useSidekick((s) => s.settings.assistantName);
  const [draft, setDraft] = useState(saved);
  const save = () => {
    const v = draft.trim() || "Sidekick";
    if (v !== saved) void updateSettings({ assistantName: v });
  };
  return (
    <label className="flex items-center gap-3 rounded-2xl bg-white/6 px-3.5 py-2.5">
      <span className="min-w-0 flex-1">
        <span className="block font-medium text-white">Call me</span>
        <span className="block text-[11.5px] text-[rgb(235_235_245/0.62)]">
          Rename me if you like. Two syllables work best.
        </span>
      </span>
      <input
        value={draft}
        aria-label="Assistant name"
        spellCheck={false}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={save}
        onKeyDown={(e) => e.key === "Enter" && save()}
        className="w-28 rounded-lg bg-white/8 px-2.5 py-1.5 text-[13px] text-white outline-none focus:ring-1 focus:ring-[#0a84ff]"
      />
    </label>
  );
}
