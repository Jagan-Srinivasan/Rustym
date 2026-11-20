//! Everything that is not drawing: data types, config, yt-dlp, mpv engine thread,
//! audio visualiser capture, tray icon, Hyprland window hiding, single-instance.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering::SeqCst},
        mpsc::{channel, RecvTimeoutError, Sender},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

pub const SOCK: &str = "/tmp/rustym-mpv.sock";
pub const LOCK: &str = "/tmp/rustym-instance.sock";
pub const BANDS: usize = 48;
pub const BROWSERS: [&str; 7] = ["firefox", "chromium", "brave", "chrome", "vivaldi", "opera", "edge"];

static PAREC_PID: AtomicU32 = AtomicU32::new(0);

/// Stops helper processes and removes sockets. Safe to call more than once.
pub fn cleanup() {
    let pid = PAREC_PID.swap(0, SeqCst);
    if pid != 0 {
        let _ = Command::new("kill").arg(pid.to_string()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    let _ = std::fs::remove_file(SOCK);
    let _ = std::fs::remove_file(LOCK);
}

pub type Repaint = Arc<dyn Fn() + Send + Sync>;

// ---------------- data ----------------

#[derive(Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub dur: Option<f64>,
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
    pub browser: String,
    pub volume: f32,
    pub viz: usize,
    pub signed_in: bool,
    /// Keep running (tray) when the window is closed. Off by default: closing the window quits.
    pub background: bool,
    pub playlists: Vec<Playlist>,
    pub recent: Vec<Track>,
    pub trending: Vec<Track>,
    pub remote: Vec<RemotePl>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            theme: 0,
            browser: "firefox".into(),
            volume: 80.0,
            viz: 0,
            signed_in: false,
            background: false,
            playlists: vec![],
            recent: vec![],
            trending: vec![],
            remote: vec![],
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
        let mut c: Config = std::fs::read_to_string(config_dir().join("config.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        if let Ok(b) = std::env::var("RUSTYM_BROWSER") {
            c.browser = b;
        }
        c
    }
    pub fn save(&self) {
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(config_dir().join("config.json"), s);
        }
    }
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

fn tz_offset() -> i64 {
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
                "session={} wayland={} display={} config_dir={}",
                std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "?".into()),
                std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "-".into()),
                std::env::var("DISPLAY").unwrap_or_else(|_| "-".into()),
                config_dir().display()
            ),
        );
        if let Ok((_, t)) = run_cmd("sh", &["-c", "pacman -Q mpv libjxl yt-dlp deno yt-dlp-ejs python ffmpeg 2>&1"]) {
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
                    "mpv is out of sync with its libraries: sudo pacman -Syu   (then: sudo pacman -S mpv)"
                } else {
                    "sudo pacman -S mpv"
                };
                push_check("mpv", 2, first_line(&t), fix);
            }
            Err(e) => push_check("mpv", 2, format!("cannot run mpv ({e})"), "sudo pacman -S mpv"),
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
                    "sudo pacman -Syu first. Still missing? find the owner with: sudo pacman -Fy ; pacman -F <libname>  then sudo pacman -S <package>",
                );
            }
        }

        // yt-dlp, python, deno
        match run_cmd("yt-dlp", &["--version"]) {
            Ok((true, t)) => push_check("yt-dlp", 0, format!("version {}", first_line(&t)), ""),
            Ok((false, t)) => push_check("yt-dlp", 2, first_line(&t), "sudo pacman -S yt-dlp"),
            Err(e) => push_check("yt-dlp", 2, format!("cannot run yt-dlp ({e})"), "sudo pacman -S yt-dlp"),
        }
        match run_cmd("python3", &["--version"]) {
            Ok((true, t)) => push_check("python3", 0, first_line(&t), ""),
            _ => push_check("python3", 2, "not found - needed for YouTube library + Firefox login", "sudo pacman -S python"),
        }
        match run_cmd("deno", &["--version"]) {
            Ok((true, t)) => push_check("deno (JS runtime)", 0, first_line(&t), ""),
            _ => push_check("deno (JS runtime)", 1, "not found - yt-dlp may fail to unlock streams", "sudo pacman -S deno yt-dlp-ejs"),
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
                match api_call_once("verify", "") {
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
                    Err(e) => push_check("playback test", 2, format!("could not run test ({e})"), "sudo pacman -S coreutils mpv"),
                }
            }
        }
        log(0, "check", "=== diagnostics finished ===");
        HEALTH_BUSY.store(false, SeqCst);
    });
}

