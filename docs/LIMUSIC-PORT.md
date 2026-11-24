# Rustym 0.4 vs LiMusic — what was ported

Built in pure Rust + egui (no Tauri / webview). Own architecture; only the *functionality* is similar.

| LiMusic feature | Rustym 0.4 | Where |
|---|---|---|
| Direct YouTube API, no ads | Native InnerTube client (cookies + SAPISIDHASH), Python removed | `ytm.rs` |
| Search that previews as you type | Debounced suggestions, tabs All/Songs/Albums/Artists/Videos, paste a YouTube link | `pages.rs` |
| Home, albums, artists, playlists | Home shelves, album / artist / playlist pages | `ytm.rs`, `pages.rs` |
| Library: likes, saved albums & artists | Liked songs, playlists, albums, artists | `pages.rs` |
| Create / edit playlists | Create, add, remove (rename/delete API in `ytm.rs`) | `ytm.rs` |
| Like, save album, subscribe | Yes | `pages.rs` |
| History in YouTube's day buckets | Yes (+ on-device history) | `pages.rs` |
| Listening stats on device | Plays, time, top songs / artists | `store.rs` |
| Radio & automix keep queue going | Radio autoplay when the queue ends | `engine.rs` |
| Queue: play next, reorder, remove | Queue panel + right-click menu | `pages.rs`, `engine.rs` |
| Shuffle / repeat | Yes | `engine.rs` |
| Gapless | Next song handed to mpv ~20 s early | `engine.rs` |
| Loudness normalisation | ffmpeg `loudnorm` | `engine.rs` |
| Tempo, pitch, reverb | speed + rubberband + aecho presets, 10-band EQ | `engine.rs` |
| Synced lyrics (several sources) | LRCLIB (synced), YouTube Music, lyrics.ovh; timing offset | `lyrics.rs` |
| Local files | Folder scan, plays directly | `local.rs` |
| Media keys / now playing | MPRIS via souvlaki | `mpris.rs` |
| Shortcuts, Ctrl+K, Ctrl+H | Yes | `pages.rs` |
| Sleep timer | Minutes or end of song | `engine.rs` |

Deliberately left out: Discord, Last.fm / ListenBrainz, video, Listen Together, device link,
karaoke, comments, self-update, Material You.

Not ported yet: word-by-word lyrics, multiple Google accounts, uploads, playlist reorder on YouTube.
Stream URL resolution still uses mpv + yt-dlp (+ deno); LiMusic's own cipher / PO-token stack is a
large separate job.
