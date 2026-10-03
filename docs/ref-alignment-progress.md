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
