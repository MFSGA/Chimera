# Chimera ↔ ref 全量架构对齐清单

**核对基线：** Chimera 本地分支 `codex/clash-api-ipc-dev-1106`，参考子模块 `ref@b7f54a4b0baf06f86c2e1675b5c5f86f96b0b866`（2026-11-07，经官方 GitHub `refs/heads/main` 查询及祖先关系验证）。此 SHA 是本次核实时的官方 main，不保证未来仍为最新。此次仅更新基线，以下历史测试数字不代表在新 SHA 上重新执行。

**最新目标（用户已明确修改）：** 以 `ref` 的代码职责、协议契约、事务语义、状态、前端接口与运行时行为作为迁移基准。**无需保留 Chimera 旧架构、旧 API 或旧 Service IPC 的兼容性**；允许连同调用方和运行时执行破坏性替换，删除被取代的旧实现，不构造长期双轨适配。迁移仍应避免无关改动、用户数据意外损毁和未测试状态下的功能失效；不应因为复制文件或编译通过就宣称整个架构已对齐。Chimera 的产品身份可独立于底层旧协议。

## 核对方法

- 参考 Rust 独立 core：`ref/backend/nyanpasu-core/src/`（本次审查为 **168** 个 Rust 文件）。
- Chimera 独立 core：`backend/chimera-core/src/`（本次审查为 **31** 个 Rust 文件，含本轮新增的同实例代理组协调；文件数不是完成率）。
- 参考 Tauri 适配层：`ref/backend/tauri/src/`（**83** 个 Rust 文件）；Chimera Tauri：`backend/tauri/src/`（**211** 个 Rust 文件）。
- 参考独立前端包：`ref/frontend/query/src`（**64** 个 TS/TSX）、`ref/frontend/rpc/src`（**9** 个）、`ref/frontend/platform/src`（**1** 个）。Chimera 当前相应能力主要合并在 `frontend/interface/src`（**70** 个 TS/TSX）及 Tauri IPC 中。
- 完整迁移需要比较架构与调用关系、端到端契约及其测试；文件统计不构成任何路径或行为完成比例。

## 已迁移并接入生产路径

| 能力                              | Chimera 路径                                                                                                | 已验证范围                                                                           | 局限                                  |
| --------------------------------- | ----------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ | ------------------------------------- |
| 原生 Clash DTO 和 Provider 元数据 | `backend/chimera-core/src/clash/{api,proxies}.rs`                                                           | 原生/legacy 代理图组装、Provider-only 节点、Group 类型                               | 对应 Tauri legacy DTO 暂时保留        |
| 实例绑定的 Clash API              | `backend/chimera-core/src/actor_v2/api.rs`                                                                  | Stale/Unavailable/Timeout/Protocol；请求前后身份校验                                 | 实例获取/宿主生命周期端点仍在 Tauri   |
| Client 协议投影和延迟参数         | `backend/chimera-core/src/client/clash_api.rs`                                                              | Provider、URL、expected、timeout                                                     | 其他 Client 工作流仍在 Tauri          |
| 连接速率衍生                      | `backend/chimera-core/src/clash/connection_rates.rs`                                                        | Tauri WebSocket 生产路径使用 core 算法                                               | 类型化 Stream Actor 仍未迁出          |
| 连接中断规则                      | `backend/chimera-core/src/connections.rs`                                                                   | 实例绑定、按组精准匹配                                                               | 由 Tauri 配置/窗口动作触发            |
| 固定组取消固定                    | `backend/chimera-core/src/actor_v2/api.rs` + Tauri adapter                                                  | DELETE 单次派发、代际保护、真实状态刷新                                              | 未并入 ref 的完整 ProxiesActor        |
| 同实例原生组延迟测试              | `backend/chimera-core/src/proxies.rs`                                                                       | 一个 ApiClient；固定组 8 并发；Provider/URL/expected；失败节点记零；实例撤销失败关闭 | 新旧 UI 仍分别调用不同的缓存视图      |
| 核心快照版本与发布判定            | `backend/chimera-core/src/proxies.rs::ProxySnapshotState`                                                   | Profile generation、checksum、staleness、相同快照仅更新验证时间                      | 外层 ProxiesGuard、事件通知还在 Tauri |
| main 原生代理导航和操作           | `frontend/interface/src/ipc/use-native-proxy-actions.ts`、`frontend/chimera/src/pages/(main)/main/proxies/` | 规范化 nodes、组测试单 IPC、取消固定、节点选择和延迟                                 | legacy/Agent 仍依赖兼容入口           |

