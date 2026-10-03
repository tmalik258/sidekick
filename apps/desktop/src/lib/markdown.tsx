// A small Markdown renderer for chat answers: paragraphs, headings, bullet
// and numbered lists (one level of nesting), quotes, simple tables, fenced
// code (with a copy button), inline code, bold, italics and links. No HTML is
// ever injected; everything renders as React elements. Links to files,
// folders and pages open through Sidekick, never in this window; files and
// folders show as chips.
//
// Built for streaming: partial input renders sensibly (an open fence, a list
// still growing), and finished blocks are memoized so only the last one
// re-renders as words arrive.

import { memo, type ReactNode, useState } from "react";
import { Icon } from "@/components/Icon";
import { api } from "./bridge";

interface ListItem {
  text: string;
  sub: string[];
}

export type Block =
  | { kind: "code"; lang: string; text: string }
  | { kind: "heading"; level: number; text: string }
  | { kind: "list"; ordered: boolean; start: number; items: ListItem[] }
  | { kind: "quote"; text: string }
  | { kind: "table"; head: string[] | null; rows: string[][] }
  | { kind: "text"; text: string };

const FENCE = /^\s*```(\S*)/;
const HEADING = /^(#{1,4})\s+(.+?)\s*#*\s*$/;
const ITEM = /^(\s*)([-*•+]|\d{1,3}[.)])\s+(.*)$/;
const QUOTE = /^\s*>\s?(.*)$/;
const TABLE_ROW = /^\s*\|.*\|\s*$/;
const TABLE_RULE = /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$/;

function cells(row: string): string[] {
  return row
    .trim()
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split("|")
    .map((c) => c.trim());
}

export function splitBlocks(src: string): Block[] {
  const blocks: Block[] = [];
  const lines = src.split("\n");
  let para: string[] = [];
  const flush = () => {
    if (para.join("").trim()) blocks.push({ kind: "text", text: para.join("\n") });
    para = [];
  };
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    const fence = line.match(FENCE);
    if (fence) {
      flush();
      const body: string[] = [];
      i++;
      while (i < lines.length && !/^\s*```/.test(lines[i])) body.push(lines[i++]);
      i++; // closing fence (may be missing while streaming)
      blocks.push({ kind: "code", lang: fence[1] ?? "", text: body.join("\n") });
      continue;
    }
    if (!line.trim()) {
      flush();
      i++;
      continue;
    }
    const heading = line.match(HEADING);
    if (heading) {
      flush();
      blocks.push({ kind: "heading", level: heading[1].length, text: heading[2] });
      i++;
      continue;
    }
    const item = line.match(ITEM);
    if (item) {
      flush();
      const ordered = /\d/.test(item[2]);
      const list: Block & { kind: "list" } = {
        kind: "list",
        ordered,
        start: ordered ? Number.parseInt(item[2], 10) : 1,
        items: [],
      };
      const baseIndent = item[1].length;
      while (i < lines.length) {
        const l = lines[i];
        const m = l.match(ITEM);
        if (m && m[1].length <= baseIndent + 1 && /\d/.test(m[2]) === ordered) {
          list.items.push({ text: m[3], sub: [] });
        } else if (m && m[1].length > baseIndent + 1 && list.items.length > 0) {
          list.items[list.items.length - 1].sub.push(m[3]);
        } else if (l.trim() && /^\s{2,}\S/.test(l) && list.items.length > 0) {
          // A wrapped line belongs to the item above.
          const last = list.items[list.items.length - 1];
          last.text = `${last.text} ${l.trim()}`;
        } else {
          break;
        }
        i++;
      }
      blocks.push(list);
      continue;
    }
    if (QUOTE.test(line)) {
      flush();
      const body: string[] = [];
      while (i < lines.length && QUOTE.test(lines[i])) body.push(lines[i++].match(QUOTE)?.[1] ?? "");
      blocks.push({ kind: "quote", text: body.join("\n") });
      continue;
    }
    if (TABLE_ROW.test(line)) {
      flush();
      const rows: string[] = [];
      while (i < lines.length && (TABLE_ROW.test(lines[i]) || TABLE_RULE.test(lines[i]))) rows.push(lines[i++]);
      const ruled = rows.length > 1 && TABLE_RULE.test(rows[1]);
      const body = rows.filter((_, n) => !(ruled && n === 1)).map(cells);
      blocks.push({ kind: "table", head: ruled ? (body.shift() ?? null) : null, rows: body });
      continue;
    }
    para.push(line);
    i++;
  }
  flush();
  return blocks;
}

