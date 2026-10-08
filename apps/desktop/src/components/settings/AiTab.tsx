"use client";

// AI: provider order, explicit model choices, local model discovery, SemIf, and voice.

import { Reorder, useDragControls } from "motion/react";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { CLAUDE_MODELS, CODEX_MODELS, explicitModel, FAST_CLAUDE_MODEL, FAST_CODEX_MODEL } from "@/lib/ai-models";
import { api, EVENTS, listen, type RouterModel } from "@/lib/bridge";
import { SETUP_STATUS_CACHE_KEY, useCached } from "@/lib/cache";
import { updateSettings, useAssistantName, useSidekick } from "@/lib/store";
import {
  AI_PROVIDERS,
  type AiProviderId,
  type AiSettings,
  CLOUD_IDS,
  type CloudId,
  type LocalModels,
  PROVIDER_LABELS,
  type ProviderStatus,
  type SetupStatus,
  type VoiceDownload,
  type VoiceSettings,
} from "@/lib/types";
import { SetupItems } from "../SetupChecklist";
import { Tip } from "../Tip";
import { Button, Field, Section, Segmented, Select, StatusDot, Switch, TextField, Toggle } from "./ui";

/** Shown instead of On/Off while a model cannot be used. */
const NOT_READY: Record<AiProviderId, string> = {
  local: "Not running",
  claude_code: "Not installed",
  codex: "Not installed",
  anthropic: "No key",
  gemini: "No key",
  groq: "No key",
  openrouter: "No key",
};

export function AiTab({ onError }: { onError: (e: string) => void }) {
  const ai = useSidekick((s) => s.settings.ai);
  const voice = useSidekick((s) => s.settings.voice);
  return (
    <>
      <Section
        title="Models"
        hint="Ask tries them top to bottom. Drag to reorder."
        keywords="ai claude codex ollama anthropic local model provider order qwen hooks mcp coding agent"
      >
        <Providers ai={ai} onError={onError} />
      </Section>
      <Section
        title="Voice"
        hint="Runs on this PC. Audio is never saved or sent."
        keywords="microphone speak talk wake word hey sidekick supertonic conversation"
      >
        <VoiceSection voice={voice} onError={onError} />
      </Section>
      <Section
        collapsible
        summary={ai.decisions ? "Best option first" : "Off"}
        title="Ranking"
        keywords="semif decisions t1 order options"
      >
        <Ranking ai={ai} onError={onError} />
      </Section>
    </>
  );
}

const PROVIDER_HINTS: Record<AiProviderId, string> = {
  claude_code: "Your Claude plan, through the claude CLI.",
  codex: "Your ChatGPT plan, through the codex CLI.",
  anthropic: "Pay as you go with ANTHROPIC_API_KEY.",
  local: "Ollama on this PC. Nothing leaves it.",
  gemini: "Free with a Google account. Questions may be used to train Google's models.",
  groq: "Free and fast; takes over when Gemini is busy. Questions may be used for training.",
  openrouter: "One key for hundreds of models, each with its price. Some are free.",
};

/** Setup steps shown inside each provider's card, until they are done. */
const PROVIDER_SETUP: Record<AiProviderId, string[]> = {
  claude_code: ["claude_code", "claude_hooks", "claude_mcp"],
  codex: ["codex", "codex_notify", "codex_mcp"],
  anthropic: ["anthropic"],
  local: ["ollama", "ollama_chat", "ollama_embed", "ollama_vision", "ollama_light"],
  gemini: [],
  groq: [],
  openrouter: [],
};

/** Where each cloud provider hands out keys, and its default model. */
const CLOUD_KEYS: Record<CloudId, { url: string; site: string; model: string }> = {
  gemini: { url: "https://aistudio.google.com/apikey", site: "Google AI Studio", model: "gemini-2.5-flash" },
  groq: { url: "https://console.groq.com/keys", site: "GroqCloud", model: "llama-3.3-70b-versatile" },
  openrouter: { url: "https://openrouter.ai/settings/keys", site: "OpenRouter", model: "openrouter/auto" },
};

