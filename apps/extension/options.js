const input = document.getElementById("token");
const status = document.getElementById("status");

chrome.storage.local.get("token").then(({ token }) => {
  input.value = token || "";
});

document.getElementById("save").addEventListener("click", async () => {
  const token = input.value.trim();
  await chrome.storage.local.set({ token });
  try {
    const res = await fetch("http://127.0.0.1:47822/browser/hello", { headers: { "x-sidekick-token": token } });
    status.textContent =
      res.status === 200
        ? "Connected."
        : res.status === 401
          ? "Sidekick is running, but the code does not match."
          : `Sidekick answered ${res.status}.`;
  } catch {
    status.textContent = "Sidekick is not running on this PC.";
  }
});
