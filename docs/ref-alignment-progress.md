# ref 对齐进度

## 当前迭代目标（2026-10-27）

- 按固定本地 `ref/` 逐批将 Chimera 共享实现对齐上游。每批开始先同步远端 `master`、记录基线和符号映射；优先整段复制完整、相互依赖的上游模块，再做必要且可审查的品牌/平台适配，避免为缩小 diff 拆散迁移。
- 先编译并运行目标模块的定向单测；修复编译/测试问题后，再补适用的主 UI、legacy UI、Agent 覆盖和简单的隔离桌面 E2E（优先用真实控件点击验证）。明确区分编译、模拟边界测试、桌面 E2E 与真实宿主副作用验证，不把前几者说成后者。
- 按用户本轮明确要求直接在当前本地 `master` 逐批推进，不新建功能分支或 worktree。每个提交保持单一清晰目的并签名；本地目标检查通过后推送 `master`，再按准确 SHA 查看 CI/E2E。远端检查 pending 时继续其他独立且已授权的工作，不将 pending 当作成功；发现失败就在 `master` 修复并重新验证。
- 对用户指定合并的旧分支，先独立复核其独有差异；若源码已在 `master`、其余改动会回退较新依赖或基线，则只记录分支已包含，不重复覆盖。不得借此处理无关分支或改动用户数据、`ref/` 与 host networking。
- Codex 当前 Goal 接口只提供读取与状态更新，不提供编辑目标描述；本节作为该持续迭代任务的可维护目标说明，不能替代或谎称已修改原 Goal 对象。

## 参考基线同步（2026-10-29）

- 上游：`https://github.com/libnyanpasu/clash-nyanpasu` 的 `main`，本次通过 SSH 获取并检出 `2fadf212f161417dcdf2f37d81a305a37ff918a4`。旧 Chimera gitlink 为 `8e1084e4e7c50531a5d16ca9a6c9dd2bb2f0f8e8`，新提交的 parent 与旧 gitlink 一致。上游提交仅更新 `@tanstack/react-virtual` 3.14.23→3.14.24 及锁文件中的 `@tanstack/virtual-core` 3.19.8→3.19.9。
- 更新后的 ref 根工作区干净；递归子模块分别为 `backend/enable-loopback` (`f1b9f8693a328ca0a6094951035b81fe302b0848`)、`vendor/windows-rs` (`d1574fd27371a9b08bc5508f70c43ccb4f84c543`)、`backend/nyanpasu-runtime` (`e73b9f5cc2aa91ac74b4c71d0acc5b48e0fd2b34`)、`nyanpasu-utils` (`2706c1b69a3b5e4f0367fb54b4a87e67769c19f8`)，均已检出。上游签名在本机无法核验：本地 GPG keyring 缺少签名者公钥；因此不声称其签名已验证。
- 本批只更新 `ref` gitlink 和本指南的基线记录，不移植依赖或产品代码；不涉及运行时行为、IPC、持久化或主机网络。检查：SSH fetch 成功、ref HEAD 与目标 SHA 相同、ref 状态干净、`git diff --check` 通过。产品构建与桌面 E2E 不适用。
- 风险：L2（参考基线变化，影响后续对齐来源）。回滚时将 gitlink 恢复到 `8e1084e4e7c50531a5d16ca9a6c9dd2bb2f0f8e8` 并还原本节和指南基线。

## 参考基线同步（2026-10-29，后续提交）

- 上游：Clash Nyanpasu `main` 更新到 `2c017d63bfec720017a9b57db629eeddd2c9cd3d`，其 parent 是原基线 `2fadf212f161417dcdf2f37d81a305a37ff918a4`。GitHub API 确认该提交为 `main` 当前 tip 且签名有效；本地通过 `origin/main` fetch 得到相同 SHA。父仓库 `ref` gitlink 从 `2fadf212` 更新到该提交。
- 上游变化：只更新 `meta-json-schema` 1.19.32→1.19.33 及锁文件；`backend/nyanpasu-geodata`、`backend/tauri/src/core/geo` 和 `backend/tauri/src/core/traffic/geo.rs` 均未改变。`backend/nyanpasu-runtime` 仍为 `e73b9f5cc2aa91ac74b4c71d0acc5b48e0fd2b34`。
- 验证：fetch 成功；ref 根目录和两个已检出的子模块均干净；GitHub API 签名状态为 valid；`git diff --check` 与文档格式检查待提交前执行。本批只更新参考基线记录和 gitlink，不改变 Chimera 运行时代码、持久化、IPC 或主机网络行为。
- 风险：L2（参考基线变化，影响后续对齐来源）。回滚时将 gitlink 恢复到 `2fadf212f161417dcdf2f37d81a305a37ff918a4`，并撤回本节和指南元数据更新。

## GeoIP/traffic 候选分支复核（2026-10-29）

- 候选：`codex/geodata-traffic-20261020`，tip `fec56bff4eb98b880282f94713cd763c230282a4`。独立复核确认该分支的 GeoIP crate、traffic/core 接入和测试代码已与 master 中的 `d9624314486e45c905876242dee5d3aacd161cba`（PR #505）相同；该提交已是 master 的祖先。
- 其余差异只有参考对齐文档、旧依赖版本及 `ref` gitlink。分支的依赖比当前 master 低，文档回退到更旧基线，gitlink 也比 master 旧；不将这些过时内容覆盖到 master。GeoIP 功能没有待补的独有源码改动。
- 独立复核未发现 GeoIP 实现 P0–P2 阻塞；记录的保留差异是 ref 未启用本机公网 IP 探测，因此本地/私有源地址可显示为 unknown。该功能的必要源码和测试已由 #505 集成；本次只处理候选分支历史归属，不再重复应用代码。

## Dashboard resize settle 对齐（PR #534，修复复核发现后重新验证中）

- 目标：对齐 Clash Nyanpasu `97e43308edabc95677c65741c81db1eeeb51689a` 的 Dashboard resize settling：窗口连续变化期间维持当前布局来源，尺寸稳定 200 ms 后才选择新预设布局；首次测得网格尺寸仍立即初始化。布局选择延迟撞上拖拽/调整大小时，等当前交互结束再应用。
- 非目标：本批不迁移 Dashboard 布局存储 schema、preferred layout id、其他 widget、DnD Grid API、legacy UI 或 agent；不写入 resize 产生的自适应投影。
- 参考映射：`ref/frontend/nyanpasu/src/pages/(main)/main/dashboard/index.tsx::{SETTLED_RESIZE_MS,selectForSize,settledTimerRef,handleSizeChange}` → `frontend/chimera/src/pages/(main)/main/dashboard/index.tsx` 中的相同常量/职责；保留 Chimera 当前 `findBestLayout`、`findClosestStoredLayout`、`adaptLayout` 与 keyed `LayoutStorage`。
- 接口与风险：新增 `DndGrid.onInteractionChange` 可选回调，报告网格拖拽/resize 的开始与结束，由 Dashboard 作为本地 UI 状态消费；无 IPC 或持久化格式变化。L1 局部 UI 交互。resize 投影只更新显示态，settled selection 不写存储。
- 验收：桌面 E2E 用快照/恢复的确定布局验证宽屏与窄屏选择、宽屏恢复及 `dashboard-widgets` 不变；timer 在网格交互活跃时不得切换布局。typecheck、格式/静态检查与目标桌面 E2E 通过。
- 回滚：撤销 Dashboard resize timer 与对应 E2E/contract/progress 记录，不改用户数据。
- 审批触发：不涉及架构或数据迁移；按用户对日常迭代的持续授权推进，不需要额外审批。
- 实际验证：初版 `9827e35f` 的 `pnpm typecheck:chimera`、`pnpm typecheck:e2e`、改动文件 Oxlint、Prettier 和 `git diff --check` 通过；独立复核发现交互期间布局切换竞态，以及首版 E2E 未断言窄屏实际布局。两项已修复，后续 SHA 的定向检查和 Windows hermetic E2E 待完成。本机桌面构建未运行：构建卷仅剩约 2 GiB 可用空间。

## App/Clash typed settings 接入（独立复核完成，Windows 桌面 E2E 待运行）

- 目标：按固定 ref 的 typed settings hooks 和 IPC 合同，让主界面、legacy UI 及共享组件通过同一组 typed application API 读取和修改应用设置、Clash 持久设置；runtime-only 的 Clash 覆盖仍通过独立 runtime API 应用。
- 非目标：本批不删除受支持的 legacy UI、Chimera Client/Service、旧用户配置读取、legacy IPC 或 Agent 能力；不迁移 Dashboard/Session/Profile 等无关设置域，不改 TUN 授权策略。
- 参考基线：Clash Nyanpasu `ref` HEAD `685042aaedaea26a591900be0b494dfe8efd79b0`；`git -C ref status --short` 干净。父仓库的 `ref` gitlink 与当前只读 checkout 不同是该工作分支原有状态，本批不移动或修改子模块。
- 参考路径和符号：`ref/frontend/query/src/ipc/use-settings.ts::{useTypedConfig,useTypedConfigField,useSettings,useSetting,useClashSettings,useClashSetting}` → `frontend/interface/src/ipc/use-settings.ts`；`ref/backend/tauri/src/ipc.rs` 的 typed app/Clash get/patch commands 与 `ref/backend/tauri/src/client/{application,clash_config}.rs` → 对应 Chimera IPC/client；`ref/backend/chimera-config/src/{application,clash/config}` → `backend/chimera-config/src/{application,clash/config}`。保留上游的 sparse typed patch、React Query invalidation 和字段 hook 结构。2026-10-19 复核 master pinned ref SHA `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3` 的 `useTypedConfig::onSuccess` 为 block callback：触发 invalidation 但不返回 Promise；Chimera 曾返回 Promise，导致 mutation completion 等待 query refetch。当前修复恢复 ref 的后台 invalidation 语义，IPC 与存储合同不变。
- 变更接口：新增/注册 `get_app_config`、`patch_app_config(ChimeraAppConfigPatch)`、`get_clash_config`、`patch_clash_config(ClashConfigPatch)`；生成 TypeScript IPC bindings。旧的 core `/configs` 操作命名为 `patch_runtime_overrides`，避免它与保存型 `patch_clash_config` 混用。共享 `useSettings/useSetting/useClashSettings/useClashSetting` 现在调用 typed API；locale 只在 UI 边界映射到后端枚举。
- 运行时行为：typed Clash patch 依据共享 `classify_clash` 和 `RequestedRuntimeFields` 判定是否需要 core lifecycle reconcile。改变运行输入会重建并应用当前核心配置；重复提交运行字段也会重新尝试；只改 Web UI 列表等 UI 字段不会重建核心。App typed `core`、`enable_builtin_enhanced` 和已连接 Service 模式下的 `enable_service_mode` 写入经过 core lifecycle actor，先 reconcile 候选运行配置，再按版本提交持久设置；候选 reconcile 失败时尝试恢复最新持久设置对应的运行态，版本冲突后也恢复最新持久设置对应的运行状态，恢复失败会连同原错误返回。Clash typed patch 在候选运行态已应用但后续保存失败时，回滚来源配置后会重新 reconcile 当前最新配置；补偿失败与原错误一起返回。App/Clash typed patch 将当前 runtime 的 mixed port 发布给 effect actor，再由现有 typed system-proxy effect 应用新端口；不在新入口中直接读取 Verge 端口或调用旧 `init_sysproxy`。
- 保留的兼容边界：`LegacyVergeBridge`/`LegacyClashBridge` 继续把 typed 持久状态投影给尚未迁完的核心、Service 与旧配置存储消费者；`get_verge_config/patch_verge_config` 只保留为旧 IPC surface 和显式兼容性验证入口。主界面、快捷键、托盘开关、系统事件与 Agent 网络动作现已调用 typed app/Clash API，不再经过旧 `patch_verge` 业务路径。App runtime patch 复用现有 core lifecycle actor 和 CoreFacade 适配层，没有另建兼容业务路径；这仍与 ref 的 ApplicationWorkflow mutation coordinator 不同，待 Chimera 运行时生命周期收敛后再评估统一。旧配置字段别名仅保留在 Rust 输入反序列化边界，生成的前端 patch contract 暴露规范字段。下一步应迁完仍依赖旧 IPC 的测试/调用者，再评估删除命令；typed→旧配置投影在完成旧文件迁移与 Service 消费者收敛前保留为单向适配。
- 验收：主/legacy UI 共用 typed hooks；生成 bindings 与 Rust patch 可选字段一致；Clash 运行字段变更实际触发生命周期 reconcile，UI-only字段只持久化；运行态和 `clash-config.yaml` 同步；错误输入不改变运行态或文件；保留旧配置和 Service/Agent 入口。
- 风险：L2（共享持久配置、运行时核心协调、系统代理）。测试计划：配置模型单测、Clash reconcile 判定单测、backend check、typed-state E2E、interface/E2E 类型检查、改动文件格式检查、diff 检查；主/legacy UI 使用同一 hooks，桌面覆盖以 hermetic settings suite 验证。
- 回滚：回退 typed hooks/commands 和调用点，恢复原有 settings/runtime IPC 命名及前端调用；保留原配置文件与用户数据，不删除 `application.yaml` 或 `clash-config.yaml`。合并前需 L2 独立复核；提交/推送需维护者明确批准。
- 实际验证：`cargo test --locked --manifest-path backend/Cargo.toml -p chimera-config` 143/143；完整 `cargo test --quiet --locked --manifest-path backend/Cargo.toml -p chimera` 默认并行运行 572 passed、0 failed、1 ignored，其中新增内置增强候选配置和 Clash typed patch 回滚测试通过；Service Mode typed patch 的候选失败恢复、版本冲突恢复也有回归测试。一次较早的并行全套运行中 3 个 Profile 文件监听用例超时；3 个用例各自单独重跑及完整串行运行均通过，之后完整默认并行复跑也通过，因此未确认产品回归。`typed_clash_patch_restores_source_and_runtime_after_persistence_failure` 使用真实 Clash 状态 actor 与配置文件，在 legacy persistence 回调注入失败，并检查来源配置与模拟运行态均回滚；未执行真实磁盘保存错误注入或完整内核 E2E。`typed_clash_compensation_reports_runtime_reconcile_failure` 覆盖错误聚合。`cargo check`、`frontend/interface` 与 E2E TypeScript 检查、E2E harness 单测 25/25、`pnpm lint:frontend-boundaries`、目标 Rust rustfmt、改动 TS/文档 Prettier、`git diff --check` 通过。Rust 全套测试中的 generated-bindings freshness 检查通过；pnpm 对临时 worktree 的 `.pnpm` hoist symlink 发出安装警告，但该测试通过。全量 `frontend/chimera` typecheck 未验证：本 worktree 缺少 Paraglide 生成目录，生成器在解析项目远程模块时无输出停滞后已中止；复用另一 worktree 的旧生成输出也产生消息键过期错误，故不作通过结论。桌面 E2E 尚未本机运行。`clash-typed-state.e2e.ts` 位于 hermetic/settings suite，直接通过 typed IPC 观察内核 `/configs` 与隔离配置文件，并检查无效端口输入保持两者不变；`windows-service-tun-lifecycle.e2e.ts` 也已改用 typed `get_app_config`/`patch_app_config` 和 `get_clash_config`/`patch_clash_config` 覆盖真实服务切换入口，两者都完成 TypeScript 检查但未在本机运行桌面 E2E。


## 参考基线同步（2026-10-17）

- 上游：Clash Nyanpasu `main`，已核对最新提交 `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3`（2026-10-17 UTC）。
- Chimera 原固定 gitlink：`31019da14006b384b3272c72b4088179a9e72c47`；本 PR 把 `ref` 更新到上述提交。此前 PR 草案目标为 `a4f706fe788e3570247ad2fa49636c6b4c55c98b`；从该草案基线到最新上游增加 3 个提交：`8f9d7e3a30` 更新上游 `libc`，`6b609e4140` 更新上游 `@hookform/resolvers`，`d43819fbb2` 修复隐藏窗口时 WebView 的可见性与内存目标。
- 对齐差异：上游新增 `backend/tauri/src/window/engine.rs::set_in_sight` 和 `backend/tauri/src/window/manager.rs::follow_minimized`，在 Windows 窗口隐藏/最小化时隐藏 WebView 并请求低内存目标，在窗口恢复时恢复可见性。Chimera 对应实现目前集中在 `backend/tauri/src/window.rs` 的 `AppWindow::create` / `focus_existing_window`，窗口事件由 `backend/tauri/src/lib.rs` 处理，尚无等价的 WebView 可见性跟随逻辑；这是下一批值得评估的共享运行时对齐项，需补充 Windows 生命周期回归验证后再迁移。上游两个依赖更新只记入基线，不移植到 Chimera。
- 上游范围：`a4f706f..d43819f` 仅改 5 个文件（两个窗口运行时文件、`backend/Cargo.lock`、`frontend/nyanpasu/package.json`、`pnpm-lock.yaml`）；runtime 递归子模块 gitlink 未变化。本批只更新参考 gitlink 和本记录，不改变 Chimera 产品代码、依赖、IPC、持久化或宿主网络行为。
- 验证：通过 `git ls-remote` 核对上游 `main` tip，并在外接构建卷的浅克隆确认 checkout HEAD 为 `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3`、工作树干净；对比 `a4f706f..d43819f` 的文件清单及窗口代码。主卷仅余约 104 MiB，直接在原 `ref` 子模块 fetch 未能写入 `FETCH_HEAD`；因此本 PR 记录的是已独立核实的上游提交，原本地子模块目录暂未切换。仅更新 gitlink 与文档，不运行产品构建或桌面 E2E；PR CI 不检查实时上游 tip。
- 风险：L2（参考基线与后续依赖/运行时审查来源变更）。回滚时恢复原 `ref` gitlink `31019da14006b384b3272c72b4088179a9e72c47`，并撤回本节。

## 当前参考基线同步（2026-10-20，本批）

- 上游：Clash Nyanpasu `main` `ae0db596e96ba09d7662cc7533a13ffe5b581bf7`；通过 GitHub API 核对当前分支 tip，并确认该提交的 GitHub signature verification 为 valid。目标树中的递归 runtime gitlink 为 `a130ca7b6015121d49e43ecd0d6af1862e60253e`。
- Chimera 原固定 gitlink：`d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3`。本批仅更新参考 gitlink 和基线文档；不移植 ref 的产品代码、工作流或依赖。`d43819f..ae0db596` 共 44 个上游提交；其中新增 `nyanpasu-tsdb` 时序存储 crate 与其持久化、内存预算、恢复和故障验证体系，并更新 widget、proxy provider cache、updater changelog、配置快照和多项依赖。这些上游能力仍需逐项映射与完整验证；尤其 TSDB 属于架构迁移，不能仅凭 gitlink 更新视为已接入 Chimera。
- 验证：GitHub API 确认 `main` tip 与目标 SHA 一致、目标提交签名有效；隔离 worktree 的 `git -C ref rev-parse HEAD` 为 `ae0db596e96ba09d7662cc7533a13ffe5b581bf7`，`git -C ref status --short` 无输出；递归 runtime 和 `nyanpasu-utils` 子模块分别为 `a130ca7b6015121d49e43ecd0d6af1862e60253e` 与 `f57f9f077c3f1cb31da0e8261c21bee7bdd75c8f`，均已检出且干净。`git diff --check` 与文档 Prettier 检查通过。PR 的新克隆/CI 仍需验证其 checkout；本批没有产品代码，桌面 E2E 不适用。父仓库自身的 `backend/chimera-runtime` 在这个隔离 worktree 中未初始化，不属于本次 `ref` 递归状态。
- 风险：L2（参考依赖基线）。本批只改变后续对齐的来源，不改变 Chimera 运行时、持久化、IPC 或主机网络行为。回滚时将 `ref` gitlink 恢复为 `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3`，并撤回本节与指南元数据。

