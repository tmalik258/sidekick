"use client";

// First run in the design layout: your name, what you do, then a short
// checklist of what was found. Both questions can be skipped; the full
// setup list stays in Settings > Setup.

import { useEffect, useState } from "react";
import { api } from "@/lib/bridge";
import { showDone, updateSettings, useDuskie } from "@/lib/store";
import type { SetupItem } from "@/lib/types";
import { Ic } from "../Icon";

const ROLES = ["Coder", "Writer", "Manager", "Student", "Something else"] as const;

export function WelcomeCard() {
  const savedName = useDuskie((s) => s.settings.userName);
  const [step, setStep] = useState(0);
  const [name, setName] = useState(savedName);
  const [items, setItems] = useState<SetupItem[] | null>(null);

  useEffect(() => {
    if (step === 2 && items === null)
      void api
        .setupStatus()
        .then((s) => setItems(s.items))
        .catch(() => setItems([]));
  }, [step, items]);

  const next = () => {
    if (step === 0 && name.trim()) void updateSettings({ userName: name.trim() });
    if (step < 2) {
      setStep(step + 1);
      return;
    }
    void updateSettings({ onboarded: true }).then(() => {
      void api.askClose();
      showDone("You're set");
    });
  };
  const role = (r: (typeof ROLES)[number]) => {
    void updateSettings({ codes: r === "Coder" });
    setStep(2);
  };

  const found = (items ?? []).filter((i) => i.done).slice(0, 2);
  const later = (items ?? []).find((i) => !i.done);
  const title = step === 0 ? "What should I call you?" : step === 1 ? "What do you do?" : "You are set";
  const detail =
    step === 2 ? "Hover me for your day, click to ask. Alt Space talks." : "Two quick questions, both optional.";

  return (
    <div className="rd-glance flex flex-col gap-2.5">
      <div className="rd-card-head">
        <p className="rd-eyebrow">Welcome to Duskie</p>
        <p className="rd-title">{title}</p>
        <p className="rd-detail">{detail}</p>
      </div>
      <div className="flex flex-col gap-1.5" style={{ marginLeft: "calc(var(--orb-indent, 55px) * -1)" }}>
        {step === 0 && (
          <input
            className="rd-field"
            placeholder="Your name"
            autoComplete="off"
            // biome-ignore lint/a11y/noAutofocus: the only field on a first-run card
            autoFocus
            value={name}
            maxLength={40}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && next()}
          />
        )}
        {step === 1 && (
          <div className="flex flex-wrap gap-1.5">
            {ROLES.map((r) => (
              <button key={r} type="button" className="chip-s" onClick={() => role(r)}>
                {r}
              </button>
            ))}
          </div>
        )}
        {step === 2 && (
          <>
            {found.map((i) => (
              <div key={i.id} className="rd-list-row mg-row">
                <span style={{ color: "var(--ok)", display: "inline-flex" }}>
                  <Ic name="check" size={13} />
                </span>
                <span className="t" style={{ fontFamily: "inherit", fontSize: 13 }}>
                  {i.title}
                </span>
              </div>
            ))}
            {later && (
              <div className="rd-list-row mg-row">
                <span style={{ color: "var(--warn)", display: "inline-flex" }}>
                  <Ic name="plus" size={13} />
                </span>
                <span className="t" style={{ fontFamily: "inherit", fontSize: 13 }}>
                  {later.title} <span className="text-[var(--ink-3)]">later in Settings</span>
                </span>
              </div>
            )}
          </>
        )}
        <div className="mt-1.5 flex items-center gap-1.5">
          <button type="button" className="rd-btn primary" onClick={next}>
            {step === 2 ? "Start" : "Continue"}
          </button>
          {step < 2 && (
            <button type="button" className="rd-btn ghost ml-auto" onClick={() => setStep(step + 1)}>
              Skip
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
