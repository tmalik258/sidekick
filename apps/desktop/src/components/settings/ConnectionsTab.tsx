"use client";

// Connections: Composio (one sign-in brings every app in the account), the calendar it
// feeds, the browser extension per browser, and Claude Code.

import { useCallback, useEffect, useState } from "react";
import { api, EVENTS, listen } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import { installExtension, startWaiting, updateSettings, useSidekick } from "@/lib/store";
import type {
  BrowserInfo,
  BrowserStatus,
  CalendarToday,
  ComposioStatus,
  ExtensionGuide,
  McpInfo,
  SetupStatus,
} from "@/lib/types";
import { Button, CopyButton, Field, Section, Select, TextField, Toggle } from "./ui";

export function ConnectionsTab({ onError }: { onError: (e: string) => void }) {
  return (
    <>
      <Section
        title="Composio"
        hint="One sign-in, the same Composio account as Claude. Every app connected there (calendar, mail, Slack, Jira and more) works in Sidekick: meeting reminders, the morning brief, and reading them in Ask mode. Changes always go through your coding agent (Claude Code or Codex), which asks first."
        keywords="apps accounts jira slack gmail notion trello github linear fathom outlook login sign in"
      >
        <ComposioCard onError={onError} />
      </Section>
      <Section
        title="Calendar"
        hint="Meetings come from Google Calendar or Outlook on Composio."
        keywords="meetings reminders join google outlook today"
      >
        <CalendarCard onError={onError} />
      </Section>
      <Section
        title="Browser"
        hint="The extension only talks to Sidekick on this PC. Logins are filled from your own password manager CLI and never stored."
        keywords="extension chrome edge firefox zen brave pairing code tabs"
      >
        <BrowserCard />
      </Section>
      <Section
        title="Claude Code"
        hint="Hooks let Sidekick hear when Claude Code finishes or asks for permission. Sidekick's tools let Claude Code search your history and show notes on the island."
        keywords="hooks mcp permission settings.json terminal"
      >
        <ClaudeCard onError={onError} />
      </Section>
      <Section
        title="Codex"
        hint="OpenAI's coding agent, if you use it instead of Claude Code or next to it. Notifications tell Sidekick when a turn is done; Sidekick's tools let Codex search your history and show notes on the island."
        keywords="openai codex chatgpt notify mcp config.toml terminal"
      >
        <CodexCard onError={onError} />
      </Section>
    </>
  );
}

