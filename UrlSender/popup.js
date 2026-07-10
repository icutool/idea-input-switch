import {
  getSettings,
  ruleConfigToSettings,
  saveSettings,
  settingsToRuleConfig,
  validateSettings
} from "./config.js";

const LOG_STORAGE_KEY = "urlSenderLogs";
const LOG_ENABLED_STORAGE_KEY = "urlSenderLogEnabled";
const form = document.getElementById("settings-form");
const baseUrlInput = document.getElementById("baseUrl");
const chinesePatternsInput = document.getElementById("chinesePatterns");
const englishPatternsInput = document.getElementById("englishPatterns");
const remoteConfigUrlInput = document.getElementById("remoteConfigUrl");
const importRemoteConfigButton = document.getElementById("importRemoteConfig");
const localConfigFileInput = document.getElementById("localConfigFile");
const exportConfigButton = document.getElementById("exportConfig");
const statusElement = document.getElementById("status");
const refreshLogsButton = document.getElementById("refreshLogs");
const clearLogsButton = document.getElementById("clearLogs");
const logEnabledInput = document.getElementById("logEnabled");
const logSwitchHintElement = document.getElementById("logSwitchHint");
const urlLogsElement = document.getElementById("urlLogs");
const tabButtons = Array.from(document.querySelectorAll("[data-tab]"));
const tabPanels = Array.from(document.querySelectorAll("[data-tab-panel]"));
const settingsActions = document.querySelector("[data-settings-actions]");
const ruleLabels = {
  keyword: "关键词",
  exact: "精准匹配",
  prefix: "前缀",
  regex: "正则"
};
const rulePlaceholders = {
  keyword: "输入关键词",
  exact: "输入完整 URL",
  prefix: "输入网址前缀",
  regex: "输入正则"
};
const ruleBuilders = [
  {
    select: document.getElementById("chineseRuleType"),
    input: document.getElementById("chineseRuleValue"),
    button: document.getElementById("addChineseRule"),
    textarea: chinesePatternsInput
  },
  {
    select: document.getElementById("englishRuleType"),
    input: document.getElementById("englishRuleValue"),
    button: document.getElementById("addEnglishRule"),
    textarea: englishPatternsInput
  }
];

void loadSettings();
void loadLogs();
void loadLogSwitch();

tabButtons.forEach((button) => {
  button.addEventListener("click", () => {
    activateTab(button.dataset.tab);
  });
});

ruleBuilders.forEach((builder) => {
  updateRuleInputPlaceholder(builder);

  builder.select.addEventListener("change", () => {
    updateRuleInputPlaceholder(builder);
  });

  builder.button.addEventListener("click", () => {
    addRuleLine(builder);
  });

  builder.input.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      addRuleLine(builder);
    }
  });
});

form.addEventListener("submit", async (event) => {
  event.preventDefault();

  const validation = validateSettings({
    baseUrl: baseUrlInput.value,
    chinesePatterns: chinesePatternsInput.value,
    englishPatterns: englishPatternsInput.value
  });

  if (!validation.ok) {
    renderStatus(validation.errors.join("\n"), "error");
    return;
  }

  await saveSettings(validation.settings);
  renderStatus("配置已保存。", "success");
});

importRemoteConfigButton.addEventListener("click", async () => {
  const url = remoteConfigUrlInput.value.trim();
  if (!url) {
    renderStatus("远程 JSON 地址不能为空。", "error");
    return;
  }

  try {
    const response = await fetch(url, {
      method: "GET",
      cache: "no-store"
    });

    if (!response.ok) {
      throw new Error(`HTTP ${response.status}`);
    }

    const config = await response.json();
    await importRuleConfig(config);
    renderStatus("远程规则已导入并保存。", "success");
  } catch (error) {
    renderStatus(`远程导入失败: ${formatError(error)}`, "error");
  }
});

localConfigFileInput.addEventListener("change", async () => {
  const file = localConfigFileInput.files?.[0];
  if (!file) {
    return;
  }

  try {
    const config = JSON.parse(await file.text());
    await importRuleConfig(config);
    renderStatus("本地规则已导入并保存。", "success");
  } catch (error) {
    renderStatus(`本地导入失败: ${formatError(error)}`, "error");
  } finally {
    localConfigFileInput.value = "";
  }
});

exportConfigButton.addEventListener("click", () => {
  const validation = validateSettings(readFormSettings());

  if (!validation.ok) {
    renderStatus(validation.errors.join("\n"), "error");
    return;
  }

  const config = settingsToRuleConfig(validation.settings);
  downloadJson(config, "idea-input-switch-config.json");
  renderStatus("规则已导出。", "success");
});

refreshLogsButton.addEventListener("click", () => {
  void loadLogs();
});

clearLogsButton.addEventListener("click", async () => {
  await chrome.storage.local.set({ [LOG_STORAGE_KEY]: [] });
  renderLogs([]);
  renderStatus("URL 日志已清空。", "success");
});

logEnabledInput.addEventListener("change", async () => {
  const enabled = logEnabledInput.checked;
  await chrome.storage.local.set({ [LOG_ENABLED_STORAGE_KEY]: enabled });
  renderLogSwitch(enabled);
});

