//! Rustym's in-app Google sign-in window (no external browser).
//!
//! A small WebKitGTK window opens Google's sign-in page. Once the Google login cookies exist it
//! writes them to the path given as the first argument (Netscape cookies.txt, readable by yt-dlp
//! and Rustym). The window stays open until Rustym has verified the login and closes it. The window's own data lives in a temporary folder that is deleted on exit.
//!
//! Build: cargo build --release --features webview-login   (needs webkit2gtk-4.1 and gtk3)

use std::{collections::HashSet, fmt::Write as _, time::{Duration, Instant}};
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    platform::unix::WindowExtUnix,
    window::WindowBuilder,
};
use wry::{WebContext, WebViewBuilder, WebViewBuilderExtUnix};

const URL: &str = "https://accounts.google.com/ServiceLogin?service=youtube&continue=https%3A%2F%2Fmusic.youtube.com";
// Google refuses sign-in from unknown embedded browsers; present a normal desktop Firefox.
const UA: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0";

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: rustym-login <cookies-output-file>");
        std::process::exit(2);
    });
    let data = std::env::temp_dir().join(format!("rustym-login-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&data);

    let event_loop = EventLoopBuilder::new().build();
    let window = WindowBuilder::new()
        .with_title("Rustym - sign in to Google")
        .with_inner_size(tao::dpi::LogicalSize::new(520.0, 740.0))
        .build(&event_loop)
        .expect("window");
    let mut ctx = WebContext::new(Some(data.clone()));
    let vbox = window.default_vbox().expect("gtk vbox");
    let webview = WebViewBuilder::with_web_context(&mut ctx)
        .with_user_agent(UA)
        .with_url(URL)
        .build_gtk(vbox)
        .expect("webview");

    let mut next = Instant::now() + Duration::from_secs(2);
    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::WaitUntil(next);
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                let _ = std::fs::remove_dir_all(&data);
                *flow = ControlFlow::Exit;
            }
            Event::MainEventsCleared if Instant::now() >= next => {
                next = Instant::now() + Duration::from_secs(2);
                let mut seen = HashSet::new();
                let mut text = String::from("# Netscape HTTP Cookie File\n");
                let mut signed_in = false;
                for u in [
                    "https://accounts.google.com",
                    "https://www.google.com",
                    "https://www.youtube.com",
                    "https://music.youtube.com",
                ] {
                    let Ok(list) = webview.cookies_for_url(u) else { continue };
                    for c in list {
                        let dom = c.domain().unwrap_or("").to_string();
                        if !(dom.contains("google.com") || dom.contains("youtube.com")) {
                            continue;
                        }
                        if !seen.insert((dom.clone(), c.path().unwrap_or("/").to_string(), c.name().to_string())) {
                            continue;
                        }
                        if matches!(c.name(), "SAPISID" | "__Secure-1PAPISID" | "__Secure-3PAPISID") {
                            signed_in = true;
                        }
                        let exp = c.expires_datetime().map(|t| t.unix_timestamp()).unwrap_or(0);
                        let sub = if dom.starts_with('.') { "TRUE" } else { "FALSE" };
                        let _ = writeln!(
                            text,
                            "{dom}\t{sub}\t{}\t{}\t{exp}\t{}\t{}",
                            c.path().unwrap_or("/"),
                            if c.secure().unwrap_or(false) { "TRUE" } else { "FALSE" },
                            c.name(),
                            c.value()
                        );
                    }
                }
                if signed_in {
                    // Keep the window open: Rustym closes it itself once it has checked that
                    // YouTube accepts this login. Until then keep the cookie file fresh.
                    let _ = std::fs::write(&out, text);
                    let _ = window.set_title("Rustym - signed in, finishing up…");
                }
            }
            _ => {}
        }
    });
}
