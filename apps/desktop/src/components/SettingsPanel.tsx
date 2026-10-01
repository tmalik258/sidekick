"use client";

import { type ReactNode, useCallback, useEffect, useState } from "react";
import { api } from "@/lib/bridge";
import { useNow } from "@/lib/hooks";
import { checkKit, cueVolume, playCue } from "@/lib/sound";
import { connect, updateSettings, useSidekick } from "@/lib/store";
import {
  type ActionRecord,
  type AppInfo,
  type CapabilityInfo,
  CUES,
  isPaused,
  MASCOT_STATES,
  type Pause,
  SENSOR_IDS,
  type SkillInfo,
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

      <Section
        title="Skills"
        hint="Auto runs the first safe option without asking. Deleting files, running installers or stopping processes always ask first."
      >
        <SkillList onError={setError} />
      </Section>

      <Section title="Sensors" hint="Each sensor can be switched off on its own (FR-SEN-12).">
        {SENSOR_IDS.map(({ id, label, hint }) => (
          <Toggle
            key={id}
            label={label}
            hint={hint}
            checked={settings.sensors[id] ?? id !== "heartbeat"}
            onChange={(on) => run(() => updateSettings({ sensors: { ...settings.sensors, [id]: on } }))}
          />
        ))}
      </Section>

      <Section
        title="Found on this PC"
        hint="Skills only offer what is installed. Install ffmpeg, ImageMagick, LibreOffice or pandoc for more conversions, then rescan."
      >
        <Capabilities onError={setError} />
      </Section>

      <Section title="Debug" hint="Tools for checking the island, mascot, and pipeline.">
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
        <RecentActions />
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

function SkillList({ onError }: { onError: (e: string) => void }) {
  const [skills, setSkills] = useState<SkillInfo[]>([]);
  const refresh = useCallback(() => void api.skillsList().then(setSkills), []);
  useEffect(refresh, [refresh]);
  const change = (s: SkillInfo, enabled: boolean, auto: boolean) => {
    setSkills((list) => list.map((x) => (x.id === s.id ? { ...x, enabled, auto } : x)));
    api.skillSet(s.id, enabled, auto).catch((e) => {
      onError(String(e));
      refresh();
    });
  };
  return (
    <ul className="flex flex-col divide-y divide-(--border)">
      {skills.map((s) => (
        <li key={s.id} className="flex items-start justify-between gap-4 py-2.5 first:pt-0 last:pb-0">
          <div className="min-w-0">
            <p className="text-[14px] font-medium">{s.name}</p>
            <p className="text-[12px] text-(--muted)">{s.description}</p>
          </div>
          <div className="flex shrink-0 items-center gap-3 pt-0.5">
            <label className="flex items-center gap-1.5 text-[12px] text-(--muted)">
              Auto
              <input
                type="checkbox"
                checked={s.auto}
                disabled={!s.enabled}
                onChange={(e) => change(s, s.enabled, e.target.checked)}
                className="size-3.5 accent-[#0a84ff]"
              />
            </label>
            <Switch checked={s.enabled} onChange={(on) => change(s, on, s.auto)} label={`Enable ${s.name}`} />
          </div>
        </li>
      ))}
    </ul>
  );
}

function Capabilities({ onError }: { onError: (e: string) => void }) {
  const [info, setInfo] = useState<CapabilityInfo | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const load = useCallback(
    (rescan: boolean) => {
      setBusy(true);
      api
        .capabilitiesGet(rescan)
        .then(setInfo)
        .catch((e) => onError(String(e)))
        .finally(() => setBusy(false));
    },
    [onError],
  );
  useEffect(() => load(false), [load]);
  return (
    <div className="flex flex-col gap-3 text-[13px]">
      <div className="flex flex-wrap gap-1.5">
        {info?.found.length ? (
          info.found.map((f) => (
            <span key={f} className="rounded-full bg-black/5 px-2.5 py-1 dark:bg-white/10">
              {f}
            </span>
          ))
        ) : (
          <span className="text-(--muted)">Nothing found yet.</span>
        )}
      </div>
      <div className="flex flex-wrap gap-2">
        <Button small onClick={() => load(true)} disabled={busy}>
          {busy ? "Scanning…" : "Rescan"}
        </Button>
        <Button
          small
          onClick={() =>
            void api
              .choicesReset()
              .then((n) => setNotice(n ? `Forgot ${n} learned choices.` : "No learned choices yet."))
          }
        >
          Forget learned choices
        </Button>
      </div>
      {notice && <p className="text-[12px] text-(--muted)">{notice}</p>}
      {info && (
        <p className="text-[12px] text-(--muted)">
          Your own skills: <span className="font-mono break-all">{info.skillsDir}</span>
        </p>
      )}
      {info?.skillErrors.map((e) => (
        <p key={e} className="text-[12px] text-red-500">
          {e}
        </p>
      ))}
    </div>
  );
}

function RecentActions() {
  const [actions, setActions] = useState<ActionRecord[]>([]);
  const refresh = useCallback(() => void api.actionsRecent(10).then(setActions), []);
  useEffect(refresh, [refresh]);
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium">Recent actions</h3>
        <Button small onClick={refresh}>
          Refresh
        </Button>
      </div>
      {actions.length === 0 ? (
        <p className="text-sm text-(--muted)">No actions yet.</p>
      ) : (
        <ul className="divide-y divide-(--border) rounded-lg border border-(--border) text-xs">
          {actions.map((a) => (
            <li key={`${a.ts}-${a.label}`} className="flex justify-between gap-3 px-3 py-1.5">
              <span className={a.ok ? "" : "text-red-500"}>
                {a.label}
                {a.auto ? " (auto)" : ""}: {a.message}
              </span>
              <span className="shrink-0 text-(--muted)">{new Date(a.ts).toLocaleTimeString()}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
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

function Toggle({
  label,
  hint,
  checked,
  onChange,
}: {
  label: string;
  hint?: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <div className="flex items-center justify-between gap-4 text-[14px]">
      <div className="min-w-0">
        <p>{label}</p>
        {hint && <p className="text-[12px] text-(--muted)">{hint}</p>}
      </div>
      <Switch checked={checked} onChange={onChange} label={label} />
    </div>
  );
}

function Switch({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={() => onChange(!checked)}
      className={`relative h-[26px] w-[44px] shrink-0 rounded-full transition-colors duration-200 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#0a84ff] ${
        checked ? "bg-[#30d158]" : "bg-black/15 dark:bg-white/20"
      }`}
    >
      <span
        className="absolute top-[2px] left-[2px] size-[22px] rounded-full bg-white shadow-[0_2px_6px_rgb(0_0_0/0.2)] transition-transform duration-[260ms] ease-(--ease-out-strong)"
        style={{ transform: checked ? "translateX(18px)" : "translateX(0)" }}
      />
    </button>
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
