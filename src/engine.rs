//! Everything that is not drawing: data types, config, yt-dlp, mpv engine thread,
//! audio visualiser capture, tray icon, window lifecycle (close-to-tray), single-instance + CLI control.

use serde::{Deserialize, Serialize};
use rand::seq::SliceRandom;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::{MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering::SeqCst},
        mpsc::{channel, RecvTimeoutError, Sender},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

/// Path of mpv's IPC socket. Lives in the per-user runtime dir (/run/user/UID) where available,
/// so two users on one machine do not collide and other users cannot connect to it.
pub fn sock() -> String {
    if let Ok(d) = std::env::var("XDG_RUNTIME_DIR") {
        if !d.is_empty() {
            return format!("{d}/rustym-mpv.sock");
        }
    }
    let uid = std::fs::metadata(config_dir()).map(|m| m.uid()).unwrap_or(0);
    format!("/tmp/rustym-mpv-{uid}.sock")
}
pub const BANDS: usize = 48;

static PAREC_PID: AtomicU32 = AtomicU32::new(0);

/// True once the tray icon has been registered.
pub static TRAY_UP: AtomicBool = AtomicBool::new(false);

/// Path of the single-instance / remote-control socket. Prefers the per-user runtime dir.
pub fn lock_path() -> PathBuf {
    if let Ok(d) = std::env::var("XDG_RUNTIME_DIR") {
        if !d.is_empty() {
            return PathBuf::from(d).join("rustym-instance.sock");
        }
    }
    let uid = std::fs::metadata(config_dir()).map(|m| m.uid()).unwrap_or(0);
    PathBuf::from(format!("/tmp/rustym-instance-{uid}.sock"))
}

/// Stops the visualiser's audio capture (the window process calls this when it closes).
pub fn stop_viz() {
    let pid = PAREC_PID.swap(0, SeqCst);
    if pid != 0 {
        let _ = Command::new("kill").arg(pid.to_string()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
}

/// Stops helper processes and removes sockets. Only the background process (the one that owns
/// the engine) calls this, because it removes mpv's and the control socket. Safe to call twice.
pub fn cleanup() {
    stop_viz();
    let _ = std::fs::remove_file(sock());
    let _ = std::fs::remove_file(lock_path());
}

pub type Repaint = Arc<dyn Fn() + Send + Sync>;

// ---------------- data ----------------

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
#[serde(default)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub dur: Option<f64>,
    pub album: String,
    /// YouTube Music browse ids, for "go to artist / album".
    pub artist_id: String,
    pub album_id: String,
    /// Handle needed to remove this row from a YouTube playlist.
    pub set_id: String,
    /// Path of a local file (empty for YouTube tracks).
    pub local: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RemotePl {
    pub id: String,
    pub title: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Playlist {
    pub name: String,
    pub tracks: Vec<Track>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub theme: usize,
    /// Unused since sign-in moved into the app; kept so old config files still load.
    pub browser: String,
    /// Google account name / avatar URL shown in the app (fetched after sign-in).
    pub account_name: String,
    pub account_photo: String,
    pub volume: f32,
    pub viz: usize,
    /// Show the audio visualiser on the Now Playing page.
    pub show_visualiser: bool,
    /// UI zoom percentage (80–125).
    pub ui_scale: f32,
    /// Use tighter spacing throughout the interface.
    pub compact_mode: bool,
    /// Automatically show lyrics when Rustym starts; the Lyrics toggle updates this preference.
    pub auto_show_lyrics: bool,
    pub signed_in: bool,
    /// Keep running (tray) when the window is closed. Off by default: closing the window quits.
    pub background: bool,
    /// Open the player window minimised on launch.
    pub start_minimised: bool,
    /// Launch Rustym automatically when the desktop session starts.
    pub start_at_login: bool,
    pub playlists: Vec<Playlist>,
    pub recent: Vec<Track>,
    /// Page shown when Rustym starts: 0 Home, 1 Search, 2 Liked songs.
    pub start_view: u8,
    /// Last search queries, offered as one-tap chips on the Search page.
    pub recent_queries: Vec<String>,
    pub trending: Vec<Track>,
    pub remote: Vec<RemotePl>,
    pub fx: Fx,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub autoplay: bool,
    /// Lyrics clock offset in milliseconds (positive = lyrics appear earlier).
    pub lyrics_offset: i32,
    pub local_dirs: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            theme: 2, // Emerald (green)
            browser: String::new(),
            account_name: String::new(),
            account_photo: String::new(),
            volume: 80.0,
            viz: 0,
            show_visualiser: true,
            ui_scale: 100.0,
            compact_mode: false,
            auto_show_lyrics: false,
            signed_in: false,
            background: false,
            start_minimised: false,
            start_at_login: false,
            playlists: vec![],
            recent: vec![],
            start_view: 0,
            recent_queries: vec![],
            trending: vec![],
            remote: vec![],
            fx: Fx::default(),
            shuffle: false,
            repeat: Repeat::Off,
            autoplay: true,
            lyrics_offset: 0,
            local_dirs: vec!["~/Music".to_string()],
        }
    }
}

pub fn config_dir() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"));
    let d = base.join("rustym");
    let _ = std::fs::create_dir_all(&d);
    d
}

