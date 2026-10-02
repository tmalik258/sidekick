"use client";

// AI: which providers answer and in what order, their models (picked from
// what is installed), SemIf ranking, and voice.

import { Reorder, useDragControls } from "motion/react";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { api, EVENTS, listen } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import { updateSettings, useSidekick } from "@/lib/store";
import {
  AI_PROVIDERS,
  type AiProviderId,
  type AiSettings,
  type LocalModels,
  PROVIDER_LABELS,
  type ProviderStatus,
  type VoiceDownload,
  type VoiceSettings,
} from "@/lib/types";
import { Button, Field, Section, Select, StatusDot, Switch, TextField, Toggle } from "./ui";

export function AiTab({ onError }: { onError: (e: string) => void }) {
  const ai = useSidekick((s) => s.settings.ai);
  const voice = useSidekick((s) => s.settings.voice);
  return (
    <>
      <Section
        title="AI"
        hint="Sidekick works fully without AI. Chat tries these top to bottom and falls back when one is not reachable."
        keywords="claude ollama anthropic local model provider order qwen"
      >
        <Providers ai={ai} onError={onError} />
      </Section>
      <Section
        title="Voice"
        hint="Speech runs on this PC. Audio is never saved or sent anywhere; only the words you say go to your AI, like a typed question."
        keywords="microphone speak talk wake word hey sidekick supertonic conversation"
      >
        <VoiceSection voice={voice} onError={onError} />
      </Section>
      <Section
        title="Ranking"
        hint="SemIf or the local model guesses which option you want and puts it first. Your past picks always win."
        keywords="semif decisions t1 order options"
      >
        <Ranking ai={ai} onError={onError} />
      </Section>
    </>
  );
}

const PROVIDER_HINTS: Record<AiProviderId, string> = {
  claude_code: "Your own Claude subscription through the claude CLI. Sidekick never reads its sign-in files.",
  anthropic: "Uses ANTHROPIC_API_KEY from your environment. The key is never stored.",
  local: "Ollama or any OpenAI-compatible server. Nothing leaves this PC.",
};

const CLAUDE_CODE_MODELS: [string, string][] = [
  ["", "Claude Code's default"],
  ["sonnet", "Sonnet"],
  ["opus", "Opus"],
  ["haiku", "Haiku"],
];

const ANTHROPIC_MODELS: [string, string][] = [
  ["claude-opus-5-5", "Opus 5.5"],
  ["claude-sonnet-5-5", "Sonnet 5.5"],
  ["claude-fable-5-1", "Fable 5.1"],
  ["claude-haiku-4-5-20251001", "Haiku 4.5"],
];

function Providers({ ai, onError }: { ai: AiSettings; onError: (e: string) => void }) {
  // Last known answers paint at once; dots and lists only change when the data does.
  const { data: status, refresh: reloadStatus } = useCached<ProviderStatus[]>("ai-status", api.aiStatus);
  const { data: models, refresh: reloadModels } = useCached<LocalModels>("local-models", api.localModels);
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
    id === "claude_code" ? ai.claudeCode.enabled : id === "anthropic" ? ai.anthropic.enabled : ai.local.enabled;
  const setEnabled = (id: AiProviderId, on: boolean) => {
    if (id === "claude_code") void save({ claudeCode: { ...ai.claudeCode, enabled: on } });
    else if (id === "anthropic") void save({ anthropic: { ...ai.anthropic, enabled: on } });
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
                  <Switch checked={enabled(id)} onChange={(on) => setEnabled(id, on)} label={PROVIDER_LABELS[id]} />
                </div>
                {id === "claude_code" && (
                  <>
                    <Field label="Model">
                      <Select
                        label="Claude Code model"
                        value={ai.claudeCode.model}
                        options={CLAUDE_CODE_MODELS}
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
                {id === "anthropic" && (
                  <Field label="Model">
                    <Select
                      label="Anthropic model"
                      value={ai.anthropic.model}
                      options={[["", "Default"], ...ANTHROPIC_MODELS]}
                      onChange={(model) => void save({ anthropic: { ...ai.anthropic, model } })}
                    />
                  </Field>
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
                      <Select
                        label="Local model"
                        value={ai.local.model}
                        options={chatModels}
                        onChange={(model) => void save({ local: { ...ai.local, model } })}
                      />
                    </Field>
                    <Field
                      label="Vision model"
                      hint="For pictures without text, e.g. moondream (about 1.7 GB). Off reads the screen as text, which is faster."
                    >
                      <Select
                        label="Vision model"
                        value={ai.local.visionModel}
                        options={[
                          ["", "Off (read text)"],
                          ...(models?.chat ?? []).map((m) => [m, m] as [string, string]),
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
        hint="Scores options from a small model's logits. Runs semif-score natively or inside WSL."
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
        label="Talk to Sidekick"
        hint={voice.wakeWord ? 'Say "Hey Sidekick", then your question.' : "Use the mic button or the Talk shortcut."}
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
              ? "Listening for Hey Sidekick."
              : "Ready. Use the mic button or the Talk shortcut."
            : "Starting the microphone..."}
        </p>
      )}
      {status?.error && <p className="text-[12px] text-red-400">{status.error}</p>}
      <Toggle
        label="Wake word"
        hint="Listens for Hey Sidekick on this PC. Off pauses the microphone until you press the mic button."
        checked={voice.wakeWord}
        onChange={(wakeWord) => void set({ wakeWord })}
      />
      <Toggle
        label="Read answers aloud"
        hint="When you asked by voice."
        checked={voice.speakAnswers}
        onChange={(speakAnswers) => void set({ speakAnswers })}
      />
      <Toggle
        label="Keep the conversation going"
        hint="After an answer is read out, listen for your reply. No need to say Hey Sidekick again."
        checked={voice.conversation}
        onChange={(conversation) => void set({ conversation })}
      />
      <Toggle
        label="Read suggestions aloud"
        hint='Hear what Sidekick noticed and answer by voice: "open", "the second one", or "not now".'
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
          <Button small onClick={() => api.voiceTest().catch((e) => onError(String(e)))}>
            Test
          </Button>
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
    <button
      type="button"
      aria-label="Drag to reorder, or use the arrow keys"
      title="Drag to reorder"
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
