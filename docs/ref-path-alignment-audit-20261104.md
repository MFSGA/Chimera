# Chimera ↔ ref：逐目录、逐文件对齐审计（2026-11-04，第一批）

> 状态：**在执行中的路径级审计，不是“所有目录已经对齐”**。本文件记录可复现的扫描清单、已经人工核对的符号/调用链、保留差异的理由和下一批入口；每个文件只有实际检查或修改后才能标记为“已判定”。

## 固定基线和范围

- Chimera 起点：`master` = `ddad1aac71c36b01aac10f650abbf3cc8749b39e`，工作区原始状态干净。
- 固定的 **只读** 上游 `ref` = `518bf49843e1f735380e11da1ebe2a85eb8ce8c6`。
- Runtime 子模块仍固定在 `2cb1160996f912b536418cb62bda470d097c1190`。
- 对比方法：仅遍历真实 `*.rs` 文件，逐相对路径比较 `ref/backend/tauri/src/` 与 `backend/tauri/src/` 的原始字节。**同路径不代表同业务实现；字节不同不代表应覆盖。** 不将 `ref` 的构建产物、第三方依赖或尚未扫描的前端目录计入数量。
- 本批代码变更仅限 `utils/open.rs`、`utils/init/mod.rs` 和新增 `utils/init/tests.rs`。不修改上游、原始 `master`、PR #585、配置格式、Core/Service、Agent、主/Legacy UI 或 Windows 宿主状态。

## backend/tauri/src：逐目录初始清单

下表是**路径字节扫描**，不是对 316 个路径全部做完语义审查。“缺失”需要进一步阅读实际引用/模块根文件后才能判断是尚未迁移、品牌改名或合法扩展。

| 子目录 | 相同 | 不同 | ref-only | Chimera-only |
| --- | ---: | ---: | ---: | ---: |
| `(root)` | 0 | 8 | 6 | 4 |
| `bridge` | 0 | 0 | 0 | 4 |
| `bundle` | 0 | 0 | 1 | 0 |
| `client` | 2 | 43 | 39 | 3 |
| `cmds` | 0 | 0 | 4 | 0 |
| `config` | 0 | 0 | 0 | 23 |
| `core` | 2 | 46 | 33 | 15 |
| `enhance` | 0 | 3 | 3 | 10 |
| `event_handler` | 0 | 0 | 2 | 0 |
| `features` | 0 | 0 | 0 | 13 |
| `server` | 0 | 0 | 2 | 0 |
| `service` | 0 | 2 | 1 | 0 |
| `state` | 0 | 10 | 3 | 1 |
| `utils` | 2 | 14 | 8 | 4 |
| `window` | 0 | 1 | 4 | 0 |
| **合计** | **6** | **127** | **106** | **77** |

## 第一批人工判定：utils/

