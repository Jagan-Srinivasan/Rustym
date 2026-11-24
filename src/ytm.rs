//! Native YouTube Music client (InnerTube web-remix API) - pure Rust, no Python.
//!
//! Everything here is blocking and meant to be called from worker threads. Requests are signed
//! the way the music.youtube.com web app signs them (cookies + SAPISIDHASH). Anonymous calls
//! (search, home, albums, artists, radio, lyrics) work without signing in.

use crate::engine::{cookie_file, Track};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::{
    collections::HashMap,
    sync::OnceLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const ORIGIN: &str = "https://music.youtube.com";
const UA: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0";

// ---------------- public data ----------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Song,
    Album,
    Artist,
    Playlist,
}

/// One tile: a song, album, artist or playlist.
#[derive(Clone, Debug)]
pub struct Card {
    pub kind: Kind,
    /// videoId (Song), browseId (Album / Artist) or playlistId without "VL" (Playlist).
    pub id: String,
    pub title: String,
    pub sub: String,
    pub thumb: String,
}

impl Card {
    pub fn to_track(&self) -> Option<Track> {
        if self.kind != Kind::Song {
            return None;
        }
        Some(Track {
            id: self.id.clone(),
            title: self.title.clone(),
            artist: artist_from_plain(&self.sub),
            ..Default::default()
        })
    }
}

#[derive(Clone, Debug)]
pub struct Shelf {
    pub title: String,
    pub cards: Vec<Card>,
}

#[derive(Clone, Default, Debug)]
pub struct SearchOut {
    pub songs: Vec<Track>,
    pub albums: Vec<Card>,
    pub artists: Vec<Card>,
    pub playlists: Vec<Card>,
}

impl SearchOut {
    pub fn is_empty(&self) -> bool {
        self.songs.is_empty() && self.albums.is_empty() && self.artists.is_empty() && self.playlists.is_empty()
    }
}

/// An album, playlist or artist page.
#[derive(Clone, Default, Debug)]
pub struct Detail {
    pub title: String,
    pub sub: String,
    pub thumb: String,
    /// Album artist (from the page header), when the page has one.
    pub artist: String,
    pub tracks: Vec<Track>,
    pub shelves: Vec<Shelf>,
    /// Playlist id that can be saved to the library (albums: the OLAK5uy… audio playlist).
    pub playlist_id: String,
}

pub enum Link {
    Playlist(String),
    Video(String),
    Artist(String),
}

// ---------------- small helpers ----------------

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Percent-encodes a string for use in a URL path or query.
pub fn enc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' || b == b'~' {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{:02X}", b));
        }
    }
    o
}

fn ymd_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn client_version() -> String {
    let days = (now_secs() / 86_400) as i64 - 1;
    let (y, m, d) = ymd_from_days(days);
    format!("1.{:04}{:02}{:02}.01.00", y, m, d)
}

fn region() -> String {
    std::env::var("RUSTYM_REGION").unwrap_or_else(|_| "IN".to_string())
}

fn is_yt(d: &str) -> bool {
    let d = d.trim_start_matches('.').to_lowercase();
    d == "youtube.com" || d.ends_with(".youtube.com")
}

fn is_google(d: &str) -> bool {
    let d = d.trim_start_matches('.').to_lowercase();
    d == "google.com" || d.ends_with(".google.com") || d.starts_with("google.") || d.contains(".google.")
}

// ---------------- cookie jar ----------------

struct Rec {
    domain: String,
    path: String,
    secure: bool,
    expiry: String,
    name: String,
    value: String,
    httponly: bool,
}

struct Jar {
    recs: Vec<Rec>,
    dirty: bool,
}

impl Jar {
    fn load() -> Jar {
        let mut recs = vec![];
        if let Ok(text) = std::fs::read_to_string(cookie_file()) {
            for raw in text.lines() {
                let mut line = raw.trim_end_matches('\r').to_string();
                let mut httponly = false;
                if let Some(rest) = line.strip_prefix("#HttpOnly_") {
                    line = rest.to_string();
                    httponly = true;
                } else if line.starts_with('#') || line.trim().is_empty() {
                    continue;
                }
                let p: Vec<&str> = line.split('\t').collect();
                if p.len() < 7 {
                    continue;
                }
                if is_yt(p[0]) || is_google(p[0]) {
                    recs.push(Rec {
                        domain: p[0].to_string(),
                        path: p[2].to_string(),
                        secure: p[3].eq_ignore_ascii_case("TRUE"),
                        expiry: p[4].to_string(),
                        name: p[5].to_string(),
                        value: p[6].to_string(),
                        httponly,
                    });
                }
            }
        }
        Jar { recs, dirty: false }
    }

    /// Cookie name -> value as the browser would send them to music.youtube.com.
    /// youtube.com cookies win; google.com ones only fill in names youtube.com lacks.
    fn values(&self) -> HashMap<String, String> {
        let mut out: HashMap<String, String> = HashMap::new();
        for r in &self.recs {
            if is_google(&r.domain) && !is_yt(&r.domain) {
                out.entry(r.name.clone()).or_insert_with(|| r.value.clone());
            }
        }
        for r in &self.recs {
            if is_yt(&r.domain) {
                out.insert(r.name.clone(), r.value.clone());
            }
        }
        out
    }

