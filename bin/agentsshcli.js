#!/usr/bin/env node
import { createRequire } from "node:module";
import { spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import readline from "node:readline/promises";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const currentDir = path.dirname(fileURLToPath(import.meta.url));
const projectRoot = path.resolve(currentDir, "..");
const homeDir = os.homedir();
const packageVersion = require(path.join(projectRoot, "package.json")).version;
const ccSwitchSkillsDir = path.join(homeDir, ".cc-switch", "skills");
const LOG_SKILL_NAME = "log-analyze";
const ENV_MAP_JSON_FILENAME = "env-map.json";
const ENV_MAP_MD_FILENAME = "env-map.md";
const DEFAULT_CONFIG_PATH = path.join(homeDir, ".agent-ssh-cli", "config.json");
const ENV_MAP_SCHEMA_VERSION = 1;
const MANAGED_START_MARKER = "<!-- agentsshcli:managed:start -->";
const MANAGED_END_MARKER = "<!-- agentsshcli:managed:end -->";
const LOCAL_START_MARKER = "<!-- agentsshcli:local:start -->";
const LOCAL_END_MARKER = "<!-- agentsshcli:local:end -->";
const INSTALL_PRESETS = {
  codex: {
    clients: ["codex"],
    primaryClient: "codex",
    linkSecondary: true
  },
  claude: {
    clients: ["claude"],
    primaryClient: "claude",
    linkSecondary: true
  },
  hermes: {
    clients: ["hermes"],
    primaryClient: "hermes",
    linkSecondary: true
  },
  opencode: {
    clients: ["opencode"],
    primaryClient: "opencode",
    linkSecondary: true
  },
  "cc-switch": {
    clients: ["cc-switch"],
    primaryClient: "cc-switch",
    linkSecondary: true
  },
  "cc-switch-codex": {
    clients: ["cc-switch", "codex"],
    primaryClient: "cc-switch",
    linkSecondary: true
  },
  "cc-switch-claude": {
    clients: ["cc-switch", "claude"],
    primaryClient: "cc-switch",
    linkSecondary: true
  },
  "cc-switch-hermes": {
    clients: ["cc-switch", "hermes"],
    primaryClient: "cc-switch",
    linkSecondary: true
  }
};
const KNOWN_CLIENTS = {
  "cc-switch": {
    label: "CC Switch",
    skillsDir: ccSwitchSkillsDir,
    isRuntimeClient: false
  },
  codex: {
    label: "Codex",
    skillsDir: path.join(homeDir, ".codex", "skills"),
    isRuntimeClient: true
  },
  claude: {
    label: "Claude Code",
    skillsDir: path.join(homeDir, ".claude", "skills"),
    isRuntimeClient: true
  },
  opencode: {
    label: "OpenCode",
    skillsDir: path.join(homeDir, ".config", "opencode", "skills"),
    isRuntimeClient: true
  },
  hermes: {
    label: "Hermes",
    skillsDir: path.join(homeDir, ".hermes", "skills"),
    isRuntimeClient: true
  }
};

function hasCcSwitchInstalled() {
  return fs.existsSync(ccSwitchSkillsDir);
}

function getDefaultClientSelection() {
  return hasCcSwitchInstalled() ? ["cc-switch", "codex"] : ["codex"];
}

function getDefaultPrimaryClient(selectedClients) {
  if (selectedClients.includes("cc-switch")) {
    return "cc-switch";
  }
  return selectedClients[0];
}

function expandHome(value) {
  if (!value) {
    return value;
  }
  if (value === "~") {
    return homeDir;
  }
  if (value.startsWith("~/")) {
    return path.join(homeDir, value.slice(2));
  }
  return value;
}

function normalizeClientName(value) {
  const normalized = String(value || "")
    .trim()
    .toLowerCase()
    .replace(/[\s_-]+/g, "");

  if (normalized === "claude" || normalized === "claudecode") {
    return "claude";
  }
  if (normalized === "ccswitch") {
    return "cc-switch";
  }
  if (normalized === "codex") {
    return "codex";
  }
  if (normalized === "opencode" || normalized === "opencode") {
    return "opencode";
  }
  if (normalized === "hermes") {
    return "hermes";
  }
  if (normalized === "custom" || normalized === "customclient") {
    return "custom";
  }
  return undefined;
}

function parseClientList(rawValue) {
  return String(rawValue || "")
    .split(/[,\s]+/)
    .map((item) => normalizeClientName(item))
    .filter(Boolean);
}

function unique(items) {
  return [...new Set(items)];
}

function formatClientOption(name, overrides = new Map()) {
  if (name === "custom") {
    const customDir = overrides.get("custom");
    return `- custom: 自定义客户端 -> ${customDir ? expandHome(customDir) : "<安装时手动输入完整 skills 根目录>"}`;
  }
  const client = KNOWN_CLIENTS[name];
  const skillsDir = expandHome(overrides.get(name) || client.skillsDir);
  const exists = fs.existsSync(skillsDir) ? "已存在" : "将创建";
  return `- ${name}: ${client.label} -> ${skillsDir} (${exists})`;
}

function parseClientRootArg(rawValue) {
  const index = String(rawValue || "").indexOf("=");
  if (index <= 0 || index === rawValue.length - 1) {
    throw new Error(`--client-root 参数格式无效: ${rawValue}`);
  }

  const clientName = normalizeClientName(rawValue.slice(0, index));
  if (!clientName) {
    throw new Error(`未知客户端: ${rawValue.slice(0, index)}`);
  }

  return {
    clientName,
    skillsDir: expandHome(rawValue.slice(index + 1))
  };
}

function timestampForBackup() {
  return new Date().toISOString().replace(/[-:.TZ]/g, "").slice(0, 14);
}

function ensureDir(dirPath) {
  fs.mkdirSync(dirPath, { recursive: true });
}

function normalizeNewlines(value) {
  return String(value || "").replace(/\r\n?/g, "\n");
}

function readJsonFile(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function writePrettyJson(targetPath, value) {
  fs.writeFileSync(targetPath, `${JSON.stringify(value, null, 2)}\n`, "utf8");
}

function computeSha256(value) {
  return crypto.createHash("sha256").update(normalizeNewlines(value), "utf8").digest("hex");
}

function normalizeStringArray(values) {
  return unique(
    (Array.isArray(values) ? values : [])
      .map((item) => String(item || "").trim())
      .filter(Boolean)
  );
}

function inferEnvironmentFromConnectionName(connectionName) {
  const normalized = String(connectionName || "").trim().toLowerCase();
  if (!normalized) {
    return "";
  }
  if (normalized.startsWith("prod") || normalized.includes(".prod")) {
    return "prod";
  }
  if (
    normalized.startsWith("yfb") ||
    normalized.startsWith("pre") ||
    normalized.includes("preprod") ||
    normalized.includes("staging")
  ) {
    return "yfb";
  }
  if (normalized.startsWith("test") || normalized.includes(".test") || normalized.includes("qa")) {
    return "test";
  }
  return "";
}

function getDefaultEnvironmentAliases(environmentName) {
  switch (environmentName) {
    case "prod":
      return ["线上", "生产", "prod"];
    case "yfb":
      return ["预发", "yfb"];
    case "test":
      return ["测试", "test"];
    default:
      return [environmentName].filter(Boolean);
  }
}

function getDefaultJumpServerAliases(environmentName) {
  switch (environmentName) {
    case "prod":
      return ["线上跳板机", "生产跳板机"];
    case "yfb":
      return ["预发跳板机"];
    case "test":
      return ["测试跳板机"];
    default:
      return [];
  }
}

function defaultLogPathPattern(environmentName) {
  switch (environmentName) {
    case "prod":
    case "yfb":
      return "/www/{project}/logs";
    case "test":
      return "/data/{project}/logs";
    default:
      return "";
  }
}

function buildEmptyEnvMapData() {
  return {
    schemaVersion: ENV_MAP_SCHEMA_VERSION,
    generatedBy: "agentsshcli",
    updatedAt: new Date().toISOString(),
    jumpServers: [],
    environments: {
      prod: {
        jumpServer: "",
        targetPreference: "hostname",
        logPathPattern: "",
        aliases: getDefaultEnvironmentAliases("prod")
      },
      yfb: {
        jumpServer: "",
        targetPreference: "hostname",
        logPathPattern: "",
        aliases: getDefaultEnvironmentAliases("yfb")
      },
      test: {
        jumpServer: "",
        targetPreference: "hostname",
        logPathPattern: "",
        aliases: getDefaultEnvironmentAliases("test")
      }
    },
    defaultEnvironment: "prod",
    projects: [],
    machineShortcuts: {},
    logRotationNotes: []
  };
}

function readConnectionEntries(configPath) {
  if (!fs.existsSync(configPath)) {
    return [];
  }
  const raw = fs.readFileSync(configPath, "utf8").trim();
  if (!raw) {
    return [];
  }
  const parsed = JSON.parse(raw);
  return Array.isArray(parsed) ? parsed : [];
}

function buildEnvMapFromConnections(configEntries) {
  const base = buildEmptyEnvMapData();
  const jumpServers = configEntries
    .filter((entry) => entry?.jumpServer?.enabled === true && entry?.name)
    .map((entry) => {
      const environment = inferEnvironmentFromConnectionName(entry.name);
      return {
        name: entry.name,
        environment,
        aliases: getDefaultJumpServerAliases(environment)
      };
    });

  base.jumpServers = jumpServers;
  for (const server of jumpServers) {
    if (!server.environment) {
      continue;
    }
    const current = base.environments[server.environment] || {
      jumpServer: "",
      targetPreference: "hostname",
      logPathPattern: "",
      aliases: getDefaultEnvironmentAliases(server.environment)
    };
    base.environments[server.environment] = {
      ...current,
      jumpServer: current.jumpServer || server.name,
      logPathPattern: current.logPathPattern || defaultLogPathPattern(server.environment)
    };
  }
  if (!base.environments.prod.jumpServer) {
    const first = jumpServers[0];
    if (first) {
      base.environments.prod.jumpServer = first.name;
    }
  }
  return base;
}

function normalizeProjectEntry(project, index, validEnvironments) {
  const name = String(project?.name || "").trim();
  if (!name) {
    throw new Error(`projects[${index}].name 不能为空`);
  }
  const aliases = normalizeStringArray(project.aliases);
  const targets = {};
  for (const [environmentName, values] of Object.entries(project.targets || {})) {
    if (!validEnvironments.has(environmentName)) {
      throw new Error(`projects[${index}].targets.${environmentName} 不是已知环境`);
    }
    targets[environmentName] = normalizeStringArray(values);
  }
  return { name, aliases, targets };
}

function normalizeEnvMapData(rawData) {
  const base = buildEmptyEnvMapData();
  const data = rawData && typeof rawData === "object" ? rawData : {};
  const environmentNames = unique([
    ...Object.keys(base.environments),
    ...Object.keys(data.environments || {})
  ]);
  const environments = {};
  for (const environmentName of environmentNames) {
    const source = data.environments?.[environmentName] || {};
    environments[environmentName] = {
      jumpServer: String(source.jumpServer || "").trim(),
      targetPreference: ["hostname", "ip"].includes(source.targetPreference) ? source.targetPreference : "hostname",
      logPathPattern: String(source.logPathPattern || "").trim(),
      aliases: normalizeStringArray(source.aliases?.length ? source.aliases : getDefaultEnvironmentAliases(environmentName))
    };
  }

  const validEnvironments = new Set(Object.keys(environments));
  const jumpServers = (Array.isArray(data.jumpServers) ? data.jumpServers : []).map((item, index) => {
    const name = String(item?.name || "").trim();
    if (!name) {
      throw new Error(`jumpServers[${index}].name 不能为空`);
    }
    const environment = String(item?.environment || "").trim();
    if (environment && !validEnvironments.has(environment)) {
      throw new Error(`jumpServers[${index}].environment=${environment} 不是已知环境`);
    }
    return {
      name,
      environment,
      aliases: normalizeStringArray(item.aliases)
    };
  });

  const jumpServerNames = new Set(jumpServers.map((item) => item.name));
  for (const [environmentName, environment] of Object.entries(environments)) {
    if (environment.jumpServer && !jumpServerNames.has(environment.jumpServer)) {
      throw new Error(`environments.${environmentName}.jumpServer=${environment.jumpServer} 在 jumpServers 中不存在`);
    }
  }

  const projects = (Array.isArray(data.projects) ? data.projects : []).map((item, index) =>
    normalizeProjectEntry(item, index, validEnvironments)
  );

  const machineShortcuts = {};
  for (const [shortcut, target] of Object.entries(data.machineShortcuts || {})) {
    const normalizedShortcut = String(shortcut || "").trim();
    const normalizedTarget = String(target || "").trim();
    if (!normalizedShortcut || !normalizedTarget) {
      throw new Error("machineShortcuts 里的 key/value 都必须是非空字符串");
    }
    machineShortcuts[normalizedShortcut] = normalizedTarget;
  }

  return {
    schemaVersion: Number.isFinite(Number(data.schemaVersion)) ? Number(data.schemaVersion) : ENV_MAP_SCHEMA_VERSION,
    generatedBy: "agentsshcli",
    updatedAt: new Date().toISOString(),
    jumpServers,
    environments,
    defaultEnvironment: validEnvironments.has(data.defaultEnvironment) ? data.defaultEnvironment : "prod",
    projects,
    machineShortcuts,
    logRotationNotes: normalizeStringArray(data.logRotationNotes)
  };
}

function renderEnvMapMarkdown(data) {
  const lines = [
    "# log-analyze Environment Map",
    "",
    "> 这个文件由 `env-map.json` 渲染生成，供人类阅读与 AI 快速核对。",
    "> 结构化数据请维护同目录下的 `env-map.json`，再执行 `agentsshcli env-map render`。",
    ""
  ];

  lines.push("## JumpServer Connections", "");
  if (data.jumpServers.length === 0) {
    lines.push("- 暂无已登记 JumpServer 连接");
  } else {
    for (const server of data.jumpServers) {
      const aliases = server.aliases.length > 0 ? `；别名：${server.aliases.join(" / ")}` : "";
      const environment = server.environment ? `；环境：${server.environment}` : "";
      lines.push(`- \`${server.name}\`${environment}${aliases}`);
    }
  }
  lines.push("");

  lines.push("## Environments", "");
  for (const [environmentName, environment] of Object.entries(data.environments)) {
    lines.push(`- \`${environmentName}\``);
    lines.push(`  - JumpServer: ${environment.jumpServer ? `\`${environment.jumpServer}\`` : "待补充"}`);
    lines.push(`  - target 偏好: \`${environment.targetPreference}\``);
    lines.push(`  - 日志路径模式: ${environment.logPathPattern ? `\`${environment.logPathPattern}\`` : "待补充"}`);
    lines.push(`  - 别名: ${environment.aliases.length > 0 ? environment.aliases.join(" / ") : "待补充"}`);
  }
  lines.push("");

  lines.push("## Projects", "");
  if (data.projects.length === 0) {
    lines.push("- 暂无项目映射");
  } else {
    for (const project of data.projects) {
      lines.push(`- \`${project.name}\`${project.aliases.length > 0 ? `；别名：${project.aliases.join(" / ")}` : ""}`);
      for (const [environmentName, targets] of Object.entries(project.targets)) {
        lines.push(`  - ${environmentName}: ${targets.length > 0 ? targets.map((item) => `\`${item}\``).join(", ") : "待补充"}`);
      }
    }
  }
  lines.push("");

  lines.push("## Machine Shortcuts", "");
  const shortcutEntries = Object.entries(data.machineShortcuts);
  if (shortcutEntries.length === 0) {
    lines.push("- 暂无机器简称映射");
  } else {
    for (const [shortcut, target] of shortcutEntries) {
      lines.push(`- \`${shortcut}\` -> \`${target}\``);
    }
  }
  lines.push("");

  lines.push("## Log Rotation Notes", "");
  if (data.logRotationNotes.length === 0) {
    lines.push("- 暂无特殊日志归档说明");
  } else {
    for (const note of data.logRotationNotes) {
      lines.push(`- ${note}`);
    }
  }
  lines.push("");
  lines.push(`_defaultEnvironment: \`${data.defaultEnvironment}\`_`);
  lines.push(`_updatedAt: ${data.updatedAt}_`);
  lines.push("");
  return `${lines.join("\n")}\n`;
}

