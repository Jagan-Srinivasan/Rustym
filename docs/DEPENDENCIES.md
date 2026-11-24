# Dependencies

## Runtime dependencies

These must be present on the machine where Rustym runs. `./scripts/check-deps.sh` tests for them, and
the in-app **Debug logs → Run diagnostics** page does the same from inside Rustym.

### Required

| Program | Arch package | Used for |
|---------|--------------|----------|
| `mpv` | `mpv` | Plays audio (`--no-video`), controlled over a Unix socket in `$XDG_RUNTIME_DIR` (`/run/user/UID/rustym-mpv.sock`) |
| `yt-dlp` | `yt-dlp` | Search, playlists, and the stream resolver that mpv calls |
| `ffmpeg` | `ffmpeg` | Optional: the audio filters behind pitch shift (needs rubberband), equaliser, reverb and normalisation. Python is **no longer needed**. |
| `webkit2gtk-4.1`, `gtk3` | `webkit2gtk-4.1`, `gtk3` | Rustym's own Google sign-in window (`rustym-login`). No external browser is used |

### Strongly recommended

| Program | Arch package | Used for |
|---------|--------------|----------|
| `deno` | `deno` | JavaScript runtime yt-dlp uses to solve YouTube's stream challenges. Without it playback may fail |
| `yt-dlp-ejs` | `yt-dlp-ejs` | The solver scripts that go with deno |
| `parec` | `libpulse` | Captures the system audio monitor for the visualiser. With PipeWire also install `pipewire-pulse`. Without it the app works but the visualiser stays flat |
| Noto fonts | `noto-fonts`, `noto-fonts-cjk`, `noto-fonts-extra` | Non-Latin text. Rustym loads Bengali, Devanagari, Tamil, Telugu, Kannada, Malayalam, Gujarati, Gurmukhi, Odia, Sinhala, Arabic, Thai and CJK fonts by scanning the usual Noto directories and asking `fc-match` for each script, so it works on Arch, Debian/Ubuntu and Fedora |
| D-Bus + a tray host | `dbus` | System tray (StatusNotifierItem). Needed only for "keep playing in background". KDE has this built in; GNOME needs the AppIndicator extension; waybar needs its `tray` module |
| `xdg-open` | `xdg-utils` | Fallback for opening the sign-in page |

### Optional

| Program | Used for |
|---------|----------|
| `timeout` (coreutils) | The 3-second playback test in diagnostics |

### Graphics / session

Rustym is an eframe/egui app and runs on **Wayland or X11** with OpenGL (`mesa`). On a normal desktop
install the needed libraries (`libxkbcommon`, `wayland`, `libx11`, `libxcursor`, `libxrandr`, `libxi`,
`fontconfig`) are already present.

## Build dependencies

Only needed when compiling from source.

| Need | Arch package | Notes |
|------|--------------|-------|
| Rust toolchain | `rust` (or `rustup`) | Rust **1.85 or newer** (current crate dependencies use edition 2024), edition 2021 |
| C toolchain | `base-devel` | Linker and C compiler for native crates |
| `pkg-config` | `pkgconf` | Locates system libraries |
| D-Bus headers | `dbus` | The tray crate (`ksni`) uses D-Bus |
| Internet access | — | `cargo` downloads crates on the first build |

### Rust crates (from `Cargo.toml`)

| Crate | Version | Purpose |
|-------|---------|---------|
| `eframe` | 0.29 | Window + egui integration |
| `egui_extras` | 0.29 (`image`, `http` features) | Loading thumbnails over HTTP |
| `image` | 0.25 (`jpeg`, `png`, `webp`) | Decoding thumbnails |
| `serde`, `serde_json` | 1 | Config and JSON parsing |
| `ksni` | 0.2.2 | System tray icon |

Crates are fetched automatically by Cargo; you do not install them by hand.

## Package names per distribution

`./install.sh` installs all of these for you; this table is for manual installs.

| Need | Arch | Debian / Ubuntu / Mint | Fedora | openSUSE |
|------|------|------------------------|--------|----------|
| Player | `mpv` | `mpv` | `mpv` | `mpv` |
| Stream resolver | `yt-dlp` | release binary from GitHub (apt's is outdated) | `yt-dlp` | `yt-dlp` |
| JS runtime | `deno yt-dlp-ejs` | `deno.land/install.sh` | `deno` | `deno.land/install.sh` |
| Audio filters | `ffmpeg` | `ffmpeg` | `ffmpeg-free` (or RPM Fusion `ffmpeg`) | `ffmpeg` |
| Visualiser (`parec`) | `libpulse` | `pulseaudio-utils` | `pulseaudio-utils` | `pulseaudio-utils` |
| Fonts | `noto-fonts noto-fonts-cjk noto-fonts-extra` | `fonts-noto-core fonts-noto-cjk` | `google-noto-sans-fonts google-noto-sans-cjk-fonts` | `noto-sans-fonts noto-sans-cjk-fonts` |
| Notifications | `libnotify` | `libnotify-bin` | `libnotify` | `libnotify-tools` |
| Build tools | `base-devel pkgconf` | `build-essential pkg-config` | `gcc gcc-c++ make pkgconf-pkg-config` | `gcc gcc-c++ make pkg-config` |
| D-Bus headers | `dbus` | `libdbus-1-dev` | `dbus-devel` | `dbus-1-devel` |
| GUI libs (headers) | `libxkbcommon wayland libx11 libxcursor libxrandr libxi mesa` | `libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev libgl1-mesa-dev` | `libxkbcommon-devel wayland-devel libX11-devel libXcursor-devel libXrandr-devel libXi-devel mesa-libGL-devel` | `libxkbcommon-devel wayland-devel libX11-devel libXcursor-devel libXrandr-devel libXi-devel Mesa-libGL-devel` |
| Sign-in window | `webkit2gtk-4.1 gtk3 libsoup3` | `libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev` | `webkit2gtk4.1-devel gtk3-devel libsoup3-devel` | `webkit2gtk4-devel gtk3-devel libsoup-devel` |
| Rust >= 1.85 | `rust` | **rustup** (apt's is too old on Ubuntu 24.04 / Debian 12) | `rust cargo` or rustup | rustup |

Older releases without WebKitGTK 4.1 (Ubuntu 20.04, Debian 10) can still build with
`cargo build --release --no-default-features --features mpris`; only the in-app Google sign-in is missing.

Run `./scripts/check-deps.sh` afterwards: it prints the exact command for your distribution for anything missing.
The in-app **Debug logs → Run diagnostics** page does the same and shows the right command for your distro.

Note: egui does not do complex-script shaping, so Bengali/Indic conjuncts and vowel signs are drawn unjoined, but every character renders instead of showing as an empty box.

## Version notes

- **Keep yt-dlp up to date.** YouTube changes often; old versions stop working.
- **Arch: always upgrade fully** (`pacman -Syu`) when installing mpv, otherwise `error while loading
  shared libraries: libjxl…` can occur. The same partial-upgrade problem exists on Debian (`apt full-upgrade`) and Fedora (`dnf upgrade --refresh`).
