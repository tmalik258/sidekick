// Rules every Ask answer should follow, shared by the Ask eval
// (scripts/ask-eval.mjs). Run this file on its own to test the rules:
//   node scripts/ask-check.mjs

/** Lines from the system prompt's own examples: a model that copies them did not answer. */
const ECHOES = [/14\.2 of 16 GB/i, /\binvoice\.pdf\b/i];
const PREAMBLE = /^(sure|okay|ok|certainly|of course|great)\b|^(i'll|i will|let me|i am going to|i'm going to)\b/i;
const SELF_INTRO = /\b(i am|i'm) (sidekick|an ai|a language model|claude)\b/i;
const PLACEHOLDER = /%USERPROFILE%|<username>|\[your [a-z ]+\]|C:\\Users\\you\b/i;
const NUMBERS_ASKED = /\b(cpu|memory|ram|disk|storage|space|battery|size|how (much|many)|slow|full)\b/i;

/**
 * What is wrong with one answer.
 * @param {{question: string, body: string, options: string[], stats: {label: string, value: string}[], error?: string}} a
 * @returns {string[]} problems, empty when the answer looks right
 */
export function checkAnswer(a) {
  const p = [];
  const body = (a.body ?? "").trim();
  if (a.error) p.push(`failed: ${a.error}`);
  else if (!body) p.push(`no answer text${a.options.length || a.stats.length ? " (only chips)" : ""}`);
  const all = [body, ...a.options, ...a.stats.map((s) => `${s.label} ${s.value}`)].join("\n");
  for (const e of ECHOES) if (e.test(all)) p.push(`copied the prompt's example (${e.source})`);
  if (PREAMBLE.test(body)) p.push("starts with a preamble");
  if (SELF_INTRO.test(body)) p.push("introduces itself");
  if (/\u2014/.test(all)) p.push("uses an em dash");
  if (PLACEHOLDER.test(all)) p.push("has a placeholder path");
  if (/\*\*|^#{1,6} /m.test(body)) p.push("shows raw markdown");
  if (/^\s*(option|stat):/im.test(body)) p.push("OPTION or STAT line left in the text");
  if (a.options.length > 3) p.push(`${a.options.length} options (max 3)`);
  if (a.stats.length > 0 && !NUMBERS_ASKED.test(a.question))
    p.push("number chips on a question that is not about numbers");
  if (a.question.trim().split(/\s+/).length <= 3 && body.length > 600) p.push("long answer to a short question");
  return p;
}

/** A case's own expectations: `expect` must match, `forbid` must not. */
export function checkCase(c, a) {
  const p = [];
  const text = [a.body, ...a.options].join("\n");
  for (const e of c.expect ?? []) if (!new RegExp(e, "i").test(text)) p.push(`expected /${e}/`);
  for (const f of c.forbid ?? []) if (new RegExp(f, "i").test(text)) p.push(`should not say /${f}/`);
  return p;
}

if (import.meta.url === `file://${process.argv[1]}` || process.argv[1]?.endsWith("ask-check.mjs")) {
  const ok = (name, got, want) => {
    const pass = JSON.stringify(got) === JSON.stringify(want);
    console.log(`${pass ? "ok  " : "FAIL"}  ${name}${pass ? "" : `: got ${JSON.stringify(got)}`}`);
    if (!pass) process.exitCode = 1;
  };
  const a = (over) => ({ question: "how do I use claude here", body: "", options: [], stats: [], ...over });
  ok(
    "the copied-examples answer from the screenshot",
    checkAnswer(
      a({
        options: ["Open invoice.pdf", "Move invoice.pdf to Invoices", "Zip invoices"],
        stats: [{ label: "Memory", value: "14.2 of 16 GB" }],
      }),
    ),
    [
      "no answer text (only chips)",
      "copied the prompt's example (14\\.2 of 16 GB)",
      "copied the prompt's example (\\binvoice\\.pdf\\b)",
      "number chips on a question that is not about numbers",
    ],
  );
  ok(
    "a good answer",
    checkAnswer(a({ body: "1. Open the Claude panel with Ctrl L.\n2. Ask it about this file." })),
    [],
  );
  ok("a preamble", checkAnswer(a({ body: "Let me check that for you." })), ["starts with a preamble"]);
  ok(
    "stats on a memory question",
    checkAnswer(
      a({
        question: "why is my pc slow",
        body: "Chrome uses 6 GB.",
        stats: [{ label: "Memory", value: "13 of 16 GB" }],
      }),
    ),
    [],
  );
  ok("case expectations", checkCase({ expect: ["ctrl"], forbid: ["sorry"] }, a({ body: "Sorry, press Ctrl L" })), [
    "should not say /sorry/",
  ]);
}
