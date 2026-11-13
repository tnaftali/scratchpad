//! Scratchpad: floating plain-text scratchpad over one file, remembers its place.

use std::{fs, ops::Range, path::PathBuf, process::Command, thread, time::Duration};

use gpui::{
    App, AppContext, Bounds, Context, Entity, FocusHandle, Focusable, FontStyle, FontWeight,
    HighlightStyle, Hsla, InteractiveElement, IntoElement, KeyBinding, KeyDownEvent, MouseButton,
    MouseUpEvent, ParentElement, Render, StrikethroughStyle, Styled, UnderlineStyle, Window,
    WindowBackgroundAppearance, WindowBounds, WindowOptions, div, px, rgb, size,
};
use gpui_base::actions::{SelectDown, SelectUp};
use gpui_component::{
    Sizable, Size, Theme, ThemeMode,
    input::{
        Copy, Cut, DeleteToBeginningOfLine, DeleteToNextWordEnd, DeleteToPreviousWordStart, Editor,
        EditorState, Enter, IndentInline, InputEvent, MoveToEnd, MoveToNextWord,
        MoveToPreviousWord, MoveToStart, OutdentInline, Paste, Redo, RopeExt, SelectAll,
        SelectToNextWordEnd, SelectToPreviousWordStart, TextDecoration, TextDecorationCollection,
        Undo,
    },
};
use serde::{Deserialize, Serialize};

const APP_ID: &str = "scratchpad";
/// Window background alpha. The text stays opaque.
const OPACITY: f32 = 0.5;
/// Proportional UI sans; size adjusted by eye.
const FONT: &str = "IBM Plex Sans";
/// Claude black.
const BACKGROUND: u32 = 0x262624;
const FOREGROUND: u32 = 0xf5f4ed;
const ACCENT: u32 = 0xd97757;
const FONT_SIZE: f32 = 17.;

#[derive(Serialize, Deserialize, Clone, Copy)]
struct Geometry {
    x: i64,
    y: i64,
    w: i64,
    h: i64,
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").expect("HOME is not set"))
}

fn note_path() -> PathBuf {
    home().join("Documents/scratchpad.md")
}

fn geometry_path() -> PathBuf {
    home().join(".local/state/scratchpad/geometry.json")
}

fn load_geometry() -> Option<Geometry> {
    serde_json::from_slice(&fs::read(geometry_path()).ok()?).ok()
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
                let path = geometry_path();
                let _ = fs::create_dir_all(path.parent().expect("has parent"));
                let _ = fs::write(path, serde_json::to_vec(&g).expect("serializes"));
            }
        }
    });
}

