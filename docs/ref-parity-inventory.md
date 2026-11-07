# Chimera ↔ ref 全量架构对齐清单

**最新核对基线（2026-11-07）：** Chimera 原 Windows 工作区 `I:\mfsga\Chimera`、分支 `codex/clash-api-ipc-dev-1106`，参考子模块 `ref@a8dd5d58ac86baa7a7e9333dc3332fe98755ef3b`。已从官方 `libnyanpasu/clash-nyanpasu` 的 `refs/heads/main` 查询、fetch 并验证原基线 `b7f54a4b0baf06f86c2e1675b5c5f86f96b0b866` 是其祖先。此轮新上游提交仅修改 `manifest/version.json`（版本清单），**没有新上游架构源码变化**。历史记录保留其原 SHA；旧测试数字不是对未来版本的通过保证。

**最新目标（用户已明确修改）：** 以 `ref` 的代码职责、协议契约、事务语义、状态、前端接口与运行时行为作为迁移基准。**无需保留 Chimera 旧架构、旧 API 或旧 Service IPC 的兼容性**；允许连同调用方和运行时执行破坏性替换，删除被取代的旧实现，不构造长期双轨适配。迁移仍应避免无关改动、用户数据意外损毁和未测试状态下的功能失效；不应因为复制文件或编译通过就宣称整个架构已对齐。Chimera 的产品身份可独立于底层旧协议。

## 核对方法

- 参考 Rust 独立 core：`ref/backend/nyanpasu-core/src/`（本次审查为 **168** 个 Rust 文件）。
- Chimera 独立 core：`backend/chimera-core/src/`（本次审查为 **42** 个 Rust 文件，含同实例代理组协调与本轮 Clash Streams Actor；文件数不是完成率）。
- 参考 Tauri 适配层：`ref/backend/tauri/src/`（**83** 个 Rust 文件）；Chimera Tauri：`backend/tauri/src/`（**211** 个 Rust 文件）。
- 参考独立前端包：`ref/frontend/query/src`（**64** 个 TS/TSX）、`ref/frontend/rpc/src`（**9** 个）、`ref/frontend/platform/src`（**1** 个）。Chimera 当前相应能力主要合并在 `frontend/interface/src`（**70** 个 TS/TSX）及 Tauri IPC 中。
- 完整迁移需要比较架构与调用关系、端到端契约及其测试；文件统计不构成任何路径或行为完成比例。

## 已迁移并接入生产路径

