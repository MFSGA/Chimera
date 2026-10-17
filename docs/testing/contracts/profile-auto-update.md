# Remote profile auto-update contract

契约 ID：`profile-auto-update`

用例标题：在主界面关闭远程 Profile 自动同步，确认间隔落盘为 0、手动同步仍可用；重新启用时空白间隔恢复为 120 分钟，并从 legacy UI 保存 0。

风险/回归：零间隔被校验或迁移拒绝；调度器对零值仍创建周期任务或启动补刷；关闭自动同步后手动更新不可用；legacy 编辑器因远程文件只读而未保存同步选项；启用但留空时没有恢复默认周期。

边界：桌面 UI E2E + loopback 网络 E2E；Rust 单元测试单独验证调度器取消、启动补刷和手动刷新语义。

基础套件：隔离套件 `profile-auto-update-main`，通过 `pnpm --filter @chimera/tauri-e2e test:profile-auto-update` 运行。由于该契约要求应用以 main 窗口启动，它不进入默认 legacy-start 的 `profiles`、`critical`、`hermetic` 和 `all` 聚合；PR/定时 Windows E2E 在 hermetic 后通过独立应用会话运行它，也可在工作流手动运行中单独选择。

运行条件：Tauri E2E 桌面程序；默认配置与数据目录隔离；专用 fixture 将 `window_type` 设为 `main`，服务附着到启动时的 main 窗口；测试在 main UI 操作完成后再创建 legacy 窗口；系统代理和 TUN 在测试开始前均关闭；本地 loopback 可用。

真实依赖：legacy/main UI、真实 Tauri IPC、Profile actor、`profiles.yaml` 持久化、真实后端 subscription fetcher，以及测试进程内的 `127.0.0.1` HTTP 服务。

模拟与绕过：通过 `import_profile` IPC 准备唯一的远程 Profile，通过 `get_profiles` 只读观察，通过 IPC 清理；被测开关、保存、手动更新、legacy 表单输入和保存均由 WDIO 用户控件执行。loopback HTTP 内容为合成的 `proxies: []`，不访问公网。

初始状态：捕获当前 Profile；读取并确认系统代理与 TUN 关闭；创建唯一 loopback subscription 和远程 Profile，默认 interval 为 120。因为 fixture 由 IPC 建立，首次 UI 操作前重新加载 main window，使 Profile query 从持久层读取 fixture；此准备刷新不参与被测结果。

被测操作：在 main Profile 详情页关闭 Auto Update 并保存；点击 Manual Sync；重新开启 Auto Update、保持 interval 为空并保存；在 legacy `/profiles` 通过 Profile 菜单打开编辑器，将 interval 输入为 0 并保存。

权威结果：`get_profiles` 返回该 Profile 的数值间隔；独立读取隔离运行目录的 `config/profiles.yaml`，核对同一 UID 的落盘字段；loopback 服务请求计数增加证明关闭自动更新后手动同步仍会发起真实请求。

当前 UI 结果：关闭后开关显示未选中；重新打开时开关状态与保存值一致，空白间隔恢复为 120。每次关闭弹窗后不刷新页面，main 操作和 legacy 操作的读取状态都收敛到对应值。订阅卡片的下次计划时间由共享 `useProfileSyncStatus` 返回的 scheduler `next_run_at` 驱动，不再用最后更新时间和间隔在前端推算；本契约未单独验收 tooltip 悬停呈现，因为当前桌面驱动的合成 mousemove 不能证明 Radix Tooltip 的真实悬停行为。后台窗口会暂停 Motion 退场动画，因此本机 E2E 不断言间隔控件动画结束后的视觉折叠；这些覆盖限制见下文。

磁盘/刷新/冷启动：每次保存后检查隔离运行目录的 `profiles.yaml`；不刷新页面。未执行应用冷启动，因此不声称验证重启后的配置恢复。

失败或边界场景：若测试启动前系统代理或 TUN 已开启则立即失败且不修改宿主状态；所有订阅请求由本地服务器处理。测试没有等待两小时，因此“零值不会按周期自动刷新”由 Rust scheduler 单测覆盖，桌面 E2E 不作该时间跨度声明。

