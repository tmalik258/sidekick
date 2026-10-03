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
  poll();
});
poll();

async function report(event) {
  try {
    await call("/browser/event", { method: "POST", body: JSON.stringify(event) });
    poll();
  } catch {
    // Sidekick is not running; nothing to do.
  }
}

// Commands are picked up with a long poll that keeps going while Sidekick
// answers, so a request (read this page, click Send) runs at once. The
// alarm restarts it if the browser put the worker to sleep.
let polling = false;
async function poll() {
  if (polling) return;
  polling = true;
  try {
    for (;;) {
      const res = await call("/browser/next");
      if (!res) break;
      if (res.status === 200) {
        const cmd = await res.json();
        // Requests are answered; the rest just run.
        if (cmd.id) answer(cmd);
        else await run(cmd);
      } else if (res.status !== 204) break;
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

async function answer(cmd) {
  let result;
  try {
    result = await request(cmd);
  } catch (e) {
    result = { error: String(e?.message || e) };
  }
  try {
    await call("/browser/result", { method: "POST", body: JSON.stringify({ id: cmd.id, result }) });
  } catch {
    // Sidekick went away.
  }
}

function denied(url, deny) {
  const host = hostOf(url);
  return (deny || []).some((d) => host === d || host.endsWith(`.${d}`));
}

async function tabFor(cmd) {
  if (cmd.tab) return chrome.tabs.get(cmd.tab);
  const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
  if (!tab) throw new Error("No open tab");
  return tab;
}

/** Waits until a tab finished loading (or 15 seconds). */
function loaded(tabId) {
  return new Promise((resolve) => {
    const done = () => {
      chrome.tabs.onUpdated.removeListener(listen);
      clearTimeout(timer);
      resolve();
    };
    const listen = (id, change) => {
      if (id === tabId && change.status === "complete") done();
    };
    const timer = setTimeout(done, 15000);
    chrome.tabs.onUpdated.addListener(listen);
    chrome.tabs.get(tabId).then((t) => {
      if (t.status === "complete") done();
    });
  });
}

/** Asks the page script; loads it first on pages opened before install. */
async function page(tabId, msg) {
  try {
    return await chrome.tabs.sendMessage(tabId, { ...msg, op: msg.type, type: "sidekick-page" });
  } catch {
    await chrome.scripting.executeScript({ target: { tabId }, files: ["content.js"] });
    return chrome.tabs.sendMessage(tabId, { ...msg, op: msg.type, type: "sidekick-page" });
  }
}

const brief = (t) => ({ tab: t.id, title: t.title || "", url: t.url || "", active: !!t.active });

async function request(cmd) {
  switch (cmd.type) {
    case "tabs":
      return { tabs: (await chrome.tabs.query({})).filter((t) => (t.url || "").startsWith("http")).map(brief) };
    case "open": {
      if (!/^https?:\/\//.test(cmd.url || "")) throw new Error("Only http and https links");
      if (denied(cmd.url, cmd.deny)) throw new Error("That site is on your ignore list");
      const tab =
        cmd.newTab === false
          ? await chrome.tabs.update((await tabFor({})).id, { url: cmd.url, active: true })
          : await chrome.tabs.create({ url: cmd.url, active: true });
      await loaded(tab.id);
      return brief(await chrome.tabs.get(tab.id));
    }
    case "switch": {
      const tab = await chrome.tabs.update(cmd.tab, { active: true });
      await chrome.windows.update(tab.windowId, { focused: true });
      return brief(tab);
    }
    case "close":
      await chrome.tabs.remove(cmd.tab);
      return { ok: true };
    case "back": {
      const tab = await tabFor(cmd);
      await chrome.tabs.goBack(tab.id);
      await loaded(tab.id);
      return brief(await chrome.tabs.get(tab.id));
    }
    case "read":
    case "act":
    case "extract": {
      const tab = await tabFor(cmd);
      if (!(tab.url || "").startsWith("http")) throw new Error("That tab is not a web page");
      if (denied(tab.url, cmd.deny)) throw new Error("That site is on your ignore list");
      const result = await page(tab.id, cmd);
      if (cmd.type === "act") {
        // A click may navigate: wait, then report where the tab is now.
        await new Promise((r) => setTimeout(r, 700));
        await loaded(tab.id);
        const now = await chrome.tabs.get(tab.id);
        return { ...result, tab: now.id, title: now.title, url: now.url };
      }
      return { ...result, tab: tab.id };
    }
    default:
      throw new Error(`Unknown request ${cmd.type}`);
  }
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

// The site in the active tab, for routines and time per site. Only the
// origin is sent (never the path), and only when it changes.
let lastSite = "";
async function reportSite() {
  try {
    const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
    const url = tab?.url || "";
    if (!url.startsWith("http")) return;
    const origin = new URL(url).origin;
    if (origin === lastSite) return;
    lastSite = origin;
    await report({ kind: "site", url: `${origin}/` });
  } catch {
    // No window or tab right now.
  }
}

chrome.tabs.onActivated.addListener(reportSite);
chrome.tabs.onUpdated.addListener((_id, change, tab) => {
  if (change.status === "complete" && tab.active) reportSite();
});
chrome.windows?.onFocusChanged.addListener(reportSite);

// Page events from the content script, with the tab they came from.
chrome.runtime.onMessage.addListener((msg, sender, reply) => {
  if (msg?.type === "sidekick-pair") {
    pair().then(reply);
    return true;
  }
  if (msg?.type === "sidekick-event" && sender.tab) report({ ...msg.event, tab: sender.tab.id });
});

chrome.alarms.create("sidekick-poll", { periodInMinutes: 0.5 });
chrome.alarms.onAlarm.addListener(() => poll());
