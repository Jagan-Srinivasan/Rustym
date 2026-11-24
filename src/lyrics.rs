//! Lyrics: time-synced from LRCLIB, with YouTube Music and lyrics.ovh as plain-text fallbacks.

use crate::engine::{log, Track};
use crate::ytm;
use serde_json::Value;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Line {
    pub t: f64,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct Lyrics {
    /// Time-synced lines (empty when only plain text is available).
    pub synced: Vec<Line>,
    pub plain: String,
    pub source: String,
}

/// Strips "(Official Video)", "[Lyrics]", "- Remastered 2011" and similar noise.
pub fn clean_title(t: &str) -> String {
    let mut out = String::new();
    let mut depth = 0i32;
    for ch in t.chars() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            c if depth == 0 => out.push(c),
            _ => {}
        }
    }
    let out = out.split(" - ").next().unwrap_or("").trim().to_string();
    if out.is_empty() {
        t.trim().to_string()
    } else {
        out
    }
}

fn first_artist(a: &str) -> String {
    a.split(',').next().unwrap_or(a).split(" & ").next().unwrap_or(a).trim().to_string()
}

/// Parses `[mm:ss.xx] text` lines (several timestamps per line are allowed).
pub fn parse_lrc(s: &str) -> Vec<Line> {
    let mut out: Vec<Line> = vec![];
    for raw in s.lines() {
        let mut rest = raw.trim();
        let mut stamps: Vec<f64> = vec![];
        while rest.starts_with('[') {
            let Some(end) = rest.find(']') else { break };
            let tag = &rest[1..end];
            if let Some((m, sec)) = tag.split_once(':') {
                if let (Ok(m), Ok(sec)) = (m.trim().parse::<f64>(), sec.trim().parse::<f64>()) {
                    stamps.push(m * 60.0 + sec);
                }
            }
            rest = rest[end + 1..].trim_start();
        }
        for t in stamps {
            out.push(Line { t, text: rest.to_string() });
        }
    }
    out.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// Index of the line being sung at `pos` seconds (with `offset_ms` added to the clock).
pub fn current(lines: &[Line], pos: f64, offset_ms: i32) -> Option<usize> {
    let p = pos + offset_ms as f64 / 1000.0;
    let n = lines.partition_point(|l| l.t <= p);
    if n == 0 {
        None
    } else {
        Some(n - 1)
    }
}

fn get_json(url: &str, query: &[(&str, String)]) -> Result<Value, String> {
    let mut req = ureq::get(url)
        .timeout(Duration::from_secs(12))
        .set("User-Agent", concat!("Rustym/", env!("CARGO_PKG_VERSION"), " (https://github.com/rustym)"));
    for (k, v) in query {
        req = req.query(k, v);
    }
    match req.call() {
        Ok(r) => r.into_json::<Value>().map_err(|e| e.to_string()),
        Err(ureq::Error::Status(c, _)) => Err(format!("HTTP {c}")),
        Err(e) => Err(format!("network error: {e}")),
    }
}

fn from_lrclib(v: &Value) -> Option<Lyrics> {
    let synced = v["syncedLyrics"].as_str().map(parse_lrc).unwrap_or_default();
    let plain = v["plainLyrics"].as_str().unwrap_or("").to_string();
    if synced.is_empty() && plain.trim().is_empty() {
        return None;
    }
    Some(Lyrics { synced, plain, source: "LRCLIB".into() })
}

fn lrclib(title: &str, artist: &str, album: &str, dur: Option<f64>) -> Option<Lyrics> {
    let mut q = vec![("track_name", title.to_string()), ("artist_name", artist.to_string())];
    if !album.is_empty() {
        q.push(("album_name", album.to_string()));
    }
    if let Some(d) = dur {
        q.push(("duration", format!("{}", d.round() as i64)));
    }
    if let Ok(v) = get_json("https://lrclib.net/api/get", &q) {
        if let Some(l) = from_lrclib(&v) {
            return Some(l);
        }
    }
    // looser search; prefer a synced hit whose length is close to ours
    let q = vec![("track_name", title.to_string()), ("artist_name", artist.to_string())];
    let v = get_json("https://lrclib.net/api/search", &q).ok()?;
    let arr = v.as_array()?;
    let mut best: Option<(&Value, f64)> = None;
    for item in arr {
        let has_sync = item["syncedLyrics"].as_str().map(|s| !s.is_empty()).unwrap_or(false);
        let d = item["duration"].as_f64().unwrap_or(0.0);
        let diff = dur.map(|x| (x - d).abs()).unwrap_or(0.0);
        let score = diff + if has_sync { 0.0 } else { 1000.0 };
        if best.map_or(true, |(_, s)| score < s) {
            best = Some((item, score));
        }
    }
    best.and_then(|(v, _)| from_lrclib(v))
}

pub fn fetch(t: &Track) -> Result<Lyrics, String> {
    let title = clean_title(&t.title);
    let artist = first_artist(&t.artist);
    if let Some(l) = lrclib(&title, &artist, &t.album, t.dur) {
        log(0, "lyrics", format!("LRCLIB: {} lines for {} - {}", l.synced.len(), artist, title));
        return Ok(l);
    }
    if t.local.is_empty() {
        if let Ok(p) = ytm::lyrics(&t.id) {
            return Ok(Lyrics { synced: vec![], plain: p, source: "YouTube Music".into() });
        }
    }
    let url = format!("https://api.lyrics.ovh/v1/{}/{}", ytm::enc(&artist), ytm::enc(&title));
    if let Ok(v) = get_json(&url, &[]) {
        if let Some(s) = v["lyrics"].as_str() {
            if !s.trim().is_empty() {
                return Ok(Lyrics { synced: vec![], plain: s.to_string(), source: "lyrics.ovh".into() });
            }
        }
    }
    Err("No lyrics found for this song.".into())
}
