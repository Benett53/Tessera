#!/bin/sh
# Installs Tessera from a release archive.
#   ./install.sh               -> for the current user, into ~/.local
#   sudo ./install.sh --system -> system wide, into /usr/local
# Running it again updates an existing installation; settings and saved
# windows in ~/.config/tessera and ~/.local/share/tessera are kept.
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
if [ "${1:-}" = "--system" ]; then
    PREFIX=/usr/local
else
    PREFIX="$HOME/.local"
fi

# A running service keeps the old file open; stop it so the new version is
# what runs from here on.
RUNNING=0
if pgrep -f "tessera --daemon" >/dev/null 2>&1; then
    RUNNING=1
    "$PREFIX/bin/tessera" --quit 2>/dev/null || tessera --quit 2>/dev/null || true
fi

install -Dm755 "$HERE/tessera" "$PREFIX/bin/tessera"
install -Dm755 "$HERE/uninstall.sh" "$PREFIX/lib/tessera/uninstall.sh"
install -Dm644 "$HERE/tessera.desktop" "$PREFIX/share/applications/tessera.desktop"
for size in 32 48 64 128 256; do
    install -Dm644 "$HERE/icons/tessera-$size.png" "$PREFIX/share/icons/hicolor/${size}x${size}/apps/tessera.png"
done

# An existing autostart entry points at wherever Tessera was before.
AUTOSTART="${XDG_CONFIG_HOME:-$HOME/.config}/autostart/tessera.desktop"
if [ "$PREFIX" = "$HOME/.local" ] && [ -f "$AUTOSTART" ]; then
    sed -i "s|^Exec=.*|Exec=$PREFIX/bin/tessera --daemon|" "$AUTOSTART"
fi

command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q "$PREFIX/share/applications" || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "$PREFIX/share/icons/hicolor" || true

echo "Tessera installed to $PREFIX/bin/tessera"
echo "Uninstall with: $PREFIX/lib/tessera/uninstall.sh"

# Start (or restart) the service for this session. It installs the KWin
# script and turns on start at login the first time.
if [ "$(id -u)" != 0 ]; then
    nohup setsid "$PREFIX/bin/tessera" --daemon >/dev/null 2>&1 &
    if [ "$RUNNING" = 0 ]; then
        echo "Open Tessera from the application menu to set up zones."
    fi
fi
