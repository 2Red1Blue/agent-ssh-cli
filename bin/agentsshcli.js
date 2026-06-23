#!/usr/bin/env node
import { createRequire } from "node:module";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const currentDir = path.dirname(fileURLToPath(import.meta.url));
const projectRoot = path.resolve(currentDir, "..");
const homeDir = os.homedir();

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

function printInstallAiHelp() {
  console.log(`用法:
  agentsshcli install-ai [--skills-dir <path>] [--config-dir <path>]

说明:
  安装或更新以下本地资产：
  - ~/.codex/skills/agent-ssh-cli/SKILL.md
  - ~/.codex/skills/log-analyze/SKILL.md
  - ~/.codex/skills/log-analyze/env-map.md（仅不存在时初始化模板）
  - ~/.agent-ssh-cli/config.json（仅不存在时初始化示例配置）

参数:
  --skills-dir <path>  指定 skills 根目录，默认 ~/.codex/skills
  --config-dir <path>  指定配置目录，默认 ~/.agent-ssh-cli
  --help               显示帮助
`);
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

function runInstallAi(args) {
  let skillsDir = path.join(homeDir, ".codex", "skills");
  let configDir = path.join(homeDir, ".agent-ssh-cli");

  for (let i = 0; i < args.length; i += 1) {
    const arg = args[i];
    if (arg === "--help" || arg === "-h") {
      printInstallAiHelp();
      process.exit(0);
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
    console.error(`未知参数: ${arg}`);
    printInstallAiHelp();
    process.exit(1);
  }

  if (!skillsDir || !configDir) {
    console.error("skills-dir 或 config-dir 无效");
    process.exit(1);
  }

  const agentSkillSource = path.join(projectRoot, "SKILL.md");
  const logSkillSource = path.join(projectRoot, "skills", "log-analyze", "SKILL.md");
  const envTemplateSource = path.join(projectRoot, "skills", "log-analyze", "env-map.template.md");
  const exampleConfigSource = path.join(projectRoot, "example.config.json");

  const agentSkillTarget = path.join(skillsDir, "agent-ssh-cli", "SKILL.md");
  const logSkillTarget = path.join(skillsDir, "log-analyze", "SKILL.md");
  const envMapTarget = path.join(skillsDir, "log-analyze", "env-map.md");
  const configTarget = path.join(configDir, "config.json");

  copyFileEnsured(agentSkillSource, agentSkillTarget);
  copyFileEnsured(logSkillSource, logSkillTarget);
  const envCreated = writeFileIfMissing(envTemplateSource, envMapTarget);
  const configCreated = writeFileIfMissing(exampleConfigSource, configTarget);

  console.log("AI 安装完成：");
  console.log(`- 已更新 skill: ${agentSkillTarget}`);
  console.log(`- 已更新 skill: ${logSkillTarget}`);
  console.log(`- ${envCreated ? "已初始化" : "已保留"} env-map: ${envMapTarget}`);
  console.log(`- ${configCreated ? "已初始化" : "已保留"} config: ${configTarget}`);
  console.log("");
  console.log("后续只需要按真实环境补全：");
  console.log("- ~/.agent-ssh-cli/config.json");
  console.log(`- ${envMapTarget}`);
}

if (process.argv[2] === "install-ai") {
  runInstallAi(process.argv.slice(3));
  process.exit(0);
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
