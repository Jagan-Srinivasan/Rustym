#!/usr/bin/env bash
# Removes everything install.sh added. Use --purge to also delete playlists/settings.
set -euo pipefail

pkill -x rustym 2>/dev/null || true
rm -f "$HOME/.local/bin/rustym"
rm -f "$HOME/.local/share/applications/rustym.desktop"
rm -f "$HOME/.local/share/icons/hicolor/scalable/apps/rustym.svg"
rm -f /tmp/rustym-mpv.sock /tmp/rustym-instance.sock
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$HOME/.local/share/applications" || true

CFG="${XDG_CONFIG_HOME:-$HOME/.config}"
if [ "${1:-}" = "--purge" ]; then
    rm -rf "$CFG/rustym"
    echo "Settings and playlists deleted."
else
    echo "Kept your playlists/settings in $CFG/rustym (run with --purge to delete)."
fi

echo "Rustym removed."
echo "Optional: sudo pacman -Rns mpv yt-dlp   (only if nothing else needs them)"
