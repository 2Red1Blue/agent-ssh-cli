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

AI 在第一次使用 `log-analyze` 前，若发现缺少环境映射，应按下面顺序**每次只问一个问题**：

1. 线上 JumpServer 连接名是什么
2. 是否有独立测试 JumpServer；如果有，连接名是什么
3. 预发布是否与线上共用同一个 JumpServer
4. 线上机器通常用 hostname 还是 IP 作为 target
5. 预发布机器通常用 hostname 还是 IP 作为 target
6. 测试机器通常用 hostname 还是 IP 作为 target
7. 线上日志根目录是什么
8. 预发布日志根目录是什么
9. 测试日志根目录是什么
10. 常用项目有哪些简称/别名
11. 每个项目在各环境下的默认目标机或目标机列表是什么

## 推荐收集后的结构

AI 收集完成后，建议把这些信息整理成：

- JumpServer 连接映射
- 环境到日志根目录映射
- 项目别名映射
- 项目到默认 target 映射
- 机器简称到 hostname/IP 映射
- 日志归档命名约定（如果团队有特殊规则，也可补进 `env-map`）

## 推荐写入位置

二选一：

1. 写到一个私有的环境映射文件，再让 `SKILL.md` 引用
2. 如果没有专门的私有配置机制，再退回直接修改本地 skill

当前推荐优先使用方案 1，也就是 `env-map`。

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

常见客户端全局 skills 根目录：

```text
Codex      -> ~/.codex/skills/
Claude     -> ~/.claude/skills/
OpenCode   -> ~/.config/opencode/skills/
Hermes     -> ~/.hermes/skills/
自定义客户端 -> 以用户提供的完整路径为准
```

## 最小验证命令

在 AI 补齐配置后，应立即要求它做最小验证：

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
你现在要初始化 log-analyze 的环境映射。请每次只问我一个问题，按以下顺序收集：
1. 线上 JumpServer 连接名
2. 测试 JumpServer 连接名（如果有）
3. 预发布是否与线上共用同一 JumpServer
4. 线上/预发布/测试各自的日志根目录
5. 常用项目简称
6. 每个项目在各环境下的默认目标机（hostname 或 IP）

收集完成后：
1. 把映射写入我本地的 log-analyze `env-map`
2. 用 agentsshcli jump-exec 做最小验证
3. 确认当前客户端技能列表里已经能看到 log-analyze
4. 给我一个简短总结，告诉我后续可以怎么直接使用 /log-analyze
```
