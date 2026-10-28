// CHRONOS browser extension — optional website attribution.
//
// Sends only the active tab's domain (never URLs, titles, or page content) to the local
// CHRONOS desktop app at http://127.0.0.1:53711. Requests include the pairing token
// stored in extension-local storage — paired automatically via the local pairing page,
// no manual copying. If the desktop app is not running, or the token does not match,
// nothing happens and nothing is stored.

const ENDPOINT = "http://127.0.0.1:53711/domain";
const PAIR_URL = "http://127.0.0.1:53711/pair";
const BLOCKLIST_URL = "http://127.0.0.1:53711/blocklist";
const BLOCKLIST_ACK_URL = "http://127.0.0.1:53711/blocklist-ack";

// CHRONOS dynamic DNR rules live in this id namespace so we never touch
// rules belonging to anything else.
const BLOCK_RULE_BASE = 900000000;

async function getToken() {
  const { chronosToken = "" } = await chrome.storage.local.get("chronosToken");
  return chronosToken;
}

async function report(domain) {
  const token = await getToken();
  if (!token || !domain) return;
  try {
    await fetch(ENDPOINT, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ token, domain }),
    });
  } catch {
    // Desktop app not running — stay silent.
  }
}

// Common multipart public suffixes (bounded, embedded — no network lookups).
const MULTIPART_SUFFIXES = new Set([
  "co.uk", "org.uk", "ac.uk", "gov.uk", "me.uk", "net.uk",
  "com.au", "net.au", "org.au", "edu.au", "gov.au",
  "co.nz", "net.nz", "org.nz", "govt.nz",
  "co.jp", "or.jp", "ne.jp", "ac.jp", "go.jp",
  "com.br", "net.br", "org.br", "gov.br",
  "com.mx", "org.mx", "net.mx",
  "co.za", "org.za", "net.za", "web.za",
  "com.sg", "org.sg", "net.sg", "edu.sg",
  "co.in", "net.in", "org.in", "firm.in",
  "com.cn", "net.cn", "org.cn", "gov.cn",
  "com.hk", "org.hk", "net.hk",
  "com.tw", "org.tw", "net.tw",
  "com.ar", "net.ar", "org.ar",
  "com.tr", "org.tr", "net.tr",
  "com.pl", "org.pl", "net.pl",
  "co.il", "org.il", "net.il", "ac.il",
  "com.ua", "net.ua", "org.ua",
]);

function registrarDomain(hostname) {
  const parts = hostname.replace(/^www\./i, "").split(".").filter(Boolean);
  if (parts.length <= 2) return parts.join(".");
  const lastTwo = parts.slice(-2).join(".");
  if (MULTIPART_SUFFIXES.has(lastTwo) && parts.length >= 3) {
    return parts.slice(-3).join(".");
  }
  return lastTwo;
}

let lastDomain = null;

async function onTabChange() {
  const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
  const domain = tab?.url ? registrarDomain(new URL(tab.url).hostname) : null;
  if (domain && domain !== lastDomain) {
    lastDomain = domain;
    report(domain);
  }
  // Any real browsing is a good moment to catch up on block-list changes.
  syncBlocking();
}

chrome.tabs.onActivated.addListener(onTabChange);
chrome.tabs.onUpdated.addListener((tabId, info) => {
  if (info.status === "complete") onTabChange();
});
chrome.windows.onFocusChanged.addListener(onTabChange);
// Re-assert the current domain when the browser starts or the extension is
// (re)loaded/enabled, so CHRONOS gets a fresh hint without waiting for a tab switch.
chrome.runtime.onStartup.addListener(onTabChange);
chrome.runtime.onInstalled.addListener(async (details) => {
  onTabChange();
  // First-time pairing: open the local pairing page right after the user approves
  // installation. Its content script stores the token — no manual copying.
  if (details.reason === "install" && !(await getToken())) {
    chrome.tabs.create({ url: PAIR_URL });
  }
});

// Toolbar icon: finish pairing (opens the local pairing page) or show options.
chrome.action.onClicked.addListener(async () => {
  if (!(await getToken())) chrome.tabs.create({ url: PAIR_URL });
  else chrome.runtime.openOptionsPage();
});

// The pairing content script asks for an immediate hint re-assert after pairing.
chrome.runtime.onMessage.addListener((msg) => {
  if (msg && msg.type === "chronos-reassert") onTabChange();
});

/* ---- optional website blocking (CHRONOS → distraction blocking) ---- */
//
// The block list is owned by the desktop app. The extension mirrors it as
// declarativeNetRequest redirect rules: a blocked site's pages are redirected
// to a local extension page instead of loading. Rules apply to main-frame
// navigation only, are re-synced at most once a minute, and disappear entirely
// (including on extension update or removal) the moment the list is empty or
// blocking is off. If CHRONOS is not running, existing rules stay as-is; a
// fetch failure never silently clears or adds rules.

let lastBlocklistSyncMs = 0;

async function syncBlocking(force = false) {
  const now = Date.now();
  if (!force && now - lastBlocklistSyncMs < 50_000) return;
  lastBlocklistSyncMs = now;
  const token = await getToken();
  if (!token) return;
  try {
    const res = await fetch(`${BLOCKLIST_URL}?token=${encodeURIComponent(token)}`);
    if (!res.ok) return;
    const data = await res.json();
    const domains = Array.isArray(data?.domains) ? data.domains.filter((d) => typeof d === "string" && d.length > 0) : [];
    let applied = 0;
    if (data?.enabled) {
      await applyBlockRules(domains);
      applied = domains.length;
    } else {
      await applyBlockRules([]);
    }
    // Acknowledge back to the desktop app: this is the only reliable signal
    // that enforcement is real (not merely configured). Also serves as a
    // heartbeat while the browser runs.
    try {
      await fetch(BLOCKLIST_ACK_URL, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ token, applied, enabled: !!data?.enabled, version: chrome.runtime.getManifest().version }),
      });
    } catch {
      // Ack is best-effort; the next sync retries it.
    }
  } catch {
    // Desktop app not running — keep current rules, stay silent.
  }
}

async function applyBlockRules(domains) {
  const existing = await chrome.declarativeNetRequest.getDynamicRules();
  const ours = existing.filter((r) => r.id >= BLOCK_RULE_BASE).map((r) => r.id);
  const add = domains.slice(0, 500).map((d, i) => ({
    id: BLOCK_RULE_BASE + i,
    priority: 1,
    action: { type: "redirect", redirect: { extensionPath: "/blocked.html" } },
    condition: { requestDomains: [d.toLowerCase()], resourceTypes: ["main_frame"] },
  }));
  await chrome.declarativeNetRequest.updateDynamicRules({ removeRuleIds: ours, addRules: add });
}

syncBlocking(true);
chrome.runtime.onStartup.addListener(() => syncBlocking(true));
chrome.runtime.onInstalled.addListener(() => syncBlocking(true));
// Service workers suspend when idle, which freezes setInterval; the alarms API
// is the reliable periodic wake-up in Manifest V3.
chrome.alarms.create("chronos-blocklist-sync", { periodInMinutes: 1 });
chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === "chronos-blocklist-sync") syncBlocking();
});
