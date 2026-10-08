"use client";

// "Your AI" for people without Ollama or a coding plan: Gemini's free tier
// with one pasted key, said plainly that free cloud tiers may train on it.

import { useState } from "react";
import { useSidekick } from "@/lib/store";
import { CloudKey } from "../settings/AiTab";

export function FreeAi() {
  const codes = useSidekick((s) => s.settings.codes);
  const [error, setError] = useState<string | null>(null);
  return (
    <div className="flex flex-col gap-2 rounded-xl border border-white/10 p-3 text-[13px]">
      <p className="font-medium">{codes === false ? "Free AI in one step" : "No AI on this PC yet? Use Gemini free"}</p>
      <p className="text-[12px] text-white/60">
        Gemini is free with a Google account. Open Google AI Studio, press Create API key, and paste it here. Free cloud
        tiers may use your questions to train their models; a model on this PC (Ollama) stays the private option.
      </p>
      <CloudKey id="gemini" onError={setError} />
      {error && <p className="text-[12px] text-[#ff9f0a]">{error}</p>}
    </div>
  );
}
