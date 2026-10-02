use anyhow::{Result, anyhow};
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

use crate::config::{Config, chimera::WindowState};

#[cfg(windows)]
const WEBVIEW2_BROWSER_ARGS: &str = "--enable-features=msWebView2EnableDraggableRegions --disable-features=OverscrollHistoryNavigation,msExperimentalScrolling";
const RESTORE_PLACEHOLDER_SIZE: (f64, f64) = (800.0, 800.0);

#[derive(Debug, Clone)]
pub struct WindowConfig {
    /// Whether only one instance of this window type is allowed
    pub singleton: bool,
    /// Whether the window should be visible when created
    pub visible_on_create: bool,
    pub default_size: (f64, f64),
    pub min_size: Option<(f64, f64)>,
    pub center: bool,
    pub resizable: bool,
    pub always_on_top: Option<bool>,
    pub decorations: Option<bool>,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            singleton: true,
            visible_on_create: true,
            default_size: (800.0, 636.0),
            min_size: Some((400.0, 600.0)),
            center: true,
            resizable: true,
            always_on_top: None,
            decorations: None,
        }
    }
}

impl WindowConfig {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set whether only one instance is allowed
    pub fn singleton(mut self, singleton: bool) -> Self {
        self.singleton = singleton;
        self
    }

    /// Set whether window is visible on creation
    pub fn visible_on_create(mut self, visible: bool) -> Self {
        self.visible_on_create = visible;
        self
    }

    pub fn default_size(mut self, width: f64, height: f64) -> Self {
        self.default_size = (width, height);
        self
    }

    pub fn min_size(mut self, width: f64, height: f64) -> Self {
        self.min_size = Some((width, height));
        self
    }

    pub fn center(mut self, center: bool) -> Self {
        self.center = center;
        self
    }

    pub fn decorations(mut self, decorations: bool) -> Self {
        self.decorations = Some(decorations);
        self
    }
}

fn focus_existing_window(app_handle: &AppHandle, label: &str) -> bool {
    let Some(window) = app_handle.get_webview_window(label) else {
        return false;
    };

    crate::trace_err!(window.unminimize(), "set win unminimize");
    crate::trace_err!(window.show(), "set win visible");
    crate::trace_err!(window.set_focus(), "set win focus");
    true
}

fn resolve_always_on_top(config: &WindowConfig) -> bool {
    config.always_on_top.unwrap_or_else(|| {
        *Config::verge()
            .latest()
            .always_on_top
            .as_ref()
            .unwrap_or(&false)
    })
}

fn default_inner_size((width, height): (f64, f64)) -> (f64, f64) {
    #[cfg(target_os = "windows")]
    {
        (width, height)
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        (width, height + 6.0)
    }
}

fn clamp_window_size(
    window: &WebviewWindow,
    state: &WindowState,
    min_size: Option<(f64, f64)>,
) -> (u32, u32) {
    let mut width = state.width;
    let mut height = state.height;

    if let Some((min_width, min_height)) = min_size {
        let scale_factor = window.scale_factor().unwrap_or(1.0);
        let min_width = (min_width * scale_factor) as u32;
        let min_height = (min_height * scale_factor) as u32;
        width = width.max(min_width);
        height = height.max(min_height);
    }

    (width, height)
}

fn restore_window_state(
    window: &WebviewWindow,
    state: Option<&WindowState>,
    min_size: Option<(f64, f64)>,
) {
    let Some(state) = state else {
        return;
    };

    let (width, height) = clamp_window_size(window, state, min_size);

    crate::trace_err!(
        window.set_position(PhysicalPosition {
            x: state.x,
            y: state.y,
        }),
        "set win position"
    );
    crate::trace_err!(
        window.set_size(PhysicalSize { width, height }),
        "set win size"
    );

    if state.maximized {
        crate::trace_err!(window.maximize(), "set win maximize");
    }

    if state.fullscreen {
        crate::trace_err!(window.set_fullscreen(true), "set win fullscreen");
    }
}

