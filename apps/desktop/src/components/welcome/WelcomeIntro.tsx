"use client";

// The first welcome step: what Sidekick is, in three short points.

import type { ReactNode } from "react";
import { useSidekick } from "@/lib/store";

export function WelcomeIntro() {
  const hotkey = useSidekick((s) => s.settings.paletteHotkey);
  const voiceStatus = useSidekick((s) => s.voiceStatus);
  const downloading = voiceStatus?.downloading ?? false;
  const missing = (voiceStatus?.missingBytes ?? 0) > 0;
  return (
    <div className="flex flex-col gap-3 text-[13.5px] leading-relaxed text-white/90">
      {(downloading || missing) && (
        <p className="rounded-2xl bg-white/[0.06] px-3.5 py-2.5 text-[12.5px] text-[rgb(235_235_245/0.7)]">
          {downloading
            ? "Getting my voice ready (speech models download once, about 205 MB)…"
            : "Speech models will download so I can talk with you."}
        </p>
      )}
      <ul className="flex flex-col gap-2">
        <Point title="Summon">
          <Kbd>{hotkey}</Kbd> or say "Hey Sidekick".
        </Point>
        <Point title="Local">What I see stays on this PC.</Point>
        <Point title="Consent">I act only when you say. Anything I make can be undone.</Point>
      </ul>
    </div>
  );
}

function Point({ title, children }: { title: string; children: ReactNode }) {
  return (
    <li className="rounded-2xl bg-white/6 px-3.5 py-2.5">
      <p className="font-medium text-white">{title}</p>
      <p className="text-[12.5px] text-[rgb(235_235_245/0.6)]">{children}</p>
    </li>
  );
}

function Kbd({ children }: { children: ReactNode }) {
  return <kbd className="rounded-md bg-white/15 px-1.5 py-0.5 font-sans text-[11.5px] text-white">{children}</kbd>;
}
