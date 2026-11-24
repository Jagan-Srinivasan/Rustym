# Now Playing expanded layout update

When the audio visualiser is hidden, the Now Playing card now uses the available vertical area below the page header and above the bottom player. In the wide lyrics layout, the artwork/info column and lyrics column share the expanded height; the lyrics scroll within their own card. In the compact layout, the lyrics panel receives the remaining vertical space beneath the track details. When the visualiser is visible, the existing compact card sizing is retained so the visualiser remains visible below it.

The change is confined to `src/pages.rs` and preserves the existing visualiser toggle preference and `id_salt` usage.
