#!/usr/bin/env node
import { createRequire } from "node:module";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const currentDir = path.dirname(fileURLToPath(import.meta.url));
const projectRoot = path.resolve(currentDir, "..");

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
