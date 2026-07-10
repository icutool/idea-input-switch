export const STORAGE_KEY = "urlSenderSettings";

export const DEFAULT_SETTINGS = {
  baseUrl: "http://127.0.0.1:5998",
  chinesePatterns: "",
  englishPatterns: ""
};

export const RULE_CONFIG_VERSION = 1;

const RULE_TYPES = new Set(["keyword", "domain", "prefix", "regex"]);

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

export function parseRuleLine(line) {
  const prefixes = [
    ["keyword", ["关键词:", "关键字:", "包含:", "keyword:", "kw:"]],
    ["domain", ["域名:", "域名精准:", "精准域名:", "domain:", "host:"]],
    ["prefix", ["前缀:", "域名前缀:", "url前缀:", "prefix:", "url:"]],
    ["regex", ["正则:", "regex:", "regexp:", "re:"]]
  ];

  const lowerLine = line.toLowerCase();
  for (const [type, labels] of prefixes) {
    const label = labels.find((candidate) => lowerLine.startsWith(candidate.toLowerCase()));
    if (label) {
      return {
        type,
        value: line.slice(label.length).trim()
      };
    }
  }

  return {
    type: "regex",
    value: line
  };
}

export function formatRuleLine(rule) {
  const labels = {
    keyword: "关键词",
    domain: "域名",
    prefix: "前缀",
    regex: "正则"
  };
  const type = RULE_TYPES.has(rule.type) ? rule.type : "regex";
  return `${labels[type]}: ${String(rule.value || "").trim()}`;
}

function normalizeRuleList(rules, fieldName) {
  if (!Array.isArray(rules)) {
    throw new Error(`${fieldName} 必须是数组。`);
  }

  return rules.map((rule, index) => {
    if (typeof rule === "string") {
      const parsed = parseRuleLine(rule.trim());
      return {
        type: parsed.type,
        value: parsed.value
      };
    }

    if (!rule || typeof rule !== "object") {
      throw new Error(`${fieldName} 第 ${index + 1} 条规则格式不正确。`);
    }

    const type = String(rule.type || "").trim().toLowerCase();
    const value = String(rule.value || "").trim();

    if (!RULE_TYPES.has(type)) {
      throw new Error(`${fieldName} 第 ${index + 1} 条规则类型不支持: ${type || "<empty>"}。`);
    }

    if (!value) {
      throw new Error(`${fieldName} 第 ${index + 1} 条规则内容不能为空。`);
    }

    return { type, value };
  });
}

function patternTextToRules(input) {
  return splitPatternLines(input).map((line) => {
    const rule = parseRuleLine(line);
    return {
      type: rule.type,
      value: rule.value
    };
  });
}

function rulesToPatternText(rules) {
  return rules.map((rule) => formatRuleLine(rule)).join("\n");
}

export function settingsToRuleConfig(settings) {
  return {
    version: RULE_CONFIG_VERSION,
    baseUrl: normalizeBaseUrl(settings.baseUrl || DEFAULT_SETTINGS.baseUrl),
    rules: {
      chinese: patternTextToRules(settings.chinesePatterns || ""),
      english: patternTextToRules(settings.englishPatterns || "")
    }
  };
}

export function ruleConfigToSettings(config, fallbackSettings = DEFAULT_SETTINGS) {
  if (!config || typeof config !== "object") {
    throw new Error("配置文件必须是 JSON 对象。");
  }

  if (config.version !== undefined && Number(config.version) !== RULE_CONFIG_VERSION) {
    throw new Error(`配置文件版本不支持: ${config.version}。`);
  }

  const rules = config.rules;
  if (!rules || typeof rules !== "object") {
    throw new Error("配置文件缺少 rules 对象。");
  }

  const chineseRules = normalizeRuleList(rules.chinese || [], "rules.chinese");
  const englishRules = normalizeRuleList(rules.english || [], "rules.english");

  const settings = {
    baseUrl: config.baseUrl ? normalizeBaseUrl(String(config.baseUrl)) : fallbackSettings.baseUrl,
    chinesePatterns: rulesToPatternText(chineseRules),
    englishPatterns: rulesToPatternText(englishRules)
  };
  const validation = validateSettings(settings);

  if (!validation.ok) {
    throw new Error(validation.errors.join("\n"));
  }

  return validation.settings;
}

