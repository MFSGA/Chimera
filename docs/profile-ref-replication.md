# Profile 流程按 ref 复刻手册

本文用于把 Chimera 的共享 Profile 业务流程逐项迁移到本地 `ref/` 的实现。目标是复刻实现和状态流，不只是做出相似 UI 或行为。legacy UI 由 Chimera 侧主动适配新 DTO、IPC 和 hooks；保留它的路由、展示和用户流程，不保留旧 Profile 业务后端作为平行实现。

逐条交给本机小模型执行的文件范围和 prompts 见[本机模型任务包](profile-local-model-task-pack.md)。

## 固定参考版本

本手册按以下已同步参考版本编写：

```text
repository: https://github.com/libnyanpasu/clash-nyanpasu
ref commit: 232321d52121fe8bb25cb2a090d814129cb50c55
date:       2026-09-24
```

同步后的 ref 工作区干净。其递归子模块按 superproject gitlink 固定为：

```text
backend/nyanpasu-runtime                         f5b581fad8bf8272e222f1e3948c7826c6665bb6
backend/nyanpasu-runtime/crates/nyanpasu-utils  cd6c9d3821a8c943bc249d96d456e2bedffd3ada
```

复刻开始和每个阶段结束时记录 `git -C ref rev-parse HEAD`、`git -C ref status --short` 和 `git -C ref submodule status --recursive`。若 ref 后续前进，先核对本手册涉及的实现差异，再选定本阶段的单一基线；不要混用不同 ref commit 的代码。

## 执行原则

1. 同一批迁移同时读 ref 与 Chimera 对应模块，先建立符号映射，再复制完整调用链。保留 ref 的目录结构、名称、事务边界、状态所有权和错误语义，只做 `Nyanpasu` / `nyanpasu` 到 Chimera 品牌标识的必要替换。
2. `backend/chimera-config` 已有的 Profile domain 是落地载体。逐项对照 ref 的 `backend/nyanpasu-config/src/profile`，对齐后续新增或不一致的实现；不要在 Tauri 层另造长期 domain。
3. 以 ref 的 `ProfilesActor` / `ProfilesClient` 为共享业务入口。主 UI、legacy UI 与 agent 都调用同一应用 API；agent orchestration 放在 feature 边界，legacy presentation adapter 放在 UI 边界。
4. 直接迁移 ref 的数据迁移，包括识别、修复、验证、备份、原子写和 revision stamp。不要改成“读不到旧格式就 warning 后当空 Profiles 启动”；这不是 ref 的行为，也可能造成旧数据被新写入覆盖。
5. 每阶段只切换完整可验证的调用链。迁移桥接只能短暂存在，必须写明退出条件；最终移除旧业务 bridge，但保留 legacy UI。
6. 按仓库 testing standard 给新增/修改测试登记运行入口；报告实际执行的 suite，区分 unit、fixture UI、真实桌面和真实网络/运行时验证。

## 实现映射