/// Plain multi-line text editor over one file.
/// Set both Wayland selections. Terminals paste PRIMARY on Shift+Insert, which is
/// what Omarchy's SUPER+V sends there. wl-copy also keeps the text after Scratchpad closes.
fn copy(text: &str) {
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

/// Live markdown styling. Markers (`**`, `#`, ...) are dimmed on every line but the caret line.
fn markdown_decorations(
    text: &str,
    caret_line: usize,
) -> (Vec<TextDecoration>, Vec<TextDecoration>) {
    use pulldown_cmark::{Event, Options, Parser, Tag};
    let accent = rgb(ACCENT).into();
    // Opaque mix: decoration colors with alpha did not render.
    let channel = |shift: u32| {
        let (f, b) = ((FOREGROUND >> shift) & 0xff, (BACKGROUND >> shift) & 0xff);
        (f * 35 + b * 65) / 100
    };
    let dim: Hsla = rgb(channel(16) << 16 | channel(8) << 8 | channel(0)).into();
    let mut code_bg: Hsla = rgb(FOREGROUND).into();
    code_bg.a = 0.1;
    let line_of = |at: usize| text[..at].matches('\n').count();
    let style = |f: &dyn Fn(&mut HighlightStyle)| {
        let mut h = HighlightStyle::default();
        f(&mut h);
        h
    };
    let (mut markers, mut styles) = (Vec::new(), Vec::new());
    let mark = |range: std::ops::Range<usize>, len: usize, markers: &mut Vec<TextDecoration>| {
        if line_of(range.start) != caret_line && range.len() >= 2 * len {
            let h = style(&|h| h.color = Some(dim));
            markers.push(TextDecoration::new(range.start..range.start + len, h));
            markers.push(TextDecoration::new(range.end - len..range.end, h));
        }
    };
    for (event, range) in Parser::new_ext(text, Options::ENABLE_STRIKETHROUGH).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { .. }) => {
                let hashes = text[range.start..]
                    .bytes()
                    .take_while(|&b| b == b'#')
                    .count();
                let body = (range.start + hashes).min(range.end);
                if line_of(range.start) != caret_line {
                    markers.push(TextDecoration::new(
                        range.start..body,
                        style(&|h| h.color = Some(dim)),
                    ));
                }
                styles.push(TextDecoration::new(
                    body..range.end,
                    style(&|h| {
                        h.color = Some(accent);
                        h.font_weight = Some(FontWeight::BOLD);
                    }),
                ));
            }
            Event::Start(Tag::Strong) => {
                mark(range.clone(), 2, &mut markers);
                styles.push(TextDecoration::new(
                    range,
                    style(&|h| h.font_weight = Some(FontWeight::BOLD)),
                ));
            }
            Event::Start(Tag::Emphasis) => {
                mark(range.clone(), 1, &mut markers);
                styles.push(TextDecoration::new(
                    range,
                    style(&|h| h.font_style = Some(FontStyle::Italic)),
                ));
            }
            Event::Start(Tag::Strikethrough) => {
                mark(range.clone(), 2, &mut markers);
                styles.push(TextDecoration::new(
                    range,
                    style(&|h| {
                        h.strikethrough = Some(StrikethroughStyle {
                            thickness: px(1.),
                            color: None,
                        })
                    }),
                ));
            }
            Event::Code(_) => {
                let ticks = text[range.clone()]
                    .bytes()
                    .take_while(|&b| b == b'`')
                    .count();
                mark(range.clone(), ticks, &mut markers);
                styles.push(TextDecoration::new(
                    range,
                    style(&|h| h.background_color = Some(code_bg)),
                ));
            }
            Event::Start(Tag::Link { .. }) => {
                styles.push(TextDecoration::new(
                    range,
                    style(&|h| {
                        h.color = Some(accent);
                        h.underline = Some(UnderlineStyle {
                            thickness: px(1.),
                            color: None,
                            wavy: false,
                        });
                    }),
                ));
            }
            _ => {}
        }
    }
    // List markers: "•" and "N." in the accent color. pulldown-cmark does not know "•".
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let body = line.trim_start_matches([' ', '\t']);
        let start = at + line.len() - body.len();
        let digits = body.bytes().take_while(u8::is_ascii_digit).count();
        let len = if body.starts_with("• ") {
            '•'.len_utf8()
        } else if digits > 0 && body[digits..].starts_with(". ") {
            digits + 1
        } else {
            0
        };
        if len > 0 {
            styles.push(TextDecoration::new(
                start..start + len,
                style(&|h| h.color = Some(accent)),
            ));
        }
        at += line.len();
    }
    (markers, styles)
}

struct Scratchpad {
    editor: Entity<EditorState>,
    /// Marker dimming (wins on overlap) and text styles.
    decorations: Option<(TextDecorationCollection, TextDecorationCollection)>,
    /// Text and caret line the decorations were built for.
    styled: (String, usize),
    focus: FocusHandle,
}

impl Scratchpad {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let text = fs::read_to_string(note_path()).unwrap_or_default();
        let text = text
            .split_inclusive('\n')
            .map(|l| {
                let body = l.trim_start();
                match body.strip_prefix("- ") {
                    Some(rest) => format!("{}• {rest}", &l[..l.len() - body.len()]),
                    None => l.to_string(),
                }
            })
            .collect::<String>();
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .line_number(false)
                .indent_guides(false)
                .folding(false)
                .searchable(false)
        });
        editor.update(cx, |e, cx| e.set_value(text, window, cx));
        // Re-style on every edit and caret move; the editor notifies, this view does not.
        cx.observe(&editor, |_, _, cx| cx.notify()).detach();
        cx.subscribe_in(
            &editor,
            window,
            |_, editor, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    // Typing "- " at the caret turns into a bullet.
                    let (value, line) = {
                        let s = editor.read(cx);
                        (s.value().to_string(), s.cursor_position().line as usize)
                    };
                    let start: usize = value.split_inclusive('\n').take(line).map(str::len).sum();
                    let body = value[start..].trim_start_matches([' ', '\t']);
                    let at =
                        value.len() - value[start..].len() + (value[start..].len() - body.len());
                    if body.starts_with("- ") && editor.read(cx).cursor() == at + 2 {
                        editor.update(cx, |e, cx| {
                            e.set_selected_range(at..at + 2, cx);
                            e.replace("• ", window, cx);
                        });
                        return;
                    }
                    let path = note_path();
                    if let Some(dir) = path.parent() {
                        let _ = fs::create_dir_all(dir);
                    }
                    if let Err(e) = fs::write(&path, editor.read(cx).value().as_ref()) {
                        eprintln!("scratchpad: cannot save {}: {e}", path.display());
                    }
                }
            },
        )
        .detach();
        Self {
            editor,
            decorations: None,
            styled: Default::default(),
            focus: cx.focus_handle(),
        }
    }
}

