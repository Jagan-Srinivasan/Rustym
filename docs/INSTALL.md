# Installing Rustym

## 1. Automatic install (Arch, Debian/Ubuntu, Fedora, openSUSE families)

```bash
tar -xzf rustym.tar.gz
cd rustym
./install.sh
```

`install.sh` detects your distribution from `/etc/os-release` (so derivatives such as CachyOS, Manjaro,
EndeavourOS, Ubuntu, Linux Mint, Pop!_OS, Debian, Fedora, Nobara and openSUSE all work) and then:

1. Installs the runtime and build dependencies with `pacman`, `apt` or `dnf` / `zypper` (sudo is asked for).
2. Installs the **current** `yt-dlp` release into `~/.local/bin` when the distro's copy is missing or stale
   (Debian/Ubuntu package it far behind YouTube's changes), and `deno` into `~/.deno` if no package exists.
3. Installs Rust through `rustup` if your `rustc` is older than 1.85 (Ubuntu 24.04 and Debian 12 ship older
   versions, which cannot build the current dependencies).
4. Builds with `cargo build --release`. If WebKitGTK 4.1 headers are missing it builds without the in-app
   sign-in window rather than failing.
5. Installs `rustym`, `rustym-login`, the icon and the launcher (`rustym.desktop`) into `~/.local`.

Options: `--no-deps` (skip the package manager), `--no-login` (no WebKitGTK), `--system` (install to `/usr/local`).

It works on **any desktop** (GNOME, KDE Plasma, XFCE, Cinnamon, MATE, LXQt, Budgie, COSMIC, Hyprland, Sway, i3 ...)
under X11 or Wayland. Notes per desktop:

| Desktop | Notes |
|---------|-------|
| KDE Plasma, XFCE, Cinnamon, MATE, LXQt, Budgie | The tray works out of the box. |
| GNOME | No tray by default. For "keep playing in background" install `gnome-shell-extension-appindicator` and enable it. |
| Hyprland, Sway, i3, river, niri | Add a StatusNotifier tray to your bar (waybar `tray` module). Bind `rustym --toggle`, `--play-pause`, `--next`, `--prev` to keys. |

Make sure `~/.local/bin` is on your `PATH` (Rustym also adds it for itself, so the launcher works regardless).

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

The `StartupWMClass` / app id must be `rustym` so the desktop matches the window to the launcher.

## 3. Development builds

```bash
cargo run              # debug build, faster to compile, slower to run
cargo build --release  # optimised: LTO enabled, symbols stripped
```

## Updating

Extract the new version and run `./install.sh` again. Your settings in `~/.config/rustym` are kept.

Keep **yt-dlp** current — YouTube changes often and an outdated yt-dlp is the most common reason
playback stops working. Run `yt-dlp -U` for a release-binary install, or update through your package manager
(`sudo pacman -Syu`, `sudo apt update && sudo apt upgrade`, `sudo dnf upgrade`).

## Uninstalling

```bash
./uninstall.sh            # remove the app, keep playlists/settings
./uninstall.sh --purge    # also delete ~/.config/rustym
./uninstall.sh --system   # if you installed with --system
```

The script does not remove mpv or yt-dlp, since other programs may use them.
