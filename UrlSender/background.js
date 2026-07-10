import { getModeForUrl, getSettings } from "./config.js";

const lastProcessedEventByTab = new Map();

chrome.runtime.onInstalled.addListener(async () => {
  await chrome.action.setBadgeBackgroundColor({ color: "#2563eb" });
});

chrome.webNavigation.onCommitted.addListener((details) => {
  void handleNavigation({
    frameId: details.frameId,
    tabId: details.tabId,
    trigger: "navigation",
    url: details.url,
    eventKey: `committed:${details.documentId ?? details.timeStamp ?? details.url}`
  });
});

chrome.webNavigation.onHistoryStateUpdated.addListener((details) => {
  void handleNavigation({
    frameId: details.frameId,
    tabId: details.tabId,
    trigger: "history",
    url: details.url,
    eventKey: `history:${details.documentId ?? details.timeStamp ?? details.url}`
  });
});

chrome.tabs.onActivated.addListener((activeInfo) => {
  void handleTabActivated(activeInfo.tabId);
});

chrome.tabs.onRemoved.addListener((tabId) => {
  lastProcessedEventByTab.delete(tabId);
});

async function handleTabActivated(tabId) {
  try {
    const tab = await chrome.tabs.get(tabId);

    await handleNavigation({
      frameId: 0,
      tabId,
      trigger: "activation",
      url: tab.url,
      eventKey: `activated:${Date.now()}`
    });
  } catch (error) {
    console.debug(`[UrlSender] Failed to inspect activated tab ${tabId}.`, error);
  }
}

async function handleNavigation(event) {
  if (event.frameId !== 0 || !event.url || !isHttpUrl(event.url)) {
    return;
  }

  const previousEventKey = lastProcessedEventByTab.get(event.tabId);
  if (previousEventKey === event.eventKey) {
    return;
  }

  lastProcessedEventByTab.set(event.tabId, event.eventKey);

  const settings = await getSettings();
  const decision = getModeForUrl(event.url, settings);

  if (decision.invalidRules.length > 0) {
    console.warn("[UrlSender] Some rules are invalid and were skipped.", decision.invalidRules);
  }

  if (decision.mode === null) {
    console.log(`[UrlSender] No rule matched for ${event.url} (${event.trigger})`);
    await clearBadge(event.tabId);
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
      `[UrlSender] Matched ${decision.label} for ${event.url} via ${event.trigger} with rule "${decision.pattern}", called ${requestUrl}`
    );
    await flashBadge(event.tabId, decision.label, "#16a34a");
  } catch (error) {
    console.error(`[UrlSender] Failed to switch mode for ${event.url} (${event.trigger})`, error);
    await flashBadge(event.tabId, "ERR", "#dc2626");
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
