# Changelog

All notable changes to Jank! are listed here, newest first.
Versions follow [docs/VERSIONING.md](docs/VERSIONING.md): `MAJOR.MINOR.PATCH[-channel]+BUILD`.

## 0.2.0-alpha (build 31027)

### Added
- Butterfly primitive (`Model > Add butterfly`) with a built-in textured model; no external files needed.
- Demo project `demo/butterfly.jank`: rigged butterfly, flutter animation, meadow scene with layers, sun and shadows.
- Time of day (World tab): sun 06:00-18:00, moon 18:00-06:00, light direction, colours, sky and fog follow the hour.
- Shadow modes Off / Blob / Cast, with shadow strength, softness and quality (Low to Ultra).
- Drawing studio: texture layers, scene layers behind and in front of the 3D scene, selection tools (rectangle, wand, UV island), colour wheel, palettes (`.jankpal`).
- Render preview window (Shift+P).
- Multithreaded software rasterizer, viewport statistics (F3), startup loader with real progress.
- Optional project chunks `LAYR` (layers) and `LITE` (time of day and shadows); older files still open.
- Project files: LICENSE, CONTRIBUTING, SECURITY, user guide, roadmap, splash guide, GitHub issue templates and CI workflow.

### Changed
- Shadows are now a real shadow map: any object can shadow any other, the ground and each other, with soft edges. They no longer appear on the lit side of the object itself.
- Texels with alpha below 128 are cut out, which gives clean wing and leaf edges.
- Undo history is limited by memory (256 MB) as well as by step count (200).
- A project with a frame camera opens looking through it, with the rigged object selected.
- Splash screen fills to about 40% first, then follows the real loading, and stays for at least two seconds.
- README, BUILDING, ARCHITECTURE, FILE_FORMAT, VERSIONING rewritten.

### Fixed
- Windows (About, Settings, palette, preview) can be dragged and resized; they no longer jump.
- Deleting a palette no longer crashes the program.

## 0.1.0-alpha (build 31026)

### Added
- First complete GUI: modes Model, Texture, Pose, Animate, Render.
- Command palette, remappable hotkeys, settings, saved panel layout, UI scale, context menus.
- Undo/redo, autosave and crash recovery, licences page.
- Gizmos: move, rotate, scale, local/world axes, snapping.
- Import: OBJ, STL, PLY, glTF/GLB with skeleton. Output: PNG, JPEG, BMP, TGA, PPM, GIF, AVI.
- Seasonal splash screens (Halloween, Christmas, New Year, Valentine).
- Single graphite design system with one accent and the Inter font.
