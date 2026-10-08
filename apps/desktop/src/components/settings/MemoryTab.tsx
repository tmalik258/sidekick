"use client";

// Memory: who you are, what Sidekick remembers, and what it has learned
// from your choices. Everything here stays on this PC and can be forgotten.

import { useState } from "react";
import { api } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import { updateSettings, useSidekick } from "@/lib/store";
import type { Learned, Settings } from "@/lib/types";
import { Memory } from "./PrivacyTab";
import { Button, Field, Section, Segmented, TextField, Toggle } from "./ui";

export function MemoryTab({ onError }: { onError: (e: string) => void }) {
  const settings = useSidekick((s) => s.settings);
  const save = (patch: Partial<Settings>) => void updateSettings(patch).catch((e: unknown) => onError(String(e)));
  return (
    <>
      <Section title="About you" keywords="name code technical developer about me">
        <Field label="Your name">
          <TextField
            label="Your name"
            value={settings.userName}
            placeholder="What should I call you?"
            onCommit={(userName) => save({ userName })}
          />
        </Field>
        <div className="flex items-center justify-between gap-4 text-[14px]">
          <div className="min-w-0">
            <p>Do you work with code?</p>
            <p className="text-[12px] text-(--muted)">Sets how answers and setup are explained.</p>
          </div>
          <Segmented
            label="Do you work with code?"
            value={settings.codes === null ? "" : settings.codes ? "yes" : "no"}
            options={[
              ["yes", "Yes"],
              ["no", "No"],
            ]}
            onChange={(v) => save({ codes: v === "yes" })}
          />
        </div>
      </Section>
      <Section
        title="What I remember"
        hint='Every model gets these. Say "remember that..." in Ask to add one.'
        keywords="memory remember facts about me know-how forget"
      >
        <Memory onError={onError} />
      </Section>
      <Section
        title="Learned from your choices"
        hint="Stays on this PC. Older habits fade on their own."
        keywords="learned habits links routines quiet forget"
      >
        <LearnedList onError={onError} />
        <Toggle
          label="Keep learning"
          hint="Off: nothing new is learned. What is here stays until you forget it."
          checked={settings.learning}
          onChange={(learning) => save({ learning })}
        />
      </Section>
    </>
  );
}

function LearnedList({ onError }: { onError: (e: string) => void }) {
  const { data, refresh } = useCached<Learned[]>("learned", api.learnedList);
  const [sure, setSure] = useState(false);
  const items = data ?? [];
  const forget = (l: Learned) =>
    void api
      .learnedForget(l.kind, l.key, l.label)
      .then(() => refresh())
      .catch((e: unknown) => onError(String(e)));
  return (
    <div className="flex flex-col gap-2.5 text-[13px]">
      {items.length === 0 ? (
        <p className="text-(--muted)">Nothing yet. As you pick options, Sidekick learns what you usually do.</p>
      ) : (
        items.map((l) => (
          <div key={`${l.kind}:${l.key}:${l.label}`} className="flex items-center gap-3">
            <div className="min-w-0 flex-1">
              <p className="text-white">{l.text}</p>
              <p className="text-[12px] text-(--muted)">{l.why}</p>
            </div>
            <Button small onClick={() => forget(l)}>
              Forget
            </Button>
          </div>
        ))
      )}
      {items.length > 0 && (
        <div className="flex justify-end">
          <Button
            small
            onClick={() => {
              if (!sure) return setSure(true);
              setSure(false);
              void api
                .learnedForgetAll()
                .then(() => refresh())
                .catch((e: unknown) => onError(String(e)));
            }}
          >
            {sure ? "Forget all? Click again" : "Forget all"}
          </Button>
        </div>
      )}
    </div>
  );
}
