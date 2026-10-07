"use client";

// The last welcome step: voice, quiet Windows, and launch on login.

import { api } from "@/lib/bridge";
import { useDnd } from "@/lib/hooks";
import { updateSettings, useSidekick } from "@/lib/store";
import { WelcomeChoice } from "./WelcomeChoice";

export function Extras() {
  const settings = useSidekick((s) => s.settings);
  const dnd = useDnd({ autoEnable: true });
  return (
    <div className="flex flex-col gap-2.5 text-[13px]">
      <WelcomeChoice
        title="Talk to me"
        hint='Say "Hey Sidekick". Downloads about 205 MB of speech models once; all on this PC.'
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
      <p className="px-1 pt-1 text-[12px] leading-relaxed text-[rgb(235_235_245/0.62)]">
        Come back in Settings anytime — for these, or what's left to set up.
      </p>
    </div>
  );
}
