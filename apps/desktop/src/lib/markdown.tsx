// A deliberately small Markdown renderer for chat answers: fenced code
// blocks (with a copy button), inline code, bold, links and paragraphs. No
// HTML is ever injected; everything renders as React text. Links to files,
// folders and pages open through Sidekick, never in this window.

import { memo, type ReactNode, useState } from "react";
import { api } from "./bridge";

type Block = { kind: "code"; lang: string; text: string } | { kind: "text"; text: string };

export function splitBlocks(src: string): Block[] {
  const blocks: Block[] = [];
  const lines = src.split("\n");
  let i = 0;
  let para: string[] = [];
  const flush = () => {
    if (para.join("").trim()) blocks.push({ kind: "text", text: para.join("\n") });
    para = [];
  };
  while (i < lines.length) {
    const fence = lines[i].match(/^\s*```(\S*)/);
    if (fence) {
      flush();
      const body: string[] = [];
      i++;
      while (i < lines.length && !/^\s*```/.test(lines[i])) body.push(lines[i++]);
      i++; // closing fence (may be missing while streaming)
      blocks.push({ kind: "code", lang: fence[1] ?? "", text: body.join("\n") });
      continue;
    }
    if (!lines[i].trim()) flush();
    else para.push(lines[i]);
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
  return (
    <button
      type="button"
      title={error ?? (web ? target : `Open ${target}`)}
      onClick={() => {
        setError(null);
        api.aiOpenLink(target).catch((e: unknown) => setError(String(e)));
      }}
      className={`inline rounded-sm text-left underline decoration-white/35 underline-offset-2 hover:decoration-white ${
        error ? "text-[#ffb4ae]" : "text-[#64d2ff]"
      }`}
    >
      {label}
    </button>
  );
}

function inline(text: string): ReactNode[] {
  const out: ReactNode[] = [];
  const re = /(\[[^\]\n]+\]\([^)\s]+(?: [^)]*)?\)|`[^`]+`|\*\*[^*]+\*\*)/g;
  let last = 0;
  let k = 0;
  for (const m of text.matchAll(re)) {
    const at = m.index ?? 0;
    if (at > last) out.push(text.slice(last, at));
    const tok = m[0];
    const link = tok.startsWith("[") ? tok.match(/^\[([^\]]+)\]\(([^)\s]+)/) : null;
    const target = link ? linkTarget(link[2]) : null;
    if (link && target) out.push(<Link key={k++} label={link[1]} target={target} />);
    else if (link) out.push(link[1]);
    else if (tok.startsWith("`"))
      out.push(
        <code key={k++} className="rounded-[5px] bg-white/10 px-1 py-px font-mono text-[0.86em]">
          {tok.slice(1, -1)}
        </code>,
      );
    else out.push(<strong key={k++}>{tok.slice(2, -2)}</strong>);
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

/** One block; while an answer streams, finished blocks keep their text and
 * so skip re-rendering, and only the last one updates. */
const TextBlock = memo(function TextBlock({ text }: { text: string }) {
  return <p className="my-1.5 whitespace-pre-wrap">{inline(text)}</p>;
});
const Code = memo(CodeBlock);

export function Markdown({ text }: { text: string }) {
  return (
    <>
      {splitBlocks(text).map((b, i) =>
        b.kind === "code" ? (
          // biome-ignore lint/suspicious/noArrayIndexKey: blocks have no identity beyond order
          <Code key={i} lang={b.lang} text={b.text} />
        ) : (
          // biome-ignore lint/suspicious/noArrayIndexKey: blocks have no identity beyond order
          <TextBlock key={i} text={b.text} />
        ),
      )}
    </>
  );
}
