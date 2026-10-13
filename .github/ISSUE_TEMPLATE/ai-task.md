---
name: Codex AI engineering task
about: 可验证的 Chimera 开发任务（不自动授权推送、系统变更、合并或发布）
title: '[AI Task] '
labels: []
assignees: []
---

## 任务目标 / 可观察的验收标准

<!-- 具体场景、成功与失败表现；避免“实现一样即可”等模糊要求。 -->

## 非目标与任务边界

<!-- 本次不更新的子模块、依赖、UI、平台、系统状态。 -->

## 基线 / 参考实现映射

- Chimera master SHA：
- ref gitlink SHA / ref 工作区状态：
- ref 路径、文件、符号：
- Chimera 对应路径、文件、符号：
- 必要品牌差异 / legacy UI / E2E / agent / 兼容性偏离：
- 暂存差异与收敛计划：
- 批量上游复制计划（如适用）：复制的完整模块/文件范围、品牌替换白名单、应保留的 Chimera-only 扩展与许可证：

## 受影响调用链

<!-- main UI、legacy UI、Agent、hook、IPC、Rust application API、持久化、核心/Service、更新器/系统网络等。 -->

## 最高风险级别与审批

- [ ] L0 非行为性普通文档（不改变规范）
- [ ] L1 有界 UI/局部低风险
- [ ] L2 数据/IPC/核心/服务/系统权限/TUN/Agent 执行/依赖
- [ ] L3 治理/CI/安全门禁/签名/Secrets/Release

原因与人工审批责任人：

## 测试合同及运行环境

<!-- 初始状态、用户操作、独立结果来源、能让测试失败的反例、异常/取消/回滚；命令、平台、suite 注册、真实/fixture 区分。 -->

## 安全、隔离与回滚

<!-- 是否会改 host proxy、安装 Service、修改 TUN/路由或磁盘配置？仅在授权的专用环境中执行。 -->

## 交付要求

<!-- Reviewer、新上下文、PR、实际 CI 和 skipped、发布授权边界。Issue 本身不授予特权。 -->
