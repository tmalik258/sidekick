"use client";

// Browser extension one-click helper: stage folder, copy path, open extensions page.

import { useEffect, useState } from "react";
import { api } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import { installExtension } from "@/lib/store";
import type { BrowserStatus, ExtensionGuide } from "@/lib/types";

/** Shared by welcome and Settings > Browser. */
export function BrowserInstallPanel({ onDone }: { onDone?: () => void }) {
  const [busy, setBusy] = useState<string | null>(null);
  const [guide, setGuide] = useState<ExtensionGuide | null>(null);
  const [error, setError] = useState<string | null>(null);

  const { data: browsers } = useCached<BrowserStatus[]>("browsers", api.browsersStatus);
  useEffect(() => {
    if (!onDone) return;
    const id = setInterval(() => void onDone(), 6000);
    return () => clearInterval(id);
  }, [onDone]);

  const install = (id: string, name: string) => {
    setBusy(id);
    setError(null);
    // In the welcome the steps stay inline; elsewhere the island keeps them.
    void installExtension(id, name, { shrink: !onDone })
      .then((g) => setGuide(g))
      .catch((e) => setError(String(e)))
      .finally(() => setBusy(null));
  };

  const list = browsers && browsers.length > 0 ? browsers : [{ id: "chrome", name: "Chrome", connected: false }];

  return (
    <div className="flex flex-col gap-2 text-[12px] leading-relaxed text-(--muted)">
      <p>
        Sidekick opens your browser and gives you the two things to paste. You still click Load unpacked once (browsers
        do not allow silent installs).
      </p>
      <div className="flex flex-wrap gap-1.5">
        {list.map((b) => (
          <button
            key={b.id}
            type="button"
            disabled={busy !== null}
            onClick={() => install(b.id, b.name)}
            className="chip self-start rounded-full bg-white px-2.5 py-1 text-[12px] font-medium text-black hover:bg-white/90 disabled:opacity-50"
          >
            {busy === b.id ? "Opening..." : b.connected ? `${b.name} (paired)` : `Set up ${b.name}`}
          </button>
        ))}
      </div>
      {guide && (
        <ol className="list-decimal space-y-1 pl-4 text-[11px] text-(--muted)">
          {guide.steps.map((s) => (
            <li key={s}>{s}</li>
          ))}
        </ol>
      )}
      {guide && (
        <div className="flex flex-wrap gap-1.5">
          <CopyButton label="Copy extensions address" text={guide.page} />
          <CopyButton label="Copy folder path" text={guide.copied} />
        </div>
      )}
      {error && <p className="text-[11px] text-[#ff453a]">{error}</p>}
    </div>
  );
}

function CopyButton({ label, text }: { label: string; text: string }) {
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
      className="chip rounded-full bg-white/[0.1] px-2.5 py-1 text-[12px] text-white/90 hover:bg-white/[0.16]"
    >
      {copied ? "Copied" : label}
    </button>
  );
}
