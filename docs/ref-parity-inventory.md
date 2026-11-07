# Chimera ↔ ref 全量架构对齐清单

**核对基线：** Chimera 本地分支 `codex/clash-api-ipc-dev-1106`，已固定的干净参考子模块 `ref@8cf63276e387afc621d46b950ab72f9268f915e7`（2026-11-07）。此处对齐对象是**当前已固定的 ref 子模块**；不能据此宣称它是 GitHub 最新提交。

**目标：** 使 Chimera 在代码职责、协议契约、事务语义、可观察状态、前端交互以及运行时行为上尽量与 ref 对齐，同时保留 Chimera 品牌、独有功能、Agent、main/legacy 双 UI 和 `backend/chimera-runtime` 所绑定的服务协议。不要求两个项目的包名、文件数量或字节内容完全相同；不允许借助整体覆盖代码“达成”表面一致。

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
| 5    | `backend/nyanpasu-core/src/state/{application,profiles,mutation,clash_config,...}`                             | `backend/chimera-core/src/state/`、`backend/tauri/src/config/`                                      | 迁移多参与者事务、持久化和失败结算、Profile jobs、状态变更广播；既有配置/迁移格式必须兼容                                                    |
| 6    | `backend/nyanpasu-core/src/{service,logs,geo,traffic,storage,utils,profile_import,...}`                        | `backend/tauri/src/{core,client,feat,...}`，独立 `chimera-traffic`/`chimera-jobs`/`chimera-geodata` | 按职责迁移业务处理，不重写已有独立 crates；检验 service IPC、geo 缓存、traffic 持久化及恢复，保留用户数据                                    |
| 7    | `backend/tauri/src/{unified_rpc.rs,adapters/}`                                                                 | `backend/tauri/src/ipc.rs`、`specta_export.rs`、`frontend/interface/src/ipc/`                       | 建立与 ref 对齐的可审计 Unified RPC/typed errors/transport；同时保留既有 Tauri IPC 作为兼容层，不能一次性删掉 Agent/legacy 依赖              |
| 8    | `frontend/{query,rpc,platform}/src`                                                                            | `frontend/interface/src`、`frontend/chimera/src`                                                    | 抽离 query/rpc/platform 层、统一缓存键、mutation 结果和失联处理；逐页迁移 main/legacy；测试窗口、系统代理、profiles、连接/流与设置           |
| 9    | `frontend/nyanpasu/src`                                                                                        | `frontend/chimera/src`                                                                              | 按行为逐页检查差异；保留 Chimera 主题、标识、Agent 诊断及功能，避免换回 Nyanpasu 品牌                                                        |
| 10   | 跨系统集成测试与文档                                                                                           | 现有 `frontend/chimera/tests`、`tauri-e2e` 等                                                       | Windows 主/旧 UI + Service 模式的真正端到端用例、实例切换/升级、数据迁移、权限、流量历史与回滚；再视 CI 能力扩展其他平台                     |

## 每批迁移的验收门槛

1. 在原目录 `I:\mfsga\Chimera` 的当前分支操作；不使用替身 worktree，不修改 `ref` 工作树，也不改变 `backend/chimera-runtime` 的 gitlink（除非经过明确版本核对与必要性验证）。
2. 先确认原始未提交差异，按路径和 hunk 精确暂存；绝不混入无关的 `pnpm-lock.yaml` 改动。
3. 迁移**所有权和生产调用路径**，而不仅仅是复制 DTO 或改名。旧 UI/Agent 仍有调用时保留明确兼容投影。
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
