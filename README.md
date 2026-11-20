# Rustym

A small, fast, **ad-free YouTube Music player for Linux**, written in Rust.

Rustym gives you a native desktop UI (built with [egui](https://github.com/emilk/egui)) on top of
[mpv](https://mpv.io) and [yt-dlp](https://github.com/yt-dlp/yt-dlp). Sign in with your Google account
to get your liked songs and playlists, search for anything, build local playlists, and watch a live
audio visualiser — with no ads and no browser tab running in the background.

> Rustym is an unofficial third-party client. It is not affiliated with or endorsed by Google or YouTube.

## Features

- Ad-free playback of YouTube Music tracks via mpv + yt-dlp
- Google sign-in (uses a private browser profile; your everyday browser is not touched)
- Your **Liked songs** and **YouTube Music playlists**
- Search, a Home page with trending songs, and recently played
- Local playlists stored on your machine
- Live audio **visualiser** (Bars, Mirror, Radial)
- 6 themes: Crimson, Ocean, Emerald, Violet, Sunset, Snow
- System tray support — optionally keep playing after the window is closed
- Single-instance: launching Rustym again just shows the existing window
- Built-in **diagnostics and debug log** page that checks mpv, yt-dlp, python, deno, your login and
  does a real playback test
- Hyprland-aware window hiding for tray mode

## Quick start (Arch / CachyOS)

```bash
tar -xzf rustym.tar.gz
cd rustym
./install.sh
rustym
```

`install.sh` installs every dependency with `pacman`, builds a release binary, and installs the
binary, icon and `.desktop` launcher into `~/.local`. Then click **Sign in with Google** inside the app.

Not on Arch? See [docs/INSTALL.md](docs/INSTALL.md).

## Dependencies at a glance

| Need | Package (Arch) | Why |
|------|----------------|-----|
| **mpv** | `mpv` | playback engine |
| **yt-dlp** | `yt-dlp` | resolves streams and search results |
| **Python 3** | `python` | bundled YouTube Music client; Firefox login export (standard library only, no pip packages) |
| **deno** | `deno`, `yt-dlp-ejs` | JS runtime yt-dlp needs to unlock YouTube streams |
| **PulseAudio tools** | `libpulse` | `parec`, used by the visualiser (works with PipeWire via `pipewire-pulse`) |
| **A web browser** | `firefox` / `chromium` / … | Google sign-in |
| **D-Bus** | `dbus` | system tray |
| **Fonts** | `noto-fonts`, `noto-fonts-cjk` | text rendering, including Indic / Arabic / CJK titles |
| **Rust toolchain** | `rust` | build from source (1.76+) |

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
├── install.sh            installer for Arch / CachyOS
├── uninstall.sh          remover (--purge also deletes settings)
├── scripts/check-deps.sh dependency checker
├── src/
│   ├── main.rs           UI (egui): pages, themes, search, playlists, settings
│   ├── engine.rs         mpv engine thread, login, diagnostics, visualiser, tray, single-instance
│   └── rustym_api.py     tiny YouTube Music web client (Python stdlib only)
└── docs/
```

## Where Rustym keeps its data

Everything lives in `~/.config/rustym/` (or `$XDG_CONFIG_HOME/rustym/`):

| File | Purpose |
|------|---------|
| `config.json` | theme, volume, browser, playlists, recently played |
| `cookies.txt` | your saved YouTube login (permissions `0600`) — **keep private** |
| `login-profile/` | Rustym's private browser profile used for sign-in |
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
