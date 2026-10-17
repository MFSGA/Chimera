# Dashboard widget configuration retry contract

- Contract ID: `dashboard-widget-config-retry`.
- Boundary: desktop UI E2E through the Dashboard, Tauri KV IPC and persistent storage. The test injects a bounded IPC failure to exercise the application's recovery path.
- Suite and execution: `dashboard-main-layout.e2e.ts`, registered in `main` and therefore executed by the Windows `hermetic` PR suite.
- Initial state: open the main Dashboard, read the existing `dashboard-widget-configs` value, and retain it for cleanup.
- Failure operation: replace the page's Tauri `invoke` function and fail Dashboard `get_storage_item` requests until the `load-error` state is observed. Release the injected failure, exit edit mode and use the visible Retry control; the retry must succeed.
- Save failure: inject one `set_storage_item` failure, change the Traffic Down unit through the visible configuration menu, assert the error state and verify the stored backend value remains unchanged. Retry from the visible control and verify both the rendered value and stored value change.
- Persistence: after observing the live UI and backend value, refresh the page and verify the selected unit is rendered again. Restore the original stored string and verify it before final refresh.
- Driver boundary: the vendored Tauri WebDriver synthesizes an untrusted `pointerdown` on pointer presses, then the corresponding mouse event; after a matched right-button press/release it emits a bubbling, cancellable `contextmenu` event. The `pointerdown` is required because the shared context-menu provider records the target in a document capture listener before resolving registered Dashboard menu items. This verifies the app's synthetic event handling and following UI action; it does not prove trusted native mouse input or a complete PointerEvent sequence. Recheck this boundary when upgrading the vendored driver.
- Diagnostics and cleanup: on failure, save screenshot, page source and error under the configured E2E artifact directory before cleanup. Restore the original KV value even after a failed assertion; aggregate the primary and cleanup errors if both fail.
- Recovery coverage: a failure to read must expose Retry; a failed write must preserve the old backend value; a successful retry must converge in the current UI and storage; refresh must reload the persisted value.