impl Scratchpad {
    /// Enter on a "- " or "N. " line starts the next item. Enter on an empty item ends the list.
    fn continue_bullet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.editor.read(cx);
        let row = state.cursor_position().line as usize;
        let line = state.text().slice_line(row).to_string();
        let line = line.trim_end_matches(['\n', '\r']);
        let body = line.trim_start();
        let indent = &line[..line.len() - body.len()];
        let digits = body.len() - body.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        let (rest, next) = if let Some(rest) = body.strip_prefix("• ") {
            (rest, "• ".to_string())
        } else if digits > 0 && body[digits..].starts_with(". ") {
            let n: u64 = body[..digits].parse().unwrap_or(0);
            (&body[digits + 2..], format!("{}. ", n + 1))
        } else {
            return;
        };
        if rest.trim().is_empty() {
            window.dispatch_action(Box::new(DeleteToBeginningOfLine), cx);
        } else {
            let text = format!("\n{indent}{next}");
            self.editor.update(cx, |e, cx| e.insert(text, window, cx));
        }
        cx.stop_propagation();
    }
}

impl Scratchpad {
    /// Tab / Shift+Tab on a "• " line moves the whole item one level. Other lines keep the default.
    /// With a selection, every line it touches moves one level.
    fn indent_bullet(&mut self, outdent: bool, window: &mut Window, cx: &mut Context<Self>) {
        const STEP: &str = "    ";
        let state = self.editor.read(cx);
        let value = state.value().to_string();
        let selection = state.selected_range();
        if !selection.is_empty() {
            let (block, indented) = shift_lines(&value, selection, outdent);
            self.editor.update(cx, |e, cx| {
                e.set_selected_range(block.clone(), cx);
                e.replace(&indented, window, cx);
                e.set_selected_range(block.start..block.start + indented.len(), cx);
            });
            cx.stop_propagation();
            return;
        }
        let caret = state.cursor();
        let start: usize = value
            .split_inclusive('\n')
            .take(state.cursor_position().line as usize)
            .map(str::len)
            .sum();
        let line = &value[start..];
        if !line.trim_start_matches(' ').starts_with("• ") {
            return;
        }
        let new_caret = if outdent {
            let n = line.len() - line.trim_start_matches(' ').len();
            let n = n.min(STEP.len());
            self.editor.update(cx, |e, cx| {
                e.set_selected_range(start..start + n, cx);
                e.replace("", window, cx);
            });
            caret.saturating_sub(n).max(start)
        } else {
            self.editor.update(cx, |e, cx| {
                e.set_selected_range(start..start, cx);
                e.replace(STEP, window, cx);
            });
            caret + STEP.len()
        };
        self.editor
            .update(cx, |e, cx| e.set_selected_range(new_caret..new_caret, cx));
        cx.stop_propagation();
    }
}