| 流程 / ref                                                                                                                                    | Chimera 目标                                                         | 迁移要求                                                                                                                                         |
| --------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| `backend/nyanpasu-config/src/profile/*`：`Profiles`、`ProfileItem`、`ProfileDefinition`、`ProfileSource`、patch、validation、dependency index | `backend/chimera-config/src/profile/*`                               | 逐文件与逐符号比对，补齐模型、验证、mutator、revision 和错误类型；只保留必要品牌与 Chimera 明确扩展。                                            |
| `backend/tauri/src/state/profiles/{actor.rs,ports.rs,scheduler.rs}`                                                                           | 新建/对齐 `backend/tauri/src/state/profiles/*`                       | Actor 单独拥有 profile 文档；消息串行化变更，CAS 持久化，派生 dependency index、远程调度和外部 watcher。文件/网络通过 ports 注入。               |
| `backend/tauri/src/client/profiles.rs`：`ProfilesClient`                                                                                      | `backend/tauri/src/client/profiles.rs`：替换现有旧 client ports 流程 | 提供 snapshot、add/import、current、patch、refresh、replace definition、reorder、delete、文件读写等 ref API。返回 commit report 和 degradation。 |
| `backend/tauri/src/service/profile_file.rs`                                                                                                   | 新建/对齐同路径 service                                              | 实现 `ProfileFsPort`、订阅抓取与材料化 journal；保留 containment/symlink 防护、原子操作、恢复和 cleanup 语义。                                   |
| `backend/tauri/src/client/application_workflow/profiles.rs` 与 `client/application_workflow/*`                                                | Chimera `client` / runtime lifecycle 对应模块                        | 将 profile commit report 接到 prepare/apply；只按依赖闭包与 `affects_current` 决定 runtime 应用，不让 IPC/UI 自己重建业务。                      |
| `backend/tauri/src/core/migration/modules/profiles.rs`、migration runner/store 与 `ProfilesFormat`                                            | Chimera `backend/tauri/src/core/migration/*`                         | 按 ref 实现步骤检查、旧 schema 转换、验证、recovery backup、原子写与 schema revision stamp；迁移状态与文件不一致时拒绝错误状态下静默继续。       |
| `backend/tauri/src/ipc.rs`：Profiles commands 和 `Profiles` 类型                                                                              | `backend/tauri/src/ipc.rs`                                           | 命令只做输入/输出转换并调用 `ChimeraClient`。用新 domain DTO；经生成器刷新 Specta bindings。                                                     |
| `frontend/interface/src/ipc/use-profile.ts`、bindings                                                                                         | `frontend/interface/src/ipc/*`                                       | 一次更新共享 DTO、命令 hooks、缓存失效和 mutation 结果处理，移除对旧 `ProfilesResponse` 形状的业务依赖。                                         |
| `frontend/nyanpasu/src/pages/(main)/main/profiles/*`                                                                                          | `frontend/chimera/src/pages/(main)/main/profiles/*`                  | 对照 ref 主 UI 的 profile 类型、组合配置、源信息、更新/编辑语义；产品品牌视觉差异除外。                                                          |
| ref 主 UI 没有的 Chimera legacy 路由与窗口                                                                                                    | `frontend/chimera/src/pages/(legacy)/*` 及相应组件                   | 主动改造为新 API；保持 legacy 原有 presentation 与流程，不复制旧 backend 行为。                                                                  |
| `features/agent` 的 profile tools/diagnostics/proposals                                                                                       | Chimera agent profile feature                                        | 只通过共用 Profile application API 读写；保留 agent 自身 proposal、confirmation、stale check、audit 和 recovery 编排。                           |

具体 ref 符号和实现细节以该 commit 的源码为准；表格用于定位，不替代读代码。每次迁移需要进一步列出实际触及的函数和调用方向。

## 逐文件迁移清单

下表是可以直接按顺序处理的文件清单。`复制/对齐` 表示以 ref 文件为结构和实现基准；`合并适配` 表示目标文件已承载 Chimera 其他能力，不能整文件覆盖；`新增` 表示 Chimera 尚无对应模块。复制前逐个 `git diff --no-index`/阅读两侧文件，保留品牌差异和已经确认的 Chimera-only 扩展。

### 1. Profile domain

先完成纯 domain，保证 Actor、IPC 和 UI 有统一类型可用。

