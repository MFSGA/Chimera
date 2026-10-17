# Main rules sidebar tooltip contract

## Contract: focused collapsed proxy item exposes the reference tooltip

- **Boundary:** Windows desktop UI E2E. The test exercises programmatic focus-
  triggered tooltip behavior on the main Rules route; it does not exercise
  keyboard navigation, network, IPC, native pointer input, or real mouse hover.
- **Suite:** `main`, executed by the PR `hermetic` suite.
- **Initial state:** Open `/main/rules` in the main window, set the application
  language to `zh-cn`, keep the sidebar collapsed, and use a 1240×638 window.
  The stable `rules-proxy-item` slot and `data-proxy-item-kind="all"` identify the
  all-proxies link.
- **Operation:** Call the link's native `HTMLElement.focus()` method through
  WebDriver execute after asserting it begins unfocused. This exercises the
  browser focus event and Radix tooltip response. It does not verify Tab order
  or keyboard delivery from a physical keyboard.
- **Expected result:** While that link remains the active element, the tooltip
  portal shows the same label and its measured position is to the right of the
  trigger. A missing tooltip, mismatched label, or incorrect side/geometry
  fails the assertion.
- **Independent result:** Read `document.activeElement`, the selected link's
  text and rectangle, and the visible `[role="tooltip"]` text, side, and
  rectangle from the rendered document.
- **Regression sensitivity:** A missing focus trigger, wrong tooltip content,
  or wrong placement fails without relying on an array-first anchor selector.
- **Known coverage limit:** This verifies focus-triggered tooltip behavior only.
  The project's vendored desktop driver dispatches synthetic keyboard events
  without the browser's default Tab traversal, and maps pointer movement to
  synthetic `mousemove`; it cannot establish Tab order or genuine pointer and
  mouse-hover behavior. See [testing upstream evidence U04](../upstream-evidence.md)
  and [U08](../upstream-evidence.md#u08-tauri-driver-的键盘输入与焦点边界).
- **Cleanup:** No product state is written. The next test sets its own route,
  language, and window dimensions.
- **Rules:** T01–T05, T07, T13.
