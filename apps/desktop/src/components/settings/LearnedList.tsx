"use client";

// Settings > Notifications: the profile, and what Duskie has learned about
// who and what to show more or less of. Every row can be reset, so nothing
// learned is out of reach.

import { useCallback, useEffect, useState } from "react";
import { api } from "@/lib/bridge";
import { filterRows, groupsOf, type LearnedSort, lean, reasonFor, sortRows, splitKey } from "@/lib/learned";
import { useDuskie } from "@/lib/store";
import type { LessonRow } from "@/lib/types";
import { Button, Field, Segmented } from "./ui";

export function NotificationProfile({ onError }: { onError: (e: string) => void }) {
  const profile = useDuskie((s) => s.settings.notifications.profile);
  return (
    <Field
      label="Profile"
      hint={
        profile === "calm"
          ? "A small pill for most things. Sound only for urgent ones."
          : "Quieter things come as a small pill with a soft sound."
      }
    >
      <Segmented
        label="Notification profile"
        value={profile}
        options={[
          ["normal", "Normal"],
          ["calm", "Calm"],
        ]}
        onChange={(p) => void api.attentionSetProfile(p).catch((e) => onError(String(e)))}
      />
    </Field>
  );
}

export function LearnedList({ onError }: { onError: (e: string) => void }) {
  const entries = useDuskie((s) => s.settings.notifications.lessons.entries);
  const [rows, setRows] = useState<LessonRow[]>([]);
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<LearnedSort>("recent");

  const refresh = useCallback(() => void api.attentionLessons().then(setRows), []);
  // The settings change on every lesson, so the list follows them.
  // biome-ignore lint/correctness/useExhaustiveDependencies: refresh when lessons change
  useEffect(refresh, [refresh, entries]);

  const reset = (scope: string) => void api.attentionReset(scope).catch((e) => onError(String(e)));
  const shown = sortRows(filterRows(rows, query), sort);

  return (
    <div className="flex flex-col gap-2.5 text-[13px]">
      <div className="flex items-center justify-between gap-3">
        <span className="font-medium">What Duskie has learned</span>
        <Button small disabled={rows.length === 0} onClick={() => reset("all")}>
          Reset all
        </Button>
      </div>
      {rows.length === 0 ? (
        <p className="text-[12px] text-(--muted)">
          Nothing yet. Acting on a notification, or choosing Show less, teaches Duskie. Dismissing does not.
        </p>
      ) : (
        <>
          <div className="flex items-center gap-2">
            <input
              type="search"
              aria-label="Search what Duskie learned"
              placeholder="Search"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              className="min-w-0 flex-1 rounded-[10px] bg-(--surface) px-3 py-1.5 text-[12.5px] outline-none focus-visible:outline-2 focus-visible:outline-(--accent)"
            />
            <Segmented
              label="Sort"
              value={sort}
              options={[
                ["recent", "Recent"],
                ["name", "Name"],
                ["level", "Level"],
              ]}
              onChange={setSort}
            />
          </div>
          <div className="flex flex-wrap gap-1.5">
            {groupsOf(rows).map((g) => (
              <Button key={g.prefix} small onClick={() => reset(`group:${g.prefix}`)}>
                Reset {g.group} ({g.count})
              </Button>
            ))}
          </div>
          <ul className="flex flex-col gap-1.5">
            {shown.map((r) => {
              const { group, name } = splitKey(r.key);
              return (
                <li key={r.key} className="flex items-center gap-2">
                  <span className="min-w-0 flex-1 truncate">
                    <span className="font-medium">{name}</span>{" "}
                    <span className="text-(--muted)">
                      {group}, {lean(r.shift) === "more" ? "more" : lean(r.shift) === "less" ? "less" : "usual"}.{" "}
                      {reasonFor(r.shift)}
                    </span>
                  </span>
                  <Button small aria-label={`Reset ${name}`} onClick={() => reset(r.key)}>
                    Reset
                  </Button>
                </li>
              );
            })}
          </ul>
        </>
      )}
    </div>
  );
}
