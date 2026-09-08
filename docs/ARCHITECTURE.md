# Cognitio 逻辑与模块边界

本文描述当前实现。产品范围以 [PRD](../PRD.md) 为准，交付状态见 [PLAN](../PLAN.md)。

## 一条主流程

```text
watcher 稳定检测 -> jobs 创建任务 -> job_runtime 领取执行
  -> worker 编排
     -> parsing / mineru 解析与缓存校验
     -> 等待 Wiki 锁
     -> agent_process / agents 生成内容
     -> publication 校验、提交、推送、远端确认
     -> archive 归档并完成任务
  -> deployment 独立跟踪 Pages

jobs / RuntimeEvent -> app_events -> AppSnapshot
  -> app-controller -> Svelte 展示组件
```

最多三个任务并行解析。Agent 执行与论文发布持有同一把 Wiki 锁；站点标题更新和重试检查也使用该锁。初始化只在未配置工作区运行，正常队列在启动恢复完成后启用。

## 职责归属

| 模块                                         | 负责的决策或副作用                                                |
| -------------------------------------------- | ----------------------------------------------------------------- |
| `jobs.rs`                                    | 任务模型、合法转换、schema 迁移和串行读改写；原子替换 journal     |
| `job_runtime.rs`                             | 执行身份、领取、取消请求与结束确认、重试入口、Wiki 锁             |
| `worker.rs`                                  | 三槽调度和流水线编排；适配器注入支持离线流程测试                  |
| `recovery.rs`                                | 将任务记录与文件/Git 事实转成恢复决定；启动、重试、失败和取消共用 |
| `publication.rs` / `git.rs`                  | 论文变更白名单、精确任务 trailer、提交归属和远端确认；异步 Git    |
| `archive.rs`                                 | 先保存目的地再移动文件；保留被替换的源文件；归档中断恢复          |
| `parsing.rs` / `mineru.rs`                   | 解析缓存校验与 MinerU 协议                                        |
| `agent_process.rs` / `agents.rs`             | 子进程、输出、超时与取消；版本能力和参数生成                      |
| `deployment.rs`                              | Pages 状态；部署失败不重新分析论文                                |
| `settings.rs` / `setup.rs` / `bootstrap.rs`  | 设置保存、初始化流程、启动恢复                                    |
| `wiki.rs`                                    | 模板、仓库和 Pages 初始化、站点标题变更策略                       |
| `commands.rs` / `app_events.rs`              | Tauri 命令边界；统一推送快照和日志                                |
| `app-controller.svelte.ts`                   | 后端快照、可编辑草稿、各命令等待状态和错误                        |
| `job-presentation.ts`                        | 主窗口与菜单栏共用的统计和阶段文案                                |
| `api.ts` / `tauri-api.ts` / `browser-api.ts` | 环境选择、原生命令调用、浏览器演示数据                            |

## 必须保持的约束

- `job.json` 是任务事实来源。业务代码通过事件更新最新记录，不能把旧快照整体写回。`AppSnapshot` 是展示投影，不能替代 journal 决定任务是否可执行。
- 每次执行持有 `activeRun`。旧执行不能覆盖新执行；取消请求写入 `Cancelling`，进程停止并完成收尾后才能重试。进入发布后不接受取消。
- `state` 决定合法操作，`stage` 描述流程位置，`blockReason` 决定恢复分类。`phase` 只是文案，只有旧 schema 迁移可以解释历史文案。
- `taskCommit` 表示本地任务提交，不能等同于已发布。只有远端包含该提交且校验通过，才能进入归档。`PublicationOutcome::Blocked` 必须结束当前流水线。
- 启动和重试都重新核对 Git 事实。已有有效任务提交时恢复发布或归档；脏仓库阻塞队列；未知远端状态保留提交和 PDF。
- 论文发布只接受新增或修改的约定内容路径。站点标题操作有独立的配置文件策略，等待未完成任务结束后才执行。
- 前端只显示后端给出的 `allowedActions`。快照修订号阻止旧响应回退界面；后台事件不覆盖未保存草稿；保存期间继续编辑的字段保持新值。

## 恢复案例

| 中断位置或事实               | 后续行为                                       |
| ---------------------------- | ---------------------------------------------- |
| 解析中断、未生成 Wiki 内容   | 重新排队，只有 manifest 完整匹配才复用解析结果 |
| Agent 留下未提交变更         | 仓库级阻塞，用户处理变更后重试                 |
| 本地任务提交有效但 push 失败 | 保存提交，重试发布，不重复解析或生成           |
| 远端确认后归档中断           | 恢复到归档，使用已记录的目的地                 |
| 源 PDF 被替换                | 归档该任务的 processing 副本，保留新源文件     |
| Pages 失败                   | 显示部署错误，论文任务保持成功                 |
| journal 损坏或版本不支持     | 报错并暂停监听，避免静默丢失任务或重复入队     |

## 后续修改入口

新增 Agent 应扩展 `agents.rs` 的能力与命令策略，保持 `agent_process.rs` 的生命周期管理。新增处理阶段应先定义 `JobStage`、事件、恢复行为和中断测试，再接入 worker。新增界面操作应经 controller 和 API 调用 Rust，不在组件中重建业务状态机。

站点标题保存涉及 Git、配置文件和 Keychain，尚不是跨系统事务；发布后本地配置保存失败需要再次保存。当前不支持多个 Cognitio 进程共同写同一个工作区。

## 验证

运行仓库规定的 lint、Vitest、build、Cargo test、Clippy 和 `git diff --check`，并用 Playwright 检查主窗口和菜单栏。流水线测试使用假的解析器、假的 Agent 和真实本地 bare Git remote，可复现 push 失败及恢复；真实 GitHub、MinerU 和三种 Agent 的凭据验收仍见 [开发者预览](DEVELOPER_PREVIEW.md)。
