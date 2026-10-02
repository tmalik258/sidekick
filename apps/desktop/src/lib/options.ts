// Answers end with "OPTION: ..." lines; the island shows them as chips you
// pick with a click or Alt 1-3, and hides the raw lines.

const LINE = /^\s*[-*]?\s*option:\s*(.+?)\s*$/i;
const MAX = 3;

export function splitOptions(text: string, streaming = false): { body: string; options: string[] } {
  const lines = text.split("\n");
  const options: string[] = [];
  const body: string[] = [];
  for (const line of lines) {
    const m = line.match(LINE);
    if (m) {
      if (options.length < MAX) options.push(m[1].replace(/^["']|["']$/g, ""));
    } else {
      body.push(line);
    }
  }
  // While streaming, hide a last line that is still turning into "OPTION:".
  if (streaming) {
    const last = (body.at(-1) ?? "").trim().toLowerCase();
    if (last && "option:".startsWith(last.replace(/^[-*]\s*/, ""))) body.pop();
  }
  return { body: body.join("\n").trimEnd(), options };
}
