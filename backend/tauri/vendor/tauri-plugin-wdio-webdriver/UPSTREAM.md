# Vendored WebDriver plugin

This is `tauri-plugin-wdio-webdriver` 1.4.0 from crates.io, SHA-256:

```text
52c5e97428174a52b7f4357bbc4535fcce52c6a06c8ee7a379502f7cda1b6045
```

The Windows compatibility changes in `Cargo.toml` and `src/platform/windows.rs` match upstream commit [`e4bdb66ec5b1d96b7fd766d70370a8e8361e0459`](https://github.com/webdriverio/desktop-mobile/commit/e4bdb66ec5b1d96b7fd766d70370a8e8361e0459). That fix aligns the plugin with Tauri 2.12's `webview2-com` 0.39 types.

The only other local edit removes trailing whitespace from three comments in `src/platform/linux.rs` so repository diff checks pass.

Keep this copy only until a crates.io release includes that upstream fix. Then replace the path dependency with the released version and remove this directory after Windows E2E build validation.
