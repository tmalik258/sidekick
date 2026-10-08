"use client";

// Home: setup, today, shortcuts, sound, history and the app
// itself. Everything here is picked or recorded; nothing needs typing.

import { useCallback, useEffect, useRef, useState } from "react";
import { api, EVENTS, listen } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import { useDnd } from "@/lib/hooks";
import { checkKit, cueVolume, playCue, SYNTH_KIT } from "@/lib/sound";
import { updateSettings, useSidekick } from "@/lib/store";
import {
  type ActionRecord,
  type AppInfo,
  type AppTime,
  CUES,
  canUndo,
  type Folder,
  type Found,
  formatDuration,
  type InboxStatus,
  MASCOT_STATES,
  type NotifyLevel,
  type StoredEvent,
} from "@/lib/types";
import { SetupChecklist, usePendingByTab } from "../SetupChecklist";
import { Tip } from "../Tip";
import { Button, ChipList, Field, FolderPicker, Section, Select, Slider, Toggle } from "./ui";

const SOUND_KITS: [string, string][] = [
  [SYNTH_KIT, "Sidekick"],
  ["01", "Classic"],
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
      <Section title="Setup" keywords="install checklist get started tools">
        <LeftElsewhere onOpenTab={onOpenTab} />
        <SetupChecklist groups={["tools"]} onOpenTab={onOpenTab} />
      </Section>
      <Section
        title="Notifications"
        hint="Windows stays quiet. Only what matters comes up on the island."
        keywords="notifications inbox do not disturb silence important digest vip whatsapp slack"
      >
        <NotificationInbox onError={onError} />
      </Section>
      <Section title="Today" keywords="time tracking hours apps">
        <TimeToday />
      </Section>
      <Section
        collapsible
        summary="Folders where your projects live"
        title="Your code"
        keywords="code folders repos git projects end of day wsl"
      >
        <CodeFolders chosen={settings.codeFolders} onChange={(codeFolders) => save({ codeFolders })} />
        <Field label="My day ends at">
          <Select
            label="My day ends at"
            value={String(settings.endOfDayHour)}
            options={END_OF_DAY}
            onChange={(v) => save({ endOfDayHour: Number(v) })}
          />
        </Field>
      </Section>
      <Section
        collapsible
        summary="Launch on login, updates"
        title="Startup and updates"
        keywords="launch login windows start update version"
      >
        <Toggle
          label="Launch on login"
          checked={settings.launchAtLogin}
          onChange={(launchAtLogin) => save({ launchAtLogin })}
        />
        <Toggle
          label="Tell me about new versions"
          checked={settings.checkUpdates}
          onChange={(checkUpdates) => save({ checkUpdates })}
        />
        <UpdateRow version={info?.version ?? ""} onError={onError} />
      </Section>
      <Section
        collapsible
        summary={settings.muted ? "Muted" : "On"}
        title="Sound"
        hint={
          settings.soundKit === SYNTH_KIT
            ? "Soft sounds made by Sidekick itself."
            : "Sounds by SND (snd.dev), designed by Dentsu Inc. and Starryworks Inc."
        }
        keywords="volume mute audio cue kit voice"
      >
        <Field label="Sounds">
          <Select
            label="Sounds"
            value={settings.soundKit}
            options={SOUND_KITS}
            onChange={(soundKit) => save({ soundKit })}
          />
        </Field>
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
      <Section collapsible summary="What Sidekick did, with Undo" title="History" keywords="undo actions recent">
        <RecentActions onError={onError} />
      </Section>
      <Section collapsible summary="Export or restore your settings" title="Backup" keywords="export import restore">
        <Backup onError={onError} />
      </Section>
      {info && (
        <Section collapsible summary={info.version} title="About" keywords="version database path">
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
          <CopyDiagnostics onError={onError} />
          <Toggle
            label="Offer to report crashes"
            hint="After a crash, Sidekick shows the report and lets you send it. It holds the version, Windows version and the error, never your files or chats."
            checked={settings.crashReports}
            onChange={(crashReports) => save({ crashReports })}
          />
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
              </div>
              <RecentEvents />
            </div>
          </details>
        </Section>
      )}
    </>
  );
}

const TAB_NAMES: Record<string, string> = { ai: "AI", connections: "Apps", privacy: "Privacy" };

