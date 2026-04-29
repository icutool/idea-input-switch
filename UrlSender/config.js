export const STORAGE_KEY = "urlSenderSettings";

export const DEFAULT_SETTINGS = {
  baseUrl: "http://127.0.0.1:5998",
  chinesePatterns: "",
  englishPatterns: ""
};

export async function getSettings() {
  const stored = await chrome.storage.local.get(STORAGE_KEY);
  return {
    ...DEFAULT_SETTINGS,
    ...(stored[STORAGE_KEY] || {})
  };
}

export async function saveSettings(settings) {
  await chrome.storage.local.set({
    [STORAGE_KEY]: settings
  });
}

export function splitPatternLines(input) {
  return input
    .split(/\r?\n/u)
    .map((line) => line.trim())
    .filter(Boolean);
}

export function normalizeBaseUrl(input) {
  return input.trim().replace(/\/+$/u, "");
}

export function compilePatternList(input) {
  const compiled = [];
  const invalid = [];

  splitPatternLines(input).forEach((pattern, index) => {
    try {
      compiled.push({
        line: index + 1,
        source: pattern,
        regex: new RegExp(pattern)
      });
    } catch (error) {
      invalid.push({
        line: index + 1,
        source: pattern,
        message: error instanceof Error ? error.message : String(error)
      });
    }
  });

  return { compiled, invalid };
}

export function validateSettings(rawSettings) {
  const baseUrl = normalizeBaseUrl(rawSettings.baseUrl || "");
  const chinesePatterns = rawSettings.chinesePatterns || "";
  const englishPatterns = rawSettings.englishPatterns || "";
  const errors = [];

  if (!baseUrl) {
    errors.push("后台地址不能为空。");
  } else {
    try {
      const parsed = new URL(baseUrl);
      if (!["http:", "https:"].includes(parsed.protocol)) {
        errors.push("后台地址只支持 http 或 https。");
      }
    } catch (error) {
      errors.push("后台地址格式不正确。");
    }
  }

  const chineseResult = compilePatternList(chinesePatterns);
  const englishResult = compilePatternList(englishPatterns);

  chineseResult.invalid.forEach((item) => {
    errors.push(`中文规则第 ${item.line} 行无效: ${item.source}`);
  });

  englishResult.invalid.forEach((item) => {
    errors.push(`英文规则第 ${item.line} 行无效: ${item.source}`);
  });

  return {
    ok: errors.length === 0,
    errors,
    settings: {
      baseUrl,
      chinesePatterns,
      englishPatterns
    }
  };
}

export function getModeForUrl(url, settings) {
  const chineseResult = compilePatternList(settings.chinesePatterns || "");
  const englishResult = compilePatternList(settings.englishPatterns || "");

  const chineseMatch = chineseResult.compiled.find((item) => item.regex.test(url));
  if (chineseMatch) {
    return {
      mode: 1,
      label: "CN",
      pattern: chineseMatch.source,
      invalidRules: [...chineseResult.invalid, ...englishResult.invalid]
    };
  }

  const englishMatch = englishResult.compiled.find((item) => item.regex.test(url));
  if (englishMatch) {
    return {
      mode: 0,
      label: "EN",
      pattern: englishMatch.source,
      invalidRules: [...chineseResult.invalid, ...englishResult.invalid]
    };
  }

  return {
    mode: null,
    label: "",
    pattern: "",
    invalidRules: [...chineseResult.invalid, ...englishResult.invalid]
  };
}