impl Config {
    pub fn load() -> Self {
        let c: Config = std::fs::read_to_string(config_dir().join("config.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        c
    }
    pub fn save(&self) {
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(config_dir().join("config.json"), s);
        }
    }
}

// ---------------- liked-songs cache (opens the Liked page instantly, works offline) ----------------

fn liked_cache_path() -> PathBuf {
    config_dir().join("liked_cache.json")
}

/// Liked songs saved by the last successful fetch (empty when there is no cache yet).
pub fn liked_cache_load() -> Vec<Track> {
    std::fs::read_to_string(liked_cache_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Replaces the cache. Written to a temp file first so a crash never leaves a half-written list.
pub fn liked_cache_save(t: &[Track]) {
    if let Ok(s) = serde_json::to_string(t) {
        let p = liked_cache_path();
        let tmp = config_dir().join("liked_cache.json.tmp");
        if std::fs::write(&tmp, s).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    }
}

pub fn liked_cache_clear() {
    let _ = std::fs::remove_file(liked_cache_path());
}

/// Keeps the cache in step with a like / unlike done inside Rustym.
pub fn liked_cache_toggle(t: &Track, like: bool) {
    let mut v = liked_cache_load();
    if v.is_empty() {
        return; // no cache yet: the next fetch creates it
    }
    v.retain(|x| x.id != t.id);
    if like {
        v.insert(0, t.clone());
    }
    liked_cache_save(&v);
}

// ---------------- debug log + diagnostics ----------------

/// One line in the in-app Logs page. level: 0 info, 1 warning, 2 error.
#[derive(Clone)]
pub struct LogEntry {
    pub t: String,
    pub level: u8,
    pub src: String,
    pub msg: String,
}

/// One diagnostic result. sev: 0 ok, 1 warning, 2 problem.
#[derive(Clone)]
pub struct Check {
    pub name: String,
    pub sev: u8,
    pub detail: String,
    pub fix: String,
}

static LOGS: Mutex<Vec<LogEntry>> = Mutex::new(Vec::new());
static HEALTH: Mutex<Vec<Check>> = Mutex::new(Vec::new());
static HEALTH_BUSY: AtomicBool = AtomicBool::new(false);

pub fn tz_offset() -> i64 {
    static TZ: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *TZ.get_or_init(|| {
        let s = Command::new("date")
            .arg("+%z")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        if s.len() == 5 && s.is_ascii() {
            let sign = if s.starts_with('-') { -1 } else { 1 };
            let h: i64 = s[1..3].parse().unwrap_or(0);
            let m: i64 = s[3..5].parse().unwrap_or(0);
            sign * (h * 3600 + m * 60)
        } else {
            0
        }
    })
}

fn stamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
        + tz_offset();
    let s = now.rem_euclid(86400);
    format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
}

/// Records an event for the Logs page and appends it to ~/.config/rustym/debug.log.
/// level: 0 info, 1 warning, 2 error.
pub fn log(level: u8, src: &str, msg: impl AsRef<str>) {
    let mut m = msg.as_ref().trim().to_string();
    if m.chars().count() > 2500 {
        m = m.chars().take(2500).collect::<String>() + " …(cut)";
    }
    let e = LogEntry { t: stamp(), level: level.min(2), src: src.to_string(), msg: m };
    let path = config_dir().join("debug.log");
    if std::fs::metadata(&path).map(|m| m.len() > 600_000).unwrap_or(false) {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(
            f,
            "{} {:5} [{}] {}",
            e.t,
            ["INFO", "WARN", "ERROR"][e.level as usize],
            e.src,
            e.msg.replace('\n', " | ")
        );
    }
    let mut v = LOGS.lock().unwrap_or_else(|p| p.into_inner());
    v.push(e);
    if v.len() > 1500 {
        let n = v.len() - 1500;
        v.drain(0..n);
    }
}

pub fn logs_snapshot() -> Vec<LogEntry> {
    LOGS.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

pub fn logs_clear() {
    LOGS.lock().unwrap_or_else(|p| p.into_inner()).clear();
    let _ = std::fs::remove_file(config_dir().join("debug.log"));
}

pub fn logs_as_text() -> String {
    let mut out = format!("Rustym {} debug report\n", env!("CARGO_PKG_VERSION"));
    for c in health_snapshot() {
        out += &format!("[check {}] {}: {}{}\n", ["ok", "warn", "PROBLEM"][c.sev.min(2) as usize], c.name, c.detail, if c.fix.is_empty() { String::new() } else { format!("  -> fix: {}", c.fix) });
    }
    out += "\n";
    for e in logs_snapshot() {
        out += &format!("{} {:5} [{}] {}\n", e.t, ["INFO", "WARN", "ERROR"][e.level as usize], e.src, e.msg);
    }
    out
}

pub fn health_snapshot() -> Vec<Check> {
    HEALTH.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

pub fn health_busy() -> bool {
    HEALTH_BUSY.load(SeqCst)
}

/// The last lines of mpv's own output from the most recent play attempt.
pub fn mpv_log_tail(n: usize) -> String {
    let t = std::fs::read_to_string(config_dir().join("mpv.log")).unwrap_or_default();
    let lines: Vec<&str> = t.lines().filter(|l| !l.trim().is_empty()).collect();
    let from = lines.len().saturating_sub(n);
    lines[from..].join("\n")
}

fn install_mpv() -> String {
    crate::distro::install_hint(&[crate::distro::Pkg::Mpv])
}
fn install_ytdlp() -> String {
    crate::distro::install_hint(&[crate::distro::Pkg::YtDlp])
}
fn install_deno() -> String {
    crate::distro::install_hint(&[crate::distro::Pkg::Deno])
}

// ---------------- managed yt-dlp + deno (bundle + in-app updater) ----------------
//
// Packages carry current copies of yt-dlp and deno in /usr/lib/rustym/ (see
// .github/workflows/build.yml); extend_path() in main.rs puts them, and the
// user-updated copies in ~/.local/bin, at the FRONT of PATH so every child
// process - mpv's ytdl_hook included - uses them instead of the distro's
// often years-old yt-dlp.

const YTDLP_LINUX_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux";
const DENO_LINUX_URL: &str = "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-unknown-linux-gnu.zip";

/// "mpv v0.42.0 Copyright (c) ..." -> "mpv v0.42.0"; "deno 2.9.8 (stable, ...)" -> "deno 2.9.8".
fn first_version(t: &str) -> String {
    let mut v = t.lines().next().unwrap_or("").to_string();
    if let Some(i) = v.find("Copyright") {
        v.truncate(i);
    }
    if let Some(i) = v.find('(') {
        v.truncate(i);
    }
    v.trim().to_string()
}

/// One row of the Settings > Dependencies card: (name, version line, path, missing).
pub type DepRow = (&'static str, String, String, bool);

/// Versions of the runtime dependencies, for the Settings page. Runs four
/// short processes; cheap enough to call when the Settings view is entered.
pub fn dep_report() -> Vec<DepRow> {
    let mut out = Vec::new();
    for (name, arg) in [
        ("yt-dlp", "--version"),
        ("deno", "--version"),
        ("mpv", "--version"),
        ("ffmpeg", "-version"),
    ] {
        let path = run_cmd("sh", &["-c", &format!("command -v {name}")])
            .ok()
            .filter(|(ok, _)| *ok)
            .map(|(_, t)| t.trim().to_string())
            .unwrap_or_default();
        let (missing, version) = match run_cmd(name, &[arg]) {
            Ok((true, t)) => (false, first_version(&t)),
            Ok((false, t)) => (true, t.lines().next().unwrap_or("").trim().to_string()),
            Err(_) => (true, String::new()),
        };
        out.push((name, version, path, missing));
    }
    out
}

/// Downloads the newest yt-dlp and deno releases into ~/.local/bin, where they
/// shadow the system copies (extend_path() gives ~/.local/bin priority).
/// Needs no root and works the same on every distro. Returns a report for the UI.
pub fn update_runtime_deps() -> String {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return "no HOME directory - cannot install".into();
    };
    let bin = home.join(".local/bin");
    if let Err(e) = std::fs::create_dir_all(&bin) {
        return format!("cannot create {}: {e}", bin.display());
    }
    let mut out: Vec<String> = Vec::new();

    // ---- yt-dlp (single self-contained Linux binary) ----
    let tmp = bin.join(".yt-dlp.dl");
    let _ = std::fs::remove_file(&tmp);
    match http_download(YTDLP_LINUX_URL, &tmp) {
        Ok(()) => {
            let dst = bin.join("yt-dlp");
            if let Err(e) = std::fs::rename(&tmp, &dst).map_err(|e| e.to_string()).and_then(|_| set_exec(&dst)) {
                out.push(format!("yt-dlp install failed: {e}"));
            } else {
                match run_cmd(dst.to_str().unwrap_or("yt-dlp"), &["--version"]) {
                    Ok((true, t)) => out.push(format!("yt-dlp {} installed to {}", t.trim(), dst.display())),
                    Ok((false, t)) => out.push(format!("yt-dlp installed but does not run: {}", t.trim())),
                    Err(e) => out.push(format!("yt-dlp installed but cannot run it: {e}")),
                }
            }
        }
        Err(e) => out.push(format!("yt-dlp update failed: {e}")),
    }

    // ---- deno (zip archive holding one binary) ----
    let zip = bin.join(".deno.zip");
    let _ = std::fs::remove_file(&zip);
    let stage = bin.join(".deno-unpack");
    let _ = std::fs::remove_dir_all(&stage);
    match http_download(DENO_LINUX_URL, &zip) {
        Ok(()) => {
            let extracted = run_cmd("unzip", &["-o", zip.to_str().unwrap_or(""), "-d", stage.to_str().unwrap_or("")])
                .map(|(ok, _)| ok)
                .unwrap_or(false)
                || run_cmd(
                    "python3",
                    &["-c", &format!("import zipfile;zipfile.ZipFile({zip:?}).extractall({stage:?})")],
                )
                .map(|(ok, _)| ok)
                .unwrap_or(false);
            if extracted {
                if let Some(b) = find_file(&stage, "deno") {
                    let dst = bin.join("deno");
                    let moved = std::fs::rename(&b, &dst)
                        .or_else(|_| std::fs::copy(&b, &dst).map(|_| ()))
                        .map_err(|e| e.to_string())
                        .and_then(|_| set_exec(&dst));
                    match moved {
                        Ok(()) => match run_cmd(dst.to_str().unwrap_or("deno"), &["--version"]) {
                            Ok((true, t)) => {
                                let v = t.trim().strip_prefix("deno ").unwrap_or(t.trim());
                                out.push(format!("deno {v} installed to {}", dst.display()))
                            }
                            Ok((false, t)) => out.push(format!("deno installed but does not run: {}", t.trim())),
                            Err(e) => out.push(format!("deno installed but cannot run it: {e}")),
                        },
                        Err(e) => out.push(format!("deno install failed: {e}")),
                    }
                } else {
                    out.push("deno zip did not contain the deno binary".into());
                }
            } else {
                out.push("could not extract deno.zip - install unzip or python3".into());
            }
        }
        Err(e) => out.push(format!("deno update failed: {e}")),
    }
    let _ = std::fs::remove_dir_all(&stage);
    let _ = std::fs::remove_file(&zip);
    let _ = std::fs::remove_file(&tmp);

    out.push("Restart Rustym so the running mpv picks the new copies up.".into());
    out.join("\n")
}

/// Streams a URL into a file (GitHub releases redirect; ureq follows).
fn http_download(url: &str, dest: &PathBuf) -> Result<(), String> {
    let resp = ureq::get(url).call().map_err(|e| e.to_string())?;
    let mut f = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    let mut reader = resp.into_reader().take(1024 * 1024 * 1024);
    std::io::copy(&mut reader, &mut f).map_err(|e| e.to_string())?;
    f.sync_all().ok();
    Ok(())
}

fn set_exec(p: &PathBuf) -> Result<(), String> {
    let mut perms = std::fs::metadata(p).map_err(|e| e.to_string())?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(p, perms).map_err(|e| e.to_string())
}

/// Finds a file called `name` up to two directory levels deep.
fn find_file(dir: &std::path::Path, name: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for e in entries.flatten() {
        let p = e.path();
        if p.is_file() && p.file_name().map(|f| f == name).unwrap_or(false) {
            return Some(p);
        }
    }
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        if e.path().is_dir() {
            if let Some(found) = find_file(&e.path(), name) {
                return Some(found);
            }
        }
    }
    None
}

fn run_cmd(prog: &str, args: &[&str]) -> Result<(bool, String), String> {
    let o = Command::new(prog)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    let mut t = String::from_utf8_lossy(&o.stdout).to_string();
    t.push_str(&String::from_utf8_lossy(&o.stderr));
    Ok((o.status.success(), t))
}

fn first_line(t: &str) -> String {
    t.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string()
}

fn push_check(name: &str, sev: u8, detail: impl Into<String>, fix: &str) {
    let d: String = detail.into();
    log(sev.min(2), "check", format!("{name}: {d}{}", if fix.is_empty() { String::new() } else { format!("  [fix: {fix}]") }));
    HEALTH
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push(Check { name: name.to_string(), sev: sev.min(2), detail: d, fix: fix.to_string() });
}

/// Runs every check that can stop rustym from working and records the results (and the details in
/// the log). `play` also does a real 3-second playback test through mpv + yt-dlp.
pub fn run_health_checks(play: bool) {
    if HEALTH_BUSY.swap(true, SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        HEALTH.lock().unwrap_or_else(|p| p.into_inner()).clear();
        log(0, "check", if play { "=== diagnostics + playback test ===" } else { "=== diagnostics ===" });
        log(
            0,
            "system",
            format!(
                "distro={} family={:?} desktop={} session={} wayland={} display={} config_dir={}",
                crate::distro::pretty_name(),
                crate::distro::family(),
                crate::distro::desktop(),
                std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "?".into()),
                std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "-".into()),
                std::env::var("DISPLAY").unwrap_or_else(|_| "-".into()),
                config_dir().display()
            ),
        );
        if let Ok((_, t)) = run_cmd("sh", &["-c", crate::distro::query_installed_cmd()]) {
            for l in t.lines() {
                log(0, "system", l);
            }
        }

        // mpv itself
        let mut mpv_ok = false;
        match run_cmd("mpv", &["--version"]) {
            Ok((true, t)) => {
                mpv_ok = true;
                push_check("mpv", 0, first_line(&t), "");
            }
            Ok((false, t)) => {
                let fix = if t.contains("shared libraries") {
                    format!("mpv is out of sync with its libraries: {}", crate::distro::upgrade_hint())
                } else {
                    install_mpv()
                };
                push_check("mpv", 2, first_line(&t), &fix);
            }
            Err(e) => push_check("mpv", 2, format!("cannot run mpv ({e})"), &install_mpv()),
        }
        if let Ok((_, t)) = run_cmd("sh", &["-c", r#"command -v mpv >/dev/null && ldd "$(command -v mpv)" 2>&1 | grep 'not found'"#]) {
            let miss: Vec<String> = t.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
            if miss.is_empty() {
                if mpv_ok {
                    push_check("mpv libraries", 0, "all shared libraries found", "");
                }
            } else {
                push_check(
                    "mpv libraries",
                    2,
                    miss.join("; "),
                    &crate::distro::missing_lib_hint(),
                );
            }
        }

        // yt-dlp, python, deno
        match run_cmd("yt-dlp", &["--version"]) {
            Ok((true, t)) => push_check("yt-dlp", 0, format!("version {}", first_line(&t)), ""),
            Ok((false, t)) => push_check("yt-dlp", 2, first_line(&t), &install_ytdlp()),
            Err(e) => push_check("yt-dlp", 2, format!("cannot run yt-dlp ({e})"), &install_ytdlp()),
        }
        push_check("YouTube Music client", 0, "built in (pure Rust, no Python needed)", "");
        match run_cmd("deno", &["--version"]) {
            Ok((true, t)) => push_check("deno (JS runtime)", 0, first_line(&t), ""),
            _ => push_check("deno (JS runtime)", 1, "not found - yt-dlp may fail to unlock streams", &install_deno()),
        }

        // login / session
        let cf = cookie_file();
        if cf.exists() {
            let age = std::fs::metadata(&cf)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .map(|d| d.as_secs() / 60)
                .unwrap_or(0);
            if has_login_cookies(&cf) {
                push_check("saved login", 0, format!("cookies.txt has Google login cookies (updated {age} min ago)"), "");
                match crate::ytm::verify() {
                    Ok(_) => {
                        push_check("YouTube Music session", 0, "YouTube Music accepts the saved login", "");
                        match api_tracks("liked", "") {
                            Ok(t) => push_check("liked songs", 0, format!("{} songs fetched", t.len()), ""),
                            Err(e) => push_check("liked songs", 2, e, "Sign out and sign in again"),
                        }
                    }
                    Err(e) => push_check("YouTube Music session", 2, e, "Settings -> Sign out, then Sign in with Google again"),
                }
            } else {
                push_check("saved login", 2, "cookies.txt has no Google login cookies", "Sign in with Google in Settings");
            }
        } else {
            push_check("saved login", 1, "not signed in (no cookies.txt)", "Sign in with Google in Settings");
        }

        // real playback test
        if play {
            if !mpv_ok {
                push_check("playback test", 2, "skipped - mpv does not start", "Fix the mpv problem above first");
            } else {
                let t0 = Instant::now();
                let out = run_cmd(
                    "timeout",
                    &[
                        "50",
                        "mpv",
                        "--no-video",
                        "--force-window=no",
                        "--ao=null",
                        "--length=3",
                        "--msg-level=all=info",
                        &format!("--ytdl-format={YTDL_FORMAT}"),
                        "https://music.youtube.com/watch?v=dQw4w9WgXcQ",
                    ],
                );
                let secs = t0.elapsed().as_secs_f32();
                match out {
                    Ok((ok, t)) => {
                        for l in t.lines().filter(|l| !l.trim().is_empty()).take(60) {
                            log(if ok { 0 } else { 1 }, "playtest", l);
                        }
                        if ok {
                            push_check("playback test", 0, format!("mpv played 3 s of a real track in {secs:.1} s"), "");
                        } else {
                            push_check("playback test", 2, play_error_hint_from(&t), "See the playtest lines in the log below");
                        }
                    }
                    Err(e) => push_check("playback test", 2, format!("could not run test ({e})"), &crate::distro::install_hint(&[crate::distro::Pkg::Coreutils, crate::distro::Pkg::Mpv])),
                }
            }
        }
        log(0, "check", "=== diagnostics finished ===");
        HEALTH_BUSY.store(false, SeqCst);
    });
}

const YTDL_FORMAT: &str = "140/251/bestaudio[protocol^=http][protocol!*=m3u8]/bestaudio/best";

// ---------------- Spotify playlist import ----------------

/// Unescapes the JSON string escapes we can meet in Spotify's embed data.
fn json_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => {},
                Some('u') => {
                    let hex: String = chars.by_ref().take(4).collect();
                    if let Ok(cp) = u32::from_str_radix(&hex, 16) {
                        if let Some(ch) = char::from_u32(cp) {
                            out.push(ch);
                        }
                    }
                }
                Some(e) => out.push(e),
                None => {},
            },
            _ => out.push(c),
        }
    }
    out
}

