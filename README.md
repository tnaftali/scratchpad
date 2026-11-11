# scratchpad

Floating markdown scratchpad for Linux, in the style of Raycast Notes. Native GPUI app.

- Edits one file: `~/Documents/scratchpad.md`.
- Live markdown styling: headings, bold, italic, strikethrough, code, links. Markers dim off the caret line.
- `- ` becomes `•`. Tab and Shift+Tab indent bullets.
- Copy on select. Remembers window size and position (Hyprland, via `hyprctl`).

## Build

```sh
cargo build --release
```

Needs `wl-copy` (copy on select) and Hyprland (window placement).
