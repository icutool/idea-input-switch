import { getSettings, saveSettings, validateSettings } from "./config.js";

const form = document.getElementById("settings-form");
const baseUrlInput = document.getElementById("baseUrl");
const chinesePatternsInput = document.getElementById("chinesePatterns");
const englishPatternsInput = document.getElementById("englishPatterns");
const statusElement = document.getElementById("status");
const ruleLabels = {
  keyword: "关键词",
  domain: "域名",
  prefix: "前缀",
  regex: "正则"
};
const rulePlaceholders = {
  keyword: "输入关键词",
  domain: "输入精准域名",
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

async function loadSettings() {
  const settings = await getSettings();
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
