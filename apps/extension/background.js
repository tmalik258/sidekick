// Sidekick extension, background. Relays page events to the desktop app on
// 127.0.0.1 and runs the commands it sends back (fill a login, close
// duplicate tabs, save the session). Nothing here talks to any other server.

const BRIDGE = "http://127.0.0.1:47822";
const MANY_TABS = 25;

async function token() {
  const { token } = await chrome.storage.local.get("token");
  return token || "";
}

/** "Chrome", "Edge", "Brave", "Opera", "Firefox": shown on Sidekick's prompt. */
function browserName() {
  const ua = navigator.userAgent;
  if (/Firefox\//.test(ua)) return "Firefox";
  if (/Edg\//.test(ua)) return "Edge";
  if (/OPR\//.test(ua)) return "Opera";
  if (navigator.brave) return "Brave";
  return "Chrome";
}

async function call(path, init = {}) {
  const t = await token();
  if (!t) return null;
  const res = await fetch(`${BRIDGE}${path}`, {
    ...init,
    headers: {
      "content-type": "application/json",
      "x-sidekick-token": t,
      "x-sidekick-browser": browserName(),
      ...(init.headers || {}),
    },
  });
  // Sidekick was reinstalled or reset: pair again.
  if (res.status === 401) {
    await chrome.storage.local.remove("token");
    pair();
  }
  return res;
}

/**
 * Asks Sidekick to connect. Sidekick shows Allow or Deny on its island; on
 * Allow it answers with the token. Waits up to 90 seconds for the answer.
 */
let pairing = null;
function pair() {
  if (pairing) return pairing;
  pairing = (async () => {
    try {
      const res = await fetch(`${BRIDGE}/browser/pair`, {
        method: "POST",
        headers: { "x-sidekick-browser": browserName() },
      });
      if (res.status !== 200) return res.status === 403 ? "denied" : "busy";
      const { token } = await res.json();
      if (!token) return "denied";
      await chrome.storage.local.set({ token });
      return "connected";
    } catch {
      return "offline";
    } finally {
      pairing = null;
    }
  })();
  return pairing;
}

chrome.runtime.onInstalled.addListener(async () => {
  if (!(await token())) pair();
});
chrome.runtime.onStartup.addListener(async () => {
  if (!(await token())) pair();
});

async function report(event) {
  try {
    await call("/browser/event", { method: "POST", body: JSON.stringify(event) });
    poll();
  } catch {
    // Sidekick is not running; nothing to do.
  }
}

// Commands are picked up with a long poll while there is reason to expect
// one (right after an event). The alarm restarts it if the worker slept.
let polling = false;
async function poll() {
  if (polling) return;
  polling = true;
  try {
    for (let i = 0; i < 3; i++) {
      const res = await call("/browser/next");
      if (!res) break;
      if (res.status === 200) await run(await res.json());
    }
  } catch {
    // Not running or not paired.
  } finally {
    polling = false;
  }
}

async function run(cmd) {
  if (cmd.type === "fill") return fill(cmd);
  if (cmd.type === "close_duplicates") return closeDuplicates();
  if (cmd.type === "save_session") return saveSession();
}

function hostOf(url) {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return "";
  }
}

/** Fills only a tab whose host is the one the login was matched to. */
async function fill(cmd) {
  const tabs = await chrome.tabs.query({});
  const tab =
    tabs.find((t) => t.id === cmd.tab && hostOf(t.url) === cmd.domain) ||
    tabs.find((t) => t.active && hostOf(t.url) === cmd.domain);
  if (!tab) return;
  await chrome.tabs.sendMessage(tab.id, {
    type: "sidekick-fill",
    domain: cmd.domain,
    username: cmd.username,
    password: cmd.password,
  });
}

function duplicateIds(tabs) {
  const seen = new Set();
  const dupes = [];
  for (const t of tabs) {
    const key = (t.url || "").split("#")[0];
    if (!key.startsWith("http")) continue;
    if (seen.has(key) && !t.active && !t.pinned) dupes.push(t.id);
    else seen.add(key);
  }
  return dupes;
}

async function closeDuplicates() {
  const tabs = await chrome.tabs.query({});
  const ids = duplicateIds(tabs);
  if (ids.length) await chrome.tabs.remove(ids);
}

async function saveSession() {
  const tabs = (await chrome.tabs.query({})).filter((t) => (t.url || "").startsWith("http"));
  const stamp = new Date().toLocaleString();
  const folder = await chrome.bookmarks.create({ title: `Sidekick session ${stamp}` });
  for (const t of tabs) await chrome.bookmarks.create({ parentId: folder.id, title: t.title || t.url, url: t.url });
}

// Many tabs: report once each time the count crosses the line.
let overLine = false;
async function checkTabs() {
  const tabs = await chrome.tabs.query({});
  const count = tabs.length;
  if (count > MANY_TABS && !overLine) {
    overLine = true;
    await report({ kind: "many_tabs", url: "", count, duplicates: duplicateIds(tabs).length });
  } else if (count <= MANY_TABS - 3) {
    overLine = false;
  }
}

chrome.tabs.onCreated.addListener(checkTabs);
chrome.tabs.onRemoved.addListener(checkTabs);

// Page events from the content script, with the tab they came from.
chrome.runtime.onMessage.addListener((msg, sender, reply) => {
  if (msg?.type === "sidekick-pair") {
    pair().then(reply);
    return true;
  }
  if (msg?.type === "sidekick-event" && sender.tab) report({ ...msg.event, tab: sender.tab.id });
});

chrome.alarms.create("sidekick-poll", { periodInMinutes: 1 });
chrome.alarms.onAlarm.addListener(() => poll());
