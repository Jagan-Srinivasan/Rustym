//! Media keys and "now playing" on the desktop (MPRIS over D-Bus), via souvlaki.

use crate::engine::{Cmd, Shared, WinCtl};
use std::sync::{mpsc::Sender, Arc, Mutex};

#[cfg(feature = "mpris")]
pub fn spawn(tx: Sender<Cmd>, shared: Arc<Mutex<Shared>>, win: Arc<WinCtl>) {
    use crate::engine::{log, show_now};
    use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPosition, PlatformConfig};
    use std::time::Duration;

    std::thread::spawn(move || {
        let cfg = PlatformConfig { dbus_name: "rustym", display_name: "Rustym", hwnd: None };
        let mut mc = match MediaControls::new(cfg) {
            Ok(m) => m,
            Err(e) => {
                log(1, "mpris", format!("media keys unavailable: {e:?}"));
                return;
            }
        };
        let tx2 = tx.clone();
        let sh2 = shared.clone();
        let attached = mc.attach(move |ev: MediaControlEvent| match ev {
            MediaControlEvent::Toggle => {
                let _ = tx2.send(Cmd::Toggle);
            }
            MediaControlEvent::Play => {
                let paused = sh2.lock().map(|s| s.paused || !s.playing).unwrap_or(true);
                if paused {
                    let _ = tx2.send(Cmd::Toggle);
                }
            }
            MediaControlEvent::Pause => {
                let playing = sh2.lock().map(|s| s.playing && !s.paused).unwrap_or(false);
                if playing {
                    let _ = tx2.send(Cmd::Toggle);
                }
            }
            MediaControlEvent::Next => {
                let _ = tx2.send(Cmd::Next);
            }
            MediaControlEvent::Previous => {
                let _ = tx2.send(Cmd::Prev);
            }
            MediaControlEvent::SetPosition(MediaPosition(d)) => {
                let _ = tx2.send(Cmd::Seek(d.as_secs_f64()));
            }
            MediaControlEvent::Raise => {
                show_now(&win);
            }
            _ => {}
        });
        if let Err(e) = attached {
            log(1, "mpris", format!("could not attach media-key handler: {e:?}"));
            return;
        }
        let mut last_id = String::new();
        let mut last_state = (false, false);
        loop {
            std::thread::sleep(Duration::from_millis(700));
            let s = match shared.lock() {
                Ok(s) => s.clone(),
                Err(_) => continue,
            };
            let id = s.current.as_ref().map(|t| t.id.clone()).unwrap_or_default();
            if id != last_id {
                last_id = id.clone();
                if let Some(t) = s.current.as_ref() {
                    let cover = if t.local.is_empty() { Some(format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", t.id)) } else { None };
                    let _ = mc.set_metadata(MediaMetadata {
                        title: Some(t.title.as_str()),
                        artist: Some(t.artist.as_str()),
                        album: if t.album.is_empty() { None } else { Some(t.album.as_str()) },
                        cover_url: cover.as_deref(),
                        duration: t.dur.map(Duration::from_secs_f64),
                    });
                }
                last_state = (s.playing, s.paused);
                let _ = mc.set_playback(playback(&s));
            } else if last_state != (s.playing, s.paused) {
                last_state = (s.playing, s.paused);
                let _ = mc.set_playback(playback(&s));
            }
        }
    });
}

#[cfg(feature = "mpris")]
fn playback(s: &Shared) -> souvlaki::MediaPlayback {
    use souvlaki::{MediaPlayback, MediaPosition};
    use std::time::Duration;
    if !s.playing {
        return MediaPlayback::Stopped;
    }
    let progress = Some(MediaPosition(Duration::from_secs_f64(s.pos.max(0.0))));
    if s.paused {
        MediaPlayback::Paused { progress }
    } else {
        MediaPlayback::Playing { progress }
    }
}

#[cfg(not(feature = "mpris"))]
pub fn spawn(_tx: Sender<Cmd>, _shared: Arc<Mutex<Shared>>, _win: Arc<WinCtl>) {}