/// Whole-line block covering `selection` (a selection that ends at a line start skips that line),
/// and that block with one indent level added to or removed from each non-empty line.
fn shift_lines(value: &str, selection: Range<usize>, outdent: bool) -> (Range<usize>, String) {
    const STEP: &str = "    ";
    let start = value[..selection.start].rfind('\n').map_or(0, |i| i + 1);
    let mut end = selection.end;
    if end > selection.start && value[..end].ends_with('\n') {
        end -= 1;
    }
    let end = value[end..].find('\n').map_or(value.len(), |i| end + i);
    let shifted = value[start..end]
        .split('\n')
        .map(|line| {
            if outdent {
                let n = (line.len() - line.trim_start_matches(' ').len()).min(STEP.len());
                line[n..].to_string()
            } else if line.is_empty() {
                String::new()
            } else {
                format!("{STEP}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    (start..end, shifted)
}

impl Focusable for Scratchpad {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.editor.read(cx).focus_handle(cx)
    }
}

impl Render for Scratchpad {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.editor.read(cx);
        let key = (
            state.value().to_string(),
            state.cursor_position().line as usize,
        );
        if self.decorations.is_none() || self.styled != key {
            let (markers, styles) = markdown_decorations(&key.0, key.1);
            match &self.decorations {
                Some((m, s)) => {
                    m.set(markers, cx);
                    s.set(styles, cx);
                }
                None => {
                    self.decorations = Some(self.editor.update(cx, |e, cx| {
                        (
                            e.create_decorations_collection(markers, cx),
                            e.create_decorations_collection(styles, cx),
                        )
                    }));
                }
            }
            self.styled = key;
        }
        let mut bg: Hsla = rgb(BACKGROUND).into();
        bg.a = OPACITY;
        div()
            .id("root")
            .track_focus(&self.focus)
            .size_full()
            .bg(bg)
            .text_color(rgb(FOREGROUND))
            .font_family(FONT)
            .font_weight(FontWeight::NORMAL)
            .text_size(px(FONT_SIZE))
            .p(px(18.))
            // Copy on select, like a terminal. Capture phase: the textarea eats mouse-up.
            // wl-copy keeps the text after Scratchpad closes; a Wayland selection dies with its owner.
            // Bindings run before key-down listeners, so catch the Enter action itself.
            .capture_action(cx.listener(|this, e: &Enter, window, cx| {
                if !e.secondary && !e.shift {
                    this.continue_bullet(window, cx);
                }
            }))
            .capture_action(cx.listener(|this, _: &IndentInline, window, cx| {
                this.indent_bullet(false, window, cx)
            }))
            .capture_action(cx.listener(|this, _: &OutdentInline, window, cx| {
                this.indent_bullet(true, window, cx)
            }))
            .capture_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                let k = &e.keystroke;
                let copying = k.modifiers.control && matches!(k.key.as_str(), "c" | "x" | "insert");
                let selected = this.editor.read(cx).selected_value();
                if copying && !selected.is_empty() {
                    copy(&selected);
                }
            }))
            .capture_any_mouse_up(cx.listener(|this, e: &MouseUpEvent, _, cx| {
                let selected = this.editor.read(cx).selected_value();
                if e.button == MouseButton::Left && !selected.is_empty() {
                    copy(&selected);
                }
            }))
            .child(
                Editor::new(&self.editor)
                    .with_size(Size::Size(px(FONT_SIZE / 0.875)))
                    .appearance(false)
                    .bordered(false)
                    .h_full(),
            )
    }
}

fn main() {
    let saved = load_geometry();
    track_geometry(saved);
    gpui_platform::application().run(move |cx| {
        gpui_component::init(cx);
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
        Theme::change(ThemeMode::Dark, None, cx);
        let theme = Theme::global_mut(cx);
        theme.foreground = rgb(FOREGROUND).into();
        theme.font_family = FONT.into();
        theme.font_size = px(FONT_SIZE);
        // Editor forces the mono theme font.
        theme.mono_font_family = FONT.into();
        theme.mono_font_size = px(FONT_SIZE);
        let (w, h) = saved.map_or((1000., 700.), |g| (g.w as f32, g.h as f32));
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(w), px(h)),
                cx,
            ))),
            window_background: WindowBackgroundAppearance::Transparent,
            titlebar: None,
            app_id: Some(APP_ID.into()),
            ..Default::default()
        };
        cx.open_window(options, |window, cx| {
            let view = cx.new(|cx| Scratchpad::new(window, cx));
            let focus = view.read(cx).editor.read(cx).focus_handle(cx);
            window.focus(&focus, cx);
            view
        })
        .expect("open window");
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_lines_whole_block() {
        let v = "a\n  b\n\nc";
        let (r, out) = shift_lines(v, 1..5, false);
        assert_eq!((r, out.as_str()), (0..v.len() - 3, "    a\n      b"));
        // selection ending at a line start leaves that line alone
        let (r, _) = shift_lines(v, 0..2, false);
        assert_eq!(r, 0..1);
        assert_eq!(shift_lines("      x\n y", 0..10, true).1, "  x\ny");
    }

    #[test]
    fn heading_marker_dims_off_caret_line() {
        let (markers, styles) = markdown_decorations("a\n# Title", 0);
        assert_eq!(
            markers.iter().map(|m| m.range.clone()).collect::<Vec<_>>(),
            [2..3]
        );
        assert_eq!(
            styles.iter().map(|m| m.range.clone()).collect::<Vec<_>>(),
            [3..9]
        );
        assert!(markdown_decorations("a\n# Title", 1).0.is_empty());
        let (_, styles) = markdown_decorations("• a\n  12. b", 0);
        assert_eq!(
            styles.iter().map(|m| m.range.clone()).collect::<Vec<_>>(),
            [0..3, 8..11]
        );
    }
}
