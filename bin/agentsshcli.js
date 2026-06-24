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
const MANAGED_START_MARKER = "<!-- agentsshcli:managed:start -->";
const MANAGED_END_MARKER = "<!-- agentsshcli:managed:end -->";
const LOCAL_START_MARKER = "<!-- agentsshcli:local:start -->";
const LOCAL_END_MARKER = "<!-- agentsshcli:local:end -->";
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

function computeSha256(value) {
  return crypto.createHash("sha256").update(normalizeNewlines(value), "utf8").digest("hex");
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
  const logSkillDir = path.join(target.skillsDir, LOG_SKILL_NAME);
  const skillFile = path.join(logSkillDir, "SKILL.md");
  const envTemplateTarget = path.join(logSkillDir, "env-map.template.md");
  const envMapTarget = path.join(logSkillDir, "env-map.md");
  const envTemplateSource = path.join(projectRoot, "skills", LOG_SKILL_NAME, "env-map.template.md");

  ensureDir(logSkillDir);
  const envTemplateCreated = writeFileIfMissing(envTemplateSource, envTemplateTarget);
  const envCreated = writeFileIfMissing(envTemplateSource, envMapTarget);

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
  const envMapTarget = path.join(logSkillDir, "env-map.md");

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
  console.log("   - 你想添加哪些常用主机");
  console.log("   - 这些主机或项目平时有哪些简称 / 别名");
  console.log("   - 这些项目的日志通常在哪个目录");
  console.log("   - 剩下的 JumpServer 菜单确认、主机搜索、真实 hostname/IP 回填，都应由 AI 完成");
  console.log("5. 再初始化 log-analyze 的私有环境映射：");
  console.log(`   - 主链 env-map: ${envMapTarget}`);
  console.log(`   - 其他客户端${linkSecondary ? "会通过软链复用这份 env-map" : "各自保留本地 env-map"}`);
  console.log("   - 这个文件建议由你当前正在使用的 AI 自行维护");
  console.log("   - 你通常只需要告诉 AI：常用主机、别名、日志目录");
  console.log("   - 下面有可直接复制给 AI 的提示词");
  console.log("6. 做最小验证：");
  console.log("   - agentsshcli list");
  console.log("   - agentsshcli jump-menu <jumpserver-connection>");
  console.log("   - agentsshcli jump-exec --timeout 120000 <jumpserver-connection> --target <known-target> \"hostname\"");
  console.log("7. 最后确认客户端技能列表中已经能看到 log-analyze。");
  console.log("");
  console.log("推荐交给 AI 的标准提示词：");
  console.log("");
  console.log("A. 初始化 JumpServer 配置");
  console.log(`请帮我初始化 agent-ssh-cli 的 JumpServer 配置。请按 README 里的 add-jump-server 流程每次只问我一个问题，收集完后执行 agentsshcli add-jump-server 写入 ${configTarget}。写入后先执行 agentsshcli jump-menu <jumpserver-connection>，把当前 JumpServer 的 Opt 菜单完整展示给我并确认这个跳板机怎么列主机、怎么搜索主机；这些确认完成后，再继续后面的最小验证。`);
  console.log("");
  console.log("B. 初始化 log-analyze 环境映射");
  console.log(`请直接维护当前主链 env-map 文件：${envMapTarget}。先用一句话告诉我这一步是在补“常用主机、主机/项目别名、日志目录”的私有信息，后面查日志时你才能自动定位。然后第一步先连接 JumpServer，执行 agentsshcli jump-menu <jumpserver-connection>，把当前 JumpServer 的 Opt 菜单完整展示给我。之后每次只问我一个问题，但只需要向我收集三类信息：我想添加哪些常用主机、这些主机或项目平时有哪些简称 / 别名、这些项目日志通常在哪个目录。JumpServer 菜单确认、主机搜索、真实 hostname / IP 验证、以及写回 ${envMapTarget} 这些动作都由你自己完成；不要一开始就要求我提供完整 hostname。若我给的是简称，先在 JumpServer 菜单层查出真实 hostname / IP，再回显给我并写入映射，然后继续补日志路径，直到客户端里可以正常使用 log-analyze。`);
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
