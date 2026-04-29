import { getModeForUrl, getSettings } from "./config.js";

const lastProcessedUrlByTab = new Map();

chrome.runtime.onInstalled.addListener(async () => {
  await chrome.action.setBadgeBackgroundColor({ color: "#2563eb" });
});

chrome.webNavigation.onCommitted.addListener((details) => {
  void handleNavigation(details);
});

chrome.webNavigation.onHistoryStateUpdated.addListener((details) => {
  void handleNavigation(details);
});

chrome.tabs.onRemoved.addListener((tabId) => {
  lastProcessedUrlByTab.delete(tabId);
});

async function handleNavigation(details) {
  if (details.frameId !== 0 || !details.url || !isHttpUrl(details.url)) {
    return;
  }

  const previousUrl = lastProcessedUrlByTab.get(details.tabId);
  if (previousUrl === details.url) {
    return;
  }

  lastProcessedUrlByTab.set(details.tabId, details.url);

  const settings = await getSettings();
  const decision = getModeForUrl(details.url, settings);

  if (decision.invalidRules.length > 0) {
    console.warn("[UrlSender] Some regex rules are invalid and were skipped.", decision.invalidRules);
  }

  if (decision.mode === null) {
    console.log(`[UrlSender] No rule matched for ${details.url}`);
    await clearBadge(details.tabId);
    return;
  }

  try {
    const requestUrl = `${settings.baseUrl}/switch?mode=${decision.mode}`;
    const response = await fetch(requestUrl, {
      method: "GET",
      cache: "no-store"
    });

    if (!response.ok) {
      throw new Error(`HTTP ${response.status}`);
    }

    console.log(
      `[UrlSender] Matched ${decision.label} for ${details.url} with rule "${decision.pattern}", called ${requestUrl}`
    );
    await flashBadge(details.tabId, decision.label, "#16a34a");
  } catch (error) {
    console.error(`[UrlSender] Failed to switch mode for ${details.url}`, error);
    await flashBadge(details.tabId, "ERR", "#dc2626");
  }
}

function isHttpUrl(url) {
  try {
    const parsed = new URL(url);
    return parsed.protocol === "http:" || parsed.protocol === "https:";
  } catch (error) {
    return false;
  }
}

async function flashBadge(tabId, text, color) {
  await chrome.action.setBadgeBackgroundColor({ color, tabId });
  await chrome.action.setBadgeText({ text, tabId });
  setTimeout(() => {
    void clearBadge(tabId);
  }, 1800);
}

async function clearBadge(tabId) {
  try {
    await chrome.action.setBadgeText({ text: "", tabId });
  } catch (error) {
    console.debug("[UrlSender] Failed to clear badge.", error);
  }
}
