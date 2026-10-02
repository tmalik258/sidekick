"use client";

// While Sidekick waits on something you finish elsewhere (signing in, loading
// the extension, an install), the island keeps the steps on screen with copy
// buttons, so nothing disappears when Settings closes. It never blocks other
// apps: outside its own shape the island is click-through.

import { useState } from "react";
import { minimizeWaiting, stopWaiting, type Waiting } from "@/lib/store";

export function IslandGuide({ waiting }: { waiting: Waiting }) {
  return (
    <div className="flex flex-col">
      <div className="flex items-center gap-2.5">
        <span className="relative flex size-2.5 shrink-0" aria-hidden="true">
          <span className="absolute inline-flex size-full animate-ping rounded-full bg-[#0a84ff] opacity-60" />
          <span className="relative inline-flex size-2.5 rounded-full bg-[#0a84ff]" />
        </span>
        <p className="min-w-0 flex-1 truncate font-display text-[15px] leading-5 font-semibold tracking-[-0.015em] text-white">
          Waiting for {waiting.label}
        </p>
      </div>
      {waiting.steps && waiting.steps.length > 0 && (
        <ol className="mt-2 list-decimal space-y-1 pl-4 text-[13px] leading-snug text-[rgb(235_235_245/0.75)]">
          {waiting.steps.map((s) => (
            <li key={s}>{s}</li>
          ))}
        </ol>
      )}
      {waiting.copies && waiting.copies.length > 0 && (
        <div className="mt-2.5 flex flex-wrap gap-1.5">
          {waiting.copies.map((c) => (
            <CopyChip key={c.label} label={c.label} text={c.text} />
          ))}
        </div>
      )}
      <div className="mt-3 flex items-center gap-1.5">
        {waiting.again && (
          <button
            type="button"
            onClick={waiting.again}
            className="chip rounded-full bg-white px-3 py-1.5 text-[12.5px] font-medium text-black hover:bg-white/90"
          >
            Open again
          </button>
        )}
        <button
          type="button"
          onClick={() => minimizeWaiting(!waiting.minimized)}
          className="chip rounded-full bg-white/[0.12] px-3 py-1.5 text-[12.5px] font-medium text-white/90 hover:bg-white/[0.2]"
        >
          {waiting.minimized ? "Keep open" : "Minimize"}
        </button>
        <button
          type="button"
          onClick={stopWaiting}
          className="chip ml-auto rounded-full px-2.5 py-1.5 text-[12.5px] text-[rgb(235_235_245/0.6)] hover:text-white"
        >
          Cancel
        </button>
      </div>
    </div>
  );
}

function CopyChip({ label, text }: { label: string; text: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      title={text}
      onClick={() =>
        void navigator.clipboard
          .writeText(text)
          .then(() => {
            setCopied(true);
            setTimeout(() => setCopied(false), 1500);
          })
          .catch(() => undefined)
      }
      className="chip max-w-full truncate rounded-full bg-white/[0.08] px-3 py-1.5 text-left text-[12.5px] text-white/90 ring-1 ring-white/10 ring-inset hover:bg-white/[0.14]"
    >
      {copied ? "Copied" : label}
    </button>
  );
}
