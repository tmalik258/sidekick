"use client";

// Apps: Composio (one sign-in brings every app in the account, and the
// meetings from its calendar), and the browser extension per browser.

import { useCallback, useEffect, useState } from "react";
import { api, EVENTS, listen } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import { installExtension, startWaiting, updateSettings, useSidekick } from "@/lib/store";
import type { BrowserInfo, BrowserStatus, CalendarToday, ComposioStatus, ExtensionGuide } from "@/lib/types";
import { ComposioApps } from "./ComposioApps";
import { Button, CopyButton, Field, Section, Select, TextField, Toggle } from "./ui";

export function ConnectionsTab({ onError }: { onError: (e: string) => void }) {
  return (
    <>
      <Section
        title="Composio"
        keywords="apps accounts calendar meetings reminders join google outlook jira slack gmail notion trello github linear fathom login sign in"
      >
        <ComposioCard onError={onError} />
      </Section>
      <Section
        title="Browser"
        hint="Talks only to Sidekick on this PC."
        keywords="extension chrome edge firefox zen brave pairing code tabs"
      >
        <BrowserCard />
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
          steps: ["Composio opened in your browser.", "Sign in and press Allow."],
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
            <p className="text-(--muted)">Sign in to Composio and press Allow.</p>
            <div>
              <Button small onClick={signIn}>
                Open it again
              </Button>
            </div>
          </div>
        ) : (
          <div className="flex items-center gap-3">
            <p className="min-w-0 flex-1 text-(--muted)">Calendar, mail, Slack and more, in one sign-in.</p>
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
      <ComposioApps status={status} onError={onError} resumeTab="connections" />
      {note && <p className="text-[12px] text-(--muted)">{note}</p>}
      <Meetings onError={onError} />
      <Toggle
        label="Use these apps in Ask"
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

/** Today's meetings and the reminder time, once a calendar is connected. */
function Meetings({ onError }: { onError: (e: string) => void }) {
  const calendar = useSidekick((s) => s.settings.calendar);
  const { data: today, refresh } = useCached<CalendarToday>("calendar-today", api.calendarToday, 30_000);
  useEffect(() => {
    const off = listen(EVENTS.composioChanged, () => setTimeout(() => void refresh().catch(() => undefined), 3000));
    return () => {
      void off.then((f) => f());
    };
  }, [refresh]);
  const sources = today?.sources ?? [];
  if (sources.length === 0 && !today?.error) return null;
  return (
    <div className="flex flex-col gap-2.5 border-t border-(--border) pt-3 text-[13px]">
      <Field label="Meeting reminder" hint={sources.join(" and ")}>
        <Select
          label="Meeting reminder"
          value={String(calendar.remindMinutes)}
          options={[1, 2, 3, 5, 10, 15].map((m) => [String(m), `${m} min before`])}
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
  const { data: browsers, refresh } = useCached<BrowserStatus[]>("browsers", api.browsersStatus);
  useEffect(() => {
    const off = listen(EVENTS.browsersChanged, () => void refresh().catch(() => undefined));
    return () => {
      void off.then((f) => f());
    };
  }, [refresh]);
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
              <p>{b.name} opened. Then:</p>
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
      <p className="mt-1 text-(--muted)">Paste it in the extension&apos;s options.</p>
    </details>
  );
}