function getEnvMapPaths(target) {
  const logSkillDir = path.join(target.skillsDir, LOG_SKILL_NAME);
  return {
    logSkillDir,
    jsonPath: path.join(logSkillDir, ENV_MAP_JSON_FILENAME),
    markdownPath: path.join(logSkillDir, ENV_MAP_MD_FILENAME),
    templatePath: path.join(logSkillDir, "env-map.template.md")
  };
}

function writeEnvMapData(target, data) {
  const paths = getEnvMapPaths(target);
  ensureDir(paths.logSkillDir);
  writePrettyJson(paths.jsonPath, data);
  fs.writeFileSync(paths.markdownPath, renderEnvMapMarkdown(data), "utf8");
  return paths;
}

function getExistingEnvMapFiles(paths) {
  const existing = [];
  if (fs.existsSync(paths.jsonPath)) {
    existing.push(paths.jsonPath);
  }
  if (fs.existsSync(paths.markdownPath)) {
    existing.push(paths.markdownPath);
  }
  return existing;
}

function ensureEnvMapFiles(target) {
  const paths = getEnvMapPaths(target);
  ensureDir(paths.logSkillDir);
  const templateSource = path.join(projectRoot, "skills", LOG_SKILL_NAME, "env-map.template.md");
  const templateCreated = writeFileIfMissing(templateSource, paths.templatePath);
  let jsonCreated = false;
  let markdownCreated = false;
  if (!fs.existsSync(paths.jsonPath)) {
    writePrettyJson(paths.jsonPath, buildEmptyEnvMapData());
    jsonCreated = true;
  }
  const data = normalizeEnvMapData(readJsonFile(paths.jsonPath));
  if (jsonCreated) {
    writePrettyJson(paths.jsonPath, data);
  }
  if (!fs.existsSync(paths.markdownPath)) {
    fs.writeFileSync(paths.markdownPath, renderEnvMapMarkdown(data), "utf8");
    markdownCreated = true;
  }
  return {
    ...paths,
    jsonCreated,
    markdownCreated,
    templateCreated
  };
}

