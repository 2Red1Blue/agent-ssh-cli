---
name: log-analyze
description: 基于 agent-ssh-cli 的日志深度分析与问题定位工作流。通过日志片段或口头描述自动提取关键信息，依赖 agentsshcli 执行远端命令，必要时通过 mysql-mcp 查询表结构，最终结合项目代码给出根因分析与修复建议。首次安装后应由 AI 交互式补齐环境映射。
user-invocable: true
---

# log-analyze

> 本 skill 是 **agent-ssh-cli 的上层诊断工作流**。
> 它不重新定义 SSH / JumpServer / 安全约束，而是直接复用 `agent-ssh-cli` 的执行层能力。

## 何时使用

- 收到线上、预发布或测试环境告警日志，需要定位问题根因
- 日志片段信息不完整，需要到目标机器查看完整上下文
- 用户口头描述问题（如 "机器 CPU 高"、"某个服务 OOM"），需要直接定位目标机
- 涉及数据库操作异常（如插入失败、SQL 错误），需要查表结构辅助分析
- 跨项目链路问题，需要从当前项目日志追踪到关联项目

## 依赖关系

- 本 skill 依赖 `agent-ssh-cli`
- 所有远端连接、JumpServer 配置、`jump-exec` 行为、命令安全边界，均遵循 `agent-ssh-cli`
- 如果需要了解底层连接、配置或命令限制，应回看 `agent-ssh-cli` 的 `SKILL.md`

## 前置条件

- `agent-ssh-cli` 已安装
- `agentsshcli` 可执行
- `~/.agent-ssh-cli/config.json` 中已配置至少一个可用 JumpServer 连接
- mysql-mcp 已配置（仅用于查询表结构及测试环境数据，不用于线上数据查询）
- 当前工作目录为对应项目代码库（用于结合源码分析）
- 当前 skill 目录下存在 `env-map.md`，用于存放当前用户/团队的私有环境映射

## 命令格式

```text
/log-analyze <日志片段或问题描述>
```

## 首次安装后的初始化规则

如果 AI 第一次使用本 skill，或发现 `env-map.md` 不存在 / 内容不完整，必须：

1. 每次只问用户一个问题
2. 按顺序收集 JumpServer 连接、环境定义、日志根目录、项目简称、目标机映射
3. 把收集结果写入当前目录的 `env-map.md`
4. 写入后立刻用 `agentsshcli jump-exec` 做最小验证

## AI 交互式初始化问题顺序

1. 线上 JumpServer 连接名是什么
2. 是否有独立测试 JumpServer；如果有，连接名是什么
3. 预发布是否与线上共用同一个 JumpServer
4. 线上日志根目录是什么
5. 预发布日志根目录是什么
6. 测试日志根目录是什么
7. 常用项目有哪些简称/别名
8. 每个项目在各环境下的默认目标机（hostname 或 IP）是什么
9. 是否有常用机器简称（如 `api-02`、`dz1-72`）
10. 默认环境是什么

## 私有映射文件

当前用户的真实环境信息应写入：

- [env-map.md](./env-map.md)

这个文件只负责保存**私有环境映射**，例如：

- JumpServer 连接名映射
- 环境定义（prod / yfb / test）
- 日志根目录映射
- 项目别名映射
- 默认 target 映射
- 机器简称到 hostname/IP 的映射

## 核心工作流

### Step 0：读取私有映射

在做任何远端命令之前，先读 `env-map.md`，完成：

- 环境推断
- 项目推断
- target 推断
- 日志根目录推断

### Step 0.5：先从告警文本提取检索锚点

如果用户给的是告警文本或日志片段，必须优先提取下面这些“检索锚点”，再决定远端命令：

1. 告警时间
2. `msg` 中的服务名列表
3. `ec2_ip` / provider / consumer IP
4. traceId / requestId / accountId / taskId / method 名
5. 异常关键词（如 `Timeout`、`DuplicateKey`、`NullPointerException`）

如果告警里已经有：

- 精确时间
- 服务名
- 机器 IP

则**禁止**第一步就做全目录、全文件递归 `grep -R`。

### Step 1：首次远端命令

优先直接使用 `agentsshcli jump-exec`，不要先做多轮探测式发现。
底层命令格式、连接校验、危险命令约束都沿用 `agent-ssh-cli`。

示例：

```bash
agentsshcli jump-exec --timeout 120000 <jumpserver-connection> --target <target> \
  "uptime && top -bn1 | head -20 && ps aux --sort=-%cpu | head -10"
```

### Step 2：后续诊断

按需追加命令：

### Step 2.1：日志检索总原则

日志诊断默认遵循下面的“由小到大”策略：

1. 先按**告警时间窗口**缩小范围
2. 再按**告警所属小时对应的日志文件**缩小范围
3. 再按**服务名**缩小到明确项目
4. 再按**机器 IP / target**缩小到明确主机
5. 再按**traceId / accountId / method / 异常关键词**取命中上下文
6. 只有 scoped 查询无结果时，才扩大时间窗、文件范围或机器范围