| 对应路径（均相对 backend/tauri/src/） | 决策 | 核对依据与本批动作 |
| --- | --- | --- |
| `utils/blocking.rs` | 已一致，保留 | 相同路径、字节完全相同；无需改动。 |
| `utils/open.rs` | **已对齐** | 两个公共函数 `that`、`with` 均保留，含 AppImage 环境变量处理与原调用路径；按上游顺序恢复整个文件，字节完全一致。调用方 `ipc.rs`/`help.rs` 无需变更。 |
| `utils/init/mod.rs` | **部分对齐：测试文件结构** | 原有 `startup_file_tests` 是内嵌模块；按上游 `#[cfg(test)] mod tests;` 抽到同目录 `tests.rs`，五项 Chimera 旧配置保护测试不删、不改变断言。上游实际初始化包含前置迁移/结构化失败处理，尚未移植。 |
| `utils/init/tests.rs` | **新建对应路径；内容不等价** | 仅移动 Chimera 现有五项启动文件保护测试，不把上游涉及子进程迁移和真实路径的测试伪称为已迁移。 |
| `utils/init/logging.rs` | 暂保留，下一批拆分 | 上游日志初始化有迁移日志/生命周期关联；Chimera 当前有 jobs capture、已验证的路径注入。直接覆盖会改变持久化与退出行为。 |
| `utils/hwid.rs` | 品牌兼容差异，保留 | 除 `clash-nyanpasu` / `clash-chimera` 盐值外一致；改盐会改变设备标识，不能复制上游品牌盐。 |
| `utils/sudo.rs` | 实际已等价，暂不做无价值修改 | 仅缺上游“read the output file”注释，提权/进程行为相同；避免为注释增加额外 diff。 |
| `utils/net.rs` | 共通逻辑待核对，**不能直接替换** | 上游 `url_delay_test/get_ipsb_asn` 接收显式 `self_proxy_port`，Chimera 使用无参 `get_reqwest_client`；涉及 `ipc.rs` 和网络代理选择，必须检查调用链和配置后统一迁移。 |
| `utils/candy.rs` | Chimera 必需扩展 + 待收敛 | `collect_logs`、现有 HTTP Client/代理和镜像站列表属于已使用的能力；上游不同代理参数与镜像站不是机械品牌替换。保留现有日志导出与下载兼容。 |
| `utils/config.rs` | 待迁移的代理配置接口 | 与 `candy.rs`/`net.rs`/下载入口配套审计，不单独调整 HTTP/系统代理语义。 |
| `utils/collect.rs` | 待逐函数核对 | 同路径但实现差别很大；先确认平台收集 API 和敏感信息边界，再定移植范围。 |
| `utils/dirs.rs` | 品牌/旧路径兼容，保留 | 现有 profile/data 根目录的发现与创建已经在 Stage 1/1b 调整；不能把上游路径名写入用户已有配置。 |
| `utils/path.rs` | Chimera 兼容适配，保留 | 上游对应路径在独立 crate `backend/nyanpasu-paths`；Chimera 当前 `PathResolver` 位于本地文件，不代表功能完全缺失。需未来路径所有权整体迁移。 |
| `utils/recovery_io.rs` | Chimera 数据保护扩展，保留 | 本地已有损坏文件恢复/安全写入用途；与 `core/corrupt.rs` 和迁移恢复配套审核。 |
| `utils/redb_ext.rs` | Chimera 扩展，需保留 | 不是上游同名文件；需结合存储消费者确认必要性，不因 ref-only 目录缺失删除。 |
| `utils/resolve.rs` | Main/Legacy 兼容，保留 | 双窗口唤起、前端 ready、托盘与 Core/Service 调用边界；依赖 #585 的原生系统验收，禁止在本批覆盖。 |
| `utils/exit.rs` | 待生命周期开箱审计 | 进程退出、服务资源所有权和真实 OS 副作用；需配套取消/重启/关闭测试。 |
| `utils/help.rs` | 待调用链扫描 | 包含文件打开及配置/外部命令兼容；先逐符号标注，再决定是否整体复制。 |
| `utils/winreg.rs` | Windows 兼容扩展，保留待审 | 涉及已安装配置目录与注册表；需专用 Windows 测试。 |
| `utils/mod.rs` | 模块清单差异，保留待分组 | 上游缺失文件和 Chimera 专属适配会改变 `mod` 依赖，必须在迁入其完整实现后统一调整。 |
| `utils/color.rs`、`core_version.rs`、`dialog.rs`、`dock.rs` | ref-only，待逐消费方核对 | 在添加新文件前先检查对应调用方和现有功能位置；避免孤立复制未装配代码。 |
| `utils/main_thread.rs`、`profiling.rs`、`proxy_env.rs`、`winhelp.rs` | ref-only，待依赖路径核对 | 分别关联主线程、分析/日志、环境代理、Windows 平台边界；不是“应直接新增”的判定。 |

## 已发现的重要差异（未纳入第一批代码修改）

