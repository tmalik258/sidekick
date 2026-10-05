"use client";

// Building blocks shared by the settings tabs. Sections hide themselves when
// the settings search does not match their title, hint or keywords.

import {
  createContext,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
  useContext,
  useEffect,
  useId,
  useRef,
  useState,
} from "react";

/** The settings search text; empty shows everything. */
export const SettingsQuery = createContext("");

function matches(query: string, ...texts: (string | undefined)[]) {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const hay = texts.filter(Boolean).join(" ").toLowerCase();
  return words.every((w) => hay.includes(w));
}

export function Section({
  title,
  hint,
  keywords,
  collapsible,
  summary,
  children,
}: {
  title: string;
  hint?: string;
  /** Extra words people might search for, e.g. "hotkey keyboard". */
  keywords?: string;
  /** Shown as one row that opens on tap; searching opens it. */
  collapsible?: boolean;
  /** One line under a collapsed title, e.g. the current value. */
  summary?: string;
  children: ReactNode;
}) {
  const query = useContext(SettingsQuery);
  const [open, setOpen] = useState(false);
  if (query && !matches(query, title, hint, keywords)) return null;
  if (collapsible && !open && !query) {
    return (
      <button
        type="button"
        onClick={() => setOpen(true)}
        aria-expanded={false}
        className="chip flex items-center gap-3 rounded-2xl bg-(--surface) px-4 py-3 text-left shadow-[0_0_0_0.5px_var(--border)] hover:bg-white/[0.06]"
      >
        <span className="min-w-0 flex-1">
          <span className="block text-[14px] font-medium text-white">{title}</span>
          {summary && <span className="block truncate text-[12px] text-(--muted)">{summary}</span>}
        </span>
        <svg aria-hidden="true" viewBox="0 0 12 12" className="size-3 shrink-0 -rotate-90 text-(--muted)">
          <path
            fill="currentColor"
            d="M2.2 4.2a.75.75 0 0 1 1.06 0L6 6.94l2.74-2.74a.75.75 0 1 1 1.06 1.06l-3.27 3.27a.75.75 0 0 1-1.06 0L2.2 5.26a.75.75 0 0 1 0-1.06Z"
          />
        </svg>
      </button>
    );
  }
  return (
    <section className="flex flex-col gap-2">
      <div className="flex items-center px-4">
        <h2 className="flex-1 text-[13px] font-semibold tracking-[-0.005em] text-[rgb(235_235_245/0.7)]">{title}</h2>
        {collapsible && !query && (
          <button
            type="button"
            onClick={() => setOpen(false)}
            className="chip text-[12px] text-(--muted) hover:text-white"
          >
            Close
          </button>
        )}
      </div>
      <div className="flex flex-col gap-3.5 rounded-2xl bg-(--surface) p-4 shadow-[0_0_0_0.5px_var(--border),0_1px_2px_rgb(0_0_0/0.04)]">
        {children}
      </div>
      {hint && <p className="px-4 text-[12px] text-(--muted)">{hint}</p>}
    </section>
  );
}

export function Button({
  children,
  onClick,
  disabled,
  small,
  active,
  primary,
}: {
  children: ReactNode;
  onClick: () => void;
  disabled?: boolean;
  small?: boolean;
  active?: boolean;
  primary?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={`chip shrink-0 rounded-full font-medium transition-colors disabled:opacity-40 ${
        primary ? "bg-white text-black hover:bg-white/90" : "bg-white/[0.1] text-white/90 hover:bg-white/[0.16]"
      } ${small ? "px-2.5 py-1 text-[12px]" : "px-3.5 py-1.5 text-[13px]"} ${active ? "ring-1 ring-white/40" : ""}`}
    >
      {children}
    </button>
  );
}

/** A few choices side by side, one picked (Off, Ask, Auto). */
export function Segmented<T extends string>({
  value,
  options,
  onChange,
  label,
  disabled,
}: {
  value: T;
  options: [T, string][];
  onChange: (v: T) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <fieldset aria-label={label} className="flex shrink-0 rounded-full bg-white/[0.08] p-0.5">
      {options.map(([v, text]) => (
        <button
          key={v}
          type="button"
          aria-pressed={value === v}
          disabled={disabled}
          onClick={() => onChange(v)}
          className={`chip rounded-full px-2.5 py-0.5 text-[12px] font-medium transition-colors disabled:opacity-40 ${
            value === v ? "bg-white text-black" : "text-white/70 hover:text-white"
          }`}
        >
          {text}
        </button>
      ))}
    </fieldset>
  );
}

