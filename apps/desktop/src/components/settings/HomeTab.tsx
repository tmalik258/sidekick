"use client";

// Home: setup, today, appearance, shortcuts, sound, history and the app
// itself. Everything here is picked or recorded; nothing needs typing.

import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "@/lib/bridge";
import { checkKit, cueVolume, playCue } from "@/lib/sound";
import { updateSettings, useSidekick } from "@/lib/store";
import {
  type ActionRecord,
  type AppInfo,
  type AppTime,
  CUES,
  canUndo,
  type Folder,
  formatDuration,
  MASCOT_STATES,
  SHORTCUT_ACTIONS,
  type StoredEvent,
  THEMES,
} from "@/lib/types";
import { Orb, THEME_STYLES } from "../Orb";
import { SetupChecklist } from "../SetupChecklist";
import { Button, Field, FolderPicker, Section, Select, ShortcutRecorder, Slider, Toggle } from "./ui";

const COLLAPSE_OPTIONS: [string, string][] = [
  ["4", "4 seconds"],
  ["8", "8 seconds"],
  ["15", "15 seconds"],
  ["30", "30 seconds"],
  ["60", "1 minute"],
];

const END_OF_DAY: [string, string][] = [16, 17, 18, 19, 20, 21, 22].map((h) => [
  String(h),
  `${h > 12 ? h - 12 : h} pm`,
]);

export function HomeTab({ onError, onOpenTab }: { onError: (e: string) => void; onOpenTab: (tab: string) => void }) {
  const settings = useSidekick((s) => s.settings);
  const mascot = useSidekick((s) => s.mascot);
  const [info, setInfo] = useState<AppInfo | null>(null);
  useEffect(() => {
    void api.appInfo().then(setInfo);
  }, []);
  const save = (patch: Parameters<typeof updateSettings>[0]) =>
    void updateSettings(patch).catch((e) => onError(String(e)));

  return (
    <>
      <Section title="Setup" keywords="install checklist get started ollama claude">
        <SetupChecklist groups={["ai", "connect", "tools"]} onOpenTab={onOpenTab} />
      </Section>
      <Section
        title="Today"
        hint="Counted from the app in front, paused while you are away. Stored only on this PC."
        keywords="time tracking hours apps"
      >
        <TimeToday />
      </Section>
      <Section title="Appearance" keywords="theme orb color look">
        <div className="grid grid-cols-3 gap-3">
          {THEMES.map((t) => (
            <button
              key={t}
              type="button"
              aria-pressed={settings.theme === t}
              onClick={() => save({ theme: t })}
              className={`chip flex flex-col items-center gap-2.5 rounded-xl bg-black py-4 text-[13px] font-medium text-white/90 ${
                settings.theme === t ? "ring-2 ring-[#0a84ff]" : "ring-1 ring-white/10"
              }`}
            >
              <Orb state="idle" size={40} theme={t} magnetic={false} />
              {THEME_STYLES[t].label}
            </button>
          ))}
        </div>
        <Field label="Close suggestions after">
          <Select
            label="Close suggestions after"
            value={String(settings.collapseAfterSecs)}
            options={COLLAPSE_OPTIONS}
            onChange={(v) => save({ collapseAfterSecs: Number(v) })}
          />
        </Field>
      </Section>
      <Section
        title="Shortcuts"
        hint="Click a shortcut, then press the keys you want. They work from any app."
        keywords="hotkey keyboard keys talk accept dismiss screen clipboard pause"
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
        title="Your code"
        hint="For the end of day check on uncommitted work and the project launcher."
        keywords="code folders repos git projects end of day wsl"
      >
        <CodeFolders chosen={settings.codeFolders} onChange={(codeFolders) => save({ codeFolders })} />
        <Field label="My day ends at" hint="When to check for work you have not pushed">
          <Select
            label="My day ends at"
            value={String(settings.endOfDayHour)}
            options={END_OF_DAY}
            onChange={(v) => save({ endOfDayHour: Number(v) })}
          />
        </Field>
      </Section>
      <Section title="Startup and updates" keywords="launch login windows start update version">
        <Toggle
          label="Start Sidekick with Windows"
          checked={settings.launchAtLogin}
          onChange={(launchAtLogin) => save({ launchAtLogin })}
        />
        <Toggle
          label="Tell me about new versions"
          hint="Checks GitHub once a day. Install is one click, and the download is checked before it runs."
          checked={settings.checkUpdates}
          onChange={(checkUpdates) => save({ checkUpdates })}
        />
      </Section>
      <Section
        title="Sound"
        hint="Sounds by SND (snd.dev), designed by Dentsu Inc. and Starryworks Inc."
        keywords="volume mute audio cue"
      >
        <KitStatus kit={settings.soundKit} />
        <Toggle label="Mute all sounds" checked={settings.muted} onChange={(muted) => save({ muted })} />
        <Slider
          label="Master volume"
          value={settings.masterVolume}
          onChange={(masterVolume) => save({ masterVolume })}
        />
        <details className="text-[13px]">
          <summary className="cursor-pointer text-(--muted)">Each sound</summary>
          <div className="mt-2 grid gap-2 sm:grid-cols-2">
            {CUES.map((cue) => (
              <div key={cue} className="flex items-center gap-2">
                <Slider
                  label={cue}
                  value={settings.cueVolumes[cue] ?? 1}
                  onChange={(v) => save({ cueVolumes: { ...settings.cueVolumes, [cue]: v } })}
                />
                <Button small onClick={() => playCue(cue, cueVolume(settings, cue), settings.soundKit)}>
                  Test
                </Button>
              </div>
            ))}
          </div>
        </details>
      </Section>
      <Section
        title="History"
        hint="Files Sidekick created can be undone for 24 hours; they go to the Recycle Bin."
        keywords="undo actions recent"
      >
        <RecentActions onError={onError} />
      </Section>
      <Section
        title="Backup"
        hint="One file with your settings, your own skills and the action history. Keys and sign-ins are left out."
        keywords="export import restore"
      >
        <Backup onError={onError} />
      </Section>
      {info && (
        <Section title="About" keywords="version database path">
          <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
            <dt className="text-(--muted)">Version</dt>
            <dd>{info.version}</dd>
            <dt className="text-(--muted)">Database</dt>
            <dd className="font-mono text-xs break-all">{info.dbPath}</dd>
            <dt className="text-(--muted)">Settings</dt>
            <dd className="font-mono text-xs break-all">{info.settingsPath}</dd>
            <dt className="text-(--muted)">Stored events</dt>
            <dd>{info.eventCount}</dd>
          </dl>
          <details className="text-[13px]">
            <summary className="cursor-pointer text-(--muted)">Debug tools</summary>
            <div className="mt-2 flex flex-col gap-2">
              <div className="flex flex-wrap gap-2">
                {MASCOT_STATES.map((s) => (
                  <Button key={s} small active={s === mascot} onClick={() => void api.debugSetState(s)}>
                    {s}
                  </Button>
                ))}
              </div>
              <div className="flex flex-wrap gap-2">
                <Button small onClick={() => void api.debugEmitEvent()}>
                  Emit test event
                </Button>
                <Button small onClick={() => void api.debugDemoFlow()}>
                  Run demo suggestion
                </Button>
              </div>
              <RecentEvents />
            </div>
          </details>
        </Section>
      )}
    </>
  );
}

