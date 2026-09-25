# ref 对齐进度

本记录使用的参考基线为 `ref` 提交
`f7dbce2997c633e484f54788035e770b3ee99773`。参考工作区存在预先的
`?? NUL`，不属于该提交，也未被复制或修改。

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

## DIFF-013：Profile 文件保存授权按来源判定（第十一阶段）

- ref commit：`efc9589fa37697d5e49ddec68cbceabcfaffd31d`；参考工作区有
  `M backend/nyanpasu-runtime`，本切片未读取或修改该子模块内容。
- ref 路径和符号：`backend/tauri/src/client/mod.rs` 的
  `NyanpasuClient::save_profile_file`，使用 `ProfileDefinition::source()` 区分
  `ProfileSource::Remote`、托管 `ProfileSource::Local` 和外部绑定；IPC 为
  `backend/tauri/src/ipc.rs::save_profile_file`。
- Chimera 路径和符号：`backend/tauri/src/client/profiles.rs` 的
  `ChimeraClient::save_profile_file`，经现有 profile file port 写入。
- 类别：临时迁移。
- 差异及必要性：拒绝 updater-owned remote 与 external source，仅允许 managed
  local source 写入；配置类 definition 校验 YAML，transform 类保留原文。ref
  使用 `nyanpasu_config::profile::Profiles` actor 快照及 `ProfilesError`；Chimera
  仍从旧 `config::profile::profiles::Profiles`/`Profile` 转换到共享 domain，且写文件
  后通过旧 runtime relevance/rebuild 路径处理，因此本切片只对齐授权与内容校验规则，
  不代表 profile 管理流程整体已对齐。
- 共通业务入口及适配边界：主界面、legacy UI 和 agent 继续调用共享 IPC；没有增加
  并行写入入口。持久化仍由当前 legacy Profiles 桥接负责。
- 影响的主界面、legacy UI、agent、数据、内核和平台：只影响 profile 文件保存命令的
  来源授权和 YAML 校验；IPC 合同及持久化 schema 不变。profile runtime rebuild 仍沿用
  ChimeraClient 当前实现。
- 实际验证结果：`cargo check -p tauri` 通过；未执行桌面 E2E 或保存/重启运行时验证。
- 收敛条件：迁移 Profiles actor、持久化状态、ProfileFs/materialization ports 与
  runtime application workflow，并将主界面、legacy UI、agent 接到同一 actor-backed
  API 后，删除旧 `ProfilesReadPort`/`ProfilesWritePort` 和对应 legacy bridge；补齐
  managed/external/remote 保存及失败恢复的契约与真实桌面覆盖后重新评估。

## T04 Profile document migration 与 stamp（阶段完成；生产启用延后）

- ref commit：`232321d52121fe8bb25cb2a090d814129cb50c55`；本次开始和结束时
  `git -C ref status --short` 均为空。
- ref → Chimera 路径/符号映射：`backend/nyanpasu-core/src/format.rs` 的 stamp API
  对应 `backend/chimera-core/src/format.rs`；Tauri 的
  `core/migration/{mod.rs,fs.rs,store.rs,modules/profiles.rs}` 对应相同 Chimera 路径；
  `runner.rs` 的生产迁移/检查/恢复段落和 Profile document 状态用例对应同名文件；
  `registry.rs::{get_migrations,find_migration}` 对应同名实现；ref fixture
  `backend/tauri/src/core/migration/fixtures/v1_6_1/profiles.yaml` 对应 Chimera 同路径。
- `chimera-core/src/format.rs` 在本切片开始前已有工作区修改；核对后内容与 ref 对应
  文件 SHA-256 均为 `08FAA7FB69F2E5D284E36D1DCD21B51C3E31F1B65370856EA4EFFDBB16FFDBCA`，
  本切片保留，没有覆盖该文件。迁移文件由固定 ref 逐段复制；只做 crate 品牌、产品
  header、YAML crate 与 release-version 字面值适配。stamp key `_nyanpasu` 保留为已有
  持久化 wire contract。保留 Chimera `typed_config` migrator，并在 registry 中按 ref
  先 Profile、后 typed config 的次序组合。
- 必要适配：ref 的 `serde_yaml` 与 Chimera 的 `serde_yaml_ng`/`serde_yaml` 是不同 crate，
  document stamp 路径统一使用 `serde_yaml_ng`，避免 YAML Mapping 类型不兼容；ref 的
  `2.0.0` gate 映射到 Chimera 当前 `0.24.1`；migration 测试 dev-dependencies 映射 ref
  已有版本，`backend/Cargo.lock` 随之更新。