function ComposioCard({ onError }: { onError: (e: string) => void }) {
  const composio = useSidekick((s) => s.settings.composio);
  const justDone = useSidekick((s) => s.justDone);
  const { data: status, refresh: reload } = useCached<ComposioStatus>("composio-status", api.composioStatus);
  const [waiting, setWaiting] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [connecting, setConnecting] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const refresh = useCallback(() => {
    setBusy(true);
    void reload()
      .catch(() => undefined)
      .finally(() => setBusy(false));
  }, [reload]);
  useEffect(() => {
    const off = listen(EVENTS.composioChanged, ({ ok, message }) => {
      setWaiting(false);
      setConnecting(null);
      setNote(message);
      if (!ok) onError(message);
      refresh();
    });
    return () => {
      void off.then((f) => f());
    };
  }, [refresh, onError]);

  const signIn = () => {
    setNote(null);
    setWaiting(true);
    api
      .composioSignIn()
      .then(() =>
        startWaiting("composio", "Composio", {
          resumeTab: "connections",
          steps: [
            "Composio opened in your browser.",
            "Sign in and press Allow, the same as in Claude.",
            "Come back here; every app you connected there shows up by itself.",
          ],
          again: signIn,
        }),
      )
      .catch((e) => {
        setWaiting(false);
        onError(String(e));
      });
  };

  // Only on the very first run; afterwards the last known state paints at once.
  if (!status) return <div className="h-9" aria-busy="true" />;

  if (!status.signedIn) {
    return (
      <div className="flex flex-col gap-3 text-[13px]">
        {waiting ? (
          <div className="flex flex-col gap-1.5 rounded-xl bg-[#0a84ff]/10 p-3">
            <p className="font-medium">Finish in the browser</p>
            <p className="text-(--muted)">
              Sign in to Composio and press Allow. Every app you already connected there (Gmail, Calendar, Slack and the
              rest) comes with it.
            </p>
            <div>
              <Button small onClick={signIn}>
                Open it again
              </Button>
            </div>
          </div>
        ) : (
          <div className="flex items-center gap-3">
            <p className="min-w-0 flex-1 text-(--muted)">
              One connection for all your apps. Uses the same Composio account as Claude.
            </p>
            <Button primary onClick={signIn}>
              Connect Composio
            </Button>
          </div>
        )}
        <OtherWays onError={onError} />
      </div>
    );
  }

  const connected = status.apps.filter((a) => a.connected || justDone === `app:${a.slug}`);
  const missing = status.apps.filter((a) => !a.connected && a.why && justDone !== `app:${a.slug}`);
  return (
    <div className="flex flex-col gap-3 text-[13px]">
      <div className="flex items-center gap-3">
        <p className="min-w-0 flex-1">
          Connected
          <span className="block text-[12px] text-(--muted)">
            {connected.length === 0
              ? "No apps connected in your Composio account yet."
              : `${connected.length} ${connected.length === 1 ? "app" : "apps"} from your Composio account`}
          </span>
        </p>
        <Button small onClick={refresh} disabled={busy}>
          {busy ? "Checking..." : "Refresh"}
        </Button>
        <Button
          small
          onClick={() =>
            void api
              .composioSignOut()
              .then(refresh)
              .catch((e) => onError(String(e)))
          }
        >
          Disconnect
        </Button>
      </div>
      {status.error && (
        <p className="text-[12px] text-(--muted)">Could not check just now, showing the last list. {status.error}</p>
      )}
      {connected.length > 0 && (
        <ul className="flex flex-wrap gap-1.5">
          {connected.map((a) => (
            <li
              key={a.slug}
              title={a.why || undefined}
              className={`flex items-center gap-1.5 rounded-full px-2.5 py-1 text-[12.5px] ${
                justDone === `app:${a.slug}` ? "bg-[#30d158]/25" : "bg-black/5 dark:bg-white/10"
              }`}
            >
              <span className="size-1.5 rounded-full bg-[#30d158]" />
              {a.name}
            </li>
          ))}
        </ul>
      )}
      {missing.length > 0 && (
        <details className="text-[12.5px]">
          <summary className="cursor-pointer text-(--muted)">
            Sidekick can also use {missing.map((a) => a.name).join(", ")}
          </summary>
          <ul className="mt-2 flex flex-col gap-1.5">
            {missing.map((a) => (
              <li key={a.slug} className="flex items-center gap-2.5 rounded-xl border border-(--border) px-2.5 py-2">
                <span className="min-w-0 flex-1">
                  <span className="block truncate font-medium">{a.name}</span>
                  <span className="block truncate text-[11px] text-(--muted)">{a.why}</span>
                </span>
                <Button
                  small
                  disabled={connecting === a.slug}
                  onClick={() => {
                    setConnecting(a.slug);
                    setNote(`Finish connecting ${a.name} in the browser.`);
                    startWaiting(`app:${a.slug}`, a.name, {
                      resumeTab: "connections",
                      steps: [
                        `${a.name} opened in your browser.`,
                        "Sign in and allow access.",
                        "Come back here; it shows up by itself.",
                      ],
                      again: () => void api.composioConnect(a.slug).catch(() => undefined),
                    });
                    api.composioConnect(a.slug).catch((e) => {
                      setConnecting(null);
                      onError(String(e));
                    });
                  }}
                >
                  {connecting === a.slug ? "Waiting..." : "Add"}
                </Button>
              </li>
            ))}
          </ul>
        </details>
      )}
      {note && <p className="text-[12px] text-(--muted)">{note}</p>}
      <Toggle
        label="Use these apps in Ask mode"
        hint="The local model reads them; changes go through your coding agent (Claude Code or Codex). Off for questions marked This PC only."
        checked={composio.enabled}
        onChange={(enabled) =>
          void updateSettings({ composio: { ...composio, enabled } }).catch((e) => onError(String(e)))
        }
      />
    </div>
  );
}