## Dashboard 现有小组件配置（实现完成，待 PR CI）

- 目标：按固定 `ref/` 基线为 Chimera 已支持的六种 Dashboard 小组件补齐逐实例配置、默认值恢复、保存状态和错误重试，并继续使用现有共享 KV 存储与跨窗口事件。
- 非目标：本批不迁入缺失的十种小组件，不更改 Dashboard 布局键/数据结构，不改 legacy UI、Agent、核心服务或宿主网络行为。
- 参考基线：Clash Nyanpasu `main` `685042aaedaea26a591900be0b494dfe8efd79b0`；功能实现时的参考基线；该提交是当前 `ref` 基线 `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3` 的祖先。
- 参考映射：`ref/frontend/nyanpasu/src/components/widgets/{consts.ts,provider.tsx,widget-config-menu.tsx,widget-config-row.tsx,widget-config.ts,widget-item.tsx,widget-sparkline.tsx,widget-shortcut.tsx}` → `frontend/chimera/src/components/widgets/` 下同名文件，保留上游目录、文件拆分与共享符号；Dashboard route 从该目录导入。`ref/frontend/nyanpasu/src/pages/(main)/main/dashboard/_modules/edit-action.tsx` → Chimera `_modules/edit-action.tsx`；Chimera 专有的布局适配、组件库与放置逻辑仍留在 Dashboard route `_modules/`。`ref/frontend/query/src/hooks/use-kv-storage.ts` → `frontend/interface/src/hooks/use-kv-storage.ts`（保留 Chimera bindings/事件接口并补齐返回写入状态和读取错误恢复）。
- 变更接口：新增存储键 `dashboard-widget-configs`，版本化、按 widget instance id 保存；现有布局 KV 键和 schema 不变。`useKvStorage` setter 增加成功布尔值，状态对象增加 `readError` 与 `refresh`；现有调用方可继续忽略这些新增结果。Traffic Down/Up、Connections、Memory、Proxy Shortcuts、Core Shortcuts 均按实例读取/保存配置；Provider 将连续变更交给 hook 的串行写入队列，并只展示最新写入结果，避免保存中的后续调整被丢弃。配置模型单测已登记到 frontend CI；本次 workflow 改动已获维护者批准。
- 验收：六种配置可打开、变更并持久化；读取失败可重试且不阻塞退出编辑，写入失败不显示成功；配置独立于布局并能恢复默认值；主 Dashboard E2E 覆盖读取/写入失败及重试、刷新后状态；单元测试覆盖配置归一化、未知/非法字段和实例隔离。
- 风险：L2（共享持久化 hook 与 Dashboard 用户偏好）。测试计划：配置模型单测、E2E harness 单测、frontend typecheck/build、frontend boundary check、主 UI dashboard E2E、格式和 diff 检查；真实桌面覆盖缺失时如实标记。
- 回滚：回退本批前端/接口代码、配置 UI 和测试入口；保留既有 Dashboard 布局键，并且不删除 `dashboard-widget-configs` 数据。该 L2 功能与 L3 workflow 改动已获维护者批准。
- 保留差异与收敛计划：ref 的 `CoreShortcuts.showChannel` 依赖 core status 返回活动 controller（HTTP、Named Pipe 或 Unix Socket）。Chimera 的 `get_core_status` 目前只返回 state、变化时间和 run type；本批保留该 IPC 合同，不伪造通道信息。若需达到该项能力，后续独立 L2 批次应从共享 core status/应用 API 暴露运行时 controller，并验证正常、Service 与失败/未知状态。ref 还包含十种当前 Chimera Dashboard 尚未提供的 widget；本批为 Chimera 已有六种补齐配置，不声称迁完整个 ref widget catalogue。ref 的 `LauncherMenu`、`NumberStepper`、`ShapeToggle` 及快捷键按钮的 `presentation/elastic` API 未在 Chimera UI 包中；配置 popover 和快捷键使用现有 Radix、Button、Switch 与系统代理/TUN 控件适配，保留字段语义与动作，菜单/按钮呈现细节待共享 UI 原语对齐时再收敛。另，ref 的 `useKvStorage` 对不同 key 共用写入计数和 revision；独立复核发现不同 key 的写入可能误挡新 key 首次读取，A→B→A 时旧 key 迟到的写入回声也可能覆盖新值。本批将写入计数、revision 与回声按 key 隔离，并由 `KvStorageWriteEchoes` 在全局事件流中消费非活动 key 的未完成回声，避免它们覆盖当前状态；这与 ref 有意不同，后续 ref hook 更新时须重新核对并保留保护。三个可执行单测覆盖 A→B→A、非活动 key 回声和相同值并行写入；本地没有独立 React hook 浏览器测试器，真实 hook 生命周期仍未桌面实测。
- ref 缺陷例外：固定参考提交 `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3` 的 `frontend/nyanpasu/src/components/widgets/widget-config.ts::normalizeWidgetConfigStorage` 将所有非 v1 版本静默归一为空配置；降级运行的旧版随后保存设置会覆盖新版配置。本 PR 对 Chimera 的同名函数改为拒绝任何显式未知版本，并在 `frontend/interface/src/hooks/use-kv-storage.ts` 的首次读取与存储事件路径呈现读取错误、推进 revision 栅栏；Dashboard Provider 现有写入门禁会阻止覆盖。保留其他 ref 对齐；当 ref 增加版本迁移/未知字段保留逻辑或调整 `dashboard-widget-configs` schema 时复查并重新对齐此例外。
- 测试与实际结果：Dashboard 配置模型与布局模型定向测试 6/6；配置模型及 KV 回声 registry（`frontend/chimera/tests/dashboard-widget-config.test.ts`、`frontend/interface/tests/kv-storage-write-echoes.test.ts`）6/6，且已登记在 frontend CI 的 `test:dashboard-widget-config` 命令；`tauri-e2e` 当前注册的 Node unit tests 25/25；frontend interface、Chimera 与 E2E TypeScript 检查通过（Chimera 类型检查使用临时生成的 Paraglide 声明 shim，未将其纳入版本控制）；`scripts/check-frontend-boundaries.ts`、改动文件 Oxlint、Prettier、四语言 865 个 message key parity 与 `git diff --check` 通过。桌面 E2E 未在本机运行。Paraglide compile 在输出“Compiling inlang project ...”后 30 秒无进展，Vite production build 也停在项目编译阶段；进程无 CPU 活动。对应两个 CDN module URL 的 HTTP HEAD 均返回 200，因此当前原因尚未定位；未把旧生成目录或临时类型 shim 当作生成验证。该分支同步最新 master 后的新 PR SHA 将运行远端 CI/E2E；此处记录的是同步前本地检查结果。
- 2026-10-17 复核修复验证：Dashboard 配置模型与 KV 回声 registry 定向测试 6/6；`frontend/interface` 和 `tauri-e2e` 的 TypeScript 检查、改动文件 Oxlint、Prettier 与 `git diff --check` 通过。`frontend/chimera` 直接 TypeScript 检查报告 Paraglide 生成消息模块缺少多个已有调用方的导出（包括 Dashboard 与未改动页面），因此该次全 UI 检查不记为通过；当前工作树 `node_modules` 符号链接也使 pnpm 脚本入口拒绝执行，本地验证改用现有依赖中的二进制。新加的未来版本 UI E2E 未在本机桌面运行，将由推送后的 Windows `hermetic` 套件验证。
- 当前状态：功能代码、配置与 KV 回声单测、主 Dashboard 刷新持久化 E2E、CI 单测入口和对齐记录已准备；真实桌面运行及本机 Paraglide 生成仍未验证。最终独立复核没有发现可操作 P0–P2 问题；复核者确认无 React hook 测试运行器，因此 A→B→A 覆盖测试使用生产回声登记器，React 生命周期集成由桌面 E2E/CI 补证。用户已批准提交、推送并开 PR；PR CI/E2E 尚未运行，不能将待运行检查视为通过。

## Dashboard 小组件库与 DnD Grid（实现、独立复核、CI/E2E 完成，已合入 master）

- 目标：将可复用网格组件迁到 `@chimera/ui` 并对齐上游拖拽源 API；Dashboard 小组件库提供搜索、直接添加，以及将条目拖到网格指定位置。保留现有六种小组件、`dashboard-widgets` 持久化格式和当前布局自适应逻辑。
- 非目标：本批不增加新的小组件类型，不改 legacy UI、agent、核心服务或系统网络行为。
- 基线：Clash Nyanpasu `main` `685042aaedaea26a591900be0b494dfe8efd79b0`（2026-10-16 fetch 后核对）。PR #486 将 master 原有 ref gitlink `c08aa9d9a17953aa2464cdae61390d5c3f9afbc7` 更新到该基线，并于 2026-10-17 以 rebase 方式合入，merge commit `72d61000a63bca6920cc2c55d9a812392bac4c43`。该功能只读取 ref 的前端源码；当时 ref 内部 runtime 指针为 `be2f561102fe898cb7a294ef14de3f4c65b68f81`。
- 参考映射：`ref/frontend/ui/src/dnd-grid/*` → `frontend/ui/src/dnd-grid/*`；新增上游 `DndGridSource`、`DndGridSourceData`、网格命中坐标和 `onExternalDrop`；`ref/frontend/nyanpasu/src/pages/(main)/main/dashboard/_modules/widget-placement.ts` → Chimera Dashboard `_modules/widget-placement.ts`；WidgetSheet 沿用上游可搜索条目和直接添加交互，但使用 Chimera 当前的 Vaul 抽屉及六项 `RENDER_MAP`。`use-grid-layout` 继续依赖 Chimera 已锁定的 `lodash-es`，其余网格结构和符号按 ref 对齐。
- 保留差异及收敛计划：当前 ref 有 16 种 Dashboard 小组件，Chimera 已实现其中 6 种。本批保留 Chimera 现有六项，不复制依赖尚未迁入的 Profile/traffic-report/provider widget 实现与配置；当对应组件和数据合同进入 Chimera 后，再按完整 widget 模块迁入并扩展库列表。
- 接口影响：`@chimera/ui` 新增 `dnd-grid` 与 `search-field` 导出；Dashboard 的拖拽源通过 `onExternalDrop(id, position)` 添加小组件。存储 schema 不变。legacy UI 与 agent 不调用此主界面布局 API。
- 风险：L2（共享 UI 交互及已持久化布局的写入路径）。验收包括定位添加、冲突拒绝、无位置时追加、搜索/直接添加和已有布局恢复；主机网络无影响。
- 实际验证：placement 定向测试 3/3 通过；共享 UI TypeScript 检查、Vite 生产构建、前端边界检查、Prettier 检查及 `git diff --check` 通过。锁文件可解析，四个新增 UI importer 与 manifest 版本一致。全量 Chimera 类型检查仍在未修改的 connections、context-menu、editor、profiles/columns 和测试 Node 类型声明处失败；未报告本批改动文件。独立复核首次发现 Add 按钮未阻止 TouchSensor 的长按事件，已补 `touchstart` 冒泡拦截；复核后续未发现可执行缺陷。
- 未覆盖：桌面 E2E 未运行；当前锁定的 Tauri WebDriver 操作合成 `MouseEvent`，无法证明真实 PointerSensor/TouchSensor 拖拽，搜索和直接添加也未在运行中的桌面窗口验证。`pnpm prepare:check` 未运行，因为它会下载/安装 sidecar 二进制，与此纯前端切片无关。隔离工作树的 `pnpm install --frozen-lockfile` 卡在 supply-chain lockfile 校验；没有绕过该校验，使用本机已安装依赖直接运行定向命令。
- 独立审查：最终复核没有发现可操作缺陷。审查环境因不能创建本地 IPC socket 而未能自行启动定向测试；该测试已在工作区命令行直接执行并通过。
- 回滚：单独回滚 PR #486 时，其 ref gitlink 应恢复为该 PR 前的 `c08aa9d9`；用户存储 schema 未变，不需要数据迁移。若本节顶部记录的后续 ref 基线同步已合并，回滚 Dashboard 功能时应保留新的 ref pin。
- 合并与验证：PR #486 的 CI run `38959272484`、Tauri E2E run `38959272498` 全部通过；独立复核无阻塞项。已于 2026-10-17 以 rebase 方式合入 `master`（merge commit `72d61000a63bca6920cc2c55d9a812392bac4c43`）。

此前 Profile 同步批次的只读参考检出为 Clash Nyanpasu `main` 的提交
`c08aa9d9a17953aa2464cdae61390d5c3f9afbc7`，该提交在上游 Actions 页面对应
`main` 构建，子模块工作区干净。主仓库 `master` 基线为
`e81eeaea46a9c229478cd29c9555b5c072f7e6cc`；本分支将 ref 更新至 `c08aa9d9`。自原基线以来上游合入了 TanStack Router、meta-json-schema、
Playwright、Prettier 和 router-plugin 更新，流量分析侧栏路由重整，以及完整的
profile transform-chain 编辑改进和相应浏览器测试；还补充了 worktree CoW 说明。
递归子模块指针未变。此同步只更新参考基线，不等于将依赖或界面代码直接应用到
Chimera。

DIFF-001 至 DIFF-012 是此前基于
`f7dbce2997c633e484f54788035e770b3ee99773` 完成的历史记录；它们不代表已按
新基线重审。后续切片应使用当前 `ref/` 提交，并明确记录与历史基线的差别。

## ref 基线同步（2026-10-14）

- 上一固定提交：`813673a8e84b51265467f02277fc7e739837f61e`。
- 同步提交：`e81eeaea46a9c229478cd29c9555b5c072f7e6cc`，已 fetch 并与
  `origin/main` 核对，是当前上游 main tip。期间依次包含 `85613d5e` Router 依赖、
  `fb0039b1` 流量分析路由、`635e96c9` meta-json-schema、`9fdd5135` Playwright、
  `d9cf2cc8` profile transform-chain 编辑、`b568b8b5` Prettier、`6083e156`
  worktree CoW 文档和 `e81eeaea` router-plugin 更新。
- 上游变化：除依赖升级与流量侧栏路由外，`d9cf2cc8` 重整 profile 转换链编辑器、
  同步历史展示和卡片能力，增加 transform-chain 列组件、Accordion 组件及浏览器
  覆盖。此次仅更新 `ref` 基线和本记录；未把上游依赖、profile UI 或其他源文件复制
  到 Chimera，也未改 Chimera 的 `package.json`、`pnpm-lock.yaml` 或前端实现。
- 验证：父仓库 `ref` gitlink 指向该提交；ref 工作区干净；递归子模块没有变化；
  `git diff --check` 通过。此批只改参考版本与本记录，不涉及产品代码，故未运行
  应用构建、桌面 E2E 或宿主网络测试。

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

## DIFF-042：按 domain 发布 post-commit effects 通知

- 基线：只读 `ref/` commit `cc21cbd31dc16c3e3b76c27867dd22aeae3b9cf1`，工作树
  clean；本轮未更新或修改 `ref/`。
- ref 到 Chimera 映射：`ref/backend/tauri/src/client/effects/ports.rs::CommitNotifications`
  的 `application_committed`、`clash_committed`、`profiles_committed`、`runtime_bound` 和
  `publish_full`，以及 `effects/actor.rs::Slice` 的单 domain merge，对应
  `backend/tauri/src/client/effects/{ports.rs,actor.rs}`。Profile TCC 提交点对应
  `application_workflow/{workflow.rs,tcc.rs}`；Chimera 的注入经过
  `state/mutation.rs::MutationCoordinator` 和 `client/mod.rs::try_new_with_args`。
- 本轮实现：通知接口现在按 application、Clash、Profiles 和已绑定 runtime ports 分片；effects actor
  将收到的 slice 合并进各自最新输入后再生成 diff。只有 source 决策为 `Committed` 才触发 TCC 的
  Profiles 通知。ProfilesClient 的序列化 source mutation 复用该 TCC owner，避免在 facade 收到
  `CommitReport` 时再次通知。主/legacy UI 的 Profiles 状态事件继续按原路径发送，新增通知只负责 shared
  effects actor 的托盘刷新。
- 保留差异：本轮只生产接入 Profiles slice。Application 和 Clash 仍由原有
  `run_verge_patch_side_effects` / `run_clash_patch_side_effects` 持有；runtime ports 通知也未接入，避免在
  其旧 sysopt/lifecycle owner 仍执行时重复应用系统代理、guard 或托盘副作用。因此本条是部分迁移，不代表
  所有 effect 已经改由 executor 执行。`reconcile(inputs)` 保留完整输入 slice，供当前兼容调用方使用。
- 影响入口：经 typed Profile TCC 完成的 add/import/delete/reorder/refresh/metadata/activate/transforms/file
  source commits 会请求一次 shared tray partial refresh；前端 Profile refresh 仍覆盖主 UI 与 legacy UI。
  agent 若经由同一 `ChimeraClient` Profile API 会复用该通知；没有增加 agent 专属业务实现。未改变 Profiles
  持久化格式或 runtime commit/rollback 语义。
- 测试契约：扩展 `docs/testing/contracts/application-effects-executor.md`。actor 单测验证 domain slice
  不覆盖 sibling state，Profile commit 只产生 Tray Part plan；TCC 单测验证 committed Profile source
  mutation 发出一次通知。fake effect port 不触碰 Tauri、系统代理或主机设置。
- 验证：`cargo fmt --manifest-path backend/Cargo.toml --package chimera -- --check`、
  `git diff --check`；`cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib
  client::effects:: -- --test-threads=1`（40 passed）；
  `cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib
  client::application_workflow::tcc::tests -- --test-threads=1`（11 passed）；
  `pnpm lint:clippy` 通过（仓库既有 warning）。Ubuntu CI 的 effects/hotkey 步骤新增执行 TCC 测试筛选。
- 验证环境差异：Cargo 首次因 `backend/chimera-runtime/chimera_service/Cargo.toml` 中重复的
  `[dev-dependencies]` table 无法解析 workspace。为运行 Rust 检查，保留了删除重复、内容相同的
  `tempfile = "3"` table；该目录是 submodule，变更只删除冗余声明，没有改变依赖集合。
- 后续：已运行 TCC committed-notification 测试和 `client::effects::` CI filter；将 application/clash
  producers 接到 slice 前，先迁移或移除对应的 legacy direct side effects；之后再接 runtime-bound/full
  lifecycle producer 和可观察 effect status。

## DIFF-043：共享依赖升级与 lockfile 对齐

