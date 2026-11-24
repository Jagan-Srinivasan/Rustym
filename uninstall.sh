#!/usr/bin/env bash
# Removes everything install.sh added. Use --purge to also delete playlists/settings.
# Use --system if you installed with ./install.sh --system.
set -euo pipefail

SUDO=""; PREFIX="$HOME/.local"; DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
APPS="$DATA/applications"; ICON_ROOT="$DATA/icons/hicolor"
PURGE=0
for a in "$@"; do
    case "$a" in
        --purge)  PURGE=1 ;;
        --system) SUDO=sudo; PREFIX=/usr/local; APPS=/usr/local/share/applications; ICON_ROOT=/usr/local/share/icons/hicolor ;;
        *) echo "unknown option: $a"; exit 2 ;;
    esac
done

pkill -x rustym 2>/dev/null || true
$SUDO rm -f "$PREFIX/bin/rustym" "$PREFIX/bin/rustym-login"
$SUDO rm -f "$APPS/rustym.desktop"
$SUDO rm -f "$ICON_ROOT/scalable/apps/rustym.svg" "$ICON_ROOT"/{48x48,128x128,256x256}/apps/rustym.png
RT="${XDG_RUNTIME_DIR:-/tmp}"
rm -f "$RT/rustym-mpv.sock" "$RT/rustym-instance.sock" /tmp/rustym-mpv*.sock /tmp/rustym-instance*.sock
CFG="${XDG_CONFIG_HOME:-$HOME/.config}"
rm -f "$CFG/autostart/rustym.desktop"
command -v update-desktop-database >/dev/null 2>&1 && $SUDO update-desktop-database "$APPS" >/dev/null 2>&1 || true
command -v gtk-update-icon-cache  >/dev/null 2>&1 && $SUDO gtk-update-icon-cache -f -t "$ICON_ROOT" >/dev/null 2>&1 || true

if [ "$PURGE" = 1 ]; then
    rm -rf "$CFG/rustym"
    echo "Settings and playlists deleted."
else
    echo "Kept your playlists/settings in $CFG/rustym (run with --purge to delete)."
fi
echo "Rustym removed."
echo "mpv / yt-dlp / deno were left in place (other programs may use them)."
