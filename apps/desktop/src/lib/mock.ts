// Browser-only stand-in for the Rust core, used when the UI runs outside
// Tauri. It mimics the app loosely; the real rules live in Rust.

import type { DirectServer, LibrarySkill, MailStatus, SkillTarget } from "./types";
import {
  type ActivitySnapshot,
  type CloudId,
  CUES,
  type Cue,
  DEFAULT_SETTINGS,
  type MascotState,
  type Settings,
  type Suggestion,
  type UpdateProgress,
  type WelcomeSpeech,
} from "./types";

type Handler = (payload: unknown) => void;

const handlers = new Map<string, Set<Handler>>();
let settings: Settings = structuredClone(DEFAULT_SETTINGS);
let mailStatus: MailStatus = { state: "disconnected", configured: true, lastOk: null, error: null, provider: null };
let library: LibrarySkill[] = [];
const mockFiles: Record<string, string> = {
  "src/main.ts": 'console.log("hello");\nexport {};\n',
  "README.md": "# Demo\n",
};
if (typeof localStorage !== "undefined") {
  try {
    Object.assign(settings, JSON.parse(localStorage.getItem("sidekick.mock.savedFolders") ?? "{}"));
  } catch {}
}
/** Preview switches: `?nomodel` has no AI set up, `?offline` no internet. */
const previewFlag = (name: string) => typeof location !== "undefined" && new URLSearchParams(location.search).has(name);

// `?onboarded` in the preview URL skips the welcome; `?color=smoke` and
// `?theme=onyx` pick the island and mascot colours (for visual checks).
if (typeof location !== "undefined") {
  const q = new URLSearchParams(location.search);
  if (q.has("onboarded")) settings.onboarded = true;
  settings.islandColor = (q.get("color") as Settings["islandColor"] | null) ?? settings.islandColor;
  settings.theme = (q.get("theme") as Settings["theme"] | null) ?? settings.theme;
  if (q.get("voice") === "full") settings.voice.listeningStyle = "full";
  if (q.has("live")) settings.voice.live = true;
}
let mascot: MascotState = previewFlag("orphan") ? "working" : "idle";
let suggestion: Suggestion | null = null;
let epoch = 0;
let mockDnd = false;
let mockUpdateProgress: UpdateProgress | null = null;
let mockActivitySnapshot: ActivitySnapshot | null = null;
function activitySnapshot(): ActivitySnapshot {
  if (mockActivitySnapshot) return mockActivitySnapshot;
  const query = new URLSearchParams(typeof window === "undefined" ? "" : window.location.search);
  const mode = query.get("activity");
  const now = Date.now();
  const activities: ActivitySnapshot["activities"] = mode
    ? [
        {
          id: "preview-download",
          source: mode === "action" ? "action" : "browser",
          parentId: null,
          label: mode === "action" ? "Opening sidekick in Cursor" : "Downloading report.zip in Chrome",
          phase:
            mode === "paused"
              ? "awaitingUser"
              : mode === "failed"
                ? "failed"
                : mode === "action"
                  ? "running"
                  : "downloading",
          startedAt: now - 12_000,
          lastProgressAt: now - (mode === "stale" ? 120_000 : 0),
          transferred: mode === "action" ? null : 18_000_000,
          total: query.has("knownTotal") ? 36_000_000 : null,
          outcome: mode === "failed" ? "NETWORK_FAILED" : null,
          dismissed: false,
          revision: 1,
          destination: null,
        },
      ]
    : [];
  if (mode === "group")
    activities.push({ ...activities[0], id: "second-download", label: "Downloading photo.zip in Chrome" });
  mockActivitySnapshot = { revision: 1, activities };
  return mockActivitySnapshot;
}

async function mockInstallUpdate(): Promise<string> {
  if (mockUpdateProgress && mockUpdateProgress.phase !== "failed") return "An update is already in progress";
  const report = (phase: UpdateProgress["phase"], done = 0, error: string | null = null) => {
    mockUpdateProgress = { phase, done, total: phase === "preparing" ? null : 100 * 1_048_576, error };
    emit("update://progress", mockUpdateProgress);
  };
  const delay = () => new Promise((resolve) => setTimeout(resolve, 600));
  report("preparing");
  await delay();
  for (let i = 0; i <= 4; i++) {
    report("downloading", i * 25 * 1_048_576);
    await delay();
  }
  if (previewFlag("updateerror")) {
    report("failed", 100 * 1_048_576, "The download did not match its checksum; nothing was installed");
    throw new Error(mockUpdateProgress?.error ?? "Update failed");
  }
  report("verifying", 100 * 1_048_576);
  await delay();
  report("opening", 100 * 1_048_576);
  return "Installing Duskie_0.2.0_x64-setup.exe";
}

const CUE_BY_STATE: Partial<Record<MascotState, Cue>> = {
  idle: "settle",
  sleeping: "yawn",
  noticing: "chirp",
  suggesting: "pop",
  listening: "open",
  success: "ding",
  error: "boop",
};

function setServers(directServers: Settings["directServers"]) {
  settings = { ...settings, directServers };
  emit("settings://changed", settings);
}

function emit(event: string, payload: unknown) {
  for (const h of handlers.get(event) ?? []) h(payload);
}

function go(state: MascotState) {
  const previous = mascot;
  mascot = state;
  epoch += 1;
  emit("mascot://state", { previous, state, cue: CUE_BY_STATE[state] ?? null });
  if (state === "success") later(4000, () => go("idle"));
  if (state === "error") later(4000, () => go("idle"));
}

function later(ms: number, fn: () => void) {
  const at = epoch;
  setTimeout(() => {
    if (epoch === at) fn();
  }, ms);
}

const cloudKeys: Record<CloudId, boolean> = {
  gemini: previewFlag("gemini"),
  groq: false,
  openrouter: false,
  openai: false,
  xai: false,
};

function saveSettings(next: Settings): Settings {
  if (settings.codeFolders.some((p) => !next.codeFolders.includes(p))) {
    next = { ...next, savedProjects: next.savedProjects.filter((p) => next.codeFolders.includes(p)) };
  }
  localStorage.setItem(
    "sidekick.mock.savedFolders",
    JSON.stringify({ codeFolders: next.codeFolders, savedProjects: next.savedProjects }),
  );
  settings = next;
  emit("settings://changed", settings);
  emit("voice://state", commands.voice_status?.({}));
  return settings;
}

