"use client";

// Compact island state while the voice (and other speech models) are still
// arriving. Shown until onboarding can speak; never a blank idle orb.

import type { VoiceStatus } from "@/lib/types";

export function PreparingVoice({ voiceStatus }: { voiceStatus: VoiceStatus | null }) {
  const models = voiceStatus?.models ?? [
    { id: "voice", label: "Supertonic voice", size: 0, installed: false },
    { id: "wake", label: "Wake word", size: 0, installed: false },
    { id: "speech", label: "Speech to text", size: 0, installed: false },
  ];
  // Show the voice first: greeting depends on it.
  const order = ["voice", "wake", "speech"];
  const rows = order.map((id) => models.find((m) => m.id === id)).filter((m): m is NonNullable<typeof m> => Boolean(m));
  return (
    <div className="flex flex-col gap-2 pb-1">
      <p className="font-display text-[15px] leading-5 font-semibold tracking-[-0.015em] text-white">Preparing voice</p>
      <ul className="flex flex-col gap-1.5">
        {rows.map((m) => (
          <li key={m.id} className="flex items-center gap-2.5 text-[12.5px] text-white/70">
            <span
              role="img"
              aria-label={m.installed ? "Ready" : "Preparing"}
              className={`size-2 shrink-0 rounded-full ${m.installed ? "bg-[#30d158]" : "animate-pulse bg-amber-400"}`}
            />
            <span className="truncate text-white/85">{m.label}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
