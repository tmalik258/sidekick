// A question the agent itself asked, with options to pick from. The agent
// sends them as structured data, so nothing is guessed from its wording.

export interface ChoiceOption {
  label: string;
  description: string;
}

export interface Choice {
  question: string;
  header: string;
  multiSelect: boolean;
  options: ChoiceOption[];
}

export interface OpenChoice {
  id: string;
  questions: Choice[];
}

/** Reads the agent's questions, dropping anything that is not usable. */
export function parseChoose(e: Record<string, unknown>): OpenChoice | null {
  if (!Array.isArray(e.questions)) return null;
  const questions: Choice[] = [];
  for (const raw of e.questions as Record<string, unknown>[]) {
    const options = (Array.isArray(raw.options) ? (raw.options as Record<string, unknown>[]) : [])
      .map((o) => ({ label: String(o.label ?? "").trim(), description: String(o.description ?? "").trim() }))
      .filter((o) => o.label);
    const question = String(raw.question ?? "").trim();
    if (!question || options.length === 0) continue;
    questions.push({
      question,
      header: String(raw.header ?? "").trim(),
      multiSelect: raw.multiSelect === true,
      options,
    });
  }
  return questions.length > 0 ? { id: String(e.id ?? ""), questions } : null;
}

/** The reply to send back for the picks (one list of labels per question). */
export function answerText(open: OpenChoice, picks: string[][]): string {
  return open.questions
    .map((q, i) => {
      const chosen = picks[i] ?? [];
      return `${q.header || q.question}: ${chosen.length > 0 ? chosen.join(", ") : "no preference"}`;
    })
    .join("\n");
}