function compareSemver(left, right) {
  const parse = (value) => {
    const normalized = String(value || "0.0.0").trim();
    const [withoutBuild] = normalized.split("+", 1);
    const hyphenIndex = withoutBuild.indexOf("-");
    const corePart = hyphenIndex === -1 ? withoutBuild : withoutBuild.slice(0, hyphenIndex);
    const prereleasePart = hyphenIndex === -1 ? "" : withoutBuild.slice(hyphenIndex + 1);
    const core = corePart
      .split(".")
      .map((item) => Number.parseInt(item, 10))
      .map((item) => (Number.isFinite(item) ? item : 0));
    const prerelease = prereleasePart
      ? prereleasePart.split(".").map((item) => (/^\d+$/.test(item) ? Number(item) : item))
      : [];
    return { core, prerelease };
  };
  const comparePrereleaseIdentifier = (leftValue, rightValue) => {
    const leftIsNumber = typeof leftValue === "number";
    const rightIsNumber = typeof rightValue === "number";
    if (leftIsNumber && rightIsNumber) {
      return leftValue === rightValue ? 0 : (leftValue > rightValue ? 1 : -1);
    }
    if (leftIsNumber) {
      return -1;
    }
    if (rightIsNumber) {
      return 1;
    }
    return String(leftValue).localeCompare(String(rightValue));
  };

  const a = parse(left);
  const b = parse(right);
  const maxLength = Math.max(a.core.length, b.core.length);
  for (let index = 0; index < maxLength; index += 1) {
    const leftValue = a.core[index] || 0;
    const rightValue = b.core[index] || 0;
    if (leftValue > rightValue) {
      return 1;
    }
    if (leftValue < rightValue) {
      return -1;
    }
  }

  if (a.prerelease.length === 0 && b.prerelease.length === 0) {
    return 0;
  }
  if (a.prerelease.length === 0) {
    return 1;
  }
  if (b.prerelease.length === 0) {
    return -1;
  }

  const prereleaseLength = Math.max(a.prerelease.length, b.prerelease.length);
  for (let index = 0; index < prereleaseLength; index += 1) {
    const leftValue = a.prerelease[index];
    const rightValue = b.prerelease[index];
    if (leftValue === undefined) {
      return -1;
    }
    if (rightValue === undefined) {
      return 1;
    }
    const compared = comparePrereleaseIdentifier(leftValue, rightValue);
    if (compared !== 0) {
      return compared;
    }
  }

  return 0;
}

function splitFrontmatter(document) {
  const normalized = normalizeNewlines(document);
  if (!normalized.startsWith("---\n")) {
    return {
      frontmatter: "",
      body: normalized
    };
  }
  const endIndex = normalized.indexOf("\n---\n", 4);
  if (endIndex === -1) {
    return {
      frontmatter: "",
      body: normalized
    };
  }
  return {
    frontmatter: normalized.slice(0, endIndex + 5).trimEnd(),
    body: normalized.slice(endIndex + 5)
  };
}

function trimSectionContent(rawContent) {
  let content = normalizeNewlines(rawContent);
  if (content.startsWith("\n")) {
    content = content.slice(1);
  }
  if (content.endsWith("\n")) {
    content = content.slice(0, -1);
  }
  return content;
}

function extractMarkedSection(document, startMarker, endMarker) {
  const startIndex = document.indexOf(startMarker);
  if (startIndex === -1) {
    return {
      found: false,
      startIndex: -1,
      endIndex: -1,
      content: undefined
    };
  }
  const endIndex = document.indexOf(endMarker, startIndex + startMarker.length);
  if (endIndex === -1) {
    return {
      found: false,
      startIndex,
      endIndex: -1,
      content: undefined
    };
  }
  return {
    found: true,
    startIndex,
    endIndex,
    content: trimSectionContent(document.slice(startIndex + startMarker.length, endIndex))
  };
}

function parseManagedSkillDocument(document) {
  const normalized = normalizeNewlines(document);
  const { frontmatter, body } = splitFrontmatter(normalized);
  const lines = body.split("\n");
  let index = 0;
  while (index < lines.length && lines[index].trim() === "") {
    index += 1;
  }

  let templateVersion;
  let expectedHash;
  while (index < lines.length) {
    const line = lines[index].trim();
    if (!line) {
      index += 1;
      continue;
    }
    const versionMatch = line.match(/^<!--\s*agentsshcli:template-version=(.+?)\s*-->$/);
    if (versionMatch) {
      templateVersion = versionMatch[1].trim();
      index += 1;
      continue;
    }
    const hashMatch = line.match(/^<!--\s*agentsshcli:managed-sha256=([a-f0-9]{64})\s*-->$/);
    if (hashMatch) {
      expectedHash = hashMatch[1].trim();
      index += 1;
      continue;
    }
    break;
  }

  const bodyWithoutMetadata = lines.slice(index).join("\n");
  const managedSection = extractMarkedSection(bodyWithoutMetadata, MANAGED_START_MARKER, MANAGED_END_MARKER);
  const localSection = extractMarkedSection(bodyWithoutMetadata, LOCAL_START_MARKER, LOCAL_END_MARKER);
  const structured = managedSection.found && localSection.found;

  if (!structured) {
    return {
      structured: false,
      frontmatter,
      body: normalized,
      templateVersion,
      expectedHash,
      localContent: localSection.content
    };
  }

  return {
    structured: true,
    frontmatter,
    templateVersion,
    expectedHash,
    managedContent: managedSection.content,
    localContent: localSection.content
  };
}

function buildManagedSkillDocument(templateDocument, localContent, version = packageVersion) {
  if (!templateDocument.structured) {
    throw new Error("模板 skill 缺少 agentsshcli 模板标记，无法生成兼容更新文件");
  }
  const normalizedLocalContent = trimSectionContent(localContent ?? templateDocument.localContent);
  const managedHash = computeSha256(templateDocument.managedContent);
  const sections = [];

  if (templateDocument.frontmatter) {
    sections.push(templateDocument.frontmatter);
    sections.push("");
  }

  sections.push(`<!-- agentsshcli:template-version=${version} -->`);
  sections.push(`<!-- agentsshcli:managed-sha256=${managedHash} -->`);
  sections.push("");
  sections.push(MANAGED_START_MARKER);
  sections.push(templateDocument.managedContent);
  sections.push(MANAGED_END_MARKER);
  sections.push("");
  sections.push(LOCAL_START_MARKER);
  sections.push(normalizedLocalContent);
  sections.push(LOCAL_END_MARKER);
  sections.push("");

  return sections.join("\n");
}

function hasLocalCustomizations(installedLocalContent, templateLocalContent) {
  return trimSectionContent(installedLocalContent).trim() !== trimSectionContent(templateLocalContent).trim();
}

function getPackagedLogSkillTemplate() {
  const sourcePath = path.join(projectRoot, "skills", LOG_SKILL_NAME, "SKILL.md");
  const parsed = parseManagedSkillDocument(fs.readFileSync(sourcePath, "utf8"));
  if (!parsed.structured) {
    throw new Error(`${sourcePath} 缺少 agentsshcli 模板标记`);
  }
  const normalized = parseManagedSkillDocument(
    buildManagedSkillDocument(parsed, parsed.localContent, packageVersion)
  );
  if (!normalized.structured) {
    throw new Error(`${sourcePath} 模板标准化失败`);
  }
  return normalized;
}