chrome.storage.onChanged.addListener((changes, areaName) => {
  if (areaName === "local" && changes[LOG_STORAGE_KEY]) {
    renderLogs(changes[LOG_STORAGE_KEY].newValue || []);
  }
  if (areaName === "local" && changes[LOG_ENABLED_STORAGE_KEY]) {
    renderLogSwitch(changes[LOG_ENABLED_STORAGE_KEY].newValue === true);
  }
});

function activateTab(tabName) {
  tabButtons.forEach((button) => {
    const active = button.dataset.tab === tabName;
    button.classList.toggle("active", active);
    button.setAttribute("aria-selected", String(active));
  });

  tabPanels.forEach((panel) => {
    panel.classList.toggle("active", panel.dataset.tabPanel === tabName);
  });

  settingsActions.classList.toggle("hidden", tabName === "logs");

  if (tabName === "logs") {
    void loadLogs();
  }
}

async function loadSettings() {
  const settings = await getSettings();
  renderSettings(settings);
}

function renderSettings(settings) {
  baseUrlInput.value = settings.baseUrl;
  chinesePatternsInput.value = settings.chinesePatterns;
  englishPatternsInput.value = settings.englishPatterns;
}

function renderStatus(message, type) {
  statusElement.textContent = message;
  statusElement.className = `status ${type}`;
}

async function loadLogs() {
  const stored = await chrome.storage.local.get(LOG_STORAGE_KEY);
  renderLogs(Array.isArray(stored[LOG_STORAGE_KEY]) ? stored[LOG_STORAGE_KEY] : []);
}

async function loadLogSwitch() {
  const stored = await chrome.storage.local.get(LOG_ENABLED_STORAGE_KEY);
  renderLogSwitch(stored[LOG_ENABLED_STORAGE_KEY] === true);
}

function renderLogSwitch(enabled) {
  logEnabledInput.checked = enabled;
  logSwitchHintElement.textContent = enabled
    ? "日志采集中。关闭后会停止新增记录，已有日志会保留。"
    : "日志采集已关闭。开启后才会记录新的 URL。";
}

function renderLogs(logs) {
  urlLogsElement.replaceChildren();

  if (logs.length === 0) {
    const empty = document.createElement("p");
    empty.className = "log-empty";
    empty.textContent = "暂无 URL 日志。访问或切换网页后会出现在这里。";
    urlLogsElement.append(empty);
    return;
  }

  logs.slice(0, 80).forEach((log) => {
    const item = document.createElement("article");
    item.className = `log-item ${log.status || "unknown"}`;

    const meta = document.createElement("div");
    meta.className = "log-meta";

    const badge = document.createElement("span");
    badge.className = "log-badge";
    badge.textContent = statusLabel(log.status);

    const time = document.createElement("span");
    time.textContent = formatLogTime(log.time);

    const source = document.createElement("span");
    source.textContent = `${log.trigger || "unknown"} · tab ${log.tabId ?? "-"} · frame ${log.frameId ?? "-"}`;

    meta.append(badge, time, source);

    const url = document.createElement("div");
    url.className = "log-url";
    url.textContent = log.url || "<empty>";

    const message = document.createElement("div");
    message.className = "log-message";
    message.textContent = buildLogMessage(log);

    item.append(meta, url, message);
    urlLogsElement.append(item);
  });
}

function statusLabel(status) {
  const labels = {
    match: "命中",
    none: "未命中",
    skip: "跳过",
    error: "错误"
  };
  return labels[status] || "日志";
}

function buildLogMessage(log) {
  const parts = [log.message || "URL 事件"];
  if (log.rule) {
    parts.push(`规则: ${log.rule}`);
  }
  if (log.requestUrl) {
    parts.push(`请求: ${log.requestUrl}`);
  }
  return parts.join(" · ");
}

function formatLogTime(value) {
  if (!value) {
    return "--:--:--";
  }

  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }

  return date.toLocaleTimeString("zh-CN", {
    hour12: false,
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit"
  });
}

function updateRuleInputPlaceholder(builder) {
  builder.input.placeholder = rulePlaceholders[builder.select.value] || "输入规则";
}

function addRuleLine(builder) {
  const value = builder.input.value.trim();
  if (!value) {
    renderStatus("规则内容不能为空。", "error");
    return;
  }

  const label = ruleLabels[builder.select.value] || "正则";
  const line = `${label}: ${value}`;
  const current = builder.textarea.value.trimEnd();
  builder.textarea.value = current ? `${current}\n${line}` : line;
  builder.input.value = "";
  builder.input.focus();
  renderStatus("规则已添加，保存后生效。", "success");
}

function readFormSettings() {
  return {
    baseUrl: baseUrlInput.value,
    chinesePatterns: chinesePatternsInput.value,
    englishPatterns: englishPatternsInput.value
  };
}

async function importRuleConfig(config) {
  const currentSettings = await getSettings();
  const settings = ruleConfigToSettings(config, currentSettings);
  await saveSettings(settings);
  renderSettings(settings);
}

function downloadJson(data, fileName) {
  const blob = new Blob([`${JSON.stringify(data, null, 2)}\n`], {
    type: "application/json;charset=utf-8"
  });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = fileName;
  document.body.append(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
}

function formatError(error) {
  return error instanceof Error ? error.message : String(error);
}
