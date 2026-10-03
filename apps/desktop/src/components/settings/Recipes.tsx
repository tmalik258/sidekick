"use client";

// Recipes: saved tasks that run by name in Ask, or on a trigger. Each runs as
// an ordinary Ask question, so sending or paying still waits for a tap.

import { useState } from "react";
import { api } from "@/lib/bridge";
import { useSidekick } from "@/lib/store";
import type { Recipe, Trigger } from "@/lib/types";
import { Button, Field, Select, TextField, Toggle } from "./ui";

const WHEN: [Trigger["when"], string][] = [
  ["manual", "When I ask"],
  ["time", "At a time"],
  ["notification", "On a notification"],
  ["download", "When a download finishes"],
  ["meeting_ended", "When a meeting ends"],
  ["app_opened", "When an app opens"],
];

const DAYS = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

export function describeTrigger(t: Trigger): string {
  switch (t.when) {
    case "time":
      return t.days.length ? `${t.days.join(", ")} at ${t.time}` : `Every day at ${t.time}`;
    case "notification":
      return `On a notification${t.app ? ` from ${t.app}` : ""}${t.contains ? ` with "${t.contains}"` : ""}`;
    case "download":
      return t.kind ? `When a ${t.kind} download finishes` : "When a download finishes";
    case "meeting_ended":
      return "When a meeting ends";
    case "app_opened":
      return `When ${t.app || "an app"} opens`;
    default:
      return "When I ask";
  }
}

function blank(when: Trigger["when"]): Trigger {
  switch (when) {
    case "time":
      return { when, time: "09:00", days: [] };
    case "notification":
      return { when, app: "", contains: "" };
    case "download":
      return { when, kind: "" };
    case "app_opened":
      return { when, app: "" };
    case "meeting_ended":
      return { when };
    default:
      return { when: "manual" };
  }
}

function TriggerFields({ trigger, onChange }: { trigger: Trigger; onChange: (t: Trigger) => void }) {
  return (
    <>
      <Field label="Runs">
        <Select
          label="When it runs"
          value={trigger.when}
          options={WHEN}
          onChange={(when) => onChange(blank(when as Trigger["when"]))}
        />
      </Field>
      {trigger.when === "time" && (
        <>
          <Field label="Time" hint="24 hour, like 17:00">
            <TextField
              label="Time"
              value={trigger.time}
              className="w-24"
              onCommit={(time) => onChange({ ...trigger, time })}
            />
          </Field>
          <fieldset className="m-0 flex flex-wrap gap-1.5 border-0 p-0">
            <legend className="sr-only">Days</legend>
            {DAYS.map((d) => {
              const on = trigger.days.includes(d);
              return (
                <button
                  key={d}
                  type="button"
                  aria-pressed={on}
                  onClick={() =>
                    onChange({ ...trigger, days: on ? trigger.days.filter((x) => x !== d) : [...trigger.days, d] })
                  }
                  className={`chip rounded-full px-2.5 py-1 text-[12px] capitalize ${
                    on ? "bg-white text-black" : "bg-white/[0.08] text-white/80 hover:bg-white/[0.14]"
                  }`}
                >
                  {d}
                </button>
              );
            })}
            <span className="self-center text-[11.5px] text-(--muted)">None picked: every day</span>
          </fieldset>
        </>
      )}
      {(trigger.when === "notification" || trigger.when === "app_opened") && (
        <Field label="App">
          <TextField
            label="App"
            value={trigger.app}
            placeholder="WhatsApp"
            className="w-40"
            onCommit={(app) => onChange({ ...trigger, app })}
          />
        </Field>
      )}
      {trigger.when === "notification" && (
        <Field label="Only with the word" hint="Optional">
          <TextField
            label="Only with the word"
            value={trigger.contains}
            placeholder="invoice"
            className="w-40"
            onCommit={(contains) => onChange({ ...trigger, contains })}
          />
        </Field>
      )}
      {trigger.when === "download" && (
        <Field label="Kind" hint="Optional: pdf, image, document">
          <TextField
            label="Download kind"
            value={trigger.kind}
            placeholder="pdf"
            className="w-32"
            onCommit={(kind) => onChange({ ...trigger, kind })}
          />
        </Field>
      )}
    </>
  );
}

