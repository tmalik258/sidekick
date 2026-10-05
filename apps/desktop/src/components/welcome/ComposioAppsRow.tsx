"use client";

// Inside the welcome's Composio row once it is connected: the account's
// apps, so Calendar, Fathom and the rest connect from the one place they
// come from.

import { useEffect, useState } from "react";
import { api, EVENTS, listen } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import type { ComposioStatus } from "@/lib/types";
import { ComposioApps } from "../settings/ComposioApps";

export function ComposioAppsRow() {
  const { data: status, refresh, set } = useCached<ComposioStatus>("composio-status", api.composioStatus);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    // Welcome remount after Connect must not keep a pre-sign-in cache.
    void refresh().catch(() => undefined);
  }, [refresh]);
  useEffect(() => {
    const off = listen(EVENTS.composioChanged, ({ ok, message }) => {
      setError(ok ? null : message);
      void api
        .composioStatus()
        .then(set)
        .catch((e) => setError(String(e)));
    });
    return () => {
      void off.then((f) => f());
    };
  }, [set]);

  // Before the first answer there is nothing to list; a key-only setup
  // (no browser sign-in) has no account apps to show.
  if (!status?.signedIn) return null;
  return (
    <div className="island-settings flex flex-col gap-2 border-t border-white/10 pt-2 text-[12.5px]">
      <ComposioApps status={status} onError={setError} resumeTab="connections" />
      {error && <p className="text-[11px] text-[#ff453a]">{error}</p>}
    </div>
  );
}
