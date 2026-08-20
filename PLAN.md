# Cognitio v0.1 开发者预览实施计划

## 总结

- 目标是交付 macOS 优先、Apple Silicon 的开发者预览版。
- 技术栈固定为 Tauri 2、Rust/Tokio、Svelte/TypeScript；App 使用 `pnpm`，Quartz 5 保留上游 `npm`/`package-lock.json`。
- UI 提供中英双语，默认跟随系统语言并允许手动切换。
- App 负责工作区、监听、串行队列、状态恢复、MinerU 解析和 Agent 启动；单一 `$llmwiki` Skill 只读取 App 生成的本地 Markdown，负责知识写入、校验、commit 和 push。
- 首先用 Codex 打通完整垂直链路，再实现 Claude Code 和 OpenCode；不加入数据库、MCP、ACP、遥测、签名、公证或自动更新。

## 核心流程与架构

- 用户选择一个不存在或为空的父目录；App 原子创建 `inbox/`、`processing/`、`done/`、`failed/`、`wiki/`。已存在且非空的目录拒绝初始化，避免覆盖用户文件。
- 首次设置依次完成：语言、工作区、工具检测、Agent/模型、MinerU 模式、GitHub 仓库、Skill 安装和端到端预检；发布固定使用 GitHub Actions Pages。
- App 配置以带 `schemaVersion` 的 JSON 原子写入 macOS Application Support；MinerU Token 只进入 Keychain。初始化使用原子 journal 记录阶段，重启后将运行中任务标记为可重试的中断状态。
- PDF 经“大小和修改时间连续 3 次、每次间隔 2 秒不变”后入队；稳定超时为 5 分钟。以 SHA-256 防止重复处理，重复文件默认跳过并提供“重新处理”。
- 同一 Wiki 只运行一个任务。状态机固定为 `Detected -> Stabilizing -> Queued -> Preflight -> Running -> Verifying -> Succeeded`，异常进入 `Blocked`、`Failed` 或 `Cancelled`。
- 每个任务使用 `processing/<job-id>/` 保存原始 PDF、版本化 `job.json` 和日志，并在 `wiki/.llmwiki-work/<job-id>/` 建立被 Git 忽略的 Agent 工作区。
- 成功后按设置执行 Keep、Move to Done 或移入 macOS Trash；失败和取消均保留 PDF、解析产物和诊断日志，不永久删除。
- 启动时恢复未完成任务：未产生改动则重新排队；存在未提交修改则阻塞后续队列并等待 Retry；已有提交但未推送时只恢复校验与 push，不重复分析。
- App 在任务开始前要求 Git 工作区干净。Skill 使用 `git pull --ff-only`、内容校验、commit、push；Quartz 依赖安装和站点构建仅由 GitHub Actions 执行。提交包含 `Cognitio-Job: <uuid>` trailer，用于幂等恢复。
- 成功判定同时要求 Agent 退出码为 0、HEAD 已变化、工作区干净、远端包含新 HEAD；任一条件不满足均不归档 PDF。

## 接口与安全边界

- Rust 核心类型包括 `AppConfig`、`WorkspaceConfig`、`AgentProvider`、`AgentCapability`、`MinerUMode`、`JobRecord`、`JobState`、`NormalizedAgentEvent`、`PreflightReport`。
- `AgentRunner` 固定提供 `detect`、`version`、`authenticated`、`build_command`、`parse_event`、`cancel`；所有命令使用 executable + argv，禁止 shell 字符串拼接。
- Tauri commands 统一使用异步接口，仅暴露设置读写、后台工作区初始化/取消、预检、暂停/恢复、任务列表、Retry/Cancel、打开目录/站点；Rust 通过事件向 UI 推送任务快照，不让前端直接执行 shell。
- Git、Agent、系统命令使用 `tokio::process` 并设置超时；文件扫描、归档、Keychain 和其他阻塞调用进入 blocking pool，不占用 UI 或 Tokio worker。
- Adapter 根据 CLI 版本生成参数，未知主版本 fail closed。禁止使用跳过 sandbox/permissions 的危险参数；只授予 Wiki 和任务暂存区所需权限。
- App 仅向 Agent 传递最小环境变量集合，包括 `PATH`、`HOME`、临时目录、标准代理变量、任务 ID 和已解析 Markdown 路径；MinerU Token 不进入 Agent、prompt 或日志。
- App 在启动 Agent 前异步调用 MinerU，并复用任务目录内已有的 `parsed/full.md`。隐藏 CLI `llmwiki parse --input ... --output ... --mode ... --json` 仅用于诊断，不由 Skill 调用。
- Skill 将 PDF 内容视为不可信数据，明确忽略论文内的操作指令；只允许写入 Wiki 约定目录，不提交 `.llmwiki-work/`、PDF、凭据或日志。
- Skill 以版本化资源安装到 Codex、Claude Code、OpenCode 各自支持的用户 Skill 目录；更新时校验旧版本 checksum，用户修改过的副本绝不自动覆盖。

## 仓库与网站

