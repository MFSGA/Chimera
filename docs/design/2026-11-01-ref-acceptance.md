# Chimera ↔ ref v2.0.1：阶段整体验收及下一阶段准入

- 日期：2026-11-01（macOS 开发机本地日期）。
- Base：Chimera `master` `dd5cd88a4ba4ac8ba4c20dc3182a035628f78d65`；ref `518bf49843e1f735380e11da1ebe2a85eb8ce8c6`；Runtime `2cb1160996f912b536418cb62bda470d097c1190`。
- 风险：**L2**（持久化数据、恢复、启动生命周期）。**性质：coder 自查与回归验收，不是不同上下文的独立 reviewer**。缺少独立审查，不能宣称已批准合入。
- 决定：**REQUEST_CHANGES / 暂不可合入**。最重要阻断项是迁移前的真实完整备份缺口；跨平台、真实冷启动及独立 crash-reporter 未验收。
- 变更治理：审查当前全部未提交的 Jobs/redb、配置迁移 I/O、session-state 恢复、Application/Clash 保护、startup-failure 诊断及相应文档；未提交、未推送、未合并、未清理用户数据或执行真实 TUN/Service/系统代理变更。所有代码修改依旧是同一工作区的未提交内容，未来需按可回滚依赖顺序分组审查。

## 已证实的阶段结果

| 运行的命令 / 范围                                                                                                                                        | macOS ARM64 结果                                                                 | 覆盖说明                                                           |
| -------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| `cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib -- --test-threads=4`                                                  | 首轮 745 通过、0 失败、1 ignored；修复后完整重跑 **749 通过、0 失败、1 ignored** | Chimera 库级测试；ignored 为会生成前端 bindings 的测试，不计入通过 |
| `cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera-jobs --all-targets -- --test-threads=4`                                     | 45 通过、0 失败（另有空目标）                                                    | 业务 journal/错误恢复/调度/执行相关单元及集成目标                  |
| `cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera-config -p chimera-core -p chimera-traffic --lib -- --test-threads=4`        | config 172/172、core 105/105、traffic 103/103（traffic 1 ignored）               | 配置和核心业务数据模型、traffic/redb 单元层                        |
| `pnpm exec tsc -p frontend/{interface,utils,ui,chimera}/tsconfig.json --noEmit`（分别运行）                                                              | 4/4 类型检查通过                                                                 | 仅类型检查，**不是**两个 UI 的端到端交互                           |
| `pnpm --filter @chimera/tauri-e2e typecheck`                                                                                                             | 通过                                                                             | E2E harness TypeScript 类型检查                                    |
| `pnpm --filter @chimera/tauri-e2e test:unit`                                                                                                             | 30/30 通过                                                                       | harness 纯单元测试，**不启动 Tauri 应用**                          |
| `cargo test --offline --locked --manifest-path backend/Cargo.toml -p chimera --lib utils::init::startup_file_tests`                                      | 4/4 通过                                                                         | 本轮新修复的 legacy 初始配置文件节点检查                           |
| `cargo fmt --manifest-path backend/Cargo.toml --package chimera --package chimera-jobs -- --check`、`pnpm exec prettier --check ...`、`git diff --check` | 见本轮最终格式验收                                                               | 语法/格式而非功能证明                                              |

所有测试依赖缓存 Nix Rust 工具链和本机 Xcode SDK；原始日志在开发机的 `/tmp/chimera-ref-accept-*.log`，不复制包含本机绝对路径的原始日志到仓库。没有执行 `--all-features`、桌面 E2E、真实 OS 网络操作或 Windows/Linux CI；**不可**把已运行的 macOS Rust/TS 测试解释为这些项目已通过。

## 审查发现（按合入风险排序）

### [HIGH / BLOCKER] 配置迁移缺少执行前完整快照