/** A file, folder or web address a link may point at. */
export function linkTarget(raw: string): string | null {
  const t = raw.trim().replace(/^<|>$/g, "");
  if (/^https?:\/\//i.test(t)) return t;
  if (/^file:\/\//i.test(t) || /^\/?[a-z]:[\\/]/i.test(t) || t.startsWith("\\\\") || t.startsWith("~/")) return t;
  return null;
}

function Link({ label, target }: { label: string; target: string }) {
  const [error, setError] = useState<string | null>(null);
  const web = /^https?:/i.test(target);
  const open = () => {
    setError(null);
    api.aiOpenLink(target).catch((e: unknown) => setError(String(e)));
  };
  if (!web) {
    // A file or folder: a chip that opens it.
    const folder = /[\\/]$/.test(target) || !/\.[a-z0-9]{1,8}$/i.test(target.replace(/[\\/]+$/, ""));
    return (
      <button
        type="button"
        title={error ?? `Open ${target}`}
        onClick={open}
        className={`chip inline-flex max-w-full items-center gap-1 rounded-md bg-white/[0.1] px-1.5 py-px align-baseline text-[0.92em] hover:bg-white/[0.16] ${
          error ? "text-[#ffb4ae]" : "text-white"
        }`}
      >
        <Icon name={folder ? "folder" : "file"} size={12} className="shrink-0 text-white/60" />
        <span className="truncate">{label}</span>
      </button>
    );
  }
  return (
    <button
      type="button"
      title={error ?? target}
      onClick={open}
      className={`inline rounded-sm text-left underline decoration-white/35 underline-offset-2 hover:decoration-white ${
        error ? "text-[#ffb4ae]" : "text-[#64d2ff]"
      }`}
    >
      {label}
    </button>
  );
}

const INLINE =
  /(\[[^\]\n]+\]\([^)\s]+(?: [^)]*)?\)|`[^`]+`|\*\*[^*]+\*\*|__[^_]+__|(?<![\w*])\*(?![\s*])[^*\n]+?\*(?![\w*])|(?<![\w_])_(?![\s_])[^_\n]+?_(?![\w_]))/g;

export function inline(text: string): ReactNode[] {
  const out: ReactNode[] = [];
  let last = 0;
  let k = 0;
  for (const m of text.matchAll(INLINE)) {
    const at = m.index ?? 0;
    if (at > last) out.push(text.slice(last, at));
    const tok = m[0];
    if (tok.startsWith("[")) {
      const link = tok.match(/^\[([^\]]+)\]\(([^)\s]+)/);
      const target = link ? linkTarget(link[2]) : null;
      if (link && target) out.push(<Link key={k++} label={link[1]} target={target} />);
      else out.push(link ? link[1] : tok);
    } else if (tok.startsWith("`")) {
      out.push(
        <code key={k++} className="rounded-[5px] bg-white/10 px-1 py-px font-mono text-[0.86em]">
          {tok.slice(1, -1)}
        </code>,
      );
    } else if (tok.startsWith("**") || tok.startsWith("__")) {
      out.push(
        <strong key={k++} className="font-semibold text-white">
          {tok.slice(2, -2)}
        </strong>,
      );
    } else {
      out.push(<em key={k++}>{tok.slice(1, -1)}</em>);
    }
    last = at + tok.length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

