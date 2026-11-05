#!/bin/sh
# Installs Jank! for the current user (no root needed):
#   ~/.local/bin/jank, a menu entry and the icon. Run it from the unpacked folder.
#   ./install.sh            install or update
#   ./install.sh --remove   remove again (your projects and settings stay)
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
BIN="$HOME/.local/bin"
APPS="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
ICONS="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/256x256/apps"
if [ "$1" = "--remove" ]; then
    rm -f "$BIN/jank" "$APPS/jank.desktop" "$ICONS/jank.png"
    echo "Jank! removed. Projects, settings and autosaves were not touched."
    exit 0
fi
mkdir -p "$BIN" "$APPS" "$ICONS"
install -m 755 "$HERE/jank" "$BIN/jank"
install -m 644 "$HERE/jank.png" "$ICONS/jank.png"
sed "s|^Exec=jank|Exec=$BIN/jank|" "$HERE/jank.desktop" > "$APPS/jank.desktop"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APPS" >/dev/null 2>&1 || true
echo "Jank! installed: $BIN/jank (and in the applications menu)."
case ":$PATH:" in *":$BIN:"*) ;; *) echo "Note: $BIN is not in your PATH; start it from the menu or add it to PATH." ;; esac
