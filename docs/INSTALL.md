# Installing Rustym

## 1. Arch / CachyOS (recommended path)

```bash
tar -xzf rustym.tar.gz
cd rustym
./install.sh
```

The script will:

1. Run `sudo pacman -Syu --needed` for the runtime dependencies (a full upgrade is used on purpose —
   installing mpv without upgrading can leave it out of sync with libraries such as `libjxl`).
2. Install `deno` and `yt-dlp-ejs` (needed for YouTube playback).
3. Install `rust` if `cargo` is not found.
4. Run `cargo build --release` (the first build takes a few minutes).
5. Install:
   - `~/.local/bin/rustym`
   - `~/.local/share/applications/rustym.desktop`
   - `~/.local/share/icons/hicolor/scalable/apps/rustym.svg`
6. Copy settings from the old `~/.config/ytm` if you used the previous name.

Make sure `~/.local/bin` is on your `PATH`. Launch with `rustym` or from your application launcher.

## 2. Manual install (any distro)

1. Install the dependencies from [DEPENDENCIES.md](DEPENDENCIES.md) for your distribution.
2. Verify them:
   ```bash
   ./scripts/check-deps.sh
   ```
3. Build:
   ```bash
   cargo build --release
   ```
4. Run it directly:
   ```bash
   ./target/release/rustym
   ```
   or install it:
   ```bash
   install -Dm755 target/release/rustym ~/.local/bin/rustym
   ```
5. (Optional) create a launcher at `~/.local/share/applications/rustym.desktop`:
   ```ini
   [Desktop Entry]
   Type=Application
   Name=Rustym
   Comment=Ad-free YouTube Music
   Exec=/home/YOU/.local/bin/rustym
   Icon=rustym
   Terminal=false
   Categories=AudioVideo;Audio;Player;
   StartupWMClass=rustym
   ```
   and place any icon named `rustym.svg` in `~/.local/share/icons/hicolor/scalable/apps/`.

The `StartupWMClass` / app id must be `rustym`; the tray-hiding logic on Hyprland matches windows by
that class.

## 3. Development builds

```bash
cargo run              # debug build, faster to compile, slower to run
cargo build --release  # optimised: LTO enabled, symbols stripped
```

## Updating

Extract the new version and run `./install.sh` again. Your settings in `~/.config/rustym` are kept.

Keep **yt-dlp** current — YouTube changes often and an outdated yt-dlp is the most common reason
playback stops working: `sudo pacman -Syu` (or your distro's equivalent).

## Uninstalling

```bash
./uninstall.sh            # remove the app, keep playlists/settings
./uninstall.sh --purge    # also delete ~/.config/rustym
```

The script does not remove mpv or yt-dlp, since other programs may use them.
