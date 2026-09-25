# Profile ref 复刻：本机小模型任务包

本文件把 [Profile ref 复刻手册](profile-ref-replication.md) 拆成可逐条交给本机模型的工作项。一次只运行一个任务；除 T00 外，每个任务只改它声明的目标文件。任务按依赖顺序执行，不要并行跑会改相同文件的工作项。

## 固定上下文

```text
Workspace: I:\mfsga\Chimera
Reference: I:\mfsga\Chimera\ref
Reference HEAD: 232321d52121fe8bb25cb2a090d814129cb50c55
Reference submodules:
  backend/nyanpasu-runtime                         f5b581fad8bf8272e222f1e3948c7826c6665bb6
  backend/nyanpasu-runtime/crates/nyanpasu-utils  cd6c9d3821a8c943bc249d96d456e2bedffd3ada
```

`ref/` 是只读参考。不要 fetch、checkout、修改 ref 或子模块。不要覆盖/重置工作区，不要清理用户文件，不要顺手格式化不相关目录，不要提交 commit。开始每个任务前检查 `git status --short`，保留已经存在的差异。本任务包生成时工作区已有这些改动：

```text
 M .gitignore
 M backend/tauri/src/client/profiles.rs
 M docs/ref-alignment-guide.md
 M docs/ref-alignment-progress.md
?? docs/profile-ref-replication.md
```

这些差异不是模型创建的，除非任务明确涉及，否则必须保留。profile client 文件已有改动尤其不能整文件覆盖。

## 所有任务都遵守的提示前缀

将下面这段放在每次模型任务开头：

> 你在 Chimera 工作区中迁移 Profile 功能，唯一实现参考是 `ref/` 的固定 HEAD `232321d52121fe8bb25cb2a090d814129cb50c55`。先读声明的 ref 文件和 Chimera 对应文件，再修改。按 ref 的路径、命名、状态所有权、事务顺序、错误处理和测试结构复刻，只允许必要品牌替换以及已记录的 Chimera 扩展。不要凭文件名猜行为，不要只抄接口，不要臆造实现。`ref/` 只读。保留任务开始前已有的其他差异。只修改本任务“允许改动”列出的目标；若发现必须改范围外文件，暂停并报告原因，不要自行扩大范围。完成后报告每个 ref→Chimera 文件/符号映射、实际 diff、运行过的命令及结果、未验证项和下一步阻塞。

## T00：只读生产调用链盘点

**依赖：** 无。**改动：** 不允许改文件。

**交给模型：**

> 从 `ref/backend/nyanpasu-config/src/profile/`、`ref/backend/tauri/src/state/profiles/`、`ref/backend/tauri/src/service/profile_file.rs`、`ref/backend/tauri/src/client/profiles.rs`、`ref/backend/tauri/src/client/application_workflow/profiles.rs` 和 `ref/backend/tauri/src/core/migration/modules/profiles.rs` 追踪 Profile 的启动、读取、修改、文件材料化、runtime apply 和恢复路径。对 Chimera 读 `backend/chimera-config/src/profile/`、`backend/tauri/src/config/profile/`、`backend/tauri/src/client/profiles.rs`、`backend/tauri/src/setup.rs`、`backend/tauri/src/ipc.rs` 与 `backend/tauri/src/features/agent/diagnostics.rs`。输出按调用方向排列的文件/符号映射表、每条路径的状态 owner、ref 与 Chimera 的差异、适合拆分的小任务。禁止改文件；不要把编译或测试说成已运行。

**验收：** 输出里必须包含真实符号名，识别旧 `LegacyProfilesReadPort`/`LegacyProfilesWritePort` 和 `ref_adapter`，明确发现 agent diagnostics 直读 `Config::profiles()` 的位置。

## T01：Profile path / source / id domain

**依赖：** T00。**允许改动：**

```text
backend/chimera-config/src/profile/id.rs
backend/chimera-config/src/profile/path.rs
backend/chimera-config/src/profile/source.rs
backend/chimera-config/src/profile/mod.rs
```

**交给模型：**

> 对照 `ref/backend/nyanpasu-config/src/profile/{id.rs,path.rs,source.rs,mod.rs}`，迁移 `ProfileId`、managed/external path、`ProfileSource`、`LocalBinding`、材料化 metadata 和 exports。先逐文件总结差异，再修改上面列出的 Chimera 文件。保留 Chimera 已有验证和序列化能力；只有经证据确认与 ref 冲突时才调整。不要修改 Tauri、IPC、bindings 或其他 domain 文件。运行与此模块直接相关的 `cargo test --manifest-path backend/Cargo.toml -p chimera-config profile::`；若过滤语法无法匹配，改用 crate 当前可用的精确测试入口并报告。

**验收：** Remote、Local Managed、Local External 三种来源表达与 ref 一致；路径安全约束未被弱化；序列化 wire shape 与测试一致。

