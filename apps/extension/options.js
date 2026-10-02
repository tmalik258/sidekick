const input = document.getElementById("token");
const status = document.getElementById("status");
const BRIDGE = "http://127.0.0.1:47822";

async function check(token) {
  try {
    const res = await fetch(`${BRIDGE}/browser/hello`, { headers: { "x-sidekick-token": token } });
    return res.status === 200 ? "connected" : res.status === 401 ? "mismatch" : "error";
  } catch {
    return "offline";
  }
}

const MESSAGES = {
  connected: "Connected.",
  denied: "Not connected: Allow was not pressed. Press Connect to try again.",
  busy: "Sidekick is already asking. Look at the island at the top of your screen.",
  offline: "Sidekick is not running on this PC. Start it, then press Connect.",
  mismatch: "Sidekick is running, but the code does not match.",
  error: "Sidekick answered unexpectedly. Try again.",
};

chrome.storage.local.get("token").then(async ({ token }) => {
  input.value = token || "";
  if (token) status.textContent = MESSAGES[await check(token)];
});

document.getElementById("connect").addEventListener("click", async () => {
  status.textContent = "Waiting for you to press Allow in Sidekick...";
  const result = await chrome.runtime.sendMessage({ type: "sidekick-pair" });
  status.textContent = MESSAGES[result] || MESSAGES.error;
});

document.getElementById("save").addEventListener("click", async () => {
  const token = input.value.trim();
  await chrome.storage.local.set({ token });
  status.textContent = MESSAGES[await check(token)];
});