- **位置**：`backend/tauri/src/core/migration/runner.rs:115-151`；参照 `ref/backend/tauri/src/core/migration/runner.rs:182-270` 的 `ensure_backup`。
- **触发条件**：已存在用户 `profiles.yaml` 等数据，迁移中有一个步骤已成功写入、后续 `typed_config` 或下一步骤失败（或中途进程中断）。
- **预期与实际**：ref 在执行任何 pending 步骤前先对 config 和 storage 创建完整、可验证的备份，失败则阻止迁移；Chimera 本地只对单文件使用安全原子写入，`run_pending` 先执行 profiles 再执行 typed config，出现某个模块失败时还会继续尝试其它模块，**没有同等的事前备份**。各次写入能保持单文件完整，却无法保证一组配置回到迁移前版本。
- **证据性质**：直接源码与执行顺序审查，数据丢失场景属于**可到达的潜在风险，未在真实用户数据上注入复现**。不应在没有备份的情况下把高风险升级归为生产就绪。
- **最小修复方向**：以 ref `core/backup.rs`、`MigrationRunner::ensure_backup`、`PathResolver::backups_dir` 和迁移结果报告作为完整功能对齐范围；确保持久化配置/存储快照有清单、不能穿透外部 symlink、不能越权删除旧备份；只在真实备份验证成功后返回可用备份路径。需要升级/中断/回退的隔离集成测试及 Windows/Linux 审核。

### [HIGH / 已修复局部缺陷] legacy 启动默认文件会把悬空链接误判为缺失

- **原位置**：`backend/tauri/src/utils/init/mod.rs:16-46`，`backend/tauri/src/lib.rs:82-84`；`path.exists()` 在悬空链接上返回 false，`utils::help::save_yaml` 的 `AtomicFile(AllowOverwrite)` 可替换该链接节点，绕开后置迁移与 typed Client 的 symlink 检查。
- **修复**：改为 `utils::init::initialize_default_yaml` → `core::corrupt::config_node_exists` 的节点检查；已有链接/已有文件一律不写，新安装缺失文件仍使用旧模板/header。新增 4 项 `tempfile` 回归验证缺失、有文件、三个悬空符号链接和非法父目录。没有改变真实用户配置或启动网络行为。
- **剩余范围**：这不是全局 legacy 配置写入策略修复；`help::save_yaml` 仍供其它业务调用，本轮仅阻断 pre-migration 默认文件误判。启动期间的错误返回仍由后续问题处理。

### [MEDIUM] 首次 Tauri 启动初始化错误仍未纳入可靠 supervision

- **位置**：`backend/tauri/src/lib.rs:82-95`（`log_err!(init::init_config())`、读取旧配置用于 locale）、`backend/tauri/src/setup.rs:39-75`，以及 `backend/tauri/src/lib.rs` 中 `builder.build(...).expect(...)`。
- **触发条件**：早期读取/目录创建/日志初始化失败，或 Tauri builder 无法创建主窗口；可能未达到 `report_boundary` 的四个出口。配置读取失败时 legacy `IVerge::new` 可能以模板继续，造成诊断不准确。
- **预期与实际**：ref 在 UI 建立前解析目录、执行受保护迁移、归类错误并使用独立 reporter；Chimera 只在 Jobs、迁移准备、迁移执行、typed Client 初始化做结构化错误日志，未提供 GUI 错误提示及相同生命周期监督。
- **修复方向**：在进程入口做 fail-closed 初始化/单例/前置迁移，接入独立诊断进程并管理启动 Owner 生命周期。不可在现有 Tauri `setup` 中补一个只能在窗口创建后出现的弹窗来声称解决。

### [MEDIUM] typed Client 失败报告路径定位不精确

- **位置**：`backend/tauri/src/startup_failure.rs` 的 `StartupFailure::from_error` 回退逻辑、`backend/tauri/src/client/mod.rs` 的 `new_typed_config_clients`。
- **触发条件**：Application/Clash 的加载失败仅产生带上下文的 `anyhow` 源链，内部没有被当前报告器提取的结构化路径类型。
- **实际**：可分类为 `UserConfig`，但回退的 `path` 只是整个配置目录，无法直接指出异常的 `application.yaml` / `clash-config.yaml`。
- **修复方向**：跟随 ref 的 typed `SetupError::LoadApplication/LoadClashConfig` 路径分类，先建立可靠的源类型和精确文件定位，再给 reporter 提供打开文件所在目录的动作，避免靠字符串解析错误。

