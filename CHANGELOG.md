# Changelog

All notable changes to Jank! are listed here, newest first.
Versions follow [docs/VERSIONING.md](docs/VERSIONING.md): `MAJOR.MINOR.PATCH[-channel]+BUILD`.

## 0.2.0-alpha (build 31035)

### Added
- **Starter scene**: the first start opens two plush bunnies on a pink rug in warm evening light, made from photos of real toys (shape from the outline, photo textures front and back, skeletons with skin weights) with a short *Idle* loop of ears, paws and feet. *File → Open the bunny scene* and a *Bunny scene* button on the empty-scene card open it again; it is built into the program (`demo/bunnies.jank`), so it never changes a file. Built by `tools/bunnies/make_bunnies.py` and `core/examples/make_bunny_demo.rs`; checked by `core/tests/bunny_demo.rs`.
- **GitHub releases**: pushing a tag `v<version>.<build>` builds and publishes a release with a `tar.gz` (program, demo scenes, `install.sh` for the applications menu, icon, `.desktop` file), an AppImage and `SHA256SUMS`; notes come from this changelog. `tools/package_release.sh` makes the same files locally. See *Making a release* in BUILDING.md.
- `core/examples/bench_edit.rs`: times Edit mode on a heavy mesh. `Editor::open_bytes` opens a project from memory.
- New screenshots in the README and the user guide (Edit mode, radial menu, Curves, the Discord card, Render, Texture).

### Changed
- **Faster Edit mode**, measured with `bench_edit` on a sphere of 19 800 vertices (milliseconds, before → after): enter Edit mode 178 → 105; overlay each frame: vertices 23.3 → 4.9, edges 21.6 → 4.8, faces 27.6 → 6.7; hover 8.3 → 3.1; click select 3.5 → 1.0; select all 28.3 → 10.8; one step of a move 22.7 → 6.0, finishing it 23.6 → 5.9; extrude 58 → 24.5; undo 29 → 8.8; leave Edit mode 27 → 6.4. Drawing a wireframe of 40 000 edges in the window: about 21 → 5. How: one fast hash map for all mesh look-ups, the edge table and the overlay built in one pass, no copy of the mesh on selection, batched wireframe drawing for heavy meshes.
- The renderer splits the picture over up to 16 threads (was 8), and Edit mode projects its points with one camera matrix per frame instead of one per point. On 1–2 cores the gain is small: the 3D picture was already drawn in parallel bands (about 49 ms on one thread, 25 ms on two for the butterfly demo at 1280×720).

### Fixed
- **The viewport and the preview window showed the picture too pale** (on every graphics card): the picture was uploaded as plain RGBA while egui treats textures as sRGB, so the colours were brightened twice. They now match the exported render exactly.
- Switches that are written back only when they change did nothing when clicked: the Discord options, *Outline the object under the mouse*, *IK while dragging* and *Projection painting in 3D*.
- The World/Snap strip of the viewport was drawn on top of the Settings window.
- *Tris to quads* and other Edit-mode operations could give a different result from run to run; they are now deterministic.

## 0.2.0-alpha (build 31034)

### Changed
- **Licence 0.2** (applies from this build): names the licensor (Thresis, Moscow, Russian Federation), the contact address thresiss@gmail.com and the official source (github.com/wwwthresis/Jank); new section 10 *Privacy* (no network, no telemetry, what the optional Discord status sends); disputes go to the courts of Moscow; the English text is binding.
- README: all features of 0.2 (Edit mode, booleans, indexed colour, projection painting, IK, graph editor, reference pictures, radial menu, themes, Discord status), `Shift+Space` plays, new questions "Does it go online?", new section *Author and contact*.
- SECURITY, CONTRIBUTING, the user guide (*Privacy*, *Help and contact*), DISCORD.md and the bug template name the author and the contact address.
- `Cargo.toml` of `jank` and `psxcore`: author, licence file, repository, `publish = false`.

### Added
- *About → Overview* shows the author, the e-mail address (with a Copy button) and the link to the repository.

## 0.2.0-alpha (build 31033)

