"use client";

// Offline, opened from the pill: what keeps working, what waits for the
// connection, and a way to check again now.

import { useState } from "react";
import { api } from "@/lib/bridge";
import { Ic } from "./Icon";

const ROWS: [string, string, string][] = [
  ["Clipboard, files, voice notes", "working", "var(--ok)"],
  ["Connected apps and web search", "waiting", "var(--warn)"],
  ["Cloud agent runs", "paused", "var(--warn)"],
];

export function NetCard({ onClose }: { onClose: () => void }) {
  const [checking, setChecking] = useState(false);
  const check = () => {
    setChecking(true);
    void api
      .netCheck(true)
      .catch(() => false)
      .finally(() => setChecking(false));
  };
  return (
    <div className="rd-glance flex flex-col gap-2.5">
      <div className="rd-card-head">
        <p className="rd-eyebrow flex items-center gap-1.5">
          <span style={{ color: "var(--warn)", display: "inline-flex" }}>
            <Ic name="wifi" size={13} />
          </span>
          No connection
        </p>
        <p className="rd-title">You are offline</p>
        <p className="rd-detail">Local things keep working. The rest waits.</p>
      </div>
      <div className="flex flex-col gap-1" style={{ marginLeft: "calc(var(--orb-indent, 0px) * -1)" }}>
        {ROWS.map(([t, r, c]) => (
          <div key={t} className="rd-list-row mg-row">
            <span className="rd-dot" style={{ background: c }} />
            <span className="t" style={{ fontFamily: "inherit", fontSize: 13 }}>
              {t}
            </span>
            <span className="r">{r}</span>
          </div>
        ))}
        <div className="mt-1.5 flex items-center gap-1.5">
          <button type="button" className="rd-btn primary" disabled={checking} onClick={check}>
            {checking ? "Checking" : "Check now"}
          </button>
          <button type="button" className="rd-btn ghost ml-auto" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