const YTDL_FORMAT: &str = "140/251/bestaudio[protocol^=http][protocol!*=m3u8]/bestaudio/best";

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
            Some(Track { id, title, artist, dur })
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

const API_SCRIPT: &str = include_str!("rustym_api.py");

/// Runs the bundled YouTube Music client (src/rustym_api.py) with the saved cookies.
fn api_call(mode: &str, arg: &str) -> Result<Value, String> {
    match api_call_once(mode, arg) {
        // YouTube rejected the saved cookies: re-read the app's own login profile (which holds the
        // freshest session) and try once more, like limusic's session keep-alive.
        Err(e) if e.contains("rejected") || e.contains("signed out") => {
            log(1, "api", format!("session rejected on '{mode}', re-reading the login profile"));
            match refresh_session() {
                Ok(()) => api_call_once(mode, arg),
                Err(why) => {
                    log(2, "api", format!("could not refresh session: {why}"));
                    Err(e)
                }
            }
        }
        r => r,
    }
}

fn api_call_once(mode: &str, arg: &str) -> Result<Value, String> {
    let t0 = Instant::now();
    let r = api_call_raw(mode, arg);
    match &r {
        Ok(_) => log(0, "api", format!("{mode} ok ({:.1}s)", t0.elapsed().as_secs_f32())),
        Err(e) => log(2, "api", format!("{mode} failed ({:.1}s): {e}", t0.elapsed().as_secs_f32())),
    }
    r
}

fn api_call_raw(mode: &str, arg: &str) -> Result<Value, String> {
    if !cookie_file().exists() {
        return Err("Not signed in (no saved cookies). Sign in again.".into());
    }
    let o = Command::new("python3")
        .arg("-c")
        .arg(API_SCRIPT)
        .arg(cookie_file())
        .arg(mode)
        .arg(arg)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("python3 not available ({e})"))?;
    let v: Value = serde_json::from_slice(&o.stdout)
        .map_err(|_| format!("YouTube Music request failed: {}", last_line(&o.stderr)))?;
    if let Some(e) = v["error"].as_str() {
        return Err(e.to_string());
    }
    Ok(v)
}

