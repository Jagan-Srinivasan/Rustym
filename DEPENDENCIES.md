# Dependencies

## Runtime dependencies

These must be present on the machine where Rustym runs. `./scripts/check-deps.sh` tests for them, and
the in-app **Debug logs → Run diagnostics** page does the same from inside Rustym.

### Required

| Program | Arch package | Used for |
|---------|--------------|----------|
| `mpv` | `mpv` | Plays audio (`--no-video`), controlled over a Unix socket (`/tmp/rustym-mpv.sock`) |
| `yt-dlp` | `yt-dlp` | Search, playlists, and the stream resolver that mpv calls |
| `python3` | `python` | Runs the bundled `rustym_api.py` (library and liked songs) and the Firefox cookie export. **Standard library only — no `pip install` needed.** |
| A web browser | `firefox`, `chromium`, `brave`, `google-chrome`, `vivaldi`, `opera` or `microsoft-edge-stable` | Google sign-in. Rustym starts it with a private profile |

### Strongly recommended

| Program | Arch package | Used for |
|---------|--------------|----------|
| `deno` | `deno` | JavaScript runtime yt-dlp uses to solve YouTube's stream challenges. Without it playback may fail |
| `yt-dlp-ejs` | `yt-dlp-ejs` | The solver scripts that go with deno |
| `parec` | `libpulse` | Captures the system audio monitor for the visualiser. With PipeWire also install `pipewire-pulse`. Without it the app works but the visualiser stays flat |
| Noto fonts | `noto-fonts`, `noto-fonts-cjk` | Non-Latin text. Rustym looks for Tamil, Devanagari, Telugu, Malayalam, Kannada, Arabic and CJK fonts under `/usr/share/fonts/noto/` and `/usr/share/fonts/noto-cjk/` (and `google-noto-cjk`) |
| D-Bus + a tray host | `dbus` | System tray (StatusNotifierItem). Needed only for "keep playing in background". KDE has this built in; GNOME needs the AppIndicator extension; waybar needs its `tray` module |
| `xdg-open` | `xdg-utils` | Fallback for opening the sign-in page |

### Optional

| Program | Used for |
|---------|----------|
| `hyprctl` (Hyprland) | Moving the window to a special workspace when hidden to tray, and bringing it back |
| `timeout` (coreutils) | The 3-second playback test in diagnostics |

### Graphics / session

Rustym is an eframe/egui app and runs on **Wayland or X11** with OpenGL (`mesa`). On a normal desktop
install the needed libraries (`libxkbcommon`, `wayland`, `libx11`, `libxcursor`, `libxrandr`, `libxi`,
`fontconfig`) are already present.

## Build dependencies

Only needed when compiling from source.

| Need | Arch package | Notes |
|------|--------------|-------|
| Rust toolchain | `rust` (or `rustup`) | Rust **1.76 or newer** (required by egui/eframe 0.29), edition 2021 |
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

## Other distributions (best effort, untested)

Package names vary. Treat these as a starting point and run `./scripts/check-deps.sh` afterwards.

**Debian / Ubuntu**
```bash
sudo apt install mpv python3 pulseaudio-utils build-essential pkg-config \
  libdbus-1-dev libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev \
  libgl1-mesa-dev libfontconfig1-dev fonts-noto fonts-noto-cjk firefox xdg-utils
# distro yt-dlp is usually outdated - install the current one:
python3 -m pip install --user -U yt-dlp      # or download the binary from the yt-dlp releases page
# deno: https://deno.com  (install script) - then: install a Rust toolchain via https://rustup.rs
```

**Fedora**
```bash
sudo dnf install mpv yt-dlp python3 pulseaudio-utils gcc pkgconf-pkg-config dbus-devel \
  libxkbcommon-devel wayland-devel libX11-devel libXcursor-devel libXrandr-devel libXi-devel \
  mesa-libGL-devel fontconfig-devel google-noto-sans-fonts google-noto-cjk-fonts firefox deno rust cargo
```

Known limitation on non-Arch systems: the font paths in `setup_fonts` (`src/main.rs`) are hard-coded to
Arch locations. On Debian/Fedora the Indic and Arabic fonts live elsewhere, so those scripts may show
as empty boxes until the paths are adjusted. Latin text always works (egui ships its own default font).

## Version notes

- **Keep yt-dlp up to date.** YouTube changes often; old versions stop working.
- **Arch: always upgrade fully** (`pacman -Syu`) when installing mpv, otherwise `error while loading
  shared libraries: libjxl…` can occur.