function CodeBlock({ lang, text }: { lang: string; text: string }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1400);
    } catch {
      // Clipboard access can be refused; nothing to do.
    }
  };
  return (
    <div className="group relative my-2 overflow-hidden rounded-xl bg-white/[0.07]">
      <div className="flex items-center justify-between px-3 pt-2 text-[11px] text-white/40">
        <span>{lang || "code"}</span>
        <button
          type="button"
          onClick={copy}
          className="rounded-md px-1.5 py-0.5 text-white/55 transition-colors duration-150 hover:bg-white/10 hover:text-white"
        >
          {copied ? "Copied" : "Copy"}
        </button>
      </div>
      <pre className="overflow-x-auto px-3 pt-1 pb-3 font-mono text-[12.5px] leading-relaxed text-white/90">{text}</pre>
    </div>
  );
}

function BlockView({ block }: { block: Block; sig: string }) {
  switch (block.kind) {
    case "code":
      return <CodeBlock lang={block.lang} text={block.text} />;
    case "heading":
      return (
        <p
          className={`mt-3 mb-1 font-semibold tracking-[-0.01em] text-white first:mt-0 ${
            block.level <= 1 ? "text-[15px]" : "text-[14px]"
          }`}
        >
          {inline(block.text)}
        </p>
      );
    case "list":
      // Bullets and numbers are drawn, not list markers, so they line up
      // with the text and look the same everywhere.
      return (
        <ul className="my-1.5 flex flex-col gap-1">
          {block.items.map((it, n) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: items only ever append while streaming
            <li key={n} className="flex gap-2">
              {block.ordered ? (
                <span className="w-4 shrink-0 text-right text-white/45 tabular-nums">{block.start + n}.</span>
              ) : (
                <span aria-hidden="true" className="mt-[0.62em] ml-1.5 size-[5px] shrink-0 rounded-full bg-white/45" />
              )}
              <span className="min-w-0 flex-1">
                {inline(it.text)}
                {it.sub.length > 0 && (
                  <ul className="mt-1 flex flex-col gap-0.5">
                    {it.sub.map((s, m) => (
                      // biome-ignore lint/suspicious/noArrayIndexKey: items only ever append while streaming
                      <li key={m} className="flex gap-2">
                        <span
                          aria-hidden="true"
                          className="mt-[0.62em] size-[5px] shrink-0 rounded-full border border-white/40"
                        />
                        <span className="min-w-0 flex-1">{inline(s)}</span>
                      </li>
                    ))}
                  </ul>
                )}
              </span>
            </li>
          ))}
        </ul>
      );
    case "quote":
      return (
        <blockquote className="my-1.5 border-l-2 border-white/20 pl-3 whitespace-pre-wrap text-white/70">
          {inline(block.text)}
        </blockquote>
      );
    case "table":
      return (
        <div className="my-2 overflow-x-auto rounded-xl bg-white/[0.05]">
          <table className="w-full border-collapse text-[12.5px]">
            {block.head && (
              <thead>
                <tr>
                  {block.head.map((c, n) => (
                    // biome-ignore lint/suspicious/noArrayIndexKey: columns have no identity beyond order
                    <th key={n} className="border-b border-white/10 px-3 py-1.5 text-left font-medium text-white/60">
                      {inline(c)}
                    </th>
                  ))}
                </tr>
              </thead>
            )}
            <tbody>
              {block.rows.map((r, n) => (
                // biome-ignore lint/suspicious/noArrayIndexKey: rows only ever append while streaming
                <tr key={n} className="border-b border-white/[0.06] last:border-0">
                  {r.map((c, m) => (
                    // biome-ignore lint/suspicious/noArrayIndexKey: columns have no identity beyond order
                    <td key={m} className="px-3 py-1.5 align-top tabular-nums">
                      {inline(c)}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      );
    default:
      return <p className="my-1.5 whitespace-pre-wrap">{inline(block.text)}</p>;
  }
}

/** Finished blocks keep their signature while an answer streams, so they
 * skip re-rendering; only the last one updates. */
const Memo = memo(BlockView, (a, b) => a.sig === b.sig);

export function Markdown({ text }: { text: string }) {
  return (
    <>
      {splitBlocks(text).map((b, i) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: blocks have no identity beyond order
        <Memo key={i} block={b} sig={JSON.stringify(b)} />
      ))}
    </>
  );
}