等待预算：loopback 监听 15 秒；页面、控件和状态收敛各最多 15 秒；手动 HTTP 刷新最多 30 秒；选择恢复及删除各最多 30 秒；`withCleanup` 保留主体错误并执行全部清理。

诊断：WDIO 最终错误、失败截图和页面源由当前 harness 管理；使用合成 Profile 名称和 loopback URL，不保存凭据或外部订阅地址。

资源与清理：关闭 loopback server；若 fixture Profile 变为 current，则恢复原 UID（或原来的 `null`）；删除唯一 Profile 并确认不存在。测试开始前检查代理/TUN，整个契约不更改这些宿主设置。清理步骤分别登记，主体异常不会跳过后续清理。

回归敏感性：后端拒绝 0、前端开关仍写正值、默认值丢失、手动同步控件失效、legacy 远程配置写入错误路径或 profile 文件未持久化时，分别在目标断言失败。

相关规则与例外：T01、T02、T03、T04、T05、T06、T07、T10、T14；没有原生鼠标可信事件或冷启动覆盖。本地系统代理/TUN 检查是宿主状态前置条件，loopback 请求不证明公网代理或 TUN 行为。

## 参考对齐

- ref commit：`c08aa9d9a17953aa2464cdae61390d5c3f9afbc7`
- ref 路径/符号：`backend/nyanpasu-config/src/profile/source.rs` 的 `RemoteSyncInterval`、`backend/tauri/src/state/profiles/actor.rs` 的 scheduled refresh 处理、`frontend/nyanpasu/src/pages/(main)/main/profiles/$type/detail/_modules/update-option-editor.tsx`、`frontend/rpc/src/rpc-bindings.ts` 中的 `RemoteSyncInterval = number`。
- Chimera 映射：`backend/chimera-config/src/profile/source.rs`；`backend/tauri/src/state/profiles/scheduler.rs` 与 `client/profiles.rs`；main UI 对应 `frontend/chimera/src/pages/(main)/main/profiles/$type/detail/_modules/update-option-editor.tsx`；保留 legacy 表单适配 `frontend/chimera/src/components/profiles/profile-dialog.tsx`；契约 spec 为 `tauri-e2e/specs/profile-auto-update.e2e.ts`。
- 窗口可见性限制：本机嵌入式 WebKit E2E 即使以 main 为初始窗口，`document.visibilityState` 仍为 `hidden` 且 `document.hasFocus()` 为 false；因此本地结果不证明前台显示或原生输入。Windows CI 曾在 legacy-first 的 hermetic 会话中于 main UI 点击 Auto Update 后丢失弹窗/开关；独立 main-first 会话隔离了窗口启动上下文，但只有更新后的 Windows CI 能验证这是否解决该失败。桌面 spec 保留真实 WDIO 控件操作，并以开关 `aria-checked`、IPC 状态、`profiles.yaml` 落盘和 loopback 请求验证业务结果；WDIO 点击不代表可信原生鼠标事件，前台像素/输入仍未覆盖。
- 执行记录（2026-10-15，macOS Apple Silicon，WebKit 605.1.15）：
  - 桌面构建：`AWS_LC_SYS_PREBUILT_NASM=1 CARGO_TARGET_DIR=<external-target> TAURI_CONFIG='{"build":{"devUrl":null},"identifier":"com.chimera.e2e.local"}' cargo build --manifest-path ./backend/tauri/Cargo.toml --features e2e`；本地专用 app ID 用于隔离正在运行的已安装 Chimera 缓存。二进制 SHA-256：`0abdbf79d839e658974b4fe33ac15b0a581122779dba3b4a799a30cd79244e90`。
  - PR #482 worktree 自有 Cargo target 恢复构建：`AWS_LC_SYS_PREBUILT_NASM=1 CARGO_TARGET_DIR=<this-worktree>/backend/target/e2e TAURI_CONFIG='{"build":{"devUrl":null},"identifier":"com.chimera.e2e.local"}' cargo build --locked --manifest-path ./backend/tauri/Cargo.toml --features e2e` 退出 0；新二进制 SHA-256 为 `dd0c94f945119e9ce5e72839585deaf96349f36fa155f4a0200bdb082383d8ad`。这次只在 PR #482 自己的 worktree 中复用 target。
  - 桌面命令：`CHIMERA_E2E_BINARY=<local-e2e-binary> CHIMERA_E2E_RUNTIME_DIR=<isolated-runtime> CHIMERA_E2E_ARTIFACT_DIR=<isolated-artifacts> TMPDIR=<external-temp> pnpm --filter @chimera/tauri-e2e exec wdio run ./wdio.conf.ts --suite profile-auto-update-main --logLevel error`。结果 1/1 通过，15.3 秒。本机运行验证 main-first 会话、Profile 状态与持久化、legacy 窗口操作及 loopback 请求；不证明前台像素、原生输入或其他平台。
  - Rust：`cargo test --locked --manifest-path backend/Cargo.toml -p chimera-config profile`、`cargo test --locked --manifest-path backend/tauri/Cargo.toml --lib scheduler_stops_refreshing_when_interval_is_patched_to_zero`、`cargo test --locked --manifest-path backend/tauri/Cargo.toml --lib disabled_auto_sync_skips_startup_catch_up_but_allows_manual_refresh`、`cargo test --locked --manifest-path backend/tauri/Cargo.toml --lib preserves_disabled_remote_sync_interval_in_runtime_adapter`、`cargo test --locked --manifest-path backend/tauri/Cargo.toml --lib remote_option_zero_interval_migrates_as_disabled_auto_sync` 与 `cargo test --locked --manifest-path backend/tauri/Cargo.toml --lib import_keeps_explicit_zero_interval_as_auto_update_off` 均通过。
  - 前端与 harness：本批最新检查 `pnpm --filter @chimera/tauri-e2e typecheck`、`pnpm --filter @chimera/tauri-e2e test:unit`（25/25，外接盘临时目录）、Prettier、`actionlint .github/workflows/e2e.yaml` 和 `git diff --check` 均通过。既有 Profile 实现检查 `pnpm typecheck:chimera`、`pnpm test:manifest`（8/8）、`pnpm lint:frontend-boundaries`、`pnpm lint:oxlint`、`pnpm prepare:check` 也通过。
  - 独立复核：profile 变更的独立审查无阻塞项；复核者未代跑 Rust 或桌面测试，以上执行记录由实现者实际运行所得。该首次本地验证发生时改动尚未提交或推送，因此当时没有远端 CI 结果；后续远端结果见下方。
  - 覆盖限制：前台像素/动画状态、冷启动恢复、周期自动请求的真实等待、真实公网及系统代理/TUN 未验证；周期调度语义由 Rust 定向测试覆盖，桌面请求仅到隔离 loopback 服务。不得将这些未覆盖项表述为已通过。

