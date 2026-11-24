//! Your own music files: scan folders and turn them into tracks mpv can play directly.

use crate::engine::Track;
use std::path::Path;

const EXT: [&str; 9] = ["mp3", "flac", "ogg", "opus", "m4a", "aac", "wav", "wma", "mka"];

fn walk(dir: &Path, depth: u32, out: &mut Vec<Track>) {
    if depth > 8 || out.len() > 50_000 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, depth + 1, out);
        } else if p.extension().and_then(|x| x.to_str()).map(|x| EXT.contains(&x.to_lowercase().as_str())).unwrap_or(false) {
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            let folder = p.parent().and_then(|d| d.file_name()).and_then(|s| s.to_str()).unwrap_or("").to_string();
            let (artist, title) = match stem.split_once(" - ") {
                Some((a, t)) => (a.trim().to_string(), t.trim().to_string()),
                None => (folder.clone(), stem.trim().to_string()),
            };
            let path = p.to_string_lossy().to_string();
            out.push(Track { id: format!("local:{path}"), title, artist, album: folder, local: path, ..Default::default() });
        }
    }
}

pub fn scan(dirs: &[String]) -> Vec<Track> {
    let mut out = vec![];
    for d in dirs {
        let d = if let Some(rest) = d.strip_prefix("~/") {
            format!("{}/{}", std::env::var("HOME").unwrap_or_default(), rest)
        } else {
            d.clone()
        };
        walk(Path::new(&d), 0, &mut out);
    }
    out.sort_by(|a, b| (a.artist.to_lowercase(), a.title.to_lowercase()).cmp(&(b.artist.to_lowercase(), b.title.to_lowercase())));
    out
}