| 能力                              | Chimera 路径                                                                                                | 已验证范围                                                                           | 局限                                  |
| --------------------------------- | ----------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ | ------------------------------------- |
| 原生 Clash DTO 和 Provider 元数据 | `backend/chimera-core/src/clash/{api,proxies}.rs`                                                           | 原生/legacy 代理图组装、Provider-only 节点、Group 类型                               | 对应 Tauri legacy DTO 暂时保留        |
| 实例绑定的 Clash API              | `backend/chimera-core/src/actor_v2/api.rs`                                                                  | Stale/Unavailable/Timeout/Protocol；请求前后身份校验                                 | 实例获取/宿主生命周期端点仍在 Tauri   |
| Client 协议投影和延迟参数         | `backend/chimera-core/src/client/clash_api.rs`                                                              | Provider、URL、expected、timeout                                                     | 其他 Client 工作流仍在 Tauri          |
| 连接速率衍生                      | `backend/chimera-core/src/clash/connection_rates.rs`                                                        | core Streams Actor 生产路径使用 core 算法                                             | 连接速率算法与历史状态现由 core 拥有  |
| Clash WebSocket Streams Actor     | `backend/chimera-core/src/clash/ws.rs`                                                                      | 实例绑定 API lease、四路类型化流、history/snapshot、重连、录制开关、事件与 traffic frames | Tauri 仅保留 Tauri Event/Channel 桥接；真实 core/跨平台 E2E 未验证 |
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
| 3    | `backend/nyanpasu-core/src/client/{application,application_workflow,profiles,system_proxy,hotkey,effects,...}` | `backend/tauri/src/client/` 和部分 `core/`                                                          | 将应用事务参与者、profiles、热键、系统代理、运行状态等核心流程迁到无 Tauri 依赖的 core；Tauri 提供 OS/UI adapter，保护 Chimera 特性          |
| 4    | `backend/nyanpasu-core/src/state/{application,profiles,mutation,clash_config,...}`                             | `backend/chimera-core/src/state/`、`backend/tauri/src/config/`                                      | 迁移多参与者事务、持久化和失败结算、Profile jobs、状态变更广播；允许新格式，必要时明确迁移用户数据而非保留旧 API                             |
| 5    | `backend/nyanpasu-core/src/{service,logs,geo,traffic,storage,utils,profile_import,...}`                        | `backend/tauri/src/{core,client,feat,...}`，独立 `chimera-traffic`/`chimera-jobs`/`chimera-geodata` | 按职责迁移业务处理，不重写已有独立 crates；检验 service IPC、geo 缓存、traffic 持久化及恢复，保留用户数据                                    |
| 6    | `backend/tauri/src/{unified_rpc.rs,adapters/}`                                                                 | `backend/tauri/src/ipc.rs`、`specta_export.rs`、`frontend/interface/src/ipc/`                       | 以 ref Unified RPC/typed errors/transport 作为新契约，**同步迁移并删除**旧 Tauri 命令与调用方；不保留旧兼容层                                |
| 7    | `frontend/{query,rpc,platform}/src`                                                                            | `frontend/interface/src`、`frontend/chimera/src`                                                    | 抽离 query/rpc/platform 层、统一缓存键、mutation 结果和失联处理；逐页迁移 main/legacy；测试窗口、系统代理、profiles、连接/流与设置           |
| 8    | `frontend/nyanpasu/src`                                                                                        | `frontend/chimera/src`                                                                              | 按行为逐页检查差异；保留 Chimera 主题、标识、Agent 诊断及功能，避免换回 Nyanpasu 品牌                                                        |
| 9    | 跨系统集成测试与文档                                                                                           | 现有 `frontend/chimera/tests`、`tauri-e2e` 等                                                       | Windows 主/旧 UI + Service 模式的真正端到端用例、实例切换/升级、数据迁移、权限、流量历史与回滚；再视 CI 能力扩展其他平台                     |

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

## 2026-11-07 runtime 默认分支合并 + core-owned ServiceProcessControl

**最新源码核对：** 对 `libnyanpasu/nyanpasu-runtime` 的
`a27c3fcc1ed57980034f25084613baa98efc01c4` 与公开
`main@041b662fe6aae22a80302b190f49e762774dbf50`
执行 GitHub compare，后者只前进 **6 个提交**，唯一修改文件为
`rust-toolchain.toml`；本批迁移所参考的 runtime Service
实现与最新上游代码相同。主仓库的 `ref` pin 没有擅自更新。

**子模块合并完成：** `MFSGA/Chimera_Service` 默认分支实际为
`main`（无 `master`）。在子模块
`codex/ref-runtime-alignment-1107` 上检查/rebase
`origin/main@305390f6`：共同祖先正是 `305390f6`，
所以无需改写提交；以**非强制快进推送**合并到
`origin/main@6a92c2ce5bd0f33d4f414c39c7e625349a510d21`。
远端 SHA 已再次核对。子模块本地备份分支为
`backup/runtime-pre-main-merge-20261107`。
先前主仓库 `f579fe7c` 的 gitlink 已指向此 commit，
现在对 GitHub CI 可以直接获取。只有源码发布与下载的
`v1.11.1` Service 二进制是否匹配仍未证明。

