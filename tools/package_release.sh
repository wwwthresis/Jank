#!/usr/bin/env bash
# Builds the Linux release of Jank! into dist/:
#   jank-<version>-build<N>-linux-<arch>.tar.gz   program, demo projects, licence, installer
#   Jank-<version>-build<N>-<arch>.AppImage       one-file program (only when appimagetool is found)
#   SHA256SUMS, NOTES.md (this version's CHANGELOG entry), VERSION
#
#   tools/package_release.sh [--no-build]
# Environment: APPIMAGETOOL=/path/to/appimagetool (default: from PATH), MAX_MB (default 20).
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' app/Cargo.toml | head -1)
BUILD=$(tr -d '[:space:]' < app/BUILD_NUMBER)
CHANNEL=$(sed -n 's/^pub const CHANNEL: Channel = Channel::\([A-Za-z]*\);/\1/p' app/src/appinfo.rs | tr '[:upper:]' '[:lower:]')
FULL="$VERSION"
[ "$CHANNEL" != "stable" ] && FULL="$VERSION-$CHANNEL"
ARCH=$(uname -m)
NAME="jank-$FULL-build$BUILD-linux-$ARCH"
MAX_MB=${MAX_MB:-20}
echo "Jank! $FULL+$BUILD ($ARCH)"

if [ "${1:-}" != "--no-build" ]; then
    (cd app && cargo build --release --locked)
fi
rm -rf dist && mkdir -p "dist/$NAME"
STAGE="dist/$NAME"
strip -o "$STAGE/jank" app/target/release/jank
SIZE=$(stat -c %s "$STAGE/jank")
echo "program: $((SIZE / 1024)) KiB"
if [ "$SIZE" -gt $((MAX_MB * 1024 * 1024)) ]; then
    echo "error: the program is larger than $MAX_MB MiB" >&2
    exit 1
fi
mkdir -p "$STAGE/demo"
cp demo/*.jank "$STAGE/demo/"
cp LICENSE README.md CHANGELOG.md app/assets/THIRD_PARTY_LICENSES.txt "$STAGE/"
cp tools/release/jank.desktop tools/release/jank.png tools/release/install.sh "$STAGE/"
cat > "$STAGE/START_HERE.txt" <<TXT
Jank! $FULL (build $BUILD) for Linux $ARCH

Start:    ./jank            (or ./jank demo/bunnies.jank)
Install:  ./install.sh      (puts Jank! into ~/.local/bin and the applications menu)
Remove:   ./install.sh --remove

Needs OpenGL 2.0 and the usual desktop libraries (X11 or Wayland).
Licence: see LICENSE. Author: Thresis, thresiss@gmail.com, https://github.com/wwwthresis/Jank
TXT
tar -C dist -czf "dist/$NAME.tar.gz" "$NAME"

TOOL=${APPIMAGETOOL:-$(command -v appimagetool || true)}

# release notes: how to start + the CHANGELOG entry of this build
awk -v want="## $FULL (build $BUILD)" 'index($0, want) == 1 { on = 1; next } on && /^## / { exit } on { print }' CHANGELOG.md > dist/changes.md
if [ ! -s dist/changes.md ]; then
    echo "warning: CHANGELOG.md has no entry \"## $FULL (build $BUILD)\"" >&2
fi
{
    echo "## Download (Linux $ARCH)"
    echo
    if [ -n "$TOOL" ]; then
        echo "- \`Jank-$FULL-build$BUILD-$ARCH.AppImage\`: one file. \`chmod +x\` it and run."
    fi
    echo "- \`$NAME.tar.gz\`: unpack and run \`./jank\`; \`./install.sh\` adds Jank! to the applications menu. Demo scenes are in \`demo/\`."
    echo
    echo "Needs OpenGL 2.0 (software Mesa is fine). Check the files with \`sha256sum -c SHA256SUMS\`."
    echo
    echo "## What's new in build $BUILD"
    cat dist/changes.md
    printf '\nFull history: [CHANGELOG.md](https://github.com/wwwthresis/Jank/blob/main/CHANGELOG.md)\n'
} > dist/NOTES.md
rm dist/changes.md
echo "$FULL+$BUILD" > dist/VERSION

# AppImage (optional)
if [ -n "$TOOL" ]; then
    APPDIR=dist/Jank.AppDir
    mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/jank/demo" "$APPDIR/usr/share/icons/hicolor/256x256/apps"
    cp "$STAGE/jank" "$APPDIR/usr/bin/jank"
    cp demo/*.jank "$APPDIR/usr/share/jank/demo/"
    cp tools/release/jank.png "$APPDIR/jank.png"
    cp tools/release/jank.png "$APPDIR/usr/share/icons/hicolor/256x256/apps/jank.png"
    cp tools/release/jank.desktop "$APPDIR/jank.desktop"
    cat > "$APPDIR/AppRun" <<'RUN'
#!/bin/sh
HERE=$(dirname "$(readlink -f "$0")")
exec "$HERE/usr/bin/jank" "$@"
RUN
    chmod +x "$APPDIR/AppRun"
    ARCH=$ARCH "$TOOL" --no-appstream "$APPDIR" "dist/Jank-$FULL-build$BUILD-$ARCH.AppImage" > dist/appimagetool.log 2>&1 || { cat dist/appimagetool.log >&2; exit 1; }
    rm -rf "$APPDIR" dist/appimagetool.log
else
    echo "appimagetool not found: skipping the AppImage"
fi
rm -rf "$STAGE"
(cd dist && shopt -s nullglob && files=(*.tar.gz *.AppImage) && sha256sum "${files[@]}" > SHA256SUMS)
ls -la dist
