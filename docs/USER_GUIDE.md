# Jank! user guide

A short tour of the program. Hold the mouse over any control to see its tooltip, and press `Ctrl+K` to search every command by name.

## The window

- **Menu bar**: File, Edit, Model, Mesh, Paint, Animation, View, Help. *Help → What's new* lists the changes of this version.
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
- The object under the mouse is outlined, so you see what a click selects (Settings → Viewport → Highlight).
- **Boolean** section: choose a cutter object and press Union, Difference or Intersect. Both meshes should be closed. *Delete the cutter afterwards* removes it.
- **Reference pictures**: *Add picture...* places a drawing in the 3D view (front, side, top or facing the view). Drag it with the Select tool, change size and opacity in the panel. They are never rendered or exported.

### Edit mode

Press `Tab` (or *Edit mesh*) to edit the selected object's vertices, edges and polygons; `Tab` again goes back. Keys work as in Blender:

| Action | Key |
|---|---|
| Vertex / edge / face select | `1` / `2` / `3` |
| Select, add, box select | click, Shift+click, drag (Shift adds, Ctrl removes) |
| Edge loop / ring / shortest path | Alt+click / Ctrl+Alt+click / Ctrl+click |
| All, none, invert, linked, more, less | `A`, `Alt+A`, `Ctrl+I`, `L` (under the mouse) or `Ctrl+L`, `Ctrl+=`, `Ctrl+-` |
| Hide / hide others / reveal | `H` / `Shift+H` / `Alt+H` |
| Move, rotate, scale | `G`, `R`, `S`, then `X`/`Y`/`Z` (Shift+axis: the other two), type a number, `Enter` or click to confirm, `Esc` or right click to cancel; Ctrl snaps, Shift is fine |
| Extrude, inset, bevel | `E`, `I`, `Ctrl+B` (wheel: segments) |
| Loop cut and slide | `Ctrl+R`, point at an edge, wheel for more cuts, click, then slide |
| Knife | `K`, click points, `Enter` cuts, `Esc` stops |
| Fill, connect, split, separate | `F`, `J`, `Y`, `P` |
| Duplicate, delete, dissolve, merge | `Shift+D`, `X`, `Ctrl+X`, `M` |
| Proportional editing, X-ray | `O` (wheel changes the radius while moving), `Alt+Z` |
| UV unwrap from seams | `U` |

The left panel has every tool as a button, plus subdivide, bridge, triangulate, tris to quads, normals, smooth and flat shading, symmetrize, seams, mirror editing and Boolean inside the mesh (selected faces against the rest). Quads and n-gons are kept when you save.

## 2. Texture

- Paint straight on the texture or on the model. Tools: brush, eraser, fill, picker, line, rectangle, selection (rectangle, wand, UV island).
- More tools: smudge, blur, dodge and burn, gradient (`G`, from the main to the second colour, dithered in pixel mode). `X` swaps the main and the second colour, `[` and `]` change the size. *Symmetric painting* mirrors the stroke, the *stabilizer* smooths shaky lines.
- *Projection painting in 3D* paints through the view onto everything that is visible, across UV seams and objects.
- **Indexed colour**: *Convert to indexed* locks the texture to the current palette. Every pixel then stores a palette entry; right-click an entry to change its colour and the whole texture follows (like PlayStation CLUTs). *Back to full colour* undoes the lock.
- Palettes hold your colours; save them as `.jankpal` files. Colour reduction and dithering give the PSX look.
- **Layers**: paint on separate layers and blend them. Scene layers put a picture behind or in front of the 3D scene.

## 3. Pose

Imported glTF/GLB models with a skeleton can be posed: select a bone and rotate it with the gizmo. Turn on **inverse kinematics** (`Shift+I` or the IK cell) and drag the end of a limb: the bones above it follow; *chain bones* sets how many (1-8).

## 4. Animate

- Animation is frame by frame: `Shift+N` adds a frame, `Shift+D` duplicates one, `,` and `.` step back and forward, `Shift+Space` plays.
- The **Curves** tab of the timeline is the graph editor: every animated value as a curve. Drag a key up or down to change it, click the graph to go to a frame. On the right, choose the easing of the current in-between (linear, in, out, in-out, back) or drag the two Bézier handles.
- Onion skin shows neighbouring frames. Each frame can have its own camera and a hand-drawn overlay.

## 5. Render

- Presets set the PSX look: vertex snapping, affine textures, colour bits, dithering, fog, resolution.
- **World** tab: turn on *Time of day* and move the hour (sun 06:00-18:00, moon 18:00-06:00). Choose a shadow mode: *Off*, *Blob* (soft spots under objects) or *Cast* (real shadows; set strength, softness and quality).
- **Output** tab / `Ctrl+E`: save a photo (PNG, JPEG, BMP, TGA, PPM) or a video (GIF, AVI).

## Radial menu

Hold `Space` over the view, point at a tool and release (or tap `Space` and click), as in Krita. The ring shows the tools of the current mode - mesh tools in Edit mode, brushes and recent colours while painting.

## Settings and shortcuts

`Ctrl+Comma` (or the gear at the bottom of the ribbon) opens Settings:

- **Appearance**: theme (six built in, one light; *Save a copy to edit* writes a `.theme` text file you can change, *Reload theme files* picks up your edits), accent colour, Compact or Comfortable spacing, text size, the tool strip, the status bar and the frame rate in it.
- **Interface**: interface scale, tooltips, reduced motion, panel layout.
- **Viewport**: sharpness, *Viewport shadows* (like the render, up to Medium, Blob or Off - only the editing view, never the render or export), redraw only when something changes, orbit and zoom speed, inverted vertical orbit, render threads, the preview window.
- **Canvas & animation**: pixel grid of the texture editor, onion-skin strength, composition guides for Render mode (thirds, centre, safe area, golden ratio).
- **Discord**: show *Playing Jank!* on your Discord profile; choose whether the project name, the details and the time are shown (see [DISCORD.md](DISCORD.md)).
- **Files** and **Hotkeys**: autosave and recovery; every shortcut can be changed, `F1` shows the current list.

## The workspace

The ribbon on the far left switches the five modes and holds New, Open, Save, Import, Export, Undo and Redo. Next to the viewport sits the tool strip with the tools of the current mode: Select and Sculpt when modelling; Pencil, Brush, Eraser, Bucket, Eyedropper, Line, Rectangle, Filled rectangle and the selection tools when painting; Pose, Draw, Paint and Move when animating; guides, preview window and camera view when rendering. Click a section title in a panel to fold it. `Ctrl+K` or the *Search commands* field at the top runs any command by name.

## Where Jank! keeps its files

Settings and key bindings go to your XDG config folder, autosave to the XDG state folder. You can move them with `JANK_CONFIG_DIR` and `JANK_STATE_DIR` (see [BUILDING.md](../BUILDING.md)).

## Trying the demo

Open `demo/butterfly.jank`: press `Shift+Space` to play, `3` to see the bones, `2` for the texture, `5` for the render, and open the World tab to change the hour and the shadows.