    /// Applies Set-Cookie headers (YouTube rotates *PSIDCC / *PSIDTS this way).
    fn merge_set_cookie(&mut self, headers: &[String]) {
        for raw in headers {
            let mut parts = raw.split(';');
            let Some(first) = parts.next() else { continue };
            let Some((name, value)) = first.split_once('=') else { continue };
            let (name, value) = (name.trim().to_string(), value.trim().to_string());
            let mut domain = ".youtube.com".to_string();
            let mut path = "/".to_string();
            let mut secure = false;
            let mut httponly = false;
            let mut gone = value.is_empty();
            let mut expiry = "0".to_string();
            for a in parts {
                let a = a.trim();
                let (k, v) = match a.split_once('=') {
                    Some((k, v)) => (k.trim().to_lowercase(), v.trim().to_string()),
                    None => (a.to_lowercase(), String::new()),
                };
                match k.as_str() {
                    "domain" => domain = if v.starts_with('.') { v } else { format!(".{v}") },
                    "path" => path = v,
                    "secure" => secure = true,
                    "httponly" => httponly = true,
                    "max-age" => {
                        if let Ok(n) = v.parse::<i64>() {
                            if n <= 0 {
                                gone = true;
                            } else {
                                expiry = (now_secs() + n as u64).to_string();
                            }
                        }
                    }
                    "expires" => {
                        if v.contains("1970") {
                            gone = true;
                        } else if expiry == "0" {
                            expiry = (now_secs() + 180 * 86_400).to_string();
                        }
                    }
                    _ => {}
                }
            }
            if !is_yt(&domain) {
                continue;
            }
            let before = self.recs.len();
            if gone {
                self.recs.retain(|r| !(r.name == name && is_yt(&r.domain)));
                if self.recs.len() != before {
                    self.dirty = true;
                }
                continue;
            }
            let mut hit = false;
            for r in self.recs.iter_mut() {
                if r.name == name && is_yt(&r.domain) {
                    hit = true;
                    if r.value != value {
                        r.value = value.clone();
                        self.dirty = true;
                    }
                    if expiry != "0" {
                        r.expiry = expiry.clone();
                    }
                }
            }
            if !hit {
                self.recs.push(Rec { domain: domain.clone(), path, secure, expiry, name, value, httponly });
                self.dirty = true;
            }
        }
    }

    fn save(&mut self) {
        if !self.dirty {
            return;
        }
        let mut s = String::from("# Netscape HTTP Cookie File\n");
        for r in &self.recs {
            s.push_str(&format!(
                "{}{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                if r.httponly { "#HttpOnly_" } else { "" },
                r.domain,
                if r.domain.starts_with('.') { "TRUE" } else { "FALSE" },
                r.path,
                if r.secure { "TRUE" } else { "FALSE" },
                r.expiry,
                r.name,
                r.value
            ));
        }
        let dest = cookie_file();
        let tmp = dest.with_extension(format!("{}.tmp", std::process::id()));
        if std::fs::write(&tmp, s).is_ok() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
            if std::fs::rename(&tmp, &dest).is_ok() {
                self.dirty = false;
            } else {
                let _ = std::fs::remove_file(&tmp);
            }
        }
    }
}

fn sapisid_header(vals: &HashMap<String, String>, ts: u64) -> String {
    let mut parts: Vec<String> = vec![];
    for (scheme, key) in [
        ("SAPISIDHASH", "SAPISID"),
        ("SAPISID1PHASH", "__Secure-1PAPISID"),
        ("SAPISID3PHASH", "__Secure-3PAPISID"),
    ] {
        let mut v = vals.get(key).cloned();
        if v.is_none() && scheme == "SAPISIDHASH" {
            v = vals.get("__Secure-3PAPISID").cloned();
        }
        if let Some(v) = v {
            let mut h = Sha1::new();
            h.update(format!("{ts} {v} {ORIGIN}").as_bytes());
            parts.push(format!("{scheme} {ts}_{:x}", h.finalize()));
        }
    }
    parts.join(" ")
}

// ---------------- transport ----------------

/// Plain HTTPS GET for non-YouTube pages (used by the Spotify playlist import).
pub fn fetch_text(url: &str) -> Result<String, String> {
    match agent().get(url).set("Accept", "text/html,application/json").call() {
        Ok(resp) => resp.into_string().map_err(|e| format!("unreadable reply: {e}")),
        Err(ureq::Error::Status(code, _)) => Err(format!("HTTP {code}")),
        Err(e) => Err(e.to_string()),
    }
}

fn agent() -> &'static ureq::Agent {
    static A: OnceLock<ureq::Agent> = OnceLock::new();
    A.get_or_init(|| ureq::AgentBuilder::new().timeout(Duration::from_secs(25)).user_agent(UA).build())
}

