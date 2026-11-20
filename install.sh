#!/usr/bin/env bash
# Installs Rustym for the current user (CachyOS / Arch).
# For other distributions see docs/INSTALL.md and docs/DEPENDENCIES.md.
set -euo pipefail
cd "$(dirname "$0")"

BIN="$HOME/.local/bin"
APPS="$HOME/.local/share/applications"
ICON_ROOT="$HOME/.local/share/icons/hicolor"
ICONS="$ICON_ROOT/scalable/apps"
CFG="${XDG_CONFIG_HOME:-$HOME/.config}"

if ! command -v pacman >/dev/null 2>&1; then
    echo "This installer only supports Arch-based systems (pacman)."
    echo "See docs/INSTALL.md for manual installation on other distributions."
    exit 1
fi

echo "==> Installing dependencies (sudo needed)"
# -Syu (not plain -S): on Arch/CachyOS installing without upgrading can leave mpv and its
# libraries (e.g. libjxl) out of sync, which breaks mpv with "error while loading shared libraries".
#   mpv, yt-dlp      playback engine + stream resolver
#   python           bundled YouTube Music client + Firefox login export (standard library only)
#   libpulse         provides `parec` for the audio visualiser (works with PipeWire via pipewire-pulse)
#   dbus             system tray (StatusNotifierItem)
#   noto-fonts       text rendering (Latin, Cyrillic, Greek, ...)
#   xdg-utils        xdg-open fallback for the sign-in page
#   base-devel, pkgconf   compiler toolchain for the build
sudo pacman -Syu --needed --noconfirm mpv yt-dlp python base-devel pkgconf noto-fonts libpulse dbus xdg-utils
# Extra scripts: Noto fonts for Indic / Arabic / CJK text (optional but recommended)
sudo pacman -S --needed --noconfirm noto-fonts-cjk || true
# GUI runtime libraries (normally already present on a desktop install)
sudo pacman -S --needed --noconfirm libxkbcommon wayland libx11 libxcursor libxrandr libxi mesa fontconfig || true
# YouTube playback needs a JS runtime for yt-dlp (deno) plus its solver scripts.
sudo pacman -S --needed --noconfirm deno yt-dlp-ejs || sudo pacman -S --needed --noconfirm deno || true
mpv --version >/dev/null 2>&1 || echo "WARNING: mpv does not start - run: sudo pacman -Syu && sudo pacman -S libjxl mpv"
if ! command -v cargo >/dev/null 2>&1; then
    sudo pacman -S --needed --noconfirm rust
fi

echo "==> Building (first build takes a few minutes)"
cargo build --release

echo "==> Installing files"
mkdir -p "$BIN" "$APPS" "$ICONS"
pkill -x rustym 2>/dev/null || true
install -Dm755 target/release/rustym "$BIN/rustym"

# Migrate from the old "ytm" name if present (copy only; the old data is left untouched).
if [ -d "$CFG/ytm" ] && [ ! -e "$CFG/rustym" ]; then
    cp -a "$CFG/ytm" "$CFG/rustym"
    echo "Copied settings/playlists from $CFG/ytm to $CFG/rustym"
fi
pkill -x ytm 2>/dev/null || true
rm -f "$BIN/ytm" "$APPS/ytm.desktop" "$ICONS/ytm.svg"

cat > "$ICONS/rustym.svg" <<'EOT'
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">
  <circle cx="32" cy="32" r="30" fill="#ff3d5e"/>
  <polygon points="25,18 25,46 48,32" fill="#ffffff"/>
</svg>
EOT

cat > "$APPS/rustym.desktop" <<EOT
[Desktop Entry]
Type=Application
Name=Rustym
Comment=Ad-free YouTube Music
Exec=$BIN/rustym
Icon=rustym
Terminal=false
Categories=AudioVideo;Audio;Player;
StartupWMClass=rustym
EOT

command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -f -t "$ICON_ROOT" >/dev/null 2>&1 || true
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APPS" || true

echo
echo "Done. Launch with: rustym   (or from your app launcher)"
echo "Closing the window quits Rustym. (Settings > Window can keep it running in the tray instead.)"
echo "Sign in from inside the app: click 'Sign in with Google'."
echo "Check your setup any time with: ./scripts/check-deps.sh"
case ":$PATH:" in
    *":$BIN:"*) ;;
    *) echo "Note: add $BIN to your PATH." ;;
esac