### [MEDIUM] 诊断与跨平台验证尚无发布级证据

- **位置**：`backend/tauri/src/startup_failure.rs::report_boundary`、`core/corrupt.rs`、`utils/recovery_io.rs`、`client/jobs.rs`、`core/session_state.rs`。
- **风险**：stderr / tracing 当前可能含完整本机路径和原始错误链，尚没有成文的敏感信息策略与独立审核；Windows rename/lock/reparse-point、Linux 文件节点、真实 GUI、进程退出及 Service/Agent 的错误通道均未跨平台测试。
- **处置**：下一批明确可显示、可复制、可上传字段的隐私边界，加入针对注入秘密/订阅 URI 的日志断言；Windows/Linux 系统错误和相同目录重启需要真正执行验证。所有未运行用例明确标注为 _pending_，不能借助纯单元测试替代。

## ref 路径/符号映射及 Chimera 扩展边界

| ref 实现                                                                                              | Chimera 当前实现                                               | 验收状态                                                |
| ----------------------------------------------------------------------------------------------------- | -------------------------------------------------------------- | ------------------------------------------------------- |
| `backend/tauri/src/core/backup.rs` + `core/migration/runner.rs::ensure_backup`                        | `backend/tauri/src/core/migration/{fs,runner,store}.rs`        | **缺失完整事前备份；HIGH**                              |
| `backend/tauri/src/core/session_state.rs::read`                                                       | 同路径新模块与 `client/session_state.rs`                       | 单元/迁移回归通过，真实 UI 未验                         |
| `backend/tauri/src/client/jobs.rs` / `nyanpasu-jobs`                                                  | `backend/tauri/src/client/jobs.rs` / `backend/chimera-jobs`    | 原始证据保全及 redb 兼容测试通过；跨平台未验            |
| `backend/tauri/src/client/{application,clash_config}.rs`                                              | 同名 typed Clients + Chimera legacy mirror                     | YAML/路径错误不覆盖测试通过；旧桥接和运行中配置仍须保留 |
| `backend/tauri/src/core/migration/child_result.rs`                                                    | Chimera `startup_failure.rs` 中部分阶段/版本分类               | 源链/JSON 单测通过，尚未有子进程传输                    |
| `backend/tauri/src/startup_failure.rs` + `backend/nyanpasu-crash-reporter` + `cmds/startup_failed.rs` | Chimera `startup_failure.rs` 只做诊断日志、`setup.rs` 四个边界 | **独立 reporter GUI 未迁移**                            |

主/legacy UI、Agent 应持续共用 `ChimeraClient` 与配置/运行事务。不能因迁移上游启动链删除 legacy UI 或绕过 Agent 确认/权限，不能更改 Chimera Client/Service core 运行语义。

## 下一阶段建议：先保障回退，再迁移独立 crash-reporter

1. **首先解决 HIGH：备份契约及迁移进程时序**。移植 ref 的备份模块、可验证 manifest、备份目录 API 与 `ensure_backup`。补齐已存在文件、符号链接、拒绝权限、部分复制、运行中 redb、修订号较新、磁盘耗尽/写失败、重启后重试的隔离验证。无已证实快照时不运行会改写用户数据的迁移。
2. **梳理 pre-Tauri 初始化与监督入口**。规划 `run` 入口、单例、logger/paths、迁移子进程（若需要）、owner 生命周期取消与 GUI 创建顺序。独立处理迁移失败和非迁移失败，保留 existing Core/Service/Agent 与 legacy 双窗口接口。这里涉及较大架构调整，需设计评审和相应批准。
3. **完整引入 reporter 进程和 UI**。在 ref 的错误协议、独立进程入口、i18n 与 reporter 窗口之间对齐；设置诊断字段最小化/隐私保护；只有真实存在的日志/备份位置才可操作。子进程失败也必须有简洁的操作系统 fallback，不将 CLI 成功视为可见窗口已展示。
4. **桌面/跨平台验收**。先在独立 macOS 测试环境做正常启动、损坏应用/Clash/Jobs/session-state、备份前失败、迁移中断重启等无真实宿主代理更改的冷启动。Windows/Linux 用 CI 或专用环境实测文件 API、报告进程和系统资源；真实 TUN/Service/系统代理修改须独立机器/专用 VM 与明确授权。独立 reviewer 使用另一个上下文复核真实 diff、所有测试与无未授权副作用，合格后才考虑提交集成。

