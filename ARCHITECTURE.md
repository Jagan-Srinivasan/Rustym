# Architecture

Rustym is a single Rust binary plus one embedded Python script, driving three external programs.

```
 ┌────────────┐   commands    ┌──────────────┐  JSON IPC   ┌─────┐   stream URL   ┌────────┐
 │  egui UI   │ ────────────▶ │ engine thread│ ──────────▶ │ mpv │ ◀───────────── │ yt-dlp │
 │ (main.rs)  │ ◀──────────── │ (engine.rs)  │ ◀────────── │     │  (ytdl_hook)   │  + deno│
 └─────┬──────┘ shared state  └──────┬───────┘  /tmp/…sock └─────┘                └────────┘
       │                             │
       │ thumbnails (HTTP)           ├── parec ──▶ FFT ──▶ visualiser bands
       ▼                             ├── python3 rustym_api.py ──▶ music.youtube.com (library)
   YouTube CDN                       └── ksni (D-Bus tray) / hyprctl (window hiding)
```

## Source files

| File | Responsibility |
|------|----------------|
| `src/main.rs` | The egui application (`struct Rustym`): themes, sidebar, pages (Home, Search, Liked, Now playing, Playlists, Settings, Logs), search and playlist fetching, drawing the visualiser |
| `src/engine.rs` | Everything that is not drawing: config and data types, logging and health checks, yt-dlp helpers, login, mpv engine thread, audio capture and FFT, tray icon, Hyprland window control, single-instance socket |
| `src/rustym_api.py` | Minimal YouTube Music web client (standard library only), embedded in the binary with `include_str!` and passed to `python3 -c` |

## Playback

1. The UI sends a play command with a queue to the engine thread.
2. The engine starts `mpv --no-video --input-ipc-server=/tmp/rustym-mpv.sock` on
   `https://music.youtube.com/watch?v=<id>`. mpv's `ytdl_hook` calls yt-dlp, which uses **deno** to
   solve YouTube's challenges.
3. Format preference: `140/251/bestaudio[...]/bestaudio/best` (AAC 128k, then Opus, then anything).
4. The engine polls mpv over the socket for position, pause state and track end, and advances the queue.
5. mpv's stderr goes to `mpv.log` so failures can be explained in the UI.

## Login

1. `open_login` launches the chosen browser with a **dedicated profile** (`--profile` for Firefox,
   `--user-data-dir` and `--password-store=basic` for Chromium-family browsers) at Google's sign-in URL.
2. While you sign in, the UI polls `check_login`, which exports cookies for `youtube.com` and
   `google.com` into `cookies.txt` — via a small embedded Python/sqlite3 script for Firefox, or
   yt-dlp's `--cookies-from-browser` for Chromium-based browsers.
3. `verify_session` asks YouTube Music itself whether the cookies yield a signed-in library, so
   "cookies exist" is never mistaken for "logged in".
4. A dedicated profile means Google does not rotate the cookies behind your back; `rustym_api.py`
   also writes rotated `Set-Cookie` values back to `cookies.txt`, like a browser would.

## YouTube Music client (`rustym_api.py`)

`python3 rustym_api.py COOKIES.TXT MODE [ARG]` prints one JSON object.

| Mode | Result |
|------|--------|
| `liked` | `{"tracks": [...]}` — liked songs |
| `playlist ID` | `{"tracks": [...]}` |
| `playlists` | `{"playlists": [{"id","title"}, ...]}` |
| `verify` | `{"ok": true}` if still signed in |
| `diag` | health report (cookie **names** only, never values) |

Errors are returned as `{"error": "..."}`. Authentication mimics the web app: cookies plus a
`SAPISIDHASH` Authorization header.

## Visualiser

`parec -d @DEFAULT_MONITOR@ --format=s16le --rate=22050 --channels=1` streams raw audio from the default
output's monitor; a 1024-sample windowed FFT is folded into 48 bands (`BANDS`) for the Bars / Mirror /
Radial renderers.

## Window and tray behaviour

- **Single instance** — a Unix socket at `/tmp/rustym-instance.sock`; a second launch sends `show` and exits.
- **Tray** — `ksni` registers a StatusNotifierItem with show/hide and playback actions.
- **Background mode** — instead of closing, the window is hidden. On Hyprland this uses
  `hyprctl dispatch movetoworkspacesilent special:rustym,class:^(rustym)$` and brings it back with
  `movetoworkspace` + `focuswindow`. The window class (`app_id`) must therefore stay `rustym`.

## Build profile

`opt-level = 3`, `lto = true`, `strip = true` in `Cargo.toml` — small, fast binary; slower compile.
