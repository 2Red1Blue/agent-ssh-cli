# AI 安装说明

如果你想让 AI 直接帮你安装，可以把下面这句话原样发给 AI：

```text
安装请阅读 https://github.com/2Red1Blue/agent-ssh-cli/blob/main/AI_INSTALL.md，按说明安装 CLI 并添加 `SKILL.md`。
```

AI 读完这份文档后，默认应完成：

1. 安装 CLI：`@2red1blue/agentsshcli`
2. 安装 `agent-ssh-cli` skill
3. 安装 `log-analyze` skill
4. 交互式选择要安装到哪些客户端
5. 为多个客户端建立“主链 + 复用”或“分别复制”的安装结构
6. 初始化本地 skill 目录结构和配置模板
7. 提示用户重启客户端并继续补配置

安装完成后的状态应该是：

- CLI 和 2 个 skill 都已经就位
- 已经明确要安装到哪些客户端
- 已经明确哪个客户端作为主链
- 只剩 `~/.agent-ssh-cli/config.json` 与主链 `log-analyze/env-map.md` 需要按真实环境交互式补全
- 客户端重启后，AI 就可以继续交互式补配置与映射

因此最上面这句提示词在当前版本仍然成立：

```text
安装请阅读 https://github.com/2Red1Blue/agent-ssh-cli/blob/main/AI_INSTALL.md，按说明安装 CLI 并添加 `SKILL.md`。
```

虽然文案里仍然写的是“添加 `SKILL.md`”，但现在 AI 真正应执行的是整套标准流程，而不是只复制一个文件。

使用本工具时，先按下面步骤安装 CLI 和 skill。CLI 仍通过 npm 安装，内部 SSH 执行逻辑由 Rust 原生执行器完成。

npm 安装会按当前系统自动拉取对应平台的 optional 预编译包；当前支持 macOS arm64/x64、Linux x64/arm64、Windows x64。

如果本机同时装了多套 Node/npm（例如系统 npm、Homebrew npm、Hermes 自带 npm），AI **不能**默认只执行一次 `npm install -g`。必须先检查 `which -a npm`，按“唯一全局前缀”逐个安装，避免 CLI 只被装进某一个工具自己的全局目录。

## 0. 先理解安装目标

`agentsshcli install-ai` 现在不是简单复制到一个目录，而是面向首次安装用户提供以下能力：

- 交互式选择客户端：
  - `cc-switch`（共享 skills 主链目录，默认 `~/.cc-switch/skills`）
  - `codex`
  - `claude`
  - `opencode`
  - `hermes`
  - `custom`（手动输入 skills 根目录完整路径）
- 指定一个“主链客户端”或“主链安装根”
- 其他客户端可选择：
  - 软链复用主链 skill / `env-map`
  - 或各自复制一份

共享与隔离原则：

- `~/.agent-ssh-cli/config.json` 是**共享 CLI 配置**
- 如果检测到 `~/.cc-switch/skills`，推荐优先把它当作共享主链目录
- `SKILL.md` 和 `env-map.md` 属于各客户端自己的 skills 目录
- 如果选择“软链复用”，多个客户端会共享主链那份 `log-analyze/env-map.md`
- 如果选择“分别复制”，每个客户端都有自己的 `env-map.md`

推荐首次安装策略：

1. 如果本机已有 `cc-switch`，优先选择 `cc-switch` 作为主链
2. 当前正在使用的 AI 客户端作为次级复用目录
3. 其他客户端优先软链复用
4. 安装后重启所有目标客户端
5. 在任一客户端中继续交互式补 `config.json` 和 `env-map.md`


## 1. 安装 CLI

推荐一行命令：

```bash
for npm_bin in $(which -a npm 2>/dev/null | awk '!seen[$0]++'); do
  prefix="$("$npm_bin" prefix -g 2>/dev/null)" || continue
  case " ${SEEN_PREFIXES:-} " in
    *" $prefix "*) continue ;;
  esac
  SEEN_PREFIXES="${SEEN_PREFIXES:-} $prefix"
  echo "[agentsshcli] install via $npm_bin -> $prefix"
  "$npm_bin" install -g @2red1blue/agentsshcli || exit 1
done
agentsshcli install-ai
```

这条命令会：

- 安装 CLI
- 自动进入客户端安装流程
- 交互式询问要安装到哪些客户端
- 如果检测到 `cc-switch`，默认将其作为主链安装根
- 交互式询问哪个客户端或安装根作为主链
- 交互式询问其余客户端是软链复用还是分别复制
- 安装 `agent-ssh-cli` skill
- 安装 `log-analyze` skill
- 初始化 `~/.agent-ssh-cli/config.json` 示例配置
- 初始化主链客户端中的 `log-analyze/env-map.md` 模板
- 给出后续“重启客户端 -> 补配置 -> 补映射 -> 验证”的标准提示

如果你已经知道要装到哪些内置客户端，也可以直接指定：

```bash
for npm_bin in $(which -a npm 2>/dev/null | awk '!seen[$0]++'); do
  prefix="$("$npm_bin" prefix -g 2>/dev/null)" || continue
  case " ${SEEN_PREFIXES:-} " in
    *" $prefix "*) continue ;;
  esac
  SEEN_PREFIXES="${SEEN_PREFIXES:-} $prefix"
  echo "[agentsshcli] install via $npm_bin -> $prefix"
  "$npm_bin" install -g @2red1blue/agentsshcli || exit 1
done
agentsshcli install-ai --clients codex,claude,hermes --primary-client codex
```

