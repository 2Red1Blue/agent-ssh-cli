# AI First-Run Guide for `log-analyze`

这份文档用于**首次安装后**，引导 AI 通过交互式问答，为 `log-analyze` 生成或更新私有 `env-map`，把它从“通用模板”补充成“适配当前团队环境”的可用 skill。

## 原则

- 首次安装时，不要把真实 IP、hostname、JumpServer 地址、项目映射直接写进公开安装说明
- 这些内容应由 AI 在用户本机按问答方式收集后，再写入用户自己的 `env-map` 或私有配置
- 公共仓库只保留通用安装和通用执行机制

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

## 推荐写入位置

二选一：

1. 写到一个私有的环境映射文件，再让 `SKILL.md` 引用
2. 如果没有专门的私有配置机制，再退回直接修改本地 skill

当前推荐优先使用方案 1，也就是 `env-map`。

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
3. 给我一个简短总结，告诉我后续可以怎么直接使用 /log-analyze
```