- 远端 PR #482 提交 `63907201927bddf9deaeafb1ec597c11d1925177` 的 Windows hermetic E2E 于 2026-10-15 失败：在 main UI 打开订阅选项并点击 Auto Update 后，等待未找到仍挂载的开关（`aria-checked` 最后观测为 `null`）；其他 hermetic spec 通过。失败发生在保存之前，不能据此认定后端零间隔保存有误。当前改动将该 spec 移到 main-first 隔离 suite，并在后半段创建 legacy 窗口；新 SHA 的 Windows 结果会由 PR/定时工作流执行该隔离 suite 验证，失败根因仍未确认。
- 同一提交的 macOS CI 在 `pnpm prepare:check` 下载 `chimera-client-alpha` 时收到 404；Ubuntu lint 和 Windows lint 通过。本地此前重跑 `pnpm prepare:check` 成功，符合滚动 Alpha 资产短暂不可用的表现，仍需修复或取得新 SHA 上的远端通过结果。
- 为在清理 fixture 前保留 Windows 失败状态，测试等待 helper 现会附带 URL、弹窗状态、开关状态和活动元素快照，并在 CI 提供的 artifact 目录保存点击失败时的即时截图。本机 WebKit 的该 spec 在加入诊断后再次通过 1/1；Windows 结果须在新提交的 CI 中重新验证。
- 2026-10-15，PR #482 当前 head `a5db2ed509b6510b31c159c3625f95f8710d4b14` 的 Windows isolated suite 再次失败：WDIO 点击 main 详情页的 update-options trigger 后，15 秒内没有观察到打开的 modal；失败在 switch、保存和 legacy UI 操作之前。run `38669802865` 的最终截图是在 `withCleanup` 删除测试 Profile 后采集的，显示空 Profile 列表，不能据此判断点击时的页面状态。当前修复保留 WDIO `.click()`，并在清理前记录 URL、modal 状态、活动 slot 和即时截图。失败根因尚未确认；本机 macOS WebKit 新代码运行 1/1，Windows 新 SHA 复验待完成。页面上下文 `HTMLElement.click()` 曾用于本地诊断但已丢弃，因为它改变了失败中的 WebDriver 交互，不能作为 Windows 用户操作失败的等价修复。
- PR #482 最新测试文件在上述 branch-local 重建二进制 `dd0c94f9` 上重新运行，`profile-auto-update-main` 仍为 1/1 通过（14.4 秒）；这不能替代 Windows 验证。
- PR #482 head `46780c445fe7b3685806c8f43125728905ed9b0b` 的 Windows isolated suite 于 2026-10-15 在 `openMainUpdateOptions` 等待主 UI 更新选项按钮时失败（run `38674429849`）；失败发生在点击、开关、保存及 legacy UI 操作之前。自动截图是在 fixture 清理后采集，显示空 Profile 列表，不能还原失败现场。
- 代码复核发现路由 helper 只等待任意 `profile-update-options-trigger` 存在，而实际点击也选择第一个同名 trigger；Profile 页面使用退出/进入动画，因此旧详情可能仍挂载但不可见。当前修复以本次 fixture 的 Profile UID 关联详情和按钮，只等待该详情中的按钮可见，并在 fixture 清理前采集 UID、路由、可见性状态及截图。这个根因判断仍待 Windows 新 SHA 的结果验证；本机 macOS WebKit 的旧二进制不能验证新增选择器。
- 这次修复的本地检查：`pnpm typecheck:chimera`、`pnpm --filter @chimera/tauri-e2e typecheck`、`pnpm --filter @chimera/tauri-e2e test:unit`（25/25）、`pnpm lint:frontend-boundaries`、改动文件 oxlint/Prettier 和 `git diff --check` 通过。没有本地重跑桌面 spec：现有 E2E 二进制早于 UID 选择器改动，且该 worktree 的 Cargo target 已有 11 GiB 构建缓存；为避免覆盖现有构建，桌面验证留给本次推送后的 Windows suite。
- 2026-10-15 fresh-context 独立复核检查了 PR #482 全量改动及新诊断，没有发现可操作问题；复核者未执行测试。该诊断改动的 Prettier、E2E TypeScript 类型检查及本机 WebKit 单 spec 均由实现者实际运行并通过。
- PR #486 head `8d6a6dfb9e2d0ec19bde94f8dd77a2eeb6e9c78d` 的 Windows Edge isolated run `38947529850` 在 legacy 窗口等待 `[data-slot="legacy-profile-update-interval"] input` 时失败。该 run 的自动截图在 `withCleanup` 删除测试 Profile 后生成，不能还原点击编辑菜单时的页面状态；日志未记录 live DOM，因此根因尚未证实。
- 后续修复针对一个可由源码确认的选择器风险：`ProfileItem` 将菜单 portal 渲染到行外，而测试使用全局首个 `profile-edit-menu-item`；等待存在并不保证选择的是唯一可见菜单项。测试现改用 `displayedElement` 选择唯一可见项，等待 legacy name/interval 字段，并确认编辑器身份与期望 interval；legacy 菜单、表单或身份断言任一失败时，都会在清理前记录 URL、目标卡片、菜单/字段状态和截图。Windows 新 SHA 仍需复验后才能确认根因与修复效果。
- 本机验证：E2E harness TypeScript 检查通过，已注册的 unit tests 25/25 通过，Prettier 与 `git diff --check` 通过。尝试桌面复验时使用了旧 PR #481 二进制 SHA-256 `7888ea34b66321f7e2255097507b27fe3be0b388ac352b6f8773ba9bbe0df0e6`；它在测试导入阶段因 `committed_degraded` subscriber ACK 失败，早于 legacy UI 流程。此运行不验证本次修改；其隔离 runtime/artifacts 已清理。
