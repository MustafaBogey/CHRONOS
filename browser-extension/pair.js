// CHRONOS pairing content script — runs only on the local pairing page served by the
// CHRONOS desktop app (http://127.0.0.1:53711/pair*). Reads the pairing token embedded
// by the app, stores it in extension-local storage, and confirms success on the page.
// The token is never displayed or logged.

const token = document.body && document.body.dataset ? document.body.dataset.token : "";
const state = document.getElementById("state");

if (token) {
  chrome.storage.local.set({ chronosToken: token }).then(() => {
    if (state) {
      state.textContent =
        "✓ Paired with CHRONOS. Website attribution is active while the CHRONOS desktop app is running — you can close this tab.";
      state.className = "state paired";
    }
    // Ask the background worker to re-assert the current domain so CHRONOS
    // sees a signal (and its status light turns green) immediately.
    try {
      chrome.runtime.sendMessage({ type: "chronos-reassert" }).catch(() => {});
    } catch { /* worker unavailable — next tab change reports anyway */ }
  });
} else if (state) {
  state.textContent =
    "Pairing unavailable — this page was not served by a running CHRONOS desktop app. Start CHRONOS and reopen the pairing page.";
}
