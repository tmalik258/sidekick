"use client";

import { type ReactNode, useCallback, useEffect, useState } from "react";
import { api } from "@/lib/bridge";
import { useNow } from "@/lib/hooks";
import { checkKit, cueVolume, playCue } from "@/lib/sound";
import { connect, updateSettings, useSidekick } from "@/lib/store";
import {
  type ActionRecord,
  AI_PROVIDERS,
  type AiProviderId,
  type AiSettings,
  type AppInfo,
  type CapabilityInfo,
  CLAUDE_HOOK_URL,
  CUES,
  canUndo,
  isPaused,
  MASCOT_STATES,
  type Pause,
  PROVIDER_LABELS,
  type ProviderStatus,
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
        <div className="flex items-center justify-between gap-4 text-sm">
          <span>
            Ask shortcut
            <span className="block text-[12px] text-(--muted)">For example Alt+Space or Ctrl+Shift+K</span>
          </span>
          <TextField
            value={settings.paletteHotkey}
            onCommit={(paletteHotkey) => run(() => updateSettings({ paletteHotkey }))}
            className="w-40 text-right"
            label="Ask shortcut"
          />
        </div>
        <Toggle
          label="Launch Sidekick when Windows starts"
          checked={settings.launchAtLogin}
          onChange={(launchAtLogin) => run(() => updateSettings({ launchAtLogin }))}
        />
      </Section>

      <Section
        title="AI"
        hint="Sidekick works fully without AI. Chat tries the providers top to bottom and falls back when one is not reachable. Ranking (T1) only ever uses SemIf or a model on this PC."
      >
        <AiSection ai={settings.ai} onError={setError} />
      </Section>

      <Section title="History" hint="Files Sidekick created can be undone for 24 hours; they go to the Recycle Bin.">
        <RecentActions onError={setError} />
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
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium">Recent actions</h3>
        <Button small onClick={refresh}>
          Refresh
        </Button>
      </div>
      {actions.length === 0 ? (
        <p className="text-sm text-(--muted)">Nothing yet. Actions you pick on the island show up here.</p>
      ) : (
        <ul className="divide-y divide-(--border) rounded-lg border border-(--border) text-[12.5px]">
          {actions.map((a) => (
            <li key={a.id} className="flex items-center justify-between gap-3 px-3 py-2">
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

/** A text input that saves on Enter or when it loses focus, not per key. */
function TextField({
  value,
  onCommit,
  placeholder,
  className = "",
  mono,
  label,
}: {
  label?: string;
  value: string;
  onCommit: (v: string) => void;
  placeholder?: string;
  className?: string;
  mono?: boolean;
}) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const commit = () => {
    if (draft !== value) onCommit(draft.trim());
  };
  return (
    <input
      value={draft}
      aria-label={label}
      placeholder={placeholder}
      spellCheck={false}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") commit();
        if (e.key === "Escape") setDraft(value);
      }}
      className={`rounded-md border border-(--border) bg-transparent px-2 py-1 text-[13px] outline-none focus:border-(--accent) ${
        mono ? "font-mono text-[12px]" : ""
      } ${className}`}
    />
  );
}

function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4 text-[13px]">
      <span className="min-w-0">
        {label}
        {hint && <span className="block text-[11.5px] text-(--muted)">{hint}</span>}
      </span>
      {children}
    </div>
  );
}

/** Splits a command line into arguments, honouring double quotes. */
function splitArgs(line: string): string[] {
  return [...line.matchAll(/"([^"]*)"|(\S+)/g)].map((m) => m[1] ?? m[2]);
}

function joinArgs(args: string[]): string {
  return args.map((a) => (/\s/.test(a) ? `"${a}"` : a)).join(" ");
}

const PROVIDER_HINTS: Record<AiProviderId, string> = {
  claude_code: "Your own Claude subscription through the claude CLI. Sidekick never reads its sign-in files.",
  anthropic: "Uses ANTHROPIC_API_KEY from your environment. The key is never stored.",
  local: "Ollama, LM Studio or any OpenAI-compatible server. Nothing leaves this PC.",
};

function AiSection({ ai, onError }: { ai: AiSettings; onError: (e: string) => void }) {
  const [status, setStatus] = useState<ProviderStatus[] | null>(null);
  const refresh = useCallback(() => {
    setStatus(null);
    void api.aiStatus().then(setStatus);
  }, []);
  // biome-ignore lint/correctness/useExhaustiveDependencies: re-check whenever the AI settings change
  useEffect(refresh, [refresh, ai]);

  const save = (next: Partial<AiSettings>) =>
    updateSettings({ ai: { ...ai, ...next } }).catch((e: unknown) => onError(String(e)));
  const available = (id: string) => status?.find((s) => s.id === id)?.available;
  const move = (id: AiProviderId, delta: number) => {
    const order = [...ai.order];
    const i = order.indexOf(id);
    const j = i + delta;
    if (i < 0 || j < 0 || j >= order.length) return;
    [order[i], order[j]] = [order[j], order[i]];
    void save({ order });
  };
  const enabled = (id: AiProviderId) =>
    id === "claude_code" ? ai.claudeCode.enabled : id === "anthropic" ? ai.anthropic.enabled : ai.local.enabled;
  const setEnabled = (id: AiProviderId, on: boolean) => {
    if (id === "claude_code") void save({ claudeCode: { ...ai.claudeCode, enabled: on } });
    else if (id === "anthropic") void save({ anthropic: { ...ai.anthropic, enabled: on } });
    else void save({ local: { ...ai.local, enabled: on } });
  };
  const order = ai.order.filter((id) => AI_PROVIDERS.includes(id));

  return (
    <div className="flex flex-col gap-4">
      <ol className="flex flex-col gap-3">
        {order.map((id, i) => (
          <li key={id} className="flex flex-col gap-2 rounded-xl border border-(--border) p-3">
            <div className="flex items-center gap-3">
              <StatusDot state={status === null ? "checking" : available(id) ? "ok" : "off"} />
              <div className="min-w-0 flex-1">
                <p className="text-[14px] font-medium">
                  {i + 1}. {PROVIDER_LABELS[id]}
                </p>
                <p className="text-[12px] text-(--muted)">{PROVIDER_HINTS[id]}</p>
              </div>
              <div className="flex gap-1">
                <Button small onClick={() => move(id, -1)} disabled={i === 0}>
                  Up
                </Button>
                <Button small onClick={() => move(id, 1)} disabled={i === order.length - 1}>
                  Down
                </Button>
              </div>
              <Switch checked={enabled(id)} onChange={(on) => setEnabled(id, on)} label={PROVIDER_LABELS[id]} />
            </div>
            {id === "claude_code" && (
              <>
                <Field label="Path to claude" hint="Empty finds it on PATH">
                  <TextField
                    label="Path to claude"
                    value={ai.claudeCode.path}
                    placeholder="claude"
                    mono
                    className="w-64"
                    onCommit={(path) => save({ claudeCode: { ...ai.claudeCode, path } })}
                  />
                </Field>
                <Field label="Model" hint="Empty uses Claude Code's default">
                  <TextField
                    label="Model"
                    value={ai.claudeCode.model}
                    placeholder="default"
                    mono
                    className="w-64"
                    onCommit={(model) => save({ claudeCode: { ...ai.claudeCode, model } })}
                  />
                </Field>
              </>
            )}
            {id === "anthropic" && (
              <Field label="Model">
                <TextField
                  label="Model"
                  value={ai.anthropic.model}
                  placeholder="claude-opus-5-5"
                  mono
                  className="w-64"
                  onCommit={(model) => save({ anthropic: { ...ai.anthropic, model } })}
                />
              </Field>
            )}
            {id === "local" && (
              <>
                <Field label="Server URL">
                  <TextField
                    label="Server URL"
                    value={ai.local.baseUrl}
                    mono
                    className="w-64"
                    onCommit={(baseUrl) => save({ local: { ...ai.local, baseUrl } })}
                  />
                </Field>
                <Field label="Model" hint="Empty uses the first model the server lists">
                  <TextField
                    label="Model"
                    value={ai.local.model}
                    placeholder="qwen3:4b"
                    mono
                    className="w-64"
                    onCommit={(model) => save({ local: { ...ai.local, model } })}
                  />
                </Field>
              </>
            )}
          </li>
        ))}
      </ol>

      <div className="flex flex-col gap-2 rounded-xl border border-(--border) p-3">
        <div className="flex items-center gap-3">
          <StatusDot state={status === null ? "checking" : available("semif") ? "ok" : "off"} />
          <div className="min-w-0 flex-1">
            <p className="text-[14px] font-medium">SemIf decisions (T1)</p>
            <p className="text-[12px] text-(--muted)">
              Scores each option straight from a small model's logits. Runs semif-score natively or inside WSL.
            </p>
          </div>
          <Switch
            checked={ai.semif.enabled}
            onChange={(enabled) => save({ semif: { ...ai.semif, enabled } })}
            label="SemIf"
          />
        </div>
        {ai.semif.enabled && (
          <>
            <Field label="Command" hint="e.g. wsl.exe -d Ubuntu-22.04 -- /home/me/semif/.venv/bin/semif-score">
              <TextField
                label="Command"
                value={joinArgs(ai.semif.command)}
                mono
                className="w-72"
                onCommit={(line) => save({ semif: { ...ai.semif, command: splitArgs(line) } })}
              />
            </Field>
            <Field label="Backend" hint="llamacpp runs a GGUF on CPU or a small GPU">
              <select
                value={ai.semif.backend}
                onChange={(e) => save({ semif: { ...ai.semif, backend: e.target.value } })}
                className="rounded-md border border-(--border) bg-transparent px-2 py-1 text-[13px]"
              >
                {["llamacpp", "torch", "mlx"].map((b) => (
                  <option key={b} value={b}>
                    {b}
                  </option>
                ))}
              </select>
            </Field>
            <Field label="Model">
              <TextField
                label="Model"
                value={ai.semif.model}
                mono
                className="w-72"
                onCommit={(model) => save({ semif: { ...ai.semif, model } })}
              />
            </Field>
            <Field label="Revision">
              <TextField
                label="Revision"
                value={ai.semif.revision}
                mono
                className="w-72"
                onCommit={(revision) => save({ semif: { ...ai.semif, revision } })}
              />
            </Field>
            {ai.semif.backend === "llamacpp" && (
              <Field label="GGUF file" hint="Path as SemIf sees it (a /home/... path inside WSL)">
                <TextField
                  label="GGUF file"
                  value={ai.semif.gguf}
                  mono
                  className="w-72"
                  onCommit={(gguf) => save({ semif: { ...ai.semif, gguf } })}
                />
              </Field>
            )}
          </>
        )}
      </div>

      <ClaudeHook />

      <Toggle
        label="Rank suggestion options with T1"
        hint="SemIf or the local model guesses which option you want and puts it first. Your past picks always win."
        checked={ai.decisions}
        onChange={(decisions) => save({ decisions })}
      />
      <div>
        <Button small onClick={refresh}>
          Check again
        </Button>
      </div>
    </div>
  );
}

function StatusDot({ state }: { state: "ok" | "off" | "checking" }) {
  const label = state === "ok" ? "Ready" : state === "off" ? "Not reachable" : "Checking";
  return (
    <span
      role="img"
      aria-label={label}
      title={label}
      className={`size-2 shrink-0 rounded-full ${
        state === "ok"
          ? "bg-[#30d158]"
          : state === "off"
            ? "bg-black/20 dark:bg-white/25"
            : "animate-pulse bg-amber-400"
      }`}
    />
  );
}

const HOOK_SNIPPET = JSON.stringify(
  {
    hooks: Object.fromEntries(
      ["Stop", "Notification"].map((event) => [
        event,
        [{ hooks: [{ type: "http", url: CLAUDE_HOOK_URL, timeout: 5 }] }],
      ]),
    ),
  },
  null,
  2,
);

/** The hook users add to their own Claude Code settings. Sidekick never edits that file. */
function ClaudeHook() {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(HOOK_SNIPPET);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard access can be refused; the text is selectable anyway.
    }
  };
  return (
    <div className="flex flex-col gap-2 rounded-xl border border-(--border) p-3">
      <div className="flex items-start gap-3">
        <div className="min-w-0 flex-1">
          <p className="text-[14px] font-medium">Claude Code hooks</p>
          <p className="text-[12px] text-(--muted)">
            Add this to <code className="font-mono">~/.claude/settings.json</code> (merge with any hooks you have) so
            Sidekick knows when a session finishes or is waiting for you.
          </p>
        </div>
        <Button small onClick={copy}>
          {copied ? "Copied" : "Copy"}
        </Button>
      </div>
      <pre className="overflow-x-auto rounded-lg bg-black/5 p-3 font-mono text-[11.5px] leading-relaxed select-all dark:bg-white/5">
        {HOOK_SNIPPET}
      </pre>
    </div>
  );
}
