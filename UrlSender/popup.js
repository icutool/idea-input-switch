import {
  getSettings,
  ruleConfigToSettings,
  saveSettings,
  settingsToRuleConfig,
  validateSettings
} from "./config.js";

const form = document.getElementById("settings-form");
const baseUrlInput = document.getElementById("baseUrl");
const chinesePatternsInput = document.getElementById("chinesePatterns");
const englishPatternsInput = document.getElementById("englishPatterns");
const remoteConfigUrlInput = document.getElementById("remoteConfigUrl");
const importRemoteConfigButton = document.getElementById("importRemoteConfig");
const localConfigFileInput = document.getElementById("localConfigFile");
const exportConfigButton = document.getElementById("exportConfig");
const statusElement = document.getElementById("status");
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
