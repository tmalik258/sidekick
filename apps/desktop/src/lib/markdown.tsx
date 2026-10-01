// A deliberately small Markdown renderer for chat answers: fenced code
// blocks (with a copy button), inline code, bold, and paragraphs. No HTML is
// ever injected; everything renders as React text.

import { type ReactNode, useState } from "react";

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

function inline(text: string): ReactNode[] {
  const out: ReactNode[] = [];
  const re = /(`[^`]+`|\*\*[^*]+\*\*)/g;
  let last = 0;
  let k = 0;
  for (const m of text.matchAll(re)) {
    const at = m.index ?? 0;
    if (at > last) out.push(text.slice(last, at));
    const tok = m[0];
    if (tok.startsWith("`"))
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
    <div className="group relative my-2 overflow-hidden rounded-xl border border-white/8 bg-black/35">
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

export function Markdown({ text }: { text: string }) {
  return (
    <>
      {splitBlocks(text).map((b, i) =>
        b.kind === "code" ? (
          // biome-ignore lint/suspicious/noArrayIndexKey: blocks have no identity beyond order
          <CodeBlock key={i} lang={b.lang} text={b.text} />
        ) : (
          // biome-ignore lint/suspicious/noArrayIndexKey: blocks have no identity beyond order
          <p key={i} className="my-1.5 whitespace-pre-wrap">
            {inline(b.text)}
          </p>
        ),
      )}
    </>
  );
}
