//! Listening history and stats, counted on this device (nothing is sent anywhere).

use crate::engine::{config_dir, Track};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct PlayStat {
    pub track: Track,
    pub plays: u32,
    pub secs: f64,
    pub last: u64,
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Hist {
    pub track: Track,
    pub ts: u64,
}

#[derive(Serialize, Deserialize, Default)]
struct Db {
    stats: HashMap<String, PlayStat>,
    history: Vec<Hist>,
}

fn load() -> Db {
    std::fs::read_to_string(config_dir().join("stats.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn db() -> &'static Mutex<Db> {
    static D: OnceLock<Mutex<Db>> = OnceLock::new();
    D.get_or_init(|| Mutex::new(load()))
}

/// Plays are counted by the background process while the window is a separate process, so the
/// window re-reads the file before showing history / stats.
pub fn reload() {
    let fresh = load();
    *db().lock().unwrap_or_else(|p| p.into_inner()) = fresh;
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn save(d: &Db) {
    if let Ok(s) = serde_json::to_string(d) {
        let p = config_dir().join("stats.json");
        let tmp = config_dir().join("stats.json.tmp");
        if std::fs::write(&tmp, s).is_ok() {
            let _ = std::fs::rename(tmp, p);
        }
    }
}

/// A play that lasted long enough to count (30 s or half the song).
pub fn count_play(t: &Track) {
    let mut d = db().lock().unwrap_or_else(|p| p.into_inner());
    *d = load();
    let e = d.stats.entry(t.id.clone()).or_insert_with(|| PlayStat { track: t.clone(), ..Default::default() });
    e.plays += 1;
    e.last = now();
    d.history.push(Hist { track: t.clone(), ts: now() });
    if d.history.len() > 2000 {
        let n = d.history.len() - 2000;
        d.history.drain(0..n);
    }
    save(&d);
}

/// Adds listened seconds (called when a song ends or is skipped).
pub fn add_secs(t: &Track, secs: f64) {
    if secs < 1.0 {
        return;
    }
    let mut d = db().lock().unwrap_or_else(|p| p.into_inner());
    *d = load();
    let e = d.stats.entry(t.id.clone()).or_insert_with(|| PlayStat { track: t.clone(), ..Default::default() });
    e.secs += secs;
    save(&d);
}

pub fn top_tracks(n: usize) -> Vec<PlayStat> {
    let d = db().lock().unwrap_or_else(|p| p.into_inner());
    let mut v: Vec<PlayStat> = d.stats.values().filter(|s| s.plays > 0).cloned().collect();
    v.sort_by(|a, b| b.plays.cmp(&a.plays).then(b.secs.partial_cmp(&a.secs).unwrap_or(std::cmp::Ordering::Equal)));
    v.truncate(n);
    v
}

/// (artist, plays, seconds) sorted by plays.
pub fn top_artists(n: usize) -> Vec<(String, u32, f64)> {
    let d = db().lock().unwrap_or_else(|p| p.into_inner());
    let mut m: HashMap<String, (u32, f64)> = HashMap::new();
    for s in d.stats.values() {
        let a = s.track.artist.split(", ").next().unwrap_or("").trim().to_string();
        if a.is_empty() {
            continue;
        }
        let e = m.entry(a).or_insert((0, 0.0));
        e.0 += s.plays;
        e.1 += s.secs;
    }
    let mut v: Vec<(String, u32, f64)> = m.into_iter().map(|(k, (p, s))| (k, p, s)).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v.truncate(n);
    v
}

/// (total plays, total seconds, distinct songs)
pub fn totals() -> (u32, f64, usize) {
    let d = db().lock().unwrap_or_else(|p| p.into_inner());
    (d.stats.values().map(|s| s.plays).sum(), d.stats.values().map(|s| s.secs).sum(), d.stats.len())
}

pub fn history(n: usize) -> Vec<Hist> {
    let d = db().lock().unwrap_or_else(|p| p.into_inner());
    d.history.iter().rev().take(n).cloned().collect()
}

pub fn clear() {
    let mut d = db().lock().unwrap_or_else(|p| p.into_inner());
    d.stats.clear();
    d.history.clear();
    save(&d);
}