默认窗口：

- 第一轮：告警时间前后 `±2 分钟`
- 第二轮：扩大到 `±10 分钟`
- 第三轮：才允许跨小时文件或扩大到更多机器

默认文件选择规则：

- 如果告警时间落在**当前小时**，优先查活跃文件：
  - `error.log`
  - `info.log`
  - `statistic.log`
- 如果告警时间**不在当前小时**，优先查对应小时归档文件：
  - `error.log_YYYY-MM-DD.log`
  - `info.log_YYYY-MM-DD_HH.log`
  - `statistic.log_YYYY-MM-DD_HH.log`

也就是说，绝大多数带时间戳的历史告警，第一轮应该直接命中“对应小时文件”，而不是扫整个 `logs/` 目录。

默认文件优先级：

1. `error.log`
2. `info.log`
3. `statistic.log`

`statistic.log` 只能在以下情况再查：

- `error.log` / `info.log` 没拿到足够链路信息
- 明确需要请求体 / 响应体 / ROI 明细 / 第三方返回
- 需要确认 provider 是否真的收到请求

### Step 2.2：禁止的低效检索方式

以下方式默认禁止，除非前两轮 scoped 查询都失败：

```bash
grep -Rni "<异常关键词>" <log-root>/<project>/logs
grep -Rni "<关键词>" <log-root>/<project>/logs | tail -50
cat <超大日志文件> | grep ...
```

原因：

- 会扫到大量历史小时文件
- 容易命中超大 `info.log` / `statistic.log`
- 输出体积失控，严重拖慢 AI 分析

### Step 2.3：最近告警优先查当前文件尾部

如果告警时间距离“现在”较近，优先只查当前活跃日志文件尾部，不要立刻扫整文件。

推荐模式：

```bash
tail -n 20000 <log-root>/<project>/logs/error.log
tail -n 30000 <log-root>/<project>/logs/info.log
```

再在 tail 结果中按“时间 + 关键词”过滤，而不是直接从整文件开扫。

### Step 2.4：非当前小时优先查归档小时文件

如果告警时间不在当前小时，优先直接构造目标文件名。

例如：

- `2026-06-23 18:17:08`

则第一候选文件通常就是：

```text
error.log_2026-06-23.log
info.log_2026-06-23_18.log
statistic.log_2026-06-23_18.log
```

推荐先确认文件存在，再做精确查询：

```bash
LOG_DIR=<log-root>/<project>/logs
ls "$LOG_DIR/error.log_2026-06-23.log" \
   "$LOG_DIR/info.log_2026-06-23_18.log" \
   "$LOG_DIR/statistic.log_2026-06-23_18.log" 2>/dev/null
```

然后第一轮只查这 1~2 个最相关文件：

```bash
grep -n -m 20 -C 3 -E "2026-06-23 18:1[5-9]|<traceId>|<accountId>|<method>|<异常关键词>" \
  "$LOG_DIR/info.log_2026-06-23_18.log"
```

如果是异常堆栈优先场景，再补：

```bash
grep -n -m 20 -C 3 -E "2026-06-23 18:1[5-9]|<traceId>|<accountId>|<method>|<异常关键词>" \
  "$LOG_DIR/error.log_2026-06-23.log"
```

### Step 2.5：有明确时间时的日志检索模板

如果用户已经给了类似：

- `告警时间: 2026-06-23 18:17:08`
- `msg: ["hw-adserving","hw-adserving-api"]`

则应该先直接构造：

- 目标服务
- 目标机器
- 目标分钟：`18:15`、`18:16`、`18:17`、`18:18`、`18:19`

第一轮推荐命令：

```bash
LOG_DIR=<log-root>/<project>/logs
grep -n -m 20 -C 3 -E "2026-06-23 18:1[5-9]|<traceId>|<accountId>|<method>|<异常关键词>" \
  "$LOG_DIR/info.log_2026-06-23_18.log" 2>/dev/null
grep -n -m 20 -C 3 -E "2026-06-23 18:1[5-9]|<traceId>|<accountId>|<method>|<异常关键词>" \
  "$LOG_DIR/error.log_2026-06-23.log" 2>/dev/null
```

如果告警时间就在当前小时，再退回活跃文件尾部策略：

```bash
tail -n 30000 "$LOG_DIR/error.log" 2>/dev/null | grep -E "2026-06-23 18:1[5-9]|<traceId>|<accountId>|<method>|<异常关键词>" | tail -n 80
tail -n 30000 "$LOG_DIR/info.log" 2>/dev/null | grep -E "2026-06-23 18:1[5-9]|<traceId>|<accountId>|<method>|<异常关键词>" | tail -n 120
```

