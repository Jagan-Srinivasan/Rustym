#!/usr/bin/env bash
# Installs Rustym for the current user on Arch-, Debian- and Fedora-based distributions
# (CachyOS, Manjaro, EndeavourOS, Ubuntu, Mint, Pop!_OS, Debian, Fedora, Nobara, openSUSE ...).
# Works with any desktop: GNOME, KDE Plasma, XFCE, Cinnamon, MATE, LXQt, Budgie, COSMIC,
# Hyprland, Sway, i3 ... on X11 or Wayland.
#
#   ./install.sh                 build + install into ~/.local
#   ./install.sh --no-deps       skip the package-manager step (you installed dependencies yourself)
#   ./install.sh --system        install into /usr/local instead (uses sudo)
#   ./install.sh --no-login      skip the in-app Google sign-in window (no WebKitGTK needed)
set -euo pipefail
cd "$(dirname "$0")"

MIN_RUST="1.85"
DO_DEPS=1; SYSTEM=0; LOGIN=1
for a in "$@"; do
    case "$a" in
        --no-deps)  DO_DEPS=0 ;;
        --system)   SYSTEM=1 ;;
        --no-login) LOGIN=0 ;;
        -h|--help)  sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "unknown option: $a (try --help)"; exit 2 ;;
    esac
done

if [ "$(id -u)" = 0 ]; then
    echo "Do not run this as root: it installs for your user and calls sudo only when needed."; exit 1
fi

say()  { printf '\033[1m==> %s\033[0m\n' "$*"; }
warn() { printf '\033[33mwarning:\033[0m %s\n' "$*" >&2; }

# ---------- detect distribution family ----------
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
# fall back to the package manager that is present
if [ "$FAMILY" = unknown ]; then
    if   command -v pacman >/dev/null 2>&1; then FAMILY=arch
    elif command -v apt-get >/dev/null 2>&1; then FAMILY=debian
    elif command -v dnf    >/dev/null 2>&1; then FAMILY=fedora
    elif command -v zypper >/dev/null 2>&1; then FAMILY=suse
    fi
fi
say "Detected: ${PRETTY_NAME:-unknown} (family: $FAMILY), desktop: ${XDG_CURRENT_DESKTOP:-unknown}, session: ${XDG_SESSION_TYPE:-unknown}"

# ---------- install locations ----------
if [ "$SYSTEM" = 1 ]; then
    PREFIX=/usr/local; SUDO_INST=sudo
    BIN=$PREFIX/bin; APPS=$PREFIX/share/applications; ICON_ROOT=$PREFIX/share/icons/hicolor
