# Contributing to Jank!

Thank you for helping. This page explains how work is organised.

Jank! is owned and maintained by **Thresis** (Moscow, Russian Federation). Questions about the project, licences or larger contributions: [thresiss@gmail.com](mailto:thresiss@gmail.com). The official repository is <https://github.com/wwwthresis/Jank>.

## Ground rules

- Code, comments, documentation and interface text are in **English**.
- Logic goes into `core/` with tests first. `app/` only draws and forwards input.
- All user actions are `commands::CommandId`s, so the menu, the palette and hotkeys share one path.
- Version-sensitive egui calls live only in `app/src/compat.rs`.
- Add a third-party crate only for a real need. Check licence (no GPL/AGPL), maturity and size, and regenerate `app/assets/THIRD_PARTY_LICENSES.txt` with `tools/gen_licenses.py`.
- No MP4/H.264/WebM output (a project decision).

## Licence of contributions

By sending code or art you agree to section 6 of [LICENSE](LICENSE): you keep your copyright and give the project owner (Thresis) the right to use and distribute your contribution as part of Jank!. Send only work you made yourself or are allowed to give. Because the licence does not allow publishing forks, contributions go through pull requests to the official repository.

## Workflow

1. Open an issue (use the templates) or pick an existing one.
2. Make a branch, change code, write or update tests.
3. Run the checks:
   ```
   cd core && cargo test
   cd ../app && cargo build --release
   ```
4. Add one short line to `CHANGELOG.md` under the next version: what was added, changed, removed or fixed. Write only what was checked.
5. Before every handover, raise `app/BUILD_NUMBER` (or the version, see `docs/VERSIONING.md`).
6. Open a pull request that describes what changed and how you checked it.

## Style

- `cargo fmt` and no warnings from `cargo build`.
- Prefer small functions and plain data. Public items in `core` get a doc comment.
- UI: follow `docs/DESIGN_SYSTEM.md` (one accent colour, flat controls, a tooltip on every control).

## Reporting bugs

Use the bug template. Security problems go by e-mail, see [SECURITY.md](SECURITY.md). Include the block from `Help > About > Support > Copy to clipboard` (version, build, platform).