| 上游 → Chimera 路径 | 判定与理由 | 后续验证 |
| --- | --- | --- |
| `core/geo/actor.rs` 和 `ports.rs` → 同名路径 | **保留 Chimera 已有保护**：Chimera 有 `CoreGeodataBinding`、`CountryIndexSnapshot`，按 Core 实例隔离 index，并提供文件 watcher 丢事件的定期刷新；上游对应实现缺少这些状态。不能为文本一致而删除。 | 对照实例切换、延迟 frame、GeoIP 地理索引测试，评估是否向上游结构收敛。 |
| `core/migration/registry.rs` → 同名路径 | **未对齐、高风险**：上游注册 profiles、app_config、typed_config、storage、application、clash_config 等模块；Chimera 当前仅 profiles 和 typed_config。不能补注册条目而没有配套 migrator/schema/文件恢复；会涉及真实用户数据。 | 优先完整扫描 `core/migration/` 的 registry → runner → modules → filesystem/recovery，再做 fixture 迁移与重复/崩溃回归。大范围持久化变更须单独授权。 |
| `state/profiles/ports.rs` → 同名路径 | **接口分叉，待完整迁移**：上游使用 `ProfileFileError`/`SubscriptionFetchError` 和远端内容 MIME/文件名元数据；Chimera 使用 `anyhow`、额外 rebuild notifier。直接复制会破坏消费者/Adapter。 | 沿 actor → service/profile_file → UI/Agent 调用链批量迁移。 |
| `core/actor_v2/intent.rs` → 同名路径 | **类型/序列化边界差异**：上游保存已序列化的 `Arc<str>`，Chimera 从 YAML Mapping 转为 `String`。这不仅是品牌名差异；直接改会影响 digest 与 Core/Service 数据所有权。 | 与 runtime builder、`chimera-core-manager`、Core lifecycle 一起迁移，测试重复 reconcile。 |
| `core/download/adapter.rs` → 同名路径 | **兼容差异**：Chimera 文档明确解释现有 reqwest `0.12` 与上游不同版本的 adapter 兼容选择；不改动依赖树。 | 对照锁文件和 HTTP retry 行为后收敛。 |
| `client/application_workflow/` → 同名目录 | **部分迁移**：上游存在 `adapters.rs`、`attempt.rs`、`preparation.rs`、`startup.rs` 和测试目录；Chimera 当前缺这些相对路径。 | 逐文件列出调用图，优先完成一个可独立测试的事务，不机械建立空文件。 |
| `ref/frontend/nyanpasu/` → `frontend/chimera/` | **品牌映射而非同路径**；Chimera 同时保留 legacy UI 和 Agent 入口。 | 后续扫描组件/hook/route/query，并用主/Legacy 双入口验收。 |

## 运行与回滚边界

- 本批仅重新排列 `utils/open.rs` 两个**原有同签名、同实现**函数，并将五项**原有、断言未修改**的启动配置保护测试移入 `utils/init/tests.rs`，使文件组织与固定 ref 对应。无生产逻辑、IPC、网络、持久化、核心管理或 ABI 变更。
- 后续扫描按**一个有界目录/完整调用链 → 源码差异 → 必要兼容 → 实际修改 → 回归/PR**推进；仅扫描到的目录都标记为“待语义审查”，不能凭自动匹配宣称已对齐。
- 回滚：撤回本批的 `utils/open.rs` 顺序调整，将 `utils/init/tests.rs` 的五项测试移回 `init/mod.rs`；用户数据与子模块无需恢复。

## 后端 Rust 文件级路径索引（仅目录/字节分类，不代表人工审查完成）

下表自动依据固定路径扫描得到，每行对应一个真实 Rust 源文件；`相同`、`不同`、`ref-only`、`Chimera-only` 只描述是否存在和是否字节相同。**未归类为已判定的文件需在后续切片逐项打开阅读。**