const CODING_AGENTS: [string, string][] = [
  ["auto", "Higher of Claude Code / Codex in the list above"],
  ["claude_code", "Claude Code"],
  ["codex", "Codex"],
];

function Providers({ ai, onError }: { ai: AiSettings; onError: (e: string) => void }) {
  // Last known answers paint at once; dots and lists only change when the data does.
  const { data: status, refresh: reloadStatus } = useCached<ProviderStatus[]>("ai-status", api.aiStatus);
  const { data: models, refresh: reloadModels } = useCached<LocalModels>("local-models", api.localModels);
  const { data: setup } = useCached<SetupStatus>(SETUP_STATUS_CACHE_KEY, api.setupStatus);
  const ollama = setup?.items.find((i) => i.id === "ollama");
  const [starting, setStarting] = useState<string | null>(null);
  // Start (or install) Ollama from here, then wait for it to answer.
  const startOllama = async () => {
    setStarting(ollama?.opensApp ? "Starting..." : "Installing...");
    try {
      await api.setupRun("ollama");
      for (let i = 0; i < 40; i++) {
        await new Promise((r) => setTimeout(r, 1500));
        const m = await api.localModels();
        if (m.reachable) break;
      }
    } catch {
      setStarting("Could not start it. Try again");
      return;
    }
    setStarting(null);
    reloadModels();
    reloadStatus();
  };
  const first = useRef(true);
  // biome-ignore lint/correctness/useExhaustiveDependencies: re-check when the local server address changes
  useEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    void reloadStatus().catch(() => undefined);
    void reloadModels().catch(() => undefined);
  }, [ai.local.baseUrl]);

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
    id === "claude_code"
      ? ai.claudeCode.enabled
      : id === "codex"
        ? ai.codex.enabled
        : id === "anthropic"
          ? ai.anthropic.enabled
          : isCloud(id)
            ? ai[id].enabled
            : ai.local.enabled;
  const setEnabled = (id: AiProviderId, on: boolean) => {
    if (id === "claude_code") void save({ claudeCode: { ...ai.claudeCode, enabled: on } });
    else if (id === "codex") void save({ codex: { ...ai.codex, enabled: on } });
    else if (id === "anthropic") void save({ anthropic: { ...ai.anthropic, enabled: on } });
    else if (isCloud(id)) void save({ [id]: { ...ai[id], enabled: on } });
    else void save({ local: { ...ai.local, enabled: on } });
  };
  const saved = ai.order.filter((id) => AI_PROVIDERS.includes(id));
  // While dragging, the list follows the cursor; it is saved on drop.
  const [dragged, setDragged] = useState<AiProviderId[] | null>(null);
  const order = dragged ?? saved;
  const drop = () => {
    if (dragged) void save({ order: dragged });
    setDragged(null);
  };
  const chatModels: [string, string][] = [
    ["", models?.chat.length ? `Newest (${models.chat[0]})` : "Newest installed"],
    ...(models?.chat ?? []).map((m) => [m, m] as [string, string]),
  ];

  return (
    <div className="flex flex-col gap-3">
      <Reorder.Group as="ol" axis="y" values={order} onReorder={setDragged} className="flex flex-col gap-3">
        {order.map((id, i) => (
          <DragRow key={id} value={id} onDrop={drop} onStep={(delta) => move(id, delta)}>
            {(handle) => (
              <>
                <div className="flex items-center gap-3">
                  {handle}
                  <StatusDot state={status === null ? "checking" : available(id) ? "ok" : "off"} />
                  <div className="min-w-0 flex-1">
                    <p className="text-[14px] font-medium">
                      {i + 1}. {PROVIDER_LABELS[id]}
                    </p>
                    <p className="text-[12px] text-(--muted)">{PROVIDER_HINTS[id]}</p>
                  </div>
                  {status !== null && !available(id) ? (
                    // Nothing to switch on yet: the step below sets it up.
                    <span className="shrink-0 text-[12.5px] text-(--muted)">
                      {id === "claude_code" && setup?.items.find((x) => x.id === "claude_code")?.done
                        ? "Out of usage"
                        : NOT_READY[id]}
                    </span>
                  ) : (
                    <Switch checked={enabled(id)} onChange={(on) => setEnabled(id, on)} label={PROVIDER_LABELS[id]} />
                  )}
                </div>
                <SetupItems ids={PROVIDER_SETUP[id]} />
                {id === "claude_code" && (
                  <>
                    <Field label="Model" hint="Availability depends on your plan and CLI version.">
                      <Select
                        label="Claude Code model"
                        value={explicitModel(ai.claudeCode.model, FAST_CLAUDE_MODEL)}
                        options={CLAUDE_MODELS}
                        onChange={(model) => void save({ claudeCode: { ...ai.claudeCode, model } })}
                      />
                    </Field>
                    <details className="text-[12.5px]">
                      <summary className="cursor-pointer text-(--muted)">Where claude is</summary>
                      <div className="mt-2">
                        <Field label="Path to claude" hint="Empty finds it on PATH">
                          <TextField
                            label="Path to claude"
                            value={ai.claudeCode.path}
                            placeholder="claude"
                            mono
                            className="w-56"
                            onCommit={(path) => save({ claudeCode: { ...ai.claudeCode, path } })}
                          />
                        </Field>
                      </div>
                    </details>
                  </>
                )}
                {id === "codex" && (
                  <>
                    <Field label="Model" hint="Availability depends on your plan and CLI version.">
                      <Select
                        label="Codex model"
                        value={explicitModel(ai.codex.model, FAST_CODEX_MODEL)}
                        options={CODEX_MODELS}
                        onChange={(model) => void save({ codex: { ...ai.codex, model } })}
                      />
                    </Field>
                    <details className="text-[12.5px]">
                      <summary className="cursor-pointer text-(--muted)">Where codex is</summary>
                      <div className="mt-2">
                        <Field label="Path to codex" hint="Empty finds it on PATH">
                          <TextField
                            label="Path to codex"
                            value={ai.codex.path}
                            placeholder="codex"
                            mono
                            className="w-56"
                            onCommit={(path) => save({ codex: { ...ai.codex, path } })}
                          />
                        </Field>
                      </div>
                    </details>
                  </>
                )}
                {id === "anthropic" && (
                  <Field label="Model">
                    <Select
                      label="Anthropic model"
                      value={explicitModel(ai.anthropic.model, FAST_CLAUDE_MODEL)}
                      options={CLAUDE_MODELS}
                      onChange={(model) => void save({ anthropic: { ...ai.anthropic, model } })}
                    />
                  </Field>
                )}
                {isCloud(id) && (
                  <CloudCard
                    id={id}
                    model={ai[id].model}
                    onModel={(model) => void save({ [id]: { ...ai[id], model } })}
                    onChanged={() => void reloadStatus().catch(() => undefined)}
                    onError={onError}
                  />
                )}
                {id === "local" && (
                  <>
                    <Field
                      label="Model"
                      hint={
                        models === null
                          ? "\u00a0"
                          : models.reachable
                            ? `${models.chat.length} chat ${models.chat.length === 1 ? "model" : "models"} installed`
                            : "Ollama is not running"
                      }
                    >
                      {models && !models.reachable ? (
                        <Button primary onClick={() => void startOllama()} disabled={starting?.endsWith("...")}>
                          {starting ??
                            (ollama && !ollama.opensApp && ollama.runnable ? "Install Ollama" : "Start Ollama")}
                        </Button>
                      ) : (
                        <Select
                          label="Local model"
                          value={ai.local.model}
                          options={chatModels}
                          onChange={(model) => void save({ local: { ...ai.local, model } })}
                        />
                      )}
                    </Field>
                    <Field label="Vision model" hint="For pictures without text">
                      <Select
                        label="Vision model"
                        value={!ai.local.visionModel || ai.local.visionModel === "off" ? "off" : ai.local.visionModel}
                        options={[
                          ["off", "Off (read text)"],
                          ...(models?.vision ?? []).map((m) => [m, m] as [string, string]),
                        ]}
                        onChange={(visionModel) => void save({ local: { ...ai.local, visionModel } })}
                      />
                    </Field>
                    <details className="text-[12.5px]">
                      <summary className="cursor-pointer text-(--muted)">Server</summary>
                      <div className="mt-2">
                        <Field label="Server URL">
                          <TextField
                            label="Server URL"
                            value={ai.local.baseUrl}
                            mono
                            className="w-56"
                            onCommit={(baseUrl) => save({ local: { ...ai.local, baseUrl } })}
                          />
                        </Field>
                      </div>
                    </details>
                  </>
                )}
              </>
            )}
          </DragRow>
        ))}
      </Reorder.Group>
      <Field label="Coding agent" hint="Makes changes for Ask">
        <Select
          label="Coding agent"
          value={ai.codingAgent}
          options={CODING_AGENTS}
          onChange={(codingAgent) => void save({ codingAgent })}
        />
      </Field>
      <div>
        <Button
          small
          onClick={() => {
            void reloadStatus().catch(() => undefined);
            void reloadModels().catch(() => undefined);
          }}
        >
          Check again
        </Button>
      </div>
    </div>
  );
}

