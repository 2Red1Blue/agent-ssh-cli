<div align="center">

# agent-ssh-cli

基于 CLI 的 SSH 代理工具，按 ssh-mcp-server 的能力映射为 Agent 可调用的远端操作能力。

远程执行 · 文件上传 · 文件下载 · 连接配置 · 命令白名单 · 命令黑名单 · Agent Skill 集成 · JumpServer 跳板机执行

<p>
  <a href="https://github.com/2Red1Blue/agent-ssh-cli"><img src="https://img.shields.io/badge/CLI-agentsshcli-2ea44f" alt="CLI agentsshcli"></a>
  <a href="https://github.com/2Red1Blue/agent-ssh-cli/blob/main/LICENSE"><img src="https://img.shields.io/badge/License-MIT-green" alt="License MIT"></a>
  <a href="https://nodejs.org/"><img src="https://img.shields.io/badge/Node.js-%3E%3D18-339933?logo=node.js&logoColor=white" alt="Node.js >=18"></a>
  <a href="https://www.npmjs.com/"><img src="https://img.shields.io/badge/npm-%3E%3D8-CB3837?logo=npm&logoColor=white" alt="npm >=8"></a>
  <a href="https://github.com/2Red1Blue/agent-ssh-cli"><img src="https://img.shields.io/badge/sys-win%2Fmac%2Flinux-0078D6" alt="sys win/mac/linux"></a>
  <a href="https://github.com/2Red1Blue/agent-ssh-cli/releases"><img src="https://img.shields.io/badge/release-v0.1.2-blue" alt="release v0.1.2"></a>
  <a href="https://github.com/2Red1Blue/agent-ssh-cli/pulls"><img src="https://img.shields.io/badge/PRs-welcome-brightgreen" alt="PRs welcome"></a>
</p>