| ref 源文件                                              | Chimera 目标文件                                   | 动作                                        |
| ------------------------------------------------------- | -------------------------------------------------- | ------------------------------------------- |
| `ref/backend/nyanpasu-config/src/profile/mod.rs`        | `backend/chimera-config/src/profile/mod.rs`        | 合并适配并核对 exports                      |
| `ref/backend/nyanpasu-config/src/profile/id.rs`         | `backend/chimera-config/src/profile/id.rs`         | 复制/对齐 `ProfileId`                       |
| `ref/backend/nyanpasu-config/src/profile/path.rs`       | `backend/chimera-config/src/profile/path.rs`       | 复制/对齐 managed/external path 校验        |
| `ref/backend/nyanpasu-config/src/profile/metadata.rs`   | `backend/chimera-config/src/profile/metadata.rs`   | 复制/对齐                                   |
| `ref/backend/nyanpasu-config/src/profile/source.rs`     | `backend/chimera-config/src/profile/source.rs`     | 复制/对齐 Local/Remote/External 类型        |
| `ref/backend/nyanpasu-config/src/profile/definition.rs` | `backend/chimera-config/src/profile/definition.rs` | 复制/对齐 Config/Composition/Transform      |
| `ref/backend/nyanpasu-config/src/profile/item.rs`       | `backend/chimera-config/src/profile/item.rs`       | 复制/对齐                                   |
| `ref/backend/nyanpasu-config/src/profile/dependency.rs` | `backend/chimera-config/src/profile/dependency.rs` | 复制/对齐依赖索引                           |
| `ref/backend/nyanpasu-config/src/profile/patch.rs`      | `backend/chimera-config/src/profile/patch.rs`      | 复制/对齐 mutators                          |
| `ref/backend/nyanpasu-config/src/profile/profiles.rs`   | `backend/chimera-config/src/profile/profiles.rs`   | 复制/对齐验证、revision、current 和列表语义 |
| `ref/backend/nyanpasu-config/src/profile/tests/*.rs`    | `backend/chimera-config/src/profile/tests/*.rs`    | 逐测试文件对齐并确认 Cargo 真正运行它们     |

### 2. Profile 文件/订阅 service 与 actor

这组承载写入授权、文件事务、状态所有权和后台事件，是业务迁移核心。

| ref 源文件                                          | Chimera 目标文件                                | 动作                                                              |
| --------------------------------------------------- | ----------------------------------------------- | ----------------------------------------------------------------- |
| `ref/backend/tauri/src/state/profiles/mod.rs`       | `backend/tauri/src/state/profiles/mod.rs`       | 新增 module exports                                               |
| `ref/backend/tauri/src/state/profiles/ports.rs`     | `backend/tauri/src/state/profiles/ports.rs`     | 新增/对齐端口合同                                                 |
| `ref/backend/tauri/src/state/profiles/actor.rs`     | `backend/tauri/src/state/profiles/actor.rs`     | 复制/适配 Actor、消息和提交语义                                   |
| `ref/backend/tauri/src/state/profiles/scheduler.rs` | `backend/tauri/src/state/profiles/scheduler.rs` | 复制/适配订阅调度和外部 watchers                                  |
| `ref/backend/tauri/src/state/mod.rs`                | `backend/tauri/src/state/mod.rs`                | 合并适配并注册 `profiles`                                         |
| `ref/backend/tauri/src/service/profile_file.rs`     | `backend/tauri/src/service/profile_file.rs`     | 新增/对齐文件、fetcher、journal 和恢复实现                        |
| `ref/backend/tauri/src/service/mod.rs`              | `backend/tauri/src/service/mod.rs`              | 合并适配 service exports                                          |
| `ref/backend/tauri/src/client/profiles.rs`          | `backend/tauri/src/client/profiles.rs`          | 重构为 `ProfilesClient`；移除旧 Read/Write port 的核心职责        |
| `ref/backend/tauri/src/client/mod.rs`               | `backend/tauri/src/client/mod.rs`               | 合并装配 `ProfilesClient`、actor、ports 与 `ChimeraClient` facade |
| `ref/backend/tauri/src/setup.rs`                    | `backend/tauri/src/setup.rs`                    | 合并启动顺序、migration 完成后装配 profile actor                  |

检查 `ProfileFsPort`、`ProfileMaterializationPort`、`SubscriptionFetcher`、`RebuildNotifier` 的实现是否都被生产 composition 注入；不要只复制 trait 和测试 mock。

### 3. 文件 schema migration

ref 有专门的 profile document migration；照此迁移，不能用 warn-only/空状态替代。