function inspectLogSkill(target, packagedTemplate = getPackagedLogSkillTemplate()) {
  const skillFile = path.join(target.skillsDir, LOG_SKILL_NAME, "SKILL.md");
  if (!fs.existsSync(skillFile)) {
    return {
      target,
      skillFile,
      exists: false,
      structured: false,
      status: "missing",
      packagedVersion: packageVersion,
      recommendedAction: "install"
    };
  }

  const parsed = parseManagedSkillDocument(fs.readFileSync(skillFile, "utf8"));
  if (!parsed.structured || !parsed.templateVersion || !parsed.expectedHash) {
    return {
      target,
      skillFile,
      exists: true,
      structured: false,
      status: "legacy",
      packagedVersion: packageVersion,
      installedVersion: parsed.templateVersion,
      parsed,
      recommendedAction: "compat-update"
    };
  }

  const installedHash = computeSha256(parsed.managedContent);
  const packagedHash = packagedTemplate.expectedHash || computeSha256(packagedTemplate.managedContent);
  const managedCustomized = installedHash !== parsed.expectedHash;
  const localCustomized = hasLocalCustomizations(parsed.localContent, packagedTemplate.localContent);
  const versionComparison = compareSemver(parsed.templateVersion || "0.0.0", packagedTemplate.templateVersion || packageVersion);
  const updateAvailable = parsed.expectedHash !== packagedHash || versionComparison < 0;

  let status = "ok";
  if (updateAvailable) {
    status = "outdated";
  } else if (managedCustomized || localCustomized) {
    status = "customized";
  }

  return {
    target,
    skillFile,
    exists: true,
    structured: true,
    status,
    packagedVersion: packageVersion,
    installedVersion: parsed.templateVersion,
    managedCustomized,
    localCustomized,
    updateAvailable,
    expectedHash: parsed.expectedHash,
    installedHash,
    packagedHash,
    parsed,
    recommendedAction: updateAvailable ? "compat-update" : (managedCustomized || localCustomized ? "review" : "none")
  };
}

function getTargetLabel(target) {
  return KNOWN_CLIENTS[target.client]?.label || target.label || target.client;
}

function buildDefaultLocalMigrationNote() {
  return [
    "> 本地扩展区说明：",
    ">",
    "> - 这里可以补充当前团队或个人的私有排障规则。",
    "> - 后续执行 `agentsshcli sync-skills` 兼容更新时，这一段会尽量保留。",
    "> - `env-map.md` 和其它私有配置不会被覆盖。"
  ].join("\n");
}

function buildCompatLocalContentFromExisting(existingInspection, packagedTemplate) {
  if (existingInspection?.parsed?.localContent) {
    return existingInspection.parsed.localContent;
  }
  return packagedTemplate.localContent;
}

async function promptCompatUpdate(inspection) {
  const rl = readline.createInterface({
    input: process.stdin,
    output: process.stdout
  });

  try {
    const targetLabel = getTargetLabel(inspection.target);
    const changeNote = inspection.exists && inspection.status === "legacy"
      ? "检测到本地 log-analyze 还是旧版模板结构。"
      : `检测到 ${targetLabel} 的 log-analyze 有新模板版本 ${packageVersion}。`;
    const customizationNote = inspection.managedCustomized || inspection.localCustomized || inspection.status === "legacy"
      ? "你的本地 skill 也有改动。"
      : "";
    console.log([changeNote, customizationNote].filter(Boolean).join(""));
    console.log("是否执行兼容更新？这会尽量保留你的本地补充，并自动备份当前 SKILL.md。[Y/n]");
    const answer = (await rl.question("> ")).trim().toLowerCase();
    return !(answer === "n" || answer === "no");
  } finally {
    rl.close();
  }
}

async function syncSingleLogSkill(target, options = {}) {
  const packagedTemplate = getPackagedLogSkillTemplate();
  const inspection = inspectLogSkill(target, packagedTemplate);
  const { logSkillDir, templateCreated: envTemplateCreated, jsonCreated, markdownCreated } = ensureEnvMapFiles(target);
  const skillFile = path.join(logSkillDir, "SKILL.md");
  const envCreated = jsonCreated || markdownCreated;

  if (!inspection.exists) {
    fs.writeFileSync(skillFile, buildManagedSkillDocument(packagedTemplate, packagedTemplate.localContent), "utf8");
    return {
      ...inspection,
      action: "installed",
      updated: true,
      envCreated,
      envTemplateCreated
    };
  }

  const overwrite = options.overwrite === true;
  if (!overwrite && !inspection.updateAvailable && inspection.status !== "legacy") {
    return {
      ...inspection,
      action: "unchanged",
      updated: false,
      envCreated,
      envTemplateCreated
    };
  }

  const interactive = options.interactive === true;
  const autoYes = options.yes === true;
  const needsPrompt = !overwrite && !autoYes && interactive && (inspection.status === "legacy" || inspection.updateAvailable);
  if (needsPrompt && !(await promptCompatUpdate(inspection))) {
    return {
      ...inspection,
      action: "skipped",
      updated: false,
      envCreated,
      envTemplateCreated
    };
  }

  if (!overwrite && !interactive && !autoYes && (inspection.status === "legacy" || inspection.managedCustomized || inspection.localCustomized)) {
    return {
      ...inspection,
      action: "skipped",
      updated: false,
      envCreated,
      envTemplateCreated,
      reason: "需要用户确认兼容更新"
    };
  }

  const shouldBackup = overwrite || inspection.status === "legacy" || inspection.managedCustomized || inspection.localCustomized;
  const backupPath = shouldBackup ? backupExistingPath(skillFile) : undefined;
  const localContent = overwrite
    ? packagedTemplate.localContent
    : buildCompatLocalContentFromExisting(inspection, packagedTemplate);
  const nextContent = buildManagedSkillDocument(packagedTemplate, localContent);
  fs.writeFileSync(skillFile, nextContent, "utf8");

  return {
    ...inspection,
    action: overwrite ? "overwritten" : "compat-updated",
    updated: true,
    backupPath,
    envCreated,
    envTemplateCreated
  };
}

function resolveSelectedTargets({ skillsDir, explicitClients = [], clientRoots = new Map() }) {
  if (skillsDir) {
    return [{
      client: "custom",
      label: "Custom",
      skillsDir
    }];
  }

  const selectedClients = explicitClients.length > 0 ? unique(explicitClients) : getDefaultClientSelection();
  if (selectedClients.length === 0) {
    throw new Error("至少需要一个目标客户端");
  }

  const targets = selectedClients.map((clientName) => ({
    client: clientName,
    label: clientName === "custom" ? "Custom" : KNOWN_CLIENTS[clientName]?.label,
    skillsDir: expandHome(
      clientRoots.get(clientName) ||
      (clientName === "custom" ? "" : KNOWN_CLIENTS[clientName]?.skillsDir)
    )
  }));

  for (const target of targets) {
    if (!target.skillsDir) {
      throw new Error(`客户端 ${target.client} 缺少 skills 根目录，请通过 --client-root ${target.client}=<path> 指定`);
    }
  }

  return targets;
}

function parseSkillCommandArgs(args) {
  let skillsDir;
  let json = false;
  let yes = false;
  let overwrite = false;
  const explicitClients = [];
  const clientRoots = new Map();

  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (arg === "--help" || arg === "-h") {
      return { help: true };
    }
    if (arg === "--skills-dir") {
      skillsDir = expandHome(args[index + 1]);
      index += 1;
      continue;
    }
    if (arg === "--client") {
      const clientName = normalizeClientName(args[index + 1]);
      if (!clientName) {
        throw new Error(`未知客户端: ${args[index + 1]}`);
      }
      explicitClients.push(clientName);
      index += 1;
      continue;
    }
    if (arg === "--clients") {
      explicitClients.push(...parseClientList(args[index + 1]));
      index += 1;
      continue;
    }
    if (arg === "--client-root") {
      const parsed = parseClientRootArg(args[index + 1]);
      clientRoots.set(parsed.clientName, parsed.skillsDir);
      index += 1;
      continue;
    }
    if (arg === "--json") {
      json = true;
      continue;
    }
    if (arg === "--yes" || arg === "-y") {
      yes = true;
      continue;
    }
    if (arg === "--overwrite") {
      overwrite = true;
      continue;
    }
    throw new Error(`未知参数: ${arg}`);
  }

  return {
    skillsDir,
    explicitClients,
    clientRoots,
    json,
    yes,
    overwrite
  };
}

function printDoctorSkillsHelp() {
  console.log(`用法:
  agentsshcli doctor-skills [--skills-dir <path>] [--client <name> ...] [--clients <list>] [--client-root <name=path> ...] [--json]

说明:
  轻量检查本地已安装的 log-analyze skill 是否缺失、过期、仍是旧版结构，或存在本地自定义改动。
`);
}

function printSyncSkillsHelp() {
  console.log(`用法:
  agentsshcli sync-skills [--skills-dir <path>] [--client <name> ...] [--clients <list>] [--client-root <name=path> ...] [--yes] [--overwrite]

说明:
  兼容更新本地已安装的 log-analyze skill。
  - 默认走兼容更新：尽量保留你的本地补充，并按需备份当前 SKILL.md
  - --overwrite 表示备份后整体覆盖为最新模板，也会重置当前模板里的本地补充区
  - --yes 表示非交互确认
`);
}

