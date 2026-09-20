# ref 对齐进度

本记录使用的参考基线为 `ref` 提交
`f7dbce2997c633e484f54788035e770b3ee99773`。参考工作区存在预先的
`?? NUL`，不属于该提交，也未被复制或修改。

## 当前 P0 收敛状态（2026-09-18）

- 所有支持的 Clash core（含 Chimera Client）已统一通过
  `chimera-config::runtime::executor` 生成 runtime；Chimera Client 的 legacy
  TUN/DNS 合同已显式编码为 `TunFlavor::ChimeraClient`，生产路径不再回退到旧
  `enhance()`。
- 旧 `enhance()` 入口已从编译图移除，未使用的 `build_from_legacy` 与
  `artifact_to_legacy_output` 兼容 wrapper 已删除；生产 runtime 生成只有一条
  shared executor 业务实现。
- `CoreManager::global()` 已从 `backend/tauri/src` 清零。生产 composition root
  创建 `LegacyCoreBridge`，由 bridge 显式持有一个 `Arc<CoreManager>`；Clash HTTP
  API、WebSocket endpoint、Agent probe、启动初始化与 crash recovery 均不再通过
  singleton service locator 取 core manager。
- 生产 `RuntimeInputOutput` 现在强制携带 `RuntimeInspectionData`；共享 executor
  生成失败会直接返回错误，不再伪装为 legacy/fallback 的 `BareRoot` inspection。
- 尚未完成的主要 ref 差异仍是 `core_lifecycle/workflow.rs` + `core/actor_v2`
  的 actor-owned lifecycle/service-host/uncertain-outcome 模型，以及 legacy typed
  config/profile 持久化边界的进一步收敛。因此 P0 的“单一 runtime 业务实现”和
  “移除 CoreManager singleton 访问”已完成，但完整 actor_v2 对齐仍是后续工作。

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
- 差异及必要性：所有 Clash 核心（Premium、Rust、Mihomo、Alpha 与
  Chimera Client）现在都通过共享 `RuntimePipelineInputs` 和 `execute` 完成
  Profile 合成、全局变换、内置脚本、字段白名单、Guard、TUN 默认值和产物日志；
  `PostProcessingOutput` 由适配器生成。Chimera Client 的 legacy TUN/DNS
  合同由 `TunFlavor::ChimeraClient` 显式表示，并通过 executor 单测锁定用户值
  不被默认值覆盖的行为。
- 失败语义：共享 executor 构建失败会直接向调用方返回错误；生产入口不再静默
  回退到旧 Enhance。旧 `enhance()` 入口已退出编译图，避免两个 runtime 业务实现
  长期并存。
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
- 当前收敛状态：Chimera Client 自定义 TUN/运行时合同和旧 Enhance 分支已经
  收敛到 shared executor；`legacy profile -> ref domain` adapter 仍保留，因此该项
  在 profile 持久化层仍是部分迁移。真实桌面 TUN/service lifecycle 仍需专属 E2E/
  host smoke 验证，不能仅凭单元测试宣称完整等同 ref。

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
- 兼容边界：生产 runtime 生成现已始终携带 shared executor 产生的 inspection；
  `RuntimeInspectionData::bare()` 仅保留在测试/兼容构造路径，不再作为生产
  Chimera Client 或 executor 失败的 fallback。
- 影响的主界面、legacy UI、agent、数据、内核和平台：新增 IPC/API 可供主界面、
  legacy UI 或 agent 复用；本批未改变既有 UI 流程、持久化格式、核心启动和
  系统代理行为。
- 实际验证结果：`cargo check --manifest-path backend/Cargo.toml -p chimera`；
  `cargo test --manifest-path backend/Cargo.toml -p chimera runtime_inspection
  -- --test-threads=1`，4 passed；`cargo test --manifest-path
  backend/tauri/Cargo.toml typescript_bindings_are_fresh --
  --test-threads=1` 通过；`pnpm typecheck` 通过。
- 收敛、移除或重新评估条件：生产构建已全部接入共享 executor，下一步只需
  在兼容测试构造器不再需要时删除 `RuntimeInspectionData::bare`，并补充真实
  runtime/E2E 的快照更新、过期检查与 core restart 验证。

## DIFF-005：CoreLifecycle 目录边界（第四阶段）