- 生产兼容边界：当前 `setup()` 仍注入旧 `LegacyProfilesReadPort`/`LegacyProfilesWritePort`，
  它们解析并写出旧 Profile schema；ref migrator 会把 `profiles.yaml` 改成新 schema。
  若立即启用会令旧消费者把 Profile 读成空配置，后续保存有覆盖数据风险。因此 setup
  暂用 `Runner::with_paths_before_profile_client_migration`：普通启动只执行兼容迁移，保留
  Profile 文件原字节；隔离测试中的完整 Runner 仍执行 Profile migrator。这是临时启动边界，
  需在 T05–T09 全部 Profile 消费者接入共享 ref API 后，于最终集成时删除并启用迁移。
- 实际验证：`cargo test -j 1 --manifest-path backend/Cargo.toml -p chimera core::migration::`
  通过（59 passed，0 failed，311 filtered）；`cargo check -j 1 --manifest-path
  backend/Cargo.toml -p chimera` 通过（100 warnings）；`cargo fmt --manifest-path
  backend/Cargo.toml --all -- --check` 与 `git diff --check` 通过。第一次并行测试因
  Windows 页文件无法映射 reqwest rlib 失败；单 job 重跑通过。未运行桌面 E2E；Profile
  production migration 仍被上述兼容闸门延后，因此这些单测不等于已验证实际升级链路。
- 状态：T04 的 ref migrator、stamp/reconciliation、状态恢复、fixtures 和安全启动闸门已完成；
  Profile 全流程尚未完成。下一阶段按顺序进入 T05 ports/service/actor，迁移主/legacy UI
  与 agent 前不启用新 Profile schema。

## T05 Profile ports、service 与 actor（基础阶段完成；生产切换留到 T09）

- ref commit：`232321d52121fe8bb25cb2a090d814129cb50c55`；本轮检查时
  `git -C ref status --short` 为空，递归子模块分别为
  `backend/nyanpasu-runtime=f5b581fad8bf8272e222f1e3948c7826c6665bb6` 和
  `backend/nyanpasu-runtime/crates/nyanpasu-utils=cd6c9d3821a8c943bc249d96d456e2bedffd3ada`。
- ref → Chimera 映射：`state/profiles/{ports.rs,actor.rs,scheduler.rs}` → 同路径，包含
  `ProfileFsPort`、`SubscriptionFetcher`、`ProfileMaterializationPort`、
  `ProfilesActor`/`ProfilesActorMessage` 和 scheduler；`service/profile_file.rs` → 同路径，
  包含 `ProfileFileService`；`client/profiles.rs::ProfilesClient` → 临时
  `client/profiles_actor_client.rs`，并由 `client/mod.rs` 注册/导出。actor client 的单独
  文件是有收敛条件的兼容边界：当前 `client/profiles.rs` 已有 legacy 产品实现和用户改动，
  全部消费者切换后再归并到 ref 对应路径。测试所需 `enhance/golden_support.rs` 从 ref
  复制并仅在 `enhance/mod.rs` 用 `#[cfg(test)]` 声明。
- 仅做必要适配：`nyanpasu_config`/`nyanpasu_core` 换成 `chimera_config`/`chimera_core`；
  Profile User-Agent 与 HWID salt 保留已有 `clash-chimera` 产品标识；为 ref 同样启用
  Tokio `test-util`，新增 `notify-debouncer-full = 0.7.0` 并由 Cargo 更新锁文件；
  `scheduler.rs` 按本地 crate 名重排 import。actor client 的 YAML revision 断言使用
  Chimera 实际 persistence payload 的 `serde_yaml_ng::Value`。
- 发现并修复一个固定 ref 测试缺陷：`writes_keep_the_schema_stamp_and_reload` 在重载后调用
  不存在的 `ProfilesClient::get()`；同文件正式 API 是无队列读取的 `snapshot()`。复现为
  client 测试编译错误后改为断言 `reloaded.snapshot().valid`，没有放宽断言。若 ref 后续
  增加 `get()` 或改动 client 读取合同，应重新核对此例。
- 生产边界：`setup.rs` 仍通过 `LegacyProfilesReadPort`/旧写入 port 使用旧 schema，且
  `with_paths_before_profile_client_migration` 保护真实 `profiles.yaml` 原字节。不能在此时
  启动新 actor 接管相同文件；`ProfilesClient::new` 加载并验证 ref 新 schema，旧消费者未
  迁移前接入会造成兼容性/数据风险。因此 T05 完成 actor、service、ports 与 client API
  基础；不改变用户入口、生产持久化或运行时。最终 actor 注入和启用 migrator 移到 T09，
  前提是 IPC、主 UI、legacy UI、agent 都已使用同一 API。
