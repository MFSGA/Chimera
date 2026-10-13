# Chimera — 独立 Code Review 清单

> 独立 Reviewer 需使用**与 Coder 不同的上下文**，查看真实 diff、固定的 `master`/ref 基线、相关代码链和实际测试/CI 日志。自称通过、fixture 响应和编译成功不能替代真实系统验收。按 [治理规范](ai-development-governance.md) 做 L0–L3 风险审批，并遵守 [测试标准](testing/README.md)。

## 1. 范围、对齐与权限

- [ ] 变更目标、非目标、影响路径、最高风险级别、人工批准及回滚方式明确；没有顺便重构、偷偷改 submodule/lockfile 或提交无关用户修改。
- [ ] 当前主仓库 `master` 基线和 `ref/` gitlink/checkout SHA、参考工作区状态已记录；若影响共通功能，ref 对应目录/文件/函数/数据模型/调用链有逐项映射。
- [ ] 仅保留必要品牌、legacy UI、E2E、agent 和可证明的兼容/缺陷差异；没有以同名接口掩盖平行业务实现，也没有照搬已确认 ref 缺陷。
- [ ] 若批量复制了上游文件/目录，已核对 ref SHA、完整文件范围、来源许可/版权、品牌映射白名单及实际替换结果；没有误改外部协议/IPC、序列化字段或第三方标识，也没有因整目录覆盖丢掉 Chimera-only 扩展。大范围复制不是免审查理由。
- [ ] 主 UI 和 legacy UI 使用同一业务状态/持久化/IPC/API/Core/Service；共享变更对两个入口影响已检查，未删除 legacy 页面/路由/窗口。
- [ ] Agent 仅通过共用 API 与业务事务访问数据；高权限修改保留用户确认、旧状态校验、执行结果观察、错误处理及恢复。

## 2. 行为、安全与数据

- [ ] 用户 Profile/订阅/设置/历史配置的兼容升级、备份、回滚和重复操作边界正确；失败或取消路径不会造成无声数据丢失。
- [ ] IPC/Rust 生命周期、任务取消、异步竞态、Core/Service 切换、进程/窗口重启、系统代理/TUN 资源所有权和旧进程清理有可靠路径。
- [ ] 操作系统权限（Windows UAC、macOS 权限、Linux、服务安装）无绕过；危险 host mutation 不因 fixture 测试而自动获得授权。
- [ ] 输入、订阅 URI、文件路径、下载、外部进程及代理参数经过必要校验；日志、截图、E2E Artifact、输出和 PR 中不泄露凭据或个人数据。
- [ ] pnpm/Cargo 锁文件、runtime pin、submodule、sidecar、签名/updater/发布资源的版本关系一致；必要生成文件用工具重建并检查，而不是人工改生成物绕过验证。
- [ ] 代码异常应给出可诊断且安全的错误，不以无边界重试/静默降级替代失败。

## 3. 测试合同和已执行证据

- [ ] 新增/修改测试符合 [T01–T* 测试合同](testing/README.md)：明确初始化、被测操作、**独立的结果观察来源**、能检出目标回归的反例，至少一个风险相关失败/取消路径（适用时）。
- [ ] 受影响的测试确实在 runner 中注册并执行；报告测试数量、filter/suite、环境/平台、日志或检查链接；跳过/未执行不当作通过。
- [ ] `pnpm --filter @chimera/tauri-e2e test:unit` 的显式列表不是所有测试；`test` 默认 smoke，PR CI 的 hermetic 不包括真实网络/upgrade/system。
- [ ] 主/legacy/agent 的关键入口、Rust IPC→持久化→运行态共享流按影响范围验证；fixture agent 修复不等于真实系统修复/授权。
- [ ] 有权限和数据变更时核对当前 UI 同步，再检查刷新/冷启动；没有用刷新、sleep、直接 IPC/DOM 操作掩盖 UI 失败。
- [ ] WebDriver 版本与受支持交互语义真实核对，不把元素 click 当作所有原生 PointerEvent、真实鼠标或系统侧授权。
- [ ] 如涉及 Windows Service/TUN，具备专用一次性 VM、宿主所有权和显式 opt-in；没有在普通共享机器或 GitHub runner 上无授权运行破坏性系统测试。
- [ ] CI 检查只宣称实际运行的步骤；无检查的 OS/feature、真机网络、Service 权限、签名验证明确列为未验证。
- [ ] 本地测试完成后，没有无意义地等待/反复轮询远端 CI；PR 记录 run URL 和 `pending`，独立工作已在授权范围内推进或已明确交接。未完成/失败的必需 CI 未被冒充为成功、被绕过合并或发布。

## 4. 发布、治理与 PR

- [ ] `AGENTS.md`、`docs/ai-*`、`LESSONS.md`、`.github/**`、权限/签名/升级流程的修改按 L3 明确维护者批准；其他 L2 高风险行为也有人工批准。
- [ ] 没有修改 workflow 权限、分支保护、强制测试、审计或状态检查来“让 PR 绿”；Renovate 已有自动合并策略未被误套在 Codex 任务上。
- [ ] 任务没有未经授权触发 daily/dev 预发布、`publish.yaml` 版本变更或 `target-release-build.yaml`；无凭据、Secret 或 Token 泄漏。
- [ ] 已填写 [PR 模板](../.github/pull_request_template.md)，写明风险、实际测试结果、仍阻断的问题、ref 映射、回滚方案、独立审查结论和人工决策。

## 5. 审查输出格式

| 严重级别       | 含义                                                                                     | 合并建议                     |
| -------------- | ---------------------------------------------------------------------------------------- | ---------------------------- |
| BLOCKER / HIGH | 数据丢失、授权/秘密泄漏、系统副作用、共享状态分叉、发布权限/质量门禁绕过、核心回滚错误等 | 阻止合并                     |
| MEDIUM         | 可复现行为问题、关键测试覆盖缺口、尚未闭环的兼容问题                                     | 修复或维护者记录风险后决定   |
| LOW            | 不改变行为或安全的改进建议                                                               | 可独立跟进，不当作已证实缺陷 |

每项发现包含：`[SEVERITY] 标题`、`file:line`、触发条件、预期/实际、代码或日志证据、影响范围、最小修复建议。不能泛称“已审查，无风险”。

```text
Base master SHA / ref SHA:
Risk: L0 / L1 / L2 / L3
Reviewer identity and context independence:
Diff scope + relevant paths:
Reference path/symbol mapping and intentional differences:
Findings: [severity, file:line, repro and evidence, suggested fix]
Actual check commands, tests, platform and results:
Skipped/blocked tests and device/network limitations:
Remaining risks and required maintainer approvals:
Decision: REQUEST_CHANGES / COMMENT / APPROVE_WITH_EVIDENCE
```
