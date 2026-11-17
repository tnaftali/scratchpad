//! macOS: floating panel, Opt+V hotkey, Cmd+W hides, window place through GPUI.

use std::time::Duration;

use gpui::{
    App, Bounds, Context, KeyBinding, Pixels, Size, TitlebarOptions, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, point, px,
};

use crate::{Geometry, save_geometry};

gpui::actions!(scratchpad, [Hide]);

/// Window background alpha. The text stays opaque.
/// macOS adds its own blur. Enough tint to keep the light text readable over white windows.
pub const OPACITY: f32 = 0.4;

pub fn init(_saved: Option<Geometry>, cx: &mut App) {
    toggle_on_hotkey(cx);
    // Hide, not close: the app keeps running so the hotkey can bring it back.
    cx.bind_keys([KeyBinding::new("cmd-w", Hide, None)]);
    cx.on_action(|_: &Hide, cx| cx.hide());
}

pub fn window_options(saved: Option<Geometry>, size: Size<Pixels>, cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(match saved {
            Some(g) => Bounds {
                origin: point(px(g.x as f32), px(g.y as f32)),
                size,
            },
            None => Bounds::centered(None, size, cx),
        })),
        // Hyprland blurs the transparent window; macOS needs Blurred for that.
        window_background: WindowBackgroundAppearance::Blurred,
        // macOS makes a window resizable only when it has a titlebar; a transparent one hides it.
        titlebar: Some(TitlebarOptions {
            title: None,
            appears_transparent: true,
            traffic_light_position: None,
        }),
        // An NSPanel above normal windows.
        kind: WindowKind::Floating,
        ..Default::default()
    }
}

/// macOS windows know their own place; Linux tracks it through Hyprland.
pub fn after_open<T: 'static>(window: &mut Window, cx: &mut Context<T>) {
    keep_on_screen(window);
    cx.observe_window_bounds(window, |_, window, _| {
        let b = window.bounds();
        save_geometry(&Geometry {
            x: f32::from(b.origin.x) as i64,
            y: f32::from(b.origin.y) as i64,
            w: f32::from(b.size.width) as i64,
            h: f32::from(b.size.height) as i64,
        });
    })
    .detach();
}

/// The GPUI clipboard is the system pasteboard, and it outlives the app. Nothing to add.
pub fn copy(_text: &str) {}

/// An NSPanel hides when its app is not active. Keep Scratchpad floating over other apps,
/// with no traffic lights.
fn keep_on_screen(window: &Window) {
    use objc2::{
        msg_send,
        runtime::{AnyObject, Bool},
    };
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    if let RawWindowHandle::AppKit(h) = handle.as_raw() {
        let view = h.ns_view.as_ptr().cast::<AnyObject>();
        // SAFETY: ns_view is GPUI's live NSView, and window creation runs on the main thread.
        unsafe {
            let panel: *mut AnyObject = msg_send![view, window];
            let _: () = msg_send![panel, setHidesOnDeactivate: Bool::NO];
            // Hide the traffic lights. The titlebar stays, so the window still resizes.
            // 0, 1, 2: NSWindowCloseButton, MiniaturizeButton, ZoomButton.
            for kind in 0usize..3 {
                let button: *mut AnyObject = msg_send![panel, standardWindowButton: kind];
                if !button.is_null() {
                    let _: () = msg_send![button, setHidden: Bool::YES];
                }
            }
        }
    }
}

/// GPUI's active_window() reads NSApp.mainWindow, and a panel never becomes main. Ask NSApp.
fn app_is_active() -> bool {
    use objc2::{
        class, msg_send,
        runtime::{AnyObject, Bool},
    };
    // SAFETY: called on the main thread from a GPUI foreground task.
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let active: Bool = msg_send![app, isActive];
        active.as_bool()
    }
}

/// Global hotkey that shows Scratchpad on top, or hides it when it is the active app.
fn toggle_on_hotkey(cx: &mut App) {
    use global_hotkey::{
        GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
        hotkey::{Code, HotKey, Modifiers},
    };
    let manager = GlobalHotKeyManager::new().expect("hotkey manager");
    let hotkey = HotKey::new(Some(Modifiers::ALT), Code::KeyV);
    if let Err(e) = manager.register(hotkey) {
        eprintln!("scratchpad: cannot register alt-v: {e}");
        return;
    }
    // shortcut: polls the hotkey channel every 50 ms, switch to an async channel if it shows in CPU.
    cx.spawn(async move |cx| {
        let _manager = manager;
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(50))
                .await;
            while let Ok(e) = GlobalHotKeyEvent::receiver().try_recv() {
                if e.state == HotKeyState::Pressed {
                    cx.update(|cx| {
                        if app_is_active() {
                            cx.hide()
                        } else {
                            cx.activate(true)
                        }
                    });
                }
            }
        }
    })
    .detach();
}