- ref 基线：实际只读 checkout 为 `cc21cbd31dc16c3e3b76c27867dd22aeae3b9cf1`，分支 `main`，工作树 clean；本轮没有 fetch 或修改 `ref/`。本文件前文仍记有 `5331747c06a5f42eeabb3e225a1e77a83f480549`，因此本次明确按实际 checkout 的 `cc21...` 比较，不声称它是最新远程 HEAD。主仓库已快进同步到 `origin/master` 的 `d7f865ee`。
- ref 路径和符号：`package.json`、`frontend/nyanpasu/package.json`、`frontend/{interface,ui,utils}/package.json`、`backend/Cargo.toml`、`backend/tauri/Cargo.toml`、`backend/nyanpasu-config/Cargo.toml`、`backend/nyanpasu-runtime/crates/{nyanpasu-utils,clash-api}/Cargo.toml`。
- Chimera 路径和符号：`package.json`、`frontend/chimera/package.json`、`frontend/{interface,ui,utils}/package.json`、`tauri-e2e/package.json`、`backend/Cargo.toml`、`backend/tauri/Cargo.toml`、`backend/chimera-utils/Cargo.toml`、`backend/chimera-clash-api/Cargo.toml`、`backend/chimera-config/Cargo.toml`、`pnpm-workspace.yaml`、`pnpm-lock.yaml`、`backend/Cargo.lock`。
- 类别：共享依赖对齐；E2E 兼容适配。
- 更新：共同前端依赖和 package manager pin 对齐到 ref（pnpm 12.8.1），并重生成锁文件；Rust 更新包括 base64 0.22→0.23、Oxc 0.151→0.152、csscolorparser 0.8→0.9、serde_with 改为 ref 的 `3.20.0` 兼容范围、sha2 0.11→0.10、windows-core 0.100→0.62。按 ref 的分层方式，`backend/Cargo.toml` 的应用级 reqwest 使用 0.12，`chimera-utils` 和 `chimera-clash-api` 则各自使用 reqwest 0.13。Cargo.lock 的 wasm-bindgen/js-sys 七件套现在与 ref 一致。对 666 个双方均只有单一解析版本的同名 crate 做比较后，没有发现本地版本低于 ref。
- peer 收敛：`.prettierrc.cjs` 没有启用 `@prettier/plugin-oxc`，所以移除未使用的根依赖。`@wdio/tauri-service@1.4.0` 固定依赖 `@wdio/globals@9.29.1`，其 `expect-webdriverio` peer 与当前 WDIO 9.31 / expect-webdriverio 6 不兼容；在 `pnpm-workspace.yaml` 以窄范围 override 将该版本映射到 9.31.3。ref 没有 E2E harness，因此此项是 Chimera E2E 扩展的临时兼容适配；待 tauri-service 更新其固定 WDIO 依赖后移除。
- 保留差异：锁文件中 `float-cmp` 为本地 0.10.0、ref 根锁为 0.9.0；0.10.0 来自运行时子工作区的 `value-trait`，与 `ref/backend/nyanpasu-runtime/Cargo.lock` 一致，ref 根锁中的 0.9.0 则来自另一条依赖路径。deep-link 包仍是本地官方插件 2.4.9 与 ref 仓库内插件 0.1.2 的差异；ref 清单本身标注该自定义插件应迁移到官方插件，因此保留 Chimera 已采用的官方实现，待 ref 完成迁移后再收敛版本和 API 差异。E2E peer override 仍按上文条件移除。
- 影响入口：主界面、legacy UI 共用同一组更新后的 frontend packages；E2E 继续使用现有 harness 和 `@wdio/tauri-service`。Rust 生产源代码和持久化合同没有改动。桌面 TUN 没有在本轮重启或改写。
- 验证：独立依赖 worktree 中 `pnpm install --frozen-lockfile --ignore-scripts`、`pnpm peers check`（无 peer 问题）、`pnpm --filter chimera-ui build`、`pnpm typecheck`、`pnpm --filter @chimera/tauri-e2e test:unit`（17 passed）；Cargo `metadata --locked`、`cargo check --locked --manifest-path backend/Cargo.toml -p chimera` 通过；`cargo test --locked --manifest-path backend/Cargo.toml -p chimera-utils --features process,reqwest -- --test-threads=1` 全部通过（66 passed），`cargo test --locked --manifest-path backend/Cargo.toml -p chimera-config -- --test-threads=1`（139 passed）通过。`cargo tree` 确认应用解析 reqwest 0.12.28、Clash API/utilities 解析 0.13.5。临时 worktree 缺少被忽略的 sidecar/resources 文件，因此 Tauri check 只读链接了当前工作树中的资源目录，没有改写资源。最初仅启用 `reqwest` 的测试调用漏掉 `process` feature，3 个 process-helper smoke tests 因 helper 不存在失败；带上 `process,reqwest` 后按完整 feature set 重跑通过。`git diff --check` 和 pnpm frozen lockfile check 通过。
- 未完成的验证：本机只安装了 `aarch64-apple-darwin` Rust target；windows-core 0.62 已由 Cargo 元数据解析，并确认其 `Error::from_thread` API 存在，但 Windows 目标编译仍需 CI 验证。没有运行 macOS 桌面 bundle、Tauri 桌面 E2E、Linux 构建或 TUN 实机验证。临时 worktree 的 `pnpm prepare:check` 曾因该 worktree 未初始化 `backend/chimera-runtime` 子模块而失败；本轮没有重跑这个会检查/更新本地 sidecar 资源的脚本。
- 收敛状态：所检查的共享依赖版本差异已按 ref 约束收敛；剩余差异均已说明来源或上游 TODO。依赖更新本身完成，Windows 和桌面运行验证仍待相应平台环境覆盖。

## DIFF-044：Connections 页面搜索状态与最新 ref 同步

- 基线：按用户要求更新本地只读 `ref/`。更新前 `cc21cbd31dc16c3e3b76c27867dd22aeae3b9cf1`，更新后
  `def0dbf2716ff972255be47924401b60db7d482c`（`main`，与 `origin/main` 一致）；参考工作树干净。此次 fetch
  更新了主仓库远程引用；上游 runtime 子模块已在该 ref 提交记录的位置，未修改其内容。
- ref 到 Chimera 映射：`ref/frontend/nyanpasu/src/pages/(main)/main/_modules/use-search-term.ts::useSearchTerm`
  对应 `frontend/chimera/src/pages/(main)/main/_modules/use-search-term.ts::useSearchTerm`；ref
  connections `index.tsx` 的 URL 查询参数 `q` 对应 Chimera 同名页面和
  `frontend/chimera/src/pages/(main)/main/connections/route.tsx::validateSearch`。
- 本轮实现：输入停止 300ms 后将搜索词写入当前 route entry 的 `q`，使用 replace 保持输入不制造历史记录；
  返回到页面条目时恢复该搜索词。表格过滤使用 deferred value，并延后挂载重表格；代理侧栏更新筛选时保留现有
  route search 字段，避免清掉搜索词。搜索页离场后不再写入 route 状态。
- 保留差异及状态：**部分迁移**。当前 Chimera 仍是只有 active scope 的旧表格和独立 proxy sidebar；最新 ref
  的 `active/all/closed` viewers、traffic range/dimension filters、分页关闭连接、filter-chip overflow、跨页
  focus navigation 都未迁入。尤其 `all/closed` 和 traffic filters 依赖 ref 的
  `nyanpasu-traffic` 持久化 store、traffic actor、RPC/IPC bindings 和 live-connection frame feed；Chimera
  尚无对应业务服务/查询 API。本轮没有用前端缓存伪装历史数据，也没有改变 legacy UI。
- 影响入口：仅主界面 connections route/search 和 proxy sidebar；legacy `/connections` 与 agent 入口未改，仍通过
  现有共享 `useClashConnections` 数据入口工作。
- 验证：`pnpm typecheck`、`pnpm --filter=chimera-ui build`、Prettier 和 `git diff --check` 通过。未运行真实桌面
  E2E；当前 Chimera 前端没有 connections 浏览器测试 suite，构建和类型检查不证明 route 返回/重载行为。
- 收敛条件：先迁移 ref `backend/nyanpasu-traffic` 的领域模型、持久化、traffic actor 与连接帧装配，映射到
  Chimera `ChimeraClient`/Tauri IPC 并用现有 Specta generator 生成 binding；之后迁入 ref connections route 的
  active/all/closed viewer、filters、toolbar/status tabs 和跨页焦点，并分别验证主 UI、受影响的 legacy 共享入口及
  agent 对同一 API 的兼容性。完成这些步骤前不得称 Connections 页面已与最新 ref 完成对齐。

## DIFF-045：等待新进程完成 exec 后再记录身份

- 基线：只读本地 `ref/` commit `0c7b15b3c61db390db21cffb63d29805abd380a1`，工作树 clean；本轮未更新或修改 `ref/`。
- ref 到 Chimera 映射：`ref/backend/nyanpasu-runtime/crates/nyanpasu-utils/src/process/pid_file.rs::PidFileGuard::write` 和
  `wait_for_process_identity` 对应 `backend/chimera-utils/src/process/pid_file.rs` 同名逻辑。
- 类别：ref 缺陷的最小修正。ref 与 Chimera 原逻辑都会在第一次读到进程身份时立即返回；Linux 在带有 `pre_exec`
  hook 的 fork/exec 启动窗口中，`/proc/<pid>/exe` 短暂指向父测试进程。CI 的
  `different_epoch_pid_files_do_not_kill_overlapping_process` 已复现该竞态：刚启动的
  `chimera-test-child` 被识别为 `process_pid_file` 并拒绝写身份记录。Chimera 现在在有限等待期间继续观察，直到
  executable 与启动命令一致；持续不匹配仍 fail closed。
- 其他 CI 修正：`core_spec_preserves_each_supported_brand_and_executable_mapping` 将平台可执行后缀纳入断言，并在
  Windows 上为 Alpha 测试夹具提供 `.exe` 文件名；
  Specta 绑定新鲜度检查在比较前统一 CRLF/LF，保留内容级差异检查。
- 影响入口：进程监督写入 epoch pid 记录及 Windows/macOS 二进制路径映射测试；不改变 pid 记录格式、终止权限或
  产品 IPC。没有 legacy UI 或 agent 独立路径。
- 验证：epoch pid 集成测试 12/12、启动身份等待竞态回归测试 1/1、核心二进制映射测试 1/1、Specta 绑定新鲜度测试
  1/1 通过；绑定生成器更新测试运行后没有产生文件差异。`pnpm run-p lint:clippy lint:rustfmt` 与
  `git diff --check` 通过。全量 `pnpm test:backend` 在本机 macOS 为 551 passed、1 ignored、1 failed；唯一失败是
  不相关的 `external_watcher_smoke_mirror_real_file_event` 超时，该用例单独重跑 1/1 通过。远程本次失败项只在
  Linux 进程启动竞态和 Windows 两个跨平台断言；macOS CI 与 Tauri E2E 均通过。
- 重新评估：当 ref 的进程身份采集等待逻辑或 ProcessKit 的 spawn/exec 行为更新时，重跑身份竞态测试，并评估移除
  本地等待差异；平台后缀断言和绑定换行规范化保留为跨平台测试要求。

## DIFF-046：在配置提交时锚定远程 profile 首次刷新时间

- 基线：只读本地 `ref/` commit `0c7b15b3c61db390db21cffb63d29805abd380a1`，工作树 clean；本轮未更新或修改 `ref/`。
- ref 到 Chimera 映射：ref `backend/tauri/src/state/profiles/jobs.rs::ProfileJobs::reconcile` 注册持久化 interval job；Chimera
  `backend/tauri/src/state/profiles/scheduler.rs::RemoteUpdateScheduler::reconcile` 维护每 profile 的 Tokio timer。
  两者都从 committed profile 的 `update_interval_minutes` 派生后台刷新，但调度器实现不同。
- 问题与修正：Linux CI 暴露了导入 profile 后立即推进 Tokio 虚拟时间的竞态：timer task 可能在时间推进后才首次被轮询，
  从而把完整 interval 错误地从推进后的时刻开始计算。现在在 reconcile 同步计算首次刷新 deadline，再由 task 等待该 deadline；
  计时从 profile 提交/调度时刻开始，不依赖 executor 何时首次运行 task。后续周期仍按原 interval 循环。
- 影响入口：通过同一个 `ProfilesClient` 的主界面、legacy UI 与 agent profile 导入/更新入口；未改变持久化格式或 IPC。
- 验证：远程失败回归用例 `client::profiles::tests::import_suggestion_is_committed_and_reschedules_the_timer` 本机通过（1/1）；
  `cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib client::profiles::tests -- --test-threads=1`
  通过（66 passed）。Cargo fmt、Prettier 和 `git diff --check` 通过。推送后的 CI 将重新验证 Linux/Windows 后端测试矩阵；
  Tauri E2E 不受此改动影响。
- 收敛条件：当 Chimera 迁移到与 ref 相同的持久化 job scheduler 时，重看首次 tick 的注册时刻与 catch-up 语义；保留
  “deadline 不依赖 task 首次 poll 时刻”的确定性要求。

## DIFF-048：允许 WebView 同源与 data URL 请求

- ref 基线：PR #478 rebase 到 master 后使用 gitlink 固定的 `31019da14006b384b3272c72b4088179a9e72c47`。已直接读取该 commit 的 `backend/tauri/tauri.conf.json::app.security.csp`，其 `connect-src` 包含 `'self' data:`；没有移动或更新 `ref/`。
- ref 到 Chimera 映射：`ref/backend/tauri/tauri.conf.json::app.security.csp` 对应 `backend/tauri/tauri.conf.json::app.security.csp`。只在 `connect-src` 加入 `'self' data:`，其它 CSP 来源不变。
- 类别与风险：共享 WebView 安全策略对齐，L2；smoke E2E 是 Chimera 扩展。
- 本轮实现：同一 Tauri CSP 覆盖 legacy 与 main WebView。Smoke 从两个窗口分别请求当前文档同源 URL，并以 `no-cors` 请求内存中的 `data:` URL；同源响应必须成功，`data:` 响应必须为 opaque 且状态 0。缺少来源时请求被 CSP 阻止，断言会失败。
- 影响入口：Tauri 共用 CSP 覆盖 legacy 和 main 两个窗口；Agent 直接使用共享 application API，不经 WebView 请求，因此不受该策略影响。没有改动 UI、IPC、配置持久化、运行时核心或 Service，也不验证公网代理。
- 测试契约：`tauri-e2e/specs/smoke.e2e.ts` 的 `allows same-origin and data URL fetches in both desktop windows`，位于现有 `smoke` 基础套件并随 PR 的 `hermetic` suite 执行。每个窗口先确认目标路由/窗口，再进行有界 fetch；检查真实 WebView 观察到的响应，不写入页面或应用状态。无外网和用户数据。
- 验证：已完成 E2E harness typecheck、harness unit tests 25/25、Oxlint、Prettier、Tauri JSON/CSP 解析及 `git diff --check`。本机 macOS 桌面 E2E 未运行；本地完整打包需要较大 Cargo target 且磁盘空间不足。合入 master 基线后，Windows Desktop `hermetic` 会执行真实 legacy/main WebView 用例；其结果仍待新 SHA 验证。
- 保留差异：legacy UI 保持现有窗口和用户流程；`data:` 响应是 opaque，测试不读取正文。没有其它收敛工作。
- 回滚：还原 `connect-src` 新增的 `'self' data:` 和本 smoke 用例即可；不迁移用户数据或更改宿主网络设置。

## DIFF-049：滚动 Chimera Client Alpha 资产失败后刷新

- ref 基线：本地 `ref/` commit `813673a8e84b51265467f02277fc7e739837f61e`，与远端 `main` 一致，工作树干净。该提交相对原分析基线 `67999753918c7b9879ddd2bacdef74246bfef1dd` 未改变本条映射的 `scripts/src/prepare` 文件。
- ref 到 Chimera 映射：ref `scripts/src/prepare/types.ts::BinInfo.refresh`、
  `scripts/src/prepare/binaries.ts::getClashMetaAlphaInfo` / `getMeowAlphaInfo` 和
  `scripts/src/prepare/resources.ts::resolveSidecar`；Chimera 对应新增的
  `scripts/src/prepare/{types,binaries,resources}.ts` 中 `BinInfo.refresh`、
  `getChimeraClientAlphaInfo` 与 `resolveSidecar`。
- 实际入口：`package.json::prepare:check` 调用 `scripts/check.ts`；Alpha task
  仍通过 `materializeSidecar` 和 `verifyAndInstallSidecar` 安装。当前 Alpha
  `BinInfo` 没有提供 `checksumURL`，因此这条路径不做 SHA-256 比较；它继续使用
  现有的文件暂存、可执行权限设置、原子替换与版本戳更新。
  为保持 Chimera 当前 Deno 准备器与安装边界，本批把 ref 的刷新重试抽到对齐
  目录；实际暂存、可选校验和原子替换仍由 `scripts/check.ts` 承担。未修改没有
  被 `prepare:check` 调用的旧 `scripts/utils/resource.ts`。
