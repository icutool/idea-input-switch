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

function parseRuleLine(line) {
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