const commands: Record<string, (args: Record<string, unknown>) => unknown> = {
  app_info: () => ({ version: "0.1.0 (browser mock)", dbPath: "-", settingsPath: "-", eventCount: 0 }),
  system_look: () => ({ transparency: !previewFlag("solid"), batterySaver: previewFlag("saver") }),
  cloud_keys: () => ({ ...cloudKeys }),
  cloud_key_set: (a) => {
    const id = a.id as CloudId;
    if (!String(a.key).trim()) throw new Error("Paste the key first.");
    cloudKeys[id] = true;
    return saveSettings({ ...settings, ai: { ...settings.ai, [id]: { ...settings.ai[id], enabled: true } } });
  },
  cloud_key_clear: (a) => {
    const id = a.id as CloudId;
    cloudKeys[id] = false;
    return saveSettings({ ...settings, ai: { ...settings.ai, [id]: { ...settings.ai[id], enabled: false } } });
  },
  openrouter_models: () => [
    {
      id: "openrouter/auto",
      name: "Auto (best for each question)",
      free: false,
      input: 0,
      output: 0,
      context: 2000000,
      tools: true,
    },
    {
      id: "meta-llama/llama-3.3-70b-instruct:free",
      name: "Meta: Llama 3.3 70B Instruct (free)",
      free: true,
      input: 0,
      output: 0,
      context: 131072,
      tools: true,
    },
    {
      id: "qwen/qwen3-coder:free",
      name: "Qwen: Qwen3 Coder (free)",
      free: true,
      input: 0,
      output: 0,
      context: 262144,
      tools: true,
    },
    {
      id: "anthropic/claude-sonnet-4.5",
      name: "Anthropic: Claude Sonnet 4.5",
      free: false,
      input: 3,
      output: 15,
      context: 1000000,
      tools: true,
    },
    {
      id: "openai/gpt-5-mini",
      name: "OpenAI: GPT-5 Mini",
      free: false,
      input: 0.25,
      output: 2,
      context: 400000,
      tools: true,
    },
    {
      id: "deepseek/deepseek-chat-v3.1",
      name: "DeepSeek: V3.1",
      free: false,
      input: 0.2,
      output: 0.8,
      context: 163840,
      tools: true,
    },
  ],
  report_save: () => "C:\\Users\\you\\Downloads\\Duskie report 2026-10-08 0930.txt",
  diagnostics: () => "Duskie 0.1.0 (browser mock)\nWindows 11 Pro 24H2\nModels in order: local, claude_code",
  crash_pending: () => (previewFlag("crash") ? "2026-10-07T09:12:00Z panicked at src/voice.rs:120:9" : null),
  crash_dismiss: () => undefined,
  settings_get: () => settings,
  settings_set: (a) => saveSettings(a.settings as Settings),
  sensors_pause: (a) => {
    const minutes = a.minutes as number | null;
    go("sleeping");
    return saveSettings({
      ...settings,
      pause:
        minutes == null
          ? { kind: "indefinite" }
          : { kind: "until", until: new Date(Date.now() + minutes * 60_000).toISOString() },
    });
  },
  sensors_resume: () => {
    go("idle");
    return saveSettings({ ...settings, pause: { kind: "none" } });
  },
  mascot_get: () => mascot,
  island_set_hit_rect: () => undefined,
  island_ready: () => undefined,
  net_status: () => !previewFlag("offline"),
  update_status: () =>
    previewFlag("update")
      ? { version: "0.2.0", current: "0.1.0", url: "https://github.com/tmalik258/sidekick/releases" }
      : null,
  activity_open_start: () => undefined,
  activity_open_finish: () => undefined,
  activity_snapshot: activitySnapshot,
  activity_reconcile: () => {
    go("idle");
    return true;
  },
  activity_check: ({ id }) => {
    const snapshot = activitySnapshot();
    if (previewFlag("activityrecover")) {
      const activity = snapshot.activities.find((a) => a.id === id);
      if (activity) {
        activity.transferred = (activity.transferred ?? 0) + 1_000_000;
        activity.lastProgressAt = Date.now();
        activity.phase = "downloading";
        activity.dismissed = false;
        snapshot.revision++;
        emit("activity://changed", structuredClone(snapshot));
      }
    }
    return structuredClone(snapshot);
  },
  activity_dismiss: ({ id }) => {
    const snapshot = activitySnapshot();
    const activity = snapshot.activities.find((a) => a.id === id);
    if (activity) {
      activity.dismissed = true;
      snapshot.revision++;
      emit("activity://changed", structuredClone(snapshot));
    }
  },
  update_progress: () => mockUpdateProgress,
  // The preview pretends a release is out, so the update UI can be seen.
  update_check: () => {
    const update = { version: "0.2.0", current: "0.1.0", url: "https://github.com/tmalik258/sidekick/releases" };
    emit("update://available", update);
    return update;
  },
  update_install: mockInstallUpdate,
  net_check: () => !previewFlag("offline") && (typeof navigator === "undefined" || navigator.onLine),
  suggestion_current: () => suggestion,
  suggestion_choose: (a) => {
    const index = a.index as number;
    const updating = suggestion?.actions[index] === "install_update";
    suggestion = null;
    emit("suggestion://clear", a.id);
    if (updating) return mockInstallUpdate();
    go("working");
    later(1400, () => {
      emit("action://result", {
        ok: index !== 2,
        message: index === 2 ? "Simulated failure" : "Opened in Zen",
        path: null,
        auto: false,
      });
      go(index === 2 ? "error" : "success");
    });
  },
  suggestion_dismiss: (a) => {
    suggestion = null;
    emit("suggestion://clear", a.id);
    go("idle");
  },
  events_recent: () => [],
  term_open: () => {
    setTimeout(() => emit("term://data", { id: 1, data: "PS C:\\code\\app> ", done: false }), 50);
    return 1;
  },
  term_write: (a) => {
    const data = (a as { data: string }).data;
    emit("term://data", { id: 1, data: data === "\r" ? "\r\nPS C:\\code\\app> " : data, done: false });
  },
  term_resize: () => undefined,
  term_close: () => undefined,
  editor_tree: () => [
    { path: "src", dir: true },
    { path: "src/main.ts", dir: false },
    { path: "README.md", dir: false },
  ],
  editor_read: (a) => ({ text: mockFiles[(a as { path: string }).path] ?? "", stamp: "s1" }),
  editor_write: (a) => {
    const { path, text } = a as { path: string; text: string };
    mockFiles[path] = text;
    return "s2";
  },
  editor_changed: () => false,
  editor_head: (a) => ((a as { path: string }).path === "src/main.ts" ? 'console.log("hi");\n' : ""),
  editor_search: (a) =>
    Object.entries(mockFiles).flatMap(([path, text]) =>
      text
        .split("\n")
        .map((line, i) => ({ path, line: i + 1, text: line }))
        .filter((h) => h.text.toLowerCase().includes((a as { query: string }).query.toLowerCase())),
    ),
  library_review: () =>
    library.some((l) => l.name === "pdf")
      ? []
      : [
          {
            name: "pdf",
            description: "Read and fill PDF forms.",
            origin: "claude",
            dir: "~/.claude/skills/pdf",
            hash: "a1",
            files: ["SKILL.md"],
            hasScripts: false,
          },
        ],
  library_list: () => library,
  library_import: (a) => {
    for (const name of (a as { names: string[] }).names)
      if (name === "pdf" && !library.some((l) => l.name === name))
        library.push({
          name,
          description: "Read and fill PDF forms.",
          origin: "claude",
          hash: "a1",
          hasScripts: false,
          targets: { claude: true, codex: true, local: true },
          synced: { claude: "a1" },
          url: null,
          commit: null,
        });
    return library;
  },
  library_set_target: (a) => {
    const { name, target, on } = a as { name: string; target: SkillTarget; on: boolean };
    library = library.map((l) => (l.name === name ? { ...l, targets: { ...l.targets, [target]: on } } : l));
    return library;
  },
  library_remove: (a) => {
    library = library.filter((l) => l.name !== (a as { name: string }).name);
    return library;
  },
  library_fetch_url: (a) => {
    const url = (a as { url: string }).url;
    const had = library.find((l) => l.name === "pdf-tools");
    return {
      url,
      commit: had ? "def5678" : "abc1234",
      skills: [
        {
          name: "pdf-tools",
          description: "Fill and split PDFs.",
          files: 2,
          bytes: 2048,
          hasScripts: false,
          status: had ? "changed" : "new",
          diff: had ? ["changed SKILL.md"] : [],
        },
        {
          name: "deploy",
          description: "Ship a build.",
          files: 3,
          bytes: 4096,
          hasScripts: true,
          status: "new",
          diff: [],
        },
      ],
    };
  },
  library_install_fetched: (a) => {
    const { url, commit, names } = a as { url: string; commit: string; names: string[] };
    for (const name of names) {
      library = library.filter((l) => l.name !== name);
      library.push({
        name,
        description: "Added from a link.",
        origin: "url",
        hash: commit,
        hasScripts: name === "deploy",
        targets: { claude: true, codex: true, local: true },
        synced: {},
        url,
        commit,
      });
    }
    return library;
  },
  library_sync: () => ({ written: library.map((l) => l.name), conflicts: [], backups: [] }),
  skills_list: () => [
    {
      id: "dev.open-in-browser",
      name: "Open dev servers in a browser",
      description: "Mock skill for the browser preview.",
      event: "port.listening",
      enabled: true,
      auto: false,
      autoByDefault: false,
      mutedUntil: null,
    },
    {
      id: "files.screenshot",
      name: "Screenshots",
      description: "Copy text out of a new screenshot.",
      event: "file.created",
      enabled: true,
      auto: true,
      autoByDefault: false,
      mutedUntil: null,
    },
    {
      id: "files.download",
      name: "Finished downloads",
      description: "Open or move a file when it lands.",
      event: "file.created",
      enabled: true,
      auto: false,
      autoByDefault: false,
      mutedUntil: new Date(Date.now() + 3 * 86_400_000).toISOString(),
    },
  ],
  skill_set: () => settings,
  capabilities_get: () => ({ found: ["Chrome (mock)"], skillsDir: "-", skillErrors: [] }),
  choices_reset: () => 0,
  routines_today: () => [
    { kind: "app", key: "code.exe", label: "Visual Studio Code", target: "", browser: "", days: 5 },
    { kind: "site", key: "github.com", label: "github.com", target: "https://github.com/", browser: "Chrome", days: 4 },
    { kind: "app", key: "slack.exe", label: "Slack", target: "", browser: "", days: 3 },
  ],
  routines_forget: () => 12,
  routines_remove: () => 1,
  ai_open_link: (a) => `Opened ${String(a.target)}`,
  agent_tune: () => null,
  agent_set_mode: () => null,
  agent_control: () => null,
  agent_project: () => "C:/code/sidekick",
  agent_reviews: () => [],
  agent_review_decide: () => null,
  agent_finish: () => "Merged wt/store-tests back",
  cursor_chats: () => [
    {
      id: "c1",
      title: "Tidy checkout form",
      path: "C:/code/shop",
      project: "shop",
      updatedAt: Date.now() - 120_000,
      status: "working",
      lastReply: "Moved validation into one hook and kept the error text as it was.",
    },
  ],
  cursor_open: () => null,
  agents_status: () => ({
    claudeCode: true,
    codex: true,
    copilot: true,
    cursor: false,
    handoff: "Claude Code",
    list: [
      { id: "claude_code", name: "Claude Code", installed: true, signedIn: true, limited: false, fix: null },
      { id: "codex", name: "Codex", installed: true, signedIn: false, limited: false, fix: "codex login" },
      { id: "copilot", name: "GitHub Copilot", installed: true, signedIn: null, limited: false, fix: null },
      {
        id: "cursor",
        name: "Cursor",
        installed: false,
        signedIn: null,
        limited: false,
        fix: "Install the Cursor CLI from cursor.com/cli",
      },
      { id: "local", name: "Local", installed: true, signedIn: true, limited: false, fix: null, local: true },
    ],
  }),
  repos_overview: () => ({
    via: "gh",
    repos: [
      {
        name: "shop",
        path: "C:/code/shop",
        slug: "acme/shop",
        branch: "fix-login",
        ahead: 1,
        behind: 3,
        changed: 2,
        ci: "fail",
        reviews: 1,
        prs: [
          {
            number: 42,
            title: "Fix login redirect",
            url: "https://github.com/acme/shop/pull/42",
            branch: "fix-login",
            mine: true,
            ci: "fail",
            failing: ["test (windows)", "https://github.com/acme/shop/actions/runs/1"],
            reviewRequested: false,
          },
          {
            number: 40,
            title: "Cart totals in cents",
            url: "https://github.com/acme/shop/pull/40",
            branch: "cents",
            mine: false,
            ci: "pass",
            failing: null,
            reviewRequested: true,
          },
        ],
      },
      {
        name: "site",
        path: "C:/code/site",
        slug: "acme/site",
        branch: "main",
        ahead: 0,
        behind: 0,
        changed: 0,
        ci: "none",
        reviews: 0,
        prs: [],
      },
    ],
  }),
  repo_pull: () => "Pulled 3 commits",
  repo_open: () => null,
  merge_state: () => ({
    branch: "feat/finish",
    main: "main",
    stashed: true,
    files: [
      {
        file: "apps/desktop/src/lib/store.ts",
        spots: 2,
        mine: 'const last = turns.findLast((t) => t.role === "user");\nconst localOnly = last?.localOnly ?? settings.ai.localOnly;',
        theirs: "const meta = await api.chatMeta(id);\nconst localOnly = meta.localOnly;",
      },
      {
        file: "crates/core/src/settings.rs",
        spots: 1,
        mine: "pub local_only: bool,",
        theirs: "pub git_watch: GitWatch,",
      },
      {
        file: "apps/desktop/src/components/settings/AiTab.tsx",
        spots: 3,
        mine: 'title="Privacy"\nhint="Chats on this PC only use Ollama."',
        theirs: 'title="Repos"\nhint="Where clones go and how often to check."',
      },
    ],
  }),
  merge_keep: () => null,
  merge_end: (a) => ({
    ok: true,
    message: a.finish ? "Merged main, your changes are back" : "Back exactly as before the update",
    path: null,
    auto: false,
    undoId: a.finish ? 7 : null,
  }),
  codex_add_notify: () => "C:\\Users\\you\\.codex\\config.toml.sidekick-backup-20261002",
  codex_add_mcp: () => null,
  guide_keys: () => null,
  dnd_get: () => mockDnd,
  dnd_set: (a) => {
    mockDnd = Boolean(a.on);
    return `Done. Do Not Disturb is ${mockDnd ? "on" : "off"}`;
  },
  recipe_save: (args) => {
    const r = args.recipe as Settings["recipes"][number];
    const recipe = { ...r, id: r.id || `r${Date.now()}`, name: r.name || r.prompt.split(" ").slice(0, 6).join(" ") };
    settings = { ...settings, recipes: [...settings.recipes.filter((x) => x.id !== recipe.id), recipe] };
    emit("settings://changed", settings);
    return `Saved recipe ${recipe.name}`;
  },
  recipe_delete: (args) => {
    settings = { ...settings, recipes: settings.recipes.filter((x) => x.id !== args.id) };
    emit("settings://changed", settings);
    return "Deleted";
  },
  recipe_run: () => "Running",
  know_how_clear: () => null,
  attention_lessons: () =>
    Object.entries(settings.notifications.lessons.entries).map(([key, l]) => ({
      key,
      shift: Math.max(-45, Math.min(20, Math.round(l.acted * 6 - l.showLess * 14))),
      updated: l.updated,
    })),
  attention_acted: () => null,
  attention_show_less: () => null,
  attention_reset: (args) => {
    const entries = { ...settings.notifications.lessons.entries };
    const scope = String(args.scope);
    for (const k of Object.keys(entries)) {
      if (scope === "all" || k === scope || (scope.startsWith("group:") && k.startsWith(scope.slice(6))))
        delete entries[k];
    }
    settings = { ...settings, notifications: { ...settings.notifications, lessons: { entries } } };
    return null;
  },
  git_status: () => [
    { path: "src/app.ts", kind: "M", staged: true, unstaged: false },
    { path: "src/util.ts", kind: "M", staged: false, unstaged: true },
    { path: "README.md", kind: "?", staged: false, unstaged: true },
  ],
  git_stage: () => "Staged",
  git_unstage: () => "Unstaged",
  git_commit: () => "Committed",
  git_write_message: () => "Tidy the util helpers",
  git_amend: () => "Amended the last commit",
  git_last_commit_pushed: () => false,
  git_log: () => [
    { hash: "b56ca02", subject: "Proposals card with Skip and Approve per row", author: "You", when: "2 hours ago" },
    { hash: "509e408", subject: "Agent card on hover", author: "You", when: "3 hours ago" },
    { hash: "a5e488a", subject: "Meeting card on hover", author: "Ayesha", when: "yesterday" },
  ],
  git_stashes: () => mockStashes,
  git_stash_apply: (a) => {
    if (a.pop) mockStashes = mockStashes.filter((x) => x.name !== a.name);
    return a.pop ? "Popped the stash" : "Applied the stash. A copy stays in Stashes.";
  },
  git_stash_drop: (a) => {
    mockStashes = mockStashes.filter((x) => x.name !== a.name);
    return "Dropped the stash";
  },
  island_width: () => undefined,
  gpu_memory: () => (new URLSearchParams(location.search).has("cpu") ? 0 : 8192),
  limits_paused: () =>
    new URLSearchParams(location.search).has("limited") ? [{ name: "Anthropic", pausedSecs: 42 }] : [],
  servers_save: (a) => {
    const given = a.server as DirectServer;
    const server: DirectServer = {
      ...given,
      id: given.id || `srv${Date.now()}`,
      confirmed: false,
      writesAllowed: [],
      envNames: Object.keys((a.env as Record<string, string>) ?? {}),
    };
    setServers([...settings.directServers.filter((s) => s.id !== server.id), server]);
    return server.id;
  },
  servers_command: (a) => {
    const s = settings.directServers.find((x) => x.id === a.id);
    if (!s) throw new Error("That server is gone.");
    return s.kind === "stdio" ? [s.command, ...s.args].join(" ") : s.url;
  },
  servers_confirm: (a) =>
    setServers(settings.directServers.map((s) => (s.id === a.id ? { ...s, confirmed: true } : s))),
  servers_test: (a) => {
    const s = settings.directServers.find((x) => x.id === a.id);
    if (!s?.confirmed) throw new Error("Confirm the command first.");
    return [
      { name: "list_issues", description: "List issues in a repo", writes: false, allowed: false },
      {
        name: "create_issue",
        description: "Open a new issue",
        writes: true,
        allowed: s.writesAllowed.includes("create_issue"),
      },
    ];
  },
  servers_allow_write: (a) =>
    setServers(
      settings.directServers.map((s) =>
        s.id === a.id
          ? {
              ...s,
              writesAllowed: [...s.writesAllowed.filter((t) => t !== a.tool), ...(a.allow ? [String(a.tool)] : [])],
            }
          : s,
      ),
    ),
  servers_sign_in: (a) =>
    setServers(
      settings.directServers.map((s) =>
        s.id === a.id ? { ...s, envNames: [...s.envNames.filter((n) => n !== "Authorization"), "Authorization"] } : s,
      ),
    ),
  servers_remove: (a) => setServers(settings.directServers.filter((s) => s.id !== a.id)),
  git_message_diff: () => ({ text: "diff --git a/src/app.ts b/src/app.ts", covered: 1, total: 1 }),
  mail_status: () => mailStatus,
  mail_connect: () => {
    mailStatus = {
      state: "connected",
      configured: true,
      lastOk: Math.floor(Date.now() / 1000),
      error: null,
      provider: "gmail",
    };
    return mailStatus;
  },
  mail_connect_imap: () => {
    mailStatus = {
      state: "connected",
      configured: true,
      lastOk: Math.floor(Date.now() / 1000),
      error: null,
      provider: "imap",
    };
    return mailStatus;
  },
  mail_disconnect: () => {
    mailStatus = { state: "disconnected", configured: true, lastOk: null, error: null, provider: null };
    return mailStatus;
  },
  attention_set_profile: (args) => {
    settings = { ...settings, notifications: { ...settings.notifications, profile: args.profile as never } };
    return null;
  },
  notifications_set_level: (args) => {
    const from = String(args.from);
    const level = String(args.level);
    const apps = { ...settings.notifications.apps, [from]: level as never };
    if (level === "auto") delete apps[from];
    settings = { ...settings, notifications: { ...settings.notifications, apps } };
    return `${from}: ${level}`;
  },
  notifications_status: () => {
    const ago = (m: number) => new Date(Date.now() - m * 60_000).toISOString();
    const items = settings.notifications.enabled
      ? [
          {
            id: 9,
            app: "Chrome",
            title: "Google",
            body: "G-482913 is your verification code",
            ts: ago(2),
            level: "now",
            why: "login code",
            code: "482913",
          },
          {
            id: 8,
            app: "WhatsApp",
            title: "Ali Khan",
            body: "Can you send the invoice today?",
            ts: ago(6),
            level: "soon",
            why: "message",
          },
          {
            id: 7,
            app: "Slack",
            title: "#dev",
            body: "Sara mentioned you: can you review the PR?",
            ts: ago(14),
            level: "soon",
            why: "mention",
          },
          {
            id: 6,
            app: "Chrome",
            title: "Daraz",
            body: "Flash sale: 50% off today only",
            ts: ago(30),
            level: "digest",
            why: "promotion",
          },
          {
            id: 5,
            app: "Windows Update",
            title: "Updates are ready",
            body: "Restart to finish installing",
            ts: ago(55),
            level: "digest",
            why: "update",
          },
        ]
      : [];
    const apps: { app: string; level: string | null; count: number }[] = [];
    for (const it of items) {
      const found = apps.find((a) => a.app === it.app);
      if (found) found.count += 1;
      else apps.push({ app: it.app, level: settings.notifications.apps[it.app] ?? null, count: 1 });
    }
    return { readable: settings.notifications.enabled, error: null, items, apps };
  },
  actions_recent: () => [],
  reveal_path: () => undefined,
  open_settings: () => commands.ask_open?.({ view: "settings" }),
  debug_set_state: (a) => go(a.state as MascotState),
  debug_emit_event: () => {
    go("noticing");
    later(1200, () => go("idle"));
  },
};