| ref 源文件                                                      | Chimera 目标文件                                            | 动作                                                              |
| --------------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------------- |
| `ref/backend/tauri/src/core/migration/modules/profiles.rs`      | `backend/tauri/src/core/migration/modules/profiles.rs`      | 复制/品牌适配 Profile schema migrator、steps、backup 和 stamp     |
| `ref/backend/tauri/src/core/migration/modules/mod.rs`           | `backend/tauri/src/core/migration/modules/mod.rs`           | 合并注册 profiles migrator                                        |
| `ref/backend/tauri/src/core/migration/fs.rs`                    | `backend/tauri/src/core/migration/fs.rs`                    | 对齐原子文档写入/读取能力                                         |
| `ref/backend/tauri/src/core/migration/mod.rs`                   | `backend/tauri/src/core/migration/mod.rs`                   | 合并迁移模块接口                                                  |
| `ref/backend/tauri/src/core/migration/registry.rs`              | `backend/tauri/src/core/migration/registry.rs`              | 合并注册顺序                                                      |
| `ref/backend/tauri/src/core/migration/runner.rs`                | `backend/tauri/src/core/migration/runner.rs`                | 对齐 check/run/rollback/recovery 协议                             |
| `ref/backend/tauri/src/core/migration/store.rs`                 | `backend/tauri/src/core/migration/store.rs`                 | 对齐持久迁移状态与 stamp 一致性                                   |
| `ref/backend/nyanpasu-core/src/format.rs`                       | `backend/chimera-core/src/format.rs`                        | 对齐 `StampedDocument` / `StampedYamlFormat` 与 stamped YAML 实现 |
| `ref/backend/tauri/src/core/migration/fixtures/*/profiles.yaml` | `backend/tauri/src/core/migration/fixtures/*/profiles.yaml` | 增补/对齐迁移前后 fixture                                         |

ref `profiles.rs` 具体包含 `MigrateProfilesNullValue`、`MigrateProfileScriptNewtype`、`MigrateProfilesCleanSchema`、`MigrateProfilesRepairSchema`。实施时应从当前 ref 读取实际 revision、check/run/rollback 和 fixture，不要复制本文中的旧值或仅照搬类型名。

### 4. 应用流程、IPC 与生成绑定

| ref 源文件/符号                                                                                              | Chimera 目标文件/符号                                       | 动作                                                                                           |
| ------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `ref/backend/tauri/src/client/application_workflow/profiles.rs`                                              | `backend/tauri/src/client/application_workflow/profiles.rs` | 新增/对齐 activate/current 的 prepare/apply/degradation 流程                                   |
| `ref/backend/tauri/src/client/application_workflow/{mod.rs,workflow.rs,preparation.rs,ports.rs,adapters.rs}` | `backend/tauri/src/client/application_workflow/*`           | 新增必要模块并合并 Chimera lifecycle/service 扩展；只迁移 profile 路径所需的最小完整 workflow  |
| `ref/backend/tauri/src/ipc.rs`：profiles imports、response DTO 与所有 profile commands                       | `backend/tauri/src/ipc.rs` 同名项                           | 合并适配成薄 wrapper，删除旧 builder/response 兼容合同                                         |
| `ref/backend/tauri/src/specta_export.rs`：profile command/type 注册                                          | `backend/tauri/src/specta_export.rs`                        | 更新注册并运行 bindings freshness 检查                                                         |
| `ref/frontend/interface/src/ipc/use-profile.ts`                                                              | `frontend/interface/src/ipc/use-profile.ts`                 | 复制/对齐 query 与 mutations，使用新 domain DTO                                                |
| `ref/frontend/interface/src/ipc/use-profile-content.ts`                                                      | `frontend/interface/src/ipc/use-profile-content.ts`         | 对齐文件读取/保存 cache 与 invalidation                                                        |
| `ref/frontend/interface/src/ipc/bindings.ts`                                                                 | `frontend/interface/src/ipc/bindings.ts`                    | 由既有 Specta generator 生成，禁止手改                                                         |
| `ref/frontend/interface/src/ipc/index.ts`                                                                    | `frontend/interface/src/ipc/index.ts`                       | 合并导出新的 Profile API/types                                                                 |
| `ref/frontend/interface/src/ipc/consts.ts`                                                                   | `frontend/interface/src/ipc/consts.ts`                      | 按实际 query key 变更合并                                                                      |
| Chimera-only `frontend/interface/src/ipc/profile-definition.ts`                                              | 同文件                                                      | 逐函数检查；保留仍被 agent/extension 使用的 presentation adapter，避免它继续充当第二套业务模型 |

