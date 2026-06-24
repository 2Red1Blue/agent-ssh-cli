---
name: log-analyze
description: 基于 agent-ssh-cli 的日志深度分析与问题定位工作流。通过日志片段或口头描述自动提取关键信息，依赖 agentsshcli 执行远端命令，必要时通过 mysql-mcp 查询表结构，最终结合项目代码给出根因分析与修复建议。首次安装后应由 AI 交互式补齐环境映射。
user-invocable: true
---

<!-- agentsshcli:managed:start -->
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
2. 先用一句人话说明：这是在补“常用项目名 / 机器简称 / 日志目录”的私有信息，后面查日志时你才能自动定位机器
3. 按顺序收集 JumpServer 连接、JumpServer 口头别名、环境定义、环境别名、日志保存路径模式、项目简称/业务别名、常用机器简称或 IP、已验证 target
4. 不要一上来要求用户提供完整 hostname；优先接受项目简称、机器简称、业务名、实例尾号或 IP
5. 首次连通验证成功后，必须把返回的真实 hostname 回显给用户，再决定是否写入默认 target / 机器简称映射
6. 询问日志路径时，不要只说“映射”；应直接问“这个项目日志一般在 /www/<project>/logs 还是 /data/<project>/logs，或者别的真实路径”
7. 把收集结果写入当前目录的 `env-map.md`
8. 写入后立刻用 `agentsshcli jump-exec` 做最小验证

## AI 交互式初始化问题顺序

1. 线上 JumpServer 连接名是什么
2. 是否有独立测试 JumpServer；如果有，连接名是什么
3. 预发布是否与线上共用同一个 JumpServer
4. 默认先查哪个环境（prod / yfb / test）
5. 线上日志通常在 `/www/<project>/logs`、`/data/<project>/logs`，还是别的真实路径
6. 预发布日志通常在哪个真实路径
7. 测试日志通常在哪个真实路径
8. 你平时最常查的项目名或简称有哪些
9. 你平时怎么称呼各环境（如“线上 / 预发 / 测试”）以及各 JumpServer（如“线上跳板机”）
10. 询问用户是否愿意直接给一组“常用主机列表”（hostname、机器简称或 IP 都可以）
11. 如果用户给了主机列表，按顺序逐个做轻量验证，并把成功项写回映射；失败项单独标记后再向用户确认
12. 如果用户没有主机列表，再退回“先拿一个最常用的机器线索做验证”：可以是机器简称、业务名、实例尾号，或直接 IP
13. 首次验证成功后，把返回的真实 hostname 展示给用户，并追问：这个项目以后默认就用这台吗，日志真实路径是否与上面的模式一致
14. 如果用户还会用其它简称（如 `api-02`、`adserving-api`、`线上`、`预发跳板机`），再分别补充到机器简称、项目别名、环境别名、JumpServer 别名映射

## 私有映射文件

当前用户的真实环境信息应写入：

- [env-map.md](./env-map.md)

这个文件只负责保存**私有环境映射**，例如：

- JumpServer 连接名映射
- JumpServer 别名映射
- 环境定义（prod / yfb / test）
- 环境别名映射
- 日志保存路径模式映射
- 项目别名映射
- 已验证默认 target 映射
- 机器简称到 hostname/IP 的映射

## 核心工作流

### Step 0：读取私有映射

在做任何远端命令之前，先读 `env-map.md`，完成：

- 环境推断
- 项目推断
- target 推断
- 日志保存路径模式推断

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

### Step 0.8：先做 target 归一化与轻量连通性探测

在真正查日志前，必须先把 target 归一化，避免因为 JumpServer 菜单别名、机器简称或 hostname 不稳定，浪费大量超时等待。

target 选择顺序：

1. 告警里有明确 `ec2_ip` / provider IP / consumer IP 时，优先直接使用 IP
2. 否则使用 `env-map.md` 中已经验证过的 hostname / IP
3. 只有用户口头给了机器简称（如 `api-03`）时，才通过 `env-map.md` 里的机器简称映射展开

连通性探测规则：

- 首轮探测命令必须是**极小命令**
- 推荐只用：`hostname`、`date +%F_%T`、`pwd`、`test -d <log-dir>`
- 首轮探测默认超时应控制在 `10000~15000ms`
- 如果目标未确认，**不要**并行对多个可疑 target 同时跑 30s 长超时命令
- 如果用户已经能直接给一组常用 host，优先让用户先列出来；一个候选 host 一个线程，但必须限制并发，不要自己猜一大批 hostname 后全量乱打
- 对批量 host 探测，单个 host 推荐先用 `8000~15000ms` 小超时；失败后记录为待确认，不要一口气把整批都拖进长超时
- 推荐并发上限 `2~4`；除非用户明确要求，不要一次性并发十几台

推荐探测命令：

```bash
agentsshcli jump-exec --timeout 15000 <jumpserver-connection> --target <target> \
  "hostname && date +%F_%T"
```

如果探测成功：

- 记录返回的真实 hostname
- 把真实 hostname 明确回显给用户，避免用户不知道 AI 最终连到的是哪台机器
- 本轮后续查询优先复用这个真实 hostname / 已验证 IP
- 不要继续混用最初的机器简称或模糊 target

如果探测失败或超时：

1. 若当前 target 是机器简称，立刻切换到其映射出的精确 hostname 或 IP
2. 若当前 target 是可疑 hostname，优先切换到告警里的 IP
3. 若同一项目存在多个候选目标机，按 `env-map.md` 中顺序**串行**尝试，不要并发乱打一圈

出现一次 alias/hostname 超时后，禁止继续对同名目标反复执行重命令。