**主仓库代码提交 `9ced8884`：** 根据上游
`ref/backend/nyanpasu-contracts/src/service.rs`，
在独立 `backend/chimera-core/src/actor_v2/service_process.rs`
建立 `ServiceProcessControl` 的命令/错误契约：
`ServiceCommand`、`ServiceCommandError`、
status JSON 解析、超时与仍在运行的区别。
OS 端实现由
`backend/tauri/src/core/service/control.rs`
的 `ServiceOsProcess` 提供，执行安装、卸载、启动、
停止、升级、重启及状态探测。独立 core 不引用 Tauri
的提权或子进程 API；所有 Tauri facade、生命周期、
IPC 状态观察及初始化调用方都直接使用该实现和 core
的结构化错误，不再保留旧的直接命令 API。

与上游差异明确保留为迁移工作，而非完成项：
当前 Chimera Service CLI 支持单独的 restart 命令，因此
进程端口暂提供 `restart_daemon`，其余类型对齐上游
语义。完整上游 `ServiceActor` 的消息队列、
会话结束保护、提权许可判定与 mutation 超时治理
尚**未**迁移，`ServiceOsProcess` 不能代替完整 Actor。

**验证：** core **163/163**，Tauri Service IPC
**7/7**、生命周期 **33/33**、Actor V2 **28/28**、
Agent probe **3/3** 和 diagnostics **6/6**，
Specta **1/1**；Tauri 非测试配置 `cargo check`、
完整前端 `pnpm typecheck`、UI **27/27**、
E2E 框架 **33/33**、runtime 版本一致性、
前端边界和 Rustfmt 通过。尚未执行真实
Service/SCM/提权 E2E、全项目 Clippy；
`v1.11.1` 已发布的二进制仍需单独更新/验证。

## 2026-11-07 Service 健康监视器单实例与停止信号（本批）

**参考基线：** 已核实官方 `libnyanpasu/clash-nyanpasu`
`main@b7f54a4b0baf06f86c2e1675b5c5f86f96b0b866`，
对应 `ref/backend/nyanpasu-core/src/actor_v2/service_actor.rs`
的 `SetHealthCheck` / 带 `round` 的 `HealthCheck` 与单一
ServiceActor mailbox。**本批未迁入 mailbox**，只修复当前
Tauri 生产宿主适配层在完整 Actor 落地前的并发缺口。

**目标 / 风险 L2：** 健康检查只允许一个实例运行，Service
停止或卸载成功后可靠发出停止信号，不允许旧启动线程在竞争中
清除新状态。**非目标：** 不切换 Service IPC 协议、Service
二进制、前端接口或主/旧 UI；不声明运行时实例所有权与 ref 全部一致。

- `backend/tauri/src/core/service/ipc.rs`：
  `ensure_health_check` 通过原子 CAS 在创建线程前取得所有权，
  线程退出/异常退出由守卫释放；线程创建失败后可重试。
  新增 12 个并发调用者的回归测试，证明只能有一个启动许可，
  以及释放后允许下次启动。
- `backend/tauri/src/core/service/control.rs`：
  `ServiceOsProcess::stop_daemon` / `uninstall` 在 OS 命令
  成功后用 Release store 发出停止标志，不再忽略弱 CAS 失败。
- `backend/tauri/src/core/service/mod.rs`：
  启动探测路径也统一使用 `ensure_health_check`，
  避免绕过原子单实例入口。

**本批验收：** Windows 主仓库 core 单测 **163/163**，
Tauri Service IPC **8/8**（含并发准入测试）、Actor V2 **28/28**、
CoreLifecycle **33/33**、Specta 类型新鲜度 **1/1**、
完整 `pnpm typecheck`、前端 main/legacy 边界、定向 Rustfmt
和 Git diff 空白检查均通过。
Tauri 测试使用仅对命令生效的
`TAURI_CONFIG='{"bundle":{"externalBin":[]}}'`，
因为本地缺少打包用 sidecar 文件；没有改变生产打包配置。
真实健康线程取消、Windows SCM/提权 Service、跨进程并发、
真实桌面/Service E2E 和完整 Clippy **未验证**。

