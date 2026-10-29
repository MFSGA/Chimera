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

| 编号                | 日期       | 模块                | 问题与证据                                                                                                                     | 状态      |
| ------------------- | ---------- | ------------------- | ------------------------------------------------------------------------------------------------------------------------------ | --------- |
| LESSON-20261014-001 | 2026-10-14 | Rust / CI           | macOS epoch PID 集成用例第二次 spawn 收到 `NotFound`；PID 文件读取竞态是候选根因，PR #479 远端复验待完成                       | Open      |
| LESSON-20261014-002 | 2026-10-14 | Proxies / E2E       | Windows 桌面 E2E 激活配置后仍显示旧代理组图标；后端缓存未随 profile 刷新事件失效，PR #479 新 SHA 远端复验待完成                | Open      |
| LESSON-20261014-003 | 2026-10-14 | Profile / E2E       | PR #481 的 Linux 定时刷新测试和 Windows 流量菜单 E2E 未等待异步状态收敛；修复待新 SHA 复验                                     | Open      |
| LESSON-20261014-004 | 2026-10-14 | Development         | PR #481 首次提交钩子未使用外接盘 Cargo target，写满主卷；显式传入 target 后 hooks 通过                                         | Mitigated |
| LESSON-20261015-001 | 2026-10-15 | E2E                 | main-first 套件误用仅存在于 main UI 的 `app-root` 作为 legacy 就绪标记；本地隔离 WebKit 运行复现并修正                         | Verified  |
| LESSON-20261015-002 | 2026-10-15 | E2E                 | PR #481 Windows 规则图标用例的路由就绪等待超时，具体条件未定位；目标用例已缩小等待范围，待新 SHA 复验                          | Open      |
| LESSON-20261015-003 | 2026-10-15 | Development         | PR #481 本地验证误用了 PR #482 Cargo target，混入另一分支类型产物；已确认并隔离构建目录                                        | Mitigated |
| LESSON-20261016-001 | 2026-10-16 | Development         | 在未准备好的 worktree 调用 `pnpm exec` 触发依赖安装；已中断，须先核对依赖状态并用已安装工具运行检查                            | Mitigated |
| LESSON-20261016-002 | 2026-10-16 | Frontend / CI       | PR #484 transform-chain 行组件首次遗漏 `isLast` 解构，提交钩子的类型检查在提交前发现并阻止了错误                               | Mitigated |
| LESSON-20261017-001 | 2026-10-17 | E2E / CI            | Windows Edge 154 对 `data:` no-cors fetch 返回 `basic`；兼容 readable `basic` 与标准 `opaque` 响应                             | Verified  |
| LESSON-20261017-002 | 2026-10-17 | Profile / Rust      | `start_producers()` 原先只投递 actor cast；await actor 回复后 Profile 测试 72/72、PR #485 CI/E2E 通过并已合入                  | Verified  |
| LESSON-20261017-003 | 2026-10-17 | E2E                 | Traffic 路由 E2E 修正窗口准备及侧栏状态读取；精确 PR SHA 的 Windows hermetic 和隔离套件通过                                    | Verified  |
| LESSON-20261017-004 | 2026-10-17 | CI / Linux          | 前端-only job 误跑依赖 GTK/GLib 的 Rust 测试；已移入后端门禁，PR #489 的 Ubuntu 测试通过                                       | Verified  |
| LESSON-20261017-005 | 2026-10-17 | E2E / Windows       | PR #484 的平滑滚动在 Windows Edge 154 只到达 49/820px；改用即时定位并在清理时复位视口                                          | Verified  |
| LESSON-20261017-006 | 2026-10-17 | E2E / Windows       | PR #490 Dashboard 右键 E2E 漏发 `pointerdown`；驱动已补合成 PointerEvent，待 Windows 复验                                      | Open      |
| LESSON-20261017-007 | 2026-10-17 | E2E / Windows       | PR #490 的 invoke 属性不可改写且测试插件未获 capability；改为 E2E-only app IPC 故障注入，待 Windows 复验                       | Open      |
| LESSON-20261018-001 | 2026-10-18 | E2E / Windows       | PR #477 E2E 等待现逐轮重新查询元素；精确 SHA 的 Windows E2E 通过                                                               | Verified  |
| LESSON-20261018-002 | 2026-10-18 | E2E / Windows       | PR #490 Dashboard 用例假设中文 UI；当前英文菜单被误判为未打开并再次右键关闭，改用稳定选择器待新 SHA 复验                       | Open      |
| LESSON-20261019-001 | 2026-10-19 | E2E / Windows       | PR #490 主题断言检查错 DOM 节点；主题类位于 `#root` 而非内部 app-root，已改用真实主题根节点待复验                              | Open      |
| LESSON-20261019-002 | 2026-10-19 | E2E / Windows       | PR #490 成功重试后配置弹层关闭；测试先重开弹层再核验控件状态                                                                   | Open      |
| LESSON-20261019-005 | 2026-10-19 | E2E / Localization  | 语言切换需先应用本地 locale，再独立验证 typed 配置、持久化与 legacy 页面；PR #499 的复现已修复，当前分支待验证                 | Mitigated |
| LESSON-20261019-006 | 2026-10-19 | Rust / Windows      | PR #494 取消刷新测试对 `Notify` 的时序未受控；新增确定性屏障覆盖通知先于 waiter 登记的情况，历史 CI 根因未证实                 | Mitigated |
| LESSON-20261019-007 | 2026-10-19 | Git / Delivery      | PR #494 首次 push 使用的 SSH 凭据无仓库写权限；改用活动 GitHub CLI 凭据的 HTTPS push 成功，提交 verified                       | Mitigated |
| LESSON-20261020-001 | 2026-10-20 | E2E / Localization  | PR #501 locale cache 受前置 spec 影响；路由标记需比较每层 outlet 的 child match key，不依赖动画透明度                          | Open      |
| LESSON-20261020-002 | 2026-10-20 | E2E / Dashboard     | PR #501 的 Popover 需等待打开状态与控件；本机 WebKit 路由动画前置偶发超时，Windows 新 SHA 待复验                               | Open      |
| LESSON-20261020-003 | 2026-10-20 | E2E / WebDriver     | PR #507 的 refresh 不等待新文档加载；需验证文档身份变化、再等 React 挂载                                                       | Open      |
| LESSON-20261021-001 | 2026-10-21 | Governance / Review | 历史重写规则修改遗漏审查清单与 PR 模板中的旧禁令；须同步所有规范副本并交叉检查                                                 | Mitigated |
| LESSON-20261021-002 | 2026-10-21 | E2E / WebDriver     | 跨平台焦点/布局快照需对应真实状态；PR #515 新 SHA E2E 复验待完成                                                               | Open      |
| LESSON-20261022-001 | 2026-10-22 | E2E / WebDriver     | `executeAsync` 完成不会自动清除页面定时器；路由轮询须停止并清除所有待执行 timer，PR #515 新 SHA 待复验                         | Open      |
| LESSON-20261023-001 | 2026-10-23 | CI / Linux          | PR #518 ARM64 target image fix; dedicated cross-build run 39623084868 passed                                                   | Verified  |
| LESSON-20261023-002 | 2026-10-23 | CI / Linux          | PR #518 preserves JSON quoting in non-nightly cross builds; dedicated ARM64 run 39623084868 passed                             | Verified  |
| LESSON-20261023-003 | 2026-10-23 | E2E / macOS         | Legacy Connections UI run stopped before assertions because embedded WebKit remained hidden; UI interaction unverified         | Open      |
| LESSON-20261025-001 | 2026-10-26 | CI / Release        | Draft Release visibility and updater signature resolution fixed; v0.27.1 build and manifest regeneration verified              | Verified  |
| LESSON-20261026-001 | 2026-10-26 | CI / Dependencies   | Alpha stable version outran rolling assets; macOS prepare 404 in run 40315139018; v0.27.1 build passed                         | Verified  |
| LESSON-20261026-002 | 2026-10-26 | Updater / Alpha     | Alpha updater asset-version regression; source fix present but Rust assertions still need a recorded run                       | Open      |
| LESSON-20261026-003 | 2026-10-26 | Updater / Release   | Draft asset URLs contained temporary `untagged-*` paths; stable tag URLs and downloads verified after regeneration             | Verified  |
| LESSON-20261026-007 | 2026-10-26 | E2E / Dashboard     | Resize E2E hid a JSON key-order assertion defect behind cleanup; capture pre-cleanup evidence and compare objects structurally | Mitigated |
| LESSON-20261027-001 | 2026-10-27 | E2E / Dashboard     | Resize E2E requested 680px but kept an 8-column grid; assert the measured four-column grid interval                            | Open      |
| LESSON-20261027-002 | 2026-10-27 | Rust / concurrency  | Agent proxy rollback and stale-port repair must serialize with newer user settings                                             | Verified  |
| LESSON-20261028-001 | 2026-10-28 | CI / Linux          | Ubuntu runner image dropped `webkit2gtk-driver`; choose the available WebKit driver package and verify against fresh CI        | Mitigated |

| LESSON-20261029-001 | 2026-10-29 | Shared KV / IPC | ref 在事件监听注册失败后仍开放读取；Chimera 增加重试与就绪门控，并用 23 项前端测试覆盖 | Mitigated |
| LESSON-20261029-002 | 2026-10-29 | E2E / Storage | PR #548 新 IPC 返回 value/revision；Dashboard 用例仍按旧字符串合同读写，Windows hermetic 失败 | Mitigated |

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

### LESSON-20261027-003：非视觉 smoke 不应等待隐藏 WebKit 窗口的路由动画

- 日期：2026-10-27
- 状态：Mitigated（CSP smoke 已单独通过；默认 smoke 仍有其他两个 macOS WebKit 用例失败）
- 模块：桌面 E2E
- 证据：macOS WebKit 的 `smoke.e2e.ts` 在访问 main 窗口 fetch policy 前等待路由动画；诊断显示文档为 hidden，动画 `currentTime` 持续为 0，超时 15 秒。该契约只检查两个窗口中的 fetch 能力，不检查布局或动画。
- 修复：`openMainRoute` 允许调用者明确跳过路由动画等待；仅非视觉 fetch smoke 使用此选项，主 UI 布局测试保留默认动画等待。
- 回归测试：`pnpm --filter @chimera/tauri-e2e typecheck`；`CHIMERA_E2E_ARTIFACT_DIR=/tmp/chimera-e2e-artifacts pnpm --filter @chimera/tauri-e2e exec wdio run ./wdio.conf.ts --spec ./specs/smoke.e2e.ts`，1 个 spec 中 5/5 用例通过。
- 验证限制：完整 `pnpm --filter @chimera/tauri-e2e test:smoke` 仍为 2 个 spec 通过、2 个失败；proxy localization 的新增配置按钮 30 秒不可点击，legacy connections 页面在 hidden 文档下动画停在 0。它们尚未在隔离的可见 macOS runner 上复现；不能把整个 smoke suite 报告为通过。
- 后续：在允许真实桌面窗口前台运行的 macOS runner 上重跑完整 smoke；只有当用例不验证视觉布局/动画时，才可跳过动画稳定等待。

### LESSON-20261028-001：Ubuntu runner 更新后应选择可用的 WebKit 驱动包

