# Rustym

A small, fast, **ad-free YouTube Music player for Linux**, written in Rust.

Rustym gives you a native desktop UI (built with [egui](https://github.com/emilk/egui)) on top of
[mpv](https://mpv.io) and [yt-dlp](https://github.com/yt-dlp/yt-dlp). Sign in with your Google account
to get your liked songs and playlists, search for anything, build local playlists, and watch a live
audio visualiser — with no ads and no browser tab running in the background.

> Rustym is an unofficial third-party client. It is not affiliated with or endorsed by Google or YouTube.

## Features

- Ad-free playback of YouTube Music tracks via mpv + yt-dlp; **pure-Rust YouTube Music client** (no Python)
- **v0.4 features:** search with live suggestions (songs / albums / artists / videos, paste a YouTube link),
  album / artist / playlist pages, YouTube Music home shelves, library (liked, playlists, albums, artists),
  like / unlike, create / edit YouTube playlists, history in YouTube's day buckets, on-device listening stats,
  full queue (play next, add, reorder, remove, save), shuffle + repeat, gapless playback, radio autoplay,
  synced lyrics (LRCLIB, YouTube Music, lyrics.ovh) with timing offset, tempo / pitch / 10-band EQ / reverb /
  loudness normalisation, sleep timer, local music files, media keys (MPRIS), keyboard shortcuts (Ctrl+H)
- Google sign-in (uses a private browser profile; your everyday browser is not touched)
- Your **Liked songs** and **YouTube Music playlists**
- Search, a Home page with trending songs, and recently played
- Local playlists stored on your machine
- Live audio **visualiser** (Bars, Mirror, Radial)
- 6 themes: Crimson, Ocean, Emerald, Violet, Sunset, Snow
- **Close to tray on every desktop** — the window is truly closed (X11 *and* Wayland, any DE/WM) while the
  music, tray icon and media keys keep running; `rustym --show / --toggle / --play-pause / --next / --prev / --quit` control it
- **Liked songs saved locally** — the Liked page opens instantly and works offline; it refreshes in the background
- **Next 6 songs prefetched** — their stream URLs are resolved ahead of time, so skips and track changes start at once, with an automatic fallback if a prefetched URL fails
- **Lyrics on request** — Now playing shows lyrics only after you press the Lyrics button for that song
- Single-instance: launching Rustym again just shows the existing window
- Built-in **diagnostics and debug log** page that checks mpv, yt-dlp, deno, your login and
  does a real playback test

## Quick start (Arch, Debian/Ubuntu, Fedora, openSUSE — any desktop)

```bash
tar -xzf rustym.tar.gz
cd rustym
./install.sh
rustym
```

`install.sh` detects your distribution (Arch-, Debian- or Fedora-based), installs the dependencies with
`pacman` / `apt` / `dnf` / `zypper`, builds a release binary, and installs the binary, icon and `.desktop`
launcher into `~/.local`. It works on GNOME, KDE Plasma, XFCE, Cinnamon, MATE, LXQt, Budgie, Hyprland, Sway and
others, on X11 and Wayland. Then click **Sign in with Google** inside the app.
Options: `--no-deps`, `--no-login`, `--system`. Details: [docs/INSTALL.md](docs/INSTALL.md).

Options: `--no-deps`, `--no-login`, `--system`. Details: [docs/INSTALL.md](docs/INSTALL.md).

## Ready-made packages (built by GitHub Actions)

The CI builds every package against the **oldest supported glibc (2.35)**, so one package per
family runs on both old and new distros (a binary built on a newer distro would refuse to start
on older ones with `version 'GLIBC_2.4x' not found`):

| Package | Distro family | Works on |
|---------|---------------|----------|
| `rustym_*_amd64.deb` | Debian / Ubuntu / Mint / Pop!\_OS | Ubuntu 22.04+, Debian 12+, and any newer derivative |
| `rustym-*.x86_64.rpm` | Fedora / RHEL-family | Fedora 36+ (uses `mpv`, `yt-dlp`, `ffmpeg-free`, `gtk3`, `webkit2gtk4.1`) |
| `rustym-*.pkg.tar.zst` | Arch / Manjaro / CachyOS / EndeavourOS | any current Arch (`pacman -U rustym-*.pkg.tar.zst`) |

Runtime dependency names baked into the packages (checked per distro):

| Dependency | .deb (Debian/Ubuntu) | .rpm (Fedora) | PKGBUILD (Arch) |
|------------|----------------------|----------------|------------------|
| playback | `mpv` | `mpv` | `mpv` |
| streams | `yt-dlp` | `yt-dlp` | `yt-dlp` |
| codecs/EQ | `ffmpeg` | `ffmpeg-free` | `ffmpeg` |
| sign-in helper | `libgtk-3-0`, `libwebkit2gtk-4.1-0` | `gtk3`, `webkit2gtk4.1` | `gtk3`, `webkit2gtk-4.1` |
| desktop integration | `xdg-utils` | `xdg-utils` | `xdg-utils` |
| visualiser | `pulseaudio-utils` (recommended to install) | `pulseaudio-utils` | `libpulse` |
| JS for yt-dlp | `deno` (Recommends) | — | `deno` (optdepends) |

## Dependencies at a glance

| Need | Arch | Debian / Ubuntu | Fedora | Why |
|------|------|-----------------|--------|-----|
| **mpv** | `mpv` | `mpv` | `mpv` | playback engine |
| **yt-dlp** | `yt-dlp` | release binary (apt's is stale) | `yt-dlp` | resolves streams and search |
| **ffmpeg** | `ffmpeg` | `ffmpeg` | `ffmpeg-free` | pitch / equaliser / reverb / normalisation |
| **deno** | `deno yt-dlp-ejs` | deno.land installer | `deno` | JS runtime yt-dlp needs for YouTube |
| **parec** | `libpulse` | `pulseaudio-utils` | `pulseaudio-utils` | visualiser (PipeWire via `pipewire-pulse`) |
| **WebKitGTK 4.1 + GTK3** | `webkit2gtk-4.1 gtk3` | `libwebkit2gtk-4.1-dev libgtk-3-dev` | `webkit2gtk4.1-devel gtk3-devel` | in-app Google sign-in (build time) |
| **Fonts** | `noto-fonts noto-fonts-cjk` | `fonts-noto-core fonts-noto-cjk` | `google-noto-sans-fonts` | Indic / Arabic / CJK titles |
| **Rust 1.85+** | `rust` | rustup | `rust` or rustup | build from source |

Full details, including build libraries and other distros: [docs/DEPENDENCIES.md](docs/DEPENDENCIES.md).
Check what is missing on your machine at any time:

```bash
./scripts/check-deps.sh
```

## Documentation

| Document | Contents |
|----------|----------|
| [docs/INSTALL.md](docs/INSTALL.md) | Install, build, update, uninstall, other distros |
| [docs/DEPENDENCIES.md](docs/DEPENDENCIES.md) | Every runtime and build dependency and why it is needed |
| [docs/USAGE.md](docs/USAGE.md) | Using the app, settings, shortcuts, environment variables |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | How the code is organised and how playback and login work |
| [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) | Fixes for common problems |

## Project layout

```
rustym/
├── Cargo.toml            crate manifest (package: rustym)
├── install.sh            installer for Arch / Debian / Fedora / openSUSE families
├── packaging/            icon + .desktop template used by the installer
├── uninstall.sh          remover (--purge also deletes settings)
├── scripts/check-deps.sh dependency checker
├── src/
│   ├── main.rs           UI (egui): pages, themes, search, playlists, settings
│   ├── engine.rs         mpv engine thread, login, diagnostics, visualiser, tray, single-instance
│   ├── distro.rs         distro/desktop detection (correct install hints on every distro)
│   └── ytm.rs, pages.rs, ...
└── docs/
```

## Where Rustym keeps its data

Everything lives in `~/.config/rustym/` (or `$XDG_CONFIG_HOME/rustym/`):

| File | Purpose |
|------|---------|
| `config.json` | theme, volume, browser, playlists, recently played |
| `cookies.txt` | your saved YouTube login (permissions `0600`) — **keep private** |
| `login-profiles/<browser>/` | Rustym's separate private browser profile for each sign-in browser |
| `debug.log`, `mpv.log` | diagnostics |

## Upgrading from the old "ytm" name

`install.sh` copies `~/.config/ytm` to `~/.config/rustym` the first time it runs (your old data is left
in place) and removes the old `ytm` binary and launcher. Note the environment variable was renamed from
`YTM_BROWSER` to `RUSTYM_BROWSER`.

## Privacy and security

- Your login is stored only in `~/.config/rustym/cookies.txt`. Anyone who can read that file can act as
  you on YouTube, so do not share it or commit it to version control.
- Rustym talks only to Google / YouTube servers (via yt-dlp, mpv and its bundled client) and loads
  thumbnails over HTTP. There is no telemetry.
- Use **Settings → Sign out** to delete the saved cookies and private browser profile.

## License

No license file was included with the source. Add one (for example MIT or GPL-3.0) before distributing.

## Modern interface preferences

Rustym's Settings page includes an **Interface & accessibility** section:

- Interface scale: adjust the full UI between 80% and 125%, applied immediately and saved in `config.json`.
- Compact layout: reduce spacing and button padding for smaller displays.
- Keep Lyrics enabled when Rustym starts: restore the Lyrics preference between app sessions. The player-bar Lyrics toggle updates this preference too.

These options are backward-compatible with existing configuration files; missing values use their defaults.
