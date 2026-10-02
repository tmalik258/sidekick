"use client";

// Browser extension one-click helper: stage folder, copy path, open extensions page.

import { useEffect, useState } from "react";
import { api } from "@/lib/bridge";
import type { BrowserStatus, ExtensionGuide } from "@/lib/types";

/** Shared by welcome and Settings > Browser. */
export function BrowserInstallPanel({ onDone }: { onDone?: () => void }) {
  const [browsers, setBrowsers] = useState<BrowserStatus[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [guide, setGuide] = useState<ExtensionGuide | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void api.browsersStatus().then(setBrowsers);
    if (!onDone) return;
    const id = setInterval(() => void onDone(), 6000);
    return () => clearInterval(id);
  }, [onDone]);

  const install = (id: string) => {
    setBusy(id);
    setError(null);
    void api
      .extensionInstall(id)
      .then((g) => {
        setGuide(g);
        onDone?.();
      })
      .catch((e) => setError(String(e)))
      .finally(() => setBusy(null));
  };

  const list = browsers.length > 0 ? browsers : [{ id: "chrome", name: "Chrome", connected: false }];

  return (
    <div className="flex flex-col gap-2 text-[12px] leading-relaxed text-(--muted)">
      <p>
        Sidekick copies the extension and opens the browser&apos;s extensions page with the folder path ready. You still
        click Load unpacked once (browsers do not allow silent installs).
      </p>
      <div className="flex flex-wrap gap-1.5">
        {list.map((b) => (
          <button
            key={b.id}
            type="button"
            disabled={busy !== null}
            onClick={() => install(b.id)}
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
          <li>
            Path on clipboard: <code className="rounded bg-black/30 px-1 font-mono text-[11px]">{guide.copied}</code>
          </li>
        </ol>
      )}
      {error && <p className="text-[11px] text-[#ff453a]">{error}</p>}
    </div>
  );
}