export function Recipes({ onError }: { onError: (e: string) => void }) {
  const recipes = useSidekick((s) => s.settings.recipes);
  const [draft, setDraft] = useState<Recipe | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const save = (r: Recipe) =>
    api
      .recipeSave(r)
      .then((m) => setNote(m))
      .catch((e) => onError(String(e)));

  return (
    <div className="flex flex-col gap-2.5 text-[13px]">
      {recipes.length === 0 && !draft && (
        <p className="text-(--muted)">
          None yet. Ask for something, then press Save as recipe, or say "every Friday at 5, email my timesheet".
        </p>
      )}
      {recipes.map((r) => (
        <details key={r.id} className="rounded-xl border border-(--border) px-3 py-2.5">
          <summary className="flex cursor-pointer list-none items-center gap-3">
            <span className="min-w-0 flex-1">
              <span className="block truncate font-medium">{r.name}</span>
              <span className="block truncate text-[12px] text-(--muted)">
                {describeTrigger(r.trigger)}
                {r.auto ? ", starts alone" : ""}
                {r.enabled ? "" : ", off"}
              </span>
            </span>
            <Button
              small
              onClick={() =>
                void api
                  .recipeRun(r.id)
                  .then(setNote)
                  .catch((e) => onError(String(e)))
              }
            >
              Run
            </Button>
          </summary>
          <div className="mt-3 flex flex-col gap-2.5">
            <Field label="Instruction">
              <TextField
                label="Instruction"
                value={r.prompt}
                className="w-64"
                onCommit={(prompt) => void save({ ...r, prompt })}
              />
            </Field>
            <TriggerFields trigger={r.trigger} onChange={(trigger) => void save({ ...r, trigger })} />
            <Toggle
              label="Start without asking"
              hint="Sending, posting and paying still wait for your tap"
              checked={r.auto}
              onChange={(auto) => void save({ ...r, auto })}
            />
            <Toggle label="On" checked={r.enabled} onChange={(enabled) => void save({ ...r, enabled })} />
            <div>
              <Button
                small
                onClick={() =>
                  void api
                    .recipeDelete(r.id)
                    .then(setNote)
                    .catch((e) => onError(String(e)))
                }
              >
                Delete
              </Button>
            </div>
          </div>
        </details>
      ))}
      {draft ? (
        <div className="flex flex-col gap-2.5 rounded-xl border border-(--border) px-3 py-2.5">
          <Field label="Name">
            <TextField
              label="Recipe name"
              value={draft.name}
              placeholder="Send timesheet"
              className="w-56"
              onCommit={(name) => setDraft({ ...draft, name })}
            />
          </Field>
          <Field label="Instruction" hint="What you would type in Ask">
            <TextField
              label="Recipe instruction"
              value={draft.prompt}
              placeholder="Email my timesheet to Sara"
              className="w-64"
              onCommit={(prompt) => setDraft({ ...draft, prompt })}
            />
          </Field>
          <TriggerFields trigger={draft.trigger} onChange={(trigger) => setDraft({ ...draft, trigger })} />
          <div className="flex gap-2">
            <Button
              small
              primary
              disabled={!draft.prompt.trim()}
              onClick={() => void save(draft).then(() => setDraft(null))}
            >
              Save
            </Button>
            <Button small onClick={() => setDraft(null)}>
              Cancel
            </Button>
          </div>
        </div>
      ) : (
        <div>
          <Button
            small
            onClick={() =>
              setDraft({ id: "", name: "", prompt: "", trigger: { when: "manual" }, auto: false, enabled: true })
            }
          >
            New recipe
          </Button>
        </div>
      )}
      {note && <p className="text-[12px] text-(--muted)">{note}</p>}
    </div>
  );
}
