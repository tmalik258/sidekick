// Lift a prose plan into Session.plan / planDoc when the agent never sent a
// structured plan update (common after Codex Plan clarifying questions).

import type { PlanDoc, PlanItem } from "./agents";

const BARE_NUM = /^\s*\d+[.)]\s*$/;
const INLINE_NUM = /^\s*\d+[.)]\s+(.+?)\s*$/;
const READY = /\bplan\s+is\s+ready\b/i;
const SECTION_STOP = /^(assumptions?|notes?|the plan is ready)\b/i;
const TITLE = /^\s*(?:#{1,3}\s*)?(?:plan\s*[:\u2014-]\s*)(.+?)\s*$/i;

/** Full document parse: title, steps with bodies, assumptions. */
export function parsePlanDocument(text: string): PlanDoc | null {
  // Numbered steps only: a plain bullet list is not a plan.
  const items = parseNumbered(text);
  if (items.length < 2) return null;
  return toDoc(text, items);
}

/** Numbered / bulleted steps only (checklist fallback). */
export function parsePlanFromText(text: string): PlanItem[] {
  return parsePlanDocument(text)?.items ?? [];
}

function toDoc(text: string, items: PlanItem[]): PlanDoc {
  const title = findTitle(text) || "Plan";
  const assumptions = findAssumptions(text);
  const markdown = buildMarkdown(title, items, assumptions);
  return { title, items, assumptions, markdown };
}

function findTitle(text: string): string {
  for (const line of text.split("\n")) {
    const m = line.match(TITLE);
    if (m?.[1]) return m[1].replace(/\*\*/g, "").trim();
    if (/^plan\b/i.test(line.trim()) && line.trim().length < 80) {
      return line
        .replace(/\*\*/g, "")
        .replace(/^#+\s*/, "")
        .trim();
    }
  }
  return "";
}

function findAssumptions(text: string): string {
  const lines = text.split("\n");
  for (let i = 0; i < lines.length; i++) {
    const line = (lines[i] ?? "").trim();
    if (!/^assumptions?\b/i.test(line)) continue;
    const rest = line.replace(/^assumptions?\s*[:.\u2014-]?\s*/i, "").trim();
    const parts: string[] = rest ? [rest] : [];
    for (let j = i + 1; j < lines.length; j++) {
      const next = (lines[j] ?? "").trim();
      if (!next) break;
      if (READY.test(next) || BARE_NUM.test(lines[j] ?? "") || INLINE_NUM.test(lines[j] ?? "")) break;
      parts.push(next);
    }
    return parts.join(" ").replace(/\*\*/g, "").trim();
  }
  return "";
}

function buildMarkdown(title: string, items: PlanItem[], assumptions: string): string {
  const lines = [`# ${title}`, ""];
  items.forEach((p, i) => {
    lines.push(`${i + 1}. **${p.text}**`);
    if (p.detail) lines.push(`   ${p.detail}`);
    lines.push("");
  });
  if (assumptions) {
    lines.push(`**Assumptions:** ${assumptions}`, "");
  }
  return lines.join("\n").trim();
}

/** `1. title` on one line, or `1.` with the step on the following lines. */
function parseNumbered(text: string): PlanItem[] {
  const lines = text.split("\n");
  const items: PlanItem[] = [];
  const seen = new Set<string>();
  let i = 0;
  while (i < lines.length) {
    const line = lines[i] ?? "";
    if (BARE_NUM.test(line)) {
      const parts: string[] = [];
      i++;
      while (i < lines.length) {
        const next = (lines[i] ?? "").trim();
        if (BARE_NUM.test(lines[i] ?? "") || INLINE_NUM.test(lines[i] ?? "")) break;
        if (SECTION_STOP.test(next)) break;
        if (!next) {
          if (parts.length > 0) break;
          i++;
          continue;
        }
        parts.push(next);
        i++;
      }
      pushStep(items, seen, parts.join(" "));
      continue;
    }
    const inline = line.match(INLINE_NUM);
    if (inline) {
      // Gather continuation lines indented or plain until next number/section.
      const parts = [inline[1] ?? ""];
      i++;
      while (i < lines.length) {
        const raw = lines[i] ?? "";
        const next = raw.trim();
        if (BARE_NUM.test(raw) || INLINE_NUM.test(raw) || SECTION_STOP.test(next)) break;
        if (!next) break;
        if (/^#{1,3}\s/.test(next)) break;
        parts.push(next);
        i++;
      }
      pushStep(items, seen, parts.join(" "));
      continue;
    }
    i++;
  }
  return items.length >= 2 ? items : [];
}

function pushStep(items: PlanItem[], seen: Set<string>, raw: string, status: PlanItem["status"] = "pending") {
  const cleaned = clean(raw);
  if (!cleaned || cleaned.length > 2000) return;
  const text = stepTitle(cleaned);
  if (!isPlanStep(text)) return;
  const key = text.toLowerCase();
  if (seen.has(key)) return;
  seen.add(key);
  const detail = stepDetail(cleaned, text);
  items.push({ text, detail, status });
}

function clean(raw: string): string {
  return raw.replace(/\*\*/g, "").replace(/^`|`$/g, "").replace(/\s+/g, " ").trim();
}

/** First sentence as the step heading. */
function stepTitle(raw: string): string {
  const first = raw.match(/^(.+?[.!?])(?:\s|$)/);
  const t = (first?.[1] ?? raw).trim();
  return t.length > 120 ? `${t.slice(0, 117)}…` : t;
}

/** Remainder after the title, the full step write-up. */
function stepDetail(raw: string, title: string): string | undefined {
  let rest = raw.trim();
  if (rest.toLowerCase().startsWith(title.toLowerCase())) {
    rest = rest.slice(title.length).trim();
  } else {
    const first = raw.match(/^(.+?[.!?])(?:\s+)([\s\S]+)$/);
    rest = first?.[2]?.trim() ?? "";
  }
  return rest.length > 0 ? rest : undefined;
}

/** Drop clarifying questions and OPTION leftovers: those are not plan steps. */
function isPlanStep(raw: string): boolean {
  if (raw.length < 3) return false;
  if (/^option:/i.test(raw)) return false;
  if (/\?\s*$/.test(raw)) return false;
  if (/^[^:]{2,40}:\s+.+\bor\b.+/i.test(raw)) return false;
  return true;
}

/** Prefer structured plan; otherwise the richest parse across agent text. */
export function planFromSession(
  plan: PlanItem[],
  entries: { kind: string; text?: string }[],
): { items: PlanItem[]; doc: PlanDoc | null } {
  let best: PlanDoc | null = null;
  for (let i = entries.length - 1; i >= 0; i--) {
    const e = entries[i];
    if (e?.kind !== "text" || !e.text) continue;
    const doc = parsePlanDocument(e.text);
    if (doc && (!best || doc.items.length > best.items.length)) best = doc;
  }
  if (!best) return { items: plan, doc: null };
  const bad = looksLikeQuestionPlan(plan);
  const proseRicher = best.items.some((p) => !!p.detail) && !plan.some((p) => !!p.detail);
  if (plan.length === 0 || bad || proseRicher || best.items.length > plan.length) {
    return { items: best.items, doc: best };
  }
  return { items: plan, doc: best };
}

export function claimsPlanReady(text: string): boolean {
  return READY.test(text);
}

/** True when a cached checklist is clarifying questions, not implementation steps. */
export function looksLikeQuestionPlan(plan: PlanItem[]): boolean {
  if (plan.length === 0) return false;
  return plan.every((p) => /\?\s*$/.test(p.text) || /^[^:]{2,40}:\s+.+\bor\b/i.test(p.text));
}