/// Signs and sends one InnerTube request. `extra_query` is appended to the URL (continuations).
fn call(endpoint: &str, extra_query: &str, extra_body: Value, auth_required: bool) -> Result<Value, String> {
    let mut jar = Jar::load();
    let vals = jar.values();
    let has_login = vals.contains_key("SAPISID") || vals.contains_key("__Secure-3PAPISID");
    if auth_required && !has_login {
        return Err("Not signed in - please sign in again".into());
    }
    let ver = client_version();
    let mut body = json!({"context": {"client": {"clientName": "WEB_REMIX", "clientVersion": ver, "hl": "en", "gl": region()}}});
    if let (Some(b), Some(e)) = (body.as_object_mut(), extra_body.as_object()) {
        for (k, v) in e {
            b.insert(k.clone(), v.clone());
        }
    }
    let url = format!("{ORIGIN}/youtubei/v1/{endpoint}?prettyPrint=false{extra_query}");
    let mut last_err = String::new();
    for attempt in 0..2 {
        let mut req = agent()
            .post(&url)
            .set("Accept", "*/*")
            .set("Content-Type", "application/json")
            .set("Origin", ORIGIN)
            .set("X-Origin", ORIGIN)
            .set("Referer", "https://music.youtube.com/")
            .set("X-Youtube-Client-Name", "67")
            .set("X-Youtube-Client-Version", &ver);
        if has_login {
            let mut cookie: Vec<String> = vals.iter().map(|(k, v)| format!("{k}={v}")).collect();
            if !vals.contains_key("SOCS") {
                cookie.push("SOCS=CAI".into());
            }
            req = req
                .set("Cookie", &cookie.join("; "))
                .set("Authorization", &sapisid_header(&vals, now_secs()))
                .set("X-Goog-AuthUser", "0");
        } else {
            req = req.set("Cookie", "SOCS=CAI");
        }
        match req.send_json(body.clone()) {
            Ok(resp) => {
                let sc: Vec<String> = resp.all("set-cookie").iter().map(|s| s.to_string()).collect();
                if has_login && !sc.is_empty() {
                    jar.merge_set_cookie(&sc);
                    jar.save();
                }
                return resp.into_json::<Value>().map_err(|e| format!("YouTube Music sent an unreadable reply: {e}"));
            }
            Err(ureq::Error::Status(code, resp)) => {
                let snippet: String = resp.into_string().unwrap_or_default().chars().take(160).collect();
                if (code == 401 || code == 403) && has_login {
                    return Err(format!("YouTube rejected the saved session (HTTP {code}) - please sign in again"));
                }
                return Err(format!("YouTube Music replied HTTP {code} ({})", snippet.replace('\n', " ")));
            }
            Err(e) => {
                last_err = format!("network error: {e}");
                if attempt == 0 {
                    std::thread::sleep(Duration::from_millis(400));
                }
            }
        }
    }
    Err(last_err)
}

/// Runs an authenticated operation.
fn authed<T>(f: impl Fn() -> Result<T, String>) -> Result<T, String> {
    f()
}

fn logged_in(resp: &Value) -> Option<bool> {
    let params = resp.pointer("/responseContext/serviceTrackingParams")?.as_array()?;
    for p in params {
        if let Some(kv) = p.get("params").and_then(|x| x.as_array()) {
            for e in kv {
                if e.get("key").and_then(|k| k.as_str()) == Some("logged_in") {
                    let v = e.get("value").map(|v| v.to_string().trim_matches('"').to_string()).unwrap_or_default();
                    return Some(v == "1");
                }
            }
        }
    }
    None
}

fn need_login(resp: &Value, what: &str) -> Result<(), String> {
    if logged_in(resp) == Some(false) {
        return Err(format!("YouTube says this session is signed out ({what}) - please sign in again"));
    }
    Ok(())
}

// ---------------- JSON walking ----------------

fn walk<'a>(v: &'a Value, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(m) => {
            out.push(v);
            for x in m.values() {
                walk(x, out);
            }
        }
        Value::Array(a) => {
            for x in a {
                walk(x, out);
            }
        }
        _ => {}
    }
}

/// Every value stored under `key` anywhere in the tree (document order).
fn find_all<'a>(v: &'a Value, key: &str) -> Vec<&'a Value> {
    let mut all = vec![];
    walk(v, &mut all);
    all.into_iter().filter_map(|o| o.get(key)).collect()
}

fn runs_text(node: &Value) -> String {
    match node.get("runs").and_then(|r| r.as_array()) {
        Some(a) => a.iter().filter_map(|r| r.get("text").and_then(|t| t.as_str())).collect::<Vec<_>>().join(""),
        None => node.get("simpleText").and_then(|t| t.as_str()).unwrap_or("").to_string(),
    }
}

fn text_at(v: &Value, ptr: &str) -> String {
    v.pointer(ptr).map(runs_text).unwrap_or_default()
}

fn thumb_of(v: &Value) -> String {
    for t in find_all(v, "thumbnails") {
        if let Some(u) = t.as_array().and_then(|a| a.last()).and_then(|l| l.get("url")).and_then(|u| u.as_str()) {
            return u.to_string();
        }
    }
    String::new()
}

fn parse_dur(t: &str) -> Option<f64> {
    let t = t.trim();
    if !t.contains(':') {
        return None;
    }
    let mut sec = 0u64;
    for part in t.split(':') {
        sec = sec * 60 + part.trim().parse::<u64>().ok()?;
    }
    Some(sec as f64)
}

const LABELS: [&str; 9] = ["Song", "Video", "Single", "Album", "EP", "Playlist", "Artist", "Podcast", "Episode"];