### Step 0.9：批量主机探测要保守

如果用户一次给了一串主机（例如 `adserving-01`、`ad-service-02`、`cps-web-01`），AI 应：

1. 优先接受用户直接给出的列表，不要先自行扩展更多猜测 hostname
2. 如果一个简称可能对应多台候选主机，可按“一个候选主机一个线程”做轻量验证
3. 但必须加并发上限，推荐同时只跑 `2~4` 个候选
4. 成功就立刻记录“用户输入 -> 真实 hostname”
5. 失败就标成待确认，并在本轮结束后集中向用户确认，不要对单个失败 host 长时间重试

原因：

- 当前 `jump-exec` 对不同 target 不是同一个目标机会话复用
- 一个 target 超时，通常会消耗掉该 target 自己完整的菜单进入 / prompt 等待时间
- 所以需要的是“受控的小并发”，而不是“自己猜很多 host 后大并发乱扫”

### Step 1：首次远端命令

优先直接使用 `agentsshcli jump-exec`，但首次命令应当是**轻量验证或小范围日志查询**，不要一上来就跑负载分析或大日志扫描。
底层命令格式、连接校验、危险命令约束都沿用 `agent-ssh-cli`。

示例：

```bash
agentsshcli jump-exec --timeout 15000 <jumpserver-connection> --target <target> \
  "hostname && test -d <log-root>/<project>/logs && echo LOG_DIR_OK"
```

只有当问题本身就是：

- CPU 高
- 机器负载高
- Java 进程异常

才在连通性确认后再执行 `uptime` / `top` / `ps` 之类命令。

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

### Step 2.2.1：大范围历史日志的 timeout 策略

如果 scoped 查询已经失败，确实需要扫更大范围的历史日志：

- 先把 `--timeout` 提高到 `120000~300000`
- 当前 `jump-exec` 不支持“无超时”；必须传正整数毫秒值
- 文件顺序要按“最可能命中”排前面，不要把整天的超大 `info.log_*` 放在最前面
- 如果关键词更可能出现在 `statistic.log` 或特定小时文件，应先查这些文件，再决定是否扩大到全天

典型反例：

```bash
grep -R -n -m 80 "<keyword>" info.log_YYYY-MM-DD* error.log_YYYY-MM-DD* statistic.log_YYYY-MM-DD*
```

这类命令容易先把数 GB 的 `info.log_*` 从 00 点扫到 23 点，明明真正命中在后面的 `statistic.log_10.log`，最终却表现成 “30 秒还没结果”。

### Step 2.3：最近告警优先查当前文件尾部

如果告警时间距离“现在”较近，优先只查当前活跃日志文件尾部，不要立刻扫整文件。

推荐模式：

```bash
tail -n 5000 <log-root>/<project>/logs/error.log
tail -n 10000 <log-root>/<project>/logs/info.log
```

再在 tail 结果中按“时间 + 关键词”过滤，而不是直接从整文件开扫。

首轮当前小时查询上限：

- `error.log` 最多 `tail -n 5000`
- `info.log` 最多 `tail -n 10000`
- 第一轮默认**不查** `statistic.log`

只有第一轮没有结果，才允许把当前小时活跃文件尾部扩大到：

- `error.log` `tail -n 15000`
- `info.log` `tail -n 20000`

如果仍无结果，再考虑相邻小时归档文件或其他目标机。

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
tail -n 5000 "$LOG_DIR/error.log" 2>/dev/null | grep -m 20 -E "2026-06-23 18:1[5-9]|<traceId>|<accountId>|<method>|<异常关键词>" | tail -n 80
tail -n 10000 "$LOG_DIR/info.log" 2>/dev/null | grep -m 20 -E "2026-06-23 18:1[5-9]|<traceId>|<accountId>|<method>|<异常关键词>" | tail -n 120
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
- 如果出现超长单行日志，优先改成“先统计命中次数 / 只取关键字段行”，不要直接整行回显

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
tail -n 5000 "$LOG_DIR/error.log" 2>/dev/null | grep -m 20 -E "<告警分钟>|<traceId>|<accountId>|<method>|<异常关键词>" | tail -n 80
tail -n 10000 "$LOG_DIR/info.log" 2>/dev/null | grep -m 20 -E "<告警分钟>|<traceId>|<accountId>|<method>|<异常关键词>" | tail -n 120
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

补充约束：

- 从步骤 3 开始切换其他目标机前，先确认首个 target 的 hostname/IP 是否真实可达
- 切换目标机时优先用已验证过的真实 hostname 或告警 IP
- 不要因为一个简称超时，就对同项目所有别名批量并发重试

## 面向当前用户场景的默认优化

针对线上大日志文件场景，默认采用以下保守策略：

- 不跨服务乱扫；先用告警 `msg` 限定服务
- 不跨机器乱扫；先用告警 `ec2_ip` / provider / consumer IP 限定机器
- 不先扫 `statistic.log`
- 不先扫整个 `logs/` 目录
- 不返回超长 JSON body
- 不先并发探测多个可疑 target
- 先用一次 `hostname && date` 建立 target 可信度，再做日志查询
- 优先回答“consumer 卡住了还是 provider 卡住了”，而不是一次性捞所有链路日志
<!-- agentsshcli:managed:end -->

<!-- agentsshcli:local:start -->
> 本地扩展区说明：
>
> - 这里可以补充当前团队或个人的私有排障规则。
> - 后续执行 `agentsshcli sync-skills` 兼容更新时，这一段会尽量保留。
> - `env-map.md` 和其它私有配置不会被覆盖。
<!-- agentsshcli:local:end -->
