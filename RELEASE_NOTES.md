# Release Notes

## v0.1.3

本次发布聚焦 AI 安装与日志排障体验：

- `install-ai` 现在把 `cc-switch` 作为一等安装目标；如果检测到 `~/.cc-switch/skills`，交互式安装会默认推荐它作为共享主链安装根，同时仍允许切换到 `codex`、`claude` 等客户端。
- 初始化 `log-analyze` 映射时，文档、模板和安装提示统一改为收集“日志保存路径模式”，明确示例为 `/www/<project>/logs` 或 `/data/<project>/logs`，避免只记录 `/www`、`/data` 这类过粗根路径。
- 补充 JumpServer 大日志排障经验：整天归档日志检索建议使用更长 `--timeout`（通常 `120000~300000`），并明确当前 `jump-exec` 不支持“无超时”。
- 增加大范围历史日志检索经验说明：优先按最可能命中的文件排序，避免把整天的超大 `info.log_*` 放在最前面导致看起来“30 秒没有结果”。

验证：

- `node --check bin/agentsshcli.js`
- `agentsshcli jump-exec --timeout 15000 prod.jumpserver --target 172.31.117.179 "hostname && date +%F_%T"`
- `agentsshcli jump-exec --timeout 30000 prod.jumpserver --target 172.31.117.179 "ls -lh /www/hw-ad-conf/logs/info.log_2026-06-23* /www/hw-ad-conf/logs/error.log_2026-06-23* /www/hw-ad-conf/logs/statistic.log_2026-06-23* 2>/dev/null | head -20"`
- `agentsshcli jump-exec --timeout 30000 prod.jumpserver --target 172.31.117.179 "LC_ALL=C grep -n -m 20 --fixed-strings 'MLUAT1070177' /www/hw-ad-conf/logs/statistic.log_2026-06-23_10.log /www/hw-ad-conf/logs/error.log_2026-06-23.log /www/hw-ad-conf/logs/info.log_2026-06-23_10.log 2>/dev/null"`

## v0.1.2

本次发布包含两部分更新：

- 合并上游上传稳定性改进：SFTP 上传支持 `.part` 临时文件、`.part.meta` 续传元数据、断点续传、失败重试，以及 `stop-daemon` 连接池停止命令。
- 增强 AI 安装流程：`agentsshcli install-ai` 现在支持多客户端安装、主链客户端选择、次级客户端软链复用或复制、自定义 skills 根目录，以及首次安装后的标准化配置提示。
- 统一 AI 一键安装文案：继续支持“安装请阅读 `AI_INSTALL.md`，按说明安装 CLI 并添加 `SKILL.md`”这句提示，同时把实际执行流程扩展为 CLI + `agent-ssh-cli` skill + `log-analyze` skill 的完整安装。
- 优化多 npm 全局目录场景：安装文档和推荐命令默认按唯一全局前缀逐个安装，避免只装进某个工具自带的 npm 全局目录。
- 补充发布记录：仓库内新增和更新了 Obsidian 风格的 npm 发布说明，记录手动发布、Trusted Publishing 和后续标准发版步骤。

验证：

- `node --check bin/agentsshcli.js`
- `npm run check:release`
- `npm test`