## 未完全对齐的主干（按依赖顺序执行）

| 顺序 | 参考源路径（`ref/` 下）                                                                                        | Chimera 当前主要位置                                                                                | 所需迁移 / 验收条件                                                                                                                          |
| ---- | -------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| 1    | `backend/nyanpasu-core/src/actor_v2/{facade,endpoint,service_actor,local_host,...}.rs`                         | `backend/tauri/src/core/actor_v2/`、`backend/tauri/src/client/core_lifecycle/`                      | 将核心实例/能力所有权、Local/Service 生命周期与接口抽入独立 core；通过服务重连、失效租约、回滚与启动失败测试                                 |
| 2    | `backend/nyanpasu-core/src/proxies.rs`                                                                         | `backend/tauri/src/core/clash/proxies.rs` + 新的 `chimera-core/src/proxies.rs`                      | 迁移长生命周期 **ProxiesActor** 与单一快照/Provider 缓存、去重刷新、变更事件、选择/取消固定后重读及中断降级；以同一状态驱动 native 和 legacy |
| 3    | `backend/nyanpasu-core/src/clash/ws.rs`                                                                        | `backend/tauri/src/core/clash/ws.rs`                                                                | 抽出实例绑定 Stream Actor、类型化帧、history/sequence、流量/内存/日志事件；Tauri 仅做 Channel/Event 桥接；保证断线重连和 Agent 统计连续性    |
| 4    | `backend/nyanpasu-core/src/client/{application,application_workflow,profiles,system_proxy,hotkey,effects,...}` | `backend/tauri/src/client/` 和部分 `core/`                                                          | 将应用事务参与者、profiles、热键、系统代理、运行状态等核心流程迁到无 Tauri 依赖的 core；Tauri 提供 OS/UI adapter，保护 Chimera 特性          |
| 5    | `backend/nyanpasu-core/src/state/{application,profiles,mutation,clash_config,...}`                             | `backend/chimera-core/src/state/`、`backend/tauri/src/config/`                                      | 迁移多参与者事务、持久化和失败结算、Profile jobs、状态变更广播；允许新格式，必要时明确迁移用户数据而非保留旧 API                             |
| 6    | `backend/nyanpasu-core/src/{service,logs,geo,traffic,storage,utils,profile_import,...}`                        | `backend/tauri/src/{core,client,feat,...}`，独立 `chimera-traffic`/`chimera-jobs`/`chimera-geodata` | 按职责迁移业务处理，不重写已有独立 crates；检验 service IPC、geo 缓存、traffic 持久化及恢复，保留用户数据                                    |
| 7    | `backend/tauri/src/{unified_rpc.rs,adapters/}`                                                                 | `backend/tauri/src/ipc.rs`、`specta_export.rs`、`frontend/interface/src/ipc/`                       | 以 ref Unified RPC/typed errors/transport 作为新契约，**同步迁移并删除**旧 Tauri 命令与调用方；不保留旧兼容层                                |
| 8    | `frontend/{query,rpc,platform}/src`                                                                            | `frontend/interface/src`、`frontend/chimera/src`                                                    | 抽离 query/rpc/platform 层、统一缓存键、mutation 结果和失联处理；逐页迁移 main/legacy；测试窗口、系统代理、profiles、连接/流与设置           |
| 9    | `frontend/nyanpasu/src`                                                                                        | `frontend/chimera/src`                                                                              | 按行为逐页检查差异；保留 Chimera 主题、标识、Agent 诊断及功能，避免换回 Nyanpasu 品牌                                                        |
| 10   | 跨系统集成测试与文档                                                                                           | 现有 `frontend/chimera/tests`、`tauri-e2e` 等                                                       | Windows 主/旧 UI + Service 模式的真正端到端用例、实例切换/升级、数据迁移、权限、流量历史与回滚；再视 CI 能力扩展其他平台                     |

