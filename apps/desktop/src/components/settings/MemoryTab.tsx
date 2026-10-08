"use client";

// Memory: who you are, what Sidekick remembers, and what it has learned
// from your choices. Everything here stays on this PC and can be forgotten.

import { useState } from "react";
import { api } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import { updateSettings, useSidekick } from "@/lib/store";
import type { Learned, Settings, SuggestionRate } from "@/lib/types";
import { Memory } from "./PrivacyTab";
import { Button, Field, Section, Segmented, TextField, Toggle } from "./ui";

export function MemoryTab({ onError }: { onError: (e: string) => void }) {
  const settings = useSidekick((s) => s.settings);
  const save = (patch: Partial<Settings>) => void updateSettings(patch).catch((e: unknown) => onError(String(e)));
  return (
    <>
      <Section title="About you" keywords="name code technical developer about me people projects answers style">
        <Field label="Your name">
          <TextField
            label="Your name"
            value={settings.userName}
            placeholder="What should I call you?"
            onCommit={(userName) => save({ userName })}
          />
        </Field>
        <Field label="People you work with" hint="Names help Ask find the right email or chat.">
          <TextField
            label="People you work with"
            value={settings.people}
            placeholder="Sara (manager), Omar (design)"
            onCommit={(people) => save({ people })}
          />
        </Field>
        <Field label="Current projects" hint="Used for standups, status and finding files.">
          <TextField
            label="Current projects"
            value={settings.projects}
            placeholder="Website relaunch, Q4 report"
            onCommit={(projects) => save({ projects })}
          />
        </Field>
        <Field label="How you like answers" hint="Every model follows this.">
          <TextField
            label="How you like answers"
            value={settings.answerStyle}
            placeholder="Short, bullet points, no jargon"
            onCommit={(answerStyle) => save({ answerStyle })}
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
        title="Suggestions you take"
        hint="At most 4 an hour interrupt you. None during meetings or fullscreen apps."
        keywords="suggestions accept rate taken dismissed quiet cap"
      >
        <RateList />
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

/** "files.screenshot" reads "Screenshot". */
function skillName(id: string): string {
  const last = id.split(".").pop() ?? id;
  const words = last.replace(/[-_]/g, " ");
  return words.charAt(0).toUpperCase() + words.slice(1);
}

function RateList() {
  const { data } = useCached<SuggestionRate[]>("suggestion-rates", api.suggestionRates);
  const items = data ?? [];
  if (items.length === 0) {
    return (
      <p className="text-[13px] text-(--muted)">Nothing yet. Each kind of suggestion shows here once you answer one.</p>
    );
  }
  return (
    <div className="flex flex-col gap-2 text-[13px]">
      {items.map((r) => {
        const total = r.taken + r.dismissed;
        const pct = Math.round((r.taken / total) * 100);
        return (
          <div key={r.skill} className="flex items-center justify-between gap-4">
            <span className="min-w-0 truncate">{skillName(r.skill)}</span>
            <span className="shrink-0 text-(--muted)">
              Taken {pct}% of {total}
            </span>
          </div>
        );
      })}
    </div>
  );
}