## T02：Profile definition / metadata / item

**依赖：** T01。**允许改动：**

```text
backend/chimera-config/src/profile/definition.rs
backend/chimera-config/src/profile/item.rs
backend/chimera-config/src/profile/metadata.rs
backend/chimera-config/src/profile/mod.rs
```

**交给模型：**

> 对照 ref 的 `definition.rs`、`item.rs`、`metadata.rs` 及 exports，迁移 File/Composition、Overlay/Script、TransformRuntime、ProfileItem 和 metadata provenance/patch DTO。确认当前 ref 实际符号后再动手，不按手册推测 API。保持 brand crate 名 `chimera-config`，不要搬入 Tauri 逻辑。运行 chimera-config domain tests。

**验收：** Composition 能表达 ref 的 base/contributor/transforms；Config/Transform 的 source 与可激活语义明确；metadata/user-name provenance 与 ref 相同。

## T03：Profiles invariants / dependency / patch / tests

**依赖：** T02。**允许改动：**

```text
backend/chimera-config/src/profile/dependency.rs
backend/chimera-config/src/profile/patch.rs
backend/chimera-config/src/profile/profiles.rs
backend/chimera-config/src/profile/tests/*.rs
backend/chimera-config/src/profile/mod.rs
```

**交给模型：**

> 对照 ref 同路径文件及全部 `profile/tests/*.rs`，迁移 Profiles validation、revision、mutators、dependency closure/index、sanitize 和 wire contract。对每项旧 Chimera 语义差异解释如何按 ref 收敛，特别检查 current 单选、全局 transforms、引用校验及删除约束。只改允许清单；不要改旧 Tauri Profile schema 或 UI。运行 `cargo test --manifest-path backend/Cargo.toml -p chimera-config` 和 `cargo fmt --manifest-path backend/Cargo.toml --all -- --check`。

**验收：** domain 测试实际运行并通过；禁止通过删除/放宽断言来“对齐”；如旧测试语义冲突，标明证据并替换为 ref 对应合同测试。

## T04：Profile document migration 与 stamp

**依赖：** T03。**允许改动：**

```text
backend/tauri/Cargo.toml
backend/Cargo.lock
backend/chimera-core/src/format.rs
backend/tauri/src/core/migration/fs.rs
backend/tauri/src/core/migration/mod.rs
backend/tauri/src/core/migration/registry.rs
backend/tauri/src/core/migration/runner.rs
backend/tauri/src/core/migration/store.rs
backend/tauri/src/core/migration/modules/mod.rs
backend/tauri/src/core/migration/modules/profiles.rs
backend/tauri/src/core/migration/fixtures/**
backend/tauri/src/setup.rs
```

**交给模型：**

> 从固定 ref 的 `backend/nyanpasu-core/src/format.rs` 和 `backend/tauri/src/core/migration/{fs.rs,mod.rs,registry.rs,runner.rs,store.rs,modules/*}` 迁移 ProfilesDocument 格式、revision stamp、各 profile schema steps、备份/原子写、迁移状态校验和启动顺序。先确认 Chimera 的现有 migration 是否与 ref 不同；不得整个覆盖现有迁移系统。不要把旧 schema 变成 warning 后加载空 Profiles；复刻 ref 当前迁移与错误/恢复策略。只用隔离 fixture/tempdir，禁止读取或更改用户实际配置。更新 profile fixtures 和相关测试，运行目标 migration tests、`cargo check --manifest-path backend/Cargo.toml -p chimera` 及 fmt check。

> 复制遇到以下已核实的项目适配时，保持迁移语义并记录：ref Tauri 将 `serde_yaml` 别名到 `serde_yaml_ng`，Chimera 同时依赖两种不同 crate，因此 stamp/document 路径必须使用 `serde_yaml_ng`；ref package 版本为 `2.0.0`，Chimera 当前 app 版本为 `0.24.1`，迁移 `introduced_in` 门槛必须对应 Chimera 的 `0.24.1` 才会在本产品执行。保留本地 `typed_config` 模块与现有 UI/setup 行为；测试所需 dev-dependencies 只从 ref 的现有依赖版本映射。

> 当前 legacy Profile client 仍读取并写入旧 schema，而 ref migrator 会把同一个 `profiles.yaml` 改写为新 schema。为避免旧主界面、legacy UI 或 agent 把用户 Profile 读成空配置，T04 必须在 setup 使用临时 `with_paths_before_profile_client_migration` 边界：隔离测试可完整运行 Profile migrator，正常启动只运行仍兼容的迁移。T05–T09 将所有消费者接入共享 ref Profile API 后，再在最终集成阶段移除此边界并启用迁移；不得提前激活。