/// Reads one JSON-escaped string starting at the opening `"` at `at`.
/// Returns the string and the index just after its closing quote.
fn json_str_from(s: &str, at: usize) -> Option<(String, usize)> {
    let b = s.as_bytes();
    if at >= b.len() || b[at] != b'"' {
        return None;
    }
    let mut i = at + 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'"' => {
                let raw = &s[at + 1..i];
                return Some((json_unescape(raw), i + 1));
            }
            _ => i += 1,
        }
    }
    None
}

/// Imports a public Spotify playlist through its embed page (no Spotify login
/// needed): the embed HTML contains the track list, which is matched to
/// YouTube Music songs one by one. Returns (playlist name, matched tracks).
pub fn spotify_import(url: &str) -> Result<(String, Vec<Track>), String> {
    let url = url.trim();
    let id = if let Some(p) = url.find("playlist/") {
        let rest = &url[p + 9..];
        rest.split(['?', '&', '"']).next().unwrap_or("").to_string()
    } else if let Some(p) = url.find("spotify:playlist:") {
        url[p + 17..].split([' ', '\"']).next().unwrap_or("").to_string()
    } else {
        String::new()
    };
    if id.len() < 16 {
        return Err("Paste a Spotify playlist link (open.spotify.com/playlist/… or spotify:playlist:…)".into());
    }
    let html = crate::ytm::fetch_text(&format!("https://open.spotify.com/embed/playlist/{id}"))?;

    // Playlist name: the first "name":" before the track list.
    let tracklist_at = html.find("\"trackList\"").ok_or("That Spotify playlist looks empty or private")?;
    let name = html[..tracklist_at]
        .rfind("\"name\":\"")
        .and_then(|p| json_str_from(&html, p + 7))
        .map(|(s, _)| s)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "Spotify import".to_string());

    // Tracks: "title":"…" followed by "subtitle":"…" (artists).
    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut i = tracklist_at;
    while let Some(rel) = html[i..].find("\"title\":\"") {
        let at = i + rel + 8;
        let Some((title, after)) = json_str_from(&html, at) else { break };
        let window = &html[after..(after + 400).min(html.len())];
        let artist = window
            .find("\"subtitle\":\"")
            .and_then(|p| json_str_from(window, p + 12))
            .map(|(s, _)| s)
            .unwrap_or_default();
        if !title.trim().is_empty() {
            pairs.push((title, artist));
        }
        i = after;
        if pairs.len() >= 50 {
            break;
        }
    }
    if pairs.is_empty() {
        return Err("No tracks found in that Spotify playlist".into());
    }

    let mut tracks = Vec::new();
    let mut missed = 0usize;
    for (title, artist) in pairs.iter() {
        let q = if artist.is_empty() { title.clone() } else { format!("{title} {artist}") };
        match crate::ytm::search(&q, 1) {
            Ok(o) if !o.songs.is_empty() => tracks.push(o.songs[0].clone()),
            _ => missed += 1,
        }
    }
    if tracks.is_empty() {
        return Err("None of the Spotify tracks could be matched on YouTube Music".into());
    }
    let _ = missed;
    Ok((name, tracks))
}

// ---------------- yt-dlp / login ----------------

pub fn run_ytdlp(args: Vec<String>) -> Result<Vec<Track>, String> {
    let out = Command::new("yt-dlp")
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| {
            log(2, "yt-dlp", format!("could not run yt-dlp: {e}"));
            format!("Could not run yt-dlp ({e}). Is it installed?")
        })?;
    let text = String::from_utf8_lossy(&out.stdout);
    let tracks: Vec<Track> = text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| {
            let id = v["id"].as_str()?.to_string();
            let title = v["title"].as_str()?.to_string();
            let artist = v["channel"]
                .as_str()
                .or(v["uploader"].as_str())
                .unwrap_or("")
                .trim_end_matches(" - Topic")
                .to_string();
            let dur = v["duration"].as_f64();
            if dur.map_or(false, |d| d > 1200.0) {
                return None;
            }
            Some(Track { id, title, artist, dur, ..Default::default() })
        })
        .collect();
    if tracks.is_empty() {
        let err = String::from_utf8_lossy(&out.stderr);
        let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("No results.");
        let target = args.last().cloned().unwrap_or_default();
        let tail: Vec<&str> = err.lines().filter(|l| !l.trim().is_empty()).rev().take(6).collect();
        log(2, "yt-dlp", format!("no results for {target}: {}", tail.into_iter().rev().collect::<Vec<_>>().join(" | ")));
        return Err(last.to_string());
    }
    Ok(tracks)
}

/// Liked songs (`mode` = "liked") or a library playlist (`mode` = "playlist", `arg` = id),
/// fetched straight from the YouTube Music API by the native client in `ytm.rs`.
pub fn api_tracks(mode: &str, arg: &str) -> Result<Vec<Track>, String> {
    let t0 = Instant::now();
    let r = match mode {
        "liked" => crate::ytm::liked(),
        _ => crate::ytm::playlist(arg).map(|d| d.tracks),
    };
    match &r {
        Ok(t) => log(0, "api", format!("{mode} ok: {} songs ({:.1}s)", t.len(), t0.elapsed().as_secs_f32())),
        Err(e) => log(2, "api", format!("{mode} failed ({:.1}s): {e}", t0.elapsed().as_secs_f32())),
    }
    match r {
        Ok(t) if t.is_empty() => Err("No songs found there.".into()),
        r => r,
    }
}

/// The playlists saved in the user's YouTube Music library.
pub fn api_playlists() -> Result<Vec<RemotePl>, String> {
    let cards = crate::ytm::library_playlists()?;
    let p: Vec<RemotePl> = cards.into_iter().map(|c| RemotePl { id: c.id, title: c.title }).collect();
    if p.is_empty() {
        Err("No playlists found in your YouTube Music library.".into())
    } else {
        Ok(p)
    }
}

/// Tries each argument set in turn and returns the first that gives tracks.
pub fn run_chain(chains: Vec<Vec<String>>) -> Result<Vec<Track>, String> {
    let mut err = String::from("No results.");
    for a in chains {
        match run_ytdlp(a) {
            Ok(t) => return Ok(t),
            Err(e) => err = e,
        }
    }
    Err(err)
}

/// Lists the signed-in user's saved YouTube playlists (flat listing of library pages).
pub fn run_playlists(chains: Vec<Vec<String>>) -> Result<Vec<RemotePl>, String> {
    let mut err = String::from("No playlists found.");
    for args in chains {
        let out = match Command::new("yt-dlp").args(&args).stdout(Stdio::piped()).stderr(Stdio::piped()).output() {
            Ok(o) => o,
            Err(e) => return Err(format!("Could not run yt-dlp ({e}). Is it installed?")),
        };
        let text = String::from_utf8_lossy(&out.stdout);
        let mut v: Vec<RemotePl> = vec![];
        for l in text.lines() {
            let Ok(j) = serde_json::from_str::<Value>(l) else { continue };
            let url = j["url"].as_str().or(j["webpage_url"].as_str()).unwrap_or("");
            let Some(title) = j["title"].as_str() else { continue };
            let id = if let Some(i) = url.split("list=").nth(1) {
                i.split('&').next().unwrap_or("").to_string()
            } else if j["ie_key"].as_str() == Some("YoutubeTab") {
                j["id"].as_str().unwrap_or("").to_string()
            } else {
                String::new()
            };
            if id.is_empty() || v.iter().any(|x| x.id == id) {
                continue;
            }
            v.push(RemotePl { id, title: title.to_string() });
        }
        if !v.is_empty() {
            return Ok(v);
        }
        err = last_line(&out.stderr);
    }
    Err(err)
}

/// The sign-in window (a small helper program installed next to Rustym).
static HELPER: std::sync::Mutex<Option<std::process::Child>> = std::sync::Mutex::new(None);

/// Signs out completely: the saved cookies.
pub fn forget_login() {
    let _ = std::fs::remove_file(cookie_file());
    let _ = std::fs::remove_file(config_dir().join("cookies.tmp"));
    close_login_window();
}

fn close_login_window() {
    if let Ok(mut g) = HELPER.lock() {
        if let Some(mut c) = g.take() {
            let pid = c.id();
            let _ = c.kill();
            let _ = c.wait();
            // the window's throw-away data folder (see rustym-login)
            let _ = std::fs::remove_dir_all(std::env::temp_dir().join(format!("rustym-login-{pid}")));
        }
    }
}

/// Path of the first executable called `name` on PATH.
fn which_bin(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(name)).find(|p| p.is_file())
}

fn login_helper() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let h = exe.parent()?.join("rustym-login");
    if h.is_file() {
        return Some(h);
    }
    which_bin("rustym-login")
}

/// Opens Rustym's own Google sign-in window. The helper saves the cookies itself and exits.
pub fn open_login() -> Result<(), String> {
    let Some(h) = login_helper() else {
        let m = format!(
            "The sign-in window (rustym-login) is not installed. Re-run ./install.sh (it needs WebKitGTK 4.1: {}).",
            crate::distro::install_hint(&[crate::distro::Pkg::Webkit])
        );
        log(2, "login", &m);
        return Err(m);
    };
    close_login_window();
    let tmp = config_dir().join("cookies.tmp");
    let _ = std::fs::remove_file(&tmp);
    match Command::new(h).arg(&tmp).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
        Ok(child) => {
            if let Ok(mut g) = HELPER.lock() {
                *g = Some(child);
            }
            log(0, "login", "opened the Rustym sign-in window");
            Ok(())
        }
        Err(e) => {
            let m = format!("Could not open the sign-in window: {e}");
            log(2, "login", &m);
            Err(m)
        }
    }
}