// A fake streamed answer, so Ask mode can be developed in a browser.
function mockChat(a: Record<string, unknown>) {
  const id = a.id as string;
  const messages = a.messages as { content: string }[];
  const last = messages[messages.length - 1]?.content ?? "";
  const formatted = /plan|compare|steps/i.test(last);
  const answer: string = formatted
    ? "Here is the plan for **Friday**:\n\n1. Send the invoice to **Ali** by 10:00\n2. Review the *Upwork* proposal\n3. Book the dentist\n\nCosts so far:\n\n| Item | Amount |\n| --- | --- |\n| Hosting | **$24** |\n| Domain | $12 |\n\n- Invoice is in [invoice-sept.pdf](C:\\Users\\you\\Downloads\\invoice-sept.pdf)\n- Notes are in [Projects](C:\\Users\\you\\Projects\\)\nOPTION: Draft the email to Ali"
    : /sidekick/i.test(last)
      ? "Found it: [Duskie's app folder](/C:/Users/you/AppData/Roaming/app.duskie.desktop), changed today. Its settings are in [settings.json](C:\\Users\\you\\AppData\\Roaming\\app.duskie.desktop\\settings.json).\nOPTION: Open the folder\nOPTION: Show settings.json"
      : `Found invoice-sept.pdf in Downloads, from Ali on Tuesday (preview, you asked "${last}").\nOPTION: Open invoice-sept.pdf\nOPTION: Show the folder\nOPTION: Find other invoices`;
  // Formatted answers arrive in bursts, like Claude Code's whole sentences.
  const words = formatted ? (answer.match(/[\s\S]{1,90}/g) ?? [answer]) : answer.split(/(?<=\s)/);
  let i = 0;
  const tick = () => {
    if (i < words.length) {
      emit("ai://delta", { id, text: words[i++] });
      setTimeout(tick, formatted ? 450 : 28);
    } else {
      const change = /\b(move|send|create|update|delete)\b/i.test(last);
      // A spoken question's answer is read aloud: the mascot talks for a bit.
      if ((a.attach as { speak?: boolean } | undefined)?.speak) {
        emit("voice://speaking", true);
        setTimeout(() => emit("voice://speaking", false), 3000);
      }
      emit("ai://done", {
        id,
        provider: "local",
        error: null,
        handoff: change ? "needs a change (JIRA_TRANSITION_ISSUE)" : null,
      });
    }
  };
  if (/meetings/i.test(last)) {
    // A local model that is not running, for the failure card.
    later(900, () =>
      emit("ai://done", {
        id,
        provider: null,
        error: "error sending request for url (http://127.0.0.1:11434/v1/chat/completions)",
        handoff: null,
      }),
    );
    return;
  }
  emit("ai://tool", { id, name: "search", label: `Searching your PC for \u201c${last.slice(0, 30)}\u201d` });
  // A task with several steps shows them one by one.
  if (/ and /i.test(last)) {
    const steps = ["notifications", "browser", "browser"];
    for (const [n, name] of steps.entries()) {
      later(250 * (n + 1), () => emit("ai://tool", { id, name }));
    }
  }
  if (/\btidy\b/i.test(last)) {
    // Separate actions, each needing its own yes.
    const asks = [
      "Archive 14 newsletters in Gmail",
      "Move 3 invoices to Documents",
      "Reply to Sara: timeline is Friday",
      "Delete 2 old build folders (1.2 GB)",
    ];
    for (const [n, label] of asks.entries())
      later(1000 + n * 60, () => emit("ai://proposal", { chatId: id, id: `p-${id}-${n}`, label, undoable: true }));
  } else if (/\bzip\b/i.test(last)) {
    // A task with several actions comes back as a plan.
    const plan = [
      "Create Downloads\\Invoices October",
      "Move 7 invoice PDFs into it",
      "Zip it as Invoices-October.zip",
    ];
    for (const [n, label] of plan.entries()) {
      later(1000 + n * 60, () =>
        emit("ai://proposal", { chatId: id, id: `p-${id}-${n}`, label, step: true, undoable: n % 2 === 0 }),
      );
    }
  } else {
    later(1200, () => emit("ai://proposal", { chatId: id, id: `p-${id}`, label: "Move invoice-sept.pdf to Invoices" }));
  }
  setTimeout(tick, 900);
}