export function Toggle({
  label,
  hint,
  checked,
  onChange,
}: {
  label: string;
  hint?: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <div className="flex items-center justify-between gap-4 text-[14px]">
      <div className="min-w-0">
        <p>{label}</p>
        {hint && <p className="text-[12px] text-(--muted)">{hint}</p>}
      </div>
      <Switch checked={checked} onChange={onChange} label={label} />
    </div>
  );
}

export function Switch({
  checked,
  onChange,
  label,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={() => onChange(!checked)}
      className={`relative h-[26px] w-[44px] shrink-0 rounded-full transition-colors duration-200 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#0a84ff] ${
        checked ? "bg-[#30d158]" : "bg-black/15 dark:bg-white/20"
      }`}
    >
      <span
        className="absolute top-[2px] left-[2px] size-[22px] rounded-full bg-white shadow-[0_2px_6px_rgb(0_0_0/0.2)] transition-transform duration-[260ms] ease-(--ease-out-strong)"
        style={{ transform: checked ? "translateX(18px)" : "translateX(0)" }}
      />
    </button>
  );
}

export function Slider({ label, value, onChange }: { label: string; value: number; onChange: (v: number) => void }) {
  return (
    <label className="flex flex-1 items-center justify-between gap-3 text-sm capitalize">
      {label}
      <input
        type="range"
        min={0}
        max={1}
        step={0.05}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        className="w-36 accent-(--accent)"
      />
    </label>
  );
}

/** A text input that saves on Enter or when it loses focus, not per key. */
export function TextField({
  value,
  onCommit,
  placeholder,
  className = "",
  mono,
  label,
}: {
  label?: string;
  value: string;
  onCommit: (v: string) => void;
  placeholder?: string;
  className?: string;
  mono?: boolean;
}) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const commit = () => {
    if (draft !== value) onCommit(draft.trim());
  };
  return (
    <input
      value={draft}
      aria-label={label}
      placeholder={placeholder}
      spellCheck={false}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") commit();
        if (e.key === "Escape") setDraft(value);
      }}
      className={`rounded-md border border-(--border) bg-transparent px-2 py-1 text-[13px] outline-none focus:border-(--accent) ${
        mono ? "font-mono text-[12px]" : ""
      } ${className}`}
    />
  );
}

export function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4 text-[13px]">
      <span className="min-w-0">
        {label}
        {hint && <span className="block text-[11.5px] text-(--muted)">{hint}</span>}
      </span>
      {children}
    </div>
  );
}

