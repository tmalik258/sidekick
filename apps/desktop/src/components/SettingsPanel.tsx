"use client";

import { type ReactNode, useCallback, useEffect, useState } from "react";
import { api } from "@/lib/bridge";
import { useNow } from "@/lib/hooks";
import { checkKit, cueVolume, playCue } from "@/lib/sound";
import { connect, updateSettings, useSidekick } from "@/lib/store";
import {
  type AppInfo,
  CUES,
  isPaused,
  MASCOT_STATES,
  type Pause,
  SENSOR_IDS,
  type StoredEvent,
  THEMES,
} from "@/lib/types";
import { Orb, THEME_STYLES } from "./Orb";

export function SettingsPanel() {
  const { settings, mascot, ready } = useSidekick();
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => connect({ sounds: false }), []);
  useEffect(() => {
    void api.appInfo().then(setInfo);
  }, []);

  const run = useCallback(async (action: () => Promise<unknown>) => {
    setError(null);
    try {
      await action();
    } catch (err) {
      setError(String(err));
    }
  }, []);

  if (!ready) return null;

  return (
    <main className="mx-auto flex max-w-2xl flex-col gap-6 px-6 py-8">
      <header className="flex items-center gap-4">
        <div className="grid size-20 place-items-center">
          <Orb state={mascot} size={60} theme={settings.theme} />
        </div>
        <div>
          <h1 className="font-display text-[26px] leading-tight font-semibold tracking-[-0.02em]">Sidekick</h1>
          <p className="text-[13px] text-(--muted)">
            Version {info?.version ?? "…"} · mascot is {mascot}
          </p>
        </div>
      </header>

      {error && (
        <p
          role="alert"
          className="rounded-lg border border-red-500/40 bg-red-500/10 px-3 py-2 text-sm text-red-600 dark:text-red-300"
        >
          {error}
        </p>
      )}

      <Section title="Appearance">
        <div className="grid grid-cols-3 gap-3">
          {THEMES.map((t) => (
            <button
              key={t}
              type="button"
              aria-pressed={settings.theme === t}
              onClick={() => run(() => updateSettings({ theme: t }))}
              className={`chip flex flex-col items-center gap-2.5 rounded-xl bg-black py-4 text-[13px] font-medium text-white/90 ${
                settings.theme === t ? "ring-2 ring-[#0a84ff]" : "ring-1 ring-white/10"
              }`}
            >
              <Orb state="idle" size={40} theme={t} magnetic={false} />
              {THEME_STYLES[t].label}
            </button>
          ))}
        </div>
      </Section>

      <Section title="Privacy" hint="Paused sensors do not run at all (FR-SET-01).">
        <PauseStatus pause={settings.pause} />
        <div className="flex flex-wrap gap-2">
          <Button onClick={() => run(() => api.sensorsPause(15))}>Pause 15 min</Button>
          <Button onClick={() => run(() => api.sensorsPause(60))}>Pause 1 hour</Button>
          <Button onClick={() => run(() => api.sensorsPause(null))}>Pause until resumed</Button>
          <Button onClick={() => run(() => api.sensorsResume())} disabled={!isPaused(settings.pause)}>
            Resume
          </Button>
        </div>
      </Section>

      <Section title="Sound" hint="Sounds by SND (snd.dev), designed by Dentsu Inc. and Starryworks Inc.">
        <KitStatus kit={settings.soundKit} />
        <Toggle
          label="Mute all sounds"
          checked={settings.muted}
          onChange={(muted) => run(() => updateSettings({ muted }))}
        />
        <Slider
          label="Master volume"
          value={settings.masterVolume}
          onChange={(masterVolume) => run(() => updateSettings({ masterVolume }))}
        />
        <div className="grid gap-2 sm:grid-cols-2">
          {CUES.map((cue) => (
            <div key={cue} className="flex items-center gap-2">
              <Slider
                label={cue}
                value={settings.cueVolumes[cue] ?? 1}
                onChange={(v) => run(() => updateSettings({ cueVolumes: { ...settings.cueVolumes, [cue]: v } }))}
              />
              <Button small onClick={() => playCue(cue, cueVolume(settings, cue), settings.soundKit)}>
                Test
              </Button>
            </div>
          ))}
        </div>
      </Section>

      <Section title="Island">
        <label className="flex items-center justify-between gap-4 text-sm">
          Collapse after (seconds)
          <input
            type="number"
            min={2}
            max={120}
            value={settings.collapseAfterSecs}
            onChange={(e) => run(() => updateSettings({ collapseAfterSecs: Number(e.target.value) }))}
            className="w-20 rounded-md border border-(--border) bg-transparent px-2 py-1 text-right"
          />
        </label>
        <Toggle
          label="Launch Sidekick when Windows starts"
          checked={settings.launchAtLogin}
          onChange={(launchAtLogin) => run(() => updateSettings({ launchAtLogin }))}
        />
      </Section>

      <Section title="Sensors" hint="Each sensor can be switched off on its own (FR-SEN-12).">
        {SENSOR_IDS.map(({ id, label }) => (
          <Toggle
            key={id}
            label={label}
            checked={settings.sensors[id] ?? true}
            onChange={(on) => run(() => updateSettings({ sensors: { ...settings.sensors, [id]: on } }))}
          />
        ))}
      </Section>

      <Section title="Debug" hint="P0 tools for checking the island, mascot, and pipeline.">
        <div className="flex flex-wrap gap-2">
          {MASCOT_STATES.map((s) => (
            <Button key={s} small active={s === mascot} onClick={() => run(() => api.debugSetState(s))}>
              {s}
            </Button>
          ))}
        </div>
        <div className="flex flex-wrap gap-2">
          <Button onClick={() => run(() => api.debugEmitEvent())}>Emit test event</Button>
          <Button onClick={() => run(() => api.debugDemoFlow())}>Run demo suggestion</Button>
        </div>
        <RecentEvents />
      </Section>

      {info && (
        <Section title="About">
          <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
            <dt className="text-(--muted)">Database</dt>
            <dd className="font-mono text-xs break-all">{info.dbPath}</dd>
            <dt className="text-(--muted)">Settings</dt>
            <dd className="font-mono text-xs break-all">{info.settingsPath}</dd>
            <dt className="text-(--muted)">Stored events</dt>
            <dd>{info.eventCount}</dd>
          </dl>
        </Section>
      )}
    </main>
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

function PauseStatus({ pause }: { pause: Pause }) {
  const now = useNow(10_000);
  let text = "Sensors are running.";
  if (pause.kind === "indefinite") text = "Paused until you resume.";
  if (pause.kind === "until" && isPaused(pause, now)) {
    const minutes = Math.max(1, Math.round((Date.parse(pause.until) - now) / 60_000));
    text = `Paused for about ${minutes} more min.`;
  }
  return <p className="text-sm">{text}</p>;
}

function RecentEvents() {
  const [events, setEvents] = useState<StoredEvent[]>([]);
  const refresh = useCallback(() => void api.eventsRecent(15).then(setEvents), []);
  useEffect(refresh, [refresh]);

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium">Recent events</h3>
        <Button small onClick={refresh}>
          Refresh
        </Button>
      </div>
      {events.length === 0 ? (
        <p className="text-sm text-(--muted)">No events stored yet.</p>
      ) : (
        <ul className="divide-y divide-(--border) rounded-lg border border-(--border) text-xs">
          {events.map((e) => (
            <li key={e.id} className="flex justify-between gap-3 px-3 py-1.5 font-mono">
              <span>{e.kind}</span>
              <span className="text-(--muted)">{new Date(e.ts).toLocaleTimeString()}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function Section({ title, hint, children }: { title: string; hint?: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-2">
      <div className="px-4">
        <h2 className="text-[13px] font-semibold tracking-[-0.005em] text-(--muted) uppercase">{title}</h2>
      </div>
      <div className="flex flex-col gap-3.5 rounded-2xl bg-(--surface) p-4 shadow-[0_0_0_0.5px_var(--border),0_1px_2px_rgb(0_0_0/0.04)]">
        {children}
      </div>
      {hint && <p className="px-4 text-[12px] text-(--muted)">{hint}</p>}
    </section>
  );
}

function Button({
  children,
  onClick,
  disabled,
  small,
  active,
}: {
  children: ReactNode;
  onClick: () => void;
  disabled?: boolean;
  small?: boolean;
  active?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={`rounded-lg border border-(--border) font-medium transition-colors hover:bg-(--hover) disabled:opacity-40 ${
        small ? "px-2 py-0.5 text-xs" : "px-3 py-1.5 text-sm"
      } ${active ? "bg-(--hover) ring-1 ring-(--accent)" : ""}`}
    >
      {children}
    </button>
  );
}

function Toggle({ label, checked, onChange }: { label: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="flex cursor-pointer items-center justify-between gap-4 text-[14px]">
      {label}
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        onClick={() => onChange(!checked)}
        className={`relative h-6.5 w-11 shrink-0 rounded-full transition-colors duration-200 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-(--accent) ${
          checked ? "bg-(--accent)" : "bg-black/15 dark:bg-white/20"
        }`}
      >
        <span
          className="absolute top-0.5 left-0.5 size-5.5 rounded-full bg-white shadow-[0_2px_6px_rgb(0_0_0/0.2)] transition-transform duration-260 ease-out-strong"
          style={{ transform: checked ? "translateX(18px)" : "translateX(0)" }}
        />
      </button>
    </label>
  );
}

function Slider({ label, value, onChange }: { label: string; value: number; onChange: (v: number) => void }) {
  return (
    <label className="flex flex-1 items-center justify-between gap-3 text-sm capitalize">
      {label}
      <input
        type="range"
        min={0}
        max={1}
        step={0.05}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        className="w-36 accent-(--accent)"
      />
    </label>
  );
}