function parseEnvMapCommandArgs(args) {
  const parsed = {
    subcommand: "",
    skillsDir: "",
    explicitClients: [],
    clientRoots: new Map(),
    configPath: DEFAULT_CONFIG_PATH,
    fromConfig: false,
    json: false,
    environment: "",
    project: "",
    alias: [],
    target: [],
    shortcut: [],
    logPathPattern: "",
    jumpServer: "",
    defaultEnvironment: "",
    note: [],
    force: false,
    help: false
  };

  if (args.length === 0 || args[0] === "--help" || args[0] === "-h") {
    parsed.help = true;
    return parsed;
  }

  parsed.subcommand = args[0];
  for (let index = 1; index < args.length; index += 1) {
    const arg = args[index];
    if (arg === "--help" || arg === "-h") {
      parsed.help = true;
      continue;
    }
    if (arg === "--skills-dir") {
      parsed.skillsDir = expandHome(args[index + 1]);
      index += 1;
      continue;
    }
    if (arg === "--client") {
      const clientName = normalizeClientName(args[index + 1]);
      if (!clientName) {
        throw new Error(`未知客户端: ${args[index + 1]}`);
      }
      parsed.explicitClients.push(clientName);
      index += 1;
      continue;
    }
    if (arg === "--clients") {
      parsed.explicitClients.push(...parseClientList(args[index + 1]));
      index += 1;
      continue;
    }
    if (arg === "--client-root") {
      const clientRoot = parseClientRootArg(args[index + 1]);
      parsed.clientRoots.set(clientRoot.clientName, clientRoot.skillsDir);
      index += 1;
      continue;
    }
    if (arg === "--config" || arg === "--config-path") {
      parsed.configPath = expandHome(args[index + 1]);
      index += 1;
      continue;
    }
    if (arg === "--from-config") {
      parsed.fromConfig = true;
      continue;
    }
    if (arg === "--force") {
      parsed.force = true;
      continue;
    }
    if (arg === "--json") {
      parsed.json = true;
      continue;
    }
    if (arg === "--env" || arg === "--environment") {
      parsed.environment = String(args[index + 1] || "").trim();
      index += 1;
      continue;
    }
    if (arg === "--project") {
      parsed.project = String(args[index + 1] || "").trim();
      index += 1;
      continue;
    }
    if (arg === "--alias") {
      parsed.alias.push(String(args[index + 1] || "").trim());
      index += 1;
      continue;
    }
    if (arg === "--target") {
      parsed.target.push(String(args[index + 1] || "").trim());
      index += 1;
      continue;
    }
    if (arg === "--shortcut") {
      parsed.shortcut.push(String(args[index + 1] || "").trim());
      index += 1;
      continue;
    }
    if (arg === "--log-path" || arg === "--log-path-pattern") {
      parsed.logPathPattern = String(args[index + 1] || "").trim();
      index += 1;
      continue;
    }
    if (arg === "--jump-server") {
      parsed.jumpServer = String(args[index + 1] || "").trim();
      index += 1;
      continue;
    }
    if (arg === "--default-env") {
      parsed.defaultEnvironment = String(args[index + 1] || "").trim();
      index += 1;
      continue;
    }
    if (arg === "--note") {
      parsed.note.push(String(args[index + 1] || "").trim());
      index += 1;
      continue;
    }
    throw new Error(`未知参数: ${arg}`);
  }

  return parsed;
}

function printEnvMapHelp() {
  console.log(`用法:
  agentsshcli env-map init [--skills-dir <path>|--client <name>|--clients <list>] [--client-root <name=path> ...] [--from-config] [--config <path>] [--force]
  agentsshcli env-map render [--skills-dir <path>|--client <name>|--clients <list>] [--client-root <name=path> ...]
  agentsshcli env-map list [--skills-dir <path>|--client <name>|--clients <list>] [--client-root <name=path> ...] [--json]
  agentsshcli env-map status [--skills-dir <path>|--client <name>|--clients <list>] [--client-root <name=path> ...] [--json]
  agentsshcli env-map validate [--skills-dir <path>|--client <name>|--clients <list>] [--client-root <name=path> ...] [--json]
  agentsshcli env-map add-host --project <name> --env <env> --target <hostOrIp> [--alias <alias> ...] [--shortcut <shortcut=hostOrIp> ...] [--skills-dir <path>|--client <name>|--clients <list>] [--client-root <name=path> ...]

说明:
  使用结构化的 env-map.json 维护 JumpServer、环境、项目别名、目标机和日志路径，再自动渲染 env-map.md 供人类阅读。
  首次创建时推荐执行 \`agentsshcli env-map init --from-config\`，自动从 ~/.agent-ssh-cli/config.json 发现 JumpServer 连接。
  为避免误覆盖，\`env-map init\` 在检测到现有 env-map.json 或 env-map.md 时默认拒绝重写；如需整体重建请显式加 \`--force\`。
`);
}

function resolveSingleEnvMapTarget(parsed) {
  const targets = resolveSelectedTargets({
    skillsDir: parsed.skillsDir,
    explicitClients: parsed.explicitClients,
    clientRoots: parsed.clientRoots
  });
  return targets[0];
}

function loadEnvMapData(target) {
  const ensured = ensureEnvMapFiles(target);
  const data = normalizeEnvMapData(readJsonFile(ensured.jsonPath));
  return {
    target,
    paths: ensured,
    data
  };
}

function validateEnvMapData(data) {
  const issues = [];
  if (data.jumpServers.length === 0) {
    issues.push("未登记任何 JumpServer 连接");
  }
  for (const [environmentName, environment] of Object.entries(data.environments)) {
    if (!environment.jumpServer) {
      issues.push(`环境 ${environmentName} 未绑定 JumpServer`);
    }
    if (!environment.logPathPattern) {
      issues.push(`环境 ${environmentName} 未配置日志路径模式`);
    }
  }
  if (!data.defaultEnvironment || !data.environments[data.defaultEnvironment]) {
    issues.push("defaultEnvironment 无效");
  }
  return issues;
}

function summarizeEnvMapStatus(data, issues) {
  return {
    jumpServers: data.jumpServers.map((item) => item.name),
    environments: Object.keys(data.environments),
    projects: data.projects.map((item) => item.name),
    machineShortcuts: Object.keys(data.machineShortcuts),
    issues
  };
}

function upsertProject(data, projectName) {
  const normalizedProjectName = String(projectName || "").trim();
  if (!normalizedProjectName) {
    throw new Error("--project 不能为空");
  }
  const existing = data.projects.find((item) => item.name === normalizedProjectName);
  if (existing) {
    return existing;
  }
  const next = {
    name: normalizedProjectName,
    aliases: [],
    targets: {}
  };
  data.projects.push(next);
  return next;
}

function parseShortcutAssignments(items) {
  const assignments = [];
  for (const raw of items) {
    const value = String(raw || "").trim();
    if (!value) {
      continue;
    }
    const index = value.indexOf("=");
    if (index <= 0 || index === value.length - 1) {
      throw new Error(`--shortcut 参数格式必须是 <alias>=<target>，实际收到: ${value}`);
    }
    assignments.push({
      alias: value.slice(0, index).trim(),
      target: value.slice(index + 1).trim()
    });
  }
  return assignments;
}