| 路径（相对 `backend/tauri/src/`） | 初步状态 |
| --- | --- |
| `bridge/clash.rs` | Chimera-only |
| `bridge/mod.rs` | Chimera-only |
| `bridge/verge.rs` | Chimera-only |
| `bridge/window.rs` | Chimera-only |
| `bundle.rs` | ref-only |
| `bundle/tests.rs` | ref-only |
| `client/agent_skills.rs` | ref-only |
| `client/app_lifecycle.rs` | ref-only |
| `client/app_update/adapters.rs` | ref-only |
| `client/app_update/changelog.rs` | ref-only |
| `client/app_update/mod.rs` | ref-only |
| `client/app_update/progress.rs` | ref-only |
| `client/app_update/tests.rs` | ref-only |
| `client/application.rs` | 不同 |
| `client/application_workflow/adapters.rs` | ref-only |
| `client/application_workflow/attempt.rs` | ref-only |
| `client/application_workflow/impact.rs` | 不同 |
| `client/application_workflow/inputs.rs` | 不同 |
| `client/application_workflow/mod.rs` | 不同 |
| `client/application_workflow/mutation.rs` | 不同 |
| `client/application_workflow/participant.rs` | 不同 |
| `client/application_workflow/policy.rs` | 不同 |
| `client/application_workflow/ports.rs` | 不同 |
| `client/application_workflow/preparation.rs` | ref-only |
| `client/application_workflow/profiles.rs` | 不同 |
| `client/application_workflow/startup.rs` | ref-only |
| `client/application_workflow/tcc.rs` | 不同 |
| `client/application_workflow/tests/closing.rs` | ref-only |
| `client/application_workflow/tests/connection_policy.rs` | ref-only |
| `client/application_workflow/tests/mod.rs` | ref-only |
| `client/application_workflow/tests/mutations.rs` | ref-only |
| `client/application_workflow/tests/recovery.rs` | ref-only |
| `client/application_workflow/tests/service_recovery.rs` | ref-only |
| `client/application_workflow/tests/startup.rs` | ref-only |
| `client/application_workflow/tests/validation.rs` | ref-only |
| `client/application_workflow/workflow.rs` | 不同 |
| `client/clash_api.rs` | 不同 |
| `client/clash_config.rs` | 不同 |
| `client/clash_info.rs` | ref-only |
| `client/clash_streams.rs` | ref-only |
| `client/configuration_status.rs` | ref-only |
| `client/convergence.rs` | 不同 |
| `client/core_lifecycle/adapters.rs` | 不同 |
| `client/core_lifecycle/apply.rs` | ref-only |
| `client/core_lifecycle/mod.rs` | 不同 |
| `client/core_lifecycle/ports.rs` | 不同 |
| `client/core_lifecycle/workflow.rs` | 不同 |
| `client/core_version.rs` | ref-only |
| `client/direct_egress.rs` | ref-only |
| `client/effects/actor.rs` | 不同 |
| `client/effects/adapters.rs` | Chimera-only |
| `client/effects/error.rs` | ref-only |
| `client/effects/executor.rs` | 不同 |
| `client/effects/mod.rs` | 不同 |
| `client/effects/plan.rs` | 不同 |
| `client/effects/ports.rs` | 不同 |
| `client/effects/status.rs` | 不同 |
| `client/effects/tests.rs` | ref-only |
| `client/error.rs` | ref-only |
| `client/event_sink.rs` | 不同 |
| `client/frontend_events.rs` | ref-only |
| `client/hotkey/actor.rs` | 不同 |
| `client/hotkey/adapters.rs` | 不同 |
| `client/hotkey/error.rs` | ref-only |
| `client/hotkey/mod.rs` | 不同 |
| `client/hotkey/ports.rs` | 不同 |
| `client/hotkey/tests.rs` | 不同 |
| `client/jobs.rs` | 不同 |
| `client/jobs_recovery_tests.rs` | Chimera-only |
| `client/logs.rs` | ref-only |
| `client/main_thread.rs` | ref-only |
| `client/mod.rs` | 不同 |
| `client/ports.rs` | 不同 |
| `client/profiles.rs` | 不同 |
| `client/rebuild.rs` | Chimera-only |
| `client/runtime.rs` | 不同 |
| `client/runtime_error.rs` | ref-only |
| `client/runtime_inspection.rs` | 不同 |
| `client/runtime_recovery.rs` | ref-only |
| `client/session_state.rs` | 不同 |
| `client/startup_recovery_tests.rs` | ref-only |
| `client/system_dns.rs` | 不同 |
| `client/system_proxy/actor.rs` | 不同 |
| `client/system_proxy/adapters.rs` | 相同 |
| `client/system_proxy/error.rs` | 不同 |
| `client/system_proxy/mod.rs` | 不同 |
| `client/system_proxy/ports.rs` | 相同 |
| `client/system_proxy/tests.rs` | 不同 |
| `client/traffic.rs` | 不同 |
| `client/ui_effects/adapters.rs` | ref-only |
| `client/ui_effects/mod.rs` | ref-only |
| `client/ui_effects/ports.rs` | ref-only |
| `client/ui_effects/tests.rs` | ref-only |
| `cmds/migrate.rs` | ref-only |
| `cmds/mod.rs` | ref-only |
| `cmds/panic_dialog.rs` | ref-only |
| `cmds/startup_failed.rs` | ref-only |
| `config/chimera/clash_strategy.rs` | Chimera-only |
| `config/chimera/logging.rs` | Chimera-only |
| `config/chimera/mod.rs` | Chimera-only |
| `config/clash/mod.rs` | Chimera-only |
| `config/core.rs` | Chimera-only |
| `config/draft.rs` | Chimera-only |
| `config/mod.rs` | Chimera-only |
| `config/profile/builder.rs` | Chimera-only |
| `config/profile/item/local.rs` | Chimera-only |
| `config/profile/item/merge.rs` | Chimera-only |
| `config/profile/item/mod.rs` | Chimera-only |
| `config/profile/item/remote.rs` | Chimera-only |
| `config/profile/item/script.rs` | Chimera-only |
| `config/profile/item/shared.rs` | Chimera-only |
| `config/profile/item/utils.rs` | Chimera-only |
| `config/profile/item_type.rs` | Chimera-only |
| `config/profile/mod.rs` | Chimera-only |
| `config/profile/profiles.rs` | Chimera-only |
| `config/profile/ref_adapter.rs` | Chimera-only |
| `config/profile/reservation_reconcile.rs` | Chimera-only |
| `config/profile/reservation_reconcile_tests.rs` | Chimera-only |
| `config/runtime.rs` | Chimera-only |
| `config/runtime_tests.rs` | Chimera-only |
| `consts.rs` | 不同 |
| `core/actor_v2/api.rs` | ref-only |
| `core/actor_v2/control_endpoint.rs` | Chimera-only |
| `core/actor_v2/endpoint.rs` | 不同 |
| `core/actor_v2/facade.rs` | 不同 |
| `core/actor_v2/intent.rs` | 不同 |
| `core/actor_v2/local_host.rs` | 不同 |
| `core/actor_v2/local_runtime.rs` | Chimera-only |
| `core/actor_v2/mod.rs` | 不同 |
| `core/actor_v2/service_actor.rs` | ref-only |
| `core/actor_v2/service_endpoint.rs` | Chimera-only |
| `core/actor_v2/service_host_adapter.rs` | ref-only |
| `core/actor_v2/tests.rs` | ref-only |
| `core/backup.rs` | 不同 |
| `core/backup/verify.rs` | Chimera-only |
| `core/clash/api.rs` | 不同 |
| `core/clash/connection_details.rs` | 不同 |
| `core/clash/connection_rates.rs` | ref-only |
| `core/clash/core.rs` | Chimera-only |
| `core/clash/mod.rs` | 不同 |
| `core/clash/proxies.rs` | 不同 |
| `core/clash/transaction.rs` | Chimera-only |
| `core/clash/transaction_tests.rs` | Chimera-only |
| `core/clash/ws.rs` | 不同 |
| `core/connection_interruption.rs` | Chimera-only |
| `core/connections.rs` | ref-only |
| `core/corrupt.rs` | 不同 |
| `core/download/adapter.rs` | 不同 |
| `core/download/mod.rs` | 相同 |
| `core/geo/actor.rs` | 不同 |
| `core/geo/adapters.rs` | 不同 |
| `core/geo/client.rs` | 不同 |
| `core/geo/fixtures.rs` | 相同 |
| `core/geo/mod.rs` | 不同 |
| `core/geo/ports.rs` | 不同 |
| `core/geo/tests.rs` | 不同 |
| `core/handle.rs` | Chimera-only |
| `core/logger.rs` | Chimera-only |
| `core/logs/actor.rs` | ref-only |
| `core/logs/codec.rs` | ref-only |
| `core/logs/mod.rs` | ref-only |
| `core/logs/model.rs` | ref-only |
| `core/logs/ports.rs` | ref-only |
| `core/logs/redb.rs` | ref-only |
| `core/logs/tests.rs` | ref-only |
| `core/manager.rs` | 不同 |
| `core/migration/child_result.rs` | ref-only |
| `core/migration/fs.rs` | 不同 |
| `core/migration/legacy_schema/application.rs` | ref-only |
| `core/migration/legacy_schema/clash.rs` | ref-only |
| `core/migration/legacy_schema/clash_config.rs` | ref-only |
| `core/migration/legacy_schema/mod.rs` | ref-only |
| `core/migration/legacy_schema/session_state.rs` | ref-only |
| `core/migration/legacy_schema/verge.rs` | ref-only |
| `core/migration/mod.rs` | 不同 |
| `core/migration/modules/app_config.rs` | ref-only |
| `core/migration/modules/application.rs` | ref-only |
| `core/migration/modules/clash_config.rs` | ref-only |
| `core/migration/modules/mod.rs` | 不同 |
| `core/migration/modules/profiles.rs` | 不同 |
| `core/migration/modules/storage.rs` | ref-only |
| `core/migration/modules/typed_config.rs` | 不同 |
| `core/migration/registry.rs` | 不同 |
| `core/migration/runner.rs` | 不同 |
| `core/migration/store.rs` | 不同 |
| `core/mod.rs` | 不同 |
| `core/proxies.rs` | ref-only |
| `core/server/mod.rs` | Chimera-only |
| `core/service/compat.rs` | 不同 |
| `core/service/control.rs` | 不同 |
| `core/service/ipc.rs` | Chimera-only |
| `core/service/mod.rs` | 不同 |
| `core/session_state.rs` | 不同 |
| `core/state.rs` | Chimera-only |
| `core/storage.rs` | 不同 |
| `core/sysopt.rs` | Chimera-only |
| `core/traffic/actor.rs` | 不同 |
| `core/traffic/client.rs` | 不同 |
| `core/traffic/geo.rs` | 不同 |
| `core/traffic/mod.rs` | 不同 |
| `core/traffic/ports.rs` | 不同 |
| `core/traffic/source.rs` | 不同 |
| `core/traffic/storage.rs` | ref-only |
| `core/traffic/tests.rs` | 不同 |
| `core/tray/display.rs` | ref-only |
| `core/tray/executor.rs` | ref-only |
| `core/tray/icon.rs` | ref-only |
| `core/tray/mod.rs` | 不同 |
| `core/tray/proxies.rs` | ref-only |
| `core/updater/instance.rs` | 不同 |
| `core/updater/mod.rs` | 不同 |
| `core/updater/ports.rs` | ref-only |
| `core/updater/shared.rs` | 不同 |
| `core/updater/tests.rs` | ref-only |
| `core/updater/transaction.rs` | Chimera-only |
| `core/widget_logo.rs` | ref-only |
| `core/win_uwp.rs` | 不同 |
| `e2e_test.rs` | Chimera-only |
| `enhance/artifact_bridge.rs` | Chimera-only |
| `enhance/artifact_snapshot.rs` | ref-only |
| `enhance/chain.rs` | 不同 |
| `enhance/content_source.rs` | Chimera-only |
| `enhance/field.rs` | Chimera-only |
| `enhance/golden.rs` | ref-only |
| `enhance/golden_support.rs` | ref-only |
| `enhance/mod.rs` | 不同 |
| `enhance/runtime_builder.rs` | Chimera-only |
| `enhance/script/adapter.rs` | Chimera-only |
| `enhance/script/js.rs` | Chimera-only |
| `enhance/script/lua.rs` | Chimera-only |
| `enhance/script/mod.rs` | Chimera-only |
| `enhance/script/runner.rs` | Chimera-only |
| `enhance/tun.rs` | Chimera-only |
| `enhance/utils.rs` | 不同 |
| `event_handler/mod.rs` | ref-only |
| `event_handler/widget.rs` | ref-only |
| `feat.rs` | Chimera-only |
| `features/agent/actions.rs` | Chimera-only |
| `features/agent/commands.rs` | Chimera-only |
| `features/agent/core_probe.rs` | Chimera-only |
| `features/agent/diagnostics.rs` | Chimera-only |
| `features/agent/e2e.rs` | Chimera-only |
| `features/agent/history.rs` | Chimera-only |
| `features/agent/intent.rs` | Chimera-only |
| `features/agent/mod.rs` | Chimera-only |
| `features/agent/model.rs` | Chimera-only |
| `features/agent/network_probe.rs` | Chimera-only |
| `features/agent/registry.rs` | Chimera-only |
| `features/agent/tun_host_probe.rs` | Chimera-only |
| `features/mod.rs` | Chimera-only |
| `host_paths.rs` | ref-only |
| `ipc.rs` | 不同 |
| `lib.rs` | 不同 |
| `main.rs` | 不同 |
| `network_probe.rs` | Chimera-only |
| `server/debug_http.rs` | ref-only |
| `server/mod.rs` | ref-only |
| `service/icon.rs` | ref-only |
| `service/mod.rs` | 不同 |
| `service/profile_file.rs` | 不同 |
| `setup.rs` | 不同 |
| `setup_tests.rs` | ref-only |
| `shutdown_hook.rs` | 不同 |
| `specta_export.rs` | 不同 |
| `startup_failure.rs` | 不同 |
| `startup_failure_tests.rs` | ref-only |
| `state/application.rs` | 不同 |
| `state/clash_config.rs` | 不同 |
| `state/config_error.rs` | ref-only |
| `state/mirror.rs` | Chimera-only |
| `state/mod.rs` | 不同 |
| `state/mutation.rs` | 不同 |
| `state/profiles/actor.rs` | 不同 |
| `state/profiles/error.rs` | ref-only |
| `state/profiles/jobs.rs` | 不同 |
| `state/profiles/mod.rs` | 不同 |
| `state/profiles/ports.rs` | 不同 |
| `state/profiles/scheduler.rs` | 不同 |
| `state/profiles/sources.rs` | ref-only |
| `state/session_state.rs` | 不同 |
| `unified_rpc.rs` | ref-only |
| `utils/blocking.rs` | 相同 |
| `utils/candy.rs` | 不同 |
| `utils/collect.rs` | 不同 |
| `utils/color.rs` | ref-only |
| `utils/config.rs` | 不同 |
| `utils/core_version.rs` | ref-only |
| `utils/dialog.rs` | ref-only |
| `utils/dirs.rs` | 不同 |
| `utils/dock.rs` | ref-only |
| `utils/exit.rs` | 不同 |
| `utils/help.rs` | 不同 |
| `utils/hwid.rs` | 不同 |
| `utils/init/logging.rs` | 不同 |
| `utils/init/mod.rs` | 不同 |
| `utils/init/tests.rs` | 不同 |
| `utils/main_thread.rs` | ref-only |
| `utils/mod.rs` | 不同 |
| `utils/net.rs` | 不同 |
| `utils/open.rs` | 相同 |
| `utils/path.rs` | Chimera-only |
| `utils/profiling.rs` | ref-only |
| `utils/proxy_env.rs` | ref-only |
| `utils/recovery_io.rs` | Chimera-only |
| `utils/redb_ext.rs` | Chimera-only |
| `utils/resolve.rs` | 不同 |
| `utils/sudo.rs` | 不同 |
| `utils/winhelp.rs` | ref-only |
| `utils/winreg.rs` | Chimera-only |
| `widget.rs` | ref-only |
| `window.rs` | Chimera-only |
| `window/engine.rs` | 不同 |
| `window/kinds.rs` | ref-only |
| `window/manager.rs` | ref-only |
| `window/mod.rs` | ref-only |
| `window/table.rs` | ref-only |