/** A consumer key, or another MCP link, instead of the browser sign-in. */
function OtherWays({ onError }: { onError: (e: string) => void }) {
  const composio = useSidekick((s) => s.settings.composio);
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const save = (patch: Partial<typeof composio>) =>
    updateSettings({ composio: { ...composio, ...patch } }).catch((e) => onError(String(e)));
  return (
    <details className="text-[12.5px]">
      <summary className="cursor-pointer text-(--muted)">Other ways to connect</summary>
      <div className="mt-2 flex flex-col gap-2.5">
        <p className="text-(--muted)">
          Paste a Composio consumer key (starts with ck_), from the Composio website under your account.
        </p>
        <div className="flex items-center gap-2">
          <input
            type="password"
            aria-label="Composio consumer key"
            value={key}
            placeholder="ck_..."
            onChange={(e) => setKey(e.target.value)}
            className="min-w-0 flex-1 rounded-lg border border-(--border) bg-transparent px-2.5 py-1.5 font-mono text-[12px] outline-none focus:border-[#0a84ff]"
          />
          <Button
            small
            primary
            disabled={busy || key.trim().length < 8}
            onClick={() => {
              setBusy(true);
              setNote(null);
              api
                .composioUseKey(key)
                .then((m) => {
                  setKey("");
                  setNote(m);
                })
                .catch((e) => setNote(String(e)))
                .finally(() => setBusy(false));
            }}
          >
            {busy ? "Checking..." : "Use key"}
          </Button>
        </div>
        <Field label="MCP link" hint="Leave empty for Composio Connect.">
          <TextField
            label="Composio MCP link"
            value={composio.url}
            placeholder="https://connect.composio.dev/mcp"
            className="w-56"
            mono
            onCommit={(url) => void save({ url, enabled: Boolean(url) || composio.enabled })}
          />
        </Field>
        <div className="flex gap-2">
          <Button
            small
            onClick={() =>
              void api
                .composioImport()
                .then(() => setNote("Copied from Claude Code."))
                .catch((e) => setNote(String(e)))
            }
          >
            Use Claude Code&apos;s
          </Button>
        </div>
        {note && <p className="text-(--muted)">{note}</p>}
      </div>
    </details>
  );
}

function CalendarCard({ onError }: { onError: (e: string) => void }) {
  const calendar = useSidekick((s) => s.settings.calendar);
  const { data: today, refresh } = useCached<CalendarToday>("calendar-today", api.calendarToday, 30_000);
  useEffect(() => {
    const off = listen(EVENTS.composioChanged, () => setTimeout(() => void refresh().catch(() => undefined), 3000));
    return () => {
      void off.then((f) => f());
    };
  }, [refresh]);
  const sources = today?.sources ?? [];
  return (
    <div className="flex flex-col gap-3 text-[13px]">
      {today && (
        <p className="text-(--muted)">
          {sources.length > 0
            ? `Reading ${sources.join(" and ")}.`
            : "No calendar connected. Connect Google Calendar or Outlook in Composio above."}
        </p>
      )}
      <Field label="Remind me before">
        <Select
          label="Remind me before"
          value={String(calendar.remindMinutes)}
          options={[1, 2, 3, 5, 10, 15].map((m) => [String(m), `${m} min`])}
          onChange={(v) =>
            void updateSettings({ calendar: { ...calendar, remindMinutes: Number(v) } }).catch((e) =>
              onError(String(e)),
            )
          }
        />
      </Field>
      {today?.error && <p className="text-[12px] text-red-400">{today.error}</p>}
      {sources.length > 0 &&
        (today && today.meetings.length > 0 ? (
          <ul className="flex flex-col gap-1.5">
            {today.meetings.map((m) => (
              <li key={`${m.start}${m.title}`} className="flex items-center justify-between gap-3">
                <span className="min-w-0 truncate">
                  <span className="text-(--muted) tabular-nums">
                    {m.start} to {m.end}
                  </span>{" "}
                  {m.title}
                </span>
                {m.joinUrl && (
                  <Button small onClick={() => void api.openReference("page", m.joinUrl ?? "")}>
                    Join
                  </Button>
                )}
              </li>
            ))}
          </ul>
        ) : (
          <p className="text-[12px] text-(--muted)">No more meetings today.</p>
        ))}
    </div>
  );
}

