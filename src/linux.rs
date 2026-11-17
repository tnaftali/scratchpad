//! Linux: Hyprland window place, Wayland clipboards, Omarchy key remaps.

use std::{process::Command, thread, time::Duration};

use gpui::{
    App, Bounds, Context, KeyBinding, Pixels, Size, Window, WindowBackgroundAppearance,
    WindowBounds, WindowOptions,
};
use gpui_base::actions::{SelectDown, SelectUp};
use gpui_component::input::{
    Copy, Cut, DeleteToNextWordEnd, DeleteToPreviousWordStart, MoveToEnd, MoveToNextWord,
    MoveToPreviousWord, MoveToStart, Paste, Redo, SelectAll, SelectToNextWordEnd,
    SelectToPreviousWordStart, Undo,
};

use crate::{Geometry, save_geometry};

/// Wayland app id and Hyprland window class.
const APP_ID: &str = "scratchpad";

/// Window background alpha. The text stays opaque.
pub const OPACITY: f32 = 0.5;

pub fn init(saved: Option<Geometry>, cx: &mut App) {
    track_geometry(saved);
    // ALT as CTRL for word movement, deletion, select all and undo. ALT+C/V/X are Hyprland binds.
    cx.bind_keys([
        KeyBinding::new("alt-left", MoveToPreviousWord, Some("Input")),
        KeyBinding::new("alt-right", MoveToNextWord, Some("Input")),
        KeyBinding::new("alt-shift-left", SelectToPreviousWordStart, Some("Input")),
        KeyBinding::new("alt-shift-right", SelectToNextWordEnd, Some("Input")),
        KeyBinding::new("alt-backspace", DeleteToPreviousWordStart, Some("Input")),
        KeyBinding::new("alt-delete", DeleteToNextWordEnd, Some("Input")),
        KeyBinding::new("alt-home", MoveToStart, Some("Input")),
        KeyBinding::new("alt-end", MoveToEnd, Some("Input")),
        KeyBinding::new("alt-shift-up", SelectUp, Some("Input")),
        KeyBinding::new("alt-shift-down", SelectDown, Some("Input")),
        KeyBinding::new("alt-a", SelectAll, Some("Input")),
        KeyBinding::new("alt-z", Undo, Some("Input")),
        KeyBinding::new("alt-shift-z", Redo, Some("Input")),
    ]);
    // Omarchy's SUPER+C / SUPER+V send ctrl-insert / shift-insert.
    cx.bind_keys([
        KeyBinding::new("ctrl-insert", Copy, Some("Input")),
        // SUPER is physically held while Omarchy injects the chord, so it can arrive merged.
        KeyBinding::new("ctrl-super-c", Copy, Some("Input")),
        KeyBinding::new("ctrl-super-v", Paste, Some("Input")),
        KeyBinding::new("ctrl-super-x", Cut, Some("Input")),
        KeyBinding::new("shift-insert", Paste, Some("Input")),
        KeyBinding::new("shift-delete", Cut, Some("Input")),
    ]);
}

/// Wayland ignores the origin; Hyprland gets it in track_geometry.
pub fn window_options(_saved: Option<Geometry>, size: Size<Pixels>, cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size, cx))),
        // Hyprland blurs the transparent window.
        window_background: WindowBackgroundAppearance::Transparent,
        titlebar: None,
        app_id: Some(APP_ID.into()),
        ..Default::default()
    }
}

pub fn after_open<T: 'static>(_window: &mut Window, _cx: &mut Context<T>) {}

/// Set both Wayland selections. Terminals paste PRIMARY on Shift+Insert, which is
/// what Omarchy's SUPER+V sends there. wl-copy also keeps the text after Scratchpad closes.
pub fn copy(text: &str) {
    use std::{io::Write, process::Stdio};
    for flags in [&[][..], &["--primary"][..]] {
        if let Ok(mut child) = Command::new("wl-copy")
            .args(flags)
            .stdin(Stdio::piped())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
        }
    }
}

fn hyprctl(args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new("hyprctl").args(args).output().ok()?;
    out.status.success().then_some(out.stdout)
}

/// This window as Hyprland sees it. Wayland clients cannot read or set their own position.
fn current_geometry() -> Option<Geometry> {
    let clients: Vec<serde_json::Value> =
        serde_json::from_slice(&hyprctl(&["clients", "-j"])?).ok()?;
    let c = clients.iter().find(|c| c["class"] == APP_ID)?;
    Some(Geometry {
        x: c["at"][0].as_i64()?,
        y: c["at"][1].as_i64()?,
        w: c["size"][0].as_i64()?,
        h: c["size"][1].as_i64()?,
    })
}

/// Restore the saved place once the window is mapped, then keep saving it.
fn track_geometry(saved: Option<Geometry>) {
    thread::spawn(move || {
        let mut restored = saved.is_none();
        let mut last = None::<(i64, i64, i64, i64)>;
        loop {
            thread::sleep(Duration::from_millis(if restored { 300 } else { 50 }));
            let Some(g) = current_geometry() else {
                continue;
            };
            if let (false, Some(s)) = (restored, saved) {
                // Resize keeps the window center, so resize first, then move. Hyprland may
                // still be applying window rules, so repeat until the place sticks.
                let sel = format!("window = \"class:{APP_ID}\"");
                for _ in 0..20 {
                    hyprctl(&[
                        "dispatch",
                        &format!(
                            "hl.dsp.window.resize({{ x = {}, y = {}, exact = true, {sel} }})",
                            s.w, s.h
                        ),
                    ]);
                    hyprctl(&[
                        "dispatch",
                        &format!(
                            "hl.dsp.window.move({{ x = {}, y = {}, exact = true, {sel} }})",
                            s.x, s.y
                        ),
                    ]);
                    thread::sleep(Duration::from_millis(150));
                    if current_geometry()
                        .is_some_and(|g| (g.x, g.y, g.w, g.h) == (s.x, s.y, s.w, s.h))
                    {
                        break;
                    }
                }
                restored = true;
                last = current_geometry().map(|g| (g.x, g.y, g.w, g.h));
                continue;
            }
            let now = (g.x, g.y, g.w, g.h);
            if last != Some(now) {
                last = Some(now);
                save_geometry(&g);
            }
        }
    });
}
