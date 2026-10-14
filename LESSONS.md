# Chimera Engineering Lessons / 已验证工程经验库

供维护者和编码/审查 Agent 在相关模块工作前阅读。本文件记录**已证实的工程失败及其防复发措施**，不是 TODO 或推测清单。

- 遵循 [AGENTS.md](AGENTS.md)、[AI 治理规范](docs/ai-development-governance.md)、[ref 对齐](docs/ref-alignment-guide.md)、[测试验收规范](docs/testing/README.md)；经验记录不能自行放宽已有门禁。
- 只收录真实复现、已确定根因、有可核对的 Issue/PR/CI/commit 的缺陷。仅凭一次失败、CI 红灯或猜测不等于上游/本地已确诊。
- 区分“真正的产品回归”“参考实现缺陷”“测试边界/驱动限制”“环境/平台问题”；对于 ref 缺陷给出对应 SHA/路径和最小复现，记录必要的偏离与复查条件。
- 不收录密钥、完整订阅、用户配置、真实个人信息、未经脱敏的截图/日志；发生 host TUN/Service 副作用时注明所在隔离 VM 和恢复证明。
- 已验证问题优先补能检出旧实现的回归测试，并注册测试执行入口，确保 CI/独立审查能发现复发。
- 再次出现时审查重复原因、是否需要添加 CI 检查；修改 AGENTS/CI/Review 门禁属于 L3，只有维护者可批准。
- 对尚无根因的问题开 Issue 标记“待核实”，**不得填写本文件为已确认事故**。

## 已确认条目索引

| 编号                | 日期       | 模块      | 问题与证据                                                                                               | 状态 |
| ------------------- | ---------- | --------- | -------------------------------------------------------------------------------------------------------- | ---- |
| LESSON-20261014-001 | 2026-10-14 | Rust / CI | macOS epoch PID 集成用例第二次 spawn 收到 `NotFound`；PID 文件读取竞态是候选根因，PR #479 远端复验待完成 | Open |

## 单条模板

```markdown
### LESSON-YYYYMMDD-001：简短标题

- 日期与时区：
- 状态：Open / Mitigated / Verified
- 严重度：Critical / High / Medium / Low
- 模块：main UI / legacy UI / agent / IPC / Rust / Profile / Core / Service / TUN / E2E / CI / 发布
- master commit / ref gitlink SHA / 平台与测试环境：
- 关联 Issue / PR / CI / 运行日志（脱敏）：
- 预期行为：
- 真实异常、复现步骤与证据：
- 确认的根因与排除项：
- 修复提交、差异映射及保留的兼容性边界：
- 原实现失败、新实现通过的回归测试，及测试注册位置：
- 验证命令、实际用例数量、结果、未覆盖/跳过项：
- 风险/回滚和清理证据（如涉及宿主状态）：
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：
```

_历史条目只按可追溯证据收录，不从既有 Git 历史自动推断事实。_

### LESSON-20261014-001：epoch PID 记录可能在检查后、读取前被并发清理

- 日期与时区：2026-10-14 UTC
- 状态：Open（本地已修复，等待 PR CI/E2E 在新 SHA 上复验）
- 严重度：Low
- 模块：Rust / CI
- master commit / ref gitlink SHA / 平台与测试环境：master 基线 `e9e3fab1913773a1ecd6ad6a689175e523208cc2` 的 ref gitlink 为 `69353fc9a7d7e4e427211a85cdaa0b9c07a4f01a`；失败 PR SHA `ef96125f05584bf2dce017ce2e2266c10bdee372` 的 ref gitlink 为 `813673a8e84b51265467f02277fc7e739837f61e`，runtime 子模块为 `e9f44e68b0c88981160f5b986f7ffcc9fb5b20a7` / macOS GitHub Actions、Apple Silicon 本地开发机
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #479](https://github.com/MFSGA/Chimera/pull/479)、[macOS CI job 115762372590](https://github.com/MFSGA/Chimera/actions/runs/38536302199/job/115762372590)；日志显示 `residual_process_is_killed_before_spawn` 在第二次 `spawn().unwrap()` 收到 `Io(NotFound)`，旧 SHA 上该集成目标 11 passed、1 failed。
- 预期行为：旧 epoch 的 PID 记录在回收或旧进程退出时消失，应按“无记录”继续启动新 epoch。
- 真实异常、复现步骤与证据：旧实现先调用 `symlink_metadata`，再 `read_to_string`。macOS CI 的第二次启动失败，错误为 OS code 2；日志没有指出产生错误的具体系统调用。进程清理路径可以并发移除同一记录；若删除发生在元数据检查成功后、读取前，`read_to_string` 会把 `NotFound` 传播为启动错误。
- 最可能根因与排除项：检查与读取之间的文件生命周期竞态与 CI 错误及并发清理路径一致，但 CI 日志没有证明 `NotFound` 确实来自该次 `read_to_string`。测试没有确定性地钩住两个文件系统调用之间的间隔，因此不能声称已在本机重现该窄竞态。
- 修复提交、差异映射及保留的兼容性边界：PR #479 在 `backend/chimera-utils/src/process/pid_file.rs::read_epoch_pid_file` 将读取阶段的 `NotFound` 映射为 `Ok(None)`，其他 I/O 错误仍传播。ref `backend/nyanpasu-runtime/crates/nyanpasu-utils/src/process/pid_file.rs::read_epoch_pid_file` 在基线 SHA `e9f44e68b0c88981160f5b986f7ffcc9fb5b20a7` 有相同检查/读取顺序；这里保留最小本地修正，不改变符号链接检查、进程身份验证、kill 权限或记录格式。详见 [DIFF-051](docs/ref-alignment-progress.md#diff-051-修正-epoch-pid-文件检查与读取之间的删除竞态)。
- 原实现失败、新实现通过的回归测试，及测试注册位置：原实现失败证据为上述 macOS `process_pid_file::residual_process_is_killed_before_spawn`；新实现本地同一集成目标 12/12 通过，位置 `backend/chimera-utils/tests/process_pid_file.rs`。新 SHA 的 macOS 远端复验尚待执行。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：`cargo test --locked --manifest-path backend/Cargo.toml -p chimera-utils --features process --test process_pid_file`，12 passed、0 failed、0 ignored；`rustfmt --check backend/chimera-utils/src/process/pid_file.rs` 通过。未运行全量 `cargo fmt --all --check`（会报告既有 vendor/runtime 子模块格式差异），不声称该全量检查通过。
- 风险/回滚和清理证据（如涉及宿主状态）：不修改宿主状态；回滚为还原 `read_epoch_pid_file` 的读取错误分支及本条记录。CI 新 SHA 通过后将状态更新为 Verified。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：无；不改变治理门禁。
