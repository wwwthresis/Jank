# Jank! design system

Two references only - Aseprite (compact, precise, tool-like controls) and Obsidian (restrained dark neutrals, quiet separation) - combined into one identity. Defined in `app/src/theme.rs` and `app/src/widgets.rs`.

## Rules
- Graphite surfaces BG0..BG3 (`#17181b` `#1f2024` `#28292e` `#33343a`), hairline borders `LINE` / `LINE_SOFT`; no pure black.
- ONE accent (`#6f93c9`) for selection, active state and focus. `WARN` and `DANGER` only mean warning and destructive. Axis colours appear only on gizmos and X/Y/Z labels.
- Flat rectangular controls, radius 3 px, interact height 22 px, button padding 8x3, no glow, no shadows beyond a thin window shadow, no gradients, no translucent cards.
- Font: Inter Regular/SemiBold (OFL). Sizes: body/buttons 13, small 11, headings 15 (semibold), monospace 12. Section headers: 10.5 pt uppercase semibold with a hairline.
- Selected states: accent-tinted fill and accent border; tabs use a 2 px accent underline.
- Layout: menu bar (24 px), toolbar (36 px), side panels, timeline, status bar (22 px).

## Windows and dialogs
- Windows open centred with `default_pos` (see `app/src/compat.rs`) and can be dragged and resized. Do not use `Window::anchor`: it pins the window and makes it jump.
- Every control has a tooltip. Destructive actions say what they do.
- Start-up uses the splash art for the season (see `docs/SPLASH_GUIDE.md`).
