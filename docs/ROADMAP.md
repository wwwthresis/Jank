# Roadmap

Where Jank! stands and what remains before a full 1.0 release. Status: **done**, **in progress**, **to do**.

## Before 1.0

| # | Step | Status |
|---|---|---|
| 1 | Check the window on a real Wayland desktop (window size fix) | to do |
| 2 | Measure frame rate on real GPUs and weak machines | to do |
| 3 | Click-through check of selection tools and palette saving in the GUI | to do |
| 4 | Save and reopen scene layers through the file dialog (by hand) | to do |
| 5 | Smoother strokes when painting in the 3D viewport | to do |
| 6 | More options in Settings (viewport, shadows defaults) | to do |
| 7 | Packaging: AppImage, `.deb`, Flatpak | to do |
| 8 | Release pipeline: build number stamping, checksums, signing | to do |
| 9 | Final licence and EULA (source-available draft is in `LICENSE`; needs the owner's name, jurisdiction and a lawyer's review) | in progress |
| 10 | Update to the newest `eframe`/`egui` and the newest stable Rust | to do |
| 11 | Automated GUI tests in CI | to do |
| 12 | Crash reports the user can send | to do |
| 13 | Accessibility review (contrast, keyboard use, scaling) | to do |
| 14 | Localisation of the interface | to do |
| 15 | glTF export and glTF animation import | to do |
| 16 | New splash screens (to be designed with the owner) | in progress |

## Small polish ideas
- Emotion labels on frame chips are cut at 8 characters.
- Build number on the splash art is baked into the picture.
- UI scale of 150% and above needs a large window.
- Progressive JPEG import.
- Recent projects list and project thumbnails.
- Colour-blind friendly axis colours option.
- More shadow presets (long evening, noon, moonlight).

## Bigger ideas
- Optional GPU rendering path (`wgpu`) for the viewport while the software renderer stays as the exact PSX reference.
- Texture atlas and palette tools for whole levels.
- Level editor: grid snapping, prefabs, sector/portal culling preview.
- Plugin interface for tools and importers.
- Scripted batch export (command line).
- Bone-based animation curves and glTF animation export.
- Windows and macOS builds.
- Sound-less turntable and sprite-sheet exporters.

## Not planned
- MP4, H.264 and WebM output.
