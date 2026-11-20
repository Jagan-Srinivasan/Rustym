# Using Rustym

## First launch

1. Start Rustym (`rustym`, or from your app launcher).
2. Click **Sign in with Google**. Rustym opens your chosen browser with a **private profile** that
   belongs to Rustym only, at Google's sign-in page.
3. Sign in normally (2-step verification works). Rustym detects the login automatically, verifies it
   against YouTube Music, and loads your liked songs and playlists.
4. You can close the browser window afterwards.

Signing in is optional. Search and playback work without an account; liked songs and your YouTube Music
playlists need one.

## Sidebar pages

| Page | What it does |
|------|--------------|
| **Home** | Trending songs, recently played, "Liked Songs" mix, and your YouTube Music playlists |
| **Search** | Type a query and press Enter or **Search** (returns up to 30 results) |
| **Liked songs** | Your YouTube Music liked songs (requires sign-in) |
| **Now playing** | Current track with the audio visualiser |
| **Playlists** | Local playlists. Create, open, **Play all**, delete. Add songs from any track list |
| **Settings** | Theme, visualiser style, sign-in browser, background mode, sign out, quit |
| **Debug logs** | Diagnostics, playback test and the event log |

Tracks longer than 20 minutes are filtered out of search results so that mixes and podcasts do not
crowd out songs.

## Keyboard

| Key | Action |
|-----|--------|
| `Space` | Play / pause (when no text box has focus) |
| `Enter` | Submit search / playlist name |

## Settings

- **Theme** — Crimson, Ocean, Emerald, Violet, Sunset, Snow (light).
- **Visualiser style** — Bars, Mirror, Radial.
- **Browser used for Google sign-in** — firefox, chromium, brave, chrome, vivaldi, opera, edge.
- **Keep playing in the background when the window is closed** — off by default, meaning closing the
  window quits the app. When on, Rustym stays in the system tray; click the tray icon to show or hide
  the window. Needs a tray host (see [DEPENDENCIES.md](DEPENDENCIES.md)).
- **Sign out** — deletes `cookies.txt` and the private login profile.
- **Use cookies.txt** — an alternative to browser sign-in: put a Netscape-format cookies file at
  `~/.config/rustym/cookies.txt` (for example exported from your browser), then click this button and
  Rustym will check it against YouTube Music.

## Environment variables

| Variable | Effect |
|----------|--------|
| `RUSTYM_BROWSER` | Overrides the sign-in browser from `config.json` (e.g. `RUSTYM_BROWSER=chromium rustym`) |
| `XDG_CONFIG_HOME` | Moves the data folder to `$XDG_CONFIG_HOME/rustym` |

(Previously `YTM_BROWSER`; the old name is no longer read.)

## Files

```
~/.config/rustym/
├── config.json      settings, local playlists, recent tracks, cached remote playlist list
├── cookies.txt      saved login (chmod 600)
├── login-profile/   private browser profile used for sign-in
├── debug.log        copy of the in-app log
└── mpv.log          last mpv/yt-dlp messages (useful when a track will not play)
/tmp/rustym-mpv.sock       mpv control socket
/tmp/rustym-instance.sock  single-instance socket
```

## Backing up playlists

Copy `~/.config/rustym/config.json`. Do **not** share `cookies.txt` or `login-profile/`.