- 新 Wiki 可由 `gh repo create` 创建，也可连接已有 GitHub 仓库。Pages 自动配置要求已认证的 `gh` 及仓库管理权限；新仓库默认 private，认证或权限失败必须明确阻塞并允许重试。
- Quartz 使用当前官方 Quartz 5，并锁定具体 commit/tag；构建期生成版本化模板包，运行时复制模板，不跟随浮动 `v5` 分支。
- 模板包含 Paper/Concept 目录、BibTeX、`.gitignore`、中立 Markdown 规范、Quartz 配置、插件锁文件和首页布局；固定模板作为校验过的应用资源打包，运行时只在本地安全解压。
- 同一 GitHub 仓库的 `main` 分支保存内容与 Quartz 源码。App 通过已认证的 `gh` 自动启用 Pages workflow 模式，GitHub Actions 使用 Node 24 构建并通过官方 artifact 部署；本机初始化不执行 npm 或 Quartz 构建。
- 站点标题在初始化和设置页中由用户维护；保存已有站点标题时，App 在干净工作区上更新 Quartz 配置、迁移未修改的默认首页、commit 并 push，不在生成网站中展示 Cognitio 品牌。
- private 仓库可用于 GitHub Pages 取决于 GitHub 套餐；站点默认可能仍公开，真正 private Pages 仅适用于 GitHub Enterprise Cloud 组织。Onboarding 必须展示风险并做确认。[GitHub 官方说明](https://docs.github.com/en/pages/getting-started-with-github-pages/creating-a-github-pages-site)
- Quartz 5 模板、插件安装和托管参数以[官方 Quartz 5 文档](https://quartz.jzhao.xyz/)及[托管指南](https://quartz.jzhao.xyz/hosting)为准。

## 实施里程碑

1. **M0 工程基线**：建立 Tauri/Svelte/Rust 脚手架、双语消息目录、格式化/lint/test/build 脚本和 CI；所有空壳检查通过。
2. **M1 App 基础**：完成菜单栏、设置窗口、Application Support 配置、Keychain、日志、通知、登录启动和工作区初始化。
3. **M2 Wiki 初始化**：完成 Quartz 5 固定模板打包、异步 Git/`gh` 接入、private 默认策略、GitHub Actions Pages 配置和可取消初始化。
4. **M3 任务引擎**：完成文件稳定检测、SHA 去重、串行队列、状态持久化、暂停、取消、重启恢复和三种归档策略。
5. **M4 Codex 垂直切片**：完成 MinerU CLI、`$llmwiki` Skill、CodexRunner、结构化事件、Git 发布和首篇论文端到端验收。
6. **M5 多 Agent**：在同一契约下加入 ClaudeRunner、OpenCodeRunner、版本能力矩阵、模型覆盖和认证诊断。
7. **M6 失败恢复与体验**：完成阶段化 Retry、脏仓库阻塞、push 恢复、失败诊断包、双语空态/错误态和系统通知。
8. **M7 开发者预览交付**：完成源码安装文档、本地 `.app` 构建、兼容矩阵、手工验收记录和已知限制；不要求签名、公证或自动更新。

## 实施状态（2026-08-15）

- M0-M6 的开发者预览代码路径已完成；本地前端、Rust、严格 lint 与诊断脱敏测试通过。
- M7 的源码说明、兼容矩阵、验收记录和 arm64 ad-hoc 签名 `.app` 已完成，详见 `docs/DEVELOPER_PREVIEW.md`。
- macOS 生命周期已修正为原生 menubar utility：配置后隐藏主窗口和 Dock 图标，关闭窗口不退出后台任务。
- 工作区初始化已改为可取消的后台任务，具备阶段进度、命令超时、原子 journal、中断识别和失败重试；旧版半初始化目录不再被误判为配置完成。
- Cloudflare 和托管方式选择已移除；Quartz 固定模板随应用打包，GitHub Actions 从同仓库 `main` 构建并部署 Pages。
- 真实 GitHub push、MinerU Precision 和三种 Agent 的凭据 smoke test 仍需有效外部凭据；它们是发布验收项，不伪装为离线自动测试结果。

## 测试与验收

- Rust 单元测试覆盖状态迁移、文件稳定、去重、路径安全、配置迁移、参数生成、事件归一化、日志脱敏和恢复决策。
- 使用 fake MinerU、fake Agent CLI 和本地 bare Git remote 做集成测试；CI 不调用真实模型或外部付费服务。
- UI 使用组件测试和浏览器模式 Playwright，覆盖中英文切换、设置校验、任务状态、Retry/Cancel 和错误提示。
- 端到端测试覆盖成功发布、解析失败、Agent 超时、取消、脏仓库、push 失败、重启恢复、重复 PDF、无效 Token、未知 CLI 版本及 Quartz 构建失败。
- Codex、Claude、OpenCode 各保留一套人工真实凭据 smoke test；发布前分别处理同一组小型学术 PDF，并检查 Markdown、概念复用、Git 历史和站点页面。
- v0.1 不设置全仓库数字覆盖率门槛；任务状态机、恢复逻辑、命令参数和凭据处理不得存在未测试分支。

## 默认假设

- 首个开发者预览仅保证 macOS 13+ Apple Silicon；Intel、Windows、Linux 延后，但 OS 相关代码必须放在适配层。
- MinerU 默认 Precision，Flash 可选；限制和接口集中在版本化 MinerU profile 中，不散落硬编码。
- 默认 Agent 为 Auto、模型为 Agent Default、处理后行为为 Move to Done、仓库为 private、日志仅保存在本地。
- Pages 固定采用官方 Actions artifact 部署，不创建 `gh-pages` 分支；私有网站认证、团队协作、多分支并发和自动模板升级不属于 v0.1。