fn artist_from_plain(plain: &str) -> String {
    for seg in plain.split(" • ") {
        let s = seg.trim();
        if s.is_empty() || LABELS.contains(&s) || parse_dur(s).is_some() {
            continue;
        }
        if s.ends_with(" plays") || s.ends_with(" views") || s.ends_with("subscribers") {
            continue;
        }
        return s.to_string();
    }
    String::new()
}

struct Meta {
    artist: String,
    artist_id: String,
    album: String,
    album_id: String,
    dur: Option<f64>,
}

fn meta_from_runs(runs: &[&Value]) -> Meta {
    let mut artists: Vec<String> = vec![];
    let mut m = Meta { artist: String::new(), artist_id: String::new(), album: String::new(), album_id: String::new(), dur: None };
    let mut plain = String::new();
    for r in runs {
        let text = r.get("text").and_then(|t| t.as_str()).unwrap_or("");
        plain.push_str(text);
        let bid = r.pointer("/navigationEndpoint/browseEndpoint/browseId").and_then(|b| b.as_str()).unwrap_or("");
        if bid.starts_with("UC") {
            artists.push(text.to_string());
            if m.artist_id.is_empty() {
                m.artist_id = bid.to_string();
            }
        } else if bid.starts_with("MPRE") {
            m.album = text.to_string();
            m.album_id = bid.to_string();
        } else if let Some(d) = parse_dur(text) {
            m.dur = Some(d);
        }
    }
    m.artist = if artists.is_empty() { artist_from_plain(&plain) } else { artists.join(", ") };
    m
}

fn col_runs<'a>(r: &'a Value, i: usize) -> Vec<&'a Value> {
    r.get("flexColumns")
        .and_then(|c| c.get(i))
        .and_then(|c| c.pointer("/musicResponsiveListItemFlexColumnRenderer/text/runs"))
        .and_then(|r| r.as_array())
        .map(|a| a.iter().collect())
        .unwrap_or_default()
}

fn first_str<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    let mut all = vec![];
    walk(v, &mut all);
    for o in all {
        if let Some(s) = o.get(key).and_then(|x| x.as_str()) {
            return Some(s);
        }
    }
    None
}

// ---------------- row parsers ----------------

/// The browseId a list row / tile navigates to, if any.
fn nav_browse(r: &Value) -> Option<&str> {
    r.pointer("/navigationEndpoint/browseEndpoint/browseId").and_then(|b| b.as_str())
}

