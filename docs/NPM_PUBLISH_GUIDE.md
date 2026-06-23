---
title: AgentSSHCLI NPM Publish Guide
date: 2026-06-23
tags:
  - agentsshcli
  - npm
  - publish
  - obsidian
aliases:
  - AgentSSHCLI 发布流程
  - NPM 发布说明
status: active
---

# AgentSSHCLI NPM Publish Guide

这份说明记录 `@2red1blue/agentsshcli` 的 npm 发布方式，包括平台包结构、手动发布步骤、费用说明，以及后续自动化发布建议。

> [!info] 当前包结构
> 主包名是 `@2red1blue/agentsshcli`。
> Rust 预编译平台包分别发布为：
> - `@2red1blue/agentsshcli-darwin-arm64`
> - `@2red1blue/agentsshcli-darwin-x64`
> - `@2red1blue/agentsshcli-linux-arm64`
> - `@2red1blue/agentsshcli-linux-x64`
> - `@2red1blue/agentsshcli-win32-x64`

## 发布原理

主包只包含：

- `bin/agentsshcli.js`
- `README.md`
- `example.config.json`

真正执行 SSH / JumpServer 逻辑的是 Rust 原生程序，它不会直接塞进主包，而是通过 `optionalDependencies` 按平台安装对应子包。这样做的好处是：

- 用户安装时不需要本地 Rust toolchain
- 不会把所有平台二进制都塞进一个 npm 包
- 不同平台可以独立构建和发布

## 什么时候可以发布

要满足下面几个条件：

1. 你拥有 npm scope `@2red1blue`
2. 你已经登录 npm：`npm login`
3. 首次发布 scoped public 包时使用 `--access public`
4. 本次要发布的版本号从未发布过
5. 如果 npm 账号启用了 2FA，需要按 npm 要求完成发布验证，或者使用符合要求的 token / trusted publishing

> [!warning] 版本不可重复
> npm 上的 `name@version` 一旦发布，就不能重复使用。
> 即使包后来被删除，原版本号也不应该再尝试复用。

## 免费还是收费

> [!success] 结论
> 公开 npm 包发布通常是免费的。
> GitHub 公共仓库使用标准 GitHub-hosted runners 的 Actions，通常也是免费的。

具体理解：

- `@2red1blue/agentsshcli` 这种 public 包，一般不需要额外付费
- 如果以后改成 private package，才可能涉及 npm 付费
- 如果 GitHub 仓库是 private，则 Actions 会走免费额度，超出后可能计费
- 如果使用 larger runners，也可能计费

## 手动发布流程

### 1. 确认版本

先确认以下文件里的版本一致：

- `package.json`
- `package-lock.json`
- `native/Cargo.toml`
- `npm/*/package.json`

当前版本策略应保持主包和平台包完全一致，例如 `0.1.0`。

### 2. 本地验证

```bash
npm run check:release
npm test
npm pack --dry-run
```

如果要重新生成本机平台的 Rust 产物：

```bash
npm run build:native
```

### 3. 生成平台包产物

当前仓库已有脚本：

- `scripts/build-native-bin.js`
- `scripts/build-native-package.js`

本机平台可直接生成：

```bash
npm run build:native-bin
node scripts/build-native-package.js
```

如果要发布全部平台，通常需要：

- 在对应平台机器上构建
- 或通过 CI 分平台构建
- 把二进制拷贝到 `native-bin/<platform>-<arch>/`
- 再生成各自 npm 子包

### 4. 先发布平台包

必须先发布平台包，再发布主包。示例：

```bash
cd npm/darwin-arm64
npm publish --access public

cd ../darwin-x64
npm publish --access public

cd ../linux-arm64
npm publish --access public

cd ../linux-x64
npm publish --access public

cd ../win32-x64
npm publish --access public
```

### 5. 最后发布主包

回到仓库根目录：

```bash
cd /Users/liuzx/Code/java/work/agent-ssh-cli
npm publish --access public
```

如果想让本地脚本按“平台包优先，主包最后”的顺序执行：

```bash
npm run publish:packages
```

只做演练但不真正发布：

```bash
npm run publish:packages:dry-run
```

### 6. 安装验证

发布后，验证最终用户安装命令：

```bash
npm install -g @2red1blue/agentsshcli
agentsshcli --version
agentsshcli --help
```

## 推荐发布顺序

> [!tip] 推荐顺序
> 1. 改版本
> 2. `npm run check:release`
> 3. `npm test`
> 4. 构建平台二进制
> 5. 发布 5 个平台包
> 6. 发布主包
> 7. 全新环境安装验证

## 自动化发布建议

自动化发布推荐使用 GitHub Actions + npm Trusted Publishing。

优点：

- 不需要长期保存 npm token
- 可以按 tag 自动发版
- 可以在不同 runner 上分别构建平台包
- 发布顺序更容易标准化

一个合理的后续拆分是：

1. `build-native` job
2. 每个平台单独产出 artifact
3. `publish-platform-packages` job
4. `publish-main-package` job

当前仓库已经有可继续使用的 workflow：

- `.github/workflows/build-native-packages.yml`
- `.github/workflows/publish.yml`

> [!note]
> 如果后续要做自动化，建议再补一份 workflow，明确：
> - 触发条件
> - tag 规范
> - 每个平台 runner
> - npm trusted publishing 配置

当前仓库也支持先走 `NPM_TOKEN` secret 方案：

- GitHub repo secret 名称：`NPM_TOKEN`
- workflow 中通过 `NODE_AUTH_TOKEN` 注入
- `actions/setup-node` 使用 npm registry

这条路线的优点是首发更快，不需要先满足 `npm trust` 的前置条件。

## 常见问题

### 为什么不能只发布主包

因为主包本身不包含 Rust 原生二进制。用户安装完主包后，`bin/agentsshcli.js` 会去查找对应平台包中的可执行文件；如果平台包不存在，CLI 就不能正常运行。 ^why-platform-packages

### 为什么本地能用，别人安装却不行

本地开发环境可能直接软链到了源码目录，或者本地已经手工执行过 `npm run build:native`。别人从 npm 安装时只会拿到你发布出去的包，因此平台包必须同步存在。 ^why-local-works

### 什么时候只需要重新编译，不需要重新全局安装

如果全局安装已经是软链到本地仓库的模式，改 Rust 代码后通常只需要重新执行：

```bash
npm run build:native
```

如果是普通的 npm 全局安装，或者你调整了包结构 / 包名 / 依赖关系，则需要重新安装。 ^when-reinstall

## 相关笔记

- [[AI_FIRST_RUN_LOG_ANALYZE]]
- [[NPM 发布说明#^why-platform-packages|为什么必须先发平台包]]
- [[NPM 发布说明#^why-local-works|为什么本地能用但别人不行]]

## 外部参考

- [npm scoped public packages](https://docs.npmjs.com/creating-and-publishing-scoped-public-packages/)
- [npm publish](https://docs.npmjs.com/cli/v8/commands/npm-publish)
- [npm trusted publishers](https://docs.npmjs.com/trusted-publishers)
- [GitHub Actions billing](https://docs.github.com/billing/managing-billing-for-github-actions/about-billing-for-github-actions)