### Added
- Discord status is live: the official Application ID is built in, and the card has a **GitHub** button (https://github.com/wwwthresis/Jank/).

## 0.2.0-alpha (build 31032)

### Added
- **Discord Rich Presence**: "Playing Jank!" with the project, the mode and what you are doing (e.g. "Editing polygons · 96 vertices, 72 faces"), the elapsed time, the Jank! logo and a mode badge; "Taking a break" after five minutes without input. Settings → Discord turns it on or off and hides the project name (hidden by default), the details or the timer, shows the connection state and a preview of the card. Talks only to the local Discord app (IPC socket, no SDK, no network, no token); see [docs/DISCORD.md](docs/DISCORD.md). Art assets for the Discord application in `docs/discord/` (`tools/gen_discord_assets.py`).

### Fixed
- The Settings window no longer jumps to a screen edge when a page is re-measured; it stays where it is and moves only by dragging.

## 0.2.0-alpha (build 31031)

### Added
- **Edit mode for meshes** (`Tab`, Model mode): vertices, edges and polygons as in Blender. `1`/`2`/`3` vertex, edge and face select; click, Shift+click, box select (Shift adds, Ctrl removes), Alt+click edge loops, Ctrl+Alt+click rings, Ctrl+click shortest path, `L` (under the mouse) / `Ctrl+L` linked, `Ctrl+I` invert, `Ctrl+=` / `Ctrl+-` grow and shrink, `H` / `Shift+H` / `Alt+H` hide and reveal. `G`/`R`/`S` move, rotate and scale with `X`/`Y`/`Z` axis locks (Shift+axis excludes it), typed values, Ctrl snapping, Shift for fine steps and proportional editing (`O`, wheel changes the radius). Tools: extrude (`E`), inset (`I`), bevel (`Ctrl+B`, segments, also vertex bevel), loop cut and slide (`Ctrl+R`, wheel for more cuts), knife (`K`), subdivide, fill (`F`), bridge loops, connect (`J`), duplicate (`Shift+D`), split (`Y`), separate (`P`), delete / dissolve (`X`, `Ctrl+X`), shrink / fatten (`Alt+S`), edge slide after a loop cut (`Shift+V`), triangulate (`Ctrl+T`), tris to quads (`Alt+J`), merge (`M`: centre, last clicked, collapse, by distance), triangulate, tris to quads, flip and recalculate normals, shade smooth / flat, symmetrize, mark and clear seams, UV unwrap from seams (`U`), mirror X editing and X-ray (`Alt+Z`). Quads and n-gons are kept between sessions (new `EDIT` chunk).
- **Boolean operations**: union, difference and intersection of two objects (Model → Boolean, choose the cutter, optionally delete it) and inside one mesh in Edit mode (selected polygons against the rest).
- **Indexed colour**: a texture can be locked to a palette; every pixel stores an index, recolouring a palette entry repaints the whole texture (like PlayStation CLUTs). Back to full colour at any time.
- **Projection painting**: paint through the view onto every visible object, across UV seams.
- **Painting**: smudge, blur, dodge, burn and gradient tools; second colour (`X` swaps); tool strength; symmetric painting; stroke stabilizer in the texture editor and the viewport.
- **Inverse kinematics** in Pose and Animate mode (`Shift+I`): drag the end of a chain and the parent bones follow (chain length 1-8 bones).
- **Graph editor**: the *Curves* tab of the timeline shows every animated value as a curve; drag keys up and down, filter by the selected object, normalize, and edit the Bézier easing of each in-between (linear, in, out, in-out, back). Easing is saved (new `EASE` chunk).
- **Reference pictures in 3D** (Model → Add reference image): front, side, top or facing the view; size, opacity, in front of or behind the models; draggable in the view; never rendered or exported (new `REFS` chunk).
- **Radial menu** on `Space` (hold, point, release; or tap and click), as in Krita; it changes with the mode and shows recent colours while painting.
- **Themes as files**: six built-in themes (Graphite, Midnight, Studio Grey, PlayStation, Amber CRT and the light Paper); `*.theme` text files in `<config>/themes` are loaded too, with *Save a copy to edit* and *Reload theme files* in Settings → Appearance.
- **What's new** window (Help → What's new), shown once after an update.
- **Hover highlight**: the object under the mouse is outlined (Settings → Viewport → Highlight).
- `Mesh` and `Paint` menus.

### Changed
- Play/pause is now `Shift+Space`, because `Space` opens the radial menu.
- Keys can belong to a context (Edit mode, painting): there they shadow the global key with the same chord, so `G` moves vertices in Edit mode and draws gradients while painting.

## 0.2.0-alpha (build 31030)

### Added
- New workspace in the style of Obsidian and Aseprite: icon ribbon with the modes and file actions, a tool strip next to the viewport, pane headers, icon tabs, foldable panel sections, tree rows for objects and bones, Aseprite-style timeline cels, a *Search commands* field in the menu bar and a settings window with a navigation list.
- All tool icons redrawn as 16×16 two-tone pixel art (67 icons, `tools/gen_icons.py`), drawn pixel-exact from one small texture.
- Tool strip: Pencil (hard pixel brush) and Brush as separate tools, selection tools, brush colour cell; guides, preview and camera toggles in Render mode.
- Settings → Appearance: accent colour (six choices), Compact / Comfortable spacing, text size, show or hide the tool strip and the status bar, frame rate in the status bar.
- Settings → Viewport: viewport shadows (like the render, up to Medium, Blob, Off), redraw only on change, orbit and zoom speed, inverted vertical orbit.
- Settings → Canvas & animation: pixel grid (now remembered), onion-skin strength, composition guides in Render mode (thirds, centre, safe area, golden ratio).
- Onion-skin toggle in the timeline.

### Changed
- Cast shadows are 2 to 2.8 times faster with identical pictures (checked pixel by pixel on the demo, Low to Ultra): the shadow map is drawn on all threads, fully lit and fully shadowed pixels are decided with one table lookup, and the map is reused while only the camera moves. Ultra: 108 ms to 38.5 ms per 1280×720 frame.
- The 3D view and the preview window are no longer drawn again when the pointer only moves over the panels.
- Graphite palette is deeper and the default accent is violet.

## 0.2.0-alpha (build 31029)

### Changed
- Copyright holder and licensor are now Thresis (`LICENSE`, About page).

## 0.2.0-alpha (build 31028)

### Added
- Winter splash screen, shown on every day from 1 December to the end of February except the holidays, which keep their own pictures.
- Source-available licence and EULA draft in `LICENSE`.

### Removed
- The "Show splash screen at start" setting: the splash screen is always shown. Old settings files that still contain it open without a problem.

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