IPC 至少搜索并审查 `get_profiles`、`import_profile`、`create_profile`、`view_profile`、`read_profile_file`、`save_profile_file`、`reorder_profile(s)`、`activate_profile`、`set_profile_valid_fields`、transform chain、metadata/options patch、definition replace、`update_profile` 和 `delete_profile`。

### 5. 主 UI 与必须主动适配的 legacy UI

主 UI 同构迁移；legacy UI 没有 ref 对应目录，逐项改到共享新 hook/API。

| ref 源                                                                                                                                                                                           | Chimera 目标                                                                                                                                                            | 动作                                                                                          |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `ref/frontend/nyanpasu/src/pages/(main)/main/profiles/` 整个目录                                                                                                                                 | `frontend/chimera/src/pages/(main)/main/profiles/` 同相对目录                                                                                                           | 对照每个 profile list/detail/inspect 文件迁移业务调用；Chimera 可保留自己的呈现组件和扩展入口 |
| ref 同目录 `_modules/{consts,error-item,profile-quick-import,profiles-navigate}.tsx`                                                                                                             | Chimera 同名/对应 `_modules/*`                                                                                                                                          | 逐文件比较，不漏导航、quick import 和 error 入口                                              |
| ref 同目录 `$type/index.tsx`、`$type/_modules/{chain-profile-import,create-composition-button,import-button,local-profile-button,profiles-header,profiles-list,remote-profile-button,utils}.tsx` | Chimera 同路由 `_modules/*`                                                                                                                                             | 更新 Config/Transform/Composition 及 Local/Remote 语义                                        |
| ref 同目录 `$type/detail/$uid.tsx`、`detail/_modules/*`                                                                                                                                          | Chimera 同路由 `detail/*`                                                                                                                                               | 更新 activate/delete/edit/refresh/source/metadata UI                                          |
| `ref/frontend/nyanpasu/src/pages/(editor)/editor/profile/index.tsx`                                                                                                                              | `frontend/chimera/src/pages/(editor)/editor/profile/index.tsx`                                                                                                          | editor 读写改用新的 `useProfileContent` / save command                                        |
| ref `frontend/nyanpasu` 不含 legacy UI                                                                                                                                                           | Chimera `frontend/chimera/src/pages/(legacy)/profiles.tsx`                                                                                                              | 主动适配新 Profile DTO/hook，保留 legacy 页面的交互和显示                                     |
| ref 无对应 Chimera 共享 Profile 组件                                                                                                                                                             | `frontend/chimera/src/components/profiles/profile-item.tsx`、`profile-dialog.tsx`、`read-profile.tsx`、`profile-side.tsx`、`modules/side-chain.tsx`、`modules/store.ts` | 检查旧类型假设并改为调用共享新 API；只在 UI 边界留展示映射                                    |
| ref quick-import 对应入口                                                                                                                                                                        | `frontend/chimera/src/components/profiles/quick-import.tsx`、`best-effort-subscription-import.tsx`、`frontend/chimera/src/pages/(legacy)/subscription-onboarding.tsx`   | 保留 Chimera-only 功能，底层导入必须调用同一 ProfilesClient IPC                               |
| ref 的 Profiles 页面无对应的 Chimera runtime diff 界面                                                                                                                                           | `frontend/chimera/src/components/profiles/runtime-config-diff-dialog.tsx` 及 `frontend/chimera/src/pages/(main)/main/profiles/inspect/*`                                | 保留扩展能力；使用共享 committed/applied snapshot API，不维护 profile 副本                    |

实际编码前先对主 UI 与 Chimera 的 profile 子目录执行文件名清单比较。上表采用同名/职责映射；若某个 ref 文件在当前 Chimera 分支改了名字，将旧文件和新文件都列入本阶段映射记录。

