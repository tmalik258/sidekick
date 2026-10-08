"use client";

// Privacy and data: pause, what Sidekick notices, what it ignores, and what
// it can search. Apps and folders are picked from what is on this PC.

import { useCallback, useEffect, useState } from "react";
import { api } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import { useNow } from "@/lib/hooks";
import { updateSettings, useSidekick } from "@/lib/store";
import {
  type CapabilityInfo,
  type Folder,
  type Found,
  isPaused,
  type LocalModels,
  type Pause,
  SENSOR_IDS,
} from "@/lib/types";
import { appName, Button, ChipList, Field, FolderPicker, Section, Select, Toggle } from "./ui";

export function PrivacyTab({ onError }: { onError: (e: string) => void }) {
  const settings = useSidekick((s) => s.settings);
  const save = (patch: Parameters<typeof updateSettings>[0]) =>
    updateSettings(patch).catch((e: unknown) => onError(String(e)));
  const run = (p: Promise<unknown>) => void p.catch((e: unknown) => onError(String(e)));
  const [apps, setApps] = useState<string[]>([]);
  useEffect(() => {
    void api
      .runningApps()
      .then(setApps)
      .catch(() => setApps([]));
  }, []);

  return (
    <>
      <Section title="Pause" keywords="stop privacy break">
        <PauseStatus pause={settings.pause} />
        <div className="flex flex-wrap gap-2">
          <Button onClick={() => run(api.sensorsPause(15))}>15 min</Button>
          <Button onClick={() => run(api.sensorsPause(60))}>1 hour</Button>
          <Button onClick={() => run(api.sensorsPause(null))}>Until I resume</Button>
          {isPaused(settings.pause) && (
            <Button primary onClick={() => run(api.sensorsResume())}>
              Resume
            </Button>
          )}
        </div>
      </Section>
      <Section
        title="Ignored apps and sites"
        hint="Nothing from these is seen or kept."
        keywords="deny block private password manager bank"
      >
        <p className="text-[13px] font-medium">Apps</p>
        <ChipList
          label="Ignored apps"
          items={settings.denyApps}
          suggestions={apps}
          placeholder="Pick a running app or type its name"
          format={appName}
          onChange={(denyApps) => void save({ denyApps })}
        />
        <p className="mt-1 text-[13px] font-medium">Sites</p>
        <ChipList
          label="Ignored sites"
          items={settings.denySites}
          suggestions={[]}
          placeholder="mybank.com"
          onChange={(denySites) => void save({ denySites })}
        />
      </Section>
      <Section
        title="Search"
        hint="Files here are searchable from Ask. Stays on this PC."
        keywords="index folders notes documents semantic meaning embedding"
      >
        <SearchSettings onError={onError} />
      </Section>
      <Section collapsible summary="Switch each one on or off" title="What Sidekick notices" keywords="sensors">
        {SENSOR_IDS.map(({ id, label, hint }) => (
          <Toggle
            key={id}
            label={label}
            hint={hint}
            checked={settings.sensors[id] ?? true}
            onChange={(on) => void save({ sensors: { ...settings.sensors, [id]: on } })}
          />
        ))}
      </Section>
      <Section
        collapsible
        summary="Tools skills can use"
        title="Found on this PC"
        keywords="capabilities tools ffmpeg rescan learned choices"
      >
        <Capabilities onError={onError} />
      </Section>
    </>
  );
}

function PauseStatus({ pause }: { pause: Pause }) {
  const now = useNow(10_000);
  let text = "Sidekick is noticing things.";
  if (pause.kind === "indefinite") text = "Paused until you resume.";
  if (pause.kind === "until" && isPaused(pause, now)) {
    const minutes = Math.max(1, Math.round((Date.parse(pause.until) - now) / 60_000));
    text = `Paused for about ${minutes} more min.`;
  }
  return <p className="text-sm">{text}</p>;
}

