"use client";

// The apps in a signed-in Composio account: connected ones as chips, and the
// ones Sidekick can use with an Add each. Settings > Apps and the welcome's
// Composio row both show this.

import { useEffect, useState } from "react";
import { api, EVENTS, listen } from "@/lib/bridge";
import { startWaiting, useSidekick } from "@/lib/store";
import type { ComposioStatus } from "@/lib/types";
import { Tip } from "../Tip";
import { Button } from "./ui";

export function ComposioApps({
  status,
  onError,
  resumeTab,
}: {
  status: ComposioStatus;
  onError: (e: string) => void;
  /** Where the island returns once an app finishes connecting. */
  resumeTab: string;
}) {
  const justDone = useSidekick((s) => s.justDone);
  const [connecting, setConnecting] = useState<string | null>(null);
  useEffect(() => {
    const off = listen(EVENTS.composioChanged, () => setConnecting(null));
    return () => {
      void off.then((f) => f());
    };
  }, []);

  const connected = status.apps.filter((a) => a.connected || justDone === `app:${a.slug}`);
  const missing = status.apps.filter((a) => !a.connected && a.why && justDone !== `app:${a.slug}`);

  const add = (slug: string, name: string) => {
    setConnecting(slug);
    startWaiting(`app:${slug}`, name, {
      resumeTab,
      steps: [`${name} opened in your browser.`, "Sign in and allow access."],
      again: () => void api.composioConnect(slug).catch(() => undefined),
    });
    api.composioConnect(slug).catch((e) => {
      setConnecting(null);
      onError(String(e));
    });
  };

  return (
    <>
      {connected.length > 0 && (
        <ul className="flex flex-wrap gap-1.5">
          {connected.map((a) => {
            const chip = (
              <span
                className={`flex items-center gap-1.5 rounded-full px-2.5 py-1 text-[12.5px] transition-colors duration-300 ${
                  justDone === `app:${a.slug}` ? "bg-[#30d158]/25" : "bg-black/5 dark:bg-white/10"
                }`}
              >
                <span className="size-1.5 rounded-full bg-[#30d158]" />
                {a.name}
              </span>
            );
            return <li key={a.slug}>{a.why ? <Tip label={a.why}>{chip}</Tip> : chip}</li>;
          })}
        </ul>
      )}
      {missing.length > 0 && (
        <div className="flex flex-col gap-1.5 text-[12.5px]">
          <p className="px-0.5 text-[12px] text-(--muted)">Sidekick can also use</p>
          <ul className="flex flex-col gap-1.5">
            {missing.map((a) => (
              <li key={a.slug} className="flex items-center gap-2.5 rounded-xl border border-(--border) px-2.5 py-2">
                <span className="min-w-0 flex-1">
                  <span className="block truncate font-medium">{a.name}</span>
                  <span className="block truncate text-[11px] text-(--muted)">{a.why}</span>
                </span>
                <Button small disabled={connecting === a.slug} onClick={() => add(a.slug, a.name)}>
                  {connecting === a.slug ? "Waiting..." : "Add"}
                </Button>
              </li>
            ))}
          </ul>
        </div>
      )}
    </>
  );
}