## 每批迁移的验收门槛

1. 在原目录 `I:\mfsga\Chimera` 的当前分支操作；不使用替身 worktree，不修改 `ref` 工作树。用户已明确要求**同时迁移 `backend/chimera-runtime`**；新 runtime 提交经过独立测试后，主仓库必须更新其 gitlink，并明确后续发布资产和可获取性条件。
2. 先确认原始未提交差异，按路径和 hunk 精确暂存；绝不混入无关的 `pnpm-lock.yaml` 改动。
3. 迁移**所有权和生产调用路径**，而不仅仅是复制 DTO 或改名。同步修改旧 UI/Agent 调用点，删除被替代的旧接口与重复实现，不再新增兼容投影。
4. 每段提交前运行独立 core、相关 Tauri Rust/Specta、interface/main/legacy TS、边界、相关单测、格式及差异检查。遇到无法执行的 Clippy、dated nightly 或真正桌面 E2E，明确标注未执行，不作通过声明。
5. 对生命周期/写操作实施失败关闭：旧实例不执行新写入；未知回复不自动重试；缓存不乐观宣称提交；核心切换后旧响应不得覆盖新快照。
6. 每一块逻辑独立 Git 本地提交，保留可审查的 Diff；不推送、不启动桌面程序。当前对齐状态不等同于全部完成，只有实际实现和对应集成测试通过后才可勾选项目。

## 本轮提交与验证

- `01d85760` — 核心同实例组测试和生产 IPC；3 个本地 mock-controller 测试。
- `530a4d79` — 前端更换为单 IPC mutation，移除被 core 替代的旧协调器；2 个前端命令契约测试。
- `1f015cee` — 将 ProxySnapshotState 的版本、发布与时效判定移入 core；增加同 checksum/旧代际/时间回退和中途 Profile 切换测试。

本轮独立 core 完整测试 **132/132**、Tauri 代理 **14/14**、Actor V2 **9/9**、Specta 生成类型检查通过；前端相关单测 **28/28**，E2E 测试框架单测 **33/33**。这些并不代表真实桌面应用、服务模式、跨平台或全量 ref 行为的端到端一致性。

## 2026-11-07 上游源码直接复制与编译适配

用户已授权采用“复制 `ref` 源文件 → 适配 Chimera crate/接口 → 编译并跑回归”的加速模式。已在原 Windows 仓库提交本地代码 `578b0357`：

| 复制来源（`ref/` 下）                            | 目标位置（`backend/chimera-core/src/` 下） | 实际应用接线                                                                                                                   | 验证                                                                  |
| ------------------------------------------------ | ------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------- |
| `backend/nyanpasu-core/src/actor_v2/intent.rs`   | `actor_v2/intent.rs`                       | Tauri 的 `actor_v2/local_runtime.rs` 使用 core `RuntimeIntentBuilder` 构造用于 reconcile 的原始字节和 digest；旧模块改为重导出 | 上游原生测试、字节原样保留测试、Tauri local runtime 恢复/确认用例     |
| `backend/nyanpasu-core/src/client/clash_info.rs` | `client/clash_info.rs`                     | Tauri `config/clash/mod.rs` 旧 `ClashInfo` 类型改为 core 重导出，保留 YAML/Service/Agent 原有计算入口和三个 IPC 字段           | 5 项上游原生测试、旧 YAML 到 core DTO 的 wire 兼容测试、Specta 新鲜度 |

本批已通过独立 core **140/140**、Tauri runtime **6/6**、Actor V2 **9/9**、proxy **14/14**、Agent 探测 **3/3**、Specta **1/1**、前端测试 **28/28**及 E2E 框架单测 **33/33**。本地 Rustfmt、TypeScript/边界检查通过；完整 Clippy 与真实桌面/Service E2E **未验证**。