[AI 一键安装](#ai-一键安装) · [手动安装](#手动安装) · [配置](#配置) · [JumpServer 跳板机模式](#jumpserver-跳板机模式) · [卸载和清理](#卸载和清理) · [许可证](#许可证) · [友情链接](#友情链接)

中文 | [English](README_EN.md)

</div>

## 简介
本项目参考 [classfang/ssh-mcp-server](https://github.com/classfang/ssh-mcp-server) 的 SSH 操作能力设计，改写为独立 CLI 形式。感谢原项目提供的思路和能力基础。

#### 他能做的事：
- 解放双手，自动运维服务器
- 部署代码，更新部署docker
- 配置nginx,配置证书
- 所有ssh能做到的事情
#### 他的能力：
- 列出本地配置中的 SSH 服务器连接
- 在指定远端服务器上执行命令
- 上传本地文件到远端服务器，支持临时文件、断点续传和失败重试
- 从远端服务器下载文件到本地
- 通过命令黑白名单限制可执行命令
- 通过本地路径白名单限制上传和下载访问范围
- 通过 JumpServer 跳板机以菜单 PTY 模式连接目标主机（`jump-exec` 子命令）

## 上传稳定性

上传会先写入远端 `<remotePath>.part` 临时文件，并写入 `<remotePath>.part.meta` 续传元数据；完成后校验大小，再 rename 为正式目标文件。上传中断后，下次上传同一个本地文件到同一个远端路径会从已有 `.part` 大小继续。

`--no-cache` 上传可用 `Ctrl+C` 停止当前进程；daemon 模式可用 `agentsshcli stop-daemon` 停止连接池进程，但它会影响同一 daemon 内其它任务，不是精确取消单个上传。

## AI 一键安装

AI 一键安装：

```
安装请阅读 https://github.com/2Red1Blue/agent-ssh-cli/blob/main/AI_INSTALL.md，按说明安装 CLI 并添加 `SKILL.md`。
```

这句话现在仍然可用。AI 读完 `AI_INSTALL.md` 后，会继续完成：

- 多 npm 前缀检查与逐个全局安装
- 交互式选择客户端：`codex` / `claude` / `opencode` / `hermes` / `custom`
- 选择主链客户端
- 选择其余客户端是软链复用还是分别复制
- 安装 `agent-ssh-cli` 与 `log-analyze`
- 初始化 `~/.agent-ssh-cli/config.json` 和主链 `env-map.md` 模板
- 提示你重启客户端后继续交互式补配置，直到客户端里能看到 `log-analyze`

如果你手动执行，一行命令也可以完成当前这套安装效果：

```bash
for npm_bin in $(which -a npm 2>/dev/null | awk '!seen[$0]++'); do
  prefix="$("$npm_bin" prefix -g 2>/dev/null)" || continue
  case " ${SEEN_PREFIXES:-} " in
    *" $prefix "*) continue ;;
  esac
  SEEN_PREFIXES="${SEEN_PREFIXES:-} $prefix"
  "$npm_bin" install -g @2red1blue/agentsshcli || exit 1
done
agentsshcli install-ai
```

如果你希望显式进入交互式客户端选择流程，也可以直接：

```bash
agentsshcli install-ai --interactive
```

如果你是这个仓库的维护者，准备通过 GitHub Actions 自动发布 npm 包，请额外阅读：

- [docs/NPM_PUBLISH_GUIDE.md](docs/NPM_PUBLISH_GUIDE.md)

当前仓库的自动发版方案已经切换为 npm Trusted Publishing，不再依赖长期 `NPM_TOKEN`。

如果希望 AI 直接支持日志排障，可同时安装本地 `log-analyze` skill。推荐组合：

- `agent-ssh-cli`：负责 JumpServer / SSH 执行
- `log-analyze`：负责环境识别、target 映射、日志排查流程

首次安装后，如果要让 AI 交互式补齐你自己的环境映射，请参考：

- [docs/AI_FIRST_RUN_LOG_ANALYZE.md](docs/AI_FIRST_RUN_LOG_ANALYZE.md)

## 手动安装
### 环境要求

- Node.js `>= 18`
- npm `>= 8`
- 系统支持 Windows / macOS / Linux
- 本机网络可访问目标 SSH 服务器
- 如使用私钥认证，私钥文件需对当前用户可读
- 预编译平台包支持 macOS arm64/x64、Linux x64/arm64、Windows x64

### 安装步骤

1. 全局安装：

```bash
for npm_bin in $(which -a npm 2>/dev/null | awk '!seen[$0]++'); do
  prefix="$("$npm_bin" prefix -g 2>/dev/null)" || continue
  case " ${SEEN_PREFIXES:-} " in
    *" $prefix "*) continue ;;
  esac
  SEEN_PREFIXES="${SEEN_PREFIXES:-} $prefix"
  "$npm_bin" install -g @2red1blue/agentsshcli || exit 1
done
agentsshcli --help
```

如果机器上同时存在多套 Node/npm（例如 Hermes、自带 Node、Homebrew Node），推荐始终按上面的“扫描 `which -a npm` 并按唯一全局前缀逐个安装”方式执行，避免 CLI 只装进其中一套工具自己的全局目录。

2. 导入 SKILL.md:

不再推荐手工“打开 `SKILL.md` 然后自己拷贝”作为默认流程。  
优先使用：

```bash
agentsshcli install-ai --interactive
```

只有在以下场景才建议手工安装：

- 目标客户端不是内置的 `codex / claude / opencode / hermes`
- 用户明确要求自己指定 skills 根目录
- 需要把 skill 装到项目级目录而不是全局目录

如果是未知客户端，推荐直接用：

```bash
agentsshcli install-ai --clients custom --client-root custom=/absolute/path/to/skills
```


## 配置

初始化配置（格式参数和ssh-mcp-server一致）：

```bash
mkdir -p ~/.agent-ssh-cli
```

编辑 `~/.agent-ssh-cli/config.json`，填写真实连接信息。默认配置文件也可以通过环境变量覆盖：

可以通过以下环境变量修改配置地点
```bash
AGENT_SSH_CONFIG=/path/to/config.json
```

配置文件是数组，每一项是一台服务器：

- `name`: 连接名，必须唯一
- `host`: SSH 主机地址
- `username`: SSH 用户名
- `password` / `passwordRef` / `privateKey`: 认证方式，密码、密码引用、私钥三类认证只能保留一种
- `port`: SSH 端口，默认 `22`
- `passphrase`: 私钥口令，仅配合 `privateKey` 使用
- `socksProxy`: SOCKS5 代理地址，例如 `socks5://127.0.0.1:1080`；也可省略协议写成 `127.0.0.1:1080`
- `jumpHost`: 跳板机连接名，填写配置文件中另一台机器的 `name`
- `pty`: 是否分配伪终端，默认 `false`，也可通过 `exec --pty` 临时开启
- `allowedLocalPaths`: 额外允许上传或下载写入的本地路径
- `commandWhitelist`: 命令白名单正则数组
- `commandBlacklist`: 命令黑名单正则数组

`commandWhitelist` 和 `commandBlacklist` 使用 JavaScript `RegExp` 语法，不是 POSIX 正则；空白字符请写成 `\\s`，不要写 `[:space:]`。

完整示例见 [example.config.json](example.config.json)。`~/.agent-ssh-cli/config.json` 保存真实连接信息。

为防止配置文件中的密码泄露，密码认证会在第一次使用该服务器时被动加密保存：首次写入明文 `password` 后，执行 `exec`、`upload` 或 `download` 连接该服务器时，CLI 会把密码加密保存到配置目录下的 `secrets.json`，生成本地 `secret.key`，并把配置中的 `password` 置空、写入 `passwordRef`。后续运行通过 `passwordRef` 解密认证；如需修改密码，把空的 `password` 重新填成新密码，下次连接会自动覆盖旧密文。

参考配置

```json
[
  {
    "name": "密码服务器",
    "host": "192.0.2.10",
    "port": 22,
    "username": "root",
    "password": "",
    "passwordRef": "agentsshcli:密码服务器",
    "jumpHost": "jump-server",
    "commandBlacklist": [
      "(^|[;&|()\\s])rm(\\s|$)",
      "(^|[;&|()\\s])shutdown(\\s|$)",
      "(^|[;&|()\\s])reboot(\\s|$)"
    ]
  },
  {
    "name": "jump-server",
    "host": "198.51.100.20",
    "port": 22,
    "username": "ubuntu",
    "privateKey": "/path/to/jump_key",
    "passphrase": "******",
    "socksProxy": "socks5://127.0.0.1:1080"
  },
  {
    "name": "密钥服务器",
    "host": "198.51.100.10",
    "port": 22,
    "username": "deploy",
    "privateKey": "/path/to/id_rsa",
    "passphrase": "******",
    "pty": false,
    "allowedLocalPaths": [
      "./tmp",
      "./dist"
    ],
    "commandWhitelist": [
      "^pwd$",
      "^ls(\\s|$)",
      "^cat\\s+/var/log/app\\.log$"
    ],
    "commandBlacklist": [
      "(^|[;&|()\\s])rm(\\s|$)",
      "(^|[;&|()\\s])shutdown(\\s|$)",
      "(^|[;&|()\\s])reboot(\\s|$)"
    ]
  }
]
```
测试命令

```bash
agentsshcli list
agentsshcli exec --no-cache 密码服务器 "pwd"
agentsshcli exec --pty 密码服务器 "tty"
agentsshcli exec 密码服务器 --command-file ./script.sh --timeout 60000
```
完成安装!

## JumpServer 跳板机模式

`jump-exec` 子命令支持通过 JumpServer 跳板机以菜单 PTY 模式登入目标主机执行命令，
适用于线上服务器只能经堡垒机访问的场景。原有 `exec / upload / download` 直连行为
完全不变；JumpServer 模式由连接配置中的 `jumpServer` 字段开启。

### 命令格式

```bash
agentsshcli jump-exec <gatewayConnection> --target <hostOrIp> "<command>" [--timeout <ms>]
```

- `<gatewayConnection>`：在 `config.json` 中配置了 `jumpServer.enabled=true` 的连接名
- `--target`：目标机器 hostname 或 IP（JumpServer 菜单会用它做搜索）
- `--timeout`：可选，默认 60000ms。命令执行阶段沿用此预算，遇到高负载机器可调大

> upload / download 不支持 JumpServer 模式，请改用直连。

### 一次性生成跳板机配置（推荐 AI 使用）

让 AI 收集完跳板机参数后，一行命令把配置写入 `~/.agent-ssh-cli/config.json`，
无需手动编辑 JSON：

```bash
agentsshcli add-jump-server \
  --name prod.jumpserver \
  --host 39.108.163.91 \
  --port 8390 \
  --username liuzx \
  --private-key /Users/liuzx/Downloads/liuzx-jumpserver.pem
```

子命令会自动填入：

- `pty: true`
- `jumpServer.enabled: true`
- `jumpServer.promptRegex: "Opt>\\s*$"`（标准 JumpServer 菜单 prompt）
- `jumpServer.shellPromptRegex: "(?m)[#$>]\\s*$"`
- `jumpServer.searchPrefix: "/"`、`charDelayMs: 60`、`enterStrategy: "direct-then-search"`
- 默认 `commandBlacklist`（拒绝 `rm` / `truncate` / `reboot` / `shutdown` / `systemctl stop|restart|reload` / `kill` / `>` / `>>`）

文件不存在会自动创建，权限置为 `0600`。同名连接已存在时报错；加 `--force` 覆盖。

### AI 交互式生成模板

把下面这段贴给 AI（Claude Code / 任意 agent）即可让它带你走一遍：

```
你是 agent-ssh-cli 跳板机配置助手。请按以下顺序问我，每次只问一个问题，
收到回答后立即更新草稿，最后用 agentsshcli add-jump-server 一次性写入。

要收集的参数：
1. 连接名（建议 prod.jumpserver / test.jumpserver，需唯一）
2. JumpServer host（IP 或域名）
3. SSH 端口（默认 22，跳板机通常自定义端口如 8390 / 2222）
4. SSH 用户名
5. 私钥绝对路径（PEM 格式，私钥需对当前用户可读）

收集完执行：
  agentsshcli add-jump-server --name <name> --host <host> --port <port> \
    --username <user> --private-key <key>

写入后用以下命令验证：
  agentsshcli list
  agentsshcli jump-exec <name> --target <一个已知的目标主机> "hostname"
```

### 手动编辑配置（不推荐）

如需手动编辑，参考下面这段完整字段：

```json
{
  "name": "prod.jumpserver",
  "host": "39.108.163.91",
  "port": 8390,
  "username": "liuzx",
  "privateKey": "/Users/liuzx/Downloads/liuzx-jumpserver.pem",
  "pty": true,
  "jumpServer": {
    "enabled": true,
    "promptRegex": "Opt>\\s*$",
    "shellPromptRegex": "(?m)[#$>]\\s*$",
    "searchPrefix": "/",
    "charDelayMs": 60,
    "enterStrategy": "direct-then-search"
  },
  "commandBlacklist": [
    "(^|[;&|()\\s])rm(\\s|$)",
    "(^|[;&|()\\s])truncate(\\s|$)",
    "(^|[;&|()\\s])reboot(\\s|$)",
    "(^|[;&|()\\s])shutdown(\\s|$)",
    "(^|[;&|()\\s])systemctl\\s+(stop|restart|reload)(\\s|$)",
    "(^|[;&|()\\s])kill(\\s|$)",
    ">",
    ">>"
  ]
}
```

`jumpServer` 字段说明：

| 字段 | 默认值 | 说明 |
|---|---|---|
| `enabled` | 必填 | 必须为 `true`，`jump-exec` 才允许使用该连接 |
| `promptRegex` | `Opt>\\s*$` | 网关菜单 prompt 正则 |
| `shellPromptRegex` | `(?m)[#$>]\\s*$` | 进入目标后 shell prompt 正则 |
| `searchPrefix` | `/` | 搜索模式前缀（菜单输入 `/<hostname>` 触发搜索） |
| `charDelayMs` | 60 | 慢速发送字符延迟（毫秒），防止菜单丢字 |
| `enterStrategy` | `direct-then-search` | `direct` = 只直接发 target；`direct-then-search` = 先直接发，超时再走搜索模式 |

### 使用示例

```bash
# 单机
agentsshcli jump-exec prod.jumpserver --target hwtf-adserving-api-02 "hostname && uptime"

# 多机循环
for host in hwtf-adserving-api-01 hwtf-adserving-api-02; do
  echo "=== $host ==="
  agentsshcli jump-exec prod.jumpserver --target $host "uptime"
done

# 拉日志最近 50 行
agentsshcli jump-exec prod.jumpserver --target hwtf-adserving-api-02 \
  "tail -50 /www/hw-adserving-api/logs/app.log"

# test 环境（IP 目标）
agentsshcli jump-exec test.jumpserver --target dz0-153 "hostname"
```

### 限制

- 仅支持 PEM 私钥认证（不支持密码、不支持 passphrase 加密的私钥）
- `upload` / `download` 不支持 JumpServer 模式（PTY 菜单环境无法保证 SFTP 通道）
- 默认 PTY 宽度 200，防止长行被折叠破坏 marker 检测
- 命令执行阶段超时跟随 `--timeout`（最低 10s），遇到高负载机器请调大

## 卸载和清理

更新到最新版：

```bash
npm install -g @2red1blue/agentsshcli@latest
```

卸载:

```bash
npm uninstall -g @2red1blue/agentsshcli
npm cache clean --force
#删除配置文件
rm -rf ~/.agent-ssh-cli
```

## 许可证

[MIT](LICENSE)

## 友情链接

- [LINUX DO - 新的理想型社区](https://linux.do/)
