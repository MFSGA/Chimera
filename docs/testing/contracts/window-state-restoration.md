# Window state restoration contract

契约 ID：`window-state-restoration`

用例：`tauri-e2e/specs/session-typed-state.e2e.ts` 中的 “restores window size and position across UI creation and the app close control”。

风险是新建主窗口或关闭主窗口时没有保存位置和大小，或重开后丢失已保存状态。用例通过 WebDriver 调整真实 Tauri 窗口，读取专属 E2E runtime 下的 `session-state.yaml`，并通过 `get_verge_config` 独立核对 legacy 兼容镜像；主窗口关闭操作使用 UI 按钮，创建窗口和捕获状态使用 IPC。此覆盖不代表应用冷启动持久化。

用例先关闭可能残留的主窗口并记录 legacy 窗口矩形，再设置第一组矩形、读取持久状态、创建主窗口并核对恢复值。随后通过第二组矩形和主窗口关闭按钮触发保存，读取更新后的状态，重新创建主窗口并检查恢复结果。等待只读取窗口句柄和两份状态，不在轮询中重复创建、保存或关闭窗口。清理恢复原窗口矩形并再次核对状态。

Ref 基线为 `def0dbf2716ff972255be47924401b60db7d482c`。窗口捕获和恢复映射到 `ref/backend/tauri/src/window.rs` 的 `AppWindow::capture_state`、`MainWindow::get_window_state` 与 `save_main_window_state_async`；Chimera 对应 `backend/tauri/src/window.rs` 的 `capture_window_state`、`restore_window_state`、`persist_window_state`。两边都以物理客户区尺寸记录状态，并按恢复窗口的屏幕缩放比例应用最小尺寸。

Windows 上恢复窗口的原生最小尺寸可能高于已保存高度。实测 CI 中 session 保存高度为 691px；重开时当前 WebView 缩放比例为 1.2，600px 的配置最小高度折算为 720px，Windows 捕获值为 721px。用例因此要求位置和宽度精确恢复，高度不得低于已保存值或当前缩放后的最小高度，也不得高出该预期超过 2px。该容差只覆盖观测到的最小尺寸钳制和 1px 原生舍入，不接受任意窗口尺寸差异。

若 Tauri/WebDriver 提供稳定的原生 scale factor 读取，或测试能在所有目标屏幕尺寸下确保保存尺寸高于窗口最小值，则可移除此边界并恢复高度精确相等断言。若最小尺寸策略改变，应重新核对 ref 并更新此契约。
