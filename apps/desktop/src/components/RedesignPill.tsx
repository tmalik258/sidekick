"use client";

// Compact island for the redesign: one line with a label, a dim detail and a
// trailing chip, as in the island design. It reports its own width so the
// shell fits it (the shell springs to whatever this measures).

import { motion } from "motion/react";
import { type ReactNode, useLayoutEffect, useRef } from "react";
import { api } from "@/lib/bridge";
import { useNow } from "@/lib/hooks";
import { Icon } from "./Icon";

export type PillKind =
  | "working"
  | "listening"
  | "done"
  | "error"
  | "focus"
  | "paused"
  | "offline"
  | "noticing"
  | "waiting"
  | "suggest";

export interface PillModel {
  kind: PillKind;
  label: string;
  /** Quieter text after the label. */
  dim?: string;
  /** Working: when it began, for the m:ss timer. */
  startedAt?: number;
  /** Working: 0..1 for the edge ring; undefined runs the ring as indeterminate. */
  progress?: number;
  /** Focus: time left, already formatted. */
  left?: string;
  /** Waiting: how many. */
  count?: number;
  onOpen?: () => void;
  /** Suggest: the likely option as a chip. */
  chip?: string;
  onChip?: () => void;
}

const clock = (ms: number) => {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
};

export function RedesignPill({ model, onWidth }: { model: PillModel; onWidth: (w: number) => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const now = useNow(1000);
  const { kind } = model;

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const report = () => onWidth(Math.ceil(el.offsetWidth));
    report();
    const ro = new ResizeObserver(report);
    ro.observe(el);
    return () => ro.disconnect();
  }, [onWidth]);

  let trail: ReactNode = null;
  if (kind === "working") trail = <span className="tnum">{clock(now - (model.startedAt ?? now))}</span>;
  else if (kind === "listening") trail = <Bars />;
  else if (kind === "done" && model.onOpen)
    trail = (
      <button
        type="button"
        className="chip-s ic-only"
        aria-label="Open"
        onClick={(e) => {
          e.stopPropagation();
          model.onOpen?.();
        }}
      >
        <Icon name="folder" size={12} />
      </button>
    );
  else if (kind === "focus")
    trail = (
      <span className="tnum" style={{ color: "var(--ink-2)" }}>
        {model.left}
      </span>
    );
  else if (kind === "paused")
    trail = <span className="dot" style={{ background: "var(--warn)", boxShadow: "0 0 8px var(--warn)" }} />;
  else if (kind === "offline")
    trail = (
      <span style={{ color: "var(--warn)", display: "inline-flex" }}>
        <Icon name="wifiOff" size={14} />
      </span>
    );
  else if (kind === "waiting") trail = <span className="badge tnum">{model.count ?? 1}</span>;
  else if (kind === "suggest" && model.chip)
    trail = (
      <button
        type="button"
        className="chip-s"
        onClick={(e) => {
          e.stopPropagation();
          model.onChip?.();
        }}
      >
        {model.chip}
      </button>
    );
  else if (kind === "error")
    trail = (
      <button
        type="button"
        className="chip-s"
        onClick={(e) => {
          e.stopPropagation();
          void api.askOpen();
        }}
      >
        Details
      </button>
    );

  return (
    <>
      {kind === "working" && <EdgeRing progress={model.progress} />}
      <motion.div
        ref={ref}
        className="cmp absolute top-0 left-0"
        initial={{ opacity: 0, filter: "blur(6px)" }}
        animate={{ opacity: 1, filter: "blur(0px)", transition: { duration: 0.28, delay: 0.07 } }}
        exit={{ opacity: 0, filter: "blur(4px)", transition: { duration: 0.11 } }}
        aria-live="polite"
      >
        {kind === "done" && (
          <span style={{ color: "var(--ok)", display: "inline-flex" }}>
            <Icon name="check" size={15} />
          </span>
        )}
        {kind === "suggest" && <span className="dot" style={{ background: "#95a89a" }} />}
        <span className={`cmp-label${kind === "suggest" ? " nt" : ""}${kind === "listening" ? " words" : ""}`}>
          {model.label}
          {model.dim && <span className="dim"> {model.dim}</span>}
        </span>
        {trail && <span className="cmp-trail">{trail}</span>}
      </motion.div>
    </>
  );
}

function Bars() {
  return (
    <span className="bars" aria-hidden>
      <i />
      <i />
      <i />
      <i />
      <i />
    </span>
  );
}

/** Progress traced along the capsule's own edge. */
function EdgeRing({ progress }: { progress?: number }) {
  const ind = progress === undefined;
  return (
    <svg className={`edge${ind ? " ind" : ""}`} aria-hidden="true">
      <rect className="trk" pathLength={100} />
      <rect
        className="val"
        pathLength={100}
        strokeDasharray={ind ? undefined : 100}
        strokeDashoffset={ind ? undefined : 100 * (1 - (progress ?? 0))}
      />
    </svg>
  );
}
