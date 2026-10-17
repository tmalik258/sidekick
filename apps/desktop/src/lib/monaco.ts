// Loads Monaco only when the editor opens, so the island never pays for it.
// Workers are bundled into public/monaco by scripts/build-monaco-workers.mjs
// and served as plain files, which keeps them working in the static export.

type Monaco = typeof import("monaco-editor");

let loading: Promise<Monaco> | null = null;

export function loadMonaco(): Promise<Monaco> {
  if (!loading) {
    (self as unknown as { MonacoEnvironment: unknown }).MonacoEnvironment = {
      getWorker(_id: string, label: string) {
        const name =
          label === "json"
            ? "json"
            : ["css", "scss", "less"].includes(label)
              ? "css"
              : label === "html"
                ? "html"
                : label === "typescript" || label === "javascript"
                  ? "ts"
                  : "editor";
        return new Worker(`/monaco/${name}.worker.js`);
      },
    };
    loading = import("monaco-editor");
  }
  return loading;
}

const LANGS: Record<string, string> = {
  ts: "typescript",
  tsx: "typescript",
  js: "javascript",
  jsx: "javascript",
  mjs: "javascript",
  json: "json",
  css: "css",
  scss: "scss",
  html: "html",
  md: "markdown",
  rs: "rust",
  py: "python",
  go: "go",
  java: "java",
  cs: "csharp",
  toml: "ini",
  yml: "yaml",
  yaml: "yaml",
  sh: "shell",
  ps1: "powershell",
  sql: "sql",
};

export function languageOf(path: string): string {
  return LANGS[path.split(".").pop()?.toLowerCase() ?? ""] ?? "plaintext";
}

/** Duskie's own colours: warm near-black, cream text, sage accents. */
export const DUSKIE_THEME = {
  base: "vs-dark" as const,
  inherit: true,
  rules: [
    { token: "comment", foreground: "6f7d71", fontStyle: "italic" },
    { token: "keyword", foreground: "e0a8c8" },
    { token: "string", foreground: "a8d18e" },
    { token: "number", foreground: "e8b56f" },
    { token: "regexp", foreground: "e8b56f" },
    { token: "type", foreground: "8ec5e8" },
    { token: "type.identifier", foreground: "8ec5e8" },
    { token: "identifier", foreground: "e8e5de" },
    { token: "delimiter", foreground: "9a978f" },
    { token: "tag", foreground: "e0a8c8" },
    { token: "attribute.name", foreground: "8ec5e8" },
  ],
  colors: {
    "editor.background": "#0e0e0d",
    "editor.foreground": "#e8e5de",
    "editorLineNumber.foreground": "#4a4f4b",
    "editorLineNumber.activeForeground": "#b9c9bb",
    "editor.lineHighlightBackground": "#f2f0eb08",
    "editor.lineHighlightBorder": "#00000000",
    "editor.selectionBackground": "#95a89a40",
    "editor.inactiveSelectionBackground": "#95a89a24",
    "editorCursor.foreground": "#b9c9bb",
    "editorIndentGuide.background1": "#f2f0eb0d",
    "editorIndentGuide.activeBackground1": "#f2f0eb26",
    "editorWidget.background": "#161615",
    "editorWidget.border": "#95a89a33",
    "editorSuggestWidget.background": "#161615",
    "editorSuggestWidget.selectedBackground": "#95a89a2a",
    "scrollbarSlider.background": "#00000000",
    "scrollbarSlider.hoverBackground": "#f2f0eb14",
    "scrollbarSlider.activeBackground": "#f2f0eb1f",
    "editorOverviewRuler.border": "#00000000",
    "editorBracketHighlight.foreground1": "#d9c9a0",
    "editorBracketHighlight.foreground2": "#c9b8e0",
    "editorBracketHighlight.foreground3": "#8ec5e8",
    "editorBracketMatch.background": "#95a89a26",
    "editorBracketMatch.border": "#00000000",
  },
};

/** The same palette for the terminal. */
export const DUSKIE_TERM = {
  background: "#0e0e0d",
  foreground: "#e8e5de",
  cursor: "#b9c9bb",
  cursorAccent: "#0e0e0d",
  selectionBackground: "#95a89a40",
  black: "#1a1a19",
  red: "#de8676",
  green: "#9cc79a",
  yellow: "#d9a55b",
  blue: "#a8c0cc",
  magenta: "#c9b8e0",
  cyan: "#95c2bb",
  white: "#e8e5de",
  brightBlack: "#5a5f5b",
  brightRed: "#eba293",
  brightGreen: "#b6d9b4",
  brightYellow: "#e8bf83",
  brightBlue: "#c2d4dc",
  brightMagenta: "#dccdee",
  brightCyan: "#b3d6d0",
  brightWhite: "#f2f0eb",
};