### 6. Agent 与旧业务代码退场

| Chimera 文件                                                                                  | 需要的改动                                                                                                         |
| --------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| `backend/tauri/src/features/agent/diagnostics.rs`                                             | 当前直接读取 `Config::profiles()`；改为从 `ChimeraClient`/共享 profile snapshot 获取数据，并保持隐私投影语义。     |
| `backend/tauri/src/features/agent/actions.rs`、`commands.rs`、`registry.rs`、`model.rs`       | 搜索 profile 提案、summary 与执行；执行通过共享 application API，保留 agent 自身 proposal/confirmation/audit。     |
| `backend/tauri/src/features/agent/e2e.rs` 及 agent 测试/fixture                               | 将旧 profile fixture 映射到新 snapshot；区分 fixture orchestration 与真实提交验证。                                |
| `backend/tauri/src/config/profile/ref_adapter.rs`                                             | 所有生产读取和写入迁移完后删除；如果迁移期仍被 RuntimeBuilder 使用，记录调用点和删除条件。                         |
| `backend/tauri/src/config/profile/{profiles.rs,builder.rs,item_type.rs,item/*}`               | 移除旧 domain 和旧持久化业务方法前，用 `rg` 找完全部生产/legacy UI 相关调用；只保留有明确兼容理由的窄 adapter。    |
| `backend/tauri/src/client/profiles.rs` 中 `LegacyProfilesReadPort`、`LegacyProfilesWritePort` | 新 client 完整承接所有调用后删除 ports、impl 与 `ClientSetupArgs` 注入项。                                         |
| `backend/tauri/src/config/profile/reservation_reconcile.rs`、`reservation_reconcile_tests.rs` | 对照 ref materialization reservation/journal 是否取代其职责，再决定迁移/删除；不得在未核对崩溃恢复语义前直接移除。 |

## 手工执行时的建议批次

为降低冲突，按下面批次逐一复制与验证，批次间先跑格式/编译和对应单测：

1. Domain 与 tests：`backend/chimera-config/src/profile/*`。
2. Migration：core migration infra、profiles migrator、fixtures 和启动顺序。
3. Ports/service/state actor：profile filesystem/materialization、scheduler、actor 与 client API；先通过隔离测试验证，保留生产 setup 的旧 schema 闸门。
4. Client/workflow/IPC/bindings：API 完整后生成 bindings。
5. Main UI/hooks：对照 ref 的页面及接口逐文件调整。
6. Legacy UI 和 agent：明确其新 API 接入点，运行各自入口覆盖。
7. 确认 IPC、主 UI、legacy UI 和 agent 全部切到共享 API 后，再在一次最终 cutover 中删除旧业务 bridge、接入 actor/service、启用 Profile migrator，并跑完整影响矩阵。

不要把这个顺序理解为允许长期并存的双写系统。除升级迁移期间的明确阶段外，Profiles 的持久化写入必须只有一个权威 owner。

## 分阶段实施顺序

### 阶段 A：盘点与领域对齐

- 对照 `ref/backend/nyanpasu-config/src/profile/*` 与 `backend/chimera-config/src/profile/*`，列出每个文件、类型、方法差异。
- 对照 ref profile tests；确认 Chimera domain 已能表达 ref 的 `current` 单选语义、composition、Local Managed/External、Remote、Transform、metadata provenance、subscription 和 revision。
- 单独记录 Chimera Client/Service 或其他明确产品扩展，不把已有差异默认视为永久例外。
- 完成本阶段后再改 Tauri，不要从 `save_profile_file` 单命令扩展出另一套 profile 架构。

### 阶段 B：持久化迁移与启动装配

