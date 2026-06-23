#!/usr/bin/env node
import { createRequire } from "node:module";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import readline from "node:readline/promises";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const currentDir = path.dirname(fileURLToPath(import.meta.url));
const projectRoot = path.resolve(currentDir, "..");
const homeDir = os.homedir();
const KNOWN_CLIENTS = {
  codex: {
    label: "Codex",
    skillsDir: path.join(homeDir, ".codex", "skills")
  },
  claude: {
    label: "Claude Code",
    skillsDir: path.join(homeDir, ".claude", "skills")
  },
  opencode: {
    label: "OpenCode",
    skillsDir: path.join(homeDir, ".config", "opencode", "skills")
  },
  hermes: {
    label: "Hermes",
    skillsDir: path.join(homeDir, ".hermes", "skills")
  }
};

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

function copySkillDir(sourceDir, targetDir) {
  ensureDir(targetDir);
  for (const entry of fs.readdirSync(sourceDir, { withFileTypes: true })) {
    const sourcePath = path.join(sourceDir, entry.name);
    const targetPath = path.join(targetDir, entry.name);

    if (entry.isDirectory()) {
      copySkillDir(sourcePath, targetPath);
      continue;
    }

    ensureDir(path.dirname(targetPath));
    fs.copyFileSync(sourcePath, targetPath);
  }
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

function installPrimarySkills(skillsDir, sources) {
  const agentSkillDir = path.join(skillsDir, "agent-ssh-cli");
  const logSkillDir = path.join(skillsDir, "log-analyze");
  const envMapTarget = path.join(logSkillDir, "env-map.md");

  const agentSkillTarget = installSingleSkillFile(sources.agentSkillSource, agentSkillDir);
  copySkillDir(sources.logSkillDir, logSkillDir);
  const envCreated = writeFileIfMissing(sources.envTemplateSource, envMapTarget);

  return {
    agentSkillDir,
    agentSkillTarget,
    logSkillDir,
    envMapTarget,
    envCreated
  };
}

function installSecondarySkills(target, primaryInstalled, sources, linkSecondary) {
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
  copySkillDir(sources.logSkillDir, logSkillDir);
  const envMapTarget = path.join(logSkillDir, "env-map.md");
  const envCreated = writeFileIfMissing(sources.envTemplateSource, envMapTarget);
  return {
    client: target.client,
    skillsDir: target.skillsDir,
    mode: "copy",
    envCreated
  };
}

async function promptInstallPlan(defaultClients, clientRoots) {
  const rl = readline.createInterface({
    input: process.stdin,
    output: process.stdout
  });

  try {
    console.log("选择要安装的客户端 skills 目录。");
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

    const defaultPrimary = selectedClients[0];
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
  console.log("");
  console.log("下一步标准流程：");
  console.log("1. 重启以下客户端，让新 skill 被重新扫描：");
  for (const target of selectedTargets) {
    console.log(`   - ${KNOWN_CLIENTS[target.client]?.label || target.client}: ${target.skillsDir}`);
  }
  console.log("2. 进入任一已重启客户端后，先初始化共享 CLI 配置：");
  console.log(`   - 共享配置路径: ${configTarget}`);
  console.log("   - 推荐让 AI 按 add-jump-server 流程，每次只问一个问题，最终写入该文件");
  console.log("3. 再初始化 log-analyze 的私有环境映射：");
  console.log(`   - 主链 env-map: ${envMapTarget}`);
  console.log(`   - 其他客户端${linkSecondary ? "会通过软链复用这份 env-map" : "各自保留本地 env-map"}`);
  console.log("4. 做最小验证：");
  console.log("   - agentsshcli list");
  console.log("   - agentsshcli jump-exec --timeout 120000 <jumpserver-connection> --target <known-target> \"hostname\"");
  console.log("5. 最后确认客户端技能列表中已经能看到 log-analyze。");
  console.log("");
  console.log("推荐交给 AI 的标准提示词：");
  console.log("");
  console.log("A. 初始化 JumpServer 配置");
  console.log(`请帮我初始化 agent-ssh-cli 的 JumpServer 配置。请按 README 里的 add-jump-server 流程每次只问我一个问题，收集完后执行 agentsshcli add-jump-server 写入 ${configTarget}，并用 agentsshcli list 和 jump-exec hostname 做最小验证。`);
  console.log("");
  console.log("B. 初始化 log-analyze 环境映射");
  console.log(`请帮我初始化 log-analyze 的环境映射。请每次只问我一个问题，收集 JumpServer 名称、环境日志根目录、项目简称、默认 target、机器简称映射，写入 ${envMapTarget}，并用 agentsshcli jump-exec 做最小验证，直到客户端里可以正常使用 log-analyze。`);
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
    logSkillDir: path.join(projectRoot, "skills", "log-analyze"),
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
    const plan = await promptInstallPlan(["codex"], clientRoots);
    selectedClients = plan.selectedClients;
    primaryClient = plan.primaryClient;
    linkSecondary = plan.linkSecondary;
  } else {
    selectedClients = ["codex"];
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

  const resolvedPrimaryClient = skillsDir ? "custom" : (primaryClient || selectedTargets[0].client);
  const primaryTarget = selectedTargets.find((target) => target.client === resolvedPrimaryClient);
  if (!primaryTarget) {
    throw new Error(`未找到主链客户端: ${resolvedPrimaryClient}`);
  }

  const primaryInstalled = installPrimarySkills(primaryTarget.skillsDir, {
    agentSkillSource: sources.agentSkillSource,
    logSkillDir: sources.logSkillDir,
    envTemplateSource: sources.envTemplateSource
  });

  const secondaryResults = [];
  for (const target of selectedTargets) {
    if (target.client === primaryTarget.client) {
      continue;
    }
    secondaryResults.push(
      installSecondarySkills(
        target,
        primaryInstalled,
        {
          agentSkillSource: sources.agentSkillSource,
          logSkillDir: sources.logSkillDir,
          envTemplateSource: sources.envTemplateSource
        },
        linkSecondary
      )
    );
  }

  console.log("AI 安装完成：");
  console.log(`- 主链客户端: ${primaryTarget.label || primaryTarget.client}`);
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