commands.ai_chat = (a) => mockChat(a);
commands.ai_run_proposal = () => ({
  ok: true,
  message: "Moved invoice-sept.pdf to Invoices",
  undoId: 1,
  path: "C:/Users/you/Documents/Invoices/invoice-sept.pdf",
});
commands.ai_cancel = () => undefined;
commands.ai_release = () => undefined;
// Kept for the speed check (scripts/speed.mjs).
commands.timing_record = (a) => {
  const w = window as unknown as { __timings?: { name: string; ms: number }[] };
  w.__timings = [...(w.__timings ?? []), { name: a.name as string, ms: a.ms as number }];
};
commands.timings_recent = () => [];

// Agent sessions: a short simulated run with a plan, steps, one question
// and two changed files to review.
let mockChanges = [
  {
    path: "src/api/client.ts",
    status: "modified",
    added: 9,
    removed: 2,
    hunks: [
      {
        header: "@@ -12,6 +12,13 @@ export async function get(url: string) {",
        lines: [
          " export async function get(url: string) {",
          "-  const res = await fetch(url);",
          "-  return res.json();",
          "+  for (let attempt = 0; attempt < 3; attempt++) {",
          "+    const res = await fetch(url);",
          "+    if (res.ok) return res.json();",
          "+    await sleep(250 * 2 ** attempt);",
          "+  }",
          "+  throw new Error('GET failed: ' + url);",
          " }",
        ],
      },
    ],
  },
  {
    path: "src/api/client.test.ts",
    status: "added",
    added: 3,
    removed: 0,
    hunks: [
      {
        header: "@@ -0,0 +1,3 @@",
        lines: ["+test('retries', async () => {", "+  expect(await get('/flaky')).toEqual({ ok: true });", "+});"],
      },
    ],
  },
];
commands.agent_start = (a) => {
  const id = `s${Date.now()}`;
  const ev = (kind: string, data: Record<string, unknown> = {}) =>
    emit("agent://event", { session: id, kind, ...data });
  const at = (ms: number, f: () => void) => setTimeout(f, ms);
  at(300, () =>
    ev("plan", {
      items: [
        { text: "Read the API client", status: "in_progress" },
        { text: "Add retries with backoff", status: "pending" },
        { text: "Run the tests", status: "pending" },
      ],
    }),
  );
  at(600, () => ev("step", { id: "t1", tool: "Read", label: "Read client.ts", detail: "", state: "running" }));
  at(1100, () => ev("step", { id: "t1", state: "done" }));
  at(1300, () => ev("text", { text: "The client calls fetch once. I'll add three tries with a growing wait." }));
  at(1700, () => ev("step", { id: "t2", tool: "Edit", label: "Edit client.ts", detail: "", state: "running" }));
  at(2200, () => {
    ev("step", { id: "t2", state: "done" });
    ev("plan", {
      items: [
        { text: "Read the API client", status: "completed" },
        { text: "Add retries with backoff", status: "completed" },
        { text: "Run the tests", status: "in_progress" },
      ],
    });
  });
  at(2500, () => ev("ask", { question: `q${id}`, label: "Run a command", detail: "pnpm test src/api" }));
  pendingAgentAnswer = () => {
    ev("answered", { question: `q${id}` });
    ev("step", { id: "t3", tool: "Bash", label: "Run a command", detail: "pnpm test src/api", state: "running" });
    at(900, () => {
      ev("step", {
        id: "t3",
        state: "done",
        output: "> vitest run src/api\n\n ✓ client.test.ts (1)\n\n Test Files  1 passed (1)\n      Tests  1 passed (1)",
      });
      ev("usage", { used: 46000, window: 200000 });
      ev("text", { text: "\n\nDone. Requests retry up to three times, and a new test covers it." });
      ev("turn", { error: null });
    });
  };
  return {
    id,
    agent: a.agent === "codex" ? "Codex" : "Claude Code",
    project:
      String(a.path ?? "")
        .split(/[\\/]/)
        .pop() ?? "project",
    branch: "main",
    reviewable: true,
  };
};
let pendingAgentAnswer: (() => void) | null = null;
commands.agent_handoff = () =>
  (commands.agent_start as (a: Record<string, unknown>) => unknown)({ agent: "claude_code", path: "handoff" });