function SearchSettings({ onError }: { onError: (e: string) => void }) {
  const folders = useSidekick((s) => s.settings.indexFolders);
  const semantic = useSidekick((s) => s.settings.semanticSearch);
  const { data: detected } = useCached<Found>("setup-detect", api.setupDetect);
  const found: Folder[] = detected?.searchFolders ?? [];
  const { data: models } = useCached<LocalModels>("local-models", api.localModels);
  const { data: status, refresh: reload } = useCached("search-status", api.searchStatus);
  const refresh = () => void reload().catch(() => undefined);
  const setSemantic = (patch: Partial<typeof semantic>) =>
    updateSettings({ semanticSearch: { ...semantic, ...patch } }).catch((e: unknown) => onError(String(e)));
  const embed = models?.embed ?? [];
  return (
    <div className="flex flex-col gap-3 text-[13px]">
      <FolderPicker
        found={found}
        chosen={folders}
        empty={detected ? "No usual folders found. Add one below." : ""}
        onChange={(indexFolders) =>
          void updateSettings({ indexFolders })
            .then(() => api.searchReindex())
            .catch((e: unknown) => onError(String(e)))
        }
      />
      <div className="flex items-center justify-between gap-2">
        <span className="text-(--muted)">
          {status === null ? "\u00a0" : `${status.items.toLocaleString()} items indexed`}
          {semantic.enabled && status && status.embedded > 0 && `, ${status.embedded.toLocaleString()} by meaning`}
        </span>
        <div className="flex gap-1.5">
          <Button
            small
            onClick={() => {
              void api.searchReindex();
              setTimeout(refresh, 15_000);
            }}
          >
            Re-index
          </Button>
          <ClearIndex onDone={refresh} />
        </div>
      </div>
      <Toggle
        label="Search by meaning"
        hint="Not just the exact words"
        checked={semantic.enabled}
        onChange={(enabled) => void setSemantic({ enabled })}
      />
      {semantic.enabled && (
        <Field
          label="Embedding model"
          hint={embed.length ? "Found in Ollama" : "None found. Install the search model in AI > Local."}
        >
          <Select
            label="Embedding model"
            value={semantic.model}
            options={embed.map((m) => [m, m])}
            onChange={(model) => void setSemantic({ model })}
          />
        </Field>
      )}
      {semantic.enabled && status?.embedError && (
        <p className="text-[12px] text-(--muted)">Search by meaning is waiting: {status.embedError}</p>
      )}
    </div>
  );
}

function ClearIndex({ onDone }: { onDone: () => void }) {
  const [sure, setSure] = useState(false);
  useEffect(() => {
    if (!sure) return;
    const id = setTimeout(() => setSure(false), 4000);
    return () => clearTimeout(id);
  }, [sure]);
  return (
    <Button
      small
      onClick={() => {
        if (!sure) return setSure(true);
        setSure(false);
        void api.searchClear().then(onDone);
      }}
    >
      {sure ? "Delete everything?" : "Clear"}
    </Button>
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
          {busy ? "Scanning..." : "Rescan"}
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

/** Facts about the user, given to every model, plus learned paths. */
export function Memory({ onError }: { onError: (e: string) => void }) {
  const memory = useSidekick((s) => s.settings.memory);
  const [draft, setDraft] = useState("");
  const [cleared, setCleared] = useState(false);
  const save = (next: string[]) => void updateSettings({ memory: next }).catch((e: unknown) => onError(String(e)));
  const add = () => {
    const v = draft.trim();
    if (v && !memory.includes(v)) save([...memory, v]);
    setDraft("");
  };
  return (
    <div className="flex flex-col gap-2 text-[13px]">
      {memory.length === 0 ? (
        <p className="text-(--muted)">Nothing yet.</p>
      ) : (
        <ul className="flex flex-col gap-1.5">
          {memory.map((m) => (
            <li key={m} className="flex items-center gap-2">
              <span className="min-w-0 flex-1">{m}</span>
              <Button small onClick={() => save(memory.filter((x) => x !== m))}>
                Forget
              </Button>
            </li>
          ))}
        </ul>
      )}
      <form
        className="flex gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          add();
        }}
      >
        <input
          aria-label="Something to remember"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          placeholder="Sign my emails as Ali"
          className="min-w-0 flex-1 rounded-lg border border-(--border) bg-transparent px-2.5 py-1.5 text-[13px] outline-none focus:border-[#0a84ff]"
        />
        <Button small primary disabled={!draft.trim()} onClick={add}>
          Add
        </Button>
      </form>
      <div className="flex items-center gap-2 text-[12px] text-(--muted)">
        <span className="flex-1">Sidekick also remembers which buttons worked in each app and site.</span>
        <Button
          small
          onClick={() =>
            void api
              .knowHowClear()
              .then(() => setCleared(true))
              .catch((e) => onError(String(e)))
          }
        >
          {cleared ? "Cleared" : "Clear"}
        </Button>
      </div>
    </div>
  );
}