function safeDecodeUrl(url) {
  try {
    return decodeURIComponent(url);
  } catch (error) {
    return url;
  }
}

function buildKeywordMatcher(value) {
  if (!value) {
    throw new Error("关键词不能为空");
  }

  const needle = value.toLowerCase();
  return (url) => {
    const rawUrl = url.toLowerCase();
    const decodedUrl = safeDecodeUrl(url).toLowerCase();
    return rawUrl.includes(needle) || decodedUrl.includes(needle);
  };
}

function normalizeRulePath(pathname) {
  const decoded = safeDecodeUrl(pathname);
  if (!decoded || decoded === "/") {
    return "/";
  }

  return decoded.replace(/\/+$/u, "") || "/";
}

function pathMatchesPrefix(targetPathname, rulePathname) {
  const targetPath = normalizeRulePath(targetPathname);
  const rulePath = normalizeRulePath(rulePathname);

  if (rulePath === "/") {
    return true;
  }

  return targetPath === rulePath || targetPath.startsWith(`${rulePath}/`);
}

function buildPrefixMatcher(value) {
  if (!value) {
    throw new Error("前缀不能为空");
  }

  const hasExplicitProtocol = /^[a-z][a-z0-9+.-]*:\/\//iu.test(value);
  const parsedRule = new URL(hasExplicitProtocol ? value : `https://${value}`);
  const rule = {
    protocol: hasExplicitProtocol ? parsedRule.protocol : "",
    host: parsedRule.host.toLowerCase(),
    pathname: parsedRule.pathname
  };

  return (url) => {
    let parsedUrl;
    try {
      parsedUrl = new URL(url);
    } catch (error) {
      return false;
    }

    if (rule.protocol && parsedUrl.protocol !== rule.protocol) {
      return false;
    }

    return (
      parsedUrl.host.toLowerCase() === rule.host &&
      pathMatchesPrefix(parsedUrl.pathname, rule.pathname)
    );
  };
}

function buildDomainMatcher(value) {
  if (!value) {
    throw new Error("域名不能为空");
  }

  const hasExplicitProtocol = /^[a-z][a-z0-9+.-]*:\/\//iu.test(value);
  const parsedRule = new URL(hasExplicitProtocol ? value : `https://${value}`);
  const hostname = parsedRule.hostname.toLowerCase();

  return (url) => {
    try {
      return new URL(url).hostname.toLowerCase() === hostname;
    } catch (error) {
      return false;
    }
  };
}

function buildRegexMatcher(value) {
  if (!value) {
    throw new Error("正则不能为空");
  }

  const regex = new RegExp(value);
  return (url) => {
    regex.lastIndex = 0;
    return regex.test(url);
  };
}

function buildRuleMatcher(type, value) {
  if (type === "keyword") {
    return buildKeywordMatcher(value);
  }

  if (type === "prefix") {
    return buildPrefixMatcher(value);
  }

  if (type === "domain") {
    return buildDomainMatcher(value);
  }

  return buildRegexMatcher(value);
}

export function compilePatternList(input) {
  const compiled = [];
  const invalid = [];

  splitPatternLines(input).forEach((line, index) => {
    const rule = parseRuleLine(line);

    try {
      compiled.push({
        line: index + 1,
        source: line,
        type: rule.type,
        value: rule.value,
        matcher: buildRuleMatcher(rule.type, rule.value)
      });
    } catch (error) {
      invalid.push({
        line: index + 1,
        source: line,
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

  const chineseMatch = chineseResult.compiled.find((item) => item.matcher(url));
  if (chineseMatch) {
    return {
      mode: 1,
      label: "CN",
      pattern: chineseMatch.source,
      invalidRules: [...chineseResult.invalid, ...englishResult.invalid]
    };
  }

  const englishMatch = englishResult.compiled.find((item) => item.matcher(url));
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
