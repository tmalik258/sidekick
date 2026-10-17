"use client";

// Appearance: how the mascot and the island look, and how the island
// behaves on screen. Every choice applies at once.

import { updateSettings, useDuskie } from "@/lib/store";
import { ISLAND_COLORS, type IslandColor, THEMES } from "@/lib/types";
import { Creature } from "../Creature";
import { THEME_LABELS } from "../creature/themes";
import { Field, Section, Select, TextField, Toggle } from "./ui";

const COLLAPSE_OPTIONS: [string, string][] = [
  ["4", "4 seconds"],
  ["6", "6 seconds"],
  ["15", "15 seconds"],
  ["30", "30 seconds"],
  ["60", "1 minute"],
];

export const ISLAND_COLOR_LABELS: Record<IslandColor, string> = {
  black_glass: "Black glass",
  graphite: "Graphite",
  midnight: "Midnight",
  smoke: "Smoke",
  warm_graphite: "Warm graphite",
  solid_black: "Solid black",
  atelier: "Atelier",
};

/** Tile previews; the real colours live in globals.css (.island-shell). */
const ISLAND_SWATCH: Record<IslandColor, string> = {
  black_glass: "rgb(10 10 12)",
  graphite: "rgb(30 30 36)",
  midnight: "rgb(28 26 46)",
  smoke: "rgb(48 48 54)",
  warm_graphite: "rgb(36 32 30)",
  solid_black: "#000",
  atelier: "#0a0a0a",
};

/** A tile in the plan's style: faint glass, accent ring on the picked one. */
const tile = (on: boolean) =>
  `chip flex flex-col items-center gap-[7px] rounded-xl px-1 pt-2.5 pb-2 text-[12px] transition-[box-shadow,background-color,transform] duration-150 active:scale-[0.97] ${
    on
      ? "bg-(--surface) text-(--text) shadow-[inset_0_0_0_1.5px_var(--accent)]"
      : "bg-(--surface)/60 text-(--muted) shadow-[inset_0_0_0_1px_var(--border)] hover:bg-(--hover)"
  }`;

export function AppearanceTab({ onError }: { onError: (e: string) => void }) {
  const settings = useDuskie((s) => s.settings);
  const save = (patch: Parameters<typeof updateSettings>[0]) =>
    void updateSettings(patch).catch((e) => onError(String(e)));

  return (
    <>
      <Section
        title="Mascot"
        keywords="theme orb color colour look mascot pearl aurora chrome peach mint lilac onyx atelier sage"
      >
        <div className="grid grid-cols-4 gap-2.5">
          {THEMES.map((t) => (
            <button
              key={t}
              type="button"
              aria-pressed={settings.theme === t}
              onClick={() => save({ theme: t })}
              className={tile(settings.theme === t)}
            >
              <span className="orb-still">
                <Creature state="idle" size={34} theme={t} magnetic={false} simple active={false} />
              </span>
              {THEME_LABELS[t]}
            </button>
          ))}
        </div>
      </Section>
      <Section title="Name" keywords="name rename wake word hey call assistant orbi">
        <Field label="Assistant name" hint={nameHint(settings.assistantName)}>
          <TextField
            label="Assistant name"
            value={settings.assistantName}
            placeholder="Duskie"
            onCommit={(assistantName) => save({ assistantName: assistantName || "Duskie" })}
          />
        </Field>
      </Section>

      <Section
        title="Island"
        hint="The capsule around the mascot when it opens. Menus follow this colour."
        keywords="island color colour glass black graphite midnight smoke atelier sage green background"
      >
        <div className="grid grid-cols-3 gap-2.5">
          {ISLAND_COLORS.map((c) => (
            <button
              key={c}
              type="button"
              aria-pressed={settings.islandColor === c}
              onClick={() => save({ islandColor: c })}
              className={tile(settings.islandColor === c)}
            >
              <span
                aria-hidden
                className="relative flex h-[22px] w-[60px] items-center gap-1.5 overflow-hidden rounded-full pl-1"
                style={{
                  background: ISLAND_SWATCH[c],
                  boxShadow:
                    c === "solid_black"
                      ? "0 0 0 0.5px rgb(255 255 255 / 0.14)"
                      : c === "atelier"
                        ? "inset 0 0.5px 0 rgb(242 240 235 / 0.18), 0 0 0 0.5px rgb(122 143 126 / 0.45)"
                        : "inset 0 0.5px 0 rgb(255 255 255 / 0.18), 0 0 0 0.5px rgb(255 255 255 / 0.14)",
                }}
              >
                <span className="orb-still">
                  <Creature state="idle" size={15} theme={settings.theme} magnetic={false} simple active={false} />
                </span>
                <span className="h-[4px] w-[26px] rounded-full bg-white/55" />
              </span>
              {ISLAND_COLOR_LABELS[c]}
            </button>
          ))}
        </div>
      </Section>
      <Section title="Behaviour" keywords="fullscreen hide alive idle collapse close suggestions atelier">
        <Field label="Close suggestions after">
          <Select
            label="Close suggestions after"
            value={String(settings.collapseAfterSecs)}
            options={COLLAPSE_OPTIONS}
            onChange={(v) => save({ collapseAfterSecs: Number(v) })}
          />
        </Field>
        <Toggle
          label="Alive mode"
          hint="While nothing is happening, Duskie glances around, blinks and smiles now and then. No sounds."
          checked={settings.alive}
          onChange={(alive) => save({ alive })}
        />
        <Toggle
          label="Hide while fullscreen"
          hint="Keeps the island out of videos, slides and games. Hover the top edge to bring it back."
          checked={settings.hideInFullscreen}
          onChange={(hideInFullscreen) => save({ hideInFullscreen })}
        />
        <Toggle
          label="Tips"
          hint="Now and then, when nothing needs you."
          checked={settings.tips}
          onChange={(tips) => save({ tips })}
        />
      </Section>
    </>
  );
}

/** What saying the name does, and a warning for names that wake by mistake. */
function nameHint(name: string): string {
  const said = `Say "Hey ${name || "Duskie"}" to talk. "Hey Duskie" keeps working too.`;
  const vowels = (name.toLowerCase().match(/[aeiouy]+/g) ?? []).length;
  return name.length < 4 || vowels < 2
    ? `${said} Short names wake by mistake more often; two syllables or more work best.`
    : said;
}