- ref commit：`f7dbce2997c633e484f54788035e770b3ee99773`
- ref 路径和符号：`backend/tauri/src/client/core_lifecycle/{mod.rs,ports.rs,adapters.rs,workflow.rs}`，
  `backend/tauri/src/core/actor_v2/{endpoint.rs,facade.rs}`，以及 `CoreLifecycleClient`/
  `CoreFacade` 的应用编排与 lower host 分层
- Chimera 路径和符号：新增
  `backend/tauri/src/client/core_lifecycle/{mod.rs,ports.rs,adapters.rs,workflow.rs}` 与
  `backend/tauri/src/core/actor_v2/{mod.rs,endpoint.rs,facade.rs}`；`client/mod.rs`、
  `client/clash_config.rs` 和 `setup.rs` 已改用新目录入口。
  原 `backend/tauri/src/client/core_bridge.rs` 已退出模块图，仅因当前工具无安全
  删除接口而暂留源码树，不再提供运行时或编译期兼容入口
- 类别：临时迁移
- 差异及必要性：将现有 CoreManager 生命周期端口、运行配置端口、运行时诊断
  DTO 和 legacy adapter 按 ref 的 core-lifecycle 目录归位，并新增
  `CoreLifecycleWorkflow` 与真实 ractor mailbox。生产 `StopCore`/`SelectCore`/
  `Reconcile` 已通过 `CoreLifecycleClient` 串行 admission，再由 workflow 获取 lifecycle lease
  执行；应用启动也只投递 `StartupReconcile`，不再由 `CoreManager::init`/`run_core`
  在 actor 外启动核心。core crash recovery 的 `Notify` 同样已接入 mailbox，并直接复用
  typed `Reconcile` workflow；旧 `CoreLifecyclePort::recover`/`CoreManager::recover_core_once`
  以及 legacy `RunType::default()` recovery 分支已删除。core updater
  现在只负责下载/解压并提交 ref-shaped
  `PreparedCoreBinary`，stop/install/restart 由 lifecycle actor 统一执行。旧 UI/API
  合同保持不变。