**验收：** 隔离 fixture 按 ref 迁移、恢复备份和 stamp 状态均通过；重复迁移不造成重复破坏；无效/状态不一致输入保持原文件并返回 ref 定义的错误。生产启动在旧 Profile client 仍存在期间保留 `profiles.yaml` 原字节，不写入 ref schema；最终集成时有明确证据后才启用迁移。

## T05：Profile ports、service 与 actor

**依赖：** T03、T04。**允许改动：**

```text
backend/tauri/Cargo.toml
backend/Cargo.lock
backend/tauri/src/state/profiles/**
backend/tauri/src/state/mod.rs
backend/tauri/src/enhance/golden_support.rs
backend/tauri/src/enhance/mod.rs
backend/tauri/src/service/profile_file.rs
backend/tauri/src/service/mod.rs
backend/tauri/src/client/profiles.rs
backend/tauri/src/client/profiles_actor_client.rs
backend/tauri/src/client/mod.rs
backend/tauri/src/setup.rs
```

**交给模型：**

> 以 ref `state/profiles/{mod.rs,ports.rs,actor.rs,scheduler.rs}`、`service/profile_file.rs`、`client/profiles.rs` 和 `client/mod.rs` 为逐文件基准，迁移 Actor ownership、消息、CAS、revision、`affects_current`、fetch-before-commit、stale fence、scheduler、external watcher、materialization prepare/promote/complete/compensate、cleanup/reconcile 与 actor client API。Chimera `client/profiles.rs` 当前有用户差异，必须先看 `git diff` 并保留无关改动，禁止整文件覆盖；将 ref actor client 暂放在 `client/profiles_actor_client.rs`，在 T06–T09 所有消费者切换后再收敛回对应 `client/profiles.rs`。不得增加双写；所有 filesystem/network 走 ports；阻塞 filesystem 使用 ref boundary。目标测试按 ref actor/client/service 测试结构运行。

> T04 的迁移闸门必须保留：当前 `setup.rs` 和旧 client 仍读写旧 `profiles.yaml`。本任务不要启动新 actor 接管该文件，也不要删除 `with_paths_before_profile_client_migration`。只有 T06–T09 把 IPC、主 UI、legacy UI、agent 全部切到同一新 API 后，才在 T09 最终集成中注入新 actor 并启用 Profile migrator。

> 若复制的 ref 模块使用了 Chimera 尚未直接依赖的 crate，只从对应 ref `Cargo.toml` 复制该依赖及版本；通过 Cargo 更新 `backend/Cargo.lock`，禁止手改锁文件或顺手同步无关依赖。

> 如果 actor 测试依赖 ref 的 `enhance::golden_support`，仅复制对应 test-only fixture，并在 `enhance/mod.rs` 用 `#[cfg(test)]` 声明；不要复制或替换整个 enhance 实现。

**验收：** actor、service、ports 与 ref ProfilesClient API 编译并通过目标单测；文件写失败和 state commit 失败分别验证。此阶段 setup 继续使用旧 client 与 T04 闸门；T09 前不得对生产 `profiles.yaml` 启动新 actor。最终 setup 注入与迁移启用属于 T09 cutover 验收，T05 进度中必须注明此临时边界。

## T06：Client workflow / IPC / generated bindings

**依赖：** T05。**允许改动：**

```text
backend/tauri/src/client/application_workflow/**
backend/tauri/src/client/application.rs
backend/tauri/src/client/mod.rs
backend/tauri/src/client/profiles.rs
backend/tauri/src/client/profiles_actor_client.rs
backend/tauri/src/ipc.rs
backend/tauri/src/specta_export.rs
backend/tauri/src/lib.rs
backend/tauri/Cargo.toml
backend/Cargo.lock
frontend/interface/src/ipc/use-profile.ts
frontend/interface/src/ipc/use-profile-content.ts
frontend/interface/src/ipc/query-options.ts
frontend/interface/src/ipc/index.ts
frontend/interface/src/ipc/bindings.ts   # only through generator
frontend/interface/src/utils/index.ts
```

**交给模型：**

> 对照 ref 的 `client/application_workflow/profiles.rs`、workflow 相关模块、`ipc.rs`、`specta_export.rs` 和 interface profile hooks，把 ProfilesClient 接到统一 application facade。补入 ref 的 `query-options.ts` 作为 hooks 所需 IPC 辅助模块。Profile IPC 只做边界转换，错误/degradation 保留 ref 语义；生成 bindings 必须运行项目现有 generator，禁止手改。处理当前工作树差异，只针对这条流程编辑。更新 shared query keys、cache invalidation、mutation 返回值。运行 Rust check/目标测试、binding freshness、`pnpm typecheck` 和 `pnpm lint:frontend-boundaries`。

**验收：** IPC 不再依赖旧 `ProfilesResponse`/ProfileBuilder 业务结构；所有 profile 命令由唯一共享 client 提供；bindings fresh。

