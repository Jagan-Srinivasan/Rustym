#!/usr/bin/env bash
# Checks that everything Rustym needs is present. Changes nothing.
# Works on Arch-, Debian-, Fedora- and openSUSE-based distributions.
# usage: ./scripts/check-deps.sh
miss=0
FAMILY=unknown
if [ -r /etc/os-release ]; then
    . /etc/os-release
    ids=" ${ID:-} ${ID_LIKE:-} "
    case "$ids" in
        *" arch "*|*" archlinux "*|*" manjaro "*|*" cachyos "*|*" endeavouros "*) FAMILY=arch ;;
        *" debian "*|*" ubuntu "*|*" linuxmint "*|*" raspbian "*)                 FAMILY=debian ;;
        *" fedora "*|*" rhel "*|*" centos "*|*" rocky "*|*" almalinux "*)          FAMILY=fedora ;;
        *" suse "*|*" opensuse "*|*" sles "*)                                      FAMILY=suse ;;
    esac
fi

# pkg <arch> <debian> <fedora> <suse> -> install command for the detected family
pkg() {
    local v
    case "$FAMILY" in
        arch)   v="$1" ;;
        debian) v="$2" ;;
        fedora) v="$3" ;;
        suse)   v="$4" ;;
        *)      echo "install '$1' (Arch name) with your package manager"; return ;;
    esac
    case "$v" in
        "("*) echo "${v#(}" | sed 's/)$//' ;;                 # a manual instruction, not a package name
        *)    case "$FAMILY" in
                  arch) echo "sudo pacman -S $v" ;; debian) echo "sudo apt install $v" ;;
                  fedora) echo "sudo dnf install $v" ;; suse) echo "sudo zypper install $v" ;;
              esac ;;
    esac
}
ok()   { printf '  [ ok ] %-16s %s\n' "$1" "$2"; }
bad()  { printf '  [MISS] %-16s %s\n' "$1" "$2"; miss=1; }
warn() { printf '  [warn] %-16s %s\n' "$1" "$2"; }

PATH="$HOME/.local/bin:$HOME/.deno/bin:$HOME/.cargo/bin:$PATH"

need() { # need <cmd> <why> <arch> <deb> <fed> <suse>
    if command -v "$1" >/dev/null 2>&1; then ok "$1" "$(command -v "$1")"
    else bad "$1" "missing - $2  ($(pkg "$3" "$4" "$5" "$6"))"; fi
}
want() {
    if command -v "$1" >/dev/null 2>&1; then ok "$1" "$(command -v "$1")"
    else warn "$1" "missing - $2  ($(pkg "$3" "$4" "$5" "$6"))"; fi
}

echo "System: ${PRETTY_NAME:-unknown}  [family: $FAMILY]  desktop: ${XDG_CURRENT_DESKTOP:-?}  session: ${XDG_SESSION_TYPE:-?}"
echo
echo "Required (runtime):"
need mpv    "audio playback engine"                      mpv    mpv    mpv    mpv
need yt-dlp "resolves YouTube streams / search"          yt-dlp "(download the latest release: https://github.com/yt-dlp/yt-dlp/releases)" yt-dlp yt-dlp
need ffmpeg "audio effects (pitch, equaliser, reverb)"   ffmpeg ffmpeg ffmpeg-free ffmpeg
if command -v yt-dlp >/dev/null 2>&1 && [ "$FAMILY" = debian ] && [ "$(command -v yt-dlp)" = /usr/bin/yt-dlp ]; then
    warn "yt-dlp age" "apt's yt-dlp is usually outdated; install the current release into ~/.local/bin (yt-dlp -U)"
