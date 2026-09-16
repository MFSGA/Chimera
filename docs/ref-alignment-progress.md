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
