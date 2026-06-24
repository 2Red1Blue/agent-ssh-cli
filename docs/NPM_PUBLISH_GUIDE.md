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

当前版本策略应保持主包和平台包完全一致，例如 `0.1.2`。

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
for npm_bin in $(which -a npm 2>/dev/null | awk '!seen[$0]++'); do
  prefix="$("$npm_bin" prefix -g 2>/dev/null)" || continue
  case " ${SEEN_PREFIXES:-} " in
    *" $prefix "*) continue ;;
  esac
  SEEN_PREFIXES="${SEEN_PREFIXES:-} $prefix"
  "$npm_bin" install -g @2red1blue/agentsshcli || exit 1
done
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
- `npm publish --provenance` 能自动生成 provenance

当前仓库已经切到 Trusted Publishing 方案，实际使用的 workflow 是：

- `.github/workflows/build-native-packages.yml`
- `.github/workflows/publish.yml`

其中 `publish.yml` 的要点是：

- 触发条件：`workflow_dispatch` 或 `push tags: v*`
- `permissions.id-token: write`
- 使用 GitHub-hosted runner
- `actions/setup-node` 版本为 Node `22.14.0`
- npm CLI 升级到 `11.5.1`
- 发布时直接执行 `npm publish --provenance --access public`
- 不再使用 `NPM_TOKEN` / `NODE_AUTH_TOKEN`

### 首次启用 Trusted Publishing

> [!warning] 首次启用时，必须先在 npm 为每个已存在的包建立 trusted publisher 关系。
> `@2red1blue/agentsshcli` 这 6 个包目前都已经存在，因此可以直接配置。

npm 官方截至 2026-06-23 的要求里，Trusted Publishing 依赖：

- GitHub Actions 的 GitHub-hosted runner
- Node `22.14.0` 或更高
- npm `11.5.1` 或更高用于发布
- 包必须已存在于 npm registry

如果你想用 CLI 批量完成配置，先确保本地：

```bash
npm install -g npm@^11.15.0
npm whoami
```

然后执行：

```bash
for pkg in \
  @2red1blue/agentsshcli \
  @2red1blue/agentsshcli-darwin-arm64 \
  @2red1blue/agentsshcli-darwin-x64 \
  @2red1blue/agentsshcli-linux-arm64 \
  @2red1blue/agentsshcli-linux-x64 \
  @2red1blue/agentsshcli-win32-x64
do
  npm trust github "$pkg" \
    --repo 2Red1Blue/agent-ssh-cli \
    --file publish.yml \
    --allow-publish \
    --yes
  sleep 2
done
```

执行后可验证：

```bash
npm trust list @2red1blue/agentsshcli
```

如果后面想撤销或重建：

```bash
npm trust list @2red1blue/agentsshcli
npm trust revoke @2red1blue/agentsshcli --id <trust-id>
```

### 自动发版操作

Trusted Publishing 配置完成后，后续发版流程就是：

1. 更新版本号
2. 运行 `npm run check:release`
3. 运行 `npm test`
4. 提交代码并推送
5. 打 tag 并推送

```bash
git tag v0.1.2
git push origin v0.1.2
```

GitHub Actions 会自动：

1. 分平台构建 Rust 二进制
2. 发布 5 个平台包
3. 发布主包
4. 创建 GitHub Release

### 触发方式说明

`publish.yml` 当前支持两种触发方式：

- 自动触发：`push.tags = v*`
- 手动触发：`workflow_dispatch`

对应 workflow 文件：

- `.github/workflows/publish.yml`

也就是说，正常情况下只要推送形如 `v0.1.2` 的 tag，就应该自动开始发版：

```bash
git tag v0.1.2
git push origin v0.1.2
```

如果需要显式触发一次手动发布，可执行：

```bash
gh workflow run publish.yml \
  --repo 2Red1Blue/agent-ssh-cli \
  --ref v0.1.2
```

这里的 `--ref` 推荐直接指定 tag，而不是 `main`，这样可以确保发布内容和目标版本完全一致。

### 如何确认是否已经触发

先看最近的 workflow run：

```bash
gh run list \
  --repo 2Red1Blue/agent-ssh-cli \
  --limit 10 \
  --json databaseId,workflowName,displayTitle,event,status,conclusion,headBranch,headSha,createdAt,url
```

重点看：

- `workflowName` 是否为 `publish`
- `event` 是 `push` 还是 `workflow_dispatch`
- `headBranch` 是否为目标 tag，例如 `v0.1.2`
- `headSha` 是否等于本次 release commit

如果想持续观察某一次 run：

```bash
gh run watch <run-id> --repo 2Red1Blue/agent-ssh-cli --exit-status
```

如果想看更细的 job 结构：

```bash
gh run view <run-id> --repo 2Red1Blue/agent-ssh-cli --json status,conclusion,jobs,url
```

### 如果 tag 已推送但没有自动触发

先检查 tag 是否真的在远端：

```bash
git ls-remote --tags origin v0.1.2
```

再检查 workflow 最近是否产生了 `push` 事件的 run。若没有，而你又确认：

- 代码已经推到远端
- tag 也已经推到远端
- `publish.yml` 仍包含 `push.tags: 'v*'`

则可先手动补触发：

```bash
gh workflow run publish.yml \
  --repo 2Red1Blue/agent-ssh-cli \
  --ref v0.1.2
```

这次 `v0.1.2` 的实际情况就是：

- `main` 和 tag 已经成功推到 GitHub
- 自动 `push tag` 没有生成新的 `publish` run
- 后续通过 `workflow_dispatch` 对 `v0.1.2` 手动补触发，发布继续执行

所以维护上建议统一按下面顺序确认：

1. 先推 `main`
2. 再推 tag
3. 看 `gh run list` 是否出现新的 `publish` run
4. 如果没有，再执行 `gh workflow run publish.yml --ref <tag>`
5. 用 `gh run watch` 盯到平台包、主包、GitHub Release 全部完成

### 首发与后续发版的区别

- 首发前如果包还不存在，不能先配 Trusted Publishing，必须先手工发布一次
- 当前这 6 个包都已经存在 `0.1.0`，后续发布 `0.1.2` 及更高版本时可以直接使用 Trusted Publishing 流程
- 切换成功后，建议删除仓库里的旧 `NPM_TOKEN` secret，避免长期凭证继续留存

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
