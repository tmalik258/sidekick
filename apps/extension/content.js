// Sidekick extension, page side. Notices a login form, a long article or an
// Upwork job and tells the background, which tells the desktop app. It never
// reads what you type, and fills a login only when you pick that on the
// island.

(() => {
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

  function passwordField() {
    return [...document.querySelectorAll('input[type="password"]')].find(visible);
  }

  function check() {
    if (passwordField()) send({ kind: "login_form" });

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

  // Fill only on this exact host, and only into what looks like the form.
  chrome.runtime.onMessage.addListener((msg) => {
    if (msg?.type !== "sidekick-fill") return;
    if (location.hostname.replace(/^www\./, "") !== msg.domain) return;
    const pass = passwordField();
    if (!pass) return;
    const form = pass.form || document;
    const user = [
      ...form.querySelectorAll('input[type="email"], input[type="text"], input[autocomplete="username"]'),
    ].find(visible);
    const set = (el, value) => {
      const proto = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value");
      proto.set.call(el, value);
      el.dispatchEvent(new Event("input", { bubbles: true }));
      el.dispatchEvent(new Event("change", { bubbles: true }));
    };
    if (user && msg.username) set(user, msg.username);
    set(pass, msg.password);
    pass.focus();
  });
})();