function BrowserCard() {
  const [guide, setGuide] = useState<{ id: string; guide: ExtensionGuide } | null>(null);
  const [error, setError] = useState<string | null>(null);
  // While a browser is being set up, watch for the extension to connect.
  const { data: browsers } = useCached<BrowserStatus[]>("browsers", api.browsersStatus, guide ? 3000 : undefined);
  useEffect(() => {
    if (guide && browsers?.some((b) => b.id === guide.id && b.connected)) setGuide(null);
  }, [browsers, guide]);

  if (!browsers) return <div className="h-12" aria-busy="true" />;
  return (
    <div className="flex flex-col gap-2.5 text-[13px]">
      {browsers.length === 0 && <p className="text-(--muted)">No supported browser found.</p>}
      {browsers.map((b) => (
        <div key={b.id} className="flex flex-col gap-2 rounded-xl border border-(--border) px-3 py-2.5">
          <div className="flex items-center gap-3">
            <span className="min-w-0 flex-1 font-medium">{b.name}</span>
            {b.connected ? (
              <span className="text-[12px] text-[#30d158]">Connected</span>
            ) : (
              <Button
                small
                primary={guide?.id !== b.id}
                onClick={() => {
                  setError(null);
                  installExtension(b.id, b.name)
                    .then((g) => setGuide({ id: b.id, guide: g }))
                    .catch((e) => setError(String(e)));
                }}
              >
                {guide?.id === b.id ? "Open again" : "Install"}
              </Button>
            )}
          </div>
          {guide?.id === b.id && (
            <div className="flex flex-col gap-1.5 rounded-lg bg-[#0a84ff]/10 p-2.5 text-[12.5px]">
              <p>{b.name} opened in your last-used profile. The steps also stay on the island:</p>
              <ol className="list-decimal space-y-0.5 pl-5">
                {guide.guide.steps.map((s) => (
                  <li key={s}>{s}</li>
                ))}
              </ol>
              <p className="text-(--muted)">This turns green by itself once it connects.</p>
            </div>
          )}
        </div>
      ))}
      {error && <p className="text-[12px] text-red-400">{error}</p>}
      <PairingCode />
    </div>
  );
}

function PairingCode() {
  const [info, setInfo] = useState<BrowserInfo | null>(null);
  useEffect(() => {
    void api.browserInfo().then(setInfo);
  }, []);
  return (
    <details className="text-[12.5px]">
      <summary className="cursor-pointer text-(--muted)">Pair with a code instead</summary>
      <div className="mt-2 flex items-center gap-2">
        <code className="min-w-0 flex-1 truncate rounded-lg bg-black/5 px-3 py-2 font-mono text-[12px] select-all dark:bg-white/5">
          {info?.token || "..."}
        </code>
        <CopyButton text={info?.token ?? ""} />
      </div>
      <p className="mt-1 text-(--muted)">Paste it in the extension&apos;s options under Use a pairing code instead.</p>
    </details>
  );
}