else
    PREFIX=$HOME/.local; SUDO_INST=
    BIN=$HOME/.local/bin
    APPS=${XDG_DATA_HOME:-$HOME/.local/share}/applications
    ICON_ROOT=${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor
fi
ICONS=$ICON_ROOT/scalable/apps
CFG=${XDG_CONFIG_HOME:-$HOME/.config}

have() { command -v "$1" >/dev/null 2>&1; }

version_ge() { # version_ge A B  -> true if A >= B
    [ "$(printf '%s\n%s\n' "$2" "$1" | sort -V | head -n1)" = "$2" ]
}

# ---------- 1. system packages ----------
install_deps_arch() {
    # -Syu (not plain -S): installing mpv without upgrading can leave it out of sync with libjxl etc.
    sudo pacman -Syu --needed --noconfirm mpv yt-dlp ffmpeg base-devel pkgconf libpulse dbus xdg-utils \
        noto-fonts libxkbcommon wayland libx11 libxcursor libxrandr libxi mesa fontconfig libnotify
    sudo pacman -S --needed --noconfirm noto-fonts-cjk noto-fonts-extra || true
    sudo pacman -S --needed --noconfirm deno yt-dlp-ejs || sudo pacman -S --needed --noconfirm deno || true
    [ "$LOGIN" = 1 ] && sudo pacman -S --needed --noconfirm webkit2gtk-4.1 gtk3 libsoup3 || true
    have cargo || sudo pacman -S --needed --noconfirm rust
}

install_deps_debian() {
    sudo apt-get update
    sudo apt-get install -y mpv ffmpeg curl ca-certificates unzip build-essential pkg-config \
        pulseaudio-utils libdbus-1-dev xdg-utils libnotify-bin fontconfig \
        libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev \
        libgl1-mesa-dev libegl1-mesa-dev libfontconfig1-dev
    # Fonts: names differ between releases, so try each separately.
    sudo apt-get install -y fonts-noto-core || sudo apt-get install -y fonts-noto || true
    sudo apt-get install -y fonts-noto-cjk fonts-noto-extra || true
    if [ "$LOGIN" = 1 ]; then
        sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev \
            || warn "WebKitGTK 4.1 not available on this release (Ubuntu 20.04 / Debian 10 or older): skipping the sign-in window."
    fi
    # python3 only needed for the pip fallback of yt-dlp below
    sudo apt-get install -y python3 || true
    # Debian's own yt-dlp is old and YouTube breaks old versions - always use the current release.
    sudo apt-get remove -y yt-dlp >/dev/null 2>&1 || true
}

install_deps_fedora() {
    # ffmpeg: Fedora ships only ffmpeg-free; the full build is in RPM Fusion. ffmpeg-free is enough.
    sudo dnf install -y mpv yt-dlp ffmpeg-free gcc gcc-c++ make pkgconf-pkg-config dbus-devel \
        pulseaudio-utils xdg-utils libnotify fontconfig-devel \
        libxkbcommon-devel wayland-devel libX11-devel libXcursor-devel libXrandr-devel libXi-devel \
        mesa-libGL-devel mesa-libEGL-devel google-noto-sans-fonts curl unzip
    sudo dnf install -y mpv-libs >/dev/null 2>&1 || true
    sudo dnf install -y google-noto-sans-cjk-fonts google-noto-sans-bengali-fonts google-noto-sans-devanagari-fonts \
        google-noto-sans-tamil-fonts google-noto-sans-arabic-fonts || true
    sudo dnf install -y deno || true
    if [ "$LOGIN" = 1 ]; then
        sudo dnf install -y webkit2gtk4.1-devel gtk3-devel libsoup3-devel \
            || warn "webkit2gtk4.1-devel not available: skipping the sign-in window."
    fi
    # mpv on Fedora needs RPM Fusion for some codecs; yt-dlp streams are Opus/AAC and work without it.
}

install_deps_suse() {
    sudo zypper --non-interactive install mpv yt-dlp ffmpeg gcc gcc-c++ make pkg-config dbus-1-devel \
        pulseaudio-utils xdg-utils libnotify-tools fontconfig-devel libxkbcommon-devel wayland-devel \
        libX11-devel libXcursor-devel libXrandr-devel libXi-devel Mesa-libGL-devel Mesa-libEGL-devel \
        noto-sans-fonts curl unzip || true
    sudo zypper --non-interactive install noto-sans-cjk-fonts || true
    if [ "$LOGIN" = 1 ]; then
        sudo zypper --non-interactive install webkit2gtk4-devel gtk3-devel libsoup-devel || \
            warn "WebKitGTK dev package not found: skipping the sign-in window."
    fi
}

if [ "$DO_DEPS" = 1 ]; then
    say "Installing dependencies (sudo needed)"
    case "$FAMILY" in
        arch)   install_deps_arch ;;
        debian) install_deps_debian ;;
        fedora) install_deps_fedora ;;
        suse)   install_deps_suse ;;
        *)
            warn "Unknown distribution - cannot install packages automatically."
            echo "Install these yourself (see docs/DEPENDENCIES.md), then re-run: ./install.sh --no-deps"
            echo "  runtime: mpv yt-dlp ffmpeg deno parec(pulseaudio-utils/libpulse) Noto fonts"
            echo "  build:   rust>=$MIN_RUST, C compiler, pkg-config, libdbus-1 headers, WebKitGTK 4.1 + GTK3 headers"
            ;;
    esac
fi

# ---------- 2. user-space tools that distro packages often get wrong ----------
mkdir -p "$BIN" 2>/dev/null || true
USERBIN=$HOME/.local/bin
mkdir -p "$USERBIN"
export PATH="$USERBIN:$HOME/.deno/bin:$HOME/.cargo/bin:$PATH"

# yt-dlp: needs to be current. Distro packages are fine on Arch/Fedora, stale on Debian/Ubuntu.
if ! have yt-dlp; then
    say "Installing the current yt-dlp release into $USERBIN"
    if have curl; then
        curl -fsSL https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp -o "$USERBIN/yt-dlp" \
            && chmod +x "$USERBIN/yt-dlp" || warn "could not download yt-dlp"
    fi
    have yt-dlp || { have pip3 && pip3 install --user -U --break-system-packages yt-dlp 2>/dev/null \
                   || pip3 install --user -U yt-dlp 2>/dev/null || warn "install yt-dlp manually"; }
elif [ "$FAMILY" != arch ] && [ -w "$(command -v yt-dlp)" ]; then
    yt-dlp -U >/dev/null 2>&1 || true
fi

# deno: JavaScript runtime yt-dlp uses to solve YouTube's stream challenges.
if ! have deno; then
    say "Installing deno into ~/.deno (needed by yt-dlp)"
    if have curl; then
        curl -fsSL https://deno.land/install.sh | sh -s -- -y >/dev/null 2>&1 \
            || warn "could not install deno automatically: see https://deno.com"
    fi
fi

