# log-analyze Environment Map

这是本机 `log-analyze` skill 的私有环境模板。

首次安装后，请由你当前使用的 AI 负责补齐和维护，不要把真实线上信息提交回公共仓库。
推荐把同目录下的 `env-map.json` 作为结构化事实源，再自动渲染出 `env-map.md` 供人类阅读。
这里记录的不是抽象“映射”概念，而是你平时会说的项目简称、机器简称、真实 hostname/IP、以及各项目真实日志目录。
用户通常只需要告诉 AI：JumpServer 与环境对应关系、日志路径模式、项目别名、常用主机列表；其余搜索、验证、写回，都由 AI 完成。

建议先执行：

```bash
agentsshcli env-map init --from-config
```

## JumpServer Connections

- `prod`
  - 线上 JumpServer 连接名
- `test`
  - 测试 JumpServer 连接名（如果有）

## JumpServer Aliases

- 在这里补 JumpServer 的口头别名到真实连接名的映射
- 例如：
  - `线上跳板机` -> `prod.jumpserver`
  - `预发跳板机` -> `prod.jumpserver`
  - `测试跳板机` -> `test.jumpserver`

## Environment Model

- 默认环境：`prod`
- `prod`
  - 通过哪个 JumpServer 进入
  - target 通常用 hostname 还是 IP
  - 日志保存路径模式（例如 `/www/<project>/logs` 或 `/data/<project>/logs`）
- `yfb`
  - 是否与 prod 共用 JumpServer
  - target 通常用 hostname 还是 IP
  - 日志保存路径模式（例如 `/www/<project>/logs` 或 `/data/<project>/logs`）
- `test`
  - 通过哪个 JumpServer 进入
  - target 通常用 hostname 还是 IP
  - 日志保存路径模式（例如 `/www/<project>/logs` 或 `/data/<project>/logs`）

## Environment Aliases

- 在这里补环境口头叫法到标准环境名的映射
- 例如：
  - `线上` / `生产` / `prod` -> `prod`
  - `预发` / `yfb` -> `yfb`
  - `测试` / `test` -> `test`

## Project Aliases

- 在这里补常用项目简称、业务名、仓库名到标准项目名的映射
- 例如：
  - `myservice1` / `服务01` -> `xxx-myservice1-01`, `xxx-myservice1-02`,
  - `myservice2` / `服务02` -> `xxx-myservice2-01`,`xxx-myservice2-03`,
  - `myservice3` / `服务03` -> `xxx-myservice3-01`

## Targets

- 在这里补每个项目在各环境下“已验证过”的默认目标机
- 如果用户一开始只知道机器简称或 IP，先验证成功再补真实 hostname

## Machine Shortcuts

- 在这里补机器简称到 hostname/IP 的映射
- 例如：`api-02` -> `xxx-myservice1-api-02`

## Log Rotation Notes

- 如果团队日志按小时归档，请记录命名规则
- 例如：
  - `info.log_YYYY-MM-DD_HH.log`
  - `statistic.log_YYYY-MM-DD_HH.log`
  - `error.log_YYYY-MM-DD.log`

## Verification

初始化完成后建议至少验证：

```bash
agentsshcli jump-exec --timeout 120000 <prod-connection> --target <known-prod-target> "hostname"
```

如果验证成功，请把返回的真实 hostname 写回本文件，避免后续继续只用模糊简称。
