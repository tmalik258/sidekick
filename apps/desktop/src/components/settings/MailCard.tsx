"use client";

// Settings > Notifications: connect Gmail so new mail arrives straight from
// the account. Duskie reads who a message is from and a few yes or no
// answers about it. It never keeps or logs the text.

import { useEffect, useState } from "react";
import { api } from "@/lib/bridge";
import { useDuskie } from "@/lib/store";
import type { MailStatus } from "@/lib/types";
import { Button, Field, TextField } from "./ui";

const LABEL: Record<MailStatus["state"], string> = {
  disconnected: "Not connected",
  connected: "Connected",
  offline: "Offline, will retry",
  auth_expired: "Sign in again",
};

export function MailCard({ onError }: { onError: (e: string) => void }) {
  const [status, setStatus] = useState<MailStatus | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    void api.mailStatus().then(setStatus);
    // Keep the label honest while Settings is open.
    const timer = setInterval(() => void api.mailStatus().then(setStatus), 15000);
    return () => clearInterval(timer);
  }, []);

  const run = (call: () => Promise<MailStatus>) => {
    setBusy(true);
    call()
      .then(setStatus)
      .catch((e) => onError(String(e)))
      .finally(() => setBusy(false));
  };

  if (!status) return null;
  const connected = status.state !== "disconnected";
  return (
    <>
      <GmailRow status={status} busy={busy} run={run} connected={connected} />
      {status.provider !== "gmail" && <ImapForm status={status} busy={busy} run={run} />}
    </>
  );
}

type Run = (call: () => Promise<MailStatus>) => void;

function GmailRow({
  status,
  busy,
  run,
  connected,
}: {
  status: MailStatus;
  busy: boolean;
  run: Run;
  connected: boolean;
}) {
  return (
    <Field
      label="Gmail"
      hint={
        status.configured
          ? "Reads who an email is from, never its text. Skips mail you already read elsewhere."
          : "Google sign-in is not set up in this build yet."
      }
    >
      <span className="text-[12px] text-(--muted)" role="status">
        {LABEL[status.state]}
      </span>
      {connected && status.state !== "auth_expired" ? (
        <Button small disabled={busy} onClick={() => run(api.mailDisconnect)}>
          Disconnect
        </Button>
      ) : (
        <Button small disabled={busy || !status.configured} onClick={() => run(api.mailConnect)}>
          {busy ? "Waiting for Google" : status.state === "auth_expired" ? "Sign in again" : "Connect Gmail"}
        </Button>
      )}
    </Field>
  );
}

/** Advanced: any mail server over IMAP, with an app password. */
function ImapForm({ status, busy, run }: { status: MailStatus; busy: boolean; run: Run }) {
  const saved = useDuskie((s) => s.settings.notifications.imap);
  const [open, setOpen] = useState(false);
  const [host, setHost] = useState(saved.host);
  const [port, setPort] = useState(String(saved.port));
  const [user, setUser] = useState(saved.user);
  const [password, setPassword] = useState("");
  const on = status.provider === "imap";
  if (!open && !on) {
    return (
      <Field label="Other mail" hint="Advanced: any provider that supports IMAP, using an app password.">
        <Button small onClick={() => setOpen(true)}>
          Set up
        </Button>
      </Field>
    );
  }
  if (on) {
    return (
      <Field label={`Mail (${saved.user})`} hint="Connected over IMAP. The password is in Windows Credential Manager.">
        <span className="text-[12px] text-(--muted)" role="status">
          {LABEL[status.state]}
        </span>
        <Button small disabled={busy} onClick={() => run(api.mailDisconnect)}>
          Disconnect
        </Button>
      </Field>
    );
  }
  return (
    <div className="flex flex-col gap-2 text-[13px]">
      <p className="text-[12px] text-(--muted)">
        Use the server name from your provider, such as imap.example.com. Most providers need an app password made in
        your account security page, not your normal password.
      </p>
      <div className="flex flex-wrap items-center gap-2">
        <TextField label="IMAP server" value={host} onCommit={setHost} placeholder="imap.example.com" />
        <TextField label="Port" value={port} onCommit={setPort} placeholder="993" className="w-16" />
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <TextField label="User" value={user} onCommit={setUser} placeholder="you@example.com" />
        <input
          type="password"
          aria-label="App password"
          placeholder="App password"
          autoComplete="off"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          className="rounded-md border border-(--border) bg-transparent px-2 py-1 text-[13px] outline-none focus:border-(--accent)"
        />
        <Button
          small
          disabled={busy || !host || !user || !password}
          onClick={() => {
            run(() => api.mailConnectImap(host, Number(port) || 993, user, password));
            setPassword("");
          }}
        >
          {busy ? "Checking" : "Connect"}
        </Button>
      </div>
      {status.error && <p className="text-[12px] text-(--err)">{status.error}</p>}
    </div>
  );
}