# ---------- 3. Rust toolchain ----------
rust_ok() {
    have cargo && have rustc || return 1
    v=$(rustc --version | awk '{print $2}')
    version_ge "$v" "$MIN_RUST"
}
if ! rust_ok; then
    say "Rust >= $MIN_RUST not found (distro packages are often older) - installing via rustup (user-local, no sudo)"
    have curl || { echo "curl is required to fetch rustup"; exit 1; }
    curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal
    # shellcheck disable=SC1091
    . "$HOME/.cargo/env"
fi
rustc --version

# ---------- 4. build ----------
say "Building (first build takes a few minutes)"
FEATURES=()
if [ "$LOGIN" = 1 ] && have pkg-config && pkg-config --exists webkit2gtk-4.1 2>/dev/null && pkg-config --exists gtk+-3.0 2>/dev/null; then
    HAVE_LOGIN=1
else
    HAVE_LOGIN=0
    [ "$LOGIN" = 1 ] && warn "WebKitGTK 4.1 / GTK3 development files not found - building without the in-app sign-in window (search, playback and local music still work)."
    FEATURES=(--no-default-features --features mpris)
fi
cargo build --release "${FEATURES[@]}"

# ---------- 5. install ----------
say "Installing files to $PREFIX"
pkill -x rustym 2>/dev/null || true
$SUDO_INST mkdir -p "$BIN" "$APPS" "$ICONS"
$SUDO_INST install -Dm755 target/release/rustym "$BIN/rustym"
[ "$HAVE_LOGIN" = 1 ] && $SUDO_INST install -Dm755 target/release/rustym-login "$BIN/rustym-login"

# Migrate from the old "ytm" name if present (copy only; old data is left untouched).
if [ -d "$CFG/ytm" ] && [ ! -e "$CFG/rustym" ]; then
    cp -a "$CFG/ytm" "$CFG/rustym"
    echo "Copied settings/playlists from $CFG/ytm to $CFG/rustym"
fi
pkill -x ytm 2>/dev/null || true
rm -f "$HOME/.local/bin/ytm" "$HOME/.local/share/applications/ytm.desktop" "$HOME/.local/share/icons/hicolor/scalable/apps/ytm.svg"

$SUDO_INST install -Dm644 packaging/rustym.svg "$ICONS/rustym.svg"
# PNG fallback for desktops/panels that cannot render SVG (rare, but e.g. some LXQt/IceWM setups)
if have rsvg-convert; then
    for s in 48 128 256; do
        d=$ICON_ROOT/${s}x${s}/apps
        $SUDO_INST mkdir -p "$d"
        rsvg-convert -w $s -h $s packaging/rustym.svg | $SUDO_INST tee "$d/rustym.png" >/dev/null
    done
fi

# Desktop entry: absolute Exec path, so it works even when ~/.local/bin is not on the launcher's PATH.
sed "s|@BIN@|$BIN|g" packaging/rustym.desktop.in | $SUDO_INST tee "$APPS/rustym.desktop" >/dev/null
$SUDO_INST chmod 644 "$APPS/rustym.desktop"

have gtk-update-icon-cache  && $SUDO_INST gtk-update-icon-cache -f -t "$ICON_ROOT" >/dev/null 2>&1 || true
have update-desktop-database && $SUDO_INST update-desktop-database "$APPS" >/dev/null 2>&1 || true
# KDE Plasma: refresh the application menu cache
have kbuildsycoca6 && kbuildsycoca6 >/dev/null 2>&1 || have kbuildsycoca5 && kbuildsycoca5 >/dev/null 2>&1 || true

# ---------- 6. make sure the shell/launchers find ~/.local/bin ----------
case ":$PATH:" in *":$BIN:"*) ;; *) NEED_PATH=1 ;; esac
if [ "${NEED_PATH:-0}" = 1 ] && [ "$SYSTEM" = 0 ]; then
    echo "Note: $BIN is not on your PATH. Add this to ~/.profile (or your shell rc) and log in again:"
    echo '      export PATH="$HOME/.local/bin:$PATH"'
fi

echo
echo "Done. Launch Rustym from your application menu, or run: rustym"
[ "$HAVE_LOGIN" = 1 ] || echo "(Built without the Google sign-in window. Install WebKitGTK 4.1 + GTK3 dev packages and re-run to enable it.)"
case "${XDG_CURRENT_DESKTOP:-}" in
    *GNOME*|*Unity*|*ubuntu*) echo "GNOME note: for the tray icon (keep playing in background) install the AppIndicator extension:"
        case "$FAMILY" in debian) echo "  sudo apt install gnome-shell-extension-appindicator" ;;
                          fedora) echo "  sudo dnf install gnome-shell-extension-appindicator" ;;
                          arch)   echo "  sudo pacman -S gnome-shell-extension-appindicator" ;; esac ;;
esac
echo "Check your setup any time with: ./scripts/check-deps.sh"
