# Chimera 完整对齐 ref `dd42ab212`：架构迁移基线与验收清单

## 1. 固定基线与决策

- Chimera 起始 `master`: `6e213838929e63a62675745e34dc745930197f2e`（已与 origin 同步）。
- 原 ref：`59724e85c34abb6995c22d101c876cc06dc16576`（2026-11-08）。
- 本轮最新 ref：`dd42ab212bcab4c61601a8c6922b26231609ba49`（2026-11-17 08:51 UTC，`origin/main`）。自旧版起增加 **160** 次提交、涉及约 **1122** 个路径。其自身 `nyanpasu-runtime` submodule 已对齐到 `3854f2c06331be0b36bb69adc59708802f661e57`。
- 最新 ref 固定 Rust `nightly-2026-11-17`；Chimera 暂仍固定 `nightly-2026-11-09`。先把 **ref Git submodule 更新并固定**，但不在同一提交盲目更换 Chimera Rust 工具链、lockfile 或 runtime gitlink。
- **目标是架构和完整功能面的上游一致性**，不再以单个小函数迁移作为完成标准。原有 Tauri `CoreLifecycle` + `ApplicationWorkflow` 双流程必须由上游的单一 `RuntimeReconciler` 取代；此处列出完整依赖顺序，逐批迁移、逐批验证和可回滚。

## 2. 上游真正的架构变化（必须迁移）

| 范围                    | 最新 ref 实现                                                                                                                                       | Chimera 当前情况                                                                                                | 完整对齐验收                                                                                                                                       |
| ----------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| Core 运行时所有权       | `backend/nyanpasu-core/src/client/reconciler/{mod,workflow,plan,apply,startup,...}.rs`：**一个 actor mailbox**，一个 reconcile round                | `backend/tauri/src/client/{core_lifecycle,application_workflow}` 两组旧控制流程；共享 CoreFacade 主要作只读 API | 将启动、显式 Start/Stop、Retry、Profile、Service、Host 切换、二进制更新汇入一个 actor；移除旧流程的生产写路径，不能保留第二状态拥有者              |
| 收敛决策                | `reconciler/plan.rs`：以已提交 target、观测事实、explicit run intent 和上次尝试做决定；`Converged/Inactive/Waiting/NotInEffect/Unknown/Closed` 区分 | 老恢复/候选/重试策略分散在 Workflow、CoreLifecycle 与本地 Runtime                                               | 精确复制决策语义和状态报告；未知结果只在事实明朗后重试；**只有显式 Stop 才保持停止**；等待不伪报成功                                               |
| 提交语义                | `reconciler/check.rs`, `committed.rs`, `mutation.rs`：预检查候选、**先提交 domain 源**、再通知收敛；失败是 committed-deferred                       | 有些 Chimera 事务仍在提交前执行 live apply 并尝试回滚                                                           | Application/Clash/Profiles 都改为统一 commit-first；保存成功而 Core 尚未应用时明确返回 deferred；旧 legacy mirror 在提交后同步，不自行控制运行时   |
| Runtime Build Port      | `reconciler/ports.rs`：capture_content、core_spec、build、publish 由 `runtime_build.rs` 的 shell ports 注入                                         | 纯 Runtime Builder、Artifact、Freeze 已抽入 `chimera-core`；但 CoreSpec 及确认后 publish 仍分散                 | 与单一 Reconciler 组装同一 builder port；candidate 一次 capture/一次 resolve；CoreSpec 由适配器提供；publish 只在已确认的 apply 和事务栅栏之后进行 |
| State engine            | `backend/nyanpasu-state-engine`：版本化持久状态与 commit/source snapshot；旧状态引擎接口删除                                                        | Chimera `chimera-core/src/state` 有独立演化的持久状态、legacy Draft/Bridge                                      | 以 ref state-engine 的版本/通知契约作为唯一来源，逐 domain 消除双写和隐式状态重建；保留 Chimera legacy UI 薄适配层                                 |
| CoreFacade/ServiceActor | ref 最新 `actor_v2/{facade,service_actor,endpoint}.rs`                                                                                              | 已从较旧 ref 迁移，额外的 Chimera service command / routing 扩展                                                | owner 唯一；Service EnsureReady 不暗中接管 Core；stopped 的旧 revision 不能成为 next reconcile 的 CAS；endpoint/instance/generation 证据必须匹配   |
| Runtime product         | `client/{runtime,artifact_snapshot,runtime_inspection}.rs`，单一确认后产品发布                                                                      | Chimera 保留 `ConfigSnapshotsGraph` 内存快照及 `.candidates` 原子提升                                           | 完整移植新 ref 的 snapshot/data flow 时一次改动持久化、inspection 与 IPC，不允许迁移一半导致前端引用失效                                           |
| Effects / 状态事件      | ref `client/effects` 从已提交状态和确认后的 ports 生成 effect                                                                                       | Chimera 仍有部分 UI/legacy effects bridge                                                                       | effects 只读已提交状态/确认的绑定；独立 SystemProxy/TUN/DNS 恢复不阻塞 source commit；保留本机网络控制扩展                                         |
| RPC 与 remote access    | `nyanpasu-rpc`, `rpc-{protocol,wire}`, `client/{remote_access,local_socket,http_server}`, `nyanpasu-cli`                                            | Chimera 有旧 IPC/UI 通道和额外 service API                                                                      | 分层引入 capability/auth、local socket、HTTP remote 与 CLI；权限必须拒绝越权写入，先补服务边界测试再迁移 UI                                        |
| Network automation      | `client/network_automation`、新网络选择策略                                                                                                         | Chimera 特有的 DNS/TUN/透明代理与多网卡目标                                                                     | 迁入 ref 的网络切换 actor 并适配 Chimera Host/Gateway 模式；保留 Fake-IP、防 DNS 泄漏、局域网信任 DNS 等独特设计；独立网络回归                     |
| UI/设置/构建            | 最新 ref frontend、settings、CLI、scripts、nightly 与发布工具                                                                                       | Chimera 产品名、传统 UI、proxy folder layout、构建流程有差异                                                    | 对照 ref feature/route/schema、交互与 IPC 契约做完整功能矩阵；应用产品外观保留品牌扩展，但不把旧框架继续作为并行实现                               |