/// Picks up the cookies the sign-in window saved.
fn check_login() -> Result<(), String> {
    let tmp = config_dir().join("cookies.tmp");
    if has_login_cookies(&tmp) {
        install_cookies(&tmp);
        Ok(())
    } else {
        Err("not signed in to Google yet".into())
    }
}

/// True while the sign-in window is still open.
fn login_window_open() -> bool {
    match HELPER.lock() {
        Ok(mut g) => match g.as_mut() {
            Some(c) => matches!(c.try_wait(), Ok(None)),
            None => false,
        },
        Err(_) => false,
    }
}

/// Where the signed-in cookies are kept (Netscape cookies.txt, readable by yt-dlp).
pub fn cookie_file() -> PathBuf {
    config_dir().join("cookies.txt")
}

/// yt-dlp arguments that make a request use the signed-in account.
/// Prefers the saved cookie file (fast, no keyring / locked-database problems).
pub fn auth_args(c: &Config) -> Vec<String> {
    if cookie_file().exists() {
        vec!["--cookies".into(), cookie_file().to_string_lossy().to_string()]
    } else {
        let _ = c;
        vec![]
    }
}

fn last_line(b: &[u8]) -> String {
    let t = String::from_utf8_lossy(b);
    t.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("no details")
        .trim()
        .to_string()
}

fn private_file(p: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600));
}

/// True if a cookies.txt contains Google's login cookies.
fn has_login_cookies(p: &std::path::Path) -> bool {
    std::fs::read_to_string(p)
        .map(|t| t.contains("\tSAPISID\t") || t.contains("\t__Secure-3PAPISID\t") || t.contains("\t__Secure-1PAPISID\t"))
        .unwrap_or(false)
}

fn install_cookies(tmp: &std::path::Path) {
    private_file(tmp);
    let _ = std::fs::rename(tmp, cookie_file());
}

/// Asks YouTube Music itself whether the saved cookies are accepted (a signed-in library comes
/// back). Without this, "login detected" only meant "cookies exist", which is why sign-in used to
/// look fine while liked songs and playlists stayed empty.
pub fn verify_session() -> Result<(), String> {
    match crate::ytm::verify() {
        Ok(()) => Ok(()),
        // offline: can't tell, don't block the login
        Err(e) if e.starts_with("network error") => Ok(()),
        Err(e) => Err(e),
    }
}

