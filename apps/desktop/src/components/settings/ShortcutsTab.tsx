"use client";

// Shortcuts: every key in one place. Keys that work anywhere can be
// changed; keys inside the island are listed so they are easy to learn.

import { updateSettings, useSidekick } from "@/lib/store";
import { type Settings, SHORTCUT_ACTIONS } from "@/lib/types";
import { Field, Section, ShortcutRecorder } from "./ui";

/** Keys used inside the island. Alt R and Alt Z are left to GPU overlays. */
const ISLAND_KEYS: [string, string][] = [
  ["Copy the answer", "Alt C"],
  ["Retry", "Alt T"],
  ["Think harder", "Alt K"],
  ["Undo", "Alt U"],
  ["Speaker on or off", "Alt S"],
  ["Read the answer aloud", "Alt L"],
  ["Talk", "Alt V"],
  ["Pick the model", "Alt M"],
  ["This PC only", "Alt P"],
  ["History", "Alt H"],
  ["Pick option 1 to 9", "Alt 1–9"],
  ["Ignore a suggestion", "Alt 0"],
  ["Switch tab", "Ctrl Tab"],
  ["Previous tab", "Ctrl Shift Tab"],
  ["New agent session", "Ctrl N"],
];

export function ShortcutsTab({ onError }: { onError: (e: string) => void }) {
  const settings = useSidekick((s) => s.settings);
  const save = (patch: Partial<Settings>) => void updateSettings(patch).catch((e: unknown) => onError(String(e)));
  return (
    <>
      <Section
        title="Anywhere"
        hint="Click one, then press the keys."
        keywords="hotkey keyboard keys ask agents talk accept dismiss screen clipboard pause focus"
      >
        <Field label="Ask">
          <ShortcutRecorder
            label="Ask"
            value={settings.paletteHotkey}
            onChange={(paletteHotkey) => save({ paletteHotkey })}
          />
        </Field>
        {SHORTCUT_ACTIONS.map((a) => (
          <Field key={a.id} label={a.label}>
            <ShortcutRecorder
              label={a.label}
              value={settings.shortcuts[a.id] ?? ""}
              onChange={(keys) => save({ shortcuts: { ...settings.shortcuts, [a.id]: keys } })}
            />
          </Field>
        ))}
      </Section>
      <Section
        title="Inside the island"
        hint="Hold Alt to see them on the buttons. Alt R and Alt Z are left free for NVIDIA and AMD overlays."
        keywords="alt keys island buttons"
      >
        {ISLAND_KEYS.map(([label, keys]) => (
          <div key={label} className="flex items-center justify-between gap-4 text-[13px]">
            <span>{label}</span>
            <span className="flex gap-1">
              {keys.split(" ").map((k) => (
                <kbd key={k} className="rounded-md bg-white/10 px-1.5 py-0.5 font-sans text-[11.5px] text-white">
                  {k}
                </kbd>
              ))}
            </span>
          </div>
        ))}
      </Section>
    </>
  );
}