commands.agent_answer = () => {
  pendingAgentAnswer?.();
  pendingAgentAnswer = null;
};
commands.agent_send = () => undefined;
commands.agent_set_mode = () => undefined;
commands.agent_control = () => undefined;
commands.agent_project = () => "C:/code/sidekick";
commands.agent_reviews = () => [];
commands.agent_review_decide = () => undefined;
commands.agent_stop = () => undefined;
commands.agent_close = () => undefined;
commands.agent_terminal = () => undefined;
commands.agent_resume = () => undefined;
commands.agent_memory = () => 312 * 1024 * 1024;
commands.agent_open_editor = () => undefined;
commands.agent_rewind_preview = () => 1;
commands.agent_rewind = () => 1;
commands.agent_files = (a) =>
  [
    "apps/desktop/src-tauri/src/island.rs",
    "apps/desktop/src-tauri/src/island_tests.rs",
    "apps/desktop/src/components/Island.tsx",
  ].filter((f) => f.toLowerCase().includes(String(a.query ?? "").toLowerCase()));
commands.agent_commands = () => [
  { name: "/compact", description: "Summarize the chat to free context", group: "Session" },
  { name: "/clear", description: "Start fresh in the same project", group: "Session" },
  { name: "/rewind", description: "Go back to an earlier message, code included", group: "Session" },
  { name: "/release", description: "Cut a release (.claude/commands/release.md)", group: "This project" },
];
commands.agent_changes = () => structuredClone(mockChanges);
commands.agent_undo = (a) => {
  mockChanges = a.path ? mockChanges.filter((f) => f.path !== a.path) : [];
};
let welcomeDeferred = false;
commands.ask_close = () => {
  if (!settings.onboarded && !welcomeDeferred) {
    commands.ask_open?.({ view: "welcome" });
    return;
  }
  emit("ask://close", { reason: "close" });
};
commands.ask_open = (a) => {
  welcomeDeferred = false;
  emit("ask://open", {
    sentAt: Date.now(),
    context: {
      app: "Visual Studio Code",
      title: "Island.tsx - sidekick",
      clipboardKind: "stack_trace",
      clipboardPreview: "Error: listen EADDRINUSE: address already in use :::3000",
      clipboardSecret: false,
    },
    prompt: (a.prompt as string | null) ?? null,
    ask: Boolean(a.ask),
    clipboard: false,
    page: null,
    view: settings.onboarded ? ((a.view as string | undefined) ?? "ask") : "welcome",
    tool: (a.tool as string | undefined) ?? null,
  });
  if (!settings.onboarded && !welcomeSpoken) {
    welcomeSpoken = true;
    speakWelcome(settings.welcomeStep);
  }
};
let welcomeSpoken = false;
commands.ask_ensure_welcome = () => {
  if (settings.onboarded || welcomeDeferred) return;
  commands.ask_open?.({ view: "welcome" });
};
commands.ask_defer_welcome = () => {
  if (settings.onboarded) {
    emit("ask://close", { reason: "close" });
    return;
  }
  welcomeDeferred = true;
  emit("ask://close", { reason: "defer" });
  // Preview a permission arriving while onboarding is hidden.
  if (previewFlag("approval")) {
    setTimeout(() => {
      suggestion = {
        id: "preview-approval",
        skillId: "notify.permission",
        title: "Allow this connection?",
        detail: "The connection needs your approval.",
        options: ["Accept", "Decline"],
        actions: ["approve", "reject"],
      };
      emit("suggestion://new", suggestion);
      go("suggesting");
    }, 500);
  }
};
commands.ask_resume_welcome = () => {
  if (settings.onboarded) return;
  welcomeDeferred = false;
  commands.ask_open?.({ view: "welcome" });
};
commands.skill_install = () => "Screenshots";
const voiceStatus = () => ({
  models: [
    { id: "wake", label: "Wake word", size: 17_626_723, installed: settings.voice.enabled },
    { id: "speech", label: "Speech to text", size: 57_267_600, installed: settings.voice.enabled },
    { id: "voice", label: "Supertonic voice", size: 128_774_318, installed: settings.voice.enabled },
  ],
  missingBytes: settings.voice.enabled ? 0 : 203_669_641,
  downloading: false,
  listening: settings.voice.enabled,
  error: null,
  voices: [
    { id: "f5", label: "Female 5 (warm)" },
    { id: "f1", label: "Female 1 (clear)" },
    { id: "m2", label: "Male 2 (deep, lively)" },
  ],
});
commands.voice_status = voiceStatus;
commands.clipboard_history = () => [
  { text: "npm run dev -- --port 3001", ts: new Date().toISOString() },
  { text: "https://github.com/tmalik258/sidekick/pull/5", ts: new Date().toISOString() },
  { text: "#0ea5e9", ts: new Date().toISOString() },
];
commands.clipboard_copy = () => undefined;
commands.search_clear = () => 0;
commands.setup_status = () => {
  const w = (id: string) => `winget install -e --id ${id} --accept-source-agreements`;
  const item = (
    id: string,
    group: "ai" | "connect" | "tools",
    title: string,
    why: string,
    done: boolean,
    status: string,
    extra: {
      command?: string;
      runnable?: boolean;
      action?: string;
      opensApp?: boolean;
      opensTerminal?: boolean;
      tab?: string;
      recommended?: boolean;
    } = {},
  ) => ({
    id,
    group,
    title,
    why,
    done,
    status,
    command: extra.command ?? null,
    runnable: extra.runnable ?? false,
    action: extra.action ?? "Install",
    opensApp: extra.opensApp ?? false,
    opensTerminal: extra.opensTerminal ?? false,
    tab: extra.tab ?? null,
    recommended: extra.recommended ?? false,
  });
  return {
    items: [
      item("claude_code", "ai", "Claude Code", "Chat, drafts and skills with your Claude plan.", true, "Installed", {
        recommended: true,
      }),
      item("ollama", "ai", "Ollama", "Free local AI on this PC, and search by meaning.", true, "Running", {
        recommended: true,
      }),
      item("ollama_embed", "ai", "Search model", "Finds things by meaning. About 270 MB.", false, "Not downloaded", {
        command: "ollama pull nomic-embed-text",
        runnable: true,
        action: "Download",
        recommended: true,
      }),
      item(
        "anthropic",
        "ai",
        "Anthropic API key",
        "Pay as you go. Opens PowerShell with the command ready to paste.",
        false,
        "",
        {
          command: 'setx ANTHROPIC_API_KEY "your-key"',
          runnable: true,
          action: "Open terminal",
          opensTerminal: true,
        },
      ),
      item(
        "claude_hooks",
        "connect",
        "Claude Code hooks",
        "Allow or deny requests from the island.",
        false,
        "Not added",
        {
          command: '{\n  "hooks": {}\n}',
          tab: "ai",
          recommended: true,
        },
      ),
      item(
        "browser",
        "connect",
        "Browser extension",
        "Page help, tabs and sessions on the island.",
        false,
        "Not paired",
        {
          tab: "browser",
          recommended: true,
        },
      ),
      item(
        "composio",
        "connect",
        "Composio",
        "One sign-in for your calendar, mail, Slack and more.",
        Boolean(settings.composio.account),
        settings.composio.account ? "Connected" : "Not connected",
        { tab: "connections", recommended: true },
      ),
      item("gh", "tools", "GitHub CLI", "Open PRs in the morning brief.", false, "Not installed", {
        command: w("GitHub.cli"),
        runnable: true,
        recommended: true,
      }),
      item("git", "tools", "Git", "Repo status and unsaved work.", true, "Installed", { recommended: true }),
      item("tesseract", "tools", "Tesseract", "Copy text out of screenshots.", false, "Not installed", {
        command: w("UB-Mannheim.TesseractOCR"),
        runnable: true,
        recommended: true,
      }),
      item("ffmpeg", "tools", "FFmpeg", "Convert videos and audio.", false, "Not installed", {
        command: w("Gyan.FFmpeg"),
        runnable: true,
      }),
    ],
    installAll: `${w("GitHub.cli")}; ${w("UB-Mannheim.TesseractOCR")}`,
  };
};
commands.setup_run = () => undefined;
commands.composio_import = () => {
  settings = {
    ...settings,
    composio: { ...settings.composio, enabled: true, url: "https://mcp.composio.dev/example" },
  };
  emit("settings://changed", settings);
  return settings;
};
const COMPOSIO_APPS = [
  ["googlecalendar", "Google Calendar", "Meeting reminders and your day"],
  ["outlook", "Outlook", "Calendar and mail from Microsoft 365"],
  ["gmail", "Gmail", "Unread client mail in the morning brief"],
  ["slack", "Slack", "Messages to you in the brief"],
  ["jira", "Jira", "Issues assigned to you"],
  ["github", "GitHub", "Pull requests and issues"],
  ["fathom", "Fathom", "Meeting notes for follow-ups"],
  ["notion", "Notion", "Pages and notes in Ask mode"],
];
// Apps already connected in the account (as Claude sees them).
const OTHER_APPS = [
  ["figma", "Figma"],
  ["googledrive", "Google Drive"],
  ["googlesheets", "Google Sheets"],
  ["linkedin", "LinkedIn"],
];
const connected = new Set(["googlecalendar", "gmail", "slack", "github"]);
commands.composio_status = () => {
  const signedIn = Boolean(settings.composio.account);
  return {
    signedIn,
    account: settings.composio.account,
    apps: [
      ...COMPOSIO_APPS.map(([slug, name, why]) => ({ slug, name, why, connected: signedIn && connected.has(slug) })),
      ...(signedIn ? OTHER_APPS.map(([slug, name]) => ({ slug, name, why: "", connected: true })) : []),
    ],
    error: null,
  };
};
const signedInNow = (how: string) => {
  settings = { ...settings, composio: { ...settings.composio, enabled: true, account: how, userId: "" } };
  emit("settings://changed", settings);
  emit("composio://changed", { ok: true, message: "Composio connected with 8 apps" });
};
commands.composio_sign_in = () => {
  later(2500, () => signedInNow("Composio Connect"));
  return "Composio opened in your browser";
};
commands.composio_use_key = () => {
  later(300, () => signedInNow("Composio key"));
  return "Composio connected";
};
commands.composio_sign_out = () => {
  settings = { ...settings, composio: { ...settings.composio, enabled: false, account: "", userId: "" } };
  emit("settings://changed", settings);
  emit("composio://changed", { ok: true, message: "Signed out" });
};
commands.composio_connect = (a) => {
  later(2000, () => {
    connected.add(a.slug as string);
    emit("composio://changed", { ok: true, message: "Connected" });
  });
};
const folder = (path: string, label: string, repos = 0) => ({ path, label, repos });
commands.setup_detect = () => ({
  codeFolders: [
    folder("C:\\Users\\you\\code", "code", 12),
    folder("\\\\wsl.localhost\\Ubuntu-22.04\\home\\you\\projects", "projects (WSL Ubuntu-22.04)", 5),
  ],
  searchFolders: [
    folder("C:\\Users\\you\\Documents", "Documents"),
    folder("C:\\Users\\you\\Desktop", "Desktop"),
    folder("C:\\Users\\you\\Downloads", "Downloads"),
  ],
  chatModels: ["qwen2.5:3b-instruct-q4_K_M"],
  embedModels: [],
  claudeInstalled: true,
  claudeHooks: false,
  claudeMcp: false,
  composioSignedIn: Boolean(settings.composio.account),
  composioInClaude: true,
  browsers: ["Chrome", "Edge", "Zen"],
  installable: [],
});
commands.setup_apply = () => {
  return ["Picked 2 code folders", "Added Claude Code hooks", "Installing the search model"];
};
commands.claude_add_hooks = () => "C:/Users/you/.claude/settings.json.sidekick-backup-20261002-101500";
commands.claude_add_mcp = () => undefined;
// Accepts any command with a name; the real check also looks for it on PATH.
commands.mcp_validate_stdio = (a) => {
  const spec = a.spec as { command?: string } | undefined;
  if (!spec?.command?.trim()) throw new Error("Enter a command to run.");
};
const browsersSeen = new Set<string>(["chrome"]);
commands.browsers_status = () => [
  { id: "chrome", name: "Chrome", connected: browsersSeen.has("chrome") },
  { id: "edge", name: "Edge", connected: browsersSeen.has("edge") },
  { id: "zen", name: "Zen", connected: browsersSeen.has("zen") },
];
commands.extension_install = (a) => {
  later(4000, () => browsersSeen.add(a.browser as string));
  return {
    copied: "C:\\Users\\you\\AppData\\Local\\app.duskie.desktop\\extension",
    page: "chrome://extensions/",
    steps: [
      "If the extensions page is not showing, paste its address into the address bar and press Enter (it is copied).",
      "Turn on Developer mode (top right), then click Load unpacked.",
      "Copy folder path below (or its Alt key), paste it into the folder box and press Enter, then Select Folder.",
      "Press Allow on Duskie's island when it asks.",
    ],
  };
};
commands.local_models = () => ({
  reachable: !previewFlag("nomodel"),
  chat: ["qwen2.5:3b-instruct-q4_K_M", "llama3.2:3b"],
  vision: ["moondream:latest"],
  embed: ["nomic-embed-text"],
});
commands.running_apps = () => ["code.exe", "chrome.exe", "slack.exe", "windowsterminal.exe", "keepassxc.exe"];
commands.suggestion_always = (a) => commands.suggestion_choose?.(a);
let laterItems = [
  { id: "l1", title: "3 new screenshots", detail: "Copy text or move them to a folder", minutesAgo: 12, missed: true },
  { id: "l2", title: "Download finished", detail: "invoice-sept.pdf", minutesAgo: 25, missed: false },
];
commands.later_list = () => laterItems;
commands.later_open = (a) => {
  laterItems = laterItems.filter((l) => l.id !== a.id);
  emit("suggestion://later", laterItems.length);
};
commands.later_clear = () => {
  laterItems = [];
  emit("suggestion://later", 0);
};
commands.later_dismiss = (a) => {
  laterItems = laterItems.filter((l) => l.id !== a.id);
  emit("suggestion://later", laterItems.length);
};
commands.skill_unmute = () => undefined;
const chats = new Map<string, { title: string; updated: string; turns: unknown[] }>([
  [
    "c1",
    {
      title: "How do I free port 3000",
      updated: new Date(Date.now() - 3_600_000).toISOString(),
      turns: [
        { role: "user", content: "How do I free port 3000" },
        { role: "assistant", content: "Run `netstat -ano | findstr :3000`, then `taskkill /PID <pid> /F`." },
      ],
    },
  ],
]);
commands.chats_list = () =>
  [...chats.entries()]
    .map(([id, c]) => ({ id, title: c.title, updated: c.updated }))
    .sort((a, b) => b.updated.localeCompare(a.updated));