**回滚 / 后续：** 可按本批代码提交回退三个 Tauri Service 文件；
没有持久化数据迁移。继续迁移上游完整 ServiceActor mailbox，
实现 `CommandSettled`、`DaemonNotRunning`、延迟超时操作追踪、
健康监视器轮次取消和 facade 所有权/失败关闭验收。此批只修复
在过渡期的单一宿主线程入口，不将 ServiceActor 主干标记完成。

## 2026-11-08 Clash WebSocket Streams Actor 迁入独立 core

本批基于 ref@b7f54a4b0baf06f86c2e1675b5c5f86f96b0b866，在隔离分支
codex/ref-streams-actor-20261108 完成本地迁移；原分支的 29 个本地提交不在此分支历史中，
ref gitlink 保持不变，也没有改动 runtime 子模块。

**所有权与生产调用路径：**
- 将四路 Clash WebSocket streams、实例绑定 API capability、流 worker、
  reconnect、stream epoch、history/snapshot、recording、memory/log/traffic
  事件和连接 frames 迁入 backend/chimera-core/src/clash/ws.rs。
- Core ApiClient 增加 capability identity / revocation 判断与有界流读取；
  Actor 只接受当前有效实例的消息，切换或撤销后清空旧快照并拒绝迟到帧。
- Tauri ChimeraClient 持有 StreamsClient 与独立 child cancellation token；
  ChimeraClientInner drop 时取消 Streams Actor，避免客户端销毁后流任务遗留。
  Tauri 层只保留 Event 转发、连接详情 Channel、CoreLifecycle API lease source
  和流量 recorder 的 frames 订阅。配置变更会更新日志 WebSocket 等级；Agent
  诊断和 IPC 直接读取 core snapshot，不再持有旧 Tauri Stream Actor.
- 新增 capability 同实例、旧实例撤销、断线/重连、四路流、录制开关、
  history 清理、订阅者关闭、日志等级重启和迟到帧 fence 测试。

**验证（Windows，本机）：**
- cargo +stable test --offline --manifest-path backend/Cargo.toml -p chimera-core --lib：
  **176 passed, 0 failed**。
- TAURI_CONFIG={"bundle":{"externalBin":[]}} cargo +stable test --offline
  --manifest-path backend/Cargo.toml -p chimera --lib：
  **773 passed, 0 failed, 1 ignored**；Specta bindings freshness 在该套件内通过。
- cargo +stable clippy --offline ... -p chimera-core --all-targets 与
  TAURI_CONFIG=... cargo +stable clippy --offline ... -p chimera --all-targets
  均完成；只有现存 workspace warnings。Tauri clippy 使用空 externalBin
  配置避开当前 checkout 缺失的 sidecar 资源。
- Interface tsc --noEmit、前端 IPC boundary、rustfmt +stable --check
  和 git diff --check 通过。仓库 hook 所要求的
  nightly-2026-11-06 未安装，因此 pnpm lint:rustfmt hook 无法启动；
  已用本机 stable Rustfmt 独立检查。未运行真实 core/桌面 E2E 或跨平台测试。

本批只作本地提交，不推送、不启动桌面程序。无关的
pnpm-lock.yaml 改动按用户授权丢弃；原工作差异保留在 stash 备份中。

## 2026-11-07 ServiceActor 命令完成与停机证明（本批）

**参考基线：** 官方 main 与 ref 均为 `b7f54a4b0baf06f86c2e1675b5c5f86f96b0b866`。直接比对 `ref/backend/nyanpasu-core/src/actor_v2/service_actor.rs` 的 `tracked`、`reap_outstanding`、`CommandSettled`、`DaemonNotRunning` 与 `Uninstall`。范围为 L2 Service OS 命令所有权及卸载安全，不声称已迁入完整 ractor mailbox。

