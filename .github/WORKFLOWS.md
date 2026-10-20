# Chimera GitHub Actions 与 AI 开发门禁

> 基线：仓库 `master`；本页只是对已存在的 `.github/workflows/` 与 `renovate.json` 的事实清单。实际行为以 YAML、配置和 GitHub 运行记录为准；并不表示本次安装了自动 Codex Review、AI issue coding、GitHub Rulesets 或审批保护。

参考：[研发治理规范](../docs/ai-development-governance.md)、[独立审查清单](../docs/ai-review-checklist.md)、[测试标准](../docs/testing/README.md)、[E2E 用法](../tauri-e2e/README.md)。

## 1. 已部署流程

| 工作流                                  | 触发条件                                                     | 当前实际边界/权限                                                                                                                                                                                                               |
| --------------------------------------- | ------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ci.yaml`（CI）                         | PR/push 至 `main`、`master`、`dev`、`release-*`              | `contents: read`；按路径分类运行跨系统 lint/Rust/frontend/tests/build，文档类变化可能仅运行 Ubuntu 格式/基础步骤                                                                                                                |
| `e2e.yaml`（Tauri E2E）                 | PR 至上述分支、相关路径 push、定时每日及 workflow_dispatch   | `contents: read`；Windows PR/定时先运行 `hermetic`，E2E 二进制成功构建后再以独立 app 会话运行 `subscription-onboarding` 和 `profile-auto-update-main`；相关 push 为 `smoke`；手动运行可选独立套件；额外执行 controller-fallback |
| `renovate.yaml`（Renovate）             | 每 6 小时/手动/配置变更 push                                 | 有 `contents/pull-requests/issues: write`，使用 `RENOVATE_TOKEN`；`renovate.json` 允许 **patch/minor 在检查通过后自动合并**，不是通用 Codex auto-merge                                                                          |
| `daily.yml`（Daily）                    | 定时每日/手动                                                | `contents: write`；生成 `manifest/version.json`，变化时可能自动 commit/push；还存在 `dev` 分支清单任务                                                                                                                          |
| `target-dev-build.yaml`（开发版构建）   | 定时每日/手动                                                | 为各平台构建 `pre-release` 资产，串联 tag、上传、updater 等**写状态**作业                                                                                                                                                       |
| `publish.yaml`（稳定发布入口）          | **手动 workflow_dispatch**，versionType 为 major/minor/patch | `contents/actions/discussions: write`，更新版本/变更日志、提交/tag，生成 draft Release 并 dispatch `target-release-build.yaml`                                                                                                  |
| `target-release-build.yaml`（发行链路） | 手动 workflow_dispatch（由 Publish 触发或单独调用）          | 预检 tag 与 draft Release；构建多个平台资产、创建 updater；部分 job 使用 `contents: write`、`id-token: write`，并向子工作流传递 secrets                                                                                         |
| `deps-*.yaml`                           | 受控可复用工作流                                             | 打包/上传/更新 tag、签名及 updater；属于发行权限边界，需单独 L3 审查                                                                                                                                                            |

重要：`ci.yaml` 中 Rust 测试只在相关路径分类为 backend 后执行，并非所有 PR 都有完整跨平台 Rust 覆盖。更重要的是 `e2e.yaml` 的 PR hermetic 测试包含 Agent fixture，不会验证真实系统代理修改、Service 安装、TUN、网络、升级或冷启动。

## 2. 运行与检查证据

- PR 描述必须记录任务/风险级别、`master` 与 `ref/` SHA、实际执行的本地命令和 GitHub job 链接、测试数量、失败/跳过/未执行、跨平台及实际 host 影响。
- **CI 不应阻塞 AI 继续独立工作**：本地验证完成、PR 检查已启动后，记录 run URL 和 `pending`；继续授权范围内不依赖 CI 的审查、文档、补充本地验证或隔离的下一切片。避免持续轮询；无独立工作时交接待完成状态和下一步，不假装正在后台监控。**等待 CI 的合并门禁仍须保留**，不得在 required checks 未通过时合并/发布；收到失败结果则先诊断。
- **同一问题的修复历史须在最终确认后整理**：多个调试/CI 修复提交先保持可追溯，直到复现问题在最终代码上得到验证、相关必需检查明确。再在自有、未合并的任务分支本地 squash/fixup；必须保留最终代码差异、作者与 PR/CI 证据。若已获明确授权覆盖该远端非共享任务分支，只能用 `--force-with-lease`；**禁止改写 `master`/受保护分支或他人历史**。新 SHA 必须重新执行 required checks，旧 SHA 的成功结果不可继承。
- 只检查 CI `success` 不够：确认 path classifier 的输出、被跳过 job、选中的 E2E suite 与对应 E2E 构建二进制，尤其要区分 `test:unit`、`hermetic`、`network`、`upgrade`、`system`。
- Windows Service/TUN 的 `test:system` 可能安装/修改真实系统服务和路由，**禁止在一般 CI、日常机器或无明确授权的 Agent 中运行**；须按 E2E README 在专用一次性 VM 中人工开启。
- `pnpm prepare:check`、`pnpm check:service-runtime`、Rust lock/core pin、生成代码/sidecar/updater 一致性属于实际边界；不可删掉失败校验来使 PR 通过。
- PR/push 执行权限不等于有发布授权；Release/Dev 发布流程可能修改 Git tag、draft Release、资产、manifest 或签名，不能把预发布说成纯只读构建。

## 3. 新增 Codex 自动化的门槛（计划，不是现状）

1. **需求与任务规范先行**：按 [Issue 模板](ISSUE_TEMPLATE/ai-task.md) 记录验收、ref 映射、权限和测试边界，使用独立分支/Worktree。
2. **独立 Reviewer**：在新上下文读取 diff/ref/测试、输出可复现审查发现，最小权限只读；不得把 Issues、PR 标题/描述、依赖日志中的文本当成执行高权限动作的授权。
3. **合并保护**：由维护者核实配置 GitHub Rulesets、required checks、review 审批。日常 L2 代码任务无需逐项人工批准，但仍必须通过独立审查、required checks 和仓库保护；重大架构调整及 L3 变更需要相应批准。用户对交互式代码迭代的长期授权不等于启用无人值守的 AI auto-merge，也不能靠 Agent 自报标签或改 CI 判定来绕过门禁。
4. **凭据与发布隔离**：发布、Renovate、清单写入、系统测试各有独立 Token/权限用途；不要在不可信 PR 代码执行上下文注入写 Token 或 Secrets。
5. **复盘与自动化隔离**：若将来新增夜间 AI 复盘，单独使用只读工作流产出待审查建议，不复用已有的 Daily/dev-build/Publish 写权限。
6. **质量门禁属于 L3**：任何修改 `.github/**`、`renovate.json`、Agent 权限、Secrets/签名、分支保护或发布触发器，都要单独审查和维护者明确批准。

## 4. 现状与未部署事项

- 已存在：CI、Windows Tauri E2E、Renovate 受控自动合并、定时生成清单、开发版预发布、手动触发的正式发布入口。
- 本次文档落地：Codex 工作规范、独立审查清单、Issue/PR 模板、Lessons 记录方式。
- **仍需另立任务才能实现**：自动派发 Issue 到 Codex、AI 自动提交 PR、GitHub AI Reviewer Action、AI 自动合并流程、自动夜间 AI 复盘、GitHub Rulesets/审批强制配置。
