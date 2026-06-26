# Release Notes

## v0.1.7

本次发布补充了 `log-analyze` 的升级说明：

- 新增文档提醒：升级 `agentsshcli` 后，建议顺手执行 `agentsshcli doctor-skills` 和 `agentsshcli sync-skills`，让本地安装的 `log-analyze` 一起同步到新模板。
- 继续强化 `log-analyze` 的反常识排障习惯：遇到现象和常识不符时，优先检查全局拦截器、AOP、Filter、`HandlerInterceptor`、MyBatis 插件和注解增强链路。

验证：

- `node --check bin/agentsshcli.js`
- `npm run check:release`
- `cargo test --manifest-path native/Cargo.toml`

## v0.1.6

本次发布修复 JumpServer 搜索与跳板机超时续期的若干问题：

- `jump-search` 修复：搜索/列表完成后 JumpServer 停在资产子 prompt `[Host]>`（而非菜单 `Opt>`），旧逻辑只等 `Opt>` 导致超时后丢弃结果。现在结果等待正则同时匹配 `Opt>` 与 `[A-Za-z]+>` 子 prompt，搜索结果能被正确收尾返回。
- `jump-search` 新增 `--mode auto|list|search`（默认 `auto`）：先走 `/query` 搜索（已验证有效），为空再回退 `p` 列权限主机本地过滤；不再强制先 `p`。`search` 为旧行为，`list` 只列主机。
- `jump-search` 输出清洗增强：跳过菜单项、表格分隔线、表头（`ID | 主机名`）、翻页提示（`页码：`/`提示：`/`上一页...下一页`/`搜索：`），只保留真实主机行。

验证：

- `node --check bin/agentsshcli.js`
- `npm run check:release`
- `cargo test --manifest-path native/Cargo.toml`
- `agentsshcli jump-search --timeout 60000 prod.jumpserver adserving-api`（返回 4 台候选）
- `agentsshcli jump-search --timeout 60000 prod.jumpserver ad-service`（返回 2 台候选）
- `agentsshcli jump-search --timeout 60000 prod.jumpserver ad-conf`（返回 2 台候选）

## v0.1.4

本次发布聚焦 AI 安装与日志排障体验：

- 统一 `exec` / `jump-exec` 的超时模型：`--timeout` 改为“空闲超时自动续期”，只要远端持续输出就不会因为固定总时长被误杀；同时新增 `--total-timeout` 作为可选总上限保护。
- `jump-exec` 缓存复用继续保留；轻量探测、重复排障可复用连接，重日志场景则可按需配合更长 `--timeout` 或 `--no-cache`。
- `install-ai` 现在把 `cc-switch` 作为一等安装目标；如果检测到 `~/.cc-switch/skills`，交互式安装会默认推荐它作为共享主链安装根，同时仍允许切换到 `codex`、`claude` 等客户端。
- 初始化 `log-analyze` 映射时，文档、模板和安装提示统一改为收集“日志保存路径模式”，明确示例为 `/www/<project>/logs` 或 `/data/<project>/logs`，避免只记录 `/www`、`/data` 这类过粗根路径。
- 补充 JumpServer 大日志排障经验：整天归档日志检索建议使用更长 `--timeout`（通常 `120000~300000`），必要时再加 `--total-timeout`；当前仍不支持“无超时”。
- 增加大范围历史日志检索经验说明：优先按最可能命中的文件排序，避免把整天的超大 `info.log_*` 放在最前面导致看起来“30 秒没有结果”。
- 新增 `doctor-skills` / `sync-skills`：支持检测本地 `log-analyze` 是否过期、是否仍是旧版模板结构，并执行“尽量保留本地补充内容”的兼容更新。
- 修复 skill 兼容更新细节：非交互安装不会再静默跳过 `log-analyze` 升级；`env-map.template.md` 改为仅首次创建；版本比较支持 prerelease；兼容更新提示文案改成面向用户可理解的描述。

验证：

- `node --check bin/agentsshcli.js`
- `npm run check:release`
- `cargo test --manifest-path native/Cargo.toml`
- `agentsshcli jump-exec --timeout 15000 prod.jumpserver --target app-conf-02 "hostname && date +%F_%T"`
- `agentsshcli jump-exec --timeout 30000 prod.jumpserver --target app-conf-02 "ls -lh /www/app-conf/logs/info.log_2026-06-23* /www/app-conf/logs/error.log_2026-06-23* /www/app-conf/logs/statistic.log_2026-06-23* 2>/dev/null | head -20"`
- `agentsshcli jump-exec --timeout 30000 prod.jumpserver --target app-conf-02 "LC_ALL=C grep -n -m 20 --fixed-strings 'CHANNEL_CODE_EXAMPLE' /www/app-conf/logs/statistic.log_2026-06-23_10.log /www/app-conf/logs/error.log_2026-06-23.log /www/app-conf/logs/info.log_2026-06-23_10.log 2>/dev/null"`

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