这次复制的两个模块均已被 Tauri 生产路径实际引用，而非闲置副本；但尚未实现 `actor_v2` 其他服务 Actor、完整 Client/Workflow、Unified RPC 或跨平台端到端一致性。

## 2026-11-07 Actor V2 Endpoint 公共契约迁移

本地提交 `20cf593a`：从
`ref/backend/nyanpasu-core/src/actor_v2/endpoint.rs`
复制 **Endpoint 公共契约**进入
`backend/chimera-core/src/actor_v2/endpoint.rs`，适配
Chimera 的 crate 名称、CoreError 构造与
`CoreStatusSnapshot.instance_id`。同时迁入 Local Host 操作结果与
状态到 IPC 的映射，以及 Service Host 的 v2 状态投影。

**实际调用路径已替换：** Tauri
`core/actor_v2/control_endpoint.rs` 和
`service_endpoint.rs` 现在使用 core 的统一
`ControlEndpoint` trait、`CoreSubmission`、
`CheckSubmission`、`ExecutionHost` 和状态类型/映射函数。
Tauri 保留具体的 Windows/Local 与 Service IPC Host 适配。

core 完整单测 **144/144**、Tauri Actor V2 **28/28**、
代理 **14/14**、Agent 探测 **3/3**、Specta **1/1**、
TypeScript 与前端边界以及 E2E 框架单测 **33/33**
通过；没有执行桌面/真实 Service E2E 或宣称 Clippy 通过。

**仍待完整复制/适配：** ref 的 CoreActor v2
`mod.rs` mailbox/订阅泵、`facade.rs`、
`service_actor.rs`、`local_host.rs`、
`service_endpoint_factory.rs`，以及完整的实例所有权移交与
应用状态协调。**不能因为公共契约已经迁入，就将 Actor V2
整项标记为已对齐。**

## 2026-11-07 ServiceActor 状态策略迁入 core

本地代码提交 `7ecde9ec`，参考
`ref@b7f54a4b0baf06f86c2e1675b5c5f86f96b0b866`。
从上游 `backend/nyanpasu-core/src/actor_v2/service_actor.rs`
原样提取 `ServicePhase` 和 `readiness()`，在
`backend/chimera-core/src/actor_v2/service_policy.rs`
集中实现探测状态、settled readiness / ready generation 以及有上限的自动重启决策。

**实际生产接线：**
`backend/tauri/src/client/core_lifecycle/mod.rs`
已重导出 core `ServicePhase` 并通过 core 规则处理
Service 探测、超时未知、耗尽锁存和状态代际；
`backend/tauri/src/core/actor_v2/facade.rs` 的自动重启决策
也已由 core 接管。Chimera 保留“必须证明 Service
真正拥有 Runtime 才能 Ready”的额外保护。
内部 readiness/generation 字段 `serde(skip)` /
`specta(skip)`，UI/Agent 对外字段不变。

**验收：** 独立 core **148/148**、Tauri 生命周期
**33/33**、Actor V2 **28/28**、代理 **14/14**、
Agent **3/3**、Specta 类型一致性 **1/1**，
前端测试 **28/28**、E2E 框架单测 **33/33**
和 TS/格式/边界检查通过。完整 Clippy、
实际 Service/桌面 E2E 尚未验证。

**仍未完成：** 上游完整 `ServiceActor`（OS 进程控制、
消息队列、版本门禁、自动重启和健康监测）、完整
`CoreActor` mailbox、`CoreClient` 与 facade 对齐。
Chimera 现有 Service IPC 和权限校验不同于上游，
这些部分不能直接整文件覆盖后仅通过编译即宣称完成。

## 2026-11-07 GitHub 分支推送与独立 core 版本门禁

用户要求先把已有分支提交推送，再继续迁移，并明确**不再保留旧 Chimera 接口兼容层**。已将此前所有已验证本地提交推送至
`origin/codex/clash-api-ipc-dev-1106`，推送后通过 `git ls-remote` 确认远端
`89051f0f8c504888f0ae2e8b98761e35ce649e93` 与本地 SHA 相同；没有推送未提交的
`pnpm-lock.yaml` 差异。