- 问题与修正：Chimera Client Alpha 使用滚动 `latest` release；macOS 资产文件名
  带稳定版本号。当 manifest 更新落后于 release 且旧资产已不可用时，首次下载
  失败会触发 `resolveChimeraClientAlpha`（与 manifest 生成共用同一 resolver）
  重新解析版本及架构映射，再按 ref 行为重试一次。刷新元数据失败时保留首次
  下载错误；重试安装失败则报告重试错误，外层原有 task retry 不变。当前 release
  资产列表确有版本化 macOS tarball；证据：[latest release assets](https://github.com/MFSGA/Chimera_Client/releases/expanded_assets/latest)。
- 保留差异与边界：只给 Chimera Client Alpha 增加刷新，未支持的 Meow 不迁入；
  Clash-rs Alpha 使用无版本号文件名，因此无需此映射。与 ref 相同，只有首次
  materialization 失败才刷新；命中版本戳有效的缓存时仍遵从现有缓存语义，直到
  manifest 更新使版本戳失效。
- 影响入口：打包/开发前的共享 sidecar 准备；不改变 main UI、legacy UI、Agent
  的 API，配置持久化、Core/Service 协议和宿主网络均不受影响。下载及安装仍复用
  现有临时目录、版本戳和原子替换逻辑；Alpha 资产当前没有 SHA-256 校验。
- 验证：`pnpm test:manifest` 为 5 passed，其中新增测试模拟旧滚动资产失败，
  验证同一调用链刷新为新 Alpha 版本并重试；`deno check --frozen --config
  scripts/deno.jsonc scripts/check.ts`、`pnpm typecheck`、`pnpm lint:oxlint`、
  Prettier 和 `git diff --check` 通过。未在本机运行 `pnpm prepare:check`，因为它会
  下载并替换工作区 sidecar；本批完整准备/打包路径由变更后的远程 CI 执行。没有
  本机 macOS Tauri 桌面 E2E 或真实 Alpha 核心运行验证。
- 收敛条件：本地准备器整体迁移到 ref 的 `scripts/src/prepare/check.ts` 后，移除
  `scripts/check.ts` 的委托桥接；若要在 manifest 日更窗口内自动淘汰已缓存的
  旧 Alpha，还需单独评估每次准备时在线解析版本的成本和离线回退策略。

## DIFF-051：修正 epoch PID 文件检查与读取之间的删除竞态

- 问题与证据：PR #479 的 macOS CI job `115762372590` 在
  `process_pid_file::residual_process_is_killed_before_spawn` 第二次 `spawn().unwrap()` 处报告 OS
  `NotFound`，日志没有指出产生错误的具体系统调用。`read_epoch_pid_file` 先 `symlink_metadata`、后
  `read_to_string`，而上一轮进程清理可并发移除同一记录；检查后读取前的删除竞态是与日志现象一致的候选根因。
- ref 映射与保留差异：ref `backend/nyanpasu-runtime/crates/nyanpasu-utils/src/process/pid_file.rs::read_epoch_pid_file`
  （runtime SHA `e9f44e68b0c88981160f5b986f7ffcc9fb5b20a7`）也有相同顺序。Chimera 在对应的
  `backend/chimera-utils/src/process/pid_file.rs::read_epoch_pid_file` 仅将读取阶段的 `NotFound` 处理为
  `Ok(None)`；符号链接/普通文件检查仍保留，其他 I/O 错误仍向上传播。该最小修正不改变 PID 身份校验、终止授权或记录格式，
  不复制 ref 中已观察到的竞态缺陷。CI 没有 syscall 级证据确认此次失败源自该读取窗口，因此该根因仍待新 SHA 远端复验。
- 验证：旧 PR SHA 的 macOS CI 为 11 passed、1 failed；修正后本地 macOS `process_pid_file` 集成目标
  12/12 通过，`rustfmt --check backend/chimera-utils/src/process/pid_file.rs` 通过。新 SHA 的 macOS CI 与
  Desktop E2E 尚待远端复验；新 SHA 用于验证修复效果，旧失败的具体 syscall 根因仍是与现有证据一致的候选判断。
- 关联记录：[LESSON-20261014-001](../LESSONS.md#lesson-20261014-001-epoch-pid-记录可能在检查后读取前被并发清理)。
- 回滚：还原读取错误分支即可；无用户数据迁移或宿主状态变更。

## DIFF-052：滚动 Chimera Client Alpha 发布期间有限重试

- ref 基线：本地 `ref/` commit `813673a8e84b51265467f02277fc7e739837f61e`，工作树干净；对应的
  `scripts/src/prepare/resources.ts::resolveSidecar` 在刷新失败时只尝试一次，本批保留其初次下载、刷新、再安装的调用顺序。
- Chimera 映射：`scripts/check.ts::tasks` 的 `chimera-client-alpha` task →
  `getChimeraClientAlphaInfo` → `scripts/src/prepare/resources.ts::resolveSidecar` →
  `materializeSidecar`。仅 Alpha task 提供 `SidecarRefreshRetryPolicy`；其他 sidecar 沿用原路径。
- 证据与原因：PR #479 macOS job
  [115791449079](https://github.com/MFSGA/Chimera/actions/runs/38544336079/job/115791449079)
  在资源准备阶段因已失效的 `0.26.3` Alpha 资产 URL 返回 404 而失败；task 在无等待的五次总尝试（含初次）内没有恢复机会。只读检查
  `MFSGA/Chimera_Client/.github/workflows/ci.yml` 发现 master nightly 发布会先删除滚动 `latest` release/tag，再上传
  新资产并发布替代 release；因此当前元数据或资产暂时 404 是已证实存在的发布窗口。CI 日志证明旧资产不可用，发布工作流
  证明窗口存在；日志没有 syscall 式证据证明此次 404 与窗口精确重叠，二者关系按候选根因记录。
- 修正：当 Alpha 初次下载后刷新元数据或新资产仍返回 404 时，按 1、2、4、8 秒间隔重新解析滚动 release 并尝试安装；
  每次 sidecar resolve 最多额外四轮，累计等待上限 15 秒。外层原有 task retry 仍为五次，因此持续 404 时整体准备过程最多可累计
  75 秒的新增等待。非 404 错误不进入该重试；缓存、文件校验/安装边界及其他 core 的 task retry 不变。
- 保留差异：ref 对 rolling asset 只刷新一次；此处的有限 404 重试是针对 Chimera Client `latest` 发布流程的本地恢复适配。
  如果上游发布流程改为原子替换 release/tag，或 Alpha 资产改为不可变版本 URL，应重新评估并移除此差异。
- 验证：用例模拟旧 Alpha 资产 404、latest tag 查询暂时 404、恢复后新资产再短暂 404；另有用例验证 404 重试预算耗尽时抛出
  最后一次错误，以及 503 不进入延迟重试。`pnpm test:manifest` 为 8 passed；`deno check --frozen --config
  scripts/deno.jsonc scripts/check.ts`、`pnpm lint:oxlint`、Prettier 和 `git diff --check` 通过。`pnpm prepare:check`
  在本机实际完成，旧 manifest 中的 `0.26.3` Alpha URL 刷新到 `0.27.0-alpha+sha.47d6954` 并安装成功；其他 sidecar 均命中缓存。
  全工作区 `pnpm typecheck` 未通过，因为该隔离工作树缺少构建生成的 `frontend/chimera/src/paraglide` 模块，错误与本次脚本 diff 无关。
  该 SHA 推送后仍需重新验证 PR required checks 与 Desktop E2E；尚未运行实际 macOS Tauri 打包。
- 风险与回滚：L2（sidecar 依赖解析/打包）。回滚可移除 retry policy 与 `resolveSidecar` 的可选重试参数；没有用户数据、
  宿主网络或持久配置变更。

## DIFF-053：profile 变更后使代理快照缓存失效

- 基线：本地 `ref/` commit `813673a8e84b51265467f02277fc7e739837f61e`，工作树干净；本批未修改 ref。
- 映射：ref `frontend/query/src/ipc/use-profile.ts::useProfileMutations` 的成功回调只失效 profile query；
  `frontend/query/src/provider/mutation-provider.tsx` 的 `profiles` key 列表也不包含 `getProxies`。ref 由
  `backend/tauri/src/client/clash_api.rs::NyanpasuClient::{get_proxies,refresh_proxies}` 读取 actor-owned
  `ProxiesClient` 快照，并以 API 实例身份和撤销状态维护其生命周期；同一实例的普通缓存为 3 秒，周期性强制刷新间隔为 10 秒。
  Chimera 的本地扩展
  `frontend/interface/src/provider/mutation-provider.tsx` 会在 profile mutation 后使 `CLASH_PROXIES_QUERY_KEY` 失效；
  后端则通过 `backend/tauri/src/ipc.rs::get_proxies` 读取共享的 `core/clash/proxies.rs::ProxiesGuard`。
- 问题证据：PR #479 旧 SHA `2188073f4ee0f749777e3a8c21e1c7aef54520c3` 的 Windows Desktop E2E 中，
  `rules-main-proxy-icon.e2e.ts` 52 项有 1 项失败。页面已导航到新 profile，直接 runtime 查询也看到带 SVG 的新代理组，
  但 30 秒后 UI 仍显示无图标 fallback。失败截图与完整用例见
  [Windows Desktop E2E run](https://github.com/MFSGA/Chimera/actions/runs/38544336107)。
- 原因与修正：Chimera profile mutation 会使前端代理 query 失效，但后端 `get_proxies` 在 3 秒缓存 TTL 内仍可返回旧快照；
  该 IPC 刷新路径不广播 proxies mutation，因而旧结果可能保持在 UI query 中。`ChimeraClient::refresh_profiles`
  现在先递增 `ProxiesGuard` generation、清除 checksum 并将 `updated_at` 置为过期，再发 profile mutation。缓存更新在提交前
  对 generation 做核对：如果旧快照请求跨过了 profile 失效点，它会被丢弃；`get_proxies` 会重新取得 API 并重试。
  后台刷新在解析 API 前捕获 generation；手动代理选择在 API 获取后、发出选择请求前重新校验 generation，并把同一 generation
  传给后续缓存刷新。profile 在 API 获取期间发生变化时，选择会被拒绝。
  如果 profile 在已发出的 HTTP 选择请求期间变化，请求本身仍可能作用于正在退出的 runtime（未实现 ref actor 的实例级撤销），
  但后续快照刷新发现 generation 已失效时会返回错误；IPC 不报告成功，也不会继续发出代理变更事件或连接中断处理。
  手动 profile mutation 与“无当前 profile 时自动激活”的完成路径都在发送事件前共用此函数。
- 保留差异与接口：没有改变 IPC、前端 query key 或持久配置；`ProxiesGuard` 是 Chimera 迁移期缓存桥接。ref 的代理快照由
  `ProxiesClient` actor 管理并绑定到可撤销的 API 实例；Chimera 当前仍使用全局 TTL guard，加 generation 栅栏保证失效后的
  旧请求不能复活缓存。后续迁移到 ref 的代理客户端/刷新契约时，应移除该全局 TTL 缓存并由共享 client 管理快照和通知。
  变更影响 profile 与代理刷新共享路径，属 L2。
- 验证：旧 SHA 的 Windows 用例 1/52 失败；旧代码本机 macOS 定向 E2E 1/1 通过。初始 generation 修复的 Rust 定向测试为
  3 passed。补上选择请求期间 profile 切换的错误返回后，第一次重跑因本机磁盘空间耗尽在链接前失败；清理当前工作树生成的 Cargo
  构建缓存后，以 `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --locked --manifest-path
  backend/Cargo.toml -p chimera --lib core::clash::proxies::tests:: -- --nocapture` 重跑，最终代码 4 passed、0 failed。
  `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo clippy --locked --manifest-path backend/Cargo.toml -p chimera --all-targets
  --all-features`、`cargo fmt --manifest-path backend/Cargo.toml -p chimera -- --check`、`pnpm test:manifest`（8 passed）和
  Deno check 通过；`pnpm exec prettier --check LESSONS.md docs/ref-alignment-progress.md` 与 `git diff --check` 通过。
  用最新 e2e 构建运行本机 macOS WebKit 定向用例：第一次在
  profile 创建前因主页面路由动画未稳定而失败；随后两个独立运行都通过 1/1，最后一次使用包含 generation 与选择预检的最终
  e2e 构建。重试通过不等于首次运行通过，也不能替代 Windows 回归复验。完整工作区 `pnpm typecheck` 仍因隔离工作树缺少
  生成的 Paraglide 文件而失败。新 SHA 的 Windows Desktop E2E
  及 PR 必需检查仍须远端复验；当前未声称 CI 已通过。
- 回滚：移除 `ProxiesGuard::invalidate` 和 `ChimeraClient::refresh_profiles` 包装，恢复各 profile mutation 直接发事件；
  无迁移用户数据、宿主网络或系统服务状态。

## DIFF-054：主界面流量分析与拓扑视图

- 目标与非目标：把 ref 的主界面流量总览、排名、flow 和 geography 视图接入 Chimera 主 UI，并让导航保持 URL 筛选状态；本批不迁移本地 IP 探测/外部地理定位，不改变流量采集 runtime、legacy UI 表现或 agent API。
- 参考版本：流量分析初版取自本地 `ref/` commit `fb0039b11a12c0f575fb92c1481f4ebaf66eb1d5`；本 PR 当前固定的 `ref/` 基线为 `e81eeaea46a9c229478cd29c9555b5c072f7e6cc`。检查 `fb0039b1..e81eeaea` 的 ref 变更后，确认本批映射的 topology、data-page、traffic utility 和 navigation-sidebar 文件未被后续提交触及。本功能提交不修改 `ref/`；同步 gitlink 是同 PR 的独立提交。
- ref 路径和符号：`ref/frontend/nyanpasu/src/pages/(main)/main/topology/` 的 overview、`traffic-context`、排名、flow、map、toolbar、transition、搜索及布局模块；`ref/frontend/nyanpasu/src/pages/(main)/main/_modules/{data-page.tsx,use-usage-label-of.ts}`、`ref/frontend/nyanpasu/src/utils/{traffic-usage,traffic-retention}.ts`，以及 `ref/frontend/ui/src/navigation-sidebar.tsx`。保留 ref 的路由、查询参数、页面模块和组件命名；新增 `Button.disableRipple` 以承载 ref 导航侧栏的 ripple 所有权。
- Chimera 映射：对应文件位于 `frontend/chimera/src/pages/(main)/main/topology/`、`frontend/chimera/src/pages/(main)/main/_modules/{data-page.tsx,use-usage-label-of.ts}`、`frontend/chimera/src/utils/` 和 `frontend/chimera/src/components/ui/navigation-sidebar.tsx`；数据仍通过共享 `frontend/interface/src/ipc/use-traffic.ts::{useTrafficReport,useTrafficUsage}` 读取。排名弹窗沿用 ref 的 50 条分页和关闭自动重试策略。增加主界面桌面导航、移动端 More 菜单入口；Connections 的 `onViewRuleUsage` 跳转到带 rule filter 的 topology route。路由生成产物为 `frontend/chimera/src/routeTree.gen.ts`。
- 类别及接口：L1、只读主界面能力。`useTrafficReport` 新增可选 `refetchInterval` 查询选项，默认查询行为不变；`useTrafficUsage` 的默认分页大小与 retry 策略对齐 ref（50 条、`retry: false`）。没有增加 IPC、持久化、runtime 或 agent tool 合同。legacy UI 继续使用共享 traffic hook，页面呈现未改变；agent 不受影响。
- 隐私和保留差异：地图仅使用 backend report 已提供的 region 字段和打包的 Natural Earth 数据；没有端到端 IP lookup、public geo 服务、地图瓦片或宿主网络探测。ref 的 local-IP probe、opt-in/geolocation index 和 `mock-traffic.tsx` 未迁入，避免制造未经用户授权的外网 IP 查询，并因 Chimera 没有 ref debug mock hooks 而跳过 mock。没有查询到 region 的本地来源会保留为未知。地图 README 记录数据来源及许可证；后续若要迁移 IP 定位，须先设计显式 opt-in、TUN 排除和隐私测试。
- 测试契约：新增 `docs/testing/contracts/traffic-analysis-main.md`；注册 `tauri-e2e/specs/traffic-analysis-main.e2e.ts` 到 `main` suite。它通过桌面控件验证 route 和 scope/metric query 保留，不把空报告/IPC 错误恢复或地图地理数据渲染当作覆盖目标。新增可执行的 `test:traffic` 单元入口覆盖 URL 到 report/topology 查询映射。
- 验收条件：四个视图可从主 UI 打开；范围和指标选项更新 URL，并在 Flow/Geography 切换时保留；Connections 规则用量代码将 rule filter 带入分析页；打包地图不产生外部定位请求；相关前端类型检查、构建、模型测试和已注册的桌面用例通过。新 E2E 不覆盖 Connections 详情交叉跳转，因为当前 main 路由用例没有安全、可控的活动连接 fixture；其代码映射由 review 检查，真实桌面行为尚未验收。桌面 E2E 当前因本机磁盘限制未完成，因此仍待新 SHA 的远端 E2E 结果。
- 验证：`pnpm typecheck` 通过；`pnpm --filter=chimera-ui build` 通过；`pnpm --filter=chimera-ui test:traffic`，5 passed；`pnpm --filter @chimera/tauri-e2e test:unit`，25 passed；`pnpm lint:frontend-boundaries`、对全部改动前端/契约文件的 Prettier 检查和 `git diff --check` 通过。`pnpm e2e:tauri:build` **未通过**：本机磁盘仅有约 3.2 GB 可用，Rust 在生成 `backend/target/e2e/debug/deps/libchimera_lib.a` 时报告 `No space left on device`；仅删除本次新建的 `backend/target/e2e` 构建输出恢复空间。真实桌面 `traffic-analysis-main.e2e.ts` 因无可执行文件而未运行，不能据此声称桌面验证通过；远端 PR Desktop E2E 仍需按新 SHA 验证。
- 风险与回滚：本批不改变宿主网络、服务权限或用户数据；完整回滚需移除 `frontend/chimera/src/pages/(main)/main/topology/`、`frontend/chimera/src/pages/(main)/main/_modules/{data-page.tsx,use-usage-label-of.ts}`、`frontend/chimera/src/utils/{traffic-retention,traffic-usage}.ts`、`frontend/chimera/src/components/ui/navigation-sidebar.tsx`、`frontend/chimera/src/assets/maps/`，并恢复 `frontend/chimera/src/components/ui/button.tsx`、`frontend/interface/src/ipc/use-traffic.ts`、`frontend/chimera/src/pages/(main)/_modules/navbar.tsx`、`frontend/chimera/src/pages/(main)/main/connections/index.tsx`、`frontend/chimera/messages/{en,ru,zh-cn,zh-tw}.json`、`frontend/chimera/package.json` 和 `frontend/chimera/src/routeTree.gen.ts`。同时移除 `frontend/chimera/tests/traffic-search.test.ts` 及其 package script、`tauri-e2e/specs/traffic-analysis-main.e2e.ts` 与 `tauri-e2e/spec-suites.ts` 注册、`docs/testing/contracts/traffic-analysis-main.md` 和本 DIFF 记录。地图 generator 未迁入；若未来要让地图数据更新自动对齐 ref，需单独补齐可复现生成器。
- 收敛条件：在补入并验证本机 Tauri E2E 之前，标记为“前端构建和单测通过、桌面 E2E 未验证”。只有在明确批准 IP 定位的隐私/网络合同并通过专用隔离测试后，才评估迁移 ref local-IP probe；legacy UI 和 agent 继续复用同一共享数据 API，不复制业务逻辑。

## DIFF-055：远程 Profile 可关闭自动同步

- 目标：按 ref 增加远程 Profile 的自动同步开关；间隔设为 0 时，周期刷新与启动补刷停止，手动同步仍可用。
- 非目标：不改变手动同步语义，不改 Core/Service 协议、代理/TUN 宿主设置、Agent API 或 legacy UI 的路由和导航；legacy 编辑器保留数字间隔输入，并只增加说明 0 会关闭自动同步的提示。
- ref 基线更新：该功能分支原本基于 master 的 `e81eeaea46a9c229478cd29c9555b5c072f7e6cc`；本工作树当时 checkout 在
  `c85406d3a16ba61cecf7f8aec7a5a051523cbc2d`，这是上游 PR #5775 提交，不是 main。现更新到主线
  `c08aa9d9a17953aa2464cdae61390d5c3f9afbc7`。GitHub 直接 fetch 超时；目标
  commit 已在本地对象库，且上游 Actions 页显示 `c08aa9d9` 的 main 构建。更新后 `git -C ref status --short` 为空。
- 映射：ref `backend/nyanpasu-config/src/profile/source.rs::RemoteSyncInterval`、
  `backend/tauri/src/state/profiles/actor.rs` 的自动刷新处理和
  `frontend/nyanpasu/src/pages/(main)/main/profiles/$type/detail/_modules/update-option-editor.tsx`；Chimera 对应
  `backend/chimera-config/src/profile/source.rs::RemoteSyncInterval`、
  `backend/tauri/src/state/profiles/scheduler.rs::RemoteUpdateScheduler`、
  `backend/tauri/src/client/profiles.rs`、`frontend/chimera/.../update-option-editor.tsx`，生成类型位于
  `frontend/interface/src/ipc/bindings.ts`。legacy 入口通过共享 `ProfilesClient` 保存，并由
  `frontend/chimera/src/components/profiles/profile-dialog.tsx` 适配数值间隔。
- 完整性：模型、配置校验、旧配置适配、迁移、actor 调度与 IPC patch 都接受 0；调度器取消周期任务且跳过启动补刷；保留
  手动刷新；main UI 按 ref 使用 `react-hook-form`/`zodResolver` 开关编辑，重新开启且间隔留空时保存默认 120；legacy 表单仍
  显示数字间隔并允许 0；修复 legacy 对只读远程物化文件的编辑路径，避免更新选项时错误 upsert 文件内容。
- 持久化与接口：远程 profile 的 `update_interval_minutes` 现在允许 0，IPC 序列化仍是数字，`bindings.ts` 由既有生成器更新；
  不改变 YAML 键或其他用户数据。增加 `@hookform/resolvers` 以保持 main editor 的 ref 表单结构。
- 风险：L2（持久配置、调度器与共享 Profile API）。任何 scope 重叠仍按 L2 处理。
- 验收：Rust 定向测试、main/legacy/E2E TypeScript 检查、manifest tests、前端 IPC 边界检查、真实桌面流程以及 loopback 手动
  请求均已执行。本机 WebKit 后台窗口会暂停 Motion 退场动画，视觉折叠与冷启动恢复不在本次实测覆盖内；远端 CI/E2E 尚未启动，
  不宣称通过。
- 实际验证：`cargo test --locked --manifest-path backend/Cargo.toml -p chimera-config profile`；`chimera` 的
  `scheduler_stops_refreshing_when_interval_is_patched_to_zero`、
  `disabled_auto_sync_skips_startup_catch_up_but_allows_manual_refresh`、
  `preserves_disabled_remote_sync_interval_in_runtime_adapter` 定向测试均通过。`pnpm typecheck:chimera`、
  `pnpm --filter @chimera/tauri-e2e typecheck`、`pnpm --filter @chimera/tauri-e2e test:unit`（25/25）、
  `pnpm test:manifest`（8/8）、`pnpm lint:frontend-boundaries`、`pnpm lint:oxlint` 与 Prettier 检查通过。
  WebKit 桌面 spec `profile-auto-update.e2e.ts` 1/1 通过，使用隔离运行目录和 loopback HTTP fixture；未覆盖真实公网、系统代理/TUN、
  前台动画与冷启动。执行命令和运行环境详见 `docs/testing/contracts/profile-auto-update.md`。
- 审查和批准：需独立复核真实 diff、ref 符号映射、失败/清理路径和宿主影响；L2 maintainer approval 后才提交和推送。
- 回滚：迁移或手工恢复已保存 0 的 interval 后，再回滚模型、配置兼容、调度器和 UI；避免旧程序把 0 当作无效配置。

## DIFF-058：流量报告暂停策略与桌面导航验证收敛

- 目标与非目标：将共享流量报告在 `refetchInterval: false` 时的暂停语义对齐本地 ref，并验证主界面流量路由在真实桌面窗口中的查询状态和侧栏辅助标签。此批不迁移 ref 的默认 2 秒轮询或 `placeholderData`，不增加本地 IP 探测，也不测试系统代理、TUN、运行时采集或连接详情的规则用量交叉跳转。
- 基线与映射：功能提交最初基于父仓库 `83a7223da5d9d9c6c789bcc32cc320aecf3ee60a` 和 ref gitlink `c08aa9d9a17953aa2464cdae61390d5c3f9afbc7`；当时只读 ref 检查在 `6feb4a71016e54260c1592f3a0d63a0169a2b40a`。随后本分支合入最新父仓库 master `ae48f24ae5012a47533784695d7109cafb753d12`（其 ref gitlink 为 `31019da14006b384b3272c72b4088179a9e72c47`）；本次另行 fetch 并核实 ref 最新 `origin/main` 为 `a4f706fe788e3570247ad2fa49636c6b4c55c98b`，ref 根目录和递归子模块均干净。`6feb4a71..a4f706fe` 未改动本批映射的 `ref/frontend/query/src/ipc/use-traffic-report.ts::useTrafficReport` 与 `ref/frontend/ui/src/navigation-sidebar.tsx::NavigationSidebarToggle`；该区间新增/调整 Flow Sankey 文件和 topology transition，将作为下一批对齐。`useTrafficReport` 的暂停、窗口聚焦/网络重连和 `retry: false` 行为对应 `frontend/interface/src/ipc/use-traffic.ts::useTrafficReport`。导航侧栏对应 `frontend/chimera/src/components/ui/navigation-sidebar.tsx::NavigationSidebarToggle`；Chimera 增加可选 `expandLabel`，拓扑侧栏传入本地化展开/收起文案。未重命名上游共享符号。
- 类别与接口：L2，共享查询行为会被拓扑和 Connections 状态统计使用；另有主界面可访问标签及四种语言消息。`useTrafficReport` 的选项限定为 `{ enabled?: boolean; refetchInterval?: number | false }`，并新增可测试的 `trafficReportQueryOptions` 描述器供 hook 复用。搜索确认 legacy 页面和 agent 当前没有 `useTrafficReport` 调用点；主界面仍通过同一共享 hook 读取。未改 IPC 命令、持久化、后端 runtime、legacy UI 或 agent 合同。
- 保留差异：相较当前 ref，Chimera 的 hook 仍未设置 ref 的默认 2 秒 `refetchInterval` 和按 topology layers 保留 `placeholderData`。显式配置轮询的调用方按 ref 关闭自动 retry 并由下一轮恢复；未配置轮询的 Connections 调用方保留 QueryClient retry，避免瞬时错误后不再恢复。此项只部分对齐，默认轮询和 placeholder 行为后续按调用方需要单独评估。
- 测试契约与验收：`docs/testing/contracts/traffic-analysis-main.md` 更新为 L2，并说明当前覆盖边界。已注册的 `tauri-e2e/specs/traffic-analysis-main.e2e.ts` 通过真实 main 窗口验证 Dashboard 准备、Traffic 导航、侧栏展开/收起标签变化、Closed scope、键盘选择 Connections，以及 Flow/Geography 保留 URL 状态。共享 hook 的单测验证未提供轮询时瞬时 IPC 错误会按 QueryClient 策略重试、显式轮询会留待下一轮恢复、暂停选项会关闭 focus/reconnect/retry。桌面 E2E 不声称覆盖这些后台事件、Connections 详情跳转、Tab 顺序或原生鼠标事件。
- 实际验证：`pnpm --filter=chimera-ui test:traffic` 8/8；`pnpm --filter @chimera/tauri-e2e test:unit` 25/25；`pnpm --filter @chimera/tauri-e2e typecheck`、`pnpm typecheck`、`pnpm lint:frontend-boundaries`、`pnpm --filter=chimera-ui build`、Prettier 和 `git diff --check` 均通过。使用外接盘独立 Cargo target 构建 `backend/tauri` 的 `e2e` feature 成功，最终二进制 SHA-256 为 `920c0cec03ff6cd854dc13e1ffb02c167bc866fb136d8aa8cc56a4caebb5e84e`。v6 和 v7 两次桌面运行失败并生成失败截图/HTML，日志有 `execute/async` 超时；之前文档将其报告为通过属错误。相同二进制在 2026-10-17T07:31:21Z 通过新隔离目录 v8 运行，macOS WebKit 605.1.15 为 1/1 通过；失败产物目录为空。精确命令见 `LESSON-20261017-003`。本地未运行 Windows/Linux E2E。
- 历史远端复验：已测试的 PR #489 SHA `13699341fc9389b4c18369609618e2fc944dddc3` 的 Windows Desktop E2E run [39037138976](https://github.com/MFSGA/Chimera/actions/runs/39037138976) 通过，53/53 specs；独立 Profile auto-update 和 controller fallback 各 1/1 通过。精确 SHA 的 [三平台 CI run 39037138645](https://github.com/MFSGA/Chimera/actions/runs/39037138645) 中 macOS、Ubuntu、Windows lint 与 Tauri 构建全部通过。这些结果只证明该历史 SHA；后续 PR head 的 CI/E2E 状态以 PR #489 最新检查为准。
- 风险、回滚与审批：不改变宿主网络、服务权限或用户数据。产品回滚限于恢复 `useTrafficReport` 原查询选项、拓扑侧栏标签调用/消息及本批 E2E/契约；ref gitlink 同步保持为独立任务。L2 产品改动需维护者复核和明确批准；因同时更新 `LESSONS.md`，该提交还需 L3 维护者批准。

## DIFF-056：按 ref 拆分核心 sidecar 解析器并补齐打包入口

- 目标：把二进制信息、平台映射和版本解析从 `scripts/check.ts` 收拢到 ref 对应的
  `scripts/src/prepare/` 模块；准备并打包 Chimera 当前支持的所有 core sidecar。
- 非目标：本批不搬迁下载/解压实现、进度显示或任务执行器，不改 Core/Service IPC、用户配置、
  数据格式或宿主网络行为；不引入 Chimera 尚未公开支持的 Meow core。
- ref 基线：`813673a8e84b51265467f02277fc7e739837f61e`（本地 `ref/` 与 `origin/main` 一致，
  工作树干净）。
- ref 到 Chimera 的路径与符号映射：
  - `ref/scripts/src/prepare/types.ts` 的 `ClashManifest`、`ResolveInfo`、`TaskDetail`、
    `DownloadProgress`、`DownloadResult`、`ResolveOptions` → `scripts/src/prepare/types.ts` 同名类型。
  - `ref/scripts/src/prepare/platform.ts::{normalizePlatform, normalizeArch, mapArch}` →
    `scripts/src/prepare/platform.ts` 同名函数；保留 ref 架构并扩展到 Chimera manifest 已支持的
    十种平台/架构组合。
  - `ref/scripts/src/prepare/versions.ts::{normalizeVersion, isValidVersion,
    extractVersionFromAssetList, fetchText}` → `scripts/src/prepare/versions.ts` 同名函数；
    `scripts/manifest.ts` 复用版本提取和文本请求实现。
  - `ref/scripts/src/prepare/binaries.ts::createBinaryResolvers` 及 Clash Premium、Mihomo、
    Mihomo Alpha、Clash-rs、Clash-rs Alpha resolver → `scripts/src/prepare/binaries.ts` 同名
    factory 和 resolver。保留 Chimera Client、Chimera Client Alpha、Chimera Service 扩展；
    Alpha Client 的刷新继续调用 Chimera manifest resolver，Service 版本继续来自本地 Cargo manifest。
    Mihomo Alpha 元数据请求异常时回退到版本 manifest；版本变化时通过 sidecar 版本戳使旧缓存失效。
  - `ref/scripts/src/prepare/check.ts` 的 core task 注册和
    `ref/backend/tauri/tauri.conf.json::bundle.externalBin` → 当前 `scripts/check.ts` 的
    task 注册及 `backend/tauri/tauri.conf.json::bundle.externalBin`。本批让 Clash Premium 和
    Mihomo Alpha sidecar 同时进入准备与 Tauri 打包清单，避免仅能解析却未随应用分发。
- 保留差异与收敛状态：记录创建时为部分迁移；后续 DIFF-057（PR #480）已把下载/解压、资源解析、
  进度和任务调度迁入同一 `scripts/src/prepare/` 模块，`scripts/check.ts` 保留为兼容委托入口。
  当前仅保留窄范围产品差异：ref 支持 Meow，但 Chimera 的应用配置和 IPC core 合同没有 Meow 变体，
  service endpoint 也拒绝该类型（底层 metadata 仍保留枚举），因此不注册 Meow sidecar。现有十种
  manifest 架构映射和 Chimera Service/Client 资产命名保留。
- 变更接口：新增内部 `createBinaryResolvers(BinaryResolverOptions)` factory；新增平台与版本 helper
  模块；`scripts/check.ts` 通过 factory 解析现有及补齐的 sidecar；Tauri bundle 增加 `clash` 和
  `mihomo-alpha` external binaries。无应用 IPC、前端 API、持久化或用户配置接口变化。
- 风险：L2（sidecar 下载、二进制分发及运行时 core 资源）。首轮独立审查发现 Alpha 缓存失效和
  元数据网络异常回退两项阻断问题；已增加版本戳校验与 pinned-manifest 回退及回归测试。修正后的独立复审已完成，
  无代码阻断项；审查建议同步修正本记录的测试数量与复审状态，现已更新。用户要求在远端 CI 运行期间继续独立批次；
  本批可在本地验收与独立复核完成后并行推送，但必须等本批自己的必需检查与审查通过后才能合并。
- 验收条件：十种架构均得到对应资产映射；Alpha 版本解析可刷新，元数据不可用时能回退且缓存按版本失效；
  Service 版本、归档名和 SHA-256 清单 URL 由本地 crate 版本推导；当前支持的 core sidecar 已注册准备任务并列入 bundle；Deno
  锁定测试、类型检查、lint、格式和差异检查通过；独立审查无阻塞项；推送该批 SHA 后所需 CI 与
  桌面 E2E 成功，失败则在同一分支修复并以新 SHA 重验。
- 验证计划与限制：`pnpm test:manifest`、带仓库 Deno 配置的冻结锁测试、`deno check`、
  `pnpm typecheck`、`pnpm lint:oxlint`、Prettier、`git diff --check`；推送后依赖 CI 的各平台
  lint/build 和 Tauri Desktop E2E 检查 sidecar 下载与打包。未在本机运行会下载并替换忽略 sidecar
  文件的 `pnpm prepare:check`，也未声称其本地通过。
- 回滚：在本批 feature 分支撤销上述解析器、task 注册、externalBin、测试入口/锁和本条记录；
  不需数据迁移或用户状态恢复。
- 阶段证据：DIFF-056 初始切片的 `pnpm test:manifest` 曾为 17/17；DIFF-057 完成后，全管线测试为
  28/28。测试入口固定使用 `scripts/deno.lock`，并将测试使用的 `@std/assert` 版本写入该锁，
  防止在仓库根目录生成额外锁文件。DIFF-057 当前切片的 Deno frozen check、Oxlint、Prettier
  与差异检查也通过。曾实际运行 `pnpm prepare:check --arch arm64 --sidecar-host aarch64-apple-darwin`，
  但下载反复重试且无进度后中断；随后对 Clash Premium、Mihomo Alpha 与 Chimera Client Alpha 的 GitHub 元数据/资产 URL
  执行请求均在 12 秒超时，故本地实际下载与安装未验证，不能记为通过。该批仍为部分迁移；推送后需记录该 SHA 的
  CI/E2E 结果，远端未通过前不得合并；DIFF-057 已完成此处预留的管线迁移。

## DIFF-057：迁移 prepare 下载、资源安装、进度和任务执行器

- 目标：完成上一切片预留的 prepare 管线迁移，将下载/解压、资源解析、进度渲染和任务调度放入 ref 对应的 `scripts/src/prepare/` 文件，并让 `pnpm prepare:check` 直接运行该入口。
- 非目标：不改变 manifest 版本解析、平台映射、应用 Core/Service IPC、用户配置、宿主网络/TUN 行为；不注册应用未支持的 Meow core；不在本机下载或替换真实 sidecar/资源来验证。
- 风险：L2（准备并原子安装 sidecar 与运行资源）。维护者已批准该批次提交/推送；合并仍需新 SHA 的 CI/E2E 通过。
- ref 基线：`813673a8e84b51265467f02277fc7e739837f61e`，本地 `ref/` 已核对为该 SHA 且工作区干净；本批未改动或重新同步 ref。
- ref 到 Chimera 的路径与符号映射：
  - `ref/scripts/src/prepare/download.ts::{downloadFile, extractZip, extractTarGz, gunzipFile}` → `scripts/src/prepare/download.ts` 同名函数；流式下载和解包流程沿用 ref。
  - `ref/scripts/src/prepare/progress.ts::{ProgressRenderer, formatSize, formatSpeed, formatProgressSize, formatResolveInfo}` → `scripts/src/prepare/progress.ts` 同名类和函数。
  - `ref/scripts/src/prepare/resources.ts::{resolveResource, resolveSidecar, resolveWintun}` → `scripts/src/prepare/resources.ts` 同名解析器；保留 Chimera 现有 Alpha metadata refresh、sidecar SHA-256 和临时版本戳。
  - `ref/scripts/src/prepare/check.ts::{runTask, resolveSidecarHost, main}` → `scripts/src/prepare/check.ts` 同名入口和任务执行路径；`createPrepareTasks`、`selectPrepareTasks` 放在 `scripts/src/prepare/tasks.ts`，并由独立测试检查支持的核心、资源和 Windows 任务。使用 Chimera 的 logger、manifest、资源路径，并注册 Clash、Mihomo、Mihomo Alpha、Clash-rs/Alpha、Chimera Client/Alpha、Chimera Service、Wintun、MMDB、GeoIP、GeoSite 与 Windows loopback 工具。保留十种架构和现有服务/客户端资产名。
  - 历史入口 `scripts/check.ts` 保留为兼容委托；`package.json::prepare:check` 改为直接运行新模块。
- 保留差异与缺陷修正：不注册上游 Meow，因为 Chimera 的 core 配置/IPC/service 合同不支持它；用现有 `verifyAndInstallSidecar` 为 sidecar 保留校验和 staged atomic install。普通资源现在也先写入同目录临时文件、下载完成后再替换：中断下载会留下半截文件并被下次按“已缓存”采用，已用测试复现该旧行为并验证新实现保留原缓存、清理临时文件。Wintun 的每次重试都会丢弃未完成归档；DLL 经同目录暂存并原子替换，失败时保留上次可用版本。迁移同时保留 DIFF-052 中仅针对 Chimera Client Alpha 滚动资产 404 的 1/2/4/8 秒有限重试；其他 sidecar 及非 404 错误不进入此延迟重试。这些是窄范围恢复性修正，ref 应在后续切片复查。
- 影响范围：仅改变 Node/Deno prepare 脚本入口、脚本测试执行权限和本地准备资源流程；主界面、legacy UI、agent、持久化数据和宿主代理状态不变。
- 验收：下载、失败清理、Alpha 刷新及有界 404 重试、非 404 快速失败、checksum 失败保留旧 sidecar、Wintun 重试与旧 DLL 保留、任务进度、注册任务清单、历史入口和所有当前 core task 可检查；`pnpm test:manifest`、Deno frozen check、workspace typecheck、oxlint、Prettier 和 diff 检查通过；独立复核无阻断项；推送后的必需平台 CI 与 Desktop E2E 通过。
- 验证计划/限制：使用假 HTTP 响应和隔离临时目录覆盖下载与恢复；本地 `pnpm prepare:check` 会访问网络并替换被忽略的 sidecar/resource 文件，本批不运行该命令。全平台真实下载与 Tauri 打包由本批推送后的工作流验证。
- 回滚：回退 `scripts/src/prepare/{check,download,progress,resources}.ts`、相应测试/测试入口、`prepare:check` 路径及本条记录；无用户数据迁移。
- 当前本地证据：`pnpm test:manifest` 28 passed、0 failed；新增假 HTTP 用例验证滚动 Alpha 在短暂 404 后恢复、重试预算耗尽时失败，以及 503 不触发延迟重试；恢复成功检查实际安装字节和版本戳，耗尽场景检查原 sidecar 与版本戳保持不变。任务级回归用例实际执行 Alpha task，验证该重试策略生效且 Mihomo Alpha 不使用该策略。下载与资源测试通过共享 `withTempFetch` 包装，在 setup/body 失败时仍独立恢复 fetch mock、清理临时目录并保留主要与清理异常。`deno check --frozen --config scripts/deno.jsonc scripts/check.ts scripts/src/prepare/check.ts scripts/src/prepare/tasks.ts`、`pnpm lint:oxlint`、Prettier、`git diff --check` 通过。更新到当前 master 的外部 worktree 中，前端/E2E typecheck 因 node_modules symlink 指向另一个 worktree 且缺少解析依赖，未能有效执行；本批新 SHA 的 CI 将提供类型检查与构建复验。针对旧 CI 失败的 `client::profiles::tests::import_suggestion_is_committed_and_reschedules_the_timer` 定向 Rust 测试在本机通过 1/1；本机 `pnpm prepare:check` 未运行，不宣称真实资源下载/安装已验证。
- 远端失败证据（修复前的 #480 head `c797797d16cc2453b01ca460a2adf3e6b90a1d67`）：CI https://github.com/MFSGA/Chimera/actions/runs/38558416365 的 Ubuntu 后端测试因观察 fetch 与 actor 提交之间的竞态失败，现已改为等待提交后的快照；Desktop E2E https://github.com/MFSGA/Chimera/actions/runs/38558416387 的 Windows 运行中 51/52 个 spec 通过，`rules-main-proxy-icon.e2e.ts` 未加载图标。PR #479 的缓存 generation 修复已通过全平台 CI 和 Desktop E2E，并于 2026-10-14 合入 master；#480 的新 SHA 仍须重新执行必需检查。当前本地 `pnpm prepare:check` 未运行，不代表真实资源下载/安装验证通过。
- 后续远端复验（PR #480 head `4a92a6916f16e2f768f2e83c1db608958fc88a49`）：[CI run 38571885904](https://github.com/MFSGA/Chimera/actions/runs/38571885904) 的 Linux/Windows lint 通过，macOS lint 在 `pnpm prepare:check` 下载 `chimera-client-alpha` 时收到 `404`；同一 SHA 的 [Desktop E2E run 38571885920](https://github.com/MFSGA/Chimera/actions/runs/38571885920) 通过。失败发生于 2026-10-14 13:31 UTC。GitHub 于 2026-10-15 02:51 UTC 发布的 [`Chimera_Client` latest release](https://github.com/MFSGA/Chimera_Client/releases/tag/latest) 当前包含预期的 `chimera-client-0.27.1-aarch64-apple-darwin.tar.gz` 资产；这说明该资产后来可用，但不能单独证明失败的历史原因。没有更改 resolver 或扩大重试预算；新 SHA 的 macOS prepare 检查仍须重新验证。当前本地没有运行 `pnpm prepare:check`，也没有下载或替换 sidecar。

## DIFF-047：同步最新 ref 的 Tokio patch 版本

- 基线：原始功能分支基于主仓库 `e9e3fab1913773a1ecd6ad6a689175e523208cc2`；只读 ref 当前固定在
  `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3`，工作树干净。该快照包含
  `67999753918c7b9879ddd2bacdef74246bfef1dd`，其 Tokio 锁定版本为 `1.53.3`。
- ref 到 Chimera 映射：`ref/backend/Cargo.lock` 的 `tokio` package 条目对应
  `backend/Cargo.lock`；双方的工作区清单都以 `tokio = "1"` 声明兼容范围。
- 类别：共享 Rust 依赖对齐。
- 更新：将锁定 Tokio 从 `1.53.2` 升到 ref 当前的 `1.53.3`，不改任何 `Cargo.toml` 约束或运行时代码。
  与 master `5fc556b4` 同步时保留其完整依赖图；相对该 master，锁文件只变更 Tokio 版本和校验值，
  没有带入无关的 Windows 依赖重解析。
- 影响入口：Rust workspace 中的 Tauri 应用、共享 actor、核心管理器、Service IPC 和 Agent 共用 Tokio；
  主 UI、legacy UI 与 agent 的业务 API、持久化、IPC 合同及用户配置格式均未改变。Windows 依赖图由 CI 验证。
- 验证：同步 master 前的功能分支上，`cargo check --locked --manifest-path backend/Cargo.toml --workspace` 通过；
  `RUST_TEST_THREADS=1 pnpm test:backend` 的全 workspace `--all-features` 测试退出码为 0。
  默认并行运行首次为 549 passed、3 failed、1 ignored，失败均为 `client::profiles` 外部文件监听
  用例在 5 秒内未观察到更新时间；随后 `external_` 子集分别以默认并行（4/4）和
  `--test-threads=1`（4/4）通过，串行全 workspace 也通过。原因尚未由本地证据确定，保留首次失败记录，
  由推送后的 CI 决定是否需要修复。首次 check 因本机磁盘空间不足失败；仅使用
  `cargo clean -p chimera` 清理可重建的应用构建产物后，check 通过。同步 master 后，
  `cargo metadata --locked --format-version 1 --manifest-path backend/Cargo.toml` 通过，且相对 master
  的锁文件差异仅为 Tokio 版本/校验值。`pnpm exec prettier --check docs/ref-alignment-progress.md`
  和 `git diff --check` 通过。最终分支的 Windows/macOS/Linux CI 和 Tauri E2E 以 PR 当前 SHA 检查为准。
- 收敛状态：Tokio 版本与当前 ref 一致；最终合并分支仍须通过 PR 当前 SHA 的 CI 和 Tauri E2E。

## DIFF-059：日语界面语言

- 目标与范围：按上游日语语言支持让主 UI 和受支持的 legacy UI 共享 `ja` 语言设置与翻译资源；安装器语言选择单列为 L3 发布配置，不包含在此 L2 功能验收中。
- 基线：当前 master 固定的本地 `ref/` 为 `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3`，工作树干净；日语功能来自上游 `b3150805e` 并已包含在该基线。本批未修改或移动 `ref/`。
- 映射：ref `backend/nyanpasu-config/src/application/i18n.rs::{I18nLanguage,default_i18n_language,I18nLanguage::as_str}`、`backend/tauri/locales/ja.json`、`frontend/nyanpasu/messages/ja.json`、`project.inlang/settings.json`、`src/assets/styles/tailwind.css`、`src/utils/language.ts`、`src/components/providers/language-provider.tsx`；Chimera 对应 `backend/chimera-config/src/application/i18n.rs`、`backend/tauri/locales/ja.json`、`frontend/chimera/messages/ja.json`、`project.inlang/settings.json`、`src/assets/styles/tailwind.css`、`src/utils/language.ts`、`src/components/providers/language-provider.tsx`、`backend/tauri/src/bridge/verge.rs`、`backend/tauri/src/client/application.rs` 与生成的 `frontend/interface/src/ipc/bindings.ts`。日历语言在 Chimera 的 Day.js 根初始化处注册，因为项目当前使用 Day.js。
- 行为和接口：增加 `I18nLanguage::Japanese`、从系统 locale 识别日语和 Rust bundle 的规范 locale key；客户端完成 typed 配置加载后再设置 Rust-native locale，避免新安装时前端与 tray 翻译语言分离。旧配置 `ja-JP` 通过共用映射选择 `ja` bundle。前端 locale cache 和首屏状态按 ref 的 `getCachedLanguage`、`setCachedLanguage` 与 `LanguageProvider` 初始化门控实现；后端查询完成前保留缓存作为快速路径，首次无缓存时应用后端解析语言，持久化值和缓存不一致时再同步。主、legacy 入口继续使用同一个 provider；IPC 命令不变，序列化配置接受历史 locale 写法并输出 ref 对齐的小写规范值。日语与英文目录现有 908 个键及占位符一致，后端日语 bundle 与英文 bundle 的 19 个叶键一致。
- 桌面用例：新增 `docs/testing/contracts/user-interface-language.md` 与 `tauri-e2e/specs/user-interface-language.e2e.ts`，注册到 `settings` suite。用例聚焦语言控件并按 Enter 打开菜单、按 ArrowDown 选择日语、再按 Enter 确认，检查 IPC、文档 `lang`、隔离运行目录中的 `application.yaml` 写盘值、刷新后的主 UI，以及 legacy 代理页的标题和三个模式标签；清理时恢复 `application.yaml` 中的原语言、两个 WebView 的语言缓存、路由和窗口尺寸，且各清理步骤失败时仍继续执行后续恢复。当前锁定的桌面驱动点击只合成 DOM `click`，Radix 菜单使用 `pointerdown` 打开，因此该契约覆盖键盘 handler，不声称覆盖鼠标打开、菜单动画或 Tab 顺序。用例不修改系统代理、TUN 或服务状态。
- 保留差异：Chimera 使用 `sys_locale` 读取系统 locale，并在本地已有的 Day.js 初始化中注册日语；ref 使用 `nyanpasu_helper` 与 date-fns。系统 locale 解析和 UI 日历表现按 ref 对齐，依赖与日期工具仍沿用 Chimera 现有调用链，待共享日期 API 迁移时再复核。旧配置的 `en-US`、`zh-CN` 与 `zh-TW` 通过 serde alias 继续可读，新写入使用 ref 的规范小写键；legacy `ja-JP` 映射留在边界适配器，待实际存量配置迁移覆盖后移除。兼容 `patch_verge_config` 在 legacy 配置落盘后才切换 Rust locale；若托盘刷新失败，会恢复 typed 配置中的原 locale 并重试原语言托盘。韩语前端目录仍未迁入，不属于本批目标；此处只新增日语。
- 风险与验收：共享语言状态与持久配置为 L2。较旧版本不认识本版写出的 `en`、`ja`、`zh-cn`、`zh-tw`；回退前需关闭应用，并将 `application.yaml` 顶层 `language` 改为旧版可读的 `en-US`。测试契约记录了各平台配置目录及该恢复步骤，Rust 回归测试用上一版 enum 验证 `ja` 会被拒绝且 `language: en-US` 可恢复读取。`.github/workflows/ci.yaml` 增加 `test:localization` 为 L3，须单独取得维护者批准；`backend/tauri/tauri.conf.json` 的 Wix/NSIS 日语安装选项不在本 diff 中，仍需未来独立审查。
- 验证：候选分支 `pnpm typecheck`、`pnpm --filter chimera-ui test:localization`（4/4；先由前端构建生成 ignored Paraglide runtime，CI 的前端测试步骤前已有 workspace build）、`pnpm --filter @chimera/tauri-e2e typecheck` 与 `pnpm --filter @chimera/tauri-e2e test:unit`（25/25）通过。`cargo +nightly test --locked --manifest-path backend/Cargo.toml -p chimera-config`（147/147）、legacy locale/rollback 定向测试（31/31）和 `japanese_write_failure_preserves_previous_application_state`（1/1）通过；包含旧版本 enum fixture 的 `downgrade_recovery_resets_language_to_a_value_the_previous_release_reads` 已在完整 147 项套件中通过。`typescript_bindings_are_fresh`（1/1）通过，生成绑定由 `update_typescript_bindings`（1/1）更新。改动 Rust 文件的 `rustfmt +nightly --check --config-path backend/rustfmt.toml --config skip_children=true ...`、相关文件 Prettier、`git diff --check` 及 `actionlint -shellcheck='' .github/workflows/ci.yaml` 通过。全 workspace `cargo fmt --all -- --check` 仍报告未修改的 vendored WebDriver 文件格式差异，改动 Rust 源文件的定向检查通过。
- 构建与桌面验证：`pnpm --filter chimera-ui build` 通过。CDN 插件首次尝试 4 分 38 秒无响应后中断；重跑时临时用 npm 官方 `@inlang/plugin-message-format@4.5.4` 与 `@inlang/plugin-m-function-matcher@2.3.4` 本地副本，二者 SHA-512 与 registry 元数据一致，构建后恢复原 `project.inlang/settings.json` 并删除临时插件；未改依赖或锁文件。为避免 CI 前端构建后再重复请求这些 CDN 插件，`test:localization` 只运行测试，使用前端构建已生成的 ignored runtime。带 `e2e` feature 的 Tauri 候选 binary 使用移除 external sidecars 的临时配置构建，因此只用于语言界面用例，不用于 core/runtime 验收。macOS WebKit 605.1.15 的最终 `user-interface-language.e2e.ts` 为 1/1 通过且无重试；更早两次本地运行分别出现 WebDriver execute/sync 超时自动重试，以及 after hook 清理失败，根因未确认。最终成功 run 恢复并读取到原 `application.yaml` 语言。未验证完整 core sidecar、冷启动或其他平台；Windows CI/E2E 未在候选 SHA 执行。
- 审查和批准：独立审查确认初版存在 legacy `patch_verge_config` 在持久化前切换 Rust locale 的 P2 回滚缺口；现已将 locale/tray 应用移至持久化之后，并增加持久化失败不切换语言、托盘刷新失败恢复旧 locale 的测试。第二次复审发现回退到不支持日语的旧版本需要把 `application.yaml` 顶层语言复位为 `en-US`；该步骤已写入测试契约并由旧版 enum fixture 测试覆盖，复核确认 P1 已解决且无 P0–P2 阻塞项。另有两项非阻塞建议：托盘两次刷新均失败的错误聚合测试，以及后端 bundle 自动键集一致性测试。共享语言与配置行为为 L2，仍须维护者明确批准提交；新增 CI 执行入口为 L3，须单独批准。
- 回滚：移除日语语言枚举和 bundle、前后端目录、共享语言选项与 Day.js locale 注册、桌面 spec 注册及此记录；不迁移或删除已保存配置。安装器 Wix/NSIS 语言列表不属于此 L2 diff。


## 2026-10-19 本地 GeoIP 索引与 traffic 地理维度（L2）

- 目标：让共享 traffic 记录按运行内核实际使用的 GeoIP 数据库解析源/目的地区域；Main、Legacy 与 agent 继续走同一个 traffic actor 和 application API。
- 非目标：不新增公网 IP 探测、外部定位服务、UI 开关、TUN/系统代理变更或用户配置迁移；不修改只读 ref、删除 legacy UI 或改变 Client/Service 生命周期。
- 基线：Chimera `master` `09f89a45875581979446a50a716bba5bb36c32c1`；该基线的 ref gitlink 为 `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3`。功能实现时只读 ref checkout 为 `4eaf13b9c5dc468fdfc38272bf76b20a681d2e54`，工作区干净且与 `origin/main` 相同；已核对 `backend/nyanpasu-geodata/`、`backend/tauri/src/core/geo/` 和 `backend/tauri/src/core/traffic/` 在 `d43819f..4eaf13b9` 间无差异。本次保留 master 原 ref gitlink，不夹带基线更新。
- 路径和符号映射：`ref/backend/nyanpasu-geodata/` → `backend/chimera-geodata/`（仅替换包名品牌，保留解析器、错误处理、测试和许可证）；`ref/backend/tauri/src/core/geo/{actor,adapters,client,fixtures,mod,ports,tests}.rs` → `backend/tauri/src/core/geo/`；`ref/backend/tauri/src/core/traffic/{actor,geo,source}.rs` → 同名 Chimera 模块。`GeoIndexActor::{follow_core,reload}` 保留；将 ref `CoreClient` 适配为共享 `CoreLifecyclePort` 与现有 `ApiClient::get_configs`，从 `ClashRuntimeConfig.geodata_mode` 选择 MMDB 或 DAT。启动时等待核心 API 可用；之后以核心生命周期状态提供的稳定实例 ID 绑定控制器、WebSocket traffic 帧和 GeoIP 索引；`CoreStatusSnapshot.instance_id` 经各 host projection 传递，兼容旧 host 时只使用非零状态时间戳作为身份。每个 WebSocket 更新保留连接时解析到的核心 ID，模式查询和索引快照都携带同一 ID，traffic pump 只在三者一致时应用索引。核心切换期间不使用旧索引；若模式查询或数据库加载失败，traffic 帧仍可记录，但不使用旧地理索引，watcher/低频复核会继续尝试。文件 watcher 驱动数据库更新，60 秒低频复核兜底 watcher 漏报。`TrafficArgs` 新增只读 index watch receiver，`pump` 对每个 traffic frame 使用当前快照；`locate_source`、`locate_destination` 沿用 ref 的地址选择与 geo basis 规则。客户端只在 traffic store 可用时创建索引 actor，并从现有 core app-data 目录读取数据库、在 app cache 保存可丢弃的派生索引。
- 保留差异：ref 的 `LocalSourceCache`、公网 IP 检测和 `enable_local_ip_probe` UI/配置不迁入；Chimera 当前 `NoLocalSource` 返回空地址，因此私有/本机来源仍显示 `unknown`。模式读取复用 active core 的 `/configs` API 并设置 `no_proxy`；API 地址来自现有控制器绑定，若用户将 controller 配为非 loopback，读取也会发往该配置地址。没有新增 GeoIP provider、公网 IP 探测或地图请求。索引只读 core 本地文件。只有另行批准显式 opt-in、TUN 排除和隐私/网络测试后，才复查 local-source 探测。
- 接口：内部 `GeoIndexArgs` 改为注入 `CoreGeodataModeSource`；`TrafficArgs` 增加 `geo` watch；Rust `ClashRuntimeConfig` 和 Specta 输出增加可选 `geodata_mode` 字段，并由现有生成器更新 TypeScript binding。现有 traffic IPC 与 UI 消费合同不变。
- 验收和测试计划：执行 `cargo test --manifest-path backend/Cargo.toml -p chimera-geodata`、`cargo test --manifest-path backend/Cargo.toml -p chimera --lib core::geo::`、`cargo test --manifest-path backend/Cargo.toml -p chimera --lib core::traffic::`、Clash API geodata-mode 反序列化测试、Specta binding freshness、`pnpm typecheck`、`pnpm lint:clippy` 与 `git diff --check`；CI 应由已有 `pnpm test:backend` 全 workspace 门禁运行新增 Rust 测试。traffic 回归还需覆盖“核心 B 的索引已发布后，核心 A 的延迟帧不能使用 B 的索引”。真实 core/Service 和桌面网络验证不由这些单测覆盖。
- 风险、回滚与审批：L2（读取运行时配置、core 数据目录并管理派生缓存；不改变网络状态或用户配置）。回滚删除本批 crate、Geo actor 和 traffic 接线，移除新增的 runtime projection/binding 字段；已生成的缓存可安全删除。该实现属于用户已批准的持续 ref 对齐目标；本批不修改 L3 工作流或规范。
- 原功能分支验证结果：`c9228e99` 的 `pnpm test:backend`（Rust workspace、all features）通过；Tauri lib 单测为 613 passed、1 ignored，`chimera-geodata` 有 73 项通过，本地公开数据库 `--test real_databases -- --ignored` 3/3 通过。新增 core-instance 集成用例验证新核心 mode 读取失败时交通帧使用核心自带地区值而不是仍发布的旧索引；下一次读取成功后改用新索引。Geo actor 测试覆盖新实例相同模式的索引重载失败和同实例重试去重。该分支的 `pnpm typecheck`、`pnpm lint:clippy`（仓库有大量既有 Rust/Clippy warning，命令退出码为 0）、目标 crate `rustfmt --check`、相关文档/bindings Prettier、locked Cargo metadata、`pnpm lint:frontend-boundaries` 与 `git diff --check` 通过。`pnpm prepare:check` 和桌面/真实 Service 矩阵未运行：该脚本可能写入本机 sidecar/resource 资产；本批没有 UI 行为改动，未运行桌面 E2E。原功能分支独立复审无 P0–P2；发现的 watcher 漏报、同模式跨实例旧索引、controller 地址描述问题均已修复并覆盖。
- 最新 master 基线复验：GeoIP 实现最初基于 `09f89a45875581979446a50a716bba5bb36c32c1`，功能提交为 `d3b6b3e69dac8e6ecdd033eca8c82c68f18947ec`；审查修复提交为 `b21896b14759928b753415e3f29b76a598199ff1`。独立复审发现旧 WebSocket traffic 帧可能与当前核心模式/索引错配；现将核心实例 ID 贯通 lifecycle、WebSocket frame、模式响应和索引快照，并增加 A 核心延迟帧在 B 核心索引已切换后的回归测试，确保使用帧自带地区值而非 B 的索引。随后 master 合入已验证签名的 `arc-swap 1.9.3` patch 更新 `b264ec54f6f51797d4f59e5a8b9e1890cacc72de`；本分支以签名合并提交 `fe97bac3dc9c85221dab77e5b5727d4117b81c5a` 同步该基线，当前最终 SHA 的所有新增提交均已本地 GPG 验证，作者/提交者字段未包含 `lmmqxyx`。

  在更新后的 lockfile 上，traffic 定向测试 15/15、Geo actor 14/14；`chimera-geodata` 所有可运行测试通过，需要 `CHIMERA_GEODATA_DIR` 的真实数据库测试 3 项按设计忽略。此前完整 `pnpm test:backend` 在使用 `TAURI_CONFIG` 跳过未准备 sidecar/resource、关闭 Rust 增量编译后通过；Tauri lib 613 passed/1 ignored。该完整 workspace 运行发生在同步 `arc-swap 1.9.3` 之前；新 SHA 上重新运行的三个定向套件已通过。`pnpm typecheck`、`pnpm lint:frontend-boundaries`、`pnpm lint:clippy`（退出码 0，存在仓库既有 warnings）、目标 crate `cargo fmt --check`、Prettier 和 `git diff --check` 在先前功能 SHA 通过；完整 macOS/Windows/Linux CI 与桌面 E2E 需以同步后的最终 SHA 运行。`TAURI_CONFIG` 测试覆盖不包括桌面 bundle/sidecar；未验证 live core/Service replacement 或 WebSocket handshake。独立复核无 P0–P2 阻塞项，覆盖为 fixture-level traffic pump 测试与代码路径。此前 b218 SHA 的远端检查仍在运行，不作为最终结论；同步提交推送后，必须用新 SHA 检查结果决定是否合并。

## DIFF-060：流量拓扑 Sankey 图和可配置排名（L2）

- 目标：将 ref 的 Flow 页面 Sankey 可视化、卡片图备选、排名选择和持久化视图设置迁入 Chimera；Flow 与地图共用一份排名偏好，并继续经现有 traffic 页面状态与查询 API 工作。
- 非目标：不改变 traffic 查询、IPC、后端数据模型、legacy UI 路由、agent API、核心生命周期或网络/TUN 行为；不新增桌面 E2E 用例。
- 基线：只读 `ref/` 与远端 `origin/main` 均为 `03ab6e5824cb44e527bde7cca28b699e8bf3a766`，ref 工作树干净。Sankey/Flow 设置实现来自 ref 提交 `1990171f1`；本次同步新增的上游提交只更新 `manifest/version.json`，没有改变本批映射的 Flow/Sankey 文件。上游 Alpha 版本值与 Chimera manifest 的差异作为独立依赖批次评估，不在此 UI 批次中混改。
- ref 到 Chimera 路径和符号映射：`ref/frontend/nyanpasu/src/pages/(main)/main/topology/flow/index.tsx` → `frontend/chimera/src/pages/(main)/main/topology/flow/index.tsx`；`flow/_modules/{sankey-layout.ts,sankey.tsx,card-graph.tsx}` → 同名 Chimera 模块；`topology/_modules/{view-settings.tsx,view-rankings.tsx}` → 同名 Chimera 模块。保留 `TrafficViewSettingsProvider`、`useTrafficViewSettings`、`toTrafficViewSettings`、`ViewRankings`、`Sankey`、`layoutSankey`、`CardGraph` 的上游分解和符号；`traffic-context.tsx::RANKINGS` 改为 ref 顺序 `process,target,exit,source,inbound`。设置通过共享 `useKvStorage` 保存在 `traffic-view`，默认选择 ref 的前三个排名和 Sankey 图；地图和 Flow 排名使用同一设置，Flow 菜单还可切换 Sankey/卡片图。
- 保留差异：Chimera 现有 ranking card 不接收 ref 的 `dimension` 属性，因此调用保持本地组件合同；保留 Chimera 已有的 process/target/exit/source/inbound 独立排名页面，不因 ref 统一入口而删除。Sankey 布局用普通 `Map` 分组及复制后排序，替代 ref 的 `Map.groupBy`/`toSorted`，计算顺序与结果不变，避免增加运行时 API 要求。当前仓库没有设置显式 Vite 浏览器 target 或 Tauri macOS minimumSystemVersion；若将来统一提高运行时最低版本，可重新评估这项差异。
- 影响范围与接口：新增只影响 Main traffic topology 页面内部组件、一个 KV 键和 5 种现有 UI 语言的消息；无 Rust/IPC、数据 schema 或 agent contract 变化。检索确认 legacy 页面没有对应 topology/Flow 路由，本批没有改其交互；agent 不使用该 presentation 设置。
- 验收与验证计划：排名顺序/未知值清理、存储迁移和非法图类型回退由纯函数测试覆盖；Sankey 位置、边宽、过滤及最小高度由布局测试覆盖；运行 `pnpm --filter chimera-ui test:traffic`、`test:localization`、`pnpm typecheck`、Main UI production build、受影响文件 Prettier、`pnpm lint:frontend-boundaries` 与 `git diff --check`。不运行桌面 E2E；这些纯测试和构建不代表真实桌面点击验证。
- 风险与回滚：L2（新增持久化 UI 偏好及主界面图形交互）。恢复方式是回退本批 Flow/排名/设置组件、上下文顺序、消息、单测、依赖入口和本条记录；`traffic-view` KV 值可保留，不影响旧代码读取的配置。
- 当前本地验证：`pnpm --filter chimera-ui test:traffic` 16/16、`pnpm --filter chimera-ui test:localization` 4/4、`pnpm typecheck`、`pnpm lint:oxlint`、`pnpm lint:frontend-boundaries`、`pnpm --filter chimera-ui build`、受影响文件与本记录的 Prettier 检查、`git diff --check` 均通过。构建只报告 Vite 既有的大 chunk 与 ripple 动态导入警告。Main/legacy 桌面 E2E 未运行，真实 WebView 和 persisted cold-start 行为未在桌面进程中验证。

## DIFF-061：同步最新 ref 子模块快照

- 目标与范围：把 Chimera 工作区使用的只读 `ref/` 快照同步到 Clash Nyanpasu 上游最新 `main`；不改变 Chimera 的产品依赖、功能代码或用户数据。
- 基线与更新：从 `03ab6e5824cb44e527bde7cca28b699e8bf3a766` 拉取上游 `main` 后，快进到 `9f72efe292975b13b5d9018a9f5641ffbb071c27`。新增上游提交为 `1f59b4fa8`（更新 `postcss` 至 8.5.32）和 `9f72efe29`（更新 `@inlang/paraglide-js` 至 2.29.2）；改动仅涉及 ref 的两个 package manifest 和 `pnpm-lock.yaml`，未改 Dashboard 实现。
- 验证：fetch 后 `ref/HEAD` 与 `ref/origin/main` 均为 `9f72efe292975b13b5d9018a9f5641ffbb071c27`，`git -C ref status --short` 为空；新 SHA 可由旧 SHA 快进到达。该批只同步参考子模块，不运行产品构建或测试。
- 影响与回滚：父仓库 `ref` gitlink 从 `03ab6e5824cb44e527bde7cca28b699e8bf3a766` 改为 `9f72efe292975b13b5d9018a9f5641ffbb071c27`；回滚仅需恢复原 gitlink。此更新作为单独提交推送，避免与 Dashboard 功能和 E2E 修复混在一起。

## Windows WebView2 随最小化/恢复同步可见性（Windows E2E 已通过；CI 依赖修复待新 SHA 验证）

- 目标：对齐固定 ref `9f72efe292975b13b5d9018a9f5641ffbb071c27` 中 `window/engine.rs::set_in_sight` 与 `window/manager.rs::follow_minimized`。最小化窗口时隐藏 WebView2 controller 并请求低内存目标；恢复或重新聚焦时先恢复 WebView 可见性和正常内存目标，再显示/聚焦原生窗口。
- 非目标：不迁移整个上游 `WindowManager`，不改变窗口创建/销毁/尺寸恢复/关闭行为，不新增托盘隐藏逻辑，不改权限或持久配置，不影响 Chimera Client/Service、legacy UI 和 Agent。
- 参考映射：`ref/backend/tauri/src/window/engine.rs::set_in_sight` → 新增 `backend/tauri/src/window/engine.rs::set_in_sight`；`ref/backend/tauri/src/window/manager.rs::{watch,follow_minimized,focus_existing_window}` → `backend/tauri/src/window.rs::{AppWindow::create,follow_minimized,focus_existing_window}`。保持上游的 `IsVisible` 幂等检查、`SetIsVisible`、`ICoreWebView2_19::SetMemoryUsageTargetLevel` 及先恢复 WebView 再显示窗口的顺序。COM 依赖与锁定 Tauri/Wry 版本匹配：`webview2-com 0.39` / `windows-core 0.62`。
- 接口与风险：新增仅内部可见的 `window::engine::set_in_sight`，并在原生窗口 resize/minimize/restore 事件接线；没有 IPC、前端公开 API、存储 schema 或网络行为变化。L1 Windows 窗口生命周期与资源使用。
- 验收：真实点击 main 窗口最小化控件后，Windows WebView 的 `document.visibilityState` 变为 `hidden`；恢复窗口后回到 `visible`。定向 hermetic desktop E2E 在 PR CI 执行。Rust 格式、编译、单测、E2E 类型检查和该用例均需覆盖；Mac 本地结果只证明跨平台 no-op 与工具链可运行，不替代 Windows WebView2 验证。
- 本机验证：macOS `cargo check --manifest-path backend/Cargo.toml -p chimera --lib` 通过；`cargo test --manifest-path backend/Cargo.toml -p chimera --lib window::tests -- --test-threads=1` 6/6；`cargo fmt --manifest-path backend/Cargo.toml --package chimera -- --check`、E2E TypeScript 检查、harness 单测 30/30、E2E 桌面构建、Prettier 与 `git diff --check` 通过。定向 WebDriver spec 在 macOS 按设计跳过，不能作为 Windows 行为验证。尝试 Windows GNU 交叉编译未完成：环境没有 `x86_64-w64-mingw32-gcc`；Windows COM 分支须由 PR Windows CI/E2E 编译和执行。
- 独立复核：无 P0–P2 阻塞项；确认 ref 映射、依赖版本、事件时序与测试清理符合目标。明确未覆盖内存 target 的实际效果和 COM HRESULT 失败恢复；PR Windows 构建和 E2E 是剩余证据。
- 远程结果与 CI 修复：PR #540 原 SHA `c1f8a498ab5781285295c25daa2b67f9e1e5aba0` 的 Windows Desktop E2E run `40632079689` 通过；CI run `40632079560` 的 Linux `Lint` job `122631703884` 在 Ubuntu 26.04 runner 安装已不存在的 `webkit2gtk-driver` 时失败，源码检查未开始。`.github/workflows/ci.yaml` 已改为根据 apt 候选包在 `webkit2gtk-driver` 与 `webkitgtk-webdriver` 间选择；本地 shell/格式检查通过。该修复与同步最新 master 后的新 SHA 远程 CI/E2E 仍待复验。
- 回滚：回退 WebView2 helper、窗口事件接线和定向 E2E；不涉及用户数据或系统设置迁移。
- 审批触发：不改变架构所有权、权限模型或持久数据，无需额外审批。

## DIFF-062：把系统代理与自启动交给共享 effects owner（覆盖 legacy 与 Agent）

- 目标：按当前只读 `ref/` 将 system-proxy/PAC/autostart actor 接入 Chimera 的共享 effects executor；typed API、legacy UI、Agent 确认后的网络动作与启动恢复都复用同一 owner，退出时由 actor 恢复启动前的代理状态。
- 非目标：不修改 TUN/Service 权限与生命周期、代理规则或配置格式；不移除 legacy UI、Chimera Client/Service 或公开的 `core::sysopt` 模块；不在日常开发主机实际开启/关闭系统代理、PAC 或登录项。
- 基线：父仓库分支从远程 `master` `5c491b2` 快进同步至 `25d1829a4816a99b402a1a655ca80654e526c2c6`；运行时 submodule 为 `ceee7f7df1d01b421fc75409ec667852b7cc7cda`。本批实际只读 `ref/` 为 `9f72efe292975b13b5d9018a9f5641ffbb071c27`，`git -C ref status --short` 为空；本批没有改动或更新 ref。
- 路径与符号映射：完整复制 `ref/backend/tauri/src/client/system_proxy/{actor.rs,adapters.rs,error.rs,mod.rs,ports.rs,tests.rs}` 到 Chimera 同路径；`SystemProxyActor`、`SystemProxyClient::{spawn,reconcile,status}`、`OsProxyPort`、`AutoLaunchPort`、`PacPort` 与 ref 对齐。`ref/backend/tauri/src/utils/blocking.rs::join` 复制到同路径，供阻塞系统 API 运行。`ApplicationEffectExecutor::system_proxy_desires` 与 `apply` 按 plan 顺序把 AutoLaunch/SystemProxy/ProxyGuard 合为一次 actor reconcile；本地保留既有 `ApplicationEffectAdapters`，继续承担 Locale/Logger/Widget/Tray 等 Chimera 适配。
- 生产接线：`setup.rs` 创建 actor 和真实平台 adapter；`ApplicationEffectExecutor` 与 `ChimeraClient` 注入同一个 `SystemProxyClient`。移除 `resolve_setup`、legacy Verge patch 与 Clash patch 中并行的 `Sysopt::init/update/reset_sysproxy` 业务调用；legacy commit 在成功持久化后向共享 effects actor 发布所变更 domain。Agent 普通开关通过 typed app patch 触发共享 effects，并等待 owner 状态后核验实际 OS 状态；Agent 的遗留代理修复通过同一 actor 做带预期端口与 loopback 检查的串行写入、读回验证和失败恢复，不再直接写 `Sysproxy`。启动完成时发布完整效果。effects 只从与当前 host 状态匹配的 `RuntimeApplyReceipt.ports.bindings.mixed_port` 取得活动端口；`ClashInfo::port` 是 controller 端口，不用于设置系统代理。Agent 与停止后 stale-proxy 校验优先使用当前 host 最近一次已确认的运行时 receipt。随机端口没有 receipt 时端口视为未知，Agent 不提供依赖该端口的开关或 stale 修复；不使用启动时可能被覆盖的 Verge 镜像猜测旧运行端口。固定端口没有 receipt 时仍可由固定配置确定。未确认活动 receipt 时 effects 发送 `None`，actor 按其 retryable unresolved-port 状态等待后续绑定。退出请求取消同一个 shutdown token，`TaskTracker` 等待代理 actor drain 与恢复完成。
- 保留差异与边界：Chimera 的 `EffectHealth` 错误码是 `&'static str`，所以将 ref 的 `EffectFailureCode` 映射为稳定 snake_case 字符串；其他本地 effects 错误模型和持久配置合同不变。`core::sysopt` 公开模块暂时保留以避免无必要的兼容性删除，但本仓生产代码不再从它应用自启动/系统代理。Widget runtime 和 ref 的 `TrayView` 缓存所有权不属于本批，PAC/system proxy host 写入仍仅由新 actor 执行。主 UI、legacy UI 和 agent 继续共享相同 Application API 与 effects actor；本批无 UI 操作变更。
- 接口变化：`ApplicationEffectExecutor::new` 增加必需的 `SystemProxyClient`；`ChimeraClient` 为经确认的 Agent 动作持有同一 actor client；`EffectsClient::reconcile_effect` 提供等待并取得最新 owner 状态的入口；system-proxy actor 增加带端点校验的 stale-proxy 修复和失败动作恢复命令。setup 为代理 owner 创建独立 PAC cache 路径；启动与 legacy commit 的 runtime binding 通知现在是异步只读活动 core 查询。未改变 IPC、持久化 wire format、TUN 或 Service API。
- 风险与回滚：L2（系统代理、PAC、登录项和退出恢复均可产生宿主副作用）。回滚需恢复原 `Sysopt` 接线、effects executor/adapters 契约、setup 注入和本条记录；无用户配置迁移。当前未执行真实 OS adapter 写入。
- 独立复核：fresh-context review 指出 stale Agent 修复与系统代理偏好并发时可能恢复过期 OS 状态、配置 CAS 更新漏写 legacy Verge 镜像、缺少生命周期命令级断言、Agent 回滚未串行化，以及混合端口变更后未重新核对旧端点；另一处旧 IPC raw mapping 也可绕过 typed guard。复核还发现 Android/iOS 下 Agent 引用被 cfg 排除的系统代理类型、随机 mixed-port 将 fallback start port 误当作活动端口，以及 stale 回滚在验证失败时会覆盖后来由宿主外部设置的新代理。当前 diff 让 typed/legacy 代理偏好、typed/raw/legacy mixed-port 更新和 Agent 设置、验证、回滚及 stale 修复共享 client 互斥；生命周期命令在 OS 写入前核对 receipt 端口；CAS 冲突时发布最新偏好并清除旧运行端口，不恢复过期 OS 配置；stale 回滚先重新读取 OS 状态，只在状态仍为本次写入值时恢复；移动平台不提供不支持的 Agent 系统代理提案与执行；普通配置与 Agent 修复共用 legacy mirror 持久化函数。第二轮复核又发现 effects 将 controller 端口误当 mixed port，Agent 对 Random 策略仅读 Verge fallback；现改用 host 状态核对过的 runtime receipt，并增加 controller/mixed 与随机端口回归断言。最终复核另发现 Random 模式重启后没有可信历史端口，现将未知端口 fail-closed，不再拿易变的 Verge 镜像认领旧系统代理。另新增 stale-proxy 生命周期、互斥串行、raw mixed-port 判定及宿主并发变更回滚覆盖。第三轮 fresh-context 复核无 P0-P2 阻塞，并指出启用期间核心停止后的 OS 与偏好恢复尚无动作级回归断言。
- 最新复核补充：Random 与 AllowFallback 策略在无 receipt 时都不能确定重启后的监听端口，当前只对 Fixed 策略回退到配置端口；启用代理动作要求活动 receipt，并在 effect 后和最终快照确认核心仍运行。旧 `patch_clash_core_config` IPC 经过 `feat::patch_clash` 调用持有 mixed-port 互斥的 client writer，raw mapping guard 有回归断言。
- 验收计划与限制：fake OS proxy/autostart/PAC ports 覆盖 owner 的 proxy/PAC/guard/revision/shutdown、stale endpoint CAS 与恢复；executor 与 lifecycle 分别验证效果分发、活动端口绑定以及 Agent stale-proxy 生命周期。Agent action tests 覆盖 proposal/planning、最终状态校验和错误映射，但不执行完整动作编排，也没有可确定重现“启用期间核心停止并回滚 OS 与偏好”的动作级测试；不会把这些测试描述为真实 owner rollback 测试。真实主机代理恢复/登录项、UI 操作和 Service/TUN 需隔离 runner 验证；本机不通过生产 adapter 修改系统设置。移动平台交叉编译会尝试使用 Android target 验证 cfg 分支。
- 当前本地证据：最新后端 SHA 执行 `cargo fmt --manifest-path backend/Cargo.toml --package chimera -- --check`、`pnpm exec prettier --check docs/ref-alignment-progress.md docs/testing/contracts/application-effects-executor.md LESSONS.md`、`git diff --check`、`cargo check --locked --manifest-path backend/Cargo.toml -p chimera --lib`、`cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib -- --test-threads=1`（659 passed、0 failed、1 ignored）、`cargo clippy --locked --manifest-path backend/Cargo.toml --all-targets --all-features` 和 `pnpm e2e:tauri:build` 均通过。E2E 测试 follow-up 让非视觉 fetch smoke 选择不等待动画；`pnpm --filter @chimera/tauri-e2e typecheck` 和 macOS WebKit 定向 smoke（5/5）通过。完整 `pnpm --filter @chimera/tauri-e2e test:smoke` 为 2 个 spec 通过、2 个失败：proxy-localization 的 Add Profile 按钮 30 秒不可点击；legacy connections 诊断显示文档 hidden、页面动画 `currentTime=0`。这两个未在可见隔离 runner 复现，所以不称完整 E2E 通过。未实测真实系统代理、PAC、自启动或 Service/TUN；Android target 下载停滞后中止，移动 cfg 交叉编译未验证。

## DIFF-063：同步上游 ref 至 2026-10-29 main

- 目标与范围：按用户明确要求更新只读参考子模块；只移动父仓库的 `ref/` gitlink，不把上游产品源码复制进 Chimera，也不改变用户数据。
- 基线：更新前 `ref` 为 `9f72efe292975b13b5d9018a9f5641ffbb071c27`，工作区及递归子模块均干净。
- 更新：fetch 上游 `main` 后快进 23 个提交至 `8e1084e4e7c50531a5d16ca9a6c9dd2bb2f0f8e8`。范围包含 Service/runtime `2.0.2`、Windows `enable-loopback` 与 Windows SDK bootstrapper、Tauri `2.8.4`、updater/resource prepare、sparkline 和页面动画修复及依赖更新。
- 固定的递归子模块：`backend/enable-loopback` `f1b9f8693a328ca0a6094951035b81fe302b0848`；`vendor/windows-rs` `d1574fd27371a9b08bc5508f70c43ccb4f84c543`；`backend/nyanpasu-runtime` `e73b9f5cc2aa91ac74b4c71d0acc5b48e0fd2b34`；`nyanpasu-utils` `2706c1b69a3b5e4f0367fb54b4a87e67769c19f8`。
- 验证：`ref/HEAD` 等于 `ref/origin/main` `8e1084e4e7c50531a5d16ca9a6c9dd2bb2f0f8e8`；`git -C ref status --short` 为空；递归子模块无前缀 `+`/`-` 标记。此次只改参考基线，不运行 Chimera 产品构建或测试。
- 影响与回滚：父仓库 `ref` gitlink 从 `9f72efe292975b13b5d9018a9f5641ffbb071c27` 更新至 `8e1084e4e7c50531a5d16ca9a6c9dd2bb2f0f8e8`。回滚只恢复旧 gitlink；每个功能迭代仍需按固定新基线单独映射、复制、审查和验证。

## DIFF-064：窗口级 KV 状态与有序存储事件（L2）

- 目标：按最新 ref 统一前后端 KV 状态所有权；每个窗口的同 key hooks 共享一个快照，读取、事件和写入确认按 revision 收敛；存储事件广播 lag 后触发 resync 并继续转发。
- 非目标：不改变 redb 持久化数据格式、用户配置键或系统网络；不删除 legacy UI、Chimera Client/Service 或旧 storage event 消费者；不引入 ref 的统一 HTTP RPC 子系统。
- 参考基线：本机 `ref` 与 upstream `main` 均为 `8e1084e4e7c50531a5d16ca9a6c9dd2bb2f0f8e8`，`ref` 工作区干净；核心 KV 实现来自上游 `eb81fda5`，当前 ref 仍保留本批对应路径与符号。
- 参考映射：`ref/backend/tauri/src/core/storage.rs::{StorageRevision,StorageItem,StorageChange,StorageNotification,forward_storage_changes,StorageResyncEvent,StorageValueChangedEvent}` → `backend/tauri/src/core/storage.rs`；ref `backend/tauri/src/ipc.rs::{get_storage_item,set_storage_item,remove_storage_item}` → Chimera 同名命令；ref `backend/tauri/src/setup.rs` 的 shutdown/TaskTracker storage forwarder → Chimera `utils/resolve.rs::resolve_setup`、`setup::AppShutdown`；`ref/frontend/query/src/provider/{kv-storage-state.ts,kv-storage-provider.tsx}`、`hooks/use-kv-storage.ts`、`provider/index.tsx` → `frontend/interface/src/provider/` 与同名 hook/RootProvider。保留 ref 的 revision merge、window-scoped query cache、单 key write queue、resync、直接订阅 query cache 以避免 deferred notification race。
- Chimera 必要适配：直接使用项目生成的 Tauri `commands` / `events`，不引入 ref 的 `useQueryApi` 与 HTTP SSE event bus（Chimera 没有该统一 RPC 架构）；`getStorageItem` 服务层保留旧 string/null 返回形状；继续发出旧 `storage_value_changed` 与 `storage_resync_required` 事件，供 legacy Jotai `ChimeraStorage` 及既有 storage listener 使用。所有窗口的 typed event 由共享 `KvStorageProvider` 处理；legacy UI 仍用现有交互，但共用同一 hook/store。当前 agent 没有 KV hook 消费者，不新增平行存储 API。移除旧事件的条件是 legacy Jotai/storage-listener 消费者全部迁到 typed shared hook。
- 测试适配：上游 React browser suite 不可直接运行于 Chimera 当前 Node/Tauri 测试包；对应逻辑以可执行 `kv-storage-state` 与 `kv-storage-provider` Node tests 覆盖，并用 runtime suite 的 `storage-event-resync.e2e.ts` 验证真实桌面中的 legacy UI 更新。广播 lag 的恢复由 Rust 单测验证；桌面 E2E 未强制填满生产广播队列。ref `frontend/query/src/provider/kv-storage-provider.tsx::connect` 在 `Promise.allSettled` 后即使监听注册失败仍会 `markReady`；本地错误注入测试复现了未注册事件监听器时初始读取提前开始。Chimera 对缺失监听器按指数退避重试（最长间隔 30 秒），两类监听都注册成功前不释放首次读取；首次读取在监听就绪后直接取权威快照，因此初次恢复不额外重复 resync 读取。
- Chimera-only 安全扩展与复查条件：ref `backend/tauri/src/ipc.rs::set_storage_item` 是无条件写入；PR #532 的独立审查发现旧客户端迁移失败时可能覆盖新版 KV 值。Chimera hook 因此在写入队列出队时复核迁移状态，并将已观察到的全局 `StorageRevision` 传给可选的条件写入；后端在 revision 锁内比较后提交，过期写入失败并重读权威状态。旧 `ChimeraStorage` 调用传 `null` 保留既有行为。每次 ref 更新 storage IPC 或 revision 语义时复核是否可上游吸收相同 CAS 契约；若上游实现等价保护，再移除 Chimera 专有接口。另，ref provider 对监听注册错误没有重试/失败门控；Chimera 保留窗口级 hook/provider 结构，但加入监听恢复与首次读取门控，属于针对已复现 ref 缺陷的有界退避扩展，待 ref 修复后复核收敛。
- 接口及风险：`get_storage_item` 返回 `{value, revision}`，写入/删除返回 revision，storage change event 增加 revision，并注册 `storage-resync-event`；`set_storage_item` 增加可选 expected revision，既有服务层传 `null` 保持兼容。风险 L2（共享持久化、IPC 与跨窗口状态）。回滚为恢复旧 command/event payload 与 hook/provider；无需迁移或删除用户数据。
- 验证：`cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib core::storage::tests` 9/9、`ipc::tests` 16/16、`typescript_bindings_are_fresh` 1/1 和生成器 `update_typescript_bindings` 1/1 通过；`pnpm --filter chimera-ui test:dashboard-widget-config` 23/23（含监听注册失败恢复用例）、`pnpm typecheck`、`pnpm --filter chimera-ui build`、`pnpm lint:frontend-boundaries`、改动文件 Oxlint/Prettier、目标 Rust 格式检查与 `git diff --check` 通过。macOS WebKit 定向 E2E 的既有成功记录早于条件写/CAS 收敛，不作为当前集成 SHA 的证明。PR #548 初始 SHA `84eb3f0d3e5b86169465a61462718844f78cc4ba` 的 Windows E2E run `40864250817` 中 55/56 specs 通过；唯一失败是 `dashboard-main-layout.e2e.ts` 的辅助函数将 revisioned storage snapshot 当字符串传给诊断和恢复路径，未发现产品存储行为失败。已修正读取解包与 fixture 写入参数；本机 E2E binary rebuild 通过。尝试运行目标 Dashboard spec 时，macOS embedded WebKit 在 before hook 路由动画处停于 `currentTime=0`，未进入测试断言（见既有 LESSON-20261023-003）；因此不作为修复结果。新 SHA 的 Windows E2E 复验待推送。CI run `40864250803` 的三平台 lint 已通过，三平台应用构建仍在执行。
- 回滚和收敛：回退上述 Rust storage、IPC/bindings、interface provider/hook、E2E 和测试入口；保留既有 redb 文件和键值。后续在 legacy 消费者迁移后复查并移除兼容 tuple events；若 Chimera 引入 remote/HTTP frontend，再评估 ref 的 event bus/SSE transport，不在本批复制无对应主机架构的模块。

## DIFF-065：托盘 effect 快照与刷新队列（部分对齐）

- 目标：按当前固定 ref 为托盘 effect 携带完整、不可变的 `TrayView`，让 effects、legacy 事件入口和托盘 renderer 经同一串行队列刷新；合并尚未处理的更新时保留最新 view，Full 刷新覆盖 Part 刷新。
- 非目标：本批不重做 Chimera 托盘菜单/代理子菜单、图标绘制、菜单模式、平台交互或 legacy UI presentation；不修改持久化配置、IPC wire format、Client/Service 生命周期或宿主代理/TUN 状态。
- 基线：父仓库 `master` `456392d5a71a8184bdb4abc674005b25ab0401c2`；只读 `ref/` 为 `2fadf212f161417dcdf2f37d81a305a37ff918a4`，工作树干净。没有同步或修改 ref。
- 路径与符号映射：`ref/backend/tauri/src/client/effects/plan.rs::{TrayMenuDesired,TrayPartDesired,TrayView}`、`client/effects/actor.rs` 的 Profile partial refresh、`client/effects/adapters.rs` 与 `core/tray/mod.rs` → Chimera 同路径。保留类型和字段名；`ApplicationEffect::Tray` 增加完整 `TrayView`，actor 将最新 view 与 refresh 一起交给 adapter。Chimera 的 `Handle::update_systray_part` 及 legacy event sink 也改走同一个 tray refresh queue。通过 typed Clash mode accessor 形成 snapshot，不再在 renderer 中读取 runtime YAML。
- 部分范围与待收敛差异：独立复审确认队列的 merge/latest-view/scheduling 没有 P0–P2 逻辑问题；本批仍为 partial。DIFF-066 已让 full menu 与 Part repaint 消费 `view.menu.core`，仅为 Clash Premium 显示并勾选 Script mode。`view.menu.selector_mode` 与 `menu_mode` 仍未接入，代理子菜单仍未按 ref 构建；Part renderer 的 `text` 和 `traffic` 也尚未显示。固定 ref 的 `tray_menu(app_handle, &view)` 还按 selector/menu mode 配置代理菜单，并在 repaint 中应用 tray text 等值。该菜单/渲染差异不得视为永久产品例外；后续托盘渲染迁移需消费完整 view，并在保留 Chimera 菜单 presentation、Client/Service 和 legacy 入口的前提下核对对应 menu actions。当前这批不宣称完整 ref tray 行为对齐。
- 接口与风险：Rust 内部 effects adapter 契约由 `refresh_tray(refresh)` 变为 `refresh_tray(refresh, view)`；`ApplicationEffect::Tray` 由单 refresh 变为 refresh + snapshot。L2（共享 app/clash 状态驱动的 UI 托盘效果）；不改变配置存储或外部 IPC。回滚恢复旧 effect 变体、adapter 参数及原托盘刷新入口。
- 验收及实际验证：托盘 queue 单测验证 Full 覆盖 Part、并发排队期间使用最新 snapshot，2/2；`client::effects::` effects/plan/actor/executor 定向测试 43/43；typed mode accessor 1/1；Rust fmt、LESSONS 与本记录 Prettier、`git diff --check` 通过。纯单测没有执行 Tauri main-thread scheduler/native menu；本机未做桌面 E2E。独立审查使用新上下文复核了实际 diff、ref SHA、队列和测试；其唯一 P2 结论为上述未消费菜单字段，按本条将其作为明确的后续收敛项，故本批标记为部分迁移。

## DIFF-066：Tray 提供 Clash Premium 脚本模式菜单项

- 目标：按 `ref` 的托盘菜单行为，仅在 Clash Premium 核心下显示 Script mode，并让菜单勾选状态和现有核心模式切换入口一致。
- 非目标：本批不迁移代理组子菜单、WebView 菜单模式、复制代理环境变量、托盘图标/tooltip、text/traffic 显示或队列/发布状态机；不改变配置、IPC、Service 合同或宿主代理/TUN。
- 基线与映射：只读 `ref/` HEAD `2c017d63bfec720017a9b57db629eeddd2c9cd3d`，工作区干净。`ref/backend/tauri/src/core/tray/mod.rs::tray_menu/repaint_part` 对应 `backend/tauri/src/core/tray/mod.rs::tray_menu/render_part_with_view`；`ClashCore::ClashPremium`、`Mode::Script` 和 `tray.script_mode` 采用上游语义。Chimera 的选择动作沿用现有 `feat::change_clash_mode`。
- 影响接口与风险：只增加托盘原生菜单项和五种 Rust locale 的显示文案；无 IPC/持久化接口变化。L2（通过现有入口修改运行模式）。
- 验收：定向 Rust 测试覆盖菜单构建与刷新共用的模式项定义和勾选模型；格式检查与推送后 CI 负责跨平台编译/回归验证。原生菜单点击和勾选显示未由纯单测证明；未执行原生菜单桌面 E2E 时明确记为未验证。
- 本地验证：`cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib core::tray::tests -- --test-threads=1` 4/4；`cargo fmt --manifest-path backend/Cargo.toml --package chimera -- --check`、本记录与 LESSONS 的 Prettier、五个 locale JSON 解析、`git diff --check` 通过。原生托盘 E2E 未运行；这批新 SHA 的远程 CI/E2E 尚待推送后核验。
- 回滚：恢复托盘菜单构建、模式勾选和事件映射，并移除五个 locale 中的 `tray.script_mode` 字段。

## DIFF-067：对齐 Runtime 的复用 PID 启动清理（L2）

- 目标：按 ref 修复 Core Manager 启动时回收已退出核心的 epoch 记录；操作系统复用了记录中的 PID 时，应删除旧记录和配置，不得误杀现在持有该 PID 的其他进程。
- 非目标：不改变核心切换策略、Service IPC、系统代理/TUN、配置格式或 UI；不迁入 Meow 专属 native store 和其他不属于本故障的 Runtime 功能。
- 基线：父仓 `master` `d03b085a51ce3050356026b0e1e0ed9d04c16b4e`；开始实现时 runtime 子模块为 `ceee7f7df1d01b421fc75409ec667852b7cc7cda`，提交前已快进同步至 `Chimera_Service/main` `70285e9ea7f3d6f3a66cfd6d33ed1d2c98811175`（本批仅更新文件不相交的依赖/自动化提交）；只读 `ref/` `2c017d63bfec720017a9b57db629eeddd2c9cd3d`，其 runtime 为 `e73b9f5cc2aa91ac74b4c71d0acc5b48e0fd2b34`、`nyanpasu-utils` gitlink 为 `2706c1b69a3b5e4f0367fb54b4a87e67769c19f8`。三处基线均已核对，`ref/` 干净且未修改。
- 路径与符号映射：`ref/backend/nyanpasu-runtime/crates/nyanpasu-utils/src/process/pid_file.rs::{ProcessIdentity,wait_for_process_identity,compare_identity,reap_record}` → `backend/chimera-runtime/crates/chimera-utils/src/process/pid_file.rs`；`ref/.../nyanpasu-core-manager/tests/manager_orchestration.rs::manager_sweeps_a_stale_record_whose_pid_was_reused` → `backend/chimera-runtime/crates/chimera-core-manager/src/manager/mod.rs::tests::startup_sweeps_a_record_whose_pid_was_reused`。保留 `chimera-utils` / `chimera-core-manager` 包名及 Client/Service 共用 manager 路径。
- 接口与风险：无 IPC、Tauri 命令、前端类型或用户配置变化；macOS 的临时 epoch PID 文件新写为 v3，解析器继续读取旧 v2。v2 记录只有秒级身份：若当前 PID 的完整秒+微秒标记与该秒值对应，复用情况无法证明，清理会失败关闭且不发信号；精度更高的 v3 才能安全区分同秒复用。风险 L2（进程身份、崩溃恢复和 Runtime 临时记录）；旧版本 manager 不认识 v3，因此回滚到旧二进制时，若残留 v3 PID 记录，启动清理会失败关闭。此格式只用于短期 Runtime 记录，不迁移用户配置。回滚当前改动即可恢复原行为。
- 验收与实际验证：`cargo +nightly test --manifest-path backend/chimera-runtime/Cargo.toml -p chimera-utils -p chimera-core-manager --all-features` 在本机 macOS arm64 通过：core manager 117 passed、1 ignored；utils 31 passed；doc test 1 passed。覆盖同秒不同微秒的 PID 复用、v2 歧义 fail-closed 且不发信号、root-owned PID 的不可读可执行文件、非法 v3 微秒字段、v3 往返及 manager 启动清扫。`cargo +nightly clippy --manifest-path backend/chimera-runtime/Cargo.toml -p chimera-utils -p chimera-core-manager --all-targets --all-features`、workspace `cargo fmt --all -- --check`、Prettier 与两层 `git diff --check` 通过；Clippy 有该子模块既有警告。曾尝试对 `x86_64-unknown-linux-gnu` 与 `x86_64-pc-windows-gnu` 执行 `cargo check --tests`，但两次都在依赖 `aws-lc-sys` 的 C 构建阶段因缺少对应 GCC 交叉编译器而停止，未到项目 Rust 代码编译；因此 Linux/Windows 目标仍未验证。主仓 `pnpm test:backend` 排除此子模块，Service 工作流只运行 Clippy 而不执行测试；本批未改 CI 工作流。
- 入口、恢复与剩余验证：主 UI、legacy UI 与 Agent 均继续通过既有共享 Core Manager，无入口分叉。单元/集成断言在 Service runtime workspace 中可由上述 `cargo test` 命令执行；当前 CI 只编译测试目标，不运行它们。Windows/Linux 需在相应 runner 执行测试；CI 工作流的测试门禁属于另一项 L3 变更，本批未改。