commands.chat_get = (a) => chats.get(a.id as string)?.turns ?? [];
commands.chat_save = (a) => {
  chats.set(a.id as string, {
    title: a.title as string,
    updated: new Date().toISOString(),
    turns: a.turns as unknown[],
  });
};
commands.chat_delete = (a) => {
  chats.delete(a.id as string);
};
commands.ai_handoff = () => "C:/Users/you/AppData/Local/Duskie/ai/handoff";
commands.backup_export = () => "C:/Users/you/Documents/Duskie backup.json";
commands.backup_import = () => "Restored settings and 2 skills";
commands.project_pick_folder = () => {
  if (previewFlag("foldercancel")) return null;
  if (previewFlag("folderfail")) throw new Error("Could not save chosen folder");
  const path = "C:\\Users\\you\\Documents\\Other project";
  saveSettings({
    ...settings,
    codeFolders: [...new Set([...settings.codeFolders, path])],
    savedProjects: [...new Set([...settings.savedProjects, path])],
  });
  return path;
};
commands.projects_list = () => [
  ...settings.savedProjects.map((path) => ({ name: path.split(/[\\/]/).at(-1) ?? path, path })),
  ...(previewFlag("noprojects")
    ? []
    : [
        { name: "sidekick", path: "C:/Users/you/code/sidekick" },
        { name: "falconxoft-api", path: "C:/Users/you/code/falconxoft-api" },
      ]),
];
commands.editors_list = () => ({
  editors: [
    { id: "cursor", name: "Cursor", minutes: 840 },
    { id: "antigravity", name: "Antigravity", minutes: 180 },
    { id: "vscode", name: "VS Code", minutes: 40 },
    { id: "pycharm", name: "PyCharm", minutes: 0 },
  ],
  current: "Cursor",
});
commands.instant_find = (a) => {
  const q = String(a.query ?? "").toLowerCase();
  const apps = [
    { name: "Cursor", id: "cursor", minutes: 840 },
    { name: "Google Chrome", id: "chrome", minutes: 400, browser: "chrome" },
    { name: "Slack", id: "slack", minutes: 120 },
    { name: "Spotify", id: "spotify", minutes: 0 },
  ].filter((x) => x.name.toLowerCase().includes(q.split(" ")[0] ?? ""));
  const files = [
    { name: "cursor-rules.md", path: "C:/Users/you/Documents/cursor-rules.md", folder: false, place: "Documents" },
    { name: "invoice-oct.pdf", path: "C:/Users/you/Downloads/invoice-oct.pdf", folder: false, place: "Downloads" },
  ].filter((x) => x.name.includes(q.split(" ")[0] ?? ""));
  return { apps, files };
};
commands.app_launch = () => undefined;
commands.windows_settings_open = () => undefined;
commands.pc_switch = (a) => `${String(a?.name)} ${a?.on ? "on" : "off"}`;
commands.file_open = (a) => {
  const how = a.how as string | null | undefined;
  const path = String(a.path ?? "");
  if (how === "editor" || how === "reveal" || how === "default") return { opened: true };
  // No default for source-like names in the browser mock.
  if (/\.(tsx?|jsx?|rs)$/i.test(path)) return { opened: false };
  return { opened: true };
};
commands.project_launch = () => "Opened sidekick in VS Code and a terminal";
commands.search_status = () => ({ items: 1240, embedded: 1240, embedError: null });
let mockStashes = [{ name: "stash@{0}", message: "On main: island colours", when: "20 minutes ago" }];