如果本机已经装了 `cc-switch`，推荐显式把它作为主链：

```bash
agentsshcli install-ai --clients cc-switch,codex,claude --primary-client cc-switch
```

如果你当前就在 Codex 里让 AI 帮你装，推荐默认方案是：

```bash
agentsshcli install-ai --clients cc-switch,codex --primary-client cc-switch
```

如果你当前就在 Claude Code 里让 AI 帮你装，推荐默认方案是：

```bash
agentsshcli install-ai --clients cc-switch,claude --primary-client cc-switch
```

如果你希望某个客户端使用自定义 skills 根目录，也可以直接传完整路径：

```bash
agentsshcli install-ai \
  --clients codex,custom \
  --primary-client custom \
  --client-root custom=/absolute/path/to/that-client/skills
```

如果想完全交互式地选择客户端和自定义路径，推荐：

```bash
agentsshcli install-ai --interactive
```

如果只想先单独安装 CLI，再手动执行安装器，也可以用下面两步：

```bash
for npm_bin in $(which -a npm 2>/dev/null | awk '!seen[$0]++'); do
  prefix="$("$npm_bin" prefix -g 2>/dev/null)" || continue
  case " ${SEEN_PREFIXES:-} " in
    *" $prefix "*) continue ;;
  esac
  SEEN_PREFIXES="${SEEN_PREFIXES:-} $prefix"
  echo "[agentsshcli] install via $npm_bin -> $prefix"
  "$npm_bin" install -g @2red1blue/agentsshcli || exit 1
done
agentsshcli --help
agentsshcli install-ai --interactive
```

如果 AI 选择自己执行安装，而不是直接复制上面的命令，也应遵守同样规则：

1. 先执行 `which -a npm`
2. 对每个 `npm` 执行 `npm prefix -g`
3. 只保留唯一前缀
4. 对每个唯一前缀对应的 `npm` 都执行一次全局安装
5. 再执行 `agentsshcli install-ai`
6. 若检测到 `~/.cc-switch/skills`，优先把 `cc-switch` 作为主链候选
7. 若用户未明确指定客户端，优先使用交互模式
8. 若用户输入的是非内置客户端，要求其提供 skills 根目录完整路径

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

`agentsshcli install-ai` 支持以下内置客户端默认目录：

```text
cc-switch -> ~/.cc-switch/skills/
codex    -> ~/.codex/skills/
claude   -> ~/.claude/skills/
opencode -> ~/.config/opencode/skills/
hermes   -> ~/.hermes/skills/
```

如果安装器检测到 `~/.cc-switch/skills` 已存在：

- 交互模式默认会把 `cc-switch` 放进默认客户端列表
- 主链默认会优先选择 `cc-switch`
- 你仍然可以改成 `codex`、`claude` 或其它目标

如果用户使用的是其它客户端，或对方有自己的 skills 根目录约定，应使用：

```bash
agentsshcli install-ai --clients custom --client-root custom=/absolute/path/to/skills
```

也就是说，判断规则是：

1. 如果存在 `cc-switch`：优先把它当共享主链目录
2. 已知客户端：直接选内置名字
3. 未知客户端：让用户输入 skills 根目录完整路径
4. 不确定客户端目录：先问清楚，不要猜

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
which -a npm
```

安装完成后，**先重启所有目标客户端**，再继续后面的配置流程。

重启后先验证 CLI：

```bash
agentsshcli list
```

然后按顺序完成：

1. 补齐 `~/.agent-ssh-cli/config.json`
2. 补齐主链客户端里的 `log-analyze/env-map.md`
3. 如果用了软链复用，其余客户端会自动共享主链 `env-map`
4. 如果用了分别复制，则需要分别维护各客户端自己的 `env-map.md`

拿到连接名后，先验证 JumpServer：

```bash
agentsshcli jump-exec --timeout 120000 <jumpserver-connection> --target <known-target> "hostname"
```

最后确认客户端技能列表中已经能看到 `log-analyze`。

## 5. 首次安装后的个性化补充

如果安装了 `log-analyze`，推荐在**首次安装完成后**，再让 AI 通过交互问答补齐你自己的环境映射、目标主机、日志目录约定。

推荐让 AI 逐条收集：

1. 线上 JumpServer 连接名
2. 测试 JumpServer 连接名（如果有）
3. 线上目标主机命名规则（hostname 还是 IP）
4. 预发布是否与线上共用 JumpServer
5. 各环境日志保存路径模式（例如 `/www/<project>/logs` 或 `/data/<project>/logs`）
6. 常用项目别名、机器别名、默认 target

如果需要完整示例流程，请阅读后续补充文档：

- `docs/AI_FIRST_RUN_LOG_ANALYZE.md`

推荐把下面两段分别发给 AI：

1. 初始化 JumpServer 配置

```text
请帮我初始化 agent-ssh-cli 的 JumpServer 配置。请按 README 里的 add-jump-server 流程每次只问我一个问题，收集完后执行 agentsshcli add-jump-server 写入 ~/.agent-ssh-cli/config.json，并用 agentsshcli list 和 jump-exec hostname 做最小验证。
```

2. 初始化 log-analyze 映射

```text
请帮我初始化 log-analyze 的环境映射。请每次只问我一个问题，收集 JumpServer 名称、各环境日志保存路径模式（例如 /www/<project>/logs 或 /data/<project>/logs）、项目简称、默认 target、机器简称映射，写入当前主链客户端的 log-analyze/env-map.md，并用 agentsshcli jump-exec 做最小验证，直到客户端里可以正常使用 log-analyze。
```

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