fi
echo
echo "Recommended:"
want deno      "JS runtime yt-dlp uses to unlock streams" deno "(curl -fsSL https://deno.land/install.sh | sh)" deno deno
want parec     "audio visualiser (PulseAudio or pipewire-pulse)" libpulse pulseaudio-utils pulseaudio-utils pulseaudio-utils
want xdg-open  "opens links"                              xdg-utils xdg-utils xdg-utils xdg-utils
want notify-send "tray-missing notification"              libnotify libnotify-bin libnotify libnotify-tools
echo
echo "In-app Google sign-in (rustym-login):"
if command -v rustym-login >/dev/null 2>&1 || [ -x ./target/release/rustym-login ]; then ok "rustym-login" "installed"
else warn "rustym-login" "not installed - needs WebKitGTK 4.1 + GTK3 at build time"; fi
if command -v pkg-config >/dev/null 2>&1; then
    if pkg-config --exists webkit2gtk-4.1 2>/dev/null; then ok "webkit2gtk-4.1" "dev files found"
    else warn "webkit2gtk-4.1" "dev files not found  ($(pkg "webkit2gtk-4.1 gtk3 libsoup3" "libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev" "webkit2gtk4.1-devel gtk3-devel libsoup3-devel" "webkit2gtk4-devel gtk3-devel"))"; fi
fi
echo
echo "Build (only if compiling from source):"
if command -v cargo >/dev/null 2>&1; then
    v=$(rustc --version | awk '{print $2}')
    if [ "$(printf '%s\n%s\n' 1.85 "$v" | sort -V | head -n1)" = 1.85 ]; then ok "cargo" "rustc $v"
    else bad "cargo" "rustc $v is too old (need 1.85+) - install via https://rustup.rs"; fi
else bad "cargo" "missing - install via https://rustup.rs (or: $(pkg rust cargo cargo cargo))"; fi
need pkg-config "needed to find system libraries while building" pkgconf pkg-config pkgconf-pkg-config pkg-config
if command -v pkg-config >/dev/null 2>&1; then
    if pkg-config --exists dbus-1 2>/dev/null; then ok "libdbus-1" "dev files found"
    else bad "libdbus-1" "dev files missing - tray/MPRIS  ($(pkg dbus libdbus-1-dev dbus-devel dbus-1-devel))"; fi
fi
echo
if command -v mpv >/dev/null 2>&1; then
    if mpv --version >/dev/null 2>&1; then ok "mpv starts" "yes"
    else
        case "$FAMILY" in
            arch)   fix="sudo pacman -Syu && sudo pacman -S libjxl mpv" ;;
            debian) fix="sudo apt update && sudo apt full-upgrade" ;;
            fedora) fix="sudo dnf upgrade --refresh" ;;
            *)      fix="update your system" ;;
        esac
        bad "mpv starts" "mpv fails to run - try: $fix"
    fi
fi
[ -n "${XDG_SESSION_TYPE:-}" ] && ok "session" "$XDG_SESSION_TYPE"
if command -v busctl >/dev/null 2>&1 || command -v gdbus >/dev/null 2>&1 || command -v dbus-send >/dev/null 2>&1; then
    if busctl --user list 2>/dev/null | grep -q StatusNotifierWatcher || \
       dbus-send --session --print-reply --dest=org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus.NameHasOwner string:org.kde.StatusNotifierWatcher 2>/dev/null | grep -q true; then
        ok "tray host" "StatusNotifier tray available"
    else
        case "${XDG_CURRENT_DESKTOP:-}" in
            *GNOME*) warn "tray host" "none - GNOME needs the AppIndicator extension ($(pkg gnome-shell-extension-appindicator gnome-shell-extension-appindicator gnome-shell-extension-appindicator gnome-shell-extension-appindicator))" ;;
            *)       warn "tray host" "none detected - only needed for 'keep playing in background'" ;;
        esac
    fi
fi
if fc-list 2>/dev/null | grep -qi "noto"; then ok "fonts" "Noto fonts found"
else warn "fonts" "no Noto fonts - non-Latin titles may not render  ($(pkg "noto-fonts noto-fonts-cjk" "fonts-noto-core fonts-noto-cjk" "google-noto-sans-fonts google-noto-sans-cjk-fonts" "noto-sans-fonts noto-sans-cjk-fonts"))"; fi
echo
if [ $miss = 0 ]; then echo "All required dependencies are present."
else echo "Some required dependencies are missing - see [MISS] lines above."; exit 1; fi