function isCloud(id: string): id is CloudId {
  return (CLOUD_IDS as string[]).includes(id);
}

/** A cloud provider's key (kept in Credential Manager) and model. */
function CloudCard({
  id,
  model,
  onModel,
  onChanged,
  onError,
}: {
  id: CloudId;
  model: string;
  onModel: (model: string) => void;
  onChanged: () => void;
  onError: (e: string) => void;
}) {
  const { data: keys, refresh } = useCached<Record<CloudId, boolean>>("cloud-keys", api.cloudKeys);
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const info = CLOUD_KEYS[id];
  const has = keys?.[id] ?? false;
  const done = () => {
    setKey("");
    void refresh().catch(() => undefined);
    onChanged();
  };
  const saveKey = () => {
    setBusy(true);
    api
      .cloudKeySet(id, key)
      .then(done, (e: unknown) => onError(String(e)))
      .finally(() => setBusy(false));
  };
  return (
    <>
      {has ? (
        <Field label="Key" hint="Saved in Windows Credential Manager">
          <Button small onClick={() => void api.cloudKeyClear(id).then(done, (e: unknown) => onError(String(e)))}>
            Remove key
          </Button>
        </Field>
      ) : (
        <Field
          label="Key"
          hint={
            <>
              Free from{" "}
              <button type="button" className="underline" onClick={() => void api.aiOpenLink(info.url)}>
                {info.site}
              </button>
            </>
          }
        >
          <div className="flex items-center gap-2">
            <input
              type="password"
              aria-label={`${PROVIDER_LABELS[id]} key`}
              value={key}
              placeholder="Paste key"
              autoComplete="off"
              onChange={(e) => setKey(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && key.trim()) saveKey();
              }}
              className="w-40 rounded-lg border border-(--border) bg-transparent px-2 py-1 font-mono text-[12.5px]"
            />
            <Button small primary onClick={saveKey} disabled={busy || !key.trim()}>
              {busy ? "Checking..." : "Save"}
            </Button>
          </div>
        </Field>
      )}
      {id === "openrouter" ? (
        <RouterModels value={model || info.model} onChange={onModel} onError={onError} />
      ) : (
        <Field label="Model" hint={`Empty uses ${info.model}`}>
          <TextField
            label={`${PROVIDER_LABELS[id]} model`}
            value={model}
            placeholder={info.model}
            mono
            className="w-56"
            onCommit={onModel}
          />
        </Field>
      )}
    </>
  );
}

