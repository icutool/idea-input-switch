import { getModeForUrl, getSettings } from "./config.js";

const lastProcessedEventByTab = new Map();
const LOG_STORAGE_KEY = "urlSenderLogs";
const MAX_LOG_ENTRIES = 200;

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
  logUrlEvent(event);

  if (event.frameId !== 0) {
    console.debug(
      `[UrlSender] Skip non-main-frame URL: ${event.url || "<empty>"} (${event.trigger}, tab=${event.tabId}, frame=${event.frameId})`
    );
    await appendUrlLog(event, {
      status: "skip",
      message: "跳过子 frame"
    });
    return;
  }

  if (!event.url) {
    console.debug(`[UrlSender] Skip empty URL (${event.trigger}, tab=${event.tabId})`);
    await appendUrlLog(event, {
      status: "skip",
      message: "跳过空 URL"
    });
    return;
  }

  if (!isHttpUrl(event.url)) {
    console.debug(`[UrlSender] Skip non-http URL: ${event.url} (${event.trigger}, tab=${event.tabId})`);
    await appendUrlLog(event, {
      status: "skip",
      message: "跳过非 http/https URL"
    });
    return;
  }

  const previousEventKey = lastProcessedEventByTab.get(event.tabId);
  if (previousEventKey === event.eventKey) {
    console.debug(`[UrlSender] Skip duplicate URL event: ${event.url} (${event.trigger}, tab=${event.tabId})`);
    await appendUrlLog(event, {
      status: "skip",
      message: "跳过重复事件"
    });
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
    await appendUrlLog(event, {
      status: "none",
      message: "未命中规则"
    });
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
    await appendUrlLog(event, {
      status: "match",
      message: `命中 ${decision.label}`,
      mode: decision.label,
      rule: decision.pattern,
      requestUrl
    });
    await flashBadge(event.tabId, decision.label, "#16a34a");
  } catch (error) {
    console.error(`[UrlSender] Failed to switch mode for ${event.url} (${event.trigger})`, error);
    await appendUrlLog(event, {
      status: "error",
      message: `切换失败: ${formatError(error)}`
    });
    await flashBadge(event.tabId, "ERR", "#dc2626");
  }
}

function logUrlEvent(event) {
  console.log(
    `[UrlSender] URL ${event.trigger}: ${event.url || "<empty>"} (tab=${event.tabId}, frame=${event.frameId}, event=${event.eventKey})`
  );
}

async function appendUrlLog(event, detail) {
  const entry = {
    id: `${Date.now()}:${Math.random().toString(16).slice(2)}`,
    time: new Date().toISOString(),
    trigger: event.trigger,
    tabId: event.tabId,
    frameId: event.frameId,
    url: event.url || "",
    eventKey: event.eventKey,
    status: detail.status,
    message: detail.message,
    mode: detail.mode || "",
    rule: detail.rule || "",
    requestUrl: detail.requestUrl || ""
  };

  try {
    const stored = await chrome.storage.local.get(LOG_STORAGE_KEY);
    const logs = Array.isArray(stored[LOG_STORAGE_KEY]) ? stored[LOG_STORAGE_KEY] : [];
    logs.unshift(entry);
    await chrome.storage.local.set({
      [LOG_STORAGE_KEY]: logs.slice(0, MAX_LOG_ENTRIES)
    });
  } catch (error) {
    console.debug("[UrlSender] Failed to append URL log.", error);
  }
}

function formatError(error) {
  return error instanceof Error ? error.message : String(error);
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