- 对照 `ref/backend/tauri/src/core/migration/modules/profiles.rs` 的 `ProfilesMigrator`、各 revision step、schema 检查与 `ProfilesFormat`。
- 将 legacy `profiles.yaml` 转换到新的 schema。复刻 ref 的转换和校验规则；对数据无法安全转换的情况沿用 ref 的错误/恢复行为，不自行变成空配置继续启动。
- 复刻备份、原子写、stamp、迁移状态一致性检查和启动顺序；用隔离 fixture 验证可恢复的持久层。实际应用启动必须保留兼容闸门，直到所有旧 schema 消费者切换完成。
- 核对 ref 当前 schema revision 和迁移测试，不能沿用本文撰写时之后可能已更新的常量。
- 迁移窗口内不得让新 actor 接管仍由 legacy UI/agent 读写的 `profiles.yaml`。新 actor/service 可先复制并通过隔离测试；生产 setup 注入 actor 与启用完整 Profile migrator 同属最终 cutover。

### 阶段 C：Actor、文件事务和后台任务

- 逐条迁移 `ProfilesActorMessage` 和 actor handlers；包括 Add、Delete、Reorder、PatchMetadata、PatchRemoteOptions、RefreshRemote、ImportRemote、ReplaceDefinition、Current、ValidFields、GlobalTransforms、external-file change、materialization reconcile。
- 复制 ref 的版本检查、revision bump、状态提交、依赖闭包、`affects_current`、materialization prepare/promote/complete/compensate 和 durable cleanup 语义。
- 接入 ProfileFs、SubscriptionFetcher、ProfileMaterialization、RebuildNotifier、remote scheduler、external watchers；阻塞文件系统工作走规定的 blocking boundary。
- 保留 ref 错误与 degradation 分类，不能把已提交但 runtime apply 失败报告成未提交，也不能把单纯 API 成功报告成 runtime 已验证。
- 在 IPC、主 UI、legacy UI 与 agent 切换完成前，不要从 production setup 启动这个 actor 或让其写入 live `profiles.yaml`；验证使用独立 tempdir。

### 阶段 D：Client、runtime workflow 和 IPC

- 将 ref `ProfilesClient` API 接入 Chimera application facade；为 `ChimeraClient` 和保留的 service/core 支持做窄 adapter，不删支持面。
- 对照 `client/application_workflow/profiles.rs` 处理 current 变更、prepare/apply、连接中断策略、degradation 和 UI refresh。
- 所有 profile IPC 改为薄边界：调用共享 client、转换 typed error/wire DTO；bindings 由既有 generator 生成。
- 将读取、编辑与保存分别对照 ref：Config 内容规范化 YAML，Transform 保留原文；只允许 Local Managed 写入；Remote 与 External 按 ref 返回不可写错误；定义无 source 时跟随 domain 类型/错误。

### 阶段 E：主 UI、legacy UI 与 agent 适配

- 主 UI 对照 ref Profile pages 和 `frontend/interface` hooks，使用相同业务语义。
- 逐个更新所有 legacy 调用点，包括 profile 列表、详情、导入、编辑器窗口、设置字段过滤、快速导入和 shell/layout 中选择 current 的逻辑。不要通过后端保留旧响应结构来避免这些改动。
- 把旧 `current: Vec<uid>` 用户操作明确映射为新单选/Composition 流程；保留 legacy 的操作步骤和展示，但用新模型解释状态。若有流程确实无法一一表达，记录产品决策，不能暗中伪造旧语义。
- Agent 继续沿共享应用 API 使用同一状态和事务；工具返回值须区分 proposal、committed、degraded、verified 等真实阶段。
- 搜索所有旧符号和命令调用，确认没有窗口或 agent 继续写旧状态。

### 阶段 F：删除过渡桥接并验收

- 删除 `LegacyProfilesReadPort`、`LegacyProfilesWritePort`、旧 `Profiles`/`Profile` business mutations、`ref_adapter` 及迁移完成后无用的兼容代码。
- 所有入口迁移并完成旧状态残留搜索后，在 setup 注入唯一 `ProfilesClient` 与 `ProfileFileService`、fetcher/notifier，再从临时 runner 切到完整 Profile migrator；迁移验证失败时不得启动 actor。
- 用启动/升级路径证明迁移结果；用主 UI 与 legacy UI 执行创建、导入、编辑/保存、激活、刷新、排序、删除；覆盖外部文件及失败恢复。Agent 使用 profile 能力的入口也执行对应回归。
- 验证 durable file、snapshot、runtime-applied 状态之间的结果；根据受支持 core/platform 列出已测和未测矩阵。
- `rg` 搜索旧 schema/API 名称并解释每个残留（migration fixture、兼容 reader 或测试 fixture 不算生产 bridge，但须有理由）。

