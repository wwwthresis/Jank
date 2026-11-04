# Jank! user guide

A short tour of the program. Hold the mouse over any control to see its tooltip, and press `Ctrl+K` to search every command by name.

## The window

- **Menu bar**: File, Edit, Model, Animation, View, Help.
- **Mode tabs** (keys `1`-`5`): Model, Texture, Pose, Animate, Render.
- **Viewport** in the middle: the 3D scene. Drag with the middle or right mouse button (or Alt + left) to orbit, hold Shift to pan, scroll to zoom. `Home` frames everything.
- **Right panel** with tabs: Scene, Look, World, Output.
- **Timeline** for animation, **status bar** with version and hints.
- `F3` shows performance statistics. `Shift+P` opens a window with the real final picture.

## Projects

| Action | Key |
|---|---|
| New / Open / Save / Save as | `Ctrl+N` / `Ctrl+O` / `Ctrl+S` / `Ctrl+Shift+S` |
| Import a model (OBJ, STL, PLY, glTF, GLB) | `Ctrl+I` |
| Export a photo or video | `Ctrl+E` |

A project is one `.jank` file holding models, textures, skeletons, animations, scene layers and lighting. Jank! autosaves while you work; if it closes unexpectedly, it offers to recover your work at the next start.

## 1. Model

- `Shift+A` adds a cube. Other primitives are in the `Model` menu and the Add row: plane, cylinder, cone, sphere, torus, butterfly.
- Gizmos: `Q` select, `W` move, `E` rotate, `R` scale. `Shift+L` switches world/local axes, `Shift+S` turns snapping on or off.
- Tools: subdivide, mirror along X, weld vertices, flat shading, automatic UV unwrap (`U`), polygon reduction.
- `Delete` removes the selection.

## 2. Texture

- Paint straight on the texture or on the model. Tools: brush, eraser, fill, picker, line, rectangle, selection (rectangle, wand, UV island).
- Palettes hold your colours; save them as `.jankpal` files. Colour reduction and dithering give the PSX look.
- **Layers**: paint on separate layers and blend them. Scene layers put a picture behind or in front of the 3D scene.

## 3. Pose

Imported glTF/GLB models with a skeleton can be posed: select a bone and rotate it with the gizmo.

## 4. Animate

- Animation is frame by frame: `Shift+N` adds a frame, `Shift+D` duplicates one, `,` and `.` step back and forward, `Space` plays.
- Onion skin shows neighbouring frames. Each frame can have its own camera and a hand-drawn overlay.

## 5. Render

- Presets set the PSX look: vertex snapping, affine textures, colour bits, dithering, fog, resolution.
- **World** tab: turn on *Time of day* and move the hour (sun 06:00-18:00, moon 18:00-06:00). Choose a shadow mode: *Off*, *Blob* (soft spots under objects) or *Cast* (real shadows; set strength, softness and quality).
- **Output** tab / `Ctrl+E`: save a photo (PNG, JPEG, BMP, TGA, PPM) or a video (GIF, AVI).

## Settings and shortcuts

`Ctrl+Comma` (or the gear at the bottom of the ribbon) opens Settings:

- **Appearance**: accent colour, Compact or Comfortable spacing, text size, the tool strip, the status bar and the frame rate in it.
- **Interface**: interface scale, tooltips, reduced motion, panel layout.
- **Viewport**: sharpness, *Viewport shadows* (like the render, up to Medium, Blob or Off - only the editing view, never the render or export), redraw only when something changes, orbit and zoom speed, inverted vertical orbit, render threads, the preview window.
- **Canvas & animation**: pixel grid of the texture editor, onion-skin strength, composition guides for Render mode (thirds, centre, safe area, golden ratio).
- **Files** and **Hotkeys**: autosave and recovery; every shortcut can be changed, `F1` shows the current list.

## The workspace

The ribbon on the far left switches the five modes and holds New, Open, Save, Import, Export, Undo and Redo. Next to the viewport sits the tool strip with the tools of the current mode: Select and Sculpt when modelling; Pencil, Brush, Eraser, Bucket, Eyedropper, Line, Rectangle, Filled rectangle and the selection tools when painting; Pose, Draw, Paint and Move when animating; guides, preview window and camera view when rendering. Click a section title in a panel to fold it. `Ctrl+K` or the *Search commands* field at the top runs any command by name.

## Where Jank! keeps its files

Settings and key bindings go to your XDG config folder, autosave to the XDG state folder. You can move them with `JANK_CONFIG_DIR` and `JANK_STATE_DIR` (see [BUILDING.md](../BUILDING.md)).

## Trying the demo

Open `demo/butterfly.jank`: press `Space` to play, `3` to see the bones, `2` for the texture, `5` for the render, and open the World tab to change the hour and the shadows.
