"use client";

// The last welcome step: voice and launch on login, each one tap.

import { api } from "@/lib/bridge";
import { updateSettings, useSidekick } from "@/lib/store";
import { WelcomeChoice } from "./WelcomeChoice";

export function Extras() {
  const settings = useSidekick((s) => s.settings);
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
        title="Launch on login"
        hint="On by default — so I'm here when you sit down."
        on={settings.launchAtLogin}
        onChange={(launchAtLogin) => void updateSettings({ launchAtLogin })}
      />
      <p className="px-1 pt-1 text-[12px] leading-relaxed text-[rgb(235_235_245/0.55)]">
        Come back in Settings anytime — for these, or what's left to set up.
      </p>
    </div>
  );
}