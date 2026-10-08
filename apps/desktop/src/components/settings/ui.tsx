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
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";
import { setOverlayHit } from "@/lib/store";
import { Tip } from "../Tip";

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
      className={`chip shrink-0 rounded-full font-medium transition-[background-color,transform] duration-150 active:scale-[0.96] disabled:opacity-40 disabled:active:scale-100 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#0a84ff] ${
        primary ? "bg-[#f5f5f7] text-black hover:bg-white" : "bg-white/[0.09] text-white hover:bg-white/[0.15]"
      } ${small ? "px-2.5 py-[5px] text-[12px]" : "px-3 py-[7px] text-[13px]"} ${active ? "shadow-[inset_0_0_0_1.5px_#0a84ff]" : ""}`}
    >
      {children}
    </button>
  );
}

/** A few choices side by side, one picked (Off, Ask, Auto): the plan's
 * segmented control, a white pill on the picked one. */
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
    <fieldset aria-label={label} className="flex shrink-0 gap-0.5 rounded-[10px] bg-white/[0.08] p-[3px]">
      {options.map(([v, text]) => (
        <button
          key={v}
          type="button"
          aria-pressed={value === v}
          disabled={disabled}
          onClick={() => onChange(v)}
          className={`chip rounded-[8px] px-2.5 py-[5px] text-[12.5px] leading-none transition-colors duration-150 disabled:opacity-40 focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-[#0a84ff] ${
            value === v ? "bg-[#f5f5f7] font-semibold text-black" : "text-[rgb(235_235_245/0.62)] hover:text-white"
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

/** On or off, as the plan's two-part switch. */
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
    <Segmented
      label={label}
      value={checked ? "on" : "off"}
      options={[
        ["on", "On"],
        ["off", "Off"],
      ]}
      onChange={(v) => {
        if ((v === "on") !== checked) onChange(v === "on");
      }}
    />
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

/** One choice in a dropdown: a value and its label, or a richer row with a
 * line under it, a letter icon and a separator before it. */
export type SelectOption =
  | [string, string]
  | { value: string; label: string; sub?: string; icon?: string; color?: string; sepBefore?: boolean };

interface Row {
  value: string;
  label: string;
  sub?: string;
  icon?: string;
  color?: string;
  sepBefore?: boolean;
}

const toRow = (o: SelectOption): Row => (Array.isArray(o) ? { value: o[0], label: o[1] } : o);

/** A dropdown in the plan's style: a quiet chip that opens a graphite menu
 * with a tick on the current choice. The current value is kept even if not
 * listed. */
export function Select({
  value,
  options,
  onChange,
  label,
  className = "",
  group,
  current: currentLabel,
  variant = "chip",
  overlay,
  searchable,
}: {
  value: string;
  options: SelectOption[];
  onChange: (v: string) => void;
  label: string;
  className?: string;
  /** A small heading over the choices ("Open with"). */
  group?: string;
  /** What the closed chip says, when not the option's label. */
  current?: string;
  /** "plain": text with a chevron, for a line of details (Agents). */
  variant?: "chip" | "plain";
  /** Float the menu over the UI (portal to body) so a size-to-content
   * panel does not grow. */
  overlay?: boolean;
  /** A filter field at the top of the menu (long lists). */
  searchable?: boolean;
}) {
  const rows = options.map(toRow);
  // Keep an unknown current value visible, but never invent a second row that
  // matches an existing option (empty/"Default" aliases, same label).
  const list: Row[] = (() => {
    if (rows.some((r) => r.value === value)) return rows;
    const labelFor = value || "Default";
    if (rows.some((r) => r.value === "" || r.label.toLowerCase() === labelFor.toLowerCase())) return rows;
    return [{ value, label: labelFor }, ...rows];
  })();
  const current = currentLabel ?? list.find((r) => r.value === value)?.label ?? (value || "Default");
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const q = query.trim().toLowerCase();
  const shown = q ? list.filter((r) => `${r.label} ${r.sub ?? ""} ${r.value}`.toLowerCase().includes(q)) : list;
  const [active, setActive] = useState(() =>
    Math.max(
      0,
      list.findIndex((r) => r.value === value),
    ),
  );
  const root = useRef<HTMLDivElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const listId = useId();
  const rich = list.some((r) => r.sub || r.icon);
  // Opens upward when the panel has no room below the chip.
  const [up, setUp] = useState(false);
  const [float, setFloat] = useState<{ left: number; top: number; width: number; above: boolean } | null>(null);

  const menuChrome = (group ? 30 : 0) + (searchable ? 44 : 0) + 16;
  const placeOverlay = () => {
    const el = root.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const want = Math.min(288, Math.max(shown.length, 1) * (rich ? 44 : 32) + menuChrome);
    const above = window.innerHeight - r.bottom < want && r.top - want > 0;
    const width = Math.min(296, Math.max(rich ? 220 : r.width, r.width));
    const left = Math.min(Math.max(8, r.left), window.innerWidth - width - 8);
    const top = above ? r.top - 4 : r.bottom + 4;
    setUp(above);
    setFloat({ left, top, width, above });
    // Expand the island's clickable area so the menu is not click-through.
    setOverlayHit({
      x: left,
      y: above ? top - want : top,
      width,
      height: want,
    });
  };

  // biome-ignore lint/correctness/useExhaustiveDependencies: list is rebuilt every render; reseat from value when opened
  useEffect(() => {
    if (!open) {
      setFloat(null);
      setQuery("");
      if (overlay) setOverlayHit(null);
      return;
    }
    const idx = shown.findIndex((r) => r.value === value);
    setActive(Math.max(0, idx));
    const el = root.current;
    if (el && overlay) {
      placeOverlay();
    } else if (el) {
      let box: Element | null = el.parentElement;
      while (box && box !== document.body && getComputedStyle(box).overflowY === "visible") box = box.parentElement;
      const limit = box && box !== document.body ? box.getBoundingClientRect().bottom : window.innerHeight;
      const want = Math.min(288, Math.max(shown.length, 1) * (rich ? 44 : 32) + menuChrome);
      const r = el.getBoundingClientRect();
      setUp(limit - r.bottom < want && r.top - want > 0);
    }
    const onDoc = (e: MouseEvent) => {
      const t = e.target as Node;
      if (!root.current?.contains(t) && !menu.current?.contains(t)) setOpen(false);
    };
    const onReposition = () => {
      if (overlay) placeOverlay();
      else setOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    if (overlay) {
      window.addEventListener("resize", onReposition);
      window.addEventListener("scroll", onReposition, true);
    }
    return () => {
      document.removeEventListener("mousedown", onDoc);
      window.removeEventListener("resize", onReposition);
      window.removeEventListener("scroll", onReposition, true);
      if (overlay) setOverlayHit(null);
    };
  }, [open, value, overlay]);

  useEffect(() => {
    if (!open || !searchable) return;
    requestAnimationFrame(() => searchRef.current?.focus({ preventScroll: true }));
  }, [open, searchable]);

  // Keep the highlight on a visible row while filtering.
  useEffect(() => {
    if (!open) return;
    setActive((i) => (shown.length === 0 ? 0 : Math.min(i, shown.length - 1)));
  }, [open, shown.length]);

  // Prefer the painted menu box once it exists (estimated height can be high).
  // biome-ignore lint/correctness/useExhaustiveDependencies: shown.length and query resize the menu, so they re-measure it
  useLayoutEffect(() => {
    if (!open || !overlay || !float) return;
    const m = menu.current;
    if (!m) return;
    const r = m.getBoundingClientRect();
    setOverlayHit({ x: r.left, y: r.top, width: r.width, height: r.height });
  }, [open, overlay, float, shown.length, query]);

  const pick = (v: string) => {
    onChange(v);
    setOpen(false);
  };

  const move = (dir: 1 | -1) => {
    if (shown.length === 0) return;
    setActive((i) => (i + dir + shown.length) % shown.length);
  };

  const onMenuKey = (e: ReactKeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      if (query) setQuery("");
      else setOpen(false);
      return;
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      move(1);
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      move(-1);
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      const next = shown[active];
      if (next) pick(next.value);
    }
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
      move(e.key === "ArrowDown" ? 1 : -1);
      return;
    }
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      if (!open) {
        setOpen(true);
        return;
      }
      const next = shown[active];
      if (next) pick(next.value);
    }
  };

  const optionsList = (
    <>
      {shown.map((r, i) => {
        const selected = r.value === value;
        return (
          <div key={r.value || "__default"}>
            {r.sepBefore && !q && <div className="mx-[9px] my-[5px] h-px scale-y-50 bg-white/12" />}
            <div
              role="option"
              tabIndex={-1}
              aria-selected={selected}
              onMouseEnter={() => setActive(i)}
              onClick={() => pick(r.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") pick(r.value);
              }}
              className={`grid w-full cursor-default items-center gap-2.5 rounded-[9px] px-[9px] py-1.5 text-left text-[13px] transition-colors duration-100 ${
                r.icon ? "grid-cols-[24px_1fr_auto]" : "grid-cols-[1fr_auto]"
              } ${i === active ? "bg-white/[0.09] text-white" : "text-white/85"}`}
            >
              {r.icon && (
                <span
                  className="grid size-6 place-items-center rounded-[7px] text-[10.5px] font-bold text-white shadow-[inset_0_0_0_0.5px_rgb(255_255_255/0.12)]"
                  style={{ background: r.color ?? "#2b2b30" }}
                  aria-hidden="true"
                >
                  {r.icon}
                </span>
              )}
              <span className="grid min-w-0">
                <span className={`truncate ${r.sub ? "font-medium" : ""}`}>{r.label}</span>
                {r.sub && <span className="truncate text-[11.5px] text-white/62">{r.sub}</span>}
              </span>
              <svg
                viewBox="0 0 16 16"
                aria-hidden="true"
                className={`size-3.5 transition-opacity duration-100 ${selected ? "opacity-100" : "opacity-0"}`}
              >
                <path
                  d="M3.5 8.5 6.5 11.5 12.5 4.5"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.8"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                />
              </svg>
            </div>
          </div>
        );
      })}
      {shown.length === 0 && <div className="px-[9px] py-2.5 text-[12.5px] text-white/45">No matches</div>}
    </>
  );

  const menuList = (
    <div
      id={listId}
      role="listbox"
      aria-label={label}
      ref={menu}
      onKeyDown={onMenuKey}
      className={`menu z-30 flex max-h-72 flex-col rounded-[15px] p-1.5 ${
        overlay
          ? `[--menu-origin:${up ? "bottom_left" : "top_left"}]`
          : `absolute ${variant === "plain" ? "left-0" : "right-0"} ${
              up ? "bottom-[calc(100%+4px)] [--menu-origin:bottom_right]" : "top-[calc(100%+4px)]"
            } ${rich ? "w-[296px]" : "w-max min-w-full max-w-64"}`
      }`}
      style={
        overlay && float
          ? {
              position: "fixed",
              left: float.left,
              top: float.above ? undefined : float.top,
              bottom: float.above ? window.innerHeight - float.top : undefined,
              width: float.width,
              zIndex: 100,
            }
          : undefined
      }
    >
      {searchable && (
        <input
          ref={searchRef}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={`Search ${label.toLowerCase()}…`}
          spellCheck={false}
          aria-label={`Search ${label}`}
          className="mb-1 w-full shrink-0 rounded-[9px] bg-white/[0.08] px-2.5 py-1.5 text-[12.5px] text-white outline-none placeholder:text-white/40 focus:bg-white/[0.11]"
        />
      )}
      {group && (
        <div className="shrink-0 px-[9px] pt-1 pb-[3px] text-[10.5px] tracking-[0.06em] text-white/45 uppercase">
          {group}
        </div>
      )}
      <div className="island-scroll min-h-0 flex-1 overflow-y-auto">{optionsList}</div>
    </div>
  );

  return (
    <div ref={root} className={`relative max-w-56 ${className}`}>
      <button
        type="button"
        aria-label={label}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={listId}
        onClick={() => setOpen((o) => !o)}
        onKeyDown={onKey}
        className={`chip flex w-full items-center gap-1.5 text-left whitespace-nowrap transition-colors duration-150 focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-[#0a84ff] ${
          variant === "plain"
            ? `rounded-md px-1 py-0.5 text-[inherit] ${open ? "bg-white/[0.1]" : "hover:bg-white/[0.06]"}`
            : `rounded-[10px] px-2.5 py-1.5 text-[13px] ${open ? "bg-white/[0.14]" : "bg-white/[0.08] hover:bg-white/[0.11]"}`
        }`}
      >
        <span className="min-w-0 flex-1 truncate">{current}</span>
        <svg
          aria-hidden="true"
          viewBox="0 0 10 10"
          className={`size-[9px] shrink-0 transition-[transform,opacity] duration-150 ${open ? "rotate-180 opacity-90" : "opacity-55"}`}
        >
          <path
            d="M2 3.5 5 6.5 8 3.5"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.6"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      </button>
      {open && !overlay && menuList}
      {open && overlay && float && createPortal(menuList, document.body)}
    </div>
  );
}

export function StatusDot({ state }: { state: "ok" | "off" | "checking" }) {
  const label = state === "ok" ? "Ready" : state === "off" ? "Not reachable" : "Checking";
  return (
    <Tip label={label}>
      <span
        role="img"
        aria-label={label}
        className={`size-2 shrink-0 rounded-full ${
          state === "ok"
            ? "bg-[#30d158]"
            : state === "off"
              ? "bg-black/20 dark:bg-white/25"
              : "animate-pulse bg-amber-400"
        }`}
      />
    </Tip>
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
          <Tip key={i} label={i}>
            <span className="flex items-center gap-1 rounded-full bg-black/5 py-0.5 pr-1 pl-2.5 text-[12px] dark:bg-white/10">
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
          </Tip>
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
