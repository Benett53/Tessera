#!/bin/sh
# Removes Tessera.
#   uninstall.sh          -> program, KWin script, menu entry, icons, autostart
#   uninstall.sh --purge  -> also settings, remembered windows and arrangements
set -eu

case "$(cd "$(dirname "$0")" && pwd)" in
    /usr/local/*) PREFIX=/usr/local ;;
    *) PREFIX="$HOME/.local" ;;
esac

if [ -x "$PREFIX/bin/tessera" ]; then
    "$PREFIX/bin/tessera" --quit 2>/dev/null || true
    # The KWin script lives in the user's data folder, also for a system install.
    if [ "$(id -u)" != 0 ]; then
        "$PREFIX/bin/tessera" --remove-kwin-script 2>/dev/null || true
    fi
fi

rm -f "$PREFIX/bin/tessera" "$PREFIX/share/applications/tessera.desktop"
for size in 32 48 64 128 256; do
    rm -f "$PREFIX/share/icons/hicolor/${size}x${size}/apps/tessera.png"
done

CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
rm -f "$CONFIG/autostart/tessera.desktop"
if [ "${1:-}" = "--purge" ]; then
    rm -rf "$CONFIG/tessera" "$DATA/tessera"
fi

command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q "$PREFIX/share/applications" || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "$PREFIX/share/icons/hicolor" || true

rm -rf "$PREFIX/lib/tessera"
echo "Tessera removed."
