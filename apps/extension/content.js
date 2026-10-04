// Sidekick extension, page side. Notices a long article or an Upwork job and
// tells the background, which tells the desktop app. It never reads keystrokes
// and never types into password fields for agent actions.

(() => {
  // Loaded once per page, even when Sidekick injects it again.
  if (window.__sidekick) return;
  window.__sidekick = true;
  const sent = new Set();
  const send = (event) => {
    if (sent.has(event.kind)) return;
    sent.add(event.kind);
    chrome.runtime.sendMessage({
      type: "sidekick-event",
      event: { ...event, url: location.href, title: document.title },
    });
  };

  const visible = (el) => !!(el.offsetWidth || el.offsetHeight || el.getClientRects().length);

  function check() {
    if (/(^|\.)upwork\.com$/.test(location.hostname) && /\/(jobs|job)\//.test(location.pathname)) {
      const text = (document.querySelector("main") || document.body).innerText.slice(0, 20000);
      send({ kind: "upwork_job", text });
      return;
    }

    const root = document.querySelector("article") || document.querySelector("main");
    if (root) {
      const text = root.innerText;
      const words = text.split(/\s+/).length;
      if (words > 1500) send({ kind: "long_read", words, text: text.slice(0, 20000) });
    }
  }

  // Pages that build their forms late (single-page apps) get a few looks.
  check();
  let looks = 0;
  const timer = setInterval(() => {
    check();
    if (++looks >= 5) clearInterval(timer);
  }, 2000);

  // Reading and acting on the page for Sidekick (only when you asked it to).
  // Elements get a short number ([12]) so a model can say "click 12".
  const INTERACTIVE =
    'a[href], button, input:not([type="hidden"]), textarea, select, summary, [role="button"], [role="link"], [role="tab"], [role="menuitem"], [role="checkbox"], [role="radio"], [role="option"], [role="switch"], [role="combobox"], [role="searchbox"], [role="textbox"], [contenteditable=""], [contenteditable="true"]';
  let nextRef = 1;
  const clean = (t) => (t || "").replace(/\s+/g, " ").trim().slice(0, 80);

  function label(el) {
    const by = el.getAttribute("aria-labelledby");
    const labelled = by && document.getElementById(by.split(" ")[0]);
    return clean(
      el.getAttribute("aria-label") ||
        labelled?.innerText ||
        el.labels?.[0]?.innerText ||
        (el.tagName !== "INPUT" && el.tagName !== "TEXTAREA" ? el.innerText : "") ||
        el.getAttribute("placeholder") ||
        el.getAttribute("title") ||
        el.getAttribute("alt") ||
        el.querySelector?.("img[alt]")?.getAttribute("alt") ||
        el.getAttribute("name") ||
        (el.type === "submit" || el.type === "button" ? el.value : ""),
    );
  }

  function kind(el) {
    const role = el.getAttribute("role");
    if (role) return role;
    const tag = el.tagName.toLowerCase();
    if (tag === "a") return "link";
    if (tag === "select") return "select";
    if (tag === "textarea" || el.isContentEditable) return "textbox";
    if (tag === "input") {
      const t = (el.type || "text").toLowerCase();
      if (t === "password") return "password";
      if (["submit", "button", "reset", "image"].includes(t)) return "button";
      if (t === "checkbox" || t === "radio") return t;
      if (t === "search") return "searchbox";
      return `textbox(${t})`;
    }
    return tag;
  }

  function inView(el) {
    const r = el.getBoundingClientRect();
    return r.bottom > 0 && r.top < innerHeight && r.right > 0 && r.left < innerWidth;
  }

  function snapshot() {
    const els = [...document.querySelectorAll(INTERACTIVE)].filter(visible);
    // What is on screen first, then the rest of the page.
    els.sort((a, b) => Number(inView(b)) - Number(inView(a)));
    const lines = [];
    for (const el of els.slice(0, 160)) {
      if (!el.dataset.sidekickRef) el.dataset.sidekickRef = String(nextRef++);
      const k = kind(el);
      const name = label(el);
      if (!name && !k.startsWith("textbox") && k !== "password") continue;
      let line = `[${el.dataset.sidekickRef}] ${k} "${name}"`;
      if (k.startsWith("textbox") || k === "searchbox" || k === "combobox") {
        const v = clean(el.isContentEditable ? el.innerText : el.value);
        if (v) line += ` = "${v}"`;
      }
      if (k === "checkbox" || k === "radio") line += el.checked ? " (checked)" : "";
      if (k === "select") line += ` = "${clean(el.selectedOptions?.[0]?.text)}"`;
      if (el.disabled) line += " (disabled)";
      lines.push(line);
    }
    const root = document.querySelector("main") || document.querySelector("article") || document.body;
    return {
      title: document.title,
      url: location.href,
      elements: lines.join("\n"),
      text: (root.innerText || "").replace(/\n{3,}/g, "\n\n").slice(0, 4000),
    };
  }

  function find(ref) {
    const el = document.querySelector(`[data-sidekick-ref="${ref}"]`);
    if (!el) throw new Error(`No element [${ref}] on the page now; read the page again`);
    return el;
  }

  /** A soft outline on what Sidekick touches, so you see it happen. */
  function flash(el) {
    const old = el.style.outline;
    el.style.outline = "2px solid #0a84ff";
    el.style.outlineOffset = "2px";
    setTimeout(() => {
      el.style.outline = old;
    }, 900);
  }

  function setValue(el, text) {
    if (el.isContentEditable) {
      el.focus();
      document.execCommand("selectAll", false);
      document.execCommand("insertText", false, text);
      return;
    }
    const proto = el.tagName === "TEXTAREA" ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, text);
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
  }

  function act(cmd) {
    if (cmd.do === "scroll") {
      window.scrollBy({ top: (cmd.text === "up" ? -1 : 1) * innerHeight * 0.8, behavior: "smooth" });
      return { ok: true, note: "Scrolled" };
    }
    const el = find(cmd.ref);
    el.scrollIntoView({ block: "center" });
    flash(el);
    const k = kind(el);
    switch (cmd.do) {
      case "click":
        el.click();
        return { ok: true, note: `Clicked ${label(el) || k}` };
      case "type":
        if (k === "password") throw new Error("Sidekick never types into password fields");
        el.focus();
        setValue(el, cmd.text || "");
        return { ok: true, note: `Typed into ${label(el) || k}` };
      case "select": {
        const want = (cmd.text || "").toLowerCase();
        const opt = [...(el.options || [])].find(
          (o) => o.text.toLowerCase().includes(want) || o.value.toLowerCase() === want,
        );
        if (!opt) throw new Error(`No option like "${cmd.text}"`);
        el.value = opt.value;
        el.dispatchEvent(new Event("change", { bubbles: true }));
        return { ok: true, note: `Picked ${opt.text}` };
      }
      case "press": {
        const key = cmd.text || "Enter";
        el.focus();
        for (const type of ["keydown", "keypress", "keyup"]) {
          el.dispatchEvent(
            new KeyboardEvent(type, {
              key,
              code: key,
              keyCode: key === "Enter" ? 13 : 0,
              which: key === "Enter" ? 13 : 0,
              bubbles: true,
            }),
          );
        }
        if (key === "Enter" && el.form && typeof el.form.requestSubmit === "function") el.form.requestSubmit();
        return { ok: true, note: `Pressed ${key}` };
      }
      case "focus":
        el.focus();
        return { ok: true, note: "Focused" };
      default:
        throw new Error(`Unknown action ${cmd.do}`);
    }
  }

  function extract(what) {
    if (what === "links") {
      return {
        links: [...document.querySelectorAll("a[href]")]
          .filter(visible)
          .slice(0, 120)
          .map((a) => ({ text: clean(a.innerText || a.getAttribute("aria-label")), href: a.href })),
      };
    }
    if (what === "tables") {
      return {
        tables: [...document.querySelectorAll("table")].slice(0, 5).map((t) =>
          [...t.rows]
            .slice(0, 60)
            .map((r) => [...r.cells].map((c) => clean(c.innerText)).join("\t"))
            .join("\n"),
        ),
      };
    }
    const root = document.querySelector("main") || document.querySelector("article") || document.body;
    return { text: (root.innerText || "").slice(0, 12000) };
  }

  chrome.runtime.onMessage.addListener((msg, _sender, reply) => {
    if (msg?.type !== "sidekick-page") return;
    try {
      if (msg.op === "read") reply(snapshot());
      else if (msg.op === "act") reply(act(msg));
      else if (msg.op === "extract") reply(extract(msg.what));
      else reply(snapshot());
    } catch (e) {
      reply({ error: String(e?.message || e) });
    }
  });
})();
