import { describe, expect, it } from "vitest";
import { answerText, parseChoose } from "./agent-questions";

const event = {
  id: "t4",
  questions: [
    {
      question: "Which database?",
      header: "DB",
      multiSelect: false,
      options: [
        { label: "SQLite", description: "local" },
        { label: "Postgres", description: "server" },
      ],
    },
  ],
};

describe("agent questions", () => {
  it("reads the questions and options", () => {
    const open = parseChoose(event);
    expect(open?.id).toBe("t4");
    expect(open?.questions[0]?.options.map((o) => o.label)).toEqual(["SQLite", "Postgres"]);
  });

  it("drops questions with no options and ignores junk", () => {
    expect(parseChoose({ id: "x", questions: [{ question: "?", options: [] }] })).toBeNull();
    expect(parseChoose({ id: "x" })).toBeNull();
  });

  it("writes the reply using the short header", () => {
    const open = parseChoose(event);
    if (!open) throw new Error("no question");
    expect(answerText(open, [["Postgres"]])).toBe("DB: Postgres");
    expect(answerText(open, [[]])).toBe("DB: no preference");
  });
});
