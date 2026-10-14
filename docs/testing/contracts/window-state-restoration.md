# Window state persistence and restoration contract

契约 ID：`window-state-restoration`

## 持久化捕获

`tauri-e2e/specs/session-typed-state.e2e.ts` 中的第一个用例通过 WebDriver 调整真实 Tauri 窗口，调用保存命令，并分别读取专属 E2E runtime 下的 `session-state.yaml` 与 `get_verge_config` 返回值。它确认捕获的宽、高、位置和窗口标志均有效，session 文件存在，并且页面刷新后 legacy 镜像仍保留宽、高。这个用例证明尺寸被捕获和持久化，不证明新窗口恢复了完全相同的高度。

## 窗口生命周期

第二个用例先关闭可能残留的主窗口，记录 legacy 窗口矩形并保存第一组几何状态；随后创建主窗口，检查新窗口句柄、位置、宽度和窗口标志。接着改变位置和宽度，再关闭主窗口并从 session 文件和 legacy 兼容镜像等待新状态收敛；没有保存更新时等待失败。第二次创建主窗口后再次检查位置、宽度和窗口标志。窗口创建和状态捕获通过 IPC。Windows/Linux 通过应用关闭控件关闭窗口；macOS 的关闭按钮位于 WebView 外部的原生标题栏，当前 embedded WebDriver 无法操作它，因此 macOS 分支显式保存状态后由 WebDriver 销毁窗口，只验证保存后的恢复，不声称覆盖原生 `CloseRequested` 保存路径。清理会关闭测试创建的主窗口并恢复原 legacy 矩形。此覆盖不代表应用冷启动持久化。

高度不属于这个生命周期用例的通过条件。Windows 原生最小客户区会按屏幕 DPI 调整实际高度，WebView 报出的缩放比例也不足以给所有窗口创建路径提供稳定的跨平台高度预期。先前 Windows CI 观察到保存高度 691px、恢复后捕获高度 721px；加入基于缩放比例的 2px 容差后仍在另一轮完整 E2E 失败（[run 38327471065](https://github.com/MFSGA/Chimera/actions/runs/38327471065)）。因此该用例完全不比较恢复前后的高度，也不以高度变化等待关闭后的持久状态。它仍通过单独的持久化捕获用例检查高度写入、镜像和刷新后读取。

这项边界意味着 E2E 可以发现主窗口没有创建、关闭没有保存、位置或宽度丢失、窗口标志变化等问题；它不会声称精确验证窗口高度恢复。只有产品需求明确要求跨平台验证恢复高度，并且能够从原生窗口 API 得到稳定的最小尺寸与缩放依据时，才应扩展这个契约。

## Ref 映射

Ref 基线为 `def0dbf2716ff972255be47924401b60db7d482c`。窗口捕获和恢复映射到 `ref/backend/tauri/src/window.rs` 的 `AppWindow::capture_state`、`MainWindow::get_window_state` 与 `save_main_window_state_async`；Chimera 对应 `backend/tauri/src/window.rs` 的 `capture_window_state`、`restore_window_state`、`persist_window_state`。双方都保存窗口矩形和状态；本契约对高度的限制是 E2E 断言边界，不改变产品恢复行为。
