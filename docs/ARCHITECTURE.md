# Architecture

Two crates, one direction of dependency: `jank` (GUI) -> `psxcore` (engine). The engine uses only the Rust standard library and never imports GUI types, so all logic is testable without a window.

## psxcore (`core/src`)

| Area | Modules |
|---|---|
| Math / data | `math`, `mesh`, `scene`, `texture`, `pose`, `anim`, `json` |
| Geometry ops | `modeling`, `decimate` (QEM), `selection`, `uvisland`, `gizmo` |
| Drawing | `paint`, `palette`, `layers` (layer stacks, blending), `lighting` (time of day, shadow settings), `raster` (multithreaded banded software PSX rasterizer: scene layers behind and in front), `shadow` (shadow map: depth from the light, soft PCF lookup), `compose` (scene to render instances, shadow inputs) |
| Editor state | `editor` (modes, tools, undo/redo, selection, pixel clipboard) |
| Files | `project` (`.jank`), `import` (OBJ/STL/PLY), `gltf`, `export`, `decode/*`, `encode/*` (PNG, JPEG, BMP, TGA, PPM, GIF, AVI, deflate), `jobs` |
| Application services | `commands` (command catalogue + fuzzy search), `keymap` (remappable chords), `settings`, `recovery` (autosave/crash), `filebrowser`, `splash` (season table), `version` |

## jank (`app/src`)

| Module | Role |
|---|---|
| `main` | `App`, menu bar, toolbar, status bar, update loop, shortcuts |
| `theme` | the design system (colours, fonts, egui `Visuals`/`Style`) - see DESIGN_SYSTEM.md |
| `widgets` | flat buttons, tabs, sections, toggles, swatches, tooltips |
| `icons` | vector icons drawn with the painter |
| `panels`, `texview`, `timeline`, `viewport`, `gizmo_ui` | mode panels, texture editor, animation strip, 3D view, gizmo overlay |
| `dialogs`, `commands_ui` | file browser, About/Licences, palette, settings, recovery |
| `splash`, `startup`, `appinfo`, `sysdate`, `crash` | splash screens, startup loader with real progress, version/About data, date helper, crash hook |
| `paintui`, `colorpick`, `preview`, `perf` | drawing studio (layers, selection, palettes), colour wheel, render preview window, frame statistics |
| `compat` | the only place that touches version-sensitive egui calls |

Rules: new logic goes into `core` with tests first; `app` only draws and forwards input. All user actions are `commands::CommandId`s so menu, palette and hotkeys share one path (`App::run_command`).

## Rendering order

`compose::render_states` turns the scene into instances (applying time of day) and, when shadows are on, builds a `ShadowMap`. `Renderer::render_layered` then draws the layers behind the model, the model, the shadow pass, the layers in front, and finally the post effects.

Shadow pass (`core/src/shadow.rs`): every casting object is drawn into a depth map seen from the light (orthographic, resolution 256-2048 by quality). For each visible pixel the renderer rebuilds its world position from the stored depth, compares it to the map with a soft PCF filter and darkens it. A per-triangle light factor keeps faces that already turn away from the light from shadowing themselves. Cut-out texels (alpha below 128) cast no shadow. Blob mode draws soft ellipsoids instead (128 px map).

## Built-in assets

`core/assets/` holds the files that are compiled into the program (`butterfly.obj`, `butterfly_texture.png`) via `include_str!` / `include_bytes!`; `modeling::butterfly()` builds the model from them.
