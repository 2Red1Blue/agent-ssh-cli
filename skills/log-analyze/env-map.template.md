# log-analyze Environment Map

这是本机 `log-analyze` skill 的私有环境映射模板。

首次安装后，请由 AI 交互式补齐，不要把真实线上信息提交回公共仓库。

## JumpServer Connections

- `prod`
  - 线上 JumpServer 连接名
- `test`
  - 测试 JumpServer 连接名（如果有）

## Environment Model

- 默认环境：`prod`
- `prod`
  - 通过哪个 JumpServer 进入
  - target 通常用 hostname 还是 IP
  - 日志根目录
- `yfb`
  - 是否与 prod 共用 JumpServer
  - target 通常用 hostname 还是 IP
  - 日志根目录
- `test`
  - 通过哪个 JumpServer 进入
  - target 通常用 hostname 还是 IP
  - 日志根目录

## Project Aliases

- 在这里补常用项目简称

## Targets

- 在这里补每个项目在各环境下的默认目标机

## Machine Shortcuts

- 在这里补机器简称到 hostname/IP 的映射

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
