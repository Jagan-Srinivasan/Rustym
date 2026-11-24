## Playback

One persistent `mpv --idle` process is started on first play and controlled over a single IPC
connection. Each song is a `loadfile`; the next queue entry is appended ~20 s before the end so mpv
plays it gaplessly. Tempo = mpv `speed`; pitch, EQ, reverb and normalisation = one `lavfi` filter
chain on mpv's `af`. When the queue runs out, `ytm::radio` appends YouTube Music's radio.
Stream resolution is still done by mpv's ytdl hook (yt-dlp + deno).

## Window and tray behaviour (two processes)

- The **background process** (plain `rustym`, or `rustym --background`) owns the mpv engine, the tray icon, media
  keys (MPRIS) and a control socket (`$XDG_RUNTIME_DIR/rustym-instance.sock`). It never opens a window itself.
- The **window process** (`rustym --gui`, started by the background process) is the egui UI. It sends commands as
  JSON lines (`cmd {...}`), polls `state` about 10 times a second and fetches `queue` only when `queue_ver` changed.
  Closing the window ends only this process. With *Close to tray* off the background process then quits too.
- Tray Show/Hide, `rustym --show/--hide/--toggle` and the control commands are handled by the background process.
  Because the window is a separate process, no "hide window" support is needed, so it behaves the same on X11 and
  every Wayland compositor (no Hyprland special-casing).
- Listening stats are written by the background process; the window re-reads `stats.json` while History / Stats are open.

## Prefetch and reliability

- When a song starts, the engine resolves the direct audio URLs (`yt-dlp --get-url`) of the **next 6** songs in the
  queue in background threads and keeps refreshing the list every few seconds (this also covers queue edits).
  Entries expire after 4 hours; a song is never resolved twice at once.
- Playing (or handing over for gapless playback) uses a prefetched URL when there is one, otherwise mpv's own yt-dlp
  integration. If a prefetched URL fails to start, it is dropped and the song is retried the normal way before any
  skip/give-up logic runs.

## Liked songs cache

`liked_cache.json` is written (atomically) after every successful fetch and updated on like / unlike. Opening the
Liked page shows it immediately and refreshes in the background; if the refresh fails the saved copy stays on screen.
