/// Tells WebView2 whether the window is in sight. Its controller otherwise
/// remains visible while the native window is minimized.
#[cfg(windows)]
pub(super) fn set_in_sight(window: &tauri::WebviewWindow, in_sight: bool) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        ICoreWebView2_19,
    };
    use windows_core::{BOOL, Interface};

    let level = if in_sight {
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
    } else {
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
    };
    let _ = window.with_webview(move |webview| unsafe {
        let controller = webview.controller();
        let mut visible = BOOL::default();
        if controller.IsVisible(&mut visible).is_ok() && visible.as_bool() == in_sight {
            return;
        }
        let _ = controller.SetIsVisible(in_sight);
        if let Ok(core) = controller.CoreWebView2()
            && let Ok(core19) = core.cast::<ICoreWebView2_19>()
        {
            let _ = core19.SetMemoryUsageTargetLevel(level);
        }
    });
}

#[cfg(not(windows))]
pub(super) fn set_in_sight(_window: &tauri::WebviewWindow, _in_sight: bool) {}