/** A dropdown. `options` are [value, label]; the current value is kept even if not listed. */
export function Select({
  value,
  options,
  onChange,
  label,
  className = "",
}: {
  value: string;
  options: [string, string][];
  onChange: (v: string) => void;
  label: string;
  className?: string;
}) {
  // Keep an unknown current value visible, but never invent a second row that
  // matches an existing option (empty/"Default" aliases, same label).
  const list = (() => {
    if (options.some(([v]) => v === value)) return options;
    const labelFor = value || "Default";
    if (options.some(([v, l]) => v === "" || l.toLowerCase() === labelFor.toLowerCase())) return options;
    return [[value, labelFor] as [string, string], ...options];
  })();
  const current = list.find(([v]) => v === value)?.[1] ?? (value || "Default");
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(() =>
    Math.max(
      0,
      list.findIndex(([v]) => v === value),
    ),
  );
  const root = useRef<HTMLDivElement>(null);
  const listId = useId();

  // biome-ignore lint/correctness/useExhaustiveDependencies: list is rebuilt every render; reseat from value when opened
  useEffect(() => {
    if (!open) return;
    setActive(
      Math.max(
        0,
        list.findIndex(([v]) => v === value),
      ),
    );
    const onDoc = (e: MouseEvent) => {
      if (root.current && !root.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [open, value]); // list identity changes every render; reseat from value when opened

  const pick = (v: string) => {
    onChange(v);
    setOpen(false);
  };

  const onKey = (e: ReactKeyboardEvent<HTMLButtonElement>) => {
    if (e.key === "Escape") {
      e.preventDefault();
      setOpen(false);
      return;
    }
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!open) {
        setOpen(true);
        return;
      }
      const dir = e.key === "ArrowDown" ? 1 : -1;
      setActive((i) => (i + dir + list.length) % list.length);
      return;
    }
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      if (!open) {
        setOpen(true);
        return;
      }
      const next = list[active];
      if (next) pick(next[0]);
    }
  };

  return (
    <div ref={root} className={`relative max-w-52 ${className}`}>
      <button
        type="button"
        aria-label={label}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={listId}
        onClick={() => setOpen((o) => !o)}
        onKeyDown={onKey}
        className={`chip flex w-full items-center gap-2 rounded-xl bg-(--surface) px-3 py-1.5 text-left text-[13px] outline-none ring-1 ring-inset transition-colors ${
          open ? "ring-(--accent)" : "ring-(--border) hover:bg-(--hover)"
        } focus-visible:ring-(--accent)`}
      >
        <span className="min-w-0 flex-1 truncate">{current}</span>
        <svg
          aria-hidden="true"
          viewBox="0 0 12 12"
          className={`size-3 shrink-0 text-(--muted) transition-transform ${open ? "rotate-180" : ""}`}
        >
          <path
            fill="currentColor"
            d="M2.2 4.2a.75.75 0 0 1 1.06 0L6 6.94l2.74-2.74a.75.75 0 1 1 1.06 1.06l-3.27 3.27a.75.75 0 0 1-1.06 0L2.2 5.26a.75.75 0 0 1 0-1.06Z"
          />
        </svg>
      </button>
      {open && (
        <div
          id={listId}
          role="listbox"
          aria-label={label}
          className="island-scroll absolute top-[calc(100%+4px)] right-0 z-30 max-h-56 w-max min-w-full max-w-64 overflow-y-auto rounded-xl bg-[#1c1c24] py-1 shadow-[0_12px_40px_rgb(0_0_0/0.55)] ring-1 ring-inset ring-white/15"
        >
          {list.map(([v, l], i) => {
            const selected = v === value;
            return (
              <div
                key={v || "__default"}
                role="option"
                tabIndex={-1}
                aria-selected={selected}
                onMouseEnter={() => setActive(i)}
                onClick={() => pick(v)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") pick(v);
                }}
                className={`flex w-full cursor-default items-center px-3 py-1.5 text-left text-[13px] ${
                  i === active || selected ? "bg-white/12 text-white" : "text-white/80"
                } ${selected ? "font-medium" : ""}`}
              >
                <span className="truncate">{l}</span>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

export function StatusDot({ state }: { state: "ok" | "off" | "checking" }) {
  const label = state === "ok" ? "Ready" : state === "off" ? "Not reachable" : "Checking";
  return (
    <span
      role="img"
      aria-label={label}
      title={label}
      className={`size-2 shrink-0 rounded-full ${
        state === "ok"
          ? "bg-[#30d158]"
          : state === "off"
            ? "bg-black/20 dark:bg-white/25"
            : "animate-pulse bg-amber-400"
      }`}
    />
  );
}

export function CopyButton({ text, label = "Copy" }: { text: string; label?: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <Button
      small
      onClick={() =>
        void navigator.clipboard
          .writeText(text)
          .then(() => {
            setCopied(true);
            setTimeout(() => setCopied(false), 1500);
          })
          .catch(() => {
            // Clipboard access can be refused; the text is selectable anyway.
          })
      }
    >
      {copied ? "Copied" : label}
    </Button>
  );
}

/** A list of removable chips with a picker (and typing) to add more. */
export function ChipList({
  items,
  onChange,
  suggestions,
  placeholder,
  label,
  format = (v: string) => v,
}: {
  items: string[];
  onChange: (next: string[]) => void;
  suggestions: string[];
  placeholder: string;
  label: string;
  /** How an item reads, e.g. "bitwarden.exe" as "Bitwarden". */
  format?: (v: string) => string;
}) {
  const [draft, setDraft] = useState("");
  const add = (v: string) => {
    const value = v.trim().toLowerCase();
    if (value && !items.includes(value)) onChange([...items, value]);
    setDraft("");
  };
  const listId = `${label.replace(/\s+/g, "-")}-options`;
  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-wrap gap-1.5">
        {items.map((i) => (
          <span
            key={i}
            title={i}
            className="flex items-center gap-1 rounded-full bg-black/5 py-0.5 pr-1 pl-2.5 text-[12px] dark:bg-white/10"
          >
            {format(i)}
            <button
              type="button"
              aria-label={`Remove ${format(i)}`}
              onClick={() => onChange(items.filter((x) => x !== i))}
              className="grid size-4 place-items-center rounded-full text-(--muted) hover:bg-black/10 hover:text-(--text) dark:hover:bg-white/15"
            >
              ×
            </button>
          </span>
        ))}
        {items.length === 0 && <span className="text-[12px] text-(--muted)">None</span>}
      </div>
      <div className="flex gap-2">
        <input
          list={listId}
          value={draft}
          aria-label={label}
          placeholder={placeholder}
          spellCheck={false}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") add(draft);
          }}
          className="min-w-0 flex-1 rounded-md border border-(--border) bg-transparent px-2 py-1 text-[13px] outline-none focus:border-(--accent)"
        />
        <datalist id={listId}>
          {suggestions
            .filter((s) => !items.includes(s))
            .map((s) => (
              <option key={s} value={s} />
            ))}
        </datalist>
        <Button small onClick={() => add(draft)} disabled={!draft.trim()}>
          Add
        </Button>
      </div>
    </div>
  );
}

/** Folder checkboxes: found ones first, then any already chosen. */
export function FolderPicker({
  found,
  chosen,
  onChange,
  empty,
}: {
  found: { path: string; label: string; repos?: number }[];
  chosen: string[];
  onChange: (next: string[]) => void;
  empty: string;
}) {
  const known = new Set(found.map((f) => f.path));
  const rows = [
    ...found,
    ...chosen.filter((c) => !known.has(c)).map((path) => ({ path, label: path.split(/[\\/]/).pop() || path })),
  ];
  const [draft, setDraft] = useState("");
  return (
    <div className="flex flex-col gap-1.5 text-[13px]">
      {rows.length === 0 && empty && <p className="text-[12px] text-(--muted)">{empty}</p>}
      {rows.map((f) => (
        <label key={f.path} className="flex cursor-pointer items-center gap-2.5">
          <input
            type="checkbox"
            checked={chosen.includes(f.path)}
            onChange={(e) => onChange(e.target.checked ? [...chosen, f.path] : chosen.filter((c) => c !== f.path))}
            className="check shrink-0"
          />
          <span className="min-w-0">
            <span className="font-medium">{f.label}</span>
            {"repos" in f && typeof f.repos === "number" && f.repos > 0 && (
              <span className="text-(--muted)">
                {" "}
                · {f.repos} {f.repos === 1 ? "repo" : "repos"}
              </span>
            )}
            <span className="block truncate font-mono text-[11px] text-(--muted)">{f.path}</span>
          </span>
        </label>
      ))}
      <div className="mt-1 flex gap-2">
        <input
          value={draft}
          placeholder="Add another folder (paste its path)"
          aria-label="Add a folder"
          spellCheck={false}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && draft.trim()) {
              onChange([...chosen, draft.trim()]);
              setDraft("");
            }
          }}
          className="min-w-0 flex-1 rounded-md border border-(--border) bg-transparent px-2 py-1 font-mono text-[12px] outline-none focus:border-(--accent)"
        />
        <Button
          small
          disabled={!draft.trim()}
          onClick={() => {
            onChange([...chosen, draft.trim()]);
            setDraft("");
          }}
        >
          Add
        </Button>
      </div>
    </div>
  );
}

/** Records a key combination: click, press the keys, done. */
export function ShortcutRecorder({
  value,
  onChange,
  label,
}: {
  value: string;
  onChange: (keys: string) => void;
  label: string;
}) {
  const [recording, setRecording] = useState(false);
  useEffect(() => {
    if (!recording) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") return setRecording(false);
      const keys = comboFrom(e);
      if (keys) {
        setRecording(false);
        onChange(keys);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [recording, onChange]);
  return (
    <div className="flex items-center gap-1.5">
      <button
        type="button"
        aria-label={`${label} shortcut`}
        onClick={() => setRecording(true)}
        className={`min-w-32 rounded-md border px-2 py-1 text-center font-mono text-[12px] ${
          recording ? "border-[#0a84ff] text-[#0a84ff]" : "border-(--border)"
        }`}
      >
        {recording ? "Press keys..." : value || "Off"}
      </button>
      {value && !recording && (
        <Button small onClick={() => onChange("")}>
          Off
        </Button>
      )}
    </div>
  );
}

const KEY_NAMES: Record<string, string> = {
  " ": "Space",
  ",": "Comma",
  ".": "Period",
  "/": "Slash",
  ";": "Semicolon",
  "'": "Quote",
  "[": "BracketLeft",
  "]": "BracketRight",
  "-": "Minus",
  "=": "Equal",
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
};

/** "Ctrl+Alt+K" from a key event, or null while only modifiers are held. */
export function comboFrom(e: { key: string; ctrlKey: boolean; altKey: boolean; shiftKey: boolean; metaKey: boolean }) {
  if (["Control", "Alt", "Shift", "Meta", "AltGraph"].includes(e.key)) return null;
  const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(Boolean);
  // A shortcut needs a modifier, except the function keys.
  if (mods.length === 0 && !/^F\d{1,2}$/.test(e.key)) return null;
  const key = KEY_NAMES[e.key] ?? (e.key.length === 1 ? e.key.toUpperCase() : e.key);
  return [...mods, key].join("+");
}

/** "keepassxc.exe" reads as "Keepassxc", "1password.exe" as "1password". */
export function appName(exe: string): string {
  const base = exe
    .replace(/\.exe$/i, "")
    .replace(/[-_]+/g, " ")
    .trim();
  return base ? base.charAt(0).toUpperCase() + base.slice(1) : exe;
}