## 回滚与交付状态

本轮唯一产品代码修复为 `backend/tauri/src/utils/init/mod.rs` 的 default YAML 创建判定及其新增单测；可单独撤销，不应对前轮 Jobs/session-state/typed-client 恢复改动作整体 `git reset`。新文件 `docs/ref-alignment-acceptance-2026-11-01.md` 和 `docs/ref-alignment-progress.md` 只存储审核记录。无 Git commit/stage/push/merge，未运行桌面 E2E、不清理 `backend/target` 或用户数据；有需要时仅考虑经核实可重建的 Rust incremental 缓存。

**独立 reviewer：未完成。最终审查意见：REQUEST_CHANGES，待 HIGH 风险和跨平台/GUI 证据满足门槛后再次验收。**

## 2026-11-01 后续进展：迁移前备份代码补齐（仍待独立审查）

上述 HIGH / BLOCKER 是**原验收时的历史发现**，不应擦除。此后参照 pinned ref 的 `backend/tauri/src/core/backup.rs`、`core/migration/runner.rs::ensure_backup`，Chimera 新增 `core/backup.rs`、`PathResolver::{backups_dir,scripts_dir}`、pending-step 和 schema-stamp 前备份门槛、启动错误报告中的已创建备份定位。macOS ARM64 `chimera --lib` **771 通过、0 失败、1 ignored**，覆盖备份失败拒绝 typed 改写、后续步骤失败仍保留备份、无虚构恢复可用性及隔离文件/符号链接测试；详情见 `docs/ref-alignment-progress.md` 最新一节。

**状态更新：原先“完全没有事前备份”的源码缺口已补齐，但发布准入仍为 `REQUEST_CHANGES`**，因为真实冷启动、Windows/Linux、存储文件被另一进程持有时的一致性、恢复工作流、备份容量限制/磁盘耗尽、独立 reviewer 均尚未验收。此外当前 `init_config()` 会在迁移 Runner 前创建 legacy 默认文件，因此首次 GUI 启动可能对生成的默认配置进行额外快照；纯 Runner 的空目录测试不能证明 GUI 首次启动不会生成备份。

## 2026-11-01 再验证：备份内容清单与隔离恢复

macOS ARM64 上的最终完整 `chimera --lib` 回归 **776 通过、0 失败、1 ignored**。原 ref v2.0.1 `core/backup.rs` 的 manifest 只描述备份来源、版本与链接，不包含逐文件完整性摘要。Chimera 在迁移前快照中增加 `format_version=1`、`files`（长度/SHA-256）、`directories`，并在提交快照与启动故障报告展示前重新校验文件树，防止把内容损坏的目录当作可靠的恢复入口。损坏、无清单的旧 ref 格式与未确认归属的临时节点不自动清理，`prune_backups` 仅对真正通过验证的目录应用保留数量。

隔离 `#[cfg(test)]` 恢复测试用真实 redb 数据库和已改写的源配置验证升级前内容确实能从快照还原到一个独占新建的空目录，亦覆盖空目录及 Unix 符号链接的原样重建、损坏文件/不安全清单提前拒绝和已有目的目录不覆盖。这**不是**产品级恢复流程：仍需操作系统级关闭/独占锁、实际 UI 提示与确认、跨平台恢复、合规的磁盘限额和可信 manifest 处理。SHA-256 与文件一起保存在同一个可修改目录，并不证明快照未被有意整体伪造。相关具体测试、代码差异和回滚路径以 `docs/ref-alignment-progress.md` 最新一节为准。

**合入状态仍为 REQUEST_CHANGES**，直到正式恢复流程、独立 reviewer 与 Windows/Linux/真实桌面验证完成。此处仅更新本轮后续证据，不改写最初风险发现的历史结论。
