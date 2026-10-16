"use client";

// Direct MCP: connect an app as a local program over stdio. Preferred over
// Composio when both exist. Writes stay blocked until allowed.

import { useState } from "react";
import { api } from "@/lib/bridge";
import { updateSettings, useDuskie } from "@/lib/store";
import type { DirectMcpServer } from "@/lib/types";
import { Button, Field, Section } from "./ui";

export function DirectMcpCard({ onError }: { onError: (e: string) => void }) {
  const servers = useDuskie((s) => s.settings.directMcp?.servers ?? []);
  const [name, setName] = useState("");
  const [command, setCommand] = useState("");
  const [args, setArgs] = useState("");
  const [busy, setBusy] = useState(false);

  const save = (next: DirectMcpServer[]) =>
    void updateSettings({ directMcp: { servers: next } }).catch((e) => onError(String(e)));

  const add = async () => {
    setBusy(true);
    try {
      const argv = args
        .split(/\s+/)
        .map((a) => a.trim())
        .filter(Boolean);
      await api.mcpValidateStdio({ command: command.trim(), args: argv });
      const row: DirectMcpServer = {
        id: crypto.randomUUID(),
        name: name.trim() || command.trim(),
        command: command.trim(),
        args: argv,
      };
      await save([...servers, row]);
      setName("");
      setCommand("");
      setArgs("");
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Section
      title="Direct (MCP)"
      hint="A program on this PC that speaks MCP over stdio. Used before Composio for the same app."
      keywords="mcp direct stdio server connect tools apps"
    >
      {servers.length === 0 ? (
        <p className="text-[13px] text-(--muted)">No direct servers yet.</p>
      ) : (
        <ul className="flex flex-col gap-2">
          {servers.map((s) => (
            <li
              key={s.id}
              className="flex items-center justify-between gap-2 rounded-[var(--r-md)] bg-(--surface) px-3 py-2 text-[13px]"
            >
              <span className="min-w-0 truncate">
                <b className="font-medium">{s.name}</b>
                <span className="ml-2 text-(--muted)">
                  {s.command} {s.args.join(" ")}
                </span>
              </span>
              <Button small onClick={() => save(servers.filter((x) => x.id !== s.id))}>
                Remove
              </Button>
            </li>
          ))}
        </ul>
      )}
      <Field label="Name">
        <input
          aria-label="Name"
          value={name}
          placeholder="GitHub"
          className="w-full rounded-md border border-(--border) bg-transparent px-2 py-1 text-[13px] outline-none focus:border-(--accent)"
          onChange={(e) => setName(e.target.value)}
        />
      </Field>
      <Field label="Program" hint="The program only — put each argument in the next box.">
        <input
          aria-label="Program"
          value={command}
          placeholder="npx"
          className="w-full rounded-md border border-(--border) bg-transparent px-2 py-1 text-[13px] outline-none focus:border-(--accent)"
          onChange={(e) => setCommand(e.target.value)}
        />
      </Field>
      <Field label="Arguments">
        <input
          aria-label="Arguments"
          value={args}
          placeholder="-y @modelcontextprotocol/server-github"
          className="w-full rounded-md border border-(--border) bg-transparent px-2 py-1 font-mono text-[12px] outline-none focus:border-(--accent)"
          onChange={(e) => setArgs(e.target.value)}
        />
      </Field>
      <Button disabled={busy || !command.trim()} onClick={() => void add()}>
        {busy ? "Checking…" : "Add server"}
      </Button>
    </Section>
  );
}
