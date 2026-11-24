# Using Rustym

## First launch

1. Start Rustym (`rustym`, or from your app launcher).
2. Click **Sign in with Google**. Rustym opens your chosen browser with a **private profile** that
   belongs to Rustym only, at Google's sign-in page. Each browser gets its own profile directory, so Firefox, Chrome, and Chromium never reuse your everyday browser profile or each other's profile data.
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
- **Close to tray** — off by default, meaning closing the window quits the app. When on, closing the
  window really closes it (it is not just hidden) while the music, tray icon and media keys keep running.
  Click the tray icon, launch Rustym again, or run `rustym --show` to get the window back. Needs a tray
  host to show an icon (see [DEPENDENCIES.md](DEPENDENCIES.md)); without one a notification explains how to reopen.
- **Liked songs** — saved on this computer (`liked_cache.json`); *Clear saved liked songs* removes the copy.
- **Command line / keybindings** — `rustym --background` (start in the tray), `--show`, `--hide`, `--toggle`,
  `--play-pause`, `--next`, `--prev`, `--quit`.
- **Lyrics** — the Lyrics button in the player bar (or *Show lyrics* on Now playing) fetches and shows the
  lyrics of the song that is playing. They are never looked up on their own, and hide again on the next song.
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
├── liked_cache.json saved copy of your liked songs
├── cookies.txt      saved login (chmod 600)
├── login-profiles/<browser>/   separate private browser profile for each sign-in browser
├── debug.log        copy of the in-app log
└── mpv.log          last mpv/yt-dlp messages (useful when a track will not play)
/tmp/rustym-mpv.sock       mpv control socket
/tmp/rustym-instance.sock  single-instance socket
```

## Backing up playlists

Copy `~/.config/rustym/config.json`. Do **not** share `cookies.txt` or `login-profiles/<browser>/`.
