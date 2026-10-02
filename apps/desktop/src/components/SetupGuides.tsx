"use client";

// Inline one-click panels for welcome setup items. Prefer automation already
// in Rust (hooks merge, extension helper, detected folders, voice download)
// over paste-this guides.

import { useEffect, useState } from "react";
import { api, EVENTS, listen } from "@/lib/bridge";
import { updateSettings, useSidekick } from "@/lib/store";
import { BrowserInstallPanel } from "./SetupBrowser";

export function ItemGuide({ id, onDone }: { id: string; onDone: () => void }) {
  switch (id) {
    case "claude_hooks":
      return <HooksGuide onDone={onDone} />;
    case "claude_mcp":
      return <McpGuide onDone={onDone} />;
    case "browser":
      return (
        <div className="border-t border-white/10 pt-2">
          <BrowserInstallPanel onDone={onDone} />
        </div>
      );
    case "code_folders":
      return (
        <FoldersGuide
          label="Code folders"
          hint="Projects Sidekick watches for status and end-of-day."
          valueKey="codeFolders"
          detectKey="codeFolders"
          onDone={onDone}
        />
      );
    case "search_folders":
      return (
        <FoldersGuide
          label="Folders to search"
          hint="Documents and notes Sidekick can search from Ask mode."
          valueKey="indexFolders"
          detectKey="searchFolders"
          onDone={onDone}
        />
      );
    case "voice":
      return <VoiceGuide onDone={onDone} />;
    case "composio":
    case "calendar":
    case "fathom":
      return <ComposioGuide onDone={onDone} />;
    default:
      return (
        <p className="text-[12px] leading-relaxed text-[rgb(235_235_245/0.6)]">
          You can finish this later in Settings. Continue welcome for now.
        </p>
      );
  }
}

function HooksGuide({ onDone }: { onDone: () => void }) {
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const add = () => {
    setBusy(true);
    setError(null);
    void api
      .claudeAddHooks()
      .then((backup) => {
        setNote(backup ? `Added. Backup saved at ${backup}.` : "Added.");
        onDone();
      })
      .catch((e) => setError(String(e)))
      .finally(() => setBusy(false));
  };

  return (
    <div className="flex flex-col gap-2 border-t border-white/10 pt-2 text-[12px] text-[rgb(235_235_245/0.7)]">
      <p>Adds Sidekick&apos;s hooks to Claude Code settings. Your other hooks stay; a backup is written first.</p>
      <GuideButton primary disabled={busy} onClick={add}>
        {busy ? "Adding..." : "Add for me"}
      </GuideButton>
      {note && <p className="text-[11px] text-[#30d158]">{note}</p>}
      {error && <p className="text-[11px] text-[#ff453a]">{error}</p>}
    </div>
  );
}

function McpGuide({ onDone }: { onDone: () => void }) {
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const add = () => {
    setBusy(true);
    setError(null);
    void api
      .claudeAddMcp()
      .then(() => {
        setNote("Added to Claude Code.");
        onDone();
      })
      .catch((e) => {
        void api
          .setupRun("claude_mcp")
          .then(() => {
            setNote("Opened PowerShell to add Sidekick tools.");
            onDone();
          })
          .catch(() => setError(String(e)));
      })
      .finally(() => setBusy(false));
  };

  return (
    <div className="flex flex-col gap-2 border-t border-white/10 pt-2 text-[12px] text-[rgb(235_235_245/0.7)]">
      <p>Registers Sidekick&apos;s MCP server with Claude Code so it can search history and notify you.</p>
      <GuideButton primary disabled={busy} onClick={add}>
        {busy ? "Adding..." : "Add for me"}
      </GuideButton>
      {note && <p className="text-[11px] text-[#30d158]">{note}</p>}
      {error && <p className="text-[11px] text-[#ff453a]">{error}</p>}
    </div>
  );
}