随后本地代码提交 `9842d0d5` 将 Service 版本门禁的**完整实现与 7 项测试**
从 `backend/tauri/src/core/service/compat.rs` **迁移**到
`backend/chimera-core/src/service/compat.rs`，并删除原 Tauri 文件；
Tauri Service IPC/状态观察/生命周期/启动更新和 IPC DTO 现在直接引用
core 的 `ServiceCompat`、`parse_service_version`，不是再保留一份旧实现。
依照 ref `backend/nyanpasu-core/src/service/compat.rs` 的纯 semver
判定和 fail-closed 分类，将服务版本判断归于独立 core。新增直接依赖
`semver`，按真实依赖更新 `backend/Cargo.lock`。

**分阶段的版本约束：** 上游当前 Service 的最低版本为 `2.0.2`，但当前仓库实际打包的
`backend/chimera-runtime/chimera_service` 仍为 Chimera 自身 `1.10.x` 版本；
本批不伪造已经完成上游 Service 可执行文件迁移，仍按**当前实际可运行服务**
设定 `REQUIRED_SERVICE_MIN = 1.10.1`。后续迁移 Service 二进制与
新协议时应同时切换版本门禁；这是分阶段一致性，而非要求保留旧兼容层。

**验证：** core 完整单测 **155/155**；Tauri Service IPC
**7/7**、生命周期 **33/33**、Actor V2 **28/28**、
Agent **3/3**、Specta 生成绑定一致性 **1/1**；
前端相关单测 **28/28**、界面/main TypeScript 和前端边界、Rustfmt
均通过。完整 Clippy 与真实 Service/桌面 E2E **未执行**。
`ServiceActor` mailbox、OS 服务进程控制、核心所有权切换与 Unified RPC
仍是待迁移任务。

## 2026-11-07 ServiceActor 状态投影所有权实际迁移

本地代码提交 `5191b34b`（基于已 rebase 的
`codex/clash-api-ipc-dev-1106@75a1f8d3`），继续对齐
`ref/backend/nyanpasu-core/src/actor_v2/service_actor.rs` 的
`ServiceHostStatus` 和 `ServicePhase` 状态机。

- 新建 `backend/chimera-core/src/actor_v2/service_status.rs`：
  **core 唯一拥有** Service daemon 状态投影、实际探测分类、
  `ready_generation`、transitional/Unknown 状态、耗尽锁存与显式重启策略。
  其中运行时所有权证据由 OS adapter 作为参数传入，而不是在独立 core
  引用 Tauri 全局路径。没有新增 Tauri 类型兼容重导出。
- 将 `ServiceRestartPolicySnapshot` 从 Tauri
  `core/actor_v2/facade.rs` 移到
  `chimera-core/src/actor_v2/service_policy.rs`，
  Tauri facade、Client lifecycle port、workflow、Service adapter
  直接使用 core 类型。
- 删除 `backend/tauri/src/client/core_lifecycle/mod.rs`
  原来的 `ServiceHostStatus` struct 和探测/状态推导 impl。
  其生产消息处理改为 core 的 `observe_probe`、
  `with_phase`、`on_probe_failure`；
  Agent 诊断和 Tauri IPC 直接引用 core 的真实类型，
  同时保持现有 Service 探测的 runtime-owner 证据检查。
- 4 项新的 core 单测锁定“不可信探测不等于停止”、
  ready generation、耗尽锁存必须由显式重启策略解除、
  外部 runtime 不能直接 Ready 和 internal 状态不泄露到 IPC。

**验收：** `chimera-core --lib` **159/159**，Tauri 生命周期
**33/33**、Service IPC **7/7**、Actor V2 **28/28**、
Agent 核心探测 **3/3**、Agent 诊断 **6/6**、
Specta 绑定新鲜度 **1/1**，main/legacy TypeScript、
前端边界检查、相关前端单测 **28/28**、
E2E 框架单测 **33/33**、Rustfmt 通过。
完整 Clippy、真实 Service/桌面 E2E 未执行。