async function runEnvMap(args) {
  const parsed = parseEnvMapCommandArgs(args);
  if (parsed.help) {
    printEnvMapHelp();
    return 0;
  }

  if (!parsed.subcommand) {
    printEnvMapHelp();
    return 1;
  }

  const target = resolveSingleEnvMapTarget(parsed);

  if (parsed.subcommand === "init") {
    const paths = getEnvMapPaths(target);
    ensureDir(paths.logSkillDir);
    const templateSource = path.join(projectRoot, "skills", LOG_SKILL_NAME, "env-map.template.md");
    writeFileIfMissing(templateSource, paths.templatePath);
    const existingEnvMapFiles = getExistingEnvMapFiles(paths);
    if (existingEnvMapFiles.length > 0 && !parsed.force) {
      throw new Error(
        `已存在 env-map 文件：${existingEnvMapFiles.join("、")}。为避免覆盖已有映射，默认拒绝重建；如确认要整体重建，请显式加 --force。`
      );
    }
    let data = buildEmptyEnvMapData();
    if (parsed.fromConfig) {
      data = buildEnvMapFromConnections(readConnectionEntries(parsed.configPath));
    }
    const writtenPaths = writeEnvMapData(target, normalizeEnvMapData(data));
    console.log(`已初始化 env-map: ${writtenPaths.jsonPath}`);
    console.log(`已渲染 env-map: ${writtenPaths.markdownPath}`);
    if (parsed.fromConfig) {
      console.log(`已从 ${parsed.configPath} 自动发现 ${data.jumpServers.length} 个 JumpServer 连接`);
    }
    return 0;
  }

  const loaded = loadEnvMapData(target);

  if (parsed.subcommand === "render") {
    writeEnvMapData(target, loaded.data);
    console.log(`已重新渲染 ${loaded.paths.markdownPath}`);
    return 0;
  }

  if (parsed.subcommand === "list") {
    if (parsed.json) {
      console.log(JSON.stringify(loaded.data, null, 2));
    } else {
      console.log(renderEnvMapMarkdown(loaded.data).trimEnd());
    }
    return 0;
  }

  if (parsed.subcommand === "status" || parsed.subcommand === "validate") {
    const issues = validateEnvMapData(loaded.data);
    const summary = summarizeEnvMapStatus(loaded.data, issues);
    if (parsed.json) {
      console.log(JSON.stringify(summary, null, 2));
    } else {
      console.log(`JumpServer: ${summary.jumpServers.length}`);
      console.log(`环境: ${summary.environments.join(", ")}`);
      console.log(`项目: ${summary.projects.length}`);
      console.log(`机器简称: ${summary.machineShortcuts.length}`);
      if (issues.length === 0) {
        console.log("状态: 已通过基本校验");
      } else {
        console.log("状态: 仍有待补项");
        for (const issue of issues) {
          console.log(`- ${issue}`);
        }
      }
    }
    return issues.length === 0 ? 0 : 2;
  }

  if (parsed.subcommand === "add-host") {
    if (!parsed.environment) {
      throw new Error("add-host 需要 --env <environment>");
    }
    if (!loaded.data.environments[parsed.environment]) {
      throw new Error(`未知环境: ${parsed.environment}`);
    }
    if (parsed.target.length === 0) {
      throw new Error("add-host 至少需要一个 --target <hostOrIp>");
    }
    const project = upsertProject(loaded.data, parsed.project);
    project.aliases = unique([...project.aliases, ...normalizeStringArray(parsed.alias)]);
    project.targets[parsed.environment] = unique([
      ...(project.targets[parsed.environment] || []),
      ...normalizeStringArray(parsed.target)
    ]);
    for (const assignment of parseShortcutAssignments(parsed.shortcut)) {
      loaded.data.machineShortcuts[assignment.alias] = assignment.target;
    }
    writeEnvMapData(target, normalizeEnvMapData(loaded.data));
    console.log(`已更新项目 ${project.name} 的 ${parsed.environment} 目标机映射`);
    return 0;
  }

  throw new Error(`未知 env-map 子命令: ${parsed.subcommand}`);
}

function formatDoctorSummary(inspection) {
  if (inspection.status === "missing") {
    return "未安装";
  }
  if (inspection.status === "legacy") {
    return "旧版结构，建议兼容更新";
  }
  if (inspection.updateAvailable) {
    if (inspection.managedCustomized || inspection.localCustomized) {
      return `有新模板版本 ${packageVersion}，且本地有改动`;
    }
    return `有新模板版本 ${packageVersion}`;
  }
  if (inspection.managedCustomized || inspection.localCustomized) {
    return "当前模板已被本地修改";
  }
  return "已是最新模板";
}

async function runDoctorSkills(args) {
  const parsed = parseSkillCommandArgs(args);
  if (parsed.help) {
    printDoctorSkillsHelp();
    return 0;
  }

  const targets = resolveSelectedTargets(parsed);
  const results = targets.map((target) => {
    const inspection = inspectLogSkill(target);
    return {
      client: target.client,
      label: getTargetLabel(target),
      skillsDir: target.skillsDir,
      skillFile: inspection.skillFile,
      status: inspection.status,
      packagedVersion: inspection.packagedVersion,
      installedVersion: inspection.installedVersion,
      updateAvailable: inspection.updateAvailable || false,
      managedCustomized: inspection.managedCustomized || false,
      localCustomized: inspection.localCustomized || false,
      recommendedAction: inspection.recommendedAction,
      summary: formatDoctorSummary(inspection)
    };
  });

  if (parsed.json) {
    console.log(JSON.stringify(results, null, 2));
    return results.some((item) => item.status !== "ok") ? 2 : 0;
  }

  console.log("log-analyze skill 检查结果：");
  for (const result of results) {
    console.log(`- ${result.label}: ${result.summary}`);
    console.log(`  - 目录: ${result.skillsDir}`);
    if (result.installedVersion) {
      console.log(`  - 已安装模板版本: ${result.installedVersion}`);
    }
    console.log(`  - 当前模板版本: ${result.packagedVersion}`);
  }

  return results.some((item) => item.status !== "ok") ? 2 : 0;
}

async function runSyncSkills(args) {
  const parsed = parseSkillCommandArgs(args);
  if (parsed.help) {
    printSyncSkillsHelp();
    return 0;
  }

  const targets = resolveSelectedTargets(parsed);
  const interactive = process.stdin.isTTY && process.stdout.isTTY;
  const results = [];

  for (const target of targets) {
    results.push(await syncSingleLogSkill(target, {
      interactive,
      yes: parsed.yes,
      overwrite: parsed.overwrite
    }));
  }

  console.log("log-analyze skill 同步结果：");
  for (const result of results) {
    console.log(`- ${getTargetLabel(result.target)}: ${result.action}`);
    console.log(`  - 目录: ${result.target.skillsDir}`);
    if (result.backupPath) {
      console.log(`  - 备份: ${result.backupPath}`);
    }
    if (result.reason) {
      console.log(`  - 说明: ${result.reason}`);
    }
  }

  return 0;
}

function installSingleSkillFile(sourceFile, targetDir) {
  const targetFile = path.join(targetDir, "SKILL.md");
  copyFileEnsured(sourceFile, targetFile);
  return targetFile;
}

function backupExistingPath(targetPath) {
  const backupPath = `${targetPath}.backup-${timestampForBackup()}`;
  fs.renameSync(targetPath, backupPath);
  return backupPath;
}

function ensureDirSymlink(sourceDir, targetDir) {
  ensureDir(path.dirname(targetDir));

  if (fs.existsSync(targetDir)) {
    const stat = fs.lstatSync(targetDir);
    if (stat.isSymbolicLink()) {
      const existingRealPath = fs.realpathSync(targetDir);
      const desiredRealPath = fs.realpathSync(sourceDir);
      if (existingRealPath === desiredRealPath) {
        return { status: "unchanged" };
      }
      fs.unlinkSync(targetDir);
    } else {
      const backupPath = backupExistingPath(targetDir);
      fs.symlinkSync(sourceDir, targetDir, "dir");
      return { status: "relinked", backupPath };
    }
  }

  fs.symlinkSync(sourceDir, targetDir, "dir");
  return { status: "linked" };
}

async function installPrimarySkills(skillsDir, sources, options = {}) {
  const agentSkillDir = path.join(skillsDir, "agent-ssh-cli");
  const logSkillDir = path.join(skillsDir, "log-analyze");
  const envMapTarget = path.join(logSkillDir, ENV_MAP_MD_FILENAME);
  const envMapJsonTarget = path.join(logSkillDir, ENV_MAP_JSON_FILENAME);

  const agentSkillTarget = installSingleSkillFile(sources.agentSkillSource, agentSkillDir);
  const logSkillResult = await syncSingleLogSkill({
    client: "primary",
    label: "Primary",
    skillsDir
  }, options);

  return {
    agentSkillDir,
    agentSkillTarget,
    logSkillDir,
    envMapTarget,
    envMapJsonTarget,
    envCreated: logSkillResult.envCreated,
    logSkillResult
  };
}

async function installSecondarySkills(target, primaryInstalled, sources, linkSecondary, options = {}) {
  const agentSkillDir = path.join(target.skillsDir, "agent-ssh-cli");
  const logSkillDir = path.join(target.skillsDir, "log-analyze");

  if (linkSecondary) {
    const agentResult = ensureDirSymlink(primaryInstalled.agentSkillDir, agentSkillDir);
    const logResult = ensureDirSymlink(primaryInstalled.logSkillDir, logSkillDir);
    return {
      client: target.client,
      skillsDir: target.skillsDir,
      mode: "symlink",
      agentResult,
      logResult
    };
  }

  installSingleSkillFile(sources.agentSkillSource, agentSkillDir);
  const logSkillResult = await syncSingleLogSkill(target, options);
  return {
    client: target.client,
    skillsDir: target.skillsDir,
    mode: "copy",
    envCreated: logSkillResult.envCreated,
    logSkillResult
  };
}

