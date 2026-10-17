"use client";

// Apps connected directly over MCP: a program on this PC, or an https
// address. Used before Composio for the same app. A program is arbitrary
// code, so it runs only after the exact command was shown and confirmed;
// variable values go to the keychain; writes stay off tool by tool.

import { useState } from "react";
import { api } from "@/lib/bridge";
import { useDuskie } from "@/lib/store";
import type { DirectServer, ServerTool } from "@/lib/types";
import { Button, Field, Section } from "./ui";

const INPUT =
  "w-full rounded-md border border-(--border) bg-transparent px-2 py-1 text-[13px] outline-none focus:border-(--accent)";

function parseEnv(raw: string): Record<string, string> {
  const env: Record<string, string> = {};
  for (const line of raw.split("\n")) {
    const i = line.indexOf("=");
    if (i > 0) env[line.slice(0, i).trim()] = line.slice(i + 1).trim();
  }
  return env;
}

function Row({ s, onError }: { s: DirectServer; onError: (e: string) => void }) {
  const [shown, setShown] = useState<string | null>(null);
  const [tools, setTools] = useState<ServerTool[] | null>(null);
  const [busy, setBusy] = useState(false);
  const run = async (f: () => Promise<void>) => {
    setBusy(true);
    try {
      await f();
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <li className="flex flex-col gap-2 rounded-[var(--r-md)] bg-(--surface) px-3 py-2 text-[13px]">
      <div className="flex items-center gap-2">
        <span className="min-w-0 flex-1 truncate">
          <b className="font-medium">{s.app}</b>
          <span className="ml-2 text-(--muted)">{s.kind === "stdio" ? `${s.command} ${s.args.join(" ")}` : s.url}</span>
        </span>
        {s.kind === "url" && (
          <Button small disabled={busy} onClick={() => void run(() => api.serversSignIn(s.id))}>
            {s.envNames.includes("Authorization") ? "Sign in again" : "Sign in"}
          </Button>
        )}
        {s.confirmed ? (
          <Button small disabled={busy} onClick={() => void run(async () => setTools(await api.serversTest(s.id)))}>
            {busy ? "Connecting..." : "Test"}
          </Button>
        ) : (
          <Button small disabled={busy} onClick={() => void run(async () => setShown(await api.serversCommand(s.id)))}>
            Review
          </Button>
        )}
        <Button small onClick={() => void run(() => api.serversRemove(s.id))}>
          Remove
        </Button>
      </div>
      {shown !== null && !s.confirmed && (
        <div className="flex flex-col gap-1.5">
          <p className="text-(--muted)">Duskie will run exactly this. Only confirm if you trust it.</p>
          <code className="mono rounded-md bg-black/30 px-2 py-1 text-[12px] break-all">{shown}</code>
          <div className="flex gap-1.5">
            <Button small onClick={() => void run(() => api.serversConfirm(s.id, shown))}>
              Confirm and allow
            </Button>
            <Button small onClick={() => setShown(null)}>
              Not now
            </Button>
          </div>
        </div>
      )}
      {tools && (
        <ul className="flex flex-col gap-1">
          {tools.length === 0 && <li className="text-(--muted)">It offers no tools.</li>}
          {tools.map((t) => (
            <li key={t.name} className="flex items-center gap-2">
              <span className="mono min-w-0 flex-1 truncate" title={t.description}>
                {t.name}
              </span>
              {t.writes ? (
                <label className="flex items-center gap-1.5 text-(--muted)">
                  <input
                    type="checkbox"
                    checked={t.allowed}
                    onChange={(e) => {
                      const allow = e.target.checked;
                      void run(async () => {
                        await api.serversAllowWrite(s.id, t.name, allow);
                        setTools((all) => all?.map((x) => (x.name === t.name ? { ...x, allowed: allow } : x)) ?? null);
                      });
                    }}
                  />
                  Can change things
                </label>
              ) : (
                <span className="text-(--muted)">Reads only</span>
              )}
            </li>
          ))}
        </ul>
      )}
    </li>
  );
}

export function DirectMcpCard({ onError }: { onError: (e: string) => void }) {
  const servers = useDuskie((s) => s.settings.directServers ?? []);
  const [app, setApp] = useState("");
  const [kind, setKind] = useState<"stdio" | "url">("stdio");
  const [command, setCommand] = useState("");
  const [args, setArgs] = useState("");
  const [url, setUrl] = useState("");
  const [env, setEnv] = useState("");
  const [busy, setBusy] = useState(false);

  const add = async () => {
    setBusy(true);
    try {
      await api.serversSave(
        {
          id: "",
          app: app.trim(),
          kind,
          command: command.trim(),
          args: args.split(/\s+/).filter(Boolean),
          envNames: [],
          url: url.trim(),
          confirmed: false,
          writesAllowed: [],
        },
        parseEnv(env),
      );
      setApp("");
      setCommand("");
      setArgs("");
      setUrl("");
      setEnv("");
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Section
      title="Direct (MCP)"
      hint="A program on this PC or an https address that speaks MCP. Used before Composio for the same app."
      keywords="mcp direct stdio server connect tools apps url"
    >
      {servers.length === 0 ? (
        <p className="text-[13px] text-(--muted)">No direct servers yet.</p>
      ) : (
        <ul className="flex flex-col gap-2">
          {servers.map((s) => (
            <Row key={s.id} s={s} onError={onError} />
          ))}
        </ul>
      )}
      <Field label="App">
        <input
          aria-label="App"
          value={app}
          placeholder="github"
          className={INPUT}
          onChange={(e) => setApp(e.target.value)}
        />
      </Field>
      <Field label="Runs as">
        <select
          aria-label="Runs as"
          value={kind}
          className={INPUT}
          onChange={(e) => setKind(e.target.value as "stdio" | "url")}
        >
          <option value="stdio">A program on this PC</option>
          <option value="url">An https address</option>
        </select>
      </Field>
      {kind === "stdio" ? (
        <>
          <Field label="Program" hint="The program only. Put each argument in the next box.">
            <input
              aria-label="Program"
              value={command}
              placeholder="npx"
              className={INPUT}
              onChange={(e) => setCommand(e.target.value)}
            />
          </Field>
          <Field label="Arguments">
            <input
              aria-label="Arguments"
              value={args}
              placeholder="-y @modelcontextprotocol/server-github"
              className={`${INPUT} font-mono text-[12px]`}
              onChange={(e) => setArgs(e.target.value)}
            />
          </Field>
        </>
      ) : (
        <Field label="Address">
          <input
            aria-label="Address"
            value={url}
            placeholder="https://"
            className={INPUT}
            onChange={(e) => setUrl(e.target.value)}
          />
        </Field>
      )}
      <Field
        label={kind === "stdio" ? "Variables" : "Headers"}
        hint="One NAME=value per line. Values are kept in your system keychain."
      >
        <textarea
          aria-label={kind === "stdio" ? "Variables" : "Headers"}
          value={env}
          rows={2}
          placeholder="GITHUB_TOKEN=..."
          className={`${INPUT} font-mono text-[12px]`}
          onChange={(e) => setEnv(e.target.value)}
        />
      </Field>
      <Button
        disabled={busy || !app.trim() || (kind === "stdio" ? !command.trim() : !url.trim())}
        onClick={() => void add()}
      >
        {busy ? "Checking..." : "Add server"}
      </Button>
    </Section>
  );
}