- `backend/chimera-core/src/actor_v2/service_actor.rs`（新增）与 `mod.rs`：独立 core 持有 Service 命令单一串行 lane、超时后不丢弃的 helper、取消调用者后的不确定结果标志、完成后 reaping 与新探测；OS probe 失败不伪造停止证明。卸载前要求 v2 `CoreStateDetail::Stopped`，Running 但无明细、仍在切换或探测失败均拒绝；若 daemon 正运行且 core 已停，先 stop、重新探测确认 OS stopped，才 uninstall。
- `backend/tauri/src/core/actor_v2/facade.rs`：生产 `ServiceTransition` 的 install/update/start/restart/stop/uninstall/auto-restart 已经由 core-owned `ServiceCommandActor` 发起，`CoreFacade::probe_service`、`confirm_stopped` 与 transition admission 接入完成/停止证明。保留 Chimera Service IPC、Tauri OS executor、已有 host transition lock 和 restart policy。
- 回归：core ServiceActor 7/7，涵盖命令超时/拒绝重入、调用者取消仍持有 helper、停止后才能卸载、未知状态拒绝卸载、假停止拒绝卸载、已停止免 stop；core 全量 170/170，Tauri lifecycle 33/33；Tauri 非测试 `cargo check` 与完整 `pnpm typecheck` 通过，定向 rustfmt、`git diff --check` 通过。Tauri 测试/检查仅命令行设置 `TAURI_CONFIG='{"bundle":{"externalBin":[]}}'`，未改变生产配置。
- **剩余：** 此处为 ServiceActor 的命令子集，尚未迁入单一 ractor mailbox、会话终止/权限检查、epoch 健康定时器、完整状态发布及上游端点恢复策略；未进行 Windows SCM/UAC/真实 Service 二进制 E2E、全量 Clippy、全部前端/UI E2E。单独 host 健康线程仍在过渡期。后续优先完成 mailbox/epoch 迁移，并在物理 Windows 环境验证命令超时与真实停机。
- **回滚：** 仅回退上述三个后端文件；无持久化数据、IPC wire、前端 API 变更。未触碰已有 `pnpm-lock.yaml` 用户差异。

### 提交后补充核验（`be07ad9566641732af2e958156bb5edda032490d`）

以上代码已经位于原分支 `codex/clash-api-ipc-dev-1106` 的本地提交，
尚未推送。对提交后的实际调用链追加复核如下：

- core `cargo +stable test --offline --locked -p chimera-core --lib`：
  **170/170**，其中新 Service 命令单测 **7/7**。
- Tauri（仅测试时通过 `TAURI_CONFIG` 排除缺失的打包 sidecar）：
  `core::actor_v2::` **30/30**（含新增 facade 取消/停机证明 2 项），
  `client::core_lifecycle::tests::` **33/33**，
  `core::service::ipc::tests::` **8/8**，
  Agent diagnostics **6/6**，Specta 绑定新鲜度 **1/1**；
  非测试 `cargo +stable check --offline --locked -p chimera` 通过。
- 完整 `pnpm typecheck`、`pnpm lint:frontend-boundaries`、
  Rustfmt 和全工作区 `pnpm lint:clippy`（`--all-targets --all-features`）
  通过，Clippy 仍存在已有警告。E2E 框架单测 **33/33**。
- interface 测试首次从仓库根目录直接运行，**53/54 通过、1 个模块加载失败**：
  `traffic-report-query.test.ts` 的 `@interface/utils` 未按子包 tsconfig 映射。
  显式设置 `TSX_TSCONFIG_PATH=frontend/interface/tsconfig.json` 后，
  完整 interface 测试 **56/56** 通过；main UI `test:traffic` **16/16**。
  不将最初的执行失败冒称成功或把测试入口问题计为已修改源码。
- **仍未验证：** 真正的 Windows SCM/UAC 提权、Service 二进制、
  main/legacy 桌面 UI E2E、断电/进程终止恢复以及跨平台集成。
  上游完整 ServiceActor ractor mailbox、watch/健康轮次与
  `CoreActor` 统一所有权仍未迁入；上述验证不证明全量 ref 已对齐。

## 2026-11-07 Service 健康检查轮次令牌（增量，非完整 Actor）

