# Now Playing layout and warning cleanup

- The Now Playing card now uses explicitly bounded artwork, metadata, and lyrics columns.
- Lyrics are rendered in one height-limited scroll area inside the card; the lyric renderer no longer creates a nested scroll area.
- Compact layouts place lyrics below the artwork/metadata row.
- YouTube's 16:9 video thumbnails are center-cropped to a 1:1 square for the Now Playing artwork tile.
- The album/artist card error kind is now used in the displayed error; unused request wrappers and the unused play-count helper were removed. Playlist and Explore API functions remain intentionally available and have narrow dead-code annotations until their UI actions are wired.

Build and verify on the target system with:

```sh
cargo fmt --check
cargo check
cargo build --release
```
