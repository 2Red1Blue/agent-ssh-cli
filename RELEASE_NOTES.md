# Release Notes

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
