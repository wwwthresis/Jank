# Roadmap

Where Jank! stands and what remains before a full 1.0 release. Status: **done**, **in progress**, **to do**.

## Before 1.0

| # | Step | Status |
|---|---|---|
| 1 | Measure frame rate on real GPUs and weak machines | to do |
| 2 | Click-through check of selection tools and palette saving in the GUI | to do |
| 3 | Save and reopen scene layers through the file dialog (by hand) | to do |
| 4 | Smoother strokes when painting in the 3D viewport | done (stroke stabilizer) |
| 5 | More options in Settings (appearance, viewport shadows, navigation, canvas) | done |
| 6 | Packaging: AppImage, `.deb`, Flatpak | to do |
| 7 | Release pipeline: build number stamping, checksums, signing | to do |
| 8 | Update to the newest `eframe`/`egui` and the newest stable Rust | to do |
| 9 | Automated GUI tests in CI | to do |
| 10 | Crash reports the user can send | to do |
| 11 | Accessibility review (contrast, keyboard use, scaling) | to do |
| 12 | Localisation of the interface | to do |
| 13 | glTF export and glTF animation import | to do |
| 14 | New splash screens (to be designed with the owner) | in progress |

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
- glTF animation export (bone curves are in the graph editor now).
- Edge slide for any edge (now only right after a loop cut), vertex slide, spin and screw tools.
- Booleans on open meshes (now the cutter and the target should be closed).
- Theme editor inside the program (themes are edited as text files now).
- Windows and macOS builds.
- Sound-less turntable and sprite-sheet exporters.

## Not planned
- MP4, H.264 and WebM output.
