import { getSettings, saveSettings, validateSettings } from "./config.js";

const form = document.getElementById("settings-form");
const baseUrlInput = document.getElementById("baseUrl");
const chinesePatternsInput = document.getElementById("chinesePatterns");
const englishPatternsInput = document.getElementById("englishPatterns");
const statusElement = document.getElementById("status");

void loadSettings();

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
