# Security policy

## Supported versions

Only the latest release receives fixes while Jank! is in the alpha and beta channels.

## Reporting a vulnerability

Please do not open a public issue for a security problem. Send a private report to the project owner (use the contact address in the repository profile, or GitHub's "Report a vulnerability" button on the Security tab). Include the version and build number (`Help > About > Support`), your platform, and the steps or a sample file that shows the problem.

You can expect an acknowledgement and then a fix in an upcoming build. Credit is given on request.

## What is in scope

- Opening files: `.jank` projects, OBJ, STL, PLY, glTF/GLB, PNG, JPEG, BMP, TGA, GIF. A crafted file must never crash the program, hang it or use unbounded memory.
- Anything that writes outside the folder you chose when saving or exporting.

## Design notes

- The engine is written in safe Rust on the standard library; the interface uses `egui`/`eframe`.
- The program makes no network connections.
- Project files have per-chunk CRC checks and unknown chunks are skipped.