- 日期与时区：2026-10-28 America/Los_Angeles
- 状态：Mitigated（工作流已改为按 apt 候选包选择驱动；新提交的 Ubuntu CI 尚待验证）
- 严重度：Medium（Linux CI 在依赖安装阶段失败，阻止整轮 CI）
- 模块：GitHub Actions / Linux Tauri dependencies
- 关联证据：[PR #540 CI run 40632079560](https://github.com/MFSGA/Chimera/actions/runs/40632079560)，失败 job `122631703884`；同一 SHA 的 [Windows Desktop E2E run 40632079689](https://github.com/MFSGA/Chimera/actions/runs/40632079689) 通过。
- 预期行为：Ubuntu runner 镜像更新后仍安装当前发行版提供的 WebKit WebDriver，并继续执行 Linux CI。
- 确认根因：GitHub Ubuntu runner 已更新到 Ubuntu 26.04，apt 源不再提供 `webkit2gtk-driver`；依赖安装在运行源码检查之前以 package-not-found 失败。Ubuntu 24.04 仍使用旧包名，不能对所有 Ubuntu runner 固定采用新名字。
- 修复与回归覆盖：`.github/workflows/ci.yaml` 先读取 `apt-cache policy webkit2gtk-driver` 的候选版本；有候选时安装旧包，无候选时安装 `webkitgtk-webdriver`。本地从实际 YAML 提取两个安装脚本并通过 ShellCheck、`bash -n`，用“旧包有候选”“候选为 none”“旧包不存在”三类 apt 元数据模拟确认选出的包名及安装参数；actionlint（关闭仓库中既有 shellcheck 集成告警）、Prettier 和 `git diff --check` 通过。该 CI 门禁仍需在推送后的 Ubuntu runner 上完成端到端验证。
- 风险与回滚：只影响 Linux CI 测试依赖选择，不改变发布或产品二进制；若 Ubuntu 仓库更改包名，CI 依赖安装仍会明确失败并由最新 runner 输出诊断。回滚为恢复原固定包名，但不适用于已升级的 runner。
- 后续：以更新提交 SHA 核对 Linux CI；通过后将本条状态更新为 Verified。

### LESSON-20261027-002：代理动作回滚与 stale-proxy 修复要串行化

- 日期：2026-10-27
- 状态：Verified（本地定向及全量 Rust lib 测试通过；远程 CI 尚待本批提交后验证）
- 模块：系统代理 / Agent / 生命周期
- 证据：独立复核发现 stale 修复遇到偏好 CAS 冲突时可能恢复过期 OS 配置；Agent 回滚在保存旧偏好与恢复 OS 之间可能被新用户偏好穿插；mixed-port 更新在 stale 修复期间也未受同一互斥保护。后续复核发现随机端口把 fallback 误当作运行端口、stale 修复失败回滚可能覆盖外部宿主改动、Agent 在 Android/iOS 引用被 cfg 排除的类型，以及 effects 将 `ClashInfo::port`（controller）误当成 mixed listener。最新复核发现核心在 Agent 快照校验后停止时，启用代理动作仍可能仅凭最后一次 receipt 端口报告成功；AllowFallback 也可能选择不同于 start port 的备用端口。
- 修复：typed/legacy 代理偏好、mixed-port 更新和 Agent 设置/验证/回滚共用 client 级互斥；legacy `patch_clash_core_config` 经 `feat::patch_clash` 到达带锁的 client mapping writer；stale 生命周期在 OS 写入前复核有效 mixed-port；effects 只使用与当前 host 状态匹配的 `RuntimeApplyReceipt.ports.bindings.mixed_port`。Agent 启用要求活动 runtime receipt，effect 后和最终快照都确认核心仍运行；Agent 和停止后 stale 修复优先使用当前 host 最近一次 receipt。Random/AllowFallback 策略无 receipt 时端口不可知，Agent 不提供依赖该端口的开关或 stale 修复；不使用可能在启动时被覆盖的 Verge 镜像猜测旧运行端口。只有 Fixed 策略缺少 receipt 时才以配置端口作为确定值。stale 回滚重新读取 OS，仅在代理仍等于本次禁用写入时恢复，并在状态未知/已变化时清除旧所有权；移动平台不暴露不支持的 Agent 系统代理提案和执行。CAS 冲突时发布最新偏好，不恢复过期 OS 状态。普通设置与 stale 修复共用 legacy Verge 镜像持久化入口。
- 回归测试：五个 `client::core_lifecycle::tests::stale_proxy_lifecycle_*` 用例覆盖核心状态/端口拒绝、成功提交与镜像持久化、镜像失败、偏好版本冲突不重新启用旧代理；`client::tests::system_proxy_mutation_guard_serializes_competing_transactions` 验证同一 client 的代理事务互斥；raw mapping guard 判定测试、`client::ports::tests::effective_random_mixed_port_prefers_the_confirmed_runtime_binding`（含 Random、AllowFallback 无回执和 Fixed fallback）、`client::application::tests::effects_receive_the_confirmed_mixed_port_not_the_controller_port`、`features::agent::actions::tests::new_action_verification_requires_desired_and_observed_state` 和 `client::system_proxy::tests::stale_proxy_rollback_does_not_overwrite_a_concurrent_host_change` 覆盖旧 IPC 锁入口、runtime receipt、controller/mixed 区分、未知端口拒绝、核心停止后的最终验证与宿主并发修改。
- 验证：`cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib -- --test-threads=1` 为 659 passed、0 failed、1 ignored。测试使用 fake OS port，不证明真实桌面代理设置行为；本机未运行生产 adapter 或代理/TUN/Service 宿主副作用 E2E。Android target 下载停滞后中止，移动 cfg 尚未交叉编译验证。当前最终 SHA 的远程 CI 待本批提交后验证。
- 覆盖缺口：单测确认最终快照在核心停止时拒绝将启用代理报告为成功；没有确定性动作级测试在同一次执行中断言核心停止后 OS 代理和偏好均已恢复。
- 后续：远程 CI 需在本批提交 SHA 上验证。若移除 CAS/互斥或改变 legacy 镜像所有权，应重审这些冲突与持久化断言。

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

### LESSON-20261014-002：profile 刷新事件未使后端代理快照缓存失效

- 日期与时区：2026-10-14 UTC
- 状态：Open（本地已修复，等待 PR #479 新 SHA 的 Windows Desktop E2E 复验）
- 严重度：Low
- 模块：Proxies / IPC / E2E
- master commit / ref gitlink SHA / 平台与测试环境：PR #479 旧 SHA `2188073f4ee0f749777e3a8c21e1c7aef54520c3`，ref gitlink SHA `813673a8e84b51265467f02277fc7e739837f61e`；GitHub Actions Windows Desktop E2E；本机 macOS WebKit 定向复跑
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #479 Desktop E2E run](https://github.com/MFSGA/Chimera/actions/runs/38544336107)，52 项中 1 项失败；归档截图显示 Rules 页的新 profile 导航已显示，但 proxy-group icon 使用无图标 fallback。
- 预期行为：profile 激活后，已挂载的 main UI 及时取得新运行时代理组快照并展示对应图标。
- 真实异常、复现步骤与证据：`rules-main-proxy-icon.e2e.ts` 先打开 Rules 页面、再激活含原始 SVG 图标的本地 profile。Windows CI 在 30 秒等待后仍观察到该 group 存在但 `<img>` 缺失；直接 runtime 查询已确认新 profile 的代理组已激活。相同定向用例在 macOS 上通过，因此此处保留 Windows 失败为回归证据，不声称本机可稳定复现。
- 确认的根因与排除项：Chimera profile 事件会使前端代理 query 失效，但 `get_proxies` 可能在 3 秒 TTL 内从 `ProxiesGuard` 返回旧快照。该 IPC 读取路径刷新缓存时不广播新的 proxies 事件，因此该次旧 query 没有后续 UI 刷新信号。另有跨越失效点的旧快照请求可能在稍后写回缓存；generation 栅栏现会拒绝这类响应。检查截图和事件/IPC 实现后确认该竞态路径；不是 SVG 图片尺寸或测试选择器问题。
- 修复提交、差异映射及保留的兼容性边界：Chimera 在 `ChimeraClient::refresh_profiles` 中递增 `ProxiesGuard` generation、标记缓存过期，再发 profile mutation；写回前核对 generation，IPC 查询在检测到并发失效时重试。后台刷新在 API 解析前捕获 generation；代理选择在 API 获取后、HTTP 选择前检查 generation，并将同一 token 用于后续缓存写回。profile 在 API 获取期间变化时选择会被拒绝；若 profile 在 HTTP 选择请求进行中切换，该请求仍可能作用于正在退出的 runtime，但缓存刷新会检测失效并返回错误，IPC 不会报告成功或继续发代理事件/连接中断处理。Chimera 尚未具备 ref actor 的实例级撤销。手动 profile mutation 和无当前配置时的自动激活完成路径都调用该函数。详见 [DIFF-053](docs/ref-alignment-progress.md#diff-053-profile-变更后使代理快照缓存失效)。
- 原实现失败、新实现通过的回归测试，及测试注册位置：原实现失败为上述 Windows 桌面用例；新增 `ProxiesGuard::invalidate` 单元用例验证缓存过期且保留最后快照。该桌面用例由现有 `tauri-e2e` 套件注册；新 SHA 的 Windows 复验待完成。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：初始 generation 修复的定向 Rust 测试为 3 passed；独立复审要求补上
  选择请求期间发生 profile 切换时的错误返回后，第一次重跑因工作树构建缓存占满主机磁盘，在链接前失败。清理该工作树的 Cargo
  生成产物后，以 `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --locked --manifest-path
backend/Cargo.toml -p chimera --lib core::clash::proxies::tests:: -- --nocapture` 重跑，最终代码 4 passed、0 failed。
  `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo clippy --locked --manifest-path backend/Cargo.toml -p chimera --all-targets
--all-features`、`cargo fmt --manifest-path backend/Cargo.toml -p chimera -- --check`、Prettier、`git diff --check`、
  `pnpm test:manifest`（8 passed）和 Deno check 通过。最新 e2e 构建上的本机
  macOS WebKit 定向 WDIO 用例首轮在主页面路由动画稳定检查失败（未进入 profile/代理断言），第二次独立运行 1/1 通过；
  旧实现的本机用例也为 1/1 通过。记录该首轮失败，重试通过不等于干净首轮通过。Windows Desktop E2E 仍需新 SHA 远端复验；
  未声称 macOS 结果能代替 Windows。全工作区 typecheck 因隔离工作树缺少生成的 Paraglide 文件而失败。
- 风险/回滚和清理证据（如涉及宿主状态）：L2，共享 profile/proxy 刷新时序。回滚为恢复 profile call sites 直接触发事件并移除缓存失效函数和对应测试；不触及持久化数据、宿主网络、TUN 或 Service 状态。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：现有 E2E 即为跨平台防复发用例；无治理门禁调整。

### LESSON-20261014-003：异步 Profile 刷新与流量菜单测试必须等待状态收敛

- 日期与时区：2026-10-14 UTC
- 状态：Open（测试修复待 PR #481 新 SHA 的 CI/E2E 复验）
- 严重度：Low
- 模块：Profile / E2E / CI
- PR head commit / ref gitlink SHA / 平台与测试环境：PR #481 初始 head `b9329eee6e6a20d64b960800c8a6cefa81439836`；ref `e81eeaea46a9c229478cd29c9555b5c072f7e6cc`；GitHub Actions Ubuntu Rust tests 与 Windows Edge hermetic suite
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #481](https://github.com/MFSGA/Chimera/pull/481)，[CI run 38592275892](https://github.com/MFSGA/Chimera/actions/runs/38592275892)，[Tauri E2E run 38592275993](https://github.com/MFSGA/Chimera/actions/runs/38592275993)。Ubuntu 在 551 passed 后，`client::profiles::tests::import_suggestion_is_committed_and_reschedules_the_timer` 因 `fetch_count >= 2` 超时失败；Windows 52/53 specs 通过，`traffic-analysis-main.e2e.ts` 未观察到 Connections 菜单项，失败截图中的 metric trigger 仍为 `aria-expanded=false`。Profile transform-chain 用例在该 E2E run 中通过。
- 预期行为：计划刷新到期后，测试等待 fetch 与提交后的 Profile snapshot；选择流量 scope 后等待控制状态更新，再打开 metric 菜单并选择指标。
- 真实异常、复现步骤与证据：Linux 测试以 200 次 `yield_now` 轮询 actor snapshot，CI 失败时仍未观察到第二次 fetch。Windows 测试只等待 URL 中的 `scope=closed`，随即点击 metric trigger 并等待 Connections 项；归档 HTML/截图显示测试结束时 scope 已选中、菜单已关闭。失败是测试证据，不代表 Profile scheduler 或流量菜单产品行为已被证明损坏。
- 确认的根因与排除项：Rust 用例将调度 actor 与异步提交误当成可在固定次数的协作让出内完成；计数是调度任务的异步副作用，有限 `yield_now` 不提供完成保证。Windows 用例没有等待 scope 控件反映 URL 状态，也没有验证点击后菜单确实打开，故失败位置不能区分 UI 事件未触发与断言时序。新测试改为等待 fetch `Notify`、提交后的 authoritative snapshot、scope `aria-checked` 和菜单 `aria-expanded`；仍须以新 CI 结果确认这组边界足以稳定通过。
- 修复提交、差异映射及保留的兼容性边界：Rust 测试用 `Notify` 等待第二次真实 fetch，并以有界等待观察已提交的 subscription；ref `backend/tauri/src/client/profiles.rs::wait_for_source` 与 `jobs::settled` 提供类似完成同步，Chimera scheduler 没有 jobs registry，因此本测试用 fetch 信号和 snapshot 适配，不改产品调度器。流量 E2E 在打开菜单前等待 scope 和 Radix `aria-expanded` 状态；不改变流量 UI 或查询行为。
- 原实现失败、新实现通过的回归测试，及测试注册位置：原实现失败证据为上述 Ubuntu 与 Windows runs。新测试修复已在 `backend/tauri/src/client/profiles.rs` 和 `tauri-e2e/specs/traffic-analysis-main.e2e.ts`；待新 SHA 运行 `pnpm test:backend` 与 hermetic E2E 后才能标记修复通过。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：`cargo fmt --manifest-path backend/Cargo.toml -p chimera -- --check`、`pnpm --filter @chimera/tauri-e2e typecheck`、`cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib --all-features client::profiles::tests::import_suggestion_is_committed_and_reschedules_the_timer -- --exact`、Prettier 和 `git diff --check` 通过；针对性 Rust 单测 1/1 通过。Mac 桌面 E2E 构建成功，但 `traffic-analysis-main.e2e.ts` 在 `beforeEach` 的既有 `waitForMainRouteSettled` 等待中超时（0/1 specs 到达目标交互断言），因此本机未验证本次修改的菜单步骤；Windows Edge hermetic CI 是该步骤的权威验证。外接盘独立 target 避免了主卷空间限制。
- 风险/回滚和清理证据（如涉及宿主状态）：测试可靠性修正，不改变运行时或持久化行为；回滚为还原上述两个测试文件。本次只下载并检查了 PR E2E 诊断工件，未运行宿主网络或 Service/TUN 操作。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：新断言在 CI 通过后作为防复发检查；无治理门禁调整。

### LESSON-20261014-004：提交钩子必须继承隔离的 Cargo target 路径

- 日期与时区：2026-10-14 UTC
- 状态：Mitigated（本地提交 hook 已在外接盘 target 上通过；后续大仓库提交沿用该环境）
- 严重度：Low
- 模块：Development / CI / Rust
- PR head commit / 平台与测试环境：PR #481 `51aa4b2d733733ecdd8a89e00f451dfa1e7d9209`；Apple Silicon macOS，隔离 worktree
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #481](https://github.com/MFSGA/Chimera/pull/481)；首次 `git commit` 的 `lint-staged` 在 `pnpm lint:clippy` 子步骤因 `No space left on device` 失败。失败前主卷剩余 116 MiB；清理该 worktree `backend/target` 后释放 701.5 MiB。
- 预期行为：在本机磁盘紧张时，commit hook 的 Clippy 构建应复用调用者指定的外接盘 Cargo target，不在默认 worktree target 重建完整依赖。
- 真实异常、复现步骤与证据：先前 Rust/E2E 构建以 `CARGO_TARGET_DIR=/Volumes/ChimeraBuild/codex-pr481-ci-fix.CC4isH` 执行；首次 `git commit` 未在该进程环境中传入此变量。lint-staged 因而创建 `backend/target`，空间耗尽后 Clang 和 rustc 报错。Git 钩子留下自动备份 stash，但已暂存的四个目标文件仍完整；专用 target 清理后主卷空间恢复。
- 确认的根因与排除项：`CARGO_TARGET_DIR=... cargo test` 只为那次 Cargo 子进程设置环境，变量没有自动成为交互 shell 的导出变量，所以后续独立的 `git commit` 钩子使用 Cargo 默认路径。证据是失败产物位于该 worktree 的 `backend/target`，清理后释放 701.5 MiB；未发现产品代码或 CI 构建失败。
- 修复提交、差异映射及保留的兼容性边界：失败后使用 `cargo clean --manifest-path backend/Cargo.toml` 清理本次专用 worktree target，再以 `CARGO_TARGET_DIR=/Volumes/ChimeraBuild/codex-pr481-ci-fix.CC4isH CARGO_INCREMENTAL=0 AWS_LC_SYS_PREBUILT_NASM=1 git commit ...` 重跑 hooks。代码未修改 hook 或其门禁，也未改动主工作区 target。
- 原实现失败、新实现通过的回归测试，及测试注册位置：没有新增产品回归测试；hook 的 `pnpm lint:clippy` 在修正环境变量后实际执行并通过。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：首次提交 hook 失败；第二次提交 hook 的 oxlint、Prettier、`pnpm typecheck:e2e`、rustfmt、`pnpm lint:clippy` 全部通过。验证前后分别观察到主卷仅剩 116 MiB，以及清理后 792 MiB；未清理主工作区或其大型 Cargo target。
- 风险/回滚和清理证据（如涉及宿主状态）：清理只针对本任务 worktree 中由失败 hook 新建的 Cargo 构建目录，`cargo clean` 报告移除 3614 文件 / 701.5 MiB；无宿主网络、Service 或 TUN 操作。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：执行 Rust 构建后若 hook 也需编译，`git commit` 命令必须显式继承 `CARGO_TARGET_DIR`；无治理门禁调整。

### LESSON-20261015-001：跨窗口 E2E 就绪检查必须使用窗口对应的 UI 标记

- 日期与时区：2026-10-15 UTC
- 状态：Verified
- 严重度：Low
- 模块：E2E / main UI / legacy UI
- master commit / ref gitlink SHA / 平台与测试环境：master 基线 `c7d21d47a374009161f271c4512722c5c5cd8eae`、PR #482 测试前 head `24ea62a3f42458c31d0860b172396e34340050d3`、ref `c08aa9d9a17953aa2464cdae61390d5c3f9afbc7`；macOS Apple Silicon、WebKit 605.1.15，使用隔离 E2E runtime 和本地专用 app ID。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #482](https://github.com/MFSGA/Chimera/pull/482)；本地失败信息为 `The legacy Profile window did not render after navigation.`，运行产物位于本次任务的外接盘临时目录。
- 预期行为：main-first 套件在 main UI 验证完成后创建 legacy 窗口；测试须确认 legacy 前端已挂载，再打开 Profile 列表并验证共享 Profile。
- 真实异常、复现步骤与证据：首次添加就绪等待时检查 `[data-slot="app-root"]`。该标记只属于 main UI；legacy 页面有自己的根节点与 `data-testid="sidebar-route-profiles"` 导航项。main-first 本地运行在进入 legacy Profile 断言前超时，确认等待条件引用了错误窗口的 DOM 合同。
- 确认的根因与排除项：根因为 E2E 等待器误用 main-only 标记，不是 Profile 后端状态问题。首次尝试也遇到内部磁盘空间不足；将本次生成的构建缓存及临时目录放到外接盘后，排除了该环境限制。
- 修复提交、差异映射及保留的兼容性边界：PR #482 在 `tauri-e2e/specs/profile-auto-update.e2e.ts` 改为等待 legacy 专属侧栏项挂载后再导航；`tauri-e2e/README.md` 说明隔离 main-first 套件与默认 legacy 窗口生命周期的区别。
- 原实现失败、新实现通过的回归测试，及测试注册位置：原实现为上述 main-first 本地 E2E 超时；修复后 `profile-auto-update-main` 在同一主机、独立 app ID 和外接盘 runtime 下通过 1/1。套件由 `isolatedE2eSuites` 注册，并由 `spec-suites.test.ts` 检查只运行一次。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：`pnpm --filter @chimera/tauri-e2e exec wdio run ./wdio.conf.ts --suite profile-auto-update-main --logLevel error`，1/1 通过；`pnpm --filter @chimera/tauri-e2e typecheck` 和 25 项 harness unit tests 通过。更新后的 Windows CI 仍须确认平台行为。
- 风险/回滚和清理证据（如涉及宿主状态）：用例只使用隔离 runtime 与 loopback 服务，并注册 Profile/窗口清理；没有启用系统代理或 TUN。回滚时可恢复上一版测试与 suite 配置。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：main 与 legacy 使用不同 DOM 标记；新增跨窗口用例前须从目标页面源码确认挂载信号。没有调整门禁。

### LESSON-20261015-002：路由就绪等待应限定到当前用例需要的出口

- 日期与时区：2026-10-15 UTC
- 状态：Open（PR #481 的 Windows 新 SHA 复验待完成）
- 严重度：Low
- 模块：E2E / main UI
- master commit / ref gitlink SHA / 平台与测试环境：master `c7d21d47a374009161f271c4512722c5c5cd8eae`；PR #481 旧 head `4431e7e641626c73c44ded27a8f56220d05a3d62`；ref `e81eeaea46a9c229478cd29c9555b5c072f7e6cc`；GitHub Actions Windows Edge hermetic E2E，另有本机 macOS WebKit 定向运行。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #481](https://github.com/MFSGA/Chimera/pull/481)，[Windows Desktop E2E run 38668811814](https://github.com/MFSGA/Chimera/actions/runs/38668811814)。该 run 的 53 个 specs 中 52 通过；`rules-main-proxy-icon.e2e.ts` 的 `before` 在 `openMainRoute('/main/rules')` 等待 15 秒后报 `The Chimera main route animation did not settle.`
- 预期行为：路由进入后按当前用例需要确认页面准备完成；目标业务结果另行等待并断言。
- 真实异常、复现步骤与证据：规则图标用例在 profile 创建、激活和图标断言前失败。它调用的共享 helper 会等待主路由透明度，并要求 settings、providers、profiles、proxies 等所有当前存在的嵌套 outlet 都恰有一个子节点。CI 日志只证明整体等待超时，没有指出哪个条件未满足；因此尚不能确认嵌套路由是失败原因，也不据此认定 Rules UI 或 profile 图标行为异常。
- 确认的根因与排除项：根因尚未确认；主路由透明度条件、嵌套 outlet 条件及 Windows 时序都不能从现有日志排除。由于该规则图标用例的契约不依赖其他嵌套路由，修复仅让它跳过这些额外 outlet 等待，同时保留其后对 runtime proxy group 与已加载 SVG 图标的明确等待。本机定向 WebKit 运行达到 1/1。该诊断运行使用当前 profile-sync E2E 二进制；Rules 页面源文件与 PR #481 相同，但导航/button/route tree 有分支差异，Windows 新 SHA 仍是决定性复验。
- 修复提交、差异映射及保留的兼容性边界：PR #481 的 `tauri-e2e/specs/main-window.ts::openMainRoute` 增加仅供特定测试选择的 `waitForNestedOutlets` 选项，默认行为不变；`rules-main-proxy-icon.e2e.ts` 关闭无关嵌套出口等待，随后继续执行原有 runtime 与 SVG 加载断言。
- 原实现失败、新实现通过的回归测试，及测试注册位置：原实现失败为上述 Windows run；修改后的 `rules-main-proxy-icon.e2e.ts` 已注册在 `main` / `hermetic`。本机定向命令 `pnpm --filter @chimera/tauri-e2e exec wdio run ./wdio.conf.ts --spec ./specs/rules-main-proxy-icon.e2e.ts --logLevel error` 结果 1/1 通过。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：E2E harness TypeScript 检查通过，注册的 harness 单测 25/25 通过，Prettier 和 `git diff --check` 通过；本机 WebKit 定向规则图标用例 1/1 通过。新 SHA 的 Windows Edge hermetic run 尚未执行；不将 Mac WebKit 结果当成 Windows 复验。
- 风险/回滚和清理证据（如涉及宿主状态）：仅测试 helper 与单个用例，未改变应用 runtime 或宿主网络；回滚为恢复原先无参数的 `openMainRoute` 调用。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：LESSON-20261014-003 也记录过主路由动画等待失败；后续将结合 Windows 新 SHA 结果判断应否进一步调整共享等待条件。当前约束是每个测试只等待其契约所需的 outlet，业务结果使用独立权威断言。无门禁变化。

### LESSON-20261015-003：Cargo 构建产物必须按 worktree 隔离

- 日期与时区：2026-10-15 UTC
- 状态：Mitigated（交付记录后在 PR #482 自己的 worktree 重建其 target）
- 严重度：Low
- 模块：Development / Rust / E2E
- master commit / ref gitlink SHA / 平台与测试环境：PR #481 head `4431e7e641626c73c44ded27a8f56220d05a3d62` 与 PR #482 worktree `/Volumes/ChimeraBuild/chimera-profile-sync`；Apple Silicon macOS。
- 关联 Issue / PR / CI / 运行日志（脱敏）：PR #481、PR #482；本地尝试精确构建 PR #481 的 E2E binary 时失败，类型错误涉及 `RemoteSyncInterval` 与 `u64`。
- 预期行为：每个分支/worktree 使用仅由该 worktree 构建的 Cargo target，避免依赖产物与另一 checkout 的源码/API 不一致。
- 真实异常、复现步骤与证据：在 PR #481 worktree 运行 `pnpm --filter=chimera-ui build` 后，以 `CARGO_TARGET_DIR=/Volumes/ChimeraBuild/chimera-profile-sync/backend/target/e2e` 编译 PR #481。其 `chimera-config` 源码使用 `u64`；但该 target 中 `chimera_config` 的 dep-info 指向 PR #482 worktree，归档 rlib 还包含 `RemoteSyncInterval` 符号，随后 Tauri 编译产生 4 个与该 API 不匹配的错误。
- 确认的根因与排除项：把 PR #482 的 target 目录复用于另一 worktree，造成 Cargo 复用了来自不同源码树的 workspace crate 产物。该失败不是 PR #481 Windows CI 日志中的规则图标超时，也不是 source tree 修改；此前 PR #481 的远端 Build Tauri 在旧 SHA 通过。
- 修复提交、差异映射及保留的兼容性边界：没有产品代码修复。后续为 PR #481 使用独立 target；已经受污染的 PR #482 target 仅在其原 worktree 内重建恢复。cargo target 不再跨 worktree 共享。
- 原实现失败、新实现通过的回归测试，及测试注册位置：这是验证环境的过程错误，没有新增产品测试；在各自 worktree 用独立 target 成功构建后才可宣告该构建通过。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：首次跨 worktree 构建失败（E0308/E0369/E0277）；PR #481 目标用例在单独已有 E2E binary 上通过 1/1，但这不能作为 PR #481 本地 build 成功证据。PR #482 自己的 target 后续在原 worktree 恢复构建成功，二进制 SHA-256 `dd0c94f945119e9ce5e72839585deaf96349f36fa155f4a0200bdb082383d8ad`；随后该分支的 profile suite 在此二进制上通过 1/1。隔离构建目录并分别验证两项已完成；该过程错误没有产品测试覆盖。
- 风险/回滚和清理证据（如涉及宿主状态）：没有清理或删除任何 Cargo target；只产生/复用了 Cargo 编译产物，主源码未改。以后需要的 target 放在各自的 worktree 子目录或独立外接盘目录。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：LESSON-20261014-004 涉及 hook 未继承 CARGO_TARGET_DIR；本次另外确认 target 本身也不能跨 worktree 共用。无 CI 门禁变化。

### LESSON-20261016-001：未准备的 worktree 中 pnpm 命令会触发依赖安装

- 日期与时区：2026-10-16 UTC
- 状态：Mitigated（依赖校验/安装已完成；无跟踪文件改动，后续直接使用预装工具完成检查）
- 严重度：Low
- 模块：Development / E2E / CI
- master commit / ref gitlink SHA / 平台与测试环境：PR #486 head `8d6a6dfb9e2d0ec19bde94f8dd77a2eeb6e9c78d`；ref gitlink `685042aaedaea26a591900be0b494dfe8efd79b0`；Apple Silicon macOS。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #486](https://github.com/MFSGA/Chimera/pull/486)，本机检查期间 `pnpm exec prettier` 输出自动依赖验证与安装进度；此前同日一次 `pnpm --filter @chimera/tauri-e2e exec wdio` 也触发依赖安装，并在 `Chimera_Client/target/debug` 启动 Rust 编译。
- 预期行为：运行检查时只调用选定的检查工具，不启动隐式安装或与检查无关的 Rust 构建。
- 真实异常、复现步骤与证据：在新 E2E worktree 中并行启动 `pnpm --filter @chimera/tauri-e2e typecheck` 与 `pnpm exec prettier --check ...`。后者输出 lockfile verification、依赖解析与 package cloning 进度；两条命令立即以 Ctrl-C 中断。此前的 WDIO exec 安装/编译也在约 84 秒后中断。当前 worktree 内 `.pnpm` 虚拟 store 显示约 1.8 GiB 逻辑内容；系统可用空间从约 1.2 GiB 降到 1.1 GiB。中止后未发现 pnpm、cargo 或 rustc 进程，也没有新增跟踪文件。
- 确认的根因与排除项：调用 pnpm 的 exec/run 入口时，当前环境因该 worktree 依赖链接未准备完成而开始自动依赖验证和安装；并行 pnpm 命令共用这一准备过程。该现象是本地验证流程失误，不是产品失败或 CI 失败。
- 修复提交、差异映射及保留的兼容性边界：没有产品代码改动。本次改用预装的 Prettier 和 TypeScript 二进制，并在 `node_modules` 中临时链接主 checkout 的准备依赖完成检查，随后移除这些临时链接；没有清理无法区分归属的 `.pnpm` 虚拟 store。
- 原实现失败、新实现通过的回归测试，及测试注册位置：这是本地执行流程错误，不适用产品回归测试；不能通过重试 pnpm 命令来证明该流程安全。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：中断后确认没有本次依赖安装或构建进程；直接 TypeScript harness 检查通过、unit tests 25/25、Prettier 与 `git diff --check` 通过。没有本机构建最新 E2E binary；该分支的 Windows CI 才能验证当前 app/test 组合。
- 风险/回滚和清理证据（如涉及宿主状态）：未修改跟踪源码以外的配置；测试 runtime/artifacts 使用仓库 `cleanupRuntimeDirectory` 安全删除。依赖虚拟 store 保留在隔离 worktree 中，系统空间仍约 1.1 GiB 可用；未触及其他 worktree 或用户 app。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：此前另一 worktree 的 WDIO exec 已出现相同安装副作用，故记录为重复流程错误。开始运行 pnpm 检查前须确认工作树依赖已准备；否则从同一 checkout 的现成二进制运行单项检查，避免 `pnpm exec` 和并行安装。没有修改 CI 或治理门禁。
- 复发记录（2026-10-23）：在未完成依赖准备的 PR worktree 中运行 `pnpm exec prettier --check LESSONS.md`，pnpm 执行 lockfile 校验并安装/替换依赖（显示 `+30 -50`）；跟踪文件没有变化。随后改用主 checkout 预装的 Prettier 二进制，格式检查通过。

### LESSON-20261016-002：行边界状态必须在渲染组件中完整解构

- 日期与时区：2026-10-16 UTC
- 状态：Mitigated（提交钩子已拦截并修复；PR #484 的新 SHA 桌面 E2E 仍待验证）
- 严重度：Low
- 模块：Frontend / CI
- master commit / ref gitlink SHA / 平台与测试环境：PR #484 基线 master `6b5fba41c867ab6c1e39f501f78aa037db96e320`、ref gitlink `e81eeaea46a9c229478cd29c9555b5c072f7e6cc`；macOS arm64 本地开发环境；上游 UI 源码读取自 ref `1a0372280a405a824353ac2fa6a5f48a3bca4e18`。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #484](https://github.com/MFSGA/Chimera/pull/484)；本地首次提交尝试的 git hook `pnpm typecheck:chimera` 输出报告 `transform-chain-column.tsx` 中找不到名称 `isLast`。该失败发生在提交创建前，因此没有错误版本的 commit SHA；修复后的实现提交为 `88ea658a3954ffd3df73b94168d4c798e1d46154`。
- 预期行为：transform-chain 最后一行的 Move down 操作禁用，其他行按行索引计算边界；每个行组件都必须接收并解构用于该边界的 `isLast` 属性，且提交前类型检查应阻止未定义标识符进入提交。
- 真实异常、复现步骤与证据：首次提交尝试的工作区中，`ItemContent` 渲染逻辑引用了 `isLast`；提交钩子的 `pnpm typecheck:chimera` 报告该标识符未定义并拒绝提交。失败现场没有保存，因此不记录当时参数解构的其他字段。修复代码将 `isLast` 纳入组件参数解构，类型检查通过。
- 确认的根因与排除项：`ItemContent` 使用的行尾状态未在该组件作用域中声明。不是 ref 的行为差异，也不是生成文件或运行时状态造成的错误。
- 修复提交、差异映射及保留的兼容性边界：PR #484 的 `frontend/chimera/src/pages/(main)/main/profiles/$type/detail/_modules/transform-chain-column.tsx` 将 `isLast` 加入 `ItemContent` 解构，保持 ref 按行计算首尾状态的分解，并禁用最后一行的无效下移操作。修复包含在提交 `88ea658a3954ffd3df73b94168d4c798e1d46154` 中。
- 原实现失败、新实现通过的回归测试，及测试注册位置：原实现被提交钩子 `pnpm typecheck:chimera` 拒绝；修复后的 `pnpm typecheck` 与生产构建均通过。边界行为也由 `tauri-e2e/specs/profile-transform-chain-ui.e2e.ts` 的重排断言覆盖，该 spec 属于 `profiles`、`critical` 和 `hermetic`。新 SHA 的 Windows Edge 桌面结果待 PR #484 CI。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：修复后 `pnpm typecheck`、`pnpm --filter=chimera-ui build`、E2E harness typecheck、harness 单测（25/25）、`pnpm lint:frontend-boundaries`、格式检查、`pnpm prepare:check` 和 `git diff --check` 通过。初始 SHA 的远端 Windows Edge profiles E2E 尚在排队；加入本经验记录会产生新提交 SHA，必须重新验证新 SHA，不复用旧 SHA 的结果。
- 风险/回滚和清理证据（如涉及宿主状态）：L3 工程经验记录；不改产品运行行为、用户数据或宿主状态。回滚只需还原本条索引和记录。维护者已明确批准添加并推送该记录。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：已有提交 hook 将 `pnpm typecheck:chimera` 作为 staged frontend 文件检查；继续保留该门禁，UI 边界状态变更需复核 prop 声明、传参和渲染解构三处。无 CI 门禁变化。

### LESSON-20261017-001：data URL E2E 应验证请求结果而非固定响应类型

- 日期与时区：2026-10-17 UTC
- 状态：Verified（PR #478 已合入；对应 CI 与 Tauri E2E 均通过）
- 严重度：Low
- 模块：E2E / CI / WebView CSP
- master commit / ref gitlink SHA / 平台与测试环境：master `79c7cb8aa53686cd67effc3d021bd06aaad3e914`；ref gitlink `31019da14006b384b3272c72b4088179a9e72c47`；Windows runner，Microsoft Edge 154。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #478](https://github.com/MFSGA/Chimera/pull/478)，[Tauri E2E run 39003668695](https://github.com/MFSGA/Chimera/actions/runs/39003668695)。Hermetic 53 个 spec 中 52 个通过；唯一失败来自新加 smoke 断言，实际 `Response.type` 为 `basic`，而测试预期 `opaque`。单独的 Profile auto-update E2E 通过。
- 预期行为：两个桌面窗口均能成功读取 same-origin 页面，并通过 `connect-src data:` 完成 data URL 请求；no-cors 响应可以是标准 opaque 响应，也可以是 WebView 暴露的 readable basic 响应。
- 真实异常、复现步骤与证据：Windows Edge 154 在 `fetch('data:text/plain,chimera-csp-probe', { mode: 'no-cors' })` 上返回 `type: 'basic'`，导致测试在类型断言处失败；此前同一窗口的请求已返回 HTTP 成功响应。失败不表示 CSP 拒绝请求。
- 确认的根因与排除项：测试将 `no-cors` data URL 的响应类别误设为固定的 `opaque`。响应类型不是所测 CSP 功能的产品合同；工作流日志没有显示请求被阻止，也没有其他 smoke 或 hermetic spec 失败。
- 修复提交、差异映射及保留的兼容性边界：`tauri-e2e/specs/smoke.e2e.ts::assertConnectSourcesAreAvailable` 接受 `opaque`（状态码 0、空 body）或 `basic`（状态码 200、完整 probe payload），其他响应类型失败。CSP 产品策略保持不变。
- 原实现失败、新实现通过的回归测试，及测试注册位置：原断言在上述 Windows Edge run 失败；修正用例仍注册于默认 smoke 和 `hermetic`。PR #478 合并 SHA `b0c6beb65c3fc6099735c0b61cd913287b77964d` 的 CI 与 Tauri E2E 均通过。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：原 Windows Edge run 的 hermetic 套件为 52/53 通过；修正后 CI run [39005445410](https://github.com/MFSGA/Chimera/actions/runs/39005445410) 与 Tauri E2E run [39005445412](https://github.com/MFSGA/Chimera/actions/runs/39005445412) 均在 SHA `b0c6beb65c3fc6099735c0b61cd913287b77964d` 通过。此测试修复不声称证明 macOS WebKit 或其他 WebView 的响应类型。
- 风险/回滚和清理证据（如涉及宿主状态）：只改测试断言和经验记录，不改变宿主代理/TUN、持久化或 CSP 策略；可回滚为恢复原 `type/status` 断言并移除此条记录。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：后续 `data:` no-cors 测试先断言 fetch 已 resolve；只有 WebView 返回 readable basic 响应时才断言 payload，标准 opaque 响应应检查其受限元数据。无 CI 门禁变更。

### LESSON-20261017-002：Profile producer 启动必须等待 actor 注册处理完成

- 日期与时区：2026-10-17 UTC
- 状态：Verified（PR #485 精确 head `c29185ad9b5d62cf6802db5ee0448cf85c309168` 的 CI/E2E 通过，且已合并）
- 严重度：Low
- 模块：Profile / Rust / Core startup / CI
- master commit / ref gitlink SHA / 平台与测试环境：PR #485 修复提交 `d1c78138`、文档修正提交 `e0629eb9`；ref gitlink `31019da14006b384b3272c72b4088179a9e72c47`；Apple Silicon macOS。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #485](https://github.com/MFSGA/Chimera/pull/485)，精确 head `c29185ad9b5d62cf6802db5ee0448cf85c309168` 的 [CI run 39037029299](https://github.com/MFSGA/Chimera/actions/runs/39037029299) 与 [Desktop E2E run 39037029364](https://github.com/MFSGA/Chimera/actions/runs/39037029364) 均通过；合并提交 `5fc556b4dc1b4724c3486d537b3d239d89c97c5e` 与该已测 head 的 tree 相同。
- 预期行为：核心启动协调确认终态后，Profiles actor 完成后台 jobs、外部文件 watcher 和 journal reconciliation 的启动尝试，再让调用方继续；测试推进虚拟时间或修改被监视文件时，actor 必须已处理 producer 启动消息。
- 真实异常、复现步骤与证据：原 `ProfilesClient::start_producers()` 只向 actor cast `StartProducers` 后立即返回。新增 profile suite 中，`external_background_commit_does_not_bypass_the_mutation_participant` 在并行运行中出现竞态，定时刷新用例会等待超时；单独重跑外部文件用例可通过，而单独运行 `import_suggestion_is_committed_and_reschedules_the_timer` 会卡在虚拟时钟等待。测试在 actor 尚未处理 cast 前就推进 Tokio paused time 或写入文件，故本地结果取决于 actor 调度时序。
- 确认的根因与排除项：cast 只保证消息排入 actor mailbox，不保证 handler 已运行；调用方把消息投递误当作 scheduler/watcher 已就绪。不是 ref 的预期功能差异或持久化问题。ref `31019da` 的 `ProfilesClient::start_producers` 也使用 cast；本 PR 增加 Chimera 内部 request/reply await 作为启动同步边界，差异已记录在 `docs/ref-alignment-profiles.md` DIFF-PROFILE-06。
- 修复提交、差异映射及保留的兼容性边界：`d1c78138` 将 `StartProducers` 改为带 reply 的 actor call，`ProfilesClient::start_producers()` 等待回复，`ChimeraClient::init_core()` 在 lifecycle terminal reply 后等待该调用；重复 release 仍保持幂等。文档更正提交 `e0629eb9` 说明这是 best-effort 注册尝试完成，不保证各个 watcher/job 均成功注册。无公共 IPC、持久化格式或 agent contract 变化。
- 原实现失败、新实现通过的回归测试，及测试注册位置：cast-only 版本在本地 profile suite 中发生上述 scheduler/watcher 时序失败；await 回复后 `backend/tauri/src/client/profiles.rs` 的 Profile 测试全套通过。该修复没有新增独立测试入口，而是让现有生产者启动、定时刷新与外部 watcher 测试等待真实 actor 完成边界。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：`cargo +nightly test --manifest-path backend/Cargo.toml --locked -p chimera startup_reconcile -- --nocapture` 通过 2/2；`cargo +nightly test --manifest-path backend/Cargo.toml --locked -p chimera client::profiles::tests:: -- --test-threads=4` 通过 72/72；`cargo clippy --manifest-path ./backend/Cargo.toml --all-targets --all-features`、包级 rustfmt、Prettier 与 `git diff --check` 通过。提交钩子的 rustfmt/Clippy 也通过。Reviewer 静态审查无 P0–P2；其独立测试复跑因主机卷空间不足无法完成。本机桌面 startup/UI E2E 未运行；远端 CI/E2E 已对合并所用的相同 tree 完成验证。
- 风险/回滚和清理证据（如涉及宿主状态）：回滚为恢复原 cast-only 启动调用及无 reply 的 actor message；这会重新允许调用方早于 producer handler 完成而继续。未修改主机网络、持久化数据或用户 Profile。审查器新建的失败 Cargo target 已经 cargo clean 清理，未触碰其他构建缓存。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：仓库维护者已批准将已确认的竞态写入 `LESSONS.md`。保留 producer startup 测试中的 await 边界；PR #485 的精确 head CI/E2E 已通过后更新本条状态；不修改治理门禁。

### LESSON-20261017-003：桌面 E2E 应以目标窗口和用户可见状态准备路由

- 日期与时区：2026-10-17 UTC
- 状态：Verified（历史通过 SHA `13699341fc9389b4c18369609618e2fc944dddc3` 的 Windows Desktop E2E 及隔离套件通过）
- 严重度：Low
- 模块：E2E / main UI / WebKit
- master commit / ref gitlink SHA / 平台与测试环境：历史通过 PR #489 head `13699341fc9389b4c18369609618e2fc944dddc3`，当时 base `aed52c17959ff1a332e7a8c165cf8fda987b3faa`；ref gitlink `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3`；Apple Silicon macOS/WebKit 605.1.15 与 GitHub Actions Windows Edge 154，使用隔离 Cargo target、runtime 和 artifact 目录。
- 关联 Issue / PR / CI / 运行日志（脱敏）：本地 `traffic-analysis-main.e2e.ts` 定向 WebKit 调试；早期 Windows 失败见 [CI run 39015988294](https://github.com/MFSGA/Chimera/actions/runs/39015988294) 和 [run 39034891250](https://github.com/MFSGA/Chimera/actions/runs/39034891250)；精确最终 SHA 的 [Windows Desktop E2E run 39037138976](https://github.com/MFSGA/Chimera/actions/runs/39037138976) 通过。
- 预期行为：用例先选中 main 窗口，再设置视口并通过可见导航进入稳定 Dashboard；菜单操作使用 WebKit 支持且可观察的真实键盘激活，随后断言 URL 与控件状态。
- 真实异常、复现步骤与证据：最初的路由 helper 在页面内容已渲染时仍因要求恰有一个 `app-content` host 超时，诊断读数为两个 host 且各有一个子节点、透明度均为 1。另在选择 main 窗口前设置视口会在切换后仍显示移动导航，Radix 动画期间直接查找选项也不稳定。Windows Edge 154 在展开侧栏后 `aria-expanded` 为 `true`，但通过此前获取的 WDIO 元素句柄读到的 `aria-label` 为空，失败于 `assert.ok(expandedLabel)`（run `39015988294`）。复核本机运行时发现 `captureFailedTest` 把 WebdriverIO 9.32/Mocha 9.32 `afterTest` 的第二个参数（Mocha context）误当作结果对象；`passed` 因而是 undefined，成功用例也生成截图/HTML。此前据这些产物判断本机失败没有依据。旧运行日志中的滚动 `execute/async` 超时随后走了 WebdriverIO 的 `scrollIntoView` fallback，不能单独作为用例失败证据。随后 Windows Edge run `39034891250` 在展开侧栏后仍观察到 `aria-expanded=false`、标签为折叠状态；截图只捕获到一个 traffic page，因此该产物不能证明失败由退出中的旧路由导致。
- 确认的根因与排除项：路由准备条件、窗口选择顺序和动画中选项定位问题已有证据。Windows 旧 SHA 的失败是否由跨重渲染复用元素句柄造成尚未独立证明；第二次展开失败的具体触发原因也未从产物中确认。应用的 `AnimatedOutletPreset` 使用 `AnimatePresence` 保留退出路由，旧页面短暂留在 DOM 是已确认的生命周期行为；历史通过 SHA `13699341fc9389b4c18369609618e2fc944dddc3` 的 Windows hermetic E2E 已通过新路由稳定等待和侧栏状态读取。没有证据显示早期失败触发了宿主网络或持久状态变化。
- 修复映射及保留边界：`tauri-e2e/specs/traffic-analysis-main.e2e.ts` 先调用 `ensureMainWindow()`，再设置视口；需要回到 Dashboard 时通过可见 navbar 导航并等待稳定页面标记，且等待退出中的 traffic page 离开 DOM；进入 Topology 后要求恰有一个 traffic page，再从该页定位侧栏控件。指标菜单用触发器 focus 与 Enter/ArrowDown/Enter；侧栏切换后的 ARIA 状态现在由当前 DOM 快照读取。所有点击仍由 WDIO 对可见控件执行，共享路由 helper 与业务断言未放宽。
- 回归验证及注册位置：用例保留在 `main` suite，覆盖 Traffic 导航、范围/指标 URL、Flow/Geography 筛选保留及侧栏标签切换。侧栏属性改为每次从当前 DOM 读取并重定位控件。历史通过 SHA `13699341fc9389b4c18369609618e2fc944dddc3` 的 Windows hermetic E2E run `39037138976` 为 53/53 通过；Profile auto-update 与 controller fallback 各 1/1 通过。
- 验证命令与覆盖：在修正 `afterTest` 参数顺序后，执行 `CHIMERA_E2E_BINARY=/Volumes/Ventoy/Chimera/traffic-analysis-target/debug/chimera CHIMERA_E2E_RUNTIME_DIR=/Volumes/Ventoy/Chimera/traffic-analysis-runtime-20261017-final-evidence CHIMERA_E2E_ARTIFACT_DIR=/Volumes/Ventoy/Chimera/traffic-analysis-artifacts-20261017-final-evidence pnpm --filter @chimera/tauri-e2e exec wdio run ./wdio.conf.ts --spec ./specs/traffic-analysis-main.e2e.ts`，macOS WebKit 605.1.15 报告 1/1 通过；隔离产物目录仅含运行日志，没有失败截图/HTML。二进制 SHA-256 `920c0cec03ff6cd854dc13e1ffb02c167bc866fb136d8aa8cc56a4caebb5e84e`。GitHub Windows Desktop E2E run `39037138976` 精确测试 PR SHA，53/53 specs 通过；精确 SHA 的 CI run `39037138645` 中三平台 lint 和 Tauri 构建全部通过。这些链接只证明历史 SHA `13699341fc9389b4c18369609618e2fc944dddc3`；当前验收状态以 PR #489 最新检查为准。
- 风险/回滚和清理证据：用例只改变其隔离窗口尺寸和 route；runtime、artifact、cargo target 均位于本次独立目录，没有改用户 Profile、系统代理或 TUN。回滚为恢复该 spec 之前的路由准备和菜单选择步骤。
- 后续防复发措施：跨窗口用例先确保目标窗口被选中再设置视口；路由断言等待业务可见标记及退出路由清理；在交互前确认当前路由只有一个目标页；动画控件用稳定的用户输入流程；状态更新后从当前 DOM 读取属性，并在等待失败时保留最后观察值。E2E 失败钩子需按当前 WDIO 版本读取第三个结果参数，并用 reporter 状态与产物交叉核对；LESSONS 条目 ID 保持唯一，已删除重复的 `LESSON-20261017-003` 副本。没有调整 CI 或权限门禁。

### LESSON-20261017-004：前端-only CI 门禁不得直接编译 Tauri Rust 测试

- 日期与时区：2026-10-17 UTC
- 状态：Verified（PR #489 当前 SHA 的 Ubuntu 后端工作区测试和前端 harness 测试通过；Desktop E2E 的独立失败记录在 LESSON-20261017-003）
- 严重度：Low
- 模块：CI / Linux / Rust
- PR head commit / ref gitlink SHA / 平台与测试环境：PR #489 失败 SHA `b46ae47111ad90ea2c3e869c3a9cf397b88fdd9a`；修复 SHA `4f5d216b4f64890b676a59e27a4821b2d3ce1398`；本地 ref `a4f706fe788e3570247ad2fa49636c6b4c55c98b`；GitHub Actions `ubuntu-latest`。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #489](https://github.com/MFSGA/Chimera/pull/489)，[失败 CI run 39014838489](https://github.com/MFSGA/Chimera/actions/runs/39014838489)，[修复后 CI run 39015789669](https://github.com/MFSGA/Chimera/actions/runs/39015789669)，[当前 SHA 的 CI run 39015988284](https://github.com/MFSGA/Chimera/actions/runs/39015988284)，[当前 SHA 的独立桌面 E2E run 39015988294](https://github.com/MFSGA/Chimera/actions/runs/39015988294)。初始失败 job 在 `Test E2E harness and frontend models` 执行 Rust 测试时，`glib-sys` 的 `pkg-config` 找不到 `glib-2.0`，退出码 101。
- 预期行为：前端-only job 只执行前端与 E2E harness 检查；需要 Tauri 系统库的 Rust 测试只在安装了 Tauri 依赖的后端 job 运行。
- 真实异常、复现步骤与证据：前端测试步骤额外执行 `cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib features::agent::history::tests`。Linux 的 `Tauri dependencies` 步骤仅在 `BACKEND == true` 时安装 GTK/GLib，因此纯前端改动未准备这些库便开始编译 Tauri crate。
- 确认的根因与排除项：该 agent history 单测被放在 `FRONTEND == true` 门禁中，和 Rust/Tauri 依赖的后端测试环境不匹配；并非测试断言或产品逻辑失败。后端门禁中的 `pnpm test:backend` 已运行 workspace 全部 crate 和 all features，包含该单测。
- 修复提交、差异映射及保留的兼容性边界：提交 `4f5d216b` 从 `Test E2E harness and frontend models` 移除重复 Rust 命令；保留受 `BACKEND == true` 控制的 `pnpm test:backend`，因此后端改动仍执行该覆盖并在 Ubuntu 先安装 Tauri 依赖。
- 原实现失败、新实现通过的回归测试，及测试注册位置：原实现的失败证据为 CI run `39014838489`。PR #489 当前 SHA 的 CI run `39015988284` 中，Ubuntu `Test E2E harness and frontend models` 与 `Test backend workspace` 均通过；后者包含该 agent history Rust 单测。桌面 E2E run `39015988294` 的 Traffic 用例失败属于独立的测试交互问题，不影响此工作流修复结论。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：本地 `pnpm exec prettier --check .github/workflows/ci.yaml`、`actionlint -ignore SC2086 .github/workflows/ci.yaml` 与 `git diff --check` 通过；PR #489 当前 SHA 的 Ubuntu 后端工作区和前端 harness 测试均通过。未在本机执行 Ubuntu 后端工作区测试；其他平台构建仍在执行。未忽略时 actionlint 只报告原有 `STORE_PATH` 导出脚本的 SC2086，与本次差异无关。
- 风险/回滚和清理证据（如涉及宿主状态）：仅调整测试所在的 CI 步骤，不改变测试断言、产品行为或权限；回滚时可恢复该 Rust 命令到前端步骤。没有宿主网络、Service、TUN 或个人配置变更。
- 后续防复发措施：新增 Rust/Tauri 测试时，确认它位于 `BACKEND == true` 且 Tauri 系统依赖已安装的后端门禁；避免在前端-only job 重复执行 workspace 已覆盖的测试。

### LESSON-20261017-005：视口之外的编辑器不依赖平滑滚动完成定位

- 日期与时区：2026-10-17 UTC
- 状态：Verified（修复代码在 PR #484 head 的 Windows Desktop E2E 中通过；lesson-only 新提交仍需 CI）
- 严重度：Low
- 模块：Profile / E2E / Windows WebView2
- master commit / ref gitlink SHA / 平台与测试环境：PR #484 base `ae48f24ae5012a47533784695d7109cafb753d12`；ref gitlink `31019da14006b384b3272c72b4088179a9e72c47`；Windows GitHub Actions，Microsoft Edge 154。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #484](https://github.com/MFSGA/Chimera/pull/484)；[失败 run 38962020671](https://github.com/MFSGA/Chimera/actions/runs/38962020671) 的 head 为 `b66eb2ffc8b08d3c6de8c249ea609fac9e4e0b5d`；[通过 run 39018873894](https://github.com/MFSGA/Chimera/actions/runs/39018873894) 的 head 为 `62e561fb149c675f1f39dc8e1ba4132d026de287`。
- 预期行为：用户打开 Profile transform-chain 编辑区时，即使编辑区在当前滚动视口之外，也应把目标定位到可见区域；测试清理应恢复视口位置，避免影响后续用例。
- 真实异常、复现步骤与证据：Windows Edge 154 的 360px 高窗口中，点击 Profile transform-chain 操作后等待编辑区出现。失败日志报告 `editorTop=335`、`visibleBottom=351`、`maxScrollTop=820`，但 `scrollTop=49`，因此编辑区仍未进入可用视口；目标断言超时，所属 hermetic 运行共 51 个 spec 通过、2 个失败。失败来自 E2E 对平滑滚动完成位置的预期，不代表本次 Profiles 数据或持久化失败。
- 确认的根因与排除项：在 CI 的小视口下依赖 `scroll-behavior: smooth` 动画没有稳定到达目标；有界等待结束时滚动仅推进 49px。没有证据表明 Profile transform-chain 状态或持久化被破坏。
- 修复提交、差异映射及保留的兼容性边界：`frontend/chimera/src/pages/(main)/main/profiles/$type/detail/_modules/action-card.tsx` 计算目标偏移后临时将视口 `scroll-behavior` 设为 `auto !important` 并直接设置 `scrollTop`，随后恢复原样式；`tauri-e2e/specs/profile-transform-chain-ui.e2e.ts` 在失败清理时将 Profile 视口复位到顶部。保留窗口尺寸和可见性断言，不把定位动画当成用户业务结果。
- 原实现失败、新实现通过的回归测试，及测试注册位置：失败 run 38962020671 在 `profile-transform-chain-ui.e2e.ts` 的视口定位等待处失败；修复后该 spec 随 hermetic suite 在 run 39018873894 对 PR head `62e561fb` 通过。测试继续注册在 `profiles`、`critical` 和 `hermetic` suite；没有新增跳过或重试。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：GitHub Windows Desktop E2E run 39018873894 对应精确实现 head `62e561fb149c675f1f39dc8e1ba4132d026de287`，结论 success。新增本条仅修改 LESSONS.md；该 lesson-only 提交的 CI/E2E 仍待新 SHA 运行，不能复用为新 SHA 的检查通过证据。
- 风险/回滚和清理证据（如涉及宿主状态）：仅新增工程经验记录，不改变产品状态、持久数据或宿主设置；回滚时移除本条及索引行即可。失败清理现在显式复位测试窗口滚动位置。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：为视口之外的元素设计桌面 E2E 时，断言目标可见状态与所需滚动距离；避免等待未经验证的平滑动画到达最终位置，并在失败清理中恢复测试拥有的滚动状态。维护者已批准此 L3 lesson；没有修改 CI 门禁。

### LESSON-20261017-006：Dashboard 右键测试必须先发送 pointerdown

- 日期与时区：2026-10-17 UTC
- 状态：Open（驱动修复已在 PR #490 工作树完成；新 SHA 的 Windows Desktop E2E 待复验）
- 严重度：Low
- 模块：E2E / Windows
- master commit / ref gitlink SHA / 平台与测试环境：master `5fc556b4dc1b4724c3486d537b3d239d89c97c5e`；ref `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3`；Windows Edge hermetic E2E
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #490](https://github.com/MFSGA/Chimera/pull/490)，[失败运行 39072145858](https://github.com/MFSGA/Chimera/actions/runs/39072145858)，52 个 spec 中 50 个通过；Dashboard 编辑菜单和后续 Agent 页面导航失败。
- 预期行为：右键 Dashboard 后，上下文菜单显示已注册的 Dashboard 操作，后续编辑流程可继续。
- 真实异常、复现步骤与证据：Windows E2E 右键后菜单等待为 false；失败截图显示应用通用剪贴板菜单，没有 Dashboard 项。随后 Agent 页就绪等待也失败。
- 确认的根因与排除项：ref 与 Chimera 的共享菜单 provider 都在 document capture 阶段通过 `pointerdown` 记录右键目标，再沿 DOM 查找已注册菜单。vendored 驱动只合成 `mousedown`、`mouseup` 和 `contextmenu`，没有触发该记录逻辑。对照 ref `frontend/nyanpasu/src/components/providers/context-menu-provider.tsx` 与 Chimera `frontend/chimera/src/components/providers/context-menu-provider.tsx` 后，未发现 Dashboard 菜单实现差异；Agent 页面失败发生在前一用例之后，待菜单修复后复验是否为连带失败。
- 修复提交、差异映射及保留的兼容性边界：`backend/tauri/vendor/tauri-plugin-wdio-webdriver/src/platform/executor.rs::dispatch_pointer_event` 在按下时、对应 MouseEvent 前合成冒泡且可取消的 `pointerdown`，保留匹配右键释放后的 `contextmenu`。该修复验证应用 DOM 事件路径，不代表完整 PointerEvent 序列或真实鼠标输入。
- 原实现失败、新实现通过的回归测试，及测试注册位置：原实现由上述 Windows PR E2E 运行失败；新 SHA 的复验仍待执行。相关回归仍由 `tauri-e2e/specs/dashboard-main-layout.e2e.ts` 的 Windows hermetic suite 运行。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：`cargo check --locked --manifest-path backend/tauri/vendor/tauri-plugin-wdio-webdriver/Cargo.toml --target-dir /tmp/chimera-wdio-plugin-check` 通过；`rustfmt --edition 2021 .../executor.rs` 与 `git diff --check` 通过。Windows E2E 尚未对修复 SHA 运行；真实可信鼠标输入不在覆盖范围。
- 风险/回滚和清理证据（如涉及宿主状态）：仅测试驱动合成事件和测试证据文档；不修改产品菜单、持久化或宿主网络。回滚为还原驱动 PointerEvent 合成并移除此记录。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：更新 [U09](docs/testing/upstream-evidence.md#u09-tauri-driver-合成-pointerevent-与右键上下文菜单边界) 及 Dashboard 测试契约，后续升级驱动时重跑相同菜单最小复现；无 CI 门禁调整。

### LESSON-20261017-007：等待可观察的失败状态并在重渲染后重新查询控件

- 日期与时区：2026-10-17 UTC
- 状态：Open（修复已在 PR #490 工作树完成；新 SHA 的 Windows Desktop E2E 待复验）
- 严重度：Low
- 模块：E2E / Windows
- master commit / ref gitlink SHA / 平台与测试环境：失败 head `ce86b993fcc9f21d863abd0c59e61c00bf192d00`；ref gitlink `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3`；Windows Edge hermetic E2E。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #490](https://github.com/MFSGA/Chimera/pull/490)，[失败运行 39074305271](https://github.com/MFSGA/Chimera/actions/runs/39074305271) 与 [39077318693](https://github.com/MFSGA/Chimera/actions/runs/39077318693)，后者 52 个 spec 中 50 个通过；Dashboard retry 和 main input 两个 spec 失败。随后 head `a4e62e56` 的 [run 39084190736](https://github.com/MFSGA/Chimera/actions/runs/39084190736) 中，Dashboard 的 `get_storage_item` 故障计数仍为 0，hermetic 为 51/52；main input 已通过。
- 预期行为：注入存储读取故障后，Dashboard 应显示 `load-error`，释放故障后 Retry 应恢复；外部控制器弹窗出现后，输入框聚焦应显示浮动标签。
- 真实异常、复现步骤与证据：两次运行均未观察到 Dashboard `load-error`；39077318693 的失败截图显示页面进入编辑模式但无错误提示。main input 在同一运行断言 DOM 矩形高度为 56px 时得到 53px；失败诊断同时显示计算样式高度为 56px。
- 确认的根因与排除项：锁定 Tauri 2.12.0 的 `scripts/core.js` 以 `Object.defineProperty` 且只提供 `value` 定义 `__TAURI_INTERNALS__.invoke`；未声明的 `writable` 和 `configurable` 默认均为 false。因此赋值不能替换该函数。run `39116055509` 的失败明确为 “The widget storage failure could not wrap Tauri invoke.”，验证了注入前提错误；这不表示 Dashboard 业务恢复逻辑失败。随后 run `39160740767` 的错误聚合只输出外层栈，隐藏了测试控制 IPC 的内部错误。Tauri 窗口 capability 未声明自定义 `e2e` 插件权限，且该插件没有权限清单，因此插件命令不允许 WebView 调用。Windows WebView 的元素矩形与计算 CSS 尺寸不同，`getBoundingClientRect()` 的 53px 不表示 `h-14` 样式错误，计算样式仍为 56px。
- 修复提交、差异映射及保留的兼容性边界：用仅在 Cargo `e2e` feature 编译的应用级 IPC 命令控制 fixture，在 `get_storage_item` / `set_storage_item` 实际 IPC 入口按唯一 key 注入故障，并通过状态命令确认触发；spec 不再改写 Tauri `invoke`。失败清理会清除后端 fixture；成功 Retry 仍经原 IPC。失败证据现在递归保存 `AggregateError` 内部错误。该修复当前位于 PR #490 工作树，精确新 SHA 的 Windows E2E 待复验。main input 固定窗口、中文、light theme 与字体加载，设置后恢复原主题；继续断言 `h-14` 的 56px 计算样式及浮动标签等行为，不再要求平台缩放后的矩形恰好为 56px。
- 原实现失败、新实现通过的回归测试，及测试注册位置：head `517f8f0` 在安装 JS 拦截器时失败，Windows E2E 没有证明 Dashboard 故障恢复行为。当前 spec 使用 E2E-only Rust fixture，并断言实际 IPC 触发计数与界面错误/重试；仍由 `tauri-e2e/specs/dashboard-main-layout.e2e.ts` 注册在 hermetic suite，新的应用级命令分发待 Windows 复验。main input 在此前运行中通过。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：应用级命令修复后，`cargo +nightly test --manifest-path backend/Cargo.toml --locked -p chimera --features e2e e2e_test::tests -- --test-threads=1` 为 1/1 通过，`cargo +nightly check --manifest-path backend/Cargo.toml --locked -p chimera --lib --no-default-features` 通过，E2E harness TypeScript 检查、Dashboard spec Oxlint、相关 Prettier 检查、Rust 格式检查和 `git diff --check` 均通过。pnpm 封装的 typecheck 因 worktree 共享 `node_modules` 符号链接触发 unsafe task-run-state 错误，直接运行等价的 TypeScript 检查通过。精确新 SHA 的 Windows E2E run `39168311925` 与 CI run `39168311983` 尚在执行；未声称桌面 E2E 已在本机运行，也未声称已验证真实鼠标输入或弹窗在其他平台的行为。
- 风险/回滚和清理证据（如涉及宿主状态）：fixture 只在 `e2e` feature 编译；spec 清除故障状态后由既有逻辑恢复 KV 配置及 UI 状态。没有宿主网络、Service、TUN 或个人配置变更。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：根据测试依赖上游证据，避免替换宿主定义的不可写 WebView IPC 函数；使用范围受限、状态可观察并可清理的 E2E 后端边界。等待状态读取和 UI 错误而非固定时序。新 SHA 的 Windows 复验通过后更新状态。
- 最新复现与修复状态：run [39086974019](https://github.com/MFSGA/Chimera/actions/runs/39086974019) 与 [39089126084](https://github.com/MFSGA/Chimera/actions/runs/39089126084) 曾在拦截器检查前失败；head `517f8f0` 的 E2E run [39116055509](https://github.com/MFSGA/Chimera/actions/runs/39116055509) 再现 invoke 不可覆盖导致注入失败，hermetic 其余 51/52 个 spec 通过。后续 run [39160740767](https://github.com/MFSGA/Chimera/actions/runs/39160740767) 再现 Dashboard 用例失败，定位到独立测试插件没有 WebView capability 权限，且 aggregate 错误未展开内部原因。当前修复改为 E2E feature 下的应用级测试 IPC，保留真实存储命令入口按 key 注入并检查触发状态；Windows E2E 尚待新 SHA 复验。该 run 中 main input 已通过：输入框聚焦后等待计算边框达到 `2px`，仍保留原精确样式断言。Dashboard cleanup 会先清除 fixture，再逐项恢复原 KV 值、读回、刷新及最终核验，并汇总清理错误。

### LESSON-20261018-001：异步出现的元素需要在每次轮询中重新查询

- 日期与时区：2026-10-18 UTC
- 状态：Verified（选择器轮询修复在 PR #477 精确 SHA `6612c0f93b0c77af193919123bc2360150afb6de` 的 Windows Desktop E2E run `39084555437` 通过；后续只增强超时诊断，新的提交仍需门禁复验）
- 严重度：Low
- 模块：E2E / Windows
- master commit / ref gitlink SHA / 平台与测试环境：master `e9d9392b901681db0eb46fc00144e83b00cb4f34`；ref gitlink `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3`；Windows Edge 154 hermetic E2E；WebdriverIO 9.32.0。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #477](https://github.com/MFSGA/Chimera/pull/477)，[Tauri E2E run 39076767361](https://github.com/MFSGA/Chimera/actions/runs/39076767361) 首次尝试中 `clash-settings-main.e2e.ts` 的目标可见性等待超时；HTML 与截图产物显示 Clash 设置页已渲染且目标卡片存在。相同 SHA 的重跑以及修复 SHA `6612c0f` 的 [Windows Desktop E2E run 39084555437](https://github.com/MFSGA/Chimera/actions/runs/39084555437) 均通过。
- 预期行为：进入 Clash 设置页后，patch settings group 可见，随后快照断言其卡片层级符合 ref。
- 真实异常、复现步骤与证据：首次 CI 尝试在创建单个 WDIO 元素对象后调用 `waitForDisplayed({ timeout: 15_000 })`，报告元素仍未显示；失败后采集的 HTML 和截图中路由及目标 group 可见。
- 确认的根因与排除项：锁定 `webdriverio@9.32.0` 的 `waitForDisplayed` 在同一个元素对象上调用 `isDisplayed` 进行轮询；该版本源码明确说明它不会等待元素存在，`isDisplayed` 在对象没有 element ID 时返回 `false`。因此目标首次异步渲染时，单个已创建元素对象不会代表后续 selector 查询。失败产物中的正确页面排除了产品卡片缺失；这次记录的是测试等待边界，不是产品 UI 回归。
- 修复提交、差异映射及保留的兼容性边界：测试改为在只读 `browser.waitUntil` 每轮重新执行精确的 direct-child selector，再检查当前元素是否显示。ref 布局断言不变；更新 U02 记录所用 WDIO 版本源码事实。
- 原实现失败、新实现通过的回归测试，及测试注册位置：原实现的 15 秒等待在上述首次 Windows run 失败；重新查询选择器的修复已在精确 SHA `6612c0f` 的 Windows Desktop E2E 中通过，并继续注册于 `tauri-e2e/specs/clash-settings-main.e2e.ts` hermetic suite。超时消息现保留最后一次 `exists`/`displayed` 观测；此诊断增强提交的门禁待复验。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：首个修复 SHA 的 Windows Desktop E2E run `39084555437` 通过；其完整 CI run `39084555350` 中 Changes、三平台 lint 均通过，macOS/Windows/Ubuntu Tauri builds 当时仍在执行。当前诊断增强的 Prettier 检查与 `git diff --check` 本地通过；E2E 类型检查未能在本机执行，因为该 worktree 缺少 WDIO/Node 类型定义，离线 pnpm 安装因锁定依赖 tarball 未缓存而中止。没有声称旧代码在所有运行中稳定失败。
- 风险/回滚和清理证据（如涉及宿主状态）：只改测试轮询和测试工具依据记录，不改产品、持久数据或宿主状态；可恢复单次元素查询与 `waitForDisplayed`。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：对可能晚于路由就绪出现的 DOM 节点，在轮询回调内重新查询 selector；等待失败时输出最后一次存在/可见状态；不添加固定暂停。无 CI 门禁调整。

### LESSON-20261018-002：E2E 控件定位不能假设当前语言

- 日期与时区：2026-10-18 UTC
- 状态：Open（选择器与显示环境修复已在 PR #490 工作树完成；新 SHA Windows Desktop E2E 待复验）
- 严重度：Low
- 模块：E2E / Windows
- master commit / ref gitlink SHA / 平台与测试环境：PR head `ad81c3f67724f8b572520fac10d34b07b52e895f`；ref gitlink `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3`；Windows Edge 154 hermetic E2E。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #490](https://github.com/MFSGA/Chimera/pull/490)，[失败运行 39169449230](https://github.com/MFSGA/Chimera/actions/runs/39169449230)，51/52 个 spec 通过；Dashboard 用例在进入存储恢复步骤前失败。
- 预期行为：Dashboard 右键菜单已打开时，测试识别编辑项并继续执行；配置控件可跨受支持语言稳定定位。
- 真实异常、复现步骤与证据：失败等待报告 `Dashboard edit menu; last observed: false`。同一 run 的 before-cleanup HTML 显示菜单的 `data-state="open"`，编辑项文本为 `Edit Widgets`。
- 确认的根因与排除项：spec 以中文 `编辑组件` 判断菜单是否打开；英文菜单被错判为关闭后再次右键，关闭已显示菜单。单位、重置、保存控件亦依赖中文 aria-label。证据显示产品菜单已正常打开；失败来自测试本地化假设，不是 Dashboard 菜单行为回归。
- 修复提交、差异映射及保留的兼容性边界：在菜单编辑项、配置选项、重置和保存按钮添加稳定 `data-slot` / `data-config-field`；测试通过这些属性定位，并用 `patch_verge_config` 固定 `zh-cn` 与 light theme，suite 结束时恢复原配置。选择器扩展不改变用户可见行为。
- 原实现失败、新实现通过的回归测试，及测试注册位置：失败复现见 run `39169449230`；修复继续由 `tauri-e2e/specs/dashboard-main-layout.e2e.ts` 的 `main` / Windows `hermetic` suite 验证。新 SHA 的桌面复验尚未完成。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：E2E TypeScript、25/25 harness 单元测试、widget 单测 6/6、Oxlint、Prettier、frontend boundary 脚本及 `git diff --check` 通过。pnpm 包装命令受共享 `node_modules` 符号链接的安全检查阻止，已直接运行等价工具。没有本机桌面 E2E 结果；新 SHA Windows E2E 待复验。
- 风险/回滚和清理证据（如涉及宿主状态）：测试在独立 E2E 配置中暂时修改应用主题，并在 suite cleanup 恢复、读回；产品仅增加定位属性。不修改用户配置、宿主网络或系统设置。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：测试 locator 使用稳定业务属性，不依赖翻译文案；布局用例仅固定其依赖的主题并恢复原设置。不修改 CI 门禁。

### LESSON-20261019-001：主题 E2E 断言必须查询实际主题根节点

- 日期与时区：2026-10-19 UTC
- 状态：Open（断言已修正；新 SHA Windows Desktop E2E 待复验）
- 严重度：Low
- 模块：E2E / Windows
- master commit / ref gitlink SHA / 平台与测试环境：PR head `8337acd7a8caa72545c77987b775fcb913ec016e`；ref gitlink `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3`；Windows Edge 154 hermetic E2E。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #490](https://github.com/MFSGA/Chimera/pull/490)，[失败运行 39180855095](https://github.com/MFSGA/Chimera/actions/runs/39180855095)，51/52 个 spec 通过；Dashboard spec 在 `before all` 超时退出。
- 预期行为：测试设为浅色主题后，应确认主题已应用，再开始依赖该主题的布局断言。
- 真实异常、复现步骤与证据：Windows runner 在 `Dashboard test locale and light theme did not apply` 超时；超时条件读取 `[data-slot="app-root"]` 是否含 `light` 类。
- 确认的根因与排除项：`ExperimentalThemeProvider` 的 `applyRootStyleVar` 将主题类加到 `document.getElementById('root')`，而不是内部的 `[data-slot="app-root"]`。因此检查的是错误 DOM 节点；不是配置写入失败或主题未应用。
- 修复提交、差异映射及保留的兼容性边界：测试现在只通过配置 IPC 临时设置主题，不再改语言；等待并断言真实 `#root` 主题类，suite cleanup 恢复并读回原主题。产品代码不变。
- 原实现失败、新实现通过的回归测试，及测试注册位置：失败见 run `39180855095`；修复后的同一 Dashboard spec 继续由 Windows hermetic suite 执行，新的 SHA 结果待验证。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：CI run `39180855195` 三平台 lint 和构建均通过，E2E harness 类型检查通过；Dashboard 桌面 E2E 在 run `39180855095` 因错误根节点断言失败。新 SHA Windows 桌面复验待执行。
- 风险/回滚和清理证据（如涉及宿主状态）：仅 E2E 临时改写主题设置，suite cleanup 读回验证原值；无产品或宿主网络变更。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：主题断言须对照 provider 实际 DOM 更新目标；语言和主题测试环境只设置用例真正依赖的偏好。无 CI 门禁调整。

### LESSON-20261019-002：成功重试后重新打开配置弹层再检查控件

- 日期与时区：2026-10-19 UTC
- 状态：Open（测试步骤已修正；新 SHA Windows Desktop E2E 待复验）
- 严重度：Low
- 模块：E2E / Windows
- master commit / ref gitlink SHA / 平台与测试环境：PR head `fec8f0ecb450aed73bffb8d622a799881c8b7b94`；ref gitlink `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3`；Windows Edge 154 hermetic E2E。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #490](https://github.com/MFSGA/Chimera/pull/490)，[失败运行 39189516640](https://github.com/MFSGA/Chimera/actions/runs/39189516640)，hermetic 51/52 个 spec 通过；Dashboard recovery 用例在成功写入后检查控件时失败。
- 预期行为：重试保存成功后，UI 与后端值一致；点击编辑栏中的 Retry 后，测试重新打开配置弹层再验证设置控件可用。
- 真实异常、复现步骤与证据：run `39189516640` 已确认 Traffic 显示 `bit/s` 且持久值为 `bits`，随后 `waitForUnitControl` 观察到 `undefined`。失败前 HTML/截图显示 Dashboard 仍在编辑模式，但 widget 配置弹层已关闭，速度单位控件不在 DOM 中。
- 确认的根因与排除项：测试点击编辑栏中的重试按钮后直接轮询弹层内的 speed-unit 控件；重试按钮位于弹层之外，点击它会关闭弹层，因此不存在待查询元素。故障注入和写入重试已成功，不是产品保存失败。
- 修复提交、差异映射及保留的兼容性边界：移除只检查现存控件的 `waitForUnitControl`；在重试后和重置后调用已有 `openTrafficConfig`，先按可见 UI 重新打开弹层，再确认速度单位控件可见且启用。产品代码不变。
- 原实现失败、新实现通过的回归测试，及测试注册位置：失败见 run `39189516640`；修复后的恢复步骤仍由 `tauri-e2e/specs/dashboard-main-layout.e2e.ts` 注册在 Windows hermetic suite，新 SHA 结果待验证。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：run `39189516650` 的 CI 三平台检查与 Tauri 构建全部通过；run `39189516640` 证明失败注入、重试写入和持久值断言已通过，但在弹层可见性检查失败。新 SHA Windows 桌面复验待执行。
- 风险/回滚和清理证据（如涉及宿主状态）：只调整 E2E 的可见 UI 交互顺序；已有清理仍恢复原 widget 配置，不改持久化实现或宿主网络。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：每次状态提交、路由或菜单操作后重新检查并按需打开目标控件的宿主弹层，不沿用已关闭弹层中的元素引用。无 CI 门禁调整。

### LESSON-20261019-003：配置回滚后重新协调运行时

- 日期与时区：2026-10-19 UTC
- 状态：独立 L2 复核完成，代码修复已在本地集成分支实现；maintainer 对本记录的批准与合入 master 待完成
- 严重度：Medium
- 模块：Settings / Clash runtime
- master commit / ref gitlink SHA / 平台与测试环境：master `02d7e3fc`；本地只读 ref checkout `685042aaedaea26a591900be0b494dfe8efd79b0`（父仓库 gitlink 为 `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3`）；macOS 本地复核与 Rust 测试。
- 关联 Issue / PR / CI / 运行日志（脱敏）：本地独立复核发现；无远端 PR 或 CI 运行。
- 预期行为：Clash typed patch 已应用候选运行配置后，若旧配置镜像写盘失败，恢复源配置并重新协调当前持久配置对应的运行态；若运行态补偿也失败，返回原始错误及补偿错误。
- 真实异常、复现步骤与证据：`ClashConfigClient::patch_typed_config` 原来先替换持久状态、reconcile 候选运行态，再写 legacy Clash/Verge 镜像；后续保存失败时只做 CAS 状态回滚和文件补写，没有恢复内核配置。文件系统写失败可使 API 返回错误但新运行态仍生效。复核通过代码调用顺序确认可达；未注入真实磁盘错误。
- 确认的根因与排除项：候选运行态应用与持久化完成处于分开的步骤；失败分支将源数据回滚误认为已恢复整体事务，遗漏了对回滚后最新状态的 lifecycle reconcile。
- 修复提交、差异映射及保留的兼容性边界：在 Clash typed patch 的补偿路径中，尽力完成 CAS 回滚与镜像保存后，对当前最新 typed 配置重新调用现有 core lifecycle；同时汇总并返回 CAS、保存和运行态补偿错误。未增加新的配置业务层或兼容入口。
- 原实现失败、新实现通过的回归测试，及测试注册位置：`backend/tauri/src/client/clash_config.rs` 的 `typed_clash_patch_restores_source_and_runtime_after_persistence_failure` 通过真实 Clash 状态 actor 与配置文件注入 legacy persistence 回调错误，并观察来源配置与模拟运行态均回滚；`typed_clash_compensation_reports_runtime_reconcile_failure` 验证运行态补偿错误与 CAS 冲突均保留。测试未注入真实磁盘保存错误或真实内核协调失败。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：`cargo test --locked --manifest-path backend/Cargo.toml -p chimera typed_clash_patch_restores_source_and_runtime_after_persistence_failure -- --nocapture` 1/1；完整 `cargo test --quiet --locked --manifest-path backend/Cargo.toml -p chimera` 572 passed、0 failed、1 ignored；`cargo check`、目标 crate `cargo fmt -- --check` 与 `git diff --check` 通过。桌面 E2E 和真实磁盘故障注入未执行。
- 风险/回滚和清理证据（如涉及宿主状态）：仅改变失败补偿时的 core reconcile 与错误聚合；回退实现 commit 会恢复原补偿路径。无宿主网络或系统设置操作。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：所有“运行态已变更、源持久化未完成”的失败路径都须验证源状态恢复后重新 reconcile 最新状态；必要时使用故障注入覆盖保存失败及 CAS 冲突。未修改 CI 门禁。

### LESSON-20261019-004：移植旧日语目录时校验当前主线键集

- 日期与时区：2026-10-19 UTC
- 状态：日语目录已补齐；旧版本回退配置兼容问题已有文档和回归测试，独立复核确认无 P0–P2 阻塞项；L2/L3 审批待完成
- 严重度：Low
- 模块：Localization / frontend
- master commit / ref gitlink SHA / 平台与测试环境：当前 master `ae324101`；ref gitlink `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3`；macOS 本地移植工作树。
- 关联 Issue / PR / CI / 运行日志（脱敏）：无远端 PR 或 CI。初次运行 `pnpm --filter chimera-ui test:localization` 时，键集断言失败；日语目录缺少 71 个当前英文目录已有的键。
- 预期行为：日语目录覆盖当前主线英文目录的所有键，且每条消息的插值字段一致。
- 真实异常、复现步骤与证据：从旧 `codex/ref-ja-localization-v2` 移植后运行上述测试，发现缺少 Dashboard widget 配置、Profile transform 与 Profile sync history 等后续新增键。补齐后首版测试命令 3/3 通过；随后新增 canonical backend mapping 覆盖，当前 suite 4/4 通过。
- 确认的根因与排除项：日语目录来自落后于当前 master 的功能分支；移植时仅验证了旧分支英文目录的键集，没有与目标基线当前英文目录重新比对。不是 Paraglide 或 locale 运行时错误。
- 修复提交、差异映射及保留的兼容性边界：参照固定 `ref/` 日语目录补入其已有翻译，并为 ref 尚无的主线键补充日语文案；保留目标基线已有消息。未更改持久化或 locale 运行时接口。
- 原实现失败、新实现通过的回归测试，及测试注册位置：`frontend/chimera/tests/localization.test.ts` 检查完整键集、占位符、历史别名与后端 canonical value；运行入口为 `pnpm --filter chimera-ui test:localization`，候选本地套件 4/4 通过（前端构建已生成 ignored Paraglide runtime）。该命令已加入当前候选的 CI 步骤，工作流变更须单独取得 L3 批准。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：初始键集断言失败 1 次；补齐后的 localization suite 4/4 通过，macOS WebKit 桌面用例最终干净运行 1/1。测试命令里的重复 Paraglide 生成曾因 CDN 插件请求无响应运行近两分钟后中断；现删除重复生成，沿用 CI 前端构建生成的 runtime。候选桌面运行使用未打包 core sidecar 的 E2E binary，仅作为日语 UI/配置路径验证。更早的两次 E2E 分别出现 WebDriver execute/sync 超时自动重试，以及 after hook 清理失败，根因未确认。完整 `chimera-config` 套件 147/147 通过，其中旧版 enum 回退复位测试通过；完整 runtime/core 桌面覆盖和 Windows CI/E2E 尚未执行。
- 风险/回滚和清理证据（如涉及宿主状态）：仅更新日语消息目录和对应验证记录；回滚该目录补丁会重新暴露当前主线缺失的翻译。无用户数据或宿主状态变化。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：从旧分支移植消息目录时，先比较目标基线与目标语言目录的键和插值字段，再运行已注册的 localization suite；CI 新步骤待单独批准。

### LESSON-20261019-005：语言切换须及时应用并按权威配置验证

- 日期与时区：2026-10-19 UTC
- 状态：Mitigated（当前分支 macOS WebKit 定向用例和桌面构建通过；Windows 新 SHA 复验待执行）
- 严重度：Low
- 模块：E2E / Localization
- master commit / ref gitlink SHA / 平台与测试环境：此前失败 SHA `3f609dfa166c68c9009232ed912b615ca4a84e84`，当前失败候选 SHA `4697281c58429a98b17af85ca2ab1ce7a97bc615`；PR #494 将 ref 从 `53b004ccb3508d0673b6cb691be65e0e48bdd593` 更新到 `4eaf13b9c5dc468fdfc38272bf76b20a681d2e54`；Windows Edge 153 hermetic E2E 与 macOS WebKit 605.1.15 embedded E2E。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #494](https://github.com/MFSGA/Chimera/pull/494)，[失败运行 39290359852](https://github.com/MFSGA/Chimera/actions/runs/39290359852)、[失败运行 39296100576](https://github.com/MFSGA/Chimera/actions/runs/39296100576)、[失败运行 39302840484](https://github.com/MFSGA/Chimera/actions/runs/39302840484)、[失败运行 39314795712](https://github.com/MFSGA/Chimera/actions/runs/39314795712) 和[PR #499 失败运行 39353234761](https://github.com/MFSGA/Chimera/actions/runs/39353234761)。
- 预期行为：测试变更语言前保存稳定的各窗口初始状态，并在 afterAll 中恢复后端配置、缓存、DOM 语言、路由和窗口尺寸。
- 真实异常、复现步骤与证据：旧 Windows runs `39290359852`、`39296100576`、`39302840484` 和 `39314795712` 暴露语言快照恢复、跨 spec locale 污染和 mutation 完成延迟。PR #499 的 Windows run `39353234761` 在刷新后等待主窗口重绘 30 秒失败，随后 cleanup 报告无法恢复 legacy locale；其余 52/53 个 spec 通过。失败发生在主界面渲染等待阶段，旧用例没有保存足够状态来区分窗口启动失败与语言状态未收敛。
- 确认的根因与排除项：语言设置通过 `LanguageProvider.setLanguage` 等待 `useSettings().upsert.mutateAsync()` 后才调用 Paraglide `setLocale`。当前 `frontend/interface/src/ipc/use-settings.ts` 的 `onSuccess` 直接返回 `queryClient.invalidateQueries()` Promise，TanStack Query 会等待该 Promise；固定 ref `4eaf13b9c5dc468fdfc38272bf76b20a681d2e54` 的 `frontend/query/src/ipc/use-settings.ts::useTypedConfig` 则在 block body 中触发 invalidation、不返回 Promise。这个已确认的共享源码差异会把 UI 语言应用推迟到配置 refetch 完成，符合失败运行中 UI 同步超时、最终快照才显示日语的现象。E2E 的连续三次远程快照也对慢驱动较敏感；修复后以一次 UI/cache 同步观察确认即时更新，并继续独立验证 typed config、磁盘、刷新后 main UI 和 legacy 页面。此前的 WebView 快照污染是另一项真实测试隔离问题，不能解释或替代这次源码差异。
- 修复提交、差异映射及保留的兼容性边界：将 Chimera `frontend/interface/src/ipc/use-settings.ts::useTypedConfig` 的成功回调改为 pinned ref `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3` 对应的 block body，使 cache invalidation 继续运行但不延迟 mutation 完成；`LanguageProvider` 的配置调用和 UI 应用仍复用共享 `useSettings` API。`tauri-e2e/specs/user-interface-language.e2e.ts` 通过真实键盘菜单选日语，分别断言当前 UI/cache、typed app config、`application.yaml`、刷新后的 main UI 与导航后 legacy 代理页。创建 main 窗口后先登记其 URL 与尺寸，再等待首屏，确保 setup 超时也进入 cleanup。保留跨 spec 快照前重载和 cleanup 恢复逻辑。无持久化格式或 IPC 接口变化。
- 原实现失败、新实现通过的回归测试，及测试注册位置：旧 Windows 失败见上述 runs；修复后的 `user-interface-language.e2e.ts` 仍注册于现有 hermetic suite。macOS WebKit 定向 spec 两次通过；同一源码与用例的 PR #494 head `c9228e99e53d46a48782660dd05a8818706b00aa` 的 [Windows Tauri E2E run 39352141194](https://github.com/MFSGA/Chimera/actions/runs/39352141194) 和[三平台 CI run 39352141200](https://github.com/MFSGA/Chimera/actions/runs/39352141200) 均通过。该通过结果来自 PR #494 的完整 diff，不代替本修复分支新 SHA 的复验。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：当前分支运行 `pnpm typecheck`、`pnpm --filter @chimera/tauri-e2e test:unit`（25/25）、`pnpm --filter chimera-ui test:localization`（4/4）、`pnpm lint:frontend-boundaries`、Prettier、`git diff --check`、`pnpm e2e:tauri:build` 均通过；当前本机 `pnpm --filter @chimera/tauri-e2e exec wdio run ./wdio.conf.ts --spec ./specs/user-interface-language.e2e.ts --logLevel warn` 在 macOS WebKit 605.1.15 通过 1/1。此前两次定向运行亦通过 1/1。新 SHA 的 Windows hermetic E2E 尚未执行；不据此声称完整桌面矩阵通过。
- 风险/回滚和清理证据（如涉及宿主状态）：本修复只改变 shared settings mutation 的完成时序，使 UI 应用不等待查询 refetch；数据写入与 cache invalidation 仍执行，持久化格式和 IPC 不变。测试继续恢复持久语言、locale cache、DOM、路由与窗口尺寸。若回滚，恢复 ref 更新前的 hook 和用例即可。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：共享设置 mutation 的 `onSuccess` 若只需触发后台 refetch，应使用不返回 invalidation Promise 的 block callback，并由 ref 对齐复核。跨用例重用 WebView 时重载并等待主 UI 的 typed 配置、DOM 和缓存收敛；先验证 UI 即时同步，再独立核验后端、磁盘、刷新和 legacy 路径。此 LESSON 更新已获维护者明确批准；未修改 CI 门禁。

### LESSON-20261019-006：取消刷新测试必须控制 Notify 的登记时序

- 日期与时区：2026-10-19 UTC
- 状态：Mitigated（旧通知实现由确定性回归用例稳定复现失败，修复版本及默认 Rust 库测试通过；新 SHA CI 待完成）
- 严重度：Low
- 模块：Rust / Profile refresh tests
- master commit / ref gitlink SHA / 平台与测试环境：PR #494 前一版 head `16b03b37c9a8c7021508ddecfb704fbc0dd68fae`；ref gitlink `53b004ccb3508d0673b6cb691be65e0e48bdd593`；GitHub Actions Windows。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #494](https://github.com/MFSGA/Chimera/pull/494)，[Windows backend job 118216586922 in run 39296098353](https://github.com/MFSGA/Chimera/actions/runs/39296098353/job/118216586922)。Run 后来被新 SHA 取消；该 job 已结束并明确失败：`caller_aborted_refresh_eventually_settles_and_does_not_leave_pending_stuck` 在 2 秒期限内未结束，后端工作区 587 项通过、1 项失败、1 项忽略。
- 预期行为：取消 refresh 的等待方后，后台 fetch 仍应结束，actor 清除 `pending_refresh`，后续 refresh 能成功。
- 真实异常、复现步骤与证据：Windows job `118216586922` 的失败日志显示该用例在 2 秒期限内未完成。旧用例在收到 fetcher 的 `started` oneshot 后调用 `release_fetch.notify_waiters()`，但 `started` 只表示已发送信号，不能证明 waiter 已登记。因此历史 CI 超时与丢失通知相符，但没有证明它就是该次失败的实际根因。
- 确认的根因与排除项：Tokio 1.53.3 的 `notify_waiters()` 不为未来的 `notified().await` 保存 permit；`notify_one()` 会保留一个 permit。旧测试没有控制通知与登记的先后顺序，无法稳定覆盖此语义。新测试增加独立的 `allow_wait` 屏障，明确先发出 release，再允许 fetcher 开始等待；这会确定性检查测试同步语义。该问题位于测试屏障，不涉及 production actor 路径。
- 修复提交、差异映射及保留的兼容性边界：只调整 Chimera `backend/tauri/src/client/profiles.rs::caller_aborted_refresh_eventually_settles_and_does_not_leave_pending_stuck`，增加登记前屏障并将 release 改为 `notify_one()`；不改 refresh actor 或 production behavior。ref `backend/tauri/src/client/profiles.rs` commit `53b004ccb3508d0673b6cb691be65e0e48bdd593` 采用旧测试顺序，后续同步时需重新检查该用例。
- 原实现失败、新实现通过的回归测试，及测试注册位置：在 `backend/tauri/src/client/profiles.rs` 的 Rust 单元测试中，用 `notify_waiters()` 临时替代 release 后，同一个 `--exact` 用例稳定在 2 秒期限失败；恢复 `notify_one()` 后同一用例 1/1 通过。通知和 waiter 注册顺序由 `allow_wait` oneshot 屏障控制。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：Cargo.lock 锁定 Tokio `1.53.3`。定向命令 `cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib client::profiles::tests::caller_aborted_refresh_eventually_settles_and_does_not_leave_pending_stuck -- --exact --nocapture` 在修复版本 1/1 通过；默认 `cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib --quiet` 为 577 passed、0 failed、1 ignored。尝试 `--all-features` 时因本机磁盘空间不足中断；Windows 新 SHA workflow 待完成，历史 CI 失败本身不能证明根因。
- 风险/回滚和清理证据（如涉及宿主状态）：测试同步调整，不改变应用运行或用户数据；回滚为恢复该用例中的 `notify_waiters()`。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：异步测试若依赖事件先后，使用独立屏障明确控制 waiter 注册前后的顺序；一次性 release 使用 `notify_one()`，广播给已登记的一组 waiters 才使用 `notify_waiters()`。未改 CI 门禁。

### LESSON-20261019-007：推送前核对 Git 写入凭据

- 日期与时区：2026-10-19 UTC
- 状态：Mitigated（错误 SSH 通道被拒后，使用活动 GitHub CLI 凭据通过 HTTPS 推送成功）
- 严重度：Low
- 模块：Git / Delivery
- master commit / ref gitlink SHA / 平台与测试环境：PR #494 head `74b6b31d0214bb111637ef1091b69c18a8621f62`；macOS。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #494](https://github.com/MFSGA/Chimera/pull/494)。
- 预期行为：远程写操作由当前已授权的维护者凭据执行，提交签名可由 GitHub 验证。
- 真实异常、复现步骤与证据：首次通过配置的 SSH push URL 推送时，GitHub 拒绝当前 SSH key；同一时刻 GitHub CLI 的活动账号有该仓库权限。随后通过一次性 GitHub CLI credential helper 的 HTTPS push 成功将提交推送到 PR #494；未将令牌写入命令参数或修改全局 Git 配置。
- 确认的根因与排除项：Git SSH 和 GitHub CLI 使用不同凭据来源；活动 CLI 会话正确，并不代表默认 SSH key 对目标仓库有写权限。
- 修复提交、差异映射及保留的兼容性边界：没有产品代码变更。此环境后续远端写操作使用 GitHub CLI credential helper 的 HTTPS 通道，除非先确认 SSH 身份有正确仓库权限。
- 原实现失败、新实现通过的回归测试，及测试注册位置：不适用；这是 Git 凭据选择问题，没有产品测试。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：本地签名校验通过；GitHub API 对提交 `74b6b31d0214bb111637ef1091b69c18a8621f62` 返回 `verified: true`、`reason: valid`；HTTPS push 成功并确认 PR head 与本地 SHA 一致。预防检查为写操作前确认活动 CLI 身份与目标仓库权限，并检查实际 push URL；没有自动化凭据测试。
- 风险/回滚和清理证据（如涉及宿主状态）：只影响该次 Git push；没有修改凭据、系统网络或全局配置。推送结果可通过后续新提交撤销，不应重写共享历史。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：执行远端写操作前核对 push URL 与 `gh auth status`；若 SSH 身份无法确认，则显式使用活动 GitHub CLI credential helper。诊断记录不得保存或输出令牌与非必要账号信息。

### LESSON-20261020-001：语言 E2E 必须独立准备并一致地恢复每个 WebView 的 locale cache

- 日期与时区：2026-10-20 UTC
- 状态：Open（语言用例及无标题 `/providers` 预检在本机复验通过；Windows hermetic 新 SHA 与 Dashboard recovery 仍待复验）
- 严重度：Low
- 模块：E2E / Localization
- master commit / ref gitlink SHA / 平台与测试环境：master `d9624314486e45c905876242dee5d3aacd161cba`；ref gitlink `ae0db596e96ba09d7662cc7533a13ffe5b581bf7`（本测试修复未修改或依赖 ref 业务实现）；GitHub Actions Windows Edge 155。
- 关联 Issue / PR / CI / 运行日志（脱敏）：PR #501 的 [Windows hermetic run 39421543438](https://github.com/MFSGA/Chimera/actions/runs/39421543438)；PR #515 的 [Windows hermetic run 39574156754](https://github.com/MFSGA/Chimera/actions/runs/39574156754) 与 [run 39582677624](https://github.com/MFSGA/Chimera/actions/runs/39582677624)。
- 预期行为：语言用例在主窗口和 legacy 窗口都以配置语言 `en` 及相同 locale cache 开始；结束时恢复原配置、缓存语义、有效页面语言、路由和尺寸。
- 真实异常、复现步骤与证据：`user-interface-language.e2e.ts` 的 `before` 曾在 legacy 窗口等待语言同步 30 秒后失败，状态为 `configured: en`、`cachedLocale: zh-cn`、`cacheSynchronized: false`、`language: en`；setup 清理只能走 fallback。2026-10-21 WebKit 复验还记录到刷新期间 `get_app_config` 脚本超时，而 DOM 已返回英语。Windows runs `39574156754`、`39582677624` 分别暴露主窗口 locale 清理失败和 ArrowDown 焦点断言失败。曾尝试用页面槽位识别路由，但完整槽位会随 Dashboard/Profiles 异步数据变化而造成清理误报。setup 标记复验发现主窗口查询错误地搜索 `data-slot="app-content"` 的子节点；修正后 legacy 唯一 route ID `/(legacy)/proxies` 仍被透明度为 0 的容器过滤。一次 WebKit 语言断言通过，但清理因路由链不一致失败；随后定位到主窗口先以 `/main` 启动并重定向至 `/main/dashboard`，用例在重定向完成前保存了 URL，清理再次打开 `/main` 后观察到 350ms 转场中的旧、新路由同时存在。两次相同快照只相隔 WDIO 默认 100ms，也不足以排除转场中的路由树。独立复核还发现只看外层 outlet 的全局 leaf ID 会漏掉内层路由过渡及动态 UID。2026-10-21 新增无标题 `/providers` 预检首轮通过修改 localStorage 后再访问根路径，实际仍落回 `/proxies`，没有验证目标路由；这是测试准备与应用内存路由竞争，不是产品路由失败。
- 确认的根因与排除项：原 setup 缺少每个 WebView 的 locale cache 显式准备，是跨用例隔离缺口。菜单用例此前未等已选英文项获得焦点，也未报告日语项自己的状态。测试路由恢复曾用 `history.replaceState` 加 `popstate` 手动改 URL，导致地址栏与 TanStack Router 实际 outlet 树不能作为同一状态验证；清理改用完整导航后仍因保存了重定向前的 `/main` 而重复触发重定向。捕获只要求两次、100ms 间隔的相同结果，短于 350ms 转场，不能证明路由树已稳定。其他 marker 缺口包括错误的父子选择器、透明度门槛和只读外层全局 leaf ID。2026-10-21 复验还发现 WebDriver 会重排返回对象属性，直接序列化造成同一路由被误判为变化；另有同步脚本超时。独立复核发现合法的 `/providers` legacy 页面没有 `<h1>`，且 setup 未捕获路由快照时 cleanup 可能跳过路由核验；修复分别保留空标题标记，并在无快照时重新稳定采样。标记现改为显式字段签名，并在 WebView 内部采样。Windows 上述失败都未证明语言切换逻辑损坏。
- 修复提交、差异映射及保留的兼容性边界：逐窗口按原配置、支持的 cache、document language 的优先级解析有效 locale；设置英语后逐窗口写 cache、刷新并等待 DOM/cache 收敛。清理时恢复配置和 locale，再导航回路由稳定后的有效 URL 并比较所有动画 outlet 标记；若 setup 未能保存路由标记，则重新导航并稳定采样，不能静默跳过验证。每个动画 outlet 标记其 transition key 对应的直接子路由 ID、确定性 32 位 FNV-1a selector hash 和 Motion `isPresent` 状态；动态 route match 可区分，原始参数不写入 DOM（该 hash 不作为加密隐私边界）。捕获要求完整标记（含可空的 legacy 标题）连续稳定至少 750ms，且不含 exiting child。普通非动画 TanStack Outlet 不在这些标记范围内，契约据此限定覆盖范围。菜单在 ArrowDown 前等待已选英文项获得焦点，失败诊断读取日语菜单项本身。无标题预检现在直接导航至 legacy `/providers`，断言路由 ID `/(legacy)/providers`，并在清理中恢复、读回原始 `memorizedRoutePathAtom`。未改产品 IPC、配置格式或宿主设置。
- 原实现失败、新实现通过的回归测试，及测试注册位置：原 Windows 失败见 runs `39421543438`、`39574156754` 和 `39582677624`。当前语言 spec 仍注册于 `settings` / Windows `hermetic`；750ms 稳定等待和重定向后 URL 版本本机 WebKit 通过 1/1（35 秒），加入 Motion `isPresent`、WebView 稳定采样及可空 legacy 标题后也通过 1/1（36.2 秒，一次 WebDriver 同步超时自动重试）；setup 无路由快照时的 cleanup 恢复现待新本机运行与 Windows E2E 验证。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：此前路由槽位版语言用例通过 1/1（32.8 秒）；route-ID 版曾因选择器、透明度、清理重定向及过短稳定窗口分别失败。加入 750ms 稳定窗口与有效 URL 后，本机语言用例曾通过 1/1（35 秒）；后续发现 WebDriver 属性顺序会造成伪变化，并出现同步脚本超时。改为显式字段签名、允许无标题 legacy 路由并在 WebView 内部 100ms 采样后，macOS WebKit 曾通过 1/1（35.3 秒，期间一次可恢复 WebDriver 同步脚本超时重试）。新无标题预检首版未能验证 `/providers`，因为改 localStorage 后根路由仍回到 `/proxies`；改为直接导航并断言 `/(legacy)/providers` 后，最终定向命令 `pnpm --filter @chimera/tauri-e2e exec wdio run ./wdio.conf.ts --spec ./specs/user-interface-language.e2e.ts` 通过 1/1（1 分 7 秒，期间一次可恢复 WebDriver 超时重试）。路由存储恢复已在测试清理中读回验证。setup 未能保存路由快照时的 fallback 仍未单独触发。E2E typecheck、Oxlint、Prettier、`git diff --check` 和 harness 单测 25/25 通过。Dashboard WebKit 定向用例最新运行在测试体前因 `waitForMainRouteSettled` 超时：诊断观察到 `/main/dashboard` 下两个 `app-content` 动画分支（`/(main)/main/` opacity 1、Dashboard opacity 0），以及一条 `currentTime=0`、时长 200ms 的活动动画；Popover recovery 断言未覆盖。Windows run `39582677624` 在这些修改前于焦点断言失败；新 SHA Windows hermetic E2E 待复验。`pnpm prepare:check` 与 `pnpm e2e:tauri:build` 曾通过；未验证冷启动或实际 glyph 覆盖。
- 风险/回滚和清理证据（如涉及宿主状态）：只改 E2E 准备、焦点/路由清理与错误诊断；cleanup 优先按恢复后的持久配置确定有效 locale，若保存 cache 与显式配置冲突，则归一化 E2E 派生 cache，而不是原样保留冲突值；不改持久化格式或宿主设置。回滚为还原该 spec 和契约。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：同一套件中各 spec 可写自己的 locale cache；共享 WebView 的后续测试须显式准备目标语言并验证恢复。无 CI 门禁改动。

### LESSON-20261020-002：Dashboard E2E 应按 Popover 实际打开状态操作

- 日期与时区：2026-10-20 UTC
- 状态：Open（状态感知修复已写入，Windows hermetic 新 SHA 复验待完成）
- 严重度：Low
- 模块：E2E / Dashboard
- master commit / ref gitlink SHA / 平台与测试环境：master `d9624314486e45c905876242dee5d3aacd161cba`；ref gitlink `ae0db596e96ba09d7662cc7533a13ffe5b581bf7`（本测试修复未修改或依赖 ref 业务实现）；GitHub Actions Windows Edge 155。
- 关联 Issue / PR / CI / 运行日志（脱敏）：PR #501 的 [Windows hermetic run 39421543438](https://github.com/MFSGA/Chimera/actions/runs/39421543438)；PR #515 的[后续 Windows hermetic run 39574156754](https://github.com/MFSGA/Chimera/actions/runs/39574156754)。
- 预期行为：在校验失败配置下，Traffic widget 的 Popover 已打开，unit 与 Reset 控件都已显示，并且二者按约定禁用。
- 真实异常、复现步骤与证据：run `39421543438` 中用例直接点击会切换开/关的 widget trigger，随后只等待 unit 元素可见，没有确认 trigger 的 `aria-expanded` 和 Reset 的挂载状态；截图显示 Popover 已关闭。run `39574156754` 中状态感知辅助函数仍在未知版本 storage error 后报告 unit 控件不可见；失败前采集的 HTML/截图显示 Dashboard 在编辑模式、header 保留 `load-error`，trigger 最终为 `aria-expanded="false"`，Popover 内容未挂载。run `39582677624` 的 before-cleanup artifact 同样显示编辑模式和 `load-error`，但 trigger 已为 `aria-expanded="false"`，speed-unit 与 Popover fields 均未挂载；`ensureTrafficConfigOpen` 的打开状态前置条件在失败前已满足，状态/portal 随后关闭或被重置，artifact 不含点击时序，尚不能区分产品状态变化与测试驱动交互。2026-10-21 本机 WebKit 复跑两次均在测试体开始前因 `main-window.ts::waitForMainRouteSettled` 超时，未到达 Popover 断言。新增诊断确认 `/main/dashboard` 下同时有 `/(main)/main/` 父路由与 Dashboard 动画分支，后者 opacity 为 0；一条 Web Animation 仍为 `running`、`currentTime=0`、时长 200ms。该观察记录了动画未推进，但尚未确定本机 WebKit 的原因。
- 确认的根因与排除项：run `39421543438` 证明第一次失败由用例在状态未知时无条件切换 Popover 导致；run `39574156754` 证明初始状态检查并等待内容仍未消除该失败。故障注入的 storage 错误确实到达且 UI 保留 `load-error`，较新失败的触发原因仍待 expanded-state 诊断和 Windows 复验。本机路由等待失败发生在 Popover 用例之前，不能据此判断产品恢复功能通过或失败。
- 修复提交、差异映射及保留的兼容性边界：打开辅助函数检查对应 trigger 的 `aria-expanded`，只在未打开时点击，随后先等待 `aria-expanded="true"`，再等待 unit 与 Reset 同时可见。此次仅提升测试状态证据，产品组件和持久化逻辑未改；若新 E2E 仍显示 trigger 未展开，再检查产品组件和用户交互，不以放宽后续断言处理。
- 原实现失败、新实现通过的回归测试，及测试注册位置：第一次状态感知修复在 `39574156754` 未通过；当前增加 expanded-state 等待的版本仍在 `dashboard-main-layout.e2e.ts` 并注册到 `main` / Windows `hermetic`。PR #507 的 run `39429486331` 对旧版同一 spec 通过，但不能代替当前改动的复验。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：run `39574156754` 的 53 个 spec 中 recovery 用例失败；run `39582677624` 的旧版修复仍因 unit 控件不可见而失败。本机 WebKit 曾通过定向 Dashboard 2/2（33.8 秒）；2026-10-21 最近两次运行均在测试体前由 `main-window.ts::waitForMainRouteSettled` 超时，Popover recovery 断言未覆盖。Windows 新 SHA hermetic E2E 待复验。
- 风险/回滚和清理证据（如涉及宿主状态）：仅改测试交互状态检查；用例仍恢复原 widget storage 并确认读回，不操作宿主网络。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：切换 Popover、菜单或对话框后，先等稳定的展开状态和目标控件可见，再断言内部操作。无 CI 门禁改动。

### LESSON-20261020-003：页面刷新后分别等待文档导航与应用挂载

- 日期与时区：2026-10-20 UTC
- 状态：Open（分阶段等待已写入，Windows hermetic 新 SHA 复验待完成）
- 严重度：Low
- 模块：E2E / WebDriver
- master commit / ref gitlink SHA / 平台与测试环境：master `d9624314486e45c905876242dee5d3aacd161cba`；ref gitlink `ae0db596e96ba09d7662cc7533a13ffe5b581bf7`（测试驱动边界，不涉及产品 ref 映射）；GitHub Actions Windows Edge 155。
- 关联 Issue / PR / CI / 运行日志（脱敏）：PR #507 的 [Windows hermetic run 39429486331](https://github.com/MFSGA/Chimera/actions/runs/39429486331)，artifact `application-typed-state`。
- 预期行为：refresh 完成后先确认新文档导航完成，再在独立期限内等待 Chimera 的 React 根挂载，随后验证 typed 配置及 YAML。
- 真实异常、复现步骤与证据：刷新调用后首个观察为 `readyState: loading`、root children 为 0；30 秒 render 等待以 root 为 0 失败。artifact 截图/HTML 随后显示 Dashboard 已渲染，失败清理成功且无前端错误捕获。
- 确认的根因与排除项：vendored `navigation::refresh` 调用的 executor 只执行 `window.location.reload(); null;`，没有等待新文档 load。测试先把导航与 React 根挂载放进一个 30 秒阶段；Windows WebDriver 在刷新过渡期的后续执行请求可跨越该阶段预算。独立复核还发现仅看 `readyState === "complete"` 可能接受刷新前旧文档的状态。证据没有显示配置 patch 或持久化丢失，也没有证明产品前端崩溃。
- 修复提交、差异映射及保留的兼容性边界：refresh 前记录 `performance.timeOrigin`，refresh 后要求 time origin 改变且 `document.readyState === "complete"`，证明进入新文档后再等待最多 60 秒应用 root 挂载。原始状态在约 0.6 秒后已报告 document complete，但 root 仍为空；失败截图显示挂载最终完成。将 Mocha 上限提高到 240 秒，以覆盖约 195 秒的阶段/诊断/清理预算并留出约 45 秒余量。无产品代码或驱动兼容分支改动。
- 原实现失败、新实现通过的回归测试，及测试注册位置：原失败见 run `39429486331`；spec 仍注册于 `settings` / Windows `hermetic`。新 SHA 结果待远端复验。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：旧 run 的 typed-state spec 失败，53 个 spec 中 52 个通过；原本只检查 readyState 的 macOS WebKit typed-state 用例 1/1 通过，但该结果不能验证刷新文档身份。增加 time origin 检查后的新 SHA 用例及 Windows hermetic E2E 待复验；此前 `pnpm --filter @chimera/tauri-e2e typecheck`、25 项 harness 单测、Prettier、`pnpm prepare:check`、`pnpm e2e:tauri:build` 通过。未验证应用冷启动。
- 风险/回滚和清理证据（如涉及宿主状态）：只改 E2E 等待阶段和其契约；原设置仍通过共享 API 恢复，持久化值仍被核验，无系统设置改动。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：在使用 vendored Tauri driver 的 refresh 用例中，应先证明 document identity 已改变并等待导航完成，再等待产品挂载标记；不以旧文档的 readyState 或固定 sleep 代替身份/阶段条件，并让 Mocha 总限时覆盖所有分阶段上限、诊断和 cleanup。工具行为已补充到 `docs/testing/upstream-evidence.md`。

### LESSON-20261021-001：治理规则修改须同步审查清单和 PR 模板

- 日期与时区：2026-10-21 UTC
- 状态：Mitigated（PR #515 合并前独立复核发现并修正）
- 严重度：Low
- 模块：Governance / Review
- master commit / ref gitlink SHA / 平台与测试环境：PR #515 基线 `2dca108525d855d08479fa23db71d9533352e727`；ref gitlink `ae0db596e96ba09d7662cc7533a13ffe5b581bf7`；文档审查
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #515](https://github.com/MFSGA/Chimera/pull/515)，独立复核发现 `docs/ai-review-checklist.md` 和 `.github/pull_request_template.md` 仍保留保护分支历史改写的绝对禁令。矛盾在合并前发现；未改写 master、标签或其他 PR 分支。
- 预期行为：治理规范、独立审查清单和 PR 模板对历史改写边界采用一致的精确授权规则。
- 真实异常、复现步骤与证据：本任务将 `AGENTS.md` 与治理规范更新为允许维护者明确授权范围内的历史重写，但第一次提交遗漏了审查清单及 PR 模板中的旧禁令；独立复核在 PR 合并前指出该不一致。
- 确认的根因与排除项：治理条款在多个文件重复维护，修改时未搜索所有受影响的规范文本；这是文档一致性流程遗漏，不涉及 Git 历史或代码安全问题。
- 修复提交、差异映射及保留的兼容性边界：审查清单及 PR 模板改为引用 `docs/ai-development-governance.md` 的精确授权范围，并要求记录目标引用、提交范围、标签和开放 PR 分支处理。未授权的引用仍保持原样。
- 原实现失败、新实现通过的回归测试，及测试注册位置：规范文本通过全仓搜索复核，没有额外自动测试；`LESSONS.md` 记录这一流程遗漏及预防措施。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：执行规范文本全仓搜索、Prettier 与 `git diff --check`；PR #515 的 CI 对修正后的新 SHA 待重新运行。
- 风险/回滚和清理证据（如涉及宿主状态）：仅修改治理说明；回滚为恢复相关文档。未变更分支保护、标签、远端历史或 CI 权限。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：修改治理规则前，搜索 `AGENTS.md`、`docs/ai-*`、`.github/**` 中重复规则，并核对 PR 模板与 review checklist；本次规则变更由维护者在会话中明确授权。

### LESSON-20261021-002：跨平台 E2E 不应把单次焦点或布局快照当成稳定结果

- 日期与时区：2026-10-21 UTC
- 状态：Open（PR #515 的新 Windows SHA 复验待完成）
- 严重度：Low
- 模块：E2E / WebDriver
- master commit / ref gitlink SHA / 平台与测试环境：PR #515 基线 `2dca108525d855d08479fa23db71d9533352e727`；ref gitlink `ae0db596e96ba09d7662cc7533a13ffe5b581bf7`；GitHub Actions Windows Edge 155 与本机 macOS WebKit 605.1.15。
- 关联 Issue / PR / CI / 运行日志（脱敏）：PR #515 的 [Windows hermetic run 39602392783](https://github.com/MFSGA/Chimera/actions/runs/39602392783)；失败 artifact 含语言菜单截图/HTML，以及 proxy node 截图/HTML。
- 预期行为：通过鼠标点击语言控件并点击日语选项后，语言切换并持久化；代理节点的 SOCKS5/UDP 标记在首屏可见，节点按钮有至少 48 CSS px 的实际高度。
- 真实异常、复现步骤与证据：Windows run 在语言菜单打开后观察到 `document.activeElement` 仍为 `role="menu"` 容器，未进入已选英文 checkbox，测试因前置焦点断言失败，尚未发送 ArrowDown。Proxy node 一次 element-scoped 读取返回 `buttonHeight: 0`；同一失败 artifact 的截图显示节点可见，HTML 中对应按钮有 `h-14` class。后置截图/HTML 不能单独证明失败时刻的实际尺寸，也不能确定是旧元素句柄、动画还是虚拟列表重排导致。
- 确认的根因与排除项：语言用例把 Radix 打开菜单后的内部焦点位置当成选语言的前置条件，导致 CI 在执行选择行为前失败；它没有证明日语选择或持久化存在问题。Proxy 节点的一次按钮尺寸读取为 `0`，稍后采集的截图/HTML 显示按钮可见且带 `h-14`；这些延后证据不能确定失败瞬间的几何状态，也不能单独证明源码布局错误。
- 修复提交、差异映射及保留的兼容性边界：ref gitlink `ae0db596e96ba09d7662cc7533a13ffe5b581bf7` 的 `frontend/nyanpasu/src/pages/(main)/main/settings/user-interface/_modules/language-selector.tsx` 使用相同的 `DropdownMenuContent` / `DropdownMenuCheckboxItem` 路径并依赖 Radix 默认行为。Chimera 移除额外的键盘焦点处理，使共享选择器回到该路径；E2E 改用 WebDriver 鼠标按下/抬起打开菜单并点击日语，继续验证配置文件、主 UI 刷新和 legacy UI。代理布局断言从当前 DOM 查找完全位于可见区域的 fixture 节点，并检查按钮与目标滚动 viewport、窗口边界及裁剪祖先的实际交集；保留 `>=48px` 尺寸要求。
- 原实现失败、新实现通过的回归测试，及测试注册位置：原失败见 run `39602392783`；两 spec 仍注册在现有 `hermetic` 套件。此前语言 spec 的一次本机通过使用程序化 `.focus()`，未验证产品自动焦点，不能作为本次修复证据；需用新源码重跑。Proxy node spec 在本机三次均于 before hook 创建 fixture 时失败，未进入布局断言。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：`pnpm --filter @chimera/tauri-e2e typecheck`、Oxlint、Prettier、`git diff --check` 和 harness unit 25/25 通过。本机 `pnpm e2e:tauri:build` 因磁盘仅剩 242 MiB，在 Rust 归档阶段以 `No space left on device` 退出；未能在本机运行桌面用例。Proxy 定向用例在本机之前三次均于 before hook 因隔离运行时默认端口 7890 不可用而失败，未覆盖本次几何断言；没有停止或更改正在运行的 Chimera 与 Clash Nyanpasu。新 SHA 鼠标 E2E 待复验；未验证真实原生键盘输入。
- 风险/回滚和清理证据（如涉及宿主状态）：产品侧只移除与 ref 不一致的菜单焦点处理；E2E 使用 W3C 鼠标动作，无持久配置、核心、系统网络或 Service 行为变化。回滚可恢复原焦点处理及旧的键盘断言。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：菜单选择回归直接点击可见语言控件和选项，再检查持久化结果；不要以 Radix 内部焦点快照代替用户所需的选择行为。布局回归需检查实际 scroll viewport 与裁剪后的尺寸，不以 CSS 类或延后的截图代替断言。鼠标 E2E 通过后关闭语言问题记录。

### LESSON-20261022-001：executeAsync 超时后必须停止页面内轮询

- 日期与时区：2026-10-22 UTC
- 状态：Open（修复已写入 PR #515；新 SHA 的 Windows hermetic E2E 待复验）
- 严重度：Low
- 模块：E2E / WebDriver
- PR head / ref gitlink SHA / 平台与测试环境：修复前 head `f159daf7eec8041edc3467c29af61785c95788d9`；master 同步到 `f5b2271b8ea177db285f8c97f9466896b4cc5bbf` 后的 timer 修复 head `3613059177571197e7fba2320129bfc033470bd5`；ref gitlink `ae0db596e96ba09d7662cc7533a13ffe5b581bf7`；macOS WebKit 605.1.15 与 GitHub Actions Windows Edge。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #515](https://github.com/MFSGA/Chimera/pull/515)；独立复核沿 `tauri-e2e/specs/user-interface-language.e2e.ts::waitForRouteMarker` 的 timeout 分支确认问题。旧 head 的 [Windows hermetic run 39616924154](https://github.com/MFSGA/Chimera/actions/runs/39616924154) 通过，但早于该修复。
- 预期行为：稳定路由采样成功或超时时，等待函数结束后都不再执行页面 DOM 采样。
- 真实异常、复现步骤与证据：原实现每次采样都用 `window.setTimeout(sample, 100)` 安排下一次读取；`finish(false)` 只清除截止 timer 并调用 WebDriver completion callback，未保存或清除 sample timer。若在稳定路由出现前超时，已有 sample timer 会在回调完成后继续重复读取 DOM。独立复核检查了仓库锁定版本的 WebKit `execute_async_script` 包装：完成 WebDriver callback 不会取消页面上下文 timer。
- 确认的根因与排除项：页面 timer 的生命周期独立于 WebDriver async callback；轮询回调还缺少 `finished` 终止检查，因此 timeout 后会继续安排新的采样。不是主 UI 路由状态或产品运行时故障。
- 修复提交、差异映射及保留的兼容性边界：`waitForRouteMarker` 保存 deadline 与 sample 两个 timer handle；`finish` 设置完成状态并清除两者，`sample` 入口在完成后立即返回。仅修改 E2E 辅助逻辑，不改产品 API、路由标记或持久化。
- 原实现失败、新实现通过的回归测试，及测试注册位置：该辅助逻辑供 `user-interface-language.e2e.ts` setup/cleanup 使用，并在现有 hermetic suite 注册。新 SHA Windows E2E 待复验；本机单独串行运行此 spec 通过 1/1，报告数条 WebDriver 可恢复脚本超时；此前与独立复核并行运行的本机尝试因共享桌面 session 冲突，在 before hook 失败，不作为验证证据。当前没有专门强制 timeout 分支的 E2E 断言。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：`pnpm --filter @chimera/tauri-e2e typecheck` 通过；`pnpm --filter @chimera/tauri-e2e test:unit` 25/25 通过；目标 spec `pnpm --filter @chimera/tauri-e2e exec wdio run ./wdio.conf.ts --spec ./specs/user-interface-language.e2e.ts --logLevel warn` 单独串行运行通过 1/1；Prettier 与 `git diff --check` 通过。一次 `pnpm prepare:check` 在资源缓存完成后无进度，手动中断，不能算通过。修复后的 Windows E2E 尚未运行。
- 风险/回滚和清理证据（如涉及宿主状态）：仅 E2E 等待器的 timer 清理；无产品配置、宿主网络或系统状态变化。回滚为恢复路由 helper 原有的 deadline-only 清理。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：使用页面轮询的 `executeAsync` helper 必须自行跟踪并清除 deadline 和循环 sample timer，且每个异步回调应检查完成状态；不能假定 WebDriver callback 完成会结束页面内 timer。无 CI 门禁变更。

### LESSON-20261023-001：Linux ARM64 cross-build 必须使用带 GTK/GLib 的目标镜像

- 日期与时区：2026-10-23 UTC
- 状态：Verified（修复后的专用 Linux ARM64 cross-build 已通过；通用 PR CI 在同步后的新 SHA 上复验中）
- 严重度：Low
- 模块：CI / Linux cross-build
- master commit / ref gitlink SHA / 平台与测试环境：失败运行基线 `2dca108525d855d08479fa23db71d9533352e727`；ref gitlink `ae0db596e96ba09d7662cc7533a13ffe5b581bf7`；Ubuntu 26.04 aarch64 cross build。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[夜间构建失败 39545347771](https://github.com/MFSGA/Chimera/actions/runs/39545347771)，失败 Job 为 `Linux aarch64 Build / build`；修复后的[专用 Linux ARM64 构建 run 39623084868](https://github.com/MFSGA/Chimera/actions/runs/39623084868)成功。
- 预期行为：Linux ARM64 Tauri cross-build 应在目标环境中找到 Tauri 所需的 GLib/GTK 开发文件并完成编译。
- 真实异常、复现步骤与证据：`glib-sys v0.18.1` 的 build script 在 `cross` 容器内运行 `pkg-config --libs --cflags glib-2.0 'glib-2.0 >= 2.70'` 失败；容器的 `/usr/lib/aarch64-linux-gnu/pkgconfig/` 中没有 `glib-2.0.pc`。同一 job 的 Ubuntu host 仅安装 GTK/WebKit host 包，无法提供给隔离的目标容器。
- 确认的根因与排除项：仓库没有 `backend/Cross.toml`，因此 aarch64 构建落到 `cross` 默认镜像；该镜像不包含项目需要的 ARM64 GLib 开发包。不是 Rust 源码或依赖版本错误，也不是 runner host 缺少包。
- 修复提交、差异映射及保留的兼容性边界：补回 pinned ref `backend/Cross.toml` 的 aarch64 target 镜像映射，使用 `ghcr.io/libnyanpasu/builder-debian-trixie-aarch64:latest`；其他 target 仍使用 cross 默认配置。没有修改应用代码或产物发布步骤。
- 原实现失败、新实现通过的回归测试，及测试注册位置：失败证据为 run `39545347771`。修复后的专用 run `39623084868` 在 `aarch64` 目标完成 Tauri cross build，并成功生成 deb/rpm；容器中的 GLib 开发依赖因此得到实际构建验证。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：`yq -p toml '.target."aarch64-unknown-linux-gnu".image' backend/Cross.toml` 输出预期镜像；将 pinned ref 的文件与 `backend/Cross.toml` 比对，`cmp` 通过；专用 Linux ARM64 run `39623084868` 成功完成 `Tauri build and upload (cross)` 并生成 deb/rpm。本机没有 Docker，目标容器验证由该 GitHub Actions run 完成；同步后新 SHA 的通用 CI 仍待复验。
- 风险/回滚和清理证据（如涉及宿主状态）：只影响 ARM64 cross-build 的构建容器镜像选择；回滚方式为移除 `backend/Cross.toml`。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：Linux cross-build 所需的 target 系统库必须安装在 cross 容器内，host 的 `apt-get install` 不能替代；优先保持与 pinned ref 的 `backend/Cross.toml` 对齐。此修复未改变 CI 门禁或发布/tag 流程。

### LESSON-20261023-002：非夜间 cross 构建的 Tauri JSON 参数必须保留引号

- 日期与时区：2026-10-23 UTC
- 状态：Verified（专用 Linux ARM64 构建已在 nightly=false 下通过；同步后的通用 PR CI 仍待完成）
- 严重度：Low
- 模块：CI / Linux cross-build
- master commit / ref gitlink SHA / 平台与测试环境：PR #518 基线 `de251518084d5bf2a6af9e1f437538f8ce41a4f7`；ref gitlink `ae0db596e96ba09d7662cc7533a13ffe5b581bf7`；GitHub Ubuntu 26.04 aarch64。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[定向构建失败 run 39621617941](https://github.com/MFSGA/Chimera/actions/runs/39621617941)；修复后的[定向构建 run 39623084868](https://github.com/MFSGA/Chimera/actions/runs/39623084868)成功。
- 预期行为：`nightly=false` 的 Cross 构建应将 `{"bundle":{"createUpdaterArtifacts":false}}` 作为有效 JSON 传给 Tauri `--config`。
- 真实异常、复现步骤与证据：run `39621617941` 在 `Tauri build and upload (cross)` 退出，Tauri 收到 `{ bundle: { createUpdaterArtifacts: false } }` 并报错 `key must be a string`；尚未进入 Rust cross 编译。
- 确认的根因与排除项：GitHub expression 字符串里嵌套双引号和 shell 双引号，渲染后 shell 移除了 JSON key 引号；与 Cross 镜像或 Rust 依赖无关。
- 修复提交、差异映射及保留的兼容性边界：在 `.github/workflows/deps-build-linux.yaml` 将目标三元组映射和 nightly 分支移到 Bash，并以单引号传入 compact JSON；夜间命令保持原构建模式。
- 原实现失败、新实现通过的回归测试，及测试注册位置：原实现失败证据为 run `39621617941`。修复后的 run `39623084868` 以 `nightly=false`、`arch=aarch64` 成功完成 `Tauri build and upload (cross)`，确认有效 JSON 参数并实际进入 Cross 容器。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：提取该步骤并以 `NIGHTLY=false`、aarch64 输入和 `pnpm` stub 执行，确认传入目标三元组及完整 JSON 参数；四架构 × 两种 nightly 模式参数矩阵 8/8 通过；`bash -n` 和 `actionlint -ignore 'label "ubuntu-26.04" is unknown' -ignore 'SC2086' -ignore 'SC2162' .github/workflows/deps-build-linux.yaml` 通过。未忽略诊断的全量 actionlint 仍报告原有 ubuntu-26.04 runner label、两项 SC2086 和一项 SC2162，修改步骤本身无诊断。Prettier 与 `git diff --check` 通过；专用 ARM64 run `39623084868` 通过。同步后新 SHA 的通用 PR CI 仍待完成。
- 风险/回滚和清理证据（如涉及宿主状态）：只改变单次 Linux cross 构建命令参数；回滚时恢复原来的 per-architecture expression 命令并移除此记录。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：避免在 GitHub expression 字符串中嵌套复杂 shell 引号；把条件分支放进 Bash，并让 JSON 参数在 shell 中使用单引号保持原样。

### LESSON-20261023-003：隐藏的 WebKit 窗口不能验证 Legacy 连接页交互

- 日期与时区：2026-10-23 UTC
- 状态：Open（本次桌面用例在交互断言前被可见性前置检查拒绝；单击详情行为仍需在前台桌面验证）
- 严重度：Low
- 模块：E2E / macOS / Legacy Connections
- master commit / ref gitlink SHA / 平台与测试环境：`eb5001a4729e6df47090a261892aac5623982c3f`；项目记录的 ref gitlink `ae0db596e96ba09d7662cc7533a13ffe5b581bf7`，本机只读 ref checkout `4eaf13b9c5dc468fdfc38272bf76b20a681d2e54`；macOS WebKit 605.1.15 嵌入式 WebDriver。
- 关联 Issue / PR / CI / 运行日志（脱敏）：本地执行 `pnpm --filter @chimera/tauri-e2e exec wdio run ./wdio.conf.ts --spec ./specs/connections-legacy-layout.e2e.ts`；WDIO session `37530cf2-72fd-4d97-aed6-4b64bec55a27`，spec 在 `beforeEach` 的页面可见性检查失败。
- 预期行为：页面可见且路由动画完成后，Legacy Connections 的 UI E2E 才开始断言布局和交互；隐藏 WebView 必须明确报告为环境阻塞，不得当作产品回归或通过结果。
- 真实异常、复现步骤与证据：spec 日志观察到 `document.visibilityState=hidden`、`document.hasFocus()=false`、`.page-transition` 的 `opacity=0`，两个 Motion animation 的 `currentTime=0`；`beforeEach` 等待连接搜索框可见超时，最终 `0` 个 spec 通过，连接详情操作没有执行。
- 确认的根因与排除项：嵌入式 WebView 没有成为可见前台窗口，现有页面动画因此未推进。不是详情弹窗断言失败，也不能据此判断单击打开详情的产品逻辑错误。
- 修复提交、差异映射及保留的兼容性边界：没有修改测试等待条件、页面透明度或 WebDriver 输入路径来绕过前置检查。现有 `connections-legacy-layout.e2e.ts` 保留可见性和动画诊断；需要前台桌面会话验证视觉及点击交互。
- 原实现失败、新实现通过的回归测试，及测试注册位置：没有新的产品回归测试结论。本次 spec 在 `beforeEach` 中止，详情单击交互未覆盖；现有 ref 浏览器测试验证的是双击，不代替 Legacy 单击验证。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：`pnpm e2e:tauri:build` 成功；目标 WDIO spec 失败在可见性前置条件，`0` 个 spec 完成。前端 typecheck、build、连接模型测试及边界检查另见本任务验证记录。
- 风险/回滚和清理证据（如涉及宿主状态）：E2E binary 使用隔离 runtime；本次只读 UI 流程未修改用户配置或宿主网络。无需产品回滚。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：docs/ref-alignment-connections.md 已记录此前相同的隐藏 WebView 限制；运行前台桌面 E2E 前继续保留 fail-closed 可见性检查，只有当检查通过并实际执行目标交互后才报告交互已验证。无 CI 门禁调整。

### LESSON-20261025-001：Release 预检必须有权限查看草稿

- 日期与时区：首次发现 2026-10-25 UTC；根因复核 2026-10-26 UTC
- 状态：Verified（master 全平台 CI、v0.27.1 发布构建及 updater manifest 重生成均通过）
- 严重度：Medium（会阻断全部发行构建，但不会错误发布草稿）
- 模块：CI / Release
- master CI 验证基线 / 发布源 SHA / ref gitlink SHA / 平台：`c057b479136e5fc1c655627758f23ce9efd196d9` / `82892bb62598a342628bb0c492abecb7aaa4b3e6` / `eb81fda5dbbe6382c31875fc2d5709429056c279`；GitHub Actions Ubuntu、Windows、macOS runners
- 关联 Issue / PR / CI / 运行日志（脱敏）：[初始失败 run 40313224403（tag SHA `00036e8`）](https://github.com/MFSGA/Chimera/actions/runs/40313224403)、[master CI run 40386437907](https://github.com/MFSGA/Chimera/actions/runs/40386437907)、[v0.27.1 发布 run 40371994350（tag SHA `82892bb`）](https://github.com/MFSGA/Chimera/actions/runs/40371994350)、[updater manifest 重生成 run 40407974104](https://github.com/MFSGA/Chimera/actions/runs/40407974104)。初始 12 次 404 导致发布未启动；修复后发布已成功。草稿标签在发布前从 `00036e8` 移至 `82892bb`。
- 预期行为：预检 token 能读取并确认指定 tag 对应的 Release 仍为 draft；已发布 Release、权限/API 错误和超时仍阻止构建。
- 真实异常、复现步骤与证据：Publish run 40313124461 创建 v0.27.1 草稿并派发目标构建。目标 run 40313224403 在 110 秒内连续 12 次调用 `gh release view v0.27.1` 均收到 404，随后失败。用维护者 CLI 查询同一草稿可见，说明延长等待本身不足以修复 Actions 预检。
- 确认的根因与排除项：`.github/workflows/target-release-build.yaml` 为 `release_preflight` 设置了 `contents: read`。GitHub 文档说明 draft releases 只会列给有推送权限的身份；只读 `GITHUB_TOKEN` 无法发现该 draft，API 以 404 隐藏资源。此前仅按一致性延迟解释 404 的结论不完整。故障发生在所有平台构建启动前，不是打包、签名、上传或 updater 生成导致。
- 修复提交、差异映射及保留的兼容性边界：`96e89e93` 让 `release_preflight` 能读取草稿；其他构建 job 保持各自权限。保留 helper 的有界 404 重试及“必须仍为 draft”的 fail-closed 检查。随后 `bbb009d9` 增加按精确 tag 查找 Release 的列表回退，`82892bb6` 通过认证的 Release Asset API 取草稿签名。新增 Deno 合约断言，防止预检权限回退为只读。经维护者批准，发布前将草稿 tag 从旧源提交移动到最终修复提交；发布成功后不再重标 tag。
- 原实现失败、新实现通过的回归测试，及测试注册位置：真实旧行为在 v0.27.1 run 40313224403 中失败；`scripts/src/ci/release-preflight_test.ts` 覆盖延迟可见、已发布、其他 API 错误、持续 404、无效 tag，并断言发布工作流给 preflight 授予 `contents: write`。测试通过现有 `pnpm test:release-preflight` 注册在 Ubuntu scripts CI job。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：本地 `pnpm test:release-preflight` 6/6 通过；`pnpm test:updater-manifest` 19/19 通过；`actionlint`、Prettier 和 `git diff --check` 通过。GitHub master CI `40386437907` 的 Changes、三平台 lint 与三平台 Tauri 构建全通过；发布 run `40371994350` 成功发布 v0.27.1；updater run `40407974104` 成功重生成清单。草稿可见性、签名解析和发布构建均在真实 GitHub 运行中验证。
- 风险/回滚和清理证据（如涉及宿主状态）：只扩大预检 job 的 repository contents 权限，以便读取 draft；不改变其他 job 的权限。发布前经授权移动草稿 tag；发布后不再重标 tag。回滚代码会重新引入草稿不可见或 updater 取不到签名的问题，不应回滚已发布 tag。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：workflow permission 与 Deno 合约测试共同锁定“preflight 可见 draft”；该 L3 修复由维护者授权。v0.27.1 发布运行使用已修复的 master 源码，后续 updater manifest 单独重生成并核验。

### LESSON-20261026-001：Alpha 版本必须从 rolling 发布资产解析

- 日期与时区：2026-10-26 UTC
- 状态：Verified（master 全平台 CI 与 v0.27.1 发布构建通过）
- 严重度：Medium（macOS 资源准备失败会阻断整轮 CI 与发布）
- 模块：CI / 依赖资源 / Chimera Client Alpha
- master commit / ref gitlink SHA / 平台与测试环境：基线 `96e89e9325aedea16df3ea7b26f2205367e8f5eb`；GitHub Actions macOS 26 arm64 runner
- 关联 Issue / PR / CI / 运行日志（脱敏）：[CI run 40315139018](https://github.com/MFSGA/Chimera/actions/runs/40315139018) 在 macOS `Prepare sidecar and resources` 失败；上游 [Chimera Client Alpha Release run 40311215709](https://github.com/MFSGA/Chimera_Client/actions/runs/40311215709) 已成功完成。
- 预期行为：Alpha 版本号与 `latest` rolling Release 实际提供的资产版本一致；稳定版可独立前进，不能据此构造 rolling 资产文件名。
- 真实异常、复现步骤与证据：`resolveChimeraClientAlpha()` 将 `latest` tag 的 commit `414335830e8d1ebb3a1b612a8702b83c51b40000` 与 `releases/latest` 的稳定版本 `v0.28.1` 拼成 `0.28.1-alpha+sha.4143358`，随后 macOS 解析器在 rolling `latest` URL 请求 `chimera-client-0.28.1-aarch64-apple-darwin.tar.gz` 并收到 404。上游 `latest` Alpha Release 的实际 macOS 资产仍为 `0.28.0`；稳定版 `v0.28.1` 的独立资产存在，但其 commit 为 `745094dd9baf13ba5c921d534df8cdb0fe97619a`，不能拿来冒充 Alpha commit 的构建。
- 确认的根因与排除项：Alpha manifest 从当前稳定 Release 推导 macOS 资产版本，却从 rolling `latest` tag 推导 Alpha SHA；两个发布流推进不同步，产生不存在的资产 URL。下载重试多次刷新到同一个 Alpha Release，无法修复版本来源不匹配。
- 修复提交、差异映射及保留的兼容性边界：改为查询 `releases/tags/latest` 的实际资产列表，从 versioned macOS archive 文件名解析基础版本，再与 `latest` tag commit SHA 组合 Alpha 版本。仍从 `latest` 下载 Alpha 资产，不回退到不同 commit 的稳定版二进制。
- 原实现失败、新实现通过的回归测试，及测试注册位置：`scripts/src/prepare/versions_test.ts` 覆盖资产版本解析；`chimera-client-alpha_test.ts` 通过注入 GitHub 请求断言调用 `git/ref/tags/latest` 与 `releases/tags/latest`，拒绝意外的 `/releases/latest` 请求，并确认 Alpha 版本取自 rolling 资产。两者通过现有 `pnpm test:manifest` 注册执行。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：本地 `pnpm test:manifest` 30/30、Deno 类型检查、Prettier、`actionlint` 和 `git diff --check` 通过。master CI run `40386437907` 的三平台 Tauri 构建通过，v0.27.1 发布 run `40371994350` 成功，验证修复后的 Alpha 资源解析可用于真实打包。此前 rolling URL 的 0.28.0 arm64 archive 返回 HTTP 200，错误的 0.28.1 URL 返回 HTTP 404。
- 风险/回滚和清理证据（如涉及宿主状态）：只调整 Alpha 版本元数据的来源；若 rolling Release 没有可解析的 versioned macOS archive，解析明确失败，不下载错误版本。回滚为恢复稳定版版本推导，但会重新引入该 404。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：保持版本字符串与实际 rolling 资产联动，并用资产清单回归测试覆盖稳定版和 Alpha 资产异步发布；本次 L2 sidecar 修复由维护者在当前“发布并修复至成功”任务中授权。

### LESSON-20261026-002：Alpha 更新器必须从 Alpha 版本生成资产名

- 日期与时区：2026-10-26 UTC
- 状态：Open（Rust 回归测试已添加；新 SHA 的远程 CI、E2E 与发布构建待验证）
- 严重度：Medium（Alpha 更新请求错误资产名时会返回 404）
- 模块：应用更新器 / Chimera Client Alpha
- master commit / ref gitlink SHA / 平台与测试环境：发现基线 `a8ebde3d2f29abdc8f9bfed5097e4cbda56b9429`；ref `4eaf13b9c5dc468fdfc38272bf76b20a681d2e54`；GitHub Linux、Windows、macOS 及 Windows WebView runner。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[master CI 40322409759](https://github.com/MFSGA/Chimera/actions/runs/40322409759) 与[最终 SHA E2E 40326080750](https://github.com/MFSGA/Chimera/actions/runs/40326080750)在代码修复前通过；Client rolling tag 当前为 `2c354de1e626310de937a32bdfb8297ca548f634`。
- 预期行为：更新 Alpha 内核时，资产文件名版本应从 `latest.chimera_client_alpha` 解析；稳定版与 Alpha 可独立前进，下载渠道继续使用 rolling `latest`。
- 真实异常、复现步骤与证据：版本清单记录稳定版 `v0.27.3`、Alpha `0.28.0-alpha+sha.2c354de`，实际 rolling 资产为 `chimera-client-0.28.0-{aarch64,x86_64}-apple-darwin.tar.gz`。更新器却用稳定版字段生成 `chimera-client-0.27.3-...`，因此无法取得当前 Alpha 资产。
- 确认的根因与排除项：Alpha 分支在 `ManifestVersion::get_matches` 复用了稳定 Client 版本；既有测试给稳定版与 Alpha 设置相同基础版本 `0.26.3`，未覆盖两条版本线分叉。ref 没有 Chimera Client Alpha 扩展；其 `MeowAlpha` 路径会从 Alpha 字段自身验证并构造资产名。
- 修复提交、差异映射及保留的兼容性边界：改为验证 Alpha 版本格式并从中提取 `0.28.0`；不合法的非空 Alpha 版本不生成下载项。旧版 manifest 缺少 Alpha 字段时保留稳定版本回退，下载 URL 和 `CoreTypeMeta::ChimeraClientAlpha` 不变。
- 原实现失败、新实现通过的回归测试，及测试注册位置：`backend/tauri/src/core/updater/mod.rs` 新增分叉版本、旧清单回退及坏版本测试，随 `pnpm test:backend` 运行；manifest resolver fixture 更新到当前 rolling tag SHA，随 `pnpm test:manifest` 运行。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：`pnpm test:manifest` 30/30、`pnpm test:release-preflight` 6/6、`pnpm test:updater-manifest` 10/10、目标包 `cargo fmt --check`、Prettier 和 `git diff --check` 通过。第一次 `pnpm lint:clippy` 暴露测试模块未导入新增辅助函数，已补上导入；本机 Rust updater 测试重编 AWS-LC 时空间降至 118 MiB，未到达断言；随后清理生成的 `backend/target`（3.3 GiB）。真实 `pnpm prepare:check` 识别出 `0.28.0-alpha+sha.2c354de`，但归档下载多次重试，因空间压力中断。新 SHA 远程 CI、E2E 和发布构建待运行。
- 风险/回滚和清理证据（如涉及宿主状态）：只改变 Alpha 资产文件名的版本来源；不改用户配置或更新协议。回滚方式为恢复稳定版字段映射。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：用稳定版 `0.27.3` 与 Alpha `0.28.0` 的明确分叉锁定资产版本来源；维持与 ref 的 Alpha 元信息解析模式一致，同时保留旧 manifest 的窄回退。

### LESSON-20261026-003：Updater manifest 必须使用已发布 tag 的稳定资产 URL

- 日期与时区：2026-10-26 UTC
- 状态：Verified（v0.27.1 manifest 已重生成；版本、签名、稳定 URL 和下载均核验通过）
- 严重度：Medium（更新器会请求不存在的临时草稿资产 URL，导致升级下载失败）
- 模块：Updater / Release
- master commit / ref gitlink SHA / 平台与测试环境：修复提交 `d0029954bc2ced5cb40d039072619143101ecb80`；验证 master `c057b479136e5fc1c655627758f23ce9efd196d9`；ref `eb81fda5dbbe6382c31875fc2d5709429056c279`。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[首次发布 run 40371994350](https://github.com/MFSGA/Chimera/actions/runs/40371994350)、[修复后 master CI run 40386437907](https://github.com/MFSGA/Chimera/actions/runs/40386437907)、[updater manifest 生成 run 40407974104](https://github.com/MFSGA/Chimera/actions/runs/40407974104)。
- 预期行为：发布草稿时更新清单可以读取签名；发布后每个更新 URL 均指向 `/releases/download/<tag>/<asset>`，可从公开发行页下载。
- 真实异常、复现步骤与证据：首次发布后检查 updater Release 中的 `update.json`，发现下载路径为临时 `/releases/download/untagged-.../`，对应请求返回 404；同一资产的正式 `v0.27.1` tag URL 返回 200。
- 确认的根因与排除项：草稿 Release 的 `browser_download_url` 使用临时 `untagged-*` 路径；将该值直接写入 updater manifest，发布后路径不会自动改成正式 tag URL。不是应用打包或签名生成失败。
- 修复提交、差异映射及保留的兼容性边界：`d0029954` 根据 repo、tag 和 asset name 构建正式下载 URL，并用 updater manifest 单测锁定 URL 生成；签名仍通过 Release Asset API 读取。v0.27.1 已发布后，仅重生成 updater Release 的 manifest；未再次移动已发布 tag 或重新构建发布。
- 原实现失败、新实现通过的回归测试，及测试注册位置：`scripts/updater-manifest.test.ts` 覆盖稳定 tag URL 与资产映射，并由 `pnpm test:updater-manifest` 运行。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：本地 updater 测试 19/19、oxlint、Prettier 与 `git diff --check` 通过；master CI run `40386437907` 的三平台 lint/build 全通过；updater run `40407974104` 成功。重新下载的 `update.json` 版本为 `v0.27.1`，所有平台条目均有非空签名，URL 均使用正式 tag 路径；5 个唯一下载 URL 的最终 HTTP 状态均为 200。
- 风险/回滚和清理证据（如涉及宿主状态）：此修复只更新 updater Release 的 manifest 资产，不修改已发布版本 tag、应用数据或已发布二进制；若再次使用草稿 `browser_download_url`，相同错误会复现。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：在 `updater-manifest.test.ts` 中覆盖 canonical URL 生成，并在 manifest 发布后检查版本、签名与资产可达性；本条记录已获维护者批准。

### LESSON-20261026-004：Tauri setup 中的任务必须使用已运行的异步 runtime

- 日期与时区：2026-10-26 UTC
- 状态：Mitigated（历史启动回归通过；当前集成 SHA 的 CI/E2E 待验证）
- 严重度：Medium（启动阶段 panic 会阻止应用窗口就绪）
- 模块：Rust / Storage event bridge / Tauri setup
- master commit / ref gitlink SHA / 平台与测试环境：修复前基线 `c057b479136e5fc1c655627758f23ce9efd196d9`；ref `eb81fda5dbbe6382c31875fc2d5709429056c279`；macOS WebKit 605.1.15。
- 关联 Issue / PR / CI / 运行日志（脱敏）：本地首次 `storage-event-resync.e2e.ts` 启动日志在 `backend/tauri/src/core/storage.rs` 的 storage listener 注册处记录 `there is no reactor running`；修复后重建当前 E2E binary，macOS WebKit 定向桌面用例 session `ad0aa229-19df-4cdb-a6fa-ffacde62350d` 通过。
- 预期行为：启动时将 storage event forwarder 注册到 Tauri 管理的 Tokio runtime，受 app shutdown token 取消并纳入 `TaskTracker` 生命周期；不能因 setup 所在线程没有 runtime 而 panic。
- 真实异常、复现步骤与证据：从 Tauri setup 主线程调用 `TaskTracker::spawn` 会在 Tokio 尝试创建任务时 panic；E2E 应用在页面断言开始前退出，因此这不是存储 UI 断言失败。
- 确认的根因与排除项：`TaskTracker::spawn` 依赖当前线程已进入 Tokio runtime，而 Tauri setup 回调没有该 runtime context。根因由启动 panic 定位；storage IPC、数据库打开和前端页面不是触发点。
- 修复提交、差异映射及保留的兼容性边界：对齐 ref `backend/tauri/src/setup.rs` 中受 shutdown/TaskTracker 管理的事件桥接生命周期，在 Chimera `resolve_setup` 取 `tauri::async_runtime::handle()` 并以 `tasks.spawn_on(..., runtime_handle.inner())` 显式指定运行时；继续使用 Chimera 的 `AppShutdown` child token，保留 typed storage event 与 legacy `storage_value_changed` / `storage_resync_required` 事件。
- 原实现失败、新实现通过的回归测试，及测试注册位置：Rust `core::storage::tests` 覆盖 event lag、恢复转发、提交/通知顺序、revision 和 fallback；`tauri-e2e/specs/storage-event-resync.e2e.ts` 在 `runtime` suite 验证真实桌面启动、legacy UI CSS 更新、缓存与 revisioned IPC snapshot。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：当前分支 Rust storage 定向测试 7/7；macOS WebKit 定向 E2E 1/1。完整跨平台 CI/E2E 尚未运行；Dashboard 单独用例在测试体开始前遇到已记录的路由动画 `currentTime=0` 超时（见 LESSON-20261020-002），不能算本功能失败，也不能算其主界面覆盖通过。
- 风险/回滚和清理证据（如涉及宿主状态）：仅改变 storage event forwarder 的任务调度入口，不触碰系统网络、用户配置或持久化格式；回滚需恢复使用 Tokio 线程本地 `spawn` 的实现，但会重现 setup panic。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：在非 Tokio setup/hook 回调中创建异步任务时，必须显式取得并传入所属 runtime handle；启动阶段回归应至少实际启动当前 E2E binary。

### LESSON-20261026-005：KV 迁移失败必须阻止默认值覆盖持久化数据

- 日期与时区：2026-10-26 UTC
- 状态：Mitigated（当前工作树定向回归、类型检查和静态检查通过；集成 SHA 的 CI/E2E 待验证）
- 严重度：Medium（旧版本无法读取新格式时，用户编辑可能覆盖持久化配置）
- 模块：Frontend / shared KV storage / Dashboard widget configuration
- master commit / ref gitlink SHA / 平台与测试环境：master 基线 `c057b479136e5fc1c655627758f23ce9efd196d9`；PR #532 原提交 `a12988c9c5039bb2343d46fa8e94d6d903a5afb8`；ref `eb81fda5dbbe6382c31875fc2d5709429056c279`；本地 Node 24 / macOS。
- 关联 Issue / PR / CI / 运行日志（脱敏）：PR #532（`https://github.com/MFSGA/Chimera/pull/532`）；独立审查定位原提交的 P2；原提交 CI `40396415742`、E2E `40396415794` 不作为修复 SHA 的验证证据。
- 预期行为：若 JSON 解析或迁移器拒绝已存值，hook 应显示读取错误并拒绝写入；排队期间如存储 revision 已变化，后端必须拒绝旧 revision 的写入，避免覆盖未知数据。
- 真实异常、复现步骤与证据：独立审查沿 `dashboard-widget-configs` 路径确认：存储值为合法 JSON 且版本为 2 时，v1 的 `normalizeWidgetConfigStorage` 抛错；原 hook 把异常静默变成默认值，provider 只验证 JSON 语法并报告读取成功，Dashboard 保存门禁因此放行 v1 写入。审查还确认 pending draft 会遮住新 revision、CAS 冲突后残留失败 draft 会影响后续编辑；最后补充复现“本窗口 W1 已提交、外窗随后提交更新值、外窗事件先于 W1 确认到达、排队的 W2 因此基于旧草稿覆盖新值”的竞态。以上由代码路径和延迟 IPC/外部事件回归用例证实，未用真实桌面持久化数据复现。
- 确认的根因与排除项：新 hook 的 `decode` 同时捕获 JSON 解析和迁移异常并返回默认值；KV provider 不持有调用方迁移器，且排队 API 原先不在实际发 IPC 时复核状态。根因不是 Dashboard 控件或 ref 的持久化实现。
- 修复提交、差异映射及保留的兼容性边界：在 Chimera `useKvStorage` 中保留 decode/migration 错误并拒绝写入；队列启动时重新校验最新已提交值；新增 expected-revision 条件写 IPC，由 `StorageInner` 在同一 revision 锁内执行比较并提交。未改变数据格式或默认迁移策略；旧的 service/Jotai 调用以无条件参数保留既有接口行为。
- 原实现失败、新实现通过的回归测试，及测试注册位置：`frontend/interface/tests/kv-storage-state.test.ts` 覆盖未知版本迁移错误和写入守门；`kv-storage-provider.test.ts` 覆盖延迟写确认期间的新未知版本事件、条件写 revision 推进、事件先到后的 draft 清理、冲突取消已排队旧写、外窗更新先于较早本地写确认时取消旧队列写、本地写回声不误取消后续写，以及监听注册失败后重试、就绪前不启动读取；`backend/tauri/src/core/storage.rs` 覆盖 stale expected revision 不落盘、不发布事件以及当前 revision 成功提交；`backend/tauri/src/ipc.rs` 锁定前端冲突识别所依赖的 IPC marker。前端测试通过 `pnpm --filter chimera-ui test:dashboard-widget-config` 注册。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：`pnpm --filter chimera-ui test:dashboard-widget-config` 23/23；`pnpm typecheck`、`pnpm lint:frontend-boundaries`、`cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib core::storage::tests` 9/9、`ipc::tests` 16/16、生成 bindings freshness 1/1、目标 Rust 格式检查、改动文件 Prettier/Oxlint 和 `git diff --check` 通过。当前集成 SHA 的 GitHub CI、桌面 E2E 尚未运行；Node 状态/IPC 存储测试不证明真实 Dashboard UI 展示或用户交互。
- 风险/回滚和清理证据（如涉及宿主状态）：conditional write 只拒绝基于过期全局 revision 的 KV 写入；写入冲突后触发读取以恢复权威状态。旧调用以 `null` 继续使用原无条件写语义。回滚时须同时撤回 IPC 参数、后端比较、前端 revision guard 与测试。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：所有 revision-aware KV 写入都须携带读取时观察到的 revision；未知版本必须 fail closed。维护者已授权记录确认的回归；此条不调整 CI 或治理门禁。

### LESSON-20261026-006：KV 命名空间迁移必须保留 legacy 事件与删除语义

- 日期与时区：2026-10-26 UTC
- 状态：Mitigated（两处兼容回归已修并通过本机定向测试；集成 SHA 的 CI/E2E 待验证）
- 严重度：Medium（legacy UI 跨窗口状态不更新；删除或清空后旧配置会再次读出）
- 模块：Backend / shared KV storage / legacy UI adapter
- master commit / ref gitlink SHA / 平台与测试环境：master 基线 `d79e6818f8fb0c8f5a8ed956b6e8c16579404002`；PR #532 原 head `278a5dae307ff69ffb887571ca8a35af8d76d73c`；ref `03ab6e5824cb44e527bde7cca28b699e8bf3a766`；macOS、Node 24。
- 关联 Issue / PR / CI / 运行日志（脱敏）：PR [#532](https://github.com/MFSGA/Chimera/pull/532)；独立审查指出 legacy event 的 `web:` 前缀与 Jotai 订阅键不一致，且未限定的 fallback key 在 remove/clear 后仍会重新读出。
- 触发条件及最小复现：窗口 A 对 `connections_table_columns` 写入 `web:connections_table_columns` 后，窗口 B 的 legacy 订阅按原始 key 查找而收不到更新；旧版写入的原始键在 namespaced key 被删除后被 `get_storage_item` fallback 再次返回。
- 确认根因及分类：本地兼容适配缺陷。后端 tuple event 原样发送存储键，旧 Jotai listener 以不带 `web:` 的 key 建立 subscriber；读路径支持旧 raw key，但 remove/clear 只删除 namespaced 值。
- 修复提交、差异映射及保留的兼容性边界：legacy UI adapter 在派发前把 `web:` 事件键还原为原始逻辑键；后端只对历史 frontend key allowlist 读取 fallback，并原子删除 namespaced 与对应旧键。调试列表可显示旧版 frontend 值；clear 只清 namespaced 与已知旧 frontend 键，其他内部键保持不动。
- 原实现失败、新实现通过的回归测试，及测试注册位置：`frontend/chimera/tests/legacy-storage-events.test.ts` 验证旧订阅收到 namespaced event，加入现有 CI 执行的 `test:dashboard-widget-config`；`backend/tauri/src/ipc.rs` 覆盖旧 key allowlist、列表优先级、remove/clear 与 internal key 保留；`backend/tauri/src/core/storage.rs` 覆盖 fallback 原子删除及 revision event。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：`pnpm --filter chimera-ui test:dashboard-widget-config` 23/23；`pnpm typecheck`（含 Chimera 与 E2E TypeScript）、`pnpm lint:frontend-boundaries`、Rust storage tests 9/9、`ipc::tests` 16/16、目标 Rust 格式检查、改动文件 Prettier/Oxlint 与 `git diff --check` 通过。集成 SHA 的多平台 CI 与 Desktop E2E 尚待结果。
- 风险/回滚和清理证据（如涉及宿主状态）：限于持久化 KV 和 legacy UI 事件边界；fallback 只匹配已知旧前端键，避免触碰未命名空间的内部键。回滚此修复会重新引入旧配置复活和 legacy UI 事件断连。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：改动持久化键前同时检查读取、写入、事件订阅、单项删除、调试枚举与全量清理；任何兼容 fallback 都须配对删除测试并明确允许的旧键集合。

### LESSON-20261026-008：修改导出 IPC 文档后要重新生成绑定

- 日期与时区：2026-10-26 UTC
- 状态：Mitigated（生成器和 freshness 测试通过；集成 SHA 的 CI/E2E 待验证）
- 严重度：Low
- 模块：Rust / IPC / Specta bindings
- master commit / ref gitlink SHA / 平台与测试环境：PR #532 原 head `944b24f21751c664bf50e645bbf754c8ca2909c3`；ref 分支 gitlink `03ab6e5824cb44e527bde7cca28b699e8bf3a766`；macOS，Rust target 在外接构建盘。
- 关联 Issue / PR / CI / 运行日志（脱敏）：PR [#532](https://github.com/MFSGA/Chimera/pull/532)；[失败 CI run 40422919747](https://github.com/MFSGA/Chimera/actions/runs/40422919747)；同一原 SHA 的 Desktop E2E run 40422919746 通过。
- 预期行为：任何 Rust IPC command 的 `specta` 导出文档或类型变化，都应与检入的 `frontend/interface/src/ipc/bindings.ts` 一致。
- 真实异常、复现步骤与证据：原 SHA 的全平台 backend workspace test 均在 `specta_export::tests::typescript_bindings_are_fresh` 失败，提示运行被忽略的 `update_typescript_bindings` 测试；生成结果显示只有 `get_all_storage_items` 与 `clear_storage` 的 Rust 文档注释还保持旧描述。
- 确认的根因与排除项：PR 更新了 IPC 命令文档，但没有更新 Specta 生成的 bindings 文件。此失败不是 KV 逻辑或目标平台特定问题。
- 修复提交、差异映射及保留的兼容性边界：在 PR #532 运行既有 ignored generator 更新 `bindings.ts` 中两条导出命令文档；不改 Rust 命令合同或运行行为。
- 原实现失败、新实现通过的回归测试，及测试注册位置：`specta_export::tests::typescript_bindings_are_fresh` 在 backend workspace `cargo test` 中执行，检查生成后 bindings 与检入文件逐字一致。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：`cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib specta_export::tests::update_typescript_bindings -- --ignored` 1/1、对应 `typescript_bindings_are_fresh` 1/1、bindings Prettier 与 `git diff --check` 通过。当前集成 SHA 的跨平台 CI 仍待结果。
- 风险/回滚和清理证据（如涉及宿主状态）：只改变生成绑定中的 IPC 文档注释；不会触及用户配置、存储数据或运行时协议。可通过重新运行生成器复原。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：修改带 `#[specta::specta]` 的 IPC 时，应运行生成器并执行 freshness 测试；CI 已有该检查，无需新增工作流。

### LESSON-20261026-007：Dashboard resize E2E 必须在清理前保留原始失败证据

- 日期与时区：2026-10-26 UTC
- 状态：Mitigated（已确认并修复对象字段顺序导致的测试误报；Dashboard resize 产品行为和新 SHA Windows E2E 仍待复验）
- 严重度：Low
- 模块：E2E / Dashboard
- master commit / ref gitlink SHA / 平台与测试环境：失败 SHA `1754d3a066e45c892f9fed59fb01230a23012ef5`；ref `03ab6e5824cb44e527bde7cca28b699e8bf3a766`；GitHub Windows runner / Edge 155
- 关联 Issue / PR / CI / 运行日志（脱敏）：PR [#534](https://github.com/MFSGA/Chimera/pull/534)，[失败 E2E run 40417623645](https://github.com/MFSGA/Chimera/actions/runs/40417623645)
- 预期行为：Dashboard resize 用例失败时，在清理恢复窗口和存储前保存失败阶段、期望与实际 UI/持久化摘要、原始错误、平台/驱动和构建身份；清理单独失败时也在继续清理前留证，并把所有清理错误写进最终元数据。
- 真实异常、复现步骤与证据：`40417623645` 中 Dashboard 的 `settles responsive layout changes without persisting a resize projection` 失败，但只报告通用 `AggregateError`，未记录 primary/cleanup 的具体错误；原失败截图由 `afterTest` 在 `finally` 已恢复窗口、存储和路由后采集。后续 [run 40423163291](https://github.com/MFSGA/Chimera/actions/runs/40423163291) 保留了 primary 错误：Wide Dashboard widgets 等待超时，但最后读取的 ID 与两个 shortcut 状态都等于期望值，仅对象字段次序不同。
- 确认的根因与排除项：`waitForWidgets` 用 `JSON.stringify(actual) === JSON.stringify(expected)` 判断对象相等；Windows WebDriver 返回对象键顺序为 `hasCoreShortcuts`、`hasProxyShortcuts`、`sparklineIds`，而期望对象以 `sparklineIds` 开始，因此相同字段值仍被判失败。这是测试断言错误，不是 Dashboard 产品 resize 回归。初始通用 AggregateError 还揭示第二个已确认的诊断缺陷：没有在清理前采集现场，也未在消息中打印嵌套错误。
- 修复提交、差异映射及保留的兼容性边界：PR #534 的本地待推送改动以 `isDeepStrictEqual` 结构化比较 widget 观察结果；失败时在清理前采集截图、页面源码、路由/视口/目标 widget 几何、脱敏存储摘要，以及布局稳定期间的前端错误事件；响应式测试主体、证据采集和清理请求都临时使用 5 秒 WDIO transport timeout 并禁用重试，显式状态等待仍保留独立期限，且不保证服务端已开始的命令会被取消；前端错误先复制到 Node 侧再清理监听器，最终 JSON 保存每步清理结果、完整清理错误、源提交和实际 E2E 二进制 SHA-256。未改变 Dashboard 产品行为。
- 原实现失败、新实现通过的回归测试，及测试注册位置：原行为由上述 Windows hermetic run 复现。新诊断路径尚无真实失败样本验证；代码仍使用同一个已注册 Dashboard spec，待新 SHA Windows hermetic run 核实原断言结果和诊断文件。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：当前本地 `pnpm --filter @chimera/tauri-e2e typecheck`、针对 spec 的 Oxlint、Prettier 与 `git diff --check` 通过；尚未运行新 SHA 桌面用例，不能声称 resize 功能或失败捕获已通过 E2E 验证。
- 风险/回滚和清理证据（如涉及宿主状态）：只读 UI 证据来自隔离 E2E runtime；存储仅记录布局类型和坐标，不保存原始持久化值。失败时仍会尝试恢复原存储、窗口与 Dashboard 路由。回滚为恢复原序列化字符串比较、`finally` 清理和错误聚合路径；会重新引入跨驱动的错误断言和失败现场丢失。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：新 SHA 的失败产物需要确认唯一 run/spec/test/attempt 路径、预清理截图/页面源码、目标状态、存储摘要和完整清理错误均已上传。无治理或 CI 门禁调整。

### LESSON-20261027-001：Dashboard resize E2E 必须把窗口缩到目标网格尺寸

- 日期与时区：2026-10-27 UTC
- 状态：Open（用例已修正；新 SHA 的 Windows 桌面复验待完成）
- 严重度：Low
- 模块：E2E / Dashboard
- master commit / ref gitlink SHA / 平台与测试环境：失败 head `77f00cd61b1f9f6074eb00da971d47f33efbb653`；ref `9f72efe292975b13b5d9018a9f5641ffbb071c27`；GitHub Windows runner / Edge 155
- 关联 Issue / PR / CI / 运行日志（脱敏）：PR [#534](https://github.com/MFSGA/Chimera/pull/534)，[Windows E2E run 40442050131](https://github.com/MFSGA/Chimera/actions/runs/40442050131)
- 预期行为：响应式用例根据真实窗口尺寸触发 `DndGrid` 网格尺寸变化，并选择与窄布局对应的保存预设。
- 真实异常、复现步骤与证据：测试请求窗口宽度 680px 后，Windows WebView 实际 viewport 是 664px；grid 容器宽度为 632px，当前 `minCellSize=64`、`gap=16`，因而实际为 8 列。用例只断言 viewport `<800`，就开始等待单项 `4x5` fixture；页面正确继续显示 `8x6` 中的宽布局 widgets。失败元数据和截图均来自 run `40442050131`。
- 确认的根因与排除项：测试将通用的 `<800` viewport 判断误当成 4 列网格条件；按与 ref 相同的公式 `floor((width + gap) / (minCellSize + gap))`，632px 容器明确得到 8 列。这次失败不是 Dashboard 产品布局逻辑偏离 ref 的证据。Chimera 与 ref 的首次主窗口尺寸也相同，均为 800×636，最小尺寸 400×600；保存的窗口几何可以覆盖首次启动默认值。
- 修复提交、差异映射及保留的兼容性边界：把 Dashboard 测试目标宽度改为 400px，并要求真实 `window.innerWidth <= 400` 且 DndGrid 容器宽度位于 304–384px 半开区间后再断言窄布局；该区间证明 64px 网格单元与 16px 间距可容纳四列，避免 4x5 fixture 在更窄网格中仍作为“最近布局”被选中。失败证据新增 DndGrid 容器边界。生产窗口默认值与布局算法保持不变。
- 原实现失败、新实现通过的回归测试，及测试注册位置：同一 `dashboard-main-layout.e2e.ts` 用例注册在 Windows `hermetic` suite；新 head 的 Windows 复验待执行。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：失败 head 的 Windows hermetic run 中 52/53 specs 通过，Dashboard resize 用例因尺寸条件失败；本条针对修正后的新 head 验证尚未运行。本地提交钩子将执行 E2E 类型检查、Oxlint 和 Prettier，不能替代真实桌面复验。
- 风险/回滚和清理证据（如涉及宿主状态）：仅调整隔离 E2E 用例的窗口尺寸断言和失败元数据，不改变应用窗口尺寸、用户设置或网格业务逻辑；回滚为恢复 680px 请求和 `<800` 判断，但会重新引入无效窄布局断言。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：后续布局用例以实际 viewport 和目标网格容器尺寸作为条件，不把通用响应式断点等同于 `DndGrid` 列数；本条新增 Windows E2E 复验后再更新状态。

### LESSON-20261029-001：存储读取必须等跨窗口事件监听成功

- 日期与时区：2026-10-29 UTC
- 状态：Mitigated（监听注册失败恢复用例通过；集成提交的 CI 与桌面 E2E 待运行）
- 严重度：Medium（初始化缺少事件监听时，窗口间更新可能长期不反映在当前 UI）
- 模块：Shared KV storage / frontend provider / IPC
- master commit / ref gitlink SHA / 平台与测试环境：master 基线 `adef2f7eaa63bc19071f126c61217bd01d0acd12`；ref `8e1084e4e7c50531a5d16ca9a6c9dd2bb2f0f8e8`；macOS Apple Silicon、Node 24。
- 关联 Issue / PR / CI / 运行日志（脱敏）：PR [#532](https://github.com/MFSGA/Chimera/pull/532) 的 stale-head 供参考；新集成提交尚未推送，远端 CI/E2E 无可用结论。
- 预期行为：窗口首次读取 KV 之前，value-changed 与 resync 监听器都应注册成功；暂时注册失败时应恢复监听，并在恢复后读取存储的最新值。
- 真实异常、复现步骤与证据：ref `frontend/query/src/provider/kv-storage-provider.tsx::connect` 对两个注册 Promise 使用 `Promise.allSettled`，即使其中一个拒绝仍调用 `markReady`，且没有再次注册。新回归用例注入首次 value-changed 注册失败，并在监听中断期间更新后端值；旧逻辑会在监听缺失时启动读取且永远不重试，错误路径由测试断言捕获。Chimera 初版重试在监听恢复时又启动重复读取，导致前端套件 23 项中 3 项因旧 `refresh` Promise 被新读取替代而失败；修正后用例重新执行 23/23 通过。
- 确认的根因与排除项：这是 ref provider 的事件注册错误恢复缺口，不是 redb 数据丢失。初始 reads 已等待 readiness，因此首次注册恢复后不需要再触发一次重复 resync；注册后的正常 resync event 仍会重读已用 keys。
- 修复提交、差异映射及保留的兼容性边界：Chimera 保留 ref 的 `KvStorageProvider`/store/readiness 分层；对缺失监听器单独指数退避重试，最大间隔 30 秒，只有两类监听均注册后才释放首次读取。对监听注册失败采用 fail-closed 行为；ref 增加等价恢复后复核是否可删除 Chimera 专属重试。
- 原实现失败、新实现通过的回归测试，及测试注册位置：`frontend/interface/tests/kv-storage-provider.test.ts::listener registration retries before authoritative reads resume` 验证首次注册失败时后端读取数为零、后端值在恢复期间发生改变、监听恢复后首次读取返回最新值，并断言两类事件监听已存在。该用例随 `pnpm --filter chimera-ui test:dashboard-widget-config` 注册。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：`pnpm --filter chimera-ui test:dashboard-widget-config` 23/23；包括错误注入用例。本地 Node 测试不证明 Windows/macOS WebView 的实际跨窗口事件传输；新提交的 CI 与 runtime Desktop E2E 待运行。
- 风险/回滚和清理证据（如涉及宿主状态）：只影响窗口内监听恢复和首次读取时序，不更改 redb 格式、键、用户配置或系统网络。回滚会恢复 ref 当前无重试的行为，并重新允许监听失败后读取开启。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：改动 ref provider 事件注册时，必须保留“注册拒绝后不读取、恢复后读取最新值”测试；未修改 L3 治理或工作流。

### LESSON-20261029-002：E2E 存储辅助函数须跟进 IPC 返回类型

- 日期与时区：2026-10-29 UTC
- 状态：Mitigated（测试辅助函数已改为读取 revisioned snapshot；PR 新 SHA 的 Windows E2E 待复验）
- 严重度：Low（桌面回归测试失败，未发现产品存储行为错误）
- 模块：E2E / Dashboard / KV storage IPC
- master commit / ref gitlink SHA / 平台与测试环境：PR #548 失败 SHA `84eb3f0d3e5b86169465a61462718844f78cc4ba`；ref `8e1084e4e7c50531a5d16ca9a6c9dd2bb2f0f8e8`；GitHub Windows / Edge 155。
- 关联 Issue / PR / CI / 运行日志（脱敏）：PR [#548](https://github.com/MFSGA/Chimera/pull/548)；[Windows Tauri E2E run 40864250817](https://github.com/MFSGA/Chimera/actions/runs/40864250817)。Hermetic suite 56 个 spec 中 55 通过，Dashboard resize spec 失败。
- 预期行为：Dashboard E2E 对 `get_storage_item` 只读快照的 `.value`，并将原字符串恢复；fixture 写入显式使用无条件 `expectedRevision: null`。诊断与清理路径也必须使用同一 IPC 合同。
- 真实异常、复现步骤与证据：Windows E2E 的 `summarizeDashboardStorage` 将 `{ value, revision }` 对象传给 `Buffer.byteLength`，触发 `ERR_INVALID_ARG_TYPE`；随后清理将对象当字符串传给 `set_storage_item`，Tauri 拒绝 `value` 的 map 类型。失败在 Dashboard 响应式布局断言期间进入证据/恢复代码，未显示持久化业务逻辑失败。
- 确认的根因与排除项：新增 revisioned IPC 后，`tauri-e2e/specs/dashboard-main-layout.e2e.ts` 的主要读路径之外仍保留旧 `string | null` 假设；证据采集、断言与清理都调用同一个已变更的 command。根因是 E2E 调用点未随接口迁移，不是存储事件、布局或 CAS 实现错误。
- 修复提交、差异映射及保留的兼容性边界：所有 Dashboard layout E2E 读取现将 `StorageItem<String>` 解包为 `.value`；fixture 初始化与恢复明确传 `expectedRevision: null`，保留此测试的强制准备/清理语义。生产合同保持 revision-aware。
- 原实现失败、新实现通过的回归测试，及测试注册位置：失败由 run `40864250817` 的 Dashboard resize 用例及清理日志复现。修复后同一 spec 注册于 Windows PR `hermetic` suite；新 SHA 桌面复验尚待运行。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：修复后 E2E TypeScript typecheck、针对 spec 的 Prettier 与 `git diff --check` 通过；本机重建 E2E binary 成功，但 macOS WebKit 在 before hook 的主路由动画停在 `currentTime=0`，未进入该 spec 断言（同类已知环境问题见 LESSON-20261023-003），不作为修复通过证据。Windows 新 SHA 的 CI/E2E 待复验。
- 风险/回滚和清理证据（如涉及宿主状态）：只改变测试读值解包、fixture 参数及诊断/清理代码；不写入真实用户目录或主机网络。失败运行的初始 E2E runtime 为隔离目录，清理错误已记录。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：变更 IPC 返回值结构时，搜索所有 E2E 调用点，包括失败证据、断言和 `finally` 清理；用实际结构化绑定类型，不能只更新主要测试路径。未修改 CI 门禁。
