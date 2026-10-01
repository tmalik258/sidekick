"use client";

import { type ReactNode, useCallback, useEffect, useState } from "react";
import { api } from "@/lib/bridge";
import { useNow } from "@/lib/hooks";
import { cueVolume, playCue } from "@/lib/sound";
import { connect, updateSettings, useSidekick } from "@/lib/store";
import { type AppInfo, CUES, isPaused, MASCOT_STATES, type Pause, SENSOR_IDS, type StoredEvent } from "@/lib/types";
import { Mascot } from "./Mascot";

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
        <Mascot state={mascot} size={64} />
        <div>
          <h1 className="text-xl font-semibold">Sidekick settings</h1>
          <p className="text-sm text-(--muted)">
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

      <Section title="Sound">
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
              <Button small onClick={() => playCue(cue, cueVolume(settings, cue))}>
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
    <section className="flex flex-col gap-3 rounded-xl border border-(--border) bg-(--surface) p-4">
      <div>
        <h2 className="font-semibold">{title}</h2>
        {hint && <p className="text-xs text-(--muted)">{hint}</p>}
      </div>
      {children}
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
      } ${active ? "bg-(--hover) ring-1 ring-sky-500" : ""}`}
    >
      {children}
    </button>
  );
}

function Toggle({ label, checked, onChange }: { label: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="flex cursor-pointer items-center justify-between gap-4 text-sm">
      {label}
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        className="size-4 accent-sky-500"
      />
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
        className="w-36 accent-sky-500"
      />
    </label>
  );
}