/** OpenRouter's models with search, prices and a Free only filter. */
function RouterModels({
  value,
  onChange,
  onError,
}: {
  value: string;
  onChange: (id: string) => void;
  onError: (e: string) => void;
}) {
  const [models, setModels] = useState<RouterModel[] | null>(null);
  const [query, setQuery] = useState("");
  const [freeOnly, setFreeOnly] = useState(false);
  const [open, setOpen] = useState(false);
  const load = () => {
    setOpen(true);
    if (!models) api.openrouterModels().then(setModels, (e: unknown) => onError(String(e)));
  };
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const shown = (models ?? [])
    .filter((m) => !freeOnly || m.free)
    .filter((m) => words.every((w) => `${m.id} ${m.name}`.toLowerCase().includes(w)))
    .slice(0, 60);
  const price = (m: RouterModel) =>
    m.input < 0 ? "Price varies" : m.free ? "Free" : `$${m.input.toFixed(2)} in, $${m.output.toFixed(2)} out`;
  const current = models?.find((m) => m.id === value);
  return (
    <div className="flex flex-col gap-2">
      <Field label="Model" hint={current ? price(current) : "Prices are per million tokens"}>
        <Button small onClick={() => (open ? setOpen(false) : load())}>
          <span className="max-w-48 truncate font-mono">{value}</span>
        </Button>
      </Field>
      {open && (
        <div className="flex flex-col gap-2 rounded-xl border border-(--border) p-2">
          <div className="flex items-center gap-2">
            <input
              type="search"
              aria-label="Search models"
              placeholder="Search models"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              className="min-w-0 flex-1 rounded-lg border border-(--border) bg-transparent px-2 py-1 text-[12.5px]"
            />
            <span className="flex shrink-0 items-center gap-1.5 text-[12.5px]">
              Free only
              <Switch checked={freeOnly} onChange={setFreeOnly} label="Free only" />
            </span>
          </div>
          {freeOnly && (
            <p className="text-[12px] text-(--muted)">
              Free models allow 20 questions a minute and 50 a day, or 1,000 a day once $10 of credits were ever bought.
            </p>
          )}
          <ul className="settings-scroll flex max-h-56 flex-col overflow-y-auto">
            {models === null && <li className="px-2 py-1 text-[12.5px] text-(--muted)">Loading...</li>}
            {shown.map((m) => (
              <li key={m.id}>
                <button
                  type="button"
                  aria-pressed={m.id === value}
                  onClick={() => {
                    onChange(m.id);
                    setOpen(false);
                  }}
                  className="flex w-full items-baseline justify-between gap-3 rounded-lg px-2 py-1 text-left text-[12.5px] hover:bg-(--border) aria-pressed:bg-(--border)"
                >
                  <span className="min-w-0 truncate">
                    {m.name}
                    {!m.tools && <span className="text-(--muted)"> · no tools</span>}
                  </span>
                  <span className="shrink-0 font-mono text-[11.5px] text-(--muted)">{price(m)}</span>
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
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

function Ranking({ ai, onError }: { ai: AiSettings; onError: (e: string) => void }) {
  const save = (next: Partial<AiSettings>) =>
    updateSettings({ ai: { ...ai, ...next } }).catch((e: unknown) => onError(String(e)));
  return (
    <>
      <Toggle
        label="Rank suggestion options"
        checked={ai.decisions}
        onChange={(decisions) => void save({ decisions })}
      />
      <Toggle
        label="Use SemIf"
        checked={ai.semif.enabled}
        onChange={(enabled) => void save({ semif: { ...ai.semif, enabled } })}
      />
      {ai.semif.enabled && (
        <>
          <Field label="Command" hint="e.g. wsl.exe -d Ubuntu-22.04 -- /home/me/semif/.venv/bin/semif-score">
            <TextField
              label="Command"
              value={joinArgs(ai.semif.command)}
              mono
              className="w-64"
              onCommit={(line) => save({ semif: { ...ai.semif, command: splitArgs(line) } })}
            />
          </Field>
          <Field label="Backend">
            <Select
              label="Backend"
              value={ai.semif.backend}
              options={[
                ["llamacpp", "llama.cpp (GGUF)"],
                ["torch", "PyTorch"],
                ["mlx", "MLX"],
              ]}
              onChange={(backend) => void save({ semif: { ...ai.semif, backend } })}
            />
          </Field>
          <Field label="Model">
            <TextField
              label="Model"
              value={ai.semif.model}
              mono
              className="w-64"
              onCommit={(model) => save({ semif: { ...ai.semif, model } })}
            />
          </Field>
          {ai.semif.backend === "llamacpp" && (
            <Field label="GGUF file" hint="Path as SemIf sees it">
              <TextField
                label="GGUF file"
                value={ai.semif.gguf}
                mono
                className="w-64"
                onCommit={(gguf) => save({ semif: { ...ai.semif, gguf } })}
              />
            </Field>
          )}
        </>
      )}
    </>
  );
}

function VoiceSection({ voice, onError }: { voice: VoiceSettings; onError: (e: string) => void }) {
  const name = useAssistantName();
  const status = useSidekick((s) => s.voiceStatus);
  const [progress, setProgress] = useState<VoiceDownload | null>(null);
  useEffect(() => {
    void api.voiceStatus().then((voiceStatus) => useSidekick.setState({ voiceStatus }));
    const off = listen(EVENTS.voiceDownload, (p) => {
      setProgress(p.finished ? null : p);
      if (p.error) onError(`Voice download: ${p.error}`);
    });
    return () => {
      void off.then((f) => f());
    };
  }, [onError]);
  const set = (patch: Partial<VoiceSettings>) =>
    updateSettings({ voice: { ...voice, ...patch } }).catch((e) => onError(String(e)));
  const missing = status?.missingBytes ?? 0;
  const downloading = status?.downloading || progress !== null;
  const mb = (n: number) => `${Math.round(n / 1_000_000)} MB`;

  return (
    <>
      <Toggle
        label={`Talk to ${name}`}
        hint={voice.wakeWord ? `Say "Hey ${name}"` : "Use the mic button or the Talk shortcut"}
        checked={voice.enabled}
        onChange={(enabled) => {
          void set({ enabled });
          if (enabled && missing > 0 && !downloading) api.voiceDownload().catch((e) => onError(String(e)));
        }}
      />
      {missing > 0 && (
        <div className="flex items-center justify-between gap-4 text-[13px]">
          <span className="min-w-0">
            {downloading && progress
              ? `Downloading ${progress.label}: ${mb(progress.done)} of ${mb(progress.total)}`
              : `Speech models are not downloaded yet (${mb(missing)}, once).`}
            {downloading && progress && (
              <span className="mt-1.5 block h-1 overflow-hidden rounded-full bg-white/10">
                <span
                  className="block h-full rounded-full bg-[#0a84ff] transition-[width] duration-300"
                  style={{ width: `${Math.round((progress.done / Math.max(1, progress.total)) * 100)}%` }}
                />
              </span>
            )}
          </span>
          {downloading ? (
            <Button small onClick={() => void api.voiceCancelDownload()}>
              Cancel
            </Button>
          ) : (
            <Button small onClick={() => api.voiceDownload().catch((e) => onError(String(e)))}>
              Download
            </Button>
          )}
        </div>
      )}
      {voice.enabled && missing === 0 && (
        <p className="text-[12px] text-(--muted)">
          {status?.listening
            ? voice.wakeWord
              ? `Listening for Hey ${name}.`
              : "Ready. Use the mic button or the Talk shortcut."
            : "Starting the microphone..."}
        </p>
      )}
      {status?.error && <p className="text-[12px] text-red-400">{status.error}</p>}
      <Toggle
        label="Wake word"
        hint="Off: only the mic button listens"
        checked={voice.wakeWord}
        onChange={(wakeWord) => void set({ wakeWord })}
      />
      <div className="flex items-center justify-between gap-4 text-[14px]">
        <div className="min-w-0">
          <p>While you talk</p>
          <p className="text-[12px] text-(--muted)">
            Compact is one slim line with your latest words. Full shows a waveform and your words in larger text. Both
            open Ask when the answer is ready.
          </p>
        </div>
        <Segmented
          label="While you talk"
          value={voice.listeningStyle ?? "compact"}
          options={[
            ["compact", "Compact"],
            ["full", "Full"],
          ]}
          onChange={(listeningStyle) => void set({ listeningStyle })}
        />
      </div>
      <Toggle
        label="Speak replies"
        hint="Also the speaker button next to the mic, Alt S."
        checked={voice.speakAnswers}
        onChange={(speakAnswers) => void set({ speakAnswers })}
      />
      <Toggle
        label="Interrupt by talking"
        hint="Talk over an answer to stop it. Works best with headphones."
        checked={voice.interrupt}
        onChange={(interrupt) => void set({ interrupt })}
      />
      <Toggle
        label="Keep the conversation going"
        hint="Listen for your reply after an answer"
        checked={voice.conversation}
        onChange={(conversation) => void set({ conversation })}
      />
      <Toggle
        label="Read suggestions aloud"
        hint='Answer with "open" or "not now"'
        checked={voice.speakSuggestions}
        onChange={(speakSuggestions) => void set({ speakSuggestions })}
      />
      <Field label="Voice">
        <div className="flex items-center gap-2">
          <Select
            label="Voice"
            value={voice.voice}
            options={(status?.voices ?? []).map((v) => [v.id, v.label])}
            onChange={(v) => void set({ voice: v })}
          />
          <VoiceTest onError={onError} />
        </div>
      </Field>
      <Field label={`Speed ${voice.speed.toFixed(1)}x`}>
        <input
          type="range"
          min={0.7}
          max={1.5}
          step={0.1}
          value={voice.speed}
          aria-label="Speech speed"
          onChange={(e) => void set({ speed: Number(e.target.value) })}
          className="w-36 accent-(--accent)"
        />
      </Field>
    </>
  );
}

/** A provider row you can drag by its handle; arrow keys on the handle move it too. */
function DragRow({
  value,
  onDrop,
  onStep,
  children,
}: {
  value: AiProviderId;
  onDrop: () => void;
  onStep: (delta: number) => void;
  children: (handle: ReactNode) => ReactNode;
}) {
  const controls = useDragControls();
  const handle = (
    <Tip label="Drag to reorder">
      <button
        type="button"
        aria-label="Drag to reorder, or use the arrow keys"
        onPointerDown={(e) => controls.start(e)}
        onKeyDown={(e) => {
          if (e.key === "ArrowUp" || e.key === "ArrowDown") {
            e.preventDefault();
            onStep(e.key === "ArrowUp" ? -1 : 1);
          }
        }}
        className="chip grid h-8 w-5 shrink-0 cursor-grab touch-none place-items-center rounded-md text-white/40 hover:text-white/80 active:cursor-grabbing"
      >
        <svg aria-hidden="true" viewBox="0 0 8 14" className="h-3.5 w-2">
          <g fill="currentColor">
            <circle cx="2" cy="2" r="1.2" />
            <circle cx="6" cy="2" r="1.2" />
            <circle cx="2" cy="7" r="1.2" />
            <circle cx="6" cy="7" r="1.2" />
            <circle cx="2" cy="12" r="1.2" />
            <circle cx="6" cy="12" r="1.2" />
          </g>
        </svg>
      </button>
    </Tip>
  );
  return (
    <Reorder.Item
      as="li"
      value={value}
      dragListener={false}
      dragControls={controls}
      onDragEnd={onDrop}
      className="flex flex-col gap-2 rounded-xl bg-(--surface) p-3 ring-1 ring-(--border) ring-inset"
    >
      {children(handle)}
    </Reorder.Item>
  );
}

/** Test with a loading state while a new voice loads, then Playing. */
function VoiceTest({ onError }: { onError: (e: string) => void }) {
  const [loading, setLoading] = useState(false);
  const speaking = useSidekick((s) => s.speaking);
  const [asked, setAsked] = useState(false);
  useEffect(() => {
    if (!speaking && asked && !loading) setAsked(false);
  }, [speaking, asked, loading]);
  const playing = asked && speaking;
  return (
    <Button
      small
      disabled={loading}
      onClick={() => {
        setLoading(true);
        setAsked(true);
        api
          .voiceTest()
          .catch((e) => {
            setAsked(false);
            onError(String(e));
          })
          .finally(() => setLoading(false));
      }}
    >
      {loading ? "Loading voice…" : playing ? "Playing…" : "Test"}
    </Button>
  );
}
