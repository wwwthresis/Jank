# Building Jank!

## Requirements

- Linux (X11 or Wayland) with OpenGL 2.0+ (software Mesa is fine).
- Rust stable, edition 2021 (developed with 1.97.0; newer stable versions are expected to work, older ones are untested) - install with <https://rustup.rs>.
- A C linker (`build-essential`). Runtime libraries normally present on a desktop: `libxkbcommon`, `libwayland-client` and X11 libraries (`libx11`, `libxcursor`, `libxrandr`, `libxi`), `libGL`/`libEGL`.
- Network access to crates.io for the first build. Versions are pinned by `Cargo.lock`; `eframe` is pinned to `=0.31.1`.

## Build and run the application

```
cd app
cargo build --release
./target/release/jank [--no-splash] [project.jank]
```

or simply `cargo run --release`. The debug profile also works (`opt-level = 1`; the engine crate stays at `opt-level = 3`).

Command-line options `--no-splash` (automated tests only: users always see the splash), `--splash 1-6|default|halloween|christmas|newyear|valentine|winter`, `--date MM-DD` (preview a seasonal splash), and an optional project path.

## Test the engine

```
cd core
cargo test --offline     # unit, fixture and demo-project tests; the engine has no external dependencies
```

## Environment variables

| Variable | Meaning |
|---|---|
| `JANK_CONFIG_DIR` | folder for settings and key bindings (default: XDG config dir) |
| `JANK_STATE_DIR` | folder for autosave / crash-recovery state (default: XDG state dir) |
| `JANK_AUTOSAVE_SECONDS` | overrides the autosave interval (used by automated tests) |
| `JANK_NO_SPLASH` | skip the splash screen (automated tests only) |
| `JANK_SPLASH`, `JANK_DATE` | force a seasonal splash / pretend it is MM-DD |
| `LIBGL_ALWAYS_SOFTWARE=1` | Mesa software rendering (headless or no GPU) |

## Regenerating the licence file

```
python3 tools/gen_licenses.py     # needs `cargo metadata` and a populated cargo registry
```

## Headless smoke test

```
xvfb-run -a -s "-screen 0 1600x900x24" env LIBGL_ALWAYS_SOFTWARE=1 ./app/target/release/jank --no-splash
```

## Upgrading egui

Every call that changes between egui versions is kept in `app/src/compat.rs`; update that file first when moving off 0.31.

## Rebuilding the butterfly demo

```
cd core
cargo run --release --example make_butterfly_demo -- path/to/butterfly_rigged.glb ../demo/butterfly.jank [preview.png]
```

The test `core/tests/butterfly_demo.rs` checks that the shipped demo opens with its skeleton, pose, layers and lighting and survives a save and reopen.

## Rebuilding the bunny demo

The demo project `demo/bunnies.jank` is made from two photos of plush toys (they are not part of the repository):

```
pip install numpy opencv-python-headless
python3 tools/bunnies/make_bunnies.py white_bunny.png cream_bunny.jpg /tmp/bunnies
cd core
cargo run --release --example make_bunny_demo -- /tmp/bunnies ../demo/bunnies.jank preview.png
```

The Python script cuts each toy out of its photo, inflates the outline into a stuffed shape, bakes the front and back textures, and places bones and skin weights; the Rust example adds the rug, the floor, the lamp light, shadows and the idle animation. `core/tests/bunny_demo.rs` checks the shipped file (skeletons, weights, closed surfaces, the animation loop, a save and reopen).

## Rebuilding the night shrine demo

`demo/night-shrine.jank` is made entirely in code, with the Edit-mode tools a user has (spin, solidify, array, poke):

```
cd core
cargo run --release --example make_shrine_demo -- ../demo/night-shrine.jank [preview.png] [theme-picture.png]
```

The optional third file is the picture of the *Shrine Night* example theme (`core/assets/themes/shrine-night.png`; reduce it to 128 colours before committing, for example `convert bg.png -dither None -colors 128 PNG8:core/assets/themes/shrine-night.png`). `core/tests/shrine_demo.rs` checks the shipped file (buildings, lamps attached to their lanterns and candles, the moonbeam, the Night Glow look, warm lamp light in a render, a save and reopen).

## Measuring render speed

```
cd core
cargo run --release --example bench_effects -- [threads] [project.jank]   # e.g. ../demo/night-shrine.jank; default: a test scene with four lanterns
```

It renders at 1280×720 the plain retro picture, each modern effect on its own (pixel lamps, tone curve, contact shadows, bloom, CRT) and the full *Night Glow* look, and prints the average milliseconds of a frame. `bench_render` times the plain renderer.

## Measuring Edit-mode speed

```
cd core
cargo run --release --example bench_edit [segments rings]     # default: a sphere of about 20 000 vertices
```

It prints the time of what happens every frame in Edit mode (overlay, hover) and of single actions (click, select all, move, extrude, undo).

## Making a release

1. Raise `app/BUILD_NUMBER` and add the entry `## <version> (build <N>)` at the top of `CHANGELOG.md` (for example `## 0.2.0-alpha (build 31036)`); the release notes are taken from it.
2. Commit and push, then tag and push the tag:

   ```
   git tag v0.2.0-alpha.31035
   git push origin v0.2.0-alpha.31035
   ```

3. The workflow `.github/workflows/release.yml` runs the engine tests, builds the program on Ubuntu 22.04 (so it also runs on older distributions), and publishes a GitHub release with the `tar.gz`, an AppImage and `SHA256SUMS`. Versions with a hyphen (`-alpha`, `-beta`) are marked as pre-releases. It can also be started by hand (*Actions → Release → Run workflow*); then the files are only kept as a workflow artifact.

The same packages can be made locally with `tools/package_release.sh` (add `--no-build` to reuse the current release build; the AppImage is made only if `appimagetool` is installed). The files go to `dist/`, which git ignores. The script refuses a program larger than 20 MB (`MAX_MB`).