fn should_center_window(window: &WebviewWindow, state: Option<&WindowState>) -> Result<bool> {
    let Some(state) = state else {
        return Ok(true);
    };

    let monitor = window
        .current_monitor()?
        .ok_or_else(|| anyhow!("monitor not found"))?;
    let PhysicalPosition { x, y } = *monitor.position();
    let PhysicalSize { width, height } = *monitor.size();
    let right = x + width as i32;
    let bottom = y + height as i32;

    let points = [
        (state.x, state.y),
        (state.x + state.width as i32, state.y),
        (state.x, state.y + state.height as i32),
        (state.x + state.width as i32, state.y + state.height as i32),
    ];

    Ok(!points
        .into_iter()
        .any(|(px, py)| px >= x && px < right && py >= y && py < bottom))
}

fn merge_captured_window_state(
    previous: Option<WindowState>,
    size: PhysicalSize<u32>,
    position: PhysicalPosition<i32>,
    maximized: bool,
    fullscreen: bool,
    minimized: bool,
) -> Option<WindowState> {
    if minimized {
        return previous;
    }

    if !maximized && !fullscreen && (size.width == 0 || size.height == 0) {
        return previous;
    }

    let mut state = previous.unwrap_or_default();
    state.maximized = maximized;
    state.fullscreen = fullscreen;

    if !maximized && !fullscreen {
        state.width = size.width;
        state.height = size.height;
        state.x = position.x;
        state.y = position.y;
    } else if state.width == 0 || state.height == 0 {
        // First-run fallback: if no normal geometry has ever been saved, keep a
        // usable size instead of persisting 0x0 while maximized/fullscreen.
        if size.width > 0 && size.height > 0 {
            state.width = size.width;
            state.height = size.height;
            state.x = position.x;
            state.y = position.y;
        }
    }

    Some(state)
}

pub(crate) fn capture_window_state(window: &WebviewWindow) -> Result<Option<WindowState>> {
    let previous = Config::verge().latest().window_size_state.clone();
    if window.current_monitor()?.is_none() {
        return Ok(previous);
    }

    let maximized = window.is_maximized()?;
    let fullscreen = window.is_fullscreen()?;
    let minimized = window.is_minimized()?;
    let size = window.inner_size()?;
    let position = window.outer_position()?;

    Ok(merge_captured_window_state(
        previous, size, position, maximized, fullscreen, minimized,
    ))
}

pub(crate) async fn persist_window_state(
    app_handle: &AppHandle,
    client: &crate::client::ChimeraClient,
    label: &str,
) -> Result<()> {
    if !matches!(
        label,
        crate::consts::LEGACY_WINDOW_LABEL | crate::consts::MAIN_WINDOW_LABEL
    ) {
        return Err(anyhow!("unknown window label: {label}"));
    }

    let Some(window) = app_handle.get_webview_window(label) else {
        return Ok(());
    };
    if window.is_minimized()? {
        return Ok(());
    }

    let state =
        capture_window_state(&window)?.map(|state| chimera_config::state::window::WindowState {
            width: state.width,
            height: state.height,
            x: state.x,
            y: state.y,
            maximized: state.maximized,
            fullscreen: state.fullscreen,
        });
    client.save_main_window_state(state).await?;
    Ok(())
}

pub(crate) async fn persist_active_window_state(
    app_handle: &AppHandle,
    client: &crate::client::ChimeraClient,
) -> Result<()> {
    let preferred = match Config::verge().latest().window_type.unwrap_or_default() {
        crate::config::chimera::WindowType::Main => crate::consts::MAIN_WINDOW_LABEL,
        crate::config::chimera::WindowType::Legacy => crate::consts::LEGACY_WINDOW_LABEL,
    };
    let fallback = if preferred == crate::consts::MAIN_WINDOW_LABEL {
        crate::consts::LEGACY_WINDOW_LABEL
    } else {
        crate::consts::MAIN_WINDOW_LABEL
    };

    let label = if app_handle.get_webview_window(preferred).is_some() {
        preferred
    } else if app_handle.get_webview_window(fallback).is_some() {
        fallback
    } else {
        return Ok(());
    };

    persist_window_state(app_handle, client, label).await
}

/// Trait for window management
pub trait AppWindow {
    fn label(&self) -> &str;
    fn title(&self) -> &str;
    fn url(&self) -> &str;

    fn config(&self) -> WindowConfig {
        WindowConfig::default()
    }

    fn get_window_state(&self) -> Option<WindowState>;

