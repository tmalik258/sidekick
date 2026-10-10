"use client";
import { api } from "@/lib/bridge";
import type { ChatSummary, SearchHit } from "@/lib/types";
import { Icon } from "../Icon";
import { Tip } from "../Tip";
import { ago, Kbd, Snippet, SOURCE_LABELS, scrollIfActive } from "./parts";

/** Past Ask chats: arrows or the mouse pick, Enter or a click opens one.
 * The delete button removes it. Typing filters by title. */
export function ChatHistory({
  items,
  filtered,
  active,
  onHover,
  onOpen,
  onDelete,
}: {
  items: ChatSummary[];
  filtered: boolean;
  active: number;
  onHover: (i: number) => void;
  onOpen: (id: string) => void;
  onDelete: (id: string) => void;
}) {
  if (items.length === 0)
    return (
      <p className="py-2 text-[13px] text-[rgb(235_235_245/0.6)]">
        {filtered ? "No chats match." : "No past chats yet."}
      </p>
    );
  return (
    <ul className="py-1" aria-label="Chat history">
      {items.map((c, i) => (
        <li key={c.id} ref={scrollIfActive(i === active)} className="flex items-center gap-0.5">
          <button
            type="button"
            aria-current={i === active}
            onMouseMove={() => onHover(i)}
            onClick={() => onOpen(c.id)}
            className={`flex min-w-0 flex-1 items-center gap-2 rounded-[14px] px-1.5 py-1.5 text-left transition-colors duration-100 ${
              i === active ? "bg-white/[0.14] ring-1 ring-inset ring-white/25" : "hover:bg-white/[0.08]"
            }`}
          >
            <span className="grid size-6 shrink-0 place-items-center rounded-full bg-white/[0.12] text-white/85">
              <Icon name="ask" size={13} />
            </span>
            <span className="min-w-0 flex-1 truncate text-[13px] text-white/90">{c.title}</span>
            <span className="shrink-0 text-[11px] text-[rgb(235_235_245/0.45)]">{ago(c.updated)}</span>
            {i === active && <Kbd>Enter</Kbd>}
          </button>
          <Tip label="Delete">
            <button
              type="button"
              aria-label={`Delete ${c.title}`}
              onClick={(e) => {
                e.stopPropagation();
                onDelete(c.id);
              }}
              className="chip grid size-7 shrink-0 place-items-center rounded-full text-white/55 hover:bg-white/[0.12] hover:text-white/90"
            >
              <Icon name="close" size={14} />
            </button>
          </Tip>
        </li>
      ))}
    </ul>
  );
}

/** Clipboard history: arrows or the mouse pick, Enter or a click copies
 * again. Typing filters the list. */
export function Clips({
  items,
  filtered,
  active,
  onHover,
}: {
  items: { text: string; ts: string }[];
  filtered: boolean;
  active: number;
  onHover: (i: number) => void;
}) {
  if (items.length === 0)
    return (
      <p className="py-2 text-[13px] text-[rgb(235_235_245/0.6)]">
        {filtered ? "No copied text matches." : "Nothing copied yet. Secrets are never kept."}
      </p>
    );
  return (
    <ul className="py-1" aria-label="Clipboard history">
      {items.map((c, i) => (
        <li key={`${c.ts}${c.text.slice(0, 40)}`} ref={scrollIfActive(i === active)}>
          <button
            type="button"
            aria-current={i === active}
            onMouseMove={() => onHover(i)}
            onClick={() => void api.clipboardCopy(c.text).then(() => api.askClose())}
            className={`flex w-full items-center gap-2 rounded-[14px] px-1.5 py-1.5 text-left transition-colors duration-100 ${
              i === active ? "bg-white/[0.14] ring-1 ring-inset ring-white/25" : "hover:bg-white/[0.08]"
            }`}
          >
            <span className="line-clamp-2 min-w-0 flex-1 font-mono text-[12px] break-all text-white/85">{c.text}</span>
            <span className="shrink-0 text-[11px] text-[rgb(235_235_245/0.45)] tabular-nums">
              {new Date(c.ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
            </span>
            {i === active && <Kbd>Enter</Kbd>}
          </button>
        </li>
      ))}
    </ul>
  );
}

export function Results({
  query,
  items,
  active,
  onHover,
  onOpen,
}: {
  query: string;
  items: SearchHit[];
  active: number;
  onHover: (i: number) => void;
  onOpen: (hit: SearchHit) => void;
}) {
  if (items.length === 0)
    return (
      <p className="py-2 text-[13px] text-[rgb(235_235_245/0.6)]">
        Nothing found for &quot;{query}&quot;. Add folders under Settings &gt; Search to find your files.
      </p>
    );
  return (
    <ul className="py-1">
      {items.map((h, i) => {
        const opens = ["file", "download", "screenshot", "page"].includes(h.source);
        return (
          <li key={`${h.source}|${h.reference}`} ref={scrollIfActive(i === active)}>
            <button
              type="button"
              disabled={!opens}
              aria-current={i === active}
              onMouseMove={() => onHover(i)}
              onClick={() => onOpen(h)}
              className={`flex w-full flex-col gap-0.5 rounded-[14px] px-1.5 py-1.5 text-left transition-colors duration-100 ${
                i === active ? "bg-white/[0.14] ring-1 ring-inset ring-white/25" : "enabled:hover:bg-white/[0.08]"
              }`}
            >
              <span className="flex items-center gap-2 text-[13px]">
                <span className="rounded-full bg-white/[0.12] px-1.5 py-px text-[10.5px] text-white/70">
                  {SOURCE_LABELS[h.source] ?? h.source}
                </span>
                <span className="truncate text-white/90">{h.title || h.reference}</span>
              </span>
              <span className="line-clamp-2 text-[12px] text-[rgb(235_235_245/0.62)]">
                <Snippet text={h.snippet} />
              </span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}
