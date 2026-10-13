## 目标与变更范围

<!-- Issue、行为目标、非目标、变更类型、独立可回滚切片 -->

## 基线及 ref 对齐记录

- Chimera master 基线 commit：
- ref gitlink SHA / ref 工作区状态：
- ref → Chimera 的路径/符号/状态所有权映射：
- 必要差异（品牌 / legacy UI / E2E / agent / 兼容 / 已证实 ref 缺陷）：
- 暂存不一致与收敛条件：
- 上游批量迁移范围（如适用）：复制的目录/文件、品牌替换白名单、许可/版权核对及未完成的调用方检查：

## 用户入口及业务调用链

<!-- 主 UI / legacy UI / Agent → hook → IPC → Rust API → persistence → Core/Service/host；说明哪项实际覆盖。 -->

## 风险等级与人类审批

- [ ] L0 仅普通文档
- [ ] L1 局部低风险
- [ ] L2 数据/IPC/代理/TUN/授权/运行时/依赖/Agent 执行
- [ ] L3 AGENTS/治理/CI/审查门禁/Secrets/签名/发布

风险取最高级，理由及审批：

## 实际验证（不要把 skipped 当作通过）

| 测试/命令                                             | 环境/版本/平台 | 实际结果（pass/fail/skip/not run） | 日志或限制 |
| ----------------------------------------------------- | -------------- | ---------------------------------- | ---------- |
| pnpm lint:prettier / typecheck / 相关 unit（如适用）  |                |                                    |            |
| pnpm lint:frontend-boundaries（如适用）               |                |                                    |            |
| pnpm test:backend / clippy / rustfmt（如适用）        |                |                                    |            |
| pnpm check:service-runtime / prepare:check（如适用）  |                |                                    |            |
| E2E harness unit + 被注册的 suite/spec                |                |                                    |            |
| 主 UI 与 legacy UI 当前状态/刷新/持久化（如适用）     |                |                                    |            |
| Agent fixture 与真实授权/host side effect（分别记录） |                |                                    |            |
| 独立 VM/网络/TUN/升级/真实 OS 检查（如适用）          |                |                                    |            |
| GitHub CI 和 E2E 的实际运行 job                       |                |                                    |            |

## 远端 CI 交接状态（不空等、不绕过门禁）

- PR / CI / E2E Run URL：
- 必需检查：pending / pass / fail / skipped / 尚未核实（逐项列出）：
- 本地验证完成后的独立工作或交接事项：
- CI 失败的复现/根因与后续处置（若适用）：
- 同一问题的多个修复 commit（如适用）：初始/修复 SHA、最终问题已解决的证据、是否已在自有任务分支 squash/fixup：
- 若重写任务分支历史：原/新 HEAD SHA、最终 diff/署名核对、明确授权及 `--force-with-lease` 记录（不得改写保护分支）：
- **最终新 SHA** 的 required CI、独立审查及批准状态（旧 SHA 结果不能复用）：
- **未通过必需检查与人工审批前，不得合并或发布。**

## 隐私、安全及回滚

<!-- 无凭据和私有配置；服务、路由、系统代理、签名、updater 的权限及错误恢复边界。 -->

## 独立 Reviewer

<!-- 新上下文，按 docs/ai-review-checklist.md；审查发现与解决、审查不支持的测试范围、人工批准状态。 -->

## 合并与发布授权

<!-- 仅提交 PR 不代表授权 tag、签名、publish、Daily/dev-build、更新资产。L2/L3 必须有人类批准。 -->