/** Steps left on other tabs, as one tap each, so Home never repeats them. */
function LeftElsewhere({ onOpenTab }: { onOpenTab: (tab: string) => void }) {
  const pending = usePendingByTab();
  const tabs = Object.keys(TAB_NAMES).filter((t) => pending[t]?.length);
  if (tabs.length === 0) return null;
  return (
    <div className="flex flex-wrap items-center gap-1.5 text-[12.5px]">
      <span className="text-(--muted)">Also to set up:</span>
      {tabs.map((t) => (
        <Tip key={t} label={pending[t].map((i) => i.title).join(", ")}>
          <button
            type="button"
            onClick={() => onOpenTab(t)}
            className="chip rounded-full bg-white/[0.08] px-2.5 py-1 font-medium text-white/90 hover:bg-white/[0.14]"
          >
            {TAB_NAMES[t]} ({pending[t].length})
          </button>
        </Tip>
      ))}
    </div>
  );
}

function CodeFolders({ chosen, onChange }: { chosen: string[]; onChange: (next: string[]) => void }) {
  const { data } = useCached<Found>("setup-detect", api.setupDetect);
  const found: Folder[] = data?.codeFolders ?? [];
  return (
    <>
      {chosen.length === 0 && found.length > 0 && (
        <p className="text-[12px] text-(--muted)">None picked, so Sidekick looks in all the ones it found.</p>
      )}
      <FolderPicker
        found={found}
        chosen={chosen}
        onChange={onChange}
        empty={data ? "No git repos found in the usual places. Add the folder that holds your projects." : ""}
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

const LEVELS: [NotifyLevel | "auto", string][] = [
  ["auto", "Sidekick decides"],
  ["now", "Right away"],
  ["soon", "Gather for later"],
  ["digest", "Digest only"],
  ["never", "Mute"],
];
const LEVEL_TONE: Record<NotifyLevel, string> = {
  now: "bg-[#ff453a]/20 text-[#ff9f97]",
  soon: "bg-[#0a84ff]/20 text-[#8cc3ff]",
  digest: "bg-white/10 text-(--muted)",
  never: "bg-white/5 text-(--muted)",
};

/** Windows' Do Not Disturb as a switch that shows the real state. */
function DoNotDisturb({ onError }: { onError: (e: string) => void }) {
  const dnd = useDnd({ onError });
  return (
    <Toggle
      label="Do Not Disturb"
      hint={dnd.busy ? "Switching..." : "Stops Windows pop-ups. Notifications still reach Sidekick."}
      checked={dnd.on}
      onChange={dnd.set}
    />
  );
}

/** Read Windows notifications and only bring up what matters. */
function NotificationInbox({ onError }: { onError: (e: string) => void }) {
  const notifications = useSidekick((s) => s.settings.notifications);
  const [status, setStatus] = useState<InboxStatus | null>(null);
  const refresh = useCallback(() => void api.notificationsStatus().then(setStatus), []);
  useEffect(() => {
    refresh();
    if (!notifications.enabled) return;
    // Refreshed when a notification arrives, not on a timer.
    const off = listen(EVENTS.inboxChanged, refresh);
    return () => void off.then((f) => f());
  }, [refresh, notifications.enabled]);
  const save = (patch: Partial<typeof notifications>) =>
    void updateSettings({ notifications: { ...notifications, ...patch } })
      .then(refresh)
      .catch((e) => onError(String(e)));
  const today = (status?.items ?? []).filter((i) => new Date(i.ts).toDateString() === new Date().toDateString());
  const important = today.filter((i) => i.level === "now" || i.level === "soon").length;

  return (
    <>
      <DoNotDisturb onError={onError} />
      <Toggle
        label="Sort my notifications"
        hint={
          !notifications.enabled
            ? "Codes, VIPs and urgent ones come up at once. Messages wait for a quiet moment. The rest goes to a digest."
            : status && !status.readable
              ? (status.error ?? "Could not read notifications yet.")
              : `${today.length} today, ${important} worth a look`
        }
        checked={notifications.enabled}
        onChange={(enabled) => save({ enabled })}
      />
      {notifications.enabled && (
        <>
          <p className="text-[13px] font-medium">
            Always come through <span className="font-normal text-(--muted)">people, by name</span>
          </p>
          <ChipList
            label="Always come through"
            items={notifications.vip}
            suggestions={[]}
            placeholder="Add a name, e.g. Ali"
            format={(v) => v.replace(/\b\w/g, (c) => c.toUpperCase())}
            onChange={(vip) => save({ vip })}
          />
          {status && status.apps.length > 0 && (
            <div className="flex flex-col gap-2">
              {status.apps.map((a) => (
                <Field key={a.app} label={a.app} hint={a.count ? `${a.count} recently` : undefined}>
                  <Select
                    label={`${a.app} notifications`}
                    value={a.level ?? "auto"}
                    options={LEVELS}
                    onChange={(level) =>
                      void api
                        .notificationsSetLevel(a.app, level as NotifyLevel | "auto")
                        .then(refresh)
                        .catch((e) => onError(String(e)))
                    }
                  />
                </Field>
              ))}
            </div>
          )}
          {today.length > 0 && (
            <details className="text-[12.5px]">
              <summary className="cursor-pointer text-(--muted)">Latest</summary>
              <ul className="mt-2 flex flex-col gap-1.5">
                {today.slice(0, 8).map((i) => (
                  <li key={i.id} className="flex items-center gap-2">
                    <span className={`shrink-0 rounded-full px-2 py-0.5 text-[11px] ${LEVEL_TONE[i.level]}`}>
                      {i.level}
                    </span>
                    <span className="min-w-0 flex-1 truncate">
                      <span className="font-medium">
                        {i.app}
                        {i.phone ? " (phone)" : ""}
                      </span>{" "}
                      {i.title}: <span className="text-(--muted)">{i.body}</span>
                      {(i.also?.length ?? 0) > 0 && (
                        <span className="text-(--muted)"> (also via {i.also?.join(", ")})</span>
                      )}
                    </span>
                  </li>
                ))}
              </ul>
            </details>
          )}
        </>
      )}
    </>
  );
}

/** This version, and a newer one when it is out: Install, or Check now. */
function UpdateRow({ version, onError }: { version: string; onError: (e: string) => void }) {
  const update = useSidekick((s) => s.update);
  const [state, setState] = useState<"idle" | "checking" | "latest" | "installing">("idle");
  const check = () => {
    setState("checking");
    api.updateCheck().then(
      (found) => setState(found ? "idle" : "latest"),
      (e) => {
        setState("idle");
        onError(String(e));
      },
    );
  };
  const install = () => {
    setState("installing");
    api.updateInstall().catch((e) => {
      setState("idle");
      onError(String(e));
    });
  };
  return (
    <div className="flex items-center justify-between gap-3 text-[13px]">
      <span className="min-w-0">
        <span className="block text-white">
          {update ? `Sidekick ${update.version} is available` : `Sidekick ${version}`}
        </span>
        <span className="block text-(--muted)">
          {update
            ? `You have ${update.current}. It downloads, checks and opens the installer.`
            : state === "latest"
              ? "You have the newest version."
              : "Checks once a day while the switch above is on."}
        </span>
      </span>
      {update ? (
        <Button small onClick={install} disabled={state === "installing"}>
          {state === "installing" ? "Opening..." : "Install"}
        </Button>
      ) : (
        <Button small onClick={check} disabled={state === "checking"}>
          {state === "checking" ? "Checking..." : "Check now"}
        </Button>
      )}
    </div>
  );
}

/** Version, models, timings and the end of the log, with secrets and your
 * name taken out, copied for a bug report. */
const ISSUES = "https://github.com/tmalik258/sidekick/issues/new";

function CopyDiagnostics({ onError }: { onError: (e: string) => void }) {
  const [copied, setCopied] = useState(false);
  const [saved, setSaved] = useState<string | null>(null);
  // Saves the log to Downloads and opens a new issue; nothing is sent until
  // you attach the file there yourself.
  const send = () =>
    void api
      .reportSave()
      .then(async (path) => {
        setSaved("Saved to Downloads. Read it, then drag it into the issue.");
        await api.revealPath(path);
        const body = "What happened:\n\nWhat I expected:\n\n(Drag the Sidekick report file from Downloads here.)\n";
        await api.aiOpenLink(`${ISSUES}?title=${encodeURIComponent("Bug report")}&body=${encodeURIComponent(body)}`);
      })
      .catch((e: unknown) => onError(String(e)));
  const copy = () =>
    void api
      .diagnostics()
      .then((text) => navigator.clipboard.writeText(text))
      .then(() => {
        setCopied(true);
        setTimeout(() => setCopied(false), 2000);
      })
      .catch((e: unknown) => onError(String(e)));
  return (
    <div className="flex items-center gap-3">
      <Button small onClick={copy}>
        {copied ? "Copied" : "Copy diagnostics"}
      </Button>
      <Button small onClick={send}>
        Send report
      </Button>
      <span className="text-[13px] text-(--muted)">
        {saved ?? "For a bug report. Keys, tokens and your name are taken out."}
      </span>
    </div>
  );
}
