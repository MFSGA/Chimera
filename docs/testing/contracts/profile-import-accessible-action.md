# Profile 远程导入动作可访问名称测试契约

- 契约 ID：`profile-import-accessible-action`
- 边界：Windows Edge 桌面 UI E2E；从 Profiles 页面点击打开导入菜单，再检查远程导入动作可见并具有本地化 `aria-label`。
- 基础套件：`main`，由 PR 的 `critical` 套件执行。
- 锁定工具：WebdriverIO `9.31.7`、`@wdio/tauri-service` `1.4.0`、vendored `tauri-plugin-wdio-webdriver` `1.4.0`（兼容补丁 `e4bdb66`）。
- 初始状态：主窗口位于 Profiles 页面，导入菜单关闭。
- 被测操作：通过可见的导入按钮打开菜单一次。
- 权威结果：`data-slot="profile-import-remote-action"` 的按钮处于可见状态，且具有非空 `aria-label`。
- 降级原因：锁定的桌面驱动把 W3C `PointerAction::Move` 变为合成 `MouseEvent("mousemove")`，不发送 `PointerEvent("pointermove")`；Radix Tooltip 的原生鼠标悬停行为无法由该路径可靠验收。源码依据见[U04 测试依赖上游依据](../upstream-evidence.md)。
- 替代验证：通过实际 UI 点击打开菜单，并验证远程动作及其可访问名称。
- 明确未覆盖：真实鼠标悬停后显示 Tooltip 的行为和 Tooltip 的视觉呈现。不得把该测试描述为 Tooltip E2E。
- 关联任务：PR #427。
- 移除条件：当锁定驱动在 Windows Edge 上能以可复现的最小用例触发真实 `PointerEvent` 悬停，并且该用例验证远程动作显示对应 Tooltip 后，恢复 Tooltip 断言并删除本例外记录。
