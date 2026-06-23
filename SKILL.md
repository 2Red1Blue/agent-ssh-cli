---
name: agent-ssh-cli
description: 使用基于 SSH 的 CLI 安全操作已配置的远端服务器。适用于需要列出连接、远程执行命令、上传/下载文件、通过 JumpServer 跳板机以菜单 PTY 模式访问目标主机，以及为跳板机一键生成连接配置的场景。
---

# agent-ssh-cli 使用说明

`agentsshcli` 是一个通过 npm 安装、由 Rust 原生执行器完成 SSH 操作的命令行工具，用于让 AI 或用户通过本地配置安全地操作远端服务器。

它能做的事：

- 列出本地配置中的 SSH 服务器连接
- 在指定远端服务器上执行命令
- 上传本地文件到远端服务器
- 从远端服务器下载文件到本地
- 通过命令黑白名单限制可执行命令
- 通过本地路径白名单限制上传和下载访问范围
- 通过 Rust daemon 短时间缓存 SSH 连接，减少连续操作时的重复连接开销
- **通过 JumpServer 跳板机以菜单 PTY 模式登入目标主机执行命令**（`jump-exec` 子命令）
- **一行命令为 JumpServer 跳板机生成完整连接配置**（`add-jump-server` 子命令，AI 友好）
- npm 安装会按当前系统自动拉取对应平台的 optional 预编译包，当前支持 macOS arm64/x64、Linux x64/arm64、Windows x64

它不做的事：

- 不保存或输出密码、私钥等敏感认证信息
- 不扫描网络或发现服务器，只使用配置文件中的连接
- 不绕过配置中的命令限制和本地路径限制

命令黑白名单使用 JavaScript `RegExp` 语法，不是 POSIX 正则。空白字符要写成 `\\s`，不要写 `[:space:]`。例如：

```json
{
  "commandBlacklist": [
    "(^|[;&|()\\s])rm(\\s|$)",
    "(^|[;&|()\\s])shutdown(\\s|$)",
    "(^|[;&|()\\s])reboot(\\s|$)"
  ]
}
```

## 安全确认

执行危险操作前必须先向用户确认，不能直接执行。

危险操作包括：

- 删除、清空、覆盖文件或目录，例如 `rm`、`truncate`、重定向覆盖、批量删除
- 清理缓存、日志、临时目录或业务数据
- 重启、关机、停止服务或杀进程，例如 `reboot`、`shutdown`、`systemctl stop`、`kill`
- 修改权限、所有者、系统配置或启动项，例如 `chmod`、`chown`、编辑 `/etc` 下文件
- 上传文件覆盖远端已有文件
- 下载文件覆盖本地已有文件
- 任何不可逆、影响线上服务、影响数据完整性的操作

确认时必须说明目标连接名、命令或文件路径、可能影响，并等待用户明确同意后再执行。

## 环境校验

调用前优先检查 CLI 本身是否可用：

```bash
agentsshcli --help
```

如果上面的命令失败，再向下检查基础环境：

```bash
node --version
npm --version
```

如果 `node` 或 `npm` 不存在，提示用户先安装 Node.js `>= 18` 和 npm `>= 8`。

CLI 可用后，再检查配置文件是否存在：

```bash
test -f "${AGENT_SSH_CONFIG:-$HOME/.agent-ssh-cli/config.json}"
```

如果配置文件不存在，提示用户创建配置文件，不继续执行 SSH 命令：

```bash
mkdir -p ~/.agent-ssh-cli
# 然后让用户编辑 ~/.agent-ssh-cli/config.json，填入真实服务器配置
```

默认配置文件：

```text
~/.agent-ssh-cli/config.json
```

为防止配置文件中的密码泄露，密码认证会在第一次使用该服务器时被动加密保存：如果目标连接的 `password` 是非空明文，下一次执行 `exec`、`upload` 或 `download` 连接该服务器前，CLI 会把密码加密写入配置目录的 `secrets.json`，生成本地 `secret.key`，并把 `config.json` 中该连接改成 `password: ""` 加 `passwordRef`。改密码时直接把空的 `password` 重新填成新密码，下一次连接会自动覆盖旧密文。私钥认证不参与这个流程。

隐藏后的密码配置示例：

```json
{
  "name": "server",
  "host": "192.0.2.10",
  "port": 22,
  "username": "root",
  "password": "",
  "passwordRef": "agentsshcli:server"
}
```

指定其它配置文件：

