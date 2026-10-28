// CHRONOS attribution options — pairing token management.
//
// Preferred: automatic pairing via the local pairing page (background/pair content
// script), or this options page opened with the token in the URL fragment
// (options.html#token=…), which is consumed once and stripped from the visible URL.
// Manual paste remains as a fallback. The token is never logged.

const PAIR_URL = "http://127.0.0.1:53711/pair";
const input = document.getElementById("token");
const status = document.getElementById("status");

function refreshStored() {
  chrome.storage.local.get("chronosToken").then(({ chronosToken }) => {
    input.value = chronosToken || "";
    status.textContent = chronosToken
      ? "Paired. Attribution is active while the CHRONOS desktop app is running."
      : "Not paired yet.";
    status.className = chronosToken ? "ok" : "";
  });
}
refreshStored();
chrome.storage.onChanged.addListener((changes, area) => {
  if (area === "local" && changes.chronosToken) refreshStored();
});

// Token-in-URL pairing: options.html#token=… or ?token=… — saved once, then removed
// from the visible URL (and from history via replaceState) immediately.
(async () => {
  let token = null;
  try {
    const hash = new URLSearchParams(location.hash.slice(1));
    const query = new URLSearchParams(location.search);
    token = hash.get("token") || query.get("token");
  } catch { /* malformed URL — ignore */ }
  if (token) {
    await chrome.storage.local.set({ chronosToken: token.trim() });
    status.textContent = "Paired automatically from the link. Attribution is active while CHRONOS is running.";
    status.className = "ok";
    refreshStored();
  }
  history.replaceState(null, "", location.pathname);
})();

document.getElementById("save").addEventListener("click", async () => {
  await chrome.storage.local.set({ chronosToken: input.value.trim() });
  status.textContent = input.value.trim()
    ? "Saved. Attribution is active while CHRONOS is running."
    : "Token cleared — attribution is paused until you pair again.";
  status.className = input.value.trim() ? "ok" : "";
});

document.getElementById("pair-auto").addEventListener("click", () => {
  chrome.tabs.create({ url: PAIR_URL });
});
