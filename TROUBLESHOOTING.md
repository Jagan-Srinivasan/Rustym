# Troubleshooting

Start with **Debug logs → Run diagnostics** inside Rustym (add the playback test for a real 3-second mpv
test). **Copy report** puts a full report on your clipboard. From a terminal, `./scripts/check-deps.sh`
checks the system dependencies.

| Symptom | Likely cause | Fix |
|---------|--------------|-----|
| `error while loading shared libraries: libjxl…` from mpv | mpv and its libraries out of sync | `sudo pacman -Syu && sudo pacman -S libjxl mpv` |
| Tracks do not play / playback test fails | Old yt-dlp, or no JS runtime | Update yt-dlp (`sudo pacman -Syu`), install `deno` and `yt-dlp-ejs`. Read `~/.config/rustym/mpv.log` |
| "Could not run yt-dlp … Is it installed?" | yt-dlp missing from `PATH` | `sudo pacman -S yt-dlp` |
| "python3 not available" | Python missing | `sudo pacman -S python` |
| Sign-in window never detected | Wrong browser selected, or login not finished | Choose the browser you actually have in Settings, finish Google sign-in, wait a few seconds, then retry |
| "YouTube rejected the saved session - please sign in again" | Cookies expired or revoked | Settings → **Sign out**, then **Sign in with Google** again |
| Signed in, but Liked songs / playlists are empty | Session not accepted by YouTube Music | Run diagnostics; check the "YouTube Music session" and "liked songs" lines; sign in again |
| Visualiser is flat | `parec` missing or no audio monitor | Install `libpulse` (and `pipewire-pulse` on PipeWire). Check `pactl list sources short` shows a `.monitor` |
| Closing the window quits the app | Background mode is off (default) | Settings → enable *Keep playing in the background* |
| No tray icon in background mode | No tray host | KDE: built in. GNOME: install an AppIndicator extension. waybar: enable the `tray` module |
| Boxes instead of Tamil/Hindi/Arabic/CJK text | Noto fonts missing or in another path | Install `noto-fonts` and `noto-fonts-cjk` (Arch). On other distros see the font note in DEPENDENCIES.md |
| "Application Not Responding" on Hyprland | Window hidden while compositor expects responses | Keep Rustym updated; report with the log. The app keeps repainting while hidden for this reason |
| App will not start: another instance | Stale socket | `rm -f /tmp/rustym-instance.sock /tmp/rustym-mpv.sock` and relaunch |
| Build error about `dbus` / `pkg-config` | Missing dev packages | Install `pkgconf` and `dbus` (Arch) or `libdbus-1-dev` + `pkg-config` (Debian) |
| Build error about Rust version | Toolchain too old | Rust 1.76+ is required: `rustup update stable` |
| Old `YTM_BROWSER` setting ignored | Variable renamed | Use `RUSTYM_BROWSER` |

## Collecting information for a bug report

1. Debug logs → **Run diagnostics** → **Copy report**.
2. Attach `~/.config/rustym/mpv.log` (it does not contain your cookies).
3. **Never** attach `cookies.txt` or the `login-profile` folder.

## Resetting everything

```bash
./uninstall.sh --purge     # removes the app and ~/.config/rustym
./install.sh               # fresh install
```