```bash
AGENT_SSH_CONFIG=/path/to/config.json agentsshcli list
```

如果 CLI 不可用但 Node/npm 正常，提示用户安装：

```bash
npm install -g agent-ssh-cli
agentsshcli --help
```

从源码开发或本地调试时，需要先构建 Rust 原生执行器：

```bash
npm run build:native
npm test
```

## 全局参数

- `--config <path>`: 指定配置文件路径，优先级高于默认配置
- `--help`, `-h`: 输出帮助
- `--version`, `-v`: 输出版本

`exec`、`upload`、`download` 默认使用 Rust daemon 连接缓存，并支持以下缓存参数：

- `--no-cache`: 跳过 Rust daemon 连接缓存，本次命令独立建立并关闭连接
- `--cache-ttl <ms>`: 设置 Rust daemon 连接缓存空闲毫秒数，默认 `180000`

缓存参数属于子命令级参数，必须放在 `exec`、`upload`、`download` 后、连接名或 `--connection` 前。放在命令末尾会被当作未知参数。

## list

列出配置中的服务器。

```bash
agentsshcli list
agentsshcli list --json
```

参数：

- `--json`: 输出 JSON 格式。当前默认输出也是 JSON。
- `--config <path>`: 指定配置文件

返回值：

- 成功时 stdout 输出服务器数组，只包含 `name`、`host`、`port`、`username`
- 不输出密码、私钥、passphrase、黑白名单等敏感或控制字段
- 退出码为 `0`

示例输出：

```json
[
  {
    "name": "服务器",
    "host": "192.0.2.10",
    "port": 22,
    "username": "root"
  }
]
```

## exec

在远端执行命令。

位置参数形式：

```bash
agentsshcli exec "<connectionName>" "<command>"
agentsshcli exec --no-cache "<connectionName>" "<command>"
agentsshcli exec --cache-ttl 60000 "<connectionName>" "<command>"
agentsshcli exec --pty "<connectionName>" "<command>"
agentsshcli exec --no-pty "<connectionName>" "<command>"
```

命名参数形式：

```bash
agentsshcli exec --connection "<connectionName>" --command "<command>" --directory "/root" --timeout 5000
agentsshcli exec --connection "<connectionName>" --command-file "./script.sh" --timeout 5000
agentsshcli exec --no-cache --connection "<connectionName>" --command "<command>"
```

参数：

- `<connectionName>`: 连接名
- `<command>`: 远端命令
- `--connection <name>`, `-c <name>`: 连接名
- `--command <command>`: 远端命令
- `--command-file <path>`: 从本地 UTF-8 文件读取远端命令，适合执行多行脚本，文件必须使用 LF 换行，不能使用 Windows CRLF 换行；不能和 `--command` 或位置参数 `<command>` 同时使用
- `--directory <dir>`, `-d <dir>`: 远端工作目录
- `--timeout <ms>`, `-t <ms>`: 超时毫秒值，默认 `30000`
- `--pty`: 本次命令分配伪终端，优先级高于配置文件
- `--no-pty`: 本次命令不分配伪终端，优先级高于配置文件
- `--no-cache`: 不复用连接，必须放在连接名或 `--connection` 前
- `--cache-ttl <ms>`: 连接缓存空闲毫秒数，必须放在连接名或 `--connection` 前

使用 `--command-file` 时，必须确保脚本文件是 LF 换行。CRLF 文件会把 `\r` 传到远端 bash，可能导致 `$'xxx\r': command not found`。

macOS/Linux 推荐写法：

```bash
cat > /tmp/remote-command.sh <<'EOF'
pwd
EOF
agentsshcli exec --connection "<connectionName>" --command-file /tmp/remote-command.sh
```

Windows PowerShell 推荐显式写 LF：

