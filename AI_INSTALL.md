# AI 安装说明

如果你想让 AI 直接帮你安装，可以把下面这句话原样发给 AI：

```text
安装请阅读 https://github.com/2Red1Blue/agent-ssh-cli/blob/main/AI_INSTALL.md，按说明安装 CLI 并添加 `SKILL.md`。
```

AI 读完这份文档后，默认应完成：

1. 安装 CLI：`@2red1blue/agentsshcli`
2. 安装 `agent-ssh-cli` skill
3. 安装 `log-analyze` skill
4. 初始化本地 skill 目录结构和配置模板

安装完成后的状态应该是：

- CLI 和 2 个 skill 都已经就位
- 只剩 `~/.agent-ssh-cli/config.json` 与 `log-analyze/env-map.md` 需要按你的真实环境交互式补全

使用本工具时，先按下面步骤安装 CLI 和 skill。CLI 仍通过 npm 安装，内部 SSH 执行逻辑由 Rust 原生执行器完成。

npm 安装会按当前系统自动拉取对应平台的 optional 预编译包；当前支持 macOS arm64/x64、Linux x64/arm64、Windows x64。


## 1. 安装 CLI

推荐一行命令：

```bash
npm install -g @2red1blue/agentsshcli && agentsshcli install-ai
```

这条命令会：

- 安装 CLI
- 安装 `agent-ssh-cli` skill
- 安装 `log-analyze` skill
- 初始化 `~/.agent-ssh-cli/config.json` 示例配置
- 初始化 `~/.codex/skills/log-analyze/env-map.md` 模板

如果你的 AI 不是使用 Codex 默认 skills 目录，也可以改成：

```bash
npm install -g @2red1blue/agentsshcli && agentsshcli install-ai --skills-dir <你的-skills-目录>
```

如果只想先单独安装 CLI，再手动执行安装器，也可以用下面两步：

```bash
npm install -g @2red1blue/agentsshcli
agentsshcli --help
agentsshcli install-ai
```

如果 `agentsshcli --help` 失败，先检查：

```bash
node --version
npm --version
```

需要 Node.js `>= 18` 和 npm `>= 8`。正式 npm 包会携带或使用已构建的原生执行器；从源码本地调试时需要额外执行：

```bash
npm run build:native
npm test
```

## 2. 初始化配置目录

```bash
mkdir -p ~/.agent-ssh-cli
```

然后创建并编辑：

```text
~/.agent-ssh-cli/config.json
```

配置内容可参考项目中的 [example.config.json](https://github.com/2Red1Blue/agent-ssh-cli/blob/main/example.config.json)。配置文件保存真实服务器连接信息，不要公开。可以询问用户如何配置或者告知用户配置目录。

## 3. 安装 skills

至少安装本仓库根目录的 [SKILL.md](https://github.com/2Red1Blue/agent-ssh-cli/blob/main/SKILL.md)。

如果你还希望 AI 直接具备“日志分析 + JumpServer 跳转”能力，再额外安装 `log-analyze` skill。首次安装时，`log-analyze` 只应包含**通用流程 + 默认检索策略**，不要预置任何个人或团队专属的 IP / hostname / 环境映射；这部分应在首次使用后由 AI 交互式补齐到私有 `env-map.md`。

`agentsshcli install-ai` 默认会把 skill 安装到 Codex 默认目录：

```text
~/.codex/skills/
```

如果你仍想手工安装，Codex 默认目录示例：

```bash
mkdir -p ~/.codex/skills/agent-ssh-cli
cp SKILL.md ~/.codex/skills/agent-ssh-cli/SKILL.md
```

安装 `log-analyze` 的示例：

```bash
mkdir -p ~/.codex/skills/log-analyze
cp skills/log-analyze/SKILL.md ~/.codex/skills/log-analyze/SKILL.md
```

如果 AI 使用其它 skills 目录，将这些 `SKILL.md` 复制到对应的 `<skill-name>/SKILL.md`。如果 `log-analyze` 采用“通用模板 + 私有 env-map”结构，首次安装时先只复制模板，再在第一次真实排障时由 AI 生成/更新 `env-map.md`。

> `log-analyze` 的检索优化规则（例如“优先按告警时间查小时归档文件，查不着再扩大范围”）属于 `SKILL.md` 模板本身的一部分，所以这类更新不需要发 npm，只需要同步更新用户本地安装的 `log-analyze/SKILL.md`。

如果希望把 `env-map` 模板也一并准备好，可以额外执行：

```bash
cp skills/log-analyze/env-map.template.md ~/.codex/skills/log-analyze/env-map.md
```

这样安装后的目录结构会是：

```text
~/.codex/skills/
  agent-ssh-cli/
    SKILL.md
  log-analyze/
    SKILL.md
    env-map.md
```

## 4. 验证

```bash
agentsshcli --help
test -f ~/.agent-ssh-cli/config.json
```

配置完成后，测试执行：

```bash
agentsshcli list
```

拿到连接名后，先验证 JumpServer：

```bash
agentsshcli jump-exec --timeout 120000 <jumpserver-connection> --target <known-target> "hostname"
```

## 5. 首次安装后的个性化补充

如果安装了 `log-analyze`，推荐在**首次安装完成后**，再让 AI 通过交互问答补齐你自己的环境映射、目标主机、日志目录约定。

推荐让 AI 逐条收集：

1. 线上 JumpServer 连接名
2. 测试 JumpServer 连接名（如果有）
3. 线上目标主机命名规则（hostname 还是 IP）
4. 预发布是否与线上共用 JumpServer
5. 各环境日志根目录（例如 `/www` 或 `/data`）
6. 常用项目别名、机器别名、默认 target

如果需要完整示例流程，请阅读后续补充文档：

- `docs/AI_FIRST_RUN_LOG_ANALYZE.md`

如果后续你更新了 `log-analyze` 的工作流模板，也要同步覆盖本地安装目录中的：

- `~/.codex/skills/log-analyze/SKILL.md`

但通常**不需要**改动用户自己的：

- `~/.codex/skills/log-analyze/env-map.md`

也就是说：

- 通用规则升级：覆盖 `SKILL.md`
- 私有环境变化：更新 `env-map.md`

## 6. 使用

个性化补充完成后，就可以直接让 AI 使用：

```text
/log-analyze <日志片段或问题描述>
```

## 7. 仓库维护者补充：自动发版

如果你是 `@2red1blue/agentsshcli` 的维护者，并希望通过 GitHub Actions 自动发布 npm 包，当前仓库推荐使用 npm Trusted Publishing，而不是长期 `NPM_TOKEN`。

请参考：

- `docs/NPM_PUBLISH_GUIDE.md`

其中已经包含：

- 如何为 6 个 npm 包建立 trusted publisher
- workflow 触发方式
- tag 发布流程
- 首发与后续发版的区别
