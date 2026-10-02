"use client";

// Skills: what runs on its own (Automations), what went quiet after Not
// now, and the full list.

import { useCallback, useEffect, useState } from "react";
import { api } from "@/lib/bridge";
import type { SkillInfo } from "@/lib/types";
import { Button, Section, Switch } from "./ui";

export function SkillsTab({ onError }: { onError: (e: string) => void }) {
  const [skills, setSkills] = useState<SkillInfo[]>([]);
  const refresh = useCallback(
    () =>
      void api
        .skillsList()
        .then(setSkills)
        .catch((e) => onError(String(e))),
    [onError],
  );
  useEffect(refresh, [refresh]);
  const change = (s: SkillInfo, enabled: boolean, auto: boolean) => {
    setSkills((list) => list.map((x) => (x.id === s.id ? { ...x, enabled, auto } : x)));
    api.skillSet(s.id, enabled, auto).catch((e) => {
      onError(String(e));
      refresh();
    });
  };
  const automations = skills.filter((s) => s.enabled && s.auto);
  const quiet = skills.filter((s) => s.mutedUntil);

  return (
    <>
      <Section
        title="Automations"
        hint="These run their first safe option without asking. Turn on more with Always do this on a suggestion, or the Auto box below."
        keywords="auto always automatic rules"
      >
        {automations.length === 0 ? (
          <p className="text-[13px] text-(--muted)">Nothing runs on its own yet.</p>
        ) : (
          <ul className="flex flex-col divide-y divide-(--border)">
            {automations.map((s) => (
              <li key={s.id} className="flex items-center justify-between gap-4 py-2 first:pt-0 last:pb-0">
                <span className="min-w-0 text-[13px]">
                  <span className="font-medium">{s.name}</span>
                  <span className="block truncate text-[12px] text-(--muted)">{s.description}</span>
                </span>
                <Button small onClick={() => change(s, true, false)}>
                  Ask me instead
                </Button>
              </li>
            ))}
          </ul>
        )}
      </Section>
      {quiet.length > 0 && (
        <Section
          title="Gone quiet"
          hint="You said Not now to these three times in a row, so they rest for a while."
          keywords="muted snoozed not now"
        >
          <ul className="flex flex-col divide-y divide-(--border)">
            {quiet.map((s) => (
              <li key={s.id} className="flex items-center justify-between gap-4 py-2 first:pt-0 last:pb-0">
                <span className="min-w-0 text-[13px]">
                  <span className="font-medium">{s.name}</span>
                  <span className="block text-[12px] text-(--muted)">
                    Back {new Date(s.mutedUntil ?? "").toLocaleDateString(undefined, { weekday: "long" })}
                  </span>
                </span>
                <Button small onClick={() => void api.skillUnmute(s.id).then(refresh)}>
                  Bring back
                </Button>
              </li>
            ))}
          </ul>
        </Section>
      )}
      <Section
        title="All skills"
        hint="Deleting files, running installers or stopping processes always ask first, even on Auto."
        keywords="skills enable disable"
      >
        <ul className="flex flex-col divide-y divide-(--border)">
          {skills.map((s) => (
            <li key={s.id} className="flex items-start justify-between gap-4 py-2.5 first:pt-0 last:pb-0">
              <div className="min-w-0">
                <p className="text-[14px] font-medium">{s.name}</p>
                <p className="text-[12px] text-(--muted)">{s.description}</p>
              </div>
              <div className="flex shrink-0 items-center gap-3 pt-0.5">
                <label className="flex items-center gap-1.5 text-[12px] text-(--muted)">
                  Auto
                  <input
                    type="checkbox"
                    checked={s.auto}
                    disabled={!s.enabled}
                    onChange={(e) => change(s, s.enabled, e.target.checked)}
                    className="size-3.5 accent-[#0a84ff]"
                  />
                </label>
                <Switch checked={s.enabled} onChange={(on) => change(s, on, s.auto)} label={`Enable ${s.name}`} />
              </div>
            </li>
          ))}
        </ul>
      </Section>
    </>
  );
}
