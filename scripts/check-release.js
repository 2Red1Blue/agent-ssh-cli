#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const projectRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const packageJson = JSON.parse(fs.readFileSync(path.join(projectRoot, "package.json"), "utf8"));
const packageLock = JSON.parse(fs.readFileSync(path.join(projectRoot, "package-lock.json"), "utf8"));
const cargoToml = fs.readFileSync(path.join(projectRoot, "native", "Cargo.toml"), "utf8");
const version = packageJson.version;

function fail(message) {
  console.error(message);
  process.exit(1);
}

if (packageLock.version !== version) {
  fail(`package-lock.json version=${packageLock.version} 与 package.json version=${version} 不一致`);
}

const rootPackageVersion = packageLock.packages?.[""]?.version;
if (rootPackageVersion !== version) {
  fail(`package-lock.json packages[\"\"] version=${rootPackageVersion} 与 package.json version=${version} 不一致`);
}

const cargoMatch = cargoToml.match(/^version\s*=\s*"([^"]+)"$/m);
if (!cargoMatch) {
  fail("native/Cargo.toml 未找到 version");
}
if (cargoMatch[1] !== version) {
  fail(`native/Cargo.toml version=${cargoMatch[1]} 与 package.json version=${version} 不一致`);
}

for (const [pkg, expected] of Object.entries(packageJson.optionalDependencies || {})) {
  if (expected !== version) {
    fail(`optionalDependencies ${pkg}=${expected} 与 package.json version=${version} 不一致`);
  }
  const suffix = pkg.replace("@2red1blue/agentsshcli-", "");
  const subPackageJsonPath = path.join(projectRoot, "npm", suffix, "package.json");
  if (!fs.existsSync(subPackageJsonPath)) {
    fail(`未找到平台包 package.json: ${subPackageJsonPath}`);
  }
  const subPackageJson = JSON.parse(fs.readFileSync(subPackageJsonPath, "utf8"));
  if (subPackageJson.name !== pkg) {
    fail(`平台包名字不一致: 期望 ${pkg}，实际 ${subPackageJson.name}`);
  }
  if (subPackageJson.version !== version) {
    fail(`平台包 ${pkg} version=${subPackageJson.version} 与 package.json version=${version} 不一致`);
  }
}

console.log(`release check passed for version ${version}`);
