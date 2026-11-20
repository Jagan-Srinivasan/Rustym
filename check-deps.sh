#!/usr/bin/env bash
# Checks that everything Rustym needs is present. Changes nothing.
# usage: ./scripts/check-deps.sh
miss=0
ok()   { printf '  [ ok ] %-14s %s\n' "$1" "$2"; }
bad()  { printf '  [MISS] %-14s %s\n' "$1" "$2"; miss=1; }
warn() { printf '  [warn] %-14s %s\n' "$1" "$2"; }

need() {  # need <command> <arch package> <why>
    if command -v "$1" >/dev/null 2>&1; then ok "$1" "$(command -v "$1")"
    else bad "$1" "missing - $3  (Arch: sudo pacman -S $2)"; fi
}
want() {
    if command -v "$1" >/dev/null 2>&1; then ok "$1" "$(command -v "$1")"
    else warn "$1" "missing - $3  (Arch: sudo pacman -S $2)"; fi
}

echo "Required (runtime):"
need mpv    mpv    "audio playback engine"
need yt-dlp yt-dlp "resolves YouTube streams / search"
need python3 python "YouTube Music library client + Firefox login export"
echo
echo "Recommended:"
want deno   deno   "JS runtime yt-dlp uses to unlock streams"
want parec  libpulse "audio visualiser (needs PulseAudio or pipewire-pulse)"
want xdg-open xdg-utils "fallback for opening the sign-in page"
want hyprctl hyprland "only needed to hide the window to tray on Hyprland"
echo
echo "A browser for sign-in (any one):"
found=0
for b in firefox chromium brave google-chrome-stable vivaldi opera microsoft-edge-stable; do
    if command -v "$b" >/dev/null 2>&1; then ok "$b" "$(command -v "$b")"; found=1; fi
done
[ $found = 1 ] || bad "browser" "none found - install firefox or chromium"
echo
echo "Build (only if compiling from source):"
need cargo rust "Rust toolchain (edition 2021, Rust 1.76 or newer)"
need pkg-config pkgconf "needed to find system libraries while building"
echo
if command -v mpv >/dev/null 2>&1; then
    if mpv --version >/dev/null 2>&1; then ok "mpv starts" "yes"
    else bad "mpv starts" "mpv fails to run - try: sudo pacman -Syu && sudo pacman -S libjxl mpv"; fi
fi
if [ -n "${XDG_SESSION_TYPE:-}" ]; then ok "session" "$XDG_SESSION_TYPE"; fi
if fc-list 2>/dev/null | grep -qi "noto"; then ok "fonts" "Noto fonts found"
else warn "fonts" "no Noto fonts found - non-Latin titles may not render (Arch: noto-fonts noto-fonts-cjk)"; fi
echo
if [ $miss = 0 ]; then echo "All required dependencies are present."
else echo "Some required dependencies are missing - see [MISS] lines above."; exit 1; fi
