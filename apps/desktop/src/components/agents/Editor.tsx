"use client";

// The full editor in the dock: project tree, tabs, Monaco with change
// markers against HEAD, quick open (Ctrl P) and project search. Saves are
// refused when the file changed on disk since it was opened, and a file an
// agent rewrote asks before reloading.

import type * as MonacoNs from "monaco-editor";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "@/lib/bridge";
import { lineMarks } from "@/lib/linediff";
import { DUSKIE_THEME, languageOf, loadMonaco } from "@/lib/monaco";
import type { EditorHit, TreeEntry } from "@/lib/types";
import { Ic } from "../Icon";
import { Terminal } from "./Terminal";

interface Tab {
  path: string;
  stamp: string;
  head: string;
  dirty: boolean;
  /** An agent or another app changed the file on disk. */
  stale: boolean;
}

const POLL_MS = 2000;

export function Editor({
  root,
  initial,
  onClose,
}: {
  root: string;
  initial?: { path: string; line?: number; atChange?: boolean } | null;
  onClose: () => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const monaco = useRef<typeof MonacoNs | null>(null);
  const editor = useRef<MonacoNs.editor.IStandaloneCodeEditor | null>(null);
  const models = useRef(new Map<string, MonacoNs.editor.ITextModel>());
  const decos = useRef<MonacoNs.editor.IEditorDecorationsCollection | null>(null);
  const [tree, setTree] = useState<TreeEntry[]>([]);
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState("");
  const [quick, setQuick] = useState<string | null>(null);
  const [find, setFind] = useState<string | null>(null);
  const [hits, setHits] = useState<EditorHit[]>([]);
  const [term, setTerm] = useState(false);
  const tabsRef = useRef(tabs);
  tabsRef.current = tabs;

  useEffect(() => {
    api
      .editorTree(root)
      .then(setTree)
      .catch((e) => setError(String(e)));
  }, [root]);

  const mark = useCallback(() => {
    const ed = editor.current;
    const m = monaco.current;
    const tab = tabsRef.current.find((t) => t.path === active);
    if (!ed || !m || !tab) return;
    const marks = lineMarks(tab.head, ed.getValue());
    const range = (line: number) => new m.Range(line, 1, line, 1);
    const all = [
      ...marks.added.map((l) => ({
        range: range(l),
        options: { isWholeLine: true, linesDecorationsClassName: "ed-add" },
      })),
      ...marks.changed.map((l) => ({
        range: range(l),
        options: { isWholeLine: true, linesDecorationsClassName: "ed-chg" },
      })),
      ...marks.removed.map((l) => ({ range: range(Math.max(1, l)), options: { linesDecorationsClassName: "ed-del" } })),
    ];
    if (decos.current) decos.current.set(all);
    else decos.current = ed.createDecorationsCollection(all);
  }, [active]);

  // Monaco loads on first open only.
  useEffect(() => {
    let gone = false;
    loadMonaco()
      .then((m) => {
        if (gone || !host.current) return;
        monaco.current = m;
        const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
        m.editor.defineTheme("duskie", DUSKIE_THEME);
        editor.current = m.editor.create(host.current, {
          theme: "duskie",
          automaticLayout: true,
          minimap: { enabled: false },
          fontSize: 13,
          fontFamily: '"JetBrains Mono", "Cascadia Code", ui-monospace, Consolas, monospace',
          fontLigatures: true,
          lineHeight: 21,
          padding: { top: 12, bottom: 12 },
          renderLineHighlight: "line",
          lineNumbersMinChars: 3,
          glyphMargin: false,
          folding: false,
          overviewRulerLanes: 0,
          hideCursorInOverviewRuler: true,
          guides: { indentation: true },
          scrollbar: { verticalScrollbarSize: 6, horizontalScrollbarSize: 6, useShadows: false },
          cursorBlinking: reduced ? "solid" : "smooth",
          smoothScrolling: !reduced,
          cursorSmoothCaretAnimation: reduced ? "off" : "on",
          scrollBeyondLastLine: false,
        });
        setReady(true);
      })
      .catch((e) => setError(String(e)));
    const owned = models.current;
    return () => {
      gone = true;
      editor.current?.dispose();
      for (const model of owned.values()) model.dispose();
      owned.clear();
    };
  }, []);

  const open = useCallback(
    async (path: string, line?: number, atChange = false) => {
      const m = monaco.current;
      const ed = editor.current;
      if (!m || !ed) return;
      setError("");
      try {
        if (!models.current.has(path)) {
          const [file, head] = await Promise.all([
            api.editorRead(root, path),
            api.editorHead(root, path).catch(() => ""),
          ]);
          const model = m.editor.createModel(file.text, languageOf(path));
          model.onDidChangeContent(() =>
            setTabs((ts) => ts.map((t) => (t.path === path && !t.dirty ? { ...t, dirty: true } : t))),
          );
          models.current.set(path, model);
          setTabs((ts) => [...ts, { path, stamp: file.stamp, head, dirty: false, stale: false }]);
        }
        const model = models.current.get(path);
        if (model) ed.setModel(model);
        setActive(path);
        if (!line && atChange && model) {
          const head =
            tabsRef.current.find((t) => t.path === path)?.head ?? (await api.editorHead(root, path).catch(() => ""));
          const m2 = lineMarks(head, model.getValue());
          line = [...m2.changed, ...m2.added, ...m2.removed.map((l) => Math.max(1, l))].sort((a, b) => a - b)[0];
        }
        if (line) {
          ed.revealLineInCenter(line);
          ed.setPosition({ lineNumber: line, column: 1 });
        }
        ed.focus();
      } catch (e) {
        setError(String(e));
      }
    },
    [root],
  );

  useEffect(() => {
    if (ready && initial?.path) void open(initial.path, initial.line, initial.atChange);
  }, [ready, initial, open]);

  useEffect(() => {
    const ed = editor.current;
    if (!ed) return;
    mark();
    const sub = ed.onDidChangeModelContent(mark);
    return () => sub.dispose();
  }, [mark]);

  const close = useCallback((path: string) => {
    models.current.get(path)?.dispose();
    models.current.delete(path);
    setTabs((ts) => {
      const left = ts.filter((t) => t.path !== path);
      setActive((a) => (a === path ? (left[left.length - 1]?.path ?? null) : a));
      return left;
    });
  }, []);

  useEffect(() => {
    if (active && editor.current) {
      const model = models.current.get(active);
      if (model && editor.current.getModel() !== model) editor.current.setModel(model);
    }
  }, [active]);

  const save = useCallback(async () => {
    const tab = tabsRef.current.find((t) => t.path === active);
    const model = active ? models.current.get(active) : null;
    if (!tab || !model) return;
    try {
      const stamp = await api.editorWrite(root, tab.path, model.getValue(), tab.stamp);
      setTabs((ts) => ts.map((t) => (t.path === tab.path ? { ...t, stamp, dirty: false, stale: false } : t)));
    } catch (e) {
      setError(String(e));
      setTabs((ts) => ts.map((t) => (t.path === tab.path ? { ...t, stale: true } : t)));
    }
  }, [active, root]);

  const reload = useCallback(
    async (path: string) => {
      const file = await api.editorRead(root, path);
      models.current.get(path)?.setValue(file.text);
      setTabs((ts) => ts.map((t) => (t.path === path ? { ...t, stamp: file.stamp, dirty: false, stale: false } : t)));
    },
    [root],
  );

  // Notice files changed on disk by an agent or another app.
  useEffect(() => {
    const id = window.setInterval(() => {
      for (const t of tabsRef.current) {
        if (t.stale) continue;
        api
          .editorChanged(root, t.path, t.stamp)
          .then((changed) => {
            if (!changed) return;
            if (t.dirty) setTabs((ts) => ts.map((x) => (x.path === t.path ? { ...x, stale: true } : x)));
            else void reload(t.path);
          })
          .catch(() => {});
      }
    }, POLL_MS);
    return () => window.clearInterval(id);
  }, [root, reload]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      if (mod && e.key.toLowerCase() === "s") {
        e.preventDefault();
        void save();
      } else if (e.ctrlKey && e.key === "`") {
        e.preventDefault();
        setTerm((t) => !t);
      } else if (mod && e.key.toLowerCase() === "p") {
        e.preventDefault();
        setQuick("");
      } else if (mod && e.shiftKey && e.key.toLowerCase() === "f") {
        e.preventDefault();
        setFind("");
      } else if (e.key === "Escape") {
        if (quick !== null || find !== null) {
          setQuick(null);
          setFind(null);
        } else onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [save, onClose, quick, find]);

  useEffect(() => {
    if (!find) {
      setHits([]);
      return;
    }
    const t = window.setTimeout(
      () =>
        void api
          .editorSearch(root, find)
          .then(setHits)
          .catch(() => setHits([])),
      200,
    );
    return () => window.clearTimeout(t);
  }, [find, root]);

  const files = useMemo(() => tree.filter((e) => !e.dir), [tree]);
  const matches = useMemo(() => {
    const q = (quick ?? "").toLowerCase();
    return files.filter((f) => f.path.toLowerCase().includes(q)).slice(0, 30);
  }, [files, quick]);
  const current = tabs.find((t) => t.path === active);

  return (
    <section className="ed flex min-h-0 flex-1 flex-col" aria-label="Editor">
      <div className="gd-bar">
        <span className="gd-pick">
          <Ic name="folder" size={14} />
          <span className="truncate">{root.split(/[\\/]/).pop()}</span>
        </span>
        <button type="button" className="rd-btn ghost chip-s" onClick={() => setQuick("")}>
          Open file <kbd>Ctrl P</kbd>
        </button>
        <button type="button" className="rd-btn ghost chip-s" onClick={() => setFind("")}>
          Search <kbd>Ctrl Shift F</kbd>
        </button>
        <button type="button" className="rd-btn ghost chip-s" aria-pressed={term} onClick={() => setTerm((t) => !t)}>
          Terminal <kbd>Ctrl `</kbd>
        </button>
        <button
          type="button"
          className="rd-btn ghost chip-s ic-only ml-auto"
          aria-label="Close editor"
          onClick={onClose}
        >
          <Ic name="x" size={12} />
        </button>
      </div>
      <div className="ed-main min-h-0 flex-1">
        <nav className="ed-tree" aria-label="Files">
          <span className="ed-head">Files</span>
          {tree.map((e) => (
            <button
              key={e.path}
              type="button"
              className="ed-node"
              data-dir={e.dir}
              data-active={e.path === active}
              style={{ paddingLeft: 8 + (e.path.split("/").length - 1) * 12 }}
              disabled={e.dir}
              onClick={() => void open(e.path)}
            >
              <Ic name={e.dir ? "folder" : "file"} size={12} />
              <span className="truncate">{e.path.split("/").pop()}</span>
            </button>
          ))}
        </nav>
        <div className="ed-pane flex min-w-0 flex-1 flex-col">
          <nav className="ed-tabs" aria-label="Open files">
            {tabs.map((t) => (
              <span key={t.path} className="ed-tab" data-active={t.path === active}>
                <button
                  type="button"
                  aria-current={t.path === active ? "page" : undefined}
                  onClick={() => setActive(t.path)}
                >
                  <Ic name="file" size={11} />
                  {t.path.split("/").pop()}
                  {t.dirty && <span className="ed-dot" role="img" aria-label="unsaved" />}
                </button>
                <button type="button" aria-label={`Close ${t.path}`} onClick={() => close(t.path)}>
                  <Ic name="x" size={10} />
                </button>
              </span>
            ))}
          </nav>
          {current?.stale && (
            <div className="ed-note" role="alert">
              <span>This file changed on disk while you edited it.</span>
              <button type="button" className="rd-btn chip-s" onClick={() => void reload(current.path)}>
                Load theirs
              </button>
              <button
                type="button"
                className="rd-btn ghost chip-s"
                onClick={() =>
                  void api
                    .editorRead(root, current.path)
                    .then((f) =>
                      setTabs((ts) =>
                        ts.map((t) => (t.path === current.path ? { ...t, stamp: f.stamp, stale: false } : t)),
                      ),
                    )
                }
              >
                Keep mine
              </button>
            </div>
          )}
          {error && (
            <p className="ed-note" role="alert">
              {error}
            </p>
          )}
          {!ready && <p className="gd-empty shimmer-text">Opening the editor</p>}
          {ready && !active && <p className="gd-empty">Pick a file, or press Ctrl P.</p>}
          <div ref={host} className="ed-host min-h-0 flex-1" hidden={!active} />
          {term && <Terminal root={root} />}
        </div>
      </div>
      {quick !== null && (
        <div className="ed-pop" role="dialog" aria-label="Open file">
          <input
            // biome-ignore lint/a11y/noAutofocus: the picker opens for typing.
            autoFocus
            aria-label="File name"
            placeholder="Go to file"
            value={quick}
            onChange={(e) => setQuick(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && matches[0]) {
                // Keep the Enter from reaching the file that opens.
                e.preventDefault();
                e.stopPropagation();
                void open(matches[0].path);
                setQuick(null);
              }
            }}
          />
          {matches.map((f) => (
            <button
              key={f.path}
              type="button"
              onClick={() => {
                void open(f.path);
                setQuick(null);
              }}
            >
              {f.path}
            </button>
          ))}
        </div>
      )}
      {find !== null && (
        <div className="ed-pop" role="dialog" aria-label="Search the project">
          <input
            // biome-ignore lint/a11y/noAutofocus: the search opens for typing.
            autoFocus
            aria-label="Search text"
            placeholder="Search in files"
            value={find}
            onChange={(e) => setFind(e.target.value)}
          />
          {hits.map((h) => (
            <button
              key={`${h.path}:${h.line}`}
              type="button"
              onClick={() => {
                void open(h.path, h.line);
                setFind(null);
              }}
            >
              <span className="mono">
                {h.path}:{h.line}
              </span>{" "}
              {h.text}
            </button>
          ))}
        </div>
      )}
    </section>
  );
}