- 兼容边界：旧 `core_bridge.rs` shim 已退出模块图；composition root 现在显式创建
  `core::actor_v2::CoreFacade`，该 facade 独占唯一 `Arc<CoreManager>`；`LegacyCoreBridge`
  只持有 `Arc<CoreFacade>` 并负责 client port/diagnostics DTO 适配，client/setup 已无
  `CoreManager` concrete ownership，仓库内也已无 `CoreManager::global()` 调用。
  `actor_v2::endpoint::CoreStatusSnapshot` 现在是 lower canonical status projection，并由
  client port 直接复用。composition root 同时用同一个 `Arc<CoreFacade>` 构造
  `LegacyCoreBridge` 与 `LegacyServiceBridge`；Service probe、`HOST_TRANSITION_LOCK`、daemon
  install/update/start/restart/stop/uninstall 及 ready/stopped confirmation 已下沉到
  `core/actor_v2::facade::ServiceTransition`，client adapter 只保留 trait 适配。生命周期执行
  底层仍由 legacy `CoreManager`/service control 实现，但 workflow
  已不再持有任何 core lifecycle lease：
  reconcile/stop/select 通过 facade-style `CoreLifecyclePort` 方法执行，lease acquisition
  被封装在 legacy adapter 内；updater 的 stop→install→reconcile 原子性由 actor-owned
  single active task + pending queue 保证，不再跨文件安装持有 `CoreManager` mutex。
  mailbox 目前接管 startup/stop/select/recover/reconcile/
  replace-binary，以及全部显式 Service lifecycle mutation。
  recover 每次执行一次同源 typed reconcile，失败后由 actor 延迟 5 秒重新投递，因此不会
  在一次 handler 中永久占住生命周期，也不再维护第二套 legacy recovery 构建逻辑。
  binary replacement 使用显式注入的 `BinaryInstaller` 和进度回调；staging `TempDir`
  由 `Arc` 保活到安装/reconcile 结束。安装当前选中 core 时执行 stop→install→reconcile，
  与 ref `ReplaceCoreBinary` 的安装后 reconcile 语义一致；旧的 `CoreUpdateLease`、
  `CoreBinaryUpdateLease` 与 client-facing 通用 lifecycle lease 均已删除。mailbox RPC 现在为每次 mutation 分配
  operation id，默认最多等待 180 秒；caller 等待超时不会取消已 admission 的操作，
  actor 会继续执行并把结果写入 bounded completed history（32 条）。新增只读
  `get_core_lifecycle_status` IPC 暴露 active/queued/completed，供超时后按 operation id
  检查结果。actor 还内置 500ms runtime-dirty 窗口：同一窗口内的后台 rebuild 请求只
  admission 一次 `Reconcile`，窗口后的新 dirty 会再次触发；Service IPC health-loop
  已改为该 best-effort dirty producer。旧 `client/rebuild.rs` coordinator 已退出模块图，
  避免 actor 外保留第二套 rebuild 协调层。workflow future panic 现在由 actor 捕获并
  latch `uncertain=true`；lower `CoreFacade` 也新增 ref-shaped `outcome_uncertain` guard。
  Local core mutation 现在由 `core/actor_v2/endpoint.rs::LocalEndpoint` 独占 `CoreManager`，并通过
  ref-shaped `ControlEndpoint::submit / wait_operation(id) / status` 协议执行。submit 分配 typed
  `OperationId`，写入 bounded terminal registry（最多保留 64 条），再由 detached lower task 持有
  `CoreManager` lifecycle lock；waiter/caller 被取消不会取消已开始的 mutation，也不会仅因为 reply
  丢失就 latch uncertain，后续仍可按 id 重新读取 `OperationInfo` 终态。lower task panic，或 lower
  wait budget 到期而 operation 仍未终态时才 latch uncertain。Service daemon mutation 已由
  独立 `ServiceActor` mailbox 持有；`HOST_TRANSITION_LOCK` 只继续覆盖跨 daemon/core handoff 的
  高层 transaction。Service waiter cancellation/actor-call timeout 会通过 shared guard latch
  uncertain，但已 admission 的 OS command 仍由 actor 串行跑完。普通 terminal operation error
  不会误触发 uncertain。lower uncertain 经 `CoreLifecyclePort` 回传并并入
  lifecycle actor 的 `uncertain=true`，之后 mutation、recover 和 runtime-dirty 都不会再触碰
  底层 lifecycle；该状态也通过 `get_core_lifecycle_status` 暴露。与 ref 的剩余差异是 local
  `ControlEndpoint` 目前承载 Chimera 的 `Reconcile/Stop/ChangeCore` command、
  `Running/Succeeded/Failed/Uncertain` phase 与 typed `OperationOutput`。`Stop` 返回 `Stopped`；
  `Reconcile/ChangeCore` 成功后从 `RuntimeLifecycleState.applied` 返回实际 `revision + core`，不会从
  desired config 猜 applied identity。lower `CoreStatusSnapshot` 也新增可选 applied identity，并且只在
  host 当前为 Running 时发布；Stopped 会压掉历史 runtime snapshot，避免 CAS 消费陈旧 revision。
  reconcile admission 现在还会先读一次 authoritative lower status 作为 `expected_applied`，endpoint
  在持有 lifecycle lock 后再次读取当前 Running applied revision 并做 CAS；revision 已变化、丢失或
  从 None 变为 Some 时都会以 terminal Failed 拒绝旧 reconcile，不会误触发 uncertain。lower
  operation registry 现在也通过 app IPC 暴露：`get_lower_core_operations` 返回 admission 顺序的
  bounded history，调用方可从中取得 typed `OperationId`；`get_lower_core_operation(id)` 可再次读取
  `Running/Succeeded/Failed/Uncertain`、typed output 与 error。Specta 同步导出 `OperationInfo`/
  `OperationOutput`/`OperationPhase`/`OperationId`。仍未迁移的是 daemon core-control
  `ServiceEndpoint`/endpoint-handle registry，以及 upper lifecycle operation id 与 lower operation id
  的显式关联字段；两套 id 当前可分别观察但不是同一个 namespace。应用退出现在使用专用 `Shutdown`
  command，而不是普通
  `StopCore`：首次 shutdown 会 latch `shutting_down=true`、停止接收后续普通 mutation/
  recover/runtime-dirty，并缓存 stop 终态；重复 shutdown 复用同一终态，不会重复 stop。
  `get_core_lifecycle_status` 同步暴露 `shutting_down`。mutation admission 现在由 actor-owned
  `VecDeque<Request>` 管理，最多保留 32 个 pending；第 33 个请求由 actor 直接拒绝并写入
  completed history。workflow ownership 每次只移动到一个 detached active task，RPC waiter
  超时或丢弃不会取消已 admission 的执行，也不会释放第二个 lifecycle operation。shutdown
  会在 actor 内 close/drain pending mutation、等待当前 active operation 终结，再运行唯一的
  final stop；并发/重复 shutdown waiter 共享同一终态。该队列/active-task ownership 已与 ref
  的 lifecycle actor 形状对齐，剩余差异主要下沉到 lower core/service host facade。
  `InstallService`/`UpdateService`/`StartService`/`RestartService`/`StopService`/
  `UninstallService` 已通过 `ServiceLifecyclePort`/transition lease 进入 mailbox；该 port 的
  production adapter 与 local-core port 现在共享同一个 lower `CoreFacade` owner。
  Service status ownership 也已下沉到同一个 `ServiceActor`：actor 自己持有
  `watch::Sender<ServiceHostStatus>`，`phase`/`compat`/daemon wire fields 与 `restart_attempts`
  只在 lower ServiceActor 中发布；`ServiceLifecyclePort::subscribe_status` 只把 receiver 交给
  `CoreLifecycleClient`，lifecycle actor 不再维护第二份 Service 状态镜像。Chimera 额外保留
  `runtime_owned`，防止“协议兼容但属于其他 runtime”的 daemon 被误判成可用 host。ServiceActor
  spawn 时从 `Probing` 做一次初始 probe；install/start/restart/uninstall 发布 transient phase，
  daemon command 完成后 probe 稳定态，`confirm_ready/confirm_stopped` 成功后也会刷新 authoritative
  watch。IPC `status_service` 与 Agent diagnostics 现在只读这同一份 cached projection，不再为每次读取主动 probe daemon。legacy adapter 持有现有
  `HOST_TRANSITION_LOCK`；start/restart 会在 bounded readiness probe 后按配置收敛到
  Service host 并再次验证，stop 会完成停止确认、Service→Local reconcile 和最终 core
  验证。uninstall 在当前 core 明确为 Running+Service 时也已按 ref 顺序收敛：先
  stop daemon + confirm stopped，再 typed reconcile 到 Local 并验证，最后才 uninstall；
  Local/Stopped 情况保持既有 UX。lower `ServiceTransition::uninstall_daemon` 也新增 fail-closed
  ownership preflight：probe 失败、Running 但无 server detail、或明确 core Running 都拒绝卸载；
  只有 Running+core Stopped 才会先 stop 并再次证明 daemon 已停后再 uninstall，NotInstalled 幂等返回。
  transition 失败会由 lifecycle actor 标记 runtime dirty 进行后续 best-effort reconcile。Service
  health-loop 仍负责实际周期 probe，但成功 observation/失败状态会通过 `ServiceLifecyclePort`
  直接 cast 回 lower ServiceActor 更新同一 watch；lifecycle actor 不再参与 daemon status 发布。
  因此 UI/Agent、显式 transition 与 health-loop 共享同一 Service 状态 owner。legacy `status --json` probe 现在统一使用
  5 秒 timeout + `kill_on_drop(true)`，因此启动期兼容性检查、health-loop 与 endpoint-down re-probe
  都有同一 fail-closed bound，不会留下孤儿 status 子进程。health loop 仅在
  Connected→Disconnected 边沿提交 `ServiceEndpointDown`；
  lower `CoreFacade` 会重新 probe，只有明确 `Stopped` 才允许 auto-restart，probe 失败、Running、
  Incompatible/NotInstalled 都 fail-closed。当前 restart budget 为 3：成功 admission 递增
  `restart_attempts`，预算耗尽后 latch `ServicePhase::Exhausted`；普通 probe/health observation
  不会冲掉 exhausted latch，显式 install/update/start/restart 会 re-arm budget。这与 ref 的
  endpoint-down/restart-budget/exhausted 基本语义一致。daemon OS mutation 现在已下沉到独立
  `core/actor_v2/service_actor.rs` mailbox；`CoreFacade` 通过 lazy `ServiceClient` 复用同一 actor，
  install/update/start/restart/stop/uninstall 与 endpoint-down auto-restart 不再由 facade future
  原地执行。caller cancellation/actor-call timeout 会 latch shared `outcome_uncertain`，但已 admission
  的 OS command 仍由 actor handler 串行持有并跑完，不会因 waiter drop 取消；后续 core/service
  mutation 因 uncertain fail-closed。高层 `HOST_TRANSITION_LOCK` 仍覆盖 daemon command + core handoff
  的跨 host transaction。剩余差异主要是内部每个 adapter leg 的 kill-safe bounded timeout，以及
  daemon core-control `ServiceEndpoint`/endpoint-handle routing；
  当前 `runas + spawn_blocking` mutation 本身仍不可安全强杀，因此只在 actor client 层提供 110 秒
  caller bound，超时后保守进入 uncertain，而 actor mailbox 继续占有 command 直到 OS 调用终结。