```powershell
[System.IO.File]::WriteAllText("$env:TEMP\remote-command.sh", "pwd`n", [System.Text.UTF8Encoding]::new($false))
agentsshcli exec --connection "<connectionName>" --command-file "$env:TEMP\remote-command.sh"
```

返回值：

- 成功且有 stdout 时，stdout 输出远端命令结果
- 成功但无 stdout 时不输出内容
- 退出码为 `0`
- 远端命令非零退出、超时、命中黑名单、未命中白名单或连接失败时，stderr 输出错误信息，退出码为 `1`

## upload

上传本地文件到远端。

位置参数形式：

```bash
agentsshcli upload "<connectionName>" "<localPath>" "<remotePath>"
agentsshcli upload --no-cache "<connectionName>" "<localPath>" "<remotePath>"
```

命名参数形式：

```bash
agentsshcli upload --connection "<connectionName>" --local "./tmp/upload.txt" --remote "/usr/local/test/upload.txt"
agentsshcli upload --no-cache --connection "<connectionName>" --local "./tmp/upload.txt" --remote "/usr/local/test/upload.txt"
```

参数：

- `<connectionName>`: 连接名
- `<localPath>`: 本地文件路径
- `<remotePath>`: 远端目标文件路径
- `--connection <name>`, `-c <name>`: 连接名
- `--local <path>`, `-l <path>`: 本地文件路径
- `--remote <path>`, `-r <path>`: 远端目标文件路径
- `--no-cache`: 不复用连接，必须放在连接名或 `--connection` 前
- `--cache-ttl <ms>`: 连接缓存空闲毫秒数，必须放在连接名或 `--connection` 前

返回值：

- 成功时 stdout 输出 `File uploaded successfully`
- 退出码为 `0`
- 本地路径不在允许范围、远端写入失败或连接失败时，stderr 输出错误信息，退出码为 `1`

## download

下载远端文件到本地。

位置参数形式：

```bash
agentsshcli download "<connectionName>" "<remotePath>" "<localPath>"
agentsshcli download --no-cache "<connectionName>" "<remotePath>" "<localPath>"
```

命名参数形式：

```bash
agentsshcli download --connection "<connectionName>" --remote "/usr/local/test/upload.txt" --local "./tmp/download.txt"
agentsshcli download --no-cache --connection "<connectionName>" --remote "/usr/local/test/upload.txt" --local "./tmp/download.txt"
```

参数：

- `<connectionName>`: 连接名
- `<remotePath>`: 远端文件路径
- `<localPath>`: 本地目标文件路径
- `--connection <name>`, `-c <name>`: 连接名
- `--remote <path>`, `-r <path>`: 远端文件路径
- `--local <path>`, `-l <path>`: 本地目标文件路径
- `--no-cache`: 不复用连接，必须放在连接名或 `--connection` 前
- `--cache-ttl <ms>`: 连接缓存空闲毫秒数，必须放在连接名或 `--connection` 前

返回值：

- 成功时 stdout 输出 `File downloaded successfully`
- 退出码为 `0`
- 本地路径不在允许范围、远端读取失败或连接失败时，stderr 输出错误信息，退出码为 `1`

## help/version

```bash
agentsshcli --help
agentsshcli help list
agentsshcli help exec
agentsshcli help upload
agentsshcli help download
agentsshcli help jump-exec
agentsshcli help add-jump-server
agentsshcli --version
```

返回值：

- help 成功时 stdout 输出帮助文本，退出码为 `0`
- version 成功时 stdout 输出版本号，退出码为 `0`

## jump-exec（JumpServer 跳板机模式）

适用场景：目标主机只能经 JumpServer 堡垒机访问。CLI 内部完成：连网关 → 等菜单 prompt → 慢速发送 target → 等 shell prompt → 执行 marker 包装命令 → 截取输出和 exit code。

```bash
agentsshcli jump-exec <gatewayConnection> --target <hostOrIp> "<command>" [--timeout <ms>]
```

参数：

- `<gatewayConnection>`：在 `config.json` 中配置了 `jumpServer.enabled=true` 的连接名
- `--target <hostOrIp>`：目标主机的 hostname 或 IP
- `--timeout <ms>`：可选，默认 `60000`。命令执行阶段沿用此预算（最低 10s），高负载机器请调大到 `120000` 以上
- `--config <path>`：可选，覆盖默认配置路径

使用前提：

- 网关连接的配置必须包含 `jumpServer.enabled=true`，可用 `add-jump-server` 子命令生成
- 仅支持 PEM 私钥认证（不支持密码、不支持加密 passphrase 的私钥）
- `upload` / `download` 不支持 JumpServer 模式

返回值：

- 成功时 stdout 输出远端命令结果（已 strip ANSI、去除 marker 和命令回显）
- 退出码为 `0`
- 远端命令非零退出、超时、命中黑名单、未启用 jumpServer、目标主机不存在或连接失败时，stderr 输出错误信息，退出码为 `1`

使用示例：

```bash
# 单机执行
agentsshcli jump-exec prod.jumpserver --target hwtf-adserving-api-02 "hostname && uptime"