如果第一轮命中为空，第二轮才允许扩大到相邻小时文件，例如 `17`、`18`、`19` 这 3 个小时文件，而不是整个目录。

### Step 2.6：命中后只取小段上下文

无论是 `grep`、`awk` 还是 `sed`，都只允许取有限上下文，不要把整段大对象直接打出来。

推荐：

```bash
grep -n -m 20 -C 3 "<关键词>" <file>
```

或：

```bash
awk 'BEGIN{n=0} /<关键词>/{n=1} n{print; c++} c>=20{exit}' <file>
```

输出控制原则：

- 单次命令默认不超过 `80~120` 行
- 单个文件同一轮最多取 `2` 段上下文
- 如果命中超长 JSON / ROI 内容，优先只保留关键字段所在行，不展开全量 body

### Step 2.7：Dubbo 超时场景的专项策略

如果告警核心关键词是：

- `Tried X times of the providers`
- `Invoke remote method timeout`
- `dubbo version`

则默认按下面顺序排查：

1. 先查 consumer 侧 `error.log` / `info.log`
2. 确认：
   - 接口名
   - 方法名
   - provider IP
   - traceId / accountId / taskId
3. 只有当 consumer 侧证据不足时，才去 provider 侧查对应分钟日志
4. provider 侧默认仍然先查 `error.log` / `info.log`
5. **不要**一上来就扫 provider 的 `statistic.log`

因为 Dubbo timeout 常见瓶颈是：

- consumer 自身线程阻塞
- provider 处理慢
- provider 下游依赖慢

而不是所有情况都要先看 provider 大流量明细日志。

**CPU/负载问题：**
```bash
uptime && top -bn1 | head -20 && ps aux --sort=-%cpu | head -15
```

**Java OOM/崩溃：**
```bash
ps aux | grep java | grep -v grep
ls -1 /tmp | grep -E 'hs_err|java_pid' | tail -20
```

**日志排查：**
```bash
LOG_DIR=<log-root>/<project>/logs
tail -n 30000 "$LOG_DIR/error.log" 2>/dev/null | grep -E "<告警分钟>|<traceId>|<accountId>|<method>|<异常关键词>" | tail -n 80
tail -n 30000 "$LOG_DIR/info.log" 2>/dev/null | grep -E "<告警分钟>|<traceId>|<accountId>|<method>|<异常关键词>" | tail -n 120
```

**线程 dump（Java 高 CPU）：**
```bash
ps aux | grep java | grep -v grep
jstack <PID> | head -200
```

### Step 3：数据库辅助分析

仅在以下情况使用 `mysql-mcp`：

- 需要查看表结构辅助分析 SQL 错误
- 需要查看测试环境的数据样本来理解字段含义

约束：

- 仅查询表结构（`DESCRIBE`、`SHOW CREATE TABLE` 等）
- 仅查询测试环境数据
- 严禁在线上数据库执行任何修改操作

### Step 4：根因分析与修复建议

综合以下信息：

1. 日志中的错误堆栈和异常信息
2. 检索到的上下文日志（请求链路、前后序操作）
3. 相关表结构（字段类型、约束、索引）
4. 项目源码中对应的代码逻辑

输出格式：

```text
## 问题定位

- 异常类型：
- 发生位置：
- 根因：

## 上下文日志

（关键日志片段）

## 修复建议

1. ...
2. ...

## 验证方式

...
```

## 最小验证命令

初始化完成后，至少验证：

```bash
agentsshcli jump-exec --timeout 120000 <prod-connection> --target <known-prod-target> "hostname"
agentsshcli jump-exec --timeout 120000 <test-connection> --target <known-test-target> "hostname"
```

如果要验证日志目录：

```bash
agentsshcli jump-exec --timeout 120000 <connection> --target <target> \
  "ls -1 <log-root>/<project>/logs | head -20"
```

## 命中范围扩大规则

只有在下面条件成立时，才允许扩大检索范围：

1. 当前服务的 `error.log` / `info.log` 在 `±2 分钟` 内无结果
2. 已尝试按 traceId / accountId / method / 异常关键词组合查询
3. 已确认当前 target 就是告警机器或最可能机器

扩大顺序必须是：

1. 同机 `±10 分钟`
2. 同机相邻小时归档文件
3. 同服务其他目标机
4. provider 侧对应机器
5. `statistic.log`
6. 最后才是有限度的目录级搜索

## 面向当前用户场景的默认优化

针对线上大日志文件场景，默认采用以下保守策略：

- 不跨服务乱扫；先用告警 `msg` 限定服务
- 不跨机器乱扫；先用告警 `ec2_ip` / provider / consumer IP 限定机器
- 不先扫 `statistic.log`
- 不先扫整个 `logs/` 目录
- 不返回超长 JSON body
- 优先回答“consumer 卡住了还是 provider 卡住了”，而不是一次性捞所有链路日志
