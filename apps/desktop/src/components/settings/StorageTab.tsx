"use client";

// Storage: what fills the disk, by group. Groups that are safe to clear
// list their parts with a box each; nothing moves until you review and
// confirm, and everything goes to the Recycle Bin.

import { useState } from "react";
import { api, type DiskGroup } from "@/lib/bridge";
import { Button, Section } from "./ui";

export function StorageTab({ onError }: { onError: (e: string) => void }) {
  const [groups, setGroups] = useState<DiskGroup[] | null>(null);
  const [scanning, setScanning] = useState(false);
  const [open, setOpen] = useState<string | null>(null);
  const [done, setDone] = useState<string | null>(null);

  const scan = () => {
    setScanning(true);
    setDone(null);
    api
      .diskGroups()
      .then(setGroups, (e: unknown) => onError(String(e)))
      .finally(() => setScanning(false));
  };
  const largest = Math.max(1, ...(groups ?? []).map((g) => g.bytes));

  return (
    <Section title="What fills the disk" keywords="disk storage space size clean node_modules cache games wsl docker">
      <div className="flex items-center justify-between gap-3 text-[13px]">
        <p className="text-(--muted)">
          {groups === null
            ? "Sizes every group. Reads only; takes up to 15 seconds."
            : groups.length === 0
              ? "Nothing big found."
              : "Open a group to see what is in it."}
        </p>
        <Button small onClick={scan} disabled={scanning}>
          {scanning ? "Scanning..." : groups ? "Scan again" : "Scan"}
        </Button>
      </div>
      {done && <p className="text-[13px] text-[#30d158]">{done}</p>}
      <ul className="flex flex-col gap-1.5">
        {groups?.map((g) => (
          <li key={g.id} className="rounded-xl border border-(--border)">
            <button
              type="button"
              aria-expanded={open === g.id}
              onClick={() => setOpen(open === g.id ? null : g.id)}
              className="flex w-full flex-col gap-1.5 px-3 py-2.5 text-left"
            >
              <span className="flex items-baseline justify-between gap-3 text-[13.5px]">
                <span className="font-medium">{g.label}</span>
                <span className="font-mono text-[12px] tabular-nums">
                  {g.partial ? "at least " : ""}
                  {human(g.bytes)}
                </span>
              </span>
              <span className="h-1.5 overflow-hidden rounded-full bg-(--border)">
                <span
                  className="block h-full rounded-full bg-(--accent)"
                  style={{ width: `${Math.max(2, (g.bytes / largest) * 100)}%` }}
                />
              </span>
              <span className="text-[12px] text-(--muted)">{g.what}</span>
            </button>
            {open === g.id && (
              <GroupItems
                group={g}
                onCleaned={(message) => {
                  setDone(message);
                  scan();
                }}
                onError={onError}
              />
            )}
          </li>
        ))}
      </ul>
    </Section>
  );
}

function GroupItems({
  group,
  onCleaned,
  onError,
}: {
  group: DiskGroup;
  onCleaned: (message: string) => void;
  onError: (e: string) => void;
}) {
  const [picked, setPicked] = useState<Set<string>>(() => new Set());
  const [confirm, setConfirm] = useState(false);
  const total = group.items.filter((i) => picked.has(i.path)).reduce((n, i) => n + i.bytes, 0);
  const toggle = (path: string) => {
    setConfirm(false);
    setPicked((s) => {
      const next = new Set(s);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };
  const clean = () => void api.diskClean(group.id, [...picked]).then(onCleaned, (e: unknown) => onError(String(e)));

  return (
    <div className="flex flex-col gap-2 border-t border-(--border) px-3 py-2.5">
      <ul className="settings-scroll flex max-h-56 flex-col gap-0.5 overflow-y-auto">
        {group.items.map((i) => (
          <li key={i.path} className="flex items-center gap-2 text-[12.5px]">
            {group.clearable && (
              <input
                type="checkbox"
                aria-label={`Select ${i.path}`}
                checked={picked.has(i.path)}
                onChange={() => toggle(i.path)}
              />
            )}
            <button
              type="button"
              title="Show in folder"
              onClick={() => void api.revealPath(i.path)}
              className="min-w-0 flex-1 truncate text-left hover:underline"
            >
              {i.path}
            </button>
            <span className="shrink-0 font-mono text-[11.5px] text-(--muted) tabular-nums">{human(i.bytes)}</span>
          </li>
        ))}
      </ul>
      <div className="flex flex-wrap items-center gap-2">
        {group.clearable && (
          <>
            <Button small onClick={() => setPicked(new Set(group.items.map((i) => i.path)))}>
              Select all
            </Button>
            {picked.size > 0 && !confirm && (
              <Button small primary onClick={() => setConfirm(true)}>
                Move {picked.size} to Recycle Bin ({human(total)})
              </Button>
            )}
            {confirm && (
              <>
                <span className="text-[12px]">You can restore them from the Recycle Bin.</span>
                <Button small primary onClick={clean}>
                  Move them
                </Button>
                <Button small onClick={() => setConfirm(false)}>
                  Cancel
                </Button>
              </>
            )}
          </>
        )}
        {group.id === "caches" && (
          <Button small onClick={() => void api.windowsSettingsOpen("storage")}>
            Open Storage Sense
          </Button>
        )}
      </div>
    </div>
  );
}

function human(bytes: number): string {
  const gb = bytes / 1024 ** 3;
  return gb >= 1 ? `${gb.toFixed(1)} GB` : `${Math.max(1, Math.round(bytes / 1024 ** 2))} MB`;
}
