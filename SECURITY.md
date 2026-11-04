# Security policy

## Supported versions

Only the latest release receives fixes while Jank! is in the alpha and beta channels.

## Reporting a vulnerability

Please do not open a public issue for a security problem. Send a private report to the author, Thresis, by e-mail to **[thresiss@gmail.com](mailto:thresiss@gmail.com)** (subject starting with `[Jank security]`), or use GitHub's "Report a vulnerability" button on the [Security tab](https://github.com/wwwthresis/Jank/security) of the repository. Include the version and build number (`Help > About > Support`), your platform, and the steps or a sample file that shows the problem.

You can expect an acknowledgement, usually within a week, and then a fix in an upcoming build. Please give us time to ship the fix before you publish details. Credit is given on request.

## What is in scope

- Opening files: `.jank` projects, OBJ, STL, PLY, glTF/GLB, PNG, JPEG, BMP, TGA, GIF. A crafted file must never crash the program, hang it or use unbounded memory.
- Anything that writes outside the folder you chose when saving or exporting.
- The optional Discord status: it must never send more than described in [docs/DISCORD.md](docs/DISCORD.md) (for example the project name while that option is off).

## Design notes

- The engine is written in safe Rust on the standard library; the interface uses `egui`/`eframe`.
- The program makes no network connections, has no telemetry and no update check.
- The only exception is optional: the Discord status is written to the local Discord app through its IPC socket on the same computer (no token, no SDK). It can be turned off in `Settings > Discord`.
- Project files have per-chunk CRC checks and unknown chunks are skipped.