# 多机循环
for host in hwtf-adserving-api-01 hwtf-adserving-api-02; do
  echo "=== $host ==="
  agentsshcli jump-exec prod.jumpserver --target $host "uptime"
done

# 高负载机器需要更长 timeout
agentsshcli jump-exec --timeout 120000 prod.jumpserver --target hwtf-adserving-api-01 "ps aux --sort=-%cpu | head -10"

# 拉日志
agentsshcli jump-exec prod.jumpserver --target hwtf-adserving-api-02 "tail -100 /www/hw-adserving-api/logs/app.log"
```

## add-jump-server（一键生成跳板机配置，AI 优先使用）

如果用户要"配置 JumpServer 跳板机 / 添加堡垒机连接 / 给 agentsshcli 加跳板机"，**优先使用此子命令**而不是手写 JSON。它会一次性把所有 `jumpServer` 字段、默认黑名单、PTY 设置写入 `~/.agent-ssh-cli/config.json`。

```bash
agentsshcli add-jump-server \
  --name <connectionName> \
  --host <jumpserverHost> \
  --port <port> \
  --username <user> \
  --private-key <absolutePathToPem> \
  [--force]
```

参数：

- `--name`：连接名（唯一），建议 `prod.jumpserver` / `test.jumpserver` 之类的命名
- `--host`：JumpServer SSH 地址（IP 或域名）
- `--port`：可选，默认 `22`；跳板机通常用自定义端口如 `8390` / `2222`
- `--username`：SSH 用户名
- `--private-key`：私钥绝对路径，必须存在且可读，仅支持 PEM 格式
- `--force`：可选，同名连接已存在时覆盖（默认报错）
- `--config <path>`：可选，覆盖默认配置路径

自动写入的字段：

- `pty: true`
- `jumpServer.enabled: true`
- `jumpServer.promptRegex: "Opt>\\s*$"`（标准 JumpServer 菜单 prompt）
- `jumpServer.shellPromptRegex: "(?m)[#$>]\\s*$"`
- `jumpServer.searchPrefix: "/"`、`charDelayMs: 60`、`enterStrategy: "direct-then-search"`
- 默认 `commandBlacklist`：`rm` / `truncate` / `reboot` / `shutdown` / `systemctl stop|restart|reload` / `kill` / `>` / `>>`

返回值：

- 成功时 stdout 输出 "已写入 新增/覆盖 连接 <name> 到 <path>" 和下一步提示
- 退出码为 `0`
- 同名连接已存在且未加 `--force`、私钥不存在、参数缺失时，stderr 输出错误信息，退出码为 `1`

### AI 交互式收集参数流程（推荐）

当用户说"加一个 JumpServer 跳板机"时，按顺序问 5 个问题，**每次只问一个**，最后一次性调用 `add-jump-server`：

1. 连接名（建议命名 `prod.jumpserver` / `test.jumpserver`，需唯一）
2. JumpServer host（IP 或域名）
3. SSH 端口（默认 22，确认是否需要自定义）
4. SSH 用户名
5. 私钥绝对路径（PEM 格式）

收集完执行：

```bash
agentsshcli add-jump-server --name <name> --host <host> --port <port> \
  --username <user> --private-key <key>
```

然后用以下命令验证：

```bash
agentsshcli list
agentsshcli jump-exec <name> --target <已知目标主机> "hostname"
```

### 何时不用 add-jump-server

- 用户已经有 `~/.agent-ssh-cli/config.json` 且包含同名连接，且**不想覆盖** → 直接告诉用户当前配置已存在
- 用户使用密码或加密私钥认证 → `add-jump-server` 不支持，需手动编辑 JSON 并参考 README
- 用户需要自定义 `promptRegex` / `commandBlacklist` 等高级字段 → 先用 `add-jump-server` 生成基础结构，再手动修改对应字段

## 错误规则

- 参数重复时失败
- 命名参数和位置参数不能混用同一字段
- `--no-cache` 和 `--cache-ttl` 必须放在 `exec`、`upload`、`download` 后、连接名或 `--connection` 前
- `timeout` 和 `cache-ttl` 必须是正整数毫秒值
- `list` 不接受位置参数
- `upload` / `download` 的本地路径必须位于当前工作目录、项目目录或 `allowedLocalPaths` 内
- `jump-exec` 要求网关连接已配置 `jumpServer.enabled=true`，且命令不能命中 `commandBlacklist`
- `upload` / `download` 不支持 JumpServer 模式
- 所有失败统一在 stderr 输出错误信息，退出码为 `1`
