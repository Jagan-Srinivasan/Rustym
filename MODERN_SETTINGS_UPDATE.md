# Modern settings update

This update adds three practical preferences to the existing Settings page:

1. **Interface scale (80–125%)** — changes egui zoom immediately and persists to `~/.config/rustym/config.json`.
2. **Compact layout** — reduces global item spacing and button padding, useful on smaller laptop displays.
3. **Persistent Lyrics preference** — optionally starts Rustym with Lyrics enabled. Toggling Lyrics from the player bar or Now Playing card updates the saved preference so it remains consistent across queue changes and app restarts.

Existing configuration files remain compatible because `Config` uses `#[serde(default)]` and the new fields have defaults.

## Build

```bash
cargo fmt
cargo check
cargo build --release
```

Compilation could not be run in the packaging environment because the Rust toolchain is not installed there.
