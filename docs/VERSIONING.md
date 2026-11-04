# Versioning

`MAJOR.MINOR.PATCH[-CHANNEL]+BUILD`, for example `0.2.0-alpha+31027`. MAJOR changes when files cannot be opened by the previous major line; MINOR adds features; PATCH fixes only. Channels order as alpha < beta < rc < stable. BUILD comes from `app/BUILD_NUMBER` and is shown on the splash screen, About page and status bar. Details and tests: `core/src/version.rs`.

## Release routine
- Every handover of the source raises `app/BUILD_NUMBER`; a feature release also raises the version in `app/Cargo.toml` and `core/Cargo.toml` (they must match).
- The channel is `CHANNEL` in `app/src/appinfo.rs`.
- Each change gets one short line in `CHANGELOG.md`.
- Tag the commit `v<version>` (for example `v0.2.0-alpha`).
