# Remote profile auto-update contract

契约 ID：`profile-auto-update`

用例标题：在主界面关闭远程 Profile 自动同步，确认间隔落盘为 0、手动同步仍可用；重新启用时空白间隔恢复为 120 分钟，并从 legacy UI 保存 0。

风险/回归：零间隔被校验或迁移拒绝；调度器对零值仍创建周期任务或启动补刷；关闭自动同步后手动更新不可用；legacy 编辑器因远程文件只读而未保存同步选项；启用但留空时没有恢复默认周期。

边界：桌面 UI E2E + loopback 网络 E2E；Rust 单元测试单独验证调度器取消、启动补刷和手动刷新语义。

基础套件：`profiles`；由 `critical`、`hermetic` 和 `all` 套件包含。PR 工作流运行 `hermetic`，但本地必须另行记录实际执行的 spec。

运行条件：Tauri E2E 桌面程序；默认配置与数据目录隔离；legacy 与 main 窗口均可用；测试先将 legacy WebDriver 窗口设为 1240×760，再创建 main 窗口，main 的实际视口尺寸未单独记录；系统代理和 TUN 在测试开始前均关闭；本地 loopback 可用。

真实依赖：legacy/main UI、真实 Tauri IPC、Profile actor、`profiles.yaml` 持久化、真实后端 subscription fetcher，以及测试进程内的 `127.0.0.1` HTTP 服务。

模拟与绕过：通过 `import_profile` IPC 准备唯一的远程 Profile，通过 `get_profiles` 只读观察，通过 IPC 清理；被测开关、保存、手动更新、legacy 表单输入和保存均由 WDIO 用户控件执行。loopback HTTP 内容为合成的 `proxies: []`，不访问公网。

初始状态：捕获当前 Profile；读取并确认系统代理与 TUN 关闭；创建唯一 loopback subscription 和远程 Profile，默认 interval 为 120。因为 fixture 由 IPC 建立，首次 UI 操作前重新加载 main window，使 Profile query 从持久层读取 fixture；此准备刷新不参与被测结果。

被测操作：在 main Profile 详情页关闭 Auto Update 并保存；点击 Manual Sync；重新开启 Auto Update、保持 interval 为空并保存；在 legacy `/profiles` 通过 Profile 菜单打开编辑器，将 interval 输入为 0 并保存。

权威结果：`get_profiles` 返回该 Profile 的数值间隔；独立读取隔离运行目录的 `config/profiles.yaml`，核对同一 UID 的落盘字段；loopback 服务请求计数增加证明关闭自动更新后手动同步仍会发起真实请求。

当前 UI 结果：关闭后开关显示未选中；重新打开时开关状态与保存值一致，空白间隔恢复为 120。每次关闭弹窗后不刷新页面，main 操作和 legacy 操作的读取状态都收敛到对应值。后台窗口会暂停 Motion 退场动画，因此本机 E2E 不断言间隔控件动画结束后的视觉折叠；该限制见下文。

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
- 窗口可见性限制：本机嵌入式 WebKit E2E 运行期间，main 窗口是后台窗口，`document.visibilityState` 为 `hidden`；Motion 的退出动画因此未完成。桌面 spec 用真实 WDIO 控件操作与开关 `aria-checked`、IPC 状态、`profiles.yaml` 落盘和 loopback 请求验证业务结果，但不把后台窗口的动画/像素状态称为可见性验证；实际用户前台显示仍待前台桌面运行覆盖。
- 执行记录（2026-10-14，macOS Apple Silicon，WebKit 605.1.15）：
  - 桌面命令：`CHIMERA_E2E_BINARY=/Volumes/ChimeraBuild/chimera-profile-sync/backend/target/e2e/debug/chimera CHIMERA_E2E_ARTIFACT_DIR=/tmp/chimera-profile-auto-update-final pnpm --filter @chimera/tauri-e2e exec wdio run ./wdio.conf.ts --spec ./specs/profile-auto-update.e2e.ts`。结果 1/1 通过，约 14 秒；6 项 harness diagnostics 通过。运行二进制 SHA-256：`95837addcc47ed1835570a11a85b00a923add4f13adb22b8b603102e33409093`。
  - Rust：`cargo test --locked --manifest-path backend/Cargo.toml -p chimera-config profile`、`cargo test --locked --manifest-path backend/tauri/Cargo.toml --lib scheduler_stops_refreshing_when_interval_is_patched_to_zero`、`cargo test --locked --manifest-path backend/tauri/Cargo.toml --lib disabled_auto_sync_skips_startup_catch_up_but_allows_manual_refresh`、`cargo test --locked --manifest-path backend/tauri/Cargo.toml --lib preserves_disabled_remote_sync_interval_in_runtime_adapter`、`cargo test --locked --manifest-path backend/tauri/Cargo.toml --lib remote_option_zero_interval_migrates_as_disabled_auto_sync` 与 `cargo test --locked --manifest-path backend/tauri/Cargo.toml --lib import_keeps_explicit_zero_interval_as_auto_update_off` 均通过。
  - 前端与 harness：`pnpm typecheck:chimera`、`pnpm --filter @chimera/tauri-e2e typecheck`、`pnpm --filter @chimera/tauri-e2e test:unit`（25/25）、`pnpm test:manifest`（8/8）、`pnpm lint:frontend-boundaries`、`pnpm lint:oxlint`、`pnpm prepare:check`、Prettier 和 `git diff --check` 均通过。
  - 独立复核：profile 变更的独立审查无阻塞项；复核者未代跑 Rust 或桌面测试，以上执行记录由实现者实际运行所得。该首次本地验证发生时改动尚未提交或推送，因此当时没有远端 CI 结果；后续远端结果见下方。
  - 覆盖限制：前台像素/动画状态、冷启动恢复、周期自动请求的真实等待、真实公网及系统代理/TUN 未验证；周期调度语义由 Rust 定向测试覆盖，桌面请求仅到隔离 loopback 服务。不得将这些未覆盖项表述为已通过。

- 远端 PR #482 当前提交 `63907201927bddf9deaeafb1ec597c11d1925177` 的 Windows hermetic E2E 于 2026-10-15 失败：在 main UI 打开订阅选项并点击 Auto Update 后，等待未找到仍挂载的开关（`aria-checked` 最后观测为 `null`）；其他 hermetic spec 通过。失败发生在保存之前，不能据此认定后端零间隔保存有误。
- 同一提交的 macOS CI 在 `pnpm prepare:check` 下载 `chimera-client-alpha` 时收到 404；Ubuntu lint 和 Windows lint 通过。本地此前重跑 `pnpm prepare:check` 成功，符合滚动 Alpha 资产短暂不可用的表现，仍需修复或取得新 SHA 上的远端通过结果。
- 为在清理 fixture 前保留 Windows 失败状态，测试等待 helper 现会附带 URL、弹窗状态、开关状态和活动元素快照，并在 CI 提供的 artifact 目录保存点击失败时的即时截图。本机 WebKit 的该 spec 在加入诊断后再次通过 1/1；Windows 结果须在新提交的 CI 中重新验证。
- 2026-10-15 fresh-context 独立复核检查了 PR #482 全量改动及新诊断，没有发现可操作问题；复核者未执行测试。该诊断改动的 Prettier、E2E TypeScript 类型检查及本机 WebKit 单 spec 均由实现者实际运行并通过。
