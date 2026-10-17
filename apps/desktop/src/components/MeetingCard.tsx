"use client";

// A call starting soon, opened from its pill: when, which call, and the
// ways to join it or put it off.

import { useState } from "react";
import { api } from "@/lib/bridge";
import { Ic } from "./Icon";

export interface SoonMeeting {
  title: string;
  mins: number;
  join: string | null | undefined;
  notes?: { title: string; source: string; reference: string } | null;
}

export function MeetingCard({ meeting }: { meeting: SoonMeeting }) {
  const [hidden, setHidden] = useState(false);
  if (hidden) return null;
  return (
    <div className="rd-glance flex flex-col gap-2.5">
      <div className="rd-card-head">
        <p className="rd-eyebrow flex items-center gap-1.5">
          <Ic name="clock" size={12} />
          Calendar{meeting.join ? ", or join the call" : ""}
        </p>
        <p className="rd-title">{meeting.mins < 1 ? "Starting now" : `Starts in ${meeting.mins} minutes`}</p>
        <p className="rd-detail">From {meeting.title}</p>
      </div>
      <div className="flex flex-col gap-1.5" style={{ marginLeft: "calc(var(--orb-indent, 0px) * -1)" }}>
        <div className="flex items-center gap-1.5">
          {meeting.join && (
            <button
              type="button"
              className="rd-btn primary"
              onClick={() => void api.openReference("page", meeting.join ?? "")}
            >
              Join the call
            </button>
          )}
          {meeting.notes && (
            <button
              type="button"
              className="rd-btn"
              title={meeting.notes.title}
              onClick={() => {
                const n = meeting.notes;
                if (n) void api.openReference(n.source, n.reference);
              }}
            >
              Open notes
            </button>
          )}
        </div>
        <div className="flex items-center gap-1.5">
          <button type="button" className="rd-btn ghost" onClick={() => setHidden(true)}>
            Later
          </button>
          <button type="button" className="rd-btn ghost ml-auto" onClick={() => setHidden(true)}>
            Dismiss
          </button>
        </div>
      </div>
    </div>
  );
}