/// Liked songs (`mode` = "liked") or a library playlist (`mode` = "playlist", `arg` = id),
/// fetched straight from the YouTube Music API.
pub fn api_tracks(mode: &str, arg: &str) -> Result<Vec<Track>, String> {
    let v = api_call(mode, arg)?;
    let t: Vec<Track> = v["tracks"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    Some(Track {
                        id: x["id"].as_str()?.to_string(),
                        title: x["title"].as_str()?.to_string(),
                        artist: x["artist"].as_str().unwrap_or("").to_string(),
                        dur: x["dur"].as_f64(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    if t.is_empty() {
        Err("No songs found there.".into())
    } else {
        Ok(t)
    }
}

/// The playlists saved in the user's YouTube Music library.
pub fn api_playlists() -> Result<Vec<RemotePl>, String> {
    let v = api_call("playlists", "")?;
    let p: Vec<RemotePl> = v["playlists"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| Some(RemotePl { id: x["id"].as_str()?.to_string(), title: x["title"].as_str()?.to_string() }))
                .collect()
        })
        .unwrap_or_default();
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

pub fn browser_bin(b: &str) -> &'static str {
    match b {
        "firefox" => "firefox",
        "chromium" => "chromium",
        "brave" => "brave",
        "chrome" => "google-chrome-stable",
        "vivaldi" => "vivaldi",
        "opera" => "opera",
        "edge" => "microsoft-edge-stable",
        _ => "xdg-open",
    }
}

/// True once `open_login` started the browser with the app's private login profile.
static ISOLATED: AtomicBool = AtomicBool::new(false);

/// The browser profile that belongs to rustym alone. Signing in there (instead of reading your
/// everyday browser) means Google never rotates these cookies behind our back - the same idea as
/// limusic's dedicated login window.
pub fn login_profile_dir() -> PathBuf {
    let d = config_dir().join("login-profile");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Signs out completely: saved cookies and the private login profile.
pub fn forget_login() {
    let _ = std::fs::remove_file(cookie_file());
    let _ = std::fs::remove_dir_all(config_dir().join("login-profile"));
}

/// Opens the Google sign-in page in the chosen browser, using rustym's private profile.
pub fn open_login(browser: &str) -> bool {
    let url = "https://accounts.google.com/ServiceLogin?service=youtube&continue=https%3A%2F%2Fmusic.youtube.com";
    let prof = login_profile_dir();
    let mut cmd = Command::new(browser_bin(browser));
    if browser == "firefox" {
        cmd.arg("--no-remote").arg("--profile").arg(&prof);
    } else if browser_bin(browser) != "xdg-open" {
        cmd.arg(format!("--user-data-dir={}", prof.display()))
            .arg("--password-store=basic")
            .arg("--no-first-run")
            .arg("--no-default-browser-check");
    }
    let isolated = cmd
        .arg(url)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok();
    ISOLATED.store(isolated && browser_bin(browser) != "xdg-open", SeqCst);
    log(
        if isolated { 0 } else { 2 },
        "login",
        format!("opening {browser} for Google sign-in (private profile {}, started={isolated})", prof.display()),
    );
    if !isolated {
        let _ = Command::new("xdg-open").arg(url).spawn();
    }
    isolated
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
        vec!["--cookies-from-browser".into(), c.browser.clone()]
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

fn browser_dirs(b: &str) -> &'static [&'static str] {
    match b {
        "firefox" => &[".mozilla/firefox", ".var/app/org.mozilla.firefox/.mozilla/firefox", "snap/firefox/common/.mozilla/firefox"],
        "chromium" => &[".config/chromium", ".var/app/org.chromium.Chromium/config/chromium", "snap/chromium/common/chromium"],
        "brave" => &[".config/BraveSoftware/Brave-Browser", ".var/app/com.brave.Browser/config/BraveSoftware/Brave-Browser"],
        "chrome" => &[".config/google-chrome", ".var/app/com.google.Chrome/config/google-chrome"],
        "vivaldi" => &[".config/vivaldi"],
        "opera" => &[".config/opera"],
        "edge" => &[".config/microsoft-edge"],
        _ => &[],
    }
}

fn browser_installed(b: &str) -> bool {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    browser_dirs(b).iter().any(|d| home.join(d).exists())
}

/// The chosen browser first, then every other browser that has a profile on disk.
/// (You might sign in with a different browser than the one selected in Settings.)
pub fn candidate_browsers(preferred: &str) -> Vec<String> {
    let mut v = vec![preferred.to_string()];
    for b in BROWSERS {
        if b != preferred && browser_installed(b) {
            v.push(b.to_string());
        }
    }
    v
}

/// Reads Firefox's cookie database *including its write-ahead log* (Firefox keeps fresh logins
/// in the -wal file, which yt-dlp ignores) and writes a Netscape cookies.txt.
const FF_SCRIPT: &str = r#"
import os, sys, shutil, sqlite3, tempfile
out = sys.argv[1]
dbs = []
for root in sys.argv[2:]:
    for dp, dn, fn in os.walk(root):
        if 'cookies.sqlite' in fn:
            dbs.append(os.path.join(dp, 'cookies.sqlite'))
if not dbs:
    print('no Firefox profile found'); sys.exit(2)
def stamp(p):
    t = os.path.getmtime(p)
    if os.path.exists(p + '-wal'):
        t = max(t, os.path.getmtime(p + '-wal'))
    return t
db = max(dbs, key=stamp)
tmp = tempfile.mkdtemp()
try:
    shutil.copy(db, tmp + '/c.sqlite')
    if os.path.exists(db + '-wal'):
        shutil.copy(db + '-wal', tmp + '/c.sqlite-wal')
    con = sqlite3.connect(tmp + '/c.sqlite')
    rows = con.execute("select host, path, isSecure, expiry, name, value from moz_cookies "
                       "where host like '%youtube.com' or host like '%google.com'").fetchall()
finally:
    shutil.rmtree(tmp, ignore_errors=True)
with open(out, 'w') as f:
    f.write('# Netscape HTTP Cookie File\n')
    for h, p, sec, exp, n, v in rows:
        if exp and exp > 10**11:
            exp //= 1000
        f.write('%s\t%s\t%s\t%s\t%s\t%s\t%s\n' % (h, 'TRUE' if h.startswith('.') else 'FALSE', p,
                'TRUE' if sec else 'FALSE', exp or 0, n, v))
print('ok', len(rows))
"#;

fn firefox_export(out: &std::path::Path, roots: &[PathBuf]) -> Result<(), String> {
    let mut cmd = Command::new("python3");
    cmd.arg("-c").arg(FF_SCRIPT).arg(out);
    for d in roots {
        cmd.arg(d);
    }
    let o = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("python3 not available ({e})"))?;
    if o.status.success() {
        Ok(())
    } else {
        let mut m = last_line(&o.stdout);
        if m == "no details" {
            m = last_line(&o.stderr);
        }
        Err(m)
    }
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

/// Looks for a Google login in the browser's cookies. On success the cookies are exported to
/// `cookie_file()` so they keep working afterwards.
pub fn check_login(browser: &str) -> Result<(), String> {
    if ISOLATED.load(SeqCst) {
        return check_login_isolated(browser);
    }
    let tmp = config_dir().join("cookies.tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut why = String::new();
    if browser == "firefox" {
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
        let roots: Vec<PathBuf> = browser_dirs("firefox").iter().map(|d| home.join(d)).collect();
        match firefox_export(&tmp, &roots) {
            Ok(()) if has_login_cookies(&tmp) => {
                install_cookies(&tmp);
                return Ok(());
            }
            Ok(()) => why = "Firefox has no Google login yet".to_string(),
            Err(e) => why = e,
        }
        let _ = std::fs::remove_file(&tmp);
    }
    // Chromium-based browsers encrypt cookies with a key from the system keyring. yt-dlp has to
    // guess which keyring, and on Hyprland it often guesses wrong, so try them all.
    let specs: Vec<String> = if browser == "firefox" {
        vec![browser.to_string()]
    } else {
        ["", "+gnomekeyring", "+kwallet6", "+kwallet5", "+kwallet", "+basictext"]
            .iter()
            .map(|k| format!("{browser}{k}"))
            .collect()
    };
    let mut last_err = String::new();
    for spec in specs {
        let _ = std::fs::remove_file(&tmp);
        let out = Command::new("yt-dlp")
            .args(["--cookies-from-browser", spec.as_str(), "--cookies"])
            .arg(&tmp)
            .args([
                "--flat-playlist",
                "--dump-json",
                "--playlist-end",
                "1",
                "--socket-timeout",
                "10",
                "https://music.youtube.com/playlist?list=LM",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("Could not run yt-dlp ({e}). Is it installed?"))?;
        if has_login_cookies(&tmp) {
            install_cookies(&tmp);
            return Ok(());
        }
        // Prefer a line that explains a cookie / keyring problem over the generic final error.
        let text = String::from_utf8_lossy(&out.stderr).to_string();
        let hint = text
            .lines()
            .rev()
            .find(|l| {
                let l = l.to_lowercase();
                l.contains("decrypt") || l.contains("keyring") || l.contains("wallet") || l.contains("cookie")
            })
            .map(|l| l.trim().to_string())
            .unwrap_or_else(|| last_line(&out.stderr));
        last_err = format!("{spec}: {hint}");
    }
    let _ = std::fs::remove_file(&tmp);
    Err(if why.is_empty() { last_err } else { format!("{why}; {last_err}") })
}

/// Reads the login out of rustym's private browser profile (see `open_login`) into `cookie_file()`.
fn check_login_isolated(browser: &str) -> Result<(), String> {
    let tmp = config_dir().join("cookies.tmp");
    let _ = std::fs::remove_file(&tmp);
    let prof = login_profile_dir();
    let res: Result<(), String> = if browser == "firefox" {
        firefox_export(&tmp, &[prof.clone()])
    } else {
        // The profile was started with --password-store=basic, so its cookies use Chromium's
        // fixed "peanuts" key and yt-dlp can read them without any keyring.
        let spec = format!("{browser}+basictext:{}", prof.display());
        match Command::new("yt-dlp")
            .args(["--cookies-from-browser", spec.as_str(), "--cookies"])
            .arg(&tmp)
            .args([
                "--flat-playlist",
                "--dump-json",
                "--no-warnings",
                "--playlist-end",
                "1",
                "--socket-timeout",
                "10",
                "https://music.youtube.com/playlist?list=LM",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
        {
            Ok(o) if tmp.exists() => {
                let _ = o;
                Ok(())
            }
            Ok(o) => Err(last_line(&o.stderr)),
            Err(e) => Err(format!("Could not run yt-dlp ({e}). Is it installed?")),
        }
    };
    match res {
        Ok(()) if has_login_cookies(&tmp) => {
            install_cookies(&tmp);
            Ok(())
        }
        Ok(()) => {
            let _ = std::fs::remove_file(&tmp);
            Err("not signed in to Google yet in the rustym sign-in window".into())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Re-reads the private login profile and swaps in the fresh cookies.
fn refresh_session() -> Result<(), String> {
    if !login_profile_dir().join("cookies.sqlite").exists() && !has_chromium_profile() {
        return Err("no login profile".into());
    }
    let b = Config::load().browser;
    ISOLATED.store(true, SeqCst);
    check_login_isolated(&b)
}

fn has_chromium_profile() -> bool {
    let p = login_profile_dir();
    p.join("Default").exists() || p.join("Local State").exists()
}

/// Asks YouTube Music itself whether the saved cookies are accepted (a signed-in library comes
/// back). Without this, "login detected" only meant "cookies exist", which is why sign-in used to
/// look fine while liked songs and playlists stayed empty.
pub fn verify_session() -> Result<(), String> {
    match api_call_once("verify", "") {
        Ok(_) => Ok(()),
        // offline: can't tell, don't block the login
        Err(e) if e.starts_with("network error") || e.contains("python3 not available") => Ok(()),
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

/// Polls every ~4s for up to ~6 minutes while the user signs in. Returns the browser that worked.
pub fn wait_for_login(preferred: &str, cancel: &AtomicBool, progress: &dyn Fn(String)) -> Result<String, String> {
    let mut last = String::from("no login found yet");
    for _ in 0..90 {
        let list = if ISOLATED.load(SeqCst) { vec![preferred.to_string()] } else { candidate_browsers(preferred) };
        for b in list {
            if cancel.load(SeqCst) {
                return Err("Cancelled.".into());
            }
            progress(format!("Checking {b}…"));
            match check_login(&b).and_then(|_| verify_session()) {
                Ok(()) => {
                    log(0, "login", format!("signed in via {b}"));
                    return Ok(b);
                }
                Err(e) => {
                    log(1, "login", format!("{b}: {e}"));
                    progress(format!("Not signed in yet — {e}"));
                    last = e;
                }
            }
        }
        for _ in 0..40 {
            if cancel.load(SeqCst) {
                return Err("Cancelled.".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    Err(last)
}

// ---------------- mpv ----------------

pub fn mpv_cmd(cmd: Value) -> Option<Value> {
    let mut s = UnixStream::connect(SOCK).ok()?;
    s.set_read_timeout(Some(Duration::from_millis(150))).ok()?;
    s.set_write_timeout(Some(Duration::from_millis(150))).ok()?;
    let req = json!({ "command": cmd, "request_id": 7 });
    writeln!(s, "{req}").ok()?;
    let mut r = BufReader::new(s);
    let mut line = String::new();
    for _ in 0..20 {
        line.clear();
        if r.read_line(&mut line).ok()? == 0 {
            return None;
        }
        if let Ok(v) = serde_json::from_str::<Value>(&line) {
            if v["request_id"] == 7 {
                return Some(v);
            }
        }
    }
    None
}

fn mpv_f64(prop: &str) -> Option<f64> {
    mpv_cmd(json!(["get_property", prop]))?["data"].as_f64()
}

fn mpv_bool(prop: &str) -> Option<bool> {
    mpv_cmd(json!(["get_property", prop]))?["data"].as_bool()
}

// ---------------- playback engine (own thread, keeps running when window is hidden) ----------------

pub enum Cmd {
    Play(Vec<Track>, usize),
    Toggle,
    Next,
    Prev,
    Seek(f64),
    Volume(f32),
    Quit,
}

#[derive(Clone, Default)]
pub struct Shared {
    pub current: Option<Track>,
    pub pos: f64,
    pub dur: f64,
    pub paused: bool,
    pub playing: bool,
    pub error: String,
}

struct Engine {
    queue: Vec<Track>,
    idx: usize,
    child: Option<Child>,
    started: Instant,
    volume: f32,
    shared: Arc<Mutex<Shared>>,
}

impl Engine {
    fn kill(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    fn play(&mut self, i: usize) {
        if i >= self.queue.len() {
            return;
        }
        self.kill();
        let _ = std::fs::remove_file(SOCK);
        let t = self.queue[i].clone();
        // mpv's own messages (mostly yt-dlp's reason when a track won't play) go to a log so
        // failures can be explained instead of silently doing nothing.
        let mpv_log_path = config_dir().join("mpv.log");
        let errf = std::fs::File::create(&mpv_log_path).map(Stdio::from).unwrap_or_else(|_| Stdio::null());
        let res = Command::new("mpv")
            .arg("--no-video")
            .arg("--force-window=no")
            .arg("--msg-level=all=error,ytdl_hook=info")
            .arg(format!("--ytdl-format={YTDL_FORMAT}"))
            .arg(format!("--input-ipc-server={SOCK}"))
            .arg(format!("--volume={}", self.volume as i32))
            .arg(format!("https://music.youtube.com/watch?v={}", t.id))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(errf)
            .spawn();
        let mut s = self.shared.lock().unwrap();
        match res {
            Ok(c) => {
                log(0, "mpv", format!("started: {} - {} (id {})", t.artist, t.title, t.id));
                self.child = Some(c);
                self.idx = i;
                self.started = Instant::now();
                s.current = Some(t.clone());
                s.pos = 0.0;
                s.dur = t.dur.unwrap_or(0.0);
                s.paused = false;
                s.playing = true;
                s.error.clear();
            }
            Err(e) => {
                log(2, "mpv", format!("could not start mpv: {e}"));
                s.error = format!("Could not start mpv ({e}). Is it installed?");
            }
        }
    }

    fn next(&mut self) {
        if self.idx + 1 < self.queue.len() {
            self.play(self.idx + 1);
        } else {
            self.kill();
            let mut s = self.shared.lock().unwrap();
            s.playing = false;
            s.pos = 0.0;
        }
    }

    fn prev(&mut self) {
        let pos = self.shared.lock().unwrap().pos;
        if pos > 4.0 || self.idx == 0 {
            self.play(self.idx);
        } else {
            self.play(self.idx - 1);
        }
    }

    fn toggle(&mut self) {
        if self.child.is_some() {
            let _ = mpv_cmd(json!(["cycle", "pause"]));
            let mut s = self.shared.lock().unwrap();
            s.paused = !s.paused;
        } else if !self.queue.is_empty() {
            self.play(self.idx);
        }
    }

    fn tick(&mut self) {
        let mut ended = false;
        let mut code = String::new();
        if let Some(c) = self.child.as_mut() {
            if let Ok(Some(st)) = c.try_wait() {
                ended = true;
                code = format!("{st}");
            }
        }
        if ended {
            self.child = None;
            if self.started.elapsed() < Duration::from_secs(4) {
                let hint = play_error_hint();
                log(
                    2,
                    "mpv",
                    format!("exited after {:.1}s ({code}). {hint}\n--- mpv output ---\n{}", self.started.elapsed().as_secs_f32(), mpv_log_tail(25)),
                );
                let mut s = self.shared.lock().unwrap();
                s.playing = false;
                s.error = hint;
            } else {
                self.next();
            }
            return;
        }
        if self.child.is_some() {
            let p = mpv_f64("time-pos");
            let d = mpv_f64("duration");
            let b = mpv_bool("pause");
            let mut s = self.shared.lock().unwrap();
            if let Some(p) = p {
                s.pos = p;
            }
            if let Some(d) = d {
                s.dur = d;
            }
            if let Some(b) = b {
                s.paused = b;
            }
        }
    }
}

/// Turns the tail of mpv.log into a message that says what actually went wrong.
fn play_error_hint() -> String {
    let txt = std::fs::read_to_string(config_dir().join("mpv.log")).unwrap_or_default();
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
    let hint = if low.contains("error while loading shared libraries") {
        "mpv is broken by a partial system upgrade: run sudo pacman -Syu (then reinstall mpv if needed)"
    } else if low.contains("javascript runtime") || low.contains("n challenge") || low.contains("ejs") || low.contains("signature solving") {
        "yt-dlp needs a JavaScript runtime: sudo pacman -S deno yt-dlp-ejs"
    } else if low.contains("confirm you") && low.contains("bot") {
        "YouTube wants a bot check: sign in again in Settings, or update yt-dlp"
    } else if low.contains("requested format is not available") {
        "No audio format came back: update yt-dlp (sudo pacman -Syu yt-dlp) and install deno yt-dlp-ejs"
    } else if low.contains("unable to download") || low.contains("name or service not known") {
        "Network problem reaching YouTube"
    } else if low.contains("ytdl_hook") && low.contains("not found") || low.contains("youtube-dl") && low.contains("not found") {
        "mpv can't find yt-dlp: sudo pacman -S yt-dlp"
    } else {
        "Couldn’t play that track. Update yt-dlp and install deno yt-dlp-ejs"
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
    let shared = Arc::new(Mutex::new(Shared::default()));
    let sh = shared.clone();
    std::thread::spawn(move || {
        let mut e = Engine {
            queue: vec![],
            idx: 0,
            child: None,
            started: Instant::now(),
            volume,
            shared: sh,
        };
        loop {
            match rx.recv_timeout(Duration::from_millis(400)) {
                Ok(Cmd::Play(q, i)) => {
                    e.queue = q;
                    e.play(i);
                }
                Ok(Cmd::Toggle) => e.toggle(),
                Ok(Cmd::Next) => e.next(),
                Ok(Cmd::Prev) => e.prev(),
                Ok(Cmd::Seek(t)) => {
                    let _ = mpv_cmd(json!(["seek", t, "absolute"]));
                    e.shared.lock().unwrap().pos = t;
                }
                Ok(Cmd::Volume(v)) => {
                    e.volume = v;
                    let _ = mpv_cmd(json!(["set_property", "volume", v as f64]));
                }
                Ok(Cmd::Quit) => {
                    e.kill();
                    cleanup();
                    std::process::exit(0);
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    e.kill();
                    break;
                }
            }
            e.tick();
            repaint();
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
        let fs = 22050.0f32;
        let mut buf = [0u8; N * 2];
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
            let samples: Vec<f32> = (0..N)
                .map(|i| i16::from_le_bytes([buf[2 * i], buf[2 * i + 1]]) as f32 / 32768.0 * win[i])
                .collect();
            let mut raw = vec![0.0f32; BANDS];
            let mut mx = 0.0f32;
            for (i, &f) in freqs.iter().enumerate() {
                let a = goertzel(&samples, f, fs);
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
            g.levels = raw;
            g.updated = Instant::now();
            g.live = true;
        }
        let _ = child.kill();
        d.lock().unwrap().live = false;
    });
    data
}

// ---------------- window hiding (Hyprland special workspace) + tray ----------------

pub struct WinCtl {
    pub hidden: AtomicBool,
    pub want_show: AtomicBool,
    pub want_hide: AtomicBool,
}

impl WinCtl {
    pub fn new() -> Self {
        WinCtl {
            hidden: AtomicBool::new(false),
            want_show: AtomicBool::new(false),
            want_hide: AtomicBool::new(false),
        }
    }
}

pub fn is_hypr() -> bool {
    std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok()
}

fn hyprctl(args: &[&str]) -> Option<String> {
    let o = Command::new("hyprctl").args(args).output().ok()?;
    Some(String::from_utf8_lossy(&o.stdout).to_string())
}

pub fn hide_window() {
    let _ = hyprctl(&["dispatch", "movetoworkspacesilent", "special:rustym,class:^(rustym)$"]);
}

pub fn show_window() {
    let id = hyprctl(&["activeworkspace", "-j"])
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v["id"].as_i64())
        .unwrap_or(1);
    let target = format!("{id},class:^(rustym)$");
    let _ = hyprctl(&["dispatch", "movetoworkspace", target.as_str()]);
    let _ = hyprctl(&["dispatch", "focuswindow", "class:^(rustym)$"]);
}

pub fn show_now(win: &WinCtl, repaint: &Repaint) {
    win.hidden.store(false, SeqCst);
    if is_hypr() {
        show_window();
    } else {
        win.want_show.store(true, SeqCst);
        repaint();
    }
}

pub fn toggle_window(win: &WinCtl, repaint: &Repaint) {
    if win.hidden.load(SeqCst) {
        show_now(win, repaint);
    } else {
        win.hidden.store(true, SeqCst);
        if is_hypr() {
            hide_window();
        } else {
            win.want_hide.store(true, SeqCst);
            repaint();
        }
    }
}

struct RustymTray {
    tx: Sender<Cmd>,
    win: Arc<WinCtl>,
    repaint: Repaint,
}

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
    fn activate(&mut self, _x: i32, _y: i32) {
        toggle_window(&self.win, &self.repaint);
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        vec![
            StandardItem {
                label: "Show / Hide".into(),
                activate: Box::new(|t: &mut RustymTray| toggle_window(&t.win, &t.repaint)),
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

pub fn spawn_tray(tx: Sender<Cmd>, win: Arc<WinCtl>, repaint: Repaint) {
    ksni::TrayService::new(RustymTray { tx, win, repaint }).spawn();
}

// ---------------- single instance ----------------

/// If another rustym is running, ask it to show its window and return true.
pub fn ping_existing() -> bool {
    if let Ok(mut s) = UnixStream::connect(LOCK) {
        let _ = s.write_all(b"show\n");
        return true;
    }
    false
}

pub fn listen_instance(win: Arc<WinCtl>, repaint: Repaint) {
    let _ = std::fs::remove_file(LOCK);
    let Ok(l) = UnixListener::bind(LOCK) else { return };
    std::thread::spawn(move || {
        for s in l.incoming() {
            if s.is_ok() {
                show_now(&win, &repaint);
            }
        }
    });
}