async function promptInstallPlan(defaultClients, clientRoots) {
  const rl = readline.createInterface({
    input: process.stdin,
    output: process.stdout
  });

  try {
    console.log("选择要安装的客户端 skills 目录。");
    if (hasCcSwitchInstalled()) {
      console.log(`检测到 CC Switch，共享 skills 根目录可作为主链默认安装根：${ccSwitchSkillsDir}`);
      console.log("如果你同时在 Codex / Claude Code 中使用 skill，推荐把 cc-switch 作为主链，再让其它客户端复用它。");
    }
    console.log("可选客户端:");
    for (const clientName of Object.keys(KNOWN_CLIENTS)) {
      console.log(formatClientOption(clientName, clientRoots));
    }
    console.log(formatClientOption("custom", clientRoots));
    console.log("");

    const clientAnswer = (await rl.question(`客户端（逗号分隔，默认 ${defaultClients.join(",")}）: `)).trim();
    const selectedClients = unique(clientAnswer ? parseClientList(clientAnswer) : defaultClients);
    if (selectedClients.length === 0) {
      throw new Error("至少需要选择一个客户端");
    }

    if (selectedClients.includes("custom") && !clientRoots.get("custom")) {
      const customPath = (await rl.question("请输入自定义客户端的 skills 根目录完整路径: ")).trim();
      if (!customPath) {
        throw new Error("自定义客户端需要提供完整的 skills 根目录");
      }
      clientRoots.set("custom", expandHome(customPath));
    }

    const defaultPrimary = getDefaultPrimaryClient(selectedClients);
    const primaryAnswer = (await rl.question(`主链客户端（默认 ${defaultPrimary}）: `)).trim();
    const primaryClient = primaryAnswer ? normalizeClientName(primaryAnswer) : defaultPrimary;
    if (!primaryClient || !selectedClients.includes(primaryClient)) {
      throw new Error("主链客户端必须包含在已选择客户端中");
    }

    const linkAnswer = (await rl.question("其余客户端是否软链复用主链 skill/env-map？[Y/n]: ")).trim().toLowerCase();
    const linkSecondary = !(linkAnswer === "n" || linkAnswer === "no");

    return {
      selectedClients,
      primaryClient,
      linkSecondary
    };
  } finally {
    rl.close();
  }
}

function printInstallAiHelp() {
  console.log(`用法:
  agentsshcli install-ai [--skills-dir <path>] [--config-dir <path>]
  agentsshcli install-ai [--preset <name>] [--config-dir <path>]
  agentsshcli install-ai [--interactive] [--clients <list>] [--primary-client <name>] [--copy-secondary]
  agentsshcli install-ai [--interactive] [--client <name> ...] [--client-root <name=path> ...]

说明:
  安装或更新以下本地资产：
  - <skills-root>/agent-ssh-cli/SKILL.md
  - <skills-root>/log-analyze/SKILL.md
  - <skills-root>/log-analyze/env-map.md（仅不存在时初始化模板）
  - ~/.agent-ssh-cli/config.json（仅不存在时初始化示例配置）

客户端默认 skills 根目录：
  - cc-switch -> ~/.cc-switch/skills（如存在，推荐作为共享主链目录）
  - codex   -> ~/.codex/skills
  - claude  -> ~/.claude/skills
  - opencode -> ~/.config/opencode/skills
  - hermes  -> ~/.hermes/skills
  - custom  -> 安装时手动输入完整 skills 根目录

参数:
  --skills-dir <path>         指定单个 skills 根目录，兼容旧行为
  --config-dir <path>         指定配置目录，默认 ~/.agent-ssh-cli
  --preset <name>             使用预设客户端组合，例如 codex / hermes / cc-switch-codex
  --interactive, -i           交互式选择客户端、主链客户端和复用方式
  --client <name>             追加一个客户端（可重复）
  --clients <a,b,c>           批量指定客户端
  --primary-client <name>     指定主链客户端
  --client-root <name=path>   覆盖某个客户端的 skills 根目录（可重复）
  --copy-secondary            其余客户端使用复制，不使用软链复用
  --link-secondary            显式启用软链复用（默认）
  --help                      显示帮助
`);
}

function printNextSteps({ selectedTargets, primaryTarget, envMapTarget, configTarget, linkSecondary }) {
  const runtimeTargets = selectedTargets.filter((target) => KNOWN_CLIENTS[target.client]?.isRuntimeClient !== false);
  console.log("");
  console.log("下一步建议流程：");
  if (runtimeTargets.length > 0) {
    console.log("1. 重启以下客户端，让新 skill 被重新扫描：");
    for (const target of runtimeTargets) {
      console.log(`   - ${KNOWN_CLIENTS[target.client]?.label || target.client}: ${target.skillsDir}`);
    }
  } else {
    console.log("1. 当前只更新了共享主链目录，还没有绑定具体 AI 客户端。");
  }
  console.log("2. 进入任一已重启客户端后，先初始化 JumpServer SSH 配置：");
  console.log(`   - 共享配置路径: ${configTarget}`);
  console.log("   - 推荐让 AI 按 add-jump-server 流程，每次只问一个问题，最终写入该文件");
  console.log("3. SSH 配好后的第一步，先连接 JumpServer 展示 Opt 菜单：");
  console.log("   - 先执行: agentsshcli jump-menu <jumpserver-connection>");
  console.log("   - 先确认当前跳板机菜单长什么样、怎么列主机、怎么搜索主机");
  console.log("4. 然后你只需要告诉 AI 这些信息：");
  console.log("   - 有哪些 JumpServer 分别对应什么环境");
  console.log("   - 各环境日志通常在哪个路径模式");
  console.log("   - 最常查的项目与简称 / 别名");
  console.log("   - 一组常用主机列表");
  console.log("   - 剩下的 JumpServer 菜单确认、主机搜索、真实 hostname/IP 回填，都应由 AI 完成");
  console.log("5. 再维护 log-analyze 的私有环境映射：");
  console.log(`   - 主链 env-map: ${envMapTarget}`);
  console.log(`   - 结构化源文件: ${envMapTarget.replace(/env-map\.md$/, ENV_MAP_JSON_FILENAME)}`);
  console.log(`   - 其他客户端${linkSecondary ? "会通过软链复用这份 env-map" : "各自保留本地 env-map"}`);
  console.log("   - 这个文件建议由你当前正在使用的 AI 自行维护");
  console.log("   - 你通常只需要告诉 AI：JumpServer 与环境关系、日志路径模式、项目别名、常用主机列表");
  console.log("   - 下面有可直接复制给 AI 的提示词");
  console.log("   - 若 env-map 尚不存在，再执行: agentsshcli env-map init --from-config");
  console.log("   - 若已存在，就直接在现有 env-map.json 基础上补充；只有整体重建时才使用 --force");
  console.log("6. 做最小验证：");
  console.log("   - agentsshcli list");
  console.log("   - agentsshcli jump-menu <jumpserver-connection>");
  console.log("   - agentsshcli jump-exec --timeout 120000 <jumpserver-connection> --target <known-target> \"hostname\"");
  console.log("7. 最后确认客户端技能列表中已经能看到 log-analyze。");
  console.log("");
  console.log("推荐交给 AI 的标准提示词：");
  console.log("");
  console.log("A. 初始化 JumpServer 配置");
  console.log(`请帮我初始化 agent-ssh-cli 的 JumpServer 配置。先确认我是否使用私钥认证、私钥路径是否真实存在，再继续收集 name、host、port、username、private-key。正式写入前先用 agentsshcli add-jump-server --dry-run 做预检，确认通过后再写入 ${configTarget}。写入后先执行 agentsshcli jump-menu <jumpserver-connection>，把当前 JumpServer 的 Opt 菜单完整展示给我并确认这个跳板机怎么列主机、怎么搜索主机；这些确认完成后，再继续后面的最小验证。`);
  console.log("");
  console.log("B. 初始化 log-analyze 环境映射");
  console.log(`请直接维护当前主链 env-map 文件：${envMapTarget}，并以同目录下的 ${ENV_MAP_JSON_FILENAME} 作为结构化事实源。如果文件还不存在，再执行 agentsshcli env-map init --from-config，自动读取 ${configTarget} 里的 JumpServer 连接；如果已经存在，就直接在现有 env-map.json 基础上补充，不要重复 init，只有确认整体重建时才使用 --force。然后先用一句话告诉我这一步是在补“常用主机、主机/项目别名、日志目录”的私有信息，后面查日志时你才能自动定位。之后不要按 14 个问题逐条追问，而是只收集 4 组信息：1. 有哪些 JumpServer 分别对应什么环境；2. 各环境日志一般在哪个路径模式；3. 最常查的项目和简称；4. 一组常用主机列表。若我给的是简称，先在 JumpServer 菜单层查出真实 hostname / IP，再用 agentsshcli env-map add-host 写回结构化映射，最后自动渲染回 ${envMapTarget}。`);
}

function copyFileEnsured(source, target) {
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.copyFileSync(source, target);
}

function writeFileIfMissing(source, target) {
  if (fs.existsSync(target)) {
    return false;
  }
  copyFileEnsured(source, target);
  return true;
}

