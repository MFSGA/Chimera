# ref 对齐进度

当前只读参考检出为 Clash Nyanpasu `main` 的最新提交
`5331747c06a5f42eeabb3e225a1e77a83f480549`，已通过远端 `HEAD` 与
`refs/heads/main` 核实；检出状态干净。主仓库 `.gitmodules` 留有 `ref` URL，
但当前 `HEAD` 没有 `ref` gitlink；按本项目约定，`ref/` 是 `.gitignore` 中的
本地只读基线，不属于主仓库提交。

DIFF-001 至 DIFF-012 是此前基于
`f7dbce2997c633e484f54788035e770b3ee99773` 完成的历史记录；它们不代表已按
新基线重审。后续切片应使用当前 `ref/` 提交，并明确记录与历史基线的差别。

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
