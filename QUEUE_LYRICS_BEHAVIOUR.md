# Queue lyrics and unplayable-track behaviour

- The Lyrics control is now a persistent on/off mode for the current Now Playing session.
- When enabled, lyrics are fetched for each newly-current queue item and remain visible as playback advances.
- Turning Lyrics off hides the panel and stops automatic lyric fetching for subsequent tracks.
- Tracks that fail immediately to load, or enter mpv idle without ever producing a playback position, are skipped.
- Failed items are attempted at most once per pass through the queue. This bounds retries even with Repeat All enabled; a successfully playing track resets the failure count.

Build verification was not available in the packaging environment because Cargo is not installed. Run `cargo fmt`, `cargo check`, and `cargo build --release` on the target CachyOS system.
