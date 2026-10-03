"use client";

// Agent: how often a step waits for a tap, and per app or site whether
// Sidekick may act there on its own, should ask, or stays out.

import { useState } from "react";
import { updateSettings, useSidekick } from "@/lib/store";
import type { AgentAsk, PlaceRule } from "@/lib/types";
import { Button, Field, Segmented, Select } from "./ui";

const ASK: [AgentAsk, string][] = [
  ["each", "Every step"],
  ["outward", "Before sending, paying or deleting"],
  ["irreversible", "Only before paying or deleting"],
];

const RULES: [PlaceRule, string][] = [
  ["allow", "Allow"],
  ["ask", "Ask"],
  ["never", "Never"],
];

export function Agent({ onError }: { onError: (e: string) => void }) {
  const agent = useSidekick((s) => s.settings.agent);
  const [place, setPlace] = useState("");
  const save = (patch: Partial<typeof agent>) =>
    void updateSettings({ agent: { ...agent, ...patch } }).catch((e) => onError(String(e)));
  const setRule = (name: string, rule: PlaceRule | null) => {
    const places = { ...agent.places };
    if (rule) places[name] = rule;
    else delete places[name];
    save({ places });
  };
  const add = () => {
    const name = place
      .trim()
      .toLowerCase()
      .replace(/^https?:\/\//, "")
      .replace(/^www\./, "")
      .replace(/\/.*$/, "");
    if (!name) return;
    setRule(name, "allow");
    setPlace("");
  };
  const entries = Object.entries(agent.places);

  return (
    <div className="flex flex-col gap-3 text-[13px]">
      <Field label="Wait for my tap" hint="Paying and deleting always wait">
        <Select
          label="Wait for my tap"
          value={agent.ask}
          options={ASK}
          onChange={(v) => save({ ask: v as AgentAsk })}
        />
      </Field>
      {entries.length > 0 && (
        <ul className="flex flex-col divide-y divide-(--border)">
          {entries.map(([name, rule]) => (
            <li key={name} className="flex items-center gap-3 py-2 first:pt-0">
              <span className="min-w-0 flex-1 truncate font-medium">{name}</span>
              <Segmented label={`Rule for ${name}`} value={rule} options={RULES} onChange={(r) => setRule(name, r)} />
              <Button small onClick={() => setRule(name, null)}>
                Remove
              </Button>
            </li>
          ))}
        </ul>
      )}
      <form
        className="flex items-center gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          add();
        }}
      >
        <input
          aria-label="App or site"
          value={place}
          placeholder="whatsapp or mail.google.com"
          spellCheck={false}
          onChange={(e) => setPlace(e.target.value)}
          className="w-56 rounded-md border border-(--border) bg-transparent px-2 py-1 text-[13px] outline-none focus:border-(--accent)"
        />
        <Button small disabled={!place.trim()} onClick={add}>
          Add
        </Button>
      </form>
      <p className="text-[12px] text-(--muted)">
        Allow: acts without asking, except paying and deleting. Never: Sidekick does not read or touch it.
      </p>
    </div>
  );
}
