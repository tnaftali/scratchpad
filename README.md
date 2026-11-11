<p align="center">
  <img src="docs/icon-512.png" width="96" alt="scratchpad icon">
</p>

<h1 align="center">scratchpad</h1>

<p align="center">
  A floating markdown scratchpad for Linux, in the style of Raycast Notes.<br>
  Native <a href="https://www.gpui.rs/">GPUI</a> app, one file, no accounts, no sync.
</p>

![scratchpad](docs/screenshot.png)

Hit a hotkey, jot something down, hit it again. The window closes and the text is already saved.

## What it does

- **One note, always saved.** Every change is written to `~/Documents/scratchpad.md` as you type. Open it in any editor.
- **Live markdown styling.** Headings, **bold**, *italic*, `~~strikethrough~~`, `` `code` `` and links are styled while you write. Markers (`#`, `~~`, backticks) are dimmed on every line except the one with the caret, so raw markdown shows only where you are editing.
- **Lists that behave.** Typing `- ` turns into a `•` bullet. Enter continues the list (bullets and `1.` numbering), and Enter on an empty item ends it. Tab and Shift+Tab indent and outdent a bullet. Bullet and number markers are tinted with the accent color.
- **Copy on select.** Selected text goes to both Wayland clipboards (regular and primary), so it pastes in terminals with Shift+Insert. It stays available after the app closes.
- **Remembers its place.** Window size and position are saved and restored (Hyprland, via `hyprctl`).
- **Translucent, dark.** Claude-black background with an orange accent, in IBM Plex Sans.

## Build

```sh
cargo build --release
```

Needs a Wayland session with `wl-copy` (from `wl-clipboard`) for copy on select. Window position tracking needs Hyprland. The font is set by `FONT` in `src/main.rs` and must be installed. Use a font with static Bold and Italic files (on Arch, `ttf-ibm-plex`): GPUI does not apply variable-font axes, so a variable font shows no bold or italic.

## Run

```sh
target/release/scratchpad
```

It works best behind a toggle key that starts the app, or closes the window if it is already open. The window class is `scratchpad`, so a Hyprland rule such as `float` and `pin` for that class gives the Raycast feel.

## Edit keys

ALT stands in for CTRL for word movement, deletion, select all and undo (`alt-left`, `alt-backspace`, `alt-a`, `alt-z`, and so on), because ALT+C/V/X are Hyprland binds on the author's setup. `ctrl-insert` and `shift-insert` copy and paste.

## License

MIT