function CodeFolders({ chosen, onChange }: { chosen: string[]; onChange: (next: string[]) => void }) {
  const [found, setFound] = useState<Folder[] | null>(null);
  useEffect(() => {
    void api
      .setupDetect()
      .then((f) => setFound(f.codeFolders))
      .catch(() => setFound([]));
  }, []);
  if (!found) return <p className="text-[12px] text-(--muted)">Looking for your code...</p>;
  return (
    <>
      {chosen.length === 0 && found.length > 0 && (
        <p className="text-[12px] text-(--muted)">None picked, so Sidekick looks in all the ones it found.</p>
      )}
      <FolderPicker
        found={found}
        chosen={chosen}
        onChange={onChange}
        empty="No git repos found in the usual places. Add the folder that holds your projects."
      />
    </>
  );
}

function KitStatus({ kit }: { kit: string }) {
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    void checkKit(kit).then((e) => live && setError(e));
    return () => {
      live = false;
    };
  }, [kit]);
  if (!error) return null;
  return (
    <p role="alert" className="text-[12px] text-red-500">
      This kit could not load, so a fallback tone plays instead: {error}
    </p>
  );
}

/** Where today went, by app and project. */
function TimeToday() {
  const [rows, setRows] = useState<AppTime[] | null>(null);
  useEffect(() => {
    void api.timeToday().then(setRows);
    const id = setInterval(() => void api.timeToday().then(setRows), 60_000);
    return () => clearInterval(id);
  }, []);
  if (!rows) return null;
  if (rows.length === 0) return <p className="text-sm text-(--muted)">Nothing counted yet today.</p>;
  const total = rows.reduce((n, r) => n + r.secs, 0);
  const top = rows[0].secs;
  return (
    <div className="flex flex-col gap-2.5">
      <p className="text-[13px] text-(--muted)">
        <span className="font-display text-[22px] font-semibold tracking-[-0.02em] text-(--text)">
          {formatDuration(total)}
        </span>{" "}
        at the computer
      </p>
      <ul className="flex flex-col gap-2">
        {rows.slice(0, 8).map((r) => (
          <li key={`${r.app}|${r.project}`} className="text-[13px]">
            <div className="flex justify-between gap-3">
              <span className="truncate">
                {r.app}
                {r.project && <span className="text-(--muted)"> · {r.project}</span>}
              </span>
              <span className="shrink-0 text-(--muted) tabular-nums">{formatDuration(r.secs)}</span>
            </div>
            <div className="mt-1 h-1.5 overflow-hidden rounded-full bg-black/5 dark:bg-white/8">
              <div className="h-full rounded-full bg-(--accent)" style={{ width: `${(r.secs / top) * 100}%` }} />
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}

function RecentActions({ onError }: { onError: (e: string) => void }) {
  const [actions, setActions] = useState<ActionRecord[]>([]);
  const refresh = useCallback(() => void api.actionsRecent(30).then(setActions), []);
  useEffect(refresh, [refresh]);
  const undo = async (id: number) => {
    try {
      await api.actionUndo(id);
    } catch (err) {
      onError(String(err));
    }
    refresh();
  };
  if (actions.length === 0) {
    return <p className="text-sm text-(--muted)">Nothing yet. Actions you pick on the island show up here.</p>;
  }
  return (
    <ul className="divide-y divide-(--border) text-[12.5px]">
      {actions.map((a) => (
        <li key={a.id} className="flex items-center justify-between gap-3 py-2 first:pt-0 last:pb-0">
          <span className={`min-w-0 ${a.ok ? "" : "text-red-500"}`}>
            <span className="font-medium">{a.label}</span>
            {a.auto ? " (auto)" : ""}
            <span className="block truncate text-(--muted)">
              {a.undone ? "Undone. " : ""}
              {a.message}
            </span>
          </span>
          <span className="flex shrink-0 items-center gap-2">
            {canUndo(a) && (
              <Button small onClick={() => void undo(a.id)}>
                Undo
              </Button>
            )}
            <span className="text-(--muted)">{new Date(a.ts).toLocaleTimeString()}</span>
          </span>
        </li>
      ))}
    </ul>
  );
}

function RecentEvents() {
  const [events, setEvents] = useState<StoredEvent[]>([]);
  useEffect(() => void api.eventsRecent(15).then(setEvents), []);
  if (events.length === 0) return <p className="text-(--muted)">No events stored yet.</p>;
  return (
    <ul className="divide-y divide-(--border) rounded-lg border border-(--border) text-xs">
      {events.map((e) => (
        <li key={e.id} className="flex justify-between gap-3 px-3 py-1.5 font-mono">
          <span>{e.kind}</span>
          <span className="text-(--muted)">{new Date(e.ts).toLocaleTimeString()}</span>
        </li>
      ))}
    </ul>
  );
}

function Backup({ onError }: { onError: (e: string) => void }) {
  const [note, setNote] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);
  return (
    <div className="flex flex-col gap-2 text-[13px]">
      <div className="flex gap-2">
        <Button
          onClick={() =>
            api
              .backupExport()
              .then((p) => setNote(`Saved to ${p}`))
              .catch((e) => onError(String(e)))
          }
        >
          Export
        </Button>
        <Button onClick={() => input.current?.click()}>Import</Button>
        <input
          ref={input}
          type="file"
          accept=".json,application/json"
          className="hidden"
          onChange={(e) => {
            const file = e.target.files?.[0];
            e.target.value = "";
            if (!file) return;
            void file
              .text()
              .then((text) => api.backupImport(text))
              .then(setNote)
              .catch((err) => onError(String(err)));
          }}
        />
      </div>
      {note && <p className="text-[12px] break-all text-(--muted)">{note}</p>}
    </div>
  );
}
