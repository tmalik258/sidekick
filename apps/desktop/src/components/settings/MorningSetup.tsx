"use client";

// Morning setup: the apps and sites you open first on most mornings,
// learned on this PC. Sidekick asks to open them (Ask first), opens them
// by itself, or does nothing (Off). One click takes an item out.

import { useEffect, useState } from "react";
import { api } from "@/lib/bridge";
import { updateSettings, useSidekick } from "@/lib/store";
import type { RoutineItem } from "@/lib/types";
import { appName, Button, Segmented } from "./ui";

type Mode = "ask" | "auto" | "off";

export function MorningSetup({ onError }: { onError: (e: string) => void }) {
  const on = useSidekick((s) => s.settings.routines);
  const auto = useSidekick((s) => s.settings.routinesAuto);
  const [items, setItems] = useState<RoutineItem[] | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [sure, setSure] = useState(false);
  const mode: Mode = !on ? "off" : auto ? "auto" : "ask";

  useEffect(() => {
    void api
      .routinesToday()
      .then(setItems)
      .catch(() => setItems([]));
  }, []);
  useEffect(() => {
    if (!sure) return;
    const id = setTimeout(() => setSure(false), 4000);
    return () => clearTimeout(id);
  }, [sure]);

  const setMode = (m: Mode) =>
    void updateSettings({ routines: m !== "off", routinesAuto: m === "auto" }).catch((e: unknown) =>
      onError(String(e)),
    );
  const remove = (item: RoutineItem) =>
    void api
      .routinesRemove(item.kind, item.key)
      .then(() => setItems((list) => list?.filter((i) => i !== item) ?? null))
      .catch((e: unknown) => onError(String(e)));

  return (
    <div className="flex flex-col gap-3 text-[13px]">
      <div className="flex items-center justify-between gap-3">
        <span>
          Your usual start
          <span className="block text-[12px] text-(--muted)">Learned from your first hour, on this PC only.</span>
        </span>
        <Segmented<Mode>
          label="Morning setup"
          value={mode}
          options={[
            ["ask", "Ask first"],
            ["auto", "Open by itself"],
            ["off", "Off"],
          ]}
          onChange={setMode}
        />
      </div>
      {on && items !== null && (
        <div className="flex flex-col gap-1.5">
          {items.length === 0 ? (
            <p className="text-(--muted)">Nothing yet. An app joins when you open it on 3 of the last 5 mornings.</p>
          ) : (
            <ul className="flex flex-wrap gap-1.5" aria-label="Today's usual start">
              {items.map((item) => {
                const name = item.kind === "app" ? appName(item.label) : item.label;
                return (
                  <li key={`${item.kind}:${item.key}`}>
                    <button
                      type="button"
                      onClick={() => remove(item)}
                      title={`Take ${name} out (${item.days} of 5 mornings)`}
                      aria-label={`Remove ${name}`}
                      className="chip group flex items-center gap-1.5 rounded-full bg-black/5 py-1 pr-2 pl-2.5 text-[12.5px] dark:bg-white/10"
                    >
                      {name}
                      {item.kind === "site" && item.browser && (
                        <span className="text-(--muted)">in {item.browser}</span>
                      )}
                      <span aria-hidden className="text-(--muted) group-hover:text-(--text)">
                        ×
                      </span>
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      )}
      <div className="flex items-center gap-2">
        <Button
          small
          onClick={() => {
            if (!sure) return setSure(true);
            setSure(false);
            void api
              .routinesForget()
              .then((n) => {
                setItems([]);
                setNotice(n ? "Forgot your morning setup." : "Nothing learned yet.");
              })
              .catch((e: unknown) => onError(String(e)));
          }}
        >
          {sure ? "Forget everything it learned?" : "Start over"}
        </Button>
        {notice && <span className="text-[12px] text-(--muted)">{notice}</span>}
      </div>
    </div>
  );
}