const soon = () => {
  const d = new Date(Date.now() + 5 * 60_000);
  const hm = (x: Date) => `${String(x.getHours()).padStart(2, "0")}:${String(x.getMinutes()).padStart(2, "0")}`;
  return {
    title: "Design review",
    start: hm(d),
    end: hm(new Date(d.getTime() + 45 * 60_000)),
    joinUrl: "https://meet.google.com/abc-defg-hij",
    notes: { title: "Design review notes", source: "page", reference: "https://docs.example.com/review" },
  };
};
commands.calendar_today = () => ({
  meetings: previewFlag("meeting")
    ? [soon()]
    : connected.has("googlecalendar")
      ? [
          { title: "Standup", start: "10:00", end: "10:15", joinUrl: "https://meet.google.com/abc-defg-hij" },
          { title: "Design review", start: "15:00", end: "15:45", joinUrl: null },
        ]
      : [],
  error: null,
  sources: connected.has("googlecalendar") ? ["Google Calendar"] : [],
});
commands.voice_download = () => {
  let done = 0;
  const total = 203_669_641;
  const tick = () => {
    done = Math.min(total, done + 30_000_000);
    emit("voice://download", { label: "Supertonic voice", done, total, finished: done >= total, error: null });
    if (done < total) later(300, tick);
  };
  tick();
};
commands.voice_cancel_download = () => undefined;
commands.voice_typing = () => undefined;
commands.voice_listen = () => {
  const words = ["What", "What time", "What time is it", "What time is it in London?"];
  for (const [i, w] of words.entries()) {
    later(400 * (i + 1), () => emit("voice://heard", { text: w, final: false, byVoice: false }));
  }
  later(2200, () => emit("voice://heard", { text: words[3], final: true, byVoice: false }));
};
commands.voice_stop = () => {
  for (const t of speechTimers) clearTimeout(t);
  speechTimers = [];
};
// The welcome line as Rust would report it while it is spoken.
const WELCOME_LINES = [
  "Hey, I'm Duskie. I live up here with you on this PC. I notice things, I help when you want, and I stay put: nothing leaves this machine, and I wait for your okay.",
  "First, how I think. If you want everything to stay on this PC, you can run a local model, only if your machine is up for it. Or use Claude Code or Codex with the plan you already have. You can use any of them, or all three, and set the order I try.",
  "Now, your world. Connect your calendar, your mail and the tools you use, and I'll start noticing what matters.",
  "A few small helpers make me sharper. Install the ones you want, and I'll wait while they finish.",
  "Welcome aboard. Say Hey Duskie whenever you need me. Do Not Disturb is on so Windows stays quiet and alerts show once up here. Launch on login is already on, so I'm here when you sit down.",
];
let welcome: WelcomeSpeech = {
  step: 0,
  lines: WELCOME_LINES,
  script: WELCOME_LINES[0],
  pieces: [],
  endsAt: null,
  silent: false,
  pending: false,
};
let speechTimers: ReturnType<typeof setTimeout>[] = [];
/** Like Rust: each step's line as sentences with when they sound. The first
 * one waits 3 s, like the model loading. */