- 影响的主界面、legacy UI、agent、数据、内核和平台：调用方继续复用同一
  `ChimeraClient` 和端口，未改变持久化格式或现有 UI/agent/E2E 入口。
- 实际验证结果：`cargo check --manifest-path backend/Cargo.toml -p chimera`；
  `cargo test --manifest-path backend/Cargo.toml -p chimera client::tests --lib --
  --test-threads=1`，16 passed；`client::core_lifecycle::tests`，22 passed（mailbox
  mutation serialization、startup reconcile admission、crash recovery signal 复用 typed reconcile、reconcile admission、
  read-only Service probe 更新 watch、health observation 无额外 re-probe 更新 cache、probe failure 发布 Unknown、
  replace-binary 的 stop→install→typed reconcile→finished 顺序、caller timeout 不取消已
  admission operation、dirty burst coalescing、后续窗口再次 reconcile、workflow
  panic 与 lower outcome-uncertain 均 latch 并阻断后续 mutation、幂等 shutdown 只 stop 一次且阻断后续 mutation、
  shutdown drain actor-owned pending queue 后等待 active 再 final stop、pending queue 超过 32 条时拒绝 overflow、
  InstallService/UpdateService 进入 mailbox、StartService/RestartService 在同一 mailbox/host-transition
  lease 内收敛到 Service host、StopService 把 core 从 Service handoff 回 Local、
  UninstallService 在 Service host 时先 stop/confirm + handoff Local 再 uninstall、
  endpoint-down restart budget/exhausted latch）；
  `core::actor_v2` tests，11 passed（endpoint waiter cancellation 后仍可按 lower id 读取终态、
  lower panic 持久化为 Uncertain、terminal error 持久化为 Failed、Stop typed terminal output、
  Stopped status 不暴露 stale applied identity、expected-applied revision CAS 拒绝 stale/missing authority、
  operation history 保持 admission 顺序且按 id 查询返回同一终态、
  Service uninstall fail-closed ownership guard、ServiceActor mailbox 在 waiter cancellation 后仍串行持有
  command、ServiceClient cancellation latch uncertain、仅明确 Stopped daemon 消耗 restart budget 并在
  budget exhausted 后停启、显式 command re-arm budget）；
  `core::service` tests，12 passed；
  `features::agent::diagnostics`，5 passed；`typescript_bindings_are_fresh`，1 passed；`pnpm typecheck`、
  `pnpm lint:frontend-boundaries`、`cargo fmt --manifest-path backend/Cargo.toml --package
  chimera` 与 `git diff --check` 通过。