function FoldersGuide({
  label,
  hint,
  valueKey,
  detectKey,
  onDone,
}: {
  label: string;
  hint: string;
  valueKey: "codeFolders" | "indexFolders";
  detectKey: "codeFolders" | "searchFolders";
  onDone: () => void;
}) {
  const settings = useSidekick((s) => s.settings);
  const current = settings[valueKey].join("\n");
  const [draft, setDraft] = useState(current);
  const [busy, setBusy] = useState(false);
  useEffect(() => setDraft(current), [current]);

  const useDetected = () => {
    setBusy(true);
    void api
      .setupDetect()
      .then((found) => {
        const paths = found[detectKey].map((f) => f.path);
        setDraft(paths.join("\n"));
        return updateSettings({ [valueKey]: paths }).then(onDone);
      })
      .finally(() => setBusy(false));
  };

  const save = () => {
    const folders = draft
      .split(/[\n;]/)
      .map((f) => f.trim())
      .filter(Boolean);
    void updateSettings({ [valueKey]: folders }).then(onDone);
  };

  return (
    <div className="flex flex-col gap-2 border-t border-white/10 pt-2">
      <p className="text-[12px] text-[rgb(235_235_245/0.6)]">{hint}</p>
      <div className="flex flex-wrap gap-1.5">
        <GuideButton primary disabled={busy} onClick={useDetected}>
          {busy ? "Detecting..." : "Use detected"}
        </GuideButton>
      </div>
      <textarea
        aria-label={label}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        rows={3}
        spellCheck={false}
        placeholder="C:\Users\you\code"
        className="resize-none rounded-lg border border-white/15 bg-black/25 px-2.5 py-2 font-mono text-[11px] text-white/90 outline-none focus:border-[#0a84ff]"
      />
      <GuideButton onClick={save}>Save folders</GuideButton>
    </div>
  );
}

function VoiceGuide({ onDone }: { onDone: () => void }) {
  const settings = useSidekick((s) => s.settings);
  const enable = () => {
    void updateSettings({ voice: { ...settings.voice, enabled: true } }).then(() => {
      void api.voiceDownload();
      onDone();
    });
  };
  return (
    <div className="flex flex-col gap-2 border-t border-white/10 pt-2 text-[12px] text-[rgb(235_235_245/0.65)]">
      <p>Downloads about 205 MB of speech models once. Audio stays on this PC.</p>
      <GuideButton primary onClick={enable}>
        Turn voice on
      </GuideButton>
    </div>
  );
}

function ComposioGuide({ onDone }: { onDone: () => void }) {
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const off = listen(EVENTS.composioChanged, (e) => {
      setNote(e.ok ? e.message : null);
      if (!e.ok) setError(e.message);
      onDone();
    });
    return () => void off.then((f) => f());
  }, [onDone]);
  const connect = () => {
    setBusy(true);
    setError(null);
    void api
      .composioSignIn()
      .then((code) => setNote(`Finish in your browser. The page shows ${code}.`))
      .catch((e) => setError(String(e)))
      .finally(() => setBusy(false));
  };

  return (
    <div className="flex flex-col gap-2 border-t border-white/10 pt-2 text-[12px] leading-relaxed text-[rgb(235_235_245/0.65)]">
      <p>Opens Composio sign-in in your browser. Calendar, Gmail and more connect from there.</p>
      <GuideButton primary disabled={busy} onClick={connect}>
        {busy ? "Opening..." : "Connect"}
      </GuideButton>
      {note && <p className="text-[11px] text-[#30d158]">{note}</p>}
      {error && <p className="text-[11px] text-[#ff453a]">{error}</p>}
    </div>
  );
}

function GuideButton({
  children,
  onClick,
  primary,
  disabled,
}: {
  children: string;
  onClick: () => void;
  primary?: boolean;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={`chip self-start rounded-full px-2.5 py-1 text-[12px] font-medium disabled:opacity-50 ${
        primary ? "bg-white text-black hover:bg-white/90" : "bg-white/12 text-white/90 hover:bg-white/20"
      }`}
    >
      {children}
    </button>
  );
}