**仍待完成：** ref 的 `ServiceProcessControl`/
`ServiceCommandError` 端口、ServiceActor OS 命令 mailbox、
权限/安装/升级/健康监测和 CoreActor 路由。此批仅转移
实际状态所有权，不声称整个 ServiceActor 已完成迁移。

## 2026-11-07 runtime Service 停机、日志与主仓库 gitlink 对齐

用户明确要求 `backend/chimera-runtime` **也直接参考上游并参与实际迁移**。
主仓库参考锁定 `ref@b7f54a4b`，其
`backend/nyanpasu-runtime@a27c3fcc` 为本批上游依据；
不能误用 Chimera runtime 自身较旧的
`backend/chimera-runtime/ref@7509cbf8`。

**独立 runtime 子模块：** 在
`backend/chimera-runtime` 的本地工作分支
`codex/ref-runtime-alignment-1107` 中提交
`6a92c2ce5bd0f33d4f414c39c7e625349a510d21`。
从上游
`nyanpasu-runtime/nyanpasu_service/src/main.rs` 复制并接入
`run_until_signal`（含 4 项测试），将原先收到 Ctrl+C
就丢弃 `handler()` future 的做法改为：若 Service 已启动，
取消真实 `SERVER_SHUTDOWN_TOKEN`、等待服务器完成
core/DNS 关闭并排空管道，第二次信号允许结束等待；其他
CLI 命令仍即时结束。此项变更**已经接入运行路径**。

同一提交参考
`nyanpasu-service-runtime/src/cmds/server.rs`：
特权 Service 启动日志只输出 `std::env::vars_os()` 的
**环境变量名称**，而非记录含凭据的值；独立测试验证
用户名、密码和 token 值不会进入日志字段。
参考上游 `nyanpasu_service/src/win_service.rs`：
Windows SCM STOP_PENDING 以 2s 间隔刷新 checkpoint，
最多报告 90s，期间等待正常 Service/core shutdown；
之后停止伪造进度（仍等待真实 handler 结束）。
另修正两项跨平台测试夹具使用
`CoreType::get_executable_name()` 或
`std::env::consts::EXE_SUFFIX`，避免 Windows 上把
`.exe` 路径误当失败。这些更改同时涵盖
`chimera_service` 与 `chimera_ipc`。

**本批运行验证：** Windows runtime `chimera-service --bin`
**55/55**、`chimera-ipc --lib` **14/14**、
`chimera-core-manager --lib` **121/121**
（另有原本标记 ignored 的 1 项）；Service/IPC/Manager
`cargo check --locked`、整个 runtime Rustfmt 通过。
主仓库 core **159/159**、Tauri Service IPC **7/7**、
Lifecycle **33/33**、Actor V2 **28/28**、Specta **1/1**、
完整 `pnpm typecheck`、前端边界、主仓库 Rustfmt、
`pnpm check:service-runtime` 通过。
**未执行**真实 Windows SCM / privileged Service、
DNS/TUN 实机集成或完整 Clippy。

**发布与合并前置条件（尚未完成）：**
`scripts/src/prepare/binaries.ts` 根据
`chimera_service/Cargo.toml` 的
`v1.11.1` 下载远程已发布的 Chimera_Service sidecar；
`scripts/check-service-runtime.ts` 只验证 Service/IPC 版本字符串相同，
**不**验证新 gitlink 与实际 release 二进制是否对应。
因此本批新源码不会自动进入已发布的 `v1.11.1` 安装包。
必须先使 `6a92c2ce` 对应的子模块 commit 可由远端读取，
随后构建、测试、发布匹配版本的新 Service 二进制、
调整版本与所需最低版本门禁并验证 prepare/打包，
才能宣称已发布 runtime 与该源码一致。
本批只作本地提交，不推送、不发布、不声称完整上游
Service/IPC/OS 进程控制架构已迁完。