## 特别核对的 Profile 语义

- **单选与组合**：ref `Profiles.current` 是 `Option<ProfileId>`。旧多选映射由迁移生成明确 `CompositionConfig`，不是把 `Vec` 保留到新业务模型。
- **来源和写入**：Local Managed、Local External（Symlink/Mirror）、Remote 的所有权不同。Editor 保存不能直接按 Profile kind 或 `file` 字符串猜权限。
- **远程订阅**：下载先校验并在提交边界检查 URL/definition fingerprint；未完成 fetch 不应留下空 placeholder；更新间隔、名称来源、subscription metadata 按 ref 处理。
- **删除/替换**：检查依赖引用；materialized file 清理需 fenced、可恢复，不能先删文件再发现状态提交失败。
- **当前运行态**：Profiles snapshot 是 committed state；运行时是经 prepare/apply 后的 applied state。apply 失败不能回滚或伪装成 profile commit 未发生。
- **外部文件**：Mirror 与 Symlink 具备不同同步语义；路径 containment、符号链接防护和 watcher fence 属于流程，不只是文件 IO 实现细节。

## 验证和交付记录

开始共享功能前阅读根目录 `AGENTS.md`、`docs/ref-alignment-guide.md`、`docs/testing/README.md`；涉及 Tauri E2E 时再读 `docs/testing/upstream-evidence.md`、`docs/testing/test-contract-template.md` 和 `tauri-e2e/README.md`。

每个阶段至少记录：

```text
ref commit / recursive submodule SHAs:
ref path + symbol -> Chimera path + symbol:
main UI / legacy UI / agent entry points:
profile document/data impact:
runtime/core/platform impact:
checks actually executed and outcomes:
not run / skipped / uncovered:
temporary bridge and its deletion condition:
```

按变更范围执行 cargo tests/check、生成绑定 freshness、`pnpm typecheck`、`pnpm lint:frontend-boundaries`、适用的 E2E suite 和 `git diff --check`。不把编译、fixture 或一次 smoke 运行说成持久化升级、真实订阅网络或完整核心矩阵通过。完整对齐前状态标记为“部分迁移”。

## 当前 Chimera 起点

目前可观察到的主要迁移边界：

- 已有 `backend/chimera-config/src/profile/*` 新领域类型，以及 `backend/tauri/src/config/profile/ref_adapter.rs` 旧持久化模型到 runtime domain 的转换。
- `backend/tauri/src/client/profiles.rs` 仍含 `LegacyProfilesReadPort` / `LegacyProfilesWritePort`，通过旧 `Config::profiles()` 和旧 `Profiles` 文档执行多数 profile mutations。
- `backend/tauri/src/ipc.rs` 的 `ProfilesResponse`、`frontend/interface/src/ipc/use-profile.ts` 和生成 bindings 仍是现有 UI 合同。
- 主 UI 位于 `frontend/chimera/src/pages/(main)/main/profiles/*`；legacy 入口位于 `frontend/chimera/src/pages/(legacy)/*`。需将两者一起迁移到新的共享 API，不能只更新主 UI。
- agent profile 工具在 `backend/tauri/src/features/agent/*`；逐项核验它是否通过共享 client，而非直接访问旧状态。
- 本文写入时工作区另有 [`backend/tauri/src/client/profiles.rs`](../backend/tauri/src/client/profiles.rs) 与 [`docs/ref-alignment-progress.md`](ref-alignment-progress.md) 未提交修改。手动复刻前先 review/保留/撤销这些改动，再开始新阶段，避免将旧 baseline 的局部保存规则改动误当成完整对齐。