- 实际验证：`cargo check --manifest-path backend/Cargo.toml -p chimera -j 1` 通过；
  `cargo test --manifest-path backend/Cargo.toml -p chimera --lib state::profiles::actor -j 1`
  3 passed；`... state::profiles::ports::tests -j 1` 2 passed；
  `... service::profile_file::tests -j 1` 46 passed；
  `... client::profiles_actor_client::tests -j 1` 66 passed；
  `cargo +nightly fmt --manifest-path backend/Cargo.toml --all -- --check` 与
  `git diff --check` 通过。以上为纯单元测试，没有运行真实桌面、生产文件迁移或网络 E2E。
- 后续收敛：T06–T09 逐步切换 IPC/UI/agent；T09 在所有入口切换后，把
  `ProfilesClient` 收敛回 `client/profiles.rs`，从 setup 注入唯一 actor/service/fetcher/notifier，
  移除 legacy Profile owner，并将 setup 切到完整 Profile migrator。生产切换未完成前，Profile
  全流程仍是部分迁移。

## T06 Client workflow / IPC / bindings（进行中；query binding generator 已接入）

- ref commit：`232321d52121fe8bb25cb2a090d814129cb50c55`；读取时 `git -C ref status --short`
  为空。
- ref → Chimera 映射：`frontend/interface/src/ipc/query-options.ts` → 同路径，完整复制
  query/mutation invoke helpers；`frontend/interface/src/utils/index.ts::Result` → 同名类型，
  仅复制 ref 的 `export` 声明以满足 helper 导入，保留本地 `unwrapResult` 实现；
  `specta_export.rs::append_query_bindings` → 同名函数，按固定 ref 精确复制；
  ref `CommandSet::new(...).build(TanstackQueryFramework::React)` → 本地独立的
  `build_profile_query_bindings()`，只选 `get_profiles`/`read_profile_file` 查询和现存 Profile
  mutations。`lib.rs::run` 与 `tests::export_bindings` 都把这一片段追加到常规 Specta 输出；
  `bindings.ts` 仅由现有 generator 更新。现在 `queries.readProfileFile` 与
  `mutations.saveProfileFile` 已生成，因此 `use-profile-content.ts` → 同路径也按 ref 全文件
  复制并改接这两个包装；main/editor 调用保持 `{ query, upsert }` 形状。
- 执行来源：前置 query helpers 由本地 `qwen2.5-coder:14b` 通过受限映射计划复制，Codex
  审核。当前 helper 复制也由该模型给出映射计划、runner 精确复制。尝试让本地 Qwen 与
  DeepSeek Aider 直接编辑 generator 时，两者都提议覆盖本地完整 builder；补丁均在写入前中止，
  没有落盘。Codex 保留本地 Chimera/Agent command registry，手动添加 profile-only 的
  `CommandSet` 拼接适配。无品牌替换。
- 保留差异与收敛条件：ref 用一组 `CommandSet` 分类全部 commands；Chimera 当前保留既有
  `build_specta_builder()` 注册表，另建 Profile-only query set，避免删除本地功能。待 T06
  IPC 切到 actor-backed shared facade、所有业务命令按 ref 分类后，再合并为完整 registry；
  当前生成 wrappers 仍指向旧 Profile IPC DTO，不能视为生产 Profile API 已切换。
- `use-profile.ts` 仍未复制：新 hook 依赖 `NewProfileRequest`、`ProfileItem` 与 ref 的
  `MutationOutcome` 返回合同；当前本地 bindings 仍为 legacy Profile DTO。不能因 query/mutation
  wrapper 已生成就把不兼容的 Profile hook 提前接入。
- 实际验证：`cargo check --manifest-path backend/Cargo.toml -p chimera -j 1` 通过（123 条已有
  warning）；`cargo fmt --manifest-path backend/Cargo.toml --all -- --check` 通过（稳定版仅提示
  nightly rustfmt 配置不可用）；忽略的 `update_typescript_bindings` 生成器执行通过；
  `typescript_bindings_are_fresh` 1 项通过；`pnpm typecheck`、`pnpm lint:frontend-boundaries`、
  `node --check scripts/ref-align.mjs`、`git diff --check` 通过。Profile content hook 子任务的
  `pnpm typecheck` 与 Prettier 检查通过。未运行桌面 E2E。
- 状态：T06 仍在进行。剩余工作是把 Profile IPC 与 actor-backed application facade 接通，迁移
  main/legacy hooks 与 UI，随后切换 agent；setup 的 Profile migrator 闸门继续保留到所有消费者
  迁移完成。此切片只提供 query/mutation 绑定生成，不改变运行时 Profile owner。