function speakWelcome(step = 0) {
  for (const t of speechTimers) clearTimeout(t);
  speechTimers = [];
  const line = WELCOME_LINES[step] ?? WELCOME_LINES[0];
  const sentences = line.match(/[^.!?]+[.!?]/g) ?? [line];
  let at = Date.now() + (step === 0 ? 3000 : 400);
  welcome = { ...welcome, step, script: line, pieces: [], endsAt: null, pending: true };
  emit("voice://welcome", welcome);
  sentences.forEach((raw, i) => {
    const text = raw.trim();
    const ms = text.split(/\s+/).length * 330;
    const startsAt = at;
    at += ms + 280;
    // Pieces arrive a little ahead of when they sound, like synthesis.
    speechTimers.push(
      setTimeout(
        () => {
          welcome = { ...welcome, pieces: [...welcome.pieces, { text, startsAt, ms }] };
          if (i === sentences.length - 1) welcome = { ...welcome, endsAt: startsAt + ms, pending: false };
          emit("voice://welcome", welcome);
        },
        Math.max(0, startsAt - Date.now() - 300),
      ),
    );
  });
}
commands.voice_welcome = () => welcome;
commands.voice_welcome_step = (a) => speakWelcome(Number(a.step) || 0);
commands.voice_say = () => undefined;
commands.voice_read = () => undefined;
commands.voice_test = () => undefined;
commands.search = (a) => [
  {
    source: "file",
    reference: "C:/Users/you/notes/acme.md",
    title: "acme.md",
    snippet: `Invoice for [${a.query}] Corp, due Friday ... rate 45 USD per hour`,
    ts: "2026-10-01T09:00:00Z",
  },
  {
    source: "chat",
    reference: "c1",
    title: "How do I free port 3000",
    snippet: `Use netstat -ano to find the [${a.query}] process`,
    ts: "2026-10-01T10:00:00Z",
  },
];
commands.search_reindex = () => undefined;
commands.open_reference = () => undefined;
commands.time_today = () => [
  { app: "Visual Studio Code", project: "sidekick", secs: 9420 },
  { app: "Google Chrome", project: "", secs: 4310 },
  { app: "Windows Terminal", project: "", secs: 2200 },
  { app: "Visual Studio Code", project: "falconxoft-api", secs: 1500 },
  { app: "Slack", project: "", secs: 640 },
];
commands.browser_info = () => ({ token: "browser-preview-pairing-code", port: 47822 });
commands.action_undo = () => "Moved photo.webp to the Recycle Bin";
const GB = 1024 ** 3;
commands.disk_groups = () =>
  [
    {
      id: "build",
      label: "Build folders",
      what: "node_modules, target and .next in your projects. The next install or build makes them again.",
      bytes: 18.4 * GB,
      partial: false,
      items: [
        { path: "C:\\Users\\you\\code\\sidekick\\target", bytes: 11.2 * GB },
        { path: "C:\\Users\\you\\code\\shop\\node_modules", bytes: 4.1 * GB },
        { path: "C:\\Users\\you\\code\\blog\\node_modules", bytes: 3.1 * GB },
      ],
      clearable: true,
    },
    {
      id: "games",
      label: "Games",
      what: "Steam and Epic libraries. Uninstall a game from its launcher.",
      bytes: 96 * GB,
      partial: false,
      items: [{ path: "D:\\SteamLibrary\\steamapps\\common\\Cyberpunk 2077", bytes: 70 * GB }],
      clearable: false,
    },
    {
      id: "installers",
      label: "Old installers",
      what: "Setup files in Downloads, already installed or not needed.",
      bytes: 2.3 * GB,
      partial: false,
      items: [{ path: "C:\\Users\\you\\Downloads\\Docker Desktop Installer.exe", bytes: 0.6 * GB }],
      clearable: true,
    },
  ].sort((a, b) => b.bytes - a.bytes);
commands.disk_clean = () => "Moved 2 items to the Recycle Bin, 7.2 GB freed";
commands.user_guess_name = () => "Taimoor";
let mockFocusUntil: number | null = null;
commands.focus_start = (args) => {
  const minutes = Number((args as { minutes?: number } | undefined)?.minutes ?? 25);
  mockFocusUntil = Date.now() + minutes * 60_000;
  emit("island://focus", { until: mockFocusUntil, held: 0 });
  return `Focusing for ${minutes} minutes`;
};
commands.focus_stop = () => {
  mockFocusUntil = null;
  emit("island://focus", { until: null, held: [], ended: true });
  return "Focus done. Nothing came in.";
};
commands.focus_status = () => ({ until: mockFocusUntil, held: 0 });
commands.suggestion_rates = () => [
  { skill: "files.screenshot", taken: 12, dismissed: 3 },
  { skill: "dev.port-in-use", taken: 2, dismissed: 7 },
];
commands.agent_usual = () => null;
commands.learned_list = () => [
  {
    kind: "choice",
    key: "url:localhost",
    label: "Chrome, private",
    text: "For links from localhost, you pick Chrome, private",
    why: "9 times",
  },
  {
    kind: "routine",
    key: "slack",
    label: "app",
    text: "You open Slack as part of your day",
    why: "On 5 of the last days",
  },
];
commands.learned_forget = () => undefined;
commands.learned_forget_all = () => undefined;
commands.freeze_report = () => [];
commands.freeze_summary = () => ({ commands: 0, stalls: 0, worstMs: 0 });
commands.ai_status = () =>
  previewFlag("nomodel")
    ? []
    : [
        { id: "claude_code", available: true, local: false, configured: true },
        { id: "codex", available: true, local: false, configured: true },
        { id: "anthropic", available: false, local: false, configured: false },
        { id: "local", available: true, local: true, configured: true },
        { id: "semif", available: false, local: true, configured: false },
      ];

export const mock = {
  async invoke<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
    const fn = commands[cmd];
    if (!fn) throw new Error(`mock: unknown command ${cmd}`);
    return fn(args) as T;
  },
  listen(event: string, handler: Handler): () => void {
    let set = handlers.get(event);
    if (!set) {
      set = new Set();
      handlers.set(event, set);
    }
    set.add(handler);
    return () => set.delete(handler);
  },
};

// Exposed for console debugging in the browser preview.
if (typeof window !== "undefined") {
  (window as unknown as { sidekickMock: unknown }).sidekickMock = { go, cues: CUES, invoke: mock.invoke, emit };
}

// Preview only (browser mock, never in the app): lets the parity script put
// the island in any state.
if (typeof window !== "undefined") {
  Object.assign(window, {
    __mockEmit: emit,
    __mockGo: go,
    __mockCall: (name: string, args: Record<string, unknown> = {}) => commands[name]?.(args),
    __mockAgents: async (calm = false) => {
      const { useAgents } = await import("./agents");
      const now = Date.now();
      const mk = (
        id: string,
        agent: string,
        project: string,
        title: string,
        status: string,
        mode: string,
        mins: number,
        extra = {},
      ) => ({
        id,
        agent,
        project,
        branch: null,
        reviewable: true,
        mode,
        title,
        status,
        entries: [],
        plan: [],
        usage: null,
        question: null,
        error: null,
        startedAt: now - mins * 60_000,
        turnAt: now - mins * 60_000,
        tookMs: null,
        changes: 0,
        ...extra,
      });
      useAgents.setState({
        sessions: [
          mk("a1", "Codex", "duskie", "Migrate tests to Vitest", "idle", "full", 22, { finished: true }),
          mk("a2", "Claude Code", "duskie", "Compact label wraps on narrow screens", "idle", "edit", 10, {
            changes: 3,
          }),
          mk("a3", "Claude Code", "falconxoft-portal", "Add CSV export to invoices", "idle", "edit", 14, {
            changes: 5,
          }),
          mk("a4", "Codex", "api-gateway", "Fix missing rate limit headers", calm ? "idle" : "waiting", "ask", 3),
          mk("a5", "Claude Code", "duskie", "Writing tests for the wash controller", "working", "edit", 1, {
            branch: "agent/claude-voice-settings",
            plan: ["Read the controller", "List the cases", "Write the tests", "Run them", "Open a review"].map(
              (text, i) => ({ text, status: i < 2 ? "completed" : i === 2 ? "in_progress" : "pending" }),
            ),
            entries: [
              {
                kind: "step",
                step: { id: "s1", tool: "edit", label: "Edited lib/wash.ts", detail: "+42 -6", state: "done" },
              },
              {
                kind: "step",
                step: {
                  id: "s2",
                  tool: "bash",
                  label: "Ran pnpm test --filter desktop",
                  detail: "38 passed",
                  state: "done",
                },
              },
            ],
          }),
        ] as never,
        current: null,
      });
    },
    __mockStore: async (patch: Record<string, unknown>) => {
      const { useDuskie } = await import("./store");
      useDuskie.setState(patch as never);
    },
    __mockWork: async (label: string) => {
      const { beginActivity } = await import("./activities");
      beginActivity(label);
      go("working");
    },
    __mockSuggest: (s: Suggestion) => {
      suggestion = s;
      emit("suggestion://new", s);
    },
  });
}