- 收敛、移除或重新评估条件：singleton service-locator 已清除，生产
  `ChimeraClient` 也已持有 actor-backed `CoreLifecycleClient`，runtime reconcile 与
  updater binary replacement 已迁入 mailbox，operation-id/status/有界等待、actor-owned
  bounded pending queue + detached active task、runtime-dirty coalescing、workflow-panic uncertain latch，
  以及显式 Service install/update/start/restart/stop/uninstall transition、actor-owned
  cached/watch `ServiceHostStatus`、外部只读 cached projection 与专用幂等 shutdown 已具备；应用启动
  core reconcile 与 service auto-update 都不再绕过 actor，旧 `CoreManager::init`、
  `CoreManager::run_core`、旧 recovery API、`CoreBinaryUpdateLease` 和无参 lifecycle rebuild
  convenience API 已删除。`core/actor_v2::CoreFacade` 的 local-core + Service-host
  ownership/status/lifecycle-lock/transition、local `ControlEndpoint` submit/wait/status + operation
  registry + typed terminal output/applied runtime identity + Running-only applied status projection +
  expected-applied revision CAS、fail-closed outcome-uncertain guard、独立 ServiceActor command +
  status/watch ownership、endpoint-down restart-budget/exhausted latch，以及 lower operation history/id
  app IPC projection 已落地；下一阶段继续把 daemon core-control `ServiceEndpoint`/endpoint handle
  收进 lower host protocol，并评估是否需要把 upper lifecycle operation id 与 lower operation id
  显式关联。当前仍是 lower host protocol 的部分迁移，而不是 singleton、
  manager ownership 或 workflow lease 问题。

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
- 差异及必要性：shared executor 的 `applied_fields` 现在按执行顺序保留为
  `RuntimeSnapshot.exists_keys`，并提供只读 `get_runtime_exists` IPC；所有生产
  runtime 生成路径都使用同一 executor 结果，不再需要旧 Enhance 的 `use_keys`
  兼容分支。