## 3. 顺序与原子交付范围

1. **B0 最新 ref 基线**（本轮）：更新 ref gitlink；核对嵌套 submodule；提取新的单 actor/commit-first 状态机验收条件；同步 `actor_v2::CoreFacade` 最新已确认差异。Chimera 仍可按原路径构建与测试，**此批不宣称架构已经完全迁移**。
2. **B1 StateEngine / commit source**：复制 ref 版本化状态快照、domain source commit 通知与 commit status；引导 typed Application/Clash/Profiles 通过这个唯一路径。要求持久化失败不会对正在运行 Core 产生效果，通知版本精确单调。此时保留旧 actor 仅作为兼容外壳。
3. **B2 Reconciler 核心与 ports**：以 ref `reconciler/{plan,workflow,mod,ports,inputs,preparation,check}.rs` 为结构主体完整迁移。冻结 Profile 输入、CoreSpec、脚本 runner 与 RuntimeSnapshot 纳入一个 injected BuildPort。禁止再次复制旧 ApplicationWorkflow 策略到新目录。
4. **B3 唯一 Runtime apply**：移植 ref `reconciler/{apply,startup,committed,mutation}.rs`，使所有启动、Stop/Retry、Host 转移和 committed changes 只从 reconcile round 提交。一次 commit 做一个清楚的可测试行为闭环；最后移除旧生产写路径和无用的只读 facade 副本，改由 owner 暴露状态。
5. **B4 Side effects / RPC / UI**：迁移新 ref effects、网络自动化、RPC/remote access、CLI、配置 schema、前端差异。每项按 upstream 完整模块复制，再以 Chimera Client/透明代理/品牌需求做单独增量扩展；不要让兼容层成为新的状态拥有者。
6. **B5 发布验证**：跨 macOS、Windows 与 Linux 打包及启动，Service/Local、热重启与手动 Stop、任意 domain 失败、网络切换和 Fake-IP/DNS 泄漏验证；所有层通过后更新发布工具链与版本配置。按需同步 `backend/chimera-runtime` 的对应更新，不盲目把 ref 的 runtime gitlink 塞进 Chimera。

## 4. 不可妥协的测试/安全门槛

- 已提交的 Application/Clash/Profile 发生写失败时，运行中的 Core 不变；提交成功但应用失败时明确 `CommittedDegraded` 或等效 deferred，而不是误报回滚。
- 启动、显式 Start/Stop、Retry、Core 升级、Service 退化/恢复、Host Handoff 均走 **同一**收敛函数；停止不等于“禁止未来启动”，显式 Stop 才保持停止意图。
- 二次确认：同一候选内容的 check/apply bytes 相同；只对属于当前实例及 host generation 的结果执行 promotion；未知结果不伪造成功；不得操作其他实例的 Service daemon。
- 运行时产物始终先验证、再被 Core 接受、再发布；旧数据/未完成状态不能通过另一条写路径覆盖已确认配置。
- `Profile` 闭包与冻结内容 hash 稳定，staged bytes 不改写本地源；强类型 Service/TUN/transparent-proxy 错误保留；支持 Chimera Client MMDB。
- `cargo test` 全量 Core + Tauri、Rustfmt、Clippy、`git diff --check`、TypeScript bindings/类型检查、桌面实机/CI、子模块状态均为发布门槛。每批次使用 GPG 签名，优先 rebase，不 squash、不覆盖别人的远程提交。

## 5. 上游破坏性变更与 Chimera 扩展

- ref 已移除 `ClashPremium` 与 Script proxy mode；这是上游真实的破坏性改变，不能假称二者仍受最新 ref 支持。迁移中先标注 Chimera 当前对这两者的使用/测试/用户持久配置；如果它们不属于需要保留的 Chimera 独有功能，按上游删除，并提供必要配置迁移与提示。
- `ChimeraClient` / `ChimeraClientAlpha` 核心类型、MMDB、MIPS TUN、透明代理 Service 模式、DNS/Fake-IP 等是 Chimera 的产品扩展。上游类型不能覆盖这些扩展；在 upstream 主干架构跑通之后以**独立 adapters/extensions**接入。
- ref 最新用 `yaml_serde` 替代 `serde_yaml_ng`；工具链使用 `nightly-2026-11-17`。更换这两项应随核心结构/依赖变更做专门验证，而不是顺带更新以求版本号一致。
- **完成标准**：不能仅凭 ref gitlink 更新、模块同名、单测绿色或抽走 pure helper 宣告“完整 ref 对齐”。必须在运行时不再保留两套写路径、UI/IPC 契约全量评审及平台端到端通过之后，才能标记完成。