    fn create(&self, app_handle: &AppHandle) -> Result<()> {
        if focus_existing_window(app_handle, self.label()) {
            return Ok(());
        }

        let config = self.config();
        let window_state = self.get_window_state();
        let mut builder = tauri::WebviewWindowBuilder::new(
            app_handle,
            self.label(),
            tauri::WebviewUrl::App(self.url().into()),
        )
        .title(self.title())
        .fullscreen(false)
        .always_on_top(resolve_always_on_top(&config))
        .resizable(config.resizable)
        .disable_drag_drop_handler();

        if let Some((width, height)) = config.min_size {
            builder = builder.min_inner_size(width, height);
        }

        if window_state.is_some() {
            builder = builder
                .inner_size(RESTORE_PLACEHOLDER_SIZE.0, RESTORE_PLACEHOLDER_SIZE.1)
                .position(0.0, 0.0);
        } else {
            let (width, height) = default_inner_size(config.default_size);
            builder = builder.inner_size(width, height);

            if config.center {
                builder = builder.center();
            }
        }

        #[cfg(windows)]
        let window = builder
            .decorations(false)
            .transparent(true)
            .visible(false)
            .additional_browser_args(WEBVIEW2_BROWSER_ARGS)
            .build();

        #[cfg(target_os = "macos")]
        let window = {
            let decorations = config.decorations.unwrap_or(true);
            if decorations {
                builder
                    .decorations(true)
                    .hidden_title(true)
                    .title_bar_style(tauri::TitleBarStyle::Overlay)
                    .visible(config.visible_on_create)
                    .build()
            } else {
                builder
                    .decorations(false)
                    .visible(config.visible_on_create)
                    .build()
            }
        };

        #[cfg(all(not(windows), not(target_os = "macos")))]
        let window = builder.build();

        let window = match window {
            Ok(window) => window,
            Err(err) => {
                log::error!(target: "app", "failed to create window, {err:?}");
                return Err(err.into());
            }
        };

        restore_window_state(&window, window_state.as_ref(), config.min_size);

        #[cfg(windows)]
        crate::trace_err!(window.set_shadow(true), "set win shadow");

        if should_center_window(&window, window_state.as_ref()).unwrap_or(true) {
            crate::trace_err!(window.center(), "set win center");
        }

        #[cfg(target_os = "macos")]
        if config.decorations.unwrap_or(true) {
            let mtm = objc2_foundation::MainThreadMarker::new().unwrap();
            macos::setup_traffic_lights_pos(window.clone(), (18.0, 22.0), mtm);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_capture_replaces_saved_geometry() {
        let state = merge_captured_window_state(
            Some(WindowState {
                width: 800,
                height: 636,
                x: 10,
                y: 20,
                maximized: false,
                fullscreen: false,
            }),
            PhysicalSize::new(1024, 720),
            PhysicalPosition::new(120, 80),
            false,
            false,
            false,
        )
        .expect("normal window state should be captured");

        assert_eq!(state.width, 1024);
        assert_eq!(state.height, 720);
        assert_eq!(state.x, 120);
        assert_eq!(state.y, 80);
        assert!(!state.maximized);
        assert!(!state.fullscreen);
    }

    #[test]
    fn maximized_capture_preserves_last_normal_geometry() {
        let previous = WindowState {
            width: 960,
            height: 680,
            x: 44,
            y: 66,
            maximized: false,
            fullscreen: false,
        };
        let state = merge_captured_window_state(
            Some(previous.clone()),
            PhysicalSize::new(1920, 1080),
            PhysicalPosition::new(0, 0),
            true,
            false,
            false,
        )
        .expect("maximized state should be captured");

        assert_eq!(state.width, previous.width);
        assert_eq!(state.height, previous.height);
        assert_eq!(state.x, previous.x);
        assert_eq!(state.y, previous.y);
        assert!(state.maximized);
    }

    #[test]
    fn invalid_normal_resize_keeps_last_valid_state() {
        let previous = WindowState {
            width: 900,
            height: 650,
            x: 30,
            y: 40,
            maximized: false,
            fullscreen: false,
        };

        let state = merge_captured_window_state(
            Some(previous.clone()),
            PhysicalSize::new(0, 0),
            PhysicalPosition::new(0, 0),
            false,
            false,
            false,
        )
        .expect("previous state should be preserved");

        assert_eq!(state.width, previous.width);
        assert_eq!(state.height, previous.height);
        assert_eq!(state.x, previous.x);
        assert_eq!(state.y, previous.y);
    }

    #[test]
    fn minimized_capture_does_not_overwrite_saved_geometry() {
        let previous = WindowState {
            width: 880,
            height: 640,
            x: 70,
            y: 90,
            maximized: false,
            fullscreen: false,
        };

        let state = merge_captured_window_state(
            Some(previous.clone()),
            PhysicalSize::new(160, 28),
            PhysicalPosition::new(-32000, -32000),
            false,
            false,
            true,
        )
        .expect("previous state should be preserved");

        assert_eq!(state.width, previous.width);
        assert_eq!(state.height, previous.height);
        assert_eq!(state.x, previous.x);
        assert_eq!(state.y, previous.y);
    }
}

#[cfg(target_os = "macos")]
pub mod macos {
    #![allow(non_snake_case)]
    use dispatch2::MainThreadBound;
    use objc2::{
        DeclaredClass, MainThreadOnly, define_class, msg_send, rc::Retained,
        runtime::ProtocolObject,
    };
    use objc2_app_kit::{NSApplicationPresentationOptions, NSWindow, NSWindowDelegate};
    use objc2_foundation::{MainThreadMarker, NSNotification, NSObject, NSObjectProtocol};
    use tauri::{Emitter, WebviewWindow, WindowEvent};

    #[derive(Debug, Clone, Copy)]
    pub struct Position {
        pub x: f64,
        pub y: f64,
    }

    impl From<(f64, f64)> for Position {
        fn from(value: (f64, f64)) -> Self {
            Self {
                x: value.0,
                y: value.1,
            }
        }
    }

    impl From<Position> for (f64, f64) {
        fn from(value: Position) -> Self {
            (value.x, value.y)
        }
    }

    fn set_traffic_lights_pos(
        window: objc2::rc::Retained<objc2_app_kit::NSWindow>,
        pos: Position,
    ) -> anyhow::Result<()> {
        use objc2_app_kit::NSWindowButton;
        use objc2_foundation::NSRect;
        let close = window
            .standardWindowButton(NSWindowButton::CloseButton)
            .ok_or(anyhow::anyhow!("failed to get close button"))?;
        let miniaturize = window
            .standardWindowButton(NSWindowButton::MiniaturizeButton)
            .ok_or(anyhow::anyhow!("failed to get miniaturize button"))?;
        let zoom = window
            .standardWindowButton(NSWindowButton::ZoomButton)
            .ok_or(anyhow::anyhow!("failed to get zoom button"))?;

        let title_bar_container_view = unsafe {
            close
                .superview()
                .and_then(|view| view.superview())
                .ok_or(anyhow::anyhow!("failed to get title bar container view"))?
        };

        let close_rect = close.frame();
        let button_height = close_rect.size.height;

        let title_bar_frame_height = button_height + pos.y;
        let mut title_bar_rect = title_bar_container_view.frame();
        title_bar_rect.size.height = title_bar_frame_height;
        title_bar_rect.origin.y = window.frame().size.height - title_bar_frame_height;
        unsafe {
            title_bar_container_view.setFrame(title_bar_rect);
        }

        let space_between = miniaturize.frame().origin.x - close.frame().origin.x;
        let window_buttons = vec![close, miniaturize, zoom];

        for (i, button) in window_buttons.into_iter().enumerate() {
            let mut rect: NSRect = button.frame();
            rect.origin.x = pos.x + (i as f64 * space_between);
            unsafe {
                button.setFrameOrigin(rect.origin);
            }
        }
        Ok(())
    }

    #[derive(Debug, Clone)]
    struct WindowState {
        window: WebviewWindow<tauri::Wry>,
        traffic_lights_pos: Position,
    }

    impl WindowState {
        fn new(window: WebviewWindow<tauri::Wry>, traffic_lights_pos: Position) -> Self {
            Self {
                window,
                traffic_lights_pos,
            }
        }

        fn with_ns_window<T>(&self, func: impl FnOnce(Retained<NSWindow>) -> T) -> T {
            let ns_window = self.window.ns_window().expect("window not found");
            let ns_window = unsafe { Retained::retain_autoreleased(ns_window as *mut NSWindow) }
                .expect("failed to retain window");
            func(ns_window)
        }

        fn apply_traffic_lights_pos(&self) {
            self.with_ns_window(|win| {
                set_traffic_lights_pos(win, self.traffic_lights_pos)
                    .expect("failed to set traffic lights pos");
            });
        }
    }

    #[derive(Debug)]
    struct TrafficLightsWindowDelegateIvars {
        app_box: WindowState,
        super_class: Retained<ProtocolObject<dyn NSWindowDelegate>>,
    }

    const WINDOW_DID_ENTER_FULL_SCREEN: &str = "internal:://window-did-enter-full-screen";
    const WINDOW_WILL_ENTER_FULL_SCREEN: &str = "internal:://window-will-enter-full-screen";
    const WINDOW_WILL_EXIT_FULL_SCREEN: &str = "internal:://window-will-exit-full-screen";
    const WINDOW_DID_EXIT_FULL_SCREEN: &str = "internal:://window-did-exit-full-screen";

    define_class! {
        #[unsafe(super(NSObject))]
        #[name = "TrafficLightsPosWindowDelegate"]
        #[thread_kind = MainThreadOnly]
        #[ivars = TrafficLightsWindowDelegateIvars]
        struct WindowDelegate;

        unsafe impl NSObjectProtocol for WindowDelegate {}

        unsafe impl NSWindowDelegate for WindowDelegate {
            #[unsafe(method(windowShouldClose:))]
            unsafe fn windowShouldClose(&self, sender: &NSWindow) -> bool {
                tracing::trace!("passthrough `windowShouldClose` to TAO layer");
                unsafe { self.ivars().super_class.windowShouldClose(sender) }
            }

            #[unsafe(method(windowWillClose:))]
            unsafe fn windowWillClose(&self, notification: &NSNotification) {
                tracing::trace!("passthrough `windowWillClose` to TAO layer");
                unsafe { self.ivars().super_class.windowWillClose(notification) }
            }

            #[unsafe(method(windowDidResize:))]
            unsafe fn windowDidResize(&self, notification: &NSNotification) {
                self.ivars().app_box.apply_traffic_lights_pos();
                tracing::trace!("passthrough `windowDidResize` to TAO layer");
                unsafe { self.ivars().super_class.windowDidResize(notification) }
            }

            #[unsafe(method(windowDidMove:))]
            unsafe fn windowDidMove(&self, notification: &NSNotification) {
                tracing::trace!("passthrough `windowDidMove` to TAO layer");
                unsafe { self.ivars().super_class.windowDidMove(notification) }
            }

            #[unsafe(method(windowDidChangeBackingProperties:))]
            unsafe fn windowDidChangeBackingProperties(&self, notification: &NSNotification) {
                self.ivars().app_box.apply_traffic_lights_pos();
                tracing::trace!("passthrough `windowDidChangeBackingProperties` to TAO layer");
                unsafe { self.ivars().super_class.windowDidChangeBackingProperties(notification) }
            }

            #[unsafe(method(windowDidBecomeKey:))]
            unsafe fn windowDidBecomeKey(&self, notification: &NSNotification) {
                tracing::trace!("passthrough `windowDidBecomeKey` to TAO layer");
                unsafe { self.ivars().super_class.windowDidBecomeKey(notification) }
            }

            #[unsafe(method(windowDidResignKey:))]
            unsafe fn windowDidResignKey(&self, notification: &NSNotification) {
                tracing::trace!("passthrough `windowDidResignKey` to TAO layer");
                unsafe { self.ivars().super_class.windowDidResignKey(notification) }
            }

            #[unsafe(method(window:willUseFullScreenPresentationOptions:))]
            unsafe fn window_willUseFullScreenPresentationOptions(&self, window: &NSWindow, options: NSApplicationPresentationOptions) -> NSApplicationPresentationOptions {
                tracing::trace!("passthrough `window_willUseFullScreenPresentationOptions` to TAO layer");
                unsafe { self.ivars().super_class.window_willUseFullScreenPresentationOptions(window, options) }
            }

            #[unsafe(method(windowDidEnterFullScreen:))]
            unsafe fn windowDidEnterFullScreen(&self, notification: &NSNotification) {
                if let Err(e) = self.ivars().app_box.window.emit(WINDOW_DID_ENTER_FULL_SCREEN, ()) {
                    log::error!("failed to emit window-did-enter-full-screen event: {}", e);
                }
                tracing::trace!("passthrough `windowDidEnterFullScreen` to TAO layer");
                unsafe { self.ivars().super_class.windowDidEnterFullScreen(notification) }
            }

            #[unsafe(method(windowWillEnterFullScreen:))]
            unsafe fn windowWillEnterFullScreen(&self, notification: &NSNotification) {
                if let Err(e) = self.ivars().app_box.window.emit(WINDOW_WILL_ENTER_FULL_SCREEN, ()) {
                    log::error!("failed to emit window-will-enter-full-screen event: {}", e);
                }
                unsafe { self.ivars().super_class.windowWillEnterFullScreen(notification) }
            }

            #[unsafe(method(windowWillExitFullScreen:))]
            unsafe fn windowWillExitFullScreen(&self, notification: &NSNotification) {
                if let Err(e) = self.ivars().app_box.window.emit(WINDOW_WILL_EXIT_FULL_SCREEN, ()) {
                    log::error!("failed to emit window-will-exit-full-screen event: {}", e);
                }
                tracing::trace!("passthrough `windowWillExitFullScreen` to TAO layer");
                unsafe { self.ivars().super_class.windowWillExitFullScreen(notification) }
            }

            #[unsafe(method(windowDidExitFullScreen:))]
            unsafe fn windowDidExitFullScreen(&self, notification: &NSNotification) {
                if let Err(e) = self.ivars().app_box.window.emit(WINDOW_DID_EXIT_FULL_SCREEN, ()) {
                    log::error!("failed to emit window-did-exit-full-screen event: {}", e);
                }
                self.ivars().app_box.apply_traffic_lights_pos();
                tracing::trace!("passthrough `windowDidExitFullScreen` to TAO layer");
                unsafe { self.ivars().super_class.windowDidExitFullScreen(notification) }
            }

            #[unsafe(method(windowDidFailToEnterFullScreen:))]
            unsafe fn windowDidFailToEnterFullScreen(&self,window: &NSWindow) {
                tracing::trace!("passthrough `windowDidFailToEnterFullScreen` to TAO layer");
                unsafe { self.ivars().super_class.windowDidFailToEnterFullScreen(window) }
            }

        }
    }

    impl WindowDelegate {
        pub fn new(window_state: WindowState, mtm: MainThreadMarker) -> Retained<Self> {
            let this = Self::alloc(mtm);
            let super_class = window_state
                .with_ns_window(|win| unsafe { win.delegate().expect("failed to get delegate") });
            let ivars = TrafficLightsWindowDelegateIvars {
                app_box: window_state,
                super_class,
            };
            let this = this.set_ivars(ivars);
            unsafe { msg_send![super(this), init] }
        }
    }

    pub fn setup_traffic_lights_pos(window: WebviewWindow, pos: (f64, f64), mtm: MainThreadMarker) {
        let window_state = WindowState::new(window.clone(), pos.into());
        let ns_window = window_state.with_ns_window(|win| win);
        // first apply the traffic lights pos
        window_state.apply_traffic_lights_pos();
        let delegate = WindowDelegate::new(window_state.clone(), mtm);
        let object: &ProtocolObject<dyn NSWindowDelegate> = ProtocolObject::from_ref(&*delegate);
        ns_window.setDelegate(Some(object));
        // The window only holds its delegate weakly, so the window's own event
        // handler keeps it alive until the window is destroyed. Window events
        // arrive on the main thread, where the delegate is released.
        let delegate = parking_lot::Mutex::new(Some(MainThreadBound::new(delegate, mtm)));
        window.on_window_event(move |event| match event {
            WindowEvent::ThemeChanged(_) => {
                window_state.apply_traffic_lights_pos();
            }
            WindowEvent::Destroyed => {
                drop(delegate.lock().take());
            }
            _ => {}
        });
    }
}