- 兼容边界：尚未发布 RuntimeSnapshot 时返回空数组；旧构造器为保持已有测试和
  调用合同，默认使用空集合，只有实际构建入口注入对应键集合。
- 实际验证结果：`cargo check --manifest-path backend/Cargo.toml -p chimera`；
  `cargo test --manifest-path backend/tauri/Cargo.toml
  typescript_bindings_are_fresh -- --test-threads=1` 通过；`pnpm typecheck`
  通过；新增 RuntimeSnapshot applied-fields 单测。
- 收敛条件：生产 RuntimeSnapshot 已统一由 executor 数据构造；待测试/兼容
  调用方不再需要旧构造器时移除空集合默认值，并补充真实 reconcile 后
  `get_runtime_exists` 的 E2E 验证。

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
- 收敛条件：生产 RuntimeSnapshot 已覆盖所有 core；后续在 ClashConfig typed
  contract 完成后移除 `Any<serde_json::Value>` 与默认值兼容分支，并补充真实
  runtime 导出/后处理输出的 E2E 验证。

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
  生成 inspection id 和组装配置/已应用字段/后处理输出/诊断图；所有 core 的
  生产提升路径都直接使用该构造，旧的多参数构造器暂保留为测试与兼容包装。
  后处理字段名称同步为 ref 的 `postprocessing_output`，并
  保留 `RUNTIME_CONFIG_FILE`，增加 ref 兼容的 `RUNTIME_CONFIG` 别名。
- 兼容边界：Chimera 的并发 `RuntimeRevisionAllocator`、Applied/Promoted
  双快照和事务恢复状态仍保留；`new_with_transform_output*` 仅是测试/过渡入口，
  不再持有独立组装逻辑。生产 `RuntimeInputOutput` 已强制携带真实
  `RuntimeInspectionData`；`bare()` 只服务兼容构造和单测。
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
- 兼容边界：`generate_runtime_output_with` 仍可从现有 legacy client info
  构造默认 bindings，供尚未持有 session resolver 的兼容调用方使用；公开的
  `build_from_legacy` wrapper 已删除。Chimera Client 与其他 core 共用同一
  RuntimeBuilder，仅在 `TunFlavor::ChimeraClient` 中保留其产品特有 TUN/DNS 合同。
- 影响的主界面、legacy UI、agent、数据、内核和平台：标准核心启动、重启和
  runtime 快照现在共享同一组具体端口；主界面、legacy UI 与 agent 继续通过同一
  application API 读取状态，系统代理和配置文件副作用保持不变。
- 实际验证结果：`cargo fmt --manifest-path backend/Cargo.toml --all`；
  `cargo check --manifest-path backend/Cargo.toml -p chimera`；
  `cargo test --manifest-path backend/tauri/Cargo.toml client::ports --
  --test-threads=1`，5 passed；`pnpm typecheck`；
  `pnpm lint:frontend-boundaries`；`git diff --check`。
- 收敛条件：补齐 Chimera 对应的 service/profile-file 端口读取接口后，将
  `SessionPortResolver` 接入 fetcher 的 `SelfProxyPortSource`；所有 core 已共用
  RuntimeBuilder，后续重点是删除仍依赖 legacy client-info 的默认 bindings 入口，
  并补充真实核心重启/端口占用 E2E 验证。

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