function ClaudeCard({ onError }: { onError: (e: string) => void }) {
  // Shares the setup checklist's cache, so both show the same state at once.
  const { data: status, refresh: reload } = useCached<SetupStatus>("setup-status", api.setupStatus);
  const items = status?.items ?? null;
  // Holds a token, so it is fetched each time and never cached on disk.
  const [mcp, setMcp] = useState<McpInfo | null>(null);
  useEffect(() => {
    void api.mcpInfo().then(setMcp);
  }, []);
  const [note, setNote] = useState<string | null>(null);
  const refresh = () => void reload().catch(() => undefined);
  const item = (id: string) => items?.find((i) => i.id === id);
  const installed = item("claude_code")?.done ?? false;
  const run = (what: "hooks" | "mcp") => {
    setNote(null);
    const p =
      what === "hooks"
        ? api
            .claudeAddHooks()
            .then((backup) => setNote(backup ? `Added. Your old settings are saved as ${backup}.` : "Added."))
        : api.claudeAddMcp().then(() => setNote("Added Sidekick's tools to Claude Code."));
    p.then(refresh).catch((e) => onError(String(e)));
  };
  const hooks = item("claude_hooks");
  const tools = item("claude_mcp");
  const command = mcp
    ? `claude mcp add --scope user --transport http sidekick ${mcp.url} --header "Authorization: Bearer ${mcp.token}"`
    : "";
  return (
    <div className="flex flex-col gap-2.5 text-[13px]">
      {!installed && items && (
        <p className="text-(--muted)">Claude Code is not installed yet. Install it from Home &gt; Setup.</p>
      )}
      <Row
        title="Hooks"
        status={hooks?.status ?? "..."}
        done={hooks?.done ?? false}
        action={
          <Button small primary onClick={() => run("hooks")}>
            Add for me
          </Button>
        }
      />
      <Row
        title="Sidekick's tools (MCP)"
        status={tools?.status ?? "..."}
        done={tools?.done ?? false}
        action={
          <Button small primary disabled={!installed} onClick={() => run("mcp")}>
            Add for me
          </Button>
        }
      />
      {note && <p className="text-[12px] text-(--muted) break-all">{note}</p>}
      <details className="text-[12.5px]">
        <summary className="cursor-pointer text-(--muted)">Do it by hand</summary>
        <div className="mt-2 flex flex-col gap-2">
          <p className="text-(--muted)">
            Merge this into <code className="font-mono">~/.claude/settings.json</code>:
          </p>
          <div className="flex items-start gap-2">
            <pre className="min-w-0 flex-1 overflow-x-auto rounded-lg bg-black/5 p-2 font-mono text-[11px] select-all dark:bg-white/5">
              {hooks?.command ?? ""}
            </pre>
            <CopyButton text={hooks?.command ?? ""} />
          </div>
          <p className="text-(--muted)">Then run once in a terminal:</p>
          <div className="flex items-start gap-2">
            <pre className="min-w-0 flex-1 overflow-x-auto rounded-lg bg-black/5 p-2 font-mono text-[11px] whitespace-pre-wrap select-all dark:bg-white/5">
              {command}
            </pre>
            <CopyButton text={command} />
          </div>
        </div>
      </details>
    </div>
  );
}

function CodexCard({ onError }: { onError: (e: string) => void }) {
  // Shares the setup checklist's cache, so both show the same state at once.
  const { data: status, refresh: reload } = useCached<SetupStatus>("setup-status", api.setupStatus);
  const items = status?.items ?? null;
  const [note, setNote] = useState<string | null>(null);
  const refresh = () => void reload().catch(() => undefined);
  const item = (id: string) => items?.find((i) => i.id === id);
  const installed = item("codex")?.done ?? false;
  const run = (what: "notify" | "mcp") => {
    setNote(null);
    const p = what === "notify" ? api.codexAddNotify() : api.codexAddMcp();
    p.then((backup) => setNote(backup ? `Added. Codex's old settings are saved as ${backup}.` : "Added."))
      .then(refresh)
      .catch((e) => onError(String(e)));
  };
  const notify = item("codex_notify");
  const tools = item("codex_mcp");
  return (
    <div className="flex flex-col gap-2.5 text-[13px]">
      {!installed && items && (
        <p className="text-(--muted)">
          Codex is not installed. Install it from Home &gt; Setup, then run codex once to sign in.
        </p>
      )}
      <Row
        title="Notifications"
        status={notify?.status ?? "..."}
        done={notify?.done ?? false}
        action={
          <Button small primary onClick={() => run("notify")}>
            Add for me
          </Button>
        }
      />
      <Row
        title="Sidekick's tools (MCP)"
        status={tools?.status ?? "..."}
        done={tools?.done ?? false}
        action={
          <Button small primary onClick={() => run("mcp")}>
            Add for me
          </Button>
        }
      />
      {note && <p className="text-[12px] text-(--muted) break-all">{note}</p>}
    </div>
  );
}

function Row({
  title,
  status,
  done,
  action,
}: {
  title: string;
  status: string;
  done: boolean;
  action: React.ReactNode;
}) {
  return (
    <div className="flex items-center gap-3 rounded-xl border border-(--border) px-3 py-2.5">
      <span className="min-w-0 flex-1">
        <span className="font-medium">{title}</span>
        <span className={`block text-[12px] ${done ? "text-[#30d158]" : "text-(--muted)"}`}>{status}</span>
      </span>
      {!done && action}
    </div>
  );
}