**官方 ref 基线** `a8dd5d58ac86baa7a7e9333dc3332fe98755ef3b`：
最新上游仅追加 `manifest/version.json` 的版本清单提交，
此前 ServiceActor 行为仍以
`ref/backend/nyanpasu-core/src/actor_v2/service_actor.rs`
的 `SetHealthCheck` / `HealthCheck { round }` 为准。

- **风险与目标 L2：** 真实 Tauri Service 监视线程执行缓慢状态探测后，
  如 OS stop / uninstall 已成功，旧探测不得将已停止的 Service
  再作为新的运行状态投影。不能只复制生成 token 的模型，必须在
  当前生产状态发布路径使用。
- **独立 core：** `backend/chimera-core/src/actor_v2/service_actor.rs`
  新增 `ServiceHealthRounds`，原子地切换奇偶代际，重复 enable /
  disable 幂等；旧代际在停用后以及再启动时均无效。新增 2 项
  代际和旧响应测试，不改变现有命令超时/完成管理。
- **真实宿主调用：** `backend/tauri/src/core/service/ipc.rs`
  启动时取得 round；监视线程轮询、状态探测完成时，以及等待
  endpoint-down 异步处理返回后再次校验，旧轮次不再更新
  `ChimeraClient` 的 Service 状态。`control.rs` 的 OS stop /
  uninstall **成功之后** 才调用 `stop_health_check` 同时注销
  当前轮次并置停止标志；失败则不伪称已停。
  新增一项通过真实宿主停止钩子撤销轮次的测试。
- **验收：** core **172/172**，Service IPC **9/9**，CoreLifecycle
  **33/33**，Actor V2 **30/30**，Specta 新鲜度 **1/1**；
  完整 `pnpm typecheck`、main/legacy 边界、interface
  **56/56**、Service runtime 版本一致性、Rustfmt、Git diff 检查、
  全目标/特性 Clippy 均通过（有既有警告）。
  使用临时 `TAURI_CONFIG` 排除缺少的打包 sidecar；未改变发布配置。
- **仍待：** 这不是 ref 的完整单 mailbox/watch 所有权迁移；
  真实 Windows SCM/UAC、进程快速 stop→start、Service 进程 crash、
  探测结果与停止命令纳秒级竞争、桌面 main/legacy E2E 尚未验证。
  Tauri 旧健康线程/全局 IPC 投影仍存在，必须在完整 ServiceActor
  迁移时替换。无存储格式、Specta wire、前端协议改变。
  后端代码可独立回滚三个文件，ref 指针已在独立提交中更新。


## 2026-11-07 失效健康线程退出时的 IPC 发布保护（后续增量）

- **基线：** 本地 `ref@a8dd5d58ac86baa7a7e9333dc3332fe98755ef3b`；本轮尝试重新查询官方 `main` 时 GitHub 连接被重置，**未验证是否存在更新 SHA**，因此不移动 ref 指针。上游 ServiceActor 的 mailbox/round 取消是行为参照。
- **生产修复：** `backend/tauri/src/core/service/ipc.rs` 的旧健康线程在 OS stop/uninstall 撤销轮次后退出时，不再无条件调用 `set_ipc_state(Disconnected)`。此前即使轮次校验阻止了迟到探测结果，线程退出仍可能覆盖新的同步 Service/Local handoff；现在退出只释放 monitor admission，状态由生命周期/同步交接路径发布。引入 `health_round_retired` 统一退出判定，宿主 stop-hook 测试验证撤销前后判定。
- **本批验收：** core 全量 **172/172**、Tauri Service IPC **9/9**、定向 Rustfmt 与 diff 空白检查通过。Tauri 使用临时 `TAURI_CONFIG` 排除缺失的打包 sidecar；第一次格式检查指出 import 换行问题，已修正并复检通过。
- **限制：** 未证明真实 SCM/UAC 的 stop→start 竞态完全消失；ServiceActor mailbox/watch/授权与跨进程 E2E 仍未迁入。无新 wire/持久化/前端修改，回滚仅涉及该 Tauri IPC 文件及本段文档。保留用户已有 `pnpm-lock.yaml` 差异。
