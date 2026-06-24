# AI First-Run Guide for `log-analyze`

这份文档用于**首次安装后**，引导 AI 通过交互式问答，为 `log-analyze` 生成或更新私有 `env-map`，把它从“通用模板”补充成“适配当前团队环境”的可用 skill。

适用前提：

- `agentsshcli install-ai` 已完成
- 目标客户端已经重启，能重新扫描 skills
- 用户已经明确 `log-analyze` 要装在哪些客户端
- 如果是多客户端安装，已经明确哪个是“主链客户端”

## 原则

- 首次安装时，不要把真实 IP、hostname、JumpServer 地址、项目映射直接写进公开安装说明
- 这些内容应由 AI 在用户本机按问答方式收集后，再写入用户自己的 `env-map` 或私有配置
- 公共仓库只保留通用安装和通用执行机制
- 通用日志检索策略也应保留在公开 `SKILL.md` 中，例如：
  - 优先按告警时间做小窗口查询
  - 非当前小时优先查 `info.log_YYYY-MM-DD_HH.log`
  - 查不着再扩大到相邻小时或更多机器

## skill 更新是否实时生效

对当前这类本地文件型 skill 来说，修改本机的 `SKILL.md` 后，后续再次调用该 skill 时通常就会按新内容执行，不需要发 npm。

需要区分两类内容：

- 通用规则：保存在 `SKILL.md`
- 私有环境：保存在 `env-map.md`

因此后续维护时：

1. 如果改的是诊断流程、检索顺序、时间窗策略、输出格式，更新 `SKILL.md`
2. 如果改的是 JumpServer、目标机、日志目录、项目映射，更新 `env-map.md`

> npm 包负责 CLI 二进制和命令能力；skill 文本本身不是 npm 发版的一部分，除非你特意把 skill 也纳入 npm 包内容并做自动分发。

## AI 应该如何做

AI 在第一次使用 `log-analyze` 前，若发现缺少环境映射，应先用一句话告诉用户：

- “我接下来是在补你们团队常用的项目名、机器简称和日志目录。补完后，后面你只给我告警或简单机器名，我就能自动定位目标机和日志。”

在正式提问前，还应先告诉用户当前主链 `env-map.json` 和 `env-map.md` 的真实路径。默认由当前 AI 继续维护这个文件；只有当用户明确要求自己手填时，AI 才停在“文件路径 + 填写顺序 + 最小验证命令”这一层。

然后推荐先执行：

```bash
agentsshcli env-map init --from-config
```

这一步只适用于 `env-map.json` / `env-map.md` 还不存在的首次初始化场景，会先从 `~/.agent-ssh-cli/config.json` 中自动发现 JumpServer 连接，减少重复提问。若文件已经存在，就直接在现有 `env-map.json` 基础上增量维护；只有确认整体重建时才使用 `agentsshcli env-map init --force`。

之后不要再按 14 个顺序问题逐条追问，而是按下面 4 组信息收集，仍然保持**每次只问一个问题**：

1. 你有哪些 JumpServer，分别对应什么环境
2. 各环境的日志路径模式是什么（例如 `/www/{project}/logs` 或 `/data/{project}/logs`）
3. 你最常查的项目有哪些，它们平时的简称 / 别名是什么
4. 给一组常用主机列表（hostname、机器简称或 IP 都可以），我来验证并回填

## 推荐收集后的结构

AI 收集完成后，建议把这些信息整理成：

- JumpServer 连接映射
- JumpServer 别名映射
- 环境到日志保存路径模式映射
- 环境别名映射
- 项目别名映射
- 项目到已验证默认 target 映射
- 机器简称到 hostname/IP 映射
- 日志归档命名约定（如果团队有特殊规则，也可补进 `env-map`）

## 推荐写入位置

二选一：

1. 写到一个私有的环境映射文件，再让 `SKILL.md` 引用
2. 如果没有专门的私有配置机制，再退回直接修改本地 skill

当前推荐优先使用方案 1，也就是结构化的 `env-map.json` + 自动渲染的 `env-map.md`。

如果安装流程使用了“主链 + 软链复用”：

- 只需要维护主链客户端的 `env-map.md`
- 其它客户端会共享这份映射

如果安装流程使用了“分别复制”：

- 每个客户端的 `env-map.md` 各自独立
- 后续需要分别维护

## 首次安装后还要同步什么

除了用户自己的 `env-map` 之外，首次安装说明还应明确两件事：

1. `log-analyze` 的通用模板会持续演进，后续升级时应覆盖本地 `SKILL.md`
2. 用户自己的 `env-map` 不应被模板升级覆盖

推荐目录结构：

```text
~/.codex/skills/log-analyze/
  SKILL.md
  env-map.md
```

如果使用了 `cc-switch` 作为共享主链，则等价推荐目录结构是：

```text
~/.cc-switch/skills/log-analyze/
  SKILL.md
  env-map.md
```

常见客户端全局 skills 根目录：

```text
CC Switch  -> ~/.cc-switch/skills/
Codex      -> ~/.codex/skills/
Claude     -> ~/.claude/skills/
OpenCode   -> ~/.config/opencode/skills/
Hermes     -> ~/.hermes/skills/
自定义客户端 -> 以用户提供的完整路径为准
```

## 最小验证命令

在 AI 补齐配置后，第一步先让它连接 JumpServer 并展示 `Opt>` 菜单；确认完菜单后，再做最小验证。验证时不要先要求用户给完整 hostname，而是优先接受：

- 机器简称，如 `api-02`
- 项目名或业务名，如 `adserving-api`
- 实例尾号
- 直接 IP
- 一组常用主机列表

若验证成功，必须把返回的真实 hostname 回显给用户，再写入 `env-map`。

推荐顺序：

```bash
agentsshcli jump-menu <prod-connection>
agentsshcli jump-menu <test-connection>
```

最小验证命令：

```bash
agentsshcli jump-exec --timeout 120000 <prod-connection> --target <known-prod-target> "hostname"
agentsshcli jump-exec --timeout 120000 <test-connection> --target <known-test-target> "hostname"
```

如果需要验证日志目录：

```bash
agentsshcli jump-exec --timeout 120000 <connection> --target <target> "ls -1 <log-root>/<project>/logs | head -20"
```

## 给 AI 的建议提示词

可以把下面这段直接交给 AI：

```text
你现在要初始化 log-analyze。第一步先连接 JumpServer，执行 agentsshcli jump-menu <jumpserver-connection>，把当前 JumpServer 的 Opt 菜单完整展示给我。确认完菜单后，再用一句话说明这一步是在补“常用主机、主机/项目别名、日志目录”的私有信息，后面查日志时你才能自动定位。然后每次只问我一个问题，但只需要向我收集三类信息：
1. 我想添加哪些常用主机
2. 这些主机或项目平时有哪些简称 / 别名
3. 这些项目日志通常在哪个目录

收集完成后：
1. 把映射写入我本地的 log-analyze `env-map`
2. JumpServer 菜单确认、主机搜索、真实 hostname / IP 验证这些动作都由你自己完成；不要一开始就要求我提供完整 hostname
3. 用 agentsshcli jump-exec 做最小验证；验证成功后把真实 hostname 回显给我
4. 确认当前客户端技能列表里已经能看到 log-analyze
5. 给我一个简短总结，告诉我后续可以怎么直接使用 /log-analyze
```