/// Checks a hand-exported cookies.txt placed at `cookie_file()`.
pub fn check_cookie_file() -> Result<(), String> {
    if !cookie_file().exists() {
        return Err(format!("No cookies file found at {}", cookie_file().display()));
    }
    let out = Command::new("yt-dlp")
        .arg("--cookies")
        .arg(cookie_file())
        .args([
            "--flat-playlist",
            "--dump-json",
            "--playlist-end",
            "1",
            "--no-warnings",
            "https://music.youtube.com/playlist?list=LM",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("Could not run yt-dlp ({e}). Is it installed?"))?;
    if out.stdout.is_empty() {
        Err(last_line(&out.stderr))
    } else {
        Ok(())
    }
}

/// Polls every ~2s (up to ~6 minutes) until the sign-in window has saved a working login.
pub fn wait_for_login(cancel: &AtomicBool, progress: &dyn Fn(String)) -> Result<(), String> {
    let mut last = String::from("no login found yet");
    for _ in 0..180 {
        if cancel.load(SeqCst) {
            close_login_window();
            return Err("Cancelled.".into());
        }
        match check_login().and_then(|_| verify_session()) {
            Ok(()) => {
                log(0, "login", "signed in");
                close_login_window();
                return Ok(());
            }
            Err(e) => {
                if !login_window_open() && !config_dir().join("cookies.tmp").exists() {
                    return Err("The sign-in window was closed before signing in.".into());
                }
                progress("Waiting for you to finish signing in…".into());
                last = e;
            }
        }
        for _ in 0..20 {
            if cancel.load(SeqCst) {
                close_login_window();
                return Err("Cancelled.".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    close_login_window();
    Err(last)
}

// ---------------- mpv ----------------

/// One persistent connection to mpv's JSON IPC socket (no reconnecting for every property read).
struct Ipc {
    w: UnixStream,
    r: BufReader<UnixStream>,
    id: u64,
}

impl Ipc {
    fn connect() -> Option<Ipc> {
        let w = UnixStream::connect(sock()).ok()?;
        w.set_write_timeout(Some(Duration::from_millis(500))).ok()?;
        let rs = w.try_clone().ok()?;
        rs.set_read_timeout(Some(Duration::from_millis(500))).ok()?;
        Some(Ipc { w, r: BufReader::new(rs), id: 100 })
    }

    fn cmd(&mut self, cmd: Value) -> Option<Value> {
        self.id += 1;
        let id = self.id;
        let req = json!({ "command": cmd, "request_id": id });
        writeln!(self.w, "{req}").ok()?;
        let mut line = String::new();
        for _ in 0..300 {
            line.clear();
            match self.r.read_line(&mut line) {
                Ok(0) => return None,
                Ok(_) => {}
                Err(_) => return None,
            }
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                if v["request_id"].as_u64() == Some(id) {
                    return Some(v);
                }
            }
        }
        None
    }

    fn get(&mut self, prop: &str) -> Option<Value> {
        let v = self.cmd(json!(["get_property", prop]))?;
        if v["error"] == "success" {
            Some(v["data"].clone())
        } else {
            None
        }
    }

    fn f64(&mut self, prop: &str) -> Option<f64> {
        self.get(prop)?.as_f64()
    }

    fn bool(&mut self, prop: &str) -> Option<bool> {
        self.get(prop)?.as_bool()
    }

    fn set(&mut self, prop: &str, val: Value) -> bool {
        self.cmd(json!(["set_property", prop, val])).map_or(false, |v| v["error"] == "success")
    }
}

// ---------------- playback engine (own thread, keeps running when window is hidden) ----------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

pub const EQ_FREQS: [u32; 10] = [31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];
pub const REVERBS: [&str; 4] = ["Off", "Small room", "Large hall", "Cathedral"];

/// Tempo, pitch, equaliser, reverb and loudness normalisation (all done by mpv / ffmpeg filters).
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Fx {
    pub speed: f32,
    /// semitones, -12 ..= +12
    pub pitch: f32,
    pub eq_on: bool,
    pub eq: [f32; 10],
    pub reverb: u8,
    pub normalize: bool,
    /// 8D audio: the sound slowly circles around your head (left <-> right auto-pan).
    pub eight_d: bool,
    /// How fast the 8D sound circles, in cycles per second (0.05 ..= 0.5).
    pub eight_d_hz: f32,
}

impl Default for Fx {
    fn default() -> Self {
        Fx { speed: 1.0, pitch: 0.0, eq_on: false, eq: [0.0; 10], reverb: 0, normalize: false, eight_d: false, eight_d_hz: 0.125 }
    }
}

/// Equaliser curve used by the player-bar "Bass" button.
pub const BASS_EQ: [f32; 10] = [6.0, 5.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];

impl Fx {
    fn af(&self) -> String {
        let mut f: Vec<String> = vec![];
        if self.eq_on {
            for (i, g) in self.eq.iter().enumerate() {
                if g.abs() > 0.05 {
                    f.push(format!("equalizer=f={}:t=o:w=1:g={:.1}", EQ_FREQS[i], g));
                }
            }
        }
        if self.pitch.abs() > 0.01 {
            f.push(format!("rubberband=pitch={:.5}", 2f32.powf(self.pitch / 12.0)));
        }
        match self.reverb {
            1 => f.push("aecho=0.8:0.88:40|60:0.3|0.2".to_string()),
            2 => f.push("aecho=0.8:0.9:60|120|180:0.4|0.3|0.2".to_string()),
            3 => f.push("aecho=0.8:0.88:250|500|750:0.35|0.25|0.15".to_string()),
            _ => {}
        }
        if self.eight_d {
            // a little room echo makes the circling feel like it is outside the headphones
            if self.reverb == 0 {
                f.push("aecho=0.8:0.85:70|130:0.25|0.15".to_string());
            }
            f.push(format!("apulsator=mode=sine:hz={:.3}:amount=1:offset_l=0:offset_r=0.5", self.eight_d_hz.clamp(0.02, 1.0)));
        }
        if self.normalize {
            f.push("loudnorm=I=-14:TP=-1.5:LRA=11".to_string());
        }
        if f.is_empty() {
            String::new()
        } else {
            format!("lavfi=[{}]", f.join(","))
        }
    }
}

#[derive(Serialize, Deserialize)]
pub enum Cmd {
    /// Replace the queue and start playing item `usize`.
    Play(Vec<Track>, usize),
    Toggle,
    Next,
    Prev,
    Seek(f64),
    SeekBy(f64),
    Volume(f32),
    /// Play queue item N now.
    Jump(usize),
    Enqueue(Vec<Track>),
    PlayNext(Vec<Track>),
    RemoveAt(usize),
    MoveItem(usize, usize),
    ClearUpcoming,
    Shuffle(bool),
    SetRepeat(Repeat),
    Autoplay(bool),
    RadioResult(Vec<Track>),
    SetFx(Fx),
    /// Pause after N minutes (None cancels).
    Sleep(Option<u64>),
    SleepEndOfTrack(bool),
    Quit,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Shared {
    pub current: Option<Track>,
    pub pos: f64,
    pub dur: f64,
    pub paused: bool,
    pub playing: bool,
    /// True while mpv is still resolving / buffering the current song.
    pub loading: bool,
    pub error: String,
    /// Cheap to clone every frame; replaced only when the queue changes. Not part of the periodic
    /// state message: the window process fetches it separately when `queue_ver` changes.
    #[serde(skip)]
    pub queue: Arc<Vec<Track>>,
    pub idx: usize,
    pub queue_ver: u64,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub autoplay: bool,
    pub sleep_left: Option<u64>,
    pub sleep_eot: bool,
}

fn track_url(t: &Track) -> String {
    if t.local.is_empty() {
        format!("https://music.youtube.com/watch?v={}", t.id)
    } else {
        t.local.clone()
    }
}

/// How many upcoming songs get their stream URL resolved ahead of time.
const PREFETCH_AHEAD: usize = 6;
/// Resolved stream URLs expire on YouTube's side after a few hours; stay well below that.
const STREAM_TTL: Duration = Duration::from_secs(4 * 3600);

/// Direct audio URLs resolved in the background, so the next songs start without waiting for yt-dlp.
#[derive(Default)]
struct StreamCache {
    urls: HashMap<String, (String, Instant)>,
    /// Ids being resolved right now (so the same song is never resolved twice at once).
    busy: HashSet<String>,
}

/// Asks yt-dlp for the direct audio URL of a song.
fn resolve_stream(id: &str) -> Option<String> {
    let url = format!("https://music.youtube.com/watch?v={id}");
    let out = Command::new("yt-dlp")
        .args(["--quiet", "--no-warnings", "--no-playlist", "--socket-timeout", "15", "--format", YTDL_FORMAT, "--get-url"])
        .arg(url.as_str())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let first = String::from_utf8_lossy(&out.stdout).lines().next().unwrap_or("").trim().to_string();
    if first.starts_with("http") {
        Some(first)
    } else {
        None
    }
}

struct Engine {
    queue: Vec<Track>,
    /// Original order while shuffle is on.
    orig: Option<Vec<Track>>,
    idx: usize,
    child: Option<Child>,
    ipc: Option<Ipc>,
    started: Instant,
    got_pos: bool,
    /// Queue index already handed to mpv for gapless playback.
    appended: Option<usize>,
    volume: f32,
    fx: Fx,
    repeat: Repeat,
    shuffle: bool,
    autoplay: bool,
    waiting_radio: bool,
    fetching: bool,
    sleep_at: Option<Instant>,
    sleep_eot: bool,
    fail_streak: u32,
    listen: f64,
    counted: bool,
    last_tick: Instant,
    shared: Arc<Mutex<Shared>>,
    tx: Sender<Cmd>,
    streams: Arc<Mutex<StreamCache>>,
    /// The song now playing was started from a prefetched URL (so a failure can retry the normal way).
    warm: bool,
    /// Same, for the song handed to mpv for gapless playback.
    appended_warm: bool,
    last_prefetch: Instant,
}

impl Engine {
    /// Takes the prefetched URL for a song, if there is a fresh one.
    fn take_warm(&self, t: &Track) -> Option<String> {
        if !t.local.is_empty() {
            return None;
        }
        let mut c = self.streams.lock().unwrap_or_else(|p| p.into_inner());
        match c.urls.remove(&t.id) {
            Some((u, at)) if at.elapsed() < STREAM_TTL => Some(u),
            _ => None,
        }
    }

    /// Resolves the stream URLs of the next `PREFETCH_AHEAD` songs in the background.
    /// Songs that are already resolved (or being resolved) are skipped, so calling this often is cheap.
    fn prefetch(&self, start: usize) {
        for t in self.queue.iter().skip(start).take(PREFETCH_AHEAD) {
            if !t.local.is_empty() || t.id.is_empty() {
                continue;
            }
            {
                let mut c = self.streams.lock().unwrap_or_else(|p| p.into_inner());
                let fresh = c.urls.get(&t.id).map_or(false, |(_, at)| at.elapsed() < STREAM_TTL);
                if fresh || c.busy.contains(&t.id) {
                    continue;
                }
                c.busy.insert(t.id.clone());
            }
            let streams = self.streams.clone();
            let id = t.id.clone();
            std::thread::spawn(move || {
                let r = resolve_stream(&id);
                let mut c = streams.lock().unwrap_or_else(|p| p.into_inner());
                c.busy.remove(&id);
                match r {
                    Some(u) => {
                        c.urls.insert(id, (u, Instant::now()));
                        if c.urls.len() > 64 {
                            c.urls.retain(|_, v| v.1.elapsed() < STREAM_TTL);
                        }
                    }
                    None => log(1, "prefetch", format!("could not resolve a stream for {id} ahead of time")),
                }
            });
        }
    }

    fn kill(&mut self) {
        self.ipc = None;
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    fn set_error(&self, msg: String) {
        let mut s = self.shared.lock().unwrap_or_else(|p| p.into_inner());
        s.error = msg;
    }

    /// Starts mpv once and keeps it; every song is just a `loadfile` on this process.
    fn ensure_mpv(&mut self) -> bool {
        if let Some(c) = self.child.as_mut() {
            if matches!(c.try_wait(), Ok(None)) && self.ipc.is_some() {
                return true;
            }
        }
        self.kill();
        let _ = std::fs::remove_file(sock());
        let log_path = config_dir().join("mpv.log");
        if std::fs::metadata(&log_path).map(|m| m.len() > 300_000).unwrap_or(false) {
            let _ = std::fs::remove_file(&log_path);
        }
        let errf = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map(Stdio::from)
            .unwrap_or_else(|_| Stdio::null());
        let mut cmd = Command::new("mpv");
        for a in [
            "--no-video",
            "--force-window=no",
            "--idle=yes",
            "--audio-display=no",
            "--gapless-audio=yes",
            "--prefetch-playlist=yes",
            "--keep-open=no",
            "--audio-client-name=Rustym",
            "--msg-level=all=error,ytdl_hook=info",
        ] {
            cmd.arg(a);
        }
        cmd.arg(format!("--ytdl-format={YTDL_FORMAT}"))
            .arg(format!("--input-ipc-server={}", sock()))
            .arg(format!("--volume={}", self.volume as i32))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(errf);
        match cmd.spawn() {
            Ok(c) => {
                self.child = Some(c);
                for _ in 0..80 {
                    if let Some(ipc) = Ipc::connect() {
                        self.ipc = Some(ipc);
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                if self.ipc.is_none() {
                    log(2, "mpv", format!("mpv started but its control socket never appeared. {}", mpv_log_tail(10)));
                    self.set_error("mpv started but did not answer. Check the Debug logs page.".into());
                    self.kill();
                    return false;
                }
                log(0, "mpv", "persistent mpv started");
                self.apply_fx();
                self.apply_repeat();
                true
            }
            Err(e) => {
                log(2, "mpv", format!("could not start mpv: {e}"));
                self.set_error(format!("Could not start mpv ({e}). Is it installed?"));
                false
            }
        }
    }

    fn apply_fx(&mut self) {
        let af = self.fx.af();
        let speed = self.fx.speed.clamp(0.25, 3.0) as f64;
        let mut failed = false;
        if let Some(ipc) = self.ipc.as_mut() {
            ipc.set("speed", json!(speed));
            let ok = ipc.set("af", json!(af.clone()));
            if !ok && !af.is_empty() {
                log(2, "fx", format!("mpv rejected the audio filter chain: {af}"));
                ipc.set("af", json!(""));
                failed = true;
            }
        }
        if failed {
            self.set_error("Audio effects failed (pitch shifting needs ffmpeg built with rubberband).".into());
        }
    }

    fn apply_repeat(&mut self) {
        let one = self.repeat == Repeat::One;
        if let Some(ipc) = self.ipc.as_mut() {
            ipc.set("loop-file", json!(if one { "inf" } else { "no" }));
        }
    }

    fn load(&mut self, url: &str, mode: &str) -> bool {
        match self.ipc.as_mut() {
            Some(ipc) => ipc.cmd(json!(["loadfile", url, mode])).map_or(false, |v| v["error"] == "success"),
            None => false,
        }
    }

    fn flush_listen(&mut self) {
        let cur = self.shared.lock().unwrap_or_else(|p| p.into_inner()).current.clone();
        if let Some(t) = cur {
            if self.listen >= 1.0 {
                crate::store::add_secs(&t, self.listen);
            }
        }
        self.listen = 0.0;
    }

    fn publish_queue(&mut self) {
        let mut s = self.shared.lock().unwrap_or_else(|p| p.into_inner());
        s.queue = Arc::new(self.queue.clone());
        s.idx = self.idx;
        s.queue_ver += 1;
        s.shuffle = self.shuffle;
        s.repeat = self.repeat;
        s.autoplay = self.autoplay;
        s.sleep_eot = self.sleep_eot;
    }

    fn invalidate_prefetch(&mut self) {
        if self.appended.take().is_some() {
            if let Some(ipc) = self.ipc.as_mut() {
                ipc.cmd(json!(["playlist-clear"]));
            }
        }
    }

    fn play(&mut self, i: usize) {
        if i >= self.queue.len() {
            return;
        }
        self.flush_listen();
        let t = self.queue[i].clone();
        // A prefetched URL starts at once; otherwise mpv resolves the song itself through yt-dlp.
        let (url, warm) = match self.take_warm(&t) {
            Some(u) => (u, true),
            None => (track_url(&t), false),
        };
        let mut ok = self.ensure_mpv() && self.load(&url, "replace");
        if !ok {
            // mpv may have died in the meantime: restart it once
            self.kill();
            ok = self.ensure_mpv() && self.load(&url, "replace");
        }
        if !ok {
            self.idx = i;
            self.fail_streak = self.fail_streak.saturating_add(1);
            let hint = play_error_hint();
            log(2, "mpv", format!(
                "could not start '{}': {hint}; skipping to the next queue item", t.title
            ));
            self.set_error(if hint.is_empty() { "Could not start this track.".into() } else { hint });
            // A queue containing only unplayable entries must not loop forever, even
            // when Repeat All is enabled. A successful track resets fail_streak below.
            if self.fail_streak < self.queue.len().max(1) as u32 && self.next_index(false).is_some() {
                self.advance(false);
            } else {
                self.stop();
            }
            return;
        }
        if let Some(ipc) = self.ipc.as_mut() {
            ipc.set("pause", json!(false));
        }
        log(0, "mpv", format!("play: {} - {} (id {}){}", t.artist, t.title, t.id, if warm { " [prefetched]" } else { "" }));
        self.warm = warm;
        self.idx = i;
        self.started = Instant::now();
        self.got_pos = false;
        self.appended = None;
        self.listen = 0.0;
        self.counted = false;
        self.last_tick = Instant::now();
        {
            let mut s = self.shared.lock().unwrap_or_else(|p| p.into_inner());
            s.dur = t.dur.unwrap_or(0.0);
            s.current = Some(t);
            s.pos = 0.0;
            s.paused = false;
            s.playing = true;
            s.loading = true;
            s.error.clear();
            s.idx = i;
        }
        self.last_prefetch = Instant::now();
        self.prefetch(i + 1);
    }

    fn stop(&mut self) {
        self.flush_listen();
        self.appended = None;
        self.waiting_radio = false;
        if let Some(ipc) = self.ipc.as_mut() {
            ipc.cmd(json!(["stop"]));
        }
        let mut s = self.shared.lock().unwrap_or_else(|p| p.into_inner());
        s.playing = false;
        s.paused = false;
        s.loading = false;
        s.pos = 0.0;
    }

    fn next_index(&self, auto: bool) -> Option<usize> {
        let n = self.queue.len();
        if n == 0 {
            return None;
        }
        if self.repeat == Repeat::One && auto {
            return Some(self.idx);
        }
        if self.idx + 1 < n {
            Some(self.idx + 1)
        } else if self.repeat == Repeat::All {
            Some(0)
        } else {
            None
        }
    }

    fn advance(&mut self, auto: bool) {
        if auto && self.sleep_eot {
            self.sleep_eot = false;
            log(0, "sleep", "sleep timer: stopping at the end of the song");
            self.stop();
            self.publish_queue();
            return;
        }
        match self.next_index(auto) {
            Some(n) => self.play(n),
            None => {
                let can_radio = self.autoplay && self.queue.get(self.idx).map_or(false, |t| t.local.is_empty());
                if can_radio {
                    self.waiting_radio = true;
                    self.fetch_radio();
                } else {
                    self.stop();
                }
            }
        }
    }

    fn fetch_radio(&mut self) {
        if self.fetching {
            return;
        }
        let Some(seed) = self.queue.get(self.idx).cloned() else { return };
        if !seed.local.is_empty() {
            return;
        }
        self.fetching = true;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = crate::ytm::radio(&seed.id).unwrap_or_else(|e| {
                log(1, "radio", format!("no radio for {}: {e}", seed.id));
                vec![]
            });
            let _ = tx.send(Cmd::RadioResult(r));
        });
    }

    fn prev(&mut self) {
        let pos = self.shared.lock().unwrap_or_else(|p| p.into_inner()).pos;
        if pos > 4.0 || self.idx == 0 {
            let i = self.idx;
            self.play(i);
        } else {
            let i = self.idx - 1;
            self.play(i);
        }
    }

    fn toggle(&mut self) {
        let (playing, paused) = {
            let s = self.shared.lock().unwrap_or_else(|p| p.into_inner());
            (s.playing, s.paused)
        };
        if playing && self.ipc.is_some() {
            if let Some(ipc) = self.ipc.as_mut() {
                ipc.set("pause", json!(!paused));
            }
            self.shared.lock().unwrap_or_else(|p| p.into_inner()).paused = !paused;
        } else if !self.queue.is_empty() {
            let i = self.idx.min(self.queue.len() - 1);
            self.play(i);
        }
    }

    fn set_queue(&mut self, q: Vec<Track>, i: usize) {
        if q.is_empty() {
            return;
        }
        let i = i.min(q.len() - 1);
        self.orig = None;
        self.waiting_radio = false;
        if self.shuffle {
            let chosen = q[i].clone();
            let mut rest: Vec<Track> = q.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, t)| t.clone()).collect();
            rest.shuffle(&mut rand::thread_rng());
            self.orig = Some(q);
            let mut nq = vec![chosen];
            nq.extend(rest);
            self.queue = nq;
            self.idx = 0;
        } else {
            self.queue = q;
            self.idx = i;
        }
        let start = self.idx;
        self.play(start);
        self.publish_queue();
    }

    fn set_shuffle(&mut self, on: bool) {
        if on == self.shuffle {
            return;
        }
        self.shuffle = on;
        self.invalidate_prefetch();
        if on {
            self.orig = Some(self.queue.clone());
            let start = (self.idx + 1).min(self.queue.len());
            self.queue[start..].shuffle(&mut rand::thread_rng());
        } else if let Some(orig) = self.orig.take() {
            let cur_id = self.queue.get(self.idx).map(|t| t.id.clone());
            let mut q = orig;
            for t in self.queue.iter() {
                if !q.iter().any(|x| x.id == t.id) {
                    q.push(t.clone());
                }
            }
            self.idx = cur_id.and_then(|id| q.iter().position(|t| t.id == id)).unwrap_or(0);
            self.queue = q;
        }
        self.publish_queue();
    }

    fn enqueue(&mut self, tracks: Vec<Track>) {
        if tracks.is_empty() {
            return;
        }
        if self.queue.is_empty() {
            self.set_queue(tracks, 0);
            return;
        }
        self.invalidate_prefetch();
        if let Some(o) = self.orig.as_mut() {
            o.extend(tracks.clone());
        }
        self.queue.extend(tracks);
        self.publish_queue();
    }

    fn play_next(&mut self, tracks: Vec<Track>) {
        if tracks.is_empty() {
            return;
        }
        if self.queue.is_empty() {
            self.set_queue(tracks, 0);
            return;
        }
        self.invalidate_prefetch();
        let at = (self.idx + 1).min(self.queue.len());
        if let Some(o) = self.orig.as_mut() {
            o.extend(tracks.clone());
        }
        for (k, t) in tracks.into_iter().enumerate() {
            self.queue.insert(at + k, t);
        }
        self.publish_queue();
    }

    fn remove_at(&mut self, i: usize) {
        if i >= self.queue.len() {
            return;
        }
        self.invalidate_prefetch();
        let gone = self.queue.remove(i);
        if let Some(o) = self.orig.as_mut() {
            if let Some(p) = o.iter().position(|t| t.id == gone.id) {
                o.remove(p);
            }
        }
        let playing = self.shared.lock().unwrap_or_else(|p| p.into_inner()).playing;
        if i < self.idx {
            self.idx -= 1;
        } else if i == self.idx && playing {
            // the next song slides into this slot
            if self.idx < self.queue.len() {
                let k = self.idx;
                self.play(k);
            } else {
                self.idx = self.queue.len().saturating_sub(1);
                self.stop();
            }
        }
        self.publish_queue();
    }

    fn move_item(&mut self, from: usize, to: usize) {
        let n = self.queue.len();
        if from >= n || to >= n || from == to {
            return;
        }
        self.invalidate_prefetch();
        let t = self.queue.remove(from);
        self.queue.insert(to, t);
        if from == self.idx {
            self.idx = to;
        } else if from < self.idx && to >= self.idx {
            self.idx -= 1;
        } else if from > self.idx && to <= self.idx {
            self.idx += 1;
        }
        self.publish_queue();
    }

    fn clear_upcoming(&mut self) {
        self.invalidate_prefetch();
        self.queue.truncate((self.idx + 1).min(self.queue.len()));
        if let Some(o) = self.orig.as_mut() {
            let keep: Vec<String> = self.queue.iter().map(|t| t.id.clone()).collect();
            o.retain(|t| keep.contains(&t.id));
        }
        self.publish_queue();
    }

    fn tick(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f64().min(1.0);
        self.last_tick = now;

        // sleep timer
        if let Some(at) = self.sleep_at {
            if now >= at {
                self.sleep_at = None;
                if let Some(ipc) = self.ipc.as_mut() {
                    ipc.set("pause", json!(true));
                }
                log(0, "sleep", "sleep timer: paused");
                let mut s = self.shared.lock().unwrap_or_else(|p| p.into_inner());
                s.paused = true;
                s.sleep_left = None;
            } else {
                let left = at.saturating_duration_since(now).as_secs();
                self.shared.lock().unwrap_or_else(|p| p.into_inner()).sleep_left = Some(left);
            }
        }

        // mpv died?
        let dead = match self.child.as_mut() {
            Some(c) => matches!(c.try_wait(), Ok(Some(_))),
            None => false,
        };
        if dead {
            self.child = None;
            self.ipc = None;
            self.appended = None;
            let playing = self.shared.lock().unwrap_or_else(|p| p.into_inner()).playing;
            if playing {
                let hint = play_error_hint();
                log(2, "mpv", format!("mpv exited unexpectedly. {hint}\n--- mpv output ---\n{}", mpv_log_tail(25)));
                let mut s = self.shared.lock().unwrap_or_else(|p| p.into_inner());
                s.playing = false;
                s.loading = false;
                s.error = hint;
            }
            return;
        }
        if self.ipc.is_none() {
            return;
        }
        let playing = self.shared.lock().unwrap_or_else(|p| p.into_inner()).playing;
        if !playing {
            return;
        }
        let (pos, dur, paused, idle, plpos) = match self.ipc.as_mut() {
            Some(ipc) => (
                ipc.f64("time-pos"),
                ipc.f64("duration"),
                ipc.bool("pause"),
                ipc.bool("idle-active").unwrap_or(false),
                ipc.get("playlist-pos").and_then(|v| v.as_i64()),
            ),
            None => return,
        };

        // gapless hand-over: mpv moved on to the entry we appended
        if let (Some(n), Some(1)) = (self.appended, plpos) {
            self.flush_listen();
            self.appended = None;
            if let Some(ipc) = self.ipc.as_mut() {
                ipc.cmd(json!(["playlist-remove", 0]));
            }
            if let Some(t) = self.queue.get(n).cloned() {
                self.idx = n;
                self.counted = false;
                self.got_pos = true;
                self.warm = self.appended_warm;
                self.started = Instant::now();
                self.fail_streak = 0;
                self.last_prefetch = Instant::now();
                self.prefetch(n + 1);
                let mut s = self.shared.lock().unwrap_or_else(|p| p.into_inner());
                s.dur = dur.or(t.dur).unwrap_or(0.0);
                s.current = Some(t);
                s.idx = n;
                s.pos = pos.unwrap_or(0.0);
                s.loading = false;
            }
            return;
        }

        if pos.is_some() {
            if !self.got_pos {
                // Reaching a real playback position confirms this queue item is playable.
                self.fail_streak = 0;
            }
            self.got_pos = true;
        }
        {
            let mut s = self.shared.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(p) = pos {
                s.pos = p;
            }
            if let Some(d) = dur {
                s.dur = d;
            }
            if let Some(b) = paused {
                s.paused = b;
            }
            s.loading = !self.got_pos;
        }

        // listening time + "counted as a play" after 30 s (or half of a short song)
        if self.got_pos && paused == Some(false) {
            self.listen += dt;
        }
        if !self.counted && self.listen >= 5.0 {
            let d = dur.unwrap_or(0.0);
            let need = if d > 0.0 { (d * 0.5).min(30.0).max(5.0) } else { 30.0 };
            if self.listen >= need {
                self.counted = true;
                let cur = self.shared.lock().unwrap_or_else(|p| p.into_inner()).current.clone();
                if let Some(t) = cur {
                    crate::store::count_play(&t);
                }
            }
        }

        // song finished (or failed to load)
        if idle && !self.waiting_radio && self.started.elapsed() > Duration::from_millis(1500) {
            if self.got_pos {
                self.fail_streak = 0;
                self.advance(true);
            } else if self.warm {
                // The prefetched URL did not work (expired, or YouTube refused it): drop it and
                // let mpv resolve the song itself, exactly as if nothing had been prefetched.
                self.warm = false;
                let title = self.queue.get(self.idx).map(|t| t.title.clone()).unwrap_or_default();
                log(1, "mpv", format!("prefetched stream for '{title}' failed; retrying with a fresh lookup"));
                let i = self.idx;
                self.play(i);
            } else {
                self.fail_streak += 1;
                let hint = play_error_hint();
                let title = self.queue.get(self.idx).map(|t| t.title.clone()).unwrap_or_default();
                log(2, "mpv", format!("could not play '{title}': {hint}\n--- mpv output ---\n{}", mpv_log_tail(25)));
                self.set_error(hint);
                let can_skip = self.next_index(false).is_some();
                // Keep trying subsequent queue items until one plays or every item has
                // failed once. This also prevents an all-broken Repeat All queue looping.
                if self.fail_streak < self.queue.len().max(1) as u32 && can_skip {
                    self.advance(false);
                } else {
                    self.stop();
                }
            }
            return;
        }

        // keep the queue going with YouTube Music radio
        let left = self.queue.len().saturating_sub(self.idx + 1);
        if self.autoplay
            && !self.fetching
            && !self.waiting_radio
            && self.got_pos
            && self.repeat == Repeat::Off
            && left <= 1
            && self.queue.get(self.idx).map_or(false, |t| t.local.is_empty())
        {
            self.fetch_radio();
        }

        // keep the next songs' stream URLs warm (also picks up queue edits and radio additions)
        if self.got_pos && self.last_prefetch.elapsed() > Duration::from_secs(5) {
            self.last_prefetch = Instant::now();
            let from = self.idx + 1;
            self.prefetch(from);
        }

        // gapless: hand the next song to mpv shortly before this one ends
        if self.appended.is_none() && self.got_pos && !self.sleep_eot && self.repeat != Repeat::One {
            if let (Some(p), Some(d)) = (pos, dur) {
                if d > 0.0 && d - p < 20.0 {
                    if let Some(n) = self.next_index(false) {
                        if let Some(t) = self.queue.get(n).cloned() {
                            let (url, warm) = match self.take_warm(&t) {
                                Some(u) => (u, true),
                                None => (track_url(&t), false),
                            };
                            if self.load(&url, "append") {
                                self.appended = Some(n);
                                self.appended_warm = warm;
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Turns the tail of mpv.log into a message that says what actually went wrong.
fn play_error_hint() -> String {
    let txt = mpv_log_tail(40);
    play_error_hint_from(&txt)
}

fn play_error_hint_from(txt: &str) -> String {
    let low = txt.to_lowercase();
    let last = txt
        .lines()
        .rev()
        .find(|l| l.contains("ERROR") || l.contains("error"))
        .unwrap_or("")
        .trim()
        .to_string();
    let hint_owned: String;
    let hint: &str = if low.contains("error while loading shared libraries") {
        hint_owned = format!("mpv is out of sync with its libraries: run {} (then reinstall mpv if needed)", crate::distro::upgrade_hint());
        &hint_owned
    } else if low.contains("javascript runtime") || low.contains("n challenge") || low.contains("ejs") || low.contains("signature solving") {
        hint_owned = format!("yt-dlp needs a JavaScript runtime (deno): {}", install_deno());
        &hint_owned
    } else if low.contains("confirm you") && low.contains("bot") {
        "YouTube wants a bot check: sign in again in Settings, or update yt-dlp"
    } else if low.contains("requested format is not available") {
        hint_owned = format!("No audio format came back: update yt-dlp (yt-dlp -U, or {}) and install deno", crate::distro::upgrade_hint());
        &hint_owned
    } else if low.contains("unable to download") || low.contains("name or service not known") {
        "Network problem reaching YouTube"
    } else if low.contains("ytdl_hook") && low.contains("not found") || low.contains("youtube-dl") && low.contains("not found") {
        hint_owned = format!("mpv can't find yt-dlp: {}", install_ytdlp());
        &hint_owned
    } else {
        "Couldn’t play that track. Update yt-dlp (yt-dlp -U) and install deno"
    };
    if last.is_empty() {
        hint.to_string()
    } else {
        let short: String = last.chars().take(110).collect();
        format!("{hint}  [{short}]")
    }
}

pub fn spawn_engine(volume: f32, repaint: Repaint) -> (Sender<Cmd>, Arc<Mutex<Shared>>) {
    let (tx, rx) = channel::<Cmd>();
    let shared = Arc::new(Mutex::new(Shared { autoplay: true, ..Default::default() }));
    let sh = shared.clone();
    let tx2 = tx.clone();
    std::thread::spawn(move || {
        let mut e = Engine {
            queue: vec![],
            orig: None,
            idx: 0,
            child: None,
            ipc: None,
            started: Instant::now(),
            got_pos: false,
            appended: None,
            volume,
            fx: Fx::default(),
            repeat: Repeat::Off,
            shuffle: false,
            autoplay: true,
            waiting_radio: false,
            fetching: false,
            sleep_at: None,
            sleep_eot: false,
            fail_streak: 0,
            listen: 0.0,
            counted: false,
            last_tick: Instant::now(),
            shared: sh,
            tx: tx2,
            streams: Arc::new(Mutex::new(StreamCache::default())),
            warm: false,
            appended_warm: false,
            last_prefetch: Instant::now(),
        };
        loop {
            let wait = if e.shared.lock().map(|s| s.playing).unwrap_or(false) { 250 } else { 600 };
            let mut got_cmd = false;
            match rx.recv_timeout(Duration::from_millis(wait)) {
                Ok(c) => {
                    got_cmd = true;
                    match c {
                        Cmd::Play(q, i) => e.set_queue(q, i),
                        Cmd::Toggle => e.toggle(),
                        Cmd::Next => e.advance(false),
                        Cmd::Prev => e.prev(),
                        Cmd::Seek(t) => {
                            if let Some(ipc) = e.ipc.as_mut() {
                                ipc.cmd(json!(["seek", t, "absolute"]));
                            }
                            e.shared.lock().unwrap_or_else(|p| p.into_inner()).pos = t;
                        }
                        Cmd::SeekBy(d) => {
                            if let Some(ipc) = e.ipc.as_mut() {
                                ipc.cmd(json!(["seek", d, "relative"]));
                            }
                        }
                        Cmd::Volume(v) => {
                            e.volume = v;
                            if let Some(ipc) = e.ipc.as_mut() {
                                ipc.set("volume", json!(v as f64));
                            }
                        }
                        Cmd::Jump(i) => {
                            e.waiting_radio = false;
                            e.play(i);
                        }
                        Cmd::Enqueue(t) => e.enqueue(t),
                        Cmd::PlayNext(t) => e.play_next(t),
                        Cmd::RemoveAt(i) => e.remove_at(i),
                        Cmd::MoveItem(a, b) => e.move_item(a, b),
                        Cmd::ClearUpcoming => e.clear_upcoming(),
                        Cmd::Shuffle(on) => e.set_shuffle(on),
                        Cmd::SetRepeat(r) => {
                            e.repeat = r;
                            e.invalidate_prefetch();
                            e.apply_repeat();
                            e.publish_queue();
                        }
                        Cmd::Autoplay(on) => {
                            e.autoplay = on;
                            e.publish_queue();
                        }
                        Cmd::RadioResult(v) => {
                            e.fetching = false;
                            let have: std::collections::HashSet<String> = e.queue.iter().map(|t| t.id.clone()).collect();
                            let fresh: Vec<Track> = v.into_iter().filter(|t| !have.contains(&t.id)).collect();
                            if fresh.is_empty() {
                                if e.waiting_radio {
                                    e.waiting_radio = false;
                                    e.stop();
                                }
                            } else {
                                if let Some(o) = e.orig.as_mut() {
                                    o.extend(fresh.clone());
                                }
                                e.queue.extend(fresh);
                                e.publish_queue();
                                if e.waiting_radio {
                                    e.waiting_radio = false;
                                    let n = e.idx + 1;
                                    if n < e.queue.len() {
                                        e.play(n);
                                    }
                                }
                            }
                        }
                        Cmd::SetFx(fx) => {
                            e.fx = fx;
                            e.apply_fx();
                        }
                        Cmd::Sleep(m) => {
                            e.sleep_at = m.map(|m| Instant::now() + Duration::from_secs(m * 60));
                            e.sleep_eot = false;
                            if m.is_none() {
                                e.shared.lock().unwrap_or_else(|p| p.into_inner()).sleep_left = None;
                            }
                            e.publish_queue();
                        }
                        Cmd::SleepEndOfTrack(on) => {
                            e.sleep_eot = on;
                            if on {
                                e.sleep_at = None;
                                e.shared.lock().unwrap_or_else(|p| p.into_inner()).sleep_left = None;
                                e.invalidate_prefetch();
                            }
                            e.publish_queue();
                        }
                        Cmd::Quit => {
                            e.flush_listen();
                            e.kill();
                            cleanup();
                            std::process::exit(0);
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    e.flush_listen();
                    e.kill();
                    break;
                }
            }
            e.tick();
            let active = e.shared.lock().map(|s| s.playing && !s.paused).unwrap_or(false);
            if got_cmd || active {
                repaint();
            }
        }
    });
    (tx, shared)
}

// ---------------- visualiser (captures system audio via parec, Goertzel bands) ----------------

pub struct VizData {
    pub levels: Vec<f32>,
    pub updated: Instant,
    pub live: bool,
}

fn goertzel(samples: &[f32], freq: f32, fs: f32) -> f32 {
    let w = 2.0 * std::f32::consts::PI * freq / fs;
    let c = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f32, 0.0f32);
    for &x in samples {
        let s = x + c * s1 - s2;
        s2 = s1;
        s1 = s;
    }
    (s1 * s1 + s2 * s2 - c * s1 * s2).max(0.0).sqrt() / samples.len() as f32
}

pub fn spawn_viz() -> Arc<Mutex<VizData>> {
    let data = Arc::new(Mutex::new(VizData {
        levels: vec![0.0; BANDS],
        updated: Instant::now(),
        live: false,
    }));
    let d = data.clone();
    std::thread::spawn(move || {
        let child = Command::new("parec")
            .args([
                "-d",
                "@DEFAULT_MONITOR@",
                "--format=s16le",
                "--rate=22050",
                "--channels=1",
                "--latency-msec=40",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let Ok(mut child) = child else { return };
        PAREC_PID.store(child.id(), SeqCst);
        let Some(mut out) = child.stdout.take() else { return };
        const N: usize = 1024;
        const FS: f32 = 22050.0;
        let mut buf = [0u8; N * 2];
        // Reused every chunk - the visualiser runs ~60 times per second, so
        // allocating fresh Vecs for each frame would churn the allocator.
        let mut samples = [0f32; N];
        let mut raw = [0f32; BANDS];
        let win: Vec<f32> = (0..N)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (N as f32 - 1.0)).cos())
            .collect();
        let freqs: Vec<f32> = (0..BANDS)
            .map(|i| 50.0 * (8000.0f32 / 50.0).powf(i as f32 / (BANDS as f32 - 1.0)))
            .collect();
        let mut peak = 0.45f32;
        loop {
            if out.read_exact(&mut buf).is_err() {
                break;
            }
            for (i, s) in samples.iter_mut().enumerate() {
                *s = i16::from_le_bytes([buf[2 * i], buf[2 * i + 1]]) as f32 / 32768.0 * win[i];
            }
            let mut mx = 0.0f32;
            for (i, &f) in freqs.iter().enumerate() {
                let a = goertzel(&samples, f, FS);
                let db = 20.0 * (a + 1e-7).log10();
                let v = ((db + 72.0) / 52.0).clamp(0.0, 1.0) * (0.85 + 0.3 * i as f32 / BANDS as f32);
                raw[i] = v;
                mx = mx.max(v);
            }
            peak = (peak * 0.992).max(mx).max(0.45);
            for v in raw.iter_mut() {
                *v = (*v / peak).clamp(0.0, 1.0);
            }
            let mut g = d.lock().unwrap();
            g.levels = raw.to_vec();
            g.updated = Instant::now();
            g.live = true;
        }
        let _ = child.kill();
        d.lock().unwrap().live = false;
    });
    data
}

// ---------------- window lifecycle (close-to-tray), tray, single-instance + CLI control ----------------
//
// Two processes, so closing the window really closes it on X11 and on every Wayland compositor:
//   * the background process (plain `rustym`, or `rustym --background`): owns the engine, the tray icon,
//     media keys and the control socket. It never opens a window itself, so it can never show
//     "not responding".
//   * the window process (`rustym --gui`, started by the background process): the egui UI. It talks to the
//     background process over the control socket. Closing the window simply ends this process.
// No "hide window" support is needed (winit's set_visible is unsupported on Wayland) and no windows are
// re-created inside one process.

pub struct WinCtl {
    /// A window process is currently running.
    pub gui_open: AtomicBool,
    pub want_show: AtomicBool,
    /// Close the window process (the app keeps running).
    pub want_hide: AtomicBool,
}

impl WinCtl {
    pub fn new() -> Self {
        WinCtl { gui_open: AtomicBool::new(false), want_show: AtomicBool::new(false), want_hide: AtomicBool::new(false) }
    }
}

pub fn show_now(win: &WinCtl) {
    if !win.gui_open.load(SeqCst) {
        win.want_show.store(true, SeqCst);
    }
}

pub fn hide_now(win: &WinCtl) {
    if win.gui_open.load(SeqCst) {
        win.want_hide.store(true, SeqCst);
    }
}

pub fn toggle_window(win: &WinCtl) {
    if win.gui_open.load(SeqCst) {
        hide_now(win);
    } else {
        show_now(win);
    }
}

/// Path of this executable, tolerant of the binary having been replaced while running.
pub fn self_exe() -> PathBuf {
    let p = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("rustym"));
    let s = p.to_string_lossy().to_string();
    match s.strip_suffix(" (deleted)") {
        Some(t) => PathBuf::from(t),
        None => p,
    }
}

/// Starts the window process.
pub fn spawn_gui() -> std::io::Result<Child> {
    Command::new(self_exe()).arg("--gui").stdin(Stdio::null()).spawn()
}

// ----- tray detection -----

/// Asks the session bus whether `name` has an owner. None = no D-Bus tool could answer.
fn name_has_owner(name: &str) -> Option<bool> {
    let sn = format!("string:{name}");
    let owned = |v: &[&str]| -> Vec<String> { v.iter().map(|x| x.to_string()).collect() };
    let attempts: [(&str, Vec<String>, &str); 3] = [
        (
            "busctl",
            owned(&["--user", "call", "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "NameHasOwner", "s", name]),
            "true",
        ),
        (
            "gdbus",
            owned(&["call", "--session", "--dest", "org.freedesktop.DBus", "--object-path", "/org/freedesktop/DBus", "--method", "org.freedesktop.DBus.NameHasOwner", name]),
            "true",
        ),
        (
            "dbus-send",
            owned(&["--session", "--print-reply", "--dest=org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus.NameHasOwner", sn.as_str()]),
            "boolean true",
        ),
    ];
    for (bin, args, yes) in attempts {
        if let Ok(o) = Command::new(bin).args(&args).stdin(Stdio::null()).stderr(Stdio::null()).output() {
            if o.status.success() {
                return Some(String::from_utf8_lossy(&o.stdout).contains(yes));
            }
        }
    }
    None
}

/// Some(true) when a StatusNotifier host (KDE, waybar, GNOME AppIndicator extension, ...) is running,
/// Some(false) when none is, None when it could not be determined.
fn watcher_present() -> Option<bool> {
    let mut answered = false;
    for name in ["org.kde.StatusNotifierWatcher", "org.freedesktop.StatusNotifierWatcher"] {
        match name_has_owner(name) {
            Some(true) => return Some(true),
            Some(false) => answered = true,
            None => {}
        }
    }
    if answered {
        Some(false)
    } else {
        None
    }
}

/// Background process: true when our tray icon is registered and a tray host is still running.
pub fn tray_available() -> bool {
    TRAY_UP.load(SeqCst) && watcher_present() != Some(false)
}

/// Any process: is a tray host (StatusNotifierWatcher) running on this desktop?
pub fn tray_host_running() -> bool {
    watcher_present() != Some(false)
}

/// After close-to-tray without a tray icon: say once, via notification, how to reopen the window.
pub fn warn_no_tray() {
    std::thread::spawn(|| {
        if tray_available() {
            return;
        }
        let _ = Command::new("notify-send")
            .args([
                "--app-name=Rustym",
                "--icon=rustym",
                "--",
                "Rustym is still running",
                "No system tray found. Launch Rustym again (or run: rustym --show) to reopen the window. rustym --quit stops it.",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    });
}

// ----- tray icon -----

struct RustymTray {
    tx: Sender<Cmd>,
    win: Arc<WinCtl>,
}

/// The tray icon as an in-binary ARGB32 pixmap. Some StatusNotifier hosts (certain panels,
/// appindicator setups and some Wayland bars) never resolve the theme `IconName`, or cannot load
/// the installed SVG, and would show an empty slot. Shipping the pixels alongside the name makes
/// the icon show up everywhere, installed or not.
static TRAY_PIXMAP: std::sync::LazyLock<ksni::Icon> = std::sync::LazyLock::new(|| {
    let img = image::load_from_memory_with_format(
        include_bytes!("../packaging/icons/rustym.png"),
        image::ImageFormat::Png,
    )
    .expect("valid tray icon PNG");
    use image::GenericImageView;
    let (width, height) = img.dimensions();
    let mut data = img.into_rgba8().into_vec();
    for pixel in data.chunks_exact_mut(4) {
        pixel.rotate_right(1); // RGBA -> ARGB
    }
    ksni::Icon {
        width: width as i32,
        height: height as i32,
        data,
    }
});

impl ksni::Tray for RustymTray {
    fn id(&self) -> String {
        "rustym".into()
    }
    fn title(&self) -> String {
        "Rustym".into()
    }
    fn icon_name(&self) -> String {
        "rustym".into()
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![TRAY_PIXMAP.clone()]
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        toggle_window(&self.win);
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        vec![
            StandardItem {
                label: "Show / Hide".into(),
                activate: Box::new(|t: &mut RustymTray| toggle_window(&t.win)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Play / Pause".into(),
                activate: Box::new(|t: &mut RustymTray| {
                    let _ = t.tx.send(Cmd::Toggle);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Next".into(),
                activate: Box::new(|t: &mut RustymTray| {
                    let _ = t.tx.send(Cmd::Next);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Previous".into(),
                activate: Box::new(|t: &mut RustymTray| {
                    let _ = t.tx.send(Cmd::Prev);
                }),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(|t: &mut RustymTray| {
                    let _ = t.tx.send(Cmd::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Registers the tray icon as soon as a tray host exists. Many sessions start the panel *after*
/// autostarted apps, so this waits instead of registering into the void and giving up.
pub fn spawn_tray(tx: Sender<Cmd>, win: Arc<WinCtl>) {
    std::thread::spawn(move || {
        loop {
            match watcher_present() {
                Some(false) => std::thread::sleep(Duration::from_secs(2)),
                _ => break, // present, or unknown: register anyway
            }
        }
        ksni::TrayService::new(RustymTray { tx, win }).spawn();
        TRAY_UP.store(true, SeqCst);
        log(0, "tray", "tray icon registered");
    });
}

// ----- single instance + remote control -----

/// Sends one command line to the running instance. Returns false when none is running.
pub fn send_instance(cmd: &str) -> bool {
    if let Ok(mut s) = UnixStream::connect(lock_path()) {
        let _ = s.write_all(format!("{cmd}\n").as_bytes());
        return true;
    }
    false
}

/// Background process: serves the control socket. Short one-word commands come from the CLI / second
/// launches; the window process keeps a connection open, sends `cmd <json>`, polls `state` and fetches
/// `queue` whenever the queue version changed.
pub fn listen_instance(win: Arc<WinCtl>, tx: Sender<Cmd>, shared: Arc<Mutex<Shared>>) {
    let path = lock_path();
    let _ = std::fs::remove_file(&path);
    let Ok(l) = UnixListener::bind(&path) else { return };
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(s) = s else { continue };
            let (win, tx, shared) = (win.clone(), tx.clone(), shared.clone());
            std::thread::spawn(move || {
                let Ok(mut w) = s.try_clone() else { return };
                for line in BufReader::new(s).lines() {
                    let Ok(line) = line else { break };
                    let line = line.trim();
                    if let Some(js) = line.strip_prefix("cmd ") {
                        if let Ok(c) = serde_json::from_str::<Cmd>(js) {
                            let _ = tx.send(c);
                        }
                        continue;
                    }
                    match line {
                        "state" => {
                            let j = serde_json::to_string(&*shared.lock().unwrap_or_else(|p| p.into_inner())).unwrap_or_default();
                            if writeln!(w, "{j}").is_err() {
                                break;
                            }
                        }
                        "queue" => {
                            let q = shared.lock().unwrap_or_else(|p| p.into_inner()).queue.clone();
                            let j = serde_json::to_string(&*q).unwrap_or_else(|_| "[]".into());
                            if writeln!(w, "{j}").is_err() {
                                break;
                            }
                        }
                        "toggle" => toggle_window(&win),
                        "hide" => hide_now(&win),
                        "playpause" => {
                            let _ = tx.send(Cmd::Toggle);
                        }
                        "next" => {
                            let _ = tx.send(Cmd::Next);
                        }
                        "prev" => {
                            let _ = tx.send(Cmd::Prev);
                        }
                        "quit" => {
                            let _ = tx.send(Cmd::Quit);
                        }
                        "ping" => {}
                        _ => show_now(&win), // "show", an empty line, or an older client
                    }
                }
            });
        }
    });
}

/// Window process: connects to the background process. Returns a command sender and a state snapshot
/// that is refreshed ~10x per second (`repaint` is called when something visible changed). If the
/// background process goes away, this process exits too.
pub fn connect_backend(repaint: Repaint) -> (Sender<Cmd>, Arc<Mutex<Shared>>) {
    let (tx, rx) = channel::<Cmd>();
    let shared = Arc::new(Mutex::new(Shared::default()));
    let sh = shared.clone();
    std::thread::spawn(move || {
        let mut stream = None;
        for _ in 0..50 {
            if let Ok(s) = UnixStream::connect(lock_path()) {
                stream = Some(s);
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let Some(stream) = stream else { std::process::exit(1) };
        let Ok(mut w) = stream.try_clone() else { std::process::exit(1) };
        let mut r = BufReader::new(stream);
        let mut queue: Arc<Vec<Track>> = Arc::new(vec![]);
        let mut queue_ver = u64::MAX;
        let mut last_sig: (Option<String>, bool, bool, bool, u64, usize) = (None, false, false, false, 0, 0);
        loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(c) => {
                    if let Ok(j) = serde_json::to_string(&c) {
                        if writeln!(w, "cmd {j}").is_err() {
                            std::process::exit(0);
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if writeln!(w, "state").is_err() {
                std::process::exit(0);
            }
            let mut line = String::new();
            match r.read_line(&mut line) {
                Ok(0) | Err(_) => std::process::exit(0),
                Ok(_) => {}
            }
            let Ok(mut st) = serde_json::from_str::<Shared>(line.trim()) else { continue };
            if st.queue_ver != queue_ver {
                if writeln!(w, "queue").is_err() {
                    std::process::exit(0);
                }
                let mut ql = String::new();
                match r.read_line(&mut ql) {
                    Ok(0) | Err(_) => std::process::exit(0),
                    Ok(_) => {}
                }
                if let Ok(v) = serde_json::from_str::<Vec<Track>>(ql.trim()) {
                    queue = Arc::new(v);
                    queue_ver = st.queue_ver;
                }
            }
            st.queue = queue.clone();
            let sig = (st.current.as_ref().map(|t| t.id.clone()), st.playing, st.paused, st.loading, st.queue_ver, st.error.len());
            if sig != last_sig {
                last_sig = sig;
                repaint();
            }
            *sh.lock().unwrap_or_else(|p| p.into_inner()) = st;
        }
    });
    (tx, shared)
}
