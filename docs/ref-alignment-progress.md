# ref 对齐进度

## DIFF-160（Runtime Reconciler）：断连模式的 Clash raw Checked CAS 与完成回执屏障（2026-11-22）

- **上游目标 / 保留范围**：固定只读 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`，继续对齐 `ref/backend/nyanpasu-core/src/client/reconciler/apply.rs` 的「source Check → durable CAS → one Core Apply → 按 Core replaced 回执结算旧实例连接」顺序。普通 `patch_clash`、`patch_clash_overrides` 的本地 `mode` 变化现在与 `allow-lan`、`ipv6`、`log-level`、`unified-delay`、`tcp-concurrent` 纯 runtime 字段一样，可以进入 Checked Source Gate；无新分支、worktree、Core mutator，现有 UI/Agent API 与 `patch_running_clash_overrides` 的同步 TransactionOutcome 不改。Service IPC/Service 模式、TUN、端口、external-controller/auth、未知字段仍退回旧路径。
- **断连请求与确切 CAS 一起走**：Clash source actor 在准备候选时沿用 `plan_clash_patch` 判定 `mode_changed && break_on_mode_change`，作为显式 `interrupt_mode` 随 `CandidateCheck::check_with_receipt_effect` 的唯一 request-id 传入 `CheckedCandidatePort::checked_for_request`、生产 `CoreLifecycleCheckedInbox` 和 `CoreLifecycleWorkflow::ApplyChecked`。无标记的 Application/Profiles 及普通配置提交保持原语义。Actor 与 Check 端双重拒绝非 Clash source 的伪造 mode 效果；只有 Source CAS 成功、精确请求的 Checked 通知到达才消费该标记，同域其他请求不能继承、CAS 失败时按 request-id 撤销。
- **执行与错误隔离**：`apply_checked_runtime` 在 `interrupt_mode=true` 时先校验完整权威 source target，不依赖是否从旧 Startup 恢复了 `applied_selection`；Core 前捕获经实例绑定的旧 Core API，唯一 Core Owner 只提交一次预检查字节，Core Apply 成功并通过第二次 Source Authority 校验后，根据下层已确认的 `replaced` 判定。只有已确认 `Patched` 且旧实例 API 可用时中断原连接；`Started/Restarted/Switched`、旧实例不可用、替换结果未知、Source 过期或 Core 失败都不向新/未知实例发送断连。断连失败是提交后降级日志，不撤销 durable CAS、重播 Core 或让第二个执行者介入；没有模式标记的 Checked 仍使用上游 Selection 政策。
- **完成回执关键竞态修复**：当 Checked 工作线程的最后一次权威读取已完成，但 CoreLifecycle Actor 在 `Completed` 前接受了更新的 SourceCommitted 通知时，Actor 原有完成屏障拒绝旧目标收敛，却可能保留工作线程写下的 `applied_selection`。现在对该 `CommittedTargetAdvanced` 检测立即使选择基线失效，才允许后续 Mode 切换检查旧选择。现有真实 Actor「新源提交在 Core 运行中被 mailbox 接受」测试扩展了这一断言，防止错误复用旧基线。
- **回归与后续**：新增 `checked_mode_interruption_is_scoped_to_exact_clash_request`（多个同域请求、跨域隔离、CAS abort 及伪造标记拒绝）、`explicit_clash_mode_effect_prepares_old_instance_without_applied_selection`（无旧选择仍准备断连、无显式 Mode 标记不继承、Source 版本不匹配在 Core 前失败）和更新 raw 模式准入测试。受限准入以外的 legacy source-rebuild/deferred handoff 仍可能不携带单次模式 Effect；这部分需要在统一 Reconciler 的 pending effects/degradations 中跟踪后再宣称完全一致。前端 `SetConfig` 异步通知、Windows CI、桌面 E2E 与模式实际连接中断仍需验收，不能由本机测试代替。本机 `CARGO_INCREMENTAL=0 cargo test --manifest-path backend/Cargo.toml -p chimera --lib --quiet` 全量回归 **1184 passed / 0 failed / 1 ignored**（包括新增真实 Actor mailbox fence 与 explicit Mode source-effect 测试）。既有大型异步 Send 类型递归未来兼容性警告尚未解决。Rustfmt、Clippy、Git Verified 与推送以最终实际输出为准。

## DIFF-159（Runtime Reconciler）：启动与普通 Reconcile 从已确认 Core 回执恢复 applied_selection（2026-11-22）

- **上游映射 / 目标 / 非目标**：固定只读 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`；对照 `ref/backend/nyanpasu-core/src/client/reconciler/apply.rs` 仅在 `ApplyOutcome::Applied` 后建立 `applied_selection` 的语义，解决 Chimera 历史 Startup/Retry/Source Reconcile 仍走独立 `CoreLifecyclePort::reconcile_profiles`，没有给 Checked Apply 路径建立已应用 mode/profile 选择基线的问题。目标是保留现有单 Core Owner、Service/S2、TUN、双 UI 和 Agent 的运行方式，不借源配置提交推断已应用 Core，**本轮不直接放宽断连策略开启时 raw mode 的 Checked 准入**。
- **唯一 Actor 的成功完成结算**：复用 `CoreLifecycleWorkflow::applied_selection`，安装和 CoreLifecycle Actor 共用的只读 `LiveSourceAuthority`。`ApplicationWorkflowActor::Completed` 已完成 source-version mailbox race fence 后，仅对于 **非 Checked 的 reconciled 命令**，并且 `result.is_ok() && verified_running && !source_advanced` 时调用 `recover_applied_selection_from_receipt`；失败、停止或不确定直接清空基线。真正 Checked Apply 仍独立使用精确源版本前后校验，不从普通 Reconcile 回执覆盖或清除。
- **证明方式 / 拒绝原则**：`LiveSourceAuthority::selection_for_applied_target` 使用与 Checked Apply 相同的三域有界、内容锁保护的完整 `RuntimeInputs::target_key` 读取，**只有**旧式运行时回执的 `target` 与最新已提交目标相等才返回 `Selection`；缺少来源权威、回执 `target=None`、当前 Core 非 Running/实例不匹配、最新内容变化、读取失败或执行期间 Core 换实例都拒绝恢复。执行侧对 `CoreLifecyclePort::active_runtime_receipt` 做前后两个实例绑定回执确认，对源权威选择重复读取并要求一致；只读，不重新生成配置、不提交 Core、不修改持久数据。普通生命周期命令进入时清除可能过时的先前选择，只有成功/证据齐全才恢复。保留 `patch_running_clash_overrides` 的同步 `TransactionOutcome` 和旧 mode 断连路径。
- **测试与待办**：新增 `legacy_running_receipt_recovers_only_matching_authoritative_selection` 验证匹配恢复、源晋升拒绝、运行 Core 退出拒绝，且整个恢复过程没有第二次 Core 提交；扩展 `authoritative_target_covers_committed_source_versions_and_managed_content` 检查旧回执目标匹配与管理 Profile 文件改变后的拒绝。下一步需要以这些运行中的 **已验证** selection 为基础，将 `on_mode_change=true` 的 Checked 准入和单次旧 Core 中断与强制的 CAS/owner 提交屏障连接；在这之前保留旧同步模式切换，避免源变更或启动竞态漏断连。现有 Windows CI/Tauri E2E 不能以 Mac Rust 测试替代。本机 `CARGO_INCREMENTAL=0 cargo test --manifest-path backend/Cargo.toml -p chimera --lib --quiet` 完整回归 **1182 passed / 0 failed / 1 ignored**，新增选择基线恢复测试及已有上下游异常分支全部运行；编译器仍报告既有 async 类型递归未来兼容性警告。rustfmt、clippy、签名和远端推送以实际完成结果为准。

## DIFF-158（Runtime Reconciler）：贯通 Core 替换回执与模式切换断连的源实例保护（2026-11-22）

- **ref 对照 / 范围 / 前置证据**：只读 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`，对齐 `ref/backend/nyanpasu-core/src/client/reconciler/apply.rs` 中 `AppliedCandidate.replaced`、`applied_selection` 与 Apply 后 `finish_interruption` 的关键先后关系，不额外创建 Runtime/Core writer 或改变本地 `master`、UI/Agent 公共 API。上一轮已证明 Checked source 版本和 `applied_selection` 的先决条件。本轮不修改 Service/TUN/系统端口的宿主副作用，不把未经证实的下层完成结果解释为热更新。
- **真实替换回执**：`core/actor_v2/local_runtime.rs::reconcile_prepared_outcome` 保留原有 `reconcile_prepared` 兼容入口，在校验同一 Core Owner、exact bytes/digest、确认 `applied_revision` 且发布已接受的运行时产物后，返回其 `ReconcileOutcomeInfo` 的 `reconcile_replaced`（Started/Restarted/Switched = true，Patched = false）。`CoreFacade::run_local_mutation` 原有序列化注册表只接受 `Result<()>`：使用同**一**注册表操作内部的一次性结果通道传出 bool，仍由原操作注册表处理超时、不确定、刷新绑定；不能在注册表外二次提交 Core。生产 `LegacyCoreBridge::reconcile_prepared_outcome` 将其发布为 `CheckedApplyOutcome {replaced:Some(bool)}`；测试/未知 `CoreLifecyclePort` 退回既有 exact Checked Apply 并返回 `None`，禁止猜成 `Some(false)`。
- **权威策略与实例保护**：`LiveSourceAuthority::verify_target` 以一个有界的 Application/Clash/Profiles 版本、当前 Profile 内容、目标 digest 读取操作验证源身份，同时携带 mode/profile 断连策略；`verify_interruption` 按上游 `Selection::interruption` 比较已应用选择与新的已提交选择，初次/未知旧选择不产生断连。Checked Workflow 在 Core Apply **之前**由唯一 CoreLifecyclePort 捕获已确认的旧 Core 实例 API（生产只认可被当前 Core 状态确认的 `active_runtime_receipt`），经单 Owner Core Apply 和第二次 Source Authority 验证成功后才更新选择。只有需要 Mode 断连、下层回执为 **Patched** 且捕获的是旧实例绑定 API 时才调用 `on_mode_change`；Started/Restarted/Switched、结果不明、无旧运行时或验证失败均不得对新实例发起误中断。断连 API 失败只记录提交后警告，不撤销持久配置或重新 Core Apply。
- **分阶段准入保护**：当前普通 `patch_clash` 和普通 `patch_clash_overrides` 新增 `mode` 字段 Checked 准入，**仅在 `break_connection.on_mode_change=false`**、本地 Core、无 Service IPC、且不混入端口/controller/auth/TUN 等额外副作用时成立。需要断连的模式切换仍沿用原先同步 Apply + 连接中断路径：旧启动途径尚未统一填充 `applied_selection`，直接放宽会让第一次模式切换漏断连。原 `patch_running_clash_overrides` 的同步 TransactionOutcome 语义不变；逐步完善已应用启动选择基线后，才能对用户开启断连的 Mode 更新全面切换。
- **验证 / 风险**：增加 Started/Patched Checked 本机 Core 回执的真实 `LocalRuntimeHost` 测试，覆盖精确字节及发布结果；扩展权威源测试，模式策略与断连判断必须基于同一版本目标读，缺少旧选择不得中断。仍须实机验证连通中的旧实例、中断失败、Core 重启及 S2 Host 迁移，旧 CI Windows backend 和 Tauri E2E 的失败不能用本机 Rust 通过替代。`CARGO_INCREMENTAL=0 cargo test --manifest-path backend/Cargo.toml -p chimera --lib --quiet` 在 macOS 本地 **1181 passed / 0 failed / 1 ignored**（含 Started/Patched 回执与权威策略测试）；在抽取 `apply_checked_runtime` 以缩短巨大生命周期异步分支后，完整测试再次 **1181 passed / 0 failed / 1 ignored**。当前编译器仍会报告大型异步类型链的 `recursion_depth_exceeding_limit` 未来兼容性警告，未通过增大递归上限掩盖，也不宣称已解决；签名、Clippy、推送和新 CI 状态以最终实际运行输出为准。

## DIFF-157（Runtime Reconciler）：权威 Checked Apply 的已应用 Selection 状态先行迁移（2026-11-22）

- **上游映射 / 非目标**：固定只读 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`；对照 `ref/backend/nyanpasu-core/src/client/reconciler/{apply.rs,committed.rs}` 的 `applied_selection` 和 `selection.interruption` 决策。仅迁移 **上一次确实成功应用的 mode/profile 选择基线**；尚未接管 `prepare_apply/finish_interruption`，因为 Chimera 的旧 `CoreLifecyclePort::reconcile_prepared` 目前只有 `Result<()>`，缺少上游 `AppliedCandidate.replaced` 与 pre-apply Core API 句柄。禁止从已保存 Clash 配置的前后差异直接中断连接，也不修改 `mode`、端口/TUN、Service Host、running override 或旧 Verge 事务执行路径。
- **生产状态与 Source Authority**：`reconciler/authority.rs::LiveSourceAuthority::read_committed_target` 现与整套 target identity 和三个源域 version 在同一个有界读取内取出 `Selection::of(RuntimeInputs)`，`verify_selection` **只有版本和 checked target 均匹配**才返回实际选择。测试/旧 Authority 默认为 `None`，而非推断已应用选择。唯一 `CoreLifecycleWorkflow::ApplyChecked` 在提交版本的首次 Authority 校验、原 Core Owner 的 exact prepared byte Apply、第二次含 selection 的 Authority 校验全部成功后，才设置共享但仅属于同一生命周期工作流的 `applied_selection`；Core 返回失败、源提前变化或 Check 失败都会清空该基线。其他非 Checked 生命周期命令可能重建/切换 Core，不具备上游已应用选择回执，因此保守清空基线，避免晚到的 Checked 命令根据旧选择误中断连接。不会建立第二 Core Owner、重新构建 Checked 字节或承诺操作未完成时的 runtime convergence。
- **回归与风险**：新增 `checked_apply_records_only_authoritatively_confirmed_selection` 验证两个 Authority 读、真实 Core Check Apply、状态设置与过期候选失败后清零；`legacy_lifecycle_operation_invalidates_checked_selection_baseline` 验证旧重建使基线失效；扩展 `authoritative_target_covers_committed_source_versions_and_managed_content` 验证同一 committed target 返回 selection，管理文件变化拒绝返回。已有 `committed.rs` 上游 Mode/Profile 断连策略测试不变。这是中断执行迁移的**必要前置状态**，不宣称已迁移 mode 单次断连、`finish_interruption` 或 Core 实例替换判别。上一提交 `c4dab37e` 的 CI Windows backend 与 Tauri Desktop smoke 均失败，本地通过不能替代跨平台验收。`CARGO_INCREMENTAL=0 cargo test --manifest-path backend/Cargo.toml -p chimera --lib --quiet` 在 macOS 本地 **1180 passed / 0 failed / 1 ignored**；Rustfmt、Clippy、签名与推送以实际结果为准。

## DIFF-156（Runtime Reconciler）：Clash raw/普通 override 运行时输入复用 Checked CAS，保留 running 事务和模式副作用（2026-11-22）

- **ref 对照 / 目标 / 非目标**：继续以只读 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01` 为架构基线；按照上游 `state/clash_config.rs::commit` 的 `CandidateCheck.check → CAS → RuntimeGate.committed` 顺序，迁移 Chimera 现有 raw Mapping 和普通 `patch_clash_overrides` 中**实际改变 runtime inputs**、无额外运行时控制/Host 副作用的配置字段。保留双 UI/Agent 共用的 typed source 与旧兼容镜像，不改 raw patch 的对外接口或配置字段格式，不另起 Core Owner、分支或 worktree。本轮**不**迁移 `mode`、端口、`external-controller`、控制器认证 `secret`，也不迁移 TUN/Service handoff 与旧 Verge 多域事务。
- **实际迁移与复用**：`backend/tauri/src/client/clash_config.rs::patch_with_overrides` 沿用原先 `apply_mapping_to_typed_clash`、`plan_clash_patch`、mixed-port 与 controller validation；当变更到达运行时、处于本地执行 Host 且 raw Mapping **全部字段仅属于** `allow-lan`、`ipv6`、`log-level`、`unified-delay`、`tcp-concurrent` 时，调用与 typed flags 同一个 `commit_checked_candidate`。它会复用生产 `CandidateCheck::check_with_receipt` 和唯一 CoreLifecycle Actor ACK，持久化 Clash CAS 后立即 `committed_with_receipt(Clash, exact-version, request)`；再尽力持久化 legacy mirrors，失败仅作为降级；**不运行第二次** `update_core_config`、旧 `SourceCommitted` 或 mode interruption。配置允许混合这些五类纯构建字段，任何额外未知字段和显式 `mode` 都退回旧路径，即使同值 mode 也不被新路径误吸收。独立的 stream log-level 更新仍由已有外层 Client 负责。
- **执行结果契约**：普通 raw `patch_clash` 与 `patch_clash_overrides` 可以在 source durable commit 后返回，Core Apply 在唯一 actor 队列异步完成；这与上游 source-owner 语义一致，但不等于本次操作已经 runtime converged。现有 `patch_running_clash_overrides` 的 `TransactionOutcome::Committed` 则**明确表示同步 Core Apply 已完成**，本轮用 `allow_checked=false` 显式保留旧 `commit_candidate_with → apply_committed` 路径，不提前谎报 committed runtime。模式切换仍由旧路径在应用完成后调用一次连接中断；接下来必须随上游 `reconciler/apply.rs` 的 `applied_selection` 和 `finish_interruption` 一起迁移。
- **校验 / 剩余风险**：`raw_override_checked_route_admits_only_pure_local_runtime_inputs` 覆盖五类运行时输入、Service Host 排除、mode/port/controller/auth/未知字段阻断及非字符串字段；`unchanged_raw_mode_request_must_not_enter_checked_apply` 验证同值模式命令仍落旧路径。DIFF-155 精确 request-id ACK/CAS/abort 与 Core 前后 Source Authority 检查不变。所有仍需实测的 Windows backend CI、桌面 E2E、Mode interruption、TUN/Service 与旧 `SetConfig` 通知结算不视为已完成；本轮未修改发布流程或跳过测试。`CARGO_INCREMENTAL=0 cargo test --manifest-path backend/Cargo.toml -p chimera --lib --quiet` 在 macOS 本地 **1178 passed / 0 failed / 1 ignored**；Rustfmt、Clippy、Prettier、签名和远端同步以最终实际运行结果为准。

## DIFF-155（Runtime Reconciler）：同域 Checked 请求身份贯通，精准 CAS 提交与撤销（2026-11-21）

- **上游对齐 / 范围**：固定只读 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`；延续现有 Application 本地内置增强、Clash typed 本地构建开关的单 Core Owner Checked Apply，不迁移 raw/override Mode/TUN/端口的专用副作用。修复 DIFF-154 仍需按**整个域**撤销未提交候选、同域多个 Checked 候选只能回退重建的问题。整个流程仍只使用原有 `CoreLifecycleClient → CoreLifecycle Actor → CoreFacade`，无分支、worktree 或并行 Core mutator。
- **生产检查身份**：`client/reconciler/check.rs::CandidateCheck::check_with_receipt` 在真实 Core Validator 完成后生成唯一 `chimera_core_manager::OperationId` 字符串，并经 `CheckedCandidatePort::checked_for_request` 随已校验的 `PreparedRuntime+CoreSpec` 发至 Core Owner，只有 ACK 后把标识返回源写入者。Application `patch_application_config_with_result`、Clash `patch_typed_config_checked` 在 CAS 成功后携带 **(domain, exact version, request)** 发布 `committed_with_receipt`；CAS 冲突或写盘失败仅 `abort_with_receipt(domain, request)`，保留同域其他检查者。Profiles 仍可使用原 `RuntimeGate::check/committed` 兼容通知；旧的 raw 和 legacy 栈继续独立预检，不捏造 receipt。
- **唯一 CoreLifecycle 邮箱与拒绝原则**：`CheckedRuntimeCandidate.request` 和 `Message::{CheckedSourceCommitted,AbortChecked}` 可带请求标识；`CheckedRuntimeInbox::take_request/discard_request` 精确匹配同域请求标识，拒绝重复标识入队。带标识的成功通知只取其指定 Checked 字节，其他同域候选不清空，也不依赖 FIFO 或同域唯一个数；旧通知仍遵循唯一候选/歧义重建规则。带标识的旧版本提交晚到时，只清理自身条目并触发已有 latest-authoritative-source 单 Owner 重规划；同版本重发不重播 Core。所有真正执行仍由原 `ApplyChecked` 的 `LiveSourceAuthority` 前后校验、S2 Owner fence、Stopped/uncertain/barrier 限制，不能用 receipt 绕过源权威。
- **测试 / 剩余边界**：新增 `same_domain_request_receipts_abort_one_and_apply_exact_committed_bytes` 在真实 CoreLifecycle Actor 中同时 ACK 三份 Clash 同域候选、撤销一份、对另一份发布 commit，验证只应用匹配 CoreSpec 的实际字节且保留第三份；`checked_inbox_rejects_duplicate_request_identity_without_touching_other_checks` 验证拒绝重复请求号与跨域隔离。旧 Profiles/legacy 路由、Service/Mode Effects、raw/override 仍未完整迁移；若 Core Owner RPC 发生不确定 ACK 或提交后通知发送失败，仍必须靠既有保守恢复和独立跨平台实机验收。上一批 `8adab786` GitHub CI Windows backend test 与 Tauri Desktop smoke 仍失败，获取详细 logs 受权限限制，不能把本地 Rust 通过解释成 CI 已修复。`CARGO_INCREMENTAL=0 cargo test --manifest-path backend/Cargo.toml -p chimera --lib --quiet` 在 macOS 本地 **1176 passed / 0 failed / 1 ignored**；`cargo check -p chimera --lib` 与新增真实 Actor 定向测试均通过。Clippy、签名及 push 以本批真实执行结果为准。

## DIFF-154（Runtime Reconciler）：Clash typed 构建选项迁入 Checked CAS / 单一 Core Apply，补未提交候选撤销（2026-11-21）

- **固定 ref / 范围 / 不做事项**：继续以只读 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01` 为基线，直接在本地主仓库 `master` 复制上游 `CandidateCheck → typed source CAS → RuntimeGate::committed → RuntimeReconciler::ApplyChecked` 的关键执行顺序。本批只切换 `patch_clash_config` 的 **typed** 请求，且必须仅显式设置 `enable_clash_fields`、`expand_include_all` 两种运行时构建开关、Application 未开启 Service Mode、Service IPC 未连接。其他 typed fields、TUN/端口/控制器、模式切换、raw/running overrides、旧 Verge 多域事务和 Service/Core Host handoff **全部保留既有执行路径**；不创建独立 Core Owner、不修改 `ref`、用户配置格式或 UI。
- **构建到提交的单次执行**：`backend/tauri/src/client/clash_config.rs::checked_clash_patch_eligible` 将请求剩余 Patch 严格与空 Patch 比较，防止显式的同值 TUN、mode、proxy 或其他控制操作混入新路径。`patch_typed_config_checked` 在持久化前用与 Profiles/Application 共用的生产 `CandidateCheck::check` 向唯一 CoreLifecycle 收件箱 ACK 原始校验字节和 CoreSpec；随后 `replace_prepared_if_version` 完成版本 CAS；**成功后立即** `RuntimeGate::committed(Clash, exact-CAS-version)`，并由原有 `CheckedSourceCommitted → ApplyChecked` 使用 Authoritative Source 前后栅栏与唯一 Core 执行者，而不是调用 `update_core_config` 做第二次构建。随后原有兼容镜像照常落盘，镜像失败记 `legacy_clash_mirror_write_failed` 且不能撤销已提交的 typed source。`ClashCommitReport.checked_version` 让 UI facade 跳过第二次旧 `SourceCommitted`；raw/override 通知逻辑不变。
- **失败撤销与并发保守策略**：检查前失败不会有 Checked 入队；检查 ACK 后的 Clash 或 Application CAS 版本冲突/写盘失败，都会调用 `CandidateCheck::abort_uncommitted`。`CoreLifecycleCheckedInbox` 仅向**原单一** CoreLifecycle mailbox 发 `AbortChecked`，`CheckedRuntimeInbox::discard_domain` 丢弃该源域所有未提交候选、保留其他两域。当前域未贯穿 operation token，故同域另一成功提交若也被保守清理，则后续 CheckedSourceCommitted 必须由既有 `Missing/Ambiguous → latest committed source` 恢复机制重建，而不能误执行其他已检查字节；这是为了正确性选择保守 fallback，不等于 ref 最终的逐 operation 精确撤销。Core 已确认的持久 CAS 版本通知不被撤销，跨域更新、S2 owner fence、Stopped 意图沿用此前行为。
- **验证 / 风险**：新增 `checked_clash_route_admits_only_local_builder_flags` 测试两项开关、Service/host 排除与 TUN/port/UI 额外字段排除；新增 `failed_source_cas_discards_only_its_domain_checked_proposals` 测试按域撤销和跨域候选保留，且 `aborted_source_cas_removes_proposal_in_real_core_owner_mailbox` 通过真正 CoreLifecycle Actor 的 ACK → Abort → 只读 RPC 验证消息顺序、跨域保留且 Core 不 Apply。`CARGO_INCREMENTAL=0 cargo check --manifest-path backend/Cargo.toml -p chimera --lib --quiet` 通过；`CARGO_INCREMENTAL=0 cargo test --manifest-path backend/Cargo.toml -p chimera --lib --quiet` **1174 passed / 0 failed / 1 ignored**。Rustfmt/Clippy/Prettier、签名和推送以最终真实执行结果为准；未声称 Service/TUN 实机、Windows 后端、Tauri E2E 或所有 Clash 编辑入口已迁移。上一批 `39d6b235` 的 CI 和 Tauri E2E 均失败，当前 GitHub 接口不能获取失败详细日志，必须独立排查；新 CI 也必须另行验收。

## DIFF-153（Runtime Reconciler）：本地 Application 增强配置首条 Checked 提交及 Core Apply 迁移（2026-11-21）

- **目标 / 上游 / 范围**：固定只读 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`，在现有 `master` 迁移上游 `state/application.rs::commit → client/reconciler/check.rs::CandidateCheck → RuntimeReconciler::Checked/SourceCommitted` 的完整执行顺序。**仅对不请求 Core 选择和 Service Host 切换、当前 Application 不启用 Service Mode 且 IPC Service 未连接的 `enable_builtin_enhanced` 更新**，实际改为 `check`（Core Owner ACK Checked 原字节）→ `replace_prepared_if_version` 持久提交 → `CheckedSourceCommitted(Application, exact-CAS-version)` → 现有唯一 CoreLifecycle Actor `ApplyChecked`。其他 Application 写入、Clash 的 legacy postcommit apply、Service/Core 迁移与多域混合 legacy Verge 事务本轮不切；不新建 Core Mutator 或改变用户磁盘数据格式。
- **实现 / 责任归属**：`backend/tauri/src/client/core_lifecycle/workflow.rs` 的 `checked_application_patch_eligible` 将可安全复用的本地 runtime-config 更新与需要专用 Service/Core handoff 的旧路径严格分开；Application 的生产 `CandidateCheck` 在选定路径调用真正的 `RuntimeGate::check`，其余继续调用 `validate_only`。`CoreLifecycleWorkflow` 在 CAS **确认持久提交后立即使用同一 Gate 的 `RuntimeGate::committed` 发布精确版本**，不依赖后续可能失败的 UI/source 读取；`ApplicationPatchResult::checked_version` 把真实 CAS 版本带给 UI Facade，只作为识别已通知的 checked 提交的凭据。`backend/tauri/src/client/application.rs` 对该版本**不再发送任何第二次提交通知**，既不重复 `CheckedSourceCommitted`，也不运行旧 `SourceCommitted` 的第二次 Core apply。`Command::PatchApplicationConfig` 标记 runtime 尚未收敛，只有后续真正的 Checked Apply 和 Source Authority 前后校验可结算本次运行时目标。持久化失败或版本冲突不通知 Checked Apply。
- **并发 / S2 / 保守恢复**：`backend/tauri/src/client/core_lifecycle/mod.rs` 将 `CheckedSourceCommitted` 的重复**同版本**与**已被较新源版本覆盖**明确区分：只有同版本重发被幂等忽略；较新 Source-only Application 提交先到邮箱时，丢弃本域已过期 Checked 候选、保留跨域候选，并依既有单 Core Owner / S2 未证明处理机制继续最新源重规划，不把旧 Checked 字节应用到新版本，也不冒充已收敛。保持 `LiveSourceAuthority` 对精确目标键的前后验证和 `MAX_CHECKED_PENDING` 限额；已通过 ACK 但 CAS 失败的孤立候选仍是后续需要基于 operation token 完成清理的边界，不能视为完整三域 Runtime Reconciler 迁移。
- **验证**：增加 `only_local_enhancement_requests_may_use_checked_apply`，覆盖本地增强、Service 连接、旧 Service Mode、Core 选择、显式 Mode 变化和非运行时请求的路由排除；此前 DIFF-150 已有真正的 Source Gate/Checked Actor 单次原字节提交与 Core Authority 前后验证测试。`CARGO_INCREMENTAL=0 cargo test --manifest-path backend/Cargo.toml -p chimera --lib --quiet` 在 macOS 上 **1171 passed / 0 failed / 1 ignored**，`cargo fmt`、`git diff --check` 通过；提交钩子及远端 CI 以本次实际提交为准。上轮 `1a76d1ab` 的 CI 和 Tauri E2E 最终均为失败，未取得需要管理员权限的日志，不能把本地通过视作跨平台可发布验收；实机 Service/TUN、Windows 测试和新路径端到端验收仍需补齐。

## DIFF-152（Runtime Reconciler）：Application 预提交检查接入生产 CandidateCheck（2026-11-21）

- **基线 / 目标 / 范围**：固定只读 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`，在 `master` 原有唯一 CoreLifecycle Actor 上继续上游 `state/application.rs::commit → client/reconciler/check.rs::CandidateCheck` 的迁移；本轮把 Application 的 runtime-affecting typed patch 预检查从 `legacy_check::check_domain_candidate` 切到与 Profiles、Clash 共用的实际生产 `CandidateCheck::validate_only`，包含统一的 read-only ProfileContentReader、RuntimeBuildService、CoreSpec 与 Core Actor validator，不另造 Core Actor/运行时 session。**Application 和 Clash 的持久化后 Core/Service Apply 仍是现有兼容路径**，不能宣称完整的 ref Source Gate/Runtime Reconciler Cutover。
- **执行接线 / 单一 Owner**：`backend/tauri/src/client/mod.rs` 在建立并连接 Profile MutationCoordinator 之前，通过 `CoreLifecycleClient::install_candidate_gate` 将**同一个** `Arc<CandidateCheck>` 发到 `ApplicationWorkflowActor`；该 Actor 仅允许在未关闭且 workflow 可用时安装；`CoreLifecycleWorkflow` 中共享 `Arc<OnceLock<Arc<CandidateCheck>>>` 跨克隆保持一次安装与不可重置。`backend/tauri/src/client/core_lifecycle/workflow.rs::check_application_candidate` 仍在旧 CoreLifecycle 串行提交命令内调用，但实际只读校验复用 ref Gate，且不向 Checked 收件箱写入候选，因此不会与旧 postcommit Core/Service 操作重放。Clash 的独立提交入口继续沿用 DIFF-151 的只读 Gate；Profiles 仍使用真正的 `RuntimeGate::check` + Checked 提交路径。
- **失败与测试边界**：真实生产工作流缺失 typed Profiles 或未安装 Gate 时直接拒绝，不能误将未检查的 Application source 当作成功；无 Session 的隔离 test workflow 继续使用 test-only 兼容检查分支。`application_validation_only_checks_core_without_queuing_a_runtime_apply` 验证提交前构建、Core positive/negative verdict 和 Checked 空邮箱。新的 Actor 安装与后续 Application/Clash 真正单路径提交必须在后续跨平台/Service 实测中继续验收。本轮不动用户 Profile 文件、系统权限、旧 UI、Agent、外部 Service 守护进程或 `ref` gitlink。
- **本地验证 / 风险**：在已安装的 macOS Rust/Node 环境中执行 `CARGO_INCREMENTAL=0 cargo test --manifest-path backend/Cargo.toml -p chimera --lib --quiet`：**1170 项通过、0 失败、1 项官方按设计忽略**；全量通过前修复了新单测局部变量遮蔽构造函数造成的 `E0618`，最终结果不含该编译错误。`cargo fmt`、Markdown Prettier、`git diff --check` 均已通过；Clippy、签名以提交结果为准。上一批 `5419fc8b` 的 CI `44950726480`（Windows backend test）与 Tauri E2E `44950726535`（desktop smoke）报告失败，且更早的 `e5ef5ed1` 两项也失败；GitHub API 当前对失败日志返回 403（缺少仓库管理员权限），无法定位根因或认定本轮已解决 Windows/E2E。新提交的远端结果必须另行检查。

## DIFF-151（Runtime Reconciler）：Clash 预提交校验复用上游候选构建，但暂不发出 Checked 入队（2026-11-21）

- **目标 / 范围 / 非目标**：仍以只读 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01` 为基线；共享 ref `client/reconciler/check.rs::CandidateCheck` 的完整输入捕获、构建、CoreSpec 解析、CoreValidator 判定，使现有生产 Clash typed/raw/override 写入口使用 Profiles 已连接的同一 Session 与 checker。Clash 当前依旧通过旧 postcommit CoreLifecycle 执行器处理 Core 应用与模式变化后的连接中断，因此**不能提前投递 `Checked` 候选**，避免新旧两个 runtime 处理路径重复 Apply。Application 的旧预检与提交路径、本轮不动的 legacy UI、Agent、Service 与持久化格式均保持不变。
- **路径 / 符号映射**：ref `backend/nyanpasu-core/src/client/reconciler/{check,preparation}.rs::CandidateCheck` → Chimera `backend/tauri/src/client/reconciler/check.rs::CandidateCheck::{validate,validate_only,check}`；`backend/tauri/src/client/mod.rs::ChimeraClientInner.checked_gate` 保存与生产 `MutationCoordinator` 安装的同一个 `Arc<CandidateCheck>`，而非另建一套 Runtime builder；`backend/tauri/src/client/clash_config.rs::check_clash_runtime_candidate` 调用只读 `validate_only`。正式 `RuntimeGate::check` 继续在验证成功后同步给唯一 CoreLifecycle Checked 邮箱 ACK。测试 fixture 无 canonical Session 时保留既有 legacy 检验接口，不伪造生产 Session。
- **安全边界 / 风险 / 回滚**：L2（Clash 预提交检查使用上游构建语义与真实 Core Actor 校验）；`validate_only` 不提交 Source、不发布 Runtime 产品、不分配第二个 Runtime owner，也不缓存 Checked 候选。两个检查路径共享同一条严格验证逻辑，只有生产 Gate check 发送 `workflow.checked`；避免未经接管的 Clash 候选充满最多 32 条的 Checked 收件箱。下一轮必须把 Clash/Application 的 durable source 通知、postcommit Effects/Mode 结算与 `Command::ApplyChecked` 改为单一执行路径，随后删除 `legacy_check`；对两个源域不能声称已经完成完整 Gate Cutover。回滚本轮验证接线与 `validate_only` 即可，不影响已持久化用户数据。
- **本地验证**：新增 `read_only_clash_validation_does_not_admit_checked_bytes` 检查构建器、validator、空 Checked/Committed 收件箱及后续正常入队。`CARGO_INCREMENTAL=0 cargo check --manifest-path backend/Cargo.toml -p chimera --lib --quiet` 通过；`CARGO_INCREMENTAL=0 cargo test --manifest-path backend/Cargo.toml -p chimera --lib --quiet` **1169/1169 通过**（1 项官方 TypeScript bindings 更新测试按设计忽略）；Rustfmt 与 `git diff --check` 通过。Clippy、签名、远端 CI 以实际提交结果为准。未运行真实 Service/TUN 或跨平台 GUI E2E。

## DIFF-150（Runtime Reconciler）：Profiles 生产 Source Gate 切换到上游式 CandidateCheck（2026-11-21）

- **目标 / 验收 / 非目标**：固定 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`，使生产 Profiles 写入调用 ref 型 `CandidateCheck::check`，并由同一个 CoreLifecycle Actor 接收 `Checked`、持久提交版本及 `ApplyChecked`；校验过的 CoreSpec、原字节与 Runtime Session 必须一致。保留 Application/Clash 旧提交流程、主/Legacy UI、Agent 和 Chimera Client/Service 支持；不更改 RPC、落盘格式、权限或宿主网络。
- **路径 / 接线 / 共享所有权**：ref `backend/nyanpasu-core/src/client/mod.rs::NyanpasuClient::try_new` 中的 `reconciler::check::CandidateCheck::new` 与 `reconciler/{check,preparation}.rs` → Chimera `backend/tauri/src/client/mod.rs::ChimeraClient::try_new_with_args` 调用 `reconciler::composition::prepare_production_gate`；它使用 CoreLifecycle 提供的唯一 `RuntimePreparationSession`、真实 `RuntimeBuildService` / `CoreCheckValidator`，以及与 `LiveSourceAuthority` 共用的 `FsProfileContentSource`。`MutationCoordinator::source_committed` 只向新 Gate 发送 Profiles 的 `CheckedSourceCommitted`，不会同时触发旧 `SourceCommitted` 重建。无 canonical Session 时初始化返回错误，不悄悄回退到独立副本。
- **风险 / 差异 / 恢复**：L2；只切换 Profiles Gate，不代表完整 ref `RuntimeReconciler` Actor、S2 Service、Effects 多域结算及 Application/Clash Gate 已迁移。失效、同域候选歧义和已前进的权威 Source 仍由唯一 CoreLifecycle Owner 保守重规划；真实安装 Service/TUN、跨平台 GUI 与发布包仍需独立验收。发生回归时可签名 revert 本轮 Source Gate 接线、注释与记录，不触碰已提交配置文件或 Core/Service 运行时资产。
- **本地验证**：`cargo check --manifest-path backend/Cargo.toml -p chimera --lib --quiet` 通过；Reconciler 定向测试 **63/63** 通过；完整 `cargo test --manifest-path backend/Cargo.toml -p chimera --lib --quiet` 在包含 `pnpm`、Prettier 的 PATH 下 **1168/1168** 通过，另 1 个官方 bindings 更新测试按设计忽略。首次全量运行曾因当前开发助手 PATH 缺失 `pnpm` 导致 `typescript_bindings_are_fresh` 无法启动 Prettier，不是 bindings 内容失败；补齐既有工具路径后该测试单独及全量均通过，未改断言或测试。crate Rustfmt、Markdown Prettier 和 `git diff --check` 通过；无真实桌面/Service 验收。后续需验证 source check → durable commit → checked Core apply 的真实用户链路，以及其余域的统一 Gate Cutover。

## DIFF-149（Runtime Reconciler）：SourceCommitted 完成邮箱版本栅栏与 S2 新源续接（2026-11-21）

- **固定基线与唯一执行者**：固定只读 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`，仍在 `master` 的现有 `CoreLifecycleClient → CoreLifecycleWorkflow → CoreFacade → LocalRuntimeHost` 迁移，不建分支、worktree 或第二个 Core mutator。生产 `MutationCoordinator` 暂时仍安装 `legacy_check`；本轮改动发生在已存在的 SourceCommitted/Checked 公用完成处理，不暗中启用新的 Gate。
- **修复 Core 回合完成邮箱里的新提交竞态**：DIFF-148 已在工作线程中增加 Checked Core 前后的权威 Source 校验，但新 `SourceCommitted` 消息仍可能在最后一次校验之后、同一 Actor 读取 `Message::Completed` 之前被接收。`ActiveOperation::observed_sources` 记录本回合启动时的单调 Source 版本；`source_committed_during_round` 对比它与 Actor 现有 `seen_sources`，只针对 `trigger=SourceCommitted` 的源应用回合判定消息级新提交。若旧 Worker 返回 `Ok` 但当前 Actor 已接收了新版本，转换为带上下文的 typed `CommittedTargetAdvanced`，然后才计算 Effects 端口刷新、状态结论和响应；不得以旧 Core 物理成功推出最新已提交配置 `Converged`。
- **S2 所有权、Effects 与续接**：延用 `CommittedTargetAdvanced` 现有的唯一 Core Owner 后续重规划语义：旧回合不清掉 `source_after_handoff`、不清理待恢复依赖、不发布旧 Runtime listener bindings，而将 `source_dirty` 保留到现有 `Command::ReconcileSource`（有 S2 Pending/Startup 租约时继续等待）；独立的 ServiceReadiness、Stop 和 ServiceRelease 不由 Source 版本差异篡改其原本结算。任何适用的重试仍须经过现有 Startup/FollowService/Service 发布约束，不创建额外 Core 执行通道。
- **跨域交错实际测试**：新增 `checked_core_completion_with_new_mailbox_source_requires_latest_reconcile`，在真正的 CoreLifecycle Actor 里先提交 Profiles v7 的 Checked 原字节并让模拟 Core 暂停返回；随后另一个 Application v1 durable notice 进入**同一邮箱**，通过只读版本 RPC 确认已被 Actor 处理，再释放 Core。测试 Authority 故意保留旧 Profiles v7 且前后两次校验均成功，证明只有 Actor 的完成阶段可以发现这次竞态；测试必须观察到 typed `CommittedTargetAdvanced`、首次 `Waiting` 与随后自动 latest `SourceCommitted` 重规划，且 Checked bytes 只提交一次。纯测试另验证跨域版本变化和 Service/Stop/ServiceRelease 各自的事件分类互不混淆。
- **明确剩余工作**：生产 `MutationCoordinator` **尚未切换新 CandidateCheck**；同域并发检查身份、S2 Service 真机处理以及 Effects 的完整跨域统一结算、旧 Gate 清理和跨平台实机发布仍需后续完成。实际 `CARGO_INCREMENTAL=0 cargo check --manifest-path backend/Cargo.toml -p chimera --lib --quiet` 通过；`cargo test -p chimera --lib client::core_lifecycle::tests::` **142/142**、`client::reconciler::` **63/63**、`core::actor_v2::local_runtime::tests::` **11/11**、`cargo test -p chimera-core --lib` **426/426**，`cargo test -p chimera --lib` 全量 **1168/1168**（另 1 个官方 Specta bindings 更新测试按设计忽略）。远端 CI 尚需等 GitHub Actions 运行结果。

## DIFF-148（Runtime Reconciler）：Checked Core 前后双 Source 身份确认与单一 Owner 最新目标恢复（2026-11-21）

- **固定单一 Core 执行者及上游基线**：固定 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01` 不变，在既有 `CoreLifecycleClient → CoreLifecycleWorkflow → CoreFacade → LocalRuntimeHost` 中直接扩充 `Command::ApplyChecked` 的生产执行逻辑；没有第二个 mutating Actor、分支、工作树或 Core 状态副本。
- **Checked 原字节 Apply 的前后权威校验**：DIFF-144 原有执行前 `RuntimeSourceAuthority::verify(domain,version,target)` 继续防止已知过期的候选进入 Core；新增成功 Core 提交**之后**对相同 `(domain,version,target)` 的第二次权威检查。只有前后均匹配的 Checked Apply 才能从本次命令返回成功。如果另一个 Source 在 Core 处理中提交、Profiles 托管内容变化或其他最新目标身份前进，权威读取器返回带上下文的 `CommittedTargetAdvanced`，不能把已确实生效的**旧** Core 配置误报为**最新已提交** Runtime 收敛。Core 提交不会在后置验证里重放或隐式重建。
- **Effects 只对当前目标结算**：`Message::Completed` 在计算并发送 Runtime port binding 给 `ApplicationWorkflow::runtime_bound` 之前识别 `ProfilesSourceAdvanced/CommittedTargetAdvanced`。源过期的 runtime 回合不向 Effects 发布已落后的端口绑定，也不调用 `runtime_reconciled()` 清除先前未结算的依赖；独立的 Service 探测、Shutdown 清理仍依其独立事件边界发布。`ReconcilerStatus::finish` 以 `Conclusion::Waiting` 而非 `Converged` 记录过期回合，避免 UI/提交回执使用旧目标声称已完成。
- **S2/最新提交后续处理**：对于这类 Source 在 Core 过程中已前进的 `Completed`，现有唯一 Actor 把 `source_dirty=true` 连同既有 `mark_runtime_dirty()` 保留到下一次串行 `Command::ReconcileSource`，**而不是**仅发送普通 `Retry`；这能穿过 `owner_transition_pending()` 的 S2 服务所有权门闩继续等待和重新规划权威最新版本，不会抢先打开另一个 Core mutator。真正的 S2 Service 租约、Service 释放和实际新旧 Gate Cutover 仍沿现有路径，未声称整体迁移完成。
- **回归覆盖**：新增异步 Core 模拟测试在 Checked 原字节已进入 Core、但尚未收到成功回执时修改权威 Source 版本，证实只有一次物理 Core 提交且二次校验返回 typed `CommittedTargetAdvanced`；不修改 Source 的对照测试验证前后两次校验均执行且可以成功。Effects/Status 测试验证过期 Source 不发布旧 Runtime 端口、不清掉独立 Service/Shutdown 事件，并维持 `Waiting` 非 `Converged`。原有 source-gated 原字节测试适配两次身份读取。实际已通过 `CARGO_INCREMENTAL=0 cargo check --manifest-path backend/Cargo.toml -p chimera --lib --quiet`；CoreLifecycle **140/140**、Reconciler **63/63**、LocalRuntimeHost **11/11**、chimera-core **426/426**，以及 `cargo test -p chimera --lib` **1166/1166**（另 1 个 bindings 更新测试按设计忽略）。Rustfmt、Clippy、签名和远端 CI 状态以实际提交/运行结果为准。
- **明确剩余项**：生产 `MutationCoordinator` **仍使用 `legacy_check`**，新 Gate 及带域 Checked 邮箱未接管生产全部配置提交；同域提交身份精确对应、S2 Service 真机运行、Effects 多领域结算、旧代码退役及 Windows/Linux/macOS 真机验证仍未完成。

## DIFF-147（Runtime Reconciler）：按 Source 域绑定 Checked 候选，交错提交拒绝错误 Apply（2026-11-21）

- **固定 ref 和单一 Owner**：保持 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01` 未改动；在既有 `CoreLifecycleClient` 的单一 `ApplicationWorkflowActor` 中迁移新 CandidateCheck 的多 Source 检查/提交对应关系，没有新建 Core mutator、分支或 worktree，也不绕过上轮共享 Runtime Session 和原字节 Apply。
- **消除 `checked: Option<_>` 单槽覆盖**：原来第二个 Checked ACK 会无条件覆盖第一个，即使由不同的 Source 域产生；现用容量上限 `MAX_CHECKED_PENDING=32` 的 `CheckedRuntimeInbox` 保存 `(SourceDomain, PreparedRuntime, CoreSpec)`。只有真实 ACK 才记录该源的候选；无 target identity / 队列已满返回 typed `RuntimeCheckUnavailable::CandidateUnavailable`，不能授权 Source 提交，也不能静默踢走已 ACK 的其他候选；Shutdown/PostStop 清空整个 inbox。
- **贯通来源类型**：`RuntimeGate::check` 从 `DomainChange::{Application,Clash,Profiles}` 选出对应 `ConfigDomain`，通过 `CheckedCandidatePort::checked(domain,prepared,core_spec)` 和 `CoreLifecycleCheckedInbox` 交给同一 Actor。`CheckedSourceCommitted(domain,version)` 只挑选属于**本域**的唯一 ACK 候选并连同其校验过的 CoreSpec 入现有串行 `Command::ApplyChecked`，不会从其他域错取、不会按全局后入先出覆盖。人工指定目标的已有兼容 Apply 也要求 domain 和 target 唯一匹配后才提取候选，保持其他待提交 Source 的候选不变。
- **同域交错时不猜测**：现有 `RuntimeGate::committed` 通知只有 domain/version，并未携带哪次 `check()` 通过的 target；如果同域存在多个仍未提交的 Checked，无法证明顺序绑定，因而拒绝挑选任何候选，将该域不确定候选全部丢弃，仅通过**同一个** CoreLifecycle Owner 的最新已提交 Source 重规划路径执行，其他域缓存不受影响。重复/旧版本通知保持 DIFF-146 的幂等保护。若某个候选执行前发现权威 Source 版本或 target 已前进，`LiveSourceAuthority` 返回有上下文信息的 `CommittedTargetAdvanced`，由既有 `Message::Completed` 的 `source_advanced` 分支重新调度最新目标，而不是把未曾提交的旧候选误记为 Core 拒绝。
- **新增回归**：收件箱单元测试覆盖跨 Application/Clash/Profiles 域相互独立、同域多个候选的拒绝猜测与其他域保留、32 个 ACK 后第 33 个失败不踢出先前候选、无 target 候选失败。真实 Actor + Profiles state manager + LiveSourceAuthority 集成测试先 ACK Profiles 和 Application（不同 CoreSpec binary path），按**相反**顺序接收 durable commit，验证两次 Apply 分别使用原先校验的 CoreSpec、没有额外旧 Reconcile、重复版本通知不会重复 Apply；新 Gate 的检查测试直接证明 Profiles/Clash 域被正确传递，Source 版本/target 变动作为 typed latest-target replan 分类。已验证 `cargo check -p chimera --lib` 通过，`cargo test -p chimera --lib` **1163/1163**（另 1 个需显式调用的绑定更新测试按设计忽略）；分类回归 `client::reconciler::` **63/63**、`client::core_lifecycle::tests::` **137/137**、`core::actor_v2::local_runtime::tests::` **11/11**、`chimera-core --lib` **426/426**、`client::runtime_build::tests::` **2/2**。
- **修复上一轮的实际 CI 失败**：核对 DIFF-146 的 GitHub Actions Run `44938459894`，三个平台 `Test backend workspace` 均由 `specta_export::tests::typescript_bindings_are_fresh` 失败触发（全部其余 1169 tests 通过，Tauri E2E 已成功）。使用本仓库被忽略的官方更新测试 `cargo test -p chimera --lib specta_export::tests::update_typescript_bindings -- --ignored --exact` 重新生成 `frontend/interface/src/ipc/bindings.ts`，使 Rust 导出的 NetworkAutomation 等新增类型与 TypeScript 一致。随后显式运行 `typescript_bindings_are_fresh -- --exact` 通过，未跳过、放宽或禁用该 CI 保护；并完成 `pnpm typecheck:interface` 与 `pnpm exec tsx --test frontend/interface/tests/clash-query-bindings.test.ts` **12/12**。新提交还需等待远端 CI 重新验收。
- **尚未进行生产 Gate Cutover**：`MutationCoordinator` 仍使用 `legacy_check`；本轮消除了新 Gate 必须解决的多域覆盖错误，但同域多个检查、Service S2 owner 交接、提交与执行跨域并发、效果结算及清理旧工作流仍需整体协调。多同域候选当前安全回退到唯一 Owner 的旧 latest-source 重建，不等于已经实现 pinned ref 的完整优化 Reconciler；未完成真机 Service、跨平台 GUI E2E 和发布包验收。

## DIFF-146（Runtime Reconciler）：新 CandidateCheck 生产依赖组装、Checked CoreSpec 与独立提交通知链（2026-11-21）

- **固定 ref 和单一执行者**：固定 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01` 未改动，对照上游 `client/reconciler/{check.rs,mod.rs,apply.rs}` 与 `client/core_lifecycle/workflow.rs`。不建分支/工作树，不启用第二个 Runtime Core mutator。旧生产 Gate 仍由 `MutationCoordinator` 安装；新 Gate 暂时只经独立组合测试验证。
- **新 Gate 的真正生产构造函数**：`client/reconciler/composition.rs` 的 `production_candidate_gate` 和 `prepare_production_gate` 用真实 `RuntimeBuildService`、`CoreCheckValidator(CoreClient)`、`EnhanceScriptRunner` 与二进制文件解析，并且要求 `CoreLifecycleClient::runtime_preparation_session()` 提供上轮共享 Revision/Ports Session、只读已提交 Application/Clash StateSnapshot、ProfilesContentReader 与同一受保护托管文件读源。Application/Clash 新增 read-only `committed_snapshot_handle` 方法，不向正在等待 Runtime 的写入 Actor 发送请求。Application/Clash 使用实际 `chimera_state_engine::StateSnapshot`，而 Profiles 继续使用其已有的 `chimera_core::state::StateSnapshot` 和文件读锁；没有第二份 StateManager 或影子提交状态。不支持 canonical session 的兼容 Owner 明确拒绝，不新建 Session 或悄悄退回旧输入。
- **消除 Checked 与 Apply 之间的二进制替换**：`CandidateCheck` 使用**相同的 CoreSpec** 给 CoreValidator 检查并随 `PreparedRuntime` 一起向 Checked Owner 发送 ACK；`CoreLifecycle` 的 `CheckedRuntimeCandidate {prepared,core_spec}` 在同一 Actor 内原子存储。`ApplyCheckedCommitted` 的入口不再接受调用者自行重传 CoreSpec，而是从原先已 ACK 的 Checked 缓存提取 CoreSpec；原字节、digest、目标、端口和二进制身份因此保持为同一次检查的提交参数。集成测试故意将验证过的二进制路径设为非默认值并核验 Core 收到的路径完全相同。
- **新增无双重 Apply 的 Checked SourceCommitted 通知**：`CoreLifecycleCheckedInbox` 将 `Checked` 和 `RuntimeGate::committed` 分别交给同一 CoreLifecycle Actor 的 `Message::Checked` 与**专用** `Message::CheckedSourceCommitted`。处理已提交版本时，按单调版本幂等去重，并将 Checked 产品连同已验证 CoreSpec 放入现有串行 `Command::ApplyChecked`，不再同时把这一通知投递给旧 `Message::SourceCommitted` 的独立 YAML 重建。Core Owner 状态不可靠、Source 过期、Shutdown/Stopped、队列积压或缓存/Authority 缺失时，仅沿用唯一 Owner 的保守延后/最新源重规划；不创建平行 Core 事务。真正执行前仍受 DIFF-144 的权威 Source 内容及版本校验，以及 DIFF-143 的 Core owner CAS、确认后产品发布保护。
- **验证范围与待处理的并发问题**：组合测试采用真正的 RuntimeBuildService 执行严格候选构建，证明 CoreValidator 检查的 bytes/digest 与 Checked 收件箱保存的 bytes/digest 完全一致，检查阶段没有 Runtime 产品写入、Core 确认或独立 revision 分配。生产式 CoreLifecycle 集成测试验证 Checked ACK→Profiles durable notification→**一次**原字节 Core Apply、原 CoreSpec、重复通知幂等且无旧 Reconcile；本轮 `CARGO_INCREMENTAL=0 cargo check --manifest-path backend/Cargo.toml -p chimera --lib --quiet` 通过；`cargo test -p chimera --lib client::reconciler::` **62/62**、`client::core_lifecycle::tests::` **133/133**、`core::actor_v2::local_runtime::tests::` **11/11**、`cargo test -p chimera-core --lib` **426/426**。`MutationCoordinator` **尚未实际切换新 Gate**：多个并行预检查候选在不同 Source 提交顺序下的目标复用、提交后变更与 Apply 的精确时序、Service S2 Owner 转移和 Effects 的统一结算尚需完成；不能把已准备的组合函数说成生产切换已结束，也未完成实机 Service、端到端 GUI 和跨平台发布验收。

## DIFF-145（Runtime Reconciler）：统一候选构建与 Core Host 的 Revision/Ports Session（2026-11-21）

- **对齐上游的共享所有权**：固定只读 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`，沿用上游 `client/reconciler/mod.rs::RuntimeReconcilerArgs` 的共享 `RuntimeRevisionAllocator` 和 `Arc<SessionPortResolver>` 思路，不建立额外 Core mutating Actor；既有 `CoreLifecycleClient → CoreFacade → LocalRuntimeHost` 仍是唯一 Core Apply 所有者。
- **同一 RuntimeLifecycle 真实句柄**：`RuntimeLifecycle::revision_allocator()` 克隆的是其内部 `Arc<AtomicU64>` 计数器，不创建新的序列；生产 `LocalRuntimeHost` 将原来的 `SessionPortResolver` 持有方式改为 `Arc<SessionPortResolver>`，并通过 `preparation_session()` 返回同一个 resolver Arc 和同一 revision allocator。`CoreFacade::preparation_session()`、`LegacyCoreBridge::runtime_preparation_session()`、`CoreLifecycleClient::runtime_preparation_session()` 将这两个句柄只读传递给候选构建层；不支持真实 LocalRuntimeHost 的兼容端口返回 `None`，不允许伪装为新会话。
- **新准备器拒绝隐式分叉**：`RuntimePreparation::new(reader,builder,session)` 现在**必须**接收同一个 `RuntimePreparationSession`，不再从 `RuntimeRevisionAllocator::new()` 创建独立修订序列，也不允许构造后用 `with_session` 覆写。新的 `CandidateCheck::Args` 只接收一份 session，而不是可随意拼配的 `revisions` 与 `ports` 两项；真实 RuntimeBuildService 测试、校验器测试以及 Apply 测试均改为从相同 `RuntimeLifecycle` 构造 session。
- **真正经过 Core 确认的端口与修订回归**：LocalRuntimeHost 集成测试从同一 resolver 解析候选，确认候选本身不改变运行中端口基线；调用真实 `reconcile_prepared`（模拟 Core 端点）并通过 Core 报告状态确认后，检查 receipt 内精确保存预先选定的 listener 绑定及 Snapshot Arc。随后再次从 Host 借用 session：`Arc::ptr_eq` 证明是同一个 resolver，未变化的策略复用确认端口；候选分配和 Core owner 侧 revision 分配连续递增。模拟 S2 从 Local 切换到 Service 后，所有先前借出的同一 resolver 立即看到已确认绑定失效，但 revision 序列不会重置。
- **专项验证**：`CARGO_INCREMENTAL=0 cargo check --manifest-path backend/Cargo.toml -p chimera --lib --quiet` 通过；`cargo test -p chimera --lib client::reconciler::` **61/61**、`client::runtime_build::tests::` **2/2**、`client::core_lifecycle::tests::` **132/132**、`core::actor_v2::local_runtime::tests::` **11/11**、`cargo test -p chimera-core --lib` **426/426**；测试执行的是模拟 Core 端点及单一生命周期 Actor，不代表完整服务端实机或 GUI E2E 已通过。
- **仍未切换生产 Gate**：本轮完成了实际 Core Host 状态共享入口、强制构造接口及真实确认后的端口/修订回归，**但现有 `MutationCoordinator` 仍使用 `legacy_check`**。`CoreLifecycleClient::runtime_preparation_session()` 已可为未来生产 `CandidateCheck` 提供唯一 session，尚未把 RuntimeBuildService/Validator/Source Gate 的整条生产装配切换完成；提交与应用之间的 Source 并发、S2 服务端细节、Effects 与清理旧实现仍待后续整体迁移。不得把本次 session 接口误报为生产 Reconciler 已接管。

## DIFF-144（Runtime Reconciler）：生产执行前读取权威 Source 版本与内容身份（2026-11-21）

- **固定 ref/唯一执行所有权**：保持 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01` 不变，对齐上游 `reconciler/{workflow.rs,apply.rs}` 的最新提交目标防护；没有创建第二个 Runtime Actor、分支或工作树。新的 `ApplyChecked` 仍由已有 `CoreLifecycleClient` 的唯一 mutating 命令槽调用 CoreFacade；继续维持上轮的原字节提交、Core 修订 CAS、Host generation CAS 及确认后出版。
- **权威目标身份读取器 `client/reconciler/authority.rs`**：定义必须显式注入的 `RuntimeSourceAuthority`、`LiveSourceAuthority`，在执行前读取已提交 Application/Clash 文档+单调版本、Profiles 文档及当前依赖闭包内的托管文件，再通过与上游一致的 `RuntimeInputs::target_key()` 重算完整目标身份。Profiles 文档与托管内容统一在原 `ProfileContentReader::committed()` 读锁内读取，防止 Profiles 写事务的文件提升与文档发布之间出现混搭。Application、Clash 在读取前后各采样一次，Profiles 在释放读锁后再检查一次版本；任何版本变动、内容摘要不匹配、托管文件读失败或缺失校验器都明确拒绝 Core Apply，不做隐式回退或重新构建另一份配置。读取器只观察源，不持有任何 Core mutation owner。
- **避免写 Actor 循环等待**：`ApplicationClient::committed_runtime_source()`、`ClashConfigClient::committed_runtime_source()` 从已有只读 `StateSnapshot` 一次性读取 `(document,version)`，不向可能正在等待 CoreLifecycle 的源 Actor 发送 `get` RPC。测试的 Static 后端同样在一把锁下返回内容及版本。
- **执行链安全装配**：`CoreLifecycleWorkflow::Command::ApplyChecked` 现在强制携带 Authority port，并在已取得唯一 CoreLifecycle 执行槽后、调用 `CoreLifecyclePort::reconcile_prepared` 之前执行 `verify(domain,version,checked_target)`；错误直接终止、不会提交 Core。生产 `CoreLifecycleClient::spawn_with_profiles_and_content` 将 Application/Clash Source 快照与 Profiles 内容读锁配成唯一可信 `LiveSourceAuthority`，组合根 `client/mod.rs` 注入 `FsProfileContentSource(paths.app_profiles_dir())`。正式 `apply_checked_committed()` 不接受调用者提供的伪造/放行读取器；无安装时明确失败。仅测试内部 helper 可以提供确定性的模拟 Authority，验证 mailbox 顺序及拒绝路径。
- **验收**：`cargo check --manifest-path backend/Cargo.toml -p chimera --lib` 通过；`cargo test -p chimera --lib client::reconciler::` **61/61**、`-p chimera --lib client::core_lifecycle::tests::` **132/132**、`-p chimera-core --lib` **426/426**。新增实时 Source Authority 测试覆盖相同提交目标、错误版本、错误目标、托管文件字节变化、文件缺失和 Profile 版本单调变动；Actor 测试验证邮箱版本没有变化但权威内容变化时拒绝 Core；生产装配测试用实际 Profiles state manager 和组合时注入的 Source 读取器，确认 Profiles 新版本的通知即使延迟也不会重复执行旧 Checked Runtime。`rustfmt`、Clippy 和 GPG 签名以实际提交为准。
- **明确未切换项**：本轮是已经接入 CoreLifecycle 唯一执行槽的真实权威验证及生产组装，**但 `MutationCoordinator` 仍使用 `legacy_check::CandidateCheck`，生产尚未调用新 `apply_checked_committed`**。新 `CandidateCheck` 还须与 `LocalRuntimeHost` 共享 Runtime revision/已确认端口基线；上游 staged content 集合与提交后权威 dependency closure 的目标身份规则需要统一；同时需处理提交与实际应用之间的并发 Source 更新、Service S2 Owner/Host 迁移和 Effects，完成整个 Actor mailbox 后才能安全切换生产 Source Gate 并清理 `legacy_*`。未执行真机 Service、全平台 GUI E2E 或发布包验收。

## DIFF-143（Runtime Reconciler）：接通原字节 Core Apply 与单一 Actor 执行路径（2026-11-21）

- **基准与所有权**：固定 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01` 未修改。对照 `ref/client/reconciler/{apply.rs,workflow.rs,mod.rs}` 和 `actor_v2/facade.rs::reconcile`，继续使用现有 CoreLifecycle 作为唯一 Runtime mutating owner，未启动第二个 Actor、未创建分支或 worktree。
- **真正按上游提交已检查的原始字节**：`core/actor_v2/local_runtime.rs::reconcile_prepared` 接收 `PreparedRuntime` 和 CoreSpec，在任何文件操作和 Core 提交前验证 target、Snapshot 产品 bytes、RuntimeIntent.config_text、digest、core_type 和 CoreSpec.kind。直接以原始 inline bytes + digest + LocalIpcSettings + 已观测的 expected_applied CAS、Core router 的 `expected_owner=(host,generation)` CAS 构造 `CoreSubmission`，不重新运行 Profile/YAML Builder。提交后按 Core 返回值和 fresh status 的 epoch、generation、source/effective hash、kind 确认，才创建包含同一 Snapshot Arc、端口、target 的 RuntimeApplyReceipt；沿用现有 `publish_profile_candidate` 做产品原字节 promotion、confirmed/applied 状态和 UI 兼容发布。状态无法确认时撤销有效绑定并将执行结果标为 uncertain，绝不伪造 Applied。提交前 CoreFacade 还要求选定 host 已连接，并把其权威代次送入 Core 提交的 owner CAS，防止读取状态与提交之间的 Owner 切换。
- **单一 CoreFacade/Actor 调用链**：在 `core/actor_v2/facade.rs` 通过现有 Core router 核验所选 host 已连接，然后通过同一 `run_local_mutation` 提交 prepared bytes；经 `client/core_lifecycle/adapters.rs::LegacyCoreBridge` 接入 `CoreLifecyclePort::reconcile_prepared`，不支持的端口显式拒绝。`CoreLifecycleWorkflow::Command::ApplyChecked` 沿用原 ApplicationWorkflow 的 Actor 串行命令槽，未绕开 Core/Service 所有权。`Message::ApplyCheckedCommitted` 从已确认接收的 checked 缓存搬运同一候选，仅当目标身份和所观测 Source 版本匹配、运行意图 Running、Owner 状态可用时进入正常队列；真正执行前 `drive()` 再次检查版本，拒绝排队时被新 Source 覆盖的旧候选，保证单次消费且不重建。
- **真实测试**：模拟 Core 端点检查提交的原始 bytes、IPC 选项、确认的摘要和真正发布的同一 Snapshot/磁盘文件完全一致；不一致候选不得提交，Core 虽返回成功却缺少 fresh status confirmation 时不得发布且状态必须 uncertain。单一 CoreLifecycle Actor 测试验证 Checked→SourceCommitted→ApplyChecked 的串行执行、无旧 `reconcile` 重建、旧版本/错误目标不得触发 Core 命令，及同一候选不可重复消费。实际验证：`CARGO_INCREMENTAL=0 cargo check --manifest-path backend/Cargo.toml -p chimera --lib --quiet` 通过；`cargo test -p chimera --lib client::core_lifecycle::tests::` **129/129**、`-p chimera --lib client::reconciler::` **60/60**、`-p chimera --lib core::actor_v2::local_runtime::tests::` **10/10**、`-p chimera-core --lib` **426/426** 通过，Owner CAS 专项端点测试 **3/3** 通过。
- **未完成的关键生产切换**：`MutationCoordinator` 仍使用 `legacy_check`，本轮新 `apply_checked_committed` 还没有生产调用。虽然匹配了 mailbox Source 版本和候选 target，但**尚未在执行时重新读取权威 Source 内容和 ProfileContentReader，重算 RuntimeInputs::target_key**；此外独立 Build/SessionPortResolver 需与 LocalRuntimeHost 共用已确认的 Runtime/端口基线，Service S2 owner 切换和 Effects 仍待整合。在这些条件满足前不得启用新 Source Gate 或删除 `legacy_*`；本轮并不宣称 RuntimeReconciler 已完全取代旧 CoreLifecycle，也未完成真实 Service/GUI E2E 和跨平台打包。

## DIFF-142（Runtime Reconciler）：将上游 Checked 候选交接迁入单一 CoreLifecycle Actor 邮箱（2026-11-21）

- **固定 ref 与源码顺序**：沿用只读 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`，在 `master` 没有新远端分叉的前提下对照 `ref/backend/nyanpasu-core/src/client/reconciler/mod.rs` 的 `Message::Checked(PreparedRuntime)`、`Message::SourceCommitted {domain,version}` 及 `workflow.checked` 缓存。上游先保存 checked 候选、再由已提交 Source 的 Actor 回合消费目标。Chimera 目前仍只有一个运行时写所有者 `CoreLifecycleClient`，不额外创建第二个 mutating Actor。
- **真实 CoreLifecycle 邮箱而非旁路缓存**：在现有 `ApplicationWorkflowActor::Message` 新增 `Checked { prepared, reply }`，在同一个 Actor `ApplicationWorkflowState` 内缓存最新的完整 `PreparedRuntime`（包含原 Snapshot Arc、精确 RuntimeIntent bytes/digest、端口候选及 target），同一邮箱继续处理既有 `SourceCommitted`。它不发布新产品、不确认端口、不执行 Core Apply；仅检查收件人的 `shutting_down` / `uncertain` 状态，并在退出与 Shutdown 时清除缓存。重复候选只保留最新一份，和上游单个 `checked: Option<PreparedRuntime>` 相同。测试专用 mailbox 读取证明数据确实被唯一 Actor 保管，消息接收本身不生成 Core `rebuild`。
- **比 ref 更严格的提交前确认**：新 `CheckedCandidatePort::checked` 改为 async，Source Gate 必须 `.await` 单一 Actor 真正保存候选的 ACK 之后才放行 Source 的持久化提交；原本单纯 cast 成功并不证明 Actor 已接收。已确认 send 失败返回 `RuntimeError::ShuttingDown`，请求可能已送达但未得到答复返回 `RuntimeError::OwnerUnresponsive` 并保留操作 ID，不把未知情况伪装为肯定关闭。现有模拟 Inbox 测试改为异步确认，验证拒绝、后端不可用、二进制缺失及关闭拒收时不会放行。
- **仍未切换生产 Source Gate / Apply**：生产 `client/mod.rs` 目前依然装配 `legacy_check::CandidateCheck`，因此新 Checked 消息只由明确调用新客户端方法的测试使用；绝不将其缓存误称为已应用。当前旧执行者的 `CoreLifecycleWorkflow` 仍会独立构建 Runtime，尚不能消费新 `PreparedRuntime.intent`。在一个明确的 CoreFacade 提交链能够使用同一份 checked bytes、确认端口/发布产品和保存 Source 版本之前，不得切换生产 `MutationCoordinator` 到新 CandidateCheck，也不能将新消息映射为旧 `ReconcileSource`（那会造成 check/apply 配置漂移）。下一步需要迁移 `ref/reconciler/{workflow.rs,mod.rs}` 的统一提交、恢复及 CoreLifecycle 端口、完成 Owner cutover，再移除 legacy 路径。
- **验证边界**：`cargo check --manifest-path backend/Cargo.toml -p chimera --lib` 通过；预提交新 Source Gate 4 项测试通过；新增 2 项真实 CoreLifecycle Actor 邮箱测试验证 Checked 接受 ACK 后先于 SourceCommitted、原数据完整保留、不调用 Core Apply、后续候选替换以及 Shutdown 拒收。`cargo test --manifest-path backend/Cargo.toml -p chimera --lib client::core_lifecycle::tests::` **127/127**、`-p chimera --lib client::reconciler::` **60/60**、`-p chimera-core --lib` **426/426**；未验证 GUI E2E、真机 Service、Runtime 实际交付及跨平台打包。

## DIFF-141（Runtime Reconciler）：迁移上游 Startup Round 与 Prepared Apply 结果/复用边界（2026-11-21）

- **基准与协同**：先将远端包含 Network Automation DIFF-140 的两笔并行提交快进合并到 `master`；固定 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01` 无任何变更。此条为 Runtime Reconciler DIFF-141，不覆盖另一开发者的 DIFF-140 记录。重点仍是优先直接复制上游源码，然后根据 `chimera_core`/`chimera_contracts` 的真实接口窄适配。
- **上游 `reconciler/startup.rs` 标准路径**：从 `ref/backend/nyanpasu-core/src/client/reconciler/startup.rs` 直接迁入 `StartupReport`、`StartupOutcome`、`StartupObservation`、`Round`、`cause` 和从 `Conclusion` 到启动结果的原始投影规则；绑定 Chimera 已有的共享 `ConvergenceHealth`、`ExecutionHost`、`ServiceEvidence`、`CoreStatusProjection`、`EngineVersions` 和 `RuntimeError`。Waiting/Blocked/Unknown/Closed 等结论保持上游的不同健康状态和原始诊断，没有将“未证实”的 Core 收敛当作 Ready。暂时不从旧 CoreLifecycle 的启动邮箱消费新类型，避免混淆两份状态机。
- **上游 `reconciler/apply.rs` 标准路径**：迁移 `PreparedCandidate`、`RuntimeReconciler::prepare_candidate` 主要实现（现在显式接收 `RuntimePreparation`、checked 缓存和上次选择）、已检查候选按 `RuntimeInputs::target_key` 复用的目标身份防护，以及 `not_submitted_outcome` 的四种依赖等待分类。针对 Chimera 的实际 `CoreFacade::ReconcileResult`，实现与上游 `submit_prepared` 对应的结构化结果投影：仅真实 `Reconciled` 得到 `Applied` 并保留原始 `RuntimeSnapshot` Arc 和 Started/Restarted/Switched 重建证据；`RolledBack` 得到 `CoreRolledBack`，`Unknown` 得到 `SubmissionUnobserved`，`Unchanged` 得到明确拒绝；原始 Core kind、retryable、operation ID 不丢失。checked 缓存仅在目标存在、与最新提交的 Source 身份精确匹配时消费，不匹配时重新执行上游 `RuntimePreparation`，随后再次解析 CoreSpec；Build 失败目前仍由已有 `RuntimeError::new` 保守映射，待上游完整 BuildRuntime typed variant 与执行者一起迁移。
- **真实所有权边界**：上述新模块是**已编译且经过测试的上游准备与结果分类组件**，它们**尚未调用生产 `CoreFacade::reconcile` 或发布 runtime 产品**。Chimera 当前生产仍由原 `CoreLifecycleClient` 唯一执行 Core Apply，MutationCoordinator 仍使用 `legacy_check::CandidateCheck`；本轮**没有启动第二个 Reconciler Actor**，也没有把未经新邮箱实际消费的 Checked 视作生产生效。完整 `ref/client/reconciler/{mod.rs,workflow.rs,apply.rs,startup.rs}` 的副作用调用与 `ref/client/core_lifecycle/workflow.rs` / Owner/Service 恢复集成仍待整体切换。切换后才能删除 `legacy_{check,preparation,status,mutation}` 等适配模块。
- **针对性验收**：`CARGO_INCREMENTAL=0 cargo check --manifest-path backend/Cargo.toml -p chimera --lib --quiet` 通过；`cargo test -p chimera --lib client::reconciler::` **60/60**、`cargo test -p chimera --lib client::core_lifecycle::apply::tests::` **5/5**、`cargo test -p chimera-core --lib` **426/426**；`client::reconciler::startup::tests` 验证 Converged、Inactive、Waiting、NotInEffect、Unknown、Closed 与 typed cause；`client::reconciler::apply::tests` 验证候选身份匹配只消费一次、缺失目标不可匹配、实际 RuntimePreparation 的匹配复用/过期重建、16 种 NotSubmitted kind/retryable 组合的等待分类、Reconciled 新旧 Core 身份、RolledBack/Unknown/Unchanged 真正不确认。未做完整 Core/Service 实机所有权切换、GUI E2E 或发布包测试。

## DIFF-142：Network Automation 策略持久化、提交验证、变更订阅与 IPC 隐私门槛（2026-11-22）

- **固定上游 ref / 领域模型**：以 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01` 中 `nyanpasu-config/src/application/mod.rs::network_automation` 为参照，在 `backend/chimera-config/src/application/mod.rs::ChimeraAppConfig` 增加 `#[serde(default)]` 的 `NetworkAutomation` 字段和 whole-document `ChimeraAppConfigPatch.network_automation: Option<NetworkAutomation>`。旧 `application.yaml` 缺失时保持禁用且规则为空；新配置可保存多条禁用规则、更新默认动作、清空整份规则，不与旧 `verge.yaml` 或 Clash 对应字段产生第二套来源。无自动迁移已存在的用户文件、无对子模块或 Core 的改动。
- **单一 Actor 的提交级验证和通知**：按上游 `nyanpasu-core/src/state/application.rs` 的提交边界，在 `backend/tauri/src/state/application.rs` 统一验证 *完整候选* `network_automation.validate()`，覆盖普通 Patch、Replace、PrepareReplace 和 CAS 版本化 Replace；测试/Static config client 使用同一校验函数，启动时对已落盘候选 fail closed。规则 ID/名称缺失、重复、无条件、非法网关 MAC、规则数超过上限、TUN + 透明捕获双启均拒绝，且失败时不写磁盘、不改版本、不通知。当前缺少生产环境观察器及敏感读/系统写权限 RPC，因而进一步**明确拒绝任何 `enabled=true`**；保存合法禁用规则不触发捕获或 Core Runtime。对齐上游 `subscribe_settings_changes()` 的 `watch` 语义：提交真正持久化并应用现有 legacy mirror 后才发布完整 typed snapshot；失败、CAS 冲突均不通知。生产订阅口已可用但**尚未被 Network Automation Actor 接管**。
- **敏感数据隔离**：`ChimeraAppConfig::unprivileged_settings_view()` 用于普通 Tauri `get_app_config`，始终返回默认的空/禁用网络策略，不泄露用户的规则 ID、SSID、网关 MAC、条件或 network action。通用 `patch_app_config` 直接拒绝携带 `network_automation` 的 patch，防止未来绕过专用 `system:write` 命令写入网络策略。内部 typed 配置 Owner 可持久化合法的禁用规则，后续必须增设**独立带授权的读/写接口**供正式规则编辑器使用，而非放宽通用 IPC；现有其它应用设置 IPC 合同、系统代理/TUN/Service 入口不变。
- **验证证据**：Windows 离线、关闭增量编译、限制 2 个编译作业，`cargo test --manifest-path backend/Cargo.toml -p chimera-config --lib application::tests:: --locked --offline` **8/8**、`-p chimera --lib state::application::tests::` **8/8**、`-p chimera --lib client::network_automation` **22/22**、`-p chimera --lib ipc::tests::` **24/24**；覆盖旧 YAML 默认、whole-document 替换、普通读取脱敏、未经授权写入拒绝、真实 YAML 持久化、invalid/enabled 拒绝保持版本/文件、CAS 冲突通知约束以及旧网络控制路径。Rustfmt/Clippy 与 Git 检查按提交钩子最终记录。CI 尚存独立的 TypeScript bindings 过期、Windows migration_child 与 IPv6 桌面 E2E 失败；本轮未覆盖并行任务未提交 `frontend/interface/src/ipc/bindings.ts`。未进行真实网络切换/Service、跨平台真机/GUI E2E。
- **未完成**：启动时真实 `network_environment` 注入、持久化 network policy `enabled=true` 的权限验证与迁移路径、独立 `sensitive_read/system:write` RPC、配置变更订阅到唯一 NetworkAutomation Actor、规则管理组件和手动 apply/wifi access；只有授权/回滚闭环和真机验收完成后，才可移除临时的启用拒绝并开始生产自动网络切换。

## DIFF-139：按固定 ref 修复 Core/Proxies Actor 关闭语义并复制 Runtime Apply 中断边界（2026-11-21）

- **固定基准和协同主线**：在 `master` 同步并行 Network Automation 四笔提交之后，以未改动的 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01` 为唯一源码对照。此轮不启动第二个 RuntimeReconciler、不修改生产 CoreLifecycle 的单一运行时写所有权，也没有删除尚需兼容的 Source Gate；只迁移原生 CoreOwner 生命周期和上游 Apply 边界所需的真实运行逻辑。
- **修正上轮两个真实的 Actor 回归**：直接对照 `ref/.../actor_v2/mod.rs::post_stop`，CoreActor 退出时对 `handoff` 使用 `abort` 和 `await`，不再等待一个从未释放的 Core Stop；保留 Chimera 的 pump 和 API lease panic/revoke 清理。对照 `ref/.../proxies.rs::post_stop`，ProxiesActor 关闭时取消/等待只读 monitor 和 timer，但不再发出虚构的 `invalidate_cache` 变更事件，订阅者在 actor sender 消失后收到正确的关闭结果。上轮失败的 `actor_stop_cancels_an_in_flight_handoff` 与 `queued_cancelled_mutation_is_not_executed_and_shutdown_closes_subscriptions` 两项分别复测通过，`chimera-core` 库全量测试 **426/426 通过**。
- **连接中断真正对齐上游 typed 协议**：`chimera-core::actor_v2::api::ApiClient::get_connections` 返回 `ApiResult`，`connections::{interrupt_connections, interrupt_by_chains}` 返回 `Result<(),ApiError>` 而不是将 Stale/Timeout/Unavailable/Protocol 丢进 `anyhow`。`ConnectionScope::for_proxy_change` 直接采用上游 `ProxyChangeBreakMode` 的单一映射，ProxiesActor 和 Tauri `core::connection_interruption` 共用该映射；Tauri 旧外层接口保留窄 `anyhow` 适配，不影响 IPC API。Typed `interrupt_failure` 保留真正的 Core API 错误类别，失败后不重复连接关闭请求。
- **上游 CoreLifecycle Apply 原源码接入标准路径**：将固定 `ref/backend/nyanpasu-core/src/client/core_lifecycle/apply.rs` 的 `RuntimeApplyOptions`、`RuntimeApplyContext`、`PreparedInterruption`、前置 `api_client_if_running()` 捕获与 Core Reconcile 成功后的 `finish_interruption` 搬入 `backend/tauri/src/client/core_lifecycle/apply.rs`。因 Chimera 旧 `CoreLifecycleWorkflow` 暂时仍持有 legacy trait port，而 ref 的新执行者持有 `chimera_core::actor_v2::facade::CoreFacade`，将 `prepare_apply` 的 facade 参数显式化；实现本身不增加 mutating actor。遵守上游：已停源（`Ok(None)`）不是错误、未知/不可用 API 不伪造成功、确认 runtime 替换旧进程时不再关闭退役 API，真正未替换时只在捕获的源实例执行断连。
- **独立边界和未完成项**：该 `apply.rs` 是以后新 `RuntimeReconciler` 执行路径的准备端口，**目前未从生产旧 CoreLifecycle 调用，也未完成 `reconciler/{mod.rs,workflow.rs,apply.rs,startup.rs}` 的统一 Actor 所有权切换**。只有在源提交、checked-product 缓存、CoreFacade 观察/提交与 Service owner 共同迁入一个 mailbox 后才能安全启用并删除 `legacy_*`。不能把复制出来的 Apply 边界误称为生产 Runtime Apply 已迁移。
- **验证**：`cargo check --manifest-path backend/Cargo.toml -p chimera --lib --quiet` 通过（本轮上游连接错误类型适配后）；`cargo test --manifest-path backend/Cargo.toml -p chimera-core --lib` **426/426 通过**；新增的 Apply 上下文测试验证无断连请求、已停止的旧实例、不可用 API、已被新实例替换，以及真实 HTTP 控制器在同一源实例上的 DELETE；格式/Clippy 和提交签名在最终提交时检查。尚未进行全平台 GUI E2E、实机 Service/网络切换和发布包验收。

## DIFF-140：迁入 Network Automation 的 ConfigCaptureSwitches，并接入 Chimera 唯一 typed 配置 Owner（2026-11-22）

- **上游对齐的执行顺序与互斥事务**：以固定 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01` 的 `ref/backend/nyanpasu-core/src/client/network_automation/switches.rs` 为蓝本，新增 `backend/tauri/src/client/network_automation/switches.rs` 和 `mod.rs` 模块导出。保留上游 `CaptureSwitches::apply` 先处理 `system_proxy`，再将 TUN 与透明代理写入一个 `ClashConfigPatch` 的动作顺序及 `MechanismOutcomes` 形状；`Keep/Enable/Disable`、关闭互斥机制、重复请求不写、独立失败不抹掉另一动作等决策与 ref 一致。由于 Chimera 的真实提交/Runtime 验证归属于 `ChimeraClient`，其 `ConfigCaptureSwitches` 只持有同一个 `ChimeraClient` 句柄，通过 `patch_app_config` / `patch_clash_config`（而不是创建第二套 Source Actor / Core/OS 写者）操作，复用应用镜像同步、配置候选验证、TUN host 预检、SourceCommitted 通知、运行时重建和 Effect 发布。
- **透明捕获的安全边界**：透明捕获由 Disabled 转为启用时必须有 Linux Service 及已确认的 `get_transparent_proxy_status().supported`；受拒时只返回 `Failed`、保留原 TUN/透明捕获状态，但允许独立的 TUN 动作继续执行。默认采用 ref/现有 UI 使用的 Tproxy 模式，首次启用且无端口时补齐固定 7894 监听策略；禁止拆分“启用 TUN、关闭透明捕获”或反向的同一事务；保留原 `redir_port` 与 `tproxy_port` 配置，禁用不清理监听端口。已提交但出现运行时或镜像降级的 `MutationOutcome` 映射到含原因的 `Failed`，不谎报 OS 已应用。实际系统代理写入仍由原有 Effects/SystemProxy Actor 处理，当前迁移没有绕过原有系统代理互斥。
- **验证**：`CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo test --manifest-path backend/Cargo.toml -p chimera --lib client::network_automation --locked --offline` **22/22**；含上游 Actor 原 13 项、纯开关互斥/端口/保留配置/降级结果 6 项及使用真实 `ChimeraClient` typed 测试 fixture 的 3 项集成测试；`cargo check -p chimera --lib --locked --offline` 通过。变更文件 Rustfmt、Git diff check 通过，提交钩子 Rustfmt/Clippy 结果记录于对应 commit。未进行 Linux 真机捕获、跨平台系统代理实际开关、完整 E2E；本模块**还未生产实例化**，因此没有自动修改网络。
- **剩余必需部分**：在用户可启用前迁移 `ChimeraAppConfig.network_automation` 的持久化/校验/系统写权限投影、策略更改通知、真实 `network_environment` 依赖注入、Actor 生命周期开关和严格 `sensitive_read/system:write` RPC；只有完整闭环后才迁移前端规则编辑器并打开生产定时策略。本次未修改 ref/Runtime 子模块、Tauri setup、Service 权限或 UI bindings，原未提交 `frontend/interface/src/ipc/bindings.ts` 原样保留。

## DIFF-138（第一阶段）：复制 RuntimeReconciler 状态、操作记录和 Host/CoreSpec 共享错误契约（2026-11-21）

- **固定 ref / 单一 Owner 边界**：本轮以只读 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01` 为模板。核对上游 `client/reconciler/{mod.rs,workflow.rs,plan.rs,apply.rs,startup.rs}` 与 `client/core_lifecycle/{workflow.rs,apply.rs,mod.rs}`，发现上游的 CoreLifecycle 是供 Reconciler 使用的约 700 行执行器，而 Chimera 原 CoreLifecycle 内有大量独立的启动/源提交/恢复与 Core Apply 所有权。**本轮只迁移可独立编译且不新增执行所有者的共享数据与纯策略**：尚未启动第二个 RuntimeReconciler、没有将生产 Source Gate 接向尚未实现的 checked inbox，也没有把上游的 workflow/apply/startup 宣称已在生产切换。现有 CoreLifecycle 仍是唯一 mutating owner。
- **上游 CoreSpec/Host/RuntimeError 的归属直接迁移**：将 `ref/backend/nyanpasu-runtime-values/src/{core_spec.rs,execution_host.rs}` 的公开结构复制到 `backend/chimera-contracts/src/{core_spec.rs,execution_host.rs}`；原 `chimera-core::client::core_spec::CoreSpecError` 和 `chimera-core::actor_v2::endpoint::ExecutionHost` 改为重导出**同一类型**，而非出现双份 Host/错误枚举。保留 Chimera 新增 Core 选择对不可表示为 UTF-8 的可执行路径的 `InvalidPath` 真实错误。共享 `RuntimeError` 恢复上游已有真实源类型的 `ResolveCoreBinary`、`InstallCoreBinary`、`PrepareServiceInstallPrompt`、`ReadCoreVersion`、`MoveHost`，其中 `ResolveCoreBinary::core_kind()` 返回上游 `BinaryNotFound`；生产候选 `reconciler/check.rs` 在 CoreSpec 失败时返回真正的 `ResolveCoreBinary`，不再误标成候选检查不可用。Host 失败携带真实 `CoreFailure` 和 Core Operation ID。
- **上游 `reconciler/mutation.rs` / `status.rs` 复制归位**：整份复制上游 `mutation.rs` 的 `AppliedCandidate`/`ApplyOutcome`/按 `OperationId` 保存的 `RoundRecord`，以及 `status.rs` 的 `EngineVersions`、`ReconcilerStatus` 和 `outcome`，只映射 `chimera_core`、`chimera_contracts` 的已迁入包路径与类型。新增共享 `DegradationReason::RuntimeDeferred {cause: Arc<RuntimeError>}`，使 Source 已提交但 Core `Waiting`/`Unknown`/`Closed` 时有真实结构化原因、并按上游区分可重试性，而不是把提交直接等同于 Applied。现有 CoreLifecycle 使用的旧 `status.rs` / `mutation.rs` 分别重命名为 `legacy_status.rs` / `legacy_mutation.rs` 并显式重连消费方，旧 UI/服务状态订阅和核心提交次序不被新投影意外改变。
- **`committed.rs` 纯策略上游拷贝**：上游 `Selection` 与 `SelectionChange::interruption` 原逻辑复制到现有 `reconciler/committed.rs`，以模式、Profile 选择及明确的 `break_connection` 策略决定中断类型；服务端释放/最后确认更新仍由原 CoreLifecycle 控制，没有增加第二个 Service 操作者。既有 `chimera_core::client::reconciler::plan.rs` 与 ref 的 `plan()` 和 `plan_owner()` 规范化对比，核心规划顺序已一致，主要差异是跨 crate 错误适配、可见性与针对 Chimera 的回归测试，因此保留这份唯一纯规划器，不复制出第二套计划算法。
- **验证证据**：`CARGO_INCREMENTAL=0 cargo check --manifest-path backend/Cargo.toml -p chimera --lib --quiet` **通过**；`cargo test -p chimera-contracts --lib` **28/28**、`cargo test -p chimera-core --lib client::core_spec::tests::` **3/3**；Tauri `reconciler::check`（含找不到 Core 可执行文件的 typed 拒绝）4/4、`mutation` 2/2、`status` 3/3、上游 Selection 3/3、旧 `legacy_status` 4/4、旧 `committed` 6/6、旧 `legacy_check` 5/5，合计 **27/27** 定向通过。`cargo test -p chimera-core --lib` **423/425，通过 423、失败 2**，不是全量绿灯：`actor_v2::tests::actor_stop_cancels_an_in_flight_handoff` 与 `proxies_actor::tests::queued_cancelled_mutation_is_not_executed_and_shutdown_closes_subscriptions` 分别独立复跑仍失败。前者当前 `CoreActor::post_stop` 等待正在进行的 handoff，但固定 ref 使用 abort+await，涉及已发出 Stop 命令的所有权语义，本轮不靠改测试/盲目 abort 来伪造通过；后者发生在没有本轮源码改动的 ProxiesActor 关闭路径。未做完整桌面 E2E、真机 Service/网络或发布打包。
- **下一阶段 DIFF-138 的明确未完成事项**：一次性迁移固定上游的 `reconciler/{mod.rs,workflow.rs,plan.rs,apply.rs,startup.rs,committed.rs}` 剩余有副作用部分，连同实际 `CoreFacade`/CoreLifecycle 端口、RuntimeSnapshotStore、Owner/Service 恢复与完整 `RuntimeBuildError` 强类型，确保唯一 Reconciler Actor 的 `Checked(PreparedRuntime)` 与 `SourceCommitted(domain,version)` 同邮箱顺序收敛，再将生产 Source Gate 接入，移除 `legacy_{check,preparation,status,mutation}` 和旧 inline apply。另需专门分析上述两个可复现 Actor 停止/订阅生命周期失败，并以真实行为验收，而非单纯修改断言。

## DIFF-139：完整复制上游 Network Automation Actor 与跨平台网络观察器（2026-11-21）

- **固定基线、复制清单和端口结构**：锁定 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`。将 `ref/backend/nyanpasu-contracts/src/network_environment.rs` 完整复制到 `backend/chimera-contracts/src/network_environment.rs`，将 `ref/backend/nyanpasu-runtime-values/src/network_automation.rs` 完整复制到 `backend/chimera-contracts/src/network_automation.rs`；其余仅替换 `nyanpasu_config`→`chimera_config`，不改字段、序列化形状、隐私投影。将 `ref/backend/nyanpasu-core/src/client/network_automation/{actor.rs,ports.rs}` 完整迁到 `backend/tauri/src/client/network_automation/`，仅适配包名和现有 `crate::utils::blocking::join`；独立 typed client 与上游 Actor message 保持 `set_policy`、`status`、`apply_now`、`request_wifi_name`、双轮相同观测才 settle、五秒轮询/取消清理语义。上游 `tests.rs` 前 13 项使用注入的 fake environment / RecordingSwitches 原始行为测试移植，其余依赖生产适配的原始测试留给真实 CaptureSwitches 的后续迁移。
- **上游 OS 观察器（可选功能）**：从 `ref/backend/nyanpasu-os/src/network_environment/{mod.rs,wifi/{mod.rs,cache.rs,guid.rs,linux.rs,macos.rs,unsupported.rs,windows.rs}}` 逐文件复制到 `backend/chimera-os/src/network_environment/`，仅包名适配；维持默认路由物理网卡选择、过滤隧道/虚拟接口、Wi-Fi 位置授权按需读取、SSID TTL 缓存与 macOS/Windows/Linux 系统专用实现。依照上游设置 `chimera-os/network-environment` **opt-in Cargo feature**，默认构建不开启，依赖 `netdev`、Linux `zbus`、macOS ObjC 等通过条件依赖引入，Windows WLAN API 使用额外必要 Win32 feature；Cargo.lock 保留原有无关包版本，仅新增/补充观察器依赖。
- **不能提前激活的边界**：与上游不同，Chimera 尚未拥有用于 Network Automation 的 `ConfigCaptureSwitches` 真正系统配置 Owner 适配、与 typed `ChimeraAppConfig` 同一事务中验证/保存的 network_automation policy、设置变更通知、网络观察环境注入和 `system:write` / `sensitive_read` 严格 RPC 与权限投影。所以**没有在生产 setup 中 spawn Actor**；没有向前端暴露可操作的 Network Automation 假开关，也没有引入第二个 SystemProxy/TUN/TransparentProxy 执行者。Actor 仅经测试 fake ports 工作，OS observer 仅按显式 feature 编译。下一批应先把捕获开关动作通过现有 `ChimeraClient::patch_app_config/patch_clash_config` 和单一系统代理 mutation lock 完成适配/故障回滚，再接持久化策略提交和权限端口；依赖都闭环后才启用生产 Actor 与规则编辑器。风险级别：isolated L1/L2 实现，未触碰用户配置的现存结构、Legacy UI、Core 运行时或子模块 gitlink，回滚可按模块 revert。
- **真实验证与环境阻碍**：最初全并行 Rust 测试因本机 D 盘目标缓存爆满报 `os error 112`；仅执行 `cargo clean --manifest-path backend/Cargo.toml -p chimera` 清理该包生成的编译缓存，未删除源码或 Git 内容。随后使用 `CARGO_INCREMENTAL=0`、`CARGO_BUILD_JOBS=2` 验证，`cargo check -p chimera --lib --offline` 通过、Actor 定向 13/13、`chimera-contracts` 28/28、`chimera-os --features network-environment` 30/30，`chimera-os --features network-environment --locked --offline` 通过。安全摘要新增 3 个测试分别核对完整状态序列化、摘要不泄露 Wi-Fi/网关/规则/错误文本、无观察器的 unsupported 降级。全部变更文件 Rustfmt 与 `git diff --check` 通过；未跑跨平台真机、Windows 位置权限系统弹窗、生产自动网络切换和桌面 E2E。CI 状态以最终 GitHub Actions 为准。与本批无关的 `frontend/interface/src/ipc/bindings.ts` 预存未提交改动保持不动。
## DIFF-137：上游 Runtime Source Gate 与结构化端口错误迁移（2026-11-21）

- **固定 ref 与复制范围**：以只读 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01` 为准，直接复制 `ref/backend/nyanpasu-runtime-values/src/ports.rs::{PortField,PortResolveError}` 到 `backend/chimera-contracts/src/ports.rs`，并将现有 `chimera-config::PickPortError` 的 serde tagged wire 对齐上游 `#[serde(tag="kind", rename_all="snake_case")]`。将 `ref/client/reconciler/check.rs` 的完整 Source Gate 检查/构建/校验/checked 交接顺序作为新的 `backend/tauri/src/client/reconciler/check.rs` 底稿；原 `check.rs` 搬到 `legacy_check.rs`，生产核心、Profiles Source Actor、Clash config 和启动装配的旧调用点都显式指向 `legacy_check`，避免仅在文件名层面宣称对齐。
- **结构化端口错误已进入生产候选解析**：`client/ports.rs::SessionPortResolver::resolve_candidate` 依上游六种 `PortField` 返回 `Result<CandidatePortBindings,PortResolveError>`；每个选择失败都保留真正的 `PickPortError` 来源，重复 listener 端口返回 `PortConflict {first,second,port}`，候选解析仍不写 confirmed binding。仅旧 `core/clash/core.rs::promote_and_start_locked` 现有 `RuntimeRestartError::Prepare(anyhow::Error)` 桥使用 `.into()`，不损失 typed error source。`chimera-contracts::runtime_error::RuntimeError` 按上游补齐共享层可直接拥有的 `ResolvePort`、`PublishRuntime`、`CoreRolledBack` 类型；没有制造跨 crate 循环，也没有把构建/发布失败伪装成 Core 通过检查。
- **上游 `CandidateCheck` 的必要适配**：直接复用 DIFF-136 的 `RuntimePreparation::prepare_candidate_inputs` 和 DIFF-135 的 `RuntimeValidatorPort`，所有候选的 Source 其它领域使用已提交快照，Profile 文件内容在持久化锁下捕获，staged bytes 覆盖候选，然后构建精确 `PreparedRuntime`，使用 `RuntimeIntent` 的同一份字节调用 Core check。与上游 `RuntimeReconcilerClient::checked` 对应的唯一暂未接通依赖抽象为 `CheckedCandidatePort::{checked,source_committed}`，不提供隐式成功的空实现：只有运行时所有者接受并保存检查通过的准备结果才放行；拒绝、验证不可用、构建失败与 Actor 关闭都会拒绝。这条复制路径**本轮仅编译并通过隔离测试，未注入生产 `MutationCoordinator`**，旧 CoreLifecycle 仍是唯一 Runtime apply owner；未启动第二个 Reconciler Actor，亦未将 `check` 结果丢弃后重新宣布通过。
- **验证证据和边界**：`cargo test --manifest-path backend/Cargo.toml -p chimera-config --lib` **249/249**、`-p chimera-contracts --lib` **25/25**、`-p chimera --lib` 定向 `client::ports::tests::` 10/10、标准 `reconciler::check::tests::` 3/3、旧 `legacy_check::tests::` 5/5、准备链 2/2、真实 runtime build 2/2，共 **22/22** 不重复测试通过；`CARGO_INCREMENTAL=0 cargo check --manifest-path backend/Cargo.toml -p chimera --lib --quiet`、单文件 Rustfmt、`git diff --check` 通过。未做全量平台/GUI E2E、真机服务端接管或发布包验收，不代表 `RuntimeReconciler` Actor 已完成迁移。
- **下一轮**：必须把固定 ref 的 `reconciler/{mod.rs,workflow.rs,plan.rs,apply.rs,startup.rs}` 与 `core_lifecycle`/CoreFacade、typed RuntimeError、Service/recovery 所有权**作为一个可编译的整体**切换；让生产的 `CheckedCandidatePort` 由唯一 Reconciler inbox 消费并把 checked product 与 SourceCommitted 版本匹配，移除旧 `legacy_check`/`legacy_preparation` 及内联 Runtime 应用，防止两套系统同时生效。此切换前保留旧 CoreLifecycle 所有权，不以静态文件或预检查测试代替 Runtime 的真实确认。

## 2026-11-21：Network Automation 纯策略模型原样迁移与 Proxy 透明代理入口对齐（partial）

- **上游基线与完整拷贝**：固定 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`。将 `ref/backend/nyanpasu-config/src/application/network_automation.rs` **整文件原样复制**到 `backend/chimera-config/src/application/network_automation.rs`，只在 `application/mod.rs` 声明并导出模块、`chimera-config/Cargo.toml` 复用工作区 `snafu` 依赖，Cargo.lock 只给 chimera-config 依赖表增加该项。原文件与新文件 `git diff --no-index` 无差异。类型覆盖 `NetworkAutomation`、`NetworkRule`、`NetworkConditions`、`NetworkObservation`、`NetworkState`、`MechanismAction(s)` 及错误；校验包含 64 条上限、ID 唯一、SSID/MAC 完整性、TUN 与透明代理互斥。该文件是**未连接到持久化配置或运行时 Actor 的纯模型**，没有改变现存默认系统代理/TUN 行为，也没有修改现有用户配置文件结构。
- **前端按上游交互复制，保留本地唯一状态所有者**：对照 `ref/frontend/nyanpasu/src/pages/(main)/main/settings/system/proxy/_modules/{transparent-proxy-button,proxy-bypass-config}.tsx`，新增 Chimera 同路径组件。Linux 透明代理入口接到现有 `useTransparentProxyAction`，共用 `ProxyButton` 的已应用状态/等待样式；操作可行时执行现有 typed Clash patch 与 Service 状态二次确认，受阻时显示 ref 风格的原因弹窗与 Service/Network 设置导航，不虚构系统已应用。复用现有仅 Linux/Mihomo/支持宿主可见、Service 状态可核验、开启不得与 TUN 冲突的 fail-closed 约束。代理绕过表单直接移植 ref 的 React Hook Form + Zod 控制/提交/重置结构，仍通过 `useSetting('system_proxy_bypass')` 单一来源；旧 `system/_modules/proxy-bypass-config.tsx` 改为兼容再导出，综合页与子页面指向同一组件。
- **刻意不连接的高风险边界**：上游 `network_automation` 配置写入、网络探测 Actor、机身 Wi-Fi/网关识别、带 `sensitive_read` 与 `system:write` 的 RPC，以及规则编辑/应用状态/开机恢复的全链尚未复制；目前**没有 Network Automation 控制面，也不允许触发自动切网或自动写系统代理**。下一轮必须以完整后端所有权、权限投影、用户同意与可验证恢复为前提，再迁入前端 `useNetworkAutomation` 和规则编辑器。Chimera 既有 Linux 捕获状态 hook 是这一轮唯一产生现实操作的路径；没有第二个捕获执行者。回滚可分别 revert 此次前端和纯模型提交，不涉及数据迁移。
- **已运行的真实检查**：`cargo test --manifest-path backend/Cargo.toml -p chimera-config --lib application::network_automation --offline` **8/8**，`cargo check --manifest-path backend/Cargo.toml -p chimera --lib --offline` **通过（仅仓库既有 warning）**；`pnpm --filter=chimera-ui build`、`pnpm typecheck`、`pnpm lint:frontend-boundaries`、Oxlint、变更范围 Prettier、`git diff --check` 通过。透明代理准入/已应用状态定向单测 **6/6**、网络接口归一化 **3/3**。**未运行真实 Linux 桌面 WebDriver/Service/TProxy 真机操作**，也未启用 Network Automation；CI 和 E2E 须以本次推送后 GitHub 的最终结果为准。原未提交 `frontend/interface/src/ipc/bindings.ts` 原样保留，不在提交中。


## DIFF-136：复制上游完整 RuntimePreparation、RuntimeBuildService 与 PreparedRuntime（2026-11-21）

- **固定上游与源码映射**：以只读 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01` 为准，将 `ref/backend/nyanpasu-core/src/client/reconciler/preparation.rs` 完整的来源捕获、候选端口解析、构建策略、`RuntimeIntentBuilder` 和 `RuntimePreparationPort` 直接作为 `backend/tauri/src/client/reconciler/preparation.rs` 底稿；上游 `client/core_lifecycle/ports.rs::{RuntimePreparationPort,PreparedRuntime}` 复制到 Chimera 同名端口模块；上游 `client/runtime_build.rs::{RuntimeBuildService,RuntimeBuildPort impl}` 复制到新文件 `backend/tauri/src/client/runtime_build.rs`。旧的 246 行 Chimera 内容捕获工作流仅移到 `client/reconciler/legacy_preparation.rs`，两个旧调用方改为指向兼容模块，避免在正式切换前造成两个 Core Apply 所有者。上游 `ref`、Runtime 子模块 gitlink、Legacy UI 和 Agent 未修改。
- **Chimera 必需类型适配**：沿用现有 `chimera_config`/`chimera_core_manager`/`chimera_core::actor_v2::intent`/`chimera_contracts::publish`；Chimera 的 RuntimeSnapshot 仍持有完整 product bytes 和 Mapping，而上游持有单独的 `config_text`，准备链直接使用该快照的原始 UTF-8 product bytes 构建 RuntimeIntent，不另做 YAML 再序列化，保证检查与将来 Core 提交/发布使用相同字节。新 BuildService 保留上游注入 content、CoreSpec、脚本运行器、候选构建和发布的端口边界，构建在 `spawn_blocking` 中创建/丢弃脚本运行器；使用 Chimera 已有内置脚本表和 `artifact_to_snapshot_data`，只在最终产品边界做一次序列化。由于共享的 `RuntimeError` 还未迁入依赖于 `RuntimeBuildError`/PortResolveError 的所有真实类型，新的 `RuntimePreparation` 构建失败暂返回带上下文的 `anyhow::Result`，而 `RuntimeBuildPort`/`RuntimePreparationPort` 保持各自上游的强类型构建、发布和 CoreSpec 错误。给 `chimera-core` 的构建错误添加真实 worker/script/artifact/serialize 失败变体，没有用错误文本假装 Runtime 已验证。
- **端口/发布边界**：上游产品原子写入复制到 `client/runtime.rs::write_product`，使用 `tokio::fs::create_dir_all`、独立 blocking atomicwrite 和上游 `PublishRuntimeError`；只有显式 `publish()` 才写最终 runtime 目录，候选构建不发布也不触发 Core 提交。`RuntimeRevisionAllocator` 按上游改为 `Arc<AtomicU64>` 可克隆共享计数器，但暂保留 Chimera 旧调用链依赖的显式溢出错误返回。提交时不建立第二个 Reconciler Actor，不切换启动/退出/重试/SourceCommit 的旧邮箱。
- **回归证据**：`CARGO_INCREMENTAL=0 cargo check --manifest-path backend/Cargo.toml -p chimera --lib --quiet` 通过；上游准备链的构建字节/候选严格策略 2/2、旧文件捕获/锁定 3/3、新真实 BuildService 和 RuntimePreparation 跨模块构建及延迟原子发布 2/2、旧统一候选检查 5/5、Runtime lifecycle/revision 19/19，共 31 项不重复定向测试通过；Rustfmt、`git diff --check` 验证。此次未执行完整 Windows/Linux/macOS GUI E2E、真机网络或打包测试。
- **下一轮的整体切换边界**：`ref/client/reconciler/check.rs` 依赖 `RuntimeReconcilerClient::checked(PreparedRuntime)` 使实际通过检查的产品与之后应用的产品具有同一身份；Chimera 尚无该上游 Actor 的 checked inbox，所以没有把新 CandidateCheck 同时连接到旧 CoreLifecycle，也没有无声丢弃 checked 结果。应整体复制并适配 `reconciler/{check.rs,mod.rs,workflow.rs,plan.rs,apply.rs,startup.rs}` 与上游 typed RuntimeError/PortResolveError 后，在一个唯一所有权切片里切换配置检查和 postcommit Core Apply；之后删除 `legacy_preparation`/旧内联路径。
## 2026-11-21：Settings System Proxy / Service 上游分区迁移（前端 partial）

- **基线、映射与目标**：固定 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`，对照 `ref/frontend/nyanpasu/src/pages/(main)/main/settings/system/{proxy,service}/index.tsx` 与 `settings/_modules/settings-navigate.tsx`，迁入 `frontend/chimera/src/pages/(main)/main/settings/system/{proxy,service}/index.tsx` 及上游相同的 System 导航分区。目标是迁入整页职责、页面排列与现有真实控制项，不创建第二套业务状态；风险 L1（前端路由/交互/i18n/E2E），不改 Core、Service、系统代理宿主状态、后端、授权模型、配置文件或子模块。
- **实施和兼容**：`/main/settings/system/proxy` 复用共享系统代理/TUN 开关、模式切换、Proxy Guard、bypass 与当前代理状态；`/main/settings/system/service` 复用 Service 模式/控制、开机自启、静默启动、DNS 和 Windows UWP 工具，保留原 AppImage Service 入口显示门槛及 Windows/macOS 工具平台条件。导航拆分为 Proxy、Service 两项，文案复制上游对应的五语言四条消息并重新生成 Paraglide/TanStack route tree。原 `/main/settings/system` 综合页保留，由父路由精确匹配时渲染；子页面通过 `Outlet` 显示。这样 Dashboard、Legacy UI 与已有 E2E 的旧深链仍能访问已存在的共享 UI/API，不复制配置持久化、Agent 或 Runtime 逻辑。
- **上游未覆盖差异 / 回滚**：ref Proxy 页面使用 `useNetworkAutomation`、NetworkRuleList、NetworkDefaultActions、状态/规则编辑器、Linux TransparentProxyButton，且有完整 `ReadOnlyScope`；Chimera 当前无相应 network automation typed query、caller-scoped permission 契约，不能通过仅渲染占位开关声称实现，也不能伪造授权。因此保留这些作为独立后续迁移项；上游组件在 Chimera 仍通过原 `system/_modules` 引入，待业务 API 所有权一致时按相同目录继续搬迁。Service 路由保留现有 `useIsAppImage` 条件与系统服务兼容 hook，不引入 ref 的另一套操作路径。只撤销本批源码/翻译/测试/文档即可回滚，不涉及真实用户设置。
- **验证约束**：本轮已执行 `pnpm --filter=chimera-ui generate:i18n` 与 `pnpm --filter=chimera-ui build` 成功；新增可见点击 E2E 契约从 Proxy 导航到 Service 再返回，并检查 URL、选中项和显示的页面控件。后续检查结果以提交前最终命令记录为准。真实桌面 WebDriver、实际代理/TUN/Service 模式交互及恢复测试在本机未执行，不应表述为已验收。原有未提交 `frontend/interface/src/ipc/bindings.ts` 与本批无关，保持不动。
## 2026-11-21：Settings 应用配置分层与 ref 页面职责对齐（前端 partial）

- **基线 / 映射 / 风险**：固定 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`，对照 `ref/frontend/nyanpasu/src/pages/(main)/main/settings/nyanpasu/{interface,tray,shortcuts,logs,public-ip}/index.tsx` 与 `settings/_modules/settings-navigate.tsx`，映射到 Chimera `frontend/chimera/src/pages/(main)/main/settings/chimera/` 同名子页面与导航。L1：只改 React 页面、路由、i18n 和桌面 UI 测试，不碰 Rust、IPC 合同、配置存储、用户数据、Core/Service、agent/legacy 共享业务实现或子模块。
- **实施**：新增 ref 风格的 Interface、Tray、Shortcuts、Logs、Public IP 五个可访问页面；导航相应拆分为独立菜单项、保留 Chimera 综合设置入口和 Debug；直接复用现有语言/主题/CSS、窗口关闭、托盘代理、快捷键、应用日志、核心日志、流量保留及公网 IP/STUN/查询控件，全部经原有共享 `useSetting`/IPC，不引入第二个配置来源。相关上游导航描述和两个日志分组标题使用 ref 五语言消息，重新生成 Paraglide 和 TanStack route tree。
- **向后兼容与剩余差异**：原 `/main/settings/chimera` 综合页面和 `/main/settings/user-interface` 独立旧页面继续可用；Chimera 父路由只在子页面匹配时使用 `Outlet`。当前托盘控件没有上游完整 WebView tray menu 选择器/图标编辑；Custom CSS 仍使用 Chimera 既有卡片而非 ref 的预设/多 CSS 列表；应用更新和远程访问的上游页面依赖尚未全量迁入的应用 API 和权限体系，不伪造管理功能或授权 UI。旧综合路径继续支持现有 Main/Legacy/E2E 深链接；后续可逐页移动组件到 ref 相同目录，待业务 API 一致后再删除兼容重复内容，而不移除受支持的 Legacy UI。
- **实际验证与未覆盖**：`pnpm --filter=chimera-ui generate:i18n`、`pnpm --filter=chimera-ui build`、`pnpm typecheck`、`pnpm lint:frontend-boundaries`、`pnpm --filter=chimera-ui test:localization`（6/6）已通过；修改的 Settings E2E 契约覆盖托盘和公网 IP 的可见导航，E2E 类型检查通过。改动范围的 Prettier/Oxlint、E2E harness 单测及实际远程 CI 以本轮最终记录为准。**没有运行真实桌面 WebDriver E2E**，不可视为桌面交互已验收。预存且与本批无关的 `frontend/interface/src/ipc/bindings.ts` 修改保持不动。回滚为本批签名 revert，无用户数据转换。


## DIFF-135：直接复制 ref 的 RuntimeBuildPort、RuntimeValidatorPort 与 CoreCheckValidator（2026-11-21）

- **上游基线与复制清单**：固定 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`，直接以 `ref/backend/nyanpasu-core/src/client/reconciler/ports.rs`、`client/reconciler/adapters.rs` 和 `ref/backend/nyanpasu-runtime-values/src/publish.rs` 为底稿，迁入 `backend/tauri/src/client/reconciler/{ports.rs,adapters.rs}` 与 `backend/chimera-contracts/src/publish.rs`。保留上游类型名、方法职责、失败码区分及序列化结构；未触碰 `ref` 和 gitlink。
- **类型与 crate 适配白名单**：`nyanpasu_config`→`chimera_config`，`nyanpasu_core_manager`→`chimera_core_manager`，`RuntimeBuildError`→`chimera_core::client::runtime_build::RuntimeBuildError`，`CoreSpecError`→`chimera_core::client::core_spec::CoreSpecError`，`RuntimeIntent`→`chimera_core::actor_v2::intent::RuntimeIntent`，`RuntimeCheckUnavailable`→`chimera_contracts::runtime_error::RuntimeCheckUnavailable`；上游 `PublishRuntimeError` 使用现有 `chimera_contracts::profiles::error::ErrorPath` 与 `atomicwrites::Error`，`backend/chimera-contracts/Cargo.toml` 引入工作区锁定的 `atomicwrites`，Cargo.lock 只增加共享包已存在的依赖项。上游 `RuntimeBuildPort` 的构建、捕获、CoreSpec 和发布类型接口已完整声明，旧 `RuntimeContentPort` 暂时仅作为已启用的 CoreLifecycle 文件读取适配保留。
- **只读验证适配**：`CoreCheckValidator` 的判断结构（通过、确定性拒绝、超时待重试、后端不可用/不支持）按上游复制；由于 Chimera 的 `CoreClient` 在 `chimera-core` crate，而 `EndpointHandle` 保持 crate 私有，在 `chimera-core::actor_v2::CoreClient` 暴露只读 `check_config(CheckSubmission)`，由 adapter 复用初始 endpoint 进行一次 advisory validation，不新增 Core submit/apply 能力或第二个运行时执行者。为错误分类提取一个纯函数以测试 CoreErrorKind 和超时语义，未将新 validator 在生产 Gate 中重复启用。
- **验证与迁移边界**：`cargo check --manifest-path backend/Cargo.toml -p chimera --lib --quiet` 通过；`cargo test --manifest-path backend/Cargo.toml -p chimera-contracts --lib` 22/22，以及 `client::reconciler::adapters::tests::` 2/2 通过；Rustfmt 与 `git diff --check` 通过。上游 `RuntimePreparation::prepare_candidate_inputs`、Typed `PreparedRuntime`、`RuntimeReconciler` 的单一执行/已提交收敛所有权及下游 `check.rs` 核心实现未在此切片启用，需连同上游 CoreLifecycle 的类型和生命周期迁移完成，才能移除旧 RuntimeContentPort / CoreLifecycle inline apply。未做完整 GUI E2E 或真机代理行为验证。

## DIFF-134：优先复制 ref 的 Runtime Inputs、Profile 锁与 Source→Runtime 数据契约（2026-11-21）

- **固定上游和复制清单**：本轮对照只读 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`，优先使用上游源码作为目标文件底稿，而不是继续手写相似实现。实际复制了 `ref/backend/nyanpasu-core/src/client/reconciler/inputs.rs` → `backend/tauri/src/client/reconciler/inputs.rs`、`client/reconciler/profiles.rs` → 同名新文件、`state/mutation/profile_content.rs` → 同名文件，以及 `state/mutation/runtime_apply.rs` → 同名新文件。`ref` gitlink、子模块及用户配置均未改动。
- **尽量保留上游语义**：`RuntimeInputs` 与 `FrozenProfileContent` 直接使用上游数据形状，`target_key()` 恢复上游返回 `Option<String>` 的未能序列化即无身份语义，其投影由已经迁入的 `state::mutation::impact` 统一提供。老 `CoreLifecycle` 的 5 个调用点通过局部 `checked_target_key()` 适配为现有 `anyhow::Result`；`ApplicationWorkflow::tcc` 从 `Ok/Err` 分支适配上游 `Some/None`，不恢复另一套旧的哈希算法。上游 `ProfileContentReader` 保持 `committed`/`candidate` 锁保护与版本读取实现，`profiles.rs` 直接提供上游三种 Profile 降级码到 typed `chimera_contracts::runtime::DegradationReason` 的映射，旧桌面 wire 由 `application_workflow/profiles.rs` 的薄适配调用 `from_shared`。`runtime_apply.rs` 的 `RuntimeApplyReceipt` 作为后续 Reconciler 接口记录，当前无第二个 Runtime 运行所有者。
- **列明仅有的兼容差异**：Chimera 现有 `ProfileContentSource`/Profile dependency closure 的捕获仍由 `chimera-core` 提供，在上游 `FrozenProfileContent` 增补 `capture_content` 以维持 staged 文件字节覆盖及冻结读取；对旧验证器保留 `into_parts()`，对旧 `unwrap_err` 测试派生 `Debug`，并保留原有冻结/目标身份测试。上游 `ProfileContentLock::write()` 返回借用守卫，但 Chimera 的 detached Profile persistence owner 会把守卫跨任务保存直到文件提升/回滚完成，因此**唯一锁语义差异**是 `OwnedRwLockWriteGuard`，否则存在 `E0597` 借用逃逸编译错误；读锁和读取快照照搬上游。`runtime_apply.rs` 的 `ExecutionHost`/`AppliedConfigBinding` 从 Chimera 的 `chimera-core::actor_v2` 引入，仅品牌/包结构映射。
- **执行与验证**：`CARGO_INCREMENTAL=0 cargo check --manifest-path backend/Cargo.toml -p chimera --lib --quiet` 通过；`client::reconciler::inputs::tests` 3/3、`client::reconciler::preparation::tests` 3/3、`state::mutation::profile_content::tests` 3/3、`client::application_workflow::profiles::shared_profile_degradations_tests` 1/1、`client::reconciler::check::tests` 5/5、Application precheck→postcommit Core 定向回归 1/1，共 16 个不同测试通过。Rustfmt 及 `git diff --check` 通过。没有执行完整 Windows/macOS GUI E2E、网络实机或打包。
- **下一步/未声称已完成**：上游 `client/reconciler/{ports.rs,preparation.rs,check.rs,workflow.rs,plan.rs,apply.rs,mod.rs}` 与本地 BuildPort、PublishRuntimeError、Core/Service API 所有权依赖紧密，尚未整体复制激活；这批文件需连同 `RuntimeBuildPort`、Typed Publication Error 和唯一 RuntimeReconciler 任务完整迁移，不能为了文件名相同放一个未启用的第二套 Runtime。保留原 Legacy UI / Agent / Chimera Client/Service 在窄适配端，待 Runtime 所有权切换再移除旧的内联 Core 应用路径。
## 2026-11-21：Settings Clash 子页面与上游导航第一阶段对齐（前端 partial）

- **固定基线、范围和风险**：使用主仓库记录的 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`，对应 `ref/frontend/nyanpasu/src/pages/(main)/main/settings/{_modules/settings-navigate.tsx,clash/{network,core,runtime,core-manager,control}/index.tsx}`。Chimera 对应 `frontend/chimera/src/pages/(main)/main/settings/{_modules/settings-navigate.tsx,clash/{route.tsx,network,core,runtime,core-manager,control}/index.tsx}`；导航与分页面按 ref 布局/职责迁移，复用现有 Chimera 设置控制组件和共享 IPC。L1 前端路由、交互及测试范围；不更改 Core/Service、应用数据、持久化格式、Runtime 或两个子模块指针。
- **实际迁移**：Clash 导航由单一综合入口拆为 Core Manager、Core、Network、Control、Runtime 五个上游分页面；相同的上游十条标签/说明翻译拷贝到五语言消息目录，Paraglide 和 TanStack route tree 已经重新生成。Network 复用既有端口、LAN、IPv6、TUN、透明代理设置；Core 复用运行字段、日志级别和连接中断策略；Runtime 复用过滤、include-all 和内置增强；Control 复用现有 External Controller、API Secret、Web UI；Core Manager 复用已有核心管理控件。所有设置继续经原 typed application hooks/IPC，未复制第二套状态/持久化入口。
- **过渡适配与回滚**：现有 `/main/settings/clash` 综合页面仍可直接打开；父路由仅在子页面匹配时通过 `Outlet` 显示 ref 子页面，以保护 Dashboard、Legacy 入口及旧 Windows E2E 的固定 URL。已有 `/main/settings/web-ui` 同样保留为可访问兼容路径，但不在新导航中重复显示。此阶段上游完整 capability/read-only 授权、Core latency-test/MIPS 调节、Clash AI，以及设置项的按上游文件目录移动尚未完成；Chimera 尚无等价 caller-scoped RPC 权限，不能以仅隐藏按钮伪造授权。后续需逐项确认相应真实 API 后再收敛。回滚只撤销本次路由/导航/翻译/测试和此记录，不涉及用户数据。
- **实际检查和边界**：`pnpm --filter=chimera-ui generate:i18n` 与 `pnpm --filter=chimera-ui build` 成功，`pnpm typecheck`、`pnpm lint:frontend-boundaries`、本地化测试 6/6、Tauri E2E harness 单测 44/44、Oxlint（存在已有 Inspect React hook warning）、改动范围 Prettier 和 `git diff --check` 通过。设置页面可见点击契约已更新为新 Control/Network 路由并通过 TypeScript 编译，**没有在本机运行真实 WebDriver 桌面 E2E**；构建、单测和 E2E 类型检查不能证明窗口中的实际点击/激活已验收。原本已脏的 `frontend/interface/src/ipc/bindings.ts` 不属于本批，未覆盖、未纳入提交。
## DIFF-133：上游 runtime impact 投影与 Application / Clash Source Actor 条件持久化结果（2026-11-21）

- **固定上游与路径映射**：本轮固定 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`，核对 `ref/backend/nyanpasu-core/src/state/mutation/impact.rs::{RuntimeDomain,reaches_runtime,application_target,clash_target,profiles_target}`、`state/{application.rs,clash_config.rs}::{ApplicationSnapshot,ClashConfigSnapshot,commit}`。对应 `backend/tauri/src/state/{mutation/impact.rs,mutation.rs,application.rs,clash_config.rs}`、`client/{application.rs,clash_config.rs}`。风险 L2：配置 Actor CAS、返回的来源变化/运行时意图、Legacy mirror 写入时机与 CoreLifecycle 既有预检/提交/应用边界。`ref`、Runtime gitlink、任何用户配置、Journal 格式未变更。
- **复制上游投影核心实现**：直接从 `ref` 复制 `impact.rs` 的生产实现（上游大型测试模块不直接复制），只按明确映射替换 `nyanpasu_config` → `chimera_config`、`NyanpasuAppConfig` → `ChimeraAppConfig`、digest 实现路径；Chimera 的 `TunStack` / `TunOverrides` 仍用本地现有值类型，不能盲目复制 `ref` 的 `SelectedTunStack`。保留上游 `RuntimeDomain` trait、三个实现、Closure 文件/定义映射和目标身份计算。补充三项 Chimera 定向测试，真实语言变更/仪表盘列表变更不应重建 Core，内置增强、Service 模式、TUN 和 Profile selection 变更应触达 Runtime。
- **Source Actor 由 upsert 转 CAS，返回真实提交影响**：`ApplicationActor` 与 `ClashConfigActor` 的 `commit` 改为读取当前版本与原始状态、计算 `impact::reaches_runtime`，调用 `replace_if_version(current_version, next)`，只有成功 `Replaced` 才应用预备好的 Legacy mirror 并返回上游风格 `reaches_runtime` 和 `operation_id`；`ReplacePreparedIfVersion` 继续消费预先完成的配置和 Legacy mirror 准备，仅在成功 CAS 后赋予相应来源影响元数据。普通 Get 快照不虚构新提交，返回 `reaches_runtime=false`、无 operation ID；两套静态测试 Source client 也保留同样数据语义。现有 CoreLifecycle 的有条件预检查和提交后应用仍为唯一 Runtime Owner，本轮**没有将 Actor 自主调用 Gate::check/committed 完成**，也未增加额外 Runtime 通知。
- **Clash Client 向上游来源影响收敛**：`should_reconcile_clash_patch` 的文档差异判定改用 `state::mutation::impact::reaches_runtime`，保留 Chimera 明确重新请求同一 Runtime 字段时的 `RequestedRuntimeFields` 额外意图；Raw overrides 的来源差异判定同样改用上游投影。Source Actor 成功返回的 `reaches_runtime` 进一步用于调试断言，确保被 Source Owner 判定会改变 Runtime 的文档不会在预检查客户端被遗漏。没有改变版本栅栏下仅提交后应用 Core、失败降级和 Legacy mirror 的时机。
- **验证与剩余差异**：`CARGO_INCREMENTAL=0 cargo check --manifest-path backend/Cargo.toml -p chimera --lib --quiet` 通过。`state::mutation::impact::tests::` 3/3、`state::application::tests::` 5/5、`state::clash_config::tests::` 3/3、`client::clash_config::tests::` 20/20，**31 项不重复的直接相关测试通过**；追加核心 Application runtime preflight / persistence error 2/2、`client::reconciler::check::tests::` 5/5 通过（其中有复测）。单独切换 Clash 预检投影后，完整 `client::clash_config::tests::` 20/20 再次通过。Rustfmt、`git diff --check` 通过。未执行完整 macOS/Windows E2E、真机网络行为或发行打包。完整上游的 `state/application.rs` 和 `state/clash_config.rs` 仍有 `MutationCoordinator` Gate 注入、Actor 自主 check/committed、Effects 提交回调、Shutdown 取消和统一 `ConfigError` 待迁移；不可将当前条件持久化与数据投影误报为这两个 Actor 已完整替代 Legacy Workflow。未来应在重构 startup 注入和删除 inline Core apply 的同一个切片中迁入 Gate 提交回调，避免重复 Runtime 应用。

## DIFF-132：Application DomainChange 候选检查归入统一 RuntimeGate 准入路径（2026-11-21）

- **固定上游及风险边界**：针对 `ref@c4b35e048` 的 `backend/nyanpasu-core/src/state/application.rs::{ApplicationActor::commit,RuntimeCandidate}`、`client/reconciler/check.rs::CandidateCheck` 及 `state/mutation/change.rs::DomainChange::Application`；本地对应 `backend/tauri/src/client/{core_lifecycle/workflow.rs,reconciler/check.rs,core_lifecycle/mod.rs}`。风险 L2（Application 预检查、条件发布、运行时应用之间的先后关系）。保留原有 Application Actor、CoreLifecycle mailbox、版本条件提交与 postcommit Core apply，不改持久化格式/用户配置/Runtime 或 `ref` gitlink，也没有额外安装第二个 gate 或第二个 runtime owner。
- **三个领域共用一条 ref 风格候选准入**：从 `CandidateCheck::check` 中抽出 `check_domain_candidate(core, committed_app, committed_clash, profiles, profile_content, RuntimeCandidate {change,staged}, service_connected)`；根据 `DomainChange::{Application,Clash,Profiles}` 取唯一待变更领域的候选，其它领域使用已提交输入。Profiles 候选使用 `capture_inputs`，Application/Clash 候选使用 `RuntimePreparation::capture_committed_inputs`，在 ContentReader 锁下原子采样已提交 Profiles 和 managed-file 内容，随后覆写候选 staged 内容。严格要求 `CheckRecord::Passed`；Core 明确拒绝转 `Arc<RuntimeError::CoreRejectedConfig>`，未知、跳过、backend/input 不可用转 `Arc<RuntimeError::CheckUnavailable>`，与 DIFF-131 typed 门禁一致。Production `CandidateCheck::check` 现在直接转发到同一函数，不保留并行的第二套文件冻结和检验实现。
- **Application 只替换准入实现，不重演写入**：`CoreLifecycleWorkflow::check_application_candidate` 使用 `DomainChange::Application { candidate: Arc<ChimeraAppConfig> }` 调用共享准入入口，保持 `service_connected` 分类、兼容用无 typed Profiles 构造跳过检查的旧测试分支，避免在 `ChimeraClient` 层预先额外运行一次检查。原 `patch_application_config` 的 `prepare_replace → read-only check → replace_prepared_if_version → postcommit apply_committed` 顺序及版本栅栏、源发布失败/冲突处理、成功 apply 后不重复 Core reconcile 均不变。上游完全 Source Actor 自主管理 `RuntimeGate::committed` 仍待后续整体迁移，当前不假装 Application 的所有权已完整换成 ref。
- **实际验证**：`CARGO_INCREMENTAL=0 cargo check --manifest-path backend/Cargo.toml -p chimera --lib --quiet` 通过。迁入 Application 检查后，`client::core_lifecycle::tests` 的候选先于提交、被拒绝后不提交、持久化失败不应用 Core、提交后运行时失败不回滚 4 个定向用例全过；`client::reconciler::check::tests` 原 4/4 及新 typed admission 来源保真共 5/5 通过；`client::clash_config::tests` 20/20 通过。新回归明确检查旧来源的 untyped validator error 通过 `Arc<RuntimeError::CheckUnavailable>` 传播，而不是伪造成 `CoreRejectedConfig`；该用例首轮断言误匹配裸 `RuntimeError` 而失败，改为下钻真实 `Arc<RuntimeError>` 后再次执行通过，并未修改运行时业务逻辑。修改文件 `rustfmt` 和 `git diff --check` 通过。未执行完整 macOS/Windows GUI E2E、网络实机验证或发布打包；后续从 Application Source Actor 自己发起 Gate、Clash Source Actor 整体切换到 ref 的 durable-commit notification 时，需要一并收敛旧 inline Core apply 才能避免重复运行。

## DIFF-131：RuntimeGate 上游 typed RuntimeError 门禁与 Clash 共享候选检查（2026-11-21）

- **固定上游、风险及范围**：对照 `ref@c4b35e048` 的 `backend/nyanpasu-core/src/state/mutation/ports.rs::RuntimeGate`、`client/reconciler/check.rs::CandidateCheck`、`state/profiles/actor.rs::check_candidate`，迁入 `backend/tauri/src/{state/mutation.rs,state/mutation/ports.rs,client/reconciler/check.rs,state/profiles/actor.rs,client/clash_config.rs,client/profiles.rs}`。风险 L2（Runtime 前置准入、Core 拒绝/未验证类别、文档和文件版本快照一致性）。不改 Profile/Journal 持久化格式、不触碰用户配置、不改变 `ref` 和 Runtime gitlink；不创建第二个运行时执行者。
- **RuntimeGate 返回上游 typed error**：`RuntimeGate::check` 不再返回 Chimera 兼容 `anyhow::Result<()>`，而是上游形状的 `Result<(), Arc<chimera_contracts::runtime_error::RuntimeError>>`。真实 Core 拒绝、CoreCheck 失败（包含 core kind）及验证端不可用、输入冻结失败、跳过/无有效 verdict 的分类从 Profile Actor 移入 `CandidateCheck` 对原始 `RuntimeCheckFailure` / `CheckRecord` 的转换边界。确定性拒绝映射 `CoreRejectedConfig`，后端不可用、缺少输入、未检查、未知错误映射 `CheckUnavailable`，保留 Core kind 和 retryable；不会凭 arbitrary 文本推断通过。`MutationCoordinator::runtime_gate()` 在尚未就绪或生产 wiring 缺失时返回 typed `CheckUnavailable::NotChecked`，避免假装 Core 拒绝。共享旧 `require_passed` / `RuntimeGateVerdict` 仍为过渡期其他旧消费方保留，并非第二套 Gate owner。
- **源 Actor 只根据 typed error 判定 CommitAborted**：`ProfilesActor::runtime_gate_failure(Arc<RuntimeError>)` 仅根据 `CheckUnavailable` 与其它明确拒绝分类为 `CommitAborted::{RuntimeFailed,RuntimeRefused}`，`source` 和 `errors` 均使用 Gate 返回的同一 Arc 错误对象，不再反向 downcast `anyhow`、复写 Core kind 或从诊断文本合成拒绝。旧 `CandidateCheckSource(anyhow::Error)` 层已删除。此错误在 prepare_file_first / Journal / MVCC Source 写入前返回，保留原有版本冲突和补偿序列。
- **非 Profile 域按上游原子采样已提交内容**：`CandidateCheck` 中 `DomainChange::Profiles` 仍以传入 Profile 候选和 staged bytes 调用 `capture_inputs`；Application / Clash 域改按上游 `capture_committed_inputs`，在一把 `ProfileContentReader` 内容锁里读取最新已提交 Profiles 快照和全部 managed-file bytes，然后覆盖传入的 staged 内容，避免预先克隆 Profile 快照与后读文件之间出现混合代际。`ChimeraClient::check_clash_runtime_candidate` 去掉独立构造 `RuntimeInputs` / Core check 的重复实现，直接以 `DomainChange::Clash { candidate }` 复用相同 `CandidateCheck`，仍只在 reaches_runtime 时执行预提交只读检查；其来源读、版本 CAS、提交后 `apply_committed` 与原 CoreLifecycle `source_committed` 保持现状，不能对已有 inline Core apply 再发一次 Gate committed，否则会造成重复运行。Application typed 源尚在原 CoreLifecycle patch 路径，本轮没有切换来源提交/应用的所有权。
- **源码验证及剩余差异**：`CARGO_INCREMENTAL=0 cargo check --manifest-path backend/Cargo.toml -p chimera --lib --quiet` 通过；typed Gate 修改后定向测试 `state::mutation::tests::` 2/2、`client::reconciler::check::tests::` 4/4、`ProfilesActor` 拒绝和未知检查 2/2、`ProfilesClient` 拒绝/不可用/提交顺序 3/3，第一组 11/11 通过。接通 Clash 共享检查、原子捕获后，`client::clash_config::tests::` 20/20、`client::reconciler::preparation::tests::` 3/3、`client::reconciler::check::tests::` 4/4、`ProfilesClient` 拒绝/不可用 2/2，第二组 29/29 通过；修改文件 rustfmt 与 `git diff --check` 通过。仍未运行完整 GUI E2E、Windows/Service、多客户端实际网络、全量打包/发布。后续应将 Application 与 Clash 的 durable Source Actor 逐步移到上游 `RuntimeGate::check/committed` 的同一 owner，并在整体替换 inline legacy runtime 应用与版本 fencing 时避免重复执行；不将本轮可复用的 preflight 误称为全部 RuntimeReconciler parity。

## DIFF-130：RuntimeGate 统一 DomainChange 候选与提交回调端口迁移（2026-11-21）

- **固定上游与边界**：对照 `ref@c4b35e048` 的 `backend/nyanpasu-core/src/state/mutation/{change.rs,ports.rs,mod.rs}`、`client/reconciler/check.rs::CandidateCheck`、`state/profiles/actor.rs::{check_candidate,committed_report}`。风险 L2（候选 preflight、源提交后的唯一 Runtime 通知及版本栅栏）；不修改持久化 Profile、Journal、`ref` gitlink 或 Chimera Runtime 子模块，继续保留现有独立 CoreLifecycle mailbox 作为唯一 Runtime apply/retry Owner。上游完整 Domain Gate 和 RuntimeError 类型仍有兼容差异，本轮不声称 Application/Clash 已切换至该 Gate。
- **直接迁移上游模块结构与候选类型**：从固定 `ref` 复制 `state/mutation/change.rs`、`ports.rs`，`DomainChange` 的三个分支保持上游结构，仅将 `NyanpasuAppConfig` / crate 路径适配为 Chimera 类型；`RuntimeCandidate` 从只带 `profiles` 改为 `{change: DomainChange, staged}`。`state/mutation.rs` 导出同一个领域类型和 Port。上游 `RuntimeGate::{check,committed}` 两方法均存在；`check` 暂保留 Chimera 当前携带 `RuntimeGateVerdict` 真实 anyhow 错误链的 `anyhow::Result<()>`，尚未冒充上游完整 `Result<(),Arc<RuntimeError>>` 错误类型。
- **独立候选检查**：`CandidateCheck` 现在从 `DomainChange` 指定领域的候选替换该领域的已提交快照：Application/Clash 使用传入源候选，其它领域从各自原 Owner 取最新快照；Profiles 使用传入候选，另两个领域按现有 typed clients 读取。保留 staged 内容冻结、真实独立 Core 验证和 `require_passed`，不通过 Gate 应用 Core。新构造参数传入现有 `ProfilesClient` 快照句柄与 `CoreLifecycleClient`，未增加任何全局状态或第二 Source Owner。
- **源提交后回调对齐**：`MutationCoordinator::source_committed` 保留提交成功后、持久化版本读取之后的 `CommitNotifications::profiles_committed`，对真正触及 Runtime 的 Profiles 写入调用 `gate.committed(ConfigDomain::Profiles, Version::new(source_version))`，其具体 `CandidateCheck` 只将对应领域和版本转送给原 CoreLifecycle `source_committed` 邮箱。源-only 操作仍只发布 source 版本，不要求 Runtime；连接时未安装 Gate 的旧测试构造路径保留既有工作流转发，避免重复通知。候选检查失败、MVCC 拒绝和前置材料化失败不会触发该调用。
- **验证与保留差异**：`CARGO_INCREMENTAL=0 cargo check --manifest-path backend/Cargo.toml -p chimera --lib --quiet` 通过；定向测试 `state::mutation::tests::` **2/2**、`client::reconciler::check::tests::` **4/4**，候选拒绝、验证不可用、非活动源文件修改 **3/3**，新增真正走 `ProfilesClient` Actor 提交的 `runtime_gate_receives_one_committed_version_only_after_profile_is_persisted` **1/1**，合计 **10/10 通过**；该测试从临时磁盘文件证明 check 不发布新文档，回调发生在 source 持久化后，元数据-only 修改不造成重复 runtime 通知。修改的 Rust 文件由 rustfmt 处理，`git diff --check` 通过。未执行真实网络/系统代理、全量桌面 GUI E2E 或发布打包。后续需继续迁入上游 `Arc<RuntimeError>` 门禁错误类型、Application/Clash 的 Source Actor 写入 Gate、独立 RuntimeReconciler 完整工作流；本轮只是从 Profile 域完成了复用同一 RuntimeGate 接口的第一条真实提交路径。

## DIFF-129：上游 NewProfileRequest 模型归属与结构化重排失败迁移（2026-11-21）

- **固定上游和路径映射**：对照 `ref@c4b35e048` 的 `backend/nyanpasu-config/src/profile/request.rs::NewProfileRequest`、`nyanpasu-core/src/state/profiles/actor.rs::ProfilesActorMessage::Reorder`、`nyanpasu-runtime-values/src/profiles.rs::ProfilesError::{ProfileInUse,ReorderListSizeMismatch,ReorderListDuplicate}`。本地由 `backend/tauri/src/state/profiles/actor.rs` 迁回 `backend/chimera-config/src/profile/request.rs`，并联动 `backend/tauri/src/{client/mod.rs,client/profiles.rs,ipc.rs}` 和共享 `chimera-contracts/src/profiles/command_failure.rs`。风险 L2（Profile 创建 IPC 类型、重排失败的类别和源文件提交边界），保持当前 `ref` 与 Runtime gitlink、持久化 Profile/Journal 格式和用户配置不变。
- **上游共享类型直接迁入**：复制 `ref` 的 `NewProfileRequest` 文件与字段，恢复它在 config 领域模块的统一导出；所有生产 Actor、Client、Facade、IPC 类型均直接引用同一模型，不留下 Tauri 重新定义或重导出。唯一偏离上游的 `#[specta(optional)]` 是已有 Chimera Legacy UI / 旧 IPC 请求可省略 `folder` 的生成绑定兼容要求，`#[serde(default)]` 仍按 ref 处理运行时缺省，其他字段语义未变。不改命令名和公开 wire 字段，也不重新写入用户 Profile。
- **重排领域错误不再用自由字符串**：按照上游 Actor 的 `ByList` 校验顺序，将长度不等映射为 `ReorderListSizeMismatch {expected,got}`，重复项映射为 `ReorderListDuplicate {uid}`；找不到 UID 仍是 `ProfileNotFound`，每种拒绝都发生在 `run_state_write` 的源提交之前。`ProfilesError::typed_failure` 与共享 `ProfileFailure` 仅在存在真实源字段时投影这两类失败及 `ProfileInUse {uid,referrers,current,global_transforms}`，不借字符串猜测、改写正常提交路径或加入另一份状态 Owner。
- **验证和待迁移项**：`CARGO_INCREMENTAL=0 cargo test --manifest-path backend/Cargo.toml -p chimera-config -p chimera-contracts --lib --quiet` 配置层 **249/249**、契约层 **22/22** 通过；`-p chimera --lib` 针对 `client::profiles::tests::{reorder_move_and_by_list,legacy_new_profile_request_without_folder_defaults_to_no_folder,add_joins_requested_folder_in_same_commit_and_persists,add_to_missing_folder_has_no_commit_or_new_file}` 和 `state::profiles::actor::tests::profile_reference_and_reorder_errors_preserve_typed_source_fields` **5/5 通过**，包含非法重排后的版本、顺序及落盘字节不变断言；修改文件已执行 rustfmt 且 `git diff --check` 通过。尚未运行完整桌面、Windows/Service、真实 Runtime 或 GUI E2E。完整上游 `ProfilesError`、独立 DomainChange/RuntimeGate 双向端口及 Core owner 迁移仍未完成，不能将这次类型归属收敛误报为整个后端 parity。

## DIFF-128：Profile 只读 Listing 投影与 Source Actor 被引用错误身份对齐（2026-11-21）

- **固定上游及目标**：`ref@c4b35e048` 的 `backend/nyanpasu-config/src/profile/{listing_view.rs,profiles.rs,mod.rs}`、`backend/nyanpasu-core/src/client/mod.rs::get_profiles_listing`、`backend/nyanpasu-runtime-values/src/profiles.rs::ProfilesError::ProfileInUse`。风险 L2（只读 API 的数据最小化、删除失败的标识信息）。保持原有 `get_profiles`、主/legacy UI、agent、RPC/IPC 和用户 Profile 文档不变，未切换已有编辑器到安全列表投影。
- **直接迁入完整上游投影**：新增 `chimera-config/src/profile/listing_view.rs`，复制 ref 的所有 `ProfilesListing`/Item/Metadata/Definition/Source/Binding/RemoteOptions DTO、纯映射、秘密泄露回归和 JSON 结构断言。唯一内容差异是测试固定 YAML fixture 由上游 `yaml_serde` 适配到 Chimera 的 `serde_yaml_ng`，不重写映射规则或 wire 字段。按 ref 在 `profile/profiles.rs` 复制 `impl From<&Profiles> for ProfilesListing`，按上游导出模块；只保留 UID、顺序、文件夹、转换依赖、来源类型、选项布尔量、数值订阅用量等允许读取的数据，不输出订阅 URL、外部文件路径、User-Agent、自由描述和运行字段清单。`ChimeraClient::get_profiles_listing` 从同一 ProfilesClient 的不可变快照构造该投影，延续 Chimera `profiles_client()?` 的非就绪错误处理，未建立并行持久化所有者。
- **Source Actor 的上游身份**：`ProfilesError::ProfileInUse` 新增真实 `uid: ProfileId`，生产删除分支使用当前被请求的 UID，不再让拒绝错误缺少对象身份。保持原有 `referrers/current/global_transforms` 和不提交任何源变更的拒绝行为；相应源所有权测试增加 UID 断言。不把这项字段补齐当作上游全部 `ProfilesError` 或 `RuntimeGate` 类型已迁完。
- **验证、回滚及未完成范围**：配置层 `cargo test --manifest-path backend/Cargo.toml -p chimera-config --lib --quiet` **249/249 通过**（包括上游 secret-free listing 断言）；`CARGO_INCREMENTAL=0 cargo test -p chimera --lib client::profiles::tests::delete_ --quiet` **9/9 通过**（增加被引用 Profile UID 的实源断言）；独立 `client::reconciler::check::tests::` **4/4 通过**，以及 `state::profiles::actor::tests::runtime_check_failure_retains_real_core_kind_and_retryability`、`unknown_validator_error_does_not_forge_an_explicit_core_rejection` 各 1 项通过；修改的 Rust 文件 `rustfmt --edition 2024 --check` 和 `git diff --check` 通过。尚未运行完整桌面构建、真实 Runtime/网络或 GUI E2E。变更只涉及源码和受影响测试，回滚可撤销本次代码差异；不能删除现存用户数据。后续应将只读 RPC/CLI/Agent 实际消费者迁入该共享 facade，并按 ref 继续收敛完整 `Profile Source Actor`、typed `RuntimeGate::check/committed`、提交错误来源和 runtime 恢复所有权。

## DIFF-127：Profile 删除清理所有权与启动悬空选择恢复（2026-11-21）

- **固定上游、目标及边界**：对照 `ref@c4b35e048` 的 `backend/nyanpasu-config/src/profile/{profiles.rs,proxy_group.rs}`、`nyanpasu-core/src/state/profiles/actor.rs::Delete`、`nyanpasu-core/src/client/profiles.rs` 和 `nyanpasu-contracts/src/profiles/error.rs::ExpectedNode`。风险 L2，涉及删除后源文档布局清理和启动时对已有文档的修复；不改变 `ref`/Runtime gitlink、Schema 4、Profile 文件材料化端口或 Core Runtime Gate，不触碰用户机器上的真实 Profile、Journal 或备份。回滚是仅撤销本次代码差异，不重置任何用户配置文件。
- **上游方法/职责直接复用**：`Profiles::remove_item_unchecked` 恢复上游仅 `items.shift_remove(uid)` 的纯低层语义，文件夹成员和 `proxy_group_layouts` 清理由唯一提交 Owner `ProfilesActor::Delete` 在 commit 之前显式处理。这与 ref 相同，避免无条件低层删除函数悄悄修改其它布局字段；保留原本完整的 `commit_with_resources`、版本冲突、资源清理和补偿路径。`profile/proxy_group.rs` 直接取自固定 ref，字节一致，包括公共 `REPAIRED_PROXY_GROUP_FOLDER_NAME`，算法和 wire 字段不变。共享 `ExpectedNode` 恢复上游 `Deserialize` 派生。
- **启动修复按 ref 迁移**：恢复 `Profiles::clear_dangling_current`，仅当 `current` 不在 `items` 时将其移除，不删任何 Profile、Transform 或其它引用。`ProfilesClient::new_with_jobs` 在原有文件夹和代理组布局修复之后调用它：若发生修复，先验证剩余文档，再只增加一次服务器拥有的 durable revision、通过既有 `PersistentStateManager` 持久化，并记录被修复的旧 ID；其它无效语义（例如缺失的 global transform）仍报错，不能被静默清除。后端调用链和主/legacy UI 共享原 Profile API，未引入新 IPC/wire 字段。
- **回归范围与验证边界**：调整纯模型测试，让 `remove_item_unchecked` 不再兼任文档清理；现有 Actor 删除测试扩充了代理组布局清理断言，新增悬空 current 只修复选择、保留 Profiles、升级 revision 并落盘的启动测试，原无效文档测试改为确实无效的 transform 引用。已有文件夹修复/Source transaction 测试仍作为相关保护。验证结果和限制须按本轮实际运行命令记录；本迁移不等于 ref 整个 ProfilesActor、RuntimeGate 和 materialization 的全量对齐。

- **本轮实际验证及磁盘恢复**：本切片配置层初次跑 **246/246 通过**，加入 DIFF-128 上游 Listing 后配置层 **249/249 通过**；受影响 Rust 文件的单文件 `rustfmt --edition 2024 --check` 和 `git diff --check` 通过；`ref/backend/nyanpasu-config/src/profile/proxy_group.rs` 与本地副本逐字节一致。全仓 `cargo fmt --all -- --check` 曾因未修改的 `tauri/vendor/tauri-plugin-wdio-webdriver`、`chimera-paths` 中既有差异未通过，未改写它们。首次 `cargo test -p chimera --lib ...startup_repairs_dangling_current` 构建遭遇 `rustc-LLVM ERROR: No space left on device`；用户确认后，仅删除可重建的 `backend/target/debug/incremental`（约 23 GB），保留其它 target、源码和用户数据，磁盘可用空间从约 132 MB 提升至约 23 GB。之后使用 `CARGO_INCREMENTAL=0` 成功编译 Tauri lib test，`startup_repairs_dangling_current_without_discarding_profiles`、`startup_repairs_invalid_profile_folders_and_persists_the_repair`、`load_fails_fast_on_invalid_document`、`delete_config_removes_folder_member_without_deleting_folder`、`delete_unreferenced_managed_profile_removes_file`、`delete_prepare_failure_preserves_item` **6/6 通过**。DIFF-128 另跑删除测试组 9/9 及 RuntimeGate 定向用例 6/6。未执行完整打包、GUI E2E 或真实 Runtime/网络验收。

## DIFF-126：Source Commit 前 RuntimeGate 真实拒绝/不可用结果与 CommitAborted 所有权迁移（2026-11-21）

- **固定上游及保护范围**：对照 `ref@c4b35e048` 的 `backend/nyanpasu-runtime-values/src/{mutation.rs,runtime_error.rs,check.rs}`、`backend/nyanpasu-core/src/state/profiles/actor.rs::check_candidate` 以及本地 `client/reconciler/check.rs`、`state/mutation.rs` 的只读 RuntimeGate。上游要求在任何源文件、materialization journal 和 MVCC 写入开始前，对真正会影响 Runtime 的 Profile 候选作独立只读验证。原 Chimera 将 `RuntimeGate::check` 的 `CheckRecord::{Rejected,Unserviceable,Skipped}` 和真实 `RuntimeCheckFailure` 一律压为 `RuntimeCheck(String)`，无法判断 Core **明确拒绝**还是**根本没有得到 verdict**。本轮风险 L2（预提交准入、真实错误来源、核心验证不可用、未知/已提交状态区分）；不改变 `ref` 和 Runtime gitlink，不改已提交 Profile/Journal、用户配置或补偿所有权。按照用户要求，不独立执行功能测试、全量构建、打包或 E2E；仓库 Rustfmt、Clippy、Prettier 属强制 Git 提交钩子。
- **直接迁入上游共享模型**：新增 `chimera-contracts/src/mutation.rs::{ConfigDomain,NotReady,CommitAborted}`，保留上游 `CommitAborted::{WriteConfig,RecoverAfterWriteFailure,RuntimeRefused {errors,source},RuntimeFailed {errors,source}}` 的结构、`serde(kind)`、错误列表及实际 `BoxedCause`，后者仅在 wire 输出中跳过原始操作来源。共享 `RuntimeError` 新增上游 `CoreRejectedConfig {core_kind,message}` 和 `CheckUnavailable {reason}`，并迁入上游 `RuntimeCheckUnavailable::Backend {kind,message,retryable}`；另外按 Chimera 已有来源区别保留 `CandidateUnavailable`、`NoEndpoint`、`HostUnsupported`、`NotChecked` 和 `Other`，而不是把无服务端/未检查的结果硬编码为配置拒绝。`ProfileFailure::Commit {reason}` 扩展为 `RuntimeRefused`、`RuntimeFailed` 两种已验证类别，未改变之前的真实文件写入/恢复失败类别。
- **RuntimeGate 结构化原始 verdict**：`client/reconciler/check.rs` 新增 `RuntimeGateVerdict {domain,record,source}`，其真实 `#[source]` 保留 `anyhow::Error` 底下的 `RuntimeCheckFailure`。原 `require_passed()` 对 `Passed` 继续准入；对 `Rejected`、`Skipped`、`Unserviceable`、`NotOwed` 返回结构化错误，不再仅抛格式化字符串。生产 `CandidateCheck` 在读 Profiles 路径、Clash 源、冻结候选以及 Core 验证的失败处复用统一错误来源包裹；仅当真实 `RuntimeCheckFailure` 或 `CheckRecord` 明确表示 Rejected 时，才认可拒绝。未知来自其它 Port 的错误保守标为 `Unserviceable`，不会凭错误文字猜测 Core 已拒绝或自动通过检查。原独立 Core candidate-check 端口不发布运行时、不执行 mutating Core Lifecycle 命令，也不重放写操作。
- **ProfilesActor 源提交判定**：`ProfilesActor::runtime_gate_failure` 将真实 `RuntimeCheckOutcome::Rejected` 映射为 `ProfilesError::Commit {source: CommitAborted::RuntimeRefused}`，保留原 CoreErrorKind、错误消息及 `CandidateCheckSource` 实际因果链；后端不可达、未准备好、输入冻结失败、Skipped/NotOwed、普通 untyped 端口失败等没有可靠 Core 拒绝 verdict 时映射为 `CommitAborted::RuntimeFailed`，带结构化 `RuntimeError::CheckUnavailable`。这两个结果仅替换以前运行时候选检查阶段的 `RuntimeCheck(String)`；原 `ensure_ready`、`source_only_document_change`、`prepare_candidate`、`StateManager::replace_if_version`、file-first、version CAS、content lock、compensate/rollback 和 source receipt 都保持原序列。**只要没有通过候选检查，就不准备文件或 Journal，更不开始 Source Commit**；结果未知只针对“检查能否通过”而不是“已经执行了一个可能提交的源写入”，不能据此重试尚未确认的后续 Core 操作。
- **源码回归、兼容及未完成范围**：新增共享 `CommitAborted` 两类失败的 serde 来源隐藏/错误列表测试定义；新增 Core `ConfigCheckFailed` 结构化拒绝、`BackendUnavailable` 真实 kind/retryability 和任意未知验证端口错误的映射定义；此前 Profile Active Config 的拒绝测试改为检验 `RuntimeRefused`，新增验证器不可达仍不得准备文件或修改 source revision 与文档的测试定义。所有这些回归测试**本轮未独立执行**，提交钩子的 Rustfmt/Clippy 不能当作多平台真实 Host 验收。上游的完整 CandidateRuntimeGate/独立 Host 验证对象和丰富 `RuntimeError::CheckUnavailable` 的 Host 类型仍有 Chimera 适配层；本轮不是整个 Source Commit 所有权的逐行替换。后续可沿着真实 Port 改善拒绝回执、明确检查来源、被取消异步操作与已提交 Runtime 的恢复，不删除 Profile、Journal、备份、迁移状态或 `.corrupt-*` 证据。

## DIFF-125：Profile Source Commit 失败分类、文件材料化与补偿错误来源保真（2026-11-21）

- **固定上游/边界**：对照 `ref@c4b35e048` 的 `backend/nyanpasu-runtime-values/src/{profiles.rs,mutation.rs}` 中 `ProfilesError::Commit`、`CommitAborted::{WriteConfig,RecoverAfterWriteFailure,RuntimeRefused,RuntimeFailed}`、`MaterializationOperation`，以及上游 Profile Source Actor 对文件材料化、版本拒绝、源提交与撤销补偿的所有权。Chimera 已使用真实共享 `ProfileFileError`、`MaterializationOperation` 和 `PersistentStateManager::replace_if_version_with_local_write`，但其 `ReplaceIfVersionError` 更细分为 State/WriteConfig/LocalWrite/ResourceRecovery/Recovery；本轮迁移真实类别和因果链而不将 Chimera 本地错误硬说成 ref 枚举。风险 L2（MVCC/源文件写入、补偿失败、文件/Journal 可能不一致），不改用户文件、配置、迁移协议、子模块指针。按维护者要求不独立运行功能测试、全量编译、打包或 E2E；Rustfmt/Clippy/Prettier 是强制提交钩子，不等同于平台运行验收。
- **共享提交错误分类**：在 `chimera-contracts/src/profiles/command_failure.rs` 新增可序列化 `CommitFailureReason::{WriteConfig,RecoverAfterWriteFailure,LocalWrite,PrepareAck,StateValidation,StateRecovery}`，其中前两类跟随 ref 的失败归属，后三种及 LocalWrite 是 Chimera 状态所有者额外区分的真实阶段，非伪造上游完整 `CommitAborted` 实现。共享 `ProfileFailure::Commit {reason}` 仅从实际 `ReplaceIfVersionError` 分类产生，不凭用户可见文本猜测错误；CAS 不匹配依旧由单独的 MVCC `VersionConflict` 处理，不把它误标成已写入提交失败。实际 manager 的 `ReplaceIfVersionError` 保持为 Tauri `ProfilesError::SourceTransaction` 的 `#[source]`，因此分类不会删除其原始详细错误。没有跨域 `RuntimeError` 的实例时，不构造假的 `RuntimeRefused`/`RuntimeFailed` 源。
- **材料化与恢复失败的真实错误链**：`ProfilesActor::materialization_call` 对 `spawn_blocking` 的 `JoinError` 使用 `anyhow::Error::new(error).context(...)`，替代直接字符串化，保留其取消/Panic 分类的实际源。源提交 `recover` 回调中，原来的资源补偿与 cleanup 撤销两项错误被格式化合并成一行；现改为 `MaterializationRecoveryFailure { resource: Option<anyhow::Error>,cleanup: Option<anyhow::Error> }`，将两个独立 Port/Journal 错误原样放入外层 `ReplaceIfVersionError::ResourceRecovery` 或 `Recovery` 源链，供诊断和以后上游错误类型继续迁移时精确判断，不改变 compensating/cleanup 的执行顺序或返回成功条件。
- **预提交拒绝的双错误保留**：原在 `replace_if_version_with_local_write` 进入 local-write 前发生版本冲突或准备 ACK 拒绝时，ProfilesActor 在 `content_lock` 保护下分别执行 `compensate`、`cancel_cleanup`，但把各自失败直接 `error.to_string()`。现保留 `Vec<anyhow::Error>` 原始对象，并在既有 `SourceTransaction {source, cleanup}` 与 `ConflictCleanup {expected,actual,cleanup}` 中新增 `cleanup_failures` 字段。历史的 `cleanup` 展示文本仍由同一批对象生成，前端错误文案和 MVCC 协议兼容；失败对象自身可用于后续不同路径的错误分类，不误认为清理已经成功，也不会因为补偿失败重放源写入。真正进入 local-write 的结果继续完全由 PersistentStateManager 负责补偿，本地 Actor 不二次抢占所有权。
- **源码回归与剩余差异**：新增共享 Commit 分类 serde 形状、真实 WriteConfig/LocalWrite 分类与原始 source 保真、ResourceRecovery 下两个独立 ProfileFileError 回收源、MVCC 前置拒绝下多个 Cleanup 失败对象仍保留的回归测试源码；按要求没有单独执行这些测试。尚未逐项迁完上游 `CommitAborted` 中依赖真实 RuntimeError 的拒绝/失败原因、Profile Source Transaction 持久化回执和全部 File/Journal/Runtime 启用边界；不能称 Profile 事务模块与 ref 已完全逐行一致。后续应优先迁入上游 Source Commit 的领域错误映射和具有真实来源的 runtime gate/prepare 错误，而不是删除或重放处于未知状态的 Profile 文件与备份。

## DIFF-124：ProfileContentError、材料化操作类型与异步订阅下载错误的真实来源迁移（2026-11-21）

- **固定上游与范围**：对照 `ref@c4b35e048` 中 `backend/nyanpasu-runtime-values/src/profiles.rs::{ProfileContentError,MaterializationOperation,ProfilesError::{FetchSubscription,ProfileContentRejected,Materialization}}` 与 `nyanpasu-contracts/src/profiles/error.rs::SubscriptionFetchError`。DIFF-123 已迁入可无损表示的 Profile 验证/MVCC 错误，仍有下载失败、内容非法、材料化端口失败被旧 ProfilesActor 降成单个字符串。本轮风险 L2（异步下载、校验、Job 状态、已提交源文件与材料化补偿）。保持上游“先下载与校验，再取得文件/源事务写入权”的提交门禁，不更改 `ref`、Runtime gitlink 或 Profile/Journal 持久化文件。用户要求不额外运行功能测试、全量编译、打包或 GUI E2E；强制 Rustfmt/Clippy/Prettier 仅是既有 Git 提交条件。
- **源承载 Profile 内容校验类型**：共享 `chimera-contracts/src/profiles/command_failure.rs` 新增上游形状的 `ProfileContentError::{NotYamlMapping {source},ReserializeYaml {source},MissingProxies,EmptyScript}`。仍使用 Chimera 当前已投入生产的 `serde_yaml` 解析器，不因架构迁移擅自改用另一套 YAML 语义；错误中保留真实解析器 `source`，向结构化序列化时按上游 `serde(kind: snake_case)` 跳过底层 Parser 错误详情。新增共享 `MaterializationOperation::{PrepareFileFirst,Compensate,Complete,PrepareCleanup,ActivateCleanup,CancelCleanup,RetryCleanup,Reconcile}`，提供固定操作类型而非再用自由格式阶段文字。以上新增 serde/serialization 定义不改变已持久化 Profile 文档或 Materialization Journal。
- **ProfilesActor 结构化失败和阶段**：此前 `ProfilesActor::validate_fetched_content` 直接返回字符串。现内部 `validate_fetched_content_typed` 在 YAML 解析失败时保存 `serde_yaml::Error`，缺失代理字段/空脚本使用精确 typed variant；旧验证入口继续投影历史文本，以保持已有镜像读、旧路径重验和旧 IPC 消息。DIFF-123 的 `ProfilesError::MaterializationFile` 进一步携带 `operation: Option<MaterializationOperation>`，对确切 `prepare materialization`、`discard staged resource`、`prepare cleanup` 的 ProfileFileError 依次标记 `PrepareFileFirst`、`Compensate`、`PrepareCleanup`；仅源类型和 context 都可验证时标注，未知调用方使用 None，仍保留原始 ProfileFileError 的 `#[source]`，不伪造文件错误的操作阶段。
- **异步 Refresh/Import 保留真实下载来源**：原 `state/profiles/ports.rs::SubscriptionFetcher` 是兼容接口，生产 `ProfileFileService` 已通过共享 `SubscriptionHttpFetcher` 返回真实 `SubscriptionFetchError`，但旧 ProfilesActor 的下载任务立即 `format!("download failed: ...")` 丢失了原始原因。现在 `RefreshFailureSource::{Subscription {url,source},Content(source)}` 作为独立 Actor 完成消息的字段贯穿实际下载、内容验证及 `CommitRefreshed` / `CommitImported`，最后结算为 `ProfilesError::{FetchSubscription {url,source},ProfileContentRejected {source}}`。只有真正从共享 HTTP Port/内容验证产生的可识别来源才进入 typed variant；测试与兼容 Fetcher 返回的一般 `anyhow::Error` 仍进入现有 `RefreshFailed` / `ImportFailed` 文本结果，不对未知错误创建虚假来源。下载或解析失败继续在任何 File/Journal/CAS 写入前退出，防止不完整的 Profile 被提交。Import worker 发生 panic/cancel 的稳定诊断及异步结果所有权未改变。
- **Jobs 安全映射与现有 UI 兼容性**：`state/profiles/jobs.rs::sync_error` 为 typed 下载与内容错误补入明确分支，仍输出旧的 `subscription_download_failed` 或 `profile_content_rejected` Job 错误码和固定安全消息，不将私有 URL、订阅参数及网络异常明文泄露至持久化 Job 日志。保留旧字符串错误的匹配以兼容历史/Mock Fetcher。Profile CommitReceipt、MVCC 版本、Profile Sync 完成状态、前端投影与 source-first 写入/补偿序列不改变；不会因为 fetch 失败自动重试一次新的源提交。
- **源码回归和剩余迁移**：共享新增 `ProfileContentError` serde 时省略底层 YAML source、`MaterializationOperation` snake_case 的测试定义；Actor 新增 YAML parser source、MissingProxies、SubscriptionHttpStatus=403 错误穿透、兼容未知 Import Fetcher 结果，以及确切 PrepareFileFirst 阶段标注；Jobs 新增 typed 下载错误的安全 code/URL 保护检查。这些源码定义未独立运行。尚未全面替换旧 ProfileActor `SubscriptionFetcher` 的 anyhow 端口或所有文件/Commit/Download 错误与完整 `nyanpasu-runtime-values::ProfilesError` 类型；后续应按真实单 Owner 与 Port 的接口逐项迁移，不能为通过检查删除用户 Profile、Journal、备份、迁移状态或 `.corrupt-*` 证据。

## DIFF-123：ProfilesError 实际来源、验证与 MVCC 冲突、自动激活降级的结构化迁移（2026-11-21）

- **上游参考与约束**：固定 `ref@c4b35e048`，对照 `backend/nyanpasu-runtime-values/src/profiles.rs::{ProfilesError,ProfileContentError,MaterializationOperation}` 及 `runtime.rs::DegradationReason::ProfileAutoActivationFailed`。上游完整 ProfilesError 包含 ProfileId、ProfileValidationError、订阅下载、文件端口、MVCC 提交与材料化等真实依赖；Chimera 当前 ProfilesActor 仍拥有专属的完整事务/Journal 恢复错误。风险 L2：真实文件错误源、版本冲突后补偿和已提交 Profile 的自动激活副作用。维护者要求继续源码迁移、不额外运行单测、全量构建、打包或 GUI E2E，仓库强制 Rustfmt/Clippy/Prettier 只属提交条件。本轮不修改 `ref` 或 Runtime gitlink，不新建 Owner，不改已提交源文件的提交顺序。
- **共享 Profile 失败投影，而非伪造完整错误**：新增 `chimera-contracts/src/profiles/command_failure.rs::ProfileFailure`，只包含 Tauri ProfilesActor 已实际拥有准确字段的 `ProfileNotFound {uid}`、`ValidationFailed {errors: Vec<ProfileValidationError>}`、`VersionConflict {expected,actual,cleanup?}`、`RevisionOverflow`、`ProfileHasNoFile` 和 `NotARemoteProfile`，使用真实 `chimera_config::profile` 的 ProfileId/验证错误类型及 ref 的 serde `kind` 结构。`ProfilesActor::ProfilesError::typed_failure` 对可无损投影的实际错误提供 `Option<ProfileFailure>`：普通 MVCC 冲突保留两个版本，清理失败保留额外 `cleanup` 证据，具体验证项保留完整枚举；只有报错文字而缺少上游真实来源的 Commit/Materialization/Rpc/下载失败不构造假 `ProfilesError`。因此这不是宣称已完全替换上游 ProfilesError 的所有变体。
- **材料化端口保留实际 ProfileFileError 因果链**：原 `ProfilesActor::materialization_error` 将所有任务/文件材料化错误都降为 `Materialization(String)`；现在通过原始 anyhow 链 `downcast::<ProfileFileError>()` 识别**真正来自共享 Port 的文件与 Journal 错误**，新增 `ProfilesError::MaterializationFile {context,#[source] source: ProfileFileError}` 保存完整原始失败及路径、错误种类、底层 source；其余未知/JoinError 保留原有文本版错误，不把未知任务取消假标成文件写入。使用原有持久化准备、CAS、补偿、版本冲突、文件栅栏与 Journal 恢复流程，不改变已提交/未知状态语义。
- **结构化 ProfileAutoActivationFailed 只接真实失败**：共享 `DegradationReason` 新增 ref 风格 `ProfileAutoActivationFailed {profile: ProfileId,cause: Box<ProfileFailure>}`（Box 与 ProfileFailure 是为 Chimera 尚未全迁完的 ProfilesActor 做的可序列化适配，不声称与 ref 内部 `Arc<ProfilesError>` 完全同型）。`ChimeraClient::typed_auto_activation_failure` 仅在 `ProfilesActor` 的确存在可无损投影的失败时生成此共享 reason；从真实错误保留 profile id、版本冲突或验证信息。Tauri `Degradation::from_shared` 仍将 reason 投影为现有旧 IPC 的 `profile_auto_activation_failed` 平铺 `code`、`message`、`retryable` 和 `system_effect` 阶段，不改变前端协议。ProfilesClient 未配置或其它未知领域错误继续沿用旧兼容 degradation，不伪造具体 cause。自动激活是在 Profile 原始源提交**之后**的附属动作，失败只产生 CommittedDegraded，不回滚、不自动重放已经持久化的 Profile。
- **新增源码回归（未执行）与剩余迁移**：共享新增 Profile 版本冲突和 UID 的 serde 定义；Tauri ProfilesActor 新增真实验证项、MVCC 冲突与 cleanup、类型未知回退、ProfileFileError `source()` 保真检查；客户端新增带真实 uid/版本的 autoactivation typed reason 以及旧前端扁平 wire 校验。按用户要求未独立运行这些测试，不能推断多平台运行验收。完整上游 `ProfilesError` 仍需要实装订阅下载、内容拒绝、材料化操作类别、文件端口上下文、源提交失败和非 Tauri Profiles Actor 共享所有权；不能把当前部分共享投影称为整域完全对齐。后续逐个真实 Port 迁移，不应修改用户 Profile 数据、备份、migration-state 或 `.corrupt-*` 文件来消除错误。

## DIFF-122：上游 CoreFailure、RuntimeError 真实来源与 Reconciler 分类迁移（2026-11-21）

- **固定上游/目标/风险**：依据 `ref@c4b35e048` 的 `backend/nyanpasu-runtime-values/src/runtime_error.rs::{CoreFailure,RuntimeError}` 和 `backend/nyanpasu-core/src/client/reconciler/{plan.rs,attempt.rs}`，将已经由 Core Manager 真实产生的错误类型连同分类、重试属性和原始 operation id 贯通到 Chimera Reconciler。风险 L2（是否拒绝重试、未确认的 Core 应用、事务所有权和 API 日志错误分类）；不能通过字符串猜测错误种类。按用户先前明确流程，不额外执行功能测试、全量构建、打包或桌面 E2E，仓库强制 Rustfmt/Clippy/Prettier 仅属于提交条件。`ref` 和 Runtime 子模块指针不变，不触碰 Profiles 持久化文件/备份。
- **共享 CoreFailure 和 RuntimeError 的真实迁入**：新增 `backend/chimera-contracts/src/runtime_error.rs`，以固定 ref 的 `CoreFailure { kind: Option<CoreErrorKind>,message,retryable,operation_id }` 为原型，`CoreErrorKind` 直接引用已固定的 `chimera-runtime/crates/chimera-core-metadata` 真实枚举，而非自定义字符串；新增该不依赖 contracts 的元数据依赖（及必要的 workspace Cargo.lock 更新），不改变任何子模块 gitlink。共享 `RuntimeError` 迁入其真实来源已存在的 `ApplyRuntime`、`StopCore`、`RecoverRuntime`、`RecoverServiceEndpoint`、`RefreshStatus`、Service install/start/stop/restart/uninstall、`SubmissionUnobserved` 等带原始 `CoreFailure` 的变体，以及 `ShuttingDown`、`OwnerUnresponsive`、`ConfigurationNotInEffect`、`ServiceHostsCore`、`NoRuntimeConfig`、`RuntimeSnapshotChanged`、`RuntimeNodeNotFound` 等无跨域依赖的变体。保留上游 `serde(kind: snake_case)`、Specta，以及 `core_failure()`、`core_kind()`、`retryable()` 对真实来源的查询。
- **Reconciler 真实错误来源**：`chimera-core/src/client/reconciler/plan.rs` 删除本地只存字符串的 `RuntimeError`，直接重导出共享类型；`runtime_apply_error(&anyhow::Error, uncertain)` 在实际错误链包含 `chimera_core_manager::CoreError` 时，复制其原始 kind、message、producer retryability、operation id；确已拒绝的操作映射 `RuntimeError::ApplyRuntime {failure}`，操作结果尚未确认映射 `RuntimeError::SubmissionUnobserved {failure}`。`backend/tauri/src/client/reconciler/{attempt.rs,status.rs}` 在对应拒绝和 unknown 结论使用同一转换函数；保留 `AttemptOutcome::{Refused,Unknown}` 的两态含义、源版本/Host 作用域及先等待依赖再尝试的门禁。不存在真实 CoreError 的其它领域错误仍保留全文至 `ConfigurationNotInEffect {reason}`，不擅自给它们添加 Core kind 或操作 id，也不增加失败后自动重试。
- **ProfileError 与其余 RuntimeError 的显式边界**：固定上游完整 `ProfilesError` 涉及 ProfileId、文件/下载错误、ProfileRevisionError、MVCC 提交和真实材料化来源；Chimera 的 Tauri ProfilesActor 目前还使用较多持久化错误和 CLI 兼容签名，若将它逐字复制到 contracts 会引入对 Tauri/Core 的反向依赖或丢失具体错误源。本轮**未删除或伪造 ProfileActor 的错误类型**，也未制造 `ProfileAutoActivationFailed {profile,cause}` 的虚假 cause。上游 `RuntimeError` 中 `BuildRuntime`、`PublishRuntime`、`ResolvePort`、`ResolveCoreBinary`、`InstallCoreBinary`、`PrepareServiceInstallPrompt`、`ReadCoreVersion`、`CheckUnavailable` 等需要对应领域完整源类型的变体本轮同样未迁，后续应由实际 owner 带着错误类型迁移。Tauri 的平铺 IPC `Degradation`、通用 `MutationOutcome` 仍暂时兼容旧前端。
- **源码回归/未验收状态**：新增共享 `CoreFailure` 序列化及未分类理由、Planner 实际 CoreError kind/retryability/operation id（含已有 anyhow 上下文）保真，以及 Tauri Reconciler refused/unknown 分流的测试定义；**未额外执行这些测试**。Rustfmt、Clippy 通过不代表 Runtime/Profiles 多平台行为已经独立验证。回滚时保持共享类型与使用方成组恢复，不删除已确认的运行时产品、Profile 数据、迁移状态、备份和 `.corrupt-*` 证据。

## DIFF-121：扩展共享 Runtime Values 降级原因并删除 Tauri 重复收据/状态/阶段类型（2026-11-21）

- **固定上游与目标**：继续以 `ref@c4b35e048` 的 `backend/nyanpasu-runtime-values/src/{runtime.rs,runtime_error.rs,profiles.rs}` 和 `backend/nyanpasu-core/src/client/reconciler/profiles.rs` 为源代码基准。DIFF-120 只迁入代理相关的 typed reason；本轮扩大到不依赖运行时错误对象的上游原因，逐步移除 Tauri 中完全重复的共享值类型。风险 L2（降级 reason 的序列化、Specta、桌面兼容状态、Profile 文件恢复）；不触碰实际 Profile/Runtime 状态机、配置、迁移协议、备份与 `ref` / Runtime gitlink。维护者此前要求不额外运行功能测试、全量构建、打包或桌面 E2E；Rustfmt、Clippy、Prettier 属仓库强制提交钩子，不等于运行时验收。
- **共享原因类型继续对齐上游**：`chimera-contracts/src/runtime.rs::DegradationReason` 新增 `ModeInterruptionFailed { cause }`、`ProfileInterruptionFailed { cause }`、`JournalInvalid`、`JobsJournalUnavailable`、`MaterializationDeferred`、`CleanupDeferred`、`HttpServerUnavailable`、`LocalSocketUnavailable`，保留 DIFF-120 的 ProxyInterruptionFailed 与 ProxyCacheRefreshFailed。错误原因分类继续使用原上游 `InterruptFailure::{Stale,Unavailable,Timeout,Protocol}`，而不是退回不区分来源的字符串；`code()` 统一将这些 typed reason 投影到旧桌面兼容 code。`impl From<ProfileDegradationCode> for DegradationReason` 现在在共享 contracts 内依据源枚举决定原因，不再由 Tauri 维护三项独立字符串映射，retryable 仍由源 port 判断，与上游一致。
- **真实的 Tauri 重复类型收敛**：共享 `RuntimeCommitStatus` 增加 Chimera 特有的 `Pending` 与 `RecoveryRequired`，共享 `DegradationPhase` 保留上游五阶段，同时明确补入 Chimera 的 `LegacyMirror`、`RuntimeCheck`、`RuntimePromote`、`RuntimePublish`、`CoreRollback`；这些额外状态**不是**谎称上游具有的成员，而是已在 Chimera 桌面 IPC 使用的兼容扩展。`backend/tauri/src/client/runtime.rs` 删除独立的 `CommitReceipt`、`RuntimeCommitStatus` 和 `DegradationPhase` 定义，直接 `pub use chimera_contracts::runtime` 对应共享类型，不改变字段名、status/phase snake_case 序列化或原有 Tauri 值构造方式。运行时入口 `MutationOutcome<T>` 与平铺 `Degradation { phase, code, message, retryable }` 暂时保留在 Tauri，因为上游共享 `MutationOutcome` 持有 typed `Degradation { reason, ... }`，直接替换将破坏现有桌面协议，不能误当作同类型。
- **统一 typed 到旧 IPC 的单一桥接**：新增 Tauri `Degradation::from_shared`：所有真正迁入的结构化降级原因在这个边界变成前端既有 `code` 字符串，phase 直接复用共享枚举，不再逐 variant 手动映射。`clash_api.rs` 的代理变更结果转换因此简化为迭代调用该桥接；`application_workflow/profiles.rs::map_profile_degradation` 也先通过共享 ProfileDegradationCode → DegradationReason 构造 ref 风格 typed Degradation，再统一输出旧平铺字段。已有 Profiles 材料化 Cleanup/Reconcile 合并为 `ProfileMaterialization` 阶段及基于 Port 的 retryability 语义不改变。
- **RuntimeError / ProfilesError 的明确剩余范围**：固定上游 `RuntimeError` 不只是名称枚举，它含 `CoreFailure`、`CoreErrorKind`、配置构建/发布、Core 二进制解析、Service 操作等真实依赖；`ProfilesError` 含 ProfileId、文件读写、订阅、版本冲突、Materialization 和事务错误来源。直接把这套类型复制进 `chimera-contracts` 会要求额外依赖、可能引入环和不真实的 cause。本轮**没有伪造** `RuntimeDeferred { cause }`、`ServiceStopFailed { cause }`、`ProfileAutoActivationFailed { profile, cause }` 等仍需领域真实错误的变体，也没有覆盖现有完整业务错误/重试机制。下一轮应按领域所有权先迁移实际错误与转换，再决定进一步收敛公共 `MutationOutcome` 与错误型降级原因。
- **源代码回归与验收状态**：共享 Contracts 新增 Profile 原因、Chimera 额外 phase/status 的 serde 回归定义；Tauri Runtime 新增三个已统一类型的 Rust TypeId/Receipt wire 检查及 typed → 旧 IPC 扁平字段检查；Profile 工作流新增三类原因映射与 retryability 验收源码。按用户约束这些测试没有独立运行，未声称多平台或 GUI E2E 通过。实际提交钩子的格式、Clippy 通过。不改变两个固定 gitlink，不删除用户数据、历史证据、备份或 `.corrupt-*` 文件。

## DIFF-120：代理变更结果与上游 Runtime Values 结构化降级类型的共享迁移（2026-11-20）

- **固定来源/范围**：以 `ref@c4b35e048` 的 `backend/nyanpasu-runtime-values/src/runtime.rs::{MutationOutcome,Degradation,DegradationReason,InterruptFailure,DegradationPhase,CommitReceipt}` 和 `backend/nyanpasu-core/src/{proxies.rs,actor_v2/api.rs}` 为固定源代码参照。此前 Chimera Core 私有 `ProxyMutationOutcome`、`ProxyDegradation` 和 `ProxyDegradationPhase` 仅提供字符串 `code`，Tauri 另持有通用 `MutationOutcome<T>`，重复定义和映射难以保留上游 typed failure cause。本轮仅迁移和代理相关的共享模型与桥接，不展开未具备完整 `RuntimeError` / `ProfilesError` 依赖的跨领域错误类型。风险 L2（业务结果序列化、前端代码映射和降级结果），保留原 `ref` / Runtime gitlink、持久化及配置。按维护者要求不额外执行功能测试、全量构建、打包或 GUI E2E；Git 提交钩子的 Rustfmt/Clippy/Prettier 属于强制提交流程。
- **共享 Runtime Values 原型**：新增 `backend/chimera-contracts/src/runtime.rs`，迁入上游 `MutationOutcome<T> { Committed, CommittedDegraded }` 及 `from_parts`, `with_commit`, `append_commit_result`, `degradations`, `into_parts`, `extend_degradations` 等 API，同时迁入 `CommitReceipt`、`RuntimeCommitStatus`、可序列化的 `Degradation`、五种上游 `DegradationPhase`、可读的 `InterruptFailure { Stale,Unavailable,Timeout,Protocol }` 和精确代理相关 `DegradationReason::{ProxyInterruptionFailed { cause },ProxyCacheRefreshFailed}`。Serde 的 `status` / `reason.code` 和 Specta 类型定义遵循上游；所有不存在完整依赖的其他原因仍暂缓迁移，**不伪造原因占位符**。新模块由 `chimera-contracts` 暴露，复用当前依赖，无需修改 Cargo.lock 或 Runtime 子模块。
- **Core 直接使用统一结果**：`proxies_actor.rs` 不再定义三个独立的代理结果 DTO，改为引用共享 `MutationOutcome<()>`、`Degradation`、`DegradationReason` 和 `DegradationPhase`（为原 Core API 消费方保留兼容别名）。断连失败对应上游 `ProxyInterruptionFailed { cause }`，通过原始 `ApiError` 保留 Stale/Unavailable/Timeout/Protocol 类型；非 API 类型的不可识别错误保守映射为 Unavailable。快照刷新降级使用 `ProxyCacheRefreshFailed`，继续保持原有 retryable 和详细 message，成功返回 `MutationOutcome::from_parts((), degradations)`，保留提交成功但外围失败只降级、不误报已回滚的 ref 语义。Provider 更新/健康检查仍按上游使用 `Result<()>`，不在本轮编造无来源的提交回执或额外 Provider 事务。
- **桌面 IPC 不破坏现有客户端**：`backend/tauri/src/client/clash_api.rs::into_mutation_outcome` 只在 Tauri 边界把共享 typed reason 映射回现有 `Degradation { phase, code, message, retryable }` 平铺字段，再生成 Tauri 的旧 `MutationOutcome<()>`。内部共享模型的 `reason: {code, cause}` 可供后续 Runtime 与 IPC 统一使用，本轮**不直接改变前端 TypeScript/Specta wire**，也不删除 Tauri 通用 RuntimeOutcome（它仍由众多其他模块使用）。唯一保留的转换发生在独立 Core 与现有桌面 UI 的兼容边界，避免跨层双重代理结果定义。
- **源码回归与待验**：新增 `chimera-contracts` 的结构化结果序列化及 `CommitReceipt` 保留测试源码；Core 测试改为检查 `reason.code()` 并新增错误来源类型映射；Tauri 测试定义检查共享 typed reason 仍投影为旧 `status: committed_degraded`、字符串 `code: proxy_interruption_failed`、原 `phase` 和 `retryable` 字段。**以上测试源码未运行**，不能称跨平台实机通过。本轮仍与上游完整 Runtime Values 的 RuntimeError/ProfilesError、其他共享降级原因、Tauri 全域值模型存在差异；下一轮适合逐域迁移，而非立刻删除前端兼容映射。回滚须按共享类型与 Core/Tauri 依赖成组恢复，保留用户配置、已验证备份、迁移状态及 `.corrupt-*` 证据。

## DIFF-119：ProxiesActor 快照指纹、Provider 写入与跨 Host 操作结果结算（2026-11-20）

- **上游固定基线和迁移目标**：对照 `ref@c4b35e048` 的 `backend/nyanpasu-core/src/proxies.rs::{State::refresh,reread_after_rejection,change_route,Message::Provider}`，继续复用上游单 CoreActor API 绑定、写入后不自动重试、写入失败后重读实际控制器状态的规则。风险 L2：Host 切换、Provider 更新、连接中断和缓存发布可能跨异步边界完成。用户此前持续要求只迁移、不独立运行测试或打包；本轮不额外执行单元测试、全量构建、GUI E2E 或 CI，仓库提交钩子仍遵循原有流程。`ref`、Runtime gitlink、现有配置及迁移/备份数据不改变。
- **快照指纹流式计算**：上游使用 `SeaHasher` 直接接收 `serde_json::to_writer`，避免为整个代理与 Provider 快照构建一次性 JSON buffer。Chimera 原本是 `DefaultHasher` 加 `serde_json::to_vec`；本轮增加 `FingerprintSink`（`io::Write` 适配原有 `DefaultHasher`），改为把相同 JSON 序列化流直接送入哈希器，不再额外分配整份 JSON 字节数组，同时**保留现有 DefaultHasher 指纹算法**，不为复制 SeaHasher 单独引入新依赖或改变既有指纹值。这是按上游数据流结构迁移，并非宣称哈希算法逐字相同。
- **当前 Core 来源判定**：新增 `State::require_source_instance`，在 Snapshot 发布前、代理路由写入完成后、连接中断完成后和 Provider 操作完成后验证原始 `ApiClient` 仍匹配 `CoreClient::api_client()` 最新能力；检查使用现有 `same_instance`（已撤销令牌、完整授权绑定或测试中共享身份），同时受 Shutdown/Lease 取消与 30 秒校验期限约束。若 API 令牌仍未收到异步撤销通知，但上游 CoreActor 已切换实例，立即清空旧缓存、阻止任何从旧 Host 发布的快照、连接副作用或普通成功结果，改为明确“需重新确认实际 Core 状态”的错误。不重放可能已经提交到旧控制器的写请求。
- **保持上游失败与降级边界**：普通路由/Provider 写入失败但来源仍有效时，依然按上游 `reread_after_rejection` 重读实际状态并返回原业务错误；普通路由成功但缓存刷新失败时仍返回现有 `proxy_cache_refresh_failed` UI 降级，连接中断失败则返回 `proxy_interruption_failed` 非重试降级，Provider 成功但重读失败仍返回上下文错误。**只有原始 Core 来源已经改变或不可验证时**，才把其写入/连接中断结果视作不确定而非正常成功或仅 UI 降级。对于最终缓存刷新失败，额外复查原始 Lease 的当前所有权以区别普通 HTTP 读失败与跨 Host 失效。新实现不改变实际写命令、Profile 改动、Provider API 或代理链中断策略。
- **测试源码和余留差异**：新增未执行的路由写入、Provider 更新和快照读取三类 pending-revocation 回归定义：旧 HTTP controller 尚能响应但 CoreClient 已改用新实例时，不允许旧结果触发连接中断、额外重读或快照发布；原 `replacement_during_chain_read` 改为要求收到“Core Lease 已退休”的错误，不再把旧 Host 的操作伪装为两项普通降级。另增加流式指纹与原 serialized-bytes hash 结果一致的源码测试。未运行这些测试或多平台/实时 Core 切换测试。新加的来源校验会在可信控制器权限不可达时保守地返回未知状态并清空旧缓存，可能增加少量 CoreActor IPC 往返；后续需与实际 Host Handoff 端到端运行结果结合验证。若回滚，应一并恢复来源校验和旧测试预期，不删除用户配置、迁移状态、备份及 `.corrupt-*` 证据。

## DIFF-118：ProxiesActor 旧 Host 失效事件隔离与 API Monitor 所有权迁移（2026-11-20）

- **固定上游与目标**：对照 `ref@c4b35e048` 中 `backend/nyanpasu-core/src/proxies.rs` 与 `backend/nyanpasu-core/src/actor_v2/api.rs` 的单 Core Lease、代理缓存 generation fence、失效订阅监控。Chimera 原有 ProxiesActor 已保留 ref 的单线程 Actor、`same_instance`、缓存快照与失效消息；这轮补齐的是本地 Host 切换通知和 Monitor 退出所有权，不另造 API 控制器。风险 L2（WebSocket 与代理缓存的旧 Host 数据、订阅通知及 Actor 退出）；不修改配置、Runtime、`ref` gitlink 和真实代理写命令协议。按用户持续要求，不单独执行功能测试、打包或 GUI E2E，仓库提交钩子只作为正常 Git 提交条件。
- **API 订阅 Monitor 的显式回收**：新增 `cancel_owned_task` 与原始 Panic payload 转发，在代理快照重读、缓存清空、手动 Invalidate、Host instance 替换和 Actor `post_stop` 时取消并 await Monitor `JoinHandle`，区分正常取消与致命 Panic。终止时也取消并回收只读的 10 秒刷新 Timer，再清空已发布的代理缓存，最后才传播首个未处理的任务 Panic。意外 Actor State Drop 仍保留 fail-closed abort 兜底。与 ref 一样只用单个实例绑定 Core API Lease，不把业务请求复制到第二个 Core。
- **旧 Host 事件严格 fencing**：过去仅成功刷新时递增 `generation`，主动清空与清理旧实例时没有推进该 fence；现 `invalidate_cache` 在取消/回收旧 Monitor 后也推进 generation，旧 `Message::Invalidated(old_generation)` 永不作用于后续新缓存。`refresh` 在新缓存发布前回收旧 Monitor，重新检查 `shutdown` 和 `snapshot.api.is_revoked()`，若状态已变化则主动清空缓存并拒绝发布；缓存命中前同样检查当前 lease 与 shutdown，不能在关闭路径复用旧 Host 数据。仍保留原有 `refresh_queued` 合并、失败后重新读取而不自动重放写入的上游语义。
- **相同数据的新实例也需要通知**：ref 的原始 `fingerprint` 通常只看代理列表内容，Chimera 使用独立的 Proxy API 订阅消费方；当 Host handoff 后新旧 Core 返回内容相同的组列表时，UI/Proxy 操作仍应重新绑定新的 Core Lease。现在刷新通知条件由“快照字节不同”扩展为“快照字节不同或 `!same_instance`”，避免把跨 Host 的真实身份变更误当作无变化；不改变代理节点排列/延迟和操作结果。
- **源代码回归和剩余边界**：新增两项未执行的 `proxies_actor_tests.rs` 验收源码：旧 generation 的迟到失效消息不得删除新的实例缓存；新 Core 即使返回完全相同的代理组，也应发出订阅变更通知。生产 WebSocket Actor 的日志/流 Worker join 已由 DIFF-117 处理，此轮不重复修改。尚未执行独立全量编译、实际 macOS/Windows/Linux Host Handoff 或多客户端 UI E2E；若需回滚应保留实例绑定的 API、Profiles、迁移状态、备份及 `.corrupt-*` 证据，不重放不确定的代理写操作。

## DIFF-117：Core API Monitor、WebSocket Worker 与旧实例缓存的退出回收（2026-11-20）

- **固定来源/风险**：参考 `ref@c4b35e048` 中 `backend/nyanpasu-core/src/{actor_v2/api.rs,clash/ws.rs}` 的单实例 API Lease、WebSocket Worker 与 Actor 停止所有权语义。在 DIFF-116 已完成 CoreActor API Monitor 显式退休与状态 Pump 回收后，继续处理 Chimera `clash/ws.rs` 尚未 join 的日志、重连、详情订阅以及多 socket worker。风险 L2（日志流重订阅、旧 Host 事件与 Panic 传播）；仍依维护者要求不额外运行单测、全量构建、桌面 E2E 或打包，仓库强制提交钩子仅是提交流程。
- **WebSocket Actor 的真实生产任务排空**：新增 `collect_stream_join`、`cancel_stream_worker` 和 `resume_stream_panic`。普通关闭和 Host API 失效允许中断只读 socket，但现在会完整 await 其 `JoinHandle`，区分预期取消与致命 Panic。`State::reset` 在丢弃旧 API Lease、清空连接详情和滚动订阅 epoch 前先回收原 logs socket；`restart_logs` 改为 async，在启动新 log-level socket 前取消并 join 旧任务，保持原 epoch fence 防止旧帧混入。`State::stop` 依次回收 reconnect owner、logs socket 和 details-subscriber watcher，清空实例绑定与发布状态后再传播异常；换订阅者时也先回收原 `details_closed` watcher。
- **WebSocket JoinSet 意外 Panic 不丢失**：主重连任务管理 Connections、Traffic 和 Memory 三个 WebSocket Worker。原逻辑可能在收到第一项 fatal `JoinError` 时直接 `resume_unwind`，使其它 worker 在尚未回收时被 JoinSet Drop 取消；其它时候又丢弃后续 `join_next` 的错误。现先保留首个原始 Panic，`abort_all()` 并完全 drain JoinSet，记录其它真实错误后再传播给上层 Actor；非致命取消仍不提升为崩溃。保留原 Core API 令牌取消、重连退避和 Actor 投递 RPC，不引入新的独立 Service/Core Owner。
- **兼容 CoreFacade API 缓存退休**：生产 `ChimeraClient` 已通过上游 `CoreActor`/`CoreFacade` 获取实例绑定 API，不再依赖 Tauri 私有缓存，故不复制或改写生产路径。只对原有无 Core Router 的兼容 Facade 增加显式退休：`chimera_core::actor_v2::api::ApiLease::stop` 对其宿主适配公开；Tauri wrapper `ApiLease::stop` 复用共享 Monitor join。兼容 `CoreFacade::clash_api_client` 在取不到可信 Running/instance receipt 时释放已缓存 Lease，在 binding 不匹配的实例替换时先 join 旧 Monitor 再创建新客户端；`finish_shutdown` 在停止 Core 前释放缓存。生产 ref ServiceActor 和 CoreActor 仍分别只有一个真正 Owner。
- **回归源码与待验差异**：在 `clash/ws_tests.rs` 新增取消后 `JoinHandle` 已回收、socket 确实被 drop，以及 fatal JoinError 在其它 worker 回收后仍可向监督层传播的测试定义；本轮未执行。上游 WebSocket Actor 的详细日志历史和投递结构与 Chimera 存在本地扩展，本轮保留这些上层协议字段，迁移的是独立任务生命周期，不称为逐行完全一致。若回滚，需成组撤回 WebSocket join、Tauri 兼容缓存退休及其共享 Lease 可见性调整；不删除用户配置、Profiles、迁移状态、备份或 `.corrupt-*` 证据。

## DIFF-116：Core API Lease、状态 Pump 与 Handoff 资源所有权迁移（2026-11-20）

- **固定来源/范围**：参照 `ref@c4b35e048` 的 `backend/nyanpasu-core/src/actor_v2/{mod.rs,api.rs}`。原上游 CoreActor 独占 API Lease、状态订阅 Pump 和 Handoff；本轮沿用其实例绑定、永久撤销、两阶段 Stop 及 generation fence 语义，补齐 Chimera 显式任务回收。风险 L2（Host 变更、API 鉴权、订阅取消及 Actor 停止），不更改用户配置、Runtime 或 `ref` gitlink。维护者要求继续迁移、不额外运行测试或打包；提交钩子不等于运行验收。
- **ApiLease 正式退出接口**：`chimera-core/src/actor_v2/api.rs::ApiLease::stop` 对旧授权先 `revoke()`，再 `abort()` 并 await Monitor `JoinHandle`，将真实 JoinError/Panic 返回给 Owner；正常取消视为预期。意外状态 Drop 仍保留上游 fail-closed 撤销行为，但正常 CoreActor 生命周期不再直接 `state.api.take()` 丢弃监视任务。原 `stop_for_test` 转而复用正式退出接口。
- **CoreActor 实际所有权**：`CoreActorState::retire_api` 统一执行撤销和 Join，已接入 API binding 替换、binding 查询失败或端点不再 Connected、EndpointDown 降级、Shutdown、Handoff 提交及 Host Adopt。CoreActor `post_stop` 先撤销所有 API Clone 授权，再依次排空 Pump/Handoff，最终 Join API Monitor 后传播第一个致命 Panic，避免异常处理本身中断已提交的 Core Stop。Host `adopt()` 现在 async，旧 generation 的只读状态 Pump 会先 abort + join，再发布新实例；保留旧状态 frame 被 generation fence 拒绝的上游机制，并且不改变 Core 变更/Stop 的业务结果判定。
- **WebSocket 流授权撤销**：共享 `ApiClient::execute_stream_frame` 的前后权威 binding 检查加入已有 30 秒 API deadline 和撤销优先 `tokio::select!`；`forward_api_stream` 每个 sample 的授权检查同样受撤销与 deadline 约束。这样旧 Core 在 status stream 中挂起的 IPC 鉴权不会阻挡永久撤销，避免 Host 替换后旧 WebSocket 继续转发新的帧。不存在成功后自动重放写入；定时检查和 CoreActor API lease 保持上游契约。
- **源码回归定义/待验**：新增显式 `ApiLease::stop()` 取消所有共享 Clone 并 Join Monitor 的 Rust 测试定义；本轮未运行任何功能测试、打包或桌面 E2E。上游 `ApiLease` 默认依赖 Drop 撤销/abort，而 Chimera 的显式 Join、前台 Core Actor await 和流读取超时为额外安全适配；不能误称逐行一致。后续仍需实际平台验证 GUI/授权切换和其它后台任务退出。若回滚，本轮的显式 Monitor Join 与调用点应同步回滚，不删除用户数据、证据、迁移状态或备份。

## DIFF-115：Core Actor Handoff 与 Service Command 兼容通路的退出所有权（2026-11-20）

- **来源和范围**：固定 `ref@c4b35e048` 的 `nyanpasu-core/src/actor_v2/mod.rs` 和 `service_actor.rs`，以 Core Stop/Handoff、只读状态 Pump 与 Service Elevated 命令的实际所有权为界。风险 L2；不更改 Core 操作协议、迁移、备份和两个 gitlink。根据用户要求，本轮不额外运行测试、打包或 E2E。
- **Core Actor**：保留上游对只读状态 Pump 的取消策略，但回收 JoinHandle 并传播意外 Panic。Handoff 可能已提交 Core Stop，不能当成普通订阅直接中断：Actor 意外退出时等待 stop-and-prove 任务，正常 HandoffStopped 消息处理时先 join 发出完成消息的任务，再更新 EndpointSlot/回复 Shutdown。原有 generation fence、StopUnconfirmed 及 I-R1 门禁不变。注意这属于 Chimera 的安全性适配，上游 `post_stop` 仍 abort Handoff，不能称为完全逐行一致。
- **Service Command 单一 Owner**：生产 `setup.rs` 已使用上游 `ServiceClient` 和 `ServiceClientCommandPort`，由 ServiceActor + TaskTracker 负责高权限命令及关闭，不新建第二套 Owner。旧 `ServiceCommandActor` 只用于无生产 Core Router 的兼容/测试路径；新增 `drain_legacy_on_shutdown`，持有串行 Mutex，等待超时仍在执行的 OS 命令而非 abort，传播真实 Panic。关闭前未重新探测 OS 状态时保留 `unsettled`，不把等待完毕误认为 Service 已停止。兼容 Facade 的 `finish_shutdown` 调用此接口；生产 Port 默认不重复执行排空。
- **测试定义和风险**：新增模拟超时 Service 命令的屏障测试源码，覆盖显式排空不取消命令、任务实际完成及未知状态 latch。按要求未运行。若 Core/Handoff 或 Service OS 操作迟迟不返回，退出会等待而不是假报取消；操作本身仍沿用既有超时预算。回滚本轮所有权改变不会删除配置、用户数据、migration-state、备份或 `.corrupt-*` 证据。

## DIFF-114：ServiceActor 超时命令与 Jobs 任务 JoinHandle 退出所有权对齐（2026-11-20）

- **固定参考与迁移目标**：参照 `ref@c4b35e048` 的 `backend/nyanpasu-core/src/actor_v2/service_actor.rs::{tracked,reap_outstanding,post_stop}` 和 `backend/nyanpasu-runtime/crates/nyanpasu-jobs/src/{actor.rs,lib.rs}`，迁移真实 Service/Jobs 任务停止时的所有权与异常传播边界。风险 **L2**：高权限服务安装/更新/停止、Journal 关闭和正在运行的 Job 都可能跨调用者超时继续执行；不能通过单纯取消来声称 OS 状态已结算。按用户近期“专心迁移，不用验证”的要求，没有单独运行功能测试、整库构建、实际多平台测试或打包；提交钩子的 Rustfmt/Clippy 仅属仓库强制提交流程。
- **ServiceActor 超时后台命令不得脱离所有权**：现有 `ServiceActorState::tracked()` 已按照上游将超过 `command_timeout` 的变更任务放入 `outstanding: Option<(ServiceCommandKind,JoinHandle<...>)>`，后续消息通过 `refuse_while_running()` 拦截冲突变更；但旧 `post_stop` 只中断 HealthTimer，然后丢弃 `outstanding`，这会让仍在执行的高权限 OS 命令脱离 Actor。现在 `post_stop` 在停止 timer 后，**take 并 await 该 outstanding JoinHandle 至真正结束，不调用 abort**，记录成功或业务失败、区分取消与真正 Panic；真实 Panic 通过 `JoinError::try_into_panic` / `resume_unwind` 向 Actor 监督机制传播。Timer 意外 Panic 也不再直接忽略。这样一次 Service 变更在退出时依旧只有一个明确的 Owner，保持上游已存在的命令防重入及状态门禁；如果底层 OS 命令永久不返回，Stop 也会等待，并不虚构安全取消语义。
- **JobsActor 执行、维护与退出排空**：此前 `Msg::Done`、`Msg::MaintenanceDone` 和 `post_stop` 多次 `let _ = task.await` 静默忽略 `JoinError`。新增 `collect_join_failure`：对已取消的任务仅警告，对真正 Panic 保留原始 payload；普通完成和维护任务在撤销 run/fault/log 状态后再传播不可恢复的 Panic。`post_stop` 先 cancel TickTimer，再 await 所有已接纳 Job（不 `abort`）和 Maintenance；**收集首个真实 Panic，但要等所有任务都归还所有权且发送 `closed` 信号后再 resume_unwind**，避免传播第一处故障时留下其他任务继续写入 Journal。已由 DIFF-112 转为 durable `Outcome::Failed("panicked")` 的业务 Handler Panic 仍属于正常完成，不会上升到 fatal。
- **JobsService 生命周期和 Journal Worker**：`start_with_clock` 的 Journal 初始化失败路径和 `JobsService::drain` 过去也忽略 Worker/Actor 的 `JoinError`。现在在关闭路径分别归还 Actor 及 Journal Worker，直到两者全部完成才传播原始 fatal panic，保留 `shutdown_timeout` 在超时后保留可重试的 owner handle 的既有合同；日志与已创建的 RunID 不会因错误传播被提前丢弃。原有主动 `Shutdown` 发送取消信号后继续排空协作式任务、阻止新 run，未改变权限、数据库文件或服务命令顺序。
- **剩余差异与风险**：这是基于上游 Service 命令所有权原则和 Jobs 关闭模型的 Chimera 补齐，不是声称上游本来逐字传播所有 Jobs Panic。JobsActor 和 ServiceActor 可能因长时间未完成的后台命令延迟退出，调用方超时仍只表示其等待已耗尽，不能将正在执行的命令重试为新 OS 写操作。后续需要对未跟踪的后台 spawn、Actor supervision 的终止回收进一步迁移。**本轮未独立验证运行行为**；若回滚，应成组撤回此轮 Join 所有权改变，保留现有配置、Profiles、持久化决议、migration-state、快照和 `.corrupt-*` 证据。`ref` 与 Runtime gitlink 不变。

## DIFF-113：Effects 与 CoreLifecycle Actor 停止时的 in-flight 任务所有权对齐（2026-11-20）

- **固定上游/核心差异**：依据 `ref@c4b35e048` 的 `backend/nyanpasu-core/src/client/effects/actor.rs::post_stop` 以及 `backend/nyanpasu-core/src/actor_v2/service_actor.rs` 的 JoinHandle fatal panic 传播语义。风险 **L2**（OS 副作用/系统代理与透明代理切换、Core/Profile 事务在停止时被抢断、Actor 意外终止）；只修改两个已有拥有者的真实生产生命周期，保留现有 `ref` 和 Runtime 子模块指针。用户此前要求**只做代码迁移、不额外测试**，故不主动运行单测、全量构建、打包或 CI；强制提交钩子的 Rustfmt/Clippy 属于提交步骤而非独立 E2E 验收。
- **EffectsActor 按上游先 drain 再关闭**：原 Chimera `Message::Shutdown` 对四个正在执行的 `port.apply(...)` 的 `JoinHandle` 调用 `abort()` 并等待取消，`post_stop` 甚至只 `abort()` 后立即返回；这可能在系统代理/透明代理、热键或其他 OS 状态还处于中间步骤时让 `port.shutdown()` 与残留写入交叉。新增 `State::drain_active`：不再 abort 正在执行的效果组，逐个 take 并 await 全部 JoinHandle；正常 `Shutdown` 首先置 `closed`、取消计时器和 pending，然后 drain 全部 active，再向待决请求返回 shutdown 降级结果并调用 `port.shutdown()`。意外 `post_stop` 也先关闭、停止计时器，再完整 drain。多个执行组分别拥有任务，若某个任务发生**非预期 fatal panic**，先等待其余组结束，再以 `try_into_panic` / `resume_unwind` 向上层传播，避免错误传播本身让其它 OS 写入失去所有权；已取消的任务记录警告。`Message::Completed` 也不再丢弃尚未 Join 的句柄，而是先等待拥有的任务结束再更新执行组状态。
- **CoreLifecycle 的真实事务 Owner 保全**：CoreLifecycle `Completed` 原先忽略 `active.task.await` 的 `JoinError`，现转为传播其中真实 Panic。原 `post_stop` 只停恢复计时器并关闭 Reconciler，若 Actor 在一个迁移/Service/Core/Profile 事务仍在执行时停止，活动任务的 workflow、lease 和未决决策被独立执行且失去主人；现从 `state.active` take 出任务并等待，不 `abort`，在等待前将共享 `startup_owner_unproven` 标记、`CoreLifecycleStatus` 的 `uncertain`/`shutting_down` 与活动项清理，禁止后续把未知完成结果误发布为成功。完成等待后关闭 Reconciler；意外 JoinError 的 Panic 向上层传播，不吞掉失去所有权的致命错误。普通已捕获的业务 Panic 仍按 DIFF-112 原流程 `mark_recovery_required` 和事务结算处理。
- **回归源码与余留差异**：新增 `effects/actor.rs` 的屏障式测试定义：在 `port.apply` 持续修改状态时调用 Shutdown，必须先等待应用完成，再进入 `port.shutdown()`；测试采用隔离 `BlockingEffectsPort`、oneshot gate、完成标志，不调用真实 OS 服务。本轮**未执行测试**，不能称作实机完成。上游 EffectsActor 本身没有 Chimera 独有的 RPC `Shutdown` 命令；本轮保留该接口，并以 ref 的停止时任务 drain 语义适配，未机械删除 Chimera UI/服务 API。下一步仍需迁移其它 Owner（例如 Core/Service/Jobs）实际 JoinHandle 和后台任务所有权，避免对故意恢复的 panic 误用 `resume_unwind`。若出现无法完成的慢 OS 操作，actor 会持续等待而非强制取消，UI RPC 目前仍有 10 秒等待上限；需要后续上层 shutdown 超时与诊断规划，不应用 `abort` 假装操作已完成。

## DIFF-112：异步 Actor 可恢复 Panic 作用域按 Future poll 收窄（2026-11-20）

- **固定上游与原始需求**：继续按 `ref@c4b35e048` 的 `backend/nyanpasu-panics/src/lib.rs::{setup_panic_hook,catch_recoverable}`、上游 Core/Effects/Jobs Owner 的 Panic 传播语义对齐；承接 DIFF-110/111 已迁移的 `chimera-panics` 和同步边界。风险 **L2**（异步事务、CoreLifecycle 不确定状态、Effects 重试、Jobs 持久化失败）；本轮根据用户持续的“专注迁移、不用验证”要求，不另行运行单元测试、完整构建、桌面 E2E 或 CI。仓库强制 Git 提交钩子仍按原设置执行 Rustfmt/Clippy 等，不视作本轮独立验收。
- **共享异步扩展，而非错误地复制同步 API**：原始上游 `catch_recoverable` 使用线程局部 `RECOVERABLE` + `scopeguard` 包裹同步闭包，不能直接在 `.await` 期间跨线程持续持有标记。基于**完全相同**的上游线程局部状态与错误类型，新增 Chimera 兼容扩展 `chimera_panics::catch_recoverable_future`：以 `std::future::poll_fn` 驱动被 pin 的 Future；每一次 `Future::poll` 前暂时设置 `RECOVERABLE=true`，以 `std::panic::catch_unwind(AssertUnwindSafe(...))` 捕获当前 poll 的 panic，若返回 `Pending`、`Ready` 或 unwind 都通过 scopeguard 恢复原线程标记。避免整个 `.await` 持有线程局部标记，将其它 Tokio 任务的致命 Panic 误标记为可恢复。用与上游同步 API 一致的 `RecoveredPanic` 产生结构化错误，保留原 hook 的帧采集/日志；这属于**上游 API 的 Chimera 异步兼容扩展**，不是声称上游本来有同名 async 方法。Future 在 poll 外被 drop 时的析构 panic 不属于此作用域。
- **八处真实异步生产/测试适配边界**：CoreLifecycle `run_profile_mutation`、`execute_lifecycle`、`probe_service` 三处转用 per-poll recoverable Future，保留 `workflow_panicked`/事务结果不确定、`mark_recovery_required`、mutation 决议等待、Service 状态 probe 失败映射；EffectsActor `port.apply` 转用同一作用域，仍以空结果触发已有健康/重试流程，新增原始 `RecoveredPanic` 和效果类型日志；CoreFacade `LocalOperationRegistry::submit` 和 `run_in_place_mutation` 两处继续在异常时返回 `Uncertain` 并锁存 `outcome_uncertain`；Jobs handler 异步运行继续以 durable `Outcome::Failed("panicked")` 记录，不污染其他 Job；仅测试使用的 ApplicationWorkflow Actor 也迁移，维持和生产 CoreLifecycle 相同的恢复决议与结算。以上 **7 处生产捕获 + 1 处测试适配**：CoreLifecycle 3、Effects 1、CoreFacade 2、Jobs 1、ApplicationWorkflow 测试 Actor 1，共 **8 个调用点**，不额外制造生产 Hook 调用。
- **有意保留的上游错误传播语义**：所有 `JoinError::try_into_panic`、`resume_unwind`、真正致命 Hook 及子进程报错维持原样，绝不把底层未知主机状态误当作普通业务成功；CoreLifecycle 对任一事务 panic 仍需进入恢复流程。上游 `nyanpasu-core::client::effects` 的停止任务 panic 传播与 Chimera 当前将外设 effect 失败交给重试预算的差异仍保留，需结合服务所有权而不是机械照抄。`AssertUnwindSafe` 只用于具体 Future poll，未跨 await 修改 TLS。
- **源代码回归定义与验收状态**：在 `chimera-panics` 新增源代码级测试定义，检查 `Pending` 返回后线程标记已经恢复、再次 poll 的 panic 被捕获且不会触发 fatal cleanup，以及正常 `Ready` 的输出保留。**本轮未运行这些测试**；也未进行 macOS/Windows/Linux 实机或 Tauri GUI E2E。若行为回归，应同时回滚 per-poll helper 和调用点，保留 Core/Jobs 原有业务错误结算、Runtime 恢复标记、用户配置与经过验证的备份。不更新 `ref`/Runtime gitlink，不做仓库外缓存清理。

## DIFF-111：Jobs 与 CoreLifecycle 同步可恢复 Panic 边界对齐（2026-11-20）

- **固定上游与目标**：以 `ref@c4b35e048` 的 `backend/nyanpasu-panics/src/lib.rs::catch_recoverable`（此前 DIFF-110 复制到 `chimera-panics`）为统一语义，把 Chimera 已明确需要吞掉 panic 并返回业务错误的**同步**回调迁移到上游的线程局部可恢复作用域。风险 **L2**（Jobs 编解码、持久化回调、Core 生命周期通知），只调整实际已有捕获边界的 panic 策略，不扩张应用致命崩溃判定、不变更 `ref` / Runtime pin、Migration 数据与备份。按维护者要求本轮不额外运行单元测试、all-features、实际二进制或 GUI E2E；仓库提交钩子附带的格式与 Clippy 检查不视作单独功能验收。
- **Jobs 三处生产边界**：`chimera-jobs/src/encoding.rs::json` 在同步 `serde_json::to_writer` 外改用 `chimera_panics::catch_recoverable(AssertUnwindSafe(...))`，继续把编码 panic 映射为 `OutputUnavailable("encoding_failed")`，保留 `size_limit` 语义。`src/job.rs::Job::new` 的同步输入验证只捕获 `serde_json::from_str`，依旧映射到 `Error::Invalid("input decoder panicked")`；**不改动**其跨 `.await` 的异步 handler `FutureExt::catch_unwind`。`src/journal.rs::Journal::call` 的同步存储回调同样调用上游可恢复 API，保留 `Error::Journal("store_panicked")` 和 oneshot 应答；作用域只覆盖本次闭包，不延伸到队列或等待部分。新增 `chimera-jobs -> chimera-panics` 本地依赖，并更新对应 Cargo.lock 直接依赖。
- **CoreLifecycle 两处同步通知边界**：将拒绝安装操作时的 `artifact.progress.finished(...)` 与执行完成后 `progress.finished(...)` 两处同步 observer 保护改为共享 `catch_recoverable`，继续在 observer panic 时记录原有日志，不覆盖真正的 CoreLifecycle 成功/失败/不确定状态。这里的通知既不执行生命周期事务，也不需要改变 `CoreFacade` 所有权；主线程上被显式恢复的 observer panic 不再触发 DIFF-110 新增的致命报告/退出。
- **异步/传播边界保持原义**：本轮故意**不改变** `run_profile_mutation`、`execute_lifecycle`、`probe_service`、Jobs handler、Effects、CoreFacade 的异步 `catch_unwind().await`：它们跨线程 poll，不能使用线程局部 `catch_recoverable` 直接包装整个 Future。所有用于真实错误传播的 `JoinError::try_into_panic` / `resume_unwind` 也保持原样，不将崩溃误报为普通业务失败。下一轮应分开处理异步任务策略和上游 fatal-hook owner shutdown，不能因迁移同步 API 而假定全局 Panic 语义已经与上游完全一致。
- **范围/风险/回滚**：本轮只修改上述五个既有同步捕获调用点、Jobs 依赖和进度文档；没有改写用户配置、存储、备份或 Hook 线程选择。若实际平台兼容性出现问题，可回滚同步捕获 API 的替换，但保留原来的业务错误判定与 Panic 恢复证据；本轮无独立功能测试结果，状态记为**待验**。

## DIFF-110：上游 Panic Hook / Frames / 独立 panic-dialog 进程首轮迁移（2026-11-20）

- **来源及目标**：以固定 `ref@c4b35e048` 的 `backend/nyanpasu-panics/src/{lib.rs,frames.rs}`、`backend/tauri/src/{lib.rs,cmds/panic_dialog.rs}` 和 `backend/nyanpasu-crash-reporter/src/report.rs::PanicDetails` 为代码参照。重点迁入 Panic 的可复用源代码、独立进程的 JSON 载荷，以及程序实际崩溃时的 Hook 调度。风险 **L2**（全局 panic 策略、事件循环退出和已有 Actor 错误隔离），遵循维护者此前“不要额外验证，继续迁移代码”的要求：本轮不单独跑功能测试、构建、完整桌面或 CI；Git 提交钩子按仓库规则执行的 Rustfmt、Clippy 和 Prettier 不代表实际系统验收。
- **完整上游底层复制**：新增 `backend/chimera-panics` workspace crate。其 `src/lib.rs` 和 `src/frames.rs` 从固定的 `nyanpasu-panics` 拷贝，保留 `PanicReport`、backtrace 的模块偏移/可信符号呈现、`setup_panic_hook`、线程局部可恢复标记、同步 `catch_recoverable` 和嵌套作用域还原，使用上游相同的 `backtrace`、`scopeguard`、`tracing` 和 Windows 模块查询依赖。更新 Cargo workspace、Tauri 依赖和 `Cargo.lock`，不修改 `ref` 与 Runtime gitlink。
- **独立桌面崩溃入口**：新增 Tauri 宿主 `panic_reporter.rs`，在桌面 `main.rs` 的 Tauri、Migration 和 startup-failed 分派**之前**识别 `--chimera-internal-panic-dialog`。独立进程通过 stdin-to-EOF 接收与上游 `PanicDetails {parent_pid,payload,location,backtrace}` 相同的 serde JSON 合同（最大 1 MiB），拒绝多余 CLI 参数、非法和超限数据。它不初始化 `AppHandle`、单实例插件、Core/Service/Jobs 或 redb，显示本地版本、平台、错误位置、payload 和堆栈。暂借用 Chimera 原有 rfd 错误对话框，不复制整套 Slint 诊断 UI，也不会自动上传敏感崩溃信息。
- **真实 Hook 与父子生命周期**：桌面 `chimera_lib::run()` 进入主应用时安装 `chimera_panics::setup_panic_hook`。Hook 使用上游帧捕获和原始错误日志，并在主线程遭遇非可恢复 panic 时，先把 JSON 同步写入**独立可执行文件子进程**、关闭 stdin 发送 EOF，不等待窗口关闭，然后退出发生 panic 的主进程。报告进程在父进程结束后继续拥有自己的 GUI 生命周期；若无法启动则以原有对话框作为最后回退。沿用上游对 Tauri 消息队列溢出 (`PostMessage failed`) 的过滤策略。内部 child reporter 在 `run()` 前分派，不会递归安装自身 Hook；Android/iOS 不引入桌面子进程入口。
- **Chimera 特有的渐进兼容边界**：与上游“所有未显式 `catch_recoverable` 的 panic 均致命”的策略不同，Chimera 现有多个同步和异步 Actor 使用标准 `catch_unwind` / `FutureExt::catch_unwind` 将任务失败与主应用隔离；未经全量转换就复制上游致命策略会使真实可恢复异步任务失败直接杀死整个客户端。故本轮 Hook 对其他线程**仍记录 upstream backtrace，但不强制启动致命报告/退出**；只在已确认的 UI 主线程执行致命 reporter/exit。下一轮应按模块逐步将可恢复同步边界迁至 `chimera_panics::catch_recoverable`，异步边界需另行设计而非跨 await 使用线程局部标记。当前启动时主线程退出尚未实现上游 `shutdown_on_main_thread` 的已持有 owner 同步清理，也不对外声称实现全部 Crash Reporter 语义。
- **源码回归定义与待验**：增加读取/格式化 `PanicDetails` 的可注入展示回调、非法/过大 JSON 拒绝的 Rust 测试定义；本轮按要求**未运行**。此轮只是将上游数据和进程边界落到真实调用路径，不包含独立 Slint UI、自动诊断 zip、GitHub issue 填报、主线程崩溃后 Core 优雅释放、Windows/Linux/macOS 打包后的子进程运行验证。若出现平台兼容性回归，需成组回滚新增 Hook、panic-dialog 分派与 `chimera-panics` 依赖，保留原用户配置、已验证备份、`migration-state.yaml` 和 `.corrupt-*` 证据。

## DIFF-109：独立启动失败报告子进程与 stdin-to-EOF 展示入口迁移（2026-11-20）

- **固定参考、目标与风险**：对照 `ref@c4b35e048` 的 `backend/tauri/src/{cmds/startup_failed.rs,startup_failure.rs,cmds/mod.rs}`，把原本由 Chimera 主进程直接弹出的 Migration/Core/Jobs 启动错误窗口改为**可独立运行的报告进程**。本轮风险 **L2**（启动失败、错误展示、子进程生命周期），遵照用户近期“**不用验证，继续迁移代码**”要求，不额外运行单元测试、完整构建、GUI E2E 或 CI 查询；仅允许提交钩子按仓库策略自动执行 Rustfmt/Clippy/Prettier。
- **主进程入口与独立所有权**：新增 `backend/tauri/src/startup_reporter.rs`，在桌面 `main.rs` 的 `chimera_lib::run()` / Tauri Builder / Windows shutdown helper 和 Migration CLI 分派之前识别内部 `--chimera-internal-startup-failed` 命令。独立进程只读取 stdin 直到 EOF、解码现有 `startup_failure::Report` 并展示错误窗口；不建立 Tauri `AppHandle`、单实例插件、Core、Service、Jobs、Client、redb 或迁移 Runner。Android/iOS 通过 cfg 保持原进程内错误对话框，不尝试启动桌面 exe。
- **生产传输合同**：`startup_failure::show_report` 使用 `startup_reporter::show_in_subprocess`，序列化包含失败阶段/分类、错误链、修订路径、应用版本、日志路径及已验证备份路径的原有 JSON `Report`；子进程读取 JSON **到 EOF**，输入长度上限 1 MiB，拒绝参数数量异常、超大或不合法 JSON。父进程通过 pipe 写入，不把错误及用户路径放进进程命令行，**关闭 stdin 后再等待**子进程正常退出，防止仍在等待输入的子进程与 `wait()` 死锁；进程启动、写入或返回码异常时回退到原有 rfd 窗口，并保留原报告文本。备份仍只有在生产 `BackupInfo::is_available()` 验证通过后才出现在报告中；展示进程不创建或恢复任何配置。
- **对齐源代码与测试定义**：参照上游 `cmds/startup_failed::read_and_show` 拆出可注入显示回调的 `read_and_show`，并新增源代码级覆盖：真实 `Report` 由 JSON stdin 到 EOF 解码、原始阶段与路径保留，格式错误/超限输入不启动窗口。这些测试**本轮未执行**。原有 `Report` wire/FailureKind/UI 文案保持兼容，避免本轮引入 Slint GUI 与多平台打包依赖。
- **剩余差异与回滚**：本轮迁移的是**独立 startup-failed 命令/报告所有权**，不是上游 `nyanpasu-crash-reporter` 的 Slint 可交互 UI，也不是 `panic-dialog` 全局 panic hook 或诊断数据采集；这些仍留待后续完整迁移，不能表述为崩溃报告系统已 100% 对齐。若发布时发现桌面子进程兼容性问题，仅回滚本轮报告分派/调用，继续保留原有进程内 fallback、共享 Migration、用户原始配置、`migration-state.yaml`、已验证备份和 `.corrupt-*` 证据；`ref` 与 Runtime pin 未改变。本轮未执行 standalone tests/builds/E2E，实际运行兼容性待验。

## DIFF-108：独立 Migration 子进程的失败载荷切换至共享 Failure v2（2026-11-20）

- **上游目标与范围**：沿用固定 `ref@c4b35e048` 的 `backend/nyanpasu-migration/src/child_result.rs::{FailureStage,Failure,Failure::from_error}`，把上一轮已共享但尚未直接用于父子进程传递的 `Failure` 作为 Chimera 桌面 Migration 子进程**真实生产载荷**。风险 **L2**（跨进程版本、备份可信性、权限/修订错误报告），不改任何迁移步骤、用户配置、migration-state schema、备份 manifest、`ref` / Runtime 指针。按维护者最近要求，此轮**不额外运行测试、桌面构建或 E2E**；仓库提交钩子必需的 Rustfmt/Clippy 属于正常提交流程。
- **协议版本化及共享载荷**：桌面 `migration_child.rs` 将 `ChildOutcome<startup_failure::Report>` 改为 `ChildOutcome<chimera_migration::child_result::Failure>`；Runner 构造失败输出 `FailureStage::RunnerConstruction`，真正的快照创建失败输出 `FailureStage::Backup`，其他迁移执行失败输出 `FailureStage::Migration`。通过共享 `Failure::from_error` 捕获有类型的版本错误、迁移步骤/状态/文档路径及完整 source chain，并由 Tauri 宿主注入备份是否真实可用及 IO 分类；仅在宿主保存报告前补充 Tauri `BackupError`、Session 和保全证据的相关路径。`ExecutionError::RecordFailure` 的次级状态持久化路径也纳入共享 Failure 路径列表，宿主分类继续保留次级 IO 权限错误。
- **显式不兼容的 wire 版本**：原 `protocol_version: 1` 的 `failure.report` 是包含 `StartupFailure`、logs、app version 的 Tauri 对象；新 `protocol_version: 2` 的 `failure.report` 是共享 `Failure {stage,config_dir,backups_dir,paths,revision,chain,backup_available,io_failure}`，成功载荷字段维持 `backup_name`。共享 `read_outcome` 改为**先解码版本信封，再拒绝不支持的版本，最后解释具体失败载荷**，防止以 v2 类型误读 v1 Tauri 报告。升级时必须让父进程与子进程来自同一兼容的可执行文件；混合版本会 fail-closed，绝不自动重复可能部分成功的迁移。
- **Tauri 宿主兼容转换**：新 `startup_failure::Report::from_migration_failure` 将 v2 `FailureStage` 映射为既有 `MigrationPrepare` / `MigrationRun`，根据共享 `RevisionFailure` 与 `IoFailure` 恢复 `NewerVersion`、`Permission`、`Busy` 和默认 Migration 分类，保留完整原因链与相应文件路径，继续由本地主进程构造可信 `version`、`logs_dir` 和旧 UI `Report` 结构。主进程接收失败结果时先核对 `config_dir` / `backups_dir` 与自身 `PathResolver` 完全一致，再对 `backup_name` 重新进行合法性校验与 manifest/哈希验证；子进程的 `backup_available` **只作诊断提示，不构成恢复授权**，快照损坏时不会在 UI 显示可恢复备份。Tauri 进程内报告、移动端迁移与 Core/Jobs/TypedClient 的旧接口仍然保留。
- **已有验收入口的源码调整**：同步修改 `migration_child` 的既有 Rust 断言和 `scripts/ci/verify-migration-binary.py`，识别新的 v2 版本号、`runner_construction`/`migration` 阶段及共享 revision/备份可用字段；只编辑测试与脚本源码，**本轮不运行它们**。本轮无新增完整构建、桌面实机、Windows/Linux 或 CI 验收结果，历史轮次的通过数不作为本轮证据。
- **剩余/回滚**：此轮保留共享 crate 的 `ChildOutcome<Report>` generic API，但桌面正式生产已改用共享 `Failure`，不再将 Tauri 启动 UI 报告写入子进程结果文件。独立 crash-reporter UI、跨版本打包升级进程和恢复交互仍未全部按上游迁移。若上线遇到协议兼容问题，必须**成对**回滚 v2 子进程写入与主进程解析/转换，不能只回滚一侧、不能删除 `migration-state.yaml`、现有用户配置、备份或 `.corrupt-*` 证据。

## DIFF-107：共享 child_result FailureStage / Failure 与源错误链归集迁移（2026-11-20）

- **固定上游/迁移目标**：以 `ref@c4b35e048` 的 `backend/nyanpasu-migration/src/child_result.rs::{FailureStage,Failure,Failure::from_error}` 为蓝本。遵照维护者本轮“**不用验证，继续迁移代码**”要求，继续真实迁移源代码，不单独运行单元测试、all-features check、构建、桌面 E2E 或 CI 状态查询。提交钩子按仓库现有设置自动运行的 Rustfmt/Clippy 属于提交行为，并非单独开展的新验收。
- **共享 Failure 合同与来源**：在 `backend/chimera-migration/src/child_result.rs` 增加 `FailureStage::{RunnerConstruction,Backup,Migration}` 和结构化 `Failure {stage,config_dir,backups_dir,paths,revision,chain,backup_available,io_failure}`，保留上游字段与 JSON 序列化形式。`Failure::from_error` 遍历 `std::error::Error::source()`，收集完整文本因果链，并通过具体 `SchemaError`、`RevisionError`、`MigrationCheckError`、`ExecutionError`、shared persistence document write/evidence 类型定位修订信息及去重的相关路径；修订路径也被纳入文件列表。IO 结果由宿主平台分类器注入（共享 `chimera-utils::io::IoFailure` 的 serde 支持），无需让独立 Migration crate 依赖 Tauri、CoreManager、UI 或 redb 宿主错误。
- **真正的 Tauri 生产调用适配**：`startup_failure::StartupFailure::from_error` 现在调用共享 `MigrationFailure::from_error`，使用其 `chain`、`revision` 和路径集合，删除 Tauri 中重复的 `SchemaError`、`RevisionError` 全部分支；宿主仍负责 BackupError、Session、文件保护证据、CoreHost、`ExecutionError::RecordFailure` 双重持久化故障的附加路径和 IO 分类，以及既有 `FailureKind`、`StartupStage` 和 UI 文案。共享收集器还在迁移之外的 startup 阶段使用，避免改变以前的 revision 诊断覆盖范围。
- **wire 兼容与下一阶段**：保留前轮生产 `ChildOutcome<startup_failure::Report>`、现有 `protocol_version:1` JSON 和 `Report`（version/logs_dir/backup_dir）格式；没有未经验收就切换父子进程 wire 类型，也没有改变备份 manifest 校验或迁移写入顺序。共享 `Failure` 现用于宿主构造真实失败报告，而不是仅定义未调用的数据类型。后续在准备验证父/子双方协议的阶段，可进一步将 `ChildOutcome` 的报告载荷直接升级为共享 `Failure`，引入对应兼容版本及 Tauri `Report` 转换，并处理异常退出/崩溃展示。
- **风险与回滚**：本轮风险 **L2**（启动失败错误分类和跨进程诊断）；改动限于共享 failure collector、Tauri 报告适配、共享 IO serde 依赖和锁文件，不修改 `ref`、Runtime 子模块或用户数据。无新增测试/编译的实际验证结果，本轮标记**待验**。遇到线上兼容故障只回滚本轮 collector/报告适配，不删除配置、迁移状态、已验证备份和 `.corrupt-*` 证据。

## DIFF-106：子进程结果协议与 RevisionFailure 从 Tauri 迁入共享 Migration（2026-11-20）

- **目标/参考/范围**：固定 `ref@c4b35e048` 的 `backend/nyanpasu-migration/src/child_result.rs`，迁移真正属于共享 Migration 的 child-result 协议及 revision 错误合同。风险 **L2**（跨进程报告读写和原有配置迁移错误分类）。按照本轮维护者“**不用验证，继续迁移代码**”的指示，专注生产代码迁移，没有运行本轮新的测试、`cargo check`、完整构建或桌面 E2E；旧轮验证不能当作本轮的测试证据。
- **共享 crate 生产所有权**：新增 `backend/chimera-migration/src/child_result.rs`，集中提供 `ChildOutcome<Report>` 的 generic wire 合同、versioned JSON Envelope（`protocol_version: 1`、`status: success|failure`、已有 `backup_name` 字段）、`read_outcome`/`write_outcome`、1 MiB 有界结果文件和拒绝已存在节点/软链接/不安全父目录的安全读写，以及备份名称拒绝路径遍历/非法字符。写入继续通过 `chimera-migration::fs::atomic_write` 与共享 persistence 原子写入执行。该模块不依赖 Tauri，也不创建/校验具体快照；通过 `serde_json` 显式依赖（同步更新 `backend/Cargo.lock`）公开给宿主。
- **上游 RevisionFailure 迁移**：将 Tauri `startup_failure::RevisionFailure` 的全部七种已有错误形态与 `path()`/`is_newer()` 迁入共享 `child_result`，保留上游四种版本差异及 Chimera 既有的 `DocumentBehindState`、`MissingDocumentStamp`、`FilesBehindState`；原 `startup_failure.rs` 用 re-export 继续提供既有调用路径，维护 JSON `snake_case` wire 格式和原有权限/版本分类语义。Tauri `migration_child.rs` 不再持有 Envelope、JSON 编解码、文件节点安全检查或备份 basename 规则，仅通过 `ChildOutcome<startup_failure::Report>` 专化共享协议；主进程仍在 Tauri 端使用真实 `BackupInfo` manifest+SHA 校验，失败报告仍由宿主生成和展示。
- **必要 Chimera 兼容差异**：上游 `child_result::Failure` 包含 `FailureStage`/`chain`/`io_failure` 等迁移专用错误。Chimera 当前已经公开更广的 `startup_failure::Report`，含 `StartupStage`、`FailureKind`、`version`、日志路径与备份提示；此轮保留当前已发布的 child JSON，以避免在未经跨进程验收时切换生产消息格式。下一轮可按上游 `Failure` 逐字段迁入，在 Tauri `StartupFailure` 专用转换适配层完成后再替换 wire type；本轮属于**部分架构迁移**，不是上游 `child_result` 100% 同型。
- **验收与回滚**：未执行测试、编译、打包、macOS/Windows/Linux 平台验证或 CI 状态核对；相关结果均标记**待验**。若发生兼容性回归，仅回滚本轮共享协议提取及 re-export，保留现有用户配置、版本状态、`.corrupt-*`、已验证备份、Legacy UI 与移动端进程内迁移。`ref` 和 Runtime gitlink 均保持固定。

## DIFF-105：正式 macOS 二进制的 Migration 子进程验收与结果路径保护（2026-11-20）

- **目标、固定来源与风险**：沿用 `ref@c4b35e048` 的 `backend/tauri/src/cmds/migrate.rs`/`backend/nyanpasu-migration/src/child_result.rs` 独立执行/失败报告边界，验证上轮 Chimera `backend/tauri/src/migration_child.rs` 的生产入口。风险 **L2**（启动时升级数据、child JSON result、快照恢复性）；不更新 `ref`/Runtime gitlink、不改变 CLI 标识、结果 JSON schema 1、备份 manifest、迁移状态或步骤 ID，不读取/写入主机当前用户配置，不启动有网络/TUN 副作用的图形应用。
- **构建空间恢复**：上一轮 macOS `cargo build -p chimera --bin chimera` 在 staticlib 归档报 `No space left on device`。先确认没有活跃 Cargo/Rustc，再**只在本仓库 `backend/target/debug/incremental/chimera_lib-*`** 删除 12 个可重新生成的最大增量缓存目录（不执行全量 `cargo clean`、不删其它项目或用户数据）。仓库该缓存逻辑大小约由 41 GB 降至 25 GB，APFS 可用空间约从 **1.2 GB 增至 8.1 GB**。实际执行 `cargo build --locked --manifest-path backend/Cargo.toml -p chimera --bin chimera --offline` 成功，生成约 252 MB 的 arm64 Mach-O debug 可执行文件；之后仍有约 5.5 GB 余量。缓存回收不是仓库源码改动，必要时 Cargo 会自动再生。
- **正式可执行文件验收**：新增**手动运行**且完全离线的 `scripts/ci/verify-migration-binary.py`（Python 3 标准库，无外部依赖），命令 `python3 scripts/ci/verify-migration-binary.py backend/target/debug/chimera`。测试真正运行已链接的 `main --chimera-internal-migrate <config> <data> <result>`，每案临时独立根，退出后自动清理。覆盖全新安装/第二次启动幂等且无无效备份、固定上游 v1.6.1 Legacy YAML 实际升级并按清单逐文件验证 SHA-256 和 typed 文件、Legacy YAML 损坏仍保留原始字节与有效快照、未来 `migration-state.yaml` schema 返回 `MigrationPrepare/NewerVersion` 且拒绝写入、占用中的普通结果文件、结果父目录不存在和 Unix 符号链接结果文件的写前拒绝。该脚本默认不自动运行于 GitHub CI，产物可按平台选择对应本机已经链接的 binary；在 Windows 上明确跳过仅 Unix 支持的软链接样例。
- **真实可执行文件发现并修复的缺陷**：首轮五场景实测中，原 `chimera_persistence::write::atomic_write` 在接收一个**已存在的 result 路径软链接**时会原子**替换软链接节点**（不会修改链接指向的外部文件），并在迁移执行**之后**返回成功；不符合 child-result 的私有临时文件必须写入全新节点的单次提交合同。现在子进程在运行 Runner **之前**使用 `symlink_metadata` 检查 result 不存在、结果父目录是已存在的非链接真实目录，失败即退出且不产生迁移状态；写入时再次校验并调用 shared `chimera_migration::fs::atomic_write`，保留原有原子落盘。父进程原本就在私有 `TempDir` 中生成路径，排除了普通多进程并发替换；未为任意不可信工作目录声称实现全平台无竞态 openat 级写入。新单元测试同时覆盖常规已存在文件、缺失父目录以及 Unix 真实/悬挂链接，确保源配置、外部数据和原结果节点均不被覆盖。
- **本地验证与限度**：共享 Migration **88/88**、Tauri 完整库 **1053/1053**（原有 1 ignored）、`cargo check --locked --manifest-path backend/Cargo.toml -p chimera --all-features --tests --bins --offline`、Rustfmt、仓库提交 Clippy 与 Git diff check 通过。重新链接 macOS debug executable 后运行上述脚本的全部场景通过。验证的是**真实完整链接的桌面命令行子进程**，不是 Tauri 主窗口的用户交互、发布模式代码签名、.app/DMG/MSI 安装、Windows/Linux 实机或 GitHub CI 最终完成态；这些仍需后续独立验收。
- **回滚与剩余差异**：若真实打包平台上出现协议兼容性问题，仅回滚本轮 child result 路径预检查与手动验收脚本，保留上轮已验证独立 Migration 架构、原始用户配置、完整快照、`migration-state.yaml` 与 `.corrupt-*` 证据。上游完整 `child_result::Failure` 仍未直接迁入共享 crate；今后可继续对齐原生崩溃报告/GUI 入口，而非误将手动 binary smoke 当成完整 UI E2E。

## DIFF-104：Migration 独立桌面子进程与 versioned child_result 协议对齐（2026-11-20）

- **上游依据与边界**：对照 `ref@c4b35e048` 的 `backend/tauri/src/{utils/init/mod.rs,cmds/migrate.rs}` 与 `backend/nyanpasu-migration/src/child_result.rs`，风险 **L2**（启动时迁移/跨进程 redb 所有权/备份与失败诊断）。上一轮仍在 Tauri 主进程 `.setup` 中执行 `Runner`，使迁移 redb 的生命周期与随后 Core、Jobs/Client 数据库打开路径共处一进程。此轮桌面生产改为在**主实例单实例插件已获取所有权后**启动同一 exe 的受控迁移子进程，等待它退出和返回结果，再执行后置 typed/兼容默认配置与客户端装配。保留原有真实 `Runner::with_paths`、Profiles/Typed Config/Storage 迁移算法和事务提交/备份策略；`ref`、Runtime gitlink 与 migration-state schema 2 不变。
- **真实生产子进程入口**：`main.rs` 在 Tauri 创建、窗口、Core 和 Windows shutdown helper 分派前识别内部 `--chimera-internal-migrate` 命令；参数必须精确包含绝对的 config 根、data 根、私有 result 路径，不重新解析用户目录，也不加载 Tauri 事件循环。子进程运行同一 Tauri migration adapter 的真实 `Runner::with_paths(...).run_pending()`，包含既有 SHA-256 verified snapshot；成功返回可验证的备份**名称**，失败保留 `MigrationPrepare` / `MigrationRun` 阶段及原有 `startup_failure::Report` 路径、版本和分类。该结果是 Tauri 宿主的兼容协议，目前**尚未**把原生上游的 `nyanpasu-migration::child_result::Failure` 原样搬到共享 crate。
- **父进程容错与安全**：`migration_child::run_subprocess` 仅由桌面主实例 Tauri setup 调用，使用独立私有 TempDir 的版本化 JSON 协议 `protocol_version: 1`。子进程失败、panic/异常退出、返回码与报告状态矛盾、结果文件不存在/不是普通文件/超过 1 MiB/格式损坏/协议未知均**fail-closed**，不尝试自动重跑可能只完成部分写入的迁移。父进程为成功快照从可信 `PathResolver::backups_dir()` 重新构造路径，校验名称（保留 semver 的 `.` / `+`，拒绝路径分隔与 `..`）并再次调用 manifest/文件哈希验证；失败报告中的备份可恢复性也必须在父进程**重新校验**，即使子进程刚成功、快照返回前被篡改也不宣称可用。子进程的输出和临时报告由父进程私有生命周期控制，缺失报告时将退出码及有限 stderr 片段包含于启动失败诊断。
- **跨平台与启动边界**：`setup::finish_migration_startup` 独立负责成功后的默认配置与已验证 BackupInfo 继续传递；后续 Core/Service/Jobs/Client 的任何启动失败仍可指向真实备份。失败由主进程复用现有 `startup_failure` 捕获并触发原有退出窗口，杜绝失败后继续初始化。Android/iOS 无可执行的桌面 child，因此保留原来已验证的**进程内 Migration**，桌面子进程模块/入口通过 cfg 排除；没有改变移动端初始化流程或主实例、deep link 控制。
- **隔离验证**：真实 `std::process::Command` 启动**独立 test-harness 子进程**并复用生产 `execute_to_file` 与父进程读取/备份验证逻辑：成功用固定上游 v1.6.1 Verge YAML 与格式有效的 redb（含必需 `clash-nyanpasu` 表和额外用户表），验证快照字节一致、父进程重开 redb 后额外用户表仍在；另验证旧 YAML 格式错误、未来 schema `NewerVersion`、无结果的异常退出、子进程报告有效备份后文件被篡改，皆保留正确阶段、路径与备份可信性。纯协议测试覆盖结果版本、链接/超大风险、非法备份名称、参数范围。共享 Migration **88/88**，Tauri 全库 **1051/1051**、1 项既有忽略，后端 `cargo check --locked -p chimera --all-features --tests --bins`、Rustfmt、Clippy 与 Git diff 检查通过。
- **未验收事项及回滚**：尝试构建完整桌面 binary 以执行真实 `main` 命令时，macOS 宿主磁盘（229 GB、剩余约 1.2 GB）在 Rust staticlib 归档阶段报 `No space left on device (os error 28)`；**没有通过真实链接后的桌面可执行文件/打包、Windows/Linux 实机、桌面 E2E 或 GitHub CI 最终验证**。只清理由本轮失败编译生成的临时归档对象，没有清理其它构建缓存、用户文件或工作区。后续还需要在磁盘恢复空间后验证实际打包的 child/main 行为、跨平台签名/单实例锁和 crash-reporter UI 的原生上游 `child_result` 类型化对齐。若桌面子进程出现环境兼容问题，只回滚本轮父子进程入口，保留原有本地 Migration、用户配置、已验证备份、`migration-state.yaml` 和 `.corrupt-*` 证据；不得自动覆盖配置来“恢复启动”。

## DIFF-103：Migration 首写入边界与启动默认配置顺序对齐（2026-11-20）

- **固定上游与目标**：参考 `ref@c4b35e048` 的 `backend/tauri/src/utils/init/mod.rs`/`backend/tauri/src/setup.rs` 的迁移优先生命周期及 `backend/nyanpasu-migration/src/runner.rs`，核对 Chimera 的 Tauri `.setup`。本轮风险 **L2**（首次启动、已有 typed 文件、迁移失败后的用户配置与快照）。此前 `.setup` 在 `Runner::with_paths`/`run_pending` **之前**调用 `init::init_config`，隐式生成 `chimera-config.yaml`、Legacy `profiles.yaml` 和 Clash overrides；还在日志初始化中通过后台线程过早读取 `Config::verge()`，构造全局配置草稿。这违反“迁移首先读取用户原始文件并取得实际快照”的边界。保留现有平台 BackupPort、Tauri 生命周期、`ref`/Runtime gitlinks、migration-state schema 2 和所有步骤 ID。
- **生产启动顺序**：将 `init::init_config` 拆成前置的 `init_logging`（只建立日志系统，不读取和生成任何 legacy YAML），和后置 `initialize_post_migration_defaults`。新增 `setup::migrate_before_legacy_defaults` 统一 `MigrationPrepare` → `MigrationRun` → `Setup`（默认值生成）三段可测试边界：迁移构造/执行任一失败即停止，不初始化 Legacy 或 typed 默认文件，不启动后续 Core、Service、Jobs、客户端。`StartupFailure` 捕获仍保留原错误链、阶段、精确路径和有效的已验证 BackupInfo；成功后才运行 `init::refresh_migrated_logging`，避免过早读取 Legacy 的进程全局 `IVerge` 草稿。仍只在主实例进入 Tauri `.setup` 后创建目录，不改变单实例声明与窗口启动次序。
- **Profiles 文档正确性修复**：后置默认值必须生成真正的**rev4 stamped** `profiles.yaml`，不可写入旧版 Legacy `current: [] / chain: []`，否则迁移状态已经记录 rev4 时，下次启动可能检测到 `FilesBehindState`/失配。使用 `chimera_config::profile::Profiles::default()` + 共享 `chimera_migration::document::write_document` 的原子且目标安全的写入路径，写入 `_nyanpasu` `document: profiles`, `schema_revision: 4`。已存在的 Profiles 文件和悬挂链接均不会被覆写；如果缺失且写入失败则作为 `Setup` 错误立即中止，而不是只打印日志并继续进入 Client。旧版 Verge/Clash overrides 的默认模板仅用于成功迁移后的兼容层，不在迁移前更改备份原貌。
- **实际失败与重启验证**：新增隔离 `setup` 回归：全新安装先完成 Migration，再创建带 stamp 的 typed Profiles 默认文档及兼容 YAML，第二次启动保持 Profiles/typed 应用字节不变、不生成额外备份；部分 typed 文件导致 `MigrationPrepare` 拒绝且不创建任何 Legacy 默认文件，原始 typed 文件字节不变；旧 Verge YAML 损坏时 `MigrationRun` 失败，`Report.backups_dir` 仅引用完整验证过的真实快照，快照与原目录均不包含错误前置初始化的默认 Profiles/overrides。原有 `init` 注入路径和无覆盖测试继续通过。
- **本地验证**：共享 Migration **88/88**，Tauri 完整库 **1041/1041**（额外 3 项真实 setup 顺序测试，1 项既有忽略），`cargo check --locked -p chimera --all-features --tests`、Rustfmt、提交 Clippy 与 Git diff check 通过。未执行独立 `migrate` 子进程协议、`child_result` 落盘与外部 crash reporter；这些属于上游下一阶段实质架构差异，不应把本轮称为完整独立进程迁移。也未核验 Windows/Linux 实机、桌面 E2E 或 GitHub CI 最终结果。
- **剩余/回滚**：Chimera 仍在 Tauri 单实例 setup 内运行 Migration，而 `ref` 可以独立进程运行并持久化跨进程错误；未来需设计独立进程对单实例锁、RustDesk/服务及 Windows 提权、退出码/报告文件的影响。若发现兼容性回归，回滚本轮初始化顺序和 stamped default，保留任何现有用户配置、有效备份、`migration-state.yaml` 和 `.corrupt-*` 数据证据；绝不自动强制恢复旧目录。

## DIFF-102：上游 v1.6.1 真实配置升级与隔离恢复验证、备份数据根边界修复（2026-11-20）

- **固定参考与范围**：以 `ref@c4b35e048` 的 `nyanpasu-migration/src/fixtures/v1_6_1/{profiles.yaml,nyanpasu-config.yaml}` 和 `nyanpasu-migration/src/backup.rs` 的备份事务/manifest 为参照，在独立临时目录运行 Chimera 的**真实** Runner、Profiles/Typed Config/Storage 三组生产 migrator。风险 **L2**（升级前快照完整性与损坏快照报告）；不读取或写入当前用户配置、不改变 `ref` / Runtime 指针、迁移状态 schema 2、步骤 ID、manifest wire 格式或备份留存数量。
- **端到端跨版本证明**：新增隔离迁移回归：直接加载上游 v1.6.1 Profiles 与 Verge YAML 样本，配以 Profiles 远端订阅配置/脚本节点和真实的 redb 用户数据；由实际 `Runner::with_paths(...).run_pending()` 完成旧配置升级，验证 Profiles rev4、Typed Config rev2、Storage rev2、三个 typed 文件及 `last_succeeded`。实际备份在任何 typed/Profiles 重写前保留原 YAML、各配置子文件和 `storage.db` 的确切字节与 manifest；借助已有**仅限测试** `restore_to_empty_sandbox` 恢复到全新隔离目录，重新以 redb 打开恢复后的数据库并核对真实原用户键。验证原升级目录未被恢复操作覆盖，备份经过 SHA-256 清单验证且仍可用。这个测试不代表生产恢复 UI 已实现，更不等于现场用户目录实机验证。
- **发现的备份数据根漏洞**：原 `create_backup` 会先创建 `<data>/backups`，之后 `backup_storage` 才检查 `storage.db`；如果数据根目录本身为符号链接，即使 `storage.db` 看上去是常规文件，也可能先在链接指向的**外部目录**创建备份目录并复制外部数据。悬挂的数据根链接还可能被误判为数据库不存在。现新增 `validate_data_root`，在任何备份目录创建前拒绝链接或非目录节点，在创建目录后和 storage 文件复制/导出前重检，保留真正缺失的数据目录（仅配置备份）的正常路径。受拒绝路径不生成任何 committed 或 `.partial` 备份，也不变更外部用户文件。
- **校验与修剪边界同步**：`prune_backups` 在遍历和删除前拒绝链接的数据根；`verify_backup` 在判定 `BackupInfo::is_available` 之前验证备份根的上一级数据根，而不只是 `backups/` 本身，避免一个已验证备份在数据目录被重定向后误被广告为可恢复。回归覆盖真实/悬挂链接、快照生成前拒绝、数据根重定向后的校验与修剪拒绝、原有效快照在另一路径保留，以及数据根真正缺失时正常创建配置备份。
- **本地验证**：共享 Migration 库 **88/88**，Tauri 完整库 **1038/1038**（含本批新增 4 项备份/恢复测试；另 1 项原有忽略），`cargo check --locked -p chimera --all-features --tests`、Rustfmt、提交 Clippy 和 Git diff check 通过。覆盖 macOS 本地沙箱；未做真实生产目录回滚、Windows/Linux 实机、桌面 E2E、CI 最终完成态核查。未来若实现正式恢复，仍须设计离线所有权、显式用户确认、原配置留存和失败事务恢复；不能将现有只用于测试的解包函数直接开放为生产恢复动作。
- **回滚约束与剩余差异**：出现兼容性回归时可回滚本轮数据根拒绝和端到端测试，但不得擅自删除迁移前备份、旧文档、未识别 `.partial` 或 `.corrupt-*` 证据。当前相对上游仍保留 Chimera 特有的带 SHA-256 的 manifest 与 Tauri 宿主 BackupPort，优先确保数据完整性而非机械复制上游的低验证逻辑。

## DIFF-101：Migration 状态加载与配置目录读取错误对齐、悬挂目录安全修复（2026-11-20）

- **参考版本与目标**：继续以固定 `ref@c4b35e048` 的 `backend/nyanpasu-migration/src/runner.rs::Error::{LoadState,ReadConfigDirectory}` 为准，消除共享 Runner 状态文件加载和配置目录预备份检查中的纯字符串错误边界。本轮风险 **L2**（迁移启动前的读操作、备份前数据保护）。不改 `ref` / Runtime gitlink、不碰实际用户目录，不变更原有 `migration-state.yaml` schema 2、Stamp、step ID/revision 或备份格式。
- **真正的共享初始化所有权**：新增 `chimera-migration::runner::load_state(path)`，由它调用 `MigrationStore::load` 并返回 `ExecutionError::LoadState {path,source}`，保留内部 `SchemaError`、IO 和 YAML 解析来源。Tauri `Runner::with_context_and_profile_migrations` 改用此共享入口，而非直接调用 store；高版本 state 在模块初始化/写盘前拒绝，启动报告仍从源错误链识别为 `NewerVersion`，并保留状态文件真实路径与 revision。
- **配置目录读取与安全修复**：`runner::has_nothing_to_protect` 对 `read_dir`、目录项迭代错误返回 `ExecutionError::ReadConfigDirectory {path,source: io::Error}`，与上游对应。特别区分真正缺失的配置目录和**悬挂目录符号链接**：`read_dir` 返回 `NotFound` 时再使用不跟随符号链接的 `config_node_exists` 检查节点是否存在；存在却不可读的用户目录绝不能误判为“新安装、无需备份”。发生这种错误直接阻止迁移，备份回调不执行，原始链接不变；普通全新目录依然允许跳过无意义备份。
- **宿主报告边界**：`startup_failure.rs` 对 `LoadState`、`ReadConfigDirectory` 解析精确路径，并继续保留底层权限/繁忙错误分类。真实 Tauri Runner 初始化的高版本 state 测试确认外层 `LoadState` + 内层 `SchemaError::Newer`、`NewerVersion` 报告、零写入；目录错误的报告保留原始 IO 来源。错误报告仍只读取现有异常及真实已验证快照，不创建/修改配置、不擅自宣布备份可恢复。
- **验证**：共享 `chimera-migration` **88/88**，包括新增加载高版本 store 的原始字节保留、悬挂配置目录拒绝及既有不可读配置祖先测试的结构化错误检查；Tauri 全库 **1034/1034**、1 项既有忽略，新增真实 Runner 初始化和启动报告路径/分类测试。`cargo check --locked -p chimera --all-features --tests`、Rustfmt、差异检查和提交 Clippy 钩子通过。未完成 Windows/Linux 实机、旧用户数据全量升级、桌面 E2E 和 GitHub CI 最终状态。
- **仍需对齐/回滚**：Tauri 仍持有 Legacy Verge/Clash 转换、Session 损坏恢复、平台路径、快照创建与备份修剪；整个 Migration 并非严格上游 100% 同型。后续优先验证备份恢复中的真实旧版本数据、跨平台路径节点和独立启动失败呈现。出现回归时回滚本轮 `load_state`/目录判定与报告映射，不清理用户原始文件、备份和 `.corrupt-*` 证据。

## DIFF-100：Migration 状态持久化、备份与文档 stamp 错误合同对齐（2026-11-20）

- **固定上游与目标**：参照 `ref@c4b35e048` 的 `backend/nyanpasu-migration/src/runner.rs::Error::{PersistState,CreateBackup,StampDocument}` 与 `backend/nyanpasu-persistence/src/write.rs`。本批风险 **L2**（迁移状态文件、写前备份、补写旧文档 stamp）。前轮已迁入共享 Runner 的步骤执行错误，尚有三类实际生产写入/备份失败仅通过 `anyhow::context` 返回文本。本轮统一为可 downcast 的 typed 边界；保持 Chimera 现有 `BackupPort` / Tauri `BackupRequest`/manifest、旧迁移步骤、文件 schema 和应用启动接口不变。
- **生产错误合同**：`chimera-migration::runner::ExecutionError` 增加 `PersistState {operation,path,source}`、`CreateBackup {source}`、`StampDocument {module,path,source}`。共享 `persist_state` 统一覆盖初始化 baseline、`in_progress`、单步骤完成、失败步骤状态和全局 `last_succeeded` 的所有实际状态写入，保留完整磁盘路径、操作阶段和 `chimera_persistence::write`/IO 原始来源。`ensure_backup` 只包装真实的宿主快照创建失败，不把无数据的新安装误报成已备份；`stamp_adopted_documents` 仅在所需快照创建成功后尝试原子补 stamp，并保留模块与文档路径。`RecordFailure.persistence` 字段现在可 downcast 为 `PersistState`，与原始 `RunStep` 错误独立可查。
- **成功状态安全边界**：最终提交 `last_succeeded` 后的 `flush_atomic` 若失败，共享 Runner 在返回 typed `PersistState` 前恢复**内存中的原成功版本**，避免同一个 Runner 对象在失败重试期间误认为新版本已经成功写盘；宿主仍只在共享 `run_pending` 完整成功后才修剪备份。旧文档被符号链接保护拒绝补 stamp 时，不设置 `stamped` 标志、不改目标文件，已经真实完成的备份可供故障诊断；备份本身失败时不得生成快照标记。
- **启动诊断与备份可信性**：Tauri `startup_failure` 从 `PersistState` 和 `StampDocument` 读取精确路径，`CreateBackup` 保持 `BackupError` 原始文件/目录来源，失败备份不显示为可恢复快照。`RecordFailure` 的持久化错误不是主错误链的 `source`（主链必须保留最初的步骤失败），因此启动诊断增加次要失败的路径回退和 IO 权限/繁忙分类，保留两个故障的文本详情；如主错误本身是权限/繁忙问题，仍优先遵从主错误。没有创建新的恢复 UI、自动覆盖任何旧文件或声称未经 manifest 校验的备份可用。
- **验证**：共享 Migration **86/86**，新增成功状态写盘失败后内存回滚、备份失败、补 stamp 软链接保护及双重失败 typed error 检查；Tauri 完整库 **1032/1032**、1 项原有忽略，其中真实 Runner 的无效备份目标检查 `CreateBackup` 类型，启动报告新测 `PersistState`、`CreateBackup`、`StampDocument` 路径及双重错误次级权限检测。后端 `cargo check --locked -p chimera --all-features --tests`、Rustfmt、Git diff check、提交 Clippy 钩子通过。仅本地 macOS 验证，未执行 Windows/Linux 实机、真实历史用户目录迁移、桌面 E2E 或 GitHub CI 最终结果。
- **剩余差异与回滚**：上游还包含 `LoadState`、`ReadConfigDirectory`、部分 `snafu::OperationError` 与独立迁移子进程/崩溃报告边界；本轮保持 `anyhow` + `thiserror` 外层合同，以兼容当前 Chimera 的 Legacy 转换端口和原始错误链。若发现生产回归，回滚本轮错误包装和启动诊断映射；不要删除 `migration-state.yaml`、原始数据、`.corrupt-*` 证据或用户备份。

## DIFF-099：共享 Runner 步骤执行结构化错误合同对齐（2026-11-20）

- **上游来源与目标**：参考 `ref@c4b35e048` 的 `backend/nyanpasu-migration/src/runner.rs::Error::{DetectBaseline,InspectModule,InspectDocument,CheckStep,RunStep,VerifyDocumentStamp,VerifyStep,RecordFailure}`。本轮风险 **L2**（迁移失败状态/启动恢复诊断），目标是让共享 Runner 生产调用路径直接返回可 downcast 的执行错误类型，而非只用 `anyhow::context` 拼接步骤或模块错误字符串。继续保留 `anyhow::Result` 对现有 `MigrationStep`、`ModuleMigrator`、Tauri BackupPort 的兼容接口，不在本轮统一替换全部底层模块为 `snafu::OperationError`。
- **实际生产执行错误合同**：新增 `chimera-migration::runner::ExecutionError`，将模块基线识别、模块/带 stamp 文档检查、步骤 `check`/`run`、执行后校验、失败状态 `flush_atomic` 等八类错误包装成 `thiserror` 结构化变体。每类记录原有 `module` 或 `step` ID，并通过 `#[source]` 保留 `MigrationCheckError` / `anyhow::Error` 原始来源；`VerifyDocumentStamp` 和 `VerifyStep` 保留上游代表性文本与缺陷语义。步骤执行失败仍 best-effort 调用 rollback，回滚失败只记录日志而不覆盖最初的步骤异常。
- **双重故障与持久化安全**：失败步骤标记落盘再次失败时，`ExecutionError::RecordFailure {step, source, persistence}` 同时保存**原始步骤错误**和**独立持久化错误**，前者继续是可追踪 source、后者是可检查的独立字段且显示于错误文本；不能把状态文件写入失败误认为步骤成功，也不会跟随迁移状态的软链接写向外部配置。保留原有 `mark_in_progress → flush → run → check → mark_failed/completed → flush` 顺序、迁移前备份保护、模块隔离和重试语义；`migration-state.yaml` schema 2、Stamp、原有步骤 ID/revision、备份格式均不改变。
- **Tauri 启动失败报告**：保留原来的 `StartupFailure` JSON 格式和类型分类逻辑；通过既有 `std::error::Error::source()` 链访问嵌套的步骤检查 IO 原因、具体配置路径和权限信息。`startup_failure` 的新增测试验证 `CheckStep` 穿透权限分类、`RecordFailure` 同时展示原始 KV/状态写盘错误；真实 Tauri Runner 的 `VerifyDocumentStamp`、`InspectDocument` 测试也验证结构化类型。此轮未增加自动恢复行为，未伪造备份路径或删除任何旧配置与 `.corrupt-*` 数据证据。
- **验证范围与限制**：共享 Migration **84/84**（新增步骤检查、步骤运行、后置条件、基线/模块检查、`RecordFailure` 双错误和软链接保护测试），Tauri 完整库 **1030/1030**、1 项原有忽略；后端 `cargo check --locked -p chimera --all-features --tests`、Rustfmt、Git diff check、提交 Clippy 钩子均通过。未执行 Windows/Linux 实机、真实历史用户目录升级、完整桌面 E2E 或 GitHub CI 最终核验。下一轮可对齐残余 `PersistState/CreateBackup/StampDocument` 的结构化包装及独立崩溃报告/启动边界；若出现生产回归，仅回滚本轮错误包装，保留旧数据、状态和备份。

## DIFF-098：Migration 修订状态不一致结构化错误与启动诊断对齐（2026-11-20）

- **上游来源及风险**：对照固定 `ref@c4b35e048` 的 `backend/nyanpasu-migration/src/runner.rs::Error::{DocumentBehindState,MissingDocumentStamp,FilesBehindState}` 与 Tauri `startup_failure`/子进程错误分层，风险 **L2**（升级数据与启动失败报告）。前轮已结构化 `NewerDocument`、`NewerAppliedModule`，但三种状态不一致仍使用纯字符串 `bail!`，无法按原因、文件路径和已应用 revision 精确呈现。此轮不修改 `migration-state.yaml` schema 2、Stamp、备份或文件恢复行为，不改 git submodule。
- **共享 Runner 错误合同**：`chimera-migration::runner::RevisionError` 新增 `DocumentBehindState {path,state_path,module,read,applied}`、`MissingDocumentStamp {path,state_path}` 和 `FilesBehindState {state_path,module,applied,reason}`，用 `thiserror` 保留以前错误说明及原始参数。`reconcile_documents` 与 `verify_applied_revisions` 现在返回可 `anyhow::downcast_ref` 的结构化错误，而不是拼接文本；判断与阻断时机完全不变，既不会重写有问题的文档，也不会自行清理 `migration-state.yaml`。
- **Tauri 启动失败映射**：`startup_failure::RevisionFailure` 以附加、`snake_case` 序列化变体接收上述三类错误，保留源文件与迁移状态路径、模块、revision 和原因；修正 `RevisionFailure::is_newer` 仅匹配真正的较新 schema/document/module，避免把文件回退、Stamp 丢失误判为 `NewerVersion`。三类不一致继续归类为 `MigrationFailed`，原有 `NewerVersion`、`Permission`、`Busy` 优先级不变。报告仅从原始 `std::error::Error::source` 链读取，不自动运行修复、删除数据或伪造可用备份。
- **验证**：共享 Migration 单元测试 **81/81**，含新增 `FilesBehindState` 真实核验路径及落盘内容不变测试；Tauri 原有 Stamp 丢失、Stamp 落后测试扩展为对结构化错误字段精确检查；Tauri 启动报告新增三类不一致、路径、序列化 roundtrip 与非 `NewerVersion` 校验，Tauri 全库 **1029/1029**、1 项既有忽略测试。`cargo check --locked -p chimera --all-features --tests`、Rustfmt、提交 Clippy、diff check 通过。尚未执行 Windows/Linux 实机、历史用户全量升级、桌面 GUI E2E 或 GitHub CI 最终状态。
- **剩余差异/回滚**：上游完整 `snafu::Error` 尚有 `CheckStep`、`RunStep`、`RecordFailure`、`InspectModule` 等逐项错误合同，而本地保留 `anyhow` 链和具体模块的错误类型；迁移子进程/独立崩溃报告器也未实现。本轮仅收敛影响启动诊断的修订不一致三项；若需回滚，只回滚本轮 typed error 与报告映射，不移除原始配置、迁移状态、备份或 `.corrupt-*` 证据。

## DIFF-097：共享 Migration Runner 完整编排与新版状态写前拒绝（2026-11-20）

- **上游基线与目标**：参考固定 `ref@c4b35e048` 的 `backend/nyanpasu-migration/src/runner.rs` 中 `Runner::run_pending` 和 `Runner::run_migration` 顺序。上一轮已将步骤执行、Document 检查、状态落盘及 Stamp 等函数下沉共享，但 Tauri 仍串联“校验版本 → 备份 → 模块执行 → 文件核验 → Stamp → `last_succeeded`”，易在两套宿主之间发生语义漂移。本轮风险 **L2**（启动迁移、备份保障、成功状态），目标是让共享 `chimera-migration::runner` 持有完整生产调用顺序；保留原有 YAML、Revision、Legacy IPC 和 git submodule 指针。
- **共享 Runner 完整编排**：新增 `runner::run_pending<C: AsRef<Ctx>, P: BackupPort>`，按上游次序检查已记录模块版本，确认有 pending 时经注入 BackupPort 创建已验证备份，逐模块执行步骤并隔离独立模块失败，核验文件实际 revision，补 Stamp（缺失快照时先备份），最后才写入 `last_succeeded`。新增 `runner::run_selected_migration`，单独执行指定步骤也沿用共享的版本检查/备份/执行事务，但不声称其他模块完成，也不修剪备份。共享 Ctx/Registry/Module/Document 不新增对 Tauri 的反向依赖。
- **Tauri 宿主收敛**：`backend/tauri/src/core/migration/runner.rs` 去除独立的 `ensure_backup`、`ensure_known_revisions`、`stamp_adopted_documents` 和 `verify_applied_revisions` 调度包装；Runner 仍向启动失败报告暴露 `backup_info`，仅把宿主 PathResolver/最后成功版本/目标版本交给 `TauriBackupPort`，继续提供真实备份的 manifest/验证/完整提交和成功后按保留策略修剪。成功创建完整快照时才记日志，旧错误上下文和从备份来源查找的诊断路径保留。保留在 Tauri 的 `run_step` 仅供现有 Runner 集成/错误回归测试。
- **安全修复**：在 `runner::ensure_baselines` 开头加入 `ensure_known_revisions`，严格在所有内存基线插入、reconcile 或状态 `flush_atomic` 前拒绝高于本构建支持的已记录修订；此前 `ensure_known_revisions` 仅在 Runner 构建完成之后运行，可能使其它模块的新增 baseline 写盘先于旧模块高版本拒绝。对比完整状态文件字节并断言未添加其它模块。新增共享编排失败注入测试：迁移前备份失败不得执行步骤/写成功状态；重试成功后只提交一次、重启不重复备份；单步选定迁移不得写全局升级成功版本。
- **验证及兼容**：共享 Migration 单元测试 **80/80**，Tauri Migration **61/61**，Tauri 全库 **1028/1028**（另有 1 项原有忽略），`cargo check --locked -p chimera --all-features --tests`、Rustfmt、提交 Clippy、Git diff 检查通过。上游的“失败模块先记录错误并继续其他独立模块、最终返回首个错误”以及备份失败阻止写入、成功后修剪等语义不变。未更改 `migration-state.yaml` schema 2、Stamp 文档格式、原有 Profiles/Typed Config/Storage 业务迁移 ID 或引入版本。
- **剩余差异及回滚**：Tauri 仍负责 Session 损坏恢复、Legacy Verge/Clash 转换、宿主路径/备份/启动注入和集成验证；共享 Runner 的错误合同目前是 `anyhow` + `thiserror`，未与上游全部 `snafu::OperationError` 逐项同型。尚未验证 Windows/Linux 实机、真实旧用户数据、桌面 UI E2E 或 GitHub CI 最终结果。若出现生产回归，回滚本轮编排委托与前置拒绝检查，不删除迁移前备份、旧配置及 `.corrupt-*` 证据。

## DIFF-096：Profiles Migrator 完整生产模块迁入共享 crate（2026-11-20）

- **上游来源与目标**：参照固定 `ref@c4b35e048` 的 `backend/nyanpasu-migration/src/modules/profiles.rs`，将 Tauri 原 `modules/profiles.rs` 中约 1,000 行生产逻辑完整迁入 `backend/chimera-migration/src/modules/profiles.rs`。本轮风险 **L2**（历史 profiles.yaml 升级、文档 stamp/回滚）；保持旧 `migration-state.yaml` schema 2、Chimera 专属 profiles domain schema、`0.24.1` 引入版本、现有四个步骤 ID 和 revision，不修改 ref / Runtime gitlink，不操作用户实际配置和系统代理。
- **上游四步骤生产所有权**：共享 `ProfilesMigrator` 实现 Document 模块，保留 `profiles/null_value` rev1、`profiles/script_newtype` rev2、`profiles/clean_schema` rev3、`profiles/repair_schema` rev4，以及 `UNSTAMPED_CEILING=4` 和 `ProfilesDocument::SCHEMA_REVISION=4`。泛型 `ModuleMigrator<C>` / `MigrationStep<C>` 使用 `C: AsRef<chimera_migration::context::Ctx>`，Tauri 与共享模块共用**唯一一份** schema 判断、旧 `!script` 标签变换、域模型校验、订阅/外部 symlink/组合节点转换、状态检查和失败回滚生产实现；共用 `chimera-migration::document` 与 `fs` 的原子文件/修订 stamp 端口。`ProfilesFormat` 改由共享 crate 定义，但仍是 `chimera_persistence::format::StampedYamlFormat<ProfilesDocument>`，与此前 `chimera_core::format` 的 re-export 为同一类型。
- **安全与兼容边界**：延续上游数据完整性策略：`clean_schema` 先保存 `profiles.yaml.bak`，转换后经 `chimera_config::profile::Profiles` 解析及 `validate()` 确认，再原子写出带旧注释头和 stamp 的配置；`repair_schema` 对已标记 rev3 但尚为旧 schema 的文件另留 `.recovery.bak`，不可静默重置或丢弃未知字段。回滚按原规则读备份并恢复；`migration-state.yaml` 仍由共享 Runner 持有。Tauri `core/migration/modules/profiles.rs` 只重导出共享生产类型与 `ProfilesFormat`，保留真实 Tauri Runner 的跨模块升级集成测试，未改变上游 UI 或 Legacy IPC。
- **测试迁移与验证**：原 Profiles 纯数据/文件迁移回归测试共约 32 项随代码迁至共享 crate，涵盖远端配置选项、UID、文件路径越界、symlink、脚本、新旧 current/chain、组合配置、损坏/重复值拒绝、`.bak` 与回滚；一项真实 Runner 集成测试留在 Tauri。共享 `chimera-migration` **77/77**、Tauri `core::migration` **61/61**、Tauri 完整库 **1026/1026**（1 项原有忽略）、`cargo check --locked -p chimera --all-features --tests`、Rustfmt、commit Clippy、Git diff check 均通过。Tauri 单测数量下降因测试随生产所有权迁至共享 crate，不是删除验证。
- **剩余差异与回滚**：Profiles、Typed Config 与 Storage 三个具体生产迁移模块现均由 `chimera-migration` 持有，Tauri 保留实际 Legacy Verge/Clash 转换、Session 恢复、宿主路径、备份生成/修剪与启动集成调度。迁移架构已经大幅收敛但不能声称 `ref` 完整 100% 对齐；仍需检查上游模块编排、错误合同、CLI/启动边界和真实老版本备份恢复。未验证 Windows/Linux、GUI E2E、真实用户数据及 GitHub CI。若发现生产兼容性回归，回滚本轮共享 Profiles 模块及 Tauri 重导出，不清理用户历史、迁移前备份和 `.corrupt-*` 证据。

## DIFF-095：Typed Config Migrator 生产所有权迁入共享 crate（2026-11-20）

- **上游基线与范围**：以固定 `ref@c4b35e048` 的 `backend/nyanpasu-migration/src/modules/typed_config.rs` 为参考，将本地 `typed_config/split_legacy_config` rev1、`typed_config/repair_clash_config` rev2 的生产步骤与 typed 文件完整/缺失/部分配置识别迁入 `chimera-migration/src/modules/typed_config.rs`；风险 **L2**（启动时配置拆分、旧 clash 配置、Session 恢复）。不更新 `ref` 和 Runtime 子模块，不访问用户真实配置或修改本机系统服务。
- **共享迁移生产逻辑**：`TypedConfigMigrator` 和两个 `MigrationStep<C>` 通过泛型 `TypedConfigHost: AsRef<chimera_migration::context::Ctx>` 运行；共享 crate 现在持有启动 baseline 判定、`application.yaml`/`session-state.yaml`/`clash-config.yaml` 的整体状态机、typed YAML 解析与验证、Legacy Runtime Clash 键识别、配置不完整及无法识别状态拒绝、拆分三文件的依序原子写入/失败时新文件回滚和旧 Clash 文件的修复写入。按 Chimera 已发布 `0.23.0` 门槛及原有两个 step ID 与 revision 保持兼容，不能直接复制上游 `2.0.0` 门槛或改名 `repair_clash_config_path`。
- **Tauri 宿主端口**：`backend/tauri/src/core/migration/modules/typed_config.rs` 仅保留 `TypedConfigHost` 注入和 Legacy `IVerge` 补丁/模板、`IClashTemp` 与 overrides 合并、`typed_config_from_legacy_parts` 转换、Chimera `serde_yaml` 格式序列化、`core::session_state::read` 损坏文件保存后恢复；配置文件状态分类、迁移步骤和原子写入不再在 Tauri 重复实现。共享 crate 使用既有 `chimera-config` typed 数据模型及相同 `serde_yaml` 编解码，未将 Tauri crate 引为依赖，Legacy UI/IPCs、Session 状态与用户配置字段不变。
- **安全边界**：按 `ref` 在识别部分 typed 配置之前先读取并恢复 Session；非法/不完整/无法辨认的 Clash 节点不被静默当作缺失；悬挂符号链接不被视为不存在。转换失败不创建新文件，多个 typed 文件的部分写入失败仅清理由本次步骤创建的文件；已有 typed 配置不会因重新启动被重写。与前轮 Migration Runner 的备份前检查及 `_nyanpasu` stamp/`migration-state.yaml` schema 2 协同保持旧版本升级语义。
- **验证**：共享 Migration 单元测试 **45/45**（新增 5 项 fake TypedConfigHost 的直接验证：两步步骤 ID、初次拆分/幂等、Clash 修复、转换失败、部分配置拒绝、符号链接目标保留）；Tauri Typed Config 原有 **15/15**、Tauri 完整库 **1058/1058**、1 项既有忽略测试；`cargo check --locked -p chimera --all-features --tests`、Rustfmt、仓库 commit 钩子 Clippy、diff check 均通过。未在 Windows/Linux、真实历史用户数据或桌面 E2E 上执行，也未核验 GitHub CI 最终结果。
- **尚未迁移与回滚**：Tauri `ProfilesMigrator` 的具体生产代码仍需按照上游 `ref` 迁入共享 crate；Typed Config 宿主端口因依赖本地旧 `IVerge` 与 Session 恢复仍保留在 Tauri，不能声称全部 Modules 100% 无宿主依赖。若生产回归，回滚共享 Typed Config 步骤及宿主注入即可，保留迁移前备份、旧配置和任何 `.corrupt-*` 证据。

## DIFF-094：Storage Migrator 生产模块迁入共享 crate（2026-11-20）

- **上游来源与目标**：参照固定 `ref@c4b35e048` 的 `backend/nyanpasu-migration/src/modules/storage.rs`，把 Chimera 已兼容的双步骤热键迁移**实际生产实现**从 `backend/tauri/src/core/migration/modules/storage.rs` 迁入 `backend/chimera-migration/src/modules/storage.rs`，不只是移动接口或测试。本轮风险 **L2**（历史热键和配置读写），不修改 `ref` / Runtime gitlink、不触碰真实用户文件、系统代理和服务。
- **共享实现与宿主适配**：共享 `StorageMigrator` 提供上游同 ID 的 `storage/hotkeys_to_kv` rev1 和 `storage/hotkeys_to_typed_config` rev2，按现有 Chimera `0.28.3` 版本门槛执行。泛型 `ModuleMigrator<C>` / `MigrationStep<C>` 通过 `AsRef<chimera_migration::context::Ctx>` 同时服务共享 Context 与 Tauri 适配 Context；静态步骤表按泛型类型提供，但生产代码只保留一份。Tauri 模块现在只重导出共享生产类型并保留真实 redb / 完整 Runner 的历史回归测试；原 Registry 顺序 `profiles → typed_config → storage` 不变，Legacy IPC 和热键 key 名不变。
- **上游安全和语义**：初次基线探测**不打开 redb**，避免迁移前备份完成前触发损坏库恢复；第 1 步 KV 保存成功后才移除旧 YAML 热键，第 2 步原子写入 typed `application.yaml` 后才删除 KV 的旧 key。保留显式空数组、Script 热键精确过滤、不可用/损坏配置错误来源、旧键缺失时无副作用、符号链接写入拒绝、原始字段保留。复用共享 `document`、`fs`、`HotkeyStoreOpener` 和上游风格错误类型；不新增 Tauri 反向依赖。
- **测试与影响范围**：共享 Migration 测试 **40/40**（其中 5 项新增共享 Ctx + 注入式 KV 测试，覆盖双步骤数据流、备份前只探测节点、KV 写失败的 YAML 保留、空值、符号链接安全）；Tauri 完整库 **1057/1057**、1 项原有忽略测试，包含 Storage 模块 12 项真实 redb/Runner 旧测试；`cargo check --locked -p chimera --all-features --tests`、Rustfmt、提交 Clippy、差异检查通过。未执行 Windows/Linux、真实历史用户数据迁移、桌面 GUI E2E 和 GitHub CI 完成态核验。
- **剩余差异及回滚**：上游其它 Profiles / Typed Config 模块仍在 Tauri，使用本地 Legacy 转换器、Profiles 文档形态和路径适配；完整迁移须逐个拆掉 Tauri 专有输入，不能机械复制上游模块导致版本/格式退化。发生不兼容时可整体回滚本轮共享 Storage 模块和 Tauri 重导出，迁移前备份、状态文件和任何 `.corrupt-*` 原始证据不得自动删除。

## DIFF-093：Migration Runner 执行事务与恢复核验下沉（2026-11-19）

- **参考基线与风险**：以 `ref@c4b35e048` 的 `backend/nyanpasu-migration/src/runner.rs` 为核心参考；本批风险 **L2**（运行前保护与旧数据迁移）。目标是在共享 `chimera-migration` 里统一 Runner 步骤执行、恢复和模块遍历的真实生产逻辑，同时保留 Tauri 的宿主路径、BackupPort 与具体迁移模块。不改 `ref` / Runtime gitlink，不清理用户历史、备份与 `.corrupt-*` 数据证据。
- **共享步骤事务**：`chimera-migration::runner::execute_step` 接管原 Tauri `run_step`、`finish_step`、`record_failure`、`check_step`、`ensure_satisfied`，沿用共享 `MigrationStep<Ctx>` 和 `Registry<Ctx>`。运行前先验证步骤需要性并落盘 `InProgress`；完成后核对 document stamp 和步骤条件，再连同模块连续 revision 记录 `Completed` / `Skipped`；发生检查、运行或后置条件错误时记录 `Failed` 并保留真实错误链，回滚为 best-effort，回滚失败不能掩盖原始运行错误；写状态文件失败时不执行下一次文件修改。
- **共享恢复和文档核验**：`runner::{ensure_baselines,reconcile_documents,ensure_known_revisions,verify_applied_revisions,stamp_adopted_documents}` 接管 stamped 文档与状态文件的双向核对，拒绝高于当前版本的文件、丢失 stamp、落后于状态的文件及不一致的落盘修订；恢复错误 `RevisionError` 移入共享 crate，Tauri 保留同一类型重导出供原有启动诊断 downcast 使用。旧文档只在完整核验达到模块 head 且 BackupPort 已成功返回后补 stamp，保证失败前不覆盖原始配置。新安装的 baseline 仍不创建无意义备份。
- **共享模块循环与最终提交**：`runner::{has_pending_steps,execute_pending_steps,commit_successful_run}` 负责逐模块执行、按 Registry 顺序处理 pending、独立模块失败时记录第一条错误并继续其他模块、重启仅重试失败步骤；全部模块、旧文档 stamp 和文件状态核验完成后才原子写入 `last_succeeded`。Tauri 仍负责 preflight 备份的实际文件生成/校验和最终备份 pruning，不能在成功状态落盘之前清理任何旧备份。单步 `run_migration` 沿用同一个共享执行事务。
- **兼容与未完成**：保留既有 `_nyanpasu` stamp、`migration-state.yaml` schema 2 和错误消息、Profiles 功能开关、Chimera 0.28.x 的步骤版本门槛、旧 IPC 以及 BackupRequest/Manifest 语义。共享 Runner 接收泛型上下文及注入的 Registry/BackupPort，Tauri `Runner` 剩余宿主编排与具体 `modules/{profiles,typed_config,storage}` 业务尚未全部迁出，尚不能声称整个 Migration crate 已按 ref 100% 对齐。
- **验证**：共享 Migration 直接验证成功、跳过、检查错误、运行错误、后置条件不满足、失败回滚、回滚失败不隐藏原始错误、重启重试、跨模块故障隔离、符号链接状态目标拒绝等，库测试 **35/35**；Tauri 完整库 **1057/1057**、1 项原有忽略项；`cargo check --locked -p chimera --all-features --tests`、Rustfmt、提交 Clippy、Git diff check 通过。仅 macOS 本地测试；Windows/Linux、用户历史数据实机升级、GUI E2E、远端 CI 尚未验收。
- **回滚和后续**：出问题先回滚本轮共享 Runner 委托与相关诊断重导出，不重置状态、不删除迁移前备份或 `.corrupt-*` 证据。下一步完整搬迁 Profiles/Typed Config/Storage 的业务模块，最终将宿主 `Runner` 收敛为纯注入适配。

## DIFF-092：Migration Module / Registry / Document 契约下沉（2026-11-19）

- **基线与目标**：继续对齐 `ref@c4b35e048` 的 `nyanpasu-migration/src/{lib.rs,registry.rs,fs.rs,runner.rs}` 和 `nyanpasu-persistence::format`。本轮风险 **L2**（启动前迁移检查及带 stamp 的文档）；不改两个 git submodule 的指针，不触碰真实用户目录、网络服务和既有应用配置。
- **共享模块契约**：新增 `chimera-migration/src/module.rs`，将 `StepCheck`、`MigrationCheckError`（保留 IO、YAML、KV、Stamp 原始错误来源）、`MigrationStep<Ctx>`、`ModuleMigrator<Ctx>`、`ModuleKind<Ctx>`、`DocumentSpec<Ctx>` 从 Tauri 搬到共享 crate。Context 采用泛型，Tauri 的 Profiles、Typed Config 和 Storage 模块继续实现共享契约并持有特有业务适配，避免迁移 crate 反向依赖宿主或修改旧 migration ID、revision、版本门槛和 Legacy IPC。Tauri `migration::mod` 只重导出共享类型，不再定义平行 trait。
- **Registry 实际所有权**：新增 `chimera-migration/src/registry.rs` 的 `Registry<Ctx>`，统一维护有序模块集合与步骤查找，构建时拒绝重复模块、重复 step ID、错误模块归属或未严格递增的 revision。Tauri 注册表只注入既有的 `profiles → typed_config → storage` 实例及必需顺序；Runner 消费共享 Registry 输出，不再自建重复的步骤列表/查找算法。已有 Heuristic rev2 冻结测试、Profiles 迁移文档顺序测试保持不变。
- **Document 实际所有权**：新增 `chimera-migration/src/document.rs`，统一 YAML 可选读取、暂态 IO 重试、悬挂软链接检查、stamp 解析/文件名校验、带指定前缀的原子写入及符号链接/目录目标保护；直接使用既有 `chimera-persistence::format` / `write`，保留 `_nyanpasu` stamp 和 `serde_yaml_ng` 字节格式。Tauri `core/migration/fs.rs` 仅保留兼容转发及原回归测试，不再复制核心解析和写入算法。
- **验证与剩余边界**：共享 Migration 库 **29/29**、Tauri 全库 **1057/1057**、1 项原有忽略测试；后端 `cargo check --locked -p chimera --all-features --tests`、Rustfmt、提交 Clippy/diff check 通过。共享 Registry 已覆盖查找、严格 revision 顺序、重复模块/错误归属；Document 已覆盖 stamp 读写、错 stamp、软链接保护。完整 Runner 执行循环、Profiles/Typed Config/Storage 的迁移业务实现和应用特有备份生成仍在 Tauri；这不是对上游独立 Migration crate 的全部完成。未进行 Windows/Linux 实机、旧用户全量升级、桌面 E2E 和 GitHub CI 验收。
- **回滚与下一步**：若发现生产迁移回归，先回滚本轮 Tauri 泛型迁移契约和共享 Registry/Document 接线，保留迁移前备份、旧配置以及任何 `.corrupt-*` 证据；下一轮集中迁移 Runner 的执行/恢复事务并逐步将具体 Module 转入共享 crate，完成对 ref 的完整迁移。

## DIFF-091：Migration Context 与 Backup Port 共享化（2026-11-19）

- **目标与基线**：以 `ref@c4b35e048` 的 `backend/nyanpasu-migration/src/{lib.rs,runner.rs}` 所定义的 `Ctx`、`PathResolver` 和迁移前 Backup gate 为准。本批风险 **L2**（旧配置备份及迁移前状态）。目标是让共享 `chimera-migration` 实际持有迁移路径、HotkeyStore 注入和安全备份决策；不更新两个 git submodule、不碰真实用户文件或启动服务，不改变旧 `migration-state.yaml` 的 schema 和 Legacy/UI/Agent 接口。
- **Context 与 Path**：新增 `chimera-migration/src/context.rs`：`MigrationPaths` 从宿主解析后的 config/data 根目录纯派生 Profiles、应用配置、Session、Clash、Storage 和迁移状态路径；常量复用 `chimera-paths`，不要求 UTF-8 路径、不创建目录。共享 `Ctx` 保存这些路径与 `Arc<dyn HotkeyStoreOpener>`。Tauri 的 `core/migration::Ctx` 保留宿主 `PathResolver` 和一个可 `Deref` 的共享 Context；除 Profiles `DocumentSpec` 函数指针适配外，迁移模块经共享 Ctx 获取派生路径与 KV opener，避免两套热键/文件存储业务逻辑。
- **Runner/Backup Port**：`chimera-migration::runner::{BackupPort,ensure_backup,has_nothing_to_protect}` 管理备份时机与“未成功备份就不得继续改写”的门槛：新安装（只有迁移状态 baseline）不创建备份；旧配置、Storage 文件或悬挂链接必须先备份；目录读取失败阻止迁移；已有快照不重复创建；备份失败不能将状态标记成已保护。Tauri `TauriBackupPort` 仅实现实际创建、完成校验的迁移备份，沿用 `BackupRequest::Migration`、关闭文件 `StorageSource::File`、manifest、安全目标与回滚约束；完成的快照才写入 Runner 的 `backup` 并可供启动故障诊断。原有 backup prune 与剩余 document/module 执行逻辑仍归 Tauri。
- **兼容验证与错误**：共享 Migration 测试 **23/23**（含空目录和 baseline-only 不做 IO、未转换的非 UTF-8 路径、旧配置/KV 触发单次备份、失败后重试、悬挂软链接与无效配置根目录）；Tauri Migration **93/93**（宿主和共享派生路径逐项等价、真实 Runner 完整备份与重试）；Tauri 全库 **1057/1057**、1 项原有忽略项；`cargo check --locked --all-features --tests`、Rustfmt、diff check 通过。仅 macOS 本地运行，不能当作 Windows/Linux、真实用户数据迁移或 UI E2E 的验收。
- **尚未完成/后续**：上游 `nyanpasu-migration` 的 `Runner` 完整循环、Document/Module traits、Registry、Modules 与 Backup 文件实现仍需分批搬迁；当前 Tauri 仅通过 BackupPort 注入实现。下批优先将 Module/Registry 对 `Ctx` 的引用改为共享上下文并迁入独立 crate，再收敛剩余执行循环/结构化错误；如发现运行时回归，回滚本轮 Context/Backup Port 接线即可，不能删除现有备份和 `.corrupt-*` 证据。

## DIFF-090：Migration 状态所有权与 Runner 调度策略下沉（2026-11-19）

- **目标与参考**：基于 `ref@c4b35e048` 的 `backend/nyanpasu-migration/src/{lib.rs,store.rs,runner.rs,fs.rs}`，新增 `backend/chimera-migration`（风险 **L2：迁移状态与启动恢复**）；验收是旧 `migration-state.yaml` 仍可按 schema 2 读取和重新写入，错误来源与保护行为不变，现有 Legacy/E2E/API 不受影响。非目标：本轮不把依赖 Tauri `PathResolver`、备份、Profiles/Typed Config 迁移模块的 Runner IO 逻辑直接挪进共享 crate，不修改真实用户数据和两个 git submodule。
- **共享迁移内容**：从 Tauri 提取上游同名的 `MigrationState`、`MigrationAdvice`、`store::{MigrationStore,ModuleState,TaskRecord,SchemaError}`、`runner::{advice_step,introduced_in_reached}`，并通过无 UI 依赖的 `StepSpec` 描述步骤元数据。状态机的启动/完成/跳过/失败流转、连续 revision 推进、版本 gating（包括 prerelease）、错误文本和存储 schema 保持现有行为。Tauri 原 `migration::store` 仅重导出共享状态类型，`Runner::advice_step` 和状态转换直接委托共享实现，只有模块基线检测通过注入模块回调留在 Tauri。
- **安全 IO 合并**：迁移状态 `load` 使用共享 `chimera_persistence::retry_io` 和 `symlink_metadata`，不把悬挂软链接当缺失文件。`flush_atomic` 保留原注释头、schema/字段名、文件写入格式；使用共享 `chimera-migration::fs::{atomic_write,validate_destination}` 在写入前拒绝符号链接、目录及其他非文件目标。Tauri 文档写入器也使用相同检查，避免两份安全策略漂移；未触碰 `.corrupt-*` 等证据文件。
- **剩余差异**：上游完整 `nyanpasu-migration` crate 自己持有 Runner、Module、Registry、路径/备份接口；Chimera 尚有 `core/migration/{runner,modules,registry}` 在 Tauri，并包含 Legacy 配置适配。下一轮应先提供独立的路径、备份、Document/Module Port，迁移剩余 Runner IO 与完整模块，最终消除 Tauri 重导出适配；本轮不可宣称 Runner 全部完成迁移。
- **验证及回滚**：新 crate 单元测试 **16/16**（从旧 Store 迁移的验证及新增状态调度/符号链接保护）、Tauri 迁移 **92/92**、Tauri 完整库 **1056/1056**、后端 all-features 编译、Rustfmt、提交检查通过；Tauri 测试数下降系 9 项原 Store 测试转移到了共享 crate，未删除验证。暂未执行 Windows/Linux、真实旧版用户升级与 GUI E2E；如果出现兼容性问题，回滚本轮 crate 接线和 Tauri 委托即可，用户配置、备份和旧证据文件必须保留。

## DIFF-089：Storage Hotkeys 两步迁移生产接线（2026-11-19）

- **固定基线与目标**：以 `ref@c4b35e048` 的 `nyanpasu-migration/src/modules/storage.rs` 和共享 `HotkeyStoreOpener` / `HotkeyStore` 契约为源，把热键从旧版 `chimera-config.yaml` 迁至 KV，再迁至 typed `application.yaml`。本轮不修改 `ref`、Runtime gitlink、真实用户文件、系统服务和网络设置。
- **上游算法与真实注册**：`backend/tauri/src/core/migration/modules/storage.rs` 保留上游两个 Migration ID：`storage/hotkeys_to_kv`（rev1）和 `storage/hotkeys_to_typed_config`（rev2）。模块注册在 `typed_config` 之后，确保 typed 文件先建立；Ctx 注入 `Arc<dyn HotkeyStoreOpener>`，生产使用共享 `chimera_core::storage::RedbHotkeyStoreOpener`，测试允许注入拒绝访问源。第 1 步保存非空旧 YAML 热键后才移除旧字段；第 2 步在 typed 文件存在时将 KV 中的列表（含显式空列表）替换 typed 字段，并且只有 typed 原子写入成功后才删除 KV 旧键。
- **安全与兼容边界**：保留上游 `clash_mode_script` 精确函数名过滤、保留其他 typed 应用字段、键不存在时无副作用，以及错误源和路径。使用 Chimera 的 `chimera_config_path`、`serde_yaml_ng`、具有链接保护的 `migration::fs::atomic_write`；上游 `2.0.0` 引入版本在本产品适配为 `0.28.3`。本地 Runner 在基线检测之后才建立迁移前备份，因此基线检测只读取 YAML 并保守检查存储节点是否存在，**不提前打开 redb**（它可能触发损坏文件恢复）；对损坏旧 YAML 的解析错误也延迟到备份后按步骤报告。无旧热键的首次安装不创建 KV，也不重写 typed 热键。
- **类型化错误与剩余差异**：共享 `MigrationCheckError::Storage` 改为原始 `BoxedCause`，匹配上游动态存储端口；Storage 模块的读取、解析、序列化、检查、原子写入、存储打开/保存/删除错误均保留来源链，当前按本地 `thiserror` + `anyhow` 合同适配上游 `snafu` 错误型态。迁移框架当前仍位于 Tauri，未整体搬到独立 `chimera-migration` crate，也未接入上轮 Live Backup 的用户可见手动入口；这些仍属于后续架构工作。
- **回归覆盖**：真实 redb + 临时路径验证 KV→typed、旧 YAML→KV→typed、空列表清空、缺失旧键幂等、Script 精确匹配、typed 缺失/损坏以及符号链接写入失败后旧 KV 不丢、注入打开失败保留真实 `io::Error`；真实 Runner 验证迁移前备份、完整两步流程、状态记录、二次启动不重写和不安全备份拒绝。Migration 定向测试 **101/101** 通过；原有备份失败和损坏文件保护测试全部保留。生成的 Specta Storage 文档注释同步到共享契约，不改变 TS 类型/IPC 形状。跨平台 E2E、真实用户旧版本数据和远端 CI 仍须另行验收。
- **后续**：进一步将 Migration 迁出 Tauri 到上游式独立 crate，并打通手动 Live Backup UI；保持旧迁移状态 schema 与用户数据兼容，不通过重置应用数据修复测试。回滚时保留已生成的备份及 `.corrupt-*` 证据。

## DIFF-088：Storage 共享所有权及 Live Snapshot / Hotkey Port（2026-11-19）

- **基线与范围**：以 `ref@c4b35e048` 的 `nyanpasu-runtime-values/src/storage.rs`、`nyanpasu-core/src/storage.rs`、`nyanpasu-migration/{backup.rs,modules/storage.rs}` 为准迁移；不改变 `ref` 或 Runtime gitlink，不访问用户实际数据库与系统配置。重点消除 Tauri 持有的 redb 数据所有权和重复的错误分类实现。
- **共享数据所有权**：`chimera-contracts/src/storage.rs` 定义上游同形的 `StorageRevision` / `StorageItem`（含 `INITIAL`、`checked_next`）及 `StorageSnapshot`、`HotkeyStore`、`HotkeyStoreOpener` 端口。`chimera-core/src/{storage.rs,redb_ext.rs}` 接管 redb 打开、恢复、条件提交、变更广播、错误分类及原有 Storage 测试；Tauri 仅保留路径注入、typed/legacy 窗口事件和现有 IPC 的兼容重导出，不复制 Core 的数据库算法。
- **上游快照/Hotkey API**：共享 Core `Storage::export_to` 从一个 redb 读事务复制全部键值到新的数据库文件，禁止覆盖已有目标；为 `StorageSnapshot` 实现动态端口。`RedbHotkeyStoreOpener` 与 `HotkeyStore` 支持按 key `hotkeys` 的缺省读取、持久写入和删除，遵循短生命周期迁移句柄语义；尚未接入完整的上游 Storage Migrator。
- **备份生产 API**：Tauri `StorageSource::Live(&dyn StorageSnapshot)` 使 `create_backup` 能在源数据库仍打开时导出一致性快照。原有 `StorageSource::File` 继续为启动前 Migration 复制关闭的文件，不能在打开的 redb 数据库上当作安全快照。Live 失败以 `BackupError::Snapshot` 保留原始错误，启动诊断保留失败路径，备份仅清理本次预留的 `.partial`。现有项目尚未从用户可见的手动备份按钮调用 Live 分支。
- **兼容与非目标**：保留 Chimera 原有的 Revision CAS、Legacy WebStorage key fallback、广播顺序、旧 Tauri 事件名称和 JSON 形状、redb 4.3 兼容分类；未引入上游完整的 snafu/specta StorageOperationError，也未宣称所有 Migration/Backup UI 已对齐。原有 Storage 数据不会自动迁移或被清理。
- **验证**：`chimera-core storage::tests` **17/17**、`chimera-core redb_ext::tests` **6/6**、Tauri 启动 Storage **2/2**、Backup **22/22**（含 Live 一致性、原文件不覆盖、失败后清理）、Migration **88/88**，IPC Storage 定向测试通过；`cargo check --locked -p chimera --all-features --tests`、Rustfmt、差异检查通过。只验证本机 macOS，Windows/Linux、用户真实热键迁移、桌面 E2E 与发布流程仍未验收。
- **后续**：完整对齐上游 `nyanpasu-migration` Storage Migrator 与 typed 错误合同，并在有完整 UI/备份边界测试后接入 Live Backup 到真正的手动备份入口。若回滚本轮，应先取消 Live 备份接线再恢复 Tauri Storage owner，不能删除已有的 `.corrupt-*` 证据。

## DIFF-087：Core Storage 启动恢复对齐 ref（2026-11-19）

- **目标与基线**：以 `ref@c4b35e048` 的 `backend/nyanpasu-core/src/storage.rs` 为准，将 `StorageInner::{try_new_with_open,open_database,create_and_init_database}` 与 `transient_database_error`、`confirmed_database_problem`、`preserve_database` 的启动恢复语义迁入 `backend/tauri/src/core/storage.rs`；风险 L2（数据库恢复与旧用户数据）。本轮不更新 `ref`、Runtime gitlink，不直接访问用户数据库，也不修改系统代理/服务。
- **实现与上游映射**：对齐上游的 `symlink_metadata`、`chimera_persistence::{retry_io,retry_with_delay,retry,is_link,preserve}`，沿用 `utils/redb_ext::{RedbFailure,find_failure}` 分类；只对明确损坏、不兼容或缺失表的数据库保留原节点并重建。暂态 IO/文件锁按上游 50/100/200 ms 重试，其他未确认失败直接返回而不改动原文件；目录与符号链接保留为 `.corrupt-*` 证据，不再递归删除。打开已有库还要验证预期 `clash-nyanpasu` 表存在。
- **必要兼容差异**：上游已将 Storage 归属 `nyanpasu-core`，Chimera 本轮仍在 `tauri/src/core/storage.rs`，保留现有 `StorageRevision` 条件提交、namespaced/legacy fallback、typed/legacy 事件和 IPC 契约；错误类型仍是本地 `thiserror` 结构而非上游的完整 `snafu`/`specta` 类型。当前 redb 是 4.3 系列，而上游版本可能不同；剩余迁移需结合迁移/备份 ports 与前端 wire 合同统一收敛，不能宣称 Storage 已完全对齐。
- **验证与失败历史**：macOS 本机 `core::storage::tests` **17/17**（损坏文件证据、缺失表、目录、链接、异常上级路径、暂态重试耗尽、未确认错误保留原始字节、现有 revision/Legacy 行为）；`core::backup::tests` **20/20**；`core::migration::` **88/88**。全 feature Tauri 测试目标编译检查及 Rustfmt/diff check 通过。一轮按 `ipc::tests` 过滤的测试额外命中 Service IPC 时序用例，其中 **1/23** 失败；该用例隔离重跑 **3/3** 通过，未修改测试或关闭断言，独立并发不稳定性尚需调查。未执行 Windows/Linux、真实用户数据库升级、全量 E2E，不能将定向验证视为发布验收。
- **回滚与后续**：如出现不兼容，可回滚本轮 Storage 修改；已产生的 `.corrupt-*` 文件必须由用户明确处理，不允许清理脚本自动删除。下一批优先搬迁共享 Core Storage 所有权和细粒度错误合同，并对上游 `StorageSnapshot`、HotkeyStore / Migration ports 建立完整路径映射与回归。

## DIFF-086：ProfileFsPort 文件操作的精准类型化错误映射（2026-11-13）

- **基线与范围**：在固定 `ref@59724e85c`、`backend/chimera-runtime@d56b083` 和本地 `codex/ref-os-dns-20261112` 上继续承接 DIFF-085。只改 `backend/tauri/src/service/profile_file.rs` 文件 I/O 失败的错误映射与对应测试，不改 App/Profiles 配置、文件事务算法、IPC 错误协议或生产行为；未合并、未推送。此轮风险为文件操作错误来源和日志文本的变化。
- **对应上游**：对照 `ref/backend/nyanpasu-profile-io/src/fs_service.rs` 的 `ProfileFsPort` 实现和 `ref/backend/nyanpasu-contracts/src/profiles/error.rs`，将 `read` 的直接 I/O 错误映射为 `ReadFile`（路径、原始 `io::Error`），将 `write_atomic` 的 `atomicwrites` 错误封装为 `AtomicWrite`（路径、原始库错误对象），将 `read_external` 映射为 `ReadExternalTarget`（外部目标、原始 `io::Error`），将 `ensure_not_symlink` 的叶节点拒绝与 metadata 读取错误分别归类为 `UnexpectedSymlink` 和 `InspectPath`。`ensure_symlink` 保持原判断/链接更新顺序，将已有文件占用、旧链接删除、metadata 读取、链接创建分别归类为 `ExistingFileBlocksSymlink`、`RemoveFile`、`InspectPath` 和 `CreateSymlink`。
- **兼容与安全边界**：`resolve`、目录链验证、`ensure_managed_parent`、`remove_nofollow`、`symlink_points_to` 及物化 journal/cleanup/recovery 暂仍返回旧的 `anyhow::Result`，避免未保留失败阶段与真实 `io::Error` 的错误统一包装。现有 Tauri `ProfileFsPort`/`ProfileMaterializationPort` **仍然是 `anyhow` 签名**，本轮仅在实际生产代码中通过 `?`/`into()` 保留底层 `ProfileFileError`，使调用方可 `downcast_ref` 且原始来源链不丢失。上游 Contracts 的完整 typed Port **仍未作为生产 Trait 接线**；不同错误文本可能影响日志消费方，须在跨平台/桌面验收时检查。
- **新增断言**：缺失托管文件与外部文件能精确 downcast 为 `ReadFile` / `ReadExternalTarget`，并保留目标及 `ErrorKind::NotFound`；将目录作为原子写入目标时返回 `AtomicWrite`，原目录标记内容不被改写；既有符号链接拒绝用例强化为 `UnexpectedSymlink`；已有正常文件阻挡 symlink 的保护强化为 `ExistingFileBlocksSymlink`。若 Windows 测试环境禁止创建测试链接，沿用旧测试的权限跳过规则，不能把跳过视为已覆盖 Windows symlink。
- **实测验证**：`cargo check --locked -p chimera --all-features --tests` **通过**；`cargo test --locked -p chimera --lib service::profile_file::tests -- --test-threads=2` **45/45 通过**；`cargo test --locked -p chimera --lib client::profiles::tests -- --test-threads=2` **86/86 通过**；`cargo fmt --check`、提交钩子进一步校验。未执行 Windows/Linux CI、实际提权/真实配置、全部后端单测或桌面 E2E。
- **后续工作**：分别审计路径解析/目录链错误、`remove_nofollow`、journal 读写与恢复的 typed mapping，在完整覆盖后再将生产 `ProfileFsPort` 和 `ProfileMaterializationPort` 替换为 Contracts 签名；严格保持 Windows reparse point 拒绝、no-follow、不覆盖旧状态及异步失败传播。


## DIFF-085：完整 Profile 错误及文件/事务 Port 契约下沉（2026-11-13）

- **基线与范围**：继续固定 `ref@59724e85c` 和 `backend/chimera-runtime@d56b083`，仅在 `codex/ref-os-dns-20261112` 开发分支操作，不推送、不合并、不触碰实际用户 Profiles 数据或系统代理。Mac 开发助手连接恢复后发现磁盘仅剩 123 MiB，因此在确认 Cargo/rustc 未运行后仅清除 `backend/target/debug/incremental/chimera_lib-*` 等可再生成构建缓存，为编译/测试腾出空间，Git 工作树未受清理影响。
- **共享错误契约**：以 `ref/backend/nyanpasu-contracts/src/profiles/error.rs` 为源复制完整 `ExpectedNode`、`ProfileFileError`（文件读写、目录/符号链接安全、materialization journal、cleanup fencing、恢复和异常路径）、`ErrorPath` 及已有的 `SubscriptionFetchError`，在 `backend/chimera-contracts/src/profiles/error.rs` 只替换 config crate 名。增加 Chimera 既有路径的显示序列化回归和 `ReadFile` wire shape、原始 `std::io::Error` 来源链测试；不改现有线上错误 wire 协议。
- **文件和物化端口**：以 `ref/backend/nyanpasu-contracts/src/profiles/ports.rs` 替换之前仅有订阅部分的 Contracts Port，完整保留 `ProfileFsPort`、`ProfileMaterializationPort`、`SubscriptionFetcher` 的上游类型化签名，以及 `MaterializationResource`、`PreparedMaterialization`、`PreparedCleanup`、`CleanupOutcome`、`ProfileDegradation*`、`MaterializationReconcileReport`。除 crate 名和文件行尾规范化外结构与固定 ref 一致。
- **Tauri 兼容性**：`backend/tauri/src/state/profiles/ports.rs` 原有重复的八种事务/降级数据类型改为共享 Contract 重导出。原有 `ProfileFsPort` / `ProfileMaterializationPort` 的 **`anyhow::Result` 签名暂保留**，它们的 `ProfileFileService` 实现、已有状态机、文件物化、恢复和调用线程规则均未改动。上游 typed Port 已可编译，但尚**未作为生产端口接线**。此分界保证不把所有文件错误草率归成同一个变体，后续需要按文件操作失败阶段逐个转换，保留真实 `io::Error` 来源和清理围栏语义。
- **实际验证**：`cargo test --locked -p chimera-contracts --lib` **8/8 通过**；`cargo check --locked -p chimera --all-features --tests` **通过**；`cargo test --locked -p chimera --lib service::profile_file::tests -- --test-threads=2` **43/43 通过**；`cargo test --locked -p chimera --lib client::profiles::tests -- --test-threads=2` **86/86 通过**。清理与测试过程未改动 `ref`、runtime 或开发分支以外的 Git 引用。仍未执行 Windows/Linux 真实 CI、完整桌面 E2E 和系统文件错误注入；不宣称跨平台验收通过。
- **下轮与回滚**：先按 ref 审计 `ProfileFileService` 每个文件与 journal 操作的错误映射，再分阶段让其实现 typed Port 并调整 Actor/IPC 接口；不能简单把 `anyhow` 强制转换成某一个 `ProfileFileError`。此轮的 Contracts 和 Tauri 重导出将分批提交，按相反顺序可独立回滚。


## DIFF-084：订阅 HTTP、解析器、设备头和结构化网络错误下沉（2026-11-13）

- **基线与目标**：基于只读固定 `ref@59724e85c`，将 Profiles 订阅下载中的请求、代理读取、响应头解析和错误归类，从 Tauri 的 Profile 文件服务移到独立 `chimera-net`。保持当前 `codex/ref-os-dns-20261112` 开发分支，不改 `ref` 或 runtime gitlink，不碰真实配置/宿主网络、不推送或合并 `master`。风险 L2：影响生产订阅 HTTP 请求与用户代理选择。
- **上游源码映射**：复制改名 `ref/backend/nyanpasu-net/src/{subscription_http.rs,subscription.rs,device_headers.rs,key_value.rs,policy.rs}` 到 `backend/chimera-net/src/`，连同上游 `subscription_http/error_contract_tests.rs`，主要差异是 `nyanpasu`→`chimera` 品牌和 crate 名。`SubscriptionHttpFetcher::with_http_timeout` 为现有外部兼容测试由仅 test 作用域提升为公开配置方法；实际请求/代理/重试算法未更改。将 `ref/backend/nyanpasu-contracts/src/profiles/error.rs::SubscriptionFetchError` 和 `ref/backend/nyanpasu-contracts/src/profiles/ports.rs` 的 `FetchedSubscription`、`SubscriptionFetcher` 精确抽取到 `chimera-contracts`，保留 HTTP 客户端构建、请求、HTTP 状态及读取响应体四种类型化错误和原始错误来源链。完整 Profile filesystem/materialization 类型化 Port **尚未迁移**。
- **生产接线**：`backend/tauri/src/service/profile_file.rs` 删除直接创建 `reqwest::Client`、重试/解析的第二套生产实现，使用上游 `chimera_net::subscription_http::SubscriptionHttpFetcher`；Tauri 侧旧 `SubscriptionFetcher` 暂维持 `anyhow::Result` 并通过薄适配器转发（错误以 context 保留来源链）。`backend/tauri/src/state/profiles/ports.rs::FetchedSubscription` 改为重导出唯一共享类型，保留 `content_type`、`file_name` 新元数据；旧 Actor 目前仅消费旧字段。`SelfProxyPortSource` 也只保留共享契约，移除重复 Tauri Port 定义。`setup.rs` 注入实际 `BundleMetadata`、应用 mixed port、实时 OS proxy source 和在 Tauri 宿主构造/净化的 `DeviceHeaders`，使硬件设备识别不下沉到 net crate。保留既有 `clash-chimera/v{version}` 默认 User-Agent、`x-hwid` 以及当前 Profiles Actor 调度/文件事务逻辑。
- **兼容范围和未迁移代码**：旧 `config/profile/item/remote.rs::subscribe_url` 仍是独立 Legacy 订阅入口；本批没有假装其已迁移。Tauri Profiles Actor 的 `anyhow` 网络错误合同和文件事务相关 Port 仍在 Tauri，下一轮需审查完整上游 Profile 错误和数据所有权后迁移，不能任意更换 IPC wire shape。Tauri 原三个仅验证 Header 解析的测试与重复实现同时删除，等价测试直接存在于上游派生的 `chimera-net/src/subscription.rs`；真实 Tauri HTTP 测试保留。
- **实际测试**：新 `chimera-net --lib` **19/19** 通过（响应头、元数据、代理优先、直接回退、超时、拒绝/重试策略及错误 wire/source-chain）；`chimera-contracts --lib` **5/5**；Tauri 全功能测试目标 `cargo check --locked -p chimera --all-features --tests` **通过**；Profiles Actor `client::profiles::tests` **86/86**，Profile File `service::profile_file::tests` **43/43**（解析测试转至 net 后）。只使用临时目录及 loopback HTTP；没有实际订阅抓取、宿主代理变更、真实系统提权。完整 workspace、Windows/Linux 编译、真实用户订阅和桌面 E2E 未验证。
- **剩余工作与回滚**：继续对齐完整 `profiles` 错误/文件 Port、独立 Legacy 订阅 HTTP 调用路径和 App/Net 网络策略；生产行为改动必须保持旧配置与现有行为，发现不兼容需记录理由。共享契约/网络模块和 Tauri 接线分两笔独立提交，均可按相反顺序回滚；不涉及用户数据迁移。


## DIFF-083：System Proxy Source / Changelog HTTP 的 ref Port 及真实生产注入（2026-11-13）

- **固定基线与边界**：继续以 `ref@59724e85c` 为只读基准。在 `codex/ref-os-dns-20261112` 迁移按需系统代理 URL 读取及更新、订阅、发布日志网络消费者，不读取/改写真实代理设置以进行测试。不合并、不推送、不修改 `ref` 或 runtime gitlink。风险为网络路由和启动依赖 L2。
- **上游源码对应**：`ref/backend/nyanpasu-contracts/src/net.rs::SystemProxySource` → `backend/chimera-contracts/src/net.rs` 同形同步 Port；`ref/backend/nyanpasu-os/src/system_proxy_source.rs` → `backend/chimera-os/src/system_proxy_source.rs`，**实现只替换 crate 名**；`ref/backend/nyanpasu-net/src/changelog_http.rs` → 新独立 `backend/chimera-net/src/changelog_http.rs`，保留按每次请求读取代理、连接超时、总超时、错误源的机制，仅更换品牌 `Chimera/`、契约 crate 名和新增隔离测试。新 `chimera-net/src/policy.rs` 当前只引入上游同形 `get_self_proxy(port)`；其余 network policy/diagnostics/订阅适配器暂未迁移，不能宣称整个 net crate 已对齐。
- **生产接线**：`backend/tauri/src/setup.rs` 创建唯一 `Arc<dyn SystemProxySource>`，分别注入 Profile File、`TauriAppUpdateBackend` 与新的 `chimera_net::changelog_http::ReqwestChangelogHttp`。App Update 保留 Chimera 特有 `fixed-webview` 更新端点与退出逻辑，移除 Tauri 直接 `utils/config::get_system_proxy` 查询。`service/profile_file.rs` 删除私有 `SystemProxySource` 和内置的 `Sysproxy::get_system_proxy`，在构造时接受 shared Port；自身代理优先、系统代理次之、失败回退直连的语义不变。Legacy `utils/config.rs::get_system_proxy` 保留兼容函数签名，但委托同一个 OS 实现以覆盖旧订阅/HTTP 调用方，不再重复实现原始查询。
- **不迁移的真实差异**：IPC、Agent diagnostics/actions 及遗留 Sysopt 读取需要完整 `Sysproxy` 字段（enable/host/port/bypass）以进行状态诊断和恢复，不能错误地替换成只返回 URL 的共享 Port。这部分尚有直接 OS 读取，待后续分离类型化完整状态读取端口和所有权。测试用的 `ProfileFileService::new` 仅在 `cfg(test)` 下可用并注入无副作用假源；生产使用 `new_with_system_proxy`。
- **本轮验收**：Tauri `cargo check --locked -p chimera --all-features --tests` 通过；Profile File 定向测试 **46/46**（新增系统代理读取错误回退直连、自身代理优先且不额外读取系统代理）；App Update 定向测试 **3/3**；新 `chimera-net` **1/1**（通过两个不同的本地 HTTP 代理验证同一 Changelog 客户端每次请求读取当前代理）。全部使用模拟端口或本地回环网络，未修改宿主系统代理。提交前还需 Rustfmt/Clippy hooks；未执行 Windows/Linux CI、真实用户代理切换、发行包更新及完整桌面 E2E。
- **回滚**：可按 Contracts/OS source 与 Net/Tauri 接线批次依次回滚；无配置迁移或宿主网络侧效应。后续优先依 `ref/backend/nyanpasu-net/src` 迁移更完整的网络策略和订阅处理，但不得在尚无验证时破坏 Legacy Profile 兼容语义。


## DIFF-082：同步最新 origin/master，保留上游 CI 修复并 rebase（2026-11-13）

- **来源与操作**：从本地 `master@7f08640f`、开发分支 `codex/ref-os-dns-20261112@2d8e2351` 开始，`git fetch --prune origin master` 获取远程 `origin/master@ba79abf6`，新增 6 笔主线提交（Profiles 转换链、E2E、manifest、CI 测试修复）。初始相对远程主线为落后 6、领先 13。仅 `backend/tauri/src/client/core_lifecycle/mod.rs` 与本地改动重叠，`ref`/runtime gitlink 无变化。随后对**当前开发分支**执行有 GPG 签名的 `git rebase origin/master`；没有更改本地 `master` 指针、远程 `master`、配置或子模块，也没有推送。
- **唯一冲突及处理依据**：本地旧 `6d3dea47` 原本修正 Core Lifecycle 两个“已停止”测试夹具的 `state_changed_at=0` 误判；远程 `ba79abf6` 已用 `ConfirmedStoppedCore`（`state_changed_at=1`）修复**完全相同两项用例**，同时保留 Service 与 Normal 的原运行模式。为避免覆盖上游且重复建立 mock，rebase 对该单一旧提交执行 `--skip`；不再使用旧提交中的 `CoreUpdateRecordingCore` 夹具修正。旧 DIFF-079 是当时的历史结果，其测试夹具说明**由本条记录替代**。其余 12 笔提交按原顺序完成 rebase、重签，无 squash。
- **旧 SHA → rebase 后 SHA**：`1f2ee880` → `bbbba22d`；`e03423b6` → `60a18761`；`f19a28a0` → `0b5fb57d`；`12903261` → `6ec8c245`；`08f63dd0` → `92a225e5`；`d2f91f9c` → `ba91584c`；`a1d43a63` → `2cdcec27`；`6d3dea47` → **由远程 `ba79abf6` 等价覆盖，不再提交**；`7841abf6` → `d781ef64`；`0e9a6c55` → `be331dba`；`a81fcee1` → `5138f5e3`；`3f6e0bbe` → `70388a27`；`2d8e2351` → `c38b24a1`。旧 DIFF-075～081 文本中的提交号用于记录它们发生时的历史位置；需要执行 `git show` / `git revert` 时应使用此处对应的新 SHA。
- **rebase 后实际验证**：`cargo check --locked --manifest-path backend/Cargo.toml -p chimera --all-features --tests` **通过**；`cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib client::core_lifecycle::tests -- --test-threads=2` **39/39**；`cargo +stable test --locked --manifest-path backend/Cargo.toml -p chimera-contracts --lib` **5/5**；`cargo +stable test --locked --manifest-path backend/Cargo.toml -p chimera-os --lib -- --test-threads=2` **23/23**。完成检查时工作树干净、相对最新 origin/master 领先 12 落后 0。**尚未验证**当前重写提交在 Ubuntu/Windows 的远程 CI、真实 Service/核心更新提权、发行安装包和桌面 E2E；不得将本机 macOS 测试当作跨平台成功。
- **下一阶段**：此处仅记录审计，不合并或推送；先确认开发分支、重写后提交签名与本地检查，再决定如何进入主线并等待真实 CI 信号。原有历史提交与 rebase 前的引用可经 Git reflog 回查，但不可直接当作当前主线 SHA。


## DIFF-081：Core Install CI 可验证性与真实拒绝授权回归（2026-11-13）

- **审计基线**：继续沿用 `ref@59724e85c` 和主仓 Core Install 提交 `a81fcee1`、`3f6e0bbe`；本轮不修改上游引入的 OS 复制/提权实现、Core Install 共享契约、`ref` 和 runtime gitlink。重点检查 Windows/Linux 构建验证是否真正完成，以及生产更新流程中拒绝权限后的实际文件效果。
- **三平台 CI 范围**：`.github/workflows/ci.yaml` 的后端矩阵包含 `ubuntu-24.04`、`macos-latest`、`windows-latest`，对应 `pnpm test:backend` 使用 `cargo test --locked --manifest-path ./backend/Cargo.toml --workspace --all-features`。当前本地工作分支未推送，**这些平台尚未在包含本轮改动的提交上执行 CI**。本机只有 `aarch64-apple-darwin` Rust target，不能从 macOS 本地测试推断 Windows/Linux 编译已通过。本轮也没有安装跨编译依赖、操作真实管理员授权或覆盖系统核心文件。
- **macOS 发行包核对**：`backend/tauri/src/core/updater/instance.rs::extract_tar_gz_core` 当前按条目文件名 `clash-rs` 提取，虽然错误消息写作 Chimera Client，但 `scripts/src/prepare/binaries.ts`、`scripts/utils/resource.ts` 确认 macOS 的 Chimera Client/Alpha 发布包内执行文件路径仍是 `{archive-dir}/clash-rs`。因此没有仅凭品牌名称修改提取器；更准确的发行包兼容性仍需依赖真实归档下载/本地打包验证。
- **新增真实拒绝授权回归**：`backend/tauri/src/client/core_lifecycle/mod.rs::tests::refused_os_core_copy_keeps_target_and_does_not_restart` 以已确认停止的 `CoreUpdateRecordingCore` + 真实 `chimera_os::core_install::FsBinaryInstaller(RefuseElevation)` + 临时目录驱动完整生命周期调用。目标目录被预先设为目录（跨平台可稳定触发普通复制失败），确认旧文件标记保留、错误含授权失败信息、没有重启/rebuild、进度上报 `finished-error`；完成后暂存 `TempDir` 按所有权规则清理。没有修改生产停机和安装语义。
- **实际验证**：`cargo test --locked -p chimera --lib client::core_lifecycle::tests -- --test-threads=2` **39/39 通过**；`cargo +stable test --locked -p chimera-os --lib core_install -- --test-threads=2` **8/8 通过**，对照 CI stable 工具链；格式检查通过。后续执行 Git hooks 后记录最终提交状态。**仍未验证** Windows/Linux 本地或远端编译/完整测试、真实发行包更新、Windows UAC/macOS 提权对话框、桌面 E2E。
- **后续验证与回滚**：在 CI 可以访问本轮提交时，优先审核三系统的 `Test backend workspace` 及 `Build backend` 日志；真实安装场景另做受控授权测试。本轮仅新增模拟安全失败路径测试与审计文档，若有问题可独立回滚，不需要改写已有 Core Install 提交。


## DIFF-080：Core Install 共享契约与 OS 安装机制迁移（2026-11-13）

- **固定基线及目标**：`ref@59724e85c`；迁移 Core 二进制复制、必要时提权、取消后的暂存租约及可选伴随目录发布。风险 L2（生产更新路径/权限边界），非目标是重新设计生命周期、更新归档格式、启用 Meow 功能或修改任何真实用户文件。未修改 ref / runtime 子模块。
- **直接源码对应**：`ref/backend/nyanpasu-contracts/src/core_install.rs` → `backend/chimera-contracts/src/core_install.rs`（仅命名空间品牌映射）；`ref/backend/nyanpasu-os/src/core_install.rs` → `backend/chimera-os/src/core_install.rs`（生产实现保留上游复制/提权/暂存租约/伴随目录替换机制；测试夹具从 Meow 名称改为通用合成资产）；`ref/backend/nyanpasu-contracts/src/profiles/error.rs::ErrorPath` → `backend/chimera-contracts/src/profiles/error.rs::ErrorPath`。当前只提取上游 ErrorPath 的 path 基础转换/显示/序列化，不搬入其余尚无对应 Profile 契约的错误类型；这是完整 profiles 迁移前的显式范围限制。
- **生产接线**：`backend/tauri/src/client/core_lifecycle/ports.rs` 不再定义私有 `BinaryInstaller`，直接重导出上游共享 Port；`workflow.rs::replace_binary` 将现有 `PreparedCoreBinary` 作为 `CoreBinaryInstallRequest` 传给共享 Port，保留原状态检查、进度回调及重启规则；`backend/tauri/src/client/mod.rs` 在生产组合处创建 `chimera_os::core_install::FsBinaryInstaller` 并注入 `InteractiveElevationConsent`。删除 `backend/tauri/src/client/core_lifecycle/adapters.rs` 的独立复制/提权实现。测试使用 RefuseElevation，不进行管理员授权。
- **刻意保留的 Chimera 差异**：当前更新器只提供独立核心二进制，不分发 Meow 的 skills 伴随目录，因此生命周期构造请求时始终 `companion: None`。共享 Port 和 OS 模块仍保留上游可选 `CoreBinaryCompanionPaths` 的定义与事务式发布机制，以便维持源码结构、测试恢复安全，但不会在真实 Chimera 更新过程中触发它。`PreparedCoreBinary` 额外持有旧 UI 的进度回调，这一状态留在 Tauri 表示层，不替代共享安装 Port；其他既有 Core 生命周期判定均未调整。
- **测试证据**：`cargo check -p chimera --all-features --tests` 已通过；OS Core Install 测试 **8/8**（普通复制不提权、拒绝提权不启动命令、取消保留暂存文件、目录交换成功及故障回滚/符号链接拒绝）；`chimera-contracts --lib` **5/5**（包括 ErrorPath JSON 序列化）；Tauri Core Lifecycle **38/38**（新增临时目录实际安装+拒绝授权+重启流程贯通用例）。格式与 Git hooks 以最终提交执行结果为准。未验证 Windows/Linux 构建、实际提权对话框、真实更新包、桌面 E2E 和远端 CI；不将这些条件标为通过。
- **回滚/待解决**：无用户数据迁移；回滚本次契约和 OS 接线提交可恢复原有 Tauri 安装路径。尚需按上游继续完整迁移 Profile 错误体系，并在 Windows/Linux 实际构建和真实更新流程中验证权限/打包行为；不应提前扩大 `companion` 语义。


## DIFF-079：Core Version 读取链路与 Lifecycle 已停止状态测试修正（2026-11-12）

- **目标与非目标**：以只读固定 `ref@59724e85c` 为源，将 core 版本读取改为上游类型化 Port / OS Sidecar Reader / Tauri 当前可执行文件来源；保留 Chimera Client、旧 UI 既有显示格式。同时独立修正此前两项 Core Lifecycle 测试夹具的不一致。暂不迁移 Core Install（该源码依赖尚未迁入的 `profiles::error::ErrorPath`，并含 Chimera 不支持的 Meow companion 语义），不触发实际服务安装、系统代理、网络配置。
- **上游源码映射**：`ref/backend/nyanpasu-contracts/src/core_version.rs` → `backend/chimera-contracts/src/core_version.rs`（仅 crate 名及 Meow→ChimeraClient 测例）；`ref/backend/nyanpasu-os/src/core_version.rs` → `backend/chimera-os/src/core_version.rs`；`ref/backend/tauri/src/adapters/core_version.rs` → `backend/tauri/src/adapters/core_version.rs`。`backend/chimera-config/src/application/clash_core.rs` 增加上游同名 `binary_name()`，补足 ChimeraClient/Alpha 二进制名称。旧版 `get_core_version` IPC 仍由 `utils/resolve.rs` 提供，但改为调用共享 `SidecarCoreVersionReader`，而非保留独立的 `tauri_plugin_shell` 版本解析器。
- **必须保留的差异与风险（L2）**：现有 ChimeraClient/Alpha 的 CLI 版本标志是 `-V`，与上游非 ClashRs 核心使用 `-v` 的默认分支不同，因此 OS reader 为这两个变体保留 `-V`；`ClashRs`/`ChimeraClient` 的旧 UI `v` 前缀仅在显示边界 `format_core_version` 保留，核心返回原始版本 token。版本读取通过运行同目录下的 sidecar 程序完成，不能将纯 mock 测试等同于真实打包/重启测试。
- **Lifecycle 用例溯源**：`workflow.rs::replace_binary` 把 `CoreState::Stopped` 且 `state_changed_at=0` 定义为状态未知，并保守执行 Stop；之前两项宣称“已经停止”的测试使用 `RecordingCore`，恰好返回 `state_changed_at=0`，所以实际额外出现 `stop-start/stop-end`。用既有 `CoreUpdateRecordingCore` 明确生成 `state_changed_at=1` 的已确认停止状态，不改变生产停机逻辑、不修改期望事件、不移除任何测试；`unavailable_service_status_is_not_treated_as_proven_stopped` 仍覆盖时间戳 0 的失败关闭语义。
- **验证与未覆盖范围**：Core Lifecycle 定向测试改动后 **37/37** 通过，独立测试提交 `6d3dea47` 的 Git hooks（Rustfmt/Clippy）也通过；`chimera-config` 测试 **2/2**，Core Version contracts **1/1**，OS Version reader **3/3**（包含两种 ChimeraClient 临时 sidecar shell 脚本的真实进程启动与 `-V` 校验），Tauri 旧 UI 显示格式回归 **1/1**；Tauri `--all-features --tests` 编译通过。Core Version 对齐提交 `7841abf6` 已通过 Git hooks（Rustfmt、Clippy、Prettier），并完成本地 GPG 签名；GitHub Verified 尚未验证。Windows/Linux 跨平台打包、真实发行 sidecar、桌面 UI E2E、全量 CI 未验证；不宣称 Core Install 已迁移。
- **回滚**：本批只改源代码、测试和文档，无数据迁移；可逐个回滚测试夹具与 Core Version 对齐提交，不触及固定 `ref` / runtime gitlink。


## DIFF-078：Service ref 对象/路径所有权纠偏（2026-11-12）

- **原因与范围**：修正 DIFF-077 删除上游 `OsServiceProcessControl`、在 Tauri 重建 OS Service Port 的偏差。以 `ref@59724e85c` 为固定基线，不更新子模块、不改 CoreActor 状态机、不调用真实 Service 安装/提权。L2：影响生产 Service 的依赖注入。
- **严格映射**：从 `ref/backend/nyanpasu-os/src/service.rs` 恢复 `OsServiceProcessControl { service_binary, paths, consent }` 和原本的 `ServiceProcessControl` 实现，`chimera_paths::PathResolver` 在 OS 层被持有。`backend/tauri/src/lib.rs` 将已有的统一 resolver 交给 `setup.rs`；`setup.rs` 按 `ref/backend/tauri/src/setup.rs` 的结构组合 `chimera_os::service::service_binary`、`OsServiceProcessControl::new` 并注入 CoreFacade，Service Actor 不再自己构造另一套 Tauri 特权命令实现。删除自创 `ResolveLegacyServiceDirs` 错误和 `install_service_with_args` 辅助函数，恢复上游安装目录解析与授权顺序。
- **必要的 Chimera 差异**：保留 `chimera-service` 二进制名，实际 Chimera CLI 的 `update` 只能传 `update`（不能传 ref 上游 Unix 专属 `--user`/`--nyanpasu-data-dir`），保留原生 `restart_daemon` 接口和 `status` 的 `kill_on_drop` 超时保护。Tauri 中尚未迁移的只读 Service 状态查询与安装提示预览仍使用遗留路径 API，**不宣称全部 Service 读取路径与上游一致**。
- **旧健康监测兼容性**：原有 Stop / Uninstall 成功后必须通知 `ipc::stop_health_check`，防止旧轮次恢复发布。新 `HealthMonitorStopAdapter` 只转发 `OsServiceProcessControl` 的 Service 命令，在两个操作成功后通知旧健康监测；自身不启动任何特权命令、不重新发现路径，也不会在失败时错误地关闭监测。后续随 Legacy IPC 健康所有权收敛可删除这一薄适配层。
- **实际验证**：Tauri `cargo check -p chimera --all-features --tests` 通过；`chimera-os --lib` **12/12**（包括注入路径、安装目录不存在和拒绝授权）；`chimera-core --lib actor_v2::service` **23/23**；Tauri `core::actor_v2::facade::tests` **7/7**；Tauri `core::service::control::tests` **1/1**。均无实际 OS Service 操作。Windows/Linux 编译、权限交互、真实桌面 E2E、完整 CI、独立复审仍未进行；上一轮发现的 Core Lifecycle 两项二进制更新失败并未解决。
- **回滚**：无数据迁移。若需要，可回滚这次 Service 纠偏提交，恢复到 `d2f91f9c` 的工作分支状态；不得把这一批未经跨平台验证的结果写作全部 ref 严格对齐完成。


## DIFF-077：Service OS process / shared contract（2026-11-12，生产接线）

- **目标**：将固定上游 `ref@59724e85c` 的 Service OS 进程调用、授权、安装参数与状态查询从 Tauri 拆到 `backend/chimera-os/src/service.rs`，并让独立 `chimera-core` 的 Service Actor 直接使用 `chimera-contracts::service`，不再保留第二套私有命令/错误/port 定义。
- **来源和结构**：复制适配 `ref/backend/nyanpasu-os/src/service.rs`、`ref/backend/nyanpasu-os/src/elevation/macos.rs` 到 `chimera-os`；`backend/chimera-core/src/actor_v2/service_process.rs` 改为从共享 Service contract 重导出命令/错误/port，只保留 Chimera IPC `StatusInfo` 解析和已有测试；Tauri `core/service/control.rs` 改为薄 shell bridge，复用新 OS `status`、`install_service_with_args`、`start/stop/uninstall/update/restart_service`，其健康监视停止仍在 Tauri 操作成功后执行。Tauri `ipc.rs` 继续通过兼容预览函数获得安装参数，无二套特权命令执行实现。
- **有理由保留的 Chimera 差异**：服务可执行文件是 `chimera-service` 而非 `nyanpasu-service`；安装参数继续使用已有 `--nyanpasu-*` 兼容选项；实际 `chimera_service` 的 `update` 子命令不接受上游追加的 `--user`/`--nyanpasu-data-dir`，因此仅传 `update` 并有单测锁定；共享 port 增加 Chimera 原有的原生 `restart_daemon()`，不可改成 stop/start 两个命令；现存 Tauri shell 目录来源尚为 `utils::dirs`，共享契约暂保留 `ResolveLegacyServiceDirs` 结构化故障分支。`status` 保留原 5 秒超时和 `kill_on_drop`，阻止超时探测留下后台子进程。
- **授权与安全性**：新 OS 安装辅助函数延迟求值 Tauri 路径及安装参数，直到 consent policy 允许后才进入操作系统授权机制；拒绝策略时既不构造安装辅助信息也不执行 helper，使用 fake runner/计数器回归。此次仅编译/模拟测试，不请求管理员授权，不执行真实 Service 安装/启动、系统代理修改或实际系统 DNS flush。未改变核心 Service 状态机、TUN、用户配置、runtime submodule 或 ref 指针。
- **验证**：`cargo check --manifest-path backend/Cargo.toml -p chimera --all-features --tests` 通过；`cargo test --manifest-path backend/Cargo.toml -p chimera-core --lib actor_v2::service` **23/23**；`cargo test --manifest-path backend/Cargo.toml -p chimera --lib core::actor_v2::facade::tests` **7/7**；`cargo test --manifest-path backend/Cargo.toml -p chimera-os --lib` 最终 **11/11**（包含授权拒绝前置、Chimera update CLI 参数、Service 二进制命名和既有 OS 测试；未执行 macOS sudo 实际提权）；定向 Rustfmt 和 `git diff --check` 通过，两个签名提交的 Git hooks（Rustfmt、Clippy、Prettier）均通过。Mac 以外的平台编译、Service 真正提权/安装、桌面 E2E、全量 CI 未运行。上一批发现的两项 Core Lifecycle 二进制更新测试失败与 Service OS 提取不同，仍需独立诊断。
- **剩余收敛**：本批服务 OS 命令统一，但 CoreActor V2 完整 ServiceActor/Host 状态所有权仍未完全迁入独立 core；Tauri 目录来源需在后续基线迁移中改为同一实例 `PathResolver`，并按功能门限验证 Windows SessionEnding、权限提示和 macOS Service sidecar。下一批 Core Install / Core Version，不应混用 ref 的不兼容更新命令参数。

## DIFF-076：System Proxy / Autostart / PAC OS 适配拆分（2026-11-12）

- **基线与范围**：主仓 `codex/ref-os-dns-20261112@e03423b6`，只读固定 `ref@59724e85c34abb6995c22d101c876cc06dc16576`。完成 OS 层的 System Proxy、Autostart 和 PAC 相关源码复制与真实生产装配；不更改 `ref/`、`backend/chimera-runtime`、用户代理设置、真实网络或系统登录项。
- **上游路径**：`ref/backend/nyanpasu-contracts/src/system_proxy.rs` → `backend/chimera-contracts/src/system_proxy.rs`，保留 `OsProxyPort`、`AutoLaunchPort`、`PacPort`、配置数据和类型化错误合同；`ref/backend/nyanpasu-os/src/{system_proxy.rs,autostart.rs,pac.rs,blocking.rs}` → 新 `backend/chimera-os/src/` 对应原名。仅实施 `nyanpasu_contracts` → `chimera_contracts` 的 crate 映射和 manifest 依赖，`verge-dev` feature 由 Tauri 传递到 OS crate，保留禁止开发构建关闭正式自启动的行为。
- **生产接线**：`backend/tauri/src/setup.rs` 继续使用 Tauri shell 的 `current_exe()` 和 AppImage 路径，构建新 `chimera_os::autostart::AutoLaunchBackend`，并注入新 `chimera_os::{system_proxy::SysproxyOsProxy,pac::HttpPacBackend}` 到原 `SystemProxyActor`。删除 `backend/tauri/src/client/system_proxy/{ports.rs,adapters.rs}`，该 Actor 的协调状态、写入序列、守护、PAC 回退、关闭恢复逻辑不改。更新 Application、Effects、Core Lifecycle、Agent 中受影响的 port 类型引用，避免重建平行实现；Main/Legacy/Agent 共享原 Actor/Client 的写入入口。
- **风险与剩余差异**：L2，系统代理/登录项和 PAC 均可造成宿主副作用，未运行任何实际修改。旧 `backend/tauri/src/core/sysopt.rs` 仍包含直接 `sysproxy` 写入的历史类，但本轮静态搜索未发现 `Sysopt` 的生产调用点；独立的 IPC、Agent、Profile File 与 HTTP 工具仍有直接的 OS proxy **只读**访问，尚未对齐 `ref/backend/nyanpasu-os/src/system_proxy_source.rs`。后续需沿真实消费者 port 边界收敛读源并独立检查旧 Sysopt 可删除性，不在本轮擅自删除其余代码。System Proxy Actor 自身仍在 Tauri，而非独立 core；属于后续应用所有权迁移，不宣称已经完成核心/GUI 拆分。
- **实际验证**：`cargo check --manifest-path backend/Cargo.toml -p chimera-os -p chimera-contracts --lib`、`cargo check --manifest-path backend/Cargo.toml -p chimera --all-features --tests` 均通过；`cargo test --manifest-path backend/Cargo.toml -p chimera-os --lib` **5/5**（含 Autostart 路径、DNS 授权拒绝、blocking panic）、`cargo test --manifest-path backend/Cargo.toml -p chimera --lib client::system_proxy::tests` **41/41**、`cargo test --manifest-path backend/Cargo.toml -p chimera --lib client::effects::executor::tests` **4/4**。这些均使用假 port，不更改宿主代理、PAC、登录项或网络。
- **额外回归与缺口**：`cargo test --manifest-path backend/Cargo.toml -p chimera --lib client::core_lifecycle::` **35/37**，两项二进制更新测试稳定失败：`binary_replacement_of_a_stopped_selected_core_installs_then_restarts` 和 `binary_install_failure_does_not_stop_or_restart_a_stopped_core`，实际在 `install` 前多出 `stop-start`/`stop-end`。本轮在 Core Lifecycle 文件只更改 Proxy 类型路径，未改二进制更新逻辑；不能据此确认是新迁移回归或证明既有基线通过，单独列为独立排查项。Windows/Linux 目标编译、真实 PAC 下载/系统代理应用、桌面重启和 Service 端到端均未执行。
- **回滚与下一步**：恢复这批 contracts/OS 文件和 Tauri 调用方类型路径，再恢复旧 Tauri `ports.rs`/`adapters.rs`；无持久化迁移或本机系统操作。下一步迁移 `system_proxy_source` 读源及 Service/Core Install 的 port 和 OS 机制，再按上游所有权将 Actor/Client 从 Tauri 下沉到 core；对两项 Core Lifecycle 用例另开小范围问题修复。

## DIFF-075：OS Contracts / System DNS 注入路径（2026-11-12，部分迁移）

- **目标**：把 DNS flush 从 Tauri 内部的匿名 `anyhow` OS 实现迁移为上游同形的类型化 port、Shell elevation policy 和独立 OS adapter；依然由现有 `ChimeraClient`、Main/Legacy/Agent 共享应用入口调用，不新增第二套系统 DNS 实现。
- **基线**：Chimera `master@7f08640f`、gitlink 与检出的只读 `ref@59724e85c34abb6995c22d101c876cc06dc16576`，开始时两者工作树干净。2026-11-12 使用 `git ls-remote` 观察到上游 `main@b35722b588e23a416186de96155c47c76b1dfd3b`，但本批未 fetch、切换、修改 ref 或更新 gitlink；这是对固定 ref 的源码迁移，不冒称已对齐实时最新 main。
- **上游路径映射**：`ref/backend/nyanpasu-contracts/src/{service,elevation,system_dns}.rs` → `backend/chimera-contracts/src/` 同名文件，保留上游枚举、Snafu 错误结构、trait 与序列化键，只有 `nyanpasu_paths` → `chimera_paths` 的 crate 标识映射；上游 OS `elevation/{mod,runner}.rs`、`system_dns.rs` → 新 `backend/chimera-os/src/`；`ref/backend/tauri/src/adapters/elevation.rs` → `backend/tauri/src/adapters/elevation.rs`。`backend/tauri/src/setup.rs` 注入 OS DNS cache 和 shell consent；`backend/tauri/src/client/system_dns.rs` 只保留调用共享 port 的客户端方法以及测试 no-op，移除原先直接调用 `runas`、`osascript`、Linux DNS 命令的第二份生产实现。`chimera-os` 当前**只实现 DNS/elevation 相关源文件**，不是上游完整 OS crate。
- **接口 / 影响**：`ClientSetupArgs` 改用 `chimera_contracts::system_dns::SystemDnsCache`，OS 层返回上游 `SystemDnsError`，Tauri 现有客户端 RPC 入口仍返回 `anyhow::Result`；因此类型化 OS 失败的来源链/序列化合同已迁移，但客户端及统一 RPC 的结构化错误转换仍属后续工作。交互式桌面 consent 只允许进入现有操作系统授权提示，不自行授予权限；拒绝时在 OS 命令启动之前失败。未改用户配置、核心、服务 IPC 或 TUN。
- **必要差异**：固定 ref 的 DNS port 在 Linux 返回 Unsupported，但 Chimera 既有实现尝试 `resolvectl`、`systemd-resolve`、`nscd`；这些候选及尝试顺序保留在新 `chimera-os`（使用可替换命令 runner），防止已有 Linux 功能退化，并有 Linux 专用 fake-runner 回归。Mac 本机无法执行该 Linux 专用测试。OS runner 中上游供其他 OS 模块使用的 `runas` 接口当前尚未被本批 DNS 代码调用；后续 Service/Core install 模块迁入后重新复核。
- **风险 / 非目标**：L2（OS 权限提示与平台机制）。非目标：安装/启动 Service、系统网络和 DNS 实际修改、更新 ref、全量 CoreActor/OS crate/Unified RPC 改造、桌面 E2E、发行或推送。若后续要修改宿主网络、安装 Service 或实跑管理员授权，需独立授权及隔离环境。本批只执行被注入的 fake runner/纯单测，不主动请求系统权限。
- **验收标准与测试计划**：契约 wire shape、source chain、DNS 授权拒绝不能调用 OS runner，原 macOS DNS 命令形状，Linux 候选失败回退；`cargo check` 新 packages 和 Tauri、`cargo test` 新 packages、改动包 Rustfmt 与 `git diff --check`；Tauri DNS 客户端用例与真实桌面授权/重启作为后续验收。主/Legacy/Agent 均保留同一客户端入口，不新增 UI。
- **本次验证**：`cargo check --manifest-path backend/Cargo.toml -p chimera-os -p chimera-contracts --lib` 通过；`cargo check --manifest-path backend/Cargo.toml -p chimera --lib` 通过（仓库既有较多 warnings）；`cargo test --manifest-path backend/Cargo.toml -p chimera-contracts --lib` **3/3**、`cargo test --manifest-path backend/Cargo.toml -p chimera-os --lib` **2/2** 均通过（macOS ARM64）；定向 `cargo fmt --check` 与 `git diff --check` 通过。在修正测试模块旧 `SystemDnsCache` 导入导致的 `E0603` 错误后，`cargo check --manifest-path backend/Cargo.toml -p chimera --all-features --tests` 通过，预提交 `pnpm lint:rustfmt` 与全仓库 `pnpm lint:clippy` 通过（有仓库既存 warnings）。Tauri 单测的测试函数、Linux/Windows 测试、跨平台 CI、真实授权提示和桌面 E2E 均未运行，不得视为通过。初次全仓 Clippy 产生缓存导致磁盘余量一度仅约 118 MiB；定向删除当前会话及既有的可再生成 Cargo build/incremental 目录后空间恢复到约 3.1 GiB，没有清理源码、现有可执行文件、用户配置或 Git stash。
- **回滚**：移除本批新增的 `chimera-os` 和 contracts OS 文件、撤销 Cargo workspace/lockfile/Tauri 组装增量，并恢复 `client/system_dns.rs` 既有 OS 实现；无持久化迁移，也没有实际系统代理、DNS 或网络状态需要回滚。
- **下一步**：完成独立的 Tauri DNS 客户端测试和 Linux 专用测试；在固定 ref 基线上按 OS port → OS mechanism → Tauri adapter 顺序迁移 `system_proxy`、`service`、`core_install`，然后把 CoreActor 生产所有权搬进独立 core。同步更新上游 ref 应作为独立、显式的固定基线变更。

## 2026-11-11：Linux 透明代理集成说明

- 本节移植自 `4b632f8f` 并保留原分支的测试证据；其中 `ref@231e3866` 是来源分支当时的参考基线，不代表当前 master 的 gitlink 已同步。
- 当前主仓仍固定 `ref@59724e85`，本地 `ref` 工作树未改变；真实 Linux 网络/桌面效果仍需单独验收。

## DIFF-072：Linux 透明代理捕获（代码迁移完成；Linux/桌面验收待补）

- **目标**：按 pinned ref 的完整调用链迁移 Mihomo REDIR/TPROXY 配置和 Linux 本地/网关流量捕获，由现有特权 Service 独占管理 nftables 与策略路由；主界面展示可用性、配置和观察到的 Service 状态。
- **非目标**：不修改 `ref/`；不执行安装/重启 Service、权限提升、宿主 nftables/路由/系统代理/TUN 修改或真实网络流量测试；不替换现有 system proxy/TUN 子系统，不删除 legacy UI，不建立 Agent 平行业务实现，不发布 Service、标签或默认分支；按后续明确授权仅推送任务分支。
- **基线**：Chimera 根仓当前 `codex/app-update-ref-alignment` HEAD `8ee098904d5a3d1f1f87756faeba013762ff604f`，工作树保留此前未提交 DIFF-071；只读 ref 根 `231e3866cb9ae0390519a22835f6a1e98e484cbf` 干净，嵌套 ref Service `3a79dabc4a1fd4a7d708bafe87fceb240fe24c3b` 干净。本批开始前，Chimera Service gitlink `6a92c2ce5bd0f33d4f414c39c7e625349a510d21` 干净；当前 Service 子仓只含 DIFF-072 未提交修改，ref 未切换或改写。上游功能提交为根 `839e7ed01646eba2e02ac4c8957db923663f17a7`、Service `4665b47ac6a524a273e6ba07438321b58e12dc22`；不快进或重置 ref。
- **映射**：`ref/backend/nyanpasu-config/src/clash/config/{clash_strategy/port.rs,mod.rs}` 与 `runtime/executor/{mod.rs,plan.rs,tests/*}` → `backend/chimera-config/src/clash/config/`、`runtime/executor/`；`ref/backend/nyanpasu-core/src/client/{ports.rs,transparent_proxy.rs,effects/{plan.rs,actor.rs,executor.rs,status.rs,tests.rs},application_workflow/impact.rs}` → `backend/tauri/src/client/{core_lifecycle,effects,application_workflow}/`；`ref/backend/nyanpasu-profile-transform/src/runtime_builder.rs` → `backend/tauri/src/enhance/runtime_builder.rs`；`ref/backend/tauri/src/{ipc.rs,specta_export.rs,desktop/effects/*}` → 对应 Chimera Tauri 路径。Service 映射：`ref/backend/nyanpasu-runtime/crates/nyanpasu-service-runtime/src/server/{transparent_proxy.rs,routing/transparent_proxy.rs,mod.rs,manager_bridge.rs}` → 当前重组后的 `backend/chimera-runtime/chimera_service/src/server/`；`ref/.../nyanpasu_ipc/src/{api/{contract.rs,network/*},client/shortcuts.rs,server/mod.rs,utils/os.rs}` → `backend/chimera-runtime/chimera_ipc/src/`。前端 `ref/frontend/nyanpasu/src/pages/(main)/main/settings/clash/network/_modules/transparent-proxy-config.tsx` 及其设置 hook、IPC/query bindings、errors、locales/tests → Chimera 对应 network settings、feature、typed interface 路径；主界面继续调用共享应用 API，legacy UI 与 Agent 不建第二份配置/Service 实现。
- **接口 / 状态所有权**：Clash typed config 增加默认关闭的透明代理策略与可选 REDIR/TPROXY 端口策略；应用 effect 把策略与当前核心 revision 一起交给已授权 Service，Service 以 revision 核对 listener 后仅管理命名空间内的规则，状态由 Service 返回。UI 通过 typed RPC 查询/修改配置和状态。沿用现有 Service socket/SID 授权；本批没有新增桌面 UID 绑定或 ACL 身份模型。其他平台保持不支持。
- **风险与审批触发**：L2（持久化 config、共享 IPC、特权 Service 与 Linux 防火墙/策略路由逻辑）。沿用当前 socket/SID 授权边界，不引入 ACL/UID 变更；fake-runner 单元测试不会触碰本机 ACL 或网络。宿主网络/权限操作、Service 安装/发布、远程 git 操作仍需各自明确授权，本批不做。
- **验收标准**：旧 config 缺少新字段时保持透明代理关闭且既有字段不丢；非法平台/核心、TUN 冲突、缺失或零监听端口、无效/重复接口被拒绝；runtime builder 只在策略启用时写入对应 Mihomo listener；Core effect 串行下发当前 revision，过期 revision 与 core handoff 会触发清理/状态错误；Service fake-runner 覆盖 nftables 规则、策略路由、替换补偿、失败清理保留、重启恢复和 shutdown fence；IPC 仅注册在已有受限 Service 传输；主界面只在 Linux/Tauri/Mihomo 或 Mihomo Alpha/Service 可用时提供动作，legacy UI/Agent 继续经同一共享 API 且原流程可编译。
- **验证计划**：按 ref 对照的 config、runtime-builder、Core actor/effects、Service IPC/rule planner 定向测试；生成并校验 typed bindings；运行根 Cargo check/相关测试、Service workspace check/相关测试、`pnpm typecheck`、前端边界 lint、scoped Rust/Prettier/JSON 格式检查及 `git diff --check`。所有 Service 网络命令在单元测试中使用 fake runner；不运行需要 Linux 网络命名空间/iptables/nftables 的真实系统测试。若当前 2.6 GiB 剩余磁盘无法支持编译，先检查已有产物使用情况，不删除用户构建缓存，准确记录未执行项。
- **回滚**：仅回退本 DIFF-072 显式修改的根仓路径与 `backend/chimera-runtime` 子仓改动；不 reset 子仓或覆盖既有文件。配置默认 disabled，不做自动用户数据改写；若曾手动保存启用值，回退旧版后将其保留为未知 YAML 的兼容风险需在发版前单独处理。
- **审查**：完成后由独立新上下文审查实际根仓与嵌套 Service diff、ref 映射、失败/恢复、socket ACL、宿主副作用与测试证据；桌面/真实 Linux 网络效果未执行时标记 partial。
### 实施与验收记录（2026-11-10）

- **基线与来源**：主 ref `231e3866cb9ae0390519a22835f6a1e98e484cbf` 只读且干净。该功能对应上游根变更 `839e7ed01646eba2e02ac4c8957db923663f17a7`，Service gitlink `4665b47ac6a524a273e6ba07438321b58e12dc22`；本机 ref Service checkout 是 `3a79dabc4a1fd4a7d708bafe87fceb240fe24c3b`，未切换/改写，迁移对照使用已存在的上游 Service commit 对象。实施分支 `codex/app-update-ref-alignment`，原有 DIFF-071 未提交改动均保留。
- **ref 路径/符号映射**：`ref/backend/nyanpasu-config/src/clash/config/{clash_strategy/port.rs,mod.rs}`、`runtime/executor/{mod.rs,plan.rs,tests/*}` → `backend/chimera-config/src/clash/config/`、`runtime/executor/`；`ref/backend/nyanpasu-core/src/client/{ports.rs,transparent_proxy.rs,effects/{plan.rs,actor.rs,executor.rs,status.rs,tests.rs},application_workflow/impact.rs}` → `backend/tauri/src/client/{core_lifecycle,effects,application_workflow}/`；`ref/backend/nyanpasu-profile-transform/src/runtime_builder.rs` → `backend/tauri/src/enhance/runtime_builder.rs`；Service `server/{transparent_proxy.rs,routing/transparent_proxy.rs,mod.rs,manager_bridge.rs}` → `backend/chimera-runtime/chimera_service/src/server/`，IPC network/client shortcuts → `backend/chimera-runtime/chimera_ipc/src/`；Tauri command/generated binding 与前端设置、optional-port、query/settings hooks、locales → 对应 Chimera typed IPC 和 Clash settings 路径。保留上游非品牌配置/状态合同、错误恢复和接口名。
- **架构差异与状态边界**：Chimera Service 使用 Axum 路由而 ref 使用 contract/op 注册，因此把相同 typed request/response 接到现有 Axum state/router；沿用既有 socket/SID 授权，不新增 ACL/UID 身份边界。旧 Service 缺少新路由时，关闭策略映射为 unsupported，启用策略仍报错。Clash typed config 默认关闭、无数据迁移；core lifecycle/effect 按当前 core revision 下发，Service 对自己的命名规则和策略路由负责。主 UI 使用共享 typed API，legacy UI 与 Agent 没有第二套状态实现。Service 启动时为 Mihomo controller socket 查找 `c"nyanpasu"` group，而安装/IPC 路径的 `NYANPASU_USER_GROUP` 是 `"chimera"`；此既有差异不属于本批透明代理实现，Linux 安装和授权效果未验证，另行追踪。
- **代码完成项**：配置校验覆盖 Linux Mihomo/Mihomo Alpha、TUN 冲突、端口/接口约束；端口解析探测 TCP 与 UDP；runtime builder 生成 REDIR/TPROXY listener 和 routing mark；application patch 在持久化前校验；新 core runtime 会在端口不变时重新 reconcile；typed IPC 与 status/query、主设置卡片及 optional-port 编辑器已接入。Service fake-runner 覆盖规则计划、授权标记、失败补偿/保留、替换与关闭；真实系统命令未运行。
- **已执行并通过**：`cargo check --locked --manifest-path backend/Cargo.toml -p chimera-config --lib`、`cargo check --locked --manifest-path backend/Cargo.toml -p chimera --lib`；配置稀疏 patch/旧字段 **5/5**、双协议端口扫描 **1/1**、Tauri effects **45/45**、application tests **12/12**、macOS 可运行的 runtime builder tests **6/6**（Linux-only 用例未包含）、旧 Service 缺路由兼容 **1/1**、Service transparent-proxy 与 Axum route 测试 **12/12**、Specta binding 生成比较 **1/1**；`pnpm typecheck`、`pnpm lint:frontend-boundaries` 通过。根仓及 runtime 子仓已修改 Rust 源的 scoped `rustfmt --check`、本批涉及 TS/JSON/Markdown 的 Prettier 检查，以及两个仓库的 `git diff --check` 均通过。
- **未执行与 partial 边界**：本机 macOS ARM64；Linux-only runtime builder 用例虽保留在 Linux target，但本机未执行。没有启动真实 Service、调用 nft/ip、改动系统代理/TUN/ACL/路由或发送流量；未运行 Linux namespace ignored test、完整 Rust workspace/Clippy 或可见桌面 E2E。代码及隔离测试完成，不代表 Linux 网络效果或桌面用户流程验收通过。
- **同工作树的前端边界修正**：frontend boundary lint 同时发现本批透明代理组件和既有 updater preference 直接访问 Tauri/generated commands；透明代理用 `@chimera/interface` 的 `isTauri`，updater release-channel query/mutation 移至共享 `query-bindings`，保留现有设置语义。此小修正解决已存在 DIFF-071 UI 的共享 IPC 边界问题，不改 updater 核心流程。
- **失败记录**：nested patch 初次漏加 serde default、macOS 误执行 Linux-only builder 用例、effect 顺序测试漏列新 effect 均已修正。独立复核发现并复现 pinned ref 同样存在的 Mihomo Alpha support mismatch，Service 三处统一为 stable/alpha 共用判定并加入回归；过程记录于 LESSONS `LESSON-20261110-001` 至 `004`。Linux 目标行为仍待 Linux 环境实跑。
- **回滚**：只回退本批在 `backend/chimera-config`、`backend/tauri`、runtime 子仓、透明代理设置/query binding/locales/生成 bindings 和本节的差异；保留此前 DIFF-071 改动。默认关闭且无 schema 迁移。不要 reset 整个工作树或 runtime 子仓。未执行 Service 安装或宿主网络变更。
- **审查**：fresh-context reviewer 确认 Service 与上游实现主体一致，最初发现并复现 stable/alpha support mismatch P2；本批按项目规则修复上游缺陷，reviewer 随后确认启用、应用后复核、stale 判定三处均共用正确 helper，stable/alpha 均接受且其他 core 仍拒绝。修复后本机透明代理/路由隔离测试 **12/12** 通过；reviewer 未自行重跑测试。无其他已确认 P0–P2；真实 Linux/桌面验收未完成。

## DIFF-071：应用更新链路端到端对齐（代码批次完成，桌面验收未完成）

- **目标**：按当前 pinned `ref` `231e3866cb9ae0390519a22835f6a1e98e484cbf` 对齐应用更新的 shared client、下载/验证/取消/安装状态、Tauri backend、bundle 元数据、更新源/渠道设置与前端调用；复用当前 Chimera 主界面和 legacy UI 的产品流程，不保留平行的业务更新实现。
- **非目标**：不修改 `ref/`；不运行真实的更新包安装/自升级，不触发 Release/tag/sign/publish，不执行宿主网络、Service/TUN 或系统代理操作；不迁移 Clash/Service 核心 sidecar 更新（`core::updater::UpdaterManager`）。
- **基线与当前差异**：Chimera `master` `0db9f364e3bab776848b09bae859561200bb8aa6`；本地 `ref` 与嵌套子模块干净，根 SHA `231e3866cb9ae0390519a22835f6a1e98e484cbf`。Chimera 当前注册默认 updater plugin，`ipc::check_update` 直接调 Tauri updater；新 `bundle`/`UpdateSource` 仅部分接入，更新来源未持久化，当前 `app_update` core 仅暴露 endpoints。ref 使用 core `client/app_update` actor、Tauri `adapters/app_update`、bundle 配置和持久化更新设置。
- **ref 路径/符号 → Chimera**：`ref/backend/nyanpasu-core/src/client/app_update/{mod.rs,progress.rs,changelog.rs,endpoints.rs}` 的更新 actor、`AppUpdateClient`、进度/错误状态、changelog 和 feed 生成 → `backend/chimera-core/src/client/app_update/{mod.rs,changelog.rs,endpoints.rs}`；`ref/backend/nyanpasu-core/src/client/application.rs::ApplicationClient::new` 的安装渠道初始化，以及 `ref/backend/nyanpasu-core/src/state/application.rs::ApplicationActor::validate_channel` 的 Nightly 保护 → Chimera `backend/tauri/src/client/application.rs::ApplicationClient::new` 与 `backend/tauri/src/state/application.rs::ApplicationActor::{validate_release_channel,prepare_replace}`。`ref/backend/tauri/src/adapters/app_update.rs::{TauriAppUpdateBackend,TauriAppUpdateEventSink}` 与 `ChangelogHttp` → Chimera 对应 adapter、`backend/chimera-contracts/src/net.rs`；`ref/backend/tauri/src/{bundle.rs,lib.rs,setup.rs}` 的 bundle metadata、updater builder、初始化和 IPC 注册 → 对应 Chimera 文件及 `backend/tauri/src/ipc.rs`。更新源 URL 解析限制为发行资产路径下的 `MFSGA/Chimera`，依据当前 `tauri.conf.json` 与 `scripts/updater-manifest.ts`，没有照搬 ref 中的旧 `libchimera/clash-chimera` 仓库路径。`ref/frontend/nyanpasu/src/components/providers/nyanpasu-update-provider.tsx` 的共享 provider/state、更新设置和 `AppUpdateChangelog` → Chimera `components/providers/chimera-update-provider.tsx`、共享更新偏好与 changelog 组件；主 About 对话框和 legacy updater adapter 都消费同一 provider/typed IPC。仅映射实际产品名/发行仓库；保留现有 Chimera Client/Service sidecar 更新实现。
- **受影响接口**：typed application settings 增加带默认值的更新源与 release-channel preference；共享 app-update snapshot/operation/event API 接入 Tauri IPC 和生成绑定；主 UI 与 legacy adapter 继续消费同一 API。Agent 不增加平行 updater；若已有调用点则复用同一 client。
- **最高风险 / 授权触发**：L3（应用更新渠道、持久化设置和安装状态）。用户明确要求按最新 ref 迁移本批，并接受此前提出的应用更新端到端范围；该授权不包含真实安装、自升级、发行签名、tag 或发布操作。
- **验收**：旧版设置可读取且缺失新字段时采用兼容默认值；新设置校验、保存、重载和 Nightly/Stable/Beta 规则与 pinned ref 一致；检查、下载进度、签名/验证失败、取消、安装失败和退出恢复有明确状态；main/legacy 共用一份更新实现；相关测试有实际执行入口，生成 binding 由既有工具更新。
- **验证计划**：config/application 更新来源与渠道单测；core app-update actor 状态/并发/失败/取消测试；Tauri adapter、bundle 与 IPC 命令测试；`cargo fmt --check`、locked backend tests、`cargo clippy`、`pnpm typecheck` 与受影响 UI/E2E suite。不会将模拟更新 backend 作为真实签名、桌面安装或重启证据；本次不运行真实自升级。
- **回滚**：回退 DIFF-071 源码和生成 binding；新增设置字段使用 serde 默认值且不删除/改写旧字段，回滚后保留未知配置字段和已有 updater 流程。提交前无真实用户配置或安装产物变更。

### 实施与验收记录（2026-11-09）

- **已迁入**：ref 风格的共享更新 actor/client、更新事件和阶段状态、发布 changelog 请求、Tauri application adapter、typed IPC/生成绑定、更新源与 release channel 配置、默认值/校验、bundle 目标和下载 URL 解析；主界面与 legacy 窗口共用同一个查询和命令，并共用更新偏好及 changelog 展示。Nightly 安装会归一化并持久化其渠道，状态 actor 会拒绝 patch/replace 将 Nightly 改为其他渠道；非 Nightly 安装遇到旧 Nightly 偏好时会持久化回当前安装渠道。若 `application.yaml` 本身是指向外部文件的符号链接，归一化写入会经真实目标路径执行以保留链接节点，并由用例验证。新增可见 UI E2E 场景只覆盖更新源跨窗口同步，渠道场景未加入该 E2E。
- **已知兼容差异**：当前 `scripts/updater.ts` 与 `scripts/updater-nightly.ts` 生成 Stable/Nightly feed，不生成 Beta feed；Beta 设置先尝试 `update-beta`，再回退 Stable feed。等发行脚本产生并验证 Beta manifest 后，删去 Stable fallback 并将 Beta 设为独立发布渠道。Windows 固定 WebView2 使用 `update{channel}-fixed-webview` 独立 feed，清单 platform key 仍为普通 target（目前 fixed feed 只要求 `windows-x86_64`）。最终独立复核发现 ref 把 target 也加上 `-fixed-webview`，与当前发布脚本输出不符；本批已移除 target 后缀，并用实际标准 platform key 夹具回归。SourceForge feed 仅在构建时提供 `CHIMERA_SOURCEFORGE_PROJECT` 且 manifest 含受校验的 SourceForge mirror 元数据时可用；当前根仓库构建/发行配置未设置该变量，因此不能声称该镜像已由发行流程启用。
- **已运行并通过**：`cargo test --quiet --manifest-path backend/Cargo.toml -p chimera-core app_update --lib`（27 项）；`cargo test --quiet --manifest-path backend/Cargo.toml -p chimera-config application::tests`（4 项）；`cargo test --quiet --manifest-path backend/Cargo.toml -p chimera --lib state::application::tests`（Nightly patch/replace 用例加入后 4 项）；`cargo test --quiet --manifest-path backend/Cargo.toml -p chimera --lib client::application::tests`（渠道归一化与 symlink 回归加入后 12 项）；`cargo test --quiet --manifest-path backend/Cargo.toml -p chimera --lib adapters::app_update::tests`（3 项）；`cargo test --quiet --manifest-path backend/Cargo.toml -p chimera --lib bundle::tests`（target 修正后 18 项）；`cargo test --quiet --manifest-path backend/Cargo.toml -p chimera --lib specta_export::tests::typescript_bindings_are_fresh`（1 项）；`cargo check --locked --quiet --manifest-path backend/Cargo.toml -p chimera --lib`；`pnpm typecheck`；`pnpm --filter @chimera/tauri-e2e test:unit`（35 项）；`pnpm --filter @chimera/tauri-e2e exec node --test ./spec-suites.test.ts`（2 项）。`tauri-e2e` 新场景已类型检查，但其跨窗口交互尚未在真实桌面运行。
- **未通过/未执行**：`pnpm e2e:tauri:build` 的前端构建通过，Rust E2E 二进制构建因本机磁盘空间不足而失败；本批生成的 `backend/target/e2e` 产物随后已清理，释放约 2.1 GiB。未启动桌面应用，也未执行桌面 E2E；`tauri-e2e/README.md` 说明该运行会启动真实核心/代理与系统路径，且当前 macOS runner 不具备 host proxy 恢复保证，本次无系统网络变更授权。故不能把 E2E 类型检查或单测表述为窗口间同步已实机验证。未运行完整 backend workspace tests、Clippy、签名资产更新或冷启动/安装/重启流程；需在有足够磁盘空间的隔离桌面 runner 补验。
- **格式检查范围**：本批 Rust 文件的 scoped `rustfmt --check`、涉及的前端 Prettier 检查和 `git diff --check` 均通过。`cargo fmt --check --manifest-path backend/Cargo.toml --all` 被未改动的 `backend/tauri/vendor/tauri-plugin-wdio-webdriver/**` 与 `backend/chimera-paths/src/{unix,windows}.rs` 既存格式差异阻断；未借本批改写这些无关文件。
- **当前审查状态**：已根据独立审查修正发行仓库 URL、固定 WebView feed、changelog 接线、取消阶段按钮和本地化问题；target key 的 ref 缺陷已按本地发布脚本修正。fresh-context 复核发现安装渠道未执行 ref 的 Nightly 归一化与拒绝规则，现已在 actor 及初始化路径补入，并通过 actor/config 测试；独立复核确认修复有效，未发现新的 Critical/High 问题。E2E 记录已收窄为仅覆盖更新源的跨窗口同步。DIFF-071 仍为部分验收，不代表发行准备完成；桌面 E2E、完整 workspace tests、Clippy 和更新安装/重启恢复尚未验证。实施时工作在 `codex/app-update-ref-alignment`，当时尚未提交或推送（本条为历史验收记录）；回滚可整体撤销本节路径映射中列出的 DIFF-071 源码、生成绑定和本节记录。

### 2026-11-11：同步 master 后的批次集成和补充验证

- 原 updater 代码按 shared core/config、Tauri/typed IPC、main/Legacy UI、E2E/文档拆为 4 个独立签名提交；以 `0a6ac8ec` 为主线，丢弃已在主线的旧 ref/Linux 透明代理历史。保留 `ref@59724e85` 与 Service `d56b083`，不升级子模块。
- 新增 `backend/tauri/src/client/mod.rs` 必要初始化接线：Bundle metadata、安装渠道、更新 actor/event/http 与关闭任务生命周期；两个测试客户端保持无真实 updater 副作用，修复三个 CoreLifecycle fixture 的 `ReleaseChannel::Stable` 参数。第一次 backend lib 编译 11 个错误、第一次 Tauri lib test 编译 3 个错误，均修复并复测。
- 实测：backend `cargo check --locked --offline` 成功；Core updater 27/27、config application 4/4、Tauri adapter 3/3、bundle 18/18、application client 12/12、application state 4/4、Specta 1/1；`pnpm typecheck`、`pnpm lint:frontend-boundaries`、`pnpm --filter=chimera-ui build`、E2E harness unit 44/44 通过。未跑真实桌面更新、签名包安装、Windows/Linux 动态验证或独立审核；当前代码不能据此宣称具备发行验收。

## 2026-11-01：备份内容校验、隔离恢复与保留安全（ref v2.0.1；L2，未提交）

- **本批目的/边界**：验证上轮 `core/backup.rs` 产生的迁移前快照确实可以还原升级前字节，而不只证明 `manifest.json` 存在。所有“恢复”仅在 `#[cfg(test)]` 的隔离 `tempfile` 目标目录运行；**没有向产品暴露覆盖用户配置的 restore 命令**，不触及真实配置、redb、Service/TUN、核心网络或窗口。独立 crash-reporter 仍待成套启动监督协议接线。
- **ref 源码映射与有意扩展**：pinned `ref/backend/tauri/src/core/backup.rs::{Manifest,create_backup,prune_backups}` 只有来源、版本、类型、链接目标等元数据，**没有逐文件摘要清单**；Chimera `backend/tauri/src/core/backup.rs::{Manifest,write_backup,BackupInfo::is_available,prune_backups}` 在保留 ref 字段基础上新增 `format_version=1`、相对路径、长度和 SHA-256 的 `files`、以及 `directories`（包括空目录）。新增 `backend/tauri/src/core/backup/verify.rs::{scan_snapshot,verify_at,verify_backup}` 检查 staged 数据与完整 manifest 一致后才发布最终备份目录；再次确认可用性时逐文件读取并比对 hash、文件列表、目录结构、链接记录和安全相对路径，拒绝外部/不明目录节点与缺失数据。只提高意外损坏检测，**并不是经过独立签名的抗恶意篡改证明**：更改数据和 manifest 的恶意行为无法由同一备份内的 hash 单独防御。
- **旧 ref 备份的兼容策略**：旧版未带校验字段的 `manifest.json` 不会被写坏或强制升级，但 `BackupInfo::is_available` 和故障报告不把这类无索引快照标为“已验证”。`prune_backups` 只在**实际 SHA-256 校验通过**时将快照计入保留数量、删除过期的已验证快照；损坏快照、旧 ref 格式、陌生目录和未归属 `.partial` 留给人工检查，防止不完整的新快照挤掉旧的可用副本。这种保守策略可能增加磁盘占用，后续需要用户明确可见的空间/保留与旧格式迁移方案。
- **隔离恢复证据**：`backup/verify.rs::restore_to_empty_sandbox` 只在 `#[cfg(test)]` 编译，必须先验证备份，再以独占创建的空目标目录还原清单列出的 config、data、空目录、文件和（Unix）有记录的链接；再重新 hash 提取结果并检查链接目标。测试用真实 redb 数据库，在建立备份后改变原始键值，确认独立恢复目录仍能读到旧交易中的值；嵌套 Profile、空目录、符号链接目标字节与原文件均保留。损坏 hash、删除文件、额外文件、恶意 `../` 清单路径及没有 checksum 的 ref 旧清单全部在创建恢复目标目录**之前**被拒绝；已有目的目录永不覆盖。产品级恢复还需要进程停止/文件所有权、跨平台链接策略、用户明确确认及原子回滚。
- **故障诊断验证**：`backend/tauri/src/startup_failure.rs::Report::from_error_with_backup` 保留现有调用，但底层 `is_available` 升级为真实逐文件校验。测试明确破坏一份本来正常的快照：不再展示 `backups_dir`；恢复原有字节后再次可见。旧版无索引文件不被伪称为已验证。
- **运行验证**：macOS ARM64，`core::backup::tests` **20/20**、`startup_failure::tests` **6/6**、`core::migration::runner::tests` **21/21** 已在本轮最终代码通过；完整 `cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib -- --test-threads=4` 在最终代码上 **776 通过、0 失败、1 ignored**；Cargo check、Rust fmt、Prettier、`git diff --check` 的最后结果以本轮终验为准。第一次隔离链接恢复测试失败，原因是测试恢复扫描错误地沿用“备份目录不允许链接”的检查；已拆分为“备份禁止链接 / 仅隔离恢复允许 manifest 中声明的链接”，重新运行 20/20。初次编译也因 `BackupError::Verification` 增加了新的错误分支而需要更新 startup-failure 匹配，修复后已通过。
- **合入风险与回滚**：L2，须额外审查较大配置或数据库的两轮 SHA 读取造成的启动性能开销、写入间隙/并发改写与 race、Unix 非 UTF-8 名称、Windows reparse/锁共享及链接恢复、旧 ref 备份格式增量兼容、磁盘空间不足和损坏备份保留策略。SHA 与清单同在备份根中，不能取代可信签名、独立的恢复校验或用户确认。本批回滚只需撤销 `core/backup.rs` 新增字段/校验、删除 `core/backup/verify.rs`、回退 prune 逻辑及关联单测；保留之前 Runner 事前备份门槛。无暂存、提交、推送或真实用户数据修改。

## 2026-11-01：迁移前完整快照与失败阻断（ref v2.0.1；L2，未提交）

- **目标、范围、非目标**：在 pinned ref `518bf49843e1f735380e11da1ebe2a85eb8ce8c6` 上关闭上轮审查 `HIGH/BLOCKER: no pre-migration backup` 的主要代码缺口：真实旧配置在任何待执行的修改步骤前创建完整配置树 + `storage.db` 文件快照，备份失败不得继续改写 typed 文件。本轮仅接通**Tauri 客户端拥有存储之前**的 `StorageSource::File` 迁移备份；不增加运行中 `Storage::export_to` 手动备份、不自动恢复或覆写旧配置、不自动提交、无特权/真实网络操作。备份仍由测试临时目录验证，生产路径装配在现有 `setup.rs` 的迁移 Runner。
- **ref 路径/符号 → Chimera**：从 `ref/backend/tauri/src/core/backup.rs` 移植 `BackupKind`、`StorageSource`（暂仅 File）、`BackupRequest`、`BackupInfo`、`create_backup`、`prune_backups`、原子预留 `.partial` 与最终目录 rename、递归 config 快照、manifest JSON、符号链接仅登记不跟随、迁移前版本与时间戳、命名碰撞和保留数 3 的语义；新建 `backend/tauri/src/core/backup.rs`，由 `core/mod.rs` 声明。ref `nyanpasu_paths::PathResolver::{backups_dir,scripts_dir}` → `utils/path.rs::PathResolver` 同名路径 API（Chimera 标准 `PathBuf`）。ref `core/migration/runner.rs::{ensure_backup,has_nothing_to_protect,run_pending,run_migration}` → Chimera `core/migration/runner.rs` 的同名门槛与 `backup: Option<BackupInfo>`；在 pending 步骤前备份，也在无 pending 但需对已有 document 写 schema stamp 的分支备份，失败则直接返回源错误，不运行配置写入。在成功结束后按 ref 保留最新三个迁移备份；在仅由迁移 Runner 创建空基线的干净新安装中不生成备份，正常已升级再次启动不产生重复备份；**当前 Tauri `init_config` 先创建默认 legacy 文件，因此真实首次 GUI 启动可能仍对这些默认文件做一次快照，尚未做真实冷启动验证**。
- **intentional safe delta**：Chimera `core::storage::Storage` 目前没有 ref 的 live redb `export_to`，所以只支持迁移前未持有的文件复制，不声称提供手动在线备份、运行中数据库一致性快照或跨进程独占。为减少无关数据被误删/越界复制，增强 ref 行为：备份目录、config 根目录和 storage 文件的符号链接/非普通节点被拒绝；备份文件树中的 symlink 只进 manifest、特殊非普通文件拒绝；`prune_backups` 只删有匹配 manifest 的完整目录，拒绝 symlink 根，不清除未知/可能仍有持有者的 `.partial`；`create_backup` 失败只清理自己本次预留的 partial。这些兼容与安全差异必须由 reviewer 和 Windows/Linux 文件系统测试复核。
- **失败诊断桥接**：`core/migration/runner.rs::backup_info` 在成功提交快照后才可取；`setup.rs` 迁移失败交给 `startup_failure.rs::report_boundary_with_backup`，`Report::from_error_with_backup` 只在当前快照路径、父目录及 manifest 均仍为正规非链接节点时标记已确认的 `backups_dir`。`BackupError::{Io,InvalidNode}` 保持精确路径用于报告。**仍未**迁移 ref 的单独 crash-reporter GUI/子进程 IPC，stderr/log 仅提供诊断和备份路径。
- **临时目录回归**：从 ref 对应 backup tests 移植/适配：manifest、时间、来源文件完整复制、无 storage、单秒命名碰撞、config/data 同树、symlink 不跟随、中途失败无残留、只 prune 已确认 backup、拒绝 backup/config/storage symlink；Runner 测试含先备份后写 typed、备份失败先停且重试成功、后续 step 失败仍保留快照并不标成功、存储节点异常失败清理、无旧数据跳过、健康重启不重复备份；诊断测试确保只有已提交且仍可用的快照才显示备份入口。macOS ARM64 经过真实运行的 `cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib -- --test-threads=4` **771 通过、0 失败、1 ignored**；本批定向备份 **14/14**、Runner **20/20**、诊断 **6/6** 在额外安全用例加入前已通过，新增测试均包含于最终完整库测试，不把 ignored、Windows/Linux 和 GUI E2E 计入成功。
- **遗留风险与回滚**：正式上线前必须独立 reviewer 审查 snapshot 可恢复性、数据大小/磁盘耗尽、文件复制期间外部写入、Windows reparse/junction/rename/锁行为、Linux owner/权限、用户跨版本升级与中断重试；需 GUI 工具供用户安全查看与恢复旧快照，并引入真正的运行中 storage 在线导出。当前没有自动回退；备份成功仅意味着有真实迁移前快照，不保证自动恢复或所有并发来源的一致性。回滚本批需撤销 `core/backup.rs`、`core/mod.rs`、`utils/path.rs` 新路径、`migration/runner.rs` 备份钩子、`setup.rs` 与 `startup_failure.rs` 的对应报告适配，以及本次测试/记录；保持前轮 Jobs/session-state/typed I/O 修改。无暂存、提交或推送。

## 2026-11-01：迁移整体验收、自查及 legacy 启动链接保护（未提交）

- **范围与结论**：基于 Chimera `dd5cd88a`、ref v2.0.1 `518bf49843e1f735380e11da1ebe2a85eb8ce8c6`、Runtime `2cb1160`，对前轮累计未提交的迁移、安全恢复、启动失败诊断进行代码自查与大范围单元测试。**尚无独立 reviewer；不得合并**。详细发现、证据、下一阶段准入顺序及回滚界限见 [2026-11-01 整体验收报告](design/2026-11-01-ref-acceptance.md)。
- **[HIGH] 持续阻断**：Chimera `core/migration/runner.rs::run_pending` 仍缺少 ref `core/migration/runner.rs::ensure_backup` 的改写前完整配置/存储快照；单文件原子写入不足以保障跨模块失败时恢复升级前状态。此项属于源码证实的设计缺口/潜在数据损失风险，未在真实用户数据上故障注入，需在继续 crash-reporter 大迁移前完成备份模块及失败隔离验收。
- **本轮修复**：`utils/init/mod.rs::init_config` 在 Tauri `setup`/迁移前对旧版 Verge、Clash guard、Profiles 创建默认文件时不再用 `path.exists()`（会误认悬空符号链接为缺失），改为可注入临时目录测试的 `initialize_default_yaml`，调用前轮 `core::corrupt::config_node_exists`；仍保留默认模板与注释头、既有配置不写入。新增四项测试覆盖缺失文件、已有文件、三种旧版配置悬空链接与异常父目录。其余启动错误在 logger/目录/窗口等早期出口仍未全部接通；本轮不擅自复制独立 GUI 或新备份格式。
- **macOS ARM64 运行证据**：`cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib -- --test-threads=4` **修复后 749 通过、0 失败、1 ignored**；`chimera-jobs --all-targets` **45 项通过**；`chimera-config` **172/172**、`chimera-core` **105/105**、`chimera-traffic` **103/103**（另 1 ignored）；四个前端包 `tsc --noEmit` 通过；E2E TypeScript 类型检查通过，E2E harness `test:unit` **30/30**。**未**执行真实 Tauri desktop E2E、Windows/Linux、Service/TUN、宿主网络或全部 features。最终 Rust fmt、Prettier、Cargo check 与 git diff 验证见当轮末次记录；未提交、未推送、未删除用户数据。

## 2026-11-01：累计迁移整体验收与符号链接失效保护（自查，未提交）

- 见本节下方的完整“整体验收与安全审查报告”：比对 ref v2.0.1 与当前 dirty tree，三项经修复前失败测试复现的迁移文件链接错误已修复；`core/migration/store.rs`、`core/migration/fs.rs` 统一使用不跟随链接的节点存在性校验，在原子写入前拒绝符号链接和非普通文件，保护原始节点。
- 最终 macOS ARM64 上定向迁移 **80/80**、完整 `chimera --lib` **745/745**（另 1 项生成 IPC bindings 的 `#[ignore]` 继续跳过）、`chimera-jobs --all-features` **47/47**，且仅用临时数据夹具；最终质量工具结果按报告末次验收。核心未解决 HIGH：Chimera Runner 尚无 ref `ensure_backup` 迁移前验证门槛，**不能宣称具备可恢复备份，也不建议合入或发布**。独立 Reviewer、Windows/Linux、真实 Tauri GUI/E2E 与 crash-reporter 仍未完成。
- 本批风险 L2，不涉及真实用户配置、系统代理/TUN、子模块内容改动、Git 提交或推送；上游与 Chimera 配置主动使用外部 symlink 的兼容策略尚待独立评估。

### 2026-11-01：整体验收与安全审查报告（详细记录）

> **决策：REQUEST_CHANGES（暂不建议合入或发布）**。本文件是本次编码会话中的**自查与测试报告，不是独立 Reviewer 的审批**；按 `docs/ai-review-checklist.md` 仍需另一上下文的独立审查及适用的维护者决策。

### 目标、基线与边界

- **目标**：对累计未提交的 Jobs journal、配置迁移 I/O、session-state、Application/Clash 失败保护和启动报告代码作链路审查，发现可证实的数据安全问题时最小修复，并在最终代码上执行更大范围回归。
- **风险**：L2（持久化配置、迁移、错误分类/启动路径）；独立 crash-reporter / 移动大规模启动生命周期属于下一阶段架构设计，本批不直接迁移。
- **主仓库**：`master` 的 `dd5cd88a`，本批只在既有未提交工作区修改；`ref` 固定 `518bf49843e1f735380e11da1ebe2a85eb8ce8c6`（v2.0.1）、`backend/chimera-runtime` 固定 `2cb1160996f912b536418cb62bda470d097c1190`；没有提交、推送或修改两个子仓库内容。
- **非目标**：不操作宿主系统网络、权限、真实配置目录、真正的用户备份，不发布、打包或启动桌面 GUI，不自动清理大型 `target` 缓存，也不重写 legacy UI / Agent / Client/Service 的共享业务实现。

### 本批新确认并修复的缺陷

| 结论 | 文件/符号 | 复现依据 | 修复与边界 |
| --- | --- | --- | --- |
| **[HIGH → FIXED]** 悬空 `migration-state.yaml` 被当成新安装 | `backend/tauri/src/core/migration/store.rs::MigrationStore::load_with` | 新增 `dangling_migration_state_link_must_not_load_as_fresh_install`：修复前 test exit 101，误返回默认迁移状态；这可能导致后续迁移用新状态覆盖链接，跳过真实版本审查 | 复用 `core::corrupt::config_node_exists` 的 `symlink_metadata`，节点存在必须尝试读取，失败保留原始 I/O 错误及链接。 |
| **[HIGH → FIXED]** Profiles 文档读取器将悬空链接当作缺失文件 | `backend/tauri/src/core/migration/fs.rs::{try_exists,read_yaml_if_exists_with,read_document_with}` | 新增 `broken_document_link_must_not_be_mistaken_for_missing_file`：修复前 test exit 101；源读取返回 `NotFound` 后直接 `Ok(None)` | 新增 `read_string_if_exists_with`：仅当目标节点确实不存在时才返回 `None`；存在链接则保留原 `NotFound` I/O 错误，且不重建。两个 reader 共用这一路径。 |
| **[MEDIUM → FIXED]** 迁移原子写入静默替换用户符号链接 | `backend/tauri/src/core/migration/fs.rs::atomic_write` | 新增 `atomic_write_preserves_existing_symlink_and_its_target`：修复前 test exit 101，实际写入返回 `Ok(())` 并替换链接节点，而非保留链接关系 | 在原子写入前对叶节点执行有限重试的 `symlink_metadata`；符号链接或非普通文件直接拒绝，保持原链接/目录内容；普通文件与不存在的新文件仍沿用原子写入。补 `atomic_write_rejects_directory_destination_without_touching_children`。此方案不是抵御恶意并发替换节点的完整沙箱。 |

这三处修复只影响迁移文件节点的“读取缺失判定”与“写入目标校验”；保留原有序列化协议、迁移修订号、窗口和客户端事务模型。映射到 ref：`ref/backend/tauri/src/core/migration/{fs.rs,store.rs,runner.rs}`、`ref/backend/tauri/src/core/corrupt.rs`；此处使用已迁入的 `config_node_exists` 是 Chimera 为防止悬空链接数据误判所做的**有依据的安全补充**（ref v2.0.1 的相应 fs 路径仍使用 `try_exists` 或直接把 `NotFound` 当缺失）。

### 仍未解决、阻止合入的事项

#### [HIGH / BLOCKING] 迁移前缺少完整、可核实的备份门槛

- **位置**：`backend/tauri/src/core/migration/runner.rs::run_pending`，新旧文件对比可见。Chimera 在 `ensure_known_revisions()` 后直接开始各模块的 `run_step`，没有保证用户数据已备份；迁移某模块失败后还可能继续处理其他模块。
- **上游证据**：`ref/backend/tauri/src/core/migration/runner.rs::run_pending` 会检查 `has_pending` 并在写入前调用 `ensure_backup()`，备份失败则停止；完成后才按保留政策 prune。
- **风险**：部分迁移已持久化、后续迁移失败或电源中断时，用户可能没有可信回退副本。当前 `startup_failure::Report.backups_dir=None` 的保守设计是正确的，但并不能替代备份。
- **合入条件**：迁入 ref 的真实备份创建/验证、覆盖范围与路径、失败即停止、恢复/兼容与保留政策；至少有“备份失败时任何待迁移原始文件未修改”和“中途故障后备份可完整读取”的独立测试。不要仅添加备份目录名或空目录冒充备份。

#### [MEDIUM / OPEN] 仍无独立的启动失败 GUI 与子进程报告合同

- **位置**：`backend/tauri/src/startup_failure.rs::report_boundary` 只记录 `tracing`、stderr，并透传 `anyhow`；`setup.rs` 的四个后期入口已接线，但在 Tauri 初始化或 logger 之前失败的场景仍缺窗口。
- **上游差异**：`ref/backend/tauri/src/{startup_failure.rs,setup.rs,lib.rs}`、`core/migration/child_result.rs` 和独立 crash-reporter 构成整体协议。Chimera 的现有 Tauri/legacy/Service/Agent composition root 不能直接替换。
- **验收条件**：先定义进程间 JSON 合同、exit code、错误路径白名单、备份存在性、窗口显示失败后的 fallback；验证应用主窗口根本未创建时的对话框、退出与无数据修改。属于需要独立设计与必要审批的架构阶段。

#### [MEDIUM / OPEN] 平台、外部链接兼容、真实启动验收

- **位置**：`core/migration/fs.rs::atomic_write` 的叶节点预检与真实原子 rename 之间仍有 TOCTOU 窗口；父目录的符号链接与外部挂载规则未本批扩围处理。与之相邻的 `chimera-core::PersistentStateManager` 自己的落盘实现可能另有链接语义，尚未经过本次写入回归。
- **条件**：先与用户数据兼容策略明确“主动将配置文件链接到外部位置”时应当拒绝、保留还是原地修改；针对 Application/Clash 的健康链接读取后更新补独立持久化测试；评审 Windows 重解析点/锁共享/权限、Linux 文件系统、macOS 完整桌面冷启动、主 UI / legacy UI / Agent 统一状态。不能因本地单元测试全绿而声称这些验收完成。

#### [LOW / OPEN] 依赖与诊断安全

- Chimera 仍是 `redb 4.3.0`，ref 使用 `4.4.0`，`CursorError` 适配缺失为已记录的有意差异；升级需要独立依赖与数据兼容验证。
- 启动诊断输出包含完整错误链及本机配置路径。在引入日志附件、上传或 UI 复制功能前需单独检查敏感字段脱敏及日志保留策略；本批未发现真实敏感数据泄漏，也没有联网发送。

### 实际执行的检查与限制

测试环境为连接的 macOS ARM64 DevSpace，Cargo `--offline --locked`，使用已缓存的 Nix PATH、现有 Xcode SDK 以及 `/usr/bin/clang` 链接器；所有本批故障注入都使用临时目录：

| 测试/命令 | 结果 |
| --- | --- |
| 三项核心反例（分别针对 state 链接、document 链接、atomic_write 链接） | 修复前各 **exit 101**，验证旧逻辑确有回归；见 `/tmp/chimera-review-pre-fix.log`、`/tmp/chimera-review-write-before.log` |
| `cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib core::migration` | 最终代码 **80/80**，见 `/tmp/chimera-review-migration-final.log` |
| `cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib -- --test-threads=1` | 最终代码 **745/745**，另 **1 项按原有 `#[ignore]` 跳过**，见 `/tmp/chimera-review-full-lib-final.log`。跳过项 `specta_export::tests::update_typescript_bindings` 会改写前端生成文件 |
| `cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera-jobs --all-features` | 全套（含文档测试）**47/47**，见 `/tmp/chimera-review-jobs.log` |

后续的 `cargo check` / rustfmt / Prettier / Git diff 与前端类型/边界检查见最终验收输出；失败项不能在更新证据前填“通过”。本次未执行真正安装、签名、Windows/Linux、桌面 GUI WebDriver、系统网络/TUN/Service 变更测试；也未运行远端 CI。构建产物仅使用缓存，未清理其他工作区数据。

### 后续实施顺序与回滚

1. **先做备份门槛**：检查 `ref/backend/tauri/src/core/{backup.rs,migration/runner.rs}`，迁入备份范围、原子创建、失败不写任何数据、备份校验及保留策略；连接 `PathResolver` 和真实配置/数据存储，落地隔离端到端备份故障注入。
2. **再做启动报告及独立界面**：按完整进程/错误协议迁移，保留 Chimera Service/Agent、legacy UI 与单一 shared API；禁止“GUI 没起时弹一个 Tauri 对话框”作为假完成。
3. **再做发行前验收**：macOS 真实冷启动和失败窗口、Windows/Linux 平台规则与隔离 E2E、CI + 新上下文独立审查。当前结论仅为自查，**绝不视为 Review 已批准**。
4. **本轮修复回滚**：只撤销 `core/migration/{fs.rs,store.rs}` 新增三项节点保护及回归测试；保留之前对齐的 I/O 重试、Jobs、session-state 和错误报告。所有变更尚未提交，两个 submodule 没有改动内容，切勿直接硬重置累计工作区。

## 2026-10-31：启动失败的类型化分类与诊断报告（ref v2.0.1；部分迁移，未提交）

- **目标与实际完成边界（L2，配置版本与启动错误合同）**：沿用本地已核对的 ref SHA `518bf49843e1f735380e11da1ebe2a85eb8ce8c6`，建立不依赖字符串猜测的启动失败分类，并在生产启动的四个实际错误出口保留完整源链、已确认数据路径、冲突版本和 I/O 故障类型。本批**不**引入上游独立 crash-reporter、迁移子进程 IPC、提前启动的备份生成器或 GUI 恢复窗口；不重建/改写任何真实用户配置，也不将“可能的备份目录”当作已存在的备份。仍保留 Chimera 的 Tauri composition root、主/legacy UI、Agent 与 Client/Service API。
- **ref 路径/符号 → Chimera 对应**：`ref/backend/tauri/src/startup_failure.rs::{FailureKind,StartupFailure,Report,StartupFailure::from_error}` 与 `ref/backend/tauri/src/core/migration/child_result.rs::{FailureStage,RevisionFailure,Failure::from_error}` → 新增 `backend/tauri/src/startup_failure.rs::{FailureKind,StartupStage,RevisionFailure,StartupFailure,Report,report_boundary}`；保留同名的非品牌模型/序列化 `snake_case`，阶段差异明确限定为 Chimera 已接入的 `Jobs / MigrationPrepare / MigrationRun / TypedClient`；该模块仅分类及诊断输出，不持久化报告或展示 GUI。完整 ref `FailureStage` 目前是子进程迁移协议，不能伪称已接通；新阶段类型是本地 compatibility adapter，待子进程迁移入口落地后收敛。
- **从字符串错误到类型化来源**：`ref/backend/tauri/src/core/migration/store.rs::Error::{NewerSchema,OlderSchema}` → `backend/tauri/src/core/migration/store.rs::SchemaError::{Newer,Older}`，从读取的版本号返回源路径、读取和支持的版本；继续拒绝不兼容 schema，**未更改** `STORE_SCHEMA_VERSION=2` 或其写入格式。 `ref/backend/tauri/src/core/migration/runner.rs::Error::{NewerDocument,NewerAppliedRevision}` → `backend/tauri/src/core/migration/runner.rs::RevisionError::{NewerDocument,NewerAppliedModule}`；仅替换先前的 `bail!` 版本字符串，保留原有拒绝时机。 `StartupFailure::from_error` 在 `std::error::Error::source()` 链提取 `MigrationCheckError` 的实际路径、`session_state::Error::ReadState`、`corrupt::Error::PreserveEvidence` 和上述版本类型，再复用 `core::corrupt::error_io` 区分权限拒绝/短暂资源占用；其他来源降级为明确的阶段分类，不猜测文件路径。JSON roundtrip 已由单测检查。
- **生产接线、对 GUI 的诚实限制**：`ref/backend/tauri/src/setup.rs` 与 `ref/backend/tauri/src/lib.rs` 负责分类及向独立 reporter 呈现；`backend/tauri/src/setup.rs` 当前仍返回 `anyhow::Result`，通过 `report_boundary(..., &PathResolver)` 分别包装 `client::jobs::start`、`migration::Runner::with_paths`、`run_pending`、`ChimeraClient::try_new_with_args`。只在错误时记录 `tracing::error!` 和 stderr 的路径/分类/原因，同时保留原始 `anyhow` 误差链供原 Tauri 调用方处理，**未变更**已启动服务、窗口、Agent、Core lifecycle 或实际退出策略。依旧可能有初始化早期尚未纳入上述四个边界的错误；没有可交互的错误窗口、备份按钮或 reporter 子进程。
- **验证与失败修复**：macOS ARM64，使用缓存 Nix 工具链 + 本机 Xcode SDK，`cargo check --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib` 通过；`cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib startup_failure::tests` **5/5** 通过（原始源链、未来/过旧 schema、未来文档和模块、权限/占用优先级、默认安全路径、序列化及无备份副作用）。`core::migration` **76/76** 通过，包括新增 `store::SchemaError` 数据不被改写的回归，以及原有未来修订拒绝测试；前轮恢复回归 `client::jobs::recovery_tests` **7/7**、`core::session_state::tests` **4/4**、`client::application::tests` **11/11**、`client::clash_config::tests` **11/11**。首次运行诊断测试为 4/5，失败因测试将 `tempfile::TempDir` 所有者立即丢弃后再次 `read_dir`；已保留临时目录对象并重跑 5/5，非产品逻辑出错。最后的 rustfmt、文档 Prettier 和 `git diff --check` 以末次验收为准。
- **风险、回滚和下一步**：L2 独立 reviewer 仍需检查 Windows/Linux、真实 Tauri desktop 冷启动及失败 UI、更多 `setup` 出口、是否合适暴露路径/详情、真实迁移备份与崩溃时序；不能把本批自测称为跨平台发布验收。回滚本轮：还原 `backend/tauri/src/{lib.rs,setup.rs,core/migration/{store.rs,runner.rs}}` 的本轮片段，删除 `backend/tauri/src/startup_failure.rs` 并更新本记录；不影响之前 Jobs/session-state/配置 I/O 修改。根分支、ref 子模块、runtime 子模块均不提交/推送，也不操作真实用户数据。

## 2026-10-31：Application / Clash 重要配置启动检查与失败保全（ref v2.0.1；未提交）

- **目标、基线及风险（L2）**：在 ref `518bf49843e1f735380e11da1ebe2a85eb8ce8c6` 的重要用户配置“失败即停止、绝不自动重建”语义上，修复 Chimera 应用/Clash 配置路径检查会把悬空符号链接当作文件缺失、继而使用默认或 legacy seed 的风险。**只修改路径判定与相关回归测试**；不重置、不迁走 Application、Clash、Profiles 或原始 legacy 配置，不更改配置格式、运行核心、网络、TUN 或已有 legacy UI / Agent 共用接口。
- **参考路径/符号映射与受约束差异**：`ref/backend/tauri/src/client/{application.rs,clash_config.rs}::{ApplicationClient,ClashConfigClient}::new` 均在已有文件出现但不可解析时报告加载错误，不进行自动损坏重建。Chimera 对应 `backend/tauri/src/client/{application.rs,clash_config.rs}::new` 保留 `PersistentStateManagerSetup::load/from_state`、legacy `VergeLegacyBridge/ClashLegacyBridge` 和 RuntimePatchCoordinator；仅把 `config_path.exists()` 换成 `crate::core::corrupt::config_node_exists` 并在错误链附上配置路径。它使用前轮已对齐的 `corrupt::retry_io` 和 `std::fs::symlink_metadata`，区分不存在（可使用原 seed）、存在（必须尝试读取）与访问失败（明确错误）。这是 ref 尚用 `Path::exists` 的 **有意安全补充**：悬空链接视为已有、必须报错；有效链接仍按此前行为加载且不修改目标。
- **迁移前的失效保护**：`ref/backend/tauri/src/core/migration/modules/typed_config.rs::{typed_file_state,classify_shared_clash_file,read_legacy_verge,merge_legacy_clash_file}` → `backend/tauri/src/core/migration/modules/typed_config.rs`；当前 Chimera 不采用 ref 的独立 application / clash_config 文档模块和 stamped 格式，故保留本地 `typed_config` 与 `serde_yaml_ng` 兼容机制，只复用相同的错误保留策略。将 `typed_path_exists` 及两处旧文件输入检测改用上述无跟随节点检查，防止 `application.yaml`、`clash-config.yaml`、旧 `verge` 或守护覆盖文件是悬空链接时被默默跳过；失败返回原有迁移错误，不写入 typed 默认配置。
- **验证**：macOS aarch64，使用缓存 Nix shell 工具链及本机 Xcode SDK/linker 环境变量执行 `cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib` 的定向过滤：`core::corrupt::tests` **3/3**、`client::application::tests` **11/11**、`client::clash_config::tests` **11/11**，并在最后修改旧版输入检查后重跑 `core::migration` **75/75**。测试覆盖普通/缺失节点、悬空链接、原始有效链接、损坏 YAML 不覆盖、legacy mirror 未初始化及旧版迁移不生成默认文件。所有文件是隔离 `tempfile`，无真实用户数据；主程序编译/格式检查结果以最终验收为准。
- **明确未完成及回滚**：上游 `ref/backend/tauri/src/startup_failure.rs` + `backend/nyanpasu-crash-reporter` + `core/migration/child_result.rs` + 新的 `setup.rs/lib.rs` 启动监督形成完整链；Chimera 当前缺少相应进程报告协议且沿用自身 Tauri/Service/Agent/legacy 架构，不能只复制弹窗声称已覆盖启动失败。独立 reporter/UI、备份与迁移错误分类、跨平台早期失败和桌面 GUI/E2E 尚待有设计、测试与审查的后续批次。本批回滚时只撤销 `core/corrupt.rs::config_node_exists`、Application/Clash 两个 client 的检查、`core/migration/modules/typed_config.rs` 的本轮节点检查及其回归测试；先前 Jobs、session-state、安全读取迁移不受影响。无提交、推送、CI 或宿主特权操作。

## 2026-10-31：session-state 启动损坏恢复（ref v2.0.1；未提交）

- **目标 / 边界 / 风险 L2**：对齐 ref 固定 SHA `518bf49843e1f735380e11da1ebe2a85eb8ce8c6` 的可丢弃窗口/会话状态恢复，在迁移 Runner、typed-config 基线检测与 Chimera SessionStateClient 初始化中复用同一个 reader。损坏的 `session-state.yaml`、目录和符号链接先保全为不覆盖既有证据的 `.corrupt-UTC` 兄弟节点，然后写入默认状态；权限、瞬时 I/O 失败不自动恢复。仅针对窗口状态：**没有**为 application、Clash、Profiles、migration-state 或 KV 等其他用户配置开启自动重建，不改变其持久化结构、身份、权限或网络行为。
- **路径 / 符号映射**：`ref/backend/tauri/src/core/session_state.rs::{read,read_with,reset,Error}` → 新增 `backend/tauri/src/core/session_state.rs`，沿用上游分级错误、`symlink_metadata`、`corrupt::{retry_io,retry_with_delay,preserve}`、默认 YAML 原子写回、50/100/200ms 退避；仅将 `nyanpasu_config::state` → `chimera_config::state`、`nyanpasu_utils::io` → 已有本地兼容 `crate::utils::recovery_io`，并因 Chimera 的迁移 fs 仍返回 `anyhow::Error` 而将 `Error::WriteDefault::source` 设为 `anyhow::Error`。声明 `backend/tauri/src/core/mod.rs::session_state` 模块。
- **双入口完整接线**：`ref/backend/tauri/src/core/migration/modules/typed_config.rs::{typed_file_state,read_session_state,validate_existing_application_and_session}` → Chimera 同名文件；先调用 `read_session_state(ctx)?.is_some()`，**在** partial typed-config 检查之前恢复并保全，完整 typed config 的验证同用该 reader。不重置不存在的其他 config 文件，仍按原规则拒绝部分缺失。上游 `ref/backend/tauri/src/client/session_state.rs::SessionStateClient::new` → Chimera 同名 client，`spawn_blocking + blocking::join` 读取该共享 reader；正常文件优先于 legacy seed，缺失才使用原 seed，损坏则使用保全后的默认状态，初始化仍调用 Chimera 的 `PersistentStateManagerSetup::from_state`、`SessionStateActor`、`WindowLegacyBridge::prepare/apply`。**保留** Chimera 事务/CAS、legacy UI 窗口镜像和 agent 共用入口；不复制 ref 的无 legacy bridge 的状态管理重构或改变调用参数。
- **测试执行及安全性**：在隔离 `tempfile` 临时目录完成：`cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib core::session_state::tests` **4/4**（有界重试、权限失败不保全、目录保全、Unix 符号链接保全且不改目标）；`... client::session_state::tests` **4/4**（有效 typed 配置优先、损坏默认化和 legacy 镜像/恢复后 patch 持久化、缺失文件回退 legacy seed、三阶段迁移/客户端启动注入恢复）；`... core::migration` **71/71**（含新增 typed-config 基线检测对 full/partial 布局的保全测试和上轮 I/O 回归）。实际 Rust format、主程序 check 和 docs 检查见本次结尾验收。所有数据损坏注入均为临时路径，未检查/修改真实用户 config。
- **局限、回滚与后续**：本批只完成 session-state 的自动恢复，不能宣称所有用户配置损坏均可恢复，也未执行 Windows 专属共享/锁违例测试、完整桌面 cold-start GUI E2E、Linux/Windows 符号链接/权限和真正的宿主文件锁测试。若必要，删除新增 `core/session_state.rs`、撤销 `core/mod.rs`、`client/session_state.rs` 和 `core/migration/modules/typed_config.rs` 的本批修改及相关记录；此前 Jobs journal 和配置迁移 I/O 修改保留。尚待独立 L2 diff/recovery 审查与跨平台 runner 验证；未提交、推送或触发 CI。

## 2026-10-31：配置迁移文件 I/O 稳健性与原始错误保留（部分迁移，未提交）

- **目标、基线和边界（L2）**：继续对齐 `ref` 固定 SHA `518bf49843e1f735380e11da1ebe2a85eb8ce8c6` 的启动数据恢复前置条件。本批只在 Chimera 已有的配置迁移 I/O 层增加临时错误的有界重试与错误保留，不对损坏的用户配置、migration-state、Profiles 或旧版配置执行自动重置、数据删除或新 schema 替换。测试路径均为 `tempfile` 隔离文件；保留 Main/Legacy/Agent、Chimera Client/Service、现有持久化布局和调用入口。
- **ref → Chimera 路径及符号**：`ref/backend/tauri/src/core/migration/fs.rs::{try_exists,read_yaml_if_exists_with,read_document_with,atomic_write}` → `backend/tauri/src/core/migration/fs.rs` 同名实现；调用前轮已迁入的 `crate::core::corrupt::{retry_io,retry}` 和 `crate::utils::recovery_io::{classify,IoFailure}`，保持 Chimera `chimera_core::format`、`serde_yaml_ng`、`anyhow::Result`，不引入 Nyanpasu 尚未采用的 Chimera 错误类型。上游四个 YAML/document 注入式重试与权限失败回归测试也完整映射到同模块。`ref/backend/tauri/src/core/migration/store.rs::MigrationStore::{load,load_with}` → `backend/tauri/src/core/migration/store.rs`，保留公共 `anyhow::Result` 而复用上游 `load_with` 重试边界及两个原始错误来源测试。额外修正 `path.exists()` 会吞掉父路径访问错误的情况：用 `retry_io(|| path.try_exists())` 传播错误，避免误把不可读取的 migration-state 当成全新默认状态；属于有理由记录的局部 ref 差异。
- **不变项、失败及回滚**：已有配置文档和迁移状态的解析、戳验证、原子替换格式与备份策略不改。短暂 `Interrupted`/`WouldBlock` 等最多重试三次（50/100/200ms），权限失败直接保留原始 `io::Error` 的 kind 和源链并返回错误；不创建 `.corrupt` 文件。对无新格式风险的原子目录创建及写入也复用同一重试策略，仍使用现有 `atomicwrites` 原子文件机制。若回滚，仅还原 `backend/tauri/src/core/migration/{fs.rs,store.rs}` 及本记录；保留前一轮 Jobs journal 迁移和 ref gitlink 不受影响。未提交、推送或触发 CI。
- **验证限制**：本机 macOS ARM64，通过旧 Nix 缓存环境 + 已安装的 Xcode SDK 执行 `cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib core::migration`，**69/69 通过**，包含新增六项与原有 Runner、Profiles、typed config、stamp、store 回归；再以本轮最终命令核对 scoped cargo fmt、主程序 cargo check、Prettier 与 `git diff --check`。无法把迁移 I/O 回归误称为全部用户配置自动恢复、启动错误对话框、Windows/Linux 文件系统或完整 GUI E2E 验收。后续必须分别评估 `core/migration/runner.rs`、session state、typed client、storage/backup、startup failure UI 的差异和跨模块错误合同，必要时做独立设计与审查后再扩围。
- **下一批待审**：ref 的 `core/session_state.rs` 在迁移和 client 构造前统一读取并保全可丢弃的窗口状态；Chimera 的 `SessionStateClient::new(path,seed,bridge)` 还承担 legacy mirror、CAS 和旧 UI 兼容路径，而且 `typed_config::validate_existing_application_and_session` 用共享 `read_yaml`。直接替换上游 client 会损坏这些合同；本批不接线自动 session-state 重建，以免制造“迁移前已重建但旧窗口桥不同步”的新风险。需先做完整临时目录分阶段迁移 + client 重启测试并独立审查。

## 2026-10-31：Jobs journal 启动安全恢复（ref v2.0.1；本地未提交）

- **目标 / 风险与基线**：L2（Jobs 持久化数据及应用启动）。沿用经检出的 ref `518bf49843e1f735380e11da1ebe2a85eb8ce8c6`（nested runtime `90ac364c9c52ea05d62f18f00b53be80b3b32144`）的“先识别损坏、保留原始证据、仅重建可恢复的 Jobs journal”策略。无需权限提升，也不更改用户配置、存储格式、TUN、Service/Agent 或任何真实数据；仅对独立临时目录实施损坏测试。
- **ref 路径与符号对齐**：`ref/backend/tauri/src/core/corrupt.rs::{error_io,is_link,retry_io,retry,preserve}` → `backend/tauri/src/core/corrupt.rs`，唯一业务适配是 `nyanpasu_core_manager::Error` → `chimera_core_manager::Error`；`ref/backend/tauri/src/client/jobs.rs::{start,start_service,open_store,Error::unusable_journal}` → 同名 Chimera Jobs client。恢复仅在 `JournalRecoveryDecode`、`JournalPruneDecode`、redb `Corrupt/Incompatible` 或非普通文件时触发，原路径先使用不可覆盖的 `.corrupt-UTC` 同级备份名保存，再启动空 journal；锁占用、权限、瞬时及未分类 I/O 故障直接失败，不会删除原件。Chimera 自有 `ProfileSyncStatus`、ProfileId/Source、客户端 API/Job execution 和 legacy/agent shared calls 全部保留。
- **临时兼容边界**：上游 `ref/backend/nyanpasu-runtime/crates/nyanpasu-utils/src/io/mod.rs::{IoFailure,classify,transient_backoff}` 与 `.../redb_ext.rs::{RedbFailure,find_failure}` 对应 `backend/tauri/src/utils/{recovery_io.rs,redb_ext.rs}`。因为 Chimera 的 `chimera-utils` 在独立 gitlink `backend/chimera-runtime`，本批未改其子模块或 Service 依赖；以后 runtime utility 与应用 pin 可协调升级时再移出 Tauri。本地 `redb 4.3.0` 不含 ref `redb 4.4.0` 的 `CursorError`，仅省略该类型的适配与测试，保留 `StorageError` 原始错误分类，不升级 redb 或改数据库布局；待依赖收敛重查这一差异。原有上游版权/SPDX 保留在复制的 I/O 分类片段。
- **失败 / 恢复证明**：从 `ref/backend/tauri/src/client/startup_recovery_tests.rs` 迁移 Jobs 的临时路径测试至 `backend/tauri/src/client/jobs_recovery_tests.rs`，覆盖外来垃圾数据、内部 JSON 无法解码、文件夹占用路径、不兼容 redb 表、被其他进程持有的文件；另外检查健康 journal 不误备份、父路径冲突不损坏原文件、权限失败不进入重建策略、Unix 符号链接只移动链接不触碰目标。完整上游 `utils/redb_ext` 与 `core/corrupt` 分类/保全单测保留，只有 redb 版本差异作最小调整。
- **验证与剩余风险**：macOS aarch64，使用本机旧 Nix 工具链缓存、指定 Xcode SDK/linker 环境变量。`cargo check --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib` 通过；`cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib client::jobs::recovery_tests` **7/7** 通过（含健康 journal 和 Unix 符号链接），同一 Tauri lib 的 `utils::recovery_io::tests` **2/2**、`utils::redb_ext::tests` **6/6**、`core::corrupt::tests` **1/1** 通过；`cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera-jobs --all-features` 全套 **47/47** 通过。`cargo fmt --manifest-path backend/Cargo.toml --package chimera --package chimera-jobs -- --check`、两个文档的 Prettier 检查和 `git diff --check` 通过。Tauri lib(test) 有既存非阻断 warning；未做 Windows/Linux 文件锁和符号链接行为、完整应用冷启动 GUI/E2E、权限真实故障注入或签名打包；这些仍需独立 Runner，不将单元/集成验证表述为生产环境跨平台验收。
- **回滚与权限**：如需回滚，可还原 `backend/tauri/src/client/jobs.rs`、`core/mod.rs`、`utils/mod.rs` 与对应对齐记录并移除三个本地兼容模块及本轮测试文件；前一轮已经完成的 `chimera-jobs` typed-error 源文件和 ref gitlink 可按独立批次保留/回滚。用户没有授权提交或推送，本批只保留在本地工作区；没有操作系统网络、安装特权服务或真实用户 journal。

## 2026-10-31：ref v2.0.1 基线与 Jobs journal 结构化错误（部分迁移，未提交）

- **目标 / 范围**：从 `ref` 旧 gitlink `1b5d73c53a7141d216a8a0a7751be38a295be059` 快进到已 fetch 的 `origin/main` `518bf49843e1f735380e11da1ebe2a85eb8ce8c6`（17 commits，v2.0.1），并先迁入启动恢复依赖的 Jobs journal 分类错误。更新后的 `ref` 工作区及嵌套子模块已检查为干净；`backend/nyanpasu-runtime` 从 `e73b9f5` 更新到 `90ac364c9c52ea05d62f18f00b53be80b3b32144`。没有修改 ref 内容或声称本次核验了上游 commit 签名。
- **完整路径 / 符号映射**：`ref/backend/nyanpasu-runtime/crates/nyanpasu-jobs/src/{model.rs,runner.rs,storage/redb.rs}` → `backend/chimera-jobs/src/` 相同文件名，复制上游 `JournalOperation`、`Error::JournalDirectory/Open/Table/Transaction/Storage/Commit/Encode/Decode/RecoveryDecode/PruneDecode`、按操作保留原始错误链和 runner 的非 `Clone` Error 匹配；保留本地 crate 身份 `chimera-jobs`。`ref/.../tests/storage_errors.rs` → `backend/chimera-jobs/tests/storage_errors.rs`，仅替换测试 crate import；`tests/support/mod.rs` 添加上游的 `skip_recovery` fault injection；已有 `execution.rs`、`scheduling.rs` 两处改用 ref 的 `matches!` 检查以适应非 `PartialEq` 错误。没有改动二进制 journal 布局、存储位置、Profile 数据或 Job 执行/提交语义。
- **风险与边界**：L2（后端持久化 journal 错误合同）。分类结构已迁入、原有运行/存储用例需保持通过，但 **自动备份与重建损坏 journal 尚未接入**；`backend/tauri/src/client/jobs.rs` 目前仍直接打开存储，未移植 ref `core/corrupt.rs` / `nyanpasu-utils::redb_ext` / startup error wiring。不能把部分基础迁移称为完整启动恢复，也不能把读权限、锁冲突、瞬时 IO 当作可删除的损坏数据。后续须首先移植 upstream 的类型化 `redb_ext`、无覆盖保全/重试和错误判断，覆盖数据证据保留及无损失败，再接入 Jobs 启动与整个应用启动路径；不要另造字符串错误判断器。保留 legacy UI / Agent / Chimera Client/Service 原有业务入口；本批没有修改其调用、网络、TUN 或用户持久化文件。
- **验证**：已运行 `cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera-jobs --all-features`，包括新增 `storage_errors` 4/4、完整集成测试和 doc-test **47/47 通过**；`cargo fmt --manifest-path backend/Cargo.toml --package chimera-jobs -- --check`、`cargo check --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib`、两份文档的 Prettier 检查和 `git diff --check` 均通过。主程序编译包含原有 229 条非阻断 warning。环境为 macOS ARM64，使用此前缓存的 Nix 工具链并仅为相关命令设置已有的 Xcode SDK 和 linker 路径。此前第一次 `cargo test` 的链接错误来自未找到 macOS SDK / `-liconv`，使用 `SDKROOT` 与 `CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=/usr/bin/clang` 修正后通过；未改系统全局配置。尚未执行完整 Tauri GUI / Windows / Linux 回归；单元/集成结果不能替代真实启动恢复与桌面 E2E。
- **回滚与提交范围**：父仓库 `ref` gitlink 与本指南的基线可单独恢复到旧 SHA；Job 错误迁移可撤回对应三个源码文件、测试支持与测试文件，并恢复 `execution.rs` 和 `scheduling.rs` 的原断言。由于尚未接入自动重建，不改变真实用户 journal。所有修改仍在本地工作区，未提交、推送或触发 CI。


## Profiles 远程入组预检：无效文件夹不得触发 Fetch（2026-10-31，工作区待提交）

- **目标、基线与边界（L2）**：在同一工作区保留前轮未提交的 folder route/E2E 修复和 HTTP(S) URL 输入校验。固定只读 ref `1b5d73c53a7141d216a8a0a7751be38a295be059`，主分支 `1d87c2a4`。当前 Actor 对 `ImportRemote` 先调用 `validate_import_request`，该方法只验证 URL/Options/文档，而 `folder` 要等异步下载完成、`CommitImported` 后续 `join_folder` 才检查。不存在的 folder 会发生不必要的订阅抓取，可能增加用户等待和外部请求。本批收敛到 ref 的下载前 folder 校验，保留提交时二次检查；不改变已有 Config/Folder 数据模型、Agent、legacy UI、文件物化、订阅获取服务、Runtime 或宿主网络配置。
- **ref 代码映射**：`ref/backend/tauri/src/state/profiles/actor.rs::validate_import_request(...,folder:Option<&FolderId>)` 在生成候选 `ProfileItem` 后调用 `Self::join_folder(&mut next, &uid, folder)?`，再执行 `next.validate()`；对应 `backend/tauri/src/state/profiles/actor.rs` 同名方法新增 `folder` 参数及加入候选布局，`ProfilesActorMessage::ImportRemote` 在注册 `PendingImport` 和调用 Fetch 前传入 `folder.as_ref()`。若目标文件夹缺失，返回现有 `ProfilesError::ValidationFailed(ProfileValidationError::FolderNotFound)`，不触发 Fetch、后台 task、事务持久化、物化或 revision 变化。既有 `CommitImported` 仍基于**最新**文档再次调用 `join_folder`，确保预检后被删除的文件夹不会留下孤儿；请求中未指定 folder 的旧版调用仍使用 `None`。Chimera 不迁入 ref 的 `TransformKind` 和下载 supervision 重构，本批仅补足本来已有的 folder 参数预检。
- **回归测试**：复用 `backend/tauri/src/client/profiles.rs::remote_import_into_unknown_folder_rejects_without_persisting`，把其 `MockSubscriptionFetcher` 改为 `expect_fetch().times(0)`，并明确要求 `FolderNotFound`、无新增 Profile、revision 和文件存在状态完全不变。旧实现会执行 Fetch，无法满足零次调用预期；新实现下载前拒绝。已有成功的 `remote_import_joins_folder_in_same_durable_commit` 及 86 项 Client 测试负责保持正常导入路径不退化。新增 `tauri-e2e/specs/profiles-main-layout.e2e.ts` 浏览器 UI 契约：在 Main Config 快捷导入输入 `ftp://example.invalid/...`，通过真实点击提交后检查表单值未清空、`get_profiles` 文档快照不变，再清空输入；测试不修改用户真实 Profile 或主动请求外部 URL，但只有在隔离 Windows Runner **真正执行**才可宣称 UI 拦截成功。
- **URL 安全事实修正**：上节纯前端 schema 确保 URL 深链和表单的 HTTP(S) 体验一致；Chimera 后端已经在 `backend/chimera-config/src/profile/profiles.rs::validate` 拒绝 `UnsupportedRemoteUrlScheme`，其 Actor `validate_import_request` 通过 `next.validate()` 覆盖新创建/导入的远程定义。因此不能将前端 `new URL()` 允许 `ftp` 错说成后端能成功持久化该 scheme，也不能宣称 URL schema 单独提供完整 SSRF 防护。本批**不增加**任何新远程协议或 IPC 调用方式，文件夹预检仅降低无效目标的无谓 Fetch。
- **验证及回滚**：已执行 `nix develop --command cargo check --manifest-path backend/Cargo.toml -p chimera --lib` 与 `nix develop --command cargo check --tests --manifest-path backend/Cargo.toml -p chimera`，**两项均通过**（分别存在已有的非阻断 warnings），因此新增零 Fetch mock 断言也已编译检查；scoped `cargo fmt --check` 通过。`cargo test -p chimera --lib --no-run` 成功完成源码编译阶段，但链接大型 Tauri 测试可执行文件时长期停在 Btrfs `reserve_bytes`（`/home` 文件系统 96% 已用，Btrfs Metadata DUP 16.51GiB 已分配、15.89GiB 已用）。本次已主动中止该本机链接（退出码 143），未完成测试二进制，**新增零次 Fetch 断言及整组 86 项 Client 测试当前不能宣称通过**；先前版本的旧二进制也不用于冒充本次结果。未删除其他用户构建产物或重配置系统文件。TypeScript（含 E2E）、URL 3/3、Folder Layout 21/21、Profile Query 15/15、E2E 工具单测 30/30、Prettier、Oxlint、前端边界和上轮新增 UI 的生产构建均通过。E2E 测试文件通过类型检查但**未运行真实桌面**。回滚时仅撤销 `validate_import_request` 的额外可选 folder 参数、`ImportRemote` 的 preflight 转发和严格零次 Fetch 断言、UI E2E 及记录，原 `CommitImported` 的原子入组保持不变，不需要迁移现有 Profile 数据。后续在具备空间的隔离 CI/Runner 上完成 `client::profiles::tests::` 测试及 Windows GUI 验收，勿修改宿主 glibc/CI 工具链或为了测试贸然删除其他缓存。

## Profiles 订阅 URL 的 HTTP(S) 校验契约迁移（2026-10-31，工作区待提交）

- **基线/范围**：固定只读 ref `1b5d73c53a7141d216a8a0a7751be38a295be059`，主仓同步基线 `1d87c2a4`；紧接上一批本地尚未提交的 Profiles Windows E2E 回归修复。目标是对齐远程订阅 URL 的 HTTP(S) 输入约束，而非替换当前 Chimera 后端下载器、Tauri IPC、Actor 事务、Proxy/Agent 或 legacy UI。先前 Chimera 的 `ProfileQuickImport`、`ProfilesRoute.validateSearch` 只执行 `new URL(value)`；此 API 对 `file:`, `ftp:`, `javascript:`, `data:` 也可成功解析，却不等于适合远程订阅。ref 使用 `z.url({ protocol: /^https?$/ })`，明确仅允许 HTTP/HTTPS。
- **ref 映射及实现**：`ref/frontend/nyanpasu/src/pages/(main)/main/profiles/_modules/subscription-url-schema.ts::subscriptionUrlSchema` → 新建 `frontend/chimera/src/pages/(main)/main/profiles/_modules/subscription-url-schema.ts`，沿用 ref 原式及现有 Zod 4 依赖；`ref/.../_modules/profile-quick-import.tsx` → Chimera 同名组件，调用 `safeParse` 后才执行现有的共享 `useProfile().create` URL 分支，失败保留输入并使用本地化错误通知，不更改 `folder` 入组参数。`ref/.../_modules/create-profile-schema.ts::formSchema.superRefine` → 现有 `frontend/chimera/src/components/profiles/profile-dialog.tsx` 的 RHF URL 字段 `rules.validate`，保证新建与编辑远程 URL 都执行同一约束；不复制 ref 尚未在 Chimera 采用的 `CreateProfileModal` 整套界面和外部链接/Composition 表单。`frontend/chimera/src/pages/(main)/main/profiles/route.tsx::asValidUrl` 亦复用该 schema，只让 HTTP/HTTPS 的 deep-link `subscribeUrl` 进入自动打开的订阅导入弹窗，其他 search 字段沿用现有验证方式。
- **测试与 CI**：新增独立 `frontend/chimera/tests/subscription-url-schema.test.ts` 三项纯单元测试，分别验证合法 HTTP/HTTPS（含本地 IPv4/IPv6 和 query）、浏览器 `URL()` 能解析却不应导入的 `file/ftp/javascript/data/mailto`、非法 URL/缺失/非字符串。注册 `frontend/chimera/package.json::test:subscription-url`，并将其放入 `.github/workflows/ci.yaml` Ubuntu 原有 `Test E2E harness and frontend models` 测试步骤；每次远程 CI 不再只检查旧布局逻辑。`pnpm --filter chimera-ui test:subscription-url` 本地 **3/3 通过**，`pnpm typecheck`（含桌面 E2E）、`pnpm web:build`、`pnpm lint:oxlint`（0 警告）、`pnpm lint:frontend-boundaries`、Folder Layout **21/21**、Profile Query **15/15**、E2E 工具单测 **30/30** 均通过。前端校验测试不证明网络请求没有发出、桌面端 UI 文案或服务端 URL 安全，真实导入和取消流程仍须在隔离 Windows 桌面测试。
- **差异、回滚与安全边界**：此批是参考行为的前端约束迁移；IPC 本身做 URL 语法解析，随后配置领域 `Profiles::validate` 会拒绝不受支持的非 HTTP(S) 远程 scheme（`UnsupportedRemoteUrlScheme`），不能把浏览器 `new URL()` 的宽松解析误称为后端落盘漏洞，也不能把合法 scheme 等同于远程资源安全。若回滚须撤销 schema、三处调用、新测试与 CI 门禁，恢复先前的宽松前端解析；不动配置文档中的历史远程源、不迁移用户 Profile 数据，不修改固定 `ref`、Runtime 子模块、正式发布工作流。任何此前存在于历史配置里的非 HTTP source 仍需独立数据兼容性评审，不能由前端 create 表单的验证代替。

## Profiles 文件夹路由卡片不依赖 `$type` 与详情表单定位回归（2026-10-31，本地待提交）

- **基线与复现**：固定只读 `ref` SHA `1b5d73c53a7141d216a8a0a7751be38a295be059`，当前 `master` SHA `1d87c2a4`。之前提交 `98fb1d2a` 的 [Windows Profiles 专项 E2E #41158693777](https://github.com/MFSGA/Chimera/actions/runs/41158693777) 使用隔离合成 Profile/Folder，`profiles-main-layout.e2e.ts` 在创建文件夹并导航 `/main/profiles/folder/f...` 时得到 Tauri 主窗口错误边界 `Invariant failed`，堆栈 `matchContext → useMatch`；后续 icon/card/progressive/layout 断言因页面留在错误边界而连锁失败。另 `profiles-detail-main.e2e.ts` 的编辑名称/显示验证错误测试在等待 `card-root` 时超时。GitHub 随运行上传的诊断 HTML 确认名称编辑器 `<Card className="w-96">` 不含 `card-root`，而其动画错误节点存在且为 `style="height: auto; opacity: 1"`。
- **ref 实现到本地路径/符号映射**：`ref/frontend/nyanpasu/src/pages/(main)/main/profiles/_modules/profile-card.tsx` 的 `Link`/`profileTypeOf(profile)` → Chimera 同名 `profile-card.tsx` 的详情按钮链接：移除 `IndexRoute.useParams()` 强依赖，用当前 `profile` 数据计算 `type`。复用已移植且与 ref 相同的 `profileTypeOf`（在 `.../_modules/utils.ts`），Config 在文件夹独立路由和总列表均跳往 `/main/profiles/profile/detail/$uid`，Transform 在自己的 Script/Overlay 类型路由跳到对应详情。其余 Proxy activate、Folder move 菜单、已有 Transform sorting 不变。对照 `ref/.../$type/detail/_modules/profile-field-dialog.tsx` 的 `data-slot="profile-field-dialog"`，给 Chimera `profile-name-editor.tsx` 的同样宽度 `w-96` 卡片补稳定 `data-slot`，不更改表单校验、动画、保存或 UI 布局。
- **测试调整及可证伪条件**：已注册的 `tauri-e2e/specs/profiles-main-layout.e2e.ts` 在通过真实文件夹链接进入独立页面后，必须找到此 UID 的 Config 卡片，并断言卡片详情 `href` 的 `pathname` 为 `/main/profiles/profile/detail/$uid`；旧 `IndexRoute.useParams()` 在本次真实执行时会使页面崩溃，因此此场景直接检出修复。`tauri-e2e/specs/profiles-detail-main.e2e.ts` 的两处 `card-root` 读取修正为 `profile-field-dialog`，仍要求卡片宽度精确为 384px、动画错误高度可见、`overflow:hidden`/`opacity:1`，并验证旧 Profile 名未写坏，不降低断言力度。
- **范围与边界**：只修改两个产品组件、两个已存在且已注册的桌面 E2E specs、对齐记录和 Lessons，L2 共享 UI 交互修复 + L1 E2E 定位修复；不改 Rust/IPC/Actor/配置持久化、主/legacy/agent 共享 API，不修改 `ref`、Runtime、系统代理、CI 权限或网络，不操作用户个人数据。本机 Nix 上执行的 TypeScript/生产构建/规则/单测只能证明静态与模型行为；没有 Windows Desktop Runner，不声称实际 E2E 已重新通过。**新 SHA 的 Windows Profiles suite 仍须实际执行**，并核对文件夹打开不触发错误边界、回到总列表后图标及卡片恢复、表单验证动画能完成；独立 E2E 失败文件可能还包含其他未揭示问题，不得将全部失败归为同一根因。
- **回滚与预期**：若必要，单独回滚 `ProfileCard` 的 `profileTypeOf` 链接与 `ProfileNameEditor` 的稳定 selector，以及两条 E2E 断言和记录；不触碰已持久化 folder 数据。应在新构建中验证从文件夹页点击卡片详情与浏览器后退保留 folder 上下文；单个测试的 `href` 检查还不是完整 click/back 实机证据。

## Development nightly 构建：Linux RPM 版本与 macOS updater 重命名可靠性修复（2026-10-31）

- **问题与范围（L3 / CI 与发布构建）**：`master` `311c75c5` 在 GitHub `[Entire] Build Developer Version` 的手动运行 [#41166406031](https://github.com/MFSGA/Chimera/actions/runs/41166406031) 与定时运行 [#41173121067](https://github.com/MFSGA/Chimera/actions/runs/41173121067) 均出现 Linux x86_64 / aarch64 的 RPM 打包拒绝 `0.15.3-alpha+311c75c`（含禁止的 `-`）；Rust Release 编译和 Debian 打包此前已成功。定时运行的 macOS x86_64 单独在 `Rename updater files` 失败，Deno 无法读取 `jsr.io/@std/path/1.1.4_meta.json`，同 SHA 手动运行的 macOS x86_64 已通过，不能据此推断 macOS 二进制编译损坏。任一 matrix 构建失败均使后续 `Update tag`、Release assets/updater 作业依赖跳过。
- **与固定 `ref` 对齐**：只读 `ref` `1b5d73c53a7141d216a8a0a7751be38a295be059` 的 `ref/scripts/src/release/prepare-nightly.ts::{isLinux,linuxVersion,GIT_SHORT_HASH}` → `scripts/prepare-nightly.ts` 同符号/语义；`ref/.github/workflows/deps-build-linux.yaml::Nightly Prepare` → Chimera 同名工作流，给 `pnpm prepare:nightly` 添加 `--linux`。Linux **Tauri 打包配置版本**使用参考 `X.Y.Z+alpha.<前 7 位 SHA>`，合法 SemVer 且不含 RPM 禁止的连字符；根目录和 `frontend/chimera/package.json` 仍使用原 `X.Y.Z-alpha+<SHA>`，Windows/macOS 维持原版本行为，NSIS 不覆盖 Tauri 配置中的基础版本。与 ref 一致改用 `git rev-parse HEAD` 固定取前 7 位，不再依赖 `--short` 在浅克隆与完整仓库中的动态位数。
- **macOS 上游对应与有依据偏差**：`ref/scripts/src/artifacts/upload-macos-updater.ts` → `scripts/deno/upload-macos-updater.ts`，保留 `globby` 的文件模式和 `foo.app.tar.gz` → `foo.<arch>.app.tar.gz` 命名规则、Deno 重命名与控制台输出；仅把失败的 `jsr:@std/path` 替为 Deno 2 支持的内置 `node:path`，避免冷缓存中此次确认的 JSR 网络依赖。为隔离测试导出 `renameMacosUpdaterFiles(backendBuildDir,targetArch)`，生产入口仍扫描 `backend/target`，不需要修改 Tauri 产物目录/发布地址/签名逻辑。ref 仍使用 JSR，这是有日志证据的有意最小缺陷适配；其他 npm 依赖与联网步骤没有去除，不能声称整个 release pipeline 离线。
- **测试契约与注册**：`scripts/prepare-nightly_test.ts` 在单独临时 Git 仓库和虚构配置目录中调用**真实 prepare 脚本**，检查 `--linux --disable-updater` 的 RPM 友好版本、SemVer metadata/签名兼容版本、SHA 7 位、禁止 `-`、不生成 ARM updater；同时检查默认 macOS 和 `--nsis` 的旧行为。`scripts/deno/upload-macos-updater_test.ts` 创建隔离的 `.app.tar.gz`、签名、`.dmg` 和其他文件，调用真正的重命名函数验证产物名称及未命中文件不变，绝不触碰真实 `backend/target`。`package.json::test:nightly-build` 分别使用 `scripts/deno.jsonc` 与 `scripts/deno/deno.jsonc` 的原有**不同**锁文件运行两组 Deno 测试，后者只开放 `--allow-sys=cpus` 供既有 globby 使用；`.github/workflows/ci.yaml` 在 Ubuntu `env.SCRIPTS == true` 的已有质量门禁后新增显式测试步骤，未跳过任何门禁。已本地执行 `pnpm test:nightly-build`，两项测试 **2/2 通过**。实际 Linux RPM/DEB 与 macOS dmg 构建须由新 SHA 的对应 GitHub Actions 执行才能验收，不能把 fixture 断言冒充真实打包成功。
- **非目标、审批与回滚**：本批只修改夜间版本准备与 macOS rename 脚本、Linux nightly 构建开关、CI 回归测试、记录；保留 `ref` 和 Runtime 子模块，未改变正式 `publish.yaml`、release workflow 的权限/分支保护、nightly tag 或现存 updater 端点，未触发发布/工作流 dispatch，也未修改宿主网络或用户存储。属于 L3 CI/发布代码，用户已明确要求修复并提交/推送；新提交后仍应由维护者审查最终 CI 和构建资产，再决定预发布处理。若 Linux 打包产生不可接受的版本排序，需先核对 RPM/DEB 包元数据及 updater 版本兼容，在新构建发布前回退 `--linux`/版本转换或选择 ref 的更完整 release finalizer；macOS 可单独回退 `node:path` 适配，不影响旧产物命名。

## 独立文件夹路由与原子入组导入对齐（2026-10-31，已提交 98fb1d2a）

- **目标、边界与基线**：固定只读 `ref` 为 `1b5d73c53a7141d216a8a0a7751be38a295be059`；当前基准 `master` 已签名提交 `2cfd24d2`（folder UI）及 `f30c6bce`（Rust 领域/Actor/Query）。本节迁入 ref 独立 `/main/profiles/folder/$folderId` 路由及**在新建/远程导入的同一持久化事务内加入指定文件夹**，不使用“导入成功后再次 set_layout”这种可能部分成功的两次写入方式。不修改 Service/TUN/系统代理、Runtime 子模块、legacy UI、订阅定时行为、用户真实数据或 pinned `ref`；本节独立于已提交 UI 的后一轮 L2 改动，尚未提交/推送。
- **后端原子入组映射**：`ref/backend/tauri/src/state/profiles/actor.rs::{NewProfileRequest.folder,ImportRemote.folder,PendingImport.folder,join_folder,Add,CommitImported}` → Chimera `backend/tauri/src/state/profiles/actor.rs` 同名符号/处理分支。`NewProfileRequest` 使用 `#[serde(default)]` 与 `#[specta(optional)]` 保持旧调用请求兼容；不存在/无效目标文件夹返回现有 `ProfilesError::ValidationFailed(ProfileValidationError::FolderNotFound)`，并且不调用持久化阶段，Transform 入组由现有领域校验拒绝。`Add` 的布局加入、文件物化和 UID 创建在一个 actor commit 内完成；远程下载仍先 Fetch/Validate、再经 `CommitImported` 将 folder membership 与 Config 在相同 revision 提交，未破坏取消/Source CAS/物化失败处理。旧 `ProfilesClient::import(url,metadata,option,explicit)` 仍可调用（默认 `None`）；新增 `import_into_folder(...,folder)`，Facade 原 `import_profile` 保留并代理 `import_profile_to_folder`，两个现有 IPC `import_profile`/`import_profile_with_mode` 仅在末尾增 `Option<FolderId>` 参数；省略该 JSON 字段的旧 main/legacy/deep-link RPC 继续按 `None` 处理。
- **生成类型/共享 Query 映射**：`backend/tauri/src/specta_export.rs` 原有注册的 import/create 命令无需改变；使用仓库 Rust Specta 测试导出 `frontend/interface/src/ipc/bindings.ts`，新增 import 命令可空 `folder` 传参及 `NewProfileRequest_Deserialize.folder?`。`frontend/interface/src/ipc/use-profile.ts::CreateParams` 的 URL 分支增可选 `folder`；默认空文件夹以 `null` 传入，`mode='direct'` 路径同时携带 folder；手工创建把 `folder` 放在生成的 `NewProfileRequest` 中。`frontend/chimera/src/components/profiles/profile-dialog.tsx::AddProfileContextValue` 新增 folder 传递；`profile-quick-import.tsx` 增 folder prop；`import-button.tsx` 用非严格 `useMatch` 在 `$type` 与 `folder/$folderId` 两种路由重用现有远程/本地对话框，不另造未受测试的新导入页面。
- **独立页面 ref → 本地路径**：`ref/frontend/nyanpasu/src/pages/(main)/main/profiles/folder/{index.tsx,$folderId/index.tsx,$folderId/_modules/{folder-empty-state.tsx,folder-header.tsx,folder-not-found.tsx}}` → Chimera `frontend/chimera/src/pages/(main)/main/profiles/folder/` 对应五文件；使用现有 `SortableProfileItem` 替代 ref `GridViewProfile` 内建 Sortable，按 `useProfilesDnd` 的 section ID 显示成员，活跃 Config 单独放顶部且不可拖动，带分批渲染/溢出页脚；空文件夹及失效 ID 均有明确视图。`folder-nav-item.tsx` 改为真实 `/main/profiles/folder/$folderId` 链接（取代前一轮总列表 hash 定位），`routeTree.gen.ts` 经项目 Vite TanStack Router 生成器自动产生路径/参数类型，未手工伪造路由 ID。
- **后端测试证据**：`backend/tauri/src/client/profiles.rs` 新增六个隔离测试，覆盖已有文件夹中的 Config 本地新建与磁盘往返、未知文件夹拒绝并维持文件未出现/原字节不变、Transform 禁止加入 Config 文件夹、远程下载成功原子入组/同 revision 持久化、远程未知文件夹拒绝并无写入、**旧版 NewProfileRequest 省略 folder 字段仍反序列化为 None**。整组 `client::profiles::tests::` 使用当前 Nix 环境隔离 glibc 2.44 loader **86/86 通过**（新增 6 项加前轮 80 项）。首次未知文件夹用例失败于测试错误假设“空 Profile 文件已存在”，修正为比较 `Option<Vec<u8>>` 的磁盘状态后完整通过，未掩盖业务失败断言。
- **生成和环境证据**：最初在手动动态加载的 Rust Specta `update_typescript_bindings` 测试中因子进程 `pnpm` 不在继承 PATH 返回 ENOENT，导出文件已完成但格式化未执行；随后使用现有 `nix develop --command pnpm exec prettier --write frontend/interface/src/ipc/bindings.ts` 格式化，并在测试进程显式加入 Node/Corepack store bin PATH 后重新执行官方 `specta_export::tests::typescript_bindings_are_fresh` **1/1 通过**。保留本机 GLIBC 混用问题的记录，不将首次失败标为通过，也不修改生成器、devShell、宿主动态库或 CI 策略。
- **前端与桌面测试边界**：`frontend/interface/tests/profile-folder-mutation.test.ts` 补 remote/default、remote/direct、manual/create 的 IPC folder 参数转发断言；`tauri-e2e/specs/profiles-main-layout.e2e.ts` 加隔离测试：用实际 `create_profile_folder` 为已有合成 Profile 创建文件夹，验证侧栏可导航到独立路由、成员显示和 folder import 入口；`finally` 即使 IPC 返回不确定也按测试独有名称检查落盘，再用 `set_profile_layout` 恢复测试前布局。**当前未实际执行桌面 WebDriver，该用例仅属于待运行的契约**。已执行并通过 `pnpm typecheck`（含 E2E 类型）、`pnpm web:build`（Vite 17,366 模块）、`pnpm lint:oxlint`（0 错误）、`pnpm lint:frontend-boundaries`、`pnpm test:profiles:query`（15/15）、`pnpm --filter=chimera-ui test:profile-layout`（21/21）、`pnpm --filter @chimera/tauri-e2e test:unit`（30/30）和两包限定的 Rustfmt。另外 `cargo clippy --manifest-path backend/Cargo.toml -p chimera --all-targets --all-features` 以状态 0 通过，Rust 依赖和原有项目路径仍有数百条非阻断警告；上述构建与静态检查不能代替真实 GUI 验证。
- **余项/回滚**：真实文件夹页面的快捷导入、表单导入、空文件夹及移动操作仍需在隔离 GUI Runner 测试；远程 import race、下载失败、低内存或 Source CAS 冲突的历史路径保留由 Client 全量回归覆盖，不声称全平台 CI 已过。若回滚本轮，请同时撤销 Actor 关联字段、Client/facade/IPC 文件夹传参、Specta binding、Query/Form 参数及新独立 folder route 和导航，保持前两个已提交的 Folder Layout 数据模型/UI 不变；任何用户实际已建立的文件夹数据不可静默删除，且旧版本可继续读取不含此可选参数的 RPC 调用。

## Profiles Folder Layout 管理界面、分组与统一 DnD 对齐（2026-10-31，已提交 2ebff38c）

- **目标 / 风险 / 非目标**：基于固定只读 `ref` `1b5d73c53a7141d216a8a0a7751be38a295be059`，让此前已提交 `f30c6bce` 的真实 `set_profile_layout` / `create_profile_folder` 在主 Profiles 页可用于创建、重命名、删除、移动、折叠和拖放 Config 文件夹。本批是共享 Query 上的 L2 UI + 持久化行为接线；不更改 Rust/IPC/Service/TUN/Agent，不更改 legacy UI 的现有表现和单独已有调用路径，不访问真实用户资料。`ref`、runtime 子模块及签名策略保持不动；本次不做提交或远程推送。
- **ref 文件映射**：`ref/frontend/nyanpasu/src/pages/(main)/main/_modules/{error-item.tsx,folder-name-dialog.tsx,use-folder-collapsed.ts}` → Chimera 相同目录/文件名（共享组件与持久化折叠状态）。`ref/frontend/nyanpasu/src/pages/(main)/main/profiles/_modules/{profiles-layout-provider.tsx,profiles-dnd-provider.tsx,profile-section.tsx,profile-sections.tsx,profile-move-menu.tsx,folder-actions-menu.tsx,use-profiles-dnd.ts}` → `frontend/chimera/src/pages/(main)/main/profiles/_modules/` 相同目录/名称，保留 `ProfilesLayoutProvider`、`ProfilesDndProvider`、`ProfileSections`、`ProfileSection`、`useProfileMove`、`ProfileMoveContextMenu`、`FolderActionsMenu`、`useProfilesDnd` 及布局/拖放规则。`folder-nav-item.tsx` 使用上游同名模块及 `FOLDER_DRAG_TYPE`/`FOLDER_ITEM_DRAG_SENSORS` 合同，按 Chimera 现有侧栏结构适配；上游 `FolderNavItem` 右键菜单的“上移/下移”合并到本地 `FolderActionsMenu` 溢出菜单，提供不依赖拖拽的可访问排序；`ref/.../$type/_modules/profiles-list.tsx` 对齐 Config → `ProfileSections`、Transform → `TransformList` 分流，保留 Transform 完整 UID 旧排序写入。`ref/.../profile-card.tsx` 的右键 `ProfileMoveContextMenu` 与 `useProfileMove` 接入现有卡片组件；新旧选择器继续使用 `profile-card` / `profile-card-sortable` / `profiles-navigate`。
- **持久化与交互所有权**：主 `profiles/route.tsx` 使用 `ProfilesLayoutProvider` → `ProfilesDndProvider` 包裹侧栏与 Outlet，使跨侧栏/Config 卡片拖放使用唯一的布局事务写入口；`ProfilesLayoutProvider` 复用真实 `useProfile().setLayout/createFolder`、Query optimistic rollback、确认对话框；无效名称由 Zod 表单拒绝。Config 激活卡片按 ref 在分组前单独呈现并不可拖拽；普通 Config 在 folder/ungrouped 分组内复用 Chimera 的 `SortableProfileItem` 外层（而不是替换已稳定的 ProfileCard），加入 `group`/`type`/碰撞检测，Folder item 与分组空区可接收 Config，Transform 列表保留原 `DragDropProvider`/`reorder_profiles_by_list`，不迁移 Transform 进入 folder。折叠状态由原有 `useKvStorage` 保存，仅修改 UI 展示。
- **依赖与国际化**：`@dnd-kit/dom@0.5.1` 与 ref 锁定版本一致，显式加入 Chimera 前端包与 pnpm lock（复用已下载的依赖）；新增 `FOLDER_ITEM_DRAG_SENSORS`，鼠标/笔需移动 8px 才开始侧栏链接拖拽，触摸维持默认长按。五种既有语言从 ref 同名 locale 迁入 `profile_folder_*`、`profile_move_*` 和关联 a11y 菜单字段（不覆盖原有翻译）。Chimera 原 `@chimera/ui`、`lodash-es`、`useLockFn`、本地通知 API 作为精确品牌/接口适配，不新增平行业务逻辑。
- **保留差异与下一轮**：本批侧栏文件夹点击链接到 Config 总列表中的对应 `#folderId` 分组锚点；ref 有 `/main/profiles/folder/$folderId` 独立路由与 folder-scoped import，在 Chimera 当前导入接口尚不支持 `folder` 写入时**暂不伪造该路由或声称导入会自动入组**。`GridViewProfile` 内建 Sortable 与 Chimera `SortableProfileItem` 外包仍存在结构差异；父级 DnD/section/member ID 与 ref 一致。正式桌面发布前必须在隔离 E2E Runner 验证创建→移动→重命名→删除、跨文件夹/空文件夹/折叠 Drop、Folder 拖拽重排、活跃 Config 仍不可拖、视口滚动、侧栏 hash、重启持久化及 Query 失败回滚；本地 TS 构建无法证明这些真实 GUI 行为。
- **执行证据与局限**：已执行 `pnpm typecheck`（含接口、主/legacy UI、E2E）、`pnpm web:build`、`pnpm lint:oxlint`（0 errors）、`pnpm lint:frontend-boundaries`、`pnpm test:profiles:query`（12/12）、`pnpm --filter=chimera-ui test:profile-layout`（21/21）、`pnpm --filter @chimera/tauri-e2e test:unit`（30/30），均通过。`tauri-e2e/specs/profiles-main-layout.e2e.ts` 增加非写入式空文件夹名拒绝及文档快照不变契约，**仅通过类型检查，未实际运行桌面 WebDriver**。Rust 编译和 Actor 事务测试结果见已提交 `f30c6bce` 的记录，未对本轮纯前端改动重新启动桌面应用或宿主网络。
- **回滚**：单独回退本节新增 UI/Provider/DnD/菜单模块、侧栏与 Config 分组接线、五语言资源及依赖声明/锁文件、E2E 断言，即可恢复先前 Config 平铺和 Transform 排序；保持 `f30c6bce` Actor/IPC 领域写入通路不变，无需迁移已存 Profile。若先在此功能上创建了真实文件夹，回滚 UI 不会删除文件夹数据，且应明确告知用户旧 UI 将暂不展示 folder 元数据。不要修改或静默丢弃用户已写入的 `folders` 字段。

## Folder Layout Actor → Client → IPC → Query 完整写入通路（2026-10-31，已提交 3609eeb8）

- **目标与非目标**：沿用只读固定 `ref` `1b5d73c53a7141d216a8a0a7751be38a295be059` 的 Folder Layout 单写入者架构，在 Chimera 已移植的 `Profiles::apply_layout` 基础上打通 Actor 原子写入、typed Client、共享 `ChimeraClient` facade、Tauri IPC、Specta TS 命令与 Query Hook（L2 持久化/API 行为）。本轮不开放新建文件夹/文件夹拖放的 UI，不修改 Runtime/Service/TUN/系统代理，保留旧的 `reorder_profiles_by_list`、legacy UI 和 Agent 消费者；先完成功能承载和隔离回归，不声称 GUI 已对齐。
- **后端映射**：`ref/backend/tauri/src/state/profiles/actor.rs::{SetLayout,CreateFolder,generate_folder_id}` → `backend/tauri/src/state/profiles/actor.rs` 相同符号。`SetLayout` 经既有 `run_state_write` 调用 `Profiles::apply_layout`，在同一受版本保护的 snapshot/actor transaction 中进行完整性验证、revision +1、持久化与后续通知；`CreateFolder` 由 Actor 唯一生成 `f` + 11 位 NanoID，先从旧文件夹移除目标成员，计算剩余 Config `ungrouped`，再调用 `apply_layout`，失败不会产生半成品文件夹。两个操作均用 `AffectsRule::Never`，不重启代理核心。失败/未知成员/重复 UID/Transform/空名字由领域模型拒绝，无写入。Chimera 没有 ref 独立 `Profiles::join_folder` 导入模式，本批保留已有新增/导入行为不改变其存储语义。
- **Client 和 RPC**：`ref/backend/tauri/src/client/profiles.rs::{set_layout,create_folder}` → `backend/tauri/src/client/profiles.rs`；`ref/backend/tauri/src/client/mod.rs::{set_profile_layout,create_profile_folder}` → `backend/tauri/src/client/mod.rs`；`ref/backend/tauri/src/ipc.rs::{set_profile_layout,create_profile_folder}` → `backend/tauri/src/ipc.rs`。两条公开命令在 `backend/tauri/src/specta_export.rs` 注册，写入成功后通过 Chimera 既有 `refresh_profiles` + `MutationOutcome`/CommitReceipt 路径通知 main/legacy UI；FolderId 返回 Actor 生成的值而非前端自造。未改既有 IPC 签名或 Reorder 完整 UID 排序路径。
- **前端 Query 映射**：`ref/frontend/query/src/ipc/profile-folders.ts::{profileSections,profileLayoutOf,applyProfileLayout}` → `frontend/interface/src/ipc/profile-folders.ts` 同名文件、函数、逻辑；在 `frontend/interface/src/ipc/{profile-bindings.ts,use-profile.ts}` 适配 Chimera generated command descriptors 和 React Query Hooks（`setLayout` 使用与 ref 相同的 `profile-layout` scope、乐观缓存、拒绝回滚、最后一次队列 settle 后失效；`createFolder` 成功后失效）。在 `frontend/interface/src/ipc/index.ts` 暴露共享工具。删除前一阶段仅用于过渡的 `frontend/chimera/src/pages/(main)/main/profiles/_modules/profile-layout-types.ts`，布局/拖放规则与测试均改为直接使用 Specta 生成的 `FolderId`、`ProfileFolder`、`ProfileLayout`、`ProfileId` 类型。后续 `ProfilesLayoutProvider` 可直接使用同一 hook 与上下文契约，不需另建数据桥接。
- **生成与测试**：Rust `specta_export::tests::update_typescript_bindings` 与 `typescript_bindings_are_fresh` 在本机一致的 glibc 2.44 loader + `strace -f -qq -e trace=execve` 子进程环境中均实际通过（各 1/1），生成 `frontend/interface/src/ipc/bindings.ts`，包含两个命令和 `ProfileLayout` 类型、已有 Folder 字段，项目 Prettier 格式化成功。`backend/tauri/src/client/profiles.rs` 新增七项隔离 Actor/Client 测试：set_layout 顺序及磁盘往返、无效布局五类输入原字节不变、跨文件夹移动、新 folder id/移动、创建时拒绝非法输入、删除成员及旧排序与文件夹布局共存。使用 Nix glibc 2.44 loader 仅执行**测试进程**后，新增 7/7 通过，原旧排序 1/1、原删除回归 9/9；此外整体 `client::profiles::tests::` 80/80 通过。`pnpm test:profiles:query` 已登记三份测试，共 12/12；`pnpm --filter=chimera-ui test:profile-layout` 21/21。完整 TypeScript、Vite 构建、Oxlint、前端边界检查也通过。新命令本轮尚未进行 GUI/WebDriver 实机操作与全平台测试。
- **测试环境兼容**：当前 Nix 开发 shell 编译 `chimera` crate 成功，但默认测试二进制解释器来自 glibc 2.42，而部分新 GTK/WebKit `.so` 要求 2.43/2.44；直接运行发生动态库加载失败（不是单测断言失败）。只对测试进程临时使用现有 `/nix/store/...glibc-2.44.../lib/ld-linux-x86-64.so.2 --library-path <glibc-2.44-lib>:$LD_LIBRARY_PATH`，所有新增及选定旧测试通过。首次手动 loader 运行 Specta 测试时 `std::process::Command::new("pnpm")` 子进程返回 ENOENT；同一 loader 命令加 `strace -f -qq -e trace=execve` 后，正式 `update_typescript_bindings` 和 `typescript_bindings_are_fresh` 均返回 0，生成产物无手工更改。这是本机工具链启动问题，尚未定位为何 `strace` 会影响可复现性；不能声称基础运行环境已经修复，也不能替代 Windows/macOS/Linux CI。
- **保留差异、后续、回滚**：仍未迁入 `ref/.../_modules/{profiles-layout-provider.tsx,profile-sections.tsx,profiles-dnd-provider.tsx,folder-name-dialog.tsx}` 以及侧栏文件夹操作；不能把后端可写入接口误说为已完成 UI。后续在隔离 E2E Runner 检验多文件夹跨区拖放、创建/重命名/删除、同时旧 UI 排序、重启持久化、快速连续 mutations/错误回滚和通知。回滚需同时撤销 Actor/Client/IPC/生成绑定、Query Hooks/测试和之前 `folder.rs` 持久化字段，先备份任何已存在于用户文档的真实 Folder 元数据，避免旧版本忽略字段造成丢失；本轮未升级 schema_revision、未触碰真实用户 Profile 文档或 Host 网络。

## Folder Layout Rust 配置领域模型与启动修复（2026-10-31，已提交 3609eeb8）

- **基线/范围/风险**：固定只读 `ref` `1b5d73c53a7141d216a8a0a7751be38a295be059`。将 Folder Layout 领域模型、序列化校验、布局原子应用、启动容错修复迁至 Chimera 配置层和 ProfilesClient 加载路径。L2 涉及可选持久化字段与 Domain Type；**领域层迁入时暂未开放文件夹 UI/写入；后续已在本文件上一节接入后端 Actor/IPC 写入接口，但 UI 仍未开放**，继续保护用户现存数据。未改动 legacy UI/agent 调用、代理/TUN、运行时构建、第三方依赖或 `ref`。后续需通过专项 E2E 再启用真正的文件夹 UI。
- **ref 目录/符号映射**：`ref/backend/nyanpasu-config/src/profile/{folder.rs,id.rs,profiles.rs,mod.rs,tests/folders.rs}` → `backend/chimera-config/src/profile/` 对应文件，新增 `FolderId`、`ProfileFolder`、`ProfileLayout`、`FolderRepairReport`、`Profiles::apply_layout`、`Profiles::repair_folders`、`validate_folders` 及对应 `ProfileValidationError`；原 `Profiles` 添加 `#[serde(default, skip_serializing_if = "Vec::is_empty")] folders`，`ProfileDocument` 添加 Specta optional 字段，不替换已有 `current`、`items`、`revision`、`global_transforms`。
- **兼容边界**：旧 `profiles.yaml` 未含 `folders` 时反序列化为空数组且重新保存不写出字段；合法布局中 Config 必须恰好出现一次，文件夹不可存 Transform，非法布局应用会保持原 `items`/`folders` 不变。应用布局后的 Transform 保持相对顺序。Chimera 的 `remove_item_unchecked` 额外清掉已删除 UID 的 folder membership（上游删除动作在 actor 路径实现，Chimera 当前尚无相同调用链）；所有既有删除入口继续走原来的原子 Commit 流程。
- **持久化修复**：`backend/tauri/src/client/profiles.rs::ProfilesClient::build` 参照 ref 在校验前读取快照，仅对不合法的文件夹元数据执行 `repair_folders`，写入时先 `bump_revision()` 后 `manager.upsert()`，遇到写入失败则阻止启动，不静默忽略。修复只处理文件夹 presentation metadata，不改变 Config/Transform 项目顺序，也不替代应用主体 `validate()`。这一步涉及启动时落盘，须使用隔离 fixture 验证真实修复结果。
- **测试/证据**：`backend/chimera-config/src/profile/tests/folders.rs` 复用 ref 23 项领域测试，再增删除 Config 同步移除成员的回归用例；已执行 `cargo test --manifest-path backend/Cargo.toml -p chimera-config --lib`，172/172 通过。`backend/tauri/src/client/profiles.rs` 新增 `startup_repairs_invalid_profile_folders_and_persists_the_repair`，断言加载、修复、durable revision +1、文件落盘；已执行 `cargo test --manifest-path backend/Cargo.toml -p chimera --lib startup_repairs_invalid_profile_folders_and_persists_the_repair -- --nocapture`（1/1 通过），实际隔离写回 revision 8，保留有效 `cfg1` 并删除重复文件夹及 `ghost`。已通过 `specta_export::tests::update_typescript_bindings` 生成 `frontend/interface/src/ipc/bindings.ts`，新增 `FolderId`、`ProfileFolder` 与 optional `folders`，无需手工改动生成文件。
- **未迁移的路径**：ref 的 `ref/backend/tauri/src/state/profiles/actor.rs::{SetLayout,CreateFolder}`、`ref/backend/tauri/src/client/profiles.rs::{set_layout,create_folder}`、`ref/backend/tauri/src/ipc.rs::{set_profile_layout,create_profile_folder}`、`ProfilesLayoutProvider` 和 folder UI 尚未接通。Chimera 仍由 `reorder_profiles_by_list` 写入顺序，原有 Profile 列表和兼容 legacy UI/agent 不使用新增 Folder 字段；不要把该阶段宣称为可用文件夹 UI。下一阶段必须同时复核新旧排序相容性、revision/MVCC、删除和导入、配置升级及 GUI E2E。
- **回滚**：若未发布/未写入真实用户文件，可整体撤销 Rust 新增的 `folder.rs`、`FolderId`、可选 `folders` 字段、加载修复与测试，并撤回对应生成类型。若已有版本写入过 `folders`，降级前应保留/备份原文档，不能把被旧版本忽略的用户文件夹布局当作可安全无损删除。暂不修改 `profiles.yaml` schema_revision 及迁移器。

## Folder Layout 纯布局编辑、拖放落点与上下文契约迁移（2026-10-31，已提交 3609eeb8）

- **目标 / 非目标 / 风险**：从固定只读 `ref` `1b5d73c53a7141d216a8a0a7751be38a295be059` 迁入可独立验证的 Folder Layout 纯逻辑与上下文契约，为未来 `ProfileSections` 的持久化架构对齐打基础。本批只新增尚未接入页面的前端模块与测试（L1，预备阶段），**不启用创建/编辑/拖放文件夹的用户操作**，不假装完成 Folder UI/后端迁移，不更改当前 Config/Transform 排序、用户配置、IPC 或服务/Agent 接口。
- **ref → Chimera 路径/符号映射**：`ref/frontend/nyanpasu/src/pages/(main)/main/profiles/_modules/profile-layout.ts` → `frontend/chimera/src/pages/(main)/main/profiles/_modules/profile-layout.ts`，保留 `isSameLayout`、`moveProfileInLayout`、`renameFolderInLayout`、`removeFolderFromLayout` 和 `reorderFoldersInLayout`；`ref/.../_modules/resolve-profile-drop.ts` → Chimera 同名文件，保留 `sectionId`、`sidebarFolderId`、`sectionItemsOf`、`withoutProfile`、`appendDestinationOf`、`resolveProfileDrop`；`ref/.../_modules/use-profile-layout.ts` → Chimera 同名 context/hook，**当前未挂载 Provider，后续同本文件上一节已完成公共 Query Hook 接线**。
- **临时类型桥接**：`frontend/chimera/src/pages/(main)/main/profiles/_modules/profile-layout-types.ts` **直接复用** Specta 生成的 `FolderId`、`ProfileFolder` 与 `ProfileId`，只定义当前 IPC 尚未暴露的 `ProfileLayout {folders,ungrouped}` 组合类型；`frontend/interface/src/ipc/bindings.ts` 已由项目生成器同步。`isSameLayout` 使用已有 `lodash-es` 的 `isEqual` 兼容 ref 的 `es-toolkit` 等值语义，避免无关依赖变化。**完成条件已满足**：后端 `set_profile_layout` 已进入 Specta，所有模块已直接引用公共类型，桥接文件已删除。
- **已证实的上游边界修正**：ref 的 `moveProfileInLayout` 对未知 Profile UID 会直接插入目标文件夹，违反其源代码注释“未知 profile 不修改布局”及 Rust `Profiles::apply_layout` 的“未知/重复/缺失 Config 必须拒绝”的校验规则。`frontend/chimera/tests/profile-layout.test.ts` 中未知 UID 断言在初始逐字迁入时确实失败（21 项中 20 通过、1 失败），记录实际新增 `not-loaded`；Chimera 仅增加一个存在性短路条件，输入不存在于 folders/ungrouped 就返回原布局。修正后 21/21 通过；这是有失败复现的最小 ref 缺陷例外，后续上游修复后重新检查并取消差异。
- **测试与覆盖**：基于 ref 的 `tests/profile-layout.test.ts`、`tests/resolve-profile-drop.test.ts` 复刻为本地 `frontend/chimera/tests/{profile-layout,resolve-profile-drop}.test.ts`，改用既有 `node:test`/`assert`，通过 `pnpm --filter=chimera-ui test:profile-layout` 运行 21 项，覆盖跨文件夹移动、同文件夹排序、无效目标、不变式、删除/重命名、侧栏/折叠项落点、隐藏活跃 Profile 后的索引映射。纯函数测试不证明 UI/IPC/持久化。
- **未迁移的业务架构及批准边界**：ref 的 `ref/backend/nyanpasu-config/src/profile/folder.rs::Profiles::apply_layout` 是权威校验与原子排序所有者，上层还有 `ref/backend/tauri/src/client/profiles.rs::set_layout`、`ref/backend/tauri/src/ipc.rs::set_profile_layout/create_profile_folder`、RPC 生成类型和前端 `ProfilesLayoutProvider`、`ProfileSections`、`ProfilesDndProvider`。Chimera 只有 `reorder_profiles_by_list`，目前没有同类命令和 generated Folder 类型。下一阶段须整条调用链迁移，保护旧配置升级、Profile 精确一次归属、Transform 相对顺序、失败回滚、legacy UI 和 agent 对共享业务的访问，并在隔离桌面 Runner 测试文件夹拖放及数据持久化。涉及跨层架构/持久化状态所有权变更，需先完成设计审查和相应授权，不能将本批纯函数接入伪持久化界面。
- **回滚**：撤销本节四个新模块（包括本地类型桥接）、两个纯单测、package script 和文档段落即可；原有 Profile 网格/UID 全量排序和后端用户数据完全未受影响。本批与上一节尚未提交的渐进渲染改动分别说明，可独立撤销。

## Profiles 列表渐进渲染与溢出页脚对齐（2026-10-31，已提交 f30c6bce）

- **目标与验收**：以固定只读 `ref` `1b5d73c53a7141d216a8a0a7751be38a295be059` 对齐 Profiles 网格的分批挂载、尾部提示和滚动尺寸检测；已有 Profile 激活、拖拽完整排序、右键菜单、详情导航及屏幕尺寸 E2E 定位点保持有效。L1 纯前端视图/性能变更。
- **上游路径/符号映射**：`ref/frontend/nyanpasu/src/pages/(main)/main/profiles/_modules/use-progressive-count.ts` → `frontend/chimera/src/pages/(main)/main/profiles/_modules/use-progressive-count.ts`，保留 `useProgressiveCount(total)`、每批 6 张、`startTransition`；`ref/.../_modules/profiles-list-footer.tsx` → Chimera 同名文件，保留 `NoMoreProfiles` 与 `useListOverflows`，仅将 `@nyanpasu/ui/scroll-area` 和 i18n 导入映射至 Chimera 包。
- **集成**：`frontend/chimera/src/pages/(main)/main/profiles/$type/_modules/profiles-list.tsx` 新增 `gridRef`，按 `filteredProfiles.slice(0, shown)` 分批呈现，使用现有 `AppContentScrollArea` 提供的 `useScrollAreaViewport` 测量是否超出滚动视口，只有实际溢出才显示 `NoMoreProfiles`。保持完整 Profile 集合参与 `reorderProfilesByList` 写入，绝不将当前渲染的部分 UID 提交给后端。保留原 `data-slot="profiles-navigate"`，增加 `data-rendered-count` 和 `data-total-count` 供桌面回归定位。
- **保留差异与后续收敛**：上游 Config 使用 `ProfileSections` 和持久化 Folder Layout，Chimera 仍在原 `$type/_modules/profiles-list.tsx` 内使用统一列表、额外外层 `SortableProfileItem` 与现有全量排序 IPC。该差异涉及 Folder Layout 的数据模型、事务/存储兼容及排序所有权，不在本批仅为列表性能调整时强行引入。待完整梳理布局存储、旧数据升级、legacy UI/Agent 消费者与桌面拖拽验收后再迁移该架构。
- **验证**：实际执行 `pnpm typecheck`、`pnpm web:build`、`pnpm lint:oxlint`、`pnpm lint:frontend-boundaries`、`pnpm test:profiles:query`（3/3）及 `pnpm --filter @chimera/tauri-e2e test:unit`（30/30），全部通过。`tauri-e2e/specs/profiles-main-layout.e2e.ts` 添加以独立 `get_profiles` 只读 IPC 为期望、等待所有配置卡片挂载的受限桌面契约；该 E2E **只经 TypeScript 检查，未实际执行**，不能当作桌面渲染速度、排序、滚动/页脚和实机性能验收。
- **边界、风险与回滚**：不改 IPC、持久化结构、运行时、Client/Service、系统代理、legacy UI、Agent、`ref` gitlink 或依赖。若分批渲染影响鼠标拖动与视口定位，可撤回新增两个模块、列表接线和 E2E 断言，还原全量渲染/原始页脚；无需迁移用户数据。后续在隔离 Windows 桌面 Runner 上补测超出 6 张卡片的渲染、列表溢出显示/隐藏、拖拽重排和激活状态。

## Profile 卡片活跃背景与操作入口对齐（2026-10-31，已提交 1ec76701）

- 目标：在固定只读 `ref` gitlink `1b5d73c53a7141d216a8a0a7751be38a295be059` 基础上继续对齐 ProfileCard 的活跃背景、底部操作入口和可拖动状态，尽量沿用 upstream 的文件路径与实现，而不替换 Chimera 已稳定的 Profile 排序/持久化层。
- 文件映射：`ref/frontend/nyanpasu/src/pages/(main)/main/profiles/_modules/{active-gradient.tsx,mesh-gradient.ts}` → `frontend/chimera/src/pages/(main)/main/profiles/_modules/` 下同名文件；复用 `@material/material-color-utilities` 与现有 `useExperimentalThemeContext()`。渐变使用低分辨率 2D canvas/CPU 绘制，仅在 hover 或 focus 时按 ref 的 30fps 节流动画；不引入 WebGL/context shader 编译。Chimera 的 `profile-card-active-background` 底层装饰和测试定位点继续保留，新增 `profile-card-gradient`。
- 操作区：在 `profile-card.tsx` 接入 ref 的非活跃 Config 激活按钮、Info 图标详情链接、原有 ViewContent/订阅更新入口与 Tooltip，继续调用 Chimera `useActiveProfile()` 和原详情路由；新增 `profile-card-activate`/`profile-card-details`。ProfileCardFeatures 的 `compact` 参数与 ref 保持一致，以减少非活跃 Config 卡片的信息密度。
- 拖拽边界：不将 dnd-kit Sortable 从现有 `$type/_modules/sortable-profile-item.tsx` 包装层移到卡片内，保留排序完成时提交完整 UID 序列。Profile 列表使用项目既有 `WHOLE_ITEM_DRAG_SENSORS`；激活中的 Profile 与 ref 一样禁止拖动。外层包装增加 `profile-card-sortable`、`data-dragging` 与 `data-drag-disabled` 供真实桌面回归定位。
- 测试：从 ref `tests/mesh-gradient.test.ts` 迁入并扩展纯函数覆盖至 `frontend/chimera/tests/mesh-gradient.test.ts`（3/3，通过 `pnpm --filter=chimera-ui test:profile-card-gradient` 运行）。现有 `tauri-e2e/specs/profiles-main-layout.e2e.ts` 增加非破坏性的控件可访问标签、渐变存在性和活跃 Profile 禁拖动断言；这些桌面断言只编译检查，**未实际执行 Tauri GUI E2E**。
- 兼容、风险及回滚：L1 视图/交互变化；继续遵守当前 1240×638 布局 E2E 的卡片高度不超过 260px 要求，配置卡片保持 240px，保留内容区滚动、原活动状态和持久化接口。纯前端，无 IPC schema、数据文件、legacy UI、Agent、网络或 `ref` 改动。回滚可删除两个渐变模块和新增测试、撤回卡片按钮/排序传感器改动，不触及用户数据。待有隔离的桌面 E2E Runner 后再实测点击、拖拽、焦点动画和布局尺寸。

## Profile 卡片 YAML 静态内容统计对齐（2026-10-31，已提交 bb342943）

- 基线：只读固定 `ref` `1b5d73c53a7141d216a8a0a7751be38a295be059`。按相同路径/文件名迁入 `ref/frontend/nyanpasu/src/pages/(main)/main/profiles/_modules/{profile-content-summary.ts,profile-content-summary.worker.ts,use-profile-content-summary.ts,profile-card-content-summary.tsx}` → `frontend/chimera/src/pages/(main)/main/profiles/_modules/`。保留 YAML 静态 `proxies`、`proxy-groups`、`rules` 数量统计，**不扩展** providers 或 runtime transforms。
- 端到端接线：`profile-card-subscription.tsx` 的 `ProfileCardTraffic` 仅对有磁盘文件的 Config 调用 `ProfileCardContentSummary`；通过 Chimera 原有 `useProfileContent(uid)` 使用已存在的 `readProfileFile` IPC 缓存，以 Web Worker 离线解析大文件；同 UID 和同内容在当前会话只汇总一次，内容变化后重新汇总。未知/不可解析时以 `—` 显示而非伪造零数量。Tooltip 以现有 `@chimera/ui/tooltip` 代替 ref `SharedTooltipTrigger`，图标、标签及 `profile-card-content-summary` 定位点保留。
- 依赖与 i18n：为前端新增精确固定 `yaml@2.9.1` 显式依赖，离线更新 `pnpm-lock.yaml`，并移植 ref 的英、日、俄、简中、繁中五种语言静态计数/提示消息键。
- 测试：新增 `frontend/chimera/tests/profile-content-summary.test.ts` 与 `pnpm --filter=chimera-ui test:profile-content-summary`。4 项测试覆盖静态计数、空映射、错误/重复键/不支持的字段结构和 YAML sequence alias，并对齐上游明确写为 null 的 scalar 应返回 unknown。`pnpm typecheck`、`pnpm web:build`、`pnpm lint:oxlint`、`pnpm lint:frontend-boundaries` 均通过；生产构建输出独立的 summary worker 资源。
- 兼容与回滚：L1 纯前端统计/已有文件读取；保留全局/Scoped Transform 拖拽、现有卡片高度 240px 的独立滚动区域、后端 API/存储/Agent/legacy UI 与 `ref` gitlink。撤销这四个模块、卡片接线、依赖、翻译和测试即可回滚。尚未执行真实桌面卡片点击、滚动、排序/窗口 E2E；构建和单测不能替代它们。

## Profiles 卡片订阅与流量展示对齐（2026-10-31，已提交 bb342943）

- 目标：基于固定只读 `ref` `1b5d73c53a7141d216a8a0a7751be38a295be059`，对齐 Profiles 的 `profile-card-subscription.tsx`：远程文件订阅显示剩余额度/总量/超额/配额百分比，提供订阅更新按钮；本地 Config 显示最近 24 小时流量和可用的上下行细分。保持 Transform、Composition、远程/本地类型区分及现有配置查询。
- 上游映射：`ref/frontend/nyanpasu/src/pages/(main)/main/profiles/_modules/profile-card-subscription.tsx` → Chimera 对应 `_modules/` 同名文件；`ref/frontend/nyanpasu/src/components/widgets/dashboard-daily-utils.ts` 中 `SubscriptionQuota`、`calculateSubscriptionQuota`、`subscriptionExpiryTimestamp` → `frontend/chimera/src/components/widgets/dashboard-daily-utils.ts` 的同名纯函数（仅迁入本批需要的部分），单测位于 `frontend/chimera/tests/dashboard-daily-utils.test.ts`，运行 `pnpm --filter=chimera-ui test:profile-card-quota`。
- 业务适配：沿用 Chimera 的 `useProfile().update`、`useBlockTask`、错误通知和 `useTrafficReport`，保留 10 秒流量轮询与 ref 的 `last24_hours`/Profile filter；Tooltip 复用 Chimera UI，过期日期使用现有 `formatDate` 提取日期部分。Profile 卡片保留 `data-slot` 及激活/编辑/详情入口。考虑当前 `profiles-main-layout.e2e.ts` 的高度不超过 260px 的断言，Config 卡片限定为 240px、指标区滚动、标题与操作按钮固定可见。
- 国际化：从 ref 同步五种语言的订阅与流量 11 个消息键，包含流量上下行带数值的文案。
- 保留差异：原先尚未迁入上游 `profile-card-content-summary.tsx`/worker 的 YAML 文件静态计数，现已在独立批次迁入（见本文件前一节）；Chimera 的 Profile card 仍沿用现有双列导航布局及局部活跃态样式。无新增后端 IPC、持久化键或主机网络操作，也不触碰 `ref` gitlink。
- 风险及回滚：L1 卡片展示、新增流量查询消费者和手动更新操作；撤销该组件、额度辅助函数、测试与卡片集成即可回滚，不删除任何用户数据。配额/过期日期单测 4/4、完整 TypeScript、Vite build、Oxlint、前端边界与国际化测试均通过；现有桌面卡片高度和真实订阅按钮/拖拽交互尚未在 GUI E2E 实测，不能以编译或工具单测替代。新测试已注册本地 npm 脚本，未擅自修改 CI workflow。

## Profiles 卡片功能标签对齐（2026-10-31，已提交 bb342943）

- 目标：以固定只读 `ref` 提交 `1b5d73c53a7141d216a8a0a7751be38a295be059` 为基线，将 `ref/frontend/nyanpasu/src/pages/(main)/main/profiles/_modules/profile-card-features.tsx` 以同路径迁入 Chimera。保留上游 `ProfileCardFeatures`、`ProfileCardChip`、`sourceLabelOf`、更新周期紧凑标签与 scoped transforms 详情 Tooltip，按等价的本地接口、消息模块与组件包适配。
- 接入位置：`frontend/chimera/src/pages/(main)/main/profiles/_modules/profile-card.tsx` 由原单一类型标签改用 `ProfileCardFeatures`，并保留活动 Profile 提示、卡片内容编辑按钮、详情链接和 `data-slot="profile-card"` 等测试定位点。`$type/_modules/profiles-list.tsx` 将现有查询的全部 Profile 集合和全局 Transform 标识传给 `sortable-profile-item.tsx`，不在每张卡片额外发起网络请求。
- 国际化：从 ref 为英语、日语、简中、繁中和俄语补入来源类型、更新周期、Composition 来源数量、Scoped Transform 数量、全局使用标记的 10 个文案键。
- 保留差异：Chimera 的双列 Transform 编辑器、卡片布局、原活跃状态背景、上下文菜单与文件编辑窗口操作保持不变；ref 的订阅配额、流量统计及卡片汇总仍待独立迁移，不因此批迁移冒充已经完成。不存在后端 IPC、持久化格式或主机网络改动。
- 风险及回滚：L1 UI 标签展示及 tooltip 交互；可撤回新增模块、列表参数及翻译键恢复原类型标签。完整 TypeScript、Vite 构建、Oxlint 与前端边界检查通过；真实桌面卡片/拖拽 E2E 尚未执行。

## Profile 卡片文件入口与公共组件路径对齐（2026-10-30，已提交 970b5058）

- 目标：固定 `ref` gitlink `1b5d73c53a7141d216a8a0a7751be38a295be059`，对齐 Profiles 公共 `view-content.tsx`、`profile-card-action.tsx` 的目录/职责，在 ProfileCard 卡片上提供 ref 同类的文件编辑入口。只处理前端展示与文件操作复用，不改主/legacy UI 共享后端、Agent、持久化或宿主网络。
- 路径/符号映射：`ref/frontend/nyanpasu/src/pages/(main)/main/profiles/_modules/{view-content,profile-card-action}.tsx` → `frontend/chimera/src/pages/(main)/main/profiles/_modules/` 对应文件；`profile-card.tsx` 继续保留原卡片主体、详情链接、右键菜单与活跃状态。旧 `profiles/$type/detail/_modules/view-content.tsx` 迁至公共模块，未保留重复实现。
- 兼容边界：Chimera `ViewContent` 仍使用现有 `commands.createEditorWindow`、`unwrapResult`、`useBlockTask`、`useLockFn` 和通知机制；ref 对应实现直接用 `rpc.createEditorWindow`。ref 的 `SharedTooltipTrigger` 未在 Chimera UI 包提供，`ProfileCardAction` 使用现有 `Tooltip/TooltipTrigger/TooltipContent` 做窄适配。只有 `hasMaterializedFile(profile)` 为真时显示内容编辑按钮，避免 composition 配置触发无文件异常。原有 `profile-card` 和详情入口定位属性不变。
- 风险与回滚：L1 UI 增加卡片文件入口；不改 IPC 类型/用户存储，撤销卡片按钮和移动的公共组件即可回滚。真实桌面点击/窗口新建的 E2E 仍需补验；typecheck 和 web build 不能代替桌面交互验证。

## Transform Chain 行组件对齐（2026-10-30，已提交 c5306307）

- 目标：按固定 `ref` (`1b5d73c53a7141d216a8a0a7751be38a295be059`) 的文件名、目录与源码结构，移植 Profiles 公共 `step-list.tsx`、`compact-controls.tsx` 和 `transform-type-icon.tsx`；只将 `@nyanpasu/ui`、`@nyanpasu/utils` 映射到同等的 Chimera 包，并应用仓库 Prettier 格式。
- 非目标：不重写为 ref 的单列 `TransformChain`，不改动全局/单 Profile 草稿、拖拽排序、保存、运行时诊断或既有后端、IPC、用户数据、legacy UI、Agent 及宿主网络。
- 参考映射：`ref/frontend/nyanpasu/src/pages/(main)/main/profiles/_modules/{step-list,compact-controls,transform-type-icon}.tsx` → `frontend/chimera/src/pages/(main)/main/profiles/_modules/` 下同名文件；`transform-chain-column.tsx` 的分组布局使用 `StageLabel`、`StepGroup`、`StepRowIcon`、`StepRowText`、`stepRowVariants` 和 `TransformTypeIcon`，原有增删/移动按钮改用 ref 的 `RowAction` 提供标签及 Tooltip。
- 保留差异：Chimera 编辑器仍有启用/未启用两列、跨列拖放、逐项运行日志和诊断失败提示；上游 ref 是单列 enabled list。原有 `data-slot` 选择器、可用性约束与保存/重置入口保持不变。待真实桌面拖拽和按钮回归验证后，再评估将 `transform-chain-column.tsx` 收敛到 ref 的 `transform-chain-list.tsx` 内部结构。
- 影响与风险：L1 前端视图/交互变化；无持久化 schema、共享 IPC 或系统副作用。回滚只需撤回这三个公共模块和列 UI 适配，不删除用户数据或改动 `ref` gitlink。
- 实际验证：`pnpm typecheck`、`pnpm web:build`、`pnpm lint:oxlint`、`pnpm lint:frontend-boundaries`、改动文件 Prettier 和 `git diff --check` 均通过；E2E 工具单测 30/30、Profile 同步历史 5/5、Profile 查询 3/3 通过。未在本机执行真实桌面 UI/拖拽 E2E，不能将已有单测视作其替代。

## 当前迭代目标（2026-10-27）

- 按固定本地 `ref/` 逐批将 Chimera 共享实现对齐上游。每批开始先同步远端 `master`、记录基线和符号映射；优先整段复制完整、相互依赖的上游模块，再做必要且可审查的品牌/平台适配，避免为缩小 diff 拆散迁移。
- 先编译并运行目标模块的定向单测；修复编译/测试问题后，再补适用的主 UI、legacy UI、Agent 覆盖和简单的隔离桌面 E2E（优先用真实控件点击验证）。明确区分编译、模拟边界测试、桌面 E2E 与真实宿主副作用验证，不把前几者说成后者。
- 按用户本轮明确要求直接在当前本地 `master` 逐批推进，不新建功能分支或 worktree。每个提交保持单一清晰目的并签名；本地目标检查通过后推送 `master`，再按准确 SHA 查看 CI/E2E。远端检查 pending 时继续其他独立且已授权的工作，不将 pending 当作成功；发现失败就在 `master` 修复并重新验证。
- 对用户指定合并的旧分支，先独立复核其独有差异；若源码已在 `master`、其余改动会回退较新依赖或基线，则只记录分支已包含，不重复覆盖。不得借此处理无关分支或改动用户数据、`ref/` 与 host networking。
- Codex 当前 Goal 接口只提供读取与状态更新，不提供编辑目标描述；本节作为该持续迭代任务的可维护目标说明，不能替代或谎称已修改原 Goal 对象。

## 参考基线同步（2026-10-29）

- 上游：`https://github.com/libnyanpasu/clash-nyanpasu` 的 `main`，本次通过 SSH 获取并检出 `2fadf212f161417dcdf2f37d81a305a37ff918a4`。旧 Chimera gitlink 为 `8e1084e4e7c50531a5d16ca9a6c9dd2bb2f0f8e8`，新提交的 parent 与旧 gitlink 一致。上游提交仅更新 `@tanstack/react-virtual` 3.14.23→3.14.24 及锁文件中的 `@tanstack/virtual-core` 3.19.8→3.19.9。
- 更新后的 ref 根工作区干净；递归子模块分别为 `backend/enable-loopback` (`f1b9f8693a328ca0a6094951035b81fe302b0848`)、`vendor/windows-rs` (`d1574fd27371a9b08bc5508f70c43ccb4f84c543`)、`backend/nyanpasu-runtime` (`e73b9f5cc2aa91ac74b4c71d0acc5b48e0fd2b34`)、`nyanpasu-utils` (`2706c1b69a3b5e4f0367fb54b4a87e67769c19f8`)，均已检出。上游签名在本机无法核验：本地 GPG keyring 缺少签名者公钥；因此不声称其签名已验证。
- 本批只更新 `ref` gitlink 和本指南的基线记录，不移植依赖或产品代码；不涉及运行时行为、IPC、持久化或主机网络。检查：SSH fetch 成功、ref HEAD 与目标 SHA 相同、ref 状态干净、`git diff --check` 通过。产品构建与桌面 E2E 不适用。
- 风险：L2（参考基线变化，影响后续对齐来源）。回滚时将 gitlink 恢复到 `8e1084e4e7c50531a5d16ca9a6c9dd2bb2f0f8e8` 并还原本节和指南基线。

## 参考基线同步（2026-10-29，后续提交）

- 上游：Clash Nyanpasu `main` 更新到 `2c017d63bfec720017a9b57db629eeddd2c9cd3d`，其 parent 是原基线 `2fadf212f161417dcdf2f37d81a305a37ff918a4`。GitHub API 确认该提交为 `main` 当前 tip 且签名有效；本地通过 `origin/main` fetch 得到相同 SHA。父仓库 `ref` gitlink 从 `2fadf212` 更新到该提交。
- 上游变化：只更新 `meta-json-schema` 1.19.32→1.19.33 及锁文件；`backend/nyanpasu-geodata`、`backend/tauri/src/core/geo` 和 `backend/tauri/src/core/traffic/geo.rs` 均未改变。`backend/nyanpasu-runtime` 仍为 `e73b9f5cc2aa91ac74b4c71d0acc5b48e0fd2b34`。
- 验证：fetch 成功；ref 根目录和两个已检出的子模块均干净；GitHub API 签名状态为 valid；`git diff --check` 与文档格式检查待提交前执行。本批只更新参考基线记录和 gitlink，不改变 Chimera 运行时代码、持久化、IPC 或主机网络行为。
- 风险：L2（参考基线变化，影响后续对齐来源）。回滚时将 gitlink 恢复到 `2fadf212f161417dcdf2f37d81a305a37ff918a4`，并撤回本节和指南元数据更新。

## GeoIP/traffic 候选分支复核（2026-10-29）

- 候选：`codex/geodata-traffic-20261020`，tip `fec56bff4eb98b880282f94713cd763c230282a4`。独立复核确认该分支的 GeoIP crate、traffic/core 接入和测试代码已与 master 中的 `d9624314486e45c905876242dee5d3aacd161cba`（PR #505）相同；该提交已是 master 的祖先。
- 其余差异只有参考对齐文档、旧依赖版本及 `ref` gitlink。分支的依赖比当前 master 低，文档回退到更旧基线，gitlink 也比 master 旧；不将这些过时内容覆盖到 master。GeoIP 功能没有待补的独有源码改动。
- 独立复核未发现 GeoIP 实现 P0–P2 阻塞；记录的保留差异是 ref 未启用本机公网 IP 探测，因此本地/私有源地址可显示为 unknown。该功能的必要源码和测试已由 #505 集成；本次只处理候选分支历史归属，不再重复应用代码。

## Dashboard resize settle 对齐（PR #534，修复复核发现后重新验证中）

- 目标：对齐 Clash Nyanpasu `97e43308edabc95677c65741c81db1eeeb51689a` 的 Dashboard resize settling：窗口连续变化期间维持当前布局来源，尺寸稳定 200 ms 后才选择新预设布局；首次测得网格尺寸仍立即初始化。布局选择延迟撞上拖拽/调整大小时，等当前交互结束再应用。
- 非目标：本批不迁移 Dashboard 布局存储 schema、preferred layout id、其他 widget、DnD Grid API、legacy UI 或 agent；不写入 resize 产生的自适应投影。
- 参考映射：`ref/frontend/nyanpasu/src/pages/(main)/main/dashboard/index.tsx::{SETTLED_RESIZE_MS,selectForSize,settledTimerRef,handleSizeChange}` → `frontend/chimera/src/pages/(main)/main/dashboard/index.tsx` 中的相同常量/职责；保留 Chimera 当前 `findBestLayout`、`findClosestStoredLayout`、`adaptLayout` 与 keyed `LayoutStorage`。
- 接口与风险：新增 `DndGrid.onInteractionChange` 可选回调，报告网格拖拽/resize 的开始与结束，由 Dashboard 作为本地 UI 状态消费；无 IPC 或持久化格式变化。L1 局部 UI 交互。resize 投影只更新显示态，settled selection 不写存储。
- 验收：桌面 E2E 用快照/恢复的确定布局验证宽屏与窄屏选择、宽屏恢复及 `dashboard-widgets` 不变；timer 在网格交互活跃时不得切换布局。typecheck、格式/静态检查与目标桌面 E2E 通过。
- 回滚：撤销 Dashboard resize timer 与对应 E2E/contract/progress 记录，不改用户数据。
- 审批触发：不涉及架构或数据迁移；按用户对日常迭代的持续授权推进，不需要额外审批。
- 实际验证：初版 `9827e35f` 的 `pnpm typecheck:chimera`、`pnpm typecheck:e2e`、改动文件 Oxlint、Prettier 和 `git diff --check` 通过；独立复核发现交互期间布局切换竞态，以及首版 E2E 未断言窄屏实际布局。两项已修复，后续 SHA 的定向检查和 Windows hermetic E2E 待完成。本机桌面构建未运行：构建卷仅剩约 2 GiB 可用空间。

## App/Clash typed settings 接入（独立复核完成，Windows 桌面 E2E 待运行）

- 目标：按固定 ref 的 typed settings hooks 和 IPC 合同，让主界面、legacy UI 及共享组件通过同一组 typed application API 读取和修改应用设置、Clash 持久设置；runtime-only 的 Clash 覆盖仍通过独立 runtime API 应用。
- 非目标：本批不删除受支持的 legacy UI、Chimera Client/Service、旧用户配置读取、legacy IPC 或 Agent 能力；不迁移 Dashboard/Session/Profile 等无关设置域，不改 TUN 授权策略。
- 参考基线：Clash Nyanpasu `ref` HEAD `685042aaedaea26a591900be0b494dfe8efd79b0`；`git -C ref status --short` 干净。父仓库的 `ref` gitlink 与当前只读 checkout 不同是该工作分支原有状态，本批不移动或修改子模块。
- 参考路径和符号：`ref/frontend/query/src/ipc/use-settings.ts::{useTypedConfig,useTypedConfigField,useSettings,useSetting,useClashSettings,useClashSetting}` → `frontend/interface/src/ipc/use-settings.ts`；`ref/backend/tauri/src/ipc.rs` 的 typed app/Clash get/patch commands 与 `ref/backend/tauri/src/client/{application,clash_config}.rs` → 对应 Chimera IPC/client；`ref/backend/chimera-config/src/{application,clash/config}` → `backend/chimera-config/src/{application,clash/config}`。保留上游的 sparse typed patch、React Query invalidation 和字段 hook 结构。2026-10-19 复核 master pinned ref SHA `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3` 的 `useTypedConfig::onSuccess` 为 block callback：触发 invalidation 但不返回 Promise；Chimera 曾返回 Promise，导致 mutation completion 等待 query refetch。当前修复恢复 ref 的后台 invalidation 语义，IPC 与存储合同不变。
- 变更接口：新增/注册 `get_app_config`、`patch_app_config(ChimeraAppConfigPatch)`、`get_clash_config`、`patch_clash_config(ClashConfigPatch)`；生成 TypeScript IPC bindings。旧的 core `/configs` 操作命名为 `patch_runtime_overrides`，避免它与保存型 `patch_clash_config` 混用。共享 `useSettings/useSetting/useClashSettings/useClashSetting` 现在调用 typed API；locale 只在 UI 边界映射到后端枚举。
- 运行时行为：typed Clash patch 依据共享 `classify_clash` 和 `RequestedRuntimeFields` 判定是否需要 core lifecycle reconcile。改变运行输入会重建并应用当前核心配置；重复提交运行字段也会重新尝试；只改 Web UI 列表等 UI 字段不会重建核心。App typed `core`、`enable_builtin_enhanced` 和已连接 Service 模式下的 `enable_service_mode` 写入经过 core lifecycle actor，先 reconcile 候选运行配置，再按版本提交持久设置；候选 reconcile 失败时尝试恢复最新持久设置对应的运行态，版本冲突后也恢复最新持久设置对应的运行状态，恢复失败会连同原错误返回。Clash typed patch 在候选运行态已应用但后续保存失败时，回滚来源配置后会重新 reconcile 当前最新配置；补偿失败与原错误一起返回。App/Clash typed patch 将当前 runtime 的 mixed port 发布给 effect actor，再由现有 typed system-proxy effect 应用新端口；不在新入口中直接读取 Verge 端口或调用旧 `init_sysproxy`。
- 保留的兼容边界：`LegacyVergeBridge`/`LegacyClashBridge` 继续把 typed 持久状态投影给尚未迁完的核心、Service 与旧配置存储消费者；`get_verge_config/patch_verge_config` 只保留为旧 IPC surface 和显式兼容性验证入口。主界面、快捷键、托盘开关、系统事件与 Agent 网络动作现已调用 typed app/Clash API，不再经过旧 `patch_verge` 业务路径。App runtime patch 复用现有 core lifecycle actor 和 CoreFacade 适配层，没有另建兼容业务路径；这仍与 ref 的 ApplicationWorkflow mutation coordinator 不同，待 Chimera 运行时生命周期收敛后再评估统一。旧配置字段别名仅保留在 Rust 输入反序列化边界，生成的前端 patch contract 暴露规范字段。下一步应迁完仍依赖旧 IPC 的测试/调用者，再评估删除命令；typed→旧配置投影在完成旧文件迁移与 Service 消费者收敛前保留为单向适配。
- 验收：主/legacy UI 共用 typed hooks；生成 bindings 与 Rust patch 可选字段一致；Clash 运行字段变更实际触发生命周期 reconcile，UI-only字段只持久化；运行态和 `clash-config.yaml` 同步；错误输入不改变运行态或文件；保留旧配置和 Service/Agent 入口。
- 风险：L2（共享持久配置、运行时核心协调、系统代理）。测试计划：配置模型单测、Clash reconcile 判定单测、backend check、typed-state E2E、interface/E2E 类型检查、改动文件格式检查、diff 检查；主/legacy UI 使用同一 hooks，桌面覆盖以 hermetic settings suite 验证。
- 回滚：回退 typed hooks/commands 和调用点，恢复原有 settings/runtime IPC 命名及前端调用；保留原配置文件与用户数据，不删除 `application.yaml` 或 `clash-config.yaml`。合并前需 L2 独立复核；提交/推送需维护者明确批准。
- 实际验证：`cargo test --locked --manifest-path backend/Cargo.toml -p chimera-config` 143/143；完整 `cargo test --quiet --locked --manifest-path backend/Cargo.toml -p chimera` 默认并行运行 572 passed、0 failed、1 ignored，其中新增内置增强候选配置和 Clash typed patch 回滚测试通过；Service Mode typed patch 的候选失败恢复、版本冲突恢复也有回归测试。一次较早的并行全套运行中 3 个 Profile 文件监听用例超时；3 个用例各自单独重跑及完整串行运行均通过，之后完整默认并行复跑也通过，因此未确认产品回归。`typed_clash_patch_restores_source_and_runtime_after_persistence_failure` 使用真实 Clash 状态 actor 与配置文件，在 legacy persistence 回调注入失败，并检查来源配置与模拟运行态均回滚；未执行真实磁盘保存错误注入或完整内核 E2E。`typed_clash_compensation_reports_runtime_reconcile_failure` 覆盖错误聚合。`cargo check`、`frontend/interface` 与 E2E TypeScript 检查、E2E harness 单测 25/25、`pnpm lint:frontend-boundaries`、目标 Rust rustfmt、改动 TS/文档 Prettier、`git diff --check` 通过。Rust 全套测试中的 generated-bindings freshness 检查通过；pnpm 对临时 worktree 的 `.pnpm` hoist symlink 发出安装警告，但该测试通过。全量 `frontend/chimera` typecheck 未验证：本 worktree 缺少 Paraglide 生成目录，生成器在解析项目远程模块时无输出停滞后已中止；复用另一 worktree 的旧生成输出也产生消息键过期错误，故不作通过结论。桌面 E2E 尚未本机运行。`clash-typed-state.e2e.ts` 位于 hermetic/settings suite，直接通过 typed IPC 观察内核 `/configs` 与隔离配置文件，并检查无效端口输入保持两者不变；`windows-service-tun-lifecycle.e2e.ts` 也已改用 typed `get_app_config`/`patch_app_config` 和 `get_clash_config`/`patch_clash_config` 覆盖真实服务切换入口，两者都完成 TypeScript 检查但未在本机运行桌面 E2E。


## 参考基线同步（2026-10-17）

- 上游：Clash Nyanpasu `main`，已核对最新提交 `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3`（2026-10-17 UTC）。
- Chimera 原固定 gitlink：`31019da14006b384b3272c72b4088179a9e72c47`；本 PR 把 `ref` 更新到上述提交。此前 PR 草案目标为 `a4f706fe788e3570247ad2fa49636c6b4c55c98b`；从该草案基线到最新上游增加 3 个提交：`8f9d7e3a30` 更新上游 `libc`，`6b609e4140` 更新上游 `@hookform/resolvers`，`d43819fbb2` 修复隐藏窗口时 WebView 的可见性与内存目标。
- 对齐差异：上游新增 `backend/tauri/src/window/engine.rs::set_in_sight` 和 `backend/tauri/src/window/manager.rs::follow_minimized`，在 Windows 窗口隐藏/最小化时隐藏 WebView 并请求低内存目标，在窗口恢复时恢复可见性。Chimera 对应实现目前集中在 `backend/tauri/src/window.rs` 的 `AppWindow::create` / `focus_existing_window`，窗口事件由 `backend/tauri/src/lib.rs` 处理，尚无等价的 WebView 可见性跟随逻辑；这是下一批值得评估的共享运行时对齐项，需补充 Windows 生命周期回归验证后再迁移。上游两个依赖更新只记入基线，不移植到 Chimera。
- 上游范围：`a4f706f..d43819f` 仅改 5 个文件（两个窗口运行时文件、`backend/Cargo.lock`、`frontend/nyanpasu/package.json`、`pnpm-lock.yaml`）；runtime 递归子模块 gitlink 未变化。本批只更新参考 gitlink 和本记录，不改变 Chimera 产品代码、依赖、IPC、持久化或宿主网络行为。
- 验证：通过 `git ls-remote` 核对上游 `main` tip，并在外接构建卷的浅克隆确认 checkout HEAD 为 `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3`、工作树干净；对比 `a4f706f..d43819f` 的文件清单及窗口代码。主卷仅余约 104 MiB，直接在原 `ref` 子模块 fetch 未能写入 `FETCH_HEAD`；因此本 PR 记录的是已独立核实的上游提交，原本地子模块目录暂未切换。仅更新 gitlink 与文档，不运行产品构建或桌面 E2E；PR CI 不检查实时上游 tip。
- 风险：L2（参考基线与后续依赖/运行时审查来源变更）。回滚时恢复原 `ref` gitlink `31019da14006b384b3272c72b4088179a9e72c47`，并撤回本节。

## 当前参考基线同步（2026-10-20，本批）

- 上游：Clash Nyanpasu `main` `ae0db596e96ba09d7662cc7533a13ffe5b581bf7`；通过 GitHub API 核对当前分支 tip，并确认该提交的 GitHub signature verification 为 valid。目标树中的递归 runtime gitlink 为 `a130ca7b6015121d49e43ecd0d6af1862e60253e`。
- Chimera 原固定 gitlink：`d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3`。本批仅更新参考 gitlink 和基线文档；不移植 ref 的产品代码、工作流或依赖。`d43819f..ae0db596` 共 44 个上游提交；其中新增 `nyanpasu-tsdb` 时序存储 crate 与其持久化、内存预算、恢复和故障验证体系，并更新 widget、proxy provider cache、updater changelog、配置快照和多项依赖。这些上游能力仍需逐项映射与完整验证；尤其 TSDB 属于架构迁移，不能仅凭 gitlink 更新视为已接入 Chimera。
- 验证：GitHub API 确认 `main` tip 与目标 SHA 一致、目标提交签名有效；隔离 worktree 的 `git -C ref rev-parse HEAD` 为 `ae0db596e96ba09d7662cc7533a13ffe5b581bf7`，`git -C ref status --short` 无输出；递归 runtime 和 `nyanpasu-utils` 子模块分别为 `a130ca7b6015121d49e43ecd0d6af1862e60253e` 与 `f57f9f077c3f1cb31da0e8261c21bee7bdd75c8f`，均已检出且干净。`git diff --check` 与文档 Prettier 检查通过。PR 的新克隆/CI 仍需验证其 checkout；本批没有产品代码，桌面 E2E 不适用。父仓库自身的 `backend/chimera-runtime` 在这个隔离 worktree 中未初始化，不属于本次 `ref` 递归状态。
- 风险：L2（参考依赖基线）。本批只改变后续对齐的来源，不改变 Chimera 运行时、持久化、IPC 或主机网络行为。回滚时将 `ref` gitlink 恢复为 `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3`，并撤回本节与指南元数据。

## Dashboard 现有小组件配置（实现完成，待 PR CI）

- 目标：按固定 `ref/` 基线为 Chimera 已支持的六种 Dashboard 小组件补齐逐实例配置、默认值恢复、保存状态和错误重试，并继续使用现有共享 KV 存储与跨窗口事件。
- 非目标：本批不迁入缺失的十种小组件，不更改 Dashboard 布局键/数据结构，不改 legacy UI、Agent、核心服务或宿主网络行为。
- 参考基线：Clash Nyanpasu `main` `685042aaedaea26a591900be0b494dfe8efd79b0`；功能实现时的参考基线；该提交是当前 `ref` 基线 `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3` 的祖先。
- 参考映射：`ref/frontend/nyanpasu/src/components/widgets/{consts.ts,provider.tsx,widget-config-menu.tsx,widget-config-row.tsx,widget-config.ts,widget-item.tsx,widget-sparkline.tsx,widget-shortcut.tsx}` → `frontend/chimera/src/components/widgets/` 下同名文件，保留上游目录、文件拆分与共享符号；Dashboard route 从该目录导入。`ref/frontend/nyanpasu/src/pages/(main)/main/dashboard/_modules/edit-action.tsx` → Chimera `_modules/edit-action.tsx`；Chimera 专有的布局适配、组件库与放置逻辑仍留在 Dashboard route `_modules/`。`ref/frontend/query/src/hooks/use-kv-storage.ts` → `frontend/interface/src/hooks/use-kv-storage.ts`（保留 Chimera bindings/事件接口并补齐返回写入状态和读取错误恢复）。
- 变更接口：新增存储键 `dashboard-widget-configs`，版本化、按 widget instance id 保存；现有布局 KV 键和 schema 不变。`useKvStorage` setter 增加成功布尔值，状态对象增加 `readError` 与 `refresh`；现有调用方可继续忽略这些新增结果。Traffic Down/Up、Connections、Memory、Proxy Shortcuts、Core Shortcuts 均按实例读取/保存配置；Provider 将连续变更交给 hook 的串行写入队列，并只展示最新写入结果，避免保存中的后续调整被丢弃。配置模型单测已登记到 frontend CI；本次 workflow 改动已获维护者批准。
- 验收：六种配置可打开、变更并持久化；读取失败可重试且不阻塞退出编辑，写入失败不显示成功；配置独立于布局并能恢复默认值；主 Dashboard E2E 覆盖读取/写入失败及重试、刷新后状态；单元测试覆盖配置归一化、未知/非法字段和实例隔离。
- 风险：L2（共享持久化 hook 与 Dashboard 用户偏好）。测试计划：配置模型单测、E2E harness 单测、frontend typecheck/build、frontend boundary check、主 UI dashboard E2E、格式和 diff 检查；真实桌面覆盖缺失时如实标记。
- 回滚：回退本批前端/接口代码、配置 UI 和测试入口；保留既有 Dashboard 布局键，并且不删除 `dashboard-widget-configs` 数据。该 L2 功能与 L3 workflow 改动已获维护者批准。
- 保留差异与收敛计划：ref 的 `CoreShortcuts.showChannel` 依赖 core status 返回活动 controller（HTTP、Named Pipe 或 Unix Socket）。Chimera 的 `get_core_status` 目前只返回 state、变化时间和 run type；本批保留该 IPC 合同，不伪造通道信息。若需达到该项能力，后续独立 L2 批次应从共享 core status/应用 API 暴露运行时 controller，并验证正常、Service 与失败/未知状态。ref 还包含十种当前 Chimera Dashboard 尚未提供的 widget；本批为 Chimera 已有六种补齐配置，不声称迁完整个 ref widget catalogue。ref 的 `LauncherMenu`、`NumberStepper`、`ShapeToggle` 及快捷键按钮的 `presentation/elastic` API 未在 Chimera UI 包中；配置 popover 和快捷键使用现有 Radix、Button、Switch 与系统代理/TUN 控件适配，保留字段语义与动作，菜单/按钮呈现细节待共享 UI 原语对齐时再收敛。另，ref 的 `useKvStorage` 对不同 key 共用写入计数和 revision；独立复核发现不同 key 的写入可能误挡新 key 首次读取，A→B→A 时旧 key 迟到的写入回声也可能覆盖新值。本批将写入计数、revision 与回声按 key 隔离，并由 `KvStorageWriteEchoes` 在全局事件流中消费非活动 key 的未完成回声，避免它们覆盖当前状态；这与 ref 有意不同，后续 ref hook 更新时须重新核对并保留保护。三个可执行单测覆盖 A→B→A、非活动 key 回声和相同值并行写入；本地没有独立 React hook 浏览器测试器，真实 hook 生命周期仍未桌面实测。
- ref 缺陷例外：固定参考提交 `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3` 的 `frontend/nyanpasu/src/components/widgets/widget-config.ts::normalizeWidgetConfigStorage` 将所有非 v1 版本静默归一为空配置；降级运行的旧版随后保存设置会覆盖新版配置。本 PR 对 Chimera 的同名函数改为拒绝任何显式未知版本，并在 `frontend/interface/src/hooks/use-kv-storage.ts` 的首次读取与存储事件路径呈现读取错误、推进 revision 栅栏；Dashboard Provider 现有写入门禁会阻止覆盖。保留其他 ref 对齐；当 ref 增加版本迁移/未知字段保留逻辑或调整 `dashboard-widget-configs` schema 时复查并重新对齐此例外。
- 测试与实际结果：Dashboard 配置模型与布局模型定向测试 6/6；配置模型及 KV 回声 registry（`frontend/chimera/tests/dashboard-widget-config.test.ts`、`frontend/interface/tests/kv-storage-write-echoes.test.ts`）6/6，且已登记在 frontend CI 的 `test:dashboard-widget-config` 命令；`tauri-e2e` 当前注册的 Node unit tests 25/25；frontend interface、Chimera 与 E2E TypeScript 检查通过（Chimera 类型检查使用临时生成的 Paraglide 声明 shim，未将其纳入版本控制）；`scripts/check-frontend-boundaries.ts`、改动文件 Oxlint、Prettier、四语言 865 个 message key parity 与 `git diff --check` 通过。桌面 E2E 未在本机运行。Paraglide compile 在输出“Compiling inlang project ...”后 30 秒无进展，Vite production build 也停在项目编译阶段；进程无 CPU 活动。对应两个 CDN module URL 的 HTTP HEAD 均返回 200，因此当前原因尚未定位；未把旧生成目录或临时类型 shim 当作生成验证。该分支同步最新 master 后的新 PR SHA 将运行远端 CI/E2E；此处记录的是同步前本地检查结果。
- 2026-10-17 复核修复验证：Dashboard 配置模型与 KV 回声 registry 定向测试 6/6；`frontend/interface` 和 `tauri-e2e` 的 TypeScript 检查、改动文件 Oxlint、Prettier 与 `git diff --check` 通过。`frontend/chimera` 直接 TypeScript 检查报告 Paraglide 生成消息模块缺少多个已有调用方的导出（包括 Dashboard 与未改动页面），因此该次全 UI 检查不记为通过；当前工作树 `node_modules` 符号链接也使 pnpm 脚本入口拒绝执行，本地验证改用现有依赖中的二进制。新加的未来版本 UI E2E 未在本机桌面运行，将由推送后的 Windows `hermetic` 套件验证。
- 当前状态：功能代码、配置与 KV 回声单测、主 Dashboard 刷新持久化 E2E、CI 单测入口和对齐记录已准备；真实桌面运行及本机 Paraglide 生成仍未验证。最终独立复核没有发现可操作 P0–P2 问题；复核者确认无 React hook 测试运行器，因此 A→B→A 覆盖测试使用生产回声登记器，React 生命周期集成由桌面 E2E/CI 补证。用户已批准提交、推送并开 PR；PR CI/E2E 尚未运行，不能将待运行检查视为通过。

## Dashboard 小组件库与 DnD Grid（实现、独立复核、CI/E2E 完成，已合入 master）

- 目标：将可复用网格组件迁到 `@chimera/ui` 并对齐上游拖拽源 API；Dashboard 小组件库提供搜索、直接添加，以及将条目拖到网格指定位置。保留现有六种小组件、`dashboard-widgets` 持久化格式和当前布局自适应逻辑。
- 非目标：本批不增加新的小组件类型，不改 legacy UI、agent、核心服务或系统网络行为。
- 基线：Clash Nyanpasu `main` `685042aaedaea26a591900be0b494dfe8efd79b0`（2026-10-16 fetch 后核对）。PR #486 将 master 原有 ref gitlink `c08aa9d9a17953aa2464cdae61390d5c3f9afbc7` 更新到该基线，并于 2026-10-17 以 rebase 方式合入，merge commit `72d61000a63bca6920cc2c55d9a812392bac4c43`。该功能只读取 ref 的前端源码；当时 ref 内部 runtime 指针为 `be2f561102fe898cb7a294ef14de3f4c65b68f81`。
- 参考映射：`ref/frontend/ui/src/dnd-grid/*` → `frontend/ui/src/dnd-grid/*`；新增上游 `DndGridSource`、`DndGridSourceData`、网格命中坐标和 `onExternalDrop`；`ref/frontend/nyanpasu/src/pages/(main)/main/dashboard/_modules/widget-placement.ts` → Chimera Dashboard `_modules/widget-placement.ts`；WidgetSheet 沿用上游可搜索条目和直接添加交互，但使用 Chimera 当前的 Vaul 抽屉及六项 `RENDER_MAP`。`use-grid-layout` 继续依赖 Chimera 已锁定的 `lodash-es`，其余网格结构和符号按 ref 对齐。
- 保留差异及收敛计划：当前 ref 有 16 种 Dashboard 小组件，Chimera 已实现其中 6 种。本批保留 Chimera 现有六项，不复制依赖尚未迁入的 Profile/traffic-report/provider widget 实现与配置；当对应组件和数据合同进入 Chimera 后，再按完整 widget 模块迁入并扩展库列表。
- 接口影响：`@chimera/ui` 新增 `dnd-grid` 与 `search-field` 导出；Dashboard 的拖拽源通过 `onExternalDrop(id, position)` 添加小组件。存储 schema 不变。legacy UI 与 agent 不调用此主界面布局 API。
- 风险：L2（共享 UI 交互及已持久化布局的写入路径）。验收包括定位添加、冲突拒绝、无位置时追加、搜索/直接添加和已有布局恢复；主机网络无影响。
- 实际验证：placement 定向测试 3/3 通过；共享 UI TypeScript 检查、Vite 生产构建、前端边界检查、Prettier 检查及 `git diff --check` 通过。锁文件可解析，四个新增 UI importer 与 manifest 版本一致。全量 Chimera 类型检查仍在未修改的 connections、context-menu、editor、profiles/columns 和测试 Node 类型声明处失败；未报告本批改动文件。独立复核首次发现 Add 按钮未阻止 TouchSensor 的长按事件，已补 `touchstart` 冒泡拦截；复核后续未发现可执行缺陷。
- 未覆盖：桌面 E2E 未运行；当前锁定的 Tauri WebDriver 操作合成 `MouseEvent`，无法证明真实 PointerSensor/TouchSensor 拖拽，搜索和直接添加也未在运行中的桌面窗口验证。`pnpm prepare:check` 未运行，因为它会下载/安装 sidecar 二进制，与此纯前端切片无关。隔离工作树的 `pnpm install --frozen-lockfile` 卡在 supply-chain lockfile 校验；没有绕过该校验，使用本机已安装依赖直接运行定向命令。
- 独立审查：最终复核没有发现可操作缺陷。审查环境因不能创建本地 IPC socket 而未能自行启动定向测试；该测试已在工作区命令行直接执行并通过。
- 回滚：单独回滚 PR #486 时，其 ref gitlink 应恢复为该 PR 前的 `c08aa9d9`；用户存储 schema 未变，不需要数据迁移。若本节顶部记录的后续 ref 基线同步已合并，回滚 Dashboard 功能时应保留新的 ref pin。
- 合并与验证：PR #486 的 CI run `38959272484`、Tauri E2E run `38959272498` 全部通过；独立复核无阻塞项。已于 2026-10-17 以 rebase 方式合入 `master`（merge commit `72d61000a63bca6920cc2c55d9a812392bac4c43`）。

此前 Profile 同步批次的只读参考检出为 Clash Nyanpasu `main` 的提交
`c08aa9d9a17953aa2464cdae61390d5c3f9afbc7`，该提交在上游 Actions 页面对应
`main` 构建，子模块工作区干净。主仓库 `master` 基线为
`e81eeaea46a9c229478cd29c9555b5c072f7e6cc`；本分支将 ref 更新至 `c08aa9d9`。自原基线以来上游合入了 TanStack Router、meta-json-schema、
Playwright、Prettier 和 router-plugin 更新，流量分析侧栏路由重整，以及完整的
profile transform-chain 编辑改进和相应浏览器测试；还补充了 worktree CoW 说明。
递归子模块指针未变。此同步只更新参考基线，不等于将依赖或界面代码直接应用到
Chimera。

DIFF-001 至 DIFF-012 是此前基于
`f7dbce2997c633e484f54788035e770b3ee99773` 完成的历史记录；它们不代表已按
新基线重审。后续切片应使用当前 `ref/` 提交，并明确记录与历史基线的差别。

## ref 基线同步（2026-10-14）

- 上一固定提交：`813673a8e84b51265467f02277fc7e739837f61e`。
- 同步提交：`e81eeaea46a9c229478cd29c9555b5c072f7e6cc`，已 fetch 并与
  `origin/main` 核对，是当前上游 main tip。期间依次包含 `85613d5e` Router 依赖、
  `fb0039b1` 流量分析路由、`635e96c9` meta-json-schema、`9fdd5135` Playwright、
  `d9cf2cc8` profile transform-chain 编辑、`b568b8b5` Prettier、`6083e156`
  worktree CoW 文档和 `e81eeaea` router-plugin 更新。
- 上游变化：除依赖升级与流量侧栏路由外，`d9cf2cc8` 重整 profile 转换链编辑器、
  同步历史展示和卡片能力，增加 transform-chain 列组件、Accordion 组件及浏览器
  覆盖。此次仅更新 `ref` 基线和本记录；未把上游依赖、profile UI 或其他源文件复制
  到 Chimera，也未改 Chimera 的 `package.json`、`pnpm-lock.yaml` 或前端实现。
- 验证：父仓库 `ref` gitlink 指向该提交；ref 工作区干净；递归子模块没有变化；
  `git diff --check` 通过。此批只改参考版本与本记录，不涉及产品代码，故未运行
  应用构建、桌面 E2E 或宿主网络测试。

## DIFF-001：共享 Profile/Runtime 领域下沉（第一阶段）

- ref commit：`f7dbce2997c633e484f54788035e770b3ee99773`
- ref 路径和符号：`backend/nyanpasu-config/src/profile/*`、
  `backend/nyanpasu-config/src/runtime/*`，包括 `Profiles`、
  `ProfileDefinition`、`ConfigValue`、`ConfigSnapshotsBuilder`、
  `RuntimePipelineInputs` 和 `execute`
- Chimera 路径和符号：`backend/chimera-config/src/profile/*`、
  `backend/chimera-config/src/runtime/*`，品牌映射为 `chimera-config`
- 类别：临时迁移
- 差异及必要性：已按 ref 的目录、文件名、领域模型和执行顺序接入共享
  配置包；旧的 `backend/tauri/src/config/profile` 与
  `backend/tauri/src/config/runtime.rs` 仍保留，作为现有生产调用链的兼容
  边界，尚未切换所有 Tauri 调用方。
- 共通业务入口及适配边界：新模块提供纯 Profile/Runtime 领域 API；现有
  Tauri Profile 存储、脚本适配器和 UI 暂不改变，下一阶段由
  `RuntimeBuilder` 负责接入并移除重复业务实现。
- 影响的主界面、legacy UI、agent、数据、内核和平台：本阶段未改变 UI、
  agent、持久化格式或内核控制；新增领域类型通过 Rust crate 导出，供后续
  IPC 和 RuntimeBuilder 使用。
- 实际验证结果：`cargo test --manifest-path backend/Cargo.toml -p
  chimera-config -- --test-threads=1`，135 passed；
  `cargo fmt --manifest-path backend/Cargo.toml --all -- --check` 通过。
- 收敛、移除或重新评估条件：RuntimeBuilder 接入新 Profiles 快照并覆盖主
  界面、legacy UI 和 agent 的共同调用链后，删除 Tauri-local 的重复 Profile
  / Runtime 业务实现；在此之前该差异必须标记为部分迁移。

## DIFF-002：标准核心切换到 ref RuntimeExecutor（第二阶段）

- ref commit：`f7dbce2997c633e484f54788035e770b3ee99773`
- ref 路径和符号：`backend/nyanpasu-config/src/runtime/executor/*`、
  `backend/tauri/src/enhance/runtime_builder.rs`、
  `backend/tauri/src/enhance/script/adapter.rs`
- Chimera 路径和符号：`backend/tauri/src/enhance/runtime_builder.rs`、
  `content_source.rs`、`artifact_bridge.rs`、`script/adapter.rs`，以及
  `Config::generate_runtime_input_with`
- 类别：临时迁移
- 差异及必要性：标准 Clash 核心（Premium、Rust、Mihomo 及 Alpha）现在
  通过共享 `RuntimePipelineInputs` 和 `execute` 完成 Profile 合成、全局
  变换、内置脚本、字段白名单、Guard、TUN 默认值和产物日志；旧的
  `PostProcessingOutput` 由适配器生成。Chimera Client 仍保留旧 Enhance
  路径，因为其自定义 TUN 合同尚未进入共享 executor，避免回归。
- 失败回退：若 legacy Profile 转换或 ref executor 构建失败，入口会记录
  warning 并回退到旧 Enhance，保证已有用户配置仍可启动；该回退是可观测的
  临时兼容边界，不代表两条实现长期并存。
- 兼容边界：`config/profile/ref_adapter.rs` 将现有 legacy profile 文档转换
  为 ref 领域模型；多选配置转换为兼容 composition，不改写用户文件。
  脚本通过现有 JavaScript/Lua runner 适配到 ref 的 `ScriptRunner` trait。
- 影响的主界面、legacy UI、agent、数据、内核和平台：共同的 runtime 生成
  入口受影响；UI、agent、E2E 入口和 legacy 持久化格式保持不变。
- 实际验证结果：`cargo test --manifest-path backend/Cargo.toml -p
  chimera config::core -- --test-threads=1`，1 passed；`cargo test
  --manifest-path backend/Cargo.toml -p chimera config::profile::ref_adapter
  -- --test-threads=1`，2 passed；`cargo test --manifest-path
  backend/Cargo.toml -p chimera enhance -- --test-threads=1`，39 passed；
  `cargo test --manifest-path backend/Cargo.toml -p chimera-config --
  --test-threads=1`，135 passed；workspace `cargo check` 通过。
- 收敛、移除或重新评估条件：为 Chimera Client 实现共享 executor 所需的
  自定义 TUN/运行时合同，并在主界面、legacy UI 和 agent 的真实启动路径
  完成验证后，移除旧 Enhance 分支及 legacy profile adapter。当前仍是部分
  迁移，不能宣称已经完全等同 ref。

## DIFF-003：应用/Clash 共享配置合同补齐（第二阶段）

- ref commit：`f7dbce2997c633e484f54788035e770b3ee99773`
- ref 路径和符号：`backend/nyanpasu-config/src/application/mod.rs`、
  `application/widget.rs`、`clash/runtime/mod.rs`、
  `clash/config/overrides/mod.rs`
- Chimera 路径和符号：对应的 `backend/chimera-config/src/*`
- 类别：兼容扩展
- 差异及必要性：补齐 `Mode::Script`、Clash runtime snapshot re-export、
  托盘菜单/关闭行为、PAC、热键、延迟测试、流量图、内存信息、代理布局、
  网络统计浮窗等 typed application 字段；`ChimeraAppConfig` 使用
  `serde(default)`，旧 `application.yaml` 缺少这些字段时继续读取并采用
  ref 默认值。网络统计枚举在 config crate 中定义，避免引入未实现的 egui
  widget 运行时依赖。
- 保留差异：legacy `IVerge` 尚未承载全部新字段，桥接只投影其已有字段；
  新字段的 UI/托盘行为仍需按产品边界逐项接入。
- 实际验证结果：`cargo test --manifest-path backend/Cargo.toml -p
  chimera-config -- --test-threads=1`，135 passed；workspace `cargo check`
  通过。
- 收敛条件：为新增字段接入对应主界面设置、legacy 兼容投影、agent 读取和
  真实托盘/PAC/hotkey 行为测试后，再评估是否可以删除旧字段或适配器。

## DIFF-004：RuntimeInspection 只读诊断投影（第三阶段）

- ref commit：`f7dbce2997c633e484f54788035e770b3ee99773`
- ref 路径和符号：`backend/tauri/src/client/runtime_inspection.rs`、
  `client/runtime.rs` 的 `RuntimeSnapshot.inspection`、
  `ipc::inspect_runtime`、`ipc::inspect_runtime_node` 及
  `specta_export` 命令注册
- Chimera 路径和符号：对应的 `backend/tauri/src/client/*`、
  `backend/tauri/src/core/clash/core.rs`、
  `backend/tauri/src/config/core.rs`、
  `backend/tauri/src/enhance/{runtime_builder,artifact_bridge}.rs`、
  `backend/tauri/src/ipc.rs` 和
  `frontend/interface/src/ipc/bindings.ts`
- 类别：临时迁移
- 差异及必要性：标准核心现在将 ref executor 产出的
  `ConfigSnapshotsGraph`、步骤日志和快照 ID 一并保存到已提升的
  `RuntimeSnapshot`，通过两个只读 IPC 投影节点摘要、YAML、父节点 diff 和
  日志；生成的 TypeScript binding 已按既有 Specta 流程更新。该能力只读取
  已发布快照，不修改代理、TUN、系统设置或用户配置。
- 兼容边界：Chimera Client 和 ref executor 构建失败的 legacy/fallback 路径
  继续使用安全的 `BareRoot` 空图，因此这些路径暂时只能显示最终产物的根节点，
  不会伪造未生成的中间节点；待其迁移到共享 executor 后再移除该兼容值。
- 影响的主界面、legacy UI、agent、数据、内核和平台：新增 IPC/API 可供主界面、
  legacy UI 或 agent 复用；本批未改变既有 UI 流程、持久化格式、核心启动和
  系统代理行为。
- 实际验证结果：`cargo check --manifest-path backend/Cargo.toml -p chimera`；
  `cargo test --manifest-path backend/Cargo.toml -p chimera runtime_inspection
  -- --test-threads=1`，4 passed；`cargo test --manifest-path
  backend/tauri/Cargo.toml typescript_bindings_are_fresh --
  --test-threads=1` 通过；`pnpm typecheck` 通过。
- 收敛、移除或重新评估条件：将 Chimera Client、legacy/fallback 构建接入
  共享 executor 并完成真实 runtime/E2E 验证后，删除 `RuntimeInspectionData::bare`
  兼容分支，补充端到端快照更新/过期检查；当前仍是部分迁移，不能宣称已完全
  等同 ref。

## DIFF-005：CoreLifecycle 目录边界（第四阶段）

- ref commit：`f7dbce2997c633e484f54788035e770b3ee99773`
- ref 路径和符号：`backend/tauri/src/client/core_lifecycle/{mod.rs,ports.rs,adapters.rs,workflow.rs}`，
  以及 `CoreLifecycleClient` 的端口/适配器分层
- Chimera 路径和符号：新增
  `backend/tauri/src/client/core_lifecycle/{mod.rs,ports.rs,adapters.rs}`；
  原 `backend/tauri/src/client/core_bridge.rs` 保留为兼容 re-export shim，
  `client/mod.rs`、`client/clash_config.rs` 和 `setup.rs` 已改用新目录入口
- 类别：临时迁移
- 差异及必要性：将现有 CoreManager 生命周期端口、运行配置端口、运行时诊断
  DTO 和 legacy adapter 按 ref 的 core-lifecycle 目录归位，保持现有行为与旧
  UI/API 合同不变；本阶段只做所有权和导入路径迁移，没有引入 actor 或改变
  核心启动、停止、切换和系统设置副作用。
- 兼容边界：`core_bridge.rs` 只保留 re-export，避免分批迁移期间破坏旧调用方；
  新的 `core_lifecycle` 仍委托 legacy `CoreManager`，尚未具备 ref 的
  `workflow.rs`、actor mailbox、超时不确定状态和 service host facade。
- 影响的主界面、legacy UI、agent、数据、内核和平台：调用方继续复用同一
  `ChimeraClient` 和端口，未改变持久化格式或现有 UI/agent/E2E 入口。
- 实际验证结果：`cargo check --manifest-path backend/Cargo.toml -p chimera`；
  `cargo test --manifest-path backend/Cargo.toml -p chimera client::tests --
  --test-threads=1`，17 passed。
- 收敛、移除或重新评估条件：完成 ref `core_lifecycle/workflow.rs` 与
  `core/actor_v2` 的最小完整迁移、接入真实 service/local host 和超时恢复测试后，
  删除 `core_bridge.rs` shim，并将 `ClientSetupArgs` 切换为 actor-backed
  `CoreLifecycleClient`；当前仍是目录和端口层的部分迁移。

## DIFF-006：Runtime exists_keys 读模型（第五阶段）

- ref commit：`f7dbce2997c633e484f54788035e770b3ee99773`
- ref 路径和符号：`backend/tauri/src/enhance/artifact_bridge.rs` 的
  `RuntimeArtifact.applied_fields` → `RuntimeSnapshot.exists_keys`，以及
  `ipc::get_runtime_exists`、`specta_export` 注册
- Chimera 路径和符号：`backend/tauri/src/enhance/artifact_bridge.rs`、
  `enhance/runtime_builder.rs`、`config/core.rs`、
  `core/clash/core.rs`、`client/runtime.rs`、`client/runtime_inspection.rs`、
  `ipc.rs` 和生成的 `frontend/interface/src/ipc/bindings.ts`
- 类别：兼容扩展
- 差异及必要性：标准 executor 的 `applied_fields` 现在按执行顺序保留为
  `RuntimeSnapshot.exists_keys`，并提供只读 `get_runtime_exists` IPC；旧
  Enhance 路径也将原有 `use_keys` 结果继续带入快照，未改变配置文件或核心
  启动行为。
- 兼容边界：尚未发布 RuntimeSnapshot 时返回空数组；旧构造器为保持已有测试和
  调用合同，默认使用空集合，只有实际构建入口注入对应键集合。
- 实际验证结果：`cargo check --manifest-path backend/Cargo.toml -p chimera`；
  `cargo test --manifest-path backend/tauri/Cargo.toml
  typescript_bindings_are_fresh -- --test-threads=1` 通过；`pnpm typecheck`
  通过；新增 RuntimeSnapshot applied-fields 单测。
- 收敛条件：当 RuntimeSnapshot 完成 ref `from_data` 统一构造并覆盖所有
  Chimera Client/legacy/fallback 路径后，移除空集合兼容构造器，并补充真实
  reconcile 后 `get_runtime_exists` 的 E2E 验证。

## DIFF-007：Runtime 只读配置与后处理投影（第六阶段）

- ref commit：`f7dbce2997c633e484f54788035e770b3ee99773`
- ref 路径和符号：`backend/tauri/src/ipc.rs` 的
  `get_runtime_config`、`get_runtime_yaml`、`get_runtime_exists`、
  `get_postprocessing_output`，以及 `specta_export` 中对应命令注册
- Chimera 路径和符号：`backend/tauri/src/ipc.rs`、
  `backend/tauri/src/specta_export.rs`、
  `frontend/interface/src/ipc/bindings.ts`，并复用
  `client::RuntimeSnapshot::{config,exists_keys,postprocessing_output}`
- 类别：兼容扩展
- 差异及必要性：runtime 配置 YAML/JSON、已应用字段和后处理输出现在都从
  已提升的 `RuntimeSnapshot` 读取，与 ref 的发布态读模型一致；未发布快照时
  配置返回 `None`、YAML 返回错误、字段和后处理输出返回默认值。配置 JSON
  暂以 `Any<serde_json::Value>` 暴露，待 ClashConfig 完成统一 typed contract
  后再替换为对应类型。
- 兼容边界：`get_runtime_yaml` 不再读取 legacy draft，而只导出最近一次已发布
  运行配置；该行为避免把未执行的草稿误报为当前 runtime，且所有新增命令均为
  只读，不改变配置、代理、TUN 或系统代理状态。
- 影响的主界面、legacy UI、agent、数据、内核和平台：新增共享 IPC/API 与
  Specta 绑定，主界面、legacy UI 和 agent 可复用同一发布态数据；现有 UI
  流程、持久化格式、核心生命周期和系统设置副作用保持不变。
- 实际验证结果：`cargo check --manifest-path backend/Cargo.toml -p chimera`；
  `cargo test --manifest-path backend/tauri/Cargo.toml
  typescript_bindings_are_fresh -- --test-threads=1`，1 passed；`pnpm typecheck`
  通过；`pnpm lint:frontend-boundaries` 通过；`git diff --check` 通过。
- 收敛条件：完成 typed ClashConfig 合同并覆盖 Chimera Client、legacy 和
  fallback 的统一 RuntimeSnapshot 构造后，移除 `Any<serde_json::Value>` 与
  默认值兼容分支，并补充真实 runtime 导出/后处理输出的 E2E 验证。

## DIFF-008：Client UI 事件 sink 合同（第七阶段）

- ref commit：`f7dbce2997c633e484f54788035e770b3ee99773`
- ref 路径和符号：`backend/tauri/src/client/event_sink.rs` 的
  `UiEventSink`、`TauriUiEventSink`、`NoopUiEventSink`
- Chimera 路径和符号：`backend/tauri/src/client/event_sink.rs`、
  `backend/tauri/src/client/mod.rs`，复用现有 `core::handle::{Handle, Message,
  StateChanged}`
- 类别：临时迁移
- 差异及必要性：事件 sink 现在提供 ref 的统一 `state_changed`、提示消息、
  托盘更新、配置/Profiles/Proxies 刷新方法，并保留 Chimera 的
  `RuntimeTransformDiagnostics` 通知扩展；新增 Tauri-owned sink 和无运行时
  测试替身，供后续 actor-backed lifecycle 装配使用。
- 兼容边界：当前生产 composition 仍注入 `LegacyUiEventSink`，默认方法继续
  通过全局 `Handle` 广播，以保持主界面与 legacy UI 的现有事件可见范围；
  `TauriUiEventSink` 暂未替换该装配，避免在生命周期迁移完成前缩小事件接收面。
- 影响的主界面、legacy UI、agent、数据、内核和平台：仅扩展共享 UI 副作用
  端口和测试能力，不改变持久化、runtime 产物、核心进程或系统代理行为。
- 实际验证结果：`cargo fmt --manifest-path backend/Cargo.toml --all`；
  `cargo check --manifest-path backend/Cargo.toml -p chimera`；新增事件映射
  单测通过；`pnpm typecheck` 与 `pnpm lint:frontend-boundaries`（未受本批
  Rust-only 变更影响，上一批已通过）。
- 收敛条件：core lifecycle actor 与 workflow 接管生产装配并完成主/legacy
  UI 事件覆盖验证后，再将 `TauriUiEventSink` 接入 composition root，移除
  `LegacyUiEventSink` 的全局兼容实现。

## DIFF-009：RuntimeSnapshotData 统一构造（第八阶段）

- ref commit：`f7dbce2997c633e484f54788035e770b3ee99773`
- ref 路径和符号：`backend/tauri/src/client/runtime.rs` 的
  `RuntimeSnapshotData` 与 `RuntimeSnapshot::from_data`
- Chimera 路径和符号：`backend/tauri/src/client/runtime.rs`、
  `backend/tauri/src/core/clash/core.rs`、`backend/tauri/src/ipc.rs`
- 类别：临时迁移
- 差异及必要性：RuntimeSnapshot 现在由单一 `from_data` 负责计算产品摘要、
  生成 inspection id 和组装配置/已应用字段/后处理输出/诊断图；标准核心
  的生产提升路径已直接使用该构造，旧的多参数构造器暂保留为 legacy 与
  单测兼容包装。后处理字段名称同步为 ref 的 `postprocessing_output`，并
  保留 `RUNTIME_CONFIG_FILE`，增加 ref 兼容的 `RUNTIME_CONFIG` 别名。
- 兼容边界：Chimera 的并发 `RuntimeRevisionAllocator`、Applied/Promoted
  双快照和事务恢复状态仍保留；`new_with_transform_output*` 仅是过渡入口，
  不再持有独立组装逻辑。legacy/fallback 仍通过 `RuntimeInspectionData::bare`
  提供稳定根节点，不伪造中间步骤。
- 影响的主界面、legacy UI、agent、数据、内核和平台：统一构造只影响共享
  runtime 发布态读模型；现有生成文件、核心启动/回滚、持久化和系统代理
  副作用保持不变，IPC 继续读取同一 `postprocessing_output` 字段。
- 实际验证结果：`cargo fmt --manifest-path backend/Cargo.toml --all -- --check`；
  `cargo test --manifest-path backend/tauri/Cargo.toml client::runtime --
  --test-threads=1`，21 passed；`cargo check --manifest-path backend/Cargo.toml
  -p chimera`；`pnpm typecheck`；`pnpm lint:frontend-boundaries`；`git diff --check`
  均通过。
- 收敛条件：所有构造调用方迁移到 `from_data` 后移除兼容构造器，并在 typed
  ClashConfig 与 actor-backed lifecycle 完成后复核 `RuntimeSnapshotData` 的
  字段所有权和名称。

## DIFF-010：会话端口解析与 executor 输入（第九阶段）

- ref commit：`f7dbce2997c633e484f54788035e770b3ee99773`
- ref 路径和符号：`backend/tauri/src/client/ports.rs` 的
  `PortsFingerprint`、`SessionPortResolver` 与 `ResolvedPortBindings` 装配
- Chimera 路径和符号：`backend/tauri/src/client/ports.rs`、
  `client/mod.rs`、`config/core.rs`、`enhance/runtime_builder.rs`、
  `core/clash/core.rs`
- 类别：临时迁移
- 差异及必要性：Chimera 现在按 ref 的 session fingerprint 缓存 mixed、HTTP、
  SOCKS 和 external-controller 的具体端口，并将解析后的
  `ResolvedPortBindings` 传给标准核心 runtime builder；配置只改变 host 时保留
  已选端口，配置策略改变时才重新探测，避免运行中的核心把自己的监听端口误判为
  冲突。`service/profile_file::SelfProxyPortSource` 在 Chimera 尚无对应 service
  模块，因此暂未复制该 trait 实现，保留 `cached_ports` 作为等价只读边界。
- 兼容边界：旧的 `generate_runtime_output_with` 与公开
  `build_from_legacy` 仍从现有 legacy client info 构造 fallback bindings；
  `ChimeraClient` 专用 Enhance 路径保持原有自定义 TUN 合同，未改变持久化格式、
  legacy UI、agent 或 E2E 入口。
- 影响的主界面、legacy UI、agent、数据、内核和平台：标准核心启动、重启和
  runtime 快照现在共享同一组具体端口；主界面、legacy UI 与 agent 继续通过同一
  application API 读取状态，系统代理和配置文件副作用保持不变。
- 实际验证结果：`cargo fmt --manifest-path backend/Cargo.toml --all`；
  `cargo check --manifest-path backend/Cargo.toml -p chimera`；
  `cargo test --manifest-path backend/tauri/Cargo.toml client::ports --
  --test-threads=1`，5 passed；`pnpm typecheck`；
  `pnpm lint:frontend-boundaries`；`git diff --check`。
- 收敛条件：补齐 Chimera 对应的 service/profile-file 端口读取接口后，将
  `SessionPortResolver` 接入 fetcher 的 `SelfProxyPortSource`；标准与
  `ChimeraClient` runtime builder 均迁移到 typed ClashConfig 后，删除 legacy
  fallback bindings，并补充真实核心重启/端口占用 E2E 验证。

## DIFF-011：Legacy Connections 虚拟行测量回调死循环（缺陷修复）

- ref commit：`f7dbce2997c633e484f54788035e770b3ee99773`
- ref 路径和符号：`frontend/nyanpasu/src/pages/(main)/main/connections/index.tsx`
  的虚拟行 `ref`；Chimera 路径和符号：
  `frontend/chimera/src/components/connections/connections-table.tsx` 的
  `ConnectionsTable`
- 类别：有意偏差（上游缺陷修复）
- 差异及必要性：ref 的内联 `ref={(node) => rowVirtualizer.measureElement(node)}`
  在当前 React 19 与 `@tanstack/react-virtual` 版本下会在每次渲染时创建新回调。
  React 重新绑定该回调时触发虚拟器测量，测量又派发状态更新，最终形成
  `ref` 重绑 → `measureElement` → 更新 → 重渲染的 Maximum update depth 循环。
  Chimera 改为直接传递虚拟器实例的稳定 `measureElement` 方法，保留 ref 的
  测量语义并阻断回调身份抖动。
- 复现证据：legacy 连接页面控制台堆栈指向
  `connections-table.tsx:440`、`Virtualizer.measureElement` 和
  `Maximum update depth exceeded`。
- 实际验证结果：`pnpm typecheck`、`pnpm lint:frontend-boundaries`、目标文件
  Prettier 检查和 `git diff --check` 均通过；尚未在本机运行桌面 E2E。
- 收敛、移除或重新评估条件：当 React/virtualizer 升级或 ref 连接表实现发生
  变化时，重新验证该差异；若上游改为稳定方法引用且回归测试覆盖，则可重新
  评估是否恢复完全一致。

## DIFF-012：Legacy 尽力导入订阅向导（第十阶段）

- ref commit：`f7dbce2997c633e484f54788035e770b3ee99773`
- ref 路径和符号：`frontend/nyanpasu/src/pages/(main)/main/profiles/_modules/profile-quick-import.tsx` 的 `ProfileQuickImport`
- Chimera 路径和符号：`frontend/chimera/src/components/profiles/quick-import.tsx` 的 `QuickImport`；
  `frontend/chimera/src/components/profiles/best-effort-subscription-import.tsx` 的
  `BestEffortSubscriptionImport`；legacy 路由
  `/(legacy)/subscription-onboarding`
- 类别：legacy UI / E2E / 临时迁移
- 差异及必要性：ref 没有对应的尽力导入向导。本轮保留原有 QuickImport 的默认行为，
  仅增加受控输入和导入回调适配；新增独立 legacy 页面，将默认导入、网络环境准备、
  地址可达性、稳定性和直连重试分别展示为持久区域，并保留失败恢复和成功后系统代理
  决策。旧 `/providers` 路由不再承载该页面。
- 共通业务入口及适配边界：配置写入复用 `useProfile().create` 和 `useSetting`；网络
  检测通过共享 `probe_network` application IPC；该 IPC 使用用户主动输入策略，允许
  合法的局域网订阅地址，而 agent 的 `network.probe` 保留严格的私网/特殊地址阻断并
  复用同一执行器；没有新增独立配置、持久化或内核实现。订阅导入通过
  `RemoteProfileImportMode` 显式区分 `default` 与 `direct`，后者只对本次抓取强制
  关闭代理，不改变最终保存的 Profile 选项。
- 影响的主界面、legacy UI、agent、数据、内核和平台：仅新增 legacy UI 入口及页面状态；
  现有主界面/legacy Profiles 流程、订阅数据格式、核心和 agent 既有入口保持不变。
  独立 E2E 已覆盖页面装配、五个持久区域、本地确定性 fallback 主路径、成功后的代理选择、
  直连失败和重试恢复；仍不证明公网上的真实网络质量、系统代理或 TUN 的主机副作用。
- 实际验证结果：`pnpm typecheck`、`pnpm lint:frontend-boundaries`、目标 oxlint、
  `git diff --check`、E2E suite 注册测试、`pnpm --filter chimera-ui build`、
  `cargo test ... network_probe`（5 passed）、`cargo test ... remote::tests`（2 passed）、
  binding freshness（1 passed）、`pnpm e2e:tauri:build` 和独立 legacy 桌面 E2E（3 passed）
  通过。对应 E2E 二进制为
  `backend/target/e2e/debug/chimera.exe`（SHA-256
  `CEFEEF4C5E2AB16E4C320315D5D26C80C4E3258F092824F2CDE7C4658032C5FF`）。Profiles 全套
  E2E 在既有 `profile-transform-chain-ui.e2e.ts` 断言处失败（实际为
  `applied`、期望 `committed_degraded`），未归因于本向导。
- 收敛、移除或重新评估条件：当前 application API 已区分导入模式并共享网络探测；
  下一步需要验证 `default` 是否应按产品定义自动使用当前系统/Chimera 代理，而不是
  继承 legacy 默认选项。随后补充代理/TUN 状态回读、真实网络和恢复测试，再重新评估
  当前 3 次探测、3000ms 波动阈值及失败恢复语义。当前仍是功能可用但对齐未完成的
  部分迁移。

## DIFF-013：在新 core manager 中保留 Chimera Client 身份与 CLI 支持

- ref commit：Clash Nyanpasu `main` 的当前 HEAD
  `5331747c06a5f42eeabb3e225a1e77a83f480549`
- ref 路径和符号：`backend/tauri/src/core/actor_v2/local_host.rs::core_spec`
  将上游 `ClashRs` / `ClashRsAlpha` 映射为 `CoreKind::ClashRust`；
  `backend/tauri/src/core/actor_v2/facade.rs::reconcile` 把该 spec 交给新 manager。
- Chimera 路径和符号：`backend/chimera-core-metadata/src/kind.rs::ClashCoreKind`、
  `feature/clash.rs::FeatureSupport`、`backend/chimera-core-manager/src/kind.rs`、
  `backend/chimera-core-manager/src/log.rs`。
- 类别：品牌兼容扩展。
- 差异及必要性：上游没有 Chimera Client。Chimera 保留独立 kind 和线上的
  `chimera-client` 名称；启动参数与 Clash-rs 共用 `-d/-c`，IPC endpoint 使用
  `--controller-ipc`，日志按已验证的 Clash-rs tracing header 解析。Chimera Client
  `clash-bin/src/main.rs` 明确提供 `-f` 到 `-c` 的兼容别名，因此现有一次性 `-t`
  检查参数也可用；`chimera` 和 `chimera_client` 旧值反序列化为同一 kind，写出时
  仍规范化为 `chimera-client`。Unix socket / named pipe capability 复用 Clash-rs 的版本门槛；
  TCP 禁用和 named-pipe security descriptor 仍报告不支持。
- 保留差异：应用设置和下载器仍使用既有 `ClashCore::ChimeraClient`、
  `chimera_utils::core::ClashCoreType::ChimeraClient` 与 `MFSGA/Chimera_Client` 发行源；
  不把它改名成 Clash-rs，也不改动核心二进制。本切片仅补齐新 manager 的 kind、CLI、
  IPC 能力和日志识别，没有把 Tauri 的 `CoreFacade` 切换到新 manager；现有
  `core runtime migration is not implemented yet` 仍待后续完整生命周期迁移处理。
- 测试契约：给定 Chimera Client kind、运行/配置路径与本机 IPC endpoint，断言序列化身份、
  启动参数、配置校验参数和 IPC 覆盖参数符合 Chimera Client 源码中的 CLI 合同；若将它
  折叠成 `ClashRust`、丢失品牌 wire 值或漏传 IPC flag，目标断言会失败。
- 实际验证结果：本 DIFF 首次检查时，manager 单测被本地 `nyanpasu-utils` 的三个缺失
  include 文件挡住：`find-macos-default-device-port.sh`、`set-macos-dns.sh`、
  `get-macos-dns.sh`。DIFF-014 已将该 macOS helper 接到有这些脚本的 Chimera Utils；随后
  `cargo test --manifest-path backend/Cargo.toml -p chimera-core-manager kind::tests`
  2 passed，`cargo check --manifest-path backend/Cargo.toml -p chimera-core-manager --lib`
  和 workspace Clippy 通过。现有 Chimera Client 包二进制执行 `-v` 得到
  `clash-rs 0.26.1`；`-h` 显示 `-c`、`-f` alias、`-t` 和 `--controller-ipc`；使用
  `-t -d /tmp/chimera-client-smoke-20260926 -f config.yaml` 对临时配置校验通过。
  这些是 CLI 兼容性冒烟证据，不代表桌面应用已接入新 manager 或 IPC socket 已运行验证。
- 下一最小步骤：在 ref 的 actor-backed `CoreFacade` 接入点将
  `ClashCore::ChimeraClient` 映射到独立 kind，并迁移一条完整 reconcile 启动路径，保持
  legacy UI、profile 持久化、agent 和现有下载更新合同。

## DIFF-014：将 macOS 网络 helper 接到 Chimera Utils

- ref commit：Clash Nyanpasu `main` `5331747c06a5f42eeabb3e225a1e77a83f480549`；
  Chimera Utils 依赖固定在 `17809a1ccded8caf83e4803f99ad8e7e29cdee20`。
- ref 路径和符号：`backend/nyanpasu-utils/src/network/mod.rs::macos`。
- Chimera 路径和符号：`backend/nyanpasu-utils/src/network/mod.rs::macos` 保留原 API，
  由 `chimera_utils::network::macos` 实现；权威 helper 与脚本位于
  `MFSGA/Chimera_Utils` 的 `network-utils/src/lib.rs` 和 `network-utils/src/scripts/`。
- 类别：品牌兼容 / 临时迁移。
- 差异及必要性：主 workspace 仍需 `nyanpasu-utils` 的 core、process 等模块，但它本地
  镜像里的网络模块引用了未检出的三个 macOS 脚本。Chimera Utils 已包含同名脚本和相同
  函数合同；将旧 `nyanpasu_utils::network::macos` 保留为薄适配层，减少重复实现并解除
  macOS all-targets 编译缺口。仅将网络能力委托给 Chimera Utils，不把仍被广泛使用的
  core/process 模块整体换名或删除。
- 共通业务入口及适配边界：该 crate 的既有函数路径和参数/返回类型不变；底层脚本由锁定的
  Chimera Utils 版本提供。现有三项 macOS 网络测试保留；本轮只编译，不在开发机上执行会
  修改系统 DNS 的用例。
- 影响范围：`nyanpasu-utils` Cargo feature `network`、macOS helper 兼容路径、manager 已有
  `tempfile` 测试依赖和 Cargo 锁文件；未改变 UI、agent、profile、运行时内核或服务协议。
- 实际验证结果：`cargo check --manifest-path backend/Cargo.toml -p nyanpasu-utils
  --all-features --all-targets` 通过；`cargo test --manifest-path backend/Cargo.toml -p
  chimera-core-manager kind::tests` 2 passed；`cargo check --manifest-path
  backend/Cargo.toml -p chimera-core-manager --lib` 通过；`cargo clippy --manifest-path
  backend/Cargo.toml --all-targets --all-features` 通过（有既有 warnings）；
  `cargo fmt --manifest-path backend/Cargo.toml --all -- --check` 与 `git diff --check`
  通过。系统 DNS 测试未执行，避免改动开发机网络设置。
- 编译发现并修正的本地 manifest 缺口：`chimera-core-manager` 的既有 quarantine 单测直接
  使用 `tempfile`，但原 Cargo 清单未声明 dev-dependency；本轮仅添加该测试依赖，未改测试
  或产品代码。
- 收敛条件：当 `nyanpasu-utils` 的剩余模块迁移完成后，移除此兼容 crate/路径；在此之前继续
  保持 legacy API，不为整体重命名而扩大本轮范围。

## DIFF-015：将本地 Normal core host 接入 CoreControl

- 本地基线：Chimera `ca4c29a8`；本轮参考基线：`ref/` commit
  `5331747c06a5f42eeabb3e225a1e77a83f480549`（Clash Nyanpasu `main` 的本地检出，非远端最新版声明）。
- ref 路径和符号：`backend/tauri/src/core/actor_v2/local_host.rs::build`、`core_spec`，以及
  `backend/tauri/src/core/actor_v2/facade.rs` 的 CoreControl 生命周期入口。
- Chimera 路径和符号：`backend/tauri/src/core/actor_v2/local_host.rs`、
  `local_runtime.rs::LocalRuntimeHost`、`facade.rs::CoreFacade` 和 `setup.rs::setup`；共享执行器为
  `chimera-core-manager::CoreControl`。
- 类别：本地运行时迁移 / 品牌兼容。
- 迁移切片：新增两个实现与测试模块，共 648 行；把 Normal 本地运行的配置生成、预检、
  reconcile、回滚识别、运行时产物提升、状态/API 信息投影和停止命令接至 CoreControl。
  `CoreFacade` 生产装配现在在 setup 中构造该 host；普通测试仍可使用旧构造器。
- Chimera 偏离及兼容：`ClashPremium`、`ClashRs`、`Mihomo` 与 alpha 选择映射到共享 kind；
  `ClashCore::ChimeraClient` 映射为独立 `CoreKind::ChimeraClient`，可执行文件继续从
  `chimera_utils::core::CoreType` 解析，不折叠成 `ClashRust`。Tauri 的运行时配置、端口解析和
  后处理仍调用 Chimera 原实现。停止核心使用 CoreControl 的 `Stop`，保留 executor 供后续启动；
  只在测试清理时关闭 executor。
- Service 边界：Service/Elevated 仍由现有兼容 `CoreManager` 分支处理，分支成功后才更新 host 类型；
  不在未实现的 Service host 迁移前抢先关闭本地核心。当前 `core::clash::CoreManager::rebuild_and_run_locked_with`
  仍是 `todo` 前的显式错误路径（`anyhow::bail!`），本切片没有宣称 Service 已迁移或可用。
- 测试契约：host 测试覆盖所有 `ClashCore` 品牌到二进制/kind 的映射、非 UTF-8 路径拒绝和隔离目录；
  runtime 测试覆盖初始状态投影、停止已停止核心的幂等性及 executor 保持开放、apply 回滚拒绝以及
  durability warning 包装。
- 实际验证：`cargo check --manifest-path backend/Cargo.toml -p chimera --lib` 通过；
  `cargo test --manifest-path backend/Cargo.toml -p chimera --lib core::actor_v2::local_host::tests -- --test-threads=1`
  4 passed；`cargo test --manifest-path backend/Cargo.toml -p chimera --lib
  core::actor_v2::local_runtime::tests -- --test-threads=1` 3 passed；
  `cargo fmt --manifest-path backend/Cargo.toml --package chimera -- --check` 与 `git diff --check` 通过。
  cargo 检查仍报告仓库已有 warnings；未启动桌面应用或实际 core 二进制，也未验证 macOS DNS/TUN 系统行为。
- 下一最小步骤：从 Service adapter 的宿主交接协议开始实现和测试 Service 模式，再决定是否将其纳入
  CoreControl；随后验证生产 setup、实际 Chimera Client 启动/停止和支持平台组合。

## DIFF-016：将共享工具实现迁入本地 chimera-utils crate

- Chimera 基线：`4f7d18bd890b8545e44e64c4c1adad0e70ee29a2`。
- ref 基线：`ref/` commit `5331747c06a5f42eeabb3e225a1a77a83f480549`，检出干净。
  该 ref 的 `backend/Cargo.toml` 将 `nyanpasu-utils` 指向
  `backend/nyanpasu-runtime/crates/nyanpasu-utils`；子模块 gitlink 为
  `f5b581fad8bf8272e222f1e3948c7826c6665bb6`，当前未初始化，因此本轮不能逐文件核对该
  子模块内容。实际迁移源是 Chimera 基线中已跟踪并被 workspace 使用的
  `backend/nyanpasu-utils`，不把它误报成已验证的 ref 子模块快照。
- 迁移映射：`backend/nyanpasu-utils/src/process/*`、`io/{mod.rs,atomic_fs.rs}`、
  `reqwest_ext.rs` → `backend/chimera-utils/src/` 对应路径；本地 crate 根模块重导出
  `chimera-platform-utils` 的 `core`、`dirs`、`network`、`os` 与 `runtime` API。
  原 `nyanpasu-utils` 的产品/平台模块改由现有 Chimera Utils 平台 crate 提供，避免复制
  第二份 core、目录和 OS 实现。
- 类别：本地共享 crate 迁移 / 兼容适配。
- 实际切片：迁入 3,179 行通用进程监督、epoch PID 文件、原子文件操作和命名管道重试实现；
  Cargo 将同源代码识别为文件移动。移除原本重复的 1,225 行 core/dirs/network/os/runtime
  包装实现，由本地 crate 有条件地重导出平台 API。新增 `process`、`reqwest` 等 feature 边界，
  同时保留上游平台包的 feature 开关。
- 调用方：`chimera-core-manager`、`chimera-clash-api`、`chimera-config` 和 Tauri 主应用都改依赖
  本地 `chimera-utils`。原平台包改用 `chimera-platform-utils` Cargo alias；它与 IPC 使用的
  `chimera_utils` 仍解析到相同 Git 包身份，因此 `CoreType` 等跨 crate 类型不被复制或转换。
  `chimera-core` 删除了无源码引用的旧 utility 依赖。
- 品牌与产品偏离：平台 `CoreType::ChimeraClient`、下载/配置映射及二进制未改；该迁移只改变
  utility crate 所有权与 Cargo 依赖名，不会把 Chimera Client 折叠成 Clash-rs。nested runtime
  子模块当前没有 Rust 源码引用 `nyanpasu-utils`，其 workspace manifest 仍声明未使用的旧 Git
  依赖；本轮不改写该独立子模块，以免产生需要另行发布的 gitlink 变更。
- 影响范围：Rust workspace 共享 utility 依赖；没有改 UI、legacy UI 交互、agent 行为、配置格式、
  Controller/Service 协议或核心启动参数。Windows 目标编译和真实子进程恢复行为尚未在本机验证。
- 实际验证：`cargo check --manifest-path backend/Cargo.toml -p chimera-utils --no-default-features
  --features process,reqwest` 通过；`cargo check --manifest-path backend/Cargo.toml
  -p chimera-utils` 通过；`cargo check --manifest-path backend/Cargo.toml -p chimera-core-manager
  -p chimera-clash-api` 通过；`cargo check --manifest-path backend/Cargo.toml -p chimera-config
  -p chimera` 通过；`cargo check --manifest-path backend/Cargo.toml --workspace` 通过；
  `cargo fmt --manifest-path backend/Cargo.toml --all -- --check` 与 `git diff --check` 通过。
  Tauri 检查使用仅在该工作树创建的 ignored `sidecar/`、`resources/` 链接和 `tmp/dist` 占位页；
  未执行测试套件或启动桌面/核心进程。Cargo 输出有仓库现存 cfg、unused 和 lifetime warnings，
  本轮未通过放宽 lint 隐藏它们。
- 下一最小步骤：初始化或隔离检出 ref 的 `backend/nyanpasu-runtime` gitlink 后，逐项核对
  `nyanpasu-utils` 子 crate 与本地 `chimera-utils` 的 `process` / atomic-fs API 演进差异；完成后
  再迁移其中实际仍被 Chimera 需要的新增通用能力，不迁入上游专属产品实现。

## DIFF-017：同步 ref 工具 crate 的进程与文件系统回归覆盖

- Chimera 基线：DIFF-016 提交 `e560d9716956aadd71016f9f16e2c7f7e2045d31`。
- ref 基线：根 `ref/` commit `5331747c06a5f42eeabb3e225a1e77a83f480549`；嵌套
  `backend/nyanpasu-runtime` gitlink `f5b581fad8bf8272e222f1e3948c7826c6665bb6`；其中
  `crates/nyanpasu-utils` gitlink `cd6c9d3821a8c943bc249d96d456e2bedffd3ada`（2026-09-14）。
  只初始化了 ref 侧子模块以只读比对，三个 ref worktree 均保持干净。
- 比对结论：本地已有的匹配 `process`、atomic-fs 和 `reqwest_ext` 生产实现与该 pin 一致；该
  utility commit 在这些文件中的新增内容是回归测试，而非新的运行时 API/行为。故本轮对齐测试和
  测试子进程，不复制 ref 的 `core`、`dirs`、`network`、`os`、`runtime` 产品/平台模块；这些 API
  继续从现有 Chimera platform crate 提供。
- 迁移范围：同步 atomic-fs、命令构造、进程引擎、错误、PID/epoch 恢复、Supervisor 和 named-pipe
  retry 的 ref 单元用例；新增全部 8 个进程集成测试及测试子进程。共迁入 1,351 行集成测试与辅助
  子进程源码，另同步源文件内的 ref 单元测试。把 crate/import 和测试二进制统一改为 Chimera 名称；
  保留独立于 Tauri 的本地进程 API 文档，不引用 Nyanpasu 设计文档。
- 本地修复与偏离：ref 的进程集成用例并行运行时，两个 legacy PID 文件用例可读到空文件。根因是
  `chimera-platform-utils::os::create_pid_file` 及 ref 的同类 helper 在 `write_all` 后未显式 flush；
  `PidFileGuard::write` 原先直接调用该 helper。现由本地进程适配层写完后 `flush().await` 再返回，确保
  启动调用方立刻读取时能看到 PID。修复后同一集成目标的默认并行运行通过；未改平台 crate 或 ref。
- 验证：`cargo fmt --manifest-path backend/Cargo.toml --all -- --check` 通过；
  `cargo test --manifest-path backend/Cargo.toml -p chimera-utils --features process,reqwest` 通过，
  执行 19 个单元测试、44 个进程集成测试和 1 个 doctest；`cargo check --manifest-path
  backend/Cargo.toml --workspace` 通过；`git diff --check` 通过。检查仍输出仓库既有 cfg、unused、
  lifetime warnings。Windows-only named-pipe 与 ACL 用例未在本机执行，未做 Windows 交叉编译。
- 下一最小步骤：对照 `chimera-platform-utils` 与本 ref pin 的目录、OS 和网络模块公开 API 及其调用方，
  只迁移 Chimera Client 实际缺少且不会复制 `CoreType::ChimeraClient` / 平台类型的通用行为；随后再决定
  是否需要独立的本地兼容实现。

## DIFF-018：恢复 core-manager 的 pinned-ref 单元回归覆盖

- ref 基线：Clash Nyanpasu 当前 root `main` 为 `926f0953b7384ab5e054972ca43f8a7dd983bb39`；Rust
  源码及 `backend/nyanpasu-runtime` gitlink 与已检查的 root `5331747c06a5f42eeabb3e225a1e77a83f480549`
  相同，runtime pin 为 `f5b581fad8bf8272e222f1e3948c7826c6665bb6`。root 后续差异仅是前端 `filesize`
  依赖升级，不包含 core-manager Rust 变更。两个参考 worktree 均只读。
- Chimera 基线：`6fe4c0f4391889255db96553a660bc07c24eb6d7`。
- ref 路径和符号：`backend/nyanpasu-runtime/crates/nyanpasu-core-manager/src/` 下 config、control、health、
  instance、kind、log、log_sink、manager、spec 的 `#[cfg(test)]` 模块，以及 `epoch.rs::epoch` 测试 helper。
- Chimera 路径和符号：对应的 `backend/chimera-core-manager/src/` 模块。只复制测试块；原生产实现保留。
  `nyanpasu_utils` / `nyanpasu_core_metadata` 测试导入映射为共享的 `chimera_utils` /
  `chimera_core_metadata` 包。`kind.rs` 的 ref 用例单独放入 `ref_tests`，保留本地 ChimeraClient 测试。
- 本地兼容保留：`CoreKind::ChimeraClient` 的独立 wire identity、CLI 参数和 Clash-rs tracing parser 映射未更改；
  新增一条日志用例断言 ChimeraClient 日志仍保留该 kind。补齐 ref 已有的空 `test-hooks` feature 声明，
  让已有条件编译路径可被 Cargo 识别；没有启用它作为默认 feature。
- 测试契约：纯单元边界分别检查配置投影/规范化、配置文件原子提交和恢复、操作 envelope、健康探测状态机、
  epoch 生命周期、各核心日志格式与有界 JSONL sink。期望值直接来自输出值、状态投影或隔离临时文件；若实现
  回退到缺失的 ref 用例覆盖之前的差异，相应的字段、状态或文件断言会失败。不会据此声称验证真实服务 IPC、
  桌面启动或真实网络。`deadline_drain_test_child` 是供子进程用例启动的 helper，单独标记 ignored。
- 实际迁移：恢复 16 个 ref 测试模块，约 3,053 行新增 Rust；生产实现仅新增 ref 的 test-only epoch helper，
  没有用 ref 覆盖本地 ChimeraClient 分支。
- 验证：迁移前 `cargo test --manifest-path backend/Cargo.toml -p chimera-core-manager --lib` 为 3 passed；
  迁移后 `cargo test --manifest-path backend/Cargo.toml -p chimera-core-manager --all-features` 为 106 passed、
  1 ignored，doc tests 0；`cargo fmt --manifest-path backend/Cargo.toml --package chimera-core-manager -- --check`
  和 `git diff --check` 通过。仍有 3 条既有 `unused` / `dead_code` warning；本轮没有放宽 lint。
- 未覆盖与下一步：未运行会启动实际 core 子进程的 ignored 用例、系统服务、TUN 或跨平台 Windows 测试。ref 的
  actor-backed Service endpoint 依赖 IPC v2；本地 `chimera-runtime` 仍是独立 Chimera Service v1.9.0（commit
  `2d9171da99c93775807359d63e783df505c447bc`）。迁移 actor 前需为服务协议制定保留现有 Chimera Service 与
  ChimeraClient 支持的兼容适配，并逐条接通 reconcile/stop 生命周期；当前 Service 运行链仍标记为部分迁移。

## DIFF-019：将 ref 通用工具实现收回本地 `chimera-utils`

- 基线：Chimera commit `6fe4c0f4391889255db96553a660bc07c24eb6d7`；Clash Nyanpasu root `main`
  `926f0953b7384ab5e054972ca43f8a7dd983bb39`、`backend/nyanpasu-runtime` pin
  `f5b581fad8bf8272e222f1e3948c7826c6665bb6`、其 `nyanpasu-utils` pin
  `cd6c9d3821a8c943bc249d96d456e2bedffd3ada`。Chimera 平台工具类型仍来自锁定的
  `MFSGA/Chimera_Utils` commit `17809a1ccded8caf83e4803f99ad8e7e29cdee20`。
- 迁移范围：将通用 `core::instance` / `core::utils`、目录、macOS 网络、OS 和 Tokio runtime
  实现放入 `backend/chimera-utils/src/`，共 1,199 行 Rust 源码，外加三份网络脚本。相应 Cargo
  feature 现在显式启用本地实现所需的依赖；Cargo.lock 移除由本 crate 不再直接使用的
  `dirs-utils`、`network-utils` 子包。
- 本地兼容与修复：`CoreType`、`ClashCoreType` 等 IPC 身份类型仍重导出同一个 Chimera 平台 crate，
  不改变 wire identity。CoreInstance 保留 Chimera 平台版已有的 builder 路径校验和优雅退出修复；
  适配器为 `ChimeraClient` 使用 Clash-rs 的 `-c` 参数并按对应日志格式解析校验输出。PID 文件写入后
  显式 flush，延续 DIFF-017 已验证的立即读取修复。没有复制会修改 macOS DNS 的上游集成测试。
- Feature 边界：本地 `core_manager` 现在显式依赖 `serde`，因为保留 identity 的 Chimera 平台类型在
  关闭 serde derive 时仍有无条件 `#[serde]` 属性；这组核心类型因此不支持无 serde 的组合。
- 验证：`cargo check --manifest-path backend/Cargo.toml -p chimera-utils` 通过；
  `cargo test --manifest-path backend/Cargo.toml -p chimera-utils --all-features` 共 66 项通过；
  `--no-default-features`、`--no-default-features --features core_manager`、
  `--no-default-features --features dirs,network,os` 和
  `--no-default-features --features process` 的 crate checks 均通过；
  `cargo check --manifest-path backend/Cargo.toml --workspace` 通过；`cargo fmt --manifest-path
  backend/Cargo.toml --all -- --check`、`git diff --check` 通过。workspace 输出约 306 条既有
  warning。当前仅安装 `aarch64-apple-darwin` target，Windows 编译未验证。
- 未覆盖与下一步：核心 IPC 类型身份未迁移，服务协议也未变；`chimera-runtime` 仍是 v1，ref 的
  Service actor/v2 控制端尚未接入。下一步须先做可并存的 v2 IPC 桥，再迁 Service actor，不能只复制
  actor 文件后留成不可调用实现。

## DIFF-020：同步 ref 最近的 `filesize` 依赖修订

- ref 基线：root `main` `926f0953b7384ab5e054972ca43f8a7dd983bb39` 将前端 `filesize` 从
  `11.0.23` 更新为 `11.0.25`；本地只对齐该项，其余 Chimera UI 依赖保留产品差异。
- 变更：更新 `frontend/chimera/package.json` 与 `pnpm-lock.yaml` 中 specifier、版本、integrity。
  未更新其它看似落后的 Nyanpasu 包：两套 UI 依赖集合存在大量有意的 Chimera 产品差异。
- 验证：标准 `pnpm install --filter=chimera-ui --frozen-lockfile` 被仓库 minimum-release-age
  策略拒绝，因为 `filesize@11.0.25` 在执行时尚未达到 12 小时；没有放宽策略。使用已缓存的本地
  Vite 可执行文件直接运行 `./node_modules/.bin/vite build`，生产构建成功并写入忽略目录
  `backend/tauri/tmp/dist`。构建仍报告现存的 Vite config、动态导入和大 chunk 警告；标准 pnpm
  安装在该新版本通过 release-age 检查前仍未验证。本地 `meta-json-schema` 仍为 `1.19.30`，
  ref 当前清单为 `1.19.31`；它不在上述 root 最近提交的变更中，本轮未混入这项旧差异。

## DIFF-021：迁入 additive Core IPC v2 wire schema

- Chimera 基线：根仓库 `05cc0805`；嵌套 `backend/chimera-runtime` 基线
  `2d9171da99c93775807359d63e783df505c447bc`。ref 基线：Clash Nyanpasu root
  `5331747c06a5f42eeabb3e225a1e77a83f480549`，`backend/nyanpasu-runtime` gitlink
  `f5b581fad8bf8272e222f1e3948c7826c6665bb6`。
- 迁移范围：新增 `chimera_ipc::api::core::v2` 的 submit、operation、status、effective-config
  与 API-connection 请求/响应模型及序列化回归测试；在 `status::CoreInfos` 加入 ref 对齐的
  optional instance/controller/health/revision/detail 投影。平台核心类型仍取自同一
  `chimera_utils::core::CoreType`，因此保留 `ChimeraClient` 的 wire 身份。此切片共新增 461 行。
- 兼容与限制：v1 endpoint、请求和响应字段均未改。旧 Service 目前对新 status 字段返回 `None`，
  `serde(default, skip_serializing_if)` 让旧 status JSON 继续可解码且不增加空字段。v2 模型目前
  是协议层，submit/operation/status routes 及客户端快捷方法尚未接入；不能据此认定 Service actor
  已可运行。
- 实际验证：`cargo test --manifest-path backend/chimera-runtime/Cargo.toml -p chimera-ipc`
  通过，6 passed、doc tests 0；`cargo check --manifest-path backend/chimera-runtime/Cargo.toml
  -p chimera-service` 通过；根 workspace `cargo check --manifest-path backend/Cargo.toml
  --workspace` 通过；runtime `cargo fmt --manifest-path backend/chimera-runtime/Cargo.toml --all
  -- --check` 与 runtime `git diff --check` 通过。根 workspace 输出约 306 条现存 warning。
- 下一切片：迁移 typed IPC client calls 和本地 `ControlEndpoint` adapter，随后才接入 Service
  actor；在 v1 服务的核心语义可以由 v2 操作协议安全表达前，保留现有 v1 服务和
  `ChimeraClient` 调用链。

## DIFF-022：补齐 Core IPC v2 typed client calls

- 基线：DIFF-021 runtime commit `8d535ca`；Clash Nyanpasu root `5331747c06a5f42eeabb3e225a1e77a83f480549`
  与 runtime pin `f5b581fad8bf8272e222f1e3948c7826c6665bb6`。
- 迁移范围：在 `chimera_ipc::client::Client` 添加 `submit_core`、`core_operation`、
  `core_status_v2`、`core_api_connection` 和 `effective_config_v2` 五个 typed 方法，以及
  对应成功 envelope aliases，共 67 行。请求/响应路径与 v2 DTO 共用同一 endpoint 常量；POST
  仍用 `simd-json` 编码，GET 无 body。原有 v1 methods 未动。
- 兼容限制：方法已在 IPC client feature 下可编译，但本地 Service 尚未挂载这些路由；调用方
  尚未切换到 v2。此前 v2 schema 的旧状态兼容测试继续保护 v1 JSON。
- 验证：`cargo test --manifest-path backend/chimera-runtime/Cargo.toml -p chimera-ipc
  --features client`，6 passed、doc tests 0；`cargo fmt --manifest-path
  backend/chimera-runtime/Cargo.toml --all -- --check` 和 `git diff --check` 通过。
- 下一切片：将 ref `ControlEndpoint` / Local CoreControl adapter 与 intent 构造迁入
  `actor_v2`，再把 Service endpoint 接上这些 typed calls；在 Service v2 handler 可提供完整
  reconcile 语义之前，不把本地 actor 默认切到 v2 Service。

## DIFF-023：迁入 CoreControl 的 endpoint port 与本地 adapter

- 基线：根仓库 `3c3a2840`；runtime IPC 已有 DIFF-021 schema 和 DIFF-022 typed calls；ref 仍为
  root `5331747c06a5f42eeabb3e225a1e77a83f480549` / runtime pin
  `f5b581fad8bf8272e222f1e3948c7826c6665bb6`。
- 迁移范围：新增 `actor_v2/control_endpoint.rs`，按 ref `actor_v2/endpoint.rs` 的公共 DTO、
  `ControlEndpoint` trait 和 `CoreControl` 本地实现迁入，包含 operation/outcome/status 投影、
  effective config、API connection、运行状态 watch 与 advisory config check。保留 Chimera
  `CoreControl` / `chimera_ipc` / `chimera_utils` 类型身份；控制器状态投影明确去除 URL 中的
  userinfo。新增 URL 凭据脱敏回归用例。新增模块约 451 行，另修正两个现有 Service 测试 fixture
  以填入 DIFF-021 的 optional `CoreInfos` 字段；总增量控制在单步 500 行内。
- 兼容与限制：旧 `actor_v2::endpoint::CoreStatusSnapshot` 留在原位，新增完整控制端口放在独立
  模块，避免改变当前 UI/CoreLifecycle 状态消费者。本地 adapter 目前提供可复用边界，但尚未由
  `CoreFacade` 接管，也未迁入 IPC v2 `ServiceEndpoint`；远端 Service 仍在 v1。
- 验证：`cargo check --manifest-path backend/Cargo.toml -p chimera` 通过；定向用例
  `cargo test --manifest-path backend/Cargo.toml -p chimera
  core::actor_v2::control_endpoint::tests::controller_status_projection_removes_embedded_credentials
  -- --exact --nocapture` 通过，1 passed、404 filtered；`cargo fmt --manifest-path
  backend/Cargo.toml --package chimera -- --check` 与 `git diff --check` 通过。Tauri check
  报告约 320 条既有 warning；新增测试首次发现的两个 `CoreInfos` fixture 缺字段已修复，复跑通过。
- 下一切片：迁入 ref `RuntimeIntentBuilder`，让生成文档、摘要和 CAS 输入形成单一 intent；之后
  再迁 `ServiceEndpoint` 与 service actor，并先补齐服务端 handler 才切换实际调用方。

## DIFF-024：迁入纯 RuntimeIntentBuilder

- 基线：根仓库 `8e9f321b`；ref `actor_v2/intent.rs` 来自 root
  `5331747c06a5f42eeabb3e225a1e77a83f480549` / runtime `f5b581fad8bf8272e222f1e3948c7826c6665bb6`。
- 迁移范围：新增 `actor_v2::intent`，将配置映射序列化成确定的文本，并用本地
  `chimera_core_manager::payload_digest` 生成请求 digest；输入还包含应用选择的
  `chimera_utils::core::CoreType` 与 `LocalIpcSettings`。保留 ref 的纯构造语义，不在 builder
  中读取全局状态或执行 I/O，共 95 行。
- 兼容与限制：intent builder 目前尚未接入 `CoreFacade` 调用链，旧 core 更新流程不变；保留
  `CoreType::ChimeraClient` 和应用本地配置类型。等本地与 Service endpoint 都准备好后，再统一从
  同一个 intent 生成 check 与 reconcile 请求。
- 验证：`cargo test --manifest-path backend/Cargo.toml -p chimera
  core::actor_v2::intent::tests -- --test-threads=1`，2 passed、405 filtered；
  `cargo fmt --manifest-path backend/Cargo.toml --package chimera -- --check` 和
  `git diff --check` 通过。Tauri 编译仍有约 323 条既有 warning。
- 下一切片：迁入 `ControlEndpoint` 的 Service IPC v2 adapter，并对照 Chimera 当前
  `ClientError`、websocket event 和 client features；Service 的 v2 handler 仍是 actor 接线前置条件。

## DIFF-025：补齐 IPC 响应的结构化错误元数据

- 基线：根仓库 `4b38dbcd`；runtime `1ccff94`；ref 为 Clash Nyanpasu root
  `5331747c06a5f42eeabb3e225a1e77a83f480549` / runtime pin
  `f5b581fad8bf8272e222f1e3948c7826c6665bb6`。
- 迁移范围：按 ref 为通用 IPC `R` envelope 增加可选 `error_kind` 与 `retryable`，并添加
  `RBuilder::other_error_with_kind`。字段在缺失时默认 `None` 且不序列化，因此 v1 成功响应的
  JSON 不变，旧服务响应仍可被新客户端解析。IPC 子模块独立构建，故 wire 层保留开放字符串，
  不反向依赖应用侧 `chimera-core-metadata`；调用端负责用本地枚举识别已知值、保留未知值。
- 兼容及限制：本切片只增加 envelope 能力；现有服务路由仍调用 `other_error`，尚未发出分类，
  Tauri client 也尚未映射这两个字段。没有切换 v1/v2 endpoint 或修改 ChimeraClient 启动流程。
- 验证：`cargo test --manifest-path backend/chimera-runtime/Cargo.toml -p chimera-ipc
  --features client,server,specta`，9 passed、doc tests 0；runtime `cargo fmt --manifest-path
  backend/chimera-runtime/Cargo.toml --all -- --check` 和 runtime `git diff --check` 通过。
- 下一切片：让 IPC client 错误类型暴露服务端错误元数据，并实现 Tauri `ServiceEndpoint` 的
  wire 映射；之后迁入服务端 `CoreControl` bridge 和 v2 handlers。只有服务端具备 ref 的排队、
  幂等与状态查询语义后，才把生产 Service 调用切至 v2。

## DIFF-026：IPC client 暴露服务端错误元数据

- 基线：根仓库 `5df71e76`；runtime `a191e67`；ref 仍为 root
  `5331747c06a5f42eeabb3e225a1e77a83f480549` / runtime pin
  `f5b581fad8bf8272e222f1e3948c7826c6665bb6`。
- 迁移范围：在本地 `ClientError` 上添加 `server_error_kind()` 与
  `server_retryable()`，从既有 `ServerResponseFailed` envelope 读取原始字符串和可选 bool；未知 kind
  原样交给应用调用方，transport error 明确返回 `None`。保持 `ClientError` 当前结构和 v1 请求签名，
  没有引入新依赖或把 app 层 metadata crate 拉入独立 runtime 子模块。
- 验证：`cargo test --manifest-path backend/chimera-runtime/Cargo.toml -p chimera-ipc
  --features client`，11 passed、doc tests 0；runtime fmt check 通过。测试时观察到 3 条既有
  server-only helper unused warnings。
- 下一切片：在 Tauri `actor_v2` 增加 Service `ControlEndpoint`，把本地 manager 错误分类与 v2
  DTO 映射到 Chimera wire 类型。websocket client/events 和 Service v2 routes 仍需分别迁移；当前
  endpoint 不会被生产调用链选用。

## DIFF-027：迁入 IPC v2 Service endpoint adapter

- 基线：根仓库 `5df71e76`；runtime `188945a`；ref root
  `5331747c06a5f42eeabb3e225a1e77a83f480549` / runtime
  `f5b581fad8bf8272e222f1e3948c7826c6665bb6`。
- 迁移范围：新增 `actor_v2::service_endpoint::ServiceEndpoint`，按 ref 把 reconcile/stop/recover
  envelope 转为 Chimera IPC v2 DTO，并接入 submit、operation long-poll、status、effective config、
  API connection typed calls；server error 通过前两步加入的 envelope 字段映射为本地错误类型，不解析
  message 文本。status 缺少详细状态时维持 `None`，不从 v1 粗状态伪造停止或启动证明。
- Chimera 差异：`CoreKind::ChimeraClient` 显式映射至同一
  `chimera_utils::core::CoreType::Clash(ChimeraClient)`，不降级成 `ClashRust`。当时 IPC/service
  尚无 config-check 路由，因此 adapter 返回 `Unsupported`；该限制已在 DIFF-028 补齐。
  IPC websocket event client 仍未迁入，adapter 暂沿用 trait 默认无事件流。
- 上游协议缺陷及修正：ref 的 IPC 注释和序列化样例声称 operation id 是连续 32 位 hex，但同一
  ref 的 `CoreControl::OperationId` parser/display 合同实际要求 `16-8-8` 分段形式。连续样例会被
  Service parser 拒绝。本地 DTO 注释/样例改为 manager 实际接受的格式；隔离工作区 `ref/` 未修改。
- 兼容与限制：此记录完成时仅导出可构造的 adapter，没有修改 `CoreFacade` 的 host 选择或生产调用
  路径；随后服务端 v2 bridge/handlers 与 Stop/API 读取接线见 DIFF-028。Service reconcile 和 core
  selection 仍走 legacy manager，不把适配器存在视为完整 host 迁移。
- 验证：`cargo check --manifest-path backend/Cargo.toml -p chimera` 通过；
  `cargo test --manifest-path backend/Cargo.toml -p chimera
  core::actor_v2::service_endpoint::tests -- --test-threads=1`，2 passed、407 filtered；
  两个 workspace 的 `git diff --check` 通过。Tauri build 报约 329 条既有 warning。

## DIFF-028：接入 Chimera Service CoreControl bridge 与 v2 Stop

- 基线：根仓库 `65625161bbaa7f150b5451eeb55d1c249408ba42`；嵌套
  `backend/chimera-runtime` 为 `ba2aee491a6ebad0aa1a3e2955a78304c9f4bfee`。只读 ref 为 root
  `5331747c06a5f42eeabb3e225a1e77a83f480549`、runtime
  `f5b581fad8bf8272e222f1e3948c7826c6665bb6`。
- ref 对应：`backend/nyanpasu-runtime/crates/nyanpasu-service-runtime/src/server/manager_bridge.rs`、
  `server/routing/core/v2.rs`、`backend/tauri/src/core/actor_v2/endpoint.rs::ServiceEndpoint` 与
  `facade.rs::stop/command`。Chimera 保留 `chimera-*` 包名、`ChimeraClient` CoreType 身份和现有
  v1 `Log(TraceLog)` 事件扩展。
- 迁移范围：Service 进程接入 manager-backed submit、operation 查询、详细状态、effective config、API
  connection 与 staged `/core/check` handlers；复制并适配 ref 的 manager bridge、事件投影、控制器访问、
  Unix/Windows pipe ACL。Tauri `ServiceEndpoint` 实现了 staged config check；生产 `CoreFacade` 在
  `RunType::Service` 下把 Stop 改为 v2 submit/wait，并通过 v2 API connection 生成兼容的
  `ClashInfo`，保留当前产品端口字段及独立 secret。丢失操作终态、ID 不匹配或未知操作结果时锁定
  `outcome_uncertain`，不把未确认停止当作成功。
- 当前边界：Service reconcile、core selection、daemon 生命周期仍经 legacy manager；IPC client
  websocket event stream 尚未迁移；本机 macOS 构建未验证 Windows ACL/pipe 分支，也未实际启动
  daemon 做跨进程路由检查。DIFF-027 的 `/core/check` 缺口已消除；尚不能称 Service host 完整对齐。
- 验证：`cargo check --manifest-path backend/chimera-runtime/Cargo.toml -p chimera-service` 与
  `cargo build --manifest-path backend/chimera-runtime/Cargo.toml -p chimera-service` 均通过；
  `cargo fmt --manifest-path backend/Cargo.toml --all` 通过（stable rustfmt 对配置中的 nightly-only
  选项发出提示）；`cargo check --manifest-path backend/Cargo.toml -p chimera` 通过，输出 323 条
  现存 warning。未运行测试。
- 后续进展：Service reconcile 与生产 facade 接线见 DIFF-029；仍需迁移 websocket event client，
  并在支持的平台验证服务端 v2 路由与 ChimeraClient core identity。

## DIFF-029：生产 LocalRuntimeHost 共用 Service v2 reconcile

- 基线：根仓库 `00e573e2d158d08c1f12b2c927bc8c8f4908c73d`；嵌套
  `backend/chimera-runtime` `646ab569cf649a50d342a5a3c6535470a0c7a778`。只读 ref 为 Clash
  Nyanpasu root `5331747c06a5f42eeabb3e225a1e77a83f480549`、runtime pin
  `f5b581fad8bf8272e222f1e3948c7826c6665bb6`；`ref/` 工作树检查为 clean。
- ref 对应：`backend/tauri/src/core/actor_v2/facade.rs::reconcile`、
  `actor_v2/endpoint.rs::ControlEndpoint::{check_config,submit,wait_operation,status}`、
  `actor_v2/endpoint.rs::ServiceEndpoint::check_config`、`client/runtime.rs::RuntimePaths`。
  本地落点为 `actor_v2/facade.rs` 与 `actor_v2/local_runtime.rs`。
- 迁移范围：生产 `CoreFacade` 对 Normal 选择 `LocalEndpoint`、对 Service 选择
  `ServiceEndpoint`；从本地切到 Service 前停止本地 core，从 Service 切回本地前以 v2 Stop
  确认旧 host 已停。`LocalRuntimeHost` 统一生成配置、写私有 candidate、调用 host check，随后读取
  status revision 并作为 `expected_applied` CAS 提交；校验操作 ID、long-poll 终态、失败与 rollback，
  只在确认应用后提升并发布 Chimera runtime snapshot。Service 的配置状态、活动 API connection、
  core selection 与快照读取也改为走相同 facade/host。无法确认 submit/wait 结果时锁定
  `outcome_uncertain`；未发布详细状态、运行中没有 revision、零 epoch 或 transitional 状态均拒绝提交。
- 保留的本地差异：继续用 `Config::render_runtime_bytes` 生成包含 Chimera header 的原始字节，check、
  digest 与 reconcile 共用完全相同的字节；未用 ref `RuntimeIntentBuilder` 直接序列化 Mapping，避免
  改变 Chimera runtime product 的格式。提交显式携带 `CoreType::ChimeraClient`，不将其折叠成
  `ClashRust`。Service staged check 沿用 ref 的共享文件路径方案；Windows service 对该 candidate
  的可读 ACL 和跨进程完整运行尚未在本机验证。`CoreFacade::new_local()` 的无生产 host 兼容入口仍
  保留 legacy manager 流程；`RunType::Elevated` 当前尚未实现，继续明确报错。
- 受影响入口：主 UI、legacy UI 和 agent 的 core lifecycle 继续汇入同一个 `CoreFacade`；本轮仅做
  `chimera` 包编译，没有桌面交互或实际 Service daemon 启动验证。IPC websocket event client 仍待迁移，
  Service 的事件订阅不因本轮而声称对齐。
- 验证：`cargo fmt --manifest-path backend/Cargo.toml --package chimera -- --check` 通过；
  `cargo check --manifest-path backend/Cargo.toml -p chimera` 通过，输出 312 条 warning；
  `cargo check --manifest-path backend/Cargo.toml -p chimera --tests` 通过，输出 234 条 test-build
  warning；`git diff --check` 通过。未运行测试或跨进程/Windows 检查。
- 后续进展：IPC event client 和 Service `api_changes` 适配见 DIFF-030；应用侧状态事件消费和 Service
  跨进程验证仍待完成。

## DIFF-030：迁入 Chimera IPC WebSocket EventStream

- 基线：根仓库 `77968039`；嵌套 `backend/chimera-runtime` `646ab569cf649a50d342a5a3c6535470a0c7a778`。
  只读 ref 仍为 Clash Nyanpasu root `5331747c06a5f42eeabb3e225a1e77a83f480549`、runtime pin
  `f5b581fad8bf8272e222f1e3948c7826c6665bb6`。
- ref 对应：`nyanpasu_ipc/src/client/shortcuts.rs::Client::events/EventStream`、
  `client/mod.rs::ClientError`、`actor_v2/endpoint.rs::ServiceEndpoint::api_changes`。本地对应
  `chimera_ipc/src/client/shortcuts.rs`、`client/mod.rs` 和 Tauri
  `actor_v2/service_endpoint.rs`。
- 迁移范围：新增基于 interprocess local socket 的 typed WebSocket `EventStream`，解码现有 Chimera
  `Event` DTO；处理 ping/pong，单帧解码错误保留为该帧错误，传输错误结束流。Windows 只对连接前
  `ERROR_PIPE_BUSY` (231) 使用 50–200ms 指数退避、最多 1 秒的重试；握手失败或已建立流不会重放。
  `ServiceEndpoint.api_changes()` 复用该流并仅投影 `CoreStatusChanged`，过滤其它现有事件变体。
  新增的 `backon` 与 `tokio-tungstenite` 仅由 IPC `client` feature 启用。
- 当前边界：endpoint 现在可以向后续 API authority/monitor 提供变化信号，但 Chimera 尚无 ref
  `ApiLease` 对应的消费任务；当前 WS connector 仍只在 facade 的成功 core mutation 后显式 restart。
  所以本切片不声称已完成 Service core 重启后的自动 API binding 变更响应。Windows named pipe 的
  busy retry、握手以及实际状态事件均未在目标系统/跨进程运行验证。
- 验证：`cargo check --manifest-path backend/chimera-runtime/Cargo.toml -p chimera-ipc --features client`
  通过；同 workspace `cargo check -p chimera-service` 通过；根 `cargo check --manifest-path
  backend/Cargo.toml -p chimera` 通过，输出 312 条 warning。两侧受影响包的 rustfmt check 与根、
  嵌套 `git diff --check` 通过。未运行测试。
- 下一步：把 `api_changes()` 接入 API binding monitor，使 Service 端 status event 能触发
  controller/secret 复核与 WS connector 重建；再做真实 Service 启停及 IPC 事件流验证，覆盖旧连接撤销、
  短暂断线、订阅结束和 host 切换。API monitor 的迁入见 DIFF-031。

## DIFF-031：消费 Service API 绑定变更并刷新 WS connector

- 基线：根仓库 `b4a15998`；嵌套 `backend/chimera-runtime` `72d79a7`。只读 ref 为 Clash Nyanpasu
  root `5331747c06a5f42eeabb3e225a1e77a83f480549`、runtime pin
  `f5b581fad8bf8272e222f1e3948c7826c6665bb6`。
- ref 对应：`backend/tauri/src/core/actor_v2/api.rs::ApiLease` 通过 API lifecycle stream 唤醒、每两秒
  校验 instance-bound API capability，并在事件流失败或绑定失效时撤销 capability。本地新增的
  `CoreFacade::monitor_service_api_binding` 使用 `ServiceEndpoint::api_changes()` 作为快速信号，并每两秒
  读取 Service authoritative `CoreApiConnection`；控制器或 secret 改变时调用既有 WS connector restart，
  查询持续失败时只触发一次重连。API stream 订阅和绑定查询均有 10 秒上限；流断开、出错或 host 离开
  Service 时丢弃订阅并重新评估。
- 本地适配及边界：仅在生产 `LocalRuntimeHost` 存在时于 Tauri setup 启动 monitor，且只在选中 Service
  host 时订阅。当前应用的 HTTP API client 是短生命周期按调用构造，因此对齐点是失效后的 WS stream
  重建；这不是 ref `ApiLease` 的逐请求 preflight/postflight、clone-wide 撤销实现，也不声称覆盖所有
  旧 HTTP 请求的取消语义。保留 `ChimeraClient` 身份、现有 connector 和 facade 入口，不改品牌或协议。
- 验证：`cargo fmt --manifest-path backend/Cargo.toml --package chimera -- --check`、
  `cargo check --manifest-path backend/Cargo.toml -p chimera` 及 `git diff --check` 通过；编译输出
  311 条既有 warning。未运行测试；本机尚未完成 Service daemon 跨进程事件验证，Windows named-pipe
  分支也未运行。
- 下一步：用真实 Service 启停和 API 地址/secret 变化验证 IPC 事件、轮询兜底与 WS 重建；评估是否需要
  在 Chimera 的长生命周期 API 使用者中迁入 ref `ApiLease` 的 capability 撤销模型。

## DIFF-032：profiles clean-schema 接受规范更新间隔字段

- 基线：本次只读 `ref/` commit `ed1931f66e08d2d233a9463c73b2376fd09a0a62`，工作树 clean。
- ref 对应：`backend/tauri/src/core/migration/modules/profiles.rs::migrate_remote_options`；
  Chimera 对应 `backend/tauri/src/core/migration/modules/profiles.rs::migrate_remote_options`。
- 类别：缺陷修正。
- 差异及原因：ref 的允许字段列表只接收旧名 `update_interval`，但目标 Profile 模型使用
  `update_interval_minutes`。用户启动日志复现了仍处于旧 Profile item 结构的记录携带该规范字段，导致
  `profiles/clean_schema` 在升级时拒绝启动。Chimera 的迁移输入现在兼容旧名与规范名；同时出现时仅接受
  数值相同的值，冲突显式失败，迁移输出仍只写规范字段，零值限制保持不变。
- 共通业务入口及适配边界：只影响启动时、ProfilesClient 构造前的 YAML schema migration；不改变主 UI、
  legacy UI 或 agent 的 Profile API。失败仍在写回原文件前返回，已有备份/恢复流程不变。
- 重新评估条件：ref 的 schema migration 允许旧 item 携带规范字段，或上游不再支持这种混合状态时，重新
  对照该输入兼容规则，并在可以移除兼容输入时收敛。
- 验证：`cargo test --manifest-path backend/Cargo.toml -p chimera --lib
  core::migration::modules::profiles::tests::` 通过（32/32）；其中新增字段映射、冲突和完整
  `run_clean_schema` 用例通过。
  `cargo fmt --manifest-path backend/Cargo.toml --package chimera -- --check` 与 `git diff --check` 通过。
  编译输出有现存 warning；未运行桌面启动或读取/修改用户的实际 profiles.yaml。

## DIFF-033：拒绝不兼容的 Windows Service 协议版本

- 基线：根仓库 `fedf3b33d061d73ef0170dd45e0a12b62d7ab51c`；嵌套
  `backend/chimera-runtime` `0309d538646aff7aa37ad338483a0909f90e348c`；只读 `ref/` 为
  `ed1931f66e08d2d233a9463c73b2376fd09a0a62`，工作树 clean。
- ref 对应：`backend/tauri/src/core/service/compat.rs::ServiceCompat::classify` 和
  `actor_v2/endpoint.rs::ServiceEndpoint::{check_config,submit,status}`。Chimera 对应
  `backend/tauri/src/core/service/compat.rs::ServiceCompat::classify`、
  `backend/tauri/src/core/actor_v2/service_endpoint.rs` 及嵌套 runtime 的
  `chimera_service/src` 路由。ref 使用显式最低 Service 版本拒绝不兼容 daemon；本地原先只检查
  major version，未能检测同为 v1 但 endpoint 集合不完整的旧发行版。
- 故障与修正：截图中的 Windows Service 返回 HTTP 404、空 body。当前客户端 reconcile 在检查配置时
  调用 `/core/check`，而已安装的 1.10.0 daemon 没有该 endpoint，故 legacy runtime patch 无法完成。
  将配套 `chimera-service` 和 `chimera-ipc` 工作区版本提升为 1.10.1，并把兼容下限设为 1.10.1；
  1.10.0 及其它低版本或非 v1 daemon 会被标为 incompatible，服务模式开关不允许启用，并展示当前与要求版本。
  主 UI 和 legacy UI 复用 `useSystemServiceMode` 中的版本判断与提示。兼容结构的 `required_min` 已通过
  现有 IPC binding generator 更新。
- 保留的 Chimera 差异：ref 当前使用 v2 release 线；Chimera Service 仍处于 v1 产品线，因此下限按本地
  `backend/chimera-runtime` 中包含所需 control endpoints 的源码版本设为 1.10.1，没有照搬 ref 的版本号。
  `MFSGA/Chimera_Service` 的 v1.10.1 已发布（[release](https://github.com/MFSGA/Chimera_Service/releases/tag/v1.10.1)；
  [发布工作流](https://github.com/MFSGA/Chimera_Service/actions/runs/36474524306) 的 11 个平台目标均成功，
  上传 22 个二进制与 SHA-256 资产）。`pnpm prepare:check` 此前命中本地版本戳缓存，没有验证干净 checkout
  下载。重新评估条件：完成真实 Windows Service 安装/更新和重启后的 named-pipe 跨进程验证，并在
  endpoint 合约变化时重新核对最低版本。
- 受影响入口：主 UI、legacy UI 的系统服务模式开关；共用 `ServiceCompat` 和 `CoreFacade` runtime 路径。
  本轮没有启动应用、安装/替换 Windows 服务或验证主机权限与 named pipe ACL；用户现有服务需在新应用构建后
  更新到兼容版本才能启用。
- 验证：`cargo test --manifest-path backend/Cargo.toml -p chimera --lib
  core::service::compat::tests::` 通过（7/7）；`cargo build --manifest-path backend/Cargo.toml -p chimera`
  通过（225 条既有 warning）；bindings generator test 通过（1/1）；
  `cargo build --manifest-path backend/chimera-runtime/Cargo.toml -p chimera-service --bin chimera-service`
  通过，生成 Windows x86_64 v1.10.1；`chimera-service update --check` 只读确认当前安装版为
  1.10.0，并计划 stop/replace/start，没有实际更新服务；`pnpm prepare:check` 通过并命中本地
  v1.10.1 sidecar cache（未验证远程 release asset）；
  `cargo check --manifest-path backend/chimera-runtime/Cargo.toml -p chimera-service --bin chimera-service`
  通过；`pnpm typecheck`、`pnpm --filter=chimera-ui build`、`pnpm lint:frontend-boundaries`、根
  `cargo fmt --manifest-path backend/Cargo.toml --package chimera -- --check` 及两侧
  `git diff --check` 通过。补齐 Service 测试所需的 `tempfile` 开发依赖并移除过期的
  `ClashCoreType::Meow` 测试项后，`cargo test --locked --manifest-path backend/chimera-runtime/Cargo.toml
  -p chimera-service --bin chimera-service` 通过（48/48）。发布后再次运行
  `cargo check --locked -p chimera` 时，Tauri build script 遇到 Windows `Access is denied`，未完成根应用构建。
  Service CI 中三平台 Clippy 均通过；该次整体 workflow 因 macOS/Windows lint fixer 并发推送同一 rustfmt
  修复而失败，自动格式提交 `87349079358cbdaf8c2557506bc20f049903aaf4` 已合入 Service `main`。

## DIFF-034：快捷键基础设施第一阶段

- 基线：只读 `ref/` commit `ed1931f66e08d2d233a9463c73b2376fd09a0a62`，工作树 clean；该 commit
  与本指南此前记录的 `5331747c06a5f42eeabb3e225a1e77a83f480549` 不同，本轮按实际检出的
  `ed1931f…` 读取，没有更新 `ref/`。
- ref 对应：`backend/tauri/src/client/hotkey/{ports.rs,actor.rs,adapters.rs,mod.rs}` 的
  `HotkeyAction`、`HotkeyBindings::{parse,diff}`、`HotkeyActor`、`HotkeyClient`、
  `TauriShortcutRegistrar`、`ChannelActionSink`、`TauriWindowControl`；以及
  `setup.rs::hotkey_action_pump`、`client/hotkey/mod.rs::dispatch_hotkey_action` 和
  `client/effects/executor.rs::apply_hotkeys`。
- Chimera 对应：新增 `backend/tauri/src/client/hotkey/{ports.rs,actor.rs,adapters.rs,mod.rs}`；
  由 `backend/tauri/src/setup.rs` 构造 actor 与 channel、启动 action pump，
  `backend/tauri/src/client/mod.rs::ChimeraClient::try_new_with_args` 在加载 typed application
  state 后注册持久化绑定；`backend/tauri/src/lib.rs` 在进程退出时释放快捷键。
- 类别：临时迁移 / Chimera adapter。领域类型、解析规则、canonical accelerator、差异计算、Actor
  所有权、Pressed-only 回调和非阻塞 channel 沿用 ref。动作适配继续调用 Chimera 已有的
  `patch_verge`、`patch_clash_overrides` 和窗口 helper，以保留当前 legacy config、Clash、TUN、系统代理
  与窗口实现。新增的快捷键错误信息覆盖 `backend/tauri/locales/{en,ru,zh-cn,zh-tw}.json`。
- 差异及边界：本地已有 `EffectKind::Hotkeys` 和配置 effect plan，但 `ApplicationEffectExecutor` 尚未迁入
  或接入生产。此阶段直接从 `ChimeraClient` 启动时 reconcile，且只读取 typed application 中已持久化的
  `hotkeys`；设置 IPC、前端录入、验证后保存及运行期变更后的 effect reconcile 尚未迁移。当前主界面和
  legacy UI 因而还不能配置快捷键。本阶段为可执行的后端基础，不代表快捷键功能已完成对齐。
- 影响的主界面、legacy UI、agent、数据、内核和平台：主/legacy UI 与 agent 入口未增加快捷键设置；按键动作
  复用 Chimera 的共享配置/运行时 mutation path。配置 schema 不变，读取已有 typed `hotkeys` 列表。只在非
  Android/iOS 桌面目标编译全局快捷键能力；本轮未做真实 OS 注册或业务动作验证。
- 验证：`cargo fmt --manifest-path backend/Cargo.toml --package chimera` 通过；`git diff --check`
  通过。首次 `cargo check --manifest-path backend/Cargo.toml -p chimera` 在 Windows Tauri build script
  中以 `Access is denied` 失败；同一 `cargo check` 指定独立 `--target-dir .tmp/hotkey-check-target`
  后通过，`chimera` 编译报告 225 条 warning。未新增或运行测试、未运行桌面应用。
- 提交：快捷键基础设施已提交为 `351ecff72`。
- 后续配置与界面切片见 DIFF-035；后续的 Hotkeys effect actor 接入见 DIFF-036。

## DIFF-035：快捷键配置 IPC 与主设置页

- 基线：只读 `ref/` commit `ed1931f66e08d2d233a9463c73b2376fd09a0a62`，工作树 clean；未更新
  `ref/`。
- ref 到 Chimera 的路径/符号映射：

  | ref | Chimera | 本次动作 |
  | --- | --- | --- |
  | `backend/tauri/src/ipc.rs::{get_hotkey_functions,get_hotkeys,set_hotkeys}` | `backend/tauri/src/ipc.rs` 同名命令 | 使用既有 `ChimeraClient` 与 typed application config |
  | `client/mod.rs::patch_app_config` 的提交前 parser 校验与提交后 effect | `client/hotkey/mod.rs::ChimeraClient::set_hotkeys`、`client/application.rs::ApplicationClient::patch_typed` | 先验证，再持久化；随后直接 reconcile hotkey owner |
  | `frontend/interface/src/hooks/{use-hotkeys.ts,use-hotkey-functions.ts}` | `frontend/interface/src/hooks/` 同名 hooks | 复用本地 React Query 与 hotkey IPC descriptors |
  | `frontend/nyanpasu/src/utils/parse-hotkey.ts` | `frontend/chimera/src/utils/parse-hotkey.ts` | 保留按键录入解析 |
  | `frontend/nyanpasu/src/pages/(main)/main/settings/nyanpasu/_modules/hotket-manager.tsx` | `frontend/chimera/src/pages/(main)/main/settings/chimera/_modules/hotket-manager.tsx` | 保留录入、清除和保存交互，按 Chimera 品牌设置路径接入 |

- 类别：临时迁移 / Chimera UI 路径映射。Specta bindings 已通过既有 generator 更新。`set_hotkeys`
  在持久化前运行插件 accelerator parser、modifier、action 与 canonical 重复绑定校验；提交后让
  `HotkeyClient` reconcile。部分 OS 注册失败通过 `MutationOutcome::CommittedDegraded` 返给调用方，配置仍保持
  已提交。该切片当时使用直接 reconcile 桥接；它已在 DIFF-036 中替换为效果 actor 路径。
- 共通入口与差异：快捷键配置仍是 typed application 中原有的 `{action},{accelerator}` 字符串列表，schema
  未变。该切片当时尚未把生产 executor 接到 application commit 通知，因此 `ChimeraClient::set_hotkeys`
  直接调用 hotkey actor，失败也没有自动重试。此桥接已在 DIFF-036 由共享 effect actor 接管；Hotkeys 以外的
  application effect owners 和提交通知仍待迁移。
- 受影响入口：主 UI 的 Chimera 设置页新增快捷键卡片，并通过共享 interface hooks 调用 generated IPC。
  legacy UI 的页面、窗口入口和既有交互未改；它没有对应的快捷键设置入口，新增 IPC 仍读写同一 typed config。
  agent 没有快捷键工具，不增加 agent 能力或第二套配置逻辑。Android/iOS 分支返回空功能列表并拒绝写入；本轮
  只编译桌面目标，没有验证 Android/iOS app 构建。
- 验证：现有 Specta `update_typescript_bindings` generator 执行成功（单独运行生成器，不是功能测试套件）；
  `pnpm typecheck`、`pnpm lint:frontend-boundaries`、`cargo fmt --manifest-path backend/Cargo.toml --package
  chimera -- --check` 通过。未新增或运行功能测试，未启动主/legacy 界面，未实测操作系统快捷键注册、占用冲突、
  退出释放或运行期恢复。
- 下一步：把 hotkey reconcile 接入生产共享 `ApplicationEffectExecutor`，验证 degradation 重试和 post-commit
  状态；DIFF-036 已迁入 Hotkeys 的效果 actor 与限次重试，完整多效果 executor 和其它提交入口通知仍待迁移。
  之后注册覆盖这条路径的单元/UI fixture 与真实桌面 E2E，并分别记录其实际执行结果。

## DIFF-036：快捷键接入效果 actor 与自动重试

- 基线：只读 `ref/` commit `ed1931f66e08d2d233a9463c73b2376fd09a0a62`，工作树 clean；未更新
  `ref/`。
- ref 到 Chimera 映射：`ref/backend/tauri/src/client/effects/{actor.rs,executor.rs,ports.rs}` 的
  `EffectsClient`、`EffectsActor`、`ApplicationEffectExecutor::apply_hotkeys` 与关闭时 `unregister_all`；
  对应 `backend/tauri/src/client/effects/actor.rs::EffectsClient::reconcile_hotkeys`、
  `backend/tauri/src/client/effects/executor.rs::HotkeyEffectExecutor`、
  `backend/tauri/src/setup.rs` 的 adapter composition，以及 `client/hotkey/mod.rs::set_hotkeys`。
- 类别：临时迁移 / Chimera adapter。启动 reconcile 和快捷键保存现在都通过共享效果 actor 的 Hotkeys
  effect group 执行；actor 管理 revision、旧请求 superseded、失败状态和 `RetryBudget` 的限次自动重试。
  配置提交仍先完成，首次 OS 注册失败会作为 `CommittedDegraded` 返回，同时 actor 按共享 backoff 重试。
  对同一 effect group 的保存调用由 hotkey mutation mutex 串行化。
- 保留差异：当前 `HotkeyEffectExecutor` 只接受 actor 筛选后的 Hotkeys plan，其它 effect 显式报告
  `Unsupported`；不能把它当作完整 `ApplicationEffectExecutor`。其余 effect owner 和全局提交通知尚未生产装配，
 所以通用 typed application patch 尚未发布 effect notification。legacy `IVerge` 类型没有 `hotkeys` 字段，
 不能通过该兼容 patch 修改快捷键。收敛条件：迁入其余 effect adapter，并让 typed application commit
 通过共同的 post-commit notification 发布 effect inputs 后，
  将 hotkey-only adapter 替换为完整 executor。
- 受影响入口：桌面启动和主 UI 的快捷键保存使用该 actor；legacy UI 页面和窗口入口未改，也没有新增快捷键卡片；
  agent 未增加快捷键工具。Android/iOS 不装配 hotkey executor。
- 验证：`cargo fmt --manifest-path backend/Cargo.toml --package chimera` 与
  `cargo check --manifest-path backend/Cargo.toml -p chimera --target-dir .tmp/hotkey-clippy-target`
  通过；`cargo clippy --manifest-path backend/Cargo.toml -p chimera --all-targets --all-features
  --target-dir .tmp/hotkey-clippy-target` 通过（存在既有 warning）。本轮没有新增或运行功能测试，未启动桌面
  应用，也未实测 OS 重试、注册冲突、退出释放或跨平台构建。快捷键设置 mutation 返回第一次 effect 结果；
  自动重试后的最终状态目前没有 IPC/UI 状态订阅入口。
- 下一步：将 retry 后的 effect 状态接到可观察状态入口；完成完整 executor 与所有 app config commit 通知，
  再覆盖主 UI、legacy 兼容入口和真实桌面运行链路。

## DIFF-037：快捷键 typed patch 提交后 reconcile

- 基线：只读 `ref/` commit `ed1931f66e08d2d233a9463c73b2376fd09a0a62`，工作树 clean；本轮未更新
  `ref/`。
- ref 到 Chimera 映射：

  | ref | Chimera | 本次动作 |
  | --- | --- | --- |
  | `client/mod.rs::patch_app_config` 的提交前快捷键校验 | `client/application.rs::ChimeraClient::patch_app_config` | typed patch 含 `hotkeys` 时先验证，再提交 |
  | `client/application_workflow/workflow.rs::{notify_committed,notify_requested}` 与 `effects::ports::CommitNotifications` | `client/hotkey/mod.rs::{set_hotkeys,reconcile_hotkeys}` 和 `effects::actor::EffectsClient::reconcile_hotkeys` | 当前只通知 Hotkeys owner；通用 notification 接线仍待迁移 |

- 类别：临时迁移 / typed application adapter。保存快捷键后统一读取当前 typed application 与 Clash config，
  再通过既有 effects actor reconcile。若读取 Clash config 在配置已提交后失败，改为返回
  `hotkey_config_read_failed` degradation；不会再把已提交的配置包装成普通失败。启动 reconcile 和
  `set_hotkeys` 继续使用同一个方法。
- 保留差异：`IVerge` 不含 `hotkeys`，所以 legacy `patch_verge` 不能修改快捷键，也不需要伪造快捷键通知。
  `patch_app_config` 当前标记为 `allow(dead_code)`，本轮为该 typed 入口补齐校验和 reconcile；主设置页当前仍由
  `set_hotkeys` 直接写 typed config。共享 `CommitNotifications` 尚未由所有提交路径调用；其余 effect owner 也未
  生产装配，故 DIFF-036 所述 hotkey-only executor 仍不是完整 `ApplicationEffectExecutor`。
- 影响入口：桌面启动及主设置页快捷键保存；legacy UI/`IVerge` 字段未变，agent 仍无快捷键工具。配置 schema
  与持久化格式未变。
- 验证：`cargo fmt --manifest-path backend/Cargo.toml --package chimera`、
  `cargo check --manifest-path backend/Cargo.toml -p chimera --target-dir .tmp/hotkey-clippy-target`、
  `cargo clippy --manifest-path backend/Cargo.toml -p chimera --all-targets --all-features --target-dir
  .tmp/hotkey-clippy-target` 通过；编译和 Clippy 保留仓库现有 warning。未新增或运行测试，未启动桌面应用，
  未实测 OS 注册与重试，也未验证 Android/iOS 构建。
- 状态：部分迁移。后续迁入 ref `ApplicationEffectExecutor` 及对应 owner adapters，并将 typed commits 接入统一
  post-commit notification，再为 retry 后状态提供 UI/IPC 订阅。

## DIFF-038：修正 Meta/CMD 快捷键提交前校验

- 基线：只读 `ref/` commit `ed1931f66e08d2d233a9463c73b2376fd09a0a62`，工作树 clean；未更新 `ref/`。
- ref 到 Chimera 的映射：`ref/backend/tauri/src/client/hotkey/ports.rs::SUPER_KEYS` 与
  `has_super_key` 对应 `backend/tauri/src/client/hotkey/ports.rs` 同名定义；前端
  `ref/frontend/nyanpasu/src/utils/parse-hotkey.ts::parseHotkey` 对应
  `frontend/chimera/src/utils/parse-hotkey.ts::parseHotkey`，录入 `Meta` 时生成 `CMD`。
- 类别：缺陷修正。ref 的修饰键检查名单遗漏 `cmd`，但其使用的快捷键解析器接受 `CMD`。因此按住
  Meta/Cmd 录入组合键后，平台解析成功仍会在配置提交前被 `MissingSuperKey` 拒绝。最小复现：在设置页录入
  `Meta+K`；前端序列化为 `CMD+K`，锁定的 `global-hotkey 0.8.0` 能解析该 accelerator，而本地名单在修正前
  不包含 `cmd`，导致校验失败，配置不保存、OS 不注册。将 `cmd` 加入既有别名名单后，提交前形状校验识别
  该组合键；平台 parser 与调用路径保持不变。
- 受影响入口：主 UI 快捷键设置的 Meta/Cmd 组合键保存、启动读取后的注册；legacy UI 和 agent 没有快捷键设置
  入口，仍复用同一 typed application 配置；配置 schema、Windows/macOS/Linux 桌面注册方式及移动端分支不变。
- 验证：核对 `backend/Cargo.lock` 锁定的 `tauri-plugin-global-shortcut 2.3.2`、`global-hotkey 0.8.0` 解析器
  源码，确认 `CMD` 映射为平台 super modifier；`cargo check --locked --manifest-path backend/Cargo.toml -p chimera
  --target-dir .tmp/hotkey-clippy-target`、`cargo fmt --manifest-path backend/Cargo.toml --package chimera -- --check`、
  根 `pnpm typecheck`、相关文件 `prettier --check`、`git diff --check` 及设置独立 target 的
  `pnpm lint:clippy` 通过（有仓库既有 warning）。Clippy 首次使用默认 `backend/target` 时遇到 Windows
  `Access is denied`，指定 `.tmp/hotkey-clippy-target` 后重跑通过。未新增或运行测试，未启动桌面应用，也未实测系统级注册。
- 例外复查条件：这是对 `ref` 同一遗漏的最小修复，不回传修改 `ref/`；当基线 `has_super_key` 或按键录入表示
  更新时重新核对 alias 清单。快捷键功能整体仍部分迁移，retry 后状态订阅、完整 effects executor 与真实桌面
  注册验证见 DIFF-036/037 后续项。

## DIFF-039：修正加号快捷键的录入与平台解析

- 基线：只读 `ref/` commit `ed1931f66e08d2d233a9463c73b2376fd09a0a62`，工作树 clean；未更新 `ref/`。
- ref 到 Chimera 的映射：`ref/frontend/nyanpasu/src/pages/(main)/main/settings/nyanpasu/_modules/hotket-manager.tsx`
  的 `handleKeyDown` / `saveHotkeys` 和 `ref/frontend/nyanpasu/src/utils/parse-hotkey.ts::parseHotkey` 对应
  Chimera 设置页同名逻辑及 `frontend/chimera/src/utils/parse-hotkey.ts::parseHotkey`；
  `ref/backend/tauri/src/client/hotkey/adapters.rs::PlatformAcceleratorValidator::canonical` 对应 Chimera 同名
  validator。ref UI 把字面加号保存为 `PLUS` 以避开 `+` 分隔符，但锁定的 `global-hotkey 0.8.0` 主键 parser
  不接受 `PLUS`；最小复现为录入 `Shift` 加主键盘 `+`，保存为 `SHIFT+PLUS` 后平台校验报无效快捷键。
- 类别：缺陷修正。Chimera 在录入边界用 `KeyboardEvent.code` 将小键盘加号记录为 parser 支持的 `NUMPADADD`；
  保留上游的主键盘 `PLUS` 配置格式，并在平台 adapter 中映射到 `EQUAL`（组合中保留 Shift），让解析器返回
  可注册的 canonical accelerator。既有 `PLUS` 配置无需迁移，配置 schema 和 UI 分隔符格式不变。
- 影响入口：主设置页录入主键盘/小键盘加号并保存；启动恢复会通过同一 canonical parser 注册。legacy UI 与 agent
  没有快捷键配置入口；动作仍通过既有共享应用 API 执行。
- 验证：`cargo check --locked --manifest-path backend/Cargo.toml -p chimera --target-dir
  .tmp/hotkey-clippy-target`、Rust 格式检查、根 `pnpm typecheck`、相关文件 Prettier 检查、设置独立 target 的
  `pnpm lint:clippy` 及 `git diff --check` 通过（Clippy 有仓库既有 warning）。未新增或运行测试，未运行桌面应用，
  也未验证具体键盘布局上的操作系统快捷键回调。
- 例外复查条件：这是对 ref 与其锁定 parser 间已复现不兼容的最小修正，不修改 `ref/`；当升级快捷键 parser，
  或调整 UI accelerator 分隔符/加号编码时，重新评估 `PLUS` 映射及键盘布局覆盖。

## DIFF-040：按当前 ref 补齐快捷键回归覆盖

- 基线：只读 `ref/` commit `cc21cbd31dc16c3e3b76c27867dd22aeae3b9cf1`，`main` 工作树 clean；本轮未更新
  `ref/`。DIFF-034 至 DIFF-039 是基于 `ed1931f…` 的历史记录；本条开始按当前 commit 核对。
- ref 到 Chimera 映射：`ref/backend/tauri/src/client/hotkey/tests.rs` 的 parse/diff/actor 契约对应新增的
  `backend/tauri/src/client/hotkey/tests.rs`；`ref/frontend/nyanpasu/src/utils/parse-hotkey.ts::parseHotkey`
  对应新增的 `frontend/chimera/src/utils/parse-hotkey.test.ts`，由根脚本 `pnpm test:hotkeys` 执行。
  生产绑定仍从前端 HotkeyManager 经 typed application IPC 写入，由 `HotkeyClient` 和共享 effects actor
  校验、持久化及 reconcile；配置 wire format 未变。
- 本轮覆盖：历史 action/accelerator 格式、解析拒绝条件、`CMD` 和 `PLUS` 兼容、平台 canonical 重复绑定、
  diff 语义、所有解绑先于注册、无效新绑定不拆除当前有效快捷键、部分失败后只重试缺失绑定、过期 revision
  不触碰注册器、退出释放后拒绝迟到 reconcile，以及 Pressed action 进入 sink。
- 保留差异与上游缺陷复现：当前 ref 的 `TauriShortcutRegistrar` 把整个快捷键插件调用包在
  `MainThreadExecutor::run` 中。仓库锁定的 `tauri-plugin-global-shortcut 2.3.2` 在 `src/lib.rs` 的
  `run_main_thread!` 宏又调用 `AppHandle::run_on_main_thread` 并同步 `recv`。若快捷键调用已经处于主线程任务中，
  插件会排入另一个主线程任务并等待；事件循环正被外层同步等待占住，内层任务无法执行。最小复现是从
  `run_on_main_thread` 回调内调用 `global_shortcut().on_shortcut(...)` / `unregister(...)`。因此 Chimera 保留
  注册 actor 的串行所有权，并让插件自身从 actor 的阻塞 worker 完成主线程 handoff，不照搬这一嵌套等待。
  当插件改为可在主线程内联执行、改成异步接口，或 ref 修复嵌套 handoff 后重新评估。
- 影响入口：桌面主设置页的全局快捷键录入、保存与 actor 注册规则；legacy UI 既有快捷操作及 agent 入口未改。
  测试使用 fake registrar/sink，不注册系统快捷键、不修改主机状态。
- 验证：Rust `client::hotkey::tests` 12 项、`pnpm test:hotkeys` 2 项；实际命令与最终结果见本次交付。
  未启动桌面应用，未实测 macOS 全局注册、键盘布局回调或权限条件。
- 状态：快捷键 parser/actor 具备可执行回归测试；整体仍是部分迁移。完整 effects executor、所有 typed
  application commit 通知、retry 后状态订阅及真实桌面注册验证仍待后续切片。

## DIFF-041：应用 effects 完整分发器

- 基线：只读 `ref/` commit `cc21cbd31dc16c3e3b76c27867dd22aeae3b9cf1`，工作树
  clean；本轮未更新或修改 `ref/`。
- ref 到 Chimera 映射：`ref/backend/tauri/src/client/effects/executor.rs::ApplicationEffectExecutor::{apply,apply_hotkeys,apply_tray,system_proxy_desires}`、
  `ref/backend/tauri/src/client/ui_effects/{ports.rs,adapters.rs}` 对应
  `backend/tauri/src/client/effects/{executor.rs,adapters.rs}`；上游
  `SystemProxyClient::reconcile` 对应本地
  `core::sysopt::Sysopt::reconcile_system_proxy` 临时兼容入口。生产 composition
  在 `setup.rs` 注入 `ApplicationEffectExecutor` 与 Tauri adapter。
- 本轮实现：按 plan 顺序分发 Locale、Logger、AutoLaunch、SystemProxy、ProxyGuard、
  Hotkeys、Widget、Tray；逐 effect 返回健康、可重试降级或 Unsupported 状态。自启动和
  系统代理同步 OS 调用进入 blocking pool；托盘工作排入 Tauri 主线程；关闭时恢复
  sysopt 保存的代理并释放快捷键。未改变配置 wire format。
- 保留差异与边界：系统代理仍由现有全局 `Sysopt` 持有，代理 guard 使用兼容循环而非 ref
  actor 的取消/定时状态；PAC 启用会报告 `pac_proxy_unsupported`，不会假报应用成功；本地
  没有 widget runtime，禁用状态为空操作，启用状态报告 `widget_runtime_unavailable`。
  托盘仍从 Chimera 的全局配置读取视图，没有迁入 ref 的 `TrayView` 缓存所有权。
- 提交通知边界：完整 dispatcher 已注入生产 `ApplicationEffectsPort`，但所有 typed
  application/clash/profile/runtime commit 通知尚未接通。当前普通设置变更继续走已有
  legacy side effects；快捷键路径仍只向共享 actor 提交 Hotkeys effect。因此本条完成的是
  executor 分发实现，不代表所有 effect 已从所有 UI/agent 入口切到该路径。
- 影响入口：对现有 legacy UI 与主 UI 操作未做交互或持久化格式改动；主界面快捷键仍通过
  共享 actor。非快捷键 effect 的统一提交入口、agent 变更通知和重试状态订阅仍待后续接线。
- 测试契约：`docs/testing/contracts/application-effects-executor.md`。fake adapters 不改宿主
  设置；测试覆盖全量 effect 顺序、未解析 mixed port、PAC 下关闭代理及 widget 未实现状态。
  CI Ubuntu job 执行 `client::effects::` 测试筛选和既有快捷键测试。
- 收敛条件：迁入 ref 对应的 system-proxy actor 与 UI effect owners、实现所需 PAC/widget
  runtime 后，将 legacy side effects 和 Sysopt adapter 收敛到单一 owner；再接通各 typed
  commit notification，并为主 UI、legacy UI 和 agent 入口补真实桌面验证。

## DIFF-042：按 domain 发布 post-commit effects 通知

- 基线：只读 `ref/` commit `cc21cbd31dc16c3e3b76c27867dd22aeae3b9cf1`，工作树
  clean；本轮未更新或修改 `ref/`。
- ref 到 Chimera 映射：`ref/backend/tauri/src/client/effects/ports.rs::CommitNotifications`
  的 `application_committed`、`clash_committed`、`profiles_committed`、`runtime_bound` 和
  `publish_full`，以及 `effects/actor.rs::Slice` 的单 domain merge，对应
  `backend/tauri/src/client/effects/{ports.rs,actor.rs}`。Profile TCC 提交点对应
  `application_workflow/{workflow.rs,tcc.rs}`；Chimera 的注入经过
  `state/mutation.rs::MutationCoordinator` 和 `client/mod.rs::try_new_with_args`。
- 本轮实现：通知接口现在按 application、Clash、Profiles 和已绑定 runtime ports 分片；effects actor
  将收到的 slice 合并进各自最新输入后再生成 diff。只有 source 决策为 `Committed` 才触发 TCC 的
  Profiles 通知。ProfilesClient 的序列化 source mutation 复用该 TCC owner，避免在 facade 收到
  `CommitReport` 时再次通知。主/legacy UI 的 Profiles 状态事件继续按原路径发送，新增通知只负责 shared
  effects actor 的托盘刷新。
- 保留差异：本轮只生产接入 Profiles slice。Application 和 Clash 仍由原有
  `run_verge_patch_side_effects` / `run_clash_patch_side_effects` 持有；runtime ports 通知也未接入，避免在
  其旧 sysopt/lifecycle owner 仍执行时重复应用系统代理、guard 或托盘副作用。因此本条是部分迁移，不代表
  所有 effect 已经改由 executor 执行。`reconcile(inputs)` 保留完整输入 slice，供当前兼容调用方使用。
- 影响入口：经 typed Profile TCC 完成的 add/import/delete/reorder/refresh/metadata/activate/transforms/file
  source commits 会请求一次 shared tray partial refresh；前端 Profile refresh 仍覆盖主 UI 与 legacy UI。
  agent 若经由同一 `ChimeraClient` Profile API 会复用该通知；没有增加 agent 专属业务实现。未改变 Profiles
  持久化格式或 runtime commit/rollback 语义。
- 测试契约：扩展 `docs/testing/contracts/application-effects-executor.md`。actor 单测验证 domain slice
  不覆盖 sibling state，Profile commit 只产生 Tray Part plan；TCC 单测验证 committed Profile source
  mutation 发出一次通知。fake effect port 不触碰 Tauri、系统代理或主机设置。
- 验证：`cargo fmt --manifest-path backend/Cargo.toml --package chimera -- --check`、
  `git diff --check`；`cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib
  client::effects:: -- --test-threads=1`（40 passed）；
  `cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib
  client::application_workflow::tcc::tests -- --test-threads=1`（11 passed）；
  `pnpm lint:clippy` 通过（仓库既有 warning）。Ubuntu CI 的 effects/hotkey 步骤新增执行 TCC 测试筛选。
- 验证环境差异：Cargo 首次因 `backend/chimera-runtime/chimera_service/Cargo.toml` 中重复的
  `[dev-dependencies]` table 无法解析 workspace。为运行 Rust 检查，保留了删除重复、内容相同的
  `tempfile = "3"` table；该目录是 submodule，变更只删除冗余声明，没有改变依赖集合。
- 后续：已运行 TCC committed-notification 测试和 `client::effects::` CI filter；将 application/clash
  producers 接到 slice 前，先迁移或移除对应的 legacy direct side effects；之后再接 runtime-bound/full
  lifecycle producer 和可观察 effect status。

## DIFF-043：共享依赖升级与 lockfile 对齐

- ref 基线：实际只读 checkout 为 `cc21cbd31dc16c3e3b76c27867dd22aeae3b9cf1`，分支 `main`，工作树 clean；本轮没有 fetch 或修改 `ref/`。本文件前文仍记有 `5331747c06a5f42eeabb3e225a1e77a83f480549`，因此本次明确按实际 checkout 的 `cc21...` 比较，不声称它是最新远程 HEAD。主仓库已快进同步到 `origin/master` 的 `d7f865ee`。
- ref 路径和符号：`package.json`、`frontend/nyanpasu/package.json`、`frontend/{interface,ui,utils}/package.json`、`backend/Cargo.toml`、`backend/tauri/Cargo.toml`、`backend/nyanpasu-config/Cargo.toml`、`backend/nyanpasu-runtime/crates/{nyanpasu-utils,clash-api}/Cargo.toml`。
- Chimera 路径和符号：`package.json`、`frontend/chimera/package.json`、`frontend/{interface,ui,utils}/package.json`、`tauri-e2e/package.json`、`backend/Cargo.toml`、`backend/tauri/Cargo.toml`、`backend/chimera-utils/Cargo.toml`、`backend/chimera-clash-api/Cargo.toml`、`backend/chimera-config/Cargo.toml`、`pnpm-workspace.yaml`、`pnpm-lock.yaml`、`backend/Cargo.lock`。
- 类别：共享依赖对齐；E2E 兼容适配。
- 更新：共同前端依赖和 package manager pin 对齐到 ref（pnpm 12.8.1），并重生成锁文件；Rust 更新包括 base64 0.22→0.23、Oxc 0.151→0.152、csscolorparser 0.8→0.9、serde_with 改为 ref 的 `3.20.0` 兼容范围、sha2 0.11→0.10、windows-core 0.100→0.62。按 ref 的分层方式，`backend/Cargo.toml` 的应用级 reqwest 使用 0.12，`chimera-utils` 和 `chimera-clash-api` 则各自使用 reqwest 0.13。Cargo.lock 的 wasm-bindgen/js-sys 七件套现在与 ref 一致。对 666 个双方均只有单一解析版本的同名 crate 做比较后，没有发现本地版本低于 ref。
- peer 收敛：`.prettierrc.cjs` 没有启用 `@prettier/plugin-oxc`，所以移除未使用的根依赖。`@wdio/tauri-service@1.4.0` 固定依赖 `@wdio/globals@9.29.1`，其 `expect-webdriverio` peer 与当前 WDIO 9.31 / expect-webdriverio 6 不兼容；在 `pnpm-workspace.yaml` 以窄范围 override 将该版本映射到 9.31.3。ref 没有 E2E harness，因此此项是 Chimera E2E 扩展的临时兼容适配；待 tauri-service 更新其固定 WDIO 依赖后移除。
- 保留差异：锁文件中 `float-cmp` 为本地 0.10.0、ref 根锁为 0.9.0；0.10.0 来自运行时子工作区的 `value-trait`，与 `ref/backend/nyanpasu-runtime/Cargo.lock` 一致，ref 根锁中的 0.9.0 则来自另一条依赖路径。deep-link 包仍是本地官方插件 2.4.9 与 ref 仓库内插件 0.1.2 的差异；ref 清单本身标注该自定义插件应迁移到官方插件，因此保留 Chimera 已采用的官方实现，待 ref 完成迁移后再收敛版本和 API 差异。E2E peer override 仍按上文条件移除。
- 影响入口：主界面、legacy UI 共用同一组更新后的 frontend packages；E2E 继续使用现有 harness 和 `@wdio/tauri-service`。Rust 生产源代码和持久化合同没有改动。桌面 TUN 没有在本轮重启或改写。
- 验证：独立依赖 worktree 中 `pnpm install --frozen-lockfile --ignore-scripts`、`pnpm peers check`（无 peer 问题）、`pnpm --filter chimera-ui build`、`pnpm typecheck`、`pnpm --filter @chimera/tauri-e2e test:unit`（17 passed）；Cargo `metadata --locked`、`cargo check --locked --manifest-path backend/Cargo.toml -p chimera` 通过；`cargo test --locked --manifest-path backend/Cargo.toml -p chimera-utils --features process,reqwest -- --test-threads=1` 全部通过（66 passed），`cargo test --locked --manifest-path backend/Cargo.toml -p chimera-config -- --test-threads=1`（139 passed）通过。`cargo tree` 确认应用解析 reqwest 0.12.28、Clash API/utilities 解析 0.13.5。临时 worktree 缺少被忽略的 sidecar/resources 文件，因此 Tauri check 只读链接了当前工作树中的资源目录，没有改写资源。最初仅启用 `reqwest` 的测试调用漏掉 `process` feature，3 个 process-helper smoke tests 因 helper 不存在失败；带上 `process,reqwest` 后按完整 feature set 重跑通过。`git diff --check` 和 pnpm frozen lockfile check 通过。
- 未完成的验证：本机只安装了 `aarch64-apple-darwin` Rust target；windows-core 0.62 已由 Cargo 元数据解析，并确认其 `Error::from_thread` API 存在，但 Windows 目标编译仍需 CI 验证。没有运行 macOS 桌面 bundle、Tauri 桌面 E2E、Linux 构建或 TUN 实机验证。临时 worktree 的 `pnpm prepare:check` 曾因该 worktree 未初始化 `backend/chimera-runtime` 子模块而失败；本轮没有重跑这个会检查/更新本地 sidecar 资源的脚本。
- 收敛状态：所检查的共享依赖版本差异已按 ref 约束收敛；剩余差异均已说明来源或上游 TODO。依赖更新本身完成，Windows 和桌面运行验证仍待相应平台环境覆盖。

## DIFF-044：Connections 页面搜索状态与最新 ref 同步

- 基线：按用户要求更新本地只读 `ref/`。更新前 `cc21cbd31dc16c3e3b76c27867dd22aeae3b9cf1`，更新后
  `def0dbf2716ff972255be47924401b60db7d482c`（`main`，与 `origin/main` 一致）；参考工作树干净。此次 fetch
  更新了主仓库远程引用；上游 runtime 子模块已在该 ref 提交记录的位置，未修改其内容。
- ref 到 Chimera 映射：`ref/frontend/nyanpasu/src/pages/(main)/main/_modules/use-search-term.ts::useSearchTerm`
  对应 `frontend/chimera/src/pages/(main)/main/_modules/use-search-term.ts::useSearchTerm`；ref
  connections `index.tsx` 的 URL 查询参数 `q` 对应 Chimera 同名页面和
  `frontend/chimera/src/pages/(main)/main/connections/route.tsx::validateSearch`。
- 本轮实现：输入停止 300ms 后将搜索词写入当前 route entry 的 `q`，使用 replace 保持输入不制造历史记录；
  返回到页面条目时恢复该搜索词。表格过滤使用 deferred value，并延后挂载重表格；代理侧栏更新筛选时保留现有
  route search 字段，避免清掉搜索词。搜索页离场后不再写入 route 状态。
- 保留差异及状态：**部分迁移**。当前 Chimera 仍是只有 active scope 的旧表格和独立 proxy sidebar；最新 ref
  的 `active/all/closed` viewers、traffic range/dimension filters、分页关闭连接、filter-chip overflow、跨页
  focus navigation 都未迁入。尤其 `all/closed` 和 traffic filters 依赖 ref 的
  `nyanpasu-traffic` 持久化 store、traffic actor、RPC/IPC bindings 和 live-connection frame feed；Chimera
  尚无对应业务服务/查询 API。本轮没有用前端缓存伪装历史数据，也没有改变 legacy UI。
- 影响入口：仅主界面 connections route/search 和 proxy sidebar；legacy `/connections` 与 agent 入口未改，仍通过
  现有共享 `useClashConnections` 数据入口工作。
- 验证：`pnpm typecheck`、`pnpm --filter=chimera-ui build`、Prettier 和 `git diff --check` 通过。未运行真实桌面
  E2E；当前 Chimera 前端没有 connections 浏览器测试 suite，构建和类型检查不证明 route 返回/重载行为。
- 收敛条件：先迁移 ref `backend/nyanpasu-traffic` 的领域模型、持久化、traffic actor 与连接帧装配，映射到
  Chimera `ChimeraClient`/Tauri IPC 并用现有 Specta generator 生成 binding；之后迁入 ref connections route 的
  active/all/closed viewer、filters、toolbar/status tabs 和跨页焦点，并分别验证主 UI、受影响的 legacy 共享入口及
  agent 对同一 API 的兼容性。完成这些步骤前不得称 Connections 页面已与最新 ref 完成对齐。

## DIFF-045：等待新进程完成 exec 后再记录身份

- 基线：只读本地 `ref/` commit `0c7b15b3c61db390db21cffb63d29805abd380a1`，工作树 clean；本轮未更新或修改 `ref/`。
- ref 到 Chimera 映射：`ref/backend/nyanpasu-runtime/crates/nyanpasu-utils/src/process/pid_file.rs::PidFileGuard::write` 和
  `wait_for_process_identity` 对应 `backend/chimera-utils/src/process/pid_file.rs` 同名逻辑。
- 类别：ref 缺陷的最小修正。ref 与 Chimera 原逻辑都会在第一次读到进程身份时立即返回；Linux 在带有 `pre_exec`
  hook 的 fork/exec 启动窗口中，`/proc/<pid>/exe` 短暂指向父测试进程。CI 的
  `different_epoch_pid_files_do_not_kill_overlapping_process` 已复现该竞态：刚启动的
  `chimera-test-child` 被识别为 `process_pid_file` 并拒绝写身份记录。Chimera 现在在有限等待期间继续观察，直到
  executable 与启动命令一致；持续不匹配仍 fail closed。
- 其他 CI 修正：`core_spec_preserves_each_supported_brand_and_executable_mapping` 将平台可执行后缀纳入断言，并在
  Windows 上为 Alpha 测试夹具提供 `.exe` 文件名；
  Specta 绑定新鲜度检查在比较前统一 CRLF/LF，保留内容级差异检查。
- 影响入口：进程监督写入 epoch pid 记录及 Windows/macOS 二进制路径映射测试；不改变 pid 记录格式、终止权限或
  产品 IPC。没有 legacy UI 或 agent 独立路径。
- 验证：epoch pid 集成测试 12/12、启动身份等待竞态回归测试 1/1、核心二进制映射测试 1/1、Specta 绑定新鲜度测试
  1/1 通过；绑定生成器更新测试运行后没有产生文件差异。`pnpm run-p lint:clippy lint:rustfmt` 与
  `git diff --check` 通过。全量 `pnpm test:backend` 在本机 macOS 为 551 passed、1 ignored、1 failed；唯一失败是
  不相关的 `external_watcher_smoke_mirror_real_file_event` 超时，该用例单独重跑 1/1 通过。远程本次失败项只在
  Linux 进程启动竞态和 Windows 两个跨平台断言；macOS CI 与 Tauri E2E 均通过。
- 重新评估：当 ref 的进程身份采集等待逻辑或 ProcessKit 的 spawn/exec 行为更新时，重跑身份竞态测试，并评估移除
  本地等待差异；平台后缀断言和绑定换行规范化保留为跨平台测试要求。

## DIFF-046：在配置提交时锚定远程 profile 首次刷新时间

- 基线：只读本地 `ref/` commit `0c7b15b3c61db390db21cffb63d29805abd380a1`，工作树 clean；本轮未更新或修改 `ref/`。
- ref 到 Chimera 映射：ref `backend/tauri/src/state/profiles/jobs.rs::ProfileJobs::reconcile` 注册持久化 interval job；Chimera
  `backend/tauri/src/state/profiles/scheduler.rs::RemoteUpdateScheduler::reconcile` 维护每 profile 的 Tokio timer。
  两者都从 committed profile 的 `update_interval_minutes` 派生后台刷新，但调度器实现不同。
- 问题与修正：Linux CI 暴露了导入 profile 后立即推进 Tokio 虚拟时间的竞态：timer task 可能在时间推进后才首次被轮询，
  从而把完整 interval 错误地从推进后的时刻开始计算。现在在 reconcile 同步计算首次刷新 deadline，再由 task 等待该 deadline；
  计时从 profile 提交/调度时刻开始，不依赖 executor 何时首次运行 task。后续周期仍按原 interval 循环。
- 影响入口：通过同一个 `ProfilesClient` 的主界面、legacy UI 与 agent profile 导入/更新入口；未改变持久化格式或 IPC。
- 验证：远程失败回归用例 `client::profiles::tests::import_suggestion_is_committed_and_reschedules_the_timer` 本机通过（1/1）；
  `cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib client::profiles::tests -- --test-threads=1`
  通过（66 passed）。Cargo fmt、Prettier 和 `git diff --check` 通过。推送后的 CI 将重新验证 Linux/Windows 后端测试矩阵；
  Tauri E2E 不受此改动影响。
- 收敛条件：当 Chimera 迁移到与 ref 相同的持久化 job scheduler 时，重看首次 tick 的注册时刻与 catch-up 语义；保留
  “deadline 不依赖 task 首次 poll 时刻”的确定性要求。

## DIFF-048：允许 WebView 同源与 data URL 请求

- ref 基线：PR #478 rebase 到 master 后使用 gitlink 固定的 `31019da14006b384b3272c72b4088179a9e72c47`。已直接读取该 commit 的 `backend/tauri/tauri.conf.json::app.security.csp`，其 `connect-src` 包含 `'self' data:`；没有移动或更新 `ref/`。
- ref 到 Chimera 映射：`ref/backend/tauri/tauri.conf.json::app.security.csp` 对应 `backend/tauri/tauri.conf.json::app.security.csp`。只在 `connect-src` 加入 `'self' data:`，其它 CSP 来源不变。
- 类别与风险：共享 WebView 安全策略对齐，L2；smoke E2E 是 Chimera 扩展。
- 本轮实现：同一 Tauri CSP 覆盖 legacy 与 main WebView。Smoke 从两个窗口分别请求当前文档同源 URL，并以 `no-cors` 请求内存中的 `data:` URL；同源响应必须成功，`data:` 响应必须为 opaque 且状态 0。缺少来源时请求被 CSP 阻止，断言会失败。
- 影响入口：Tauri 共用 CSP 覆盖 legacy 和 main 两个窗口；Agent 直接使用共享 application API，不经 WebView 请求，因此不受该策略影响。没有改动 UI、IPC、配置持久化、运行时核心或 Service，也不验证公网代理。
- 测试契约：`tauri-e2e/specs/smoke.e2e.ts` 的 `allows same-origin and data URL fetches in both desktop windows`，位于现有 `smoke` 基础套件并随 PR 的 `hermetic` suite 执行。每个窗口先确认目标路由/窗口，再进行有界 fetch；检查真实 WebView 观察到的响应，不写入页面或应用状态。无外网和用户数据。
- 验证：已完成 E2E harness typecheck、harness unit tests 25/25、Oxlint、Prettier、Tauri JSON/CSP 解析及 `git diff --check`。本机 macOS 桌面 E2E 未运行；本地完整打包需要较大 Cargo target 且磁盘空间不足。合入 master 基线后，Windows Desktop `hermetic` 会执行真实 legacy/main WebView 用例；其结果仍待新 SHA 验证。
- 保留差异：legacy UI 保持现有窗口和用户流程；`data:` 响应是 opaque，测试不读取正文。没有其它收敛工作。
- 回滚：还原 `connect-src` 新增的 `'self' data:` 和本 smoke 用例即可；不迁移用户数据或更改宿主网络设置。

## DIFF-049：滚动 Chimera Client Alpha 资产失败后刷新

- ref 基线：本地 `ref/` commit `813673a8e84b51265467f02277fc7e739837f61e`，与远端 `main` 一致，工作树干净。该提交相对原分析基线 `67999753918c7b9879ddd2bacdef74246bfef1dd` 未改变本条映射的 `scripts/src/prepare` 文件。
- ref 到 Chimera 映射：ref `scripts/src/prepare/types.ts::BinInfo.refresh`、
  `scripts/src/prepare/binaries.ts::getClashMetaAlphaInfo` / `getMeowAlphaInfo` 和
  `scripts/src/prepare/resources.ts::resolveSidecar`；Chimera 对应新增的
  `scripts/src/prepare/{types,binaries,resources}.ts` 中 `BinInfo.refresh`、
  `getChimeraClientAlphaInfo` 与 `resolveSidecar`。
- 实际入口：`package.json::prepare:check` 调用 `scripts/check.ts`；Alpha task
  仍通过 `materializeSidecar` 和 `verifyAndInstallSidecar` 安装。当前 Alpha
  `BinInfo` 没有提供 `checksumURL`，因此这条路径不做 SHA-256 比较；它继续使用
  现有的文件暂存、可执行权限设置、原子替换与版本戳更新。
  为保持 Chimera 当前 Deno 准备器与安装边界，本批把 ref 的刷新重试抽到对齐
  目录；实际暂存、可选校验和原子替换仍由 `scripts/check.ts` 承担。未修改没有
  被 `prepare:check` 调用的旧 `scripts/utils/resource.ts`。
- 问题与修正：Chimera Client Alpha 使用滚动 `latest` release；macOS 资产文件名
  带稳定版本号。当 manifest 更新落后于 release 且旧资产已不可用时，首次下载
  失败会触发 `resolveChimeraClientAlpha`（与 manifest 生成共用同一 resolver）
  重新解析版本及架构映射，再按 ref 行为重试一次。刷新元数据失败时保留首次
  下载错误；重试安装失败则报告重试错误，外层原有 task retry 不变。当前 release
  资产列表确有版本化 macOS tarball；证据：[latest release assets](https://github.com/MFSGA/Chimera_Client/releases/expanded_assets/latest)。
- 保留差异与边界：只给 Chimera Client Alpha 增加刷新，未支持的 Meow 不迁入；
  Clash-rs Alpha 使用无版本号文件名，因此无需此映射。与 ref 相同，只有首次
  materialization 失败才刷新；命中版本戳有效的缓存时仍遵从现有缓存语义，直到
  manifest 更新使版本戳失效。
- 影响入口：打包/开发前的共享 sidecar 准备；不改变 main UI、legacy UI、Agent
  的 API，配置持久化、Core/Service 协议和宿主网络均不受影响。下载及安装仍复用
  现有临时目录、版本戳和原子替换逻辑；Alpha 资产当前没有 SHA-256 校验。
- 验证：`pnpm test:manifest` 为 5 passed，其中新增测试模拟旧滚动资产失败，
  验证同一调用链刷新为新 Alpha 版本并重试；`deno check --frozen --config
  scripts/deno.jsonc scripts/check.ts`、`pnpm typecheck`、`pnpm lint:oxlint`、
  Prettier 和 `git diff --check` 通过。未在本机运行 `pnpm prepare:check`，因为它会
  下载并替换工作区 sidecar；本批完整准备/打包路径由变更后的远程 CI 执行。没有
  本机 macOS Tauri 桌面 E2E 或真实 Alpha 核心运行验证。
- 收敛条件：本地准备器整体迁移到 ref 的 `scripts/src/prepare/check.ts` 后，移除
  `scripts/check.ts` 的委托桥接；若要在 manifest 日更窗口内自动淘汰已缓存的
  旧 Alpha，还需单独评估每次准备时在线解析版本的成本和离线回退策略。

## DIFF-051：修正 epoch PID 文件检查与读取之间的删除竞态

- 问题与证据：PR #479 的 macOS CI job `115762372590` 在
  `process_pid_file::residual_process_is_killed_before_spawn` 第二次 `spawn().unwrap()` 处报告 OS
  `NotFound`，日志没有指出产生错误的具体系统调用。`read_epoch_pid_file` 先 `symlink_metadata`、后
  `read_to_string`，而上一轮进程清理可并发移除同一记录；检查后读取前的删除竞态是与日志现象一致的候选根因。
- ref 映射与保留差异：ref `backend/nyanpasu-runtime/crates/nyanpasu-utils/src/process/pid_file.rs::read_epoch_pid_file`
  （runtime SHA `e9f44e68b0c88981160f5b986f7ffcc9fb5b20a7`）也有相同顺序。Chimera 在对应的
  `backend/chimera-utils/src/process/pid_file.rs::read_epoch_pid_file` 仅将读取阶段的 `NotFound` 处理为
  `Ok(None)`；符号链接/普通文件检查仍保留，其他 I/O 错误仍向上传播。该最小修正不改变 PID 身份校验、终止授权或记录格式，
  不复制 ref 中已观察到的竞态缺陷。CI 没有 syscall 级证据确认此次失败源自该读取窗口，因此该根因仍待新 SHA 远端复验。
- 验证：旧 PR SHA 的 macOS CI 为 11 passed、1 failed；修正后本地 macOS `process_pid_file` 集成目标
  12/12 通过，`rustfmt --check backend/chimera-utils/src/process/pid_file.rs` 通过。新 SHA 的 macOS CI 与
  Desktop E2E 尚待远端复验；新 SHA 用于验证修复效果，旧失败的具体 syscall 根因仍是与现有证据一致的候选判断。
- 关联记录：[LESSON-20261014-001](../LESSONS.md#lesson-20261014-001-epoch-pid-记录可能在检查后读取前被并发清理)。
- 回滚：还原读取错误分支即可；无用户数据迁移或宿主状态变更。

## DIFF-052：滚动 Chimera Client Alpha 发布期间有限重试

- ref 基线：本地 `ref/` commit `813673a8e84b51265467f02277fc7e739837f61e`，工作树干净；对应的
  `scripts/src/prepare/resources.ts::resolveSidecar` 在刷新失败时只尝试一次，本批保留其初次下载、刷新、再安装的调用顺序。
- Chimera 映射：`scripts/check.ts::tasks` 的 `chimera-client-alpha` task →
  `getChimeraClientAlphaInfo` → `scripts/src/prepare/resources.ts::resolveSidecar` →
  `materializeSidecar`。仅 Alpha task 提供 `SidecarRefreshRetryPolicy`；其他 sidecar 沿用原路径。
- 证据与原因：PR #479 macOS job
  [115791449079](https://github.com/MFSGA/Chimera/actions/runs/38544336079/job/115791449079)
  在资源准备阶段因已失效的 `0.26.3` Alpha 资产 URL 返回 404 而失败；task 在无等待的五次总尝试（含初次）内没有恢复机会。只读检查
  `MFSGA/Chimera_Client/.github/workflows/ci.yml` 发现 master nightly 发布会先删除滚动 `latest` release/tag，再上传
  新资产并发布替代 release；因此当前元数据或资产暂时 404 是已证实存在的发布窗口。CI 日志证明旧资产不可用，发布工作流
  证明窗口存在；日志没有 syscall 式证据证明此次 404 与窗口精确重叠，二者关系按候选根因记录。
- 修正：当 Alpha 初次下载后刷新元数据或新资产仍返回 404 时，按 1、2、4、8 秒间隔重新解析滚动 release 并尝试安装；
  每次 sidecar resolve 最多额外四轮，累计等待上限 15 秒。外层原有 task retry 仍为五次，因此持续 404 时整体准备过程最多可累计
  75 秒的新增等待。非 404 错误不进入该重试；缓存、文件校验/安装边界及其他 core 的 task retry 不变。
- 保留差异：ref 对 rolling asset 只刷新一次；此处的有限 404 重试是针对 Chimera Client `latest` 发布流程的本地恢复适配。
  如果上游发布流程改为原子替换 release/tag，或 Alpha 资产改为不可变版本 URL，应重新评估并移除此差异。
- 验证：用例模拟旧 Alpha 资产 404、latest tag 查询暂时 404、恢复后新资产再短暂 404；另有用例验证 404 重试预算耗尽时抛出
  最后一次错误，以及 503 不进入延迟重试。`pnpm test:manifest` 为 8 passed；`deno check --frozen --config
  scripts/deno.jsonc scripts/check.ts`、`pnpm lint:oxlint`、Prettier 和 `git diff --check` 通过。`pnpm prepare:check`
  在本机实际完成，旧 manifest 中的 `0.26.3` Alpha URL 刷新到 `0.27.0-alpha+sha.47d6954` 并安装成功；其他 sidecar 均命中缓存。
  全工作区 `pnpm typecheck` 未通过，因为该隔离工作树缺少构建生成的 `frontend/chimera/src/paraglide` 模块，错误与本次脚本 diff 无关。
  该 SHA 推送后仍需重新验证 PR required checks 与 Desktop E2E；尚未运行实际 macOS Tauri 打包。
- 风险与回滚：L2（sidecar 依赖解析/打包）。回滚可移除 retry policy 与 `resolveSidecar` 的可选重试参数；没有用户数据、
  宿主网络或持久配置变更。

## DIFF-053：profile 变更后使代理快照缓存失效

- 基线：本地 `ref/` commit `813673a8e84b51265467f02277fc7e739837f61e`，工作树干净；本批未修改 ref。
- 映射：ref `frontend/query/src/ipc/use-profile.ts::useProfileMutations` 的成功回调只失效 profile query；
  `frontend/query/src/provider/mutation-provider.tsx` 的 `profiles` key 列表也不包含 `getProxies`。ref 由
  `backend/tauri/src/client/clash_api.rs::NyanpasuClient::{get_proxies,refresh_proxies}` 读取 actor-owned
  `ProxiesClient` 快照，并以 API 实例身份和撤销状态维护其生命周期；同一实例的普通缓存为 3 秒，周期性强制刷新间隔为 10 秒。
  Chimera 的本地扩展
  `frontend/interface/src/provider/mutation-provider.tsx` 会在 profile mutation 后使 `CLASH_PROXIES_QUERY_KEY` 失效；
  后端则通过 `backend/tauri/src/ipc.rs::get_proxies` 读取共享的 `core/clash/proxies.rs::ProxiesGuard`。
- 问题证据：PR #479 旧 SHA `2188073f4ee0f749777e3a8c21e1c7aef54520c3` 的 Windows Desktop E2E 中，
  `rules-main-proxy-icon.e2e.ts` 52 项有 1 项失败。页面已导航到新 profile，直接 runtime 查询也看到带 SVG 的新代理组，
  但 30 秒后 UI 仍显示无图标 fallback。失败截图与完整用例见
  [Windows Desktop E2E run](https://github.com/MFSGA/Chimera/actions/runs/38544336107)。
- 原因与修正：Chimera profile mutation 会使前端代理 query 失效，但后端 `get_proxies` 在 3 秒缓存 TTL 内仍可返回旧快照；
  该 IPC 刷新路径不广播 proxies mutation，因而旧结果可能保持在 UI query 中。`ChimeraClient::refresh_profiles`
  现在先递增 `ProxiesGuard` generation、清除 checksum 并将 `updated_at` 置为过期，再发 profile mutation。缓存更新在提交前
  对 generation 做核对：如果旧快照请求跨过了 profile 失效点，它会被丢弃；`get_proxies` 会重新取得 API 并重试。
  后台刷新在解析 API 前捕获 generation；手动代理选择在 API 获取后、发出选择请求前重新校验 generation，并把同一 generation
  传给后续缓存刷新。profile 在 API 获取期间发生变化时，选择会被拒绝。
  如果 profile 在已发出的 HTTP 选择请求期间变化，请求本身仍可能作用于正在退出的 runtime（未实现 ref actor 的实例级撤销），
  但后续快照刷新发现 generation 已失效时会返回错误；IPC 不报告成功，也不会继续发出代理变更事件或连接中断处理。
  手动 profile mutation 与“无当前 profile 时自动激活”的完成路径都在发送事件前共用此函数。
- 保留差异与接口：没有改变 IPC、前端 query key 或持久配置；`ProxiesGuard` 是 Chimera 迁移期缓存桥接。ref 的代理快照由
  `ProxiesClient` actor 管理并绑定到可撤销的 API 实例；Chimera 当前仍使用全局 TTL guard，加 generation 栅栏保证失效后的
  旧请求不能复活缓存。后续迁移到 ref 的代理客户端/刷新契约时，应移除该全局 TTL 缓存并由共享 client 管理快照和通知。
  变更影响 profile 与代理刷新共享路径，属 L2。
- 验证：旧 SHA 的 Windows 用例 1/52 失败；旧代码本机 macOS 定向 E2E 1/1 通过。初始 generation 修复的 Rust 定向测试为
  3 passed。补上选择请求期间 profile 切换的错误返回后，第一次重跑因本机磁盘空间耗尽在链接前失败；清理当前工作树生成的 Cargo
  构建缓存后，以 `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --locked --manifest-path
  backend/Cargo.toml -p chimera --lib core::clash::proxies::tests:: -- --nocapture` 重跑，最终代码 4 passed、0 failed。
  `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo clippy --locked --manifest-path backend/Cargo.toml -p chimera --all-targets
  --all-features`、`cargo fmt --manifest-path backend/Cargo.toml -p chimera -- --check`、`pnpm test:manifest`（8 passed）和
  Deno check 通过；`pnpm exec prettier --check LESSONS.md docs/ref-alignment-progress.md` 与 `git diff --check` 通过。
  用最新 e2e 构建运行本机 macOS WebKit 定向用例：第一次在
  profile 创建前因主页面路由动画未稳定而失败；随后两个独立运行都通过 1/1，最后一次使用包含 generation 与选择预检的最终
  e2e 构建。重试通过不等于首次运行通过，也不能替代 Windows 回归复验。完整工作区 `pnpm typecheck` 仍因隔离工作树缺少
  生成的 Paraglide 文件而失败。新 SHA 的 Windows Desktop E2E
  及 PR 必需检查仍须远端复验；当前未声称 CI 已通过。
- 回滚：移除 `ProxiesGuard::invalidate` 和 `ChimeraClient::refresh_profiles` 包装，恢复各 profile mutation 直接发事件；
  无迁移用户数据、宿主网络或系统服务状态。

## DIFF-054：主界面流量分析与拓扑视图

- 目标与非目标：把 ref 的主界面流量总览、排名、flow 和 geography 视图接入 Chimera 主 UI，并让导航保持 URL 筛选状态；本批不迁移本地 IP 探测/外部地理定位，不改变流量采集 runtime、legacy UI 表现或 agent API。
- 参考版本：流量分析初版取自本地 `ref/` commit `fb0039b11a12c0f575fb92c1481f4ebaf66eb1d5`；本 PR 当前固定的 `ref/` 基线为 `e81eeaea46a9c229478cd29c9555b5c072f7e6cc`。检查 `fb0039b1..e81eeaea` 的 ref 变更后，确认本批映射的 topology、data-page、traffic utility 和 navigation-sidebar 文件未被后续提交触及。本功能提交不修改 `ref/`；同步 gitlink 是同 PR 的独立提交。
- ref 路径和符号：`ref/frontend/nyanpasu/src/pages/(main)/main/topology/` 的 overview、`traffic-context`、排名、flow、map、toolbar、transition、搜索及布局模块；`ref/frontend/nyanpasu/src/pages/(main)/main/_modules/{data-page.tsx,use-usage-label-of.ts}`、`ref/frontend/nyanpasu/src/utils/{traffic-usage,traffic-retention}.ts`，以及 `ref/frontend/ui/src/navigation-sidebar.tsx`。保留 ref 的路由、查询参数、页面模块和组件命名；新增 `Button.disableRipple` 以承载 ref 导航侧栏的 ripple 所有权。
- Chimera 映射：对应文件位于 `frontend/chimera/src/pages/(main)/main/topology/`、`frontend/chimera/src/pages/(main)/main/_modules/{data-page.tsx,use-usage-label-of.ts}`、`frontend/chimera/src/utils/` 和 `frontend/chimera/src/components/ui/navigation-sidebar.tsx`；数据仍通过共享 `frontend/interface/src/ipc/use-traffic.ts::{useTrafficReport,useTrafficUsage}` 读取。排名弹窗沿用 ref 的 50 条分页和关闭自动重试策略。增加主界面桌面导航、移动端 More 菜单入口；Connections 的 `onViewRuleUsage` 跳转到带 rule filter 的 topology route。路由生成产物为 `frontend/chimera/src/routeTree.gen.ts`。
- 类别及接口：L1、只读主界面能力。`useTrafficReport` 新增可选 `refetchInterval` 查询选项，默认查询行为不变；`useTrafficUsage` 的默认分页大小与 retry 策略对齐 ref（50 条、`retry: false`）。没有增加 IPC、持久化、runtime 或 agent tool 合同。legacy UI 继续使用共享 traffic hook，页面呈现未改变；agent 不受影响。
- 隐私和保留差异：地图仅使用 backend report 已提供的 region 字段和打包的 Natural Earth 数据；没有端到端 IP lookup、public geo 服务、地图瓦片或宿主网络探测。ref 的 local-IP probe、opt-in/geolocation index 和 `mock-traffic.tsx` 未迁入，避免制造未经用户授权的外网 IP 查询，并因 Chimera 没有 ref debug mock hooks 而跳过 mock。没有查询到 region 的本地来源会保留为未知。地图 README 记录数据来源及许可证；后续若要迁移 IP 定位，须先设计显式 opt-in、TUN 排除和隐私测试。
- 测试契约：新增 `docs/testing/contracts/traffic-analysis-main.md`；注册 `tauri-e2e/specs/traffic-analysis-main.e2e.ts` 到 `main` suite。它通过桌面控件验证 route 和 scope/metric query 保留，不把空报告/IPC 错误恢复或地图地理数据渲染当作覆盖目标。新增可执行的 `test:traffic` 单元入口覆盖 URL 到 report/topology 查询映射。
- 验收条件：四个视图可从主 UI 打开；范围和指标选项更新 URL，并在 Flow/Geography 切换时保留；Connections 规则用量代码将 rule filter 带入分析页；打包地图不产生外部定位请求；相关前端类型检查、构建、模型测试和已注册的桌面用例通过。新 E2E 不覆盖 Connections 详情交叉跳转，因为当前 main 路由用例没有安全、可控的活动连接 fixture；其代码映射由 review 检查，真实桌面行为尚未验收。桌面 E2E 当前因本机磁盘限制未完成，因此仍待新 SHA 的远端 E2E 结果。
- 验证：`pnpm typecheck` 通过；`pnpm --filter=chimera-ui build` 通过；`pnpm --filter=chimera-ui test:traffic`，5 passed；`pnpm --filter @chimera/tauri-e2e test:unit`，25 passed；`pnpm lint:frontend-boundaries`、对全部改动前端/契约文件的 Prettier 检查和 `git diff --check` 通过。`pnpm e2e:tauri:build` **未通过**：本机磁盘仅有约 3.2 GB 可用，Rust 在生成 `backend/target/e2e/debug/deps/libchimera_lib.a` 时报告 `No space left on device`；仅删除本次新建的 `backend/target/e2e` 构建输出恢复空间。真实桌面 `traffic-analysis-main.e2e.ts` 因无可执行文件而未运行，不能据此声称桌面验证通过；远端 PR Desktop E2E 仍需按新 SHA 验证。
- 风险与回滚：本批不改变宿主网络、服务权限或用户数据；完整回滚需移除 `frontend/chimera/src/pages/(main)/main/topology/`、`frontend/chimera/src/pages/(main)/main/_modules/{data-page.tsx,use-usage-label-of.ts}`、`frontend/chimera/src/utils/{traffic-retention,traffic-usage}.ts`、`frontend/chimera/src/components/ui/navigation-sidebar.tsx`、`frontend/chimera/src/assets/maps/`，并恢复 `frontend/chimera/src/components/ui/button.tsx`、`frontend/interface/src/ipc/use-traffic.ts`、`frontend/chimera/src/pages/(main)/_modules/navbar.tsx`、`frontend/chimera/src/pages/(main)/main/connections/index.tsx`、`frontend/chimera/messages/{en,ru,zh-cn,zh-tw}.json`、`frontend/chimera/package.json` 和 `frontend/chimera/src/routeTree.gen.ts`。同时移除 `frontend/chimera/tests/traffic-search.test.ts` 及其 package script、`tauri-e2e/specs/traffic-analysis-main.e2e.ts` 与 `tauri-e2e/spec-suites.ts` 注册、`docs/testing/contracts/traffic-analysis-main.md` 和本 DIFF 记录。地图 generator 未迁入；若未来要让地图数据更新自动对齐 ref，需单独补齐可复现生成器。
- 收敛条件：在补入并验证本机 Tauri E2E 之前，标记为“前端构建和单测通过、桌面 E2E 未验证”。只有在明确批准 IP 定位的隐私/网络合同并通过专用隔离测试后，才评估迁移 ref local-IP probe；legacy UI 和 agent 继续复用同一共享数据 API，不复制业务逻辑。

## DIFF-055：远程 Profile 可关闭自动同步

- 目标：按 ref 增加远程 Profile 的自动同步开关；间隔设为 0 时，周期刷新与启动补刷停止，手动同步仍可用。
- 非目标：不改变手动同步语义，不改 Core/Service 协议、代理/TUN 宿主设置、Agent API 或 legacy UI 的路由和导航；legacy 编辑器保留数字间隔输入，并只增加说明 0 会关闭自动同步的提示。
- ref 基线更新：该功能分支原本基于 master 的 `e81eeaea46a9c229478cd29c9555b5c072f7e6cc`；本工作树当时 checkout 在
  `c85406d3a16ba61cecf7f8aec7a5a051523cbc2d`，这是上游 PR #5775 提交，不是 main。现更新到主线
  `c08aa9d9a17953aa2464cdae61390d5c3f9afbc7`。GitHub 直接 fetch 超时；目标
  commit 已在本地对象库，且上游 Actions 页显示 `c08aa9d9` 的 main 构建。更新后 `git -C ref status --short` 为空。
- 映射：ref `backend/nyanpasu-config/src/profile/source.rs::RemoteSyncInterval`、
  `backend/tauri/src/state/profiles/actor.rs` 的自动刷新处理和
  `frontend/nyanpasu/src/pages/(main)/main/profiles/$type/detail/_modules/update-option-editor.tsx`；Chimera 对应
  `backend/chimera-config/src/profile/source.rs::RemoteSyncInterval`、
  `backend/tauri/src/state/profiles/scheduler.rs::RemoteUpdateScheduler`、
  `backend/tauri/src/client/profiles.rs`、`frontend/chimera/.../update-option-editor.tsx`，生成类型位于
  `frontend/interface/src/ipc/bindings.ts`。legacy 入口通过共享 `ProfilesClient` 保存，并由
  `frontend/chimera/src/components/profiles/profile-dialog.tsx` 适配数值间隔。
- 完整性：模型、配置校验、旧配置适配、迁移、actor 调度与 IPC patch 都接受 0；调度器取消周期任务且跳过启动补刷；保留
  手动刷新；main UI 按 ref 使用 `react-hook-form`/`zodResolver` 开关编辑，重新开启且间隔留空时保存默认 120；legacy 表单仍
  显示数字间隔并允许 0；修复 legacy 对只读远程物化文件的编辑路径，避免更新选项时错误 upsert 文件内容。
- 持久化与接口：远程 profile 的 `update_interval_minutes` 现在允许 0，IPC 序列化仍是数字，`bindings.ts` 由既有生成器更新；
  不改变 YAML 键或其他用户数据。增加 `@hookform/resolvers` 以保持 main editor 的 ref 表单结构。
- 风险：L2（持久配置、调度器与共享 Profile API）。任何 scope 重叠仍按 L2 处理。
- 验收：Rust 定向测试、main/legacy/E2E TypeScript 检查、manifest tests、前端 IPC 边界检查、真实桌面流程以及 loopback 手动
  请求均已执行。本机 WebKit 后台窗口会暂停 Motion 退场动画，视觉折叠与冷启动恢复不在本次实测覆盖内；远端 CI/E2E 尚未启动，
  不宣称通过。
- 实际验证：`cargo test --locked --manifest-path backend/Cargo.toml -p chimera-config profile`；`chimera` 的
  `scheduler_stops_refreshing_when_interval_is_patched_to_zero`、
  `disabled_auto_sync_skips_startup_catch_up_but_allows_manual_refresh`、
  `preserves_disabled_remote_sync_interval_in_runtime_adapter` 定向测试均通过。`pnpm typecheck:chimera`、
  `pnpm --filter @chimera/tauri-e2e typecheck`、`pnpm --filter @chimera/tauri-e2e test:unit`（25/25）、
  `pnpm test:manifest`（8/8）、`pnpm lint:frontend-boundaries`、`pnpm lint:oxlint` 与 Prettier 检查通过。
  WebKit 桌面 spec `profile-auto-update.e2e.ts` 1/1 通过，使用隔离运行目录和 loopback HTTP fixture；未覆盖真实公网、系统代理/TUN、
  前台动画与冷启动。执行命令和运行环境详见 `docs/testing/contracts/profile-auto-update.md`。
- 审查和批准：需独立复核真实 diff、ref 符号映射、失败/清理路径和宿主影响；L2 maintainer approval 后才提交和推送。
- 回滚：迁移或手工恢复已保存 0 的 interval 后，再回滚模型、配置兼容、调度器和 UI；避免旧程序把 0 当作无效配置。

## DIFF-058：流量报告暂停策略与桌面导航验证收敛

- 目标与非目标：将共享流量报告在 `refetchInterval: false` 时的暂停语义对齐本地 ref，并验证主界面流量路由在真实桌面窗口中的查询状态和侧栏辅助标签。此批不迁移 ref 的默认 2 秒轮询或 `placeholderData`，不增加本地 IP 探测，也不测试系统代理、TUN、运行时采集或连接详情的规则用量交叉跳转。
- 基线与映射：功能提交最初基于父仓库 `83a7223da5d9d9c6c789bcc32cc320aecf3ee60a` 和 ref gitlink `c08aa9d9a17953aa2464cdae61390d5c3f9afbc7`；当时只读 ref 检查在 `6feb4a71016e54260c1592f3a0d63a0169a2b40a`。随后本分支合入最新父仓库 master `ae48f24ae5012a47533784695d7109cafb753d12`（其 ref gitlink 为 `31019da14006b384b3272c72b4088179a9e72c47`）；本次另行 fetch 并核实 ref 最新 `origin/main` 为 `a4f706fe788e3570247ad2fa49636c6b4c55c98b`，ref 根目录和递归子模块均干净。`6feb4a71..a4f706fe` 未改动本批映射的 `ref/frontend/query/src/ipc/use-traffic-report.ts::useTrafficReport` 与 `ref/frontend/ui/src/navigation-sidebar.tsx::NavigationSidebarToggle`；该区间新增/调整 Flow Sankey 文件和 topology transition，将作为下一批对齐。`useTrafficReport` 的暂停、窗口聚焦/网络重连和 `retry: false` 行为对应 `frontend/interface/src/ipc/use-traffic.ts::useTrafficReport`。导航侧栏对应 `frontend/chimera/src/components/ui/navigation-sidebar.tsx::NavigationSidebarToggle`；Chimera 增加可选 `expandLabel`，拓扑侧栏传入本地化展开/收起文案。未重命名上游共享符号。
- 类别与接口：L2，共享查询行为会被拓扑和 Connections 状态统计使用；另有主界面可访问标签及四种语言消息。`useTrafficReport` 的选项限定为 `{ enabled?: boolean; refetchInterval?: number | false }`，并新增可测试的 `trafficReportQueryOptions` 描述器供 hook 复用。搜索确认 legacy 页面和 agent 当前没有 `useTrafficReport` 调用点；主界面仍通过同一共享 hook 读取。未改 IPC 命令、持久化、后端 runtime、legacy UI 或 agent 合同。
- 保留差异：相较当前 ref，Chimera 的 hook 仍未设置 ref 的默认 2 秒 `refetchInterval` 和按 topology layers 保留 `placeholderData`。显式配置轮询的调用方按 ref 关闭自动 retry 并由下一轮恢复；未配置轮询的 Connections 调用方保留 QueryClient retry，避免瞬时错误后不再恢复。此项只部分对齐，默认轮询和 placeholder 行为后续按调用方需要单独评估。
- 测试契约与验收：`docs/testing/contracts/traffic-analysis-main.md` 更新为 L2，并说明当前覆盖边界。已注册的 `tauri-e2e/specs/traffic-analysis-main.e2e.ts` 通过真实 main 窗口验证 Dashboard 准备、Traffic 导航、侧栏展开/收起标签变化、Closed scope、键盘选择 Connections，以及 Flow/Geography 保留 URL 状态。共享 hook 的单测验证未提供轮询时瞬时 IPC 错误会按 QueryClient 策略重试、显式轮询会留待下一轮恢复、暂停选项会关闭 focus/reconnect/retry。桌面 E2E 不声称覆盖这些后台事件、Connections 详情跳转、Tab 顺序或原生鼠标事件。
- 实际验证：`pnpm --filter=chimera-ui test:traffic` 8/8；`pnpm --filter @chimera/tauri-e2e test:unit` 25/25；`pnpm --filter @chimera/tauri-e2e typecheck`、`pnpm typecheck`、`pnpm lint:frontend-boundaries`、`pnpm --filter=chimera-ui build`、Prettier 和 `git diff --check` 均通过。使用外接盘独立 Cargo target 构建 `backend/tauri` 的 `e2e` feature 成功，最终二进制 SHA-256 为 `920c0cec03ff6cd854dc13e1ffb02c167bc866fb136d8aa8cc56a4caebb5e84e`。v6 和 v7 两次桌面运行失败并生成失败截图/HTML，日志有 `execute/async` 超时；之前文档将其报告为通过属错误。相同二进制在 2026-10-17T07:31:21Z 通过新隔离目录 v8 运行，macOS WebKit 605.1.15 为 1/1 通过；失败产物目录为空。精确命令见 `LESSON-20261017-003`。本地未运行 Windows/Linux E2E。
- 历史远端复验：已测试的 PR #489 SHA `13699341fc9389b4c18369609618e2fc944dddc3` 的 Windows Desktop E2E run [39037138976](https://github.com/MFSGA/Chimera/actions/runs/39037138976) 通过，53/53 specs；独立 Profile auto-update 和 controller fallback 各 1/1 通过。精确 SHA 的 [三平台 CI run 39037138645](https://github.com/MFSGA/Chimera/actions/runs/39037138645) 中 macOS、Ubuntu、Windows lint 与 Tauri 构建全部通过。这些结果只证明该历史 SHA；后续 PR head 的 CI/E2E 状态以 PR #489 最新检查为准。
- 风险、回滚与审批：不改变宿主网络、服务权限或用户数据。产品回滚限于恢复 `useTrafficReport` 原查询选项、拓扑侧栏标签调用/消息及本批 E2E/契约；ref gitlink 同步保持为独立任务。L2 产品改动需维护者复核和明确批准；因同时更新 `LESSONS.md`，该提交还需 L3 维护者批准。

## DIFF-056：按 ref 拆分核心 sidecar 解析器并补齐打包入口

- 目标：把二进制信息、平台映射和版本解析从 `scripts/check.ts` 收拢到 ref 对应的
  `scripts/src/prepare/` 模块；准备并打包 Chimera 当前支持的所有 core sidecar。
- 非目标：本批不搬迁下载/解压实现、进度显示或任务执行器，不改 Core/Service IPC、用户配置、
  数据格式或宿主网络行为；不引入 Chimera 尚未公开支持的 Meow core。
- ref 基线：`813673a8e84b51265467f02277fc7e739837f61e`（本地 `ref/` 与 `origin/main` 一致，
  工作树干净）。
- ref 到 Chimera 的路径与符号映射：
  - `ref/scripts/src/prepare/types.ts` 的 `ClashManifest`、`ResolveInfo`、`TaskDetail`、
    `DownloadProgress`、`DownloadResult`、`ResolveOptions` → `scripts/src/prepare/types.ts` 同名类型。
  - `ref/scripts/src/prepare/platform.ts::{normalizePlatform, normalizeArch, mapArch}` →
    `scripts/src/prepare/platform.ts` 同名函数；保留 ref 架构并扩展到 Chimera manifest 已支持的
    十种平台/架构组合。
  - `ref/scripts/src/prepare/versions.ts::{normalizeVersion, isValidVersion,
    extractVersionFromAssetList, fetchText}` → `scripts/src/prepare/versions.ts` 同名函数；
    `scripts/manifest.ts` 复用版本提取和文本请求实现。
  - `ref/scripts/src/prepare/binaries.ts::createBinaryResolvers` 及 Clash Premium、Mihomo、
    Mihomo Alpha、Clash-rs、Clash-rs Alpha resolver → `scripts/src/prepare/binaries.ts` 同名
    factory 和 resolver。保留 Chimera Client、Chimera Client Alpha、Chimera Service 扩展；
    Alpha Client 的刷新继续调用 Chimera manifest resolver，Service 版本继续来自本地 Cargo manifest。
    Mihomo Alpha 元数据请求异常时回退到版本 manifest；版本变化时通过 sidecar 版本戳使旧缓存失效。
  - `ref/scripts/src/prepare/check.ts` 的 core task 注册和
    `ref/backend/tauri/tauri.conf.json::bundle.externalBin` → 当前 `scripts/check.ts` 的
    task 注册及 `backend/tauri/tauri.conf.json::bundle.externalBin`。本批让 Clash Premium 和
    Mihomo Alpha sidecar 同时进入准备与 Tauri 打包清单，避免仅能解析却未随应用分发。
- 保留差异与收敛状态：记录创建时为部分迁移；后续 DIFF-057（PR #480）已把下载/解压、资源解析、
  进度和任务调度迁入同一 `scripts/src/prepare/` 模块，`scripts/check.ts` 保留为兼容委托入口。
  当前仅保留窄范围产品差异：ref 支持 Meow，但 Chimera 的应用配置和 IPC core 合同没有 Meow 变体，
  service endpoint 也拒绝该类型（底层 metadata 仍保留枚举），因此不注册 Meow sidecar。现有十种
  manifest 架构映射和 Chimera Service/Client 资产命名保留。
- 变更接口：新增内部 `createBinaryResolvers(BinaryResolverOptions)` factory；新增平台与版本 helper
  模块；`scripts/check.ts` 通过 factory 解析现有及补齐的 sidecar；Tauri bundle 增加 `clash` 和
  `mihomo-alpha` external binaries。无应用 IPC、前端 API、持久化或用户配置接口变化。
- 风险：L2（sidecar 下载、二进制分发及运行时 core 资源）。首轮独立审查发现 Alpha 缓存失效和
  元数据网络异常回退两项阻断问题；已增加版本戳校验与 pinned-manifest 回退及回归测试。修正后的独立复审已完成，
  无代码阻断项；审查建议同步修正本记录的测试数量与复审状态，现已更新。用户要求在远端 CI 运行期间继续独立批次；
  本批可在本地验收与独立复核完成后并行推送，但必须等本批自己的必需检查与审查通过后才能合并。
- 验收条件：十种架构均得到对应资产映射；Alpha 版本解析可刷新，元数据不可用时能回退且缓存按版本失效；
  Service 版本、归档名和 SHA-256 清单 URL 由本地 crate 版本推导；当前支持的 core sidecar 已注册准备任务并列入 bundle；Deno
  锁定测试、类型检查、lint、格式和差异检查通过；独立审查无阻塞项；推送该批 SHA 后所需 CI 与
  桌面 E2E 成功，失败则在同一分支修复并以新 SHA 重验。
- 验证计划与限制：`pnpm test:manifest`、带仓库 Deno 配置的冻结锁测试、`deno check`、
  `pnpm typecheck`、`pnpm lint:oxlint`、Prettier、`git diff --check`；推送后依赖 CI 的各平台
  lint/build 和 Tauri Desktop E2E 检查 sidecar 下载与打包。未在本机运行会下载并替换忽略 sidecar
  文件的 `pnpm prepare:check`，也未声称其本地通过。
- 回滚：在本批 feature 分支撤销上述解析器、task 注册、externalBin、测试入口/锁和本条记录；
  不需数据迁移或用户状态恢复。
- 阶段证据：DIFF-056 初始切片的 `pnpm test:manifest` 曾为 17/17；DIFF-057 完成后，全管线测试为
  28/28。测试入口固定使用 `scripts/deno.lock`，并将测试使用的 `@std/assert` 版本写入该锁，
  防止在仓库根目录生成额外锁文件。DIFF-057 当前切片的 Deno frozen check、Oxlint、Prettier
  与差异检查也通过。曾实际运行 `pnpm prepare:check --arch arm64 --sidecar-host aarch64-apple-darwin`，
  但下载反复重试且无进度后中断；随后对 Clash Premium、Mihomo Alpha 与 Chimera Client Alpha 的 GitHub 元数据/资产 URL
  执行请求均在 12 秒超时，故本地实际下载与安装未验证，不能记为通过。该批仍为部分迁移；推送后需记录该 SHA 的
  CI/E2E 结果，远端未通过前不得合并；DIFF-057 已完成此处预留的管线迁移。

## DIFF-057：迁移 prepare 下载、资源安装、进度和任务执行器

- 目标：完成上一切片预留的 prepare 管线迁移，将下载/解压、资源解析、进度渲染和任务调度放入 ref 对应的 `scripts/src/prepare/` 文件，并让 `pnpm prepare:check` 直接运行该入口。
- 非目标：不改变 manifest 版本解析、平台映射、应用 Core/Service IPC、用户配置、宿主网络/TUN 行为；不注册应用未支持的 Meow core；不在本机下载或替换真实 sidecar/资源来验证。
- 风险：L2（准备并原子安装 sidecar 与运行资源）。维护者已批准该批次提交/推送；合并仍需新 SHA 的 CI/E2E 通过。
- ref 基线：`813673a8e84b51265467f02277fc7e739837f61e`，本地 `ref/` 已核对为该 SHA 且工作区干净；本批未改动或重新同步 ref。
- ref 到 Chimera 的路径与符号映射：
  - `ref/scripts/src/prepare/download.ts::{downloadFile, extractZip, extractTarGz, gunzipFile}` → `scripts/src/prepare/download.ts` 同名函数；流式下载和解包流程沿用 ref。
  - `ref/scripts/src/prepare/progress.ts::{ProgressRenderer, formatSize, formatSpeed, formatProgressSize, formatResolveInfo}` → `scripts/src/prepare/progress.ts` 同名类和函数。
  - `ref/scripts/src/prepare/resources.ts::{resolveResource, resolveSidecar, resolveWintun}` → `scripts/src/prepare/resources.ts` 同名解析器；保留 Chimera 现有 Alpha metadata refresh、sidecar SHA-256 和临时版本戳。
  - `ref/scripts/src/prepare/check.ts::{runTask, resolveSidecarHost, main}` → `scripts/src/prepare/check.ts` 同名入口和任务执行路径；`createPrepareTasks`、`selectPrepareTasks` 放在 `scripts/src/prepare/tasks.ts`，并由独立测试检查支持的核心、资源和 Windows 任务。使用 Chimera 的 logger、manifest、资源路径，并注册 Clash、Mihomo、Mihomo Alpha、Clash-rs/Alpha、Chimera Client/Alpha、Chimera Service、Wintun、MMDB、GeoIP、GeoSite 与 Windows loopback 工具。保留十种架构和现有服务/客户端资产名。
  - 历史入口 `scripts/check.ts` 保留为兼容委托；`package.json::prepare:check` 改为直接运行新模块。
- 保留差异与缺陷修正：不注册上游 Meow，因为 Chimera 的 core 配置/IPC/service 合同不支持它；用现有 `verifyAndInstallSidecar` 为 sidecar 保留校验和 staged atomic install。普通资源现在也先写入同目录临时文件、下载完成后再替换：中断下载会留下半截文件并被下次按“已缓存”采用，已用测试复现该旧行为并验证新实现保留原缓存、清理临时文件。Wintun 的每次重试都会丢弃未完成归档；DLL 经同目录暂存并原子替换，失败时保留上次可用版本。迁移同时保留 DIFF-052 中仅针对 Chimera Client Alpha 滚动资产 404 的 1/2/4/8 秒有限重试；其他 sidecar 及非 404 错误不进入此延迟重试。这些是窄范围恢复性修正，ref 应在后续切片复查。
- 影响范围：仅改变 Node/Deno prepare 脚本入口、脚本测试执行权限和本地准备资源流程；主界面、legacy UI、agent、持久化数据和宿主代理状态不变。
- 验收：下载、失败清理、Alpha 刷新及有界 404 重试、非 404 快速失败、checksum 失败保留旧 sidecar、Wintun 重试与旧 DLL 保留、任务进度、注册任务清单、历史入口和所有当前 core task 可检查；`pnpm test:manifest`、Deno frozen check、workspace typecheck、oxlint、Prettier 和 diff 检查通过；独立复核无阻断项；推送后的必需平台 CI 与 Desktop E2E 通过。
- 验证计划/限制：使用假 HTTP 响应和隔离临时目录覆盖下载与恢复；本地 `pnpm prepare:check` 会访问网络并替换被忽略的 sidecar/resource 文件，本批不运行该命令。全平台真实下载与 Tauri 打包由本批推送后的工作流验证。
- 回滚：回退 `scripts/src/prepare/{check,download,progress,resources}.ts`、相应测试/测试入口、`prepare:check` 路径及本条记录；无用户数据迁移。
- 当前本地证据：`pnpm test:manifest` 28 passed、0 failed；新增假 HTTP 用例验证滚动 Alpha 在短暂 404 后恢复、重试预算耗尽时失败，以及 503 不触发延迟重试；恢复成功检查实际安装字节和版本戳，耗尽场景检查原 sidecar 与版本戳保持不变。任务级回归用例实际执行 Alpha task，验证该重试策略生效且 Mihomo Alpha 不使用该策略。下载与资源测试通过共享 `withTempFetch` 包装，在 setup/body 失败时仍独立恢复 fetch mock、清理临时目录并保留主要与清理异常。`deno check --frozen --config scripts/deno.jsonc scripts/check.ts scripts/src/prepare/check.ts scripts/src/prepare/tasks.ts`、`pnpm lint:oxlint`、Prettier、`git diff --check` 通过。更新到当前 master 的外部 worktree 中，前端/E2E typecheck 因 node_modules symlink 指向另一个 worktree 且缺少解析依赖，未能有效执行；本批新 SHA 的 CI 将提供类型检查与构建复验。针对旧 CI 失败的 `client::profiles::tests::import_suggestion_is_committed_and_reschedules_the_timer` 定向 Rust 测试在本机通过 1/1；本机 `pnpm prepare:check` 未运行，不宣称真实资源下载/安装已验证。
- 远端失败证据（修复前的 #480 head `c797797d16cc2453b01ca460a2adf3e6b90a1d67`）：CI https://github.com/MFSGA/Chimera/actions/runs/38558416365 的 Ubuntu 后端测试因观察 fetch 与 actor 提交之间的竞态失败，现已改为等待提交后的快照；Desktop E2E https://github.com/MFSGA/Chimera/actions/runs/38558416387 的 Windows 运行中 51/52 个 spec 通过，`rules-main-proxy-icon.e2e.ts` 未加载图标。PR #479 的缓存 generation 修复已通过全平台 CI 和 Desktop E2E，并于 2026-10-14 合入 master；#480 的新 SHA 仍须重新执行必需检查。当前本地 `pnpm prepare:check` 未运行，不代表真实资源下载/安装验证通过。
- 后续远端复验（PR #480 head `4a92a6916f16e2f768f2e83c1db608958fc88a49`）：[CI run 38571885904](https://github.com/MFSGA/Chimera/actions/runs/38571885904) 的 Linux/Windows lint 通过，macOS lint 在 `pnpm prepare:check` 下载 `chimera-client-alpha` 时收到 `404`；同一 SHA 的 [Desktop E2E run 38571885920](https://github.com/MFSGA/Chimera/actions/runs/38571885920) 通过。失败发生于 2026-10-14 13:31 UTC。GitHub 于 2026-10-15 02:51 UTC 发布的 [`Chimera_Client` latest release](https://github.com/MFSGA/Chimera_Client/releases/tag/latest) 当前包含预期的 `chimera-client-0.27.1-aarch64-apple-darwin.tar.gz` 资产；这说明该资产后来可用，但不能单独证明失败的历史原因。没有更改 resolver 或扩大重试预算；新 SHA 的 macOS prepare 检查仍须重新验证。当前本地没有运行 `pnpm prepare:check`，也没有下载或替换 sidecar。

## DIFF-047：同步最新 ref 的 Tokio patch 版本

- 基线：原始功能分支基于主仓库 `e9e3fab1913773a1ecd6ad6a689175e523208cc2`；只读 ref 当前固定在
  `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3`，工作树干净。该快照包含
  `67999753918c7b9879ddd2bacdef74246bfef1dd`，其 Tokio 锁定版本为 `1.53.3`。
- ref 到 Chimera 映射：`ref/backend/Cargo.lock` 的 `tokio` package 条目对应
  `backend/Cargo.lock`；双方的工作区清单都以 `tokio = "1"` 声明兼容范围。
- 类别：共享 Rust 依赖对齐。
- 更新：将锁定 Tokio 从 `1.53.2` 升到 ref 当前的 `1.53.3`，不改任何 `Cargo.toml` 约束或运行时代码。
  与 master `5fc556b4` 同步时保留其完整依赖图；相对该 master，锁文件只变更 Tokio 版本和校验值，
  没有带入无关的 Windows 依赖重解析。
- 影响入口：Rust workspace 中的 Tauri 应用、共享 actor、核心管理器、Service IPC 和 Agent 共用 Tokio；
  主 UI、legacy UI 与 agent 的业务 API、持久化、IPC 合同及用户配置格式均未改变。Windows 依赖图由 CI 验证。
- 验证：同步 master 前的功能分支上，`cargo check --locked --manifest-path backend/Cargo.toml --workspace` 通过；
  `RUST_TEST_THREADS=1 pnpm test:backend` 的全 workspace `--all-features` 测试退出码为 0。
  默认并行运行首次为 549 passed、3 failed、1 ignored，失败均为 `client::profiles` 外部文件监听
  用例在 5 秒内未观察到更新时间；随后 `external_` 子集分别以默认并行（4/4）和
  `--test-threads=1`（4/4）通过，串行全 workspace 也通过。原因尚未由本地证据确定，保留首次失败记录，
  由推送后的 CI 决定是否需要修复。首次 check 因本机磁盘空间不足失败；仅使用
  `cargo clean -p chimera` 清理可重建的应用构建产物后，check 通过。同步 master 后，
  `cargo metadata --locked --format-version 1 --manifest-path backend/Cargo.toml` 通过，且相对 master
  的锁文件差异仅为 Tokio 版本/校验值。`pnpm exec prettier --check docs/ref-alignment-progress.md`
  和 `git diff --check` 通过。最终分支的 Windows/macOS/Linux CI 和 Tauri E2E 以 PR 当前 SHA 检查为准。
- 收敛状态：Tokio 版本与当前 ref 一致；最终合并分支仍须通过 PR 当前 SHA 的 CI 和 Tauri E2E。

## DIFF-059：日语界面语言

- 目标与范围：按上游日语语言支持让主 UI 和受支持的 legacy UI 共享 `ja` 语言设置与翻译资源；安装器语言选择单列为 L3 发布配置，不包含在此 L2 功能验收中。
- 基线：当前 master 固定的本地 `ref/` 为 `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3`，工作树干净；日语功能来自上游 `b3150805e` 并已包含在该基线。本批未修改或移动 `ref/`。
- 映射：ref `backend/nyanpasu-config/src/application/i18n.rs::{I18nLanguage,default_i18n_language,I18nLanguage::as_str}`、`backend/tauri/locales/ja.json`、`frontend/nyanpasu/messages/ja.json`、`project.inlang/settings.json`、`src/assets/styles/tailwind.css`、`src/utils/language.ts`、`src/components/providers/language-provider.tsx`；Chimera 对应 `backend/chimera-config/src/application/i18n.rs`、`backend/tauri/locales/ja.json`、`frontend/chimera/messages/ja.json`、`project.inlang/settings.json`、`src/assets/styles/tailwind.css`、`src/utils/language.ts`、`src/components/providers/language-provider.tsx`、`backend/tauri/src/bridge/verge.rs`、`backend/tauri/src/client/application.rs` 与生成的 `frontend/interface/src/ipc/bindings.ts`。日历语言在 Chimera 的 Day.js 根初始化处注册，因为项目当前使用 Day.js。
- 行为和接口：增加 `I18nLanguage::Japanese`、从系统 locale 识别日语和 Rust bundle 的规范 locale key；客户端完成 typed 配置加载后再设置 Rust-native locale，避免新安装时前端与 tray 翻译语言分离。旧配置 `ja-JP` 通过共用映射选择 `ja` bundle。前端 locale cache 和首屏状态按 ref 的 `getCachedLanguage`、`setCachedLanguage` 与 `LanguageProvider` 初始化门控实现；后端查询完成前保留缓存作为快速路径，首次无缓存时应用后端解析语言，持久化值和缓存不一致时再同步。主、legacy 入口继续使用同一个 provider；IPC 命令不变，序列化配置接受历史 locale 写法并输出 ref 对齐的小写规范值。日语与英文目录现有 908 个键及占位符一致，后端日语 bundle 与英文 bundle 的 19 个叶键一致。
- 桌面用例：新增 `docs/testing/contracts/user-interface-language.md` 与 `tauri-e2e/specs/user-interface-language.e2e.ts`，注册到 `settings` suite。用例聚焦语言控件并按 Enter 打开菜单、按 ArrowDown 选择日语、再按 Enter 确认，检查 IPC、文档 `lang`、隔离运行目录中的 `application.yaml` 写盘值、刷新后的主 UI，以及 legacy 代理页的标题和三个模式标签；清理时恢复 `application.yaml` 中的原语言、两个 WebView 的语言缓存、路由和窗口尺寸，且各清理步骤失败时仍继续执行后续恢复。当前锁定的桌面驱动点击只合成 DOM `click`，Radix 菜单使用 `pointerdown` 打开，因此该契约覆盖键盘 handler，不声称覆盖鼠标打开、菜单动画或 Tab 顺序。用例不修改系统代理、TUN 或服务状态。
- 保留差异：Chimera 使用 `sys_locale` 读取系统 locale，并在本地已有的 Day.js 初始化中注册日语；ref 使用 `nyanpasu_helper` 与 date-fns。系统 locale 解析和 UI 日历表现按 ref 对齐，依赖与日期工具仍沿用 Chimera 现有调用链，待共享日期 API 迁移时再复核。旧配置的 `en-US`、`zh-CN` 与 `zh-TW` 通过 serde alias 继续可读，新写入使用 ref 的规范小写键；legacy `ja-JP` 映射留在边界适配器，待实际存量配置迁移覆盖后移除。兼容 `patch_verge_config` 在 legacy 配置落盘后才切换 Rust locale；若托盘刷新失败，会恢复 typed 配置中的原 locale 并重试原语言托盘。韩语前端目录仍未迁入，不属于本批目标；此处只新增日语。
- 风险与验收：共享语言状态与持久配置为 L2。较旧版本不认识本版写出的 `en`、`ja`、`zh-cn`、`zh-tw`；回退前需关闭应用，并将 `application.yaml` 顶层 `language` 改为旧版可读的 `en-US`。测试契约记录了各平台配置目录及该恢复步骤，Rust 回归测试用上一版 enum 验证 `ja` 会被拒绝且 `language: en-US` 可恢复读取。`.github/workflows/ci.yaml` 增加 `test:localization` 为 L3，须单独取得维护者批准；`backend/tauri/tauri.conf.json` 的 Wix/NSIS 日语安装选项不在本 diff 中，仍需未来独立审查。
- 验证：候选分支 `pnpm typecheck`、`pnpm --filter chimera-ui test:localization`（4/4；先由前端构建生成 ignored Paraglide runtime，CI 的前端测试步骤前已有 workspace build）、`pnpm --filter @chimera/tauri-e2e typecheck` 与 `pnpm --filter @chimera/tauri-e2e test:unit`（25/25）通过。`cargo +nightly test --locked --manifest-path backend/Cargo.toml -p chimera-config`（147/147）、legacy locale/rollback 定向测试（31/31）和 `japanese_write_failure_preserves_previous_application_state`（1/1）通过；包含旧版本 enum fixture 的 `downgrade_recovery_resets_language_to_a_value_the_previous_release_reads` 已在完整 147 项套件中通过。`typescript_bindings_are_fresh`（1/1）通过，生成绑定由 `update_typescript_bindings`（1/1）更新。改动 Rust 文件的 `rustfmt +nightly --check --config-path backend/rustfmt.toml --config skip_children=true ...`、相关文件 Prettier、`git diff --check` 及 `actionlint -shellcheck='' .github/workflows/ci.yaml` 通过。全 workspace `cargo fmt --all -- --check` 仍报告未修改的 vendored WebDriver 文件格式差异，改动 Rust 源文件的定向检查通过。
- 构建与桌面验证：`pnpm --filter chimera-ui build` 通过。CDN 插件首次尝试 4 分 38 秒无响应后中断；重跑时临时用 npm 官方 `@inlang/plugin-message-format@4.5.4` 与 `@inlang/plugin-m-function-matcher@2.3.4` 本地副本，二者 SHA-512 与 registry 元数据一致，构建后恢复原 `project.inlang/settings.json` 并删除临时插件；未改依赖或锁文件。为避免 CI 前端构建后再重复请求这些 CDN 插件，`test:localization` 只运行测试，使用前端构建已生成的 ignored runtime。带 `e2e` feature 的 Tauri 候选 binary 使用移除 external sidecars 的临时配置构建，因此只用于语言界面用例，不用于 core/runtime 验收。macOS WebKit 605.1.15 的最终 `user-interface-language.e2e.ts` 为 1/1 通过且无重试；更早两次本地运行分别出现 WebDriver execute/sync 超时自动重试，以及 after hook 清理失败，根因未确认。最终成功 run 恢复并读取到原 `application.yaml` 语言。未验证完整 core sidecar、冷启动或其他平台；Windows CI/E2E 未在候选 SHA 执行。
- 审查和批准：独立审查确认初版存在 legacy `patch_verge_config` 在持久化前切换 Rust locale 的 P2 回滚缺口；现已将 locale/tray 应用移至持久化之后，并增加持久化失败不切换语言、托盘刷新失败恢复旧 locale 的测试。第二次复审发现回退到不支持日语的旧版本需要把 `application.yaml` 顶层语言复位为 `en-US`；该步骤已写入测试契约并由旧版 enum fixture 测试覆盖，复核确认 P1 已解决且无 P0–P2 阻塞项。另有两项非阻塞建议：托盘两次刷新均失败的错误聚合测试，以及后端 bundle 自动键集一致性测试。共享语言与配置行为为 L2，仍须维护者明确批准提交；新增 CI 执行入口为 L3，须单独批准。
- 回滚：移除日语语言枚举和 bundle、前后端目录、共享语言选项与 Day.js locale 注册、桌面 spec 注册及此记录；不迁移或删除已保存配置。安装器 Wix/NSIS 语言列表不属于此 L2 diff。


## 2026-10-19 本地 GeoIP 索引与 traffic 地理维度（L2）

- 目标：让共享 traffic 记录按运行内核实际使用的 GeoIP 数据库解析源/目的地区域；Main、Legacy 与 agent 继续走同一个 traffic actor 和 application API。
- 非目标：不新增公网 IP 探测、外部定位服务、UI 开关、TUN/系统代理变更或用户配置迁移；不修改只读 ref、删除 legacy UI 或改变 Client/Service 生命周期。
- 基线：Chimera `master` `09f89a45875581979446a50a716bba5bb36c32c1`；该基线的 ref gitlink 为 `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3`。功能实现时只读 ref checkout 为 `4eaf13b9c5dc468fdfc38272bf76b20a681d2e54`，工作区干净且与 `origin/main` 相同；已核对 `backend/nyanpasu-geodata/`、`backend/tauri/src/core/geo/` 和 `backend/tauri/src/core/traffic/` 在 `d43819f..4eaf13b9` 间无差异。本次保留 master 原 ref gitlink，不夹带基线更新。
- 路径和符号映射：`ref/backend/nyanpasu-geodata/` → `backend/chimera-geodata/`（仅替换包名品牌，保留解析器、错误处理、测试和许可证）；`ref/backend/tauri/src/core/geo/{actor,adapters,client,fixtures,mod,ports,tests}.rs` → `backend/tauri/src/core/geo/`；`ref/backend/tauri/src/core/traffic/{actor,geo,source}.rs` → 同名 Chimera 模块。`GeoIndexActor::{follow_core,reload}` 保留；将 ref `CoreClient` 适配为共享 `CoreLifecyclePort` 与现有 `ApiClient::get_configs`，从 `ClashRuntimeConfig.geodata_mode` 选择 MMDB 或 DAT。启动时等待核心 API 可用；之后以核心生命周期状态提供的稳定实例 ID 绑定控制器、WebSocket traffic 帧和 GeoIP 索引；`CoreStatusSnapshot.instance_id` 经各 host projection 传递，兼容旧 host 时只使用非零状态时间戳作为身份。每个 WebSocket 更新保留连接时解析到的核心 ID，模式查询和索引快照都携带同一 ID，traffic pump 只在三者一致时应用索引。核心切换期间不使用旧索引；若模式查询或数据库加载失败，traffic 帧仍可记录，但不使用旧地理索引，watcher/低频复核会继续尝试。文件 watcher 驱动数据库更新，60 秒低频复核兜底 watcher 漏报。`TrafficArgs` 新增只读 index watch receiver，`pump` 对每个 traffic frame 使用当前快照；`locate_source`、`locate_destination` 沿用 ref 的地址选择与 geo basis 规则。客户端只在 traffic store 可用时创建索引 actor，并从现有 core app-data 目录读取数据库、在 app cache 保存可丢弃的派生索引。
- 保留差异：ref 的 `LocalSourceCache`、公网 IP 检测和 `enable_local_ip_probe` UI/配置不迁入；Chimera 当前 `NoLocalSource` 返回空地址，因此私有/本机来源仍显示 `unknown`。模式读取复用 active core 的 `/configs` API 并设置 `no_proxy`；API 地址来自现有控制器绑定，若用户将 controller 配为非 loopback，读取也会发往该配置地址。没有新增 GeoIP provider、公网 IP 探测或地图请求。索引只读 core 本地文件。只有另行批准显式 opt-in、TUN 排除和隐私/网络测试后，才复查 local-source 探测。
- 接口：内部 `GeoIndexArgs` 改为注入 `CoreGeodataModeSource`；`TrafficArgs` 增加 `geo` watch；Rust `ClashRuntimeConfig` 和 Specta 输出增加可选 `geodata_mode` 字段，并由现有生成器更新 TypeScript binding。现有 traffic IPC 与 UI 消费合同不变。
- 验收和测试计划：执行 `cargo test --manifest-path backend/Cargo.toml -p chimera-geodata`、`cargo test --manifest-path backend/Cargo.toml -p chimera --lib core::geo::`、`cargo test --manifest-path backend/Cargo.toml -p chimera --lib core::traffic::`、Clash API geodata-mode 反序列化测试、Specta binding freshness、`pnpm typecheck`、`pnpm lint:clippy` 与 `git diff --check`；CI 应由已有 `pnpm test:backend` 全 workspace 门禁运行新增 Rust 测试。traffic 回归还需覆盖“核心 B 的索引已发布后，核心 A 的延迟帧不能使用 B 的索引”。真实 core/Service 和桌面网络验证不由这些单测覆盖。
- 风险、回滚与审批：L2（读取运行时配置、core 数据目录并管理派生缓存；不改变网络状态或用户配置）。回滚删除本批 crate、Geo actor 和 traffic 接线，移除新增的 runtime projection/binding 字段；已生成的缓存可安全删除。该实现属于用户已批准的持续 ref 对齐目标；本批不修改 L3 工作流或规范。
- 原功能分支验证结果：`c9228e99` 的 `pnpm test:backend`（Rust workspace、all features）通过；Tauri lib 单测为 613 passed、1 ignored，`chimera-geodata` 有 73 项通过，本地公开数据库 `--test real_databases -- --ignored` 3/3 通过。新增 core-instance 集成用例验证新核心 mode 读取失败时交通帧使用核心自带地区值而不是仍发布的旧索引；下一次读取成功后改用新索引。Geo actor 测试覆盖新实例相同模式的索引重载失败和同实例重试去重。该分支的 `pnpm typecheck`、`pnpm lint:clippy`（仓库有大量既有 Rust/Clippy warning，命令退出码为 0）、目标 crate `rustfmt --check`、相关文档/bindings Prettier、locked Cargo metadata、`pnpm lint:frontend-boundaries` 与 `git diff --check` 通过。`pnpm prepare:check` 和桌面/真实 Service 矩阵未运行：该脚本可能写入本机 sidecar/resource 资产；本批没有 UI 行为改动，未运行桌面 E2E。原功能分支独立复审无 P0–P2；发现的 watcher 漏报、同模式跨实例旧索引、controller 地址描述问题均已修复并覆盖。
- 最新 master 基线复验：GeoIP 实现最初基于 `09f89a45875581979446a50a716bba5bb36c32c1`，功能提交为 `d3b6b3e69dac8e6ecdd033eca8c82c68f18947ec`；审查修复提交为 `b21896b14759928b753415e3f29b76a598199ff1`。独立复审发现旧 WebSocket traffic 帧可能与当前核心模式/索引错配；现将核心实例 ID 贯通 lifecycle、WebSocket frame、模式响应和索引快照，并增加 A 核心延迟帧在 B 核心索引已切换后的回归测试，确保使用帧自带地区值而非 B 的索引。随后 master 合入已验证签名的 `arc-swap 1.9.3` patch 更新 `b264ec54f6f51797d4f59e5a8b9e1890cacc72de`；本分支以签名合并提交 `fe97bac3dc9c85221dab77e5b5727d4117b81c5a` 同步该基线，当前最终 SHA 的所有新增提交均已本地 GPG 验证，作者/提交者字段未包含 `lmmqxyx`。

  在更新后的 lockfile 上，traffic 定向测试 15/15、Geo actor 14/14；`chimera-geodata` 所有可运行测试通过，需要 `CHIMERA_GEODATA_DIR` 的真实数据库测试 3 项按设计忽略。此前完整 `pnpm test:backend` 在使用 `TAURI_CONFIG` 跳过未准备 sidecar/resource、关闭 Rust 增量编译后通过；Tauri lib 613 passed/1 ignored。该完整 workspace 运行发生在同步 `arc-swap 1.9.3` 之前；新 SHA 上重新运行的三个定向套件已通过。`pnpm typecheck`、`pnpm lint:frontend-boundaries`、`pnpm lint:clippy`（退出码 0，存在仓库既有 warnings）、目标 crate `cargo fmt --check`、Prettier 和 `git diff --check` 在先前功能 SHA 通过；完整 macOS/Windows/Linux CI 与桌面 E2E 需以同步后的最终 SHA 运行。`TAURI_CONFIG` 测试覆盖不包括桌面 bundle/sidecar；未验证 live core/Service replacement 或 WebSocket handshake。独立复核无 P0–P2 阻塞项，覆盖为 fixture-level traffic pump 测试与代码路径。此前 b218 SHA 的远端检查仍在运行，不作为最终结论；同步提交推送后，必须用新 SHA 检查结果决定是否合并。

## DIFF-060：流量拓扑 Sankey 图和可配置排名（L2）

- 目标：将 ref 的 Flow 页面 Sankey 可视化、卡片图备选、排名选择和持久化视图设置迁入 Chimera；Flow 与地图共用一份排名偏好，并继续经现有 traffic 页面状态与查询 API 工作。
- 非目标：不改变 traffic 查询、IPC、后端数据模型、legacy UI 路由、agent API、核心生命周期或网络/TUN 行为；不新增桌面 E2E 用例。
- 基线：只读 `ref/` 与远端 `origin/main` 均为 `03ab6e5824cb44e527bde7cca28b699e8bf3a766`，ref 工作树干净。Sankey/Flow 设置实现来自 ref 提交 `1990171f1`；本次同步新增的上游提交只更新 `manifest/version.json`，没有改变本批映射的 Flow/Sankey 文件。上游 Alpha 版本值与 Chimera manifest 的差异作为独立依赖批次评估，不在此 UI 批次中混改。
- ref 到 Chimera 路径和符号映射：`ref/frontend/nyanpasu/src/pages/(main)/main/topology/flow/index.tsx` → `frontend/chimera/src/pages/(main)/main/topology/flow/index.tsx`；`flow/_modules/{sankey-layout.ts,sankey.tsx,card-graph.tsx}` → 同名 Chimera 模块；`topology/_modules/{view-settings.tsx,view-rankings.tsx}` → 同名 Chimera 模块。保留 `TrafficViewSettingsProvider`、`useTrafficViewSettings`、`toTrafficViewSettings`、`ViewRankings`、`Sankey`、`layoutSankey`、`CardGraph` 的上游分解和符号；`traffic-context.tsx::RANKINGS` 改为 ref 顺序 `process,target,exit,source,inbound`。设置通过共享 `useKvStorage` 保存在 `traffic-view`，默认选择 ref 的前三个排名和 Sankey 图；地图和 Flow 排名使用同一设置，Flow 菜单还可切换 Sankey/卡片图。
- 保留差异：Chimera 现有 ranking card 不接收 ref 的 `dimension` 属性，因此调用保持本地组件合同；保留 Chimera 已有的 process/target/exit/source/inbound 独立排名页面，不因 ref 统一入口而删除。Sankey 布局用普通 `Map` 分组及复制后排序，替代 ref 的 `Map.groupBy`/`toSorted`，计算顺序与结果不变，避免增加运行时 API 要求。当前仓库没有设置显式 Vite 浏览器 target 或 Tauri macOS minimumSystemVersion；若将来统一提高运行时最低版本，可重新评估这项差异。
- 影响范围与接口：新增只影响 Main traffic topology 页面内部组件、一个 KV 键和 5 种现有 UI 语言的消息；无 Rust/IPC、数据 schema 或 agent contract 变化。检索确认 legacy 页面没有对应 topology/Flow 路由，本批没有改其交互；agent 不使用该 presentation 设置。
- 验收与验证计划：排名顺序/未知值清理、存储迁移和非法图类型回退由纯函数测试覆盖；Sankey 位置、边宽、过滤及最小高度由布局测试覆盖；运行 `pnpm --filter chimera-ui test:traffic`、`test:localization`、`pnpm typecheck`、Main UI production build、受影响文件 Prettier、`pnpm lint:frontend-boundaries` 与 `git diff --check`。不运行桌面 E2E；这些纯测试和构建不代表真实桌面点击验证。
- 风险与回滚：L2（新增持久化 UI 偏好及主界面图形交互）。恢复方式是回退本批 Flow/排名/设置组件、上下文顺序、消息、单测、依赖入口和本条记录；`traffic-view` KV 值可保留，不影响旧代码读取的配置。
- 当前本地验证：`pnpm --filter chimera-ui test:traffic` 16/16、`pnpm --filter chimera-ui test:localization` 4/4、`pnpm typecheck`、`pnpm lint:oxlint`、`pnpm lint:frontend-boundaries`、`pnpm --filter chimera-ui build`、受影响文件与本记录的 Prettier 检查、`git diff --check` 均通过。构建只报告 Vite 既有的大 chunk 与 ripple 动态导入警告。Main/legacy 桌面 E2E 未运行，真实 WebView 和 persisted cold-start 行为未在桌面进程中验证。

## DIFF-061：同步最新 ref 子模块快照

- 目标与范围：把 Chimera 工作区使用的只读 `ref/` 快照同步到 Clash Nyanpasu 上游最新 `main`；不改变 Chimera 的产品依赖、功能代码或用户数据。
- 基线与更新：从 `03ab6e5824cb44e527bde7cca28b699e8bf3a766` 拉取上游 `main` 后，快进到 `9f72efe292975b13b5d9018a9f5641ffbb071c27`。新增上游提交为 `1f59b4fa8`（更新 `postcss` 至 8.5.32）和 `9f72efe29`（更新 `@inlang/paraglide-js` 至 2.29.2）；改动仅涉及 ref 的两个 package manifest 和 `pnpm-lock.yaml`，未改 Dashboard 实现。
- 验证：fetch 后 `ref/HEAD` 与 `ref/origin/main` 均为 `9f72efe292975b13b5d9018a9f5641ffbb071c27`，`git -C ref status --short` 为空；新 SHA 可由旧 SHA 快进到达。该批只同步参考子模块，不运行产品构建或测试。
- 影响与回滚：父仓库 `ref` gitlink 从 `03ab6e5824cb44e527bde7cca28b699e8bf3a766` 改为 `9f72efe292975b13b5d9018a9f5641ffbb071c27`；回滚仅需恢复原 gitlink。此更新作为单独提交推送，避免与 Dashboard 功能和 E2E 修复混在一起。

## Windows WebView2 随最小化/恢复同步可见性（Windows E2E 已通过；CI 依赖修复待新 SHA 验证）

- 目标：对齐固定 ref `9f72efe292975b13b5d9018a9f5641ffbb071c27` 中 `window/engine.rs::set_in_sight` 与 `window/manager.rs::follow_minimized`。最小化窗口时隐藏 WebView2 controller 并请求低内存目标；恢复或重新聚焦时先恢复 WebView 可见性和正常内存目标，再显示/聚焦原生窗口。
- 非目标：不迁移整个上游 `WindowManager`，不改变窗口创建/销毁/尺寸恢复/关闭行为，不新增托盘隐藏逻辑，不改权限或持久配置，不影响 Chimera Client/Service、legacy UI 和 Agent。
- 参考映射：`ref/backend/tauri/src/window/engine.rs::set_in_sight` → 新增 `backend/tauri/src/window/engine.rs::set_in_sight`；`ref/backend/tauri/src/window/manager.rs::{watch,follow_minimized,focus_existing_window}` → `backend/tauri/src/window.rs::{AppWindow::create,follow_minimized,focus_existing_window}`。保持上游的 `IsVisible` 幂等检查、`SetIsVisible`、`ICoreWebView2_19::SetMemoryUsageTargetLevel` 及先恢复 WebView 再显示窗口的顺序。COM 依赖与锁定 Tauri/Wry 版本匹配：`webview2-com 0.39` / `windows-core 0.62`。
- 接口与风险：新增仅内部可见的 `window::engine::set_in_sight`，并在原生窗口 resize/minimize/restore 事件接线；没有 IPC、前端公开 API、存储 schema 或网络行为变化。L1 Windows 窗口生命周期与资源使用。
- 验收：真实点击 main 窗口最小化控件后，Windows WebView 的 `document.visibilityState` 变为 `hidden`；恢复窗口后回到 `visible`。定向 hermetic desktop E2E 在 PR CI 执行。Rust 格式、编译、单测、E2E 类型检查和该用例均需覆盖；Mac 本地结果只证明跨平台 no-op 与工具链可运行，不替代 Windows WebView2 验证。
- 本机验证：macOS `cargo check --manifest-path backend/Cargo.toml -p chimera --lib` 通过；`cargo test --manifest-path backend/Cargo.toml -p chimera --lib window::tests -- --test-threads=1` 6/6；`cargo fmt --manifest-path backend/Cargo.toml --package chimera -- --check`、E2E TypeScript 检查、harness 单测 30/30、E2E 桌面构建、Prettier 与 `git diff --check` 通过。定向 WebDriver spec 在 macOS 按设计跳过，不能作为 Windows 行为验证。尝试 Windows GNU 交叉编译未完成：环境没有 `x86_64-w64-mingw32-gcc`；Windows COM 分支须由 PR Windows CI/E2E 编译和执行。
- 独立复核：无 P0–P2 阻塞项；确认 ref 映射、依赖版本、事件时序与测试清理符合目标。明确未覆盖内存 target 的实际效果和 COM HRESULT 失败恢复；PR Windows 构建和 E2E 是剩余证据。
- 远程结果与 CI 修复：PR #540 原 SHA `c1f8a498ab5781285295c25daa2b67f9e1e5aba0` 的 Windows Desktop E2E run `40632079689` 通过；CI run `40632079560` 的 Linux `Lint` job `122631703884` 在 Ubuntu 26.04 runner 安装已不存在的 `webkit2gtk-driver` 时失败，源码检查未开始。`.github/workflows/ci.yaml` 已改为根据 apt 候选包在 `webkit2gtk-driver` 与 `webkitgtk-webdriver` 间选择；本地 shell/格式检查通过。该修复与同步最新 master 后的新 SHA 远程 CI/E2E 仍待复验。
- 回滚：回退 WebView2 helper、窗口事件接线和定向 E2E；不涉及用户数据或系统设置迁移。
- 审批触发：不改变架构所有权、权限模型或持久数据，无需额外审批。

## DIFF-062：把系统代理与自启动交给共享 effects owner（覆盖 legacy 与 Agent）

- 目标：按当前只读 `ref/` 将 system-proxy/PAC/autostart actor 接入 Chimera 的共享 effects executor；typed API、legacy UI、Agent 确认后的网络动作与启动恢复都复用同一 owner，退出时由 actor 恢复启动前的代理状态。
- 非目标：不修改 TUN/Service 权限与生命周期、代理规则或配置格式；不移除 legacy UI、Chimera Client/Service 或公开的 `core::sysopt` 模块；不在日常开发主机实际开启/关闭系统代理、PAC 或登录项。
- 基线：父仓库分支从远程 `master` `5c491b2` 快进同步至 `25d1829a4816a99b402a1a655ca80654e526c2c6`；运行时 submodule 为 `ceee7f7df1d01b421fc75409ec667852b7cc7cda`。本批实际只读 `ref/` 为 `9f72efe292975b13b5d9018a9f5641ffbb071c27`，`git -C ref status --short` 为空；本批没有改动或更新 ref。
- 路径与符号映射：完整复制 `ref/backend/tauri/src/client/system_proxy/{actor.rs,adapters.rs,error.rs,mod.rs,ports.rs,tests.rs}` 到 Chimera 同路径；`SystemProxyActor`、`SystemProxyClient::{spawn,reconcile,status}`、`OsProxyPort`、`AutoLaunchPort`、`PacPort` 与 ref 对齐。`ref/backend/tauri/src/utils/blocking.rs::join` 复制到同路径，供阻塞系统 API 运行。`ApplicationEffectExecutor::system_proxy_desires` 与 `apply` 按 plan 顺序把 AutoLaunch/SystemProxy/ProxyGuard 合为一次 actor reconcile；本地保留既有 `ApplicationEffectAdapters`，继续承担 Locale/Logger/Widget/Tray 等 Chimera 适配。
- 生产接线：`setup.rs` 创建 actor 和真实平台 adapter；`ApplicationEffectExecutor` 与 `ChimeraClient` 注入同一个 `SystemProxyClient`。移除 `resolve_setup`、legacy Verge patch 与 Clash patch 中并行的 `Sysopt::init/update/reset_sysproxy` 业务调用；legacy commit 在成功持久化后向共享 effects actor 发布所变更 domain。Agent 普通开关通过 typed app patch 触发共享 effects，并等待 owner 状态后核验实际 OS 状态；Agent 的遗留代理修复通过同一 actor 做带预期端口与 loopback 检查的串行写入、读回验证和失败恢复，不再直接写 `Sysproxy`。启动完成时发布完整效果。effects 只从与当前 host 状态匹配的 `RuntimeApplyReceipt.ports.bindings.mixed_port` 取得活动端口；`ClashInfo::port` 是 controller 端口，不用于设置系统代理。Agent 与停止后 stale-proxy 校验优先使用当前 host 最近一次已确认的运行时 receipt。随机端口没有 receipt 时端口视为未知，Agent 不提供依赖该端口的开关或 stale 修复；不使用启动时可能被覆盖的 Verge 镜像猜测旧运行端口。固定端口没有 receipt 时仍可由固定配置确定。未确认活动 receipt 时 effects 发送 `None`，actor 按其 retryable unresolved-port 状态等待后续绑定。退出请求取消同一个 shutdown token，`TaskTracker` 等待代理 actor drain 与恢复完成。
- 保留差异与边界：Chimera 的 `EffectHealth` 错误码是 `&'static str`，所以将 ref 的 `EffectFailureCode` 映射为稳定 snake_case 字符串；其他本地 effects 错误模型和持久配置合同不变。`core::sysopt` 公开模块暂时保留以避免无必要的兼容性删除，但本仓生产代码不再从它应用自启动/系统代理。Widget runtime 和 ref 的 `TrayView` 缓存所有权不属于本批，PAC/system proxy host 写入仍仅由新 actor 执行。主 UI、legacy UI 和 agent 继续共享相同 Application API 与 effects actor；本批无 UI 操作变更。
- 接口变化：`ApplicationEffectExecutor::new` 增加必需的 `SystemProxyClient`；`ChimeraClient` 为经确认的 Agent 动作持有同一 actor client；`EffectsClient::reconcile_effect` 提供等待并取得最新 owner 状态的入口；system-proxy actor 增加带端点校验的 stale-proxy 修复和失败动作恢复命令。setup 为代理 owner 创建独立 PAC cache 路径；启动与 legacy commit 的 runtime binding 通知现在是异步只读活动 core 查询。未改变 IPC、持久化 wire format、TUN 或 Service API。
- 风险与回滚：L2（系统代理、PAC、登录项和退出恢复均可产生宿主副作用）。回滚需恢复原 `Sysopt` 接线、effects executor/adapters 契约、setup 注入和本条记录；无用户配置迁移。当前未执行真实 OS adapter 写入。
- 独立复核：fresh-context review 指出 stale Agent 修复与系统代理偏好并发时可能恢复过期 OS 状态、配置 CAS 更新漏写 legacy Verge 镜像、缺少生命周期命令级断言、Agent 回滚未串行化，以及混合端口变更后未重新核对旧端点；另一处旧 IPC raw mapping 也可绕过 typed guard。复核还发现 Android/iOS 下 Agent 引用被 cfg 排除的系统代理类型、随机 mixed-port 将 fallback start port 误当作活动端口，以及 stale 回滚在验证失败时会覆盖后来由宿主外部设置的新代理。当前 diff 让 typed/legacy 代理偏好、typed/raw/legacy mixed-port 更新和 Agent 设置、验证、回滚及 stale 修复共享 client 互斥；生命周期命令在 OS 写入前核对 receipt 端口；CAS 冲突时发布最新偏好并清除旧运行端口，不恢复过期 OS 配置；stale 回滚先重新读取 OS 状态，只在状态仍为本次写入值时恢复；移动平台不提供不支持的 Agent 系统代理提案与执行；普通配置与 Agent 修复共用 legacy mirror 持久化函数。第二轮复核又发现 effects 将 controller 端口误当 mixed port，Agent 对 Random 策略仅读 Verge fallback；现改用 host 状态核对过的 runtime receipt，并增加 controller/mixed 与随机端口回归断言。最终复核另发现 Random 模式重启后没有可信历史端口，现将未知端口 fail-closed，不再拿易变的 Verge 镜像认领旧系统代理。另新增 stale-proxy 生命周期、互斥串行、raw mixed-port 判定及宿主并发变更回滚覆盖。第三轮 fresh-context 复核无 P0-P2 阻塞，并指出启用期间核心停止后的 OS 与偏好恢复尚无动作级回归断言。
- 最新复核补充：Random 与 AllowFallback 策略在无 receipt 时都不能确定重启后的监听端口，当前只对 Fixed 策略回退到配置端口；启用代理动作要求活动 receipt，并在 effect 后和最终快照确认核心仍运行。旧 `patch_clash_core_config` IPC 经过 `feat::patch_clash` 调用持有 mixed-port 互斥的 client writer，raw mapping guard 有回归断言。
- 验收计划与限制：fake OS proxy/autostart/PAC ports 覆盖 owner 的 proxy/PAC/guard/revision/shutdown、stale endpoint CAS 与恢复；executor 与 lifecycle 分别验证效果分发、活动端口绑定以及 Agent stale-proxy 生命周期。Agent action tests 覆盖 proposal/planning、最终状态校验和错误映射，但不执行完整动作编排，也没有可确定重现“启用期间核心停止并回滚 OS 与偏好”的动作级测试；不会把这些测试描述为真实 owner rollback 测试。真实主机代理恢复/登录项、UI 操作和 Service/TUN 需隔离 runner 验证；本机不通过生产 adapter 修改系统设置。移动平台交叉编译会尝试使用 Android target 验证 cfg 分支。
- 当前本地证据：最新后端 SHA 执行 `cargo fmt --manifest-path backend/Cargo.toml --package chimera -- --check`、`pnpm exec prettier --check docs/ref-alignment-progress.md docs/testing/contracts/application-effects-executor.md LESSONS.md`、`git diff --check`、`cargo check --locked --manifest-path backend/Cargo.toml -p chimera --lib`、`cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib -- --test-threads=1`（659 passed、0 failed、1 ignored）、`cargo clippy --locked --manifest-path backend/Cargo.toml --all-targets --all-features` 和 `pnpm e2e:tauri:build` 均通过。E2E 测试 follow-up 让非视觉 fetch smoke 选择不等待动画；`pnpm --filter @chimera/tauri-e2e typecheck` 和 macOS WebKit 定向 smoke（5/5）通过。完整 `pnpm --filter @chimera/tauri-e2e test:smoke` 为 2 个 spec 通过、2 个失败：proxy-localization 的 Add Profile 按钮 30 秒不可点击；legacy connections 诊断显示文档 hidden、页面动画 `currentTime=0`。这两个未在可见隔离 runner 复现，所以不称完整 E2E 通过。未实测真实系统代理、PAC、自启动或 Service/TUN；Android target 下载停滞后中止，移动 cfg 交叉编译未验证。

## DIFF-063：同步上游 ref 至 2026-10-29 main

- 目标与范围：按用户明确要求更新只读参考子模块；只移动父仓库的 `ref/` gitlink，不把上游产品源码复制进 Chimera，也不改变用户数据。
- 基线：更新前 `ref` 为 `9f72efe292975b13b5d9018a9f5641ffbb071c27`，工作区及递归子模块均干净。
- 更新：fetch 上游 `main` 后快进 23 个提交至 `8e1084e4e7c50531a5d16ca9a6c9dd2bb2f0f8e8`。范围包含 Service/runtime `2.0.2`、Windows `enable-loopback` 与 Windows SDK bootstrapper、Tauri `2.8.4`、updater/resource prepare、sparkline 和页面动画修复及依赖更新。
- 固定的递归子模块：`backend/enable-loopback` `f1b9f8693a328ca0a6094951035b81fe302b0848`；`vendor/windows-rs` `d1574fd27371a9b08bc5508f70c43ccb4f84c543`；`backend/nyanpasu-runtime` `e73b9f5cc2aa91ac74b4c71d0acc5b48e0fd2b34`；`nyanpasu-utils` `2706c1b69a3b5e4f0367fb54b4a87e67769c19f8`。
- 验证：`ref/HEAD` 等于 `ref/origin/main` `8e1084e4e7c50531a5d16ca9a6c9dd2bb2f0f8e8`；`git -C ref status --short` 为空；递归子模块无前缀 `+`/`-` 标记。此次只改参考基线，不运行 Chimera 产品构建或测试。
- 影响与回滚：父仓库 `ref` gitlink 从 `9f72efe292975b13b5d9018a9f5641ffbb071c27` 更新至 `8e1084e4e7c50531a5d16ca9a6c9dd2bb2f0f8e8`。回滚只恢复旧 gitlink；每个功能迭代仍需按固定新基线单独映射、复制、审查和验证。

## DIFF-064：窗口级 KV 状态与有序存储事件（L2）

- 目标：按最新 ref 统一前后端 KV 状态所有权；每个窗口的同 key hooks 共享一个快照，读取、事件和写入确认按 revision 收敛；存储事件广播 lag 后触发 resync 并继续转发。
- 非目标：不改变 redb 持久化数据格式、用户配置键或系统网络；不删除 legacy UI、Chimera Client/Service 或旧 storage event 消费者；不引入 ref 的统一 HTTP RPC 子系统。
- 参考基线：本机 `ref` 与 upstream `main` 均为 `8e1084e4e7c50531a5d16ca9a6c9dd2bb2f0f8e8`，`ref` 工作区干净；核心 KV 实现来自上游 `eb81fda5`，当前 ref 仍保留本批对应路径与符号。
- 参考映射：`ref/backend/tauri/src/core/storage.rs::{StorageRevision,StorageItem,StorageChange,StorageNotification,forward_storage_changes,StorageResyncEvent,StorageValueChangedEvent}` → `backend/tauri/src/core/storage.rs`；ref `backend/tauri/src/ipc.rs::{get_storage_item,set_storage_item,remove_storage_item}` → Chimera 同名命令；ref `backend/tauri/src/setup.rs` 的 shutdown/TaskTracker storage forwarder → Chimera `utils/resolve.rs::resolve_setup`、`setup::AppShutdown`；`ref/frontend/query/src/provider/{kv-storage-state.ts,kv-storage-provider.tsx}`、`hooks/use-kv-storage.ts`、`provider/index.tsx` → `frontend/interface/src/provider/` 与同名 hook/RootProvider。保留 ref 的 revision merge、window-scoped query cache、单 key write queue、resync、直接订阅 query cache 以避免 deferred notification race。
- Chimera 必要适配：直接使用项目生成的 Tauri `commands` / `events`，不引入 ref 的 `useQueryApi` 与 HTTP SSE event bus（Chimera 没有该统一 RPC 架构）；`getStorageItem` 服务层保留旧 string/null 返回形状；继续发出旧 `storage_value_changed` 与 `storage_resync_required` 事件，供 legacy Jotai `ChimeraStorage` 及既有 storage listener 使用。所有窗口的 typed event 由共享 `KvStorageProvider` 处理；legacy UI 仍用现有交互，但共用同一 hook/store。当前 agent 没有 KV hook 消费者，不新增平行存储 API。移除旧事件的条件是 legacy Jotai/storage-listener 消费者全部迁到 typed shared hook。
- 测试适配：上游 React browser suite 不可直接运行于 Chimera 当前 Node/Tauri 测试包；对应逻辑以可执行 `kv-storage-state` 与 `kv-storage-provider` Node tests 覆盖，并用 runtime suite 的 `storage-event-resync.e2e.ts` 验证真实桌面中的 legacy UI 更新。广播 lag 的恢复由 Rust 单测验证；桌面 E2E 未强制填满生产广播队列。ref `frontend/query/src/provider/kv-storage-provider.tsx::connect` 在 `Promise.allSettled` 后即使监听注册失败仍会 `markReady`；本地错误注入测试复现了未注册事件监听器时初始读取提前开始。Chimera 对缺失监听器按指数退避重试（最长间隔 30 秒），两类监听都注册成功前不释放首次读取；首次读取在监听就绪后直接取权威快照，因此初次恢复不额外重复 resync 读取。
- Chimera-only 安全扩展与复查条件：ref `backend/tauri/src/ipc.rs::set_storage_item` 是无条件写入；PR #532 的独立审查发现旧客户端迁移失败时可能覆盖新版 KV 值。Chimera hook 因此在写入队列出队时复核迁移状态，并将已观察到的全局 `StorageRevision` 传给可选的条件写入；后端在 revision 锁内比较后提交，过期写入失败并重读权威状态。旧 `ChimeraStorage` 调用传 `null` 保留既有行为。每次 ref 更新 storage IPC 或 revision 语义时复核是否可上游吸收相同 CAS 契约；若上游实现等价保护，再移除 Chimera 专有接口。另，ref provider 对监听注册错误没有重试/失败门控；Chimera 保留窗口级 hook/provider 结构，但加入监听恢复与首次读取门控，属于针对已复现 ref 缺陷的有界退避扩展，待 ref 修复后复核收敛。
- 接口及风险：`get_storage_item` 返回 `{value, revision}`，写入/删除返回 revision，storage change event 增加 revision，并注册 `storage-resync-event`；`set_storage_item` 增加可选 expected revision，既有服务层传 `null` 保持兼容。风险 L2（共享持久化、IPC 与跨窗口状态）。回滚为恢复旧 command/event payload 与 hook/provider；无需迁移或删除用户数据。
- 验证：`cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib core::storage::tests` 9/9、`ipc::tests` 16/16、`typescript_bindings_are_fresh` 1/1 和生成器 `update_typescript_bindings` 1/1 通过；`pnpm --filter chimera-ui test:dashboard-widget-config` 23/23（含监听注册失败恢复用例）、`pnpm typecheck`、`pnpm --filter chimera-ui build`、`pnpm lint:frontend-boundaries`、改动文件 Oxlint/Prettier、目标 Rust 格式检查与 `git diff --check` 通过。macOS WebKit 定向 E2E 的既有成功记录早于条件写/CAS 收敛，不作为当前集成 SHA 的证明。PR #548 初始 SHA `84eb3f0d3e5b86169465a61462718844f78cc4ba` 的 Windows E2E run `40864250817` 中 55/56 specs 通过；唯一失败是 `dashboard-main-layout.e2e.ts` 的辅助函数将 revisioned storage snapshot 当字符串传给诊断和恢复路径，未发现产品存储行为失败。已修正读取解包与 fixture 写入参数；本机 E2E binary rebuild 通过。尝试运行目标 Dashboard spec 时，macOS embedded WebKit 在 before hook 路由动画处停于 `currentTime=0`，未进入测试断言（见既有 LESSON-20261023-003）；因此不作为修复结果。新 SHA 的 Windows E2E 复验待推送。CI run `40864250803` 的三平台 lint 已通过，三平台应用构建仍在执行。
- 回滚和收敛：回退上述 Rust storage、IPC/bindings、interface provider/hook、E2E 和测试入口；保留既有 redb 文件和键值。后续在 legacy 消费者迁移后复查并移除兼容 tuple events；若 Chimera 引入 remote/HTTP frontend，再评估 ref 的 event bus/SSE transport，不在本批复制无对应主机架构的模块。

## DIFF-065：托盘 effect 快照与刷新队列（部分对齐）

- 目标：按当前固定 ref 为托盘 effect 携带完整、不可变的 `TrayView`，让 effects、legacy 事件入口和托盘 renderer 经同一串行队列刷新；合并尚未处理的更新时保留最新 view，Full 刷新覆盖 Part 刷新。
- 非目标：本批不重做 Chimera 托盘菜单/代理子菜单、图标绘制、菜单模式、平台交互或 legacy UI presentation；不修改持久化配置、IPC wire format、Client/Service 生命周期或宿主代理/TUN 状态。
- 基线：父仓库 `master` `456392d5a71a8184bdb4abc674005b25ab0401c2`；只读 `ref/` 为 `2fadf212f161417dcdf2f37d81a305a37ff918a4`，工作树干净。没有同步或修改 ref。
- 路径与符号映射：`ref/backend/tauri/src/client/effects/plan.rs::{TrayMenuDesired,TrayPartDesired,TrayView}`、`client/effects/actor.rs` 的 Profile partial refresh、`client/effects/adapters.rs` 与 `core/tray/mod.rs` → Chimera 同路径。保留类型和字段名；`ApplicationEffect::Tray` 增加完整 `TrayView`，actor 将最新 view 与 refresh 一起交给 adapter。Chimera 的 `Handle::update_systray_part` 及 legacy event sink 也改走同一个 tray refresh queue。通过 typed Clash mode accessor 形成 snapshot，不再在 renderer 中读取 runtime YAML。
- 部分范围与待收敛差异：独立复审确认队列的 merge/latest-view/scheduling 没有 P0–P2 逻辑问题；本批仍为 partial。DIFF-066 已让 full menu 与 Part repaint 消费 `view.menu.core`，仅为 Clash Premium 显示并勾选 Script mode。`view.menu.selector_mode` 与 `menu_mode` 仍未接入，代理子菜单仍未按 ref 构建；Part renderer 的 `text` 和 `traffic` 也尚未显示。固定 ref 的 `tray_menu(app_handle, &view)` 还按 selector/menu mode 配置代理菜单，并在 repaint 中应用 tray text 等值。该菜单/渲染差异不得视为永久产品例外；后续托盘渲染迁移需消费完整 view，并在保留 Chimera 菜单 presentation、Client/Service 和 legacy 入口的前提下核对对应 menu actions。当前这批不宣称完整 ref tray 行为对齐。
- 接口与风险：Rust 内部 effects adapter 契约由 `refresh_tray(refresh)` 变为 `refresh_tray(refresh, view)`；`ApplicationEffect::Tray` 由单 refresh 变为 refresh + snapshot。L2（共享 app/clash 状态驱动的 UI 托盘效果）；不改变配置存储或外部 IPC。回滚恢复旧 effect 变体、adapter 参数及原托盘刷新入口。
- 验收及实际验证：托盘 queue 单测验证 Full 覆盖 Part、并发排队期间使用最新 snapshot，2/2；`client::effects::` effects/plan/actor/executor 定向测试 43/43；typed mode accessor 1/1；Rust fmt、LESSONS 与本记录 Prettier、`git diff --check` 通过。纯单测没有执行 Tauri main-thread scheduler/native menu；本机未做桌面 E2E。独立审查使用新上下文复核了实际 diff、ref SHA、队列和测试；其唯一 P2 结论为上述未消费菜单字段，按本条将其作为明确的后续收敛项，故本批标记为部分迁移。

## DIFF-066：Tray 提供 Clash Premium 脚本模式菜单项

- 目标：按 `ref` 的托盘菜单行为，仅在 Clash Premium 核心下显示 Script mode，并让菜单勾选状态和现有核心模式切换入口一致。
- 非目标：本批不迁移代理组子菜单、WebView 菜单模式、复制代理环境变量、托盘图标/tooltip、text/traffic 显示或队列/发布状态机；不改变配置、IPC、Service 合同或宿主代理/TUN。
- 基线与映射：只读 `ref/` HEAD `2c017d63bfec720017a9b57db629eeddd2c9cd3d`，工作区干净。`ref/backend/tauri/src/core/tray/mod.rs::tray_menu/repaint_part` 对应 `backend/tauri/src/core/tray/mod.rs::tray_menu/render_part_with_view`；`ClashCore::ClashPremium`、`Mode::Script` 和 `tray.script_mode` 采用上游语义。Chimera 的选择动作沿用现有 `feat::change_clash_mode`。
- 影响接口与风险：只增加托盘原生菜单项和五种 Rust locale 的显示文案；无 IPC/持久化接口变化。L2（通过现有入口修改运行模式）。
- 验收：定向 Rust 测试覆盖菜单构建与刷新共用的模式项定义和勾选模型；格式检查与推送后 CI 负责跨平台编译/回归验证。原生菜单点击和勾选显示未由纯单测证明；未执行原生菜单桌面 E2E 时明确记为未验证。
- 本地验证：`cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib core::tray::tests -- --test-threads=1` 4/4；`cargo fmt --manifest-path backend/Cargo.toml --package chimera -- --check`、本记录与 LESSONS 的 Prettier、五个 locale JSON 解析、`git diff --check` 通过。原生托盘 E2E 未运行；这批新 SHA 的远程 CI/E2E 尚待推送后核验。
- 回滚：恢复托盘菜单构建、模式勾选和事件映射，并移除五个 locale 中的 `tray.script_mode` 字段。

## DIFF-067：对齐 Runtime 的复用 PID 启动清理（L2）

- 目标：按 ref 修复 Core Manager 启动时回收已退出核心的 epoch 记录；操作系统复用了记录中的 PID 时，应删除旧记录和配置，不得误杀现在持有该 PID 的其他进程。
- 非目标：不改变核心切换策略、Service IPC、系统代理/TUN、配置格式或 UI；不迁入 Meow 专属 native store 和其他不属于本故障的 Runtime 功能。
- 基线：父仓 `master` `d03b085a51ce3050356026b0e1e0ed9d04c16b4e`；开始实现时 runtime 子模块为 `ceee7f7df1d01b421fc75409ec667852b7cc7cda`，提交前已快进同步至 `Chimera_Service/main` `70285e9ea7f3d6f3a66cfd6d33ed1d2c98811175`（本批仅更新文件不相交的依赖/自动化提交）；只读 `ref/` `2c017d63bfec720017a9b57db629eeddd2c9cd3d`，其 runtime 为 `e73b9f5cc2aa91ac74b4c71d0acc5b48e0fd2b34`、`nyanpasu-utils` gitlink 为 `2706c1b69a3b5e4f0367fb54b4a87e67769c19f8`。三处基线均已核对，`ref/` 干净且未修改。
- 路径与符号映射：`ref/backend/nyanpasu-runtime/crates/nyanpasu-utils/src/process/pid_file.rs::{ProcessIdentity,wait_for_process_identity,compare_identity,reap_record}` → `backend/chimera-runtime/crates/chimera-utils/src/process/pid_file.rs`；`ref/.../nyanpasu-core-manager/tests/manager_orchestration.rs::manager_sweeps_a_stale_record_whose_pid_was_reused` → `backend/chimera-runtime/crates/chimera-core-manager/src/manager/mod.rs::tests::startup_sweeps_a_record_whose_pid_was_reused`。保留 `chimera-utils` / `chimera-core-manager` 包名及 Client/Service 共用 manager 路径。
- 接口与风险：无 IPC、Tauri 命令、前端类型或用户配置变化；macOS 的临时 epoch PID 文件新写为 v3，解析器继续读取旧 v2。v2 记录只有秒级身份：若当前 PID 的完整秒+微秒标记与该秒值对应，复用情况无法证明，清理会失败关闭且不发信号；精度更高的 v3 才能安全区分同秒复用。风险 L2（进程身份、崩溃恢复和 Runtime 临时记录）；旧版本 manager 不认识 v3，因此回滚到旧二进制时，若残留 v3 PID 记录，启动清理会失败关闭。此格式只用于短期 Runtime 记录，不迁移用户配置。回滚当前改动即可恢复原行为。
- 验收与实际验证：`cargo +nightly test --manifest-path backend/chimera-runtime/Cargo.toml -p chimera-utils -p chimera-core-manager --all-features` 在本机 macOS arm64 通过：core manager 117 passed、1 ignored；utils 31 passed；doc test 1 passed。覆盖同秒不同微秒的 PID 复用、v2 歧义 fail-closed 且不发信号、root-owned PID 的不可读可执行文件、非法 v3 微秒字段、v3 往返及 manager 启动清扫。`cargo +nightly clippy --manifest-path backend/chimera-runtime/Cargo.toml -p chimera-utils -p chimera-core-manager --all-targets --all-features`、workspace `cargo fmt --all -- --check`、Prettier 与两层 `git diff --check` 通过；Clippy 有该子模块既有警告。曾尝试对 `x86_64-unknown-linux-gnu` 与 `x86_64-pc-windows-gnu` 执行 `cargo check --tests`，但两次都在依赖 `aws-lc-sys` 的 C 构建阶段因缺少对应 GCC 交叉编译器而停止，未到项目 Rust 代码编译；因此 Linux/Windows 目标仍未验证。主仓 `pnpm test:backend` 排除此子模块，Service 工作流只运行 Clippy 而不执行测试；本批未改 CI 工作流。
- 入口、恢复与剩余验证：主 UI、legacy UI 与 Agent 均继续通过既有共享 Core Manager，无入口分叉。单元/集成断言在 Service runtime workspace 中可由上述 `cargo test` 命令执行；当前 CI 只编译测试目标，不运行它们。Windows/Linux 需在相应 runner 执行测试；CI 工作流的测试门禁属于另一项 L3 变更，本批未改。

## DIFF-068：Windows 核心优雅退出（ref 对齐并修复上游缺陷，Windows 实机验证待运行）

- 目标：先逐层复现 ref 的 Windows 控制台 helper 与 Core Manager 接线，再修复其不可达/广播缺陷；优雅退出必须针对一个核心进程组，失败时仍由 Job Object 及时回收整棵进程树。
- 非目标：不改变 Unix SIGTERM 行为、Service IPC/安装协议、权限模型、核心选择、PID 所有权、用户配置、系统代理/TUN 或 UI；本批不修改 CI 工作流，也不触碰 `ref/`。
- 基线：本地 `master` `304afdc0c97f20dcab696008eb30c96ccd7b0250`；只读 `ref/` `1b5d73c53a7141d216a8a0a7751be38a295be059`，其 runtime 固定在 `e73b9f5cc2aa91ac74b4c71d0acc5b48e0fd2b34`，状态干净；Chimera runtime submodule 起始为 `3f57af32040402b37baf9826736a19a4086c2d27`。未切换 `ref/` 或 runtime 基线。
- 路径与符号映射：`ref/backend/nyanpasu-runtime/crates/nyanpasu-utils/src/process/{windows_shutdown.rs,command.rs,engine.rs,mod.rs}` 和 `tests/process_kill.rs` → `backend/chimera-runtime/crates/chimera-utils/src/process/` 与 Windows 进程集成测试；`nyanpasu-core-manager::{spec.rs,runtime/process.rs,instance.rs,manager/mod.rs}` → `chimera-core-manager` 同名层；`ref/backend/tauri/src/core/actor_v2/local_host.rs` 与 `setup.rs` → Chimera local host/setup；`ref/backend/nyanpasu_service/src/main.rs` 和 service manager bridge → Chimera Service 对应入口。保留 helper entry、`Shutdown`、`Command` builder、engine graceful-kill 分层与 ManagerOptions 参数传递，仅把上游私有标识映射为 `--chimera-process-shutdown` 及 Chimera 包名。
- 上游实现基线：ref 的 `windows_shutdown.rs` 用单次 helper `AttachConsole(pid)` 后发送 Ctrl+C；engine 在 graceful-kill 时启动 helper，在宽限期内等待，失败则强制终止进程树。实现已按同一调用链接入本地 Core Manager、本地 Tauri host 和 Chimera Service；Service 与桌面进程均在正常启动前识别 helper 参数。
- 已确认的 ref 缺陷及修正：ref 对核心使用 `CREATE_NO_WINDOW`，使其没有可附加的控制台，`AttachConsole(pid)` 因而失败；ref 的 `CTRL_C_EVENT` 配合 process group id 0 还会广播给该控制台中的其他进程。当前实现让 host 按需持有隐藏控制台，核心用独立控制台进程组启动；helper 发送进程组定向的 `CTRL_BREAK_EVENT`。这项行为修正有意偏离 ref 的信号种类和 group id，以保留 helper 接线、单次请求、等待宽限期和 Job Object 回退语义，同时不复制已确认的上游缺陷。
- helper 生命周期修正：pump 直接持有并轮询 helper 子进程；核心结束或 Job Object 回退时会取消并回收 helper。`Shutdown` 同时持有目标进程句柄，直到 helper 完成/取消并回收之后才释放，防止旧 PID 在 helper 清理窗口内被系统复用；若无法取得该句柄，则关闭优雅信号路径并保留 Job Object 回退。
- 依赖/API 差异：固定 ref 的 ProcessKit 为 2.2.5，没有创建独立 Windows graceful-control 进程组的 builder；本实现使用 ProcessKit 3.3.4 的 `windows_graceful_ctrl_break()`，并在单一 `chimera-utils` adapter 中将旧 `output_events()` 接口迁移到新版 `events()`。版本变更是提供独立组标志的窄依赖升级，不替换 Chimera 的 Core Manager 或 containment 模型；现有 Job Object 仍负责强制清理。若后续在兼容的 ProcessKit 2.x 中获得同等受支持的 API，应评估是否收敛该 major 版本差异。
- 风险与恢复：L2（共享进程生命周期、Service 核心管理、依赖 pin）。回滚时一并撤销 runtime submodule gitlink、根仓库桌面/Service 接线、测试与本条记录；Job Object 原强制终止路径保持为失败回退。没有改变 Service 协议、持久化数据或宿主网络。
- 测试契约：`docs/testing/contracts/windows-core-graceful-shutdown.md` 定义初始状态、目标组与同控制台兄弟组的可观察结果、helper 失败注入、清理与未覆盖边界。用例使用真实 Windows Job Object 和控制台进程组，不把 fixture/unit 测试当成系统行为证据；它不覆盖生产隐藏控制台分配路径，也尚未验证实际 core 对 Ctrl+Break 的处理。runtime 子模块的测试尚未接入现有根仓 CI 门禁；本批不改 L3 workflow。
- 已执行：ProcessKit 3.3.4 API 适配后，`cargo +nightly test --manifest-path backend/chimera-runtime/Cargo.toml -p chimera-utils --features process` 在 macOS arm64 通过（30 tests、1 doctest）；`cargo +nightly test --manifest-path backend/chimera-runtime/Cargo.toml -p chimera-core-manager --all-features` 通过（117 passed、1 ignored）；Service `--all-targets` 检查通过；`cargo +nightly check --manifest-path backend/Cargo.toml -p chimera --all-targets` 通过。`chimera-utils` 含测试目标的 `x86_64-pc-windows-gnu` 交叉检查通过，确认 Windows helper 与两个集成测试可编译；macOS 上 Windows 测试被 `cfg(windows)` 排除，因此不能据此声称控制台事件已运行通过。Service 的 Windows 全量交叉检查在依赖 `aws-lc-sys` 的 C 编译阶段因本机缺少 `x86_64-w64-mingw32-gcc` 停止，未到 Service Rust 代码；实际 Windows 集成运行仍待有 Windows runner 的环境。一开始的 3.3.4 编译尝试暴露 API 适配不完整，已修正为 ProcessKit 3 的 `ProcessEvents`/`ErrorKind` API，最终本地测试与 utility Windows 交叉检查均通过。
- 收敛与 CI 后续：Windows runner 上确认两个真实子进程中仅目标收到 Ctrl+Break、缺失 helper 时快速走 Job Object 回退后，记录最终命令和结果。`pnpm test:backend` 不覆盖该 runtime 子模块，后续若要把该测试纳入 CI，需另行评估 L3 workflow 变更；本批不声称 CI 已执行这些断言。

## DIFF-069：启动顺序第一阶段——路径所有权（部分对齐）

**Reference:** pinned `ref` at `518bf49843e1f735380e11da1ebe2a85eb8ce8c6` (read-only).
**Chimera base:** `19b1281de2fbc58186056699068931934decbe6b`.
**Scope and risk:** L2, backend initialization sequencing and filesystem roots.
**Goal:** prepare the Chimera launch path to use the *same* resolved config/data roots for pre-Tauri default configuration, logger creation and Tauri composition. This is **partial alignment**, not the complete upstream startup lifecycle.

### Source mapping

| `ref` implementation | Chimera implementation | Stage 1 outcome |
| --- | --- | --- |
| `backend/tauri/src/lib.rs::run`: `host_paths::discover` before other initialization | `backend/tauri/src/lib.rs::run`: `PathResolver::from_env` | Resolve once before `init_config`, pass to the Tauri setup closure |
| `backend/nyanpasu-paths/src/lib.rs::PathResolver` | `backend/tauri/src/utils/path.rs::PathResolver` | Reuse existing Chimera resolver; do **not** change its persisted paths or branding |
| `backend/tauri/src/utils/init/mod.rs::init_config(paths)` | `backend/tauri/src/utils/init/mod.rs::init_config(paths)` | Create legacy defaults only beneath the injected config root, retaining non-overwrite and best-effort semantics |
| `backend/tauri/src/utils/init/logging.rs::init(..., paths)` | `backend/tauri/src/utils/init/logging.rs::init(paths)` | Use the injected data/log root for both initial appender and future appender rotation |
| `backend/tauri/src/setup.rs::setup(..., paths, owners)` | `backend/tauri/src/setup.rs::setup(app, paths)` | Consume already-resolved paths; no independent path discovery in setup |
| `backend/tauri/src/lib.rs`: logging failure exits before building Tauri | `backend/tauri/src/lib.rs::run` | Return the startup error before continuing when path resolution or logger initialization fails |

Only the product naming, existing config formats and Chimera Client wiring remain adapted here. No public IPC or persisted schema changes.

### Behavior and non-goals

1. The production path is now `run → PathResolver::from_env → init_config(&paths) → logging::init(&paths) → initialize_default_configs(&paths) → Tauri setup(&paths)`.
2. The initial defaults keep the existing `# Clash Chimera` header, existing YAML locations and corruption-evidence protections. Existing config files and symlinks must not be overwritten.
3. **Not changed:** native single-instance implementation, command parsing, pre-Tauri subprocess migration, root directory creation semantics, process exit-code policy, crash reporter, task ownership, core/service start ordering, traffic storage, silent startup, main/legacy UI and Agent. Do not mistake the partial integration for upstream parity.
4. **Known remaining difference:** Chimera's `PathResolver::from_env` still uses `utils::dirs`, which creates base directories during discovery; `ref` discovers without side effects, checks its single-instance lock, then creates roots. The current Chimera single-instance Tauri plugin remains unchanged. Aligning this boundary needs a separate stage and cold-start/double-launch platform testing.
5. **Known remaining difference:** Chimera `main` still calls `run().unwrap()` instead of the ref's typed `ExitCode` and one-shot failure presentation, and `resolve_setup` still runs service/core/storage/resource initialization. Those changes require dedicated failure, restart and host-effect tests; avoid broad changes in this slice.

### Acceptance / tests

- Existing isolated defaults tests and a new unit test ensure all legacy YAML defaults are written beneath the injected config root and no unrelated data root is created by default initialization.
- An unreadable config node, dangling symlink, and existing user config must retain the pre-existing recovery behavior.
- Local macOS Rust verification on this slice: full `cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib -- --test-threads=2` passed **778/778**, with 1 pre-existing ignored generated-bindings writer test. Targeted startup file safety tests **5/5**, injected log appender **1/1**, path tests **2/2** and startup failure tests **6/6** passed. Rust formatting, `cargo clippy --locked --manifest-path backend/Cargo.toml -p chimera --all-targets --all-features` (success with pre-existing warnings), tracked-document Prettier and `git diff --check` passed. After tightening the log-content assertion, its focused test passed again. An initial full-suite run failed because the isolated worktree lacked a `pnpm` executable for an existing Specta freshness test; after supplying a temporary Corepack-backed `pnpm` wrapper, the entire suite passed without changing that test or generated bindings.
- The full Windows E2E and macOS/Linux build matrix must be recorded against the new commit **before claiming a complete platform acceptance**. Pure Rust tests do not prove native single-instance or real service/TUN startup.
- Scope rollback: revert this stage's commit. No changes to `ref` or submodule gitlinks.

### Future stages (separate changes)

- **Stage 1b:** ref-style pure path discovery and single-instance placement; avoid directory writes from a losing instance. Needs explicit Windows/macOS/Linux double-launch and portable-mode tests. Do not copy upstream directory-file replacement without assessing user-data impact.
- **Stage 2:** migration order, safe preflight and structured failure reporting, preserving existing snapshots and recovery.
- **Stage 3:** setup lifetime owners and cancellation, explicit ownership transfer on partial failure.
- **Stage 4:** core/service startup reconciliation and runtime state, preserving Chimera Client and agent confirmation boundaries.
- **Stage 5:** main/legacy window readiness, deep links, tray and shutdown ordering.

## DIFF-070：启动顺序第 1b 阶段——纯路径发现与显式目录准备（部分对齐）

- **目标与范围**：沿固定 `ref` 的 `PathResolver::discover` → `create_base_dirs` 边界，把 Tauri 入口从「解析路径时立即创建目录」调整为「一次发现 config/data 两个根目录，再显式准备目录」。这只是第一阶段的路径处理部分，**不是单实例启动顺序完全对齐**。风险等级 L2（启动路径、文件系统副作用）。
- **基线**：Chimera 父提交 `9d31a8d0f523326fd62d518dfccebda62db7854e`（第 1 阶段）；只读 `ref` gitlink `518bf49843e1f735380e11da1ebe2a85eb8ce8c6`，工作区干净、未更新。
- **映射**：`ref/backend/nyanpasu-paths/src/lib.rs::PathResolver::{discover,create_base_dirs}` → `backend/tauri/src/utils/path.rs::PathResolver::{from_env,create_base_dirs}`；`ref/backend/tauri/src/lib.rs::run` 的目录准备步骤 → Chimera `backend/tauri/src/lib.rs::run`。Chimera `backend/tauri/src/utils/dirs.rs` 将 `discover_config_dir` / `discover_data_dir` 分离为不写磁盘的读取路径，既有 `app_config_dir` / `app_data_dir` 仍保留旧的 create-on-access 兼容契约，不更改 IPC 和持久化路径。
- **启动行为**：在已经注入同一份 `PathResolver` 的前提下，
  1. `from_env` 只发现两个根目录，不创建 config/data/logs/cache/profiles；E2E 环境变量路径同样只发现，不立即创建。
  2. `run` 显式调用 `create_base_dirs()`，配置和数据根目录均必须准备成功；profiles、logs 和 cache 为 best-effort。日志初始化与配置默认值创建仍使用同一份 resolver，属于之前阶段的改动。
  3. 可选目录如果已被普通文件挡住，只记录错误、保留用户文件，不主动删除；这是对 ref 在该异常路径会先删除文件行为的一处保护性偏离。必要根目录也改用不会删除既有普通文件的 `std::fs::create_dir_all`：如果用户文件占用了根目录路径，启动明确报错并保留文件，而不是沿用旧 helper 的替换行为。这是针对数据损失风险的明确安全偏离；旧 `dirs::app_config_dir/app_data_dir` 兼容访问器本批未改。
- **明确保留的差异与后续**：`ref` 在创建根目录前由独立 `single_instance::SingleInstance` 检查，Chimera 目前使用 Tauri `tauri_plugin_single_instance` 在 Builder 中检测并**转发二次启动到已有主/Legacy 窗口**。直接复制 ref 的前置锁会让二次启动无法正常唤起窗口；本阶段**不替换该插件、不改第二实例退出或深链接机制**，因此第二实例可能仍会在插件阻止它之前准备目录。若后续要对齐单实例时序，须先设计可验证的转发协议，覆盖 Windows/macOS/Linux、portable 和多用户边界，不能仅为了减少启动目录写入而退化既有体验。
- **验证**：定向 `cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib utils::path::tests -- --test-threads=1` 包含显式且幂等的目录准备、必要目录失败时不创建另一根目录、必要根目录被普通文件占用时保护原始字节、可选叶目录文件保护以及原 config/data 路径布局。安全保护补强后，完整 `cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib -- --test-threads=2` **782/782 通过、1 项既有忽略**，其中定向路径测试 **6/6 通过**；`cargo clippy --locked --manifest-path backend/Cargo.toml -p chimera --all-targets --all-features` 通过（保留既有 warnings）；Rustfmt、Prettier 与 `git diff --check` 均通过。新 SHA 的远程 CI 与 Windows E2E 仍待运行；纯 Rust 单测不能证明原生单实例和真实桌面启动。
- **回滚**：回退本批 `lib.rs`、`utils/dirs.rs`、`utils/path.rs` 与本条记录即可，保留第 1 阶段已验证的 `PathResolver` 注入；无 Schema、Service/TUN、用户网络状态或子模块变更。

## DIFF-072：Path Alignment——open utility 与 init 测试布局

- **当前基线**：Chimera `origin/master` `3813878613292ef35b27f72c89c788eceb766c67`；只读 ref `59724e85c34abb6995c22d101c876cc06dc16576`。
- **旧分支处置**：`codex/ref-path-alignment-utils-20261104` 相对当前 master 落后 109、领先 5。两个早期 PathResolver/discover/logging 提交的功能已存在于当前 master；两个版本号提交不属于本功能。未重放旧历史，也未覆盖当前已演进的 `lib.rs` / `setup.rs`。
- **本次映射**：`ref/backend/tauri/src/utils/open.rs::{that,with}` → `backend/tauri/src/utils/open.rs::{that,with}`；`ref/backend/tauri/src/utils/init/mod.rs::tests` 与 `ref/backend/tauri/src/utils/init/tests.rs` → Chimera 同路径的外部测试模块。open helper 只对齐函数顺序和 `allow(dead_code)` 注解；启动配置五项测试原样移出内联模块，断言和测试边界不变。
- **保留差异**：不迁移当前 ref 的独立 migrations、startup failure UI、Core/Service owner 架构；不改变现有 PathResolver、配置路径/格式、IPC、Core/Service、TUN、DNS、系统代理或用户数据。
- **审计结论**：当前 ref 的 `open.rs` 与测试模块布局已逐文件核对；旧分支 2026-11-04 审计文件使用过时的 ref `518bf498`，且 `/docs/*` 按仓库策略忽略本地审计报告，因此不纳入版本控制。当前迁移范围和结论以本条记录为准。
- **验证**：定向 `cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib utils::init::tests -- --test-threads=1` **5/5 通过**（通过 dev shell 的 GLIBC 2.44 loader 直接运行）；`cargo fmt --manifest-path backend/Cargo.toml --package chimera -- --check`、`cargo clippy --locked --manifest-path backend/Cargo.toml -p chimera --all-targets --all-features`、Prettier 和 `git diff --check` 通过，Clippy 保留仓库/子模块既有 warnings。完整 lib 测试尝试受跨进程子测试加载系统 nix-ld / GLIBC 2.42 与 dev shell GTK/WebKit 所需 GLIBC 2.43/2.44 不一致阻塞，未记为通过；未修改系统环境。未执行真实桌面、发行产物、Windows/macOS 运行验证。

## DIFF-073：Runtime ManageableField 与字段所有权基础（部分对齐）

**2026-11-11 主线整合说明：** DIFF-073/074 源于分支 `codex/ref-tun-auto-route-ownership-20261111` 的代码移植，原分支使用的 `ref@ea0e590b` 是其源代码参考，不代表本次主仓参考子模块已更新。整合时保留当前 `master` 的 `ref@59724e85` 及 `ref/backend/Cargo.lock` 原有工作区修改；原分支列出的测试数字属于来源分支的历史证据，不能直接视为新整合提交的验收。此轮仅整合功能代码，真实 TUN/桌面测试后续独立安排。

- **目标与范围**：按当前固定 `ref` 建立可声明的 Runtime 字段所有权模型，并将现有 `ClashGuardOverrides` 中 `unified-delay`、`tcp-concurrent` 接入。风险 L2：两个应用配置字段的持久化/IPC 类型由布尔值改为 tagged `ManageableField<bool>`；旧布尔配置保留读取兼容。
- **基线**：父仓库 `master` `3813878613292ef35b27f72c89c788eceb766c67`，与 `origin/master` 一致。只读参考 `ref` 为 `ea0e590b491cd661e51409b4e20b3f070aeb1178`。开始本批前工作区已有 MMDB、Runtime config diff UI、IPC 和 `ref` gitlink 等未提交修改；全部保留并在其上叠加本批变更，没有 reset、stash、clean、切换分支或新建 worktree。
- **源码映射**：`ref/backend/nyanpasu-config/src/clash/config/overrides/manageable.rs` → `backend/chimera-config/src/clash/config/overrides/manageable.rs`；`ref/.../runtime/post_process/{ownership.rs,apply.rs,mod.rs}` → `backend/chimera-config/src/runtime/post_process/`；`ref/backend/nyanpasu-macro/src/owned_fields.rs` → `backend/chimera-macro/src/owned_fields.rs`；`ref/.../clash/config/overrides/mod.rs::ClashGuardOverrides` → Chimera 同名类型；`ref/.../runtime/executor/{mod.rs,plan.rs}` 的过滤后继承恢复与 guard 应用 → Chimera `runtime/executor/mod.rs::execute` 和 `builtin.rs::apply_guard_with_source`。宏生成路径适配为 `::chimera_config`，字段名遵循 Serde `rename` / `rename_all` 与 `cfg`。
- **实现语义**：增加 `ManageableField<T> { Managed(T), Unmanaged }`、`Ownership::{Forced,Removed,Default,Inherited,Table}`、`OwnedField` / `OwnedFields` 与 `OwnedFields` derive；增加 `apply_settings`、`apply_defaults`、`keep_inherited`，均返回可回放的 JSON Patch。执行链在第一轮白名单过滤前保留配置快照，过滤后先恢复 `Inherited` 字段，再应用 `Managed` guard 值。Managed 仍强制覆盖 Profile；Unmanaged 保留 Profile/Transform 的原值；原配置没有该字段时不合成。
- **兼容性适配**：参考类型写出 tagged `kind/value` 结构；为保护现有 Chimera 用户配置，自定义 Deserialize 同时接受旧布尔值并解释为 `Managed(value)`。因此旧配置可升级读取，但旧版本二进制能否读取新 tagged 序列化格式不作兼容承诺，降级验证未执行。Specta 绑定已重新生成，`ClashGuardOverrides` 两个字段现在是 `ManageableField<boolean>`。
- **前端入口与明确未迁移项**：Clash 设置页的 `ManagedGuardFieldSelector` 已接入本批，使用 `useClashSetting('overrides')` 通过 `ClashConfigPatch` 的 nested sparse patch 只更新所选字段，避免整张 overrides 表替换造成的并发覆盖；为两个字段提供启用、禁用、跟随配置文件三态。英文、日文、俄文、简体中文、繁体中文文案均已补齐，Paraglide 生成和 Chimera TypeScript 检查通过。尚未执行 UI interaction E2E、真实 Core 应用或冷启动持久化验证。`TunOverrides` / TUN auto-route 所有权、独立 Runtime plan、端口 Remove 策略、独立 KeepInherited/ManagedSettings/Defaults snapshot 节点均留待后续批次；当前 snapshot 仍使用合并后的 `GuardOverrides` 节点。未启动真实 Core、未改变 TUN/DNS/系统代理、未验证 Windows/macOS/Linux 桌面行为。不得将本批描述为完整 Runtime ownership parity。
- **测试契约**：`docs/testing/contracts/runtime-field-ownership.md` 覆盖旧/新序列化、Serde 字段映射、嵌套 Table、Forced/Removed/Default/Inherited、JSON Patch 回放、过滤后恢复、缺失值不合成和 managed 覆盖。
- **实际验证**：默认 features 下 `cargo test --locked --manifest-path backend/Cargo.toml -p chimera-config --lib` **201/201 通过**；关闭 `default-meta`、保留 `nesting` 的 `cargo test --locked --manifest-path backend/Cargo.toml -p chimera-config --no-default-features --features nesting --lib` **192/192 通过**。Tauri `RequestedRuntimeFields` 对 nested override patch 的识别测试 **2/2 通过**。`cargo clippy --locked --manifest-path backend/Cargo.toml -p chimera-config --all-targets --all-features` 通过，仅有既有测试 fixture 的两个 dead-code warning。Nix dev shell 中 Tauri RuntimeBuilder 定向测试 **8/8**、Specta binding freshness **1/1** 通过；`pnpm typecheck`、`pnpm lint:frontend-boundaries`、`pnpm lint:oxlint`、`pnpm --filter chimera-ui build` 通过。Vite 报告既有 configLoader/native 与大 chunk warning；改动文件 Prettier、Rustfmt 与 `git diff --check` 通过。未启动真实 Core 或桌面交互 E2E。
- **回滚**：回退本批新增的 `manageable.rs`、`runtime/post_process/`、`owned_fields.rs`，并恢复 `ClashGuardOverrides` 两个字段、Executor 过滤/guard 接线、宏依赖与生成 bindings；回退前需确认已保存的 tagged 配置是否仍需由当前版本读取。保留本批开始前的 MMDB、UI/IPC 和 `ref` gitlink 改动。

## DIFF-074：TUN Auto-route Ownership（部分对齐）

- **目标与范围**：将 ref 的 `TunOverrides.auto_route` 接入 Chimera 持久化配置与 Runtime Executor。风险 L2：影响生成给 Mihomo/ClashRs 的 TUN YAML 和旧配置读取，但不创建 TUN 设备、不改主机路由/DNS/系统代理。当前不新增 UI 控件；ref 也没有对应的 UI selector。
- **基线**：父仓库 `origin/master` `3813878613292ef35b27f72c89c788eceb766c67`；固定只读 ref `ea0e590b491cd661e51409b4e20b3f070aeb1178`。分支 `codex/ref-tun-auto-route-ownership-20261111`，此前 DIFF-071 的 Runtime Ownership WIP 作为本批基线保留；原始 master 工作区没有修改。
- **源码映射**：`ref/backend/nyanpasu-config/src/clash/config/overrides/tun.rs::TunOverrides` → `backend/chimera-config/src/clash/config/overrides/tun.rs`；ref `ClashConfig.tun` → Chimera `ClashConfig.tun`；ref `TunParams.overrides` → Chimera `runtime/executor/mod.rs::TunParams.overrides` 与 Tauri `RuntimeBuilder` 注入；ref `runtime/executor/plan.rs::{tun_ownership,tun_entries}` → Chimera `builtin::apply_tun_overrides_with_source`，在首轮字段过滤后恢复 inherited 字段并应用 managed 设置。
- **运行语义**：默认 `auto-route` 为 `Managed(true)`；`Managed(false)` 强制 false；`Unmanaged` 从过滤前的 Profile/transform 快照恢复原值，即使整个 `tun` 表被首轮字段过滤移除；原 Profile 没有 `auto-route` 时不合成。`tun.enable` 在同一 ownership pass 中强制 true/false；关闭 TUN 时不创建缺失的 `tun` 表，也不强制 `auto-route`。`ChimeraClient` 的 legacy TUN 使用 `route-all` / `dns-hijack` / `so-mark`，因此忽略该 Mihomo/ClashRs 专用 `auto-route` ownership，保留产品行为。
- **兼容性**：旧 `ClashConfig` 缺少 `tun` section 时使用默认值；旧裸布尔 `tun.auto-route` 读取为 `Managed(value)`，新写入采用 tagged `ManageableField`。Specta bindings 已重新生成；未声称旧版本二进制可读取新 tagged 格式。
- **架构差异**：Chimera 仍使用现有 Executor pipeline；TUN ownership 与 guard ownership 共享当前 `GuardOverrides` snapshot 节点，`tun.enable` / `stack` / DNS defaults 的最终化仍留在现有 Finalizing 节点。没有迁移 ref 独立的 Runtime plan 与独立 KeepInherited/ManagedSettings/Defaults snapshot nodes。
- **验证**：`chimera-config` 默认 features **212/212**；`--no-default-features --features nesting` **203/203**；Clippy all-targets/all-features 通过（保留两个既有测试 fixture dead-code warnings）；Tauri RuntimeBuilder tests **9/9**，request-impact tests **5/5**，typed Clash patch reconcile test **1/1**，Specta binding freshness **1/1**；`pnpm typecheck`、`pnpm lint:frontend-boundaries`、`pnpm lint:oxlint`、`pnpm --filter chimera-ui build`、Rustfmt、Prettier 与 `git diff --check` 通过。Vite 保留既有 configLoader/native 和 large-chunk warnings。没有执行真实 Core/TUN 设备、主机路由、DNS 或系统代理验证。
- **状态**：来源功能已移植到当前主仓，参考 gitlink 保留为 `59724e85`；真实 Core/TUN 设备及主机网络验证仍待后续执行。

## 2026-11-14：主窗口显示前恢复 macOS 应用激活

- **目标 / 验收**：对齐启动窗口最终显示路径，macOS 上在显示 Chimera 主界面或保留的 Legacy 界面之前将应用激活策略设为 `Regular` 并激活应用，避免 Tauri 显示窗口但应用仍处于不可前台激活状态。非 macOS 行为保持不变。
- **非目标**：不改变 `enable_silent_start`、主/Legacy UI 路由、前端就绪握手、10 秒窗口就绪兜底、Core/Service 启动顺序或用户配置；本批没有复现原始“不显示”问题，因此不将该差异描述为已确认根因。
- **基线 / 风险 / 接口**：父仓库 `origin/master` 与工作分支起点 `ccaeff0ed44c6556d7755f01dcf135ea4fe70d3a`；固定 ref gitlink 与只读工作区 `59724e85c34abb6995c22d101c876cc06dc16576`。风险 L2（macOS 原生应用激活策略）；没有 IPC、持久化格式或公开接口变化。仅在 `WindowManager::apply` 已运行于主线程且目标为 Main/Legacy 时调用 AppKit。
- **源码映射**：`ref/backend/tauri/src/utils/dock.rs::macos::show_dock_icon` 与 `ref/backend/tauri/src/window/manager.rs::apply` 的 show 分支 → `backend/tauri/src/utils/dock.rs` 与 `backend/tauri/src/window/manager.rs::apply`。Chimera 同时支持 Main/Legacy 两个主界面，所以两者均恢复普通激活策略；没有移植 ref 在关闭主窗口时切换到 Accessory 的行为，避免影响 Chimera 同时存在的 Legacy/editor 窗口及当前一直保留 Dock 图标的产品行为。
- **验证**：macOS `cargo check --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib` 通过；`cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib window::table::tests -- --test-threads=1` **12/12 通过**；Rustfmt、进度文档 Prettier 与 `git diff --check` 通过。首次编译因隔离 worktree 缺少被 `.gitignore` 排除的 sidecar/resources 失败；为避免下载或复制资源，本次只在隔离 worktree 建立指向已有本地资源目录的 ignored symlink 后重跑成功。编译/测试包含项目现有 warnings。没有运行桌面冷启动、WDIO 或像素可见性验证，因此不能证明该 AppKit 差异就是原始问题的根因或已覆盖所有启动故障。
- **回滚**：移除 `utils/dock.rs`、`utils/mod.rs` 模块声明及 `WindowManager::apply` 中的 macOS 调用；不回退其它启动、Client/Service 或窗口状态改动。代码随 `codex/ref-startup-window-20261114` 推送；`ref` gitlink 保持不变。

## 2026-11-14：开发启动不再被 bindings 格式化阻断

- **目标 / 验收**：debug Tauri setup 生成 IPC bindings 时使用由 `CARGO_MANIFEST_DIR` 定位的绝对目标路径和仓库本地 Prettier；Prettier 不存在或执行失败时不再让 setup 失败，避免非必要格式化阻止初始化窗口创建。Bindings 导出和结构规范化仍是必需步骤，失败仍会透传。
- **非目标**：不更改 release / E2E 分支、IPC schema、生成文件内容、formatter 配置、用户数据、主/Legacy 窗口路由或 Core/Service 启动次序；不把 Specta 导出失败降级为成功。
- **基线 / 风险 / 接口**：父仓库远端工作分支提交 `a283ffceefb619e9db71a3a4eaad787ef460b3cc`；固定 ref gitlink / 工作区 `59724e85c34abb6995c22d101c876cc06dc16576`，未改动。风险 L1（仅 debug、非 E2E 开发启动）；没有 IPC、持久化或公开接口变化。
- **源码映射**：`ref/backend/tauri/src/lib.rs::run` 中基于 `CARGO_MANIFEST_DIR` 找本地 `.bin/prettier` 并忽略其输出 → `backend/tauri/src/lib.rs::run` 的 Tauri setup closure；Chimera 保留自己的 bindings 路径和 `normalize_typescript_bindings`，只迁移本地工具定位、绝对文件路径和格式化 best-effort 行为。该闭包早于 `WindowManager` 与首个窗口创建，因此 formatter 的可选失败不应阻断 UI。
- **验证**：macOS `cargo check --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib` 通过；`cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib specta_export::tests::typescript_bindings_are_fresh -- --test-threads=1` **1/1 通过**；Rustfmt、进度文档 Prettier 与 `git diff --check` 通过。独立审查与固定 ref 比对确认本地工具定位、跨平台调用及 formatter 错误不阻断 setup；本次未运行时注入缺失/失败 formatter，也未改变进程工作目录验证绝对路径。编译保留项目现有 warnings。未运行桌面冷启动；本批测试确认生成契约新鲜，不证明实际窗口像素可见性。
- **回滚**：仅恢复原先相对绑定路径与 `npx prettier` 阻断步骤；保留前一批 Dock activation 对齐。若回退，先确认新版本要求格式化失败继续启动的产品行为仍然适用。

## 2026-11-14：启动窗体与唤窗路由读取共享应用配置

- **目标 / 验收**：让初次窗口显示、深链接目标及快捷键切换目标读取已管理的共享应用配置快照，沿用 Chimera 的 `enable_silent_start` 和 Main/Legacy 选择；不再从 `Config::verge()` 另读一份 legacy 全局状态。
- **非目标**：不改变静默启动策略、默认 Legacy 界面、窗口标签、深链接队列、快捷键行为、IPC、配置格式或 `verge.yaml` 兼容桥；不改 Core/Service 初始化顺序，也不把同步窗口显示延后到后台 Core 启动完成。
- **基线 / 风险 / 接口**：父提交 `fc79f9ce43ba3d861b4f322d933fda8a4c023219`；固定 ref gitlink 与只读工作区 `59724e85c34abb6995c22d101c876cc06dc16576`。风险 L2（启动窗体选择与共享应用状态读取）。Tauri 内部 `configured_window_label` 改为接收 `&AppHandle`；无公开 IPC 或前端接口变化。
- **源码映射**：`ref/backend/tauri/src/utils/resolve.rs::resolve_setup` 从 `NyanpasuClient.app_config_snapshot()` 读取静默启动状态 → `backend/tauri/src/utils/resolve.rs::resolve_setup` 从 `ChimeraClient::get_app_config()` 读取 `ChimeraAppConfig`。Chimera 增补的 `WindowType::{Main,Legacy}` 同时由该共享快照提供；初次打开、深链接和热键复用同一解析函数。旧 `IVerge` 配置仍通过 `bridge/verge.rs::application_from_legacy` 初始化 typed config 并由 mirror 兼容。
- **验证**：定向启动窗体配置单测 **2/2 通过**；`bridge::verge::tests` **4/4 通过**，包括 legacy silent-start 开关与 Main/Legacy 投影；macOS `cargo check --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib` 通过；Rustfmt、文档 Prettier 与 `git diff --check` 通过。独立审查无阻断发现，并确认设置管理先于窗口路由读取；测试只验证纯配置选择和标签映射，未创建真实 Tauri 窗口。Rust 编译保留项目既有 warnings。未运行桌面冷启动、前端 WebDriver 或 macOS/Windows/Linux 窗口可见性测试；这些纯 Rust 检查不证明实际 webview 已显示。
- **回滚**：恢复 `resolve.rs` 对 `Config::verge().latest()` 的初次显示/路由读取及无参数 `configured_window_label` 调用；同时恢复 `lib.rs` 深链接和 `client/hotkey/adapters.rs` 的调用签名。保留已有 `application_from_legacy` 投影断言。

## 2026-11-14：启动资源复用已解析的数据根目录

- **目标 / 验收**：按固定 `ref` 的资源初始化入口，把资源目标目录改为启动时已发现并准备的 `PathResolver::app_data_dir()`，把包内资源源目录改为 Tauri `Manager::path().resource_dir()`；不再由启动资源 helper 通过全局目录状态重复查找两个路径。
- **非目标**：不改资源文件名单、文件较新时覆盖到数据根目录的既有规则、错误时继续显示 UI 的 best-effort 行为、数据目录布局、GIS/核心资源语义、日志或 Core/Service 次序。
- **基线 / 风险 / 接口**：父提交 `192da09a47a588c1f9d47f41cb85a9245cd25d8b`；固定 ref gitlink 与只读工作区 `59724e85c34abb6995c22d101c876cc06dc16576`。风险 L2（启动时复制包内资源到应用数据目录）。`resolve_setup` 内部增加 `&PathResolver` 参数，`init_resources` 改为接收显式目标/源目录；无 IPC 或持久化 schema 变化。
- **源码映射**：`ref/backend/tauri/src/utils/init/mod.rs::{bundled_resources_dir,init_resources}` → Chimera 同路径同名函数；`ref::resolve_setup` 使用 Tauri resource dir 与既有 `PathResolver` → Chimera `utils::resolve::resolve_setup(app, paths)`。只做必要的 Chimera 路径类型适配，并保留上游相同资源文件和复制时间戳规则。
- **验证**：`utils::init::tests` **7/7 通过**，其中新增用例验证资源只复制到注入的数据目录，资源源目录缺失时不创建数据根目录；macOS `cargo check --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib`、Rustfmt 与 `git diff --check` 通过。编译保留项目既有 warnings。未启动实际桌面包，因此未验证打包后 `resource_dir` 的内容/平台布局及窗口可见性。
- **回滚**：将 `init_resources` 恢复为无参数全局目录查询，将 `resolve_setup` 恢复单参数，并恢复 `lib.rs` 调用；移除新增临时目录单测。无需用户配置迁移或宿主系统清理。

## 2026-11-14：启动 Storage 使用同一数据根目录

- **目标 / 验收**：把 startup Storage 打开路径接到入口已解析并显式传入的 `PathResolver`，与 ref 在 setup 中使用 `paths.storage_path()` 的所有权一致。
- **非目标**：不改变现有 `storage.db` 文件名、redb schema、旧数据、Storage IPC/事件、web storage 监听、事务语义或故障时 UI best-effort 行为；不迁移其它运行时 Core/Service 路径 helper。
- **基线 / 风险 / 接口**：父提交 `e8494e863d753f3ab5526c4a52b231484b3eb15`；固定 ref gitlink 与只读工作区 `59724e85c34abb6995c22d101c876cc06dc16576`。风险 L2（打开持久 Storage 数据库）。`core::storage::setup` 内部增加 `&PathResolver` 参数并经 `open_storage` 测试边界使用其 `storage_path()`；无 IPC、schema 或文件位置变化。
- **源码映射**：`ref/backend/tauri/src/setup.rs::setup` 中 `Storage::try_new(paths.storage_path())` → `backend/tauri/src/core/storage.rs::{setup,open_storage}`，调用自 `utils::resolve::resolve_setup`。新 Tauri 内部路径共享前两批传入的 resolver，不再调用 `utils::dirs::storage_path()`。
- **保留差异 / 后续**：本批只迁移路径所有权；打开数据库失败仍沿用 Chimera `resolve_setup` 的 `log_err!` best-effort 行为，而 ref 会向 setup 返回错误。Storage IPC 随后会读取已管理的 `Storage`，因此失败语义需要单独迁移并覆盖，不把这条路径接线当作错误处理已对齐。
- **验证**：新增隔离数据根用例 **1/1 通过**，确认 Storage DB 建在注入 data root、config root 不被创建；macOS `cargo check --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib`、Rustfmt 与 `git diff --check` 通过。编译保留项目既有 warnings。未运行完整桌面冷启动或 web storage UI/E2E。
- **回滚**：恢复 `core::storage::setup(app)` 的旧签名、`utils::dirs::storage_path()` 查找和 `resolve_setup` 调用；移除新增 helper 单测。没有 Storage 文件搬迁，无需备份或清理用户数据。

## 2026-11-14：Storage 打开失败按启动边界传播

- **目标 / 验收**：对齐 ref 在 migrations 之后打开 Storage、把失败返回 setup 的顺序。Storage 无法打开时不得继续到首个主/Legacy 窗口和依赖该状态的 IPC；故障分类日志必须带上数据库路径。
- **非目标**：不改变 redb schema、数据库位置/字节、IPC/事件与 listener 语义；不重做全局 setup owner 架构，也不改变 Core/Service 后台启动或 UI 选择。
- **基线 / 风险 / 接口**：父提交 `e86544c175f75d5b412a51d32c1d5df3dce56ead`；固定 ref `59724e85c34abb6995c22d101c876cc06dc16576`。风险 L2（持久 Storage 和启动故障处理）。新增内部 `StartupStage::Storage`，`core::storage::setup` 移到 composition root 并通过 `report_boundary` 传播错误；无 IPC 或对外类型变化。
- **源码映射 / 有意差异**：ref `backend/tauri/src/setup.rs::setup` 在 migrations 后先创建 Core/Service owners 并启动 Jobs，再打开 `Storage::try_new(paths.storage_path())?`，随后构造 Client。Chimera 在 migration backup 边界之后先调用 `core::storage::setup(app, &paths)`，新增 `StartupStage::Storage` 映射 `FailureKind::Internal` 与 `paths.storage_path()`，之后才创建 Jobs/Core/Service owners。**这是对 ref 顺序的有意偏离**：Storage 失败时不创建还需回收的后台 owner；现有 `AppShutdown` 仍管理 Storage 成功后的 Jobs。后续 DIFF-069 Stage 3 若完成 setup 失败时 owners 的可靠取消/回收，应重新评估将 Storage 顺序收敛到 ref。
- **验证**：`core::storage::tests` **11/11 通过**，包括隔离无效数据库文件用例，确认错误边界失败、报告带 Storage 路径且原文件字节保留；`startup_failure::tests` **8/8 通过**。macOS `cargo check --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib`、`cargo clippy --offline --locked --manifest-path backend/Cargo.toml -p chimera --all-targets --all-features`、Rustfmt、Prettier 与 `git diff --check` 通过；保留项目现有 warnings。后续启动窗口复验中 E2E build 通过、`smoke.e2e.ts` **5/5 通过**；它没有断言原生窗口像素可见。`connections-legacy-layout.e2e.ts` 在可见性前置钩子失败、**0 个场景完成**，重现项目已有 LESSON-20261023-003 记载的隐藏 WebView 症状，根因仍未确认。未通过桌面 Tauri setup 注入无效数据库，所以错误对话框及首窗失败路径仍未由桌面运行证明。
- **保留差异 / 后续**：Storage 失败会阻止 setup 继续，但 `lib.rs` 的 `builder.build` 失败分支仍以通用 `Internal` 和 `path=None` 显示 one-shot 错误窗；详细错误链含数据库路径，独立 Path 标签仍写 `unresolved path`。DIFF-069 Stage 2/3 需要让结构化边界报告传到错误窗，并加入真实 Tauri setup 失败验证。
- **回滚**：恢复 `resolve_setup` 中的 best-effort Storage open 和原 Jobs 顺序，移除 `StartupStage::Storage` 与失败测试；Storage 数据文件无迁移，无需回滚用户数据。

## 2026-11-14：Tauri setup 失败弹窗保留结构化路径

- **目标 / 验收**：Storage、迁移、Jobs、CoreHost 或 TypedClient 的 startup boundary 失败后，Tauri `builder.build` 错误弹窗继续显示原始 stage、failure kind 与已分类路径；Storage 错误不再单独标注为 `unresolved path`。
- **非目标**：不改失败边界的顺序、不在失败时启动窗口、不迁移独立 crash-reporter 子进程，也不改变错误分类、备份或用户数据。
- **基线 / 风险 / 接口**：父提交 `2024f382eb2c7ecf675df3f0b4126a1086ac6e0f`；固定只读 ref `59724e85c34abb6995c22d101c876cc06dc16576`。风险 L2（启动错误呈现）。Tauri 内部 `setup::setup` 增加报告捕获参数；不改 IPC、持久化或外部 API。
- **源码映射 / 部分对齐**：ref `backend/tauri/src/startup_failure.rs::StartupFailure::from_setup` 为 `OpenStorage` 生成 `paths.storage_path()`，再由 `setup.rs::TauriStartupFailureHost::show` 交给 `startup_failure::show(Report)`；Chimera `startup_failure.rs::report_boundary_with_backup_and_capture` 在原日志/错误链传播边界捕获同一个 `Report`，`lib.rs::builder.build` 失败出口用 `show_report` 呈现 stage、kind、path、detail 与已确认备份目录。迁移之后的 Storage、Jobs、CoreHost、TypedClient 边界都传入 `migrations.backup_info()`，只有仍存在且 manifest 已验证的快照才展示。Chimera 仍通过 Tauri builder 错误出口显示，未迁移 ref 的 `run_setup`、owner retain/exit 或独立 crash-reporter；报告捕获是这层 Tauri 错误包装的窄适配。
- **验证**：`cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib startup_failure::tests -- --test-threads=1` **9/9 通过**，新断言验证 Storage 报告精确路径和已验证备份目录进入弹窗文本。macOS `cargo check --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib` 与 `cargo clippy --offline --locked --manifest-path backend/Cargo.toml -p chimera --all-targets --all-features` 通过，保留仓库已有 warnings。首轮链接因只剩 258 MiB 报 `errno=28`，未进入测试；删除 ignored 的 `backend/target/e2e` Cargo 构建缓存（12.3 GiB，未动源码或 E2E 证据）后，盘余 9.5 GiB 重跑通过。针对修改 Rust 文件的 `rustfmt`、记录文档的 Prettier 与 `git diff --check` 通过；未运行桌面 setup 故障注入，故未实证原生弹窗可见性。
- **保留差异 / 后续**：没有经过结构化 boundary 的错误（如 `RuntimePaths::from_resolver`、`client.get_app_config()` 及 `core::clash::setup`）仍会使用通用 fallback；本批只保证已包装边界的 Report。Tauri `builder.build` 错误呈现与 ref 的 setup owner/error-host 流程仍不同。后续 DIFF-069 Stage 3 需要解决 setup 部分失败后的 owner 取消/移交，再考虑迁移 ref 的 `run_setup` 生命周期，而不是继续扩展这个报告侧带。
- **回滚**：恢复 `setup::setup` 签名和 `lib.rs` 通用 `Internal/path=None` builder 错误出口；移除 capture 参数、`show_report` 和该报告回归测试。无持久数据修改。

## 2026-11-14：首窗就绪握手等待应用设置

- **目标 / 验收**：让初始隐藏 WebView 在应用设置查询成功或失败后再报告 ready，依 ref 的 `WindowReadyReporter` 时序显示；错误边界仍报告 ready，确保异常页不会永远隐藏。
- **非目标**：不等待 Core/Service 启动或运行时 reconcile，不改变首次路由、Main/Legacy 选择、设置查询内容、IPC 合同或 WebView 动画。
- **基线 / 风险 / 接口**：父提交 `e86544c175f75d5b412a51d32c1d5df3dce56ead`；固定 ref `59724e85c34abb6995c22d101c876cc06dc16576`。风险 L2（首窗显示时序）；无 IPC、持久化或公开接口变化。
- **源码映射**：`ref/frontend/nyanpasu/src/pages/__root.tsx::WindowReadyReporter` 在 `useSettings().query` 成功/失败后调用 `reportWindowReady`，`Catch` 仍直接报告 → `frontend/chimera/src/pages/__root.tsx::WindowReadyReporter` 使用同一 `useSettings` 条件，并保留 `Catch` 的错误页报告。
- **验证**：`pnpm typecheck` 通过；`pnpm e2e:tauri:build` 通过；`smoke.e2e.ts` **5/5 通过**。定向 `connections-legacy-layout.e2e.ts` 在 beforeEach 的可见性前置检查失败、**0 个场景完成**；屏幕截图 `tauri-e2e/.tmp/artifacts/startup-ready-handshake/connections-legacy-layout-before-each-failure.png` 显示 Legacy 导航栏已渲染、内容区仍透明，诊断为 `document.visibilityState=hidden`、`document.hasFocus()=false`，`.page-transition` 动画未推进。与已有 LESSON-20261023-003 一致，因此本地 WebKit E2E 不能证明该时序修复已让原生首窗可见，也没有放宽可见性断言。日志和截图均为 ignored 的本机证据。
- **保留差异 / 后续**：Chimera 仍在 setup 阶段优先显示窗口、后台启动 Core；ref 的 `resolve_setup` 同步等待 `startup_reconcile` 后才显示 Main。保留此产品差异，避免初始化等待阻止 UI 出现。Legacy visibility E2E 的证据只确认被测 WebView 为 hidden；当前尚不能归因于应用代码或自动化前台环境，也不能据此证明用户原生首窗可见。需要能把本次 E2E 进程带到前台的桌面会话或原生可见性断言复验。
- **回滚**：移除 `WindowReadyReporter` 并恢复 `App` 挂载时立即报告；保留 `Catch` 错误页报告。

## 2026-11-14：macOS 交通灯初始化运行于主线程

- **目标 / 验收**：让 macOS 原生交通灯窗口适配只在 AppKit 主线程创建 `MainThreadMarker` 并安装，消除从 Tokio worker 创建窗口时的主线程断言 panic。
- **非目标**：不改变交通灯位置、窗口装饰、fullscreen/resize 行为、窗口显隐、App activation、标签或非 macOS 窗口。
- **基线 / 风险 / 接口**：父提交 `e86544c175f75d5b412a51d32c1d5df3dce56ead`；固定 ref `59724e85c34abb6995c22d101c876cc06dc16576`。风险 L2（macOS 原生窗口生命周期）；无 IPC、持久化或公开接口变化。
- **源码映射**：ref 在 `window/kinds.rs::hide_traffic_lights` 通过 `WebviewWindow::run_on_main_thread` 调度 AppKit；`window/manager.rs::create` 用 Tauri builder 的 `traffic_light_position`。Chimera 保留 `window.rs::setup_traffic_lights_pos` 的 delegate 适配（用于 resize/fullscreen 后保持自定义位置），由 `window/manager.rs::create` 先调用 `run_on_main_thread`，再获取 marker；若调度或 marker 失败则记录错误而不 panic。
- **验证**：E2E build 通过；真实 `smoke.e2e.ts` 经 `create_main_window` 打开第二个 Main WebView 并从两个窗口验证 fetch 合同，**5/5 通过**；对应运行日志未发现 `MainThreadMarker` panic。该场景不验证原生窗口前台可见状态；Legacy 首窗 visibility E2E 仍失败在 WebView hidden 前置条件，其根因未确认。针对本批 Rust 文件的 rustfmt 检查、`cargo check --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib`、`cargo clippy --offline --locked --manifest-path backend/Cargo.toml -p chimera --all-targets --all-features` 与 `git diff --check` 通过；保留既有 warnings。全仓 `cargo fmt --all --check` 被未修改的 vendor/path 文件格式差异挡住，未格式化无关源码。
- **保留差异 / 后续**：Ref 使用 builder traffic-light position 且只对 tray menu 隐藏按钮；Chimera 的 fullscreen/resize delegate 是历史窗口扩展，尚未证明可由 builder 语义完全替代，故保留并只修复线程边界。
- **回滚**：恢复 `MainThreadMarker::new().unwrap()` 及直接 `setup_traffic_lights_pos` 调用；不要撤销独立的窗口就绪握手或 Storage 启动失败边界。

## 2026-11-14：启动 setup 失败沿 ref owner 生命周期回收（DIFF-069 Stage 3，部分对齐）

- **目标 / 验收**：按 ref `setup.rs::{run_setup,StartupOwners}` 将 Tauri setup 内错误转换为结构化启动报告；失败时先取消共享任务、保留仍存活的 startup owner，再请求以失败码退出。启动退出必须排空已跟踪的 Jobs、system proxy、Core/Clash 订阅、Core 状态/服务监视和其它 composition tasks；Client 创建前失败时关闭已构建的 `CoreControl`。只有 `ChimeraClient` 接管完整 core 后才移除暂存 core owner。
- **非目标**：不改变 Core/Service reconcile 策略、Main/Legacy 窗口选择、窗口 ready 协议或首窗显示时序；不迁移 crash reporter、IPC 或持久化格式；不声称已修复隐藏 WebView 的根因。
- **基线 / 风险 / 接口**：父提交 `2bd820a477f8c42556281e564106bca1c9631065`；固定 ref `59724e85c34abb6995c22d101c876cc06dc16576`（只读且干净）。风险 L2（启动阶段持久 Storage 与 owner 生命周期）。内部签名变化：`setup::setup` 接收 `&mut AppShutdown`；`CoreFacade::new_local_with_control`、`CoreLifecycleClient::spawn_with_profiles`、服务 API monitor 与本地 Core 状态 watcher 接收同一 cancellation token / `TaskTracker`；没有 IPC、持久化 schema 或外部 API 变化。
- **源码映射**：`ref/backend/tauri/src/setup.rs::{StartupOwners::{request_shutdown,wait_shutdown},run_setup,StartupFailureHost,TauriStartupFailureHost}` → `backend/tauri/src/setup.rs::{AppShutdown,run_setup,StartupFailureHost,TauriStartupFailureHost}` 与 `backend/tauri/src/lib.rs::run`。错误先复用 `report_boundary_with_backup_and_capture` 捕获的 Report，未分类 setup 错误才回退为 `StartupStage::Setup`；再取消 owner、显示 Report、保留 `AppShutdown` 并请求退出。Chimera 没有单独的 ref `CoreClient` 字段：本地 `CoreControl` 暂存到 `AppShutdown`，完整 `ChimeraClient` 管理后由现有 `utils::help::cleanup_processes_async` 收尾。`utils::help::cleanup_processes_async` 在 Client 尚未完成构造时不再把“没有 Client”记作清理失败。
- **顺序收敛**：有了 Client 构造前的可靠 owner 取消/排空后，`Storage` 打开移到 migrations、Jobs、CoreHost、shortcut 与 system-proxy owners 初始化之后、TypedClient 构造之前，与 ref 的 Storage 所有权顺序收敛。Storage 打开失败仍保留已确认的 migration backup 信息，并经同一 startup failure host 报告。
- **保留差异 / 后续**：这是 setup 错误边界与部分 owner 回收的对齐，不是完整启动流程对齐。未包装的错误仍映射为通用 Setup/Internal 报告，不会伪造某个具体文件路径，也不带 migration backup；ref 将 `CoreClient` 放在 owner 中并在 owner shutdown 时 finish，Chimera 在完整 Client 管理后仍通过现有 `cleanup_processes_async` 请求 CoreLifecycle shutdown。主窗口与后台 startup reconcile 的先后仍按 Chimera 现状保留；固定 ref 在 `resolve_setup` 中的 reconcile / show 顺序还需单独评估，不能因迁移而延迟可见 UI。此前 Legacy E2E 捕获到 hidden WebView 但根因未确认，本批没有真实桌面冷启动或原生可见性证据。
- **验证**：`setup::tests` **3/3 通过**（setup 失败先取消并保留 owner、partial CoreControl 关闭、成功路径不请求退出）；`utils::resolve::startup_shutdown_tests` **3/3 通过**（已开始的准备任务在 shutdown drain 完成前不退出）；`startup_failure::tests` **9/9 通过**（原始 stage/path/verified backup 报告保持）。首次测试编译发现 `utils::resolve` 测试构造 `AppShutdown` 时未初始化新增的 Core owner 字段，改用 `AppShutdown::default()` 后定向测试通过；这是本批内已修正的测试接线错误。编译保留仓库现有 warnings；桌面启动、错误对话框原生可见性和 Windows/macOS/Linux 窗口矩阵未验证。
- **回滚 / 后续计划**：回滚本批 `setup.rs` / `lib.rs` / `startup_failure.rs` / `setup_tests.rs` / `core/clash/mod.rs` / `client/{mod.rs,core_lifecycle/mod.rs}` / `core/actor_v2/{facade.rs,local_runtime.rs}` / `utils/{help.rs,resolve.rs}` 中对应变更。之后继续 DIFF-069 Stage 4：逐项对照 ref 的 Core/Service 初始 reconcile 和启动状态；Stage 5 再收敛窗口 readiness、深链接、tray 与退出时序。当前没有 L3 approval trigger；任务分支 push 仍遵循已授权的签名与分支范围。

## 2026-11-14：启动 Service 预检与监视器使用共享 Client owner（DIFF-069 Stage 4a，阶段完成）

- **目标 / 验收**：将启动 Service 预检的配置读取和执行方式收敛到 ref 的应用 Client/异步生命周期：只读 `ChimeraClient` 的 typed application config，在已跟踪的 Tauri 异步启动任务中完成预检，不再从 `Config::verge()` 读取重复状态，也不嵌套同步 `block_on`。Service 预检仍为 best-effort；失败要记录并继续已有 Core startup reconcile。Core/Service IPC 状态转换和周期健康监视注册到同一 `AppShutdown` TaskTracker，监听传入 Client 的 shutdown child token；调用方在探测之间协作取消。探测状态发布、显式 Service 操作与 host 切换共享串行锁；先结束正在进行的探测，再在锁内作废旧代次并释放锁，随后由现有 Service actor 持锁执行命令。端点掉线恢复在释放探测锁后申请独立 Service lease，等锁期间响应关闭信号，并在真正获锁后复查关闭状态和健康代次，防止退出过程或停止/重启后的过期恢复。启动初始化在释放探测锁后才等待旧监视器退出；已进入锁保护区的探测/Service 操作会排空后再做 Core shutdown，仍在等待锁的 IPC 事件遇到关闭会取消。IPC 转换持锁后的核心状态读取最多等待 5 秒，并可由 shutdown 取消，避免无期限卡住 TaskTracker 排空。更新完成后重新读取当前 typed Service Mode，避免用过期偏好启动监视器。
- **非目标**：不替换现有 Core/Service actor 或 host ownership 算法；不改变 Service 更新/健康监视策略、窗口打开时序、Main/Legacy 选择、IPC、持久化格式或权限；不把当前结构称为完整 ref startup reconciliation。窗口创建请求仍先于 Service 预检及 Core reconcile。
- **基线 / 风险 / 接口**：父提交 `bf96ada9b72fa64658c683499119f81e075b6050`；固定 ref `59724e85c34abb6995c22d101c876cc06dc16576`（只读、干净）。风险 L2（Service 生命周期与 core 启动顺序）。内部接口变化：`ChimeraClientInner` 保存 setup 传入的 shutdown child `CancellationToken` 和共享 `TaskTracker`，暴露 caller-managed child-token 与 tracked-spawn 边界；`utils::init::init_service` 改为 async；`core::service::init_service` 从同一 Client typed config 读取 Service Mode 并在启动监视器前重读；`request_service_endpoint_down_if` 在等待 lease 时可取消，并在 lease 获得后复查关闭状态和健康代次；tracked IPC 状态转换通过 `await_core_status_or_shutdown` 等待至多 5 秒且可立即响应 shutdown。Approval triggers：无，本批未替换主生命周期架构；若后续需要替换 Core/Service ownership 算法，先提供具体设计供维护者审阅。无 IPC 或持久化合同变化。
- **源码映射**：`ref/backend/tauri/src/utils/resolve.rs::resolve_setup` → `backend/tauri/src/utils/resolve.rs::resolve_setup`；ref `nyanpasu-core/src/client/app_lifecycle.rs::{shutdown_child_token,track_until_shutdown}` → Chimera `ChimeraClient::{shutdown_child_token,spawn_tracked}`；ref `application_workflow/startup::startup_reconcile` / `NyanpasuClient::startup_reconcile` → Chimera `ApplicationWorkflow::startup_reconcile` / `CoreLifecycleClient::startup_reconcile`，ref `client/app_lifecycle.rs::start_background_sources` → `ChimeraClient::init_core` 中 `ProfilesClient::start_producers`。ref 的 startup API 返回结构化 `StartupReport`；Chimera 仍返回 `anyhow::Result<()>`，留待下一阶段迁移。ref 通过 shared Client workflow 读取 application config；ref 没有直接对应的 Chimera Service preflight adapter，本阶段把 `utils::init::init_service` 与 `core::service::init_service` 的配置来源统一到 `ChimeraClient::get_app_config`，并将 `core/service/ipc.rs` 的健康轮询与状态事件任务纳入同一 Tracker。Service preflight 保留为 Chimera adapter，当前仍先于 `init_core`。相同/较新 Service 版本不需要更新时保持静默。
- **验证 / 覆盖**：`core::service::ipc::tests` **14/14 通过**（包括旧健康代次等待 host lock 时失效、监视器退场等待/重新取得 admission、转换代次推进、迟到 Stop 回调保持后续 Start 代次，以及核心状态读取可由 shutdown 取消并有有限超时）。`client::tests::tracked_background_work_drains_after_root_shutdown` **1/1**；`client::core_lifecycle::tests` **39/39**；`utils::resolve::startup_shutdown_tests` **3/3**、`setup::tests` **3/3**、`startup_failure::tests` **9/9**、`startup_reconcile` **2/2** 均通过。`cargo check --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib` 与 `cargo clippy --offline --locked --manifest-path backend/Cargo.toml -p chimera --all-targets --all-features` 通过；保留仓库既有编译/lint warnings。Rustfmt、该文档 Prettier 检查与 `git diff --check` 通过。**覆盖边界**：测试没有驱动真实 Service wrapper 的完整成功/失败/重启重交接，也没有实测已安装 Service、系统 TUN 或原生窗口可见性；当前机器锁屏，无法进行原生冷启动可见性验证。启动任务对已经提交的 `client.init_core()` 故意等待生命周期 actor 的终态回复，不取消可能仍在排队/执行的 Core 命令；若该 reconcile 永不返回，shutdown drain 仍会等待，下一阶段需按 ref `StartupOutcome::Unsettled` 和 recovery 语义评估安全收敛。
- **保留差异 / 后续**：本地 Tauri 的窗口创建调用先于 Service/Core reconcile；这只是创建请求顺序，不代表原生首窗已经可见，且前端 ready 握手之前窗口会保持隐藏。ref 在 `resolve_setup` 中同步等待 reconcile 的实现本身标有需重构的 TODO。此时序差异的重新评估条件是 ref 将 reconcile 与窗口可见性解耦，并且冷启动验证能证明首窗不会等待 Service/Core；在此之前不能直接把阻塞顺序迁回。Ref 的 `application_workflow/startup.rs` 具有结构化 `StartupReport`、owner evidence 与 `reestablish` 流程，而 Chimera 仍以 `CoreLifecycleWorkflow::reconcile()` 和既有 Service adapter 承担不同的状态/host owner 协调，故本节仅完成 Stage 4a，不是完整启动流程对齐。继续迁移前，逐项核对 ref 启动状态、Legacy UI/Agent 入口和 recovery/failure 行为；若需要替换 Core/Service owner 算法，先提交具体设计供维护者审阅。
- **回滚 / 后续**：回退 `client/{mod.rs,core_lifecycle/mod.rs}`、`core/service/{mod.rs,ipc.rs}`、`ipc.rs`、`utils/{init/mod.rs,resolve.rs}` 以及本节即可；没有数据格式或用户数据迁移。下一阶段继续对比 ref 的结构化 startup report / reconcile 与当前 `CoreLifecycleWorkflow` 语义，并迁移可兼容的状态与证据模型；保留先显示 Main/Legacy 的 Chimera 扩展，不把 ref 在 resolve_setup 内阻塞 reconcile 的顺序直接带回。

## DIFF-069 Stage 4b：Startup report 与 owner 证据接入（基础迁移完成；整体启动迁移仍为 partial）

- **目标 / 验收**：按固定 ref 的 `StartupReport`、`StartupObservation`、`ServiceEvidence`、`OwnerPlan` 和 `plan_owner` 迁移启动决策入口；启动命令独立于普通 `Reconcile`，首份报告一次性保存并供 `ChimeraClient::init_core` 消费。Service 不可判定、另一实例持有 Core 或现有 Core 状态未确认时，不因“Service Mode 当前不可连接”而盲目启动第二个 Local Core。需要退役本实例的 Service Core 时，先确认 stop 终态和 Service 再探测结果，再执行 Local reconcile。
- **非目标**：本批不合并 Profile TCC 与 Core actor，不改变 legacy/main/agent 使用的 application API，不改变 Service 安装/启动权限或首窗时序；不声称已迁移 ref 的完整 `reestablish`、receipt 验证、RecoveryTarget、DeferredRetry 与 backoff。窗口仍在 `resolve_setup` 中优先发起创建请求，Core 启动留在 AppShutdown 跟踪任务中。
- **基线 / 风险 / 接口**：父提交 `1a9c0b3f95c470b9dba0db76d1f00c0eee93dfcc`；ref `59724e85c34abb6995c22d101c876cc06dc16576`（工作区干净）；风险 L2（启动 Core/Service ownership）。作用接口：`client/application_workflow/startup.rs`、`client/core_lifecycle/{mod.rs,workflow.rs,ports.rs,adapters.rs}`、`core/actor_v2/facade.rs`、`ChimeraClient::init_core`。本批沿用维护者“持续迁移直到 startup 对齐 ref”的明确范围，不替换 actor 或数据所有权边界；若后续完整 recovery 需要合并/替换 actor 状态所有权，再单独提交具体架构方案。
- **路径 / 符号映射**：ref `backend/nyanpasu-core/src/client/application_workflow/startup.rs::{StartupReport,StartupOutcome,StartupObservation,ServiceEvidence,OwnerPlan,plan_owner,ApplicationWorkflow::startup_reconcile}` → Chimera `backend/tauri/src/client/application_workflow/startup.rs` 的同名数据/纯决策符号及 `CoreLifecycleWorkflow::startup_reconcile`；ref `core_lifecycle::effective_host` → Chimera 启动适配根据 typed `ChimeraAppConfig`, `ServicePhase`, 当前 `ExecutionHost` 求取；ref `retire_unreceipted` / owner stop 端口 → Chimera `CoreLifecyclePort` 与 `CoreFacade` 的 Service 定向停核适配；ref `resolve_setup` 同步 reconcile → Chimera 保留异步启动任务和首窗先创建的必要产品边界。
- **实际验证 / 覆盖**：阶段开始时 `client::application_workflow::startup::tests` **3/3**、`client::core_lifecycle::tests` **44/44** 通过。独立复核第一轮发现 actor/Service lease 反向等待风险，已在提交前把租约获取移到 mailbox 入队前并新增回归测试。第二轮复核随后定位到 Service owner handback 会走无条件 Stop，且本地 transition mutex 无法封住跨进程替换竞态；修复与专门验证记录于 Stage 4c。**未覆盖**：真实 Service wrapper / TUN、原生首窗冷启动可见性；当前桌面会话锁屏，无法完成窗口实测。完整 ref `reestablish`、receipt 验证和 retry recovery 尚未迁移。
- **回滚 / 审批触发**：回退本批 startup report / command / typed owner plan、Service 定向停核 port/adaptor、`LESSONS.md` 回归记录及本节；不涉及持久数据迁移。若实际实现需要重写 CoreFacade 主机状态所有权、合并 CoreLifecycle/Profile TCC actor 或改变 UI/Agent application API，则暂停并先提供新的具体设计审阅。

## DIFF-069 Stage 4c：Service handoff owner fence（实现与本地验证完成，独立复核待进行）

- **目标 / 验收**：Service Core 状态不可读时不提交 Service 命令或本地启动；当前 Service owner 要交回 Local 时按证据分流：foreign / 已停止 / 缺失 Core 直接走无 Service Stop 的 Local reconcile；确认属于本应用的运行 Core 时携带进程 UUID 与 revision 发起条件停止，并二次探测后才启动 Local。跨应用进程的条件比较与停止必须在 Service manager 同一互斥区内完成。
- **非目标**：不更改 ref `plan_owner` 决策表；`ForeignCore` 强制 Local、条件停止协议、兼容旧 Service 时 fail-closed 属于 Chimera 共享 Service 实例隔离适配。此阶段不迁移 ref `reestablish` / receipt、恢复目标及 DeferredRetry / backoff，不改变 Main / Legacy 窗口创建顺序、Service 权限、TUN 或网络配置。
- **基线 / 风险 / 接口**：父提交 `1a9c0b3f95c470b9dba0db76d1f00c0eee93dfcc`；固定 ref `59724e85c34abb6995c22d101c876cc06dc16576`，只读且干净。风险 L2（启动 host ownership 与 Service Core）。新增接口为 IPC tagged variant `CoreCommandInfo::StopIfOwner { expected_owner }`、manager 内部 `CoreCommand::StopIfOwner(CoreOwner)`、manager 锁内 `stop_if_owner`，以及 startup 专用 `reconcile_after_service_release` ports。旧版 Service 对未知 variant 会拒绝请求，客户端不退化为无条件 Stop；启动保持 RecoveryRequired，等待 Service 更新或下一次启动。Approval trigger：无，本次只增加带 owner fence 的单条 IPC 命令，没有重写 actor / 状态所有权边界。
- **路径 / 符号映射**：ref `startup.rs::{plan_owner,ServiceEvidence}` / `reestablish` 的 owner handoff → Chimera `application_workflow/startup.rs::ServiceEvidence::expected_owner` 与 `core_lifecycle/workflow.rs::{RetireServiceThenLocal,HandBackToLocal}`；ref 的 Service stop port → Chimera `CoreLifecyclePort::retire_service_core` → `CoreFacade::retire_service_core` → `ServiceEndpoint::wire_submit_request` → `chimera_ipc::api::core::v2::CoreCommandInfo::StopIfOwner` → `CoreManagerService::submit_v2` → `CoreControl` → `CoreManager::stop_if_owner` → `RuntimeInstance::cancel_if_owner` → `Supervisor::cancel_if_current`。manager 在 `CoreManager::ctrl` 锁内比较 active owner 并与其它 manager 命令串行；supervisor fence 另以当前 child UUID/PID 检查并围住 DNS pre-stop / cancel，防止不经过 manager 锁的自动 respawn 替换 child。
- **验证契约**：Service 观察 token 缺 UUID、epoch 或 revision 时，停止或 handback 必须失败关闭；若共享 Service 在观察后换成另一实例 / 配置，旧 token 必须返回 OwnerConflict 且 replacement 保持运行；foreign Service handback 不能提交 Stop，但需以 Local reconcile 完成启动。
- **实际验证 / 覆盖**：`cargo test --offline --locked --manifest-path backend/chimera-runtime/Cargo.toml -p chimera-core-manager --lib -- --test-threads=1` **137 通过、2 忽略**；其中 Manager 测试覆盖 owner token 合同，respawn 状态替换由测试替身模拟。新增真实子进程监督器测试 `cargo test --offline --locked --manifest-path backend/chimera-runtime/Cargo.toml -p chimera-utils --features process --lib process::supervisor::tests::cancel_if_current_serializes_respawn_during_pre_stop -- --test-threads=1` **1/1**，实际等待子进程退出发生在 pre-stop 临界区内，并断言监督器没有发布替代 child。IPC `stop_if_owner_roundtrips_the_full_owner_identity` **1/1**；Tauri Service adapter `stop_if_owner_wire_keeps_process_and_revision_fence` **1/1**；`cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib startup_ -- --test-threads=1` **33/33**（foreign / unreadable handback、owner retirement、二次 probe、actor lease 竞态）。`cargo check` 已对 backend `chimera --lib` 及 runtime `chimera-core-manager` / `chimera-service` / `chimera-ipc` 通过。以上为 Rust 单元与编译验证，不是已安装 Service 的跨进程桌面 E2E。
- **保留差异 / 后续**：Service handoff 条件 stop 不覆盖普通用户主动 Stop 与非启动 reconcile 的原有命令；这些不属于本阶段 startup fence，需按调用点单独审核。独立复核另发现 manager 锁不覆盖 `Instance` 的自动 respawn；Stage 4d 为 supervisor 当前子进程增加 owner / PID fence 并将 DNS pre-stop 置于取消临界区，未把 manager 锁单独误报为完整停止围栏。Stage 4d 继续对照 ref `reestablish` 的状态快照、S3 retirement / receipt 与 S4 committed config apply；Stage 4 完整前不能称启动流程已对齐。
- **回滚 / 后续审批**：移除新增 `StopIfOwner` IPC variant / manager command / owner comparison，以及 startup handback 的证据分流、对应 tests / LESSONS / progress 记录。回滚会重新开放当前已封住的跨进程 Stop 竞态，须暂停相关 Service→Local retirement，不可将旧的无条件 Stop 当作等价降级。若下一阶段需要跨 actor 合并或更换状态所有权，提供架构方案后再实施。

## DIFF-069 Stage 4d：启动 owner recovery 与失败 Service 回退（本地验证与独立复核完成）

- **目标 / 验收**：对照固定 ref `application_workflow/startup.rs::{reestablish,establish,retire_and_apply,apply_committed,wait_for_dependency,schedule_retry}`。启动 Service Core reconcile 已知失败时，只有新鲜探测证明属于本实例、服务端条件停止命中原 owner，且二次探测确认释放后，才回退 Local；Service 不可读、owner 不完整、停止未确认或 reconcile outcome 不确定时保持 startup owner gate，不启动第二份 Core。Service 由 Unknown 探测恢复为同一 ready generation 时，owner-unproven startup 也会重新走 fresh proof，不能因 readiness generation 不变而永久锁住。
- **非目标**：本批没有将 ref 的统一 `ApplicationWorkflow` attempt/deferred-target 状态机完整替换 Chimera 的 `CoreLifecycleWorkflow` + Profile TCC 双边界，也没有迁移其恢复 receipt 的全部 compare matrix、target identity、3 次应用重试和 5/5/10/30/60 秒依赖退避；这些仍是 Stage 4 的未完成差异。不能因实现 Service→Local fallback 就声称完整 `reestablish` 已迁移。首窗继续在 `resolve_setup` 中先发起创建请求，Core/Service 恢复不阻塞窗口创建。
- **基线 / 风险 / 接口**：父提交 `1a9c0b3f95c470b9dba0db76d1f00c0eee93dfcc`；固定 ref `59724e85c34abb6995c22d101c876cc06dc16576`，只读且干净；风险 L2（Service Core ownership / recovery admission）。新接口沿用 `RecoverStartup`、共享 owner-unproven gate 与 Service ready-generation；本阶段增加对同 generation owner evidence 变化的 recovery signal。嵌套 `backend/chimera-runtime` 的 supervisor current-child owner/PID fence 是 Stage 4c review finding 的修复，避免把 manager 控制锁误认为可阻止 autonomous respawn。
- **路径 / 符号映射**：ref `startup.rs::ServiceEvidence / OwnerPlan / plan_owner` → Chimera `client/application_workflow/startup.rs::{ServiceEvidence,OwnerPlan,plan_owner}`；ref `ApplicationWorkflow::reestablish` 的 S1 Service settle / probe 与 owner evidence → `CoreLifecycleWorkflow::startup_reconcile_once`、`service_evidence`、`CoreLifecycleClient::startup_recovery_needed`；ref Service handback / retire → `CoreLifecyclePort::retire_service_core` → `CoreFacade::retire_service_core` → tagged `StopIfOwner` IPC → Service manager owner comparison → `Instance` supervisor `cancel_if_current`（UUID/PID + pre-stop DNS restore + cancel 的单一 child-current 临界区）。其余 S3 receipt 验证和 S4 attempt / retry target 尚未接线。
- **实际验证 / 覆盖**：backend `client::core_lifecycle::tests` **48/48**、Profile TCC **12/12**；`cargo check --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib` 通过（仓库既有 warnings）；runtime `chimera-core-manager` 全套 **137/137 通过，2 ignored**；same-generation Service evidence 变化的纯判定单测包含在 48 项生命周期测试内；真实监督器当前 child fence 测试 **1/1**。**未覆盖**：owner evidence 变化到 watch loop、租约获取与 `RecoverStartup` 投递的 actor 集成链、本机锁屏会话的原生窗口冷启动可见性、真实 Service wrapper/跨进程 Core handback、TUN/host 网络、Windows/Linux CI。
- **保留差异 / 后续**：本轮保留服务 owner 不可证明时关闭 Profile runtime/TCC 及其它 Core lifecycle mutations 的 fail-closed gate；`CoreFacade` 与 Service 的共享 ownership 保持现有边界。Stage 4e 应逐项映射 ref `RuntimeApplyReceipt` 验证、`DeferredTarget` identity 与 backoff、已知 retryable vs deterministic outcomes、显式恢复，以及 Legacy UI/Main/Agent 复用同一 startup/recovery API。Stage 4 完整前不标记启动对齐完成。
- **回滚 / 后续审批**：回退 same-generation owner evidence recovery trigger、Stage 4d startup fallback/generation changes、同批测试和本节；若回退 supervisor fence，也须同步暂停 Service 的条件停止路径，避免重新开放自动 respawn TOCTOU。若后续需要替换 actor / 状态所有权架构，先提交具体迁移设计并取得维护者审批。

## DIFF-069 Stage 4e：启动目标重试与报告幂等（实现和本地验证完成；整体仍 partial）

- **目标 / 验收**：按固定 ref 的 `startup.rs::{wait_for_dependency,schedule_retry,DeferredTarget}` 与 `convergence.rs::{RETRY_DELAYS,REESTABLISH_WAIT_DELAYS,next_wait}`，让启动目标保留身份、应用重试预算和依赖退避。启动目标失败后首次延迟 1 秒，最多自动尝试 3 次（后续 5 / 30 秒）；依赖等待按 5 / 5 / 10 / 30 / 60 秒循环且不扣应用预算。Service lease 取不到时重新进入依赖等待；owner / runtime 结果不确定时不自动重试。普通 Core/Profile runtime reconcile 成功解决旧目标，目标变化后旧 timer 不得再执行。
- **非目标**：不替换 `CoreLifecycleWorkflow` 与 Profile TCC 的状态所有权，不新增 IPC / 持久化格式，不把窗口创建改为等待 Core。ref 的完整 `RuntimeApplyReceipt` recovery compare matrix、stop intent、S3 对无 receipt 运行实例的条件退役与 S4 stopped-baseline apply 尚未迁移；首窗冷启动原生可见性、已安装 Service 跨进程 handback、TUN/host network 和多平台桌面 E2E 未验证。
- **基线 / 风险 / 接口**：父提交 `1a9c0b3f95c470b9dba0db76d1f00c0eee93dfcc`；固定 ref `59724e85c34abb6995c22d101c876cc06dc16576`，只读且干净；风险 L2（启动 retry target / Core lifecycle）。增加内部 `StartupRetryTarget`、`StartupRetryDue` / `StartupRetryLeaseFailed` actor 消息与 `RetryStartup` 命令；没有 public API、IPC 或数据 schema 变化。仍以现有 `CoreLifecycleWorkflow` 为 startup retry owner，属于当前架构下的兼容映射。
- **路径 / 符号映射**：ref `application_workflow/startup.rs::wait_for_dependency/schedule_retry` 与 `DeferredTarget::{identity,attempts_remaining,waits,health,next_attempt}` → Chimera `client/application_workflow/startup.rs::StartupRetryTarget`；ref `convergence::next_wait` → Chimera `client/convergence.rs::{RetryBudget,next_wait}`；ref 由共享 `ApplicationWorkflow` 持有目标 → Chimera `CoreLifecycleWorkflow::startup_retry_target`。Chimera actor 在 deadline 到达后先申请 Service transition lease，再将 `RetryStartup` 投递到同一生命周期 mailbox；lease 失败用独立消息重新安排依赖退避。运行时输入可读时使用完整 `RuntimeInputs::target_key()`；内容不可读时只使用带 `profile-content-unavailable:` 前缀的临时身份，并只作不扣预算的 dependency wait，内容恢复后完整 key 会导致身份重新建立。无 Profiles 的兼容路径使用 application / clash / profile projection。
- **实际验证 / 覆盖**：`client::core_lifecycle::tests` **53/53**、startup retry policy **3/3**、Profile TCC **12/12**；其中 actor 集成覆盖 transient failure → Service lease → mailbox retry → recovered、lease 失败后依赖退避重排、uncertain 不重试、成功 Core 目标变更使旧 timer 失效、recovery 保留首次 StartupReport。`cargo check --offline --locked --message-format=short --manifest-path backend/Cargo.toml -p chimera --lib` 通过，保留仓库 warnings。测试使用 mock Core / Service port；不等于安装 Service 或真实 Core 冷启动证明。
- **保留差异 / 后续**：Profile 候选被确认应用后会清掉旧 startup target；已保存但保持停止的 Profile 状态仍受现有 CoreLifecycle 与 TCC 分界影响，没有 ref `CoreRunIntent` stop-intent 闭环。启动 owner/recovery 仍直接调用 Chimera reconcile port，不能据此声称 ref 的 receipt 验证、verified-running-receipt 接纳或 unreceipted stop-first 语义已经迁移。下一阶段对齐 `RuntimeApplyReceipt` target identity、host / running intent / source hash / core kind / confirmed binding 比较，以及 S3 retirement / S4 committed apply；再检查 Main / Legacy / Agent 都通过同一 CoreLifecycle/Application API。原生首窗可见性仍因当前桌面会话条件无法实测。
- **回滚 / 后续审批**：回退 `StartupRetryTarget`、actor timer 与 RetryStartup 命令、reconcile target refresh、对应测试和本节；恢复仅一次启动 reconcile 的旧行为。若后续实现需要把 startup retry / stop intent 从 CoreLifecycle 移入共享 ApplicationWorkflow 或替换 Core/Service actor 状态所有权，先提交具体架构方案供维护者审阅；receipt verifier 与窄 adapter 迁移可继续在现有边界内推进。

## DIFF-069 Stage 4f：恢复回执与启动 owner 退役（实现完成；整体仍 partial）

- **目标 / 验收**：对照固定 ref `runtime_recovery.rs::verify_recovery_target` 与 `application_workflow/startup.rs::{retire_and_apply,verified_running_receipt,stop_owner,apply_committed}`。仅显式 Recovery 可复用当前运行实例，并且运行回执、Host、Running 意图及完整最新已提交目标必须全部匹配；否则先停止当前 owner 并确认停止，再从已提交配置重建。已有未决 Core mutation、owner 不可证明或 Stop 未确认时，不能继续尝试重建。
- **非目标**：不更换 Chimera 的 `CoreLifecycleWorkflow` / Profile TCC 状态所有权，不改变 Core/Service ownership adapter 或 IPC，也不等待 Core/Service 完成后再创建启动窗口。首窗仍先打开，后台启动流程继续通过共享 `ChimeraClient` / `CoreLifecycleClient` 执行。
- **基线 / 风险 / 接口**：父提交 `1a9c0b3f95c470b9dba0db76d1f00c0eee93dfcc`；只读 ref `59724e85c34abb6995c22d101c876cc06dc16576`，本批检查时干净。风险 L2（Core/Service runtime ownership 与启动恢复）；新增内部恢复来源标记和 owner 停止确认分支，无 IPC、持久化 schema 或公开 API 变化。回滚时需一起移除相应 owner 证明与停止确认测试，不可单独回退 Service 条件停止适配。
- **路径 / 符号映射**：ref `runtime_recovery::verify_recovery_target` / `startup::verified_running_receipt` → `CoreLifecycleWorkflow::{recovery_receipt_matches,startup_target_identity}`；ref `startup::retire_and_apply/stop_owner` → `startup_reconcile_once`、`retire_service_startup_owner` 与 Local Stop 状态确认；ref `startup::apply_committed` → 既有 `CoreLifecyclePort` 从 Typed Application/Clash/Profile 的已提交快照生成并 reconcile runtime。仅 `RecoverStartup` 继承 receipt 复用资格；首次 `StartupReconcile` 和 `RetryStartup` 都携带原始来源策略。Service 使用条件 owner token 停止，并通过新鲜探测确认已释放；Local Stop 必须回报停止且 Host 状态为 Stopped 后才重新 reconcile。未决 mutation 会在 startup 入口早退，保持现有不确定状态和 owner gate。
- **启动窗口与外围步骤**：当前 `resolve_setup` 在启动异步 Service/Core reconcile 前请求创建配置选择的 Main/Legacy 窗口；`WindowManager` 先建隐藏窗口，在 `WindowReadyReporter`（等待 settings 成功或失败）上报后显示，10 秒兜底仍保留。此顺序有意保留，以免 Core 初始化等待阻止首窗显示。并行对照发现 `core::clash::setup` 曾保留旧 connector 的固定 10 秒启动延迟；当前 `StreamsClient` 已从生命周期取得实例绑定的 API client，Core 未就绪时会自行重试，故本批移除该延迟并立即启动 streams/event receiver。另在 startup reconcile 返回后请求一次初始 proxy snapshot，映射 ref `setup_proxies`；若 Core API 尚不可读，现有 ProxiesClient 定时刷新继续负责重试。静默启动和 Main/Legacy 选择属于 Chimera 产品配置差异。
- **验证 / 覆盖**：`cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib client::core_lifecycle::tests::` **58/58 通过**，包括精确 Recovery receipt 复用、初次启动停止旧 owner、stale target 停止后重建、Local Stop 未确认时拒绝重建；`cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera-core --lib clash::ws::tests::` **12/12 通过**；`cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera-core --lib proxies_actor::tests::` **28/28 通过**；`cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib utils::resolve::` **10/10 通过**；`cargo check --offline --locked --message-format=short --manifest-path backend/Cargo.toml -p chimera --lib`、修改 Rust 文件 Rustfmt、`git diff --check` 与进度文档 Prettier 检查通过，保留项目已有 warnings。没有执行原生冷启动/桌面 WebDriver：本轮桌面检查报告 Mac 当前锁定，不能借此宣称首窗在真实桌面已可见；已有 Legacy E2E hidden-webview 前置失败仍未定根因。
- **保留差异 / 后续**：ref 在 Tauri setup 中同步等待 startup reconcile，再启动 background sources 并创建 Main；Chimera 继续异步启动，先显示 Main/Legacy，并且 Profile producers 只有在 lifecycle 返回 Ready 且 owner 非 uncertain 后才启动。这是为初始化窗口及时呈现和防止未证明 owner 时 Profile producer 介入而保留的窄差异，需继续在同一 Client/Workflow API 内收敛，不能标记为整体启动流程完成。后续需复核 startup failure/retry 时的 Profile producer 放行条件、Service preflight → reconcile → window 的启动顺序、Clash/Tauri event adapter 与 tray 首次刷新路径，并在可用解锁桌面会话中重跑首窗可见性验收。
- **回滚**：回退本阶段 `startup.rs`、`core_lifecycle/workflow.rs`、`core_lifecycle/mod.rs` 的 receipt 来源、stop-first/recovery gate 及对应测试；恢复 Clash streams 旧的 10 秒延迟需先证明其授权问题仍然存在，并恢复记录为待解决的差异。

## DIFF-069 Stage 4g：所有 settled owner plan 共用 S3/S4（实现与本地验证完成；启动整体仍 partial）

- **目标 / 验收**：ref `ApplicationWorkflow::establish` 的每一种已决 owner plan 在 S2 handoff 后都必须进入同一个 S3/S4 收尾：先精确验证仅 Recovery 可复用的运行回执；否则停止当前 owner 并取得新鲜 Stopped 证明，再应用最新已提交目标。停止错误按不确定、可重试、确定失败和已确认 NotStarted 分类，任何未确认运行 owner 上都不继续 apply。
- **非目标**：不合并 `CoreLifecycleWorkflow` 与 Profile TCC、不移动 startup/retry 的状态所有权、不新增 IPC 或持久化字段；不改变先创建 Main/Legacy 窗口再启动异步 Service/Core reconcile 的顺序。原生窗口可见性仍需解锁桌面验证。
- **基线 / 风险 / 接口**：父提交 `1a9c0b3f95c470b9dba0db76d1f00c0eee93dfcc`；只读 ref `59724e85c34abb6995c22d101c876cc06dc16576`；风险 L2（Core/Service owner handoff 与停止确认）。内部接口变化限于 `CoreLifecyclePort::select_execution_host` 与已有 receipt / retirement ports 的调用顺序；无用户 API、IPC、持久化 schema 变化。没有新增审批触发。
- **路径 / 符号映射**：ref `startup::establish` → Chimera `CoreLifecycleWorkflow::startup_reconcile_once` 的 `RetireServiceThenLocal`、`HandBackToLocal`、`AdoptService`、`Owned`；所有成功分支汇入统一的 `recovery_receipt_matches`、`confirm_local_owner_stopped` / `retire_service_startup_owner` 与 `reconcile_application`。ref `verified_running_receipt` → adapter 对 active receipt 的完整 host / revision / epoch / source hash / generation / effective hash / core kind 确认，并在 coarse status 复查后再次读取同一 receipt；ref `stop_owner` → 新鲜 Stopped state + runtime baseline 双重确认。owner transition 失败现在复用 dependency/transient/deterministic 分类。
- **验证 / 覆盖**：`cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib client::core_lifecycle::tests:: -- --test-threads=1` **67/67 通过**；`cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera-core --lib clash::ws::tests:: -- --test-threads=1` **13/13**；`cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib utils::resolve:: -- --test-threads=1` **10/10**；`cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera-core --lib proxies_actor::tests:: -- --test-threads=1` **28/28**；`cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib window::table::tests:: -- --test-threads=1` **12/12**；`cargo check --offline --locked --message-format=short --manifest-path backend/Cargo.toml -p chimera --lib` 通过。测试覆盖两种切换方向的 stop-before-apply 次序、匹配 receipt 的恢复接纳、rich receipt 在 coarse `Running` 后失效、确定 stop 失败、StopUnconfirmed、已接受但仍 Running、NotStarted 后 fresh Stopped，以及窗口 ready/fallback 状态转移。变更 Rust 文件逐文件 Rustfmt、进度文档 Prettier 与 `git diff --check` 通过。全工作区 `cargo fmt --all -- --check` 未通过：报告 `backend/tauri/vendor/tauri-plugin-wdio-webdriver/**` 和 `backend/chimera-paths/src/{unix,windows}.rs` 的既有格式差异；这些文件不在本次改动中，未顺手重排。以上均为本地隔离 Rust 测试，未证明 Service 跨进程 handoff、host 网络或窗口原生可见性。构建仅有仓库既有 warning。
- **保留差异 / 后续**：ref 在 shared `ApplicationWorkflow` / `CoreLifecycleWorkflow` 中统一持有 startup、stop intent、attempt 与 Profile mutation；Chimera 仍分属 CoreLifecycle 与 Profile TCC。将该边界合并是 shared state-ownership 架构调整，需先有维护者审阅的具体设计；当前继续保留。另保留面向初始化窗口的先开窗异步启动顺序；锁屏环境不能证明原生窗口可见，待可用桌面重验。整体启动流程不标记为完全对齐。
- **回滚**：撤回本阶段 owner-plan 共用 S3/S4 收尾、receipt 再验证与 stop 分类分支及 `ST-LIFE-01` 至 `ST-LIFE-09` 契约测试；保留 Stage 4f 的 Service ownership fence，不单独撤回其 IPC/manager 安全围栏。

## DIFF-069 Stage 4h：Service stop 错误保持 typed classification（本地修复与验证完成；启动整体仍 partial）

- **目标 / 验收**：修复独立复核指出的 P1/P2。ref `startup::stop_owner` 只把 `NotStarted` 作为幂等停止；`StopUnconfirmed`、quarantine 与其它确定失败不得因随后观察到 `Stopped` 而被当成可安全交接。Service operation 的错误 `kind`、`retryable` 与 operation id 必须保留到 Startup recovery gate。
- **非目标**：不改变 ref 固定版本、不调整 UI 首窗 / Service preflight 顺序、不移动 CoreLifecycle/Profile TCC 状态所有权；不改 IPC 或持久化 schema。原生窗口冷启动可见性仍待解锁桌面验收。
- **基线 / 风险 / 接口**：父提交 `1a9c0b3f95c470b9dba0db76d1f00c0eee93dfcc`；ref `59724e85c34abb6995c22d101c876cc06dc16576`（本轮检查时工作区干净）；风险 L2（Service/Core 停止及 owner handoff）。内部 `stop_service_core` 接受 `ControlEndpoint` 测试替身并返回含 wire `CoreError` 的错误；无用户 API、IPC 或存储接口变化。
- **路径 / 符号映射**：ref `application_workflow/startup.rs::stop_owner` → Chimera `core/actor_v2/facade.rs::{stop_service_core,service_stop_operation_error}`，终态 `OperationPhase::Failed` 保留 wire kind、retryable、operation id；`StopUnconfirmed`、`Quarantined`、`Internal` 或未分类结果 latch outcome-uncertain。ref 停止确认规则 → `core_lifecycle/workflow.rs::{confirm_local_owner_stopped,retire_service_startup_owner}`：新鲜 Stopped 状态不能覆盖确定停止错误，仅 typed `NotStarted` 可幂等继续。`CoreLifecyclePort` fake 的异常停止投影仅用于测试，不改变生产 owner adapter。
- **实际验证 / 覆盖**：`cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib core::actor_v2::facade::tests:: -- --test-threads=1` **9/9**；`cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib client::core_lifecycle::tests:: -- --test-threads=1` **70/70**；`cargo check --offline --locked --message-format=short --manifest-path backend/Cargo.toml -p chimera --lib` 通过。新增 facade 用例验证 `StopUnconfirmed` / `Quarantined` 的 uncertain latch 与 retryable 错误分类；新增生命周期用例验证服务报告 error-stopped 仍不可信、Service / Local `InvalidConfig` 停止错误不被吞掉，且 Local/Service 均不在错误后 apply；`NotStarted` 新鲜 Stopped 例外仍通过。修改 Rust 文件逐文件 Rustfmt、进度文档 Prettier 与 `git diff --check` 通过。构建输出保留仓库 warnings；全工作区 `cargo fmt --all -- --check` 仍受既有 vendor 与 `chimera-paths` 格式差异影响，未重排无关文件。以上是本地隔离 Rust 测试，不证明真实 Service 跨进程 handoff、主机网络副作用或桌面首窗可见性。
- **保留差异 / 后续**：启动目标重试和 owner S1–S4 已迁入 Chimera，但其状态所有权仍位于 `CoreLifecycleWorkflow`，ref 位于共享 `ApplicationWorkflow`，Profile TCC 与启动 stop intent 尚未统一；这是共享状态所有权架构调整，需先完成并审阅具体迁移设计。保留先请求 Main/Legacy 窗口、再异步执行 Service/Core 启动的 Chimera 顺序，以免 ref `resolve_setup` 当前同步等待 Core 而阻塞初始化窗口；在可用解锁桌面会话中验证原生显示。Legacy UI 与 agent 继续共用同一 Client/API。整体启动流程不标记为完全对齐。
- **回滚**：回退 facade 对 typed Service stop error 的保留、uncertain latch、Local/Service stop rejection 及 `ST-LIFE-10`–`ST-LIFE-12` 用例；回滚前必须保留 Stage 4f/4g 的 owner fence、receipt gate 和 stop-before-apply 保护，不能仅删除 failure gate。

## DIFF-069 Stage 4i：保持本地 stop 错误并恢复 Service quarantine（本地验证完成；启动整体仍 partial）

- **目标 / 验收**：关闭 Service stop 错误穿过本地异步 operation registry 时丢失 `CoreError` 分类的缺口；`NotStarted` 与可重试错误必须原样到达 startup gate。启动探测遇到本安装的、带错误原因的 Service `Stopped` 投影时，先要求 Service manager 执行 owner-fenced `CoreCommand::Recover` 并重新探测；manager 未确认恢复时绝不向另一 host apply。foreign Service 保留 owner 标记且不发 Recover、Stop 或 Core apply。后续独立复核还发现旧版无身份 `Recover` wire command 可绕过围栏；当前 Service 在 admission 前拒绝该命令。
- **非目标**：不改变 fixed ref、不把 Profile TCC 与 `CoreLifecycleWorkflow` 合并、不改 Core/Service state owner 或持久化格式、不调整 Main/Legacy 首窗顺序。没有真实 Service wrapper、TUN 或可见桌面会话验证。
- **基线 / 风险 / 接口**：父提交 `1a9c0b3f95c470b9dba0db76d1f00c0eee93dfcc`；ref `59724e85c34abb6995c22d101c876cc06dc16576`；风险 L2（启动 owner 与 Service quarantine recovery）。内部 `MutationFailure` 新增 typed `CoreError` 分支；新增兼容性扩展 `CoreCommandInfo::RecoverIfServiceOwner { expected_config_dir, expected_data_dir }`。Tauri 端在 probe 与 Recover 间声明安装身份，Service `submit_v2` 在 manager admission 前与当前服务身份比较；旧 `Recover` wire variant 为保持协议可解码而保留，但 Service 对它返回 `OperationConflict`、不进入 manager admission，要求调用方升级到带安装身份的变体。此拒绝是为跨进程共享 Service 增加的安全兼容边界，旧调用方的无身份恢复会失败关闭。
- **路径 / 符号映射**：ref `startup::stop_owner` 对 `NotStarted` 的幂等处理及 `ServiceEvidence::from_probe` → Chimera `CoreFacade::{LocalOperationRegistry,run_local_mutation}` 保留 Core stop 的 `kind/retryable/operation_id`，并在 `CoreLifecycleWorkflow::{recover_service_error_stop,startup_reconcile_once}` 中处理错误停止投影；ref/runtime `CoreCommand::Recover` → Chimera `ServiceEndpoint::wire_submit_request` 的安装身份封装、`chimera_service::CoreManagerService::submit_v2` 服务端身份围栏、再映射至既有 `CoreCommand::Recover`。这里有一项明确的安全收紧：ref 将所有错误原因的 `Stopped` 当作无运行 owner，而 Chimera runtime 的 quarantine 也会投影为 `Stopped { reason: Some(Error) }`，因此必须确认本安装身份在服务端仍匹配、恢复后 generation 未变化且新鲜 probe 仍证明本安装；foreign error-stopped 状态保持 `Unproven`，避免影响另一实例。
- **实际验证 / 覆盖**：本轮实际重跑 Service manager 的 config-dir 不匹配、data-dir 不匹配及旧版无身份 Recover 拒绝测试 **3/3**，`client::core_lifecycle::tests` **74/74**，`client::application_workflow::startup_state_tests` **1/1**，以及 `cargo check --offline --locked --message-format=short --manifest-path backend/Cargo.toml -p chimera --lib`。此前 startup 证据决策 **5/5**、facade **11/11**、Tauri Service endpoint **5/5**、IPC core v2 wire **6/6** 有本地通过记录。新回归覆盖 registry 保留 `NotStarted` / `QueueFull` / `BackendUnavailable` 分类、Service quarantine 恢复终态、本安装错误原因 stopped 状态先 recovery 后 Local apply、恢复失败阻止另一 host apply、foreign error-stopped 状态不触发 Recover/Stop/Local apply，以及 owner 在 recovery 窗口变化后仍禁止 Local apply。构建保留仓库现有 warnings；隔离测试不证明安装 Service 的跨进程清理、主机网络副作用或初始化窗口原生可见性。
- **保留差异 / 后续**：ref 把 startup attempt、stop intent 与 Profile mutation 放在共享 `ApplicationWorkflow` 中；Chimera 启动状态仍由 `CoreLifecycleWorkflow` 管理。该 state ownership 合并属于主要架构调整，需要先完成具体迁移设计审阅；当前先保留既有边界。窗口仍按 Chimera 的先创建请求、后异步 reconcile 顺序；桌面锁定时无法证实原生显示。故启动迁移仍为 partial，后续继续核对 ref `reestablish`、receipt/recovery target、retry/backoff 与启动入口的完整行为。
- **回滚**：回退 `MutationFailure::Core` 保留、Service quarantine recovery port / adapter / facade 路径、owner-fenced Recover wire variant / Service admission guard、`CoreStoppedWithReason` / runtime owner evidence 分支及 `ST-LIFE-13`–`ST-LIFE-16`；不得回退 Stage 4f/4g/4h 的 owner fence、stop confirmation 与 typed failure gate。

## DIFF-069 Stage 4j：启动状态收拢到共享 ApplicationWorkflow（过渡阶段完成；启动整体仍 partial）

- **目标 / 验收**：把 Startup report、重试目标和 owner 不确定标记改为同一个 `StartupWorkflowState`，并由实际生产装配中的 `ApplicationWorkflow` 持有共享句柄；Core Lifecycle 仅经同一句柄读写，不再自行创建第二份状态。保留首个 Startup report 幂等及重试调度行为。
- **非目标 / 边界**：本批不移动 startup `reestablish` 算法、不合并 Profile TCC 的 `DeferredTarget` 与 startup 重试模型、不改持久化或 IPC wire，也不改变 `Main/Legacy` 首窗时序。当前 Tauri `resolve_setup` 先同步请求所选窗口，再将 Service/Core 初始化投递到受 `AppShutdown` 跟踪的异步任务。ref `resolve_setup` 先 `block_on(startup_reconcile)` 后开窗，且源码留有不应阻塞 setup 的 TODO；为了用户提出的首窗可见性目标，这个顺序作为有证据的 Chimera 兼容修正保留，仍待解锁桌面会话的原生显示验收。
- **基线 / 风险 / 接口**：维护者授权继续迁移至 ref；父提交 `1a9c0b3f95c470b9dba0db76d1f00c0eee93dfcc`，固定且只读 `ref@59724e85c34abb6995c22d101c876cc06dc16576`。风险 L2（共享启动状态并发可见性），无配置格式或外部 wire 变更；新增内部 `StartupWorkflowState` 及 `CoreLifecycleClient::connect_application_workflow` 接线。
- **路径 / 符号映射**：ref `backend/nyanpasu-core/src/client/application_workflow/{workflow.rs,startup.rs}::{ApplicationWorkflow::{startup,live,deferred},StartupReport}` → Chimera `backend/tauri/src/client/application_workflow/{workflow.rs,startup.rs}::StartupWorkflowState`；现有 `CoreLifecycleWorkflow` 通过同一 handle 调用，而 Actor client 与 report 查询也使用这个状态对象。ref 的 startup 算法仍在 `client/core_lifecycle/workflow.rs`，所以这是状态归属的桥接阶段，不宣称整个 startup owner/attempt 已对齐。
- **实际验证**：固定 ref 启动状态迁移的既有记录为 `cargo check --offline --locked --message-format=short --manifest-path backend/Cargo.toml -p chimera --lib`、共享状态接线回归 **1/1**、生命周期集合 **74/74**、Service owner 围栏回归 **3/3** 通过。Stage 4j 独立复核未发现状态分裂、锁竞争或恢复围栏绕过；其指出的两个 P2 覆盖缺口已补上：生产 `ApplicationWorkflowClient::spawn` 接线保持同一个 report Arc，匹配本 Service config/data roots 的 Recover 到达 manager admission；另新增 `ready_generation` 单独变化的触发回归。后续复测中 Tauri lifecycle 集合 **76/76**、Service recovery 正向与拒绝路径 **5/5** 通过。`cargo fmt` 对受影响 Rust 文件检查通过，文档 Prettier 与根仓/runtime 子模块 `git diff --check` 通过。Service 用例使用临时 manager、状态用例使用隔离内存；没有实际安装 Service、系统网络、TUN、Windows/Linux runner 或原生桌面可见性验证。
- **保留差异 / 下一批**：ref 将 `startup_reconcile`、`reestablish`、显式 start/retry/recovery、`LiveAttempt` 和同一 `DeferredTarget` 生命周期直接实现于共享 `ApplicationWorkflow`。Chimera 仍将完整动作算法放在 `CoreLifecycleWorkflow`，还需将 start intent/attempt 与 Profile TCC transaction 按 ref 的单 Actor 顺序收敛，并验证重试 target 变化、Service owner 替换及启动报告保持不变；同时通过解锁桌面 UI 验证普通冷启动、silent start、Legacy 首窗和退出清理。整体状态仍是 partial。
- **回滚**：删除 `StartupWorkflowState` 和测试，恢复 `CoreLifecycleWorkflow` 内原始 report/retry/owner 字段，移除 ApplicationWorkflow 连接时共享状态的注入及 client 字段调整；保留 Stage 4i 的 owner-fenced Service Recover、旧无身份 Recover 拒绝与首窗异步顺序。

## 2026-11-15：修复同步 setup 下启动期 tracked task 的 Tokio panic（DIFF-070 Stage 4k）

- **目标 / 验收**：Tauri 同步 setup 回调及其同步调用链启动的所有 tracked task 必须显式送到 Tauri 管理的异步 runtime，不依赖调用线程已有 Tokio reactor；继续先请求创建初始化窗口，不等待 Core/Service 初始化。
- **范围 / 风险 / 接口**：非目标是调整窗口首开顺序、Profile TCC/startup ownership 或公开 IPC/API；L2（启动任务和退出排空），仅新增 crate-private scheduler 并替换 setup 调度点，没有持久化或外部接口变化；审批触发：无。
- **参考基线 / 路径映射**：固定 `ref@59724e85c34abb6995c22d101c876cc06dc16576`，工作区干净。ref 在 [`backend/tauri/src/setup.rs`](../ref/backend/tauri/src/setup.rs) 用 `tauri::async_runtime::spawn` 启动 hotkey pump；Chimera 对应的 [`backend/tauri/src/setup.rs`](../backend/tauri/src/setup.rs) 同步 setup 路径使用 `TaskTracker`，因此统一映射到 [`backend/tauri/src/utils/task.rs`](../backend/tauri/src/utils/task.rs) 的 `spawn_tracked_on_tauri_runtime`，并供 `CoreFacade::start_service_api_monitor`、`LocalRuntimeHost`、`core::clash::setup` 和窗口初始化复用。这样保留 ref 的 Tauri runtime 所有权并额外保留 Chimera 的 shutdown tracking。
- **证据 / 修复**：主仓库 push E2E run [43720593308](https://github.com/MFSGA/Chimera/actions/runs/43720593308) 先在 `local_runtime.rs:192` 发现 no-reactor。PR #638 的 Windows Desktop run [43724729524](https://github.com/MFSGA/Chimera/actions/runs/43724729524) 证明首修不完整：失败附件 `tauri-e2e-diagnostics-43724729524` 中三个启动尝试均在 `setup.rs:300` 的快捷键泵报同一 panic；附件只有 backend logs，没有截图/视频，因为 WebDriver 未就绪。已把 scheduler 提取到共享 helper，并替换 setup hotkey pump、facade service monitor 及同步 `core::clash::setup` event forwarders 的裸 `TaskTracker::spawn`；Local runtime 和 `resolve_setup` 也复用该 helper。
- **验证 / 覆盖**：`tracked_status_task_can_start_from_synchronous_setup` **1/1**、Local runtime 集合 **7/7**、window resolution/shutdown 集合 **3/3**、Clash 集合 **24/24**；`pnpm lint:clippy`、Cargo fmt、Markdown Prettier、`git diff --check` 通过。Clippy/Cargo 输出仓库中已有警告。PR #638 旧 SHA 的 Windows Desktop E2E 为失败；修订 SHA 的 Desktop E2E 与平台 CI 在 push 后待运行，不能据本地单测声称窗口已经真实显示；本批独立复核待完成。
- **保留差异 / 后续**：与 ref 相同，后台任务由 Tauri async runtime 管理；Chimera 用 `spawn_on` 保留同步 setup 与 shutdown tracking。下一批继续迁移 startup reconcile / reestablish 与 `ApplicationWorkflow` 的 attempt / deferred target 所有权。普通冷启动、silent start、Legacy 首窗及退出清理仍需桌面验证，整体启动对齐状态为 partial。
- **回滚**：恢复本批将 `TaskTracker::spawn_on` 改为 Tauri runtime handle 的调度并移除此回归与记录；这会重新引入同步 setup 无 reactor 时的 panic。

## 2026-11-15：将启动 attempt 与 deferred target 收敛到 ApplicationWorkflow（DIFF-070 Stage 4l，进行中）

- **目标 / 验收**：参照 ref 将 StartupReconcile、显式 start、自动/显式 retry、recovery 和 Service readiness follow 作为同一 `ApplicationWorkflow` 的 command 执行；让该 workflow 唯一拥有 `StartupReport`、当前 `LiveAttempt` 和唯一 `DeferredTarget`，并与 Profile TCC mutations 共用 actor admission 与顺序。未解决 attempt 不得被替换；新 target identity、stop intent、Service generation、预算耗尽和 recovery 证据变化必须保留各自 ref 语义。
- **非目标 / 边界**：不改变首次窗口请求顺序、主/legacy UI/agent 外部 API、IPC wire、配置持久化格式、Client/Service 支持或系统权限；不迁移非启动的 core binary install / Service 管理命令。保留 `CoreLifecycleWorkflow` 作为窄 core/service/runtime adapter；不以共享 Mutex 把两个 workflow 重新拼成隐藏双重状态源。
- **基线 / 映射**：工作分支从 PR #638 当前提交 `f0a95131e72d1c52fb59df2038d852a51c323cda` 叠加；ref 固定 `59724e85c34abb6995c22d101c876cc06dc16576`，工作区 clean。ref `backend/nyanpasu-core/src/client/application_workflow/{mod.rs,startup.rs,attempt.rs,mutation.rs,workflow.rs}` 的 `ApplicationWorkflowActor`, `LiveAttempt`, `DeferredTarget`, `startup_reconcile`, `reestablish` → Chimera `backend/tauri/src/client/{application_workflow,core_lifecycle}` 的对应模块。待代码阶段填写最终文件/符号表及必要 Client/Service adapter。
- **风险 / 接口 / 授权**：L2，内部 actor state ownership 与启动/恢复排队；预计只改 crate-private Rust 接口，不改用户数据或外部 schema。维护者已明确要求继续对齐启动流程，本次按 ref 的既有 owner boundary 收敛；没有 L3、host-network 或破坏性操作。
- **测试计划**：依据 ref startup 套件和现有 `core_lifecycle` / `application_workflow` 覆盖 startup idempotence、明确 start、target identity 更新、automatic retry budget、dependency wait、Service readiness generation、unresolved submission、Recovery、与 Profile TCC 同 actor 的顺序；在实现完成后运行对应 Rust tests、fmt、Clippy，并由 PR Windows Desktop E2E 验证首窗实际初始化路径。未运行的真实桌面可见性、Legacy/agent 冷启动覆盖必须标明。
- **回滚**：撤销本阶段提交，恢复 `CoreLifecycleWorkflow` 的启动算法/`StartupRetryTarget` 与单独的 Profile `DeferredTarget`，保留 Stage 4k runtime scheduler 修复和 PR #638 基线；回滚不得移动或回退 PR #638 的提交。
- **审批触发**：不更改公开 API、IPC、持久化、权限、服务安装或 host 网络。该内部 owner 合并由用户的 ref 启动迁移请求授权；若实现过程中发现必须改变外部状态所有权或 persisted-data contract，先暂停该扩展并记录为新审批范围。


### Stage 4l 实施记录（2026-11-15）：单一生产 Actor workflow，整体仍 partial

- 生产 `ApplicationWorkflowState` 现在只持有一个 `ApplicationWorkflow`；Core/Service 命令和 Profile TCC mutations 都从同一对象取出执行后再归还。它将既有 `CoreLifecycleWorkflow` 保留为 `ApplicationWorkflow.lifecycle` 适配器，连接 Profile mutation 状态会更新这个对象而不会替换生命周期 owner。若接线消息在命令运行中到达，Actor 延迟接线到 Completed 后完成。
- ref 映射：`ref/.../application_workflow/mod.rs::{ApplicationWorkflowActor,ApplicationWorkflowState}` → Chimera `backend/tauri/src/client/core_lifecycle/mod.rs`；ref `ApplicationWorkflow.lifecycle` → Chimera `backend/tauri/src/client/application_workflow/workflow.rs::ApplicationWorkflow.lifecycle`。本批未移动 startup 算法和共享的 `StartupWorkflowState`，也未统一 `LiveAttempt` / TCC 与 startup `DeferredTarget`；下一批继续迁移这些状态，不把此批称为完整 startup 对齐。
- 验证：Rust type check 通过；配置仅跳过本机缺少的 sidecar/resources bundle 登记，不是完整桌面包验证。单项 Rust 测试尝试因磁盘空间不足，在测试开始前的依赖重编译阶段失败，断言未执行。真实启动窗口、平台 E2E 和独立 review 尚未验证。此前普通 `cargo check` 也因磁盘不足失败。
- 风险 / 接口：L2，仅 `crate::client` 内部 Rust owner / 分派接口改变；IPC、持久化、Core/Service wire、首窗顺序均未改。回滚为恢复旧的两个 Actor 字段及单独取/还 workflow 的派发逻辑；不动 PR #638 基线。独立 reviewer 必须复核 panic / cancellation 归还、并发接线和启动幂等，再考虑推送。

### Stage 4m 实施记录（2026-11-15）：启动重试调度归入 ApplicationWorkflow Actor（整体仍 partial）

- **范围 / 目标**：将启动重试的 due 判断、预算准入、lease 失败后的 dependency wait 和下一次调度查询移到 Actor 持有的 `ApplicationWorkflow`。Actor completion 与 `StartupRetryDue` / `StartupRetryLeaseFailed` 消息现在只通过该对象访问启动调度；保持 retry identity、预算、due 时间、lease 和原有退避不变。
- **路径 / 符号映射**：ref `application_workflow::ApplicationWorkflow` 对其 `deferred` target 做 admission / retry scheduling → Chimera `backend/tauri/src/client/application_workflow/workflow.rs::{startup_retry_schedule,startup_retry_due,begin_startup_retry,defer_startup_retry}`；`backend/tauri/src/client/core_lifecycle/mod.rs` 的 Actor tick 和 lease-failure 分支改为调用这组接口。底层状态仍由同一 `StartupWorkflowState` handle 保持兼容访问，`CoreLifecycleWorkflow` 仍实现 probe、reestablish 与 report 更新。
- **保留差异 / 下一批**：本批只迁调度边界，未迁 `startup_reconcile_once`、`StartupReport` 写入、startup `StartupRetryTarget` 和 Profile TCC 的 `DeferredTarget` 合并，也未移动 `LiveAttempt` / recovery；因此并未宣称整个启动生命周期已对齐 ref。下一批需将 startup command/report 与 reestablish target 进一步移到 shared `ApplicationWorkflow`，同时保留 owner fence、Client/Service 适配和异步首窗顺序。
- **验证状态**：`rustfmt --edition 2024 --check`（本批三个 Rust 文件）和 `git diff --check` 通过。修正访问级别后，Cargo library check 通过；该次检查用临时 `TAURI_CONFIG` 跳过本机缺失的 sidecar/resources bundle 登记，因此不等同于桌面包验证。test-profile 依赖编译在断言运行前因空间不足终止，`cargo check --tests` 也因无法写入 rmeta 而未完成；不将它们视作通过。pnpm 文档 Prettier 尝试补齐缺失的 workspace 依赖时因磁盘空间不足而未执行。没有原生桌面启动、Legacy/Agent 冷启动或平台 E2E 证据。
- **独立复核 / 未完成覆盖**：独立静态复核曾提出两项 P2 覆盖缺口：执行中延迟挂接 workflow / 重复连接，以及生产 Actor 上真实 Profile TCC 事务路径；本轮各补一项回归。真实 StartupReconcile 测试反证了此前“`ConfirmedStoppedCore` 不会调用 baseline”的静态判断：mock 返回带变更时间的 stopped 状态，却未实现 `observe_runtime_baseline`，导致预期 `Ready` 实际降级。mock 补充权威 `Stopped` baseline 后，定向用例通过；此结论依据实际 actor 测试，不沿用未执行测试的静态推断。
- **风险 / 回滚**：L2，仅 crate-private Actor/workflow 调度接口与测试替身变化，无 IPC、持久化、首窗顺序或 Core/Service wire 变化。回滚 Stage 4m 时将调度方法放回 `CoreLifecycleWorkflow` 并恢复 Actor 对 adapter 的调用；保留 Stage 4l 的统一生产 Actor 接线与 Stage 4k 的 Tauri runtime scheduler 修复。完成 Stage 4n 后，外置 checkout 上 `cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera client::core_lifecycle::tests:: -- --test-threads=1` **79/79** 通过；rustfmt 和 `git diff --check` 通过。测试只证明模拟 Core/Service adapter 和临时 Profiles store 的 Rust Actor 流程；没有桌面 E2E、原生首窗可见性、安装 Service、系统网络或跨平台证据。不提交或推送。

### Stage 4n 实施记录（2026-11-15）：启动报告由 ApplicationWorkflow Actor 持有（整体仍 partial）

- **目标 / 非目标**：让生产 Actor 内的 `ApplicationWorkflow` 成为首次 `StartupReport` 的唯一缓存 owner；重复 StartupReconcile 和读报告走 Actor mailbox，不再从 `CoreLifecycleClient` 持有的共享 report mutex 读取。修复延迟连接方超时/取消时仍被接入的风险，并让并发测试明确等待 pending connection 登记。保持首窗先创建再异步 startup 的顺序，不改 Service/Core ownership 算法、重试目标策略、Profile 持久化、IPC wire、legacy UI、Agent 或外部 API。
- **参考基线 / 路径映射**：ref `59724e85c34abb6995c22d101c876cc06dc16576` 干净且只读。ref `backend/nyanpasu-core/src/client/application_workflow/workflow.rs::ApplicationWorkflow::startup: Option<StartupReport>` → Chimera `backend/tauri/src/client/application_workflow/workflow.rs::ApplicationWorkflow::startup_report`；ref `startup.rs::{startup_reconcile,startup_unsettled}` → Chimera 的 ApplicationWorkflow startup command 分派及 `StartupReport` 读请求 `Message::ReadStartupReport`。`CoreLifecycleWorkflow::{startup_reconcile,recover_startup,retry_startup}` 现在返回结构化报告，生产缓存由上层 app workflow 写入；service lease 失败通过 `Command::CacheStartupUnsettled` 进入同一 owner。测试专用 Direct adapter 保留自己的 test-only cache。
- **连接取消 / 错误路径**：Ractor `RpcReplyPort::is_closed` 检查延迟请求是否已超时或取消。pending operation 完成后，Actor 在 shutdown 或 reply channel 已关闭时丢弃待接入对象；否则先确认连接结果发送成功，再在同一 Actor handler 内同步 attach，避免孤儿 workflow 永久把 `mutation_connected` 置 true。测试专用登记回执只用于确定性同步 Actor mailbox，不增加外部接口。
- **风险 / 接口**：L2，修改仅限 `crate::client` 内部 workflow ownership 与 Actor message/command。新增 `ApplicationWorkflow::startup_report` 读取、`ReadStartupReport` 消息、`CacheStartupUnsettled` 命令和连接登记回执；没有 IPC、磁盘格式、用户数据、权限或 host-network 变化。startup retry target 与 owner-unproven 仍在可共享的 `StartupWorkflowState`，startup owner 算法仍在 `CoreLifecycleWorkflow`；LiveAttempt、统一 `DeferredTarget` 与 ref `reestablish` decomposition 尚未迁移，因此 startup 整体仍 partial。
- **验证 / 覆盖**：外置卷 `CARGO_TARGET_DIR=/Volumes/ChimeraStartupBuild/target` 下执行 `cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera client::core_lifecycle::tests:: -- --test-threads=1`，**79 passed、0 failed、0 ignored**。包含真实 Actor startup report 缓存/幂等、Profile TCC 临时持久化事务、active-operation connection duplicate rejection、pending connection caller cancellation 后可重新连接、Startup recovery 保留首份 report，以及 Service/Core startup owner 回归。`rustfmt --edition 2024` 与 `git diff --check` 通过；Cargo 保留已有 warnings 和 Apple linker compact-unwind 警告。没有真实桌面冷启动、原生窗口可见性、Legacy/Agent E2E、跨平台或真实 Service/TUN 验证；文档 Prettier 未运行，因为本机/外置副本未找到可执行的 workspace prettier。
- **独立审查 / 回滚**：独立 reviewer 的最新 P2 发现已通过 `reply.is_closed`、shutdown rejection 和取消回归修复；后续独立复核仍需检查本次报告 owner 移动和 connection response/attach 顺序。回滚本阶段应恢复 report Arc/Mutex compatibility handle 和无条件 pending attach，同时移除此阶段测试；保留 Stage 4l/4m owner/retry 调度与 Stage 4k runtime scheduler 修复。未提交、推送或合并。

### Stage 4o 实施记录（2026-11-15）：启动失败分类归入 ApplicationWorkflow（整体仍 partial）

- **目标 / 非目标**：将启动错误分类和 `StartupOwnerFailure` outcome 映射从 CoreLifecycle adapter 搬到 `application_workflow/startup.rs`，让它们与启动报告、owner evidence 和 retry policy 一起归于对应领域模块。只调整内部模块边界，不改错误分类语义、Core/Service owner algorithm、首窗展示次序、重试预算、Profile TCC 或外部契约；没有试图在此阶段搬完整 `reestablish`。
- **参考基线 / 路径映射**：本次只读 ref 为 `59724e85c34abb6995c22d101c876cc06dc16576`，干净且未修改。ref 把 StartupReconcile / `reestablish` 的错误判定与结果映射放在 `backend/nyanpasu-core/src/client/application_workflow/startup.rs`；Chimera 对应 helper `startup_failure_is_retryable`、`startup_failure_waits_for_dependency`、`startup_stop_error_is_idempotent` 和 `StartupOwnerFailure` 从 `backend/tauri/src/client/core_lifecycle/workflow.rs` 移到 `backend/tauri/src/client/application_workflow/startup.rs`。它们仍根据 Chimera 的 `CoreError`、Profile runtime check 和 Service lease 边界映射结果，因而这属于路径/职责收敛，不宣称算法和状态所有权已与 ref 相同。
- **接口 / 差异 / 验收**：无 IPC、持久化、公共 Rust API、Service/Core wire 或 Tauri setup 接口变化。Startup `StartupRetryTarget` 和 `owner_unproven` 仍由兼容用 `StartupWorkflowState` 共享；完整 owner/reestablish 编排仍位于 `CoreLifecycleWorkflow`，ref 的 `LiveAttempt` 与单一 `DeferredTarget` 尚未迁移。验收要求是错误映射保持一致、连接取消与启动报告恢复路径可回归，且首窗仍先创建再异步启动 reconcile。
- **风险 / 回滚 / 审批触发**：L2，只有 crate 内部模块可见性与 import 路径变化；回滚时将上述三个 helper 和 `StartupOwnerFailure` 定义移回 adapter 并恢复原引用即可。若后续要合并 Profile TCC、CoreLifecycle actor 和 AppWorkflow actor，或改变共享状态所有权/API 边界，则先提供具体设计并走架构审阅。
- **验证 / 覆盖**：`rustfmt --edition 2024` 和 `git diff --check` 通过。外置卷上实际运行 `CARGO_TARGET_DIR=/Volumes/ChimeraStartupBuild/target TMPDIR=/Volumes/ChimeraStartupBuild/tmp TAURI_CONFIG='{"bundle":{"resources":[],"externalBin":[]}}' cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera client::core_lifecycle::tests:: -- --test-threads=1`，**80 passed、0 failed、0 ignored**。本轮新增 `startup_lease_failure_caches_an_unsettled_report_and_is_idempotent`，并扩展排队超时测试，断言最终报告可查询、重放幂等；修复独立审查提出的两个 P2 覆盖缺口。Reviewer 对 Stage 4n 未发现可操作的生产正确性问题；其审查为静态审查，测试证据来自本轮实际 Cargo 运行。该 Cargo `TAURI_CONFIG` 跳过缺失 sidecar/resources 的 bundle 登记，不是桌面包或可见窗口验证；Legacy/Agent 冷启动、真实 Service/TUN 和跨平台仍未验证。文档 Prettier 未运行：本机仅发现 pnpm 可执行文件，未发现 workspace prettier。


### Stage 4p 实施记录（2026-11-15）：启动重试目标改由 ApplicationWorkflow 持有（整体仍 partial）

- **目标 / 非目标**：移除生产 Actor 与 CoreLifecycle adapter 之间共享的 `Arc<Mutex<Option<StartupRetryTarget>>>`；启动重试目标改为 `ApplicationWorkflow` 内部独占的普通状态，due 查询、预算准入、dependency wait 和生命周期命令执行都通过同一个 Actor workflow 访问。保留既有 identity、预算、退避、lease、错误分类和首窗先显示再异步启动的行为。不在本阶段重写 Core/Service owner 算法，不合并 mutation `DeferredTarget`，也不改持久化、IPC、Legacy UI、Agent 或首窗创建次序。
- **参考基线 / 路径与符号映射**：只读 `ref` SHA `59724e85c34abb6995c22d101c876cc06dc16576`，ref 工作树干净且未修改。ref `backend/nyanpasu-core/src/client/application_workflow/workflow.rs::ApplicationWorkflow::{startup,deferred}` 及 `application_workflow/mod.rs` actor retry scheduler → Chimera `backend/tauri/src/client/application_workflow/workflow.rs::ApplicationWorkflow::{startup,startup_retry_schedule,startup_retry_due,begin_startup_retry,defer_startup_retry}`。Chimera 的 `StartupWorkflowState.retry_target` 现在是由该 workflow 独占的 `Option<StartupRetryTarget>`；`core_lifecycle/workflow.rs::{startup_reconcile,recover_startup,retry_startup,execute}` 接收可变借用，仅作为现有 Core/Service 执行适配器。Profile TCC 确认已提交运行时后，由上层 workflow 清除旧 startup retry target，保持原有覆盖规则。
- **保留差异 / 下一步**：`StartupWorkflowState` 仍携带一个 `Arc<AtomicBool>` owner-unproven 信号，仅供 ApplicationWorkflow Actor 外的 Service readiness observer 读取；startup owner 判断、reestablish / Core 与 Service handoff 算法仍在 `CoreLifecycleWorkflow`，startup retry target 与 Profile mutation 的 `DeferredTarget` 尚未统一。ref 的单一 ApplicationWorkflow 状态所有权和 startup 实现路径尚未完全迁移，因此整个启动流程继续标记为 partial。主/Legacy 窗口真实可见性仍需可用桌面会话上的冷启动验收。
- **接口 / 风险 / 回滚**：风险 L2（Core/Service runtime 生命周期状态 owner 与 actor 调度）。仅 crate 内部 Rust 参数和状态所有权调整：生命周期命令的 startup 方法现在借用 ApplicationWorkflow 持有的重试目标；无 IPC、公开 API、持久化数据格式、权限或网络设置变化。回滚时恢复共享 retry-target handle 与旧 adapter 方法签名；保留 Stage 4n 的 report cache owner 与 Stage 4o 的错误分类路径。
- **验证 / 覆盖**：外置卷上执行 `cargo check --tests --offline --locked --message-format=short --manifest-path backend/Cargo.toml -p chimera`，通过（使用外置 `CARGO_TARGET_DIR` / `TMPDIR` 和 `TAURI_CONFIG` 跳过本机缺失 sidecar/resources 的 bundle 登记）。执行 `cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera client::core_lifecycle::tests:: -- --test-threads=1`，**82 passed、0 failed、0 ignored**。新增 `startup_retry_lease_failure_is_drained_after_actor_operation_returns`，让 Service lease 等待期间另一个 Core Stop 操作持有工作流，再触发 lease 失败并验证 `Completed` 归还后重排；保留 helper 单测和原有集成测试，分别覆盖 queue drain 与不消耗自动 retry budget。Rustfmt 与 `git diff --check` 检查通过。测试覆盖模拟 Core/Service adapter、ApplicationWorkflow Actor 与 Profiles 临时持久化路径；没有原生首窗可见性、Legacy/Agent 冷启动、真实 Service/TUN、桌面包、E2E 或跨平台证据。Cargo 保留仓库已有 warnings。
- **独立审查 / 修复**：Stage 4p 首轮独立审查发现 P2 竞态：重试目标被标为 Pending 后异步申请 Service lease；期间 Actor 可移走 workflow，失败消息到达时若目标 workflow 不在便会丢弃。新增 pending lease-failure 队列，在 workflow 完成归还时应用失败并重排 retry。第一版只覆盖 helper，独立审查指出它不能证明 `Completed` 的实际恢复/重排；后补 Actor 级回归，阻塞 Service lease 与 Core Stop 并观察失败消息被处理，再断言 stop 完成后 retry 获得新的未来 deadline。独立复核确认该交错被实际覆盖且移除 drain 会使测试失败；未发现新的生产正确性问题。stale identity 过滤与 shutdown timer gate 经静态检查，无专门回归。

### Maintainer-directed integration to the primary branch (2026-11-15)

- **Authorization / target**: the maintainer explicitly requested that this startup-alignment batch be integrated into the primary branch and pushed before the next migration iteration. The repository's actual primary branch is `master` (`origin/HEAD -> origin/master`), not a branch named `main`.
- **Remote baseline / integration method**: the exact `master` SHA before integration was `f511a68580e647d783edbfb6289ceb70f68259c6`; the prepared worktree at `f0a95131e72d1c52fb59df2038d852a51c323cda` was ahead by two commits with no commits behind. The signed commit `cbe53f9fafafbfa26ee70603643cb3a4761f8220` was fast-forwarded to `master` and pushed to GitHub; post-fetch confirmed `origin/master` at the same SHA. `git verify-commit` reported a good signature from key `B47A1748927B1B15`. Recovery refs `recovery/startup-workflow-pre-main-merge-20261115` and `recovery/startup-workflow-final-cbe53f9f` preserve the base and final diff in the external checkout. GitHub reported `master.protected=false` at integration time.
- **Evidence / maintainer exception**: local verification on that exact code state: Core Lifecycle tests **82/82**, `cargo check --tests`, Rustfmt, and `git diff --check`; independent review found no actionable production issue. Windows Desktop push run [43783527841](https://github.com/MFSGA/Chimera/actions/runs/43783527841) completed successfully, including the smoke suite's first-render checks for both supported windows. CI run [43783527814](https://github.com/MFSGA/Chimera/actions/runs/43783527814) is still in progress; it is not recorded as passed. This was a maintainer-directed manual integration before all CI completed, not a CI pass or substitute for review. macOS/Linux first-window visibility, full release packaging, and real Service/TUN remain unverified.
- **History / rollback**: integration is complete; tags and other PR branches were not changed. If rollback is needed, revert `cbe53f9f` with a new signed commit or restore the preserved recovery diff.

### Stage 4q 实施记录（2026-11-16）：生产启动报告与 retry target 由 ApplicationWorkflow 调度（整体仍 partial）

- **目标 / 非目标**：让生产 `ApplicationWorkflow` 直接处理首次 `StartupReconcile`、启动恢复、自动 retry、报告幂等与失败后的 retry target 更新；CoreLifecycle 保留为 Core/Service/runtime 执行适配器。本批不移动完整 owner/reestablish 算法，不改变窗口创建顺序、IPC、持久化格式、权限、Client/Service 能力、Legacy UI 或 agent API。
- **基线 / 路径与符号映射**：父提交为已推送的 `cbe53f9fafafbfa26ee70603643cb3a4761f8220`；只读 ref SHA `59724e85c34abb6995c22d101c876cc06dc16576`，ref 工作树 clean。ref `application_workflow/startup.rs::{startup_reconcile,explicit_start,retry_reestablish,recover_by_reestablishing}` 与 `workflow.rs::ApplicationWorkflow::execute` → Chimera `backend/tauri/src/client/application_workflow/{startup.rs,workflow.rs}::{ApplicationWorkflow::{startup_reconcile,recover_startup,retry_startup,execute_lifecycle},update_startup_retry_target}`。配置提交、普通 reconcile 与 core selection 后的启动 retry target 刷新也由此 ApplicationWorkflow 根据结果更新；生产调用不再把存储的 retry target 可变借给 `CoreLifecycleWorkflow::execute`。`CoreLifecycleWorkflow::startup_reconcile_once`、Service owner 探测、停止/交接与运行时 reconcile 仍留在 adapter。
- **风险 / 接口 / 数据**：L2，仅 crate 内部 Rust 调度/状态所有权变化；没有 IPC、公开 Rust API、持久化 schema、用户数据或 host network mutation 变化。窗口创建与显示顺序未改。测试专用 Direct adapter 保留旧调用兼容，仅供 `cfg(test)` 使用；生产 actor 使用 ApplicationWorkflow 的调度入口。
- **验证 / 覆盖**：外置卷 `/Volumes/ChimeraStartupBuild` 上执行 `cargo check --tests --offline --locked --message-format=short --manifest-path backend/Cargo.toml -p chimera` 通过；同卷运行 `cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera client::core_lifecycle::tests:: -- --test-threads=1`，**82 passed、0 failed、0 ignored**；对应 Rust 文件已 `rustfmt --edition 2024`，`git diff --check` 通过。Cargo 检查用 `TAURI_CONFIG` 跳过本机缺失 sidecar/resources bundle 登记，不是完整桌面包验证。既有测试覆盖 actor 启动接线、报告幂等、失败重试、retry target identity 更新和 Service lease 交错；本批未在 macOS 原生桌面运行首窗 E2E。Windows smoke 的首渲染结果来自父提交 `cbe53f9f`，不能代替本批新 SHA 的平台结果；当前 CI 对 `cbe53f9f` 尚有一个 run 仍在进行。
- **保留差异 / 下一步**：完整 `startup_reconcile_once` owner/reestablish 算法仍位于 `CoreLifecycleWorkflow`；ref 的 `LiveAttempt` 与共享 `DeferredTarget` 还未替代 Chimera 的 `StartupRetryTarget`，`owner_unproven` 仍通过 atomic signal 给 actor 外的 Service readiness observer 使用。普通/静默启动、macOS/Linux 原生首窗、Legacy/agent 冷启动、退出清理与实际 Service/TUN 尚无本批验证，所以启动整体继续标为 partial。
- **回滚**：撤销本阶段提交并恢复 `CoreLifecycleWorkflow` 启动调度入口及 retry target 更新；保留已推送的 `cbe53f9f`、Stage 4k Tauri runtime scheduler 修复与 recovery refs。

### Stage 4r 实施记录（2026-11-16）：macOS 首窗激活策略对齐 ref（启动整体仍 partial）

- **目标 / 非目标**：让 macOS 进程启动时先采用 ref 的 Accessory policy，并由首个主界面窗口就绪时切回 Regular、激活应用后显示窗口。保留 Chimera 的 Main/Legacy 选择，以及先创建窗口、后台启动 Core 的时序；不宣称已确认或修复 macOS E2E hidden WebView 的原因。
- **基线 / 路径映射**：ref SHA `59724e85c34abb6995c22d101c876cc06dc16576`，工作树 clean。ref `utils/resolve.rs::resolve_setup` 设置 `ActivationPolicy::Accessory`、`window/manager.rs::apply` 在首次主窗口显示时调用 `utils::dock::macos::show_dock_icon`；Chimera 在 `utils/resolve.rs::resolve_setup` 增加同样的 Accessory 设置，并保留 `window/manager.rs::apply` 在 Main/Legacy 首次显示前恢复 Dock/激活应用的适配。
- **风险 / 接口 / 回滚**：L2（macOS 原生应用激活策略）；不涉及 IPC、数据、Core/Service owner 或启动 API。回滚为移除 `resolve_setup` 新增的 Accessory 设置行。
- **验证 / 覆盖**：外置卷执行 `cargo check --tests --offline --locked --message-format=short --manifest-path backend/Cargo.toml -p chimera` 通过；Rustfmt 与 `git diff --check` 通过。`window::table::tests` 12/12 和 `utils::resolve::startup_window_config_tests` 2/2 通过；它们验证 ready/watchdog 状态和 Main/Legacy/silent-start 选择，不证明 macOS 原生前台可见性。Cargo 使用临时 `TAURI_CONFIG` 跳过本机缺失的 bundle sidecar/resources 登记，不是完整桌面包验证。没有 macOS 前台桌面会话 E2E；已有 hidden WebView 失败证据仍不能定因。
- **下一步 / 保留差异**：首窗可见性需在激活的 macOS 桌面会话验证。启动主流程的 `startup_reconcile_once`、完整 owner/reestablish 算法和 `LiveAttempt`/共享 `DeferredTarget` 仍需继续迁入对齐 ref 的 ApplicationWorkflow；本阶段不改变窗口与后台 startup reconcile 的先后。

### Stage 4s 实施记录（2026-11-16）：将启动 owner 决策迁入 ApplicationWorkflow（启动整体仍 partial）

- **目标 / 验收**：将首次启动的 Core owner 探测、Service/Core handoff 决策、owner 停止确认、recovery receipt 判断、Service 失败后的 Local fallback 和最终 runtime reconcile 编排迁入 `ApplicationWorkflow`。CoreLifecycle 只提供带类型的 Core/Service 读取与执行适配口；保留原 Service owner fence、fresh status/probe 二次确认和失败分类。生产 `StartupReconcile`、启动恢复和 retry 都调用迁入后的 ApplicationWorkflow 路径。
- **非目标**：本批不改变主/Legacy 首窗选择和窗口先显示、Core 后台启动顺序；不改 IPC、持久化格式、Client/Service 功能、agent 或用户配置；暂不合并 `StartupRetryTarget` 与 Profile `DeferredTarget`，也不引入 ref 的完整 `LiveAttempt`/`AttemptOrigin` 状态机。macOS 原生首窗可见性仍需激活桌面会话验证。
- **基线 / 风险 / 接口**：父提交 `b282c47c576ec9d231766e6d2dae0ce42112fed0`（已在 `origin/master`）；ref 固定于 `59724e85c34abb6995c22d101c876cc06dc16576`，引用工作树 clean。风险 L2（共享 startup orchestration 的状态所有权变化）；只有 crate 内 Rust 调用签名变化，不改 IPC、公开 API 或落盘数据。该迁移处于维护者明确授权的 startup/ref 对齐范围内。
- **路径 / 符号映射**：ref `backend/nyanpasu-core/src/client/application_workflow/startup.rs::{startup_reconcile, reestablish, establish}` 与 `workflow.rs::ApplicationWorkflow` → Chimera `backend/tauri/src/client/application_workflow/startup.rs::ApplicationWorkflow::{startup_reconcile_once, finish_startup_report, startup_failure}`。`CoreLifecycleWorkflow` 仍提供 `startup_core_status`、`service_evidence`、owner retirement/confirmation、receipt revalidation、host selection 和 runtime reconcile adapter；主/Legacy 窗口与启动任务接线未触碰。测试专用 Direct adapter 现在委托给 ApplicationWorkflow，并同步旧测试 target 状态。
- **接受标准 / 实际验证**：外置构建卷 `/Volumes/ChimeraStartupBuild` 上执行 `cargo check --tests --offline --locked --message-format=short --manifest-path backend/Cargo.toml -p chimera` 通过（临时 `TAURI_CONFIG` 只跳过本机缺失 bundle resources/sidecar 的登记；不是完整桌面包验证）；`client::core_lifecycle::tests::` **83/83** 与 `client::application_workflow::startup::` **8/8** 通过。新增 Local→Service 接管后 Service reconcile 失败的回归用例：修复前测试以 `startup Core reconciliation failed` 降级而失败，修复后通过并验证 fallback 后 owner 为 Local；Rustfmt、Markdown Prettier 和 `git diff --check` 均通过。检查/测试输出仍含仓库既有 warnings。没有运行桌面 UI E2E、未做 Service/TUN 宿主变更、也没有证明原生 macOS WebView 已可见。
- **独立复审修复**：复审发现 Service fallback 原先依据初始 owner 判断；Local owner 通过 `AdoptService` 交接后，首次 Service reconcile 失败会漏掉 fallback。现在失败后重新读取当前 Core owner，只在仍确认由 Service 选中时执行带 owner fence 的释放和 Local fallback，匹配 ref `startup.rs::reestablish` 的交接后 owner 判断。仍无法读取或 outcome uncertain 时不发起额外 owner mutation。
- **剩余差异 / 回滚 / 审批触发**：ref 的 `LiveAttempt`、共享 `DeferredTarget`、Lifecycle/ExplicitStart/Recovery 的统一 reestablish 生命周期尚未迁入；Chimera 的 `owner_unproven` 仍是面向 Service readiness observer 的共享 atomic。下一步需按 ref attempt/target 状态重新审查重试 target 变化、interruption recovery 和配置保存语义，再迁移其完整关联路径。回滚时恢复 `CoreLifecycleWorkflow::startup_reconcile_once` 及其 report/failure helpers，并撤回对应 ApplicationWorkflow adapter 调用；保留父提交的 macOS activation-policy 对齐。没有扩大到其它分支、标签、发行或宿主网络改动；若后续触及持久化、权限边界或其他共享分支，再按其范围单独审查授权。

### Stage 4t 实施记录（2026-11-16）：共享 DeferredTarget 承接启动重试（启动整体仍 partial）

- **目标 / 非目标**：将启动重试与 Profile TCC 重试纳入同一个 `ApplicationWorkflow.deferred` 状态；使用 ref 的 `TargetOrigin`、`ReestablishCause`、identity、attempt/wait 预算、health 与 deadline 结构。保留 Chimera TCC 所需的 decision handle、baseline 和 domain 元数据；保留 Direct CoreLifecycle 测试夹层。此次不迁移 ref 的 `LiveAttempt`/`AttemptOrigin` 中断恢复状态机，不改变窗口创建顺序、IPC、持久化格式、Core/Service 权限边界、Client/Service 能力、Legacy UI 或 Agent API。
- **基线 / 接口 / 风险**：父提交 `efff6f34180a3c8bee4b0021e3c01fe80468409d`；只读 ref SHA `59724e85c34abb6995c22d101c876cc06dc16576`，ref 工作树干净。风险 L2（Core/Service 重试预算及 Profile 事务恢复状态共享）。仅改变 crate 内部 Rust 状态与调用签名，无 IPC、公开 API 或用户数据格式变化。由维护者此前明确授权的 startup/ref 对齐任务覆盖；未触及未授权分支、标签、发行或宿主网络。
- **ref 路径与符号映射**：ref `backend/nyanpasu-core/src/client/application_workflow/{workflow.rs,mutation.rs,startup.rs}::{ApplicationWorkflow.deferred,DeferredTarget,TargetOrigin,ReestablishCause,open_reestablish_target,update/retry scheduling}` → Chimera `backend/tauri/src/client/application_workflow/{workflow.rs,mutation.rs,startup.rs}::{ApplicationWorkflow.deferred,DeferredTarget,TargetOrigin,ReestablishCause,update_reestablish_target}`。ref 的共享状态承载启动及 mutation target；Chimera 将原 Profile TCC 专属 `digest/baseline/domain/decision` 放入 `MutationDeferredContext`，`is_mutation` / `is_reestablish` 将 actor 重试入口按 origin 隔离。`tcc.rs::recover_previous` 只把 mutation context 当作 Profile 回滚基线，避免误用启动 target。空 TCC target 不再覆盖已建立的 startup target；两侧同时存在 target 时连接会返回错误而不丢弃任一状态。`StartupRetryTarget` 仅保留在 `cfg(test)` 的 Direct adapter 兼容路径，生产路径不再持有第二份启动 retry target。
- **验收 / 实际结果**：外置卷 `/Volumes/ChimeraStartupBuild` 上 `cargo check --manifest-path backend/Cargo.toml -p chimera --tests --offline --locked --message-format=short` 通过；`client::application_workflow::` **34/34**、`client::core_lifecycle::tests::` **83/83** 通过，包含新共享 target 的 attempt/dependency-wait 用例和现有 Service lease、启动失败回退、TCC 恢复用例。测试通过模拟 adapter/隔离 Profile 数据，不证明真实 Service/TUN 或可见桌面启动。已执行 scoped Rustfmt 与 `git diff --check`；Cargo 输出包含项目既有 warnings。未运行桌面 E2E、macOS 原生首窗可见性、Windows/Linux 或 release bundle。
- **存储处理 / 回滚 / 后续**：因系统空间需求，通过 `cargo clean --manifest-path backend/Cargo.toml` 清除另一任务工作树内可重建的 1.3 GiB `backend/target` 缓存；外置构建卷继续承载 Cargo target，源码和用户数据未清理。回滚时恢复 Stage 4s 的旧 `DeferredTarget`/`StartupRetryTarget` 字段及其 TCC 与 ApplicationWorkflow 调用；不需要用户数据转换。下一阶段迁移 `LiveAttempt`/`AttemptOrigin` 的 unresolved outcome、explicit recovery 和 attempt conclusion；在此之前启动整体保持 partial。
- **独立审查 / CI**：本批尚无独立 reviewer 对新 diff 的审查；需在集成说明中如实保留该缺口。前一 Stage 4s SHA 的 CI 结果不转移到本批新 SHA；本批推送后按新 commit 记录 CI pending/结果，不能称旧 SHA 检查覆盖新代码。

### Stage 4t 维护者指定集成（2026-11-16）

- **授权 / 目标**：维护者在本任务中明确要求启动对齐迭代先并入主分支并推送；项目主分支是 `master`（`origin/HEAD -> origin/master`）。本次按该范围更新 `master`，未改标签、其它分支或 PR。
- **远端基线 / 集成**：推送前重新 fetch，`origin/master` 精确为 `efff6f34180a3c8bee4b0021e3c01fe80468409d`；`master` 未保护（GitHub API `.protected=false`），干净本地 checkout 仅 fast-forward 到签名提交 `7cf8b09a5f5a06d07b8059dfadd8b26bf336b627`，然后推送。任务分支 `codex/startup-workflow-followup-20261116` 与 `master` 均指向该 SHA；GitHub commit verification 为 `verified=true`。本地 recovery refs `recovery/startup-followup-master-before-stage4t-20261116` 和 `recovery/startup-followup-stage4t-20261116` 保存集成前基线与 Stage 4t 完整结果。
- **实际验证 / 限制**：本地 check、Clippy、34 个 ApplicationWorkflow 和 83 个 CoreLifecycle 单测通过，格式检查通过。新 SHA 的 [CI run 43800083538](https://github.com/MFSGA/Chimera/actions/runs/43800083538) 与 [Tauri E2E run 43800083628](https://github.com/MFSGA/Chimera/actions/runs/43800083628) 推送后均为 `in_progress`；不声称通过，也不以父 SHA 的结果代替。没有独立 reviewer 审查本批 diff；这是维护者指定的手工集成例外，不是 CI 或独立审查通过。macOS 原生首窗可见性、桌面用户流程、Windows/Linux 与 Service/TUN 仍未覆盖。
- **回滚**：在 `master` 上对 `7cf8b09a` 做新的签名 revert；不重置或强推共享主分支。上述本地 recovery refs 保留完整实现和原基线。

### Stage 4u 实施记录（2026-11-16）：单一 LiveAttempt 恢复事实源（启动整体仍 partial）

- **目标 / 非目标 / 验收**：将运行中的或未收敛的 Profile TCC、启动 reestablish 和 Core lifecycle 操作记录到 `ApplicationWorkflow.live`；恢复方向从 Actor 持有的 `LiveAttempt`/`AttemptOrigin` 读取，actor 在未解决时隔离新的运行时操作。保留纯 source-only 应用设置写入及 shutdown。非目标是改动主/Legacy 首窗时序、IPC、持久化、Client/Service 支持、agent API，或宣称整个启动工作流已达到 ref。
- **基线 / 风险 / 接口**：父提交 `6626fe03b32a4d4820a12306cc93f7aada7d5009`；只读 ref `59724e85c34abb6995c22d101c876cc06dc16576`，实际检查路径 `/Users/adam/.codex/worktrees/startup-workflow-ref-alignment/Chimera/ref` 的 HEAD 与 status；基线干净且 SHA 未变。风险 L2（共享 Core/Service 与 Profile 事务恢复状态归属）；改变仅限 crate 内 Rust 字段、调用签名与 actor admission，不改公开接口、IPC 或落盘数据。当前批准范围是维护者持续进行的 startup/ref 对齐迭代；没有新的权限、系统网络或数据迁移触发点。
- **ref 路径 / 符号映射**：ref `backend/nyanpasu-core/src/client/application_workflow/{attempt.rs,workflow.rs,tcc.rs,startup.rs}::{LiveAttempt,AttemptOrigin,AttemptStage,ApplicationWorkflow.live,recover}` → Chimera `backend/tauri/src/client/application_workflow/{attempt.rs,workflow.rs,tcc.rs,startup.rs}::{LiveAttempt,AttemptOrigin,AttemptStage,ApplicationWorkflow.live,recover_previous}`，并在 `backend/tauri/src/client/core_lifecycle/mod.rs::reject_pending_for_application_recovery` 接入隔离。Profile 决策与 baseline/receipt 记入 attempt；启动重试的 `DeferredTarget` 在重试执行期间从同一 live attempt 承载；`RecoveryContext` 现在只是面向既有状态消费者的投影，不再作为恢复决策来源。
- **修复 / 保留差异**：TCC、startup 与 lifecycle 路径在完成时清除 live slot；不确定结果保留 unresolved 原因，新的 ApplicationWorkflow gate 只放行匹配的显式恢复、shutdown，以及原有无 runtime 副作用的设置写入/启动报告缓存。SourceDecision、Profile Mutation 和 lifecycle 的普通 `Reconcile` 可通过匹配的 `RecoverCore` 执行当前持久化配置协调；只有 source 决策已达到 `Committed` 或 `Aborted { Restored }` 时，成功的当前配置协调才清除旧 attempt 与 deferred target。`Undecided` 与 `Aborted { NeedsRecovery }` 保留恢复 owner，避免事务稍后提交或仍需资源恢复时丢失同步责任。成功协调当前配置后不回滚旧 baseline；若 TCC 变更边界意外遇到 unresolved 的已提交目标，则拒绝并要求显式协调；启动 reestablish 仍须使用 `RecoverStartup`。忙碌、已有排队工作或 unresolved attempt 会推迟 deferred Profile retry；两个排队 gate 拒绝已开始的 retry 时会恢复未来 deadline，不扣减 retry budget。shutdown 在 workflow 执行时保留原 attempt。CoreLifecycle 原有 lower-level `uncertain` gate 仍优先生效；当它无法证明操作结果时，现有策略继续阻断恢复前的运行时命令，不由本批绕过。首轮 Client 回归发现纯设置保存被新 gate 拦截，后续仅放行 command 仍会覆盖原 attempt；两部分已修复并记录在 `LESSON-20261116-002`。独立复核又发现 deferred retry 丢失、通用 reconcile 不可达以及 shutdown 覆盖 unresolved attempt；均已修复并以实际 actor 队列/恢复入口回归覆盖，记录于 `LESSON-20261116-003`。第二轮复核发现已提交 Profile target 在成功 reconcile 后回滚到旧 baseline；修复后由包含持久化 Profiles snapshot 的 actor 测试验证最终 active Profiles 仍与 source 一致。复核还发现 Undecided source attempt 会被清除；现在未决和 `NeedsRecovery` 决策会保留 live owner，并由阻塞 source persistence 的 actor 回归覆盖，记录于 `LESSON-20261116-004`。Chimera 仍用自己的 `RuntimeApplyReceipt`、事务恢复与 `RecoveryContext` 投影；ref 的 `AppliedVerdict`（product 与 Service release 确认义务）尚未完整迁入。ref 将 lifecycle action 结算和 reestablish recovery 接入同一 routine 的部分也仍待继续对照；后续先统一两层 outcome evidence，再迁入 AppliedVerdict/confirm replay，之后重跑 source commit、restore 和 host handoff 回归。故本批不宣称完整 interruption recovery 对齐。
- **测试计划 / 当前结果**：`cargo check --manifest-path backend/Cargo.toml -p chimera --tests --offline --locked --message-format=short` 与 `cargo clippy --manifest-path backend/Cargo.toml -p chimera --all-targets --offline --locked` 均退出 0，输出含仓库既有 warnings。首次 `cargo test --manifest-path backend/Cargo.toml -p chimera --tests --offline --locked client:: -- --test-threads=1` 为 **389 通过、1 失败**，`preexisting_uncertain_runtime_blocks_startup_mutations` 检出纯设置写入与原尝试保留回归；修正后该 actor 回归 **1/1**，attempt 单测 **4/4**，`unresolved_live_attempt_isolated_until_explicit_reconcile_recovery` 与 `explicit_reconcile_keeps_committed_profiles_active_after_deferred_retry_recovery` 两个 actor 回归各 **1/1**；之后增加 `reconcile_preserves_an_undecided_profile_source_attempt_until_commit` 与 `reconcile_preserves_a_profile_source_attempt_after_resource_recovery_failure`，两项定向回归各 **1/1**。最终 `cargo test --manifest-path backend/Cargo.toml -p chimera --lib --offline --locked client:: -- --test-threads=1` 完整 `client::` 集合 **395/395 通过**。`cargo fmt --manifest-path backend/Cargo.toml --package chimera -- --check`、Markdown Prettier 与 `git diff --check` 通过；全 workspace `cargo fmt --manifest-path backend/Cargo.toml -- --check` 被未改动的 WDIO vendor 和 `chimera-paths` 源文件现有格式差异挡住，本批没有格式化那些无关文件。测试使用本地 fake Core/Service 与隔离数据；未运行原生桌面 E2E、首窗可见性、Windows/Linux、真实 Service/TUN 或打包验证。Cargo 配置通过 `TAURI_CONFIG` 跳过本机缺失的 bundle resource/sidecar 登记，故 check/clippy 不是完整桌面打包证明。第二轮独立复核已完成，未发现 blocker/P2/P3；reviewer 未重复跑测试。推送的代码 SHA `704079c34bb398ef41959f818b903075f2e33c51` 的 [GitHub CI run 43823471577](https://github.com/MFSGA/Chimera/actions/runs/43823471577) 与 [Tauri E2E run 43823471418](https://github.com/MFSGA/Chimera/actions/runs/43823471418) 已启动，记录时均为 `in_progress`，不声称通过。
- **回滚**：以签名 revert 撤销本批代码和文档提交即可，不触及用户配置或外部状态；保留父提交中的共享 DeferredTarget、startup owner/首窗顺序及 Service owner fence。CI 必须针对本批替代 SHA 重新运行；前轮结果不转移。

### Stage 4v 实施记录（2026-11-16）：保存并重放已提交的 Profile Try 结算义务（启动整体仍 partial）

- **目标 / 非目标 / 验收**：参照 ref 的 `AppliedVerdict`，在 source transaction 收到 ACK 前把已应用的 receipt、runtime product 和后续确认义务记入 `LiveAttempt`。若 source 已提交但 product 确认或 Service owner 释放中断，下一次显式恢复必须先用原 verdict 幂等完成，不可丢失已提交结果或重复应用 Core。保留用户确认的 Main/Legacy 原生首窗可见现状和既有“先建窗、后异步启动”顺序；不改窗口、IPC、持久化 schema、主/Legacy UI、Agent API、Core/Service wire、权限或网络设置。
- **基线 / 路径与符号映射**：初始同步基线为 `db12c88bcb7faf30f694b3a0a4325202d5710eba`；工作期间远端新增 heap-profiling 提交 `7d50e4ca42d310d84855fa56595e2a9df4c3160d`，本地 `master` 已安全 fast-forward 到该 SHA 并恢复本轮改动；最终本地验证以 `7d50e4ca42d310d84855fa56595e2a9df4c3160d` 为父基线，已验证代码随后记录在提交 `b3a235300f110d24b1ab5c98f002b20166ba3c14`。只读 ref `59724e85c34abb6995c22d101c876cc06dc16576` 干净。ref `backend/nyanpasu-core/src/client/application_workflow/attempt.rs::{AppliedVerdict,LiveAttempt.verdict}` 及 `tcc.rs::{record_verdict,finish_applied,recover}` → Chimera `backend/tauri/src/client/application_workflow/attempt.rs::{AppliedVerdict,LiveAttempt.verdict,record_attempt_verdict}` 和 `tcc.rs::{finish_applied,recover_applied}`；ref `tcc.rs::confirm_profile_runtime` 的 committed-product 确认映射到 Chimera `backend/tauri/src/core/actor_v2/local_runtime.rs::confirm_profile_runtime` 的候选文件提升与 snapshot 发布。ref 按 `impact::leaves_service_mode(change)` 标记离开 Service 的义务；Chimera 的该 TCC 入口只处理 Profile 变更，因此使用经确认的旧 receipt `Service` → 新 receipt `Local` 迁移作为确认条件。TCC 的 actor 内收尾通过 crate-private `ProfileRuntime::try_release_service_owner`、`CoreLifecycleWorkflow::try_release_service_owner`、`ServiceLifecyclePort::try_begin_transition` 和 `CoreFacade::try_begin_service_transition` 取非等待式 transition lease；`ServiceCommandActor::try_reap_finished` 只回收已完成 helper、不重做 Service probe。之后调用已有 owner fence 与 fresh probe；启动命令仍使用自己在进入 actor 前取得的 lease 与原 `release_service_owner`。
- **接口 / 风险**：L2（Profile runtime 的确认与 Core/Service 恢复边界）。新增字段和 trait 方法均为 crate-private；不改变落盘格式或对外 API。确认候选时校验 product bytes、revision、Core identity 和当前确认 receipt。中断发生在候选文件消费后，LocalRuntime 会从已验证的 committed product 重建候选文件；恢复会先观察当前 runtime，只有需要时才恢复 committed target，并在 receipt revision 已变化时使用当前 receipt 的 artifact，之后重放 product 发布和 owner fence。
- **失败路径 / 保留差异**：product 发布或 Service owner release 失败都会保留 committed verdict 与 unresolved attempt，新运行时 mutation 被原有恢复 gate 阻止，后续恢复会重试确认。actor 内 TCC 收尾不会等待 host transition lease：若别的 Service/health transition 已持有 lease，或低层 helper 仍在运行/Service helper lane 忙，就快速返回可恢复错误，让 actor 先归还 workflow；下一次尝试会用非等待 `try_reap_finished()` 回收已完成 helper（不做额外 probe），因此不会因完成后仍残留的 `outstanding` 标志永久卡住。没有在 `try_lock` 路径调用可能重新 probe 的 `command_settled()`。这样不形成“持有 lease 的请求排在等待同一 actor 的 TCC 后面”的环形等待。获取 lease 成功时，owner fence 与 fresh Service probes 都在同一 lease 生命周期内；probe 仍使用现有有界 Service command timeout。ref 对 source product 发布失败采用可重试 degradation，Chimera 的 Profile product promotion 失败会保留 recovery attempt；继续保留该既有 adapter 错误模型，不宣称整体 TCC/lifecycle recovery 已完全等同 ref。
- **测试计划 / 实际结果**：依据 `docs/testing/README.md`、`docs/testing/upstream-evidence.md` 和 `docs/testing/test-contract-template.md` 检查测试边界。`cargo test --manifest-path backend/Cargo.toml -p chimera-core --lib --offline --locked actor_v2::service_actor::tests::timed_out_helper_refuses_overlapping_mutation_and_false_stop_proof -- --test-threads=1` **1/1**，验证已完成 helper 可被回收且不额外 probe；完整 facade 集合 `core::actor_v2::facade::tests::` **12/12**（含 lease 忙碌及 release helper 后不经 `command_settled()` 即由 TCC try-path 重试成功）；TCC 集合 `client::application_workflow::tcc::tests::` **13/13**；LocalRuntime replay **1/1**；完整 `client::` 集合 **396/396**。`cargo check --manifest-path backend/Cargo.toml -p chimera -p chimera-core --tests --offline --locked --message-format=short` 与相同包的 `cargo clippy --all-targets --offline --locked` 均退出 0。`cargo fmt --manifest-path backend/Cargo.toml --package chimera --package chimera-core --check`、Markdown Prettier 与 `git diff --check` 通过。首次定向测试编译发现缺少 `anyhow::Context` 导入，修复后以上目标、Client 集合、check 与 Clippy 均重新执行成功；见 `LESSON-20261116-006`。输出仍含仓库既有 warnings 和 Apple linker compact-unwind 提示。
- **未覆盖 / 当前结论**：本轮没有改窗口路径；“首窗可见”是维护者在本对话确认的观察，不是本地桌面 E2E 证据。没有运行原生冷启动、Legacy/Agent 桌面流程、Windows/Linux、完整 bundle、真实 Service/TUN 或系统网络验证。改动仅经 fake Core/Service 与隔离 Profiles store 测试；ref 对齐及启动流程整体仍 partial。
- **独立审查 / 集成 / 回滚 / 审批触发**：独立 reviewer 首轮发现 Service owner release 未持有 host transition lease（P2），修复为非等待式 lease acquisition 后又发现 stale Service helper 可能使后续恢复一直被拒（P2）；现已由非等待式已完成 helper reaper 与“无需 command_settled 即可重试”回归修复。最终独立复核未发现 blocker/P1/P2/P3，reviewer 未重跑本地测试。用户授权的主分支迭代以签名提交 `b3a235300f110d24b1ab5c98f002b20166ba3c14` 推送到 `master`；GitHub commit verification 为 `verified=true`、`reason=valid`，父提交为同步后的 `7d50e4ca42d310d84855fa56595e2a9df4c3160d`。该 SHA 的 [CI run 43878006256](https://github.com/MFSGA/Chimera/actions/runs/43878006256) 记录时为 `pending`，[Tauri E2E run 43878006485](https://github.com/MFSGA/Chimera/actions/runs/43878006485) 为 `in_progress`；均不声称通过。回滚使用新的签名 revert 撤销本阶段代码和进度记录，保留 Stage 4u 的 LiveAttempt、共享 DeferredTarget、owner fence 和窗口顺序；无用户数据转换。无新的审批触发点；本次未改标签、其它分支或 PR。

### Stage 4w 实施记录（2026-11-18）：对齐 Service endpoint 有界恢复（启动整体仍 partial）

- **目标 / 非目标**：在现有 `CoreLifecycle` Actor、Service transition lease 和 Core owner 边界内迁入 ref 的 Service endpoint 恢复策略：最多 3 次 endpoint 重连、尝试之间至少 5 秒、不可重试错误暂停；通过 ref `CoreFacade::recover_service_endpoint` 将新 endpoint 重新接回同一 Core router。保持 Main/Legacy 初始化窗口创建与激活顺序（维护者已确认首窗可见）；不改变 Core/Service IPC、持久化配置、公开应用 API、TUN/系统代理、Profile TCC 状态所有权、Legacy UI 或 Agent。ref 的 RPC 授权改造和 OpenDAL 配置同步与本批启动范围无关，不迁移。
- **基线 / 风险**：`master` 与 `origin/master` 均为 `02eca617effbecfffbc99a2b43b0eda5a5ad4d13`。同步前 `ref/` gitlink/HEAD 为 `dd42ab212bcab4c61601a8c6922b26231609ba49`，本轮更新到上游 `main` `c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`；以 SSH `git ls-remote` 核对 SHA、完整 fetch 并验证为快进。`ref/` 工作区及子模块干净；嵌套 runtime `3854f2c06331be0b36bb69adc59708802f661e57`，enable-loopback `f1b9f8693a328ca0a6094951035b81fe302b0848`。启动恢复源码在这 32 个上游提交中未变化。风险 L2（Service/Core 运行时 endpoint 恢复）。
- **ref → Chimera 路径 / 符号**：`ref/backend/nyanpasu-core/src/client/core_lifecycle/workflow.rs::{ServiceRecovery,recovery_due,recover_service_endpoint,finish_service_recovery,follow_host}`、`client/reconciler/mod.rs::Message::Tick` → `backend/tauri/src/client/core_lifecycle/workflow.rs::ServiceRecovery`、`ApplicationWorkflowActor::ServiceRecoveryTick`；ref `backend/nyanpasu-core/src/actor_v2/facade.rs::CoreFacade::recover_service_endpoint` → 共用 `backend/chimera-core/src/actor_v2/facade.rs::CoreFacade::recover_service_endpoint`，经 Tauri `LegacyCoreBridge` 调用。生产路径不再丢弃 `ServiceActor::recover_endpoint()` 返回的 endpoint，而由共享 Core router 接管；旧的 ServiceTransition 回退只保留给无共享 recovery facade 的兼容 fixture。恢复提交前仍先取得 Service transition lease。
- **实现**：Actor 每 5 秒检查 router 的已发布状态；只在 owner 为 Service、router 明确为 Service/Degraded、策略仍 armed 且间隔到期时排入恢复。成功或失败完成后至少等待 5 秒再试，累计最多 3 次；terminal error 暂停，显式 Start/Restart 或重新进入 Service owner 重新 arm，Stop/Uninstall 与 handback Local 抑制恢复。Service readiness follow-up 在恢复仍 armed 且 endpoint degraded 时不抢先 handback Local。请求在 startup owner 暂未确认时仅允许走这条有 router degraded 证据且持有 transition lease 的 endpoint 恢复；Core 结果不确定、shutdown、未决 ApplicationWorkflow attempt 仍阻止恢复，普通 runtime mutation 仍受 owner gate 限制。共享 recovery facade 在新尝试前调用 `consume_settled_action()`；前一操作仍未 settle 时不发起重连且不消耗次数。实际恢复成功后请求一次现有 runtime 协调；若 startup owner 仍未确认，则使用共享的去重调度器，在重新取得 Service transition lease 后排入 `RecoverStartup`；暂时拿不到该 lease 时以 5 秒间隔最多重试三次，仅重试排队阶段、不额外消耗启动业务尝试。lease 调用本身延续现有异步等待语义；三次上限仅统计其返回错误的重试次数。
- **受影响接口 / 保留差异**：只改 crate-private Rust ports、adapter、CoreLifecycle workflow 与 actor 内部消息；无 IPC/schema/生成绑定变化。Main/Legacy 首窗流程、Agent 共用 API、配置落盘、系统网络流程未改。代码实现了该恢复策略，但没有本机真实 Service/TUN 或桌面恢复证据，因此本切片与整体 startup/ref 对齐状态均保持 partial。
- **检查计划 / 实际结果**：本批没有新增或执行单元测试/E2E。最新 `cargo check --manifest-path backend/Cargo.toml -p chimera -p chimera-core --offline --locked --message-format=short` 与相同包的 `cargo check --tests --offline --locked --message-format=short` 均退出 0（后者只编译测试目标，未执行断言）。`cargo fmt --manifest-path backend/Cargo.toml --package chimera -- --check`、文档 Prettier 和 `git diff --check` 均退出 0。编译输出含仓库既有 warnings。静态 reviewer 发现的恢复重试、readiness handback、runtime resync、pending action settlement 与 owner 未确认时未排入 `RecoverStartup` 问题均已修复；独立复核确认 lease 重试为最多三次、间隔五秒，且不会重复执行 `RecoverStartup`。未覆盖 Service runtime 与桌面行为测试。
- **未覆盖 / 当前结论**：没有运行原生冷启动、Main/Legacy/Agent 桌面流程、首窗 E2E、Windows/Linux、完整 bundle、真实 Service/TUN 或系统网络验证。首窗可见性是维护者在本对话确认的观察，本批未改窗口顺序。编译与格式检查不能证明真实 Service 恢复成功。
- **回滚 / 审批触发 / 集成**：恢复本批工作区代码和进度记录、并将 `ref/` gitlink 恢复到旧 SHA 即可撤销；无用户数据或系统状态迁移。风险 L2，无新增审批触发点。本轮只同步 `ref/` 并在本地 `master` 形成可审阅改动；未提交、未推送，未改其它分支、标签或 PR。
