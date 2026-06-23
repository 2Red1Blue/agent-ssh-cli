#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const projectRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const packageJson = JSON.parse(fs.readFileSync(path.join(projectRoot, "package.json"), "utf8"));
const version = packageJson.version;
const platformPackages = Object.keys(packageJson.optionalDependencies || {});

const args = new Set(process.argv.slice(2));
const dryRun = args.has("--dry-run");
const mainOnly = args.has("--main-only");
const platformsOnly = args.has("--platforms-only");
const platformArgIndex = process.argv.indexOf("--platform");
const platformFilter = platformArgIndex >= 0 ? process.argv[platformArgIndex + 1] : "";

if (mainOnly && platformsOnly) {
  console.error("不能同时使用 --main-only 和 --platforms-only");
  process.exit(1);
}

if (platformArgIndex >= 0 && !platformFilter) {
  console.error("--platform 后必须跟目录名，例如 darwin-arm64");
  process.exit(1);
}

const selectedPlatformPackages = platformFilter
  ? platformPackages.filter((pkg) => pkg.endsWith(`-${platformFilter}`))
  : platformPackages;

if (platformFilter && selectedPlatformPackages.length === 0) {
  console.error(`未找到平台包: ${platformFilter}`);
  process.exit(1);
}

function run(cmd, cmdArgs, cwd = projectRoot) {
  console.log(`\n> ${cmd} ${cmdArgs.join(" ")}`);
  execFileSync(cmd, cmdArgs, {
    cwd,
    stdio: "inherit",
  });
}

function getExecutableNameForDir(dirName) {
  return dirName.startsWith("win32-") ? "agentsshcli-native.exe" : "agentsshcli-native";
}

function npmViewVersion(pkg) {
  try {
    return execFileSync("npm", ["view", `${pkg}@${version}`, "version"], {
      cwd: projectRoot,
      stdio: ["ignore", "pipe", "ignore"],
      encoding: "utf8",
    }).trim();
  } catch {
    return "";
  }
}

function publishPackage(pkg, cwd) {
  if (dryRun) {
    run("npm", ["pack", "--dry-run"], cwd);
    return;
  }
  try {
    run("npm", ["publish", "--access", "public"], cwd);
  } catch (error) {
    if (npmViewVersion(pkg) === version) {
      console.log(`${pkg}@${version} 已存在，跳过`);
      return;
    }
    throw error;
  }
}

for (const pkg of selectedPlatformPackages) {
  const suffix = pkg.replace("@2red1blue/agentsshcli-", "");
  const cwd = path.join(projectRoot, "npm", suffix);
  if (!fs.existsSync(path.join(cwd, "package.json"))) {
    console.error(`未找到平台包目录: ${cwd}`);
    process.exit(1);
  }
}

if (!mainOnly) {
  for (const pkg of selectedPlatformPackages) {
    const suffix = pkg.replace("@2red1blue/agentsshcli-", "");
    const cwd = path.join(projectRoot, "npm", suffix);
    const executableName = getExecutableNameForDir(suffix);
    const binaryPath = path.join(cwd, "bin", executableName);
    if (!fs.existsSync(binaryPath)) {
      console.error(`平台包 ${pkg} 缺少二进制文件: ${binaryPath}`);
      console.error("请先为该平台构建 native-bin 并执行 node scripts/build-native-package.js");
      process.exit(1);
    }
    publishPackage(pkg, cwd);
  }
}

if (!platformsOnly) {
  publishPackage(packageJson.name, projectRoot);
}