fn track_from_row(r: &Value) -> Option<Track> {
    if r.get("musicItemRendererDisplayPolicy").map(|v| v.to_string().contains("GREY_OUT")).unwrap_or(false) {
        return None;
    }
    if nav_browse(r).is_some() {
        return None; // album / artist / playlist row
    }
    let vid = r
        .pointer("/playlistItemData/videoId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| first_str(r, "videoId").map(|s| s.to_string()))?;
    let title: String = col_runs(r, 0).iter().filter_map(|x| x.get("text").and_then(|t| t.as_str())).collect();
    if title.is_empty() {
        return None;
    }
    let mut meta = meta_from_runs(&col_runs(r, 1));
    // a third column often holds the album
    if meta.album.is_empty() {
        let c2 = col_runs(r, 2);
        if !c2.is_empty() {
            let m2 = meta_from_runs(&c2);
            meta.album = c2.iter().filter_map(|x| x.get("text").and_then(|t| t.as_str())).collect::<String>();
            meta.album_id = m2.album_id;
        }
    }
    if meta.dur.is_none() {
        meta.dur = r
            .pointer("/fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text")
            .map(runs_text)
            .and_then(|t| parse_dur(&t));
    }
    let set_id = first_str(r, "playlistSetVideoId").or_else(|| first_str(r, "setVideoId")).unwrap_or("").to_string();
    Some(Track {
        id: vid,
        title,
        artist: meta.artist,
        dur: meta.dur,
        album: meta.album,
        artist_id: meta.artist_id,
        album_id: meta.album_id,
        set_id,
        local: String::new(),
    })
}

fn kind_of_browse(b: &str) -> Option<(Kind, String)> {
    if b.starts_with("MPRE") {
        Some((Kind::Album, b.to_string()))
    } else if b.starts_with("UC") {
        Some((Kind::Artist, b.to_string()))
    } else if b == "VLLM" || b == "VLSE" {
        None
    } else if let Some(rest) = b.strip_prefix("VL") {
        Some((Kind::Playlist, rest.to_string()))
    } else {
        None
    }
}

fn card_from_row(r: &Value) -> Option<Card> {
    let (kind, id) = kind_of_browse(nav_browse(r)?)?;
    let title: String = col_runs(r, 0).iter().filter_map(|x| x.get("text").and_then(|t| t.as_str())).collect();
    if title.is_empty() {
        return None;
    }
    let sub: String = col_runs(r, 1).iter().filter_map(|x| x.get("text").and_then(|t| t.as_str())).collect();
    Some(Card { kind, id, title, sub, thumb: thumb_of(r) })
}

fn card_from_two_row(r: &Value) -> Option<Card> {
    let title = text_at(r, "/title");
    if title.is_empty() {
        return None;
    }
    let sub = text_at(r, "/subtitle");
    let thumb = thumb_of(r);
    if let Some(b) = nav_browse(r) {
        let (kind, id) = kind_of_browse(b)?;
        return Some(Card { kind, id, title, sub, thumb });
    }
    if let Some(v) = r.pointer("/navigationEndpoint/watchEndpoint/videoId").and_then(|v| v.as_str()) {
        return Some(Card { kind: Kind::Song, id: v.to_string(), title, sub, thumb });
    }
    if let Some(p) = r.pointer("/navigationEndpoint/watchPlaylistEndpoint/playlistId").and_then(|v| v.as_str()) {
        return Some(Card { kind: Kind::Playlist, id: p.to_string(), title, sub, thumb });
    }
    None
}

fn tracks_in(v: &Value) -> Vec<Track> {
    find_all(v, "musicResponsiveListItemRenderer").into_iter().filter_map(track_from_row).collect()
}

fn cards_in(v: &Value) -> Vec<Card> {
    let mut out: Vec<Card> = vec![];
    for r in find_all(v, "musicTwoRowItemRenderer") {
        if let Some(c) = card_from_two_row(r) {
            out.push(c);
        }
    }
    for r in find_all(v, "musicResponsiveListItemRenderer") {
        if let Some(c) = card_from_row(r) {
            out.push(c);
        }
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|c| seen.insert(format!("{:?}{}", c.kind, c.id)));
    out
}

fn dedup(tracks: Vec<Track>) -> Vec<Track> {
    let mut seen = std::collections::HashSet::new();
    tracks.into_iter().filter(|t| seen.insert(t.id.clone())).collect()
}

fn next_token(resp: &Value) -> Option<String> {
    let mut all = vec![];
    walk(resp, &mut all);
    for d in all {
        if let Some(c) = d.pointer("/nextContinuationData/continuation").and_then(|c| c.as_str()) {
            return Some(c.to_string());
        }
        if let Some(t) = d.pointer("/continuationCommand/token").and_then(|c| c.as_str()) {
            return Some(t.to_string());
        }
    }
    None
}

fn continuation(tok: &str) -> Result<Value, String> {
    let q = enc(tok);
    call("browse", &format!("&ctoken={q}&continuation={q}&type=next"), json!({}), false)
}

fn header_of(resp: &Value) -> (String, String, String) {
    for key in ["musicResponsiveHeaderRenderer", "musicDetailHeaderRenderer", "musicImmersiveHeaderRenderer", "musicVisualHeaderRenderer"] {
        if let Some(h) = find_all(resp, key).into_iter().next() {
            let title = text_at(h, "/title");
            let mut sub = text_at(h, "/subtitle");
            if sub.is_empty() {
                sub = text_at(h, "/straplineTextOne");
            }
            return (title, sub, thumb_of(h));
        }
    }
    (String::new(), String::new(), String::new())
}

fn header_artist(resp: &Value) -> String {
    for key in ["musicResponsiveHeaderRenderer", "musicDetailHeaderRenderer"] {
        if let Some(h) = find_all(resp, key).into_iter().next() {
            let a = text_at(h, "/straplineTextOne");
            if !a.is_empty() {
                return a;
            }
            if let Some(runs) = h.pointer("/subtitle/runs").and_then(|r| r.as_array()) {
                let rr: Vec<&Value> = runs.iter().collect();
                return meta_from_runs(&rr).artist;
            }
        }
    }
    String::new()
}

fn carousel_shelves(resp: &Value) -> Vec<Shelf> {
    let mut out = vec![];
    for key in ["musicCarouselShelfRenderer", "musicImmersiveCarouselShelfRenderer"] {
        for s in find_all(resp, key) {
            let title = s
                .pointer("/header/musicCarouselShelfBasicHeaderRenderer/title")
                .map(runs_text)
                .unwrap_or_default();
            let mut cards: Vec<Card> = vec![];
            if let Some(items) = s.get("contents").and_then(|c| c.as_array()) {
                for it in items {
                    if let Some(r) = it.get("musicTwoRowItemRenderer") {
                        if let Some(c) = card_from_two_row(r) {
                            cards.push(c);
                        }
                    } else if let Some(r) = it.get("musicResponsiveListItemRenderer") {
                        if let Some(c) = card_from_row(r) {
                            cards.push(c);
                        } else if let Some(t) = track_from_row(r) {
                            cards.push(Card { kind: Kind::Song, id: t.id, title: t.title, sub: t.artist, thumb: thumb_of(r) });
                        }
                    }
                }
            }
            if !cards.is_empty() {
                out.push(Shelf { title: if title.is_empty() { "More".into() } else { title }, cards });
            }
        }
    }
    out
}

// ---------------- public API: discovery ----------------

pub fn parse_link(s: &str) -> Option<Link> {
    let s = s.trim();
    if !(s.starts_with("http://") || s.starts_with("https://")) {
        return None;
    }
    let param = |k: &str| -> Option<String> {
        let q = s.split_once('?')?.1;
        for kv in q.split('&') {
            if let Some((a, b)) = kv.split_once('=') {
                if a == k && !b.is_empty() {
                    return Some(b.split('#').next().unwrap_or(b).to_string());
                }
            }
        }
        None
    };
    if let Some(l) = param("list") {
        if !l.starts_with("RD") || param("v").is_none() {
            return Some(Link::Playlist(l));
        }
    }
    if let Some(v) = param("v") {
        return Some(Link::Video(v));
    }
    if s.contains("youtu.be/") {
        let id = s.split("youtu.be/").nth(1)?.split(['?', '&', '/']).next()?.to_string();
        if !id.is_empty() {
            return Some(Link::Video(id));
        }
    }
    if let Some(rest) = s.split("/channel/").nth(1) {
        let id = rest.split(['?', '&', '/']).next()?.to_string();
        if id.starts_with("UC") {
            return Some(Link::Artist(id));
        }
    }
    None
}

pub fn suggestions(q: &str) -> Result<Vec<String>, String> {
    let resp = call("music/get_search_suggestions", "", json!({"input": q}), false)?;
    let mut out: Vec<String> = vec![];
    for key in ["searchSuggestionRenderer", "historySuggestionRenderer"] {
        for r in find_all(&resp, key) {
            let t = text_at(r, "/suggestion");
            if !t.is_empty() && !out.contains(&t) {
                out.push(t);
            }
        }
    }
    out.truncate(8);
    Ok(out)
}

/// filter: 0 all, 1 songs, 2 albums, 3 artists, 4 videos
pub fn search(q: &str, filter: usize) -> Result<SearchOut, String> {
    let params = match filter {
        1 => Some("EgWKAQIIAWoMEA4QChADEAQQCRAF"),
        2 => Some("EgWKAQIYAWoMEA4QChADEAQQCRAF"),
        3 => Some("EgWKAQIgAWoMEA4QChADEAQQCRAF"),
        4 => Some("EgWKAQIQAWoMEA4QChADEAQQCRAF"),
        _ => None,
    };
    let mut body = json!({"query": q});
    if let Some(p) = params {
        body["params"] = json!(p);
    }
    let resp = call("search", "", body, false)?;
    let mut out = SearchOut::default();
    out.songs = dedup(tracks_in(&resp));
    for c in cards_in(&resp) {
        match c.kind {
            Kind::Album => out.albums.push(c),
            Kind::Artist => out.artists.push(c),
            Kind::Playlist => out.playlists.push(c),
            Kind::Song => {}
        }
    }
    if out.is_empty() {
        return Err("No results.".into());
    }
    Ok(out)
}

pub fn home() -> Result<Vec<Shelf>, String> {
    let resp = authed(|| call("browse", "", json!({"browseId": "FEmusic_home"}), false))?;
    let mut shelves = carousel_shelves(&resp);
    let mut tok = next_token(&resp);
    for _ in 0..2 {
        let Some(t) = tok.take() else { break };
        match continuation(&t) {
            Ok(more) => {
                shelves.extend(carousel_shelves(&more));
                tok = next_token(&more);
            }
            Err(_) => break,
        }
    }
    if shelves.is_empty() {
        return Err("YouTube Music returned an empty home page.".into());
    }
    Ok(shelves)
}

#[allow(dead_code)] // Kept as a supported API operation for the Explore page.
pub fn explore_charts() -> Result<Vec<Shelf>, String> {
    let resp = call("browse", "", json!({"browseId": "FEmusic_explore"}), false)?;
    let s = carousel_shelves(&resp);
    if s.is_empty() {
        Err("Nothing to explore right now.".into())
    } else {
        Ok(s)
    }
}

fn fetch_tracks(browse_id: &str, limit: usize) -> Result<Detail, String> {
    let resp = authed(|| call("browse", "", json!({"browseId": browse_id}), false))?;
    let (title, mut sub, thumb) = header_of(&resp);
    let mut tracks = tracks_in(&resp);
    if tracks.is_empty() {
        need_login(&resp, "empty library response")?;
    }
    let mut cur = resp.clone();
    for _ in 0..12 {
        if tracks.len() >= limit {
            break;
        }
        let Some(tok) = next_token(&cur) else { break };
        let more = continuation(&tok)?;
        let t = tracks_in(&more);
        if t.is_empty() {
            break;
        }
        tracks.extend(t);
        cur = more;
    }
    let tracks = dedup(tracks);
    let playlist_id = find_all(&resp, "playlistId")
        .into_iter()
        .filter_map(|v| v.as_str())
        .find(|s| s.starts_with("OLAK"))
        .unwrap_or("")
        .to_string();
    if sub.is_empty() {
        sub = format!("{} songs", tracks.len());
    }
    let artist = header_artist(&resp);
    Ok(Detail { title, sub, thumb, artist, tracks, shelves: vec![], playlist_id })
}

pub fn liked() -> Result<Vec<Track>, String> {
    Ok(fetch_tracks("VLLM", 2000)?.tracks)
}

pub fn playlist(id: &str) -> Result<Detail, String> {
    let bid = if id.starts_with("VL") || id.starts_with("MPRE") { id.to_string() } else { format!("VL{id}") };
    let mut d = fetch_tracks(&bid, 2000)?;
    if d.playlist_id.is_empty() {
        d.playlist_id = id.trim_start_matches("VL").to_string();
    }
    Ok(d)
}

pub fn album(browse_id: &str) -> Result<Detail, String> {
    let mut d = fetch_tracks(browse_id, 500)?;
    // album rows have no artist column: take it from the page header
    let artist = d.artist.clone();
    for t in d.tracks.iter_mut() {
        if t.artist.is_empty() {
            t.artist = artist.clone();
        }
        if t.album.is_empty() {
            t.album = d.title.clone();
        }
        t.album_id = browse_id.to_string();
    }
    Ok(d)
}

pub fn artist(browse_id: &str) -> Result<Detail, String> {
    let resp = call("browse", "", json!({"browseId": browse_id}), false)?;
    let (title, sub, thumb) = header_of(&resp);
    let tracks = find_all(&resp, "musicShelfRenderer")
        .into_iter()
        .next()
        .map(|s| dedup(tracks_in(s)))
        .unwrap_or_default();
    let shelves = carousel_shelves(&resp);
    if tracks.is_empty() && shelves.is_empty() {
        return Err("That artist page is empty.".into());
    }
    Ok(Detail { title, sub, thumb, artist: String::new(), tracks, shelves, playlist_id: String::new() })
}

// ---------------- public API: library ----------------

pub fn library_playlists() -> Result<Vec<Card>, String> {
    authed(|| {
        let resp = call("browse", "", json!({"browseId": "FEmusic_liked_playlists"}), true)?;
        let mut out: Vec<Card> = cards_in(&resp).into_iter().filter(|c| c.kind == Kind::Playlist).collect();
        if out.is_empty() {
            need_login(&resp, "empty playlists response")?;
        }
        let mut cur = resp;
        for _ in 0..5 {
            let Some(tok) = next_token(&cur) else { break };
            let more = continuation(&tok)?;
            let new: Vec<Card> = cards_in(&more)
                .into_iter()
                .filter(|c| c.kind == Kind::Playlist && !out.iter().any(|o| o.id == c.id))
                .collect();
            if new.is_empty() {
                break;
            }
            out.extend(new);
            cur = more;
        }
        Ok(out)
    })
}

pub fn library_albums() -> Result<Vec<Card>, String> {
    authed(|| {
        let resp = call("browse", "", json!({"browseId": "FEmusic_liked_albums"}), true)?;
        let out: Vec<Card> = cards_in(&resp).into_iter().filter(|c| c.kind == Kind::Album).collect();
        if out.is_empty() {
            need_login(&resp, "empty albums response")?;
        }
        Ok(out)
    })
}

pub fn library_artists() -> Result<Vec<Card>, String> {
    authed(|| {
        let resp = call("browse", "", json!({"browseId": "FEmusic_library_corpus_artists"}), true)?;
        let out: Vec<Card> = cards_in(&resp).into_iter().filter(|c| c.kind == Kind::Artist).collect();
        if out.is_empty() {
            need_login(&resp, "empty artists response")?;
        }
        Ok(out)
    })
}

/// YouTube Music's own history, in its day buckets ("Today", "Yesterday", …).
pub fn history() -> Result<Vec<(String, Vec<Track>)>, String> {
    authed(|| {
        let resp = call("browse", "", json!({"browseId": "FEmusic_history"}), true)?;
        let mut out: Vec<(String, Vec<Track>)> = vec![];
        for s in find_all(&resp, "musicShelfRenderer") {
            let title = text_at(s, "/title");
            let tracks = tracks_in(s);
            if !tracks.is_empty() {
                out.push((if title.is_empty() { "Earlier".into() } else { title }, tracks));
            }
        }
        if out.is_empty() {
            need_login(&resp, "empty history response")?;
        }
        Ok(out)
    })
}

/// The signed-in Google account's display name and avatar URL.
pub fn account() -> Result<(String, String), String> {
    authed(|| {
        let resp = call(
            "account/account_menu",
            "",
            json!({"deviceTheme": "DEVICE_THEME_SELECTED", "userInterfaceTheme": "USER_INTERFACE_THEME_DARK"}),
            true,
        )?;
        let hs = find_all(&resp, "activeAccountHeaderRenderer");
        let Some(h) = hs.first() else {
            return Err("no account info in reply".into());
        };
        let name = text_at(h, "/accountName");
        let photo = thumb_of(h);
        if name.is_empty() && photo.is_empty() {
            return Err("empty account info".into());
        }
        Ok((name, photo))
    })
}

pub fn verify() -> Result<(), String> {
    authed(|| {
        let resp = call("browse", "", json!({"browseId": "FEmusic_liked_playlists"}), true)?;
        need_login(&resp, "verify")?;
        if logged_in(&resp).is_none() && cards_in(&resp).iter().all(|c| c.kind != Kind::Playlist) {
            return Err("YouTube rejected the saved session (no library in reply) - please sign in again".into());
        }
        Ok(())
    })
}

// ---------------- public API: playback helpers ----------------

/// Radio / automix: songs YouTube Music suggests after `video_id` (the seed itself is first).
pub fn radio(video_id: &str) -> Result<Vec<Track>, String> {
    let body = json!({
        "videoId": video_id,
        "playlistId": format!("RDAMVM{video_id}"),
        "isAudioOnly": true,
        "tunerSettingValue": "AUTOMIX_SETTING_NORMAL",
        "watchEndpointMusicSupportedConfigs": {"watchEndpointMusicConfig": {"hasPersistentPlaylistPanel": true, "musicVideoType": "MUSIC_VIDEO_TYPE_ATV"}}
    });
    let resp = call("next", "", body, false)?;
    let mut out: Vec<Track> = vec![];
    for p in find_all(&resp, "playlistPanelVideoRenderer") {
        if p.get("unplayableText").is_some() {
            continue;
        }
        let Some(id) = p.get("videoId").and_then(|v| v.as_str()) else { continue };
        let title = text_at(p, "/title");
        if title.is_empty() {
            continue;
        }
        let runs: Vec<&Value> = p.pointer("/longBylineText/runs").and_then(|r| r.as_array()).map(|a| a.iter().collect()).unwrap_or_default();
        let meta = meta_from_runs(&runs);
        let dur = parse_dur(&text_at(p, "/lengthText")).or(meta.dur);
        out.push(Track {
            id: id.to_string(),
            title,
            artist: meta.artist,
            dur,
            album: meta.album,
            artist_id: meta.artist_id,
            album_id: meta.album_id,
            set_id: String::new(),
            local: String::new(),
        });
    }
    let out = dedup(out);
    if out.is_empty() {
        Err("No radio suggestions for that song.".into())
    } else {
        Ok(out)
    }
}

/// Plain-text lyrics from YouTube Music itself (not time-synced).
pub fn lyrics(video_id: &str) -> Result<String, String> {
    let resp = call("next", "", json!({"videoId": video_id, "isAudioOnly": true}), false)?;
    let bid = find_all(&resp, "browseId")
        .into_iter()
        .filter_map(|v| v.as_str())
        .find(|s| s.starts_with("MPLYt"))
        .ok_or_else(|| "YouTube Music has no lyrics for this song.".to_string())?
        .to_string();
    let l = call("browse", "", json!({"browseId": bid}), false)?;
    let desc = find_all(&l, "musicDescriptionShelfRenderer")
        .into_iter()
        .next()
        .map(|s| text_at(s, "/description"))
        .unwrap_or_default();
    if desc.trim().is_empty() {
        Err("YouTube Music has no lyrics for this song.".into())
    } else {
        Ok(desc)
    }
}

// ---------------- public API: writing to the account ----------------

pub fn rate(video_id: &str, like: bool) -> Result<(), String> {
    let ep = if like { "like/like" } else { "like/removelike" };
    authed(|| call(ep, "", json!({"target": {"videoId": video_id}}), true)).map(|_| ())
}

/// Saves / removes an album (its OLAK5uy… playlist) or a playlist in the library.
pub fn save_playlist(playlist_id: &str, save: bool) -> Result<(), String> {
    let ep = if save { "like/like" } else { "like/removelike" };
    authed(|| call(ep, "", json!({"target": {"playlistId": playlist_id}}), true)).map(|_| ())
}

pub fn subscribe(channel_id: &str, on: bool) -> Result<(), String> {
    let ep = if on { "subscription/subscribe" } else { "subscription/unsubscribe" };
    authed(|| call(ep, "", json!({"channelIds": [channel_id]}), true)).map(|_| ())
}

fn edit_ok(resp: &Value) -> Result<(), String> {
    match resp.get("status").and_then(|s| s.as_str()) {
        Some("STATUS_SUCCEEDED") | None => Ok(()),
        Some(other) => Err(format!("YouTube Music refused the change ({other})")),
    }
}

pub fn create_playlist(title: &str, video_ids: &[String]) -> Result<String, String> {
    let r = authed(|| {
        call(
            "playlist/create",
            "",
            json!({"title": title, "description": "", "privacyStatus": "PRIVATE", "videoIds": video_ids}),
            true,
        )
    })?;
    r.get("playlistId")
        .and_then(|p| p.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "YouTube Music did not return a playlist id.".to_string())
}

pub fn playlist_add(playlist_id: &str, video_ids: &[String]) -> Result<(), String> {
    let actions: Vec<Value> = video_ids
        .iter()
        .map(|v| json!({"action": "ACTION_ADD_VIDEO", "addedVideoId": v, "dedupeOption": "DEDUPE_OPTION_SKIP"}))
        .collect();
    let r = authed(|| call("browse/edit_playlist", "", json!({"playlistId": playlist_id, "actions": actions.clone()}), true))?;
    edit_ok(&r)
}

pub fn playlist_remove(playlist_id: &str, video_id: &str, set_id: &str) -> Result<(), String> {
    if set_id.is_empty() {
        return Err("This row has no playlist handle - reload the playlist and try again.".into());
    }
    let actions = json!([{"action": "ACTION_REMOVE_VIDEO", "removedVideoId": video_id, "setVideoId": set_id}]);
    let r = authed(|| call("browse/edit_playlist", "", json!({"playlistId": playlist_id, "actions": actions.clone()}), true))?;
    edit_ok(&r)
}

#[allow(dead_code)] // Kept for playlist-management UI actions.
pub fn playlist_rename(playlist_id: &str, name: &str) -> Result<(), String> {
    let actions = json!([{"action": "ACTION_SET_PLAYLIST_NAME", "playlistName": name}]);
    let r = authed(|| call("browse/edit_playlist", "", json!({"playlistId": playlist_id, "actions": actions.clone()}), true))?;
    edit_ok(&r)
}

#[allow(dead_code)] // Kept for playlist-management UI actions.
pub fn playlist_delete(playlist_id: &str) -> Result<(), String> {
    authed(|| call("playlist/delete", "", json!({"playlistId": playlist_id}), true)).map(|_| ())
}