## T07：Main UI 与 editor 适配

**依赖：** T06。**允许改动：**

```text
frontend/chimera/src/pages/(main)/main/profiles/**
frontend/chimera/src/pages/(editor)/editor/profile/index.tsx
frontend/chimera/src/components/profiles/**
frontend/interface/src/ipc/profile-definition.ts
frontend/interface/src/ipc/use-profile.ts
frontend/interface/src/ipc/use-profile-content.ts
```

**交给模型：**

> 对照 ref `frontend/nyanpasu/src/pages/(main)/main/profiles/**` 和 profile editor 页面，逐文件迁移 Profile 列表、详情、composition、source、transform chain、导入、更新选项和文件编辑流程。保留 Chimera 视觉/产品扩展，但新业务 DTO 来自 shared interface；不可改后端 schema 来迁就 UI。使用当前页面和组件，不要一次重写整个路由。运行 `pnpm typecheck`、`pnpm lint:frontend-boundaries` 和相关 UI lint/build。

**验收：** 主 UI 和 editor 不再从旧 Profile 类型猜测 source/kind；所有写入用新 hooks/IPC。

## T08：Legacy UI 主动适配

**依赖：** T07。**允许改动：**

```text
frontend/chimera/src/pages/(legacy)/**
frontend/chimera/src/components/profiles/**
frontend/interface/src/ipc/use-profile.ts
frontend/interface/src/ipc/use-profile-content.ts
```

**交给模型：**

> 搜索 legacy routes、windows 和 components 中全部 profile 查询/创建/编辑/激活/删除/刷新调用。将它们改接 ref-aligned shared API，同时保留现有 legacy 页面、展示与用户操作流程。不要为了减少 UI diff 在 Rust backend 添加旧 DTO 或另一套 business bridge。将 legacy multi-current 的交互按迁移后的 Composition/单 current 语义表达；不得静默丢失用户选择。运行 typecheck、boundary lint 和每个受影响 legacy E2E suite。

**验收：** 所有受影响 legacy UI 入口已列全且复用主 UI 的同一 Profile hooks/IPC；E2E 报告实际 suite 与未测项。

## T09：Agent snapshot 与旧业务清理

**依赖：** T06；清理动作等 T07/T08 UI 搜索确认后执行。**允许改动：**

```text
backend/tauri/src/features/agent/**
backend/tauri/src/config/profile/**
backend/tauri/src/config/runtime.rs
backend/tauri/src/client/mod.rs
backend/tauri/src/client/profiles.rs
backend/tauri/src/client/profiles_actor_client.rs
backend/tauri/src/setup.rs
```

**交给模型：**

> 将 agent 对 Profiles 的读取/诊断改为共享 Profile client snapshot；执行继续走共享 API，保留 agent proposal、confirmation、stale-state、audit、verification/recovery。随后 `rg` 查旧 Profile domain、Config::profiles、LegacyProfilesReadPort/WritePort、ref_adapter、旧 IPC/type。只有确认所有生产、main UI、legacy UI 和 agent 调用已迁移后，删除无用旧业务实现；必要 UI 展示 adapter 可留在前端边界，不得维持第二套状态。运行 agent 目标测试、相关 Rust tests、typecheck、boundary lint、bindings freshness、完整受影响 E2E，并逐条报告搜索残留。

> 最终 cutover 时才修改 `setup.rs`，从临时 `with_paths_before_profile_client_migration` 切到完整 Profile migrator，并在 `ChimeraClient` composition 中注入唯一 `ProfilesClient`、`ProfileFileService`、fetcher 与 notifier。启动顺序必须先成功迁移/验证 `profiles.yaml`，再启动 actor；旧 schema 消费者清零前不得提前启用。

**验收：** 不再有生产入口读写旧 Profiles owner；所有残留有具体迁移/fixture/UI adapter 解释；真实桌面、fixture 和纯单元测试分开报告。

## T10：最终对照审查（只读）

**依赖：** T01–T09。**改动：** 不允许改文件。

**交给模型：**

> 按 `docs/profile-ref-replication.md` 对固定 ref commit 做逐文件/符号、生产装配、状态所有权、事务、错误处理、legacy UI 与 agent 的最终审查。运行 `git diff --check`；只读比较 ref 与 Chimera，不要自动格式化或修代码。输出完全对齐、必要差异、未完成迁移、路径残留与实际验证证据。任何尚有旧业务实现承担核心语义的事项都必须判为部分迁移。

## 给模型的单任务反馈格式

要求模型每次最后严格输出：

```text
任务 ID / 状态：完成 / 部分完成 / 阻塞
ref HEAD：
修改文件：
ref 符号 -> Chimera 符号：
保留的本地改动：
验证命令和实际结果：
未运行/失败/跳过：
与 ref 的剩余差异：
下一任务及前置条件：
```
