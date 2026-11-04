# Jank! design system

Two references only - **Obsidian** (the workspace: deep graphite surfaces, a thin icon ribbon, quiet panes with hairline separators, one accent colour) and **Aseprite** (the tools: 16×16 pixel icons, a column of tool cells, bevelled plates, frame cels) - combined into one identity. Defined in `app/src/theme.rs`, `app/src/widgets.rs`, `app/src/icons.rs` and `app/src/chrome.rs`.

## Colours
- Graphite surfaces `BG0..BG3` (`#131316` `#1b1b1f` `#26262b` `#313137`), hairlines `LINE` (`#3a3a41`) / `LINE_SOFT` (`#27272c`); no pure black.
- Bevel colours `HILITE` (`#3d3d45`, 1 px top edge of a raised plate) and `SHADE` (`#0d0d10`, outline and bottom edge). They are used for nothing else.
- ONE accent, chosen by the user in Settings → Appearance: Violet `#8a7bf2` (default), Blue `#5c9cf0`, Teal `#3cb4a6`, Green `#7dbf69`, Amber `#dda03f`, Rose `#e0688a`. Read it with `theme::accent()` at paint time - never cache it in a `const`. It marks selection, the active mode, tool, tab and focus, and nothing else.
- `WARN` and `DANGER` only mean warning and destructive. Axis colours appear only on gizmos and X/Y/Z labels. Onion-skin colours only on ghosts.

## Shapes and type
- Controls are Aseprite plates: radius 2 px, 1 px dark outline, 1 px highlight on top, shade at the bottom. Pressed and selected plates are sunk (bevel inverted) and accent-tinted. No glows, no gradients.
- Obsidian elements (ribbon buttons, tabs, tree rows, navigation lists, the command pill) are borderless with a soft rounded hover plate (radius 4 px); the active one gets the accent (a 2 px bar on the ribbon, a 2 px underline on tabs, a tinted plate on rows).
- Font: Inter Regular/SemiBold (OFL). Body and buttons 13 pt (12 / 14.5 with the Small / Large text setting), small 11, headings body + 2 (semibold), monospace body - 1. Section headers: 10.5 pt uppercase semibold, a pixel chevron and a hairline; a click folds the section.
- Density: *Compact* (default: item spacing 6×4, button padding 8×3, interact height 22) or *Comfortable* (8×6, 10×5, 26).

## Icons
- Every icon is a hand-placed 16×16 bitmap with two tones (ink and 45 % shade), drawn in `tools/gen_icons.py` and generated into `app/src/icon_data.rs`. Run `python3 tools/gen_icons.py` after editing; `--preview` writes a sheet (needs Pillow).
- Icons are one white atlas texture tinted with the ink colour, drawn at whole-pixel scales only and snapped to the pixel grid, so they stay hard-edged at every interface scale. Never draw them at fractional scales or with smoothing.
- Draw new icons in the same style: 1 px outlines, shade for volume, no anti-aliasing, the motif centred in about 14×14 pixels.

## Layout
- Menu bar (26 px): wordmark, menus, project name and the *Search commands* pill (opens the palette).
- Ribbon (40 px, far left, full height): the five modes, file actions, undo/redo; About, palette and Settings at the bottom.
- Left pane: a pane header (mode icon, name, caption) over the mode's sections. Texture editor pane where it applies.
- Tool strip (36 px, next to the viewport, can be hidden): the tools of the current mode as pixel-icon cells, the brush colour cell at the bottom while painting. When the strip is shown, panels do not repeat the tool buttons.
- Right pane: icon tabs (Scene, Look, World, Output) and a pane header. Objects and bones are tree rows.
- Timeline: transport, animation tabs, frame operations and Aseprite-style cels (number header, cel dot filled when the frame has a drawing, tween arrow, hold width).
- Status bar (22 px, can be hidden): mode icon and message on the left; mode, selection, optional frame rate and the build on the right.

## Windows and dialogs
- Windows open centred with `default_pos` (see `app/src/compat.rs`) and can be dragged. Do not use `Window::anchor`: it pins the window. After an appearance change the Settings window is put back in the centre for a few frames, because egui re-measures it.
- Settings use an Obsidian-style navigation list on the left and sections on the right. Every option applies at once and is saved automatically.
- Every control has a tooltip. Destructive actions say what they do.
- Start-up uses the splash art for the season (see `docs/SPLASH_GUIDE.md`). The splash screen is always shown; there is deliberately no setting for it.