async function runInstallAi(args) {
  let skillsDir;
  let configDir = path.join(homeDir, ".agent-ssh-cli");
  let interactive = false;
  let linkSecondary = true;
  const explicitClients = [];
  const clientRoots = new Map();
  let primaryClient;
  let presetName;

  for (let i = 0; i < args.length; i += 1) {
    const arg = args[i];
    if (arg === "--help" || arg === "-h") {
      printInstallAiHelp();
      return 0;
    }
    if (arg === "--skills-dir") {
      skillsDir = expandHome(args[i + 1]);
      i += 1;
      continue;
    }
    if (arg === "--config-dir") {
      configDir = expandHome(args[i + 1]);
      i += 1;
      continue;
    }
    if (arg === "--preset") {
      presetName = String(args[i + 1] || "").trim();
      i += 1;
      continue;
    }
    if (arg === "--interactive" || arg === "-i") {
      interactive = true;
      continue;
    }
    if (arg === "--client") {
      const clientName = normalizeClientName(args[i + 1]);
      if (!clientName) {
        throw new Error(`未知客户端: ${args[i + 1]}`);
      }
      explicitClients.push(clientName);
      i += 1;
      continue;
    }
    if (arg === "--clients") {
      explicitClients.push(...parseClientList(args[i + 1]));
      i += 1;
      continue;
    }
    if (arg === "--primary-client") {
      primaryClient = normalizeClientName(args[i + 1]);
      if (!primaryClient) {
        throw new Error(`未知主链客户端: ${args[i + 1]}`);
      }
      i += 1;
      continue;
    }
    if (arg === "--client-root") {
      const parsed = parseClientRootArg(args[i + 1]);
      clientRoots.set(parsed.clientName, parsed.skillsDir);
      i += 1;
      continue;
    }
    if (arg === "--copy-secondary") {
      linkSecondary = false;
      continue;
    }
    if (arg === "--link-secondary") {
      linkSecondary = true;
      continue;
    }
    console.error(`未知参数: ${arg}`);
    printInstallAiHelp();
    return 1;
  }

  if (!configDir) {
    throw new Error("config-dir 无效");
  }

  if (skillsDir && explicitClients.length > 0) {
    throw new Error("--skills-dir 不能和 --client/--clients 混用");
  }
  if (presetName) {
    const preset = INSTALL_PRESETS[presetName];
    if (!preset) {
      throw new Error(`未知 preset: ${presetName}`);
    }
    if (skillsDir) {
      primaryClient = "custom";
      linkSecondary = false;
    } else {
      explicitClients.push(...preset.clients);
      primaryClient = preset.primaryClient;
      linkSecondary = preset.linkSecondary;
    }
  }

  const sources = {
    agentSkillSource: path.join(projectRoot, "SKILL.md"),
    envTemplateSource: path.join(projectRoot, "skills", "log-analyze", "env-map.template.md")
  };
  const exampleConfigSource = path.join(projectRoot, "example.config.json");
  const configTarget = path.join(configDir, "config.json");
  const configCreated = writeFileIfMissing(exampleConfigSource, configTarget);

  let selectedClients = [];
  if (skillsDir) {
    selectedClients = ["codex"];
  } else if (explicitClients.length > 0) {
    selectedClients = unique(explicitClients);
  } else if (interactive || (process.stdin.isTTY && process.stdout.isTTY)) {
    const plan = await promptInstallPlan(getDefaultClientSelection(), clientRoots);
    selectedClients = plan.selectedClients;
    primaryClient = plan.primaryClient;
    linkSecondary = plan.linkSecondary;
  } else {
    selectedClients = getDefaultClientSelection();
  }

  if (selectedClients.length === 0) {
    throw new Error("至少需要一个安装目标");
  }

  if (!skillsDir) {
    for (const clientName of selectedClients) {
      if (clientName !== "custom" && !KNOWN_CLIENTS[clientName]) {
        throw new Error(`不支持的客户端: ${clientName}`);
      }
    }
  }

  const selectedTargets = skillsDir
    ? [{ client: "custom", label: "Custom", skillsDir }]
    : selectedClients.map((clientName) => ({
      client: clientName,
      label: clientName === "custom" ? "Custom" : KNOWN_CLIENTS[clientName].label,
      skillsDir: expandHome(
        clientRoots.get(clientName) ||
        (clientName === "custom" ? "" : KNOWN_CLIENTS[clientName].skillsDir)
      )
    }));

  for (const target of selectedTargets) {
    if (!target.skillsDir) {
      throw new Error(`客户端 ${target.client} 缺少 skills 根目录，请通过 --client-root ${target.client}=<path> 指定，或在交互模式下输入`);
    }
  }

  const resolvedPrimaryClient = skillsDir
    ? "custom"
    : (primaryClient || getDefaultPrimaryClient(selectedTargets.map((target) => target.client)));
  const primaryTarget = selectedTargets.find((target) => target.client === resolvedPrimaryClient);
  if (!primaryTarget) {
    throw new Error(`未找到主链客户端: ${resolvedPrimaryClient}`);
  }

  const primaryInstalled = await installPrimarySkills(primaryTarget.skillsDir, {
    agentSkillSource: sources.agentSkillSource,
    envTemplateSource: sources.envTemplateSource
  }, {
    interactive: process.stdin.isTTY && process.stdout.isTTY,
    yes: !(process.stdin.isTTY && process.stdout.isTTY)
  });

  const secondaryResults = [];
  for (const target of selectedTargets) {
    if (target.client === primaryTarget.client) {
      continue;
    }
    secondaryResults.push(await installSecondarySkills(
      target,
      primaryInstalled,
      {
        agentSkillSource: sources.agentSkillSource,
        envTemplateSource: sources.envTemplateSource
      },
      linkSecondary,
      {
        interactive: process.stdin.isTTY && process.stdout.isTTY,
        yes: !(process.stdin.isTTY && process.stdout.isTTY)
      }
    ));
  }

  console.log("AI 安装完成：");
  console.log(`- 主链安装根: ${primaryTarget.label || primaryTarget.client}`);
  console.log(`- 主链 skills 根目录: ${primaryTarget.skillsDir}`);
  console.log(`- 已更新 skill: ${primaryInstalled.agentSkillTarget}`);
  console.log(`- 已更新 skill: ${path.join(primaryInstalled.logSkillDir, "SKILL.md")}`);
  console.log(`- ${primaryInstalled.envCreated ? "已初始化" : "已保留"} env-map: ${primaryInstalled.envMapTarget}`);
  console.log(`- 结构化 env-map: ${primaryInstalled.envMapJsonTarget}`);
  console.log(`- ${configCreated ? "已初始化" : "已保留"} config: ${configTarget}`);

  for (const result of secondaryResults) {
    if (result.mode === "symlink") {
      console.log(`- 已为 ${KNOWN_CLIENTS[result.client]?.label || result.client} 建立复用目录: ${result.skillsDir}`);
      if (result.agentResult.backupPath) {
        console.log(`  - 已备份原 agent-ssh-cli 目录到: ${result.agentResult.backupPath}`);
      }
      if (result.logResult.backupPath) {
        console.log(`  - 已备份原 log-analyze 目录到: ${result.logResult.backupPath}`);
      }
    } else {
      console.log(`- 已复制安装到 ${KNOWN_CLIENTS[result.client]?.label || result.client}: ${result.skillsDir}`);
    }
  }

  printNextSteps({
    selectedTargets,
    primaryTarget,
    envMapTarget: primaryInstalled.envMapTarget,
    configTarget,
    linkSecondary
  });

  return 0;
}

if (process.argv[2] === "install-ai") {
  try {
    const exitCode = await runInstallAi(process.argv.slice(3));
    process.exit(exitCode);
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  }
}

if (process.argv[2] === "doctor-skills") {
  try {
    const exitCode = await runDoctorSkills(process.argv.slice(3));
    process.exit(exitCode);
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  }
}

if (process.argv[2] === "sync-skills") {
  try {
    const exitCode = await runSyncSkills(process.argv.slice(3));
    process.exit(exitCode);
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  }
}

if (process.argv[2] === "env-map") {
  try {
    const exitCode = await runEnvMap(process.argv.slice(3));
    process.exit(exitCode);
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  }
}

function getExecutableName() {
  return process.platform === "win32" ? "agentsshcli-native.exe" : "agentsshcli-native";
}

function getPlatformPackageBinary() {
  const packageName = `@2red1blue/agentsshcli-${process.platform}-${process.arch}`;
  try {
    const packageJsonPath = require.resolve(`${packageName}/package.json`);
    return path.join(path.dirname(packageJsonPath), "bin", getExecutableName());
  } catch {
    return undefined;
  }
}

function getCandidatePaths() {
  const executableName = getExecutableName();
  return [
    getPlatformPackageBinary(),
    path.join(projectRoot, "native-bin", `${process.platform}-${process.arch}`, executableName),
    path.join(projectRoot, "native-bin", executableName),
    path.join(projectRoot, "native", "target", "release", executableName),
    path.join(projectRoot, "native", "target", "debug", executableName)
  ].filter(Boolean);
}

function findNativeBinary() {
  return getCandidatePaths().find((candidate) => fs.existsSync(candidate));
}

const binaryPath = findNativeBinary();
if (!binaryPath) {
  console.error(`未找到 ${process.platform}-${process.arch} 的 Rust 原生可执行文件，请安装对应平台包或先运行 npm run build:native-bin`);
  process.exit(1);
}

const result = spawnSync(binaryPath, process.argv.slice(2), {
  stdio: "inherit"
});

if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}

process.exit(result.status ?? 1);
