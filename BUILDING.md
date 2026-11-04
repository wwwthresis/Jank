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

Command-line options: `--no-splash`, `--splash 1-5|default|halloween|christmas|newyear|valentine`, `--date MM-DD` (preview a seasonal splash), and an optional project path.

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
| `JANK_NO_SPLASH` | skip the splash screen |
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
