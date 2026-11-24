//! v0.4 UI: native search with suggestions, album / artist / playlist pages, queue panel,
//! synced lyrics, sound effects + sleep timer, history, stats, local files, shortcuts.
//! (Child module of main.rs, so it can use the app's private types and widgets.)

use super::*;

fn fmt_hms(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    let (h, m) = (s / 3600, (s % 3600) / 60);
    if h > 0 {
        format!("{h} h {m} min")
    } else {
        format!("{m} min")
    }
}

/// Artwork with a soft accent glow ring behind it.
fn glow_art(ui: &mut egui::Ui, th: Theme, id: &str, art: Rect) {
    let p = ui.painter();
    p.rect_filled(art.expand(6.0), 22.0, th.accent.gamma_multiply(0.14));
    p.rect_filled(art.expand(2.0), 20.0, th.accent.gamma_multiply(0.28));
    thumb(ui, id, art, 18.0);
}

/// Big rounded accent pill for Play / Pause (the hero action button).
pub(crate) fn play_pill(th: Theme, label: &str) -> egui::Button<'static> {
    let fg = if th.light { Color32::WHITE } else { Color32::from_rgb(20, 15, 11) };
    egui::Button::new(RichText::new(label.to_string()).size(14.5).strong().color(fg))
        .fill(th.accent)
        .rounding(15.0)
}

/// Small outlined circle button (heart / lyrics / visualiser / queue).
fn ring_btn(ui: &mut egui::Ui, th: Theme, glyph: &str, active: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::click());
    let k = ui.ctx().animate_bool(resp.id, resp.hovered());
    let p = ui.painter();
    let c = rect.center();
    if active {
        p.circle_filled(c, 19.0, th.accent.gamma_multiply(0.14));
    } else if k > 0.01 {
        p.circle_filled(c, 19.0, th.accent.gamma_multiply(0.07 * k));
    }
    p.circle_stroke(c, 19.5, Stroke::new(1.4_f32, if active { th.accent } else { th.dim2.gamma_multiply(0.7 + 0.3 * k) }));
    p.text(c, Align2::CENTER_CENTER, glyph, FontId::proportional(15.0), if active { th.accent } else { lerp_col(th.dim2, th.text, k) });
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// A vinyl record with grooves, a slowly rotating sheen while playing, and a
/// mini equaliser underneath fed by the live audio levels.
fn vinyl(ui: &mut egui::Ui, th: Theme, size: f32, playing: bool, levels: &[f32]) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(size, size + 64.0), Sense::hover());
    // Clip everything to the allocated rect so the record can never spill
    // outside the Now Playing card.
    let p = ui.painter_at(rect);
    let c = Pos2::new(rect.center().x, rect.top() + size / 2.0);
    let r = size / 2.0;
    // Record body + grooves
    p.circle_filled(c, r, Color32::from_rgb(13, 11, 9));
    for i in 1..7 {
        let rr = r * (0.28 + 0.68 * i as f32 / 7.0);
        p.circle_stroke(c, rr, Stroke::new(1.0_f32, Color32::from_white_alpha(9)));
    }
    p.circle_stroke(c, r - 1.0, Stroke::new(1.2_f32, th.accent.gamma_multiply(0.10)));
    // Sheen wedge rotating while playing
    let t = ui.ctx().input(|i| i.time) as f32;
    let a0 = if playing { t * 0.9 } else { 0.6 };
    for k in 0..10 {
        let a = a0 + k as f32 * 0.05;
        let (s, co) = a.sin_cos();
        let r1 = r * 0.34;
        let r2 = r * (0.88 + 0.03 * (k % 3) as f32);
        let fade = (1.0 - k as f32 / 10.0) * 0.10;
        p.line_segment(
            [Pos2::new(c.x + co * r1, c.y + s * r1), Pos2::new(c.x + co * r2, c.y + s * r2)],
            Stroke::new(2.0_f32, Color32::from_white_alpha((fade * 255.0) as u8)),
        );
    }
    // Centre label in the accent colour, with a spindle hole.
    p.circle_filled(c, r * 0.26, th.accent);
    p.circle_stroke(c, r * 0.26, Stroke::new(1.0_f32, Color32::from_black_alpha(32)));
    p.circle_filled(c, r * 0.035, Color32::from_rgb(13, 11, 9));

    // Mini equaliser underneath.
    let n = 32;
    let gap = 3.0;
    let bw = ((rect.width() - gap * (n as f32 - 1.0)) / n as f32).max(1.0);
    let base = rect.bottom() - 30.0;
    for i in 0..n {
        let v = levels.get(i * BANDS / n).copied().unwrap_or(0.05);
        let h = (v * 34.0).max(2.5);
        let x = rect.left() + i as f32 * (bw + gap);
        let rr = Rect::from_min_max(Pos2::new(x, base - h), Pos2::new(x + bw, base));
        p.rect_filled(rr, bw / 2.0, th.accent.gamma_multiply(0.35 + 0.6 * v));
    }
}

/// Album / artist / playlist / song tile with a square picture.
pub fn media_card(ui: &mut egui::Ui, th: Theme, c: &ytm::Card, w: f32) -> egui::Response {
    let h = w + 50.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
    let k = ui.ctx().animate_bool(resp.id, resp.hovered());
    let p = ui.painter();
    p.rect_filled(rect, 12.0, lerp_col(th.card, th.card2, k));
    let img = Rect::from_min_size(rect.min + Vec2::splat(8.0), Vec2::splat(w - 16.0));
    let rad = if c.kind == ytm::Kind::Artist { (w - 16.0) / 2.0 } else { 10.0 };
    if c.thumb.is_empty() {
        p.rect_filled(img, rad, th.card2);
    } else {
        // centre-crop to fill the square tile instead of stretching wide / tall pictures
        cover_image(ui, &c.thumb, img, rad);
    }
    let p = ui.painter();
    if c.kind != ytm::Kind::Artist {
        play_bubble(p, th, img.right_bottom() + Vec2::new(-26.0, -26.0 + 6.0 * (1.0 - k)), k);
    }
    let tw = w - 20.0;
    ellipsis_text(p, Pos2::new(rect.left() + 10.0, img.bottom() + 16.0), &c.title, FontId::proportional(13.5), th.text, tw);
    ellipsis_text(p, Pos2::new(rect.left() + 10.0, img.bottom() + 34.0), &c.sub, FontId::proportional(12.0), th.dim, tw);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

impl Rustym {
    // ---------------- small helpers ----------------

    /// Runs `f` on a worker thread and posts its message back to the UI.
    pub(crate) fn bg(&self, ctx: &egui::Context, f: impl FnOnce() -> Msg + Send + 'static) {
        let tx = self.msg_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let m = f();
            let _ = tx.send(m);
            ctx.request_repaint();
        });
    }

    /// Same, without a context (the UI repaints a few times a second anyway).
    fn bgn(&self, f: impl FnOnce() -> Msg + Send + 'static) {
        let tx = self.msg_tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
    }

    pub(crate) fn pill(&self, ui: &mut egui::Ui, text: &str, active: bool) -> egui::Response {
        let th = self.theme;
        let col = if active { th.accent } else { th.dim2 };
        let mut b = egui::Button::new(RichText::new(text).size(12.0).color(col)).rounding(12.0);
        b = if active { b.fill(th.accent.gamma_multiply(0.16)) } else { b.fill(th.card) };
        let r = ui.add(b);
        if r.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        r
    }

    pub(crate) fn yt_removable(&self) -> bool {
        matches!(self.view, View::Remote(_)) && self.cfg.signed_in
    }

    // ---------------- background fetches ----------------

    pub(crate) fn fetch_home(&mut self, ctx: &egui::Context) {
        if self.home_loading {
            return;
        }
        self.home_loading = true;
        self.bg(ctx, || match ytm::home() {
            Ok(v) => Msg::Home(v),
            Err(e) => Msg::HomeErr(e),
        });
    }

    /// Loads the Google account's name and picture for the sidebar, home page and settings.
    pub(crate) fn fetch_account(&mut self, ctx: &egui::Context) {
        if !self.cfg.signed_in {
            return;
        }
        self.bg(ctx, || match ytm::account() {
            Ok((n, ph)) => Msg::Account(n, ph),
            Err(e) => {
                log(1, "account", &e);
                Msg::Account(String::new(), String::new())
            }
        });
    }

    pub(crate) fn fetch_liked_ids(&mut self, ctx: &egui::Context) {
        if !self.cfg.signed_in {
            return;
        }
        self.bg(ctx, || match ytm::liked() {
            Ok(v) => Msg::LikedIds(v.into_iter().map(|t| t.id).collect()),
            Err(e) => {
                log(1, "likes", &e);
                // offline / failed: fall back to the saved copy instead of forgetting every like
                Msg::LikedIds(liked_cache_load().into_iter().map(|t| t.id).collect())
            }
        });
    }

    pub(crate) fn fetch_cards(&mut self, ctx: &egui::Context, k: u8) {
        if !self.cfg.signed_in {
            return;
        }
        self.lib_loading = true;
        self.lib_err.clear();
        self.bg(ctx, move || {
            let r = if k == 0 { ytm::library_albums() } else { ytm::library_artists() };
            match r {
                Ok(v) => Msg::Cards(k, v),
                Err(e) => Msg::CardsErr(k, e),
            }
        });
    }

    pub(crate) fn fetch_history(&mut self, ctx: &egui::Context) {
        if !self.cfg.signed_in {
            self.hist_local = true;
            return;
        }
        self.hist_loading = true;
        self.hist_err.clear();
        self.bg(ctx, || match ytm::history() {
            Ok(v) => Msg::History(v),
            Err(e) => Msg::HistoryErr(e),
        });
    }

    pub(crate) fn scan_local(&mut self, ctx: &egui::Context) {
        self.loading = true;
        let dirs = self.cfg.local_dirs.clone();
        self.bg(ctx, move || Msg::Local(local::scan(&dirs)));
    }

    pub(crate) fn fetch_lyrics(&mut self, ctx: &egui::Context) {
        let Some(t) = self.snap.current.clone() else { return };
        self.lyrics_for = t.id.clone();
        self.lyrics = None;
        self.lyrics_err.clear();
        self.lyrics_loading = true;
        self.lyric_cur = None;
        self.bg(ctx, move || match lyrics::fetch(&t) {
            Ok(l) => Msg::Lyrics(t.id.clone(), l),
            Err(e) => Msg::LyricsErr(t.id.clone(), e),
        });
    }

    /// Songs for a search phrase (native first, yt-dlp as a fallback). Used by the mixes.
    pub(crate) fn start_query(&mut self, ctx: &egui::Context, label: String, query: String) {
        self.loading = true;
        self.status.clear();
        self.label = label;
        self.req += 1;
        let id = self.req;
        self.bg(ctx, move || {
            let r = ytm::search(&query, 1).map(|o| o.songs).or_else(|e1| {
                let args = vec![
                    "--flat-playlist".to_string(),
                    "--dump-json".to_string(),
                    "--no-warnings".to_string(),
                    format!("ytsearch30:{query}"),
                ];
                run_ytdlp(args).map_err(|e2| format!("{e1} | yt-dlp: {e2}"))
            });
            match r {
                Ok(t) => Msg::Results(id, t),
                Err(e) => Msg::Error(id, e),
            }
        });
    }

    pub(crate) fn run_search(&mut self, ctx: &egui::Context) {
        let q = self.query.trim().to_string();
        if q.is_empty() {
            return;
        }
        self.show_sug = false;
        self.suggestions.clear();
        self.sug_dirty = false;
        match ytm::parse_link(&q) {
            Some(ytm::Link::Playlist(id)) => return self.open_detail(ctx, ytm::Kind::Playlist, id),
            Some(ytm::Link::Artist(id)) => return self.open_detail(ctx, ytm::Kind::Artist, id),
            Some(ytm::Link::Video(id)) => {
                self.start_radio(&id);
                return;
            }
            None => {}
        }
        self.loading = true;
        self.status.clear();
        self.label = format!("Results for “{q}”");
        // Remember the query for the recent-searches chips.
        if !self.cfg.recent_queries.iter().any(|s| s == &q) {
            self.cfg.recent_queries.insert(0, q.clone());
            self.cfg.recent_queries.truncate(8);
            self.cfg.save();
        }
        self.req += 1;
        let id = self.req;
        let filter = self.sfilter;
        self.sres = ytm::SearchOut::default();
        self.tracks.clear();
        self.bg(ctx, move || {
            let r = ytm::search(&q, filter).or_else(|e1| {
                let args = vec![
                    "--flat-playlist".to_string(),
                    "--dump-json".to_string(),
                    "--no-warnings".to_string(),
                    format!("ytsearch30:{q}"),
                ];
                run_ytdlp(args)
                    .map(|t| ytm::SearchOut { songs: t, ..Default::default() })
                    .map_err(|e2| format!("{e1} | yt-dlp: {e2}"))
            });
            match r {
                Ok(o) => Msg::SearchRes(id, o),
                Err(e) => Msg::Error(id, e),
            }
        });
    }

    pub(crate) fn open_detail(&mut self, ctx: &egui::Context, kind: ytm::Kind, id: String) {
        if self.view != View::Detail {
            self.back.push(self.view);
        }
        self.view = View::Detail;
        self.detail = None;
        self.detail_kind = kind;
        self.detail_id = id.clone();
        self.tracks.clear();
        self.status.clear();
        self.loading = true;
        self.label = "Loading…".into();
        self.req += 1;
        let rid = self.req;
        self.bg(ctx, move || {
            let r = match kind {
                ytm::Kind::Artist => ytm::artist(&id),
                ytm::Kind::Album => ytm::album(&id),
                _ => ytm::playlist(&id),
            };
            match r {
                Ok(d) => Msg::Detail(rid, d),
                Err(e) => Msg::DetailErr(rid, e),
            }
        });
    }

    pub(crate) fn open_card(&mut self, ctx: &egui::Context, c: ytm::Card) {
        match c.kind {
            ytm::Kind::Song => {
                if let Some(t) = c.to_track() {
                    self.record_recent(t.clone());
                    let _ = self.cmd.send(Cmd::Play(vec![t], 0));
                }
            }
            k => self.open_detail(ctx, k, c.id),
        }
    }

    pub(crate) fn start_radio(&mut self, id: &str) {
        let id = id.to_string();
        self.toast = Some(("Starting radio…".into(), Instant::now()));
        self.bgn(move || match ytm::radio(&id) {
            Ok(v) => Msg::PlayNow(v),
            Err(e) => Msg::Toast(format!("Radio failed: {e}")),
        });
    }

    pub(crate) fn toggle_like(&mut self, t: &Track) {
        if !self.cfg.signed_in {
            self.toast = Some(("Sign in to like songs".into(), Instant::now()));
            return;
        }
        let like = !self.liked_ids.contains(&t.id);
        if like {
            self.liked_ids.insert(t.id.clone());
        } else {
            self.liked_ids.remove(&t.id);
        }
        let id = t.id.clone();
        let tr = t.clone();
        self.bgn(move || match ytm::rate(&id, like) {
            Ok(()) => {
                liked_cache_toggle(&tr, like);
                Msg::Toast(if like { "Added to Liked songs".into() } else { "Removed from Liked songs".into() })
            }
            Err(e) => Msg::Toast(format!("Couldn’t update like: {e}")),
        });
    }

    pub(crate) fn remove_from_yt(&mut self, i: usize) {
        let View::Remote(r) = self.view else { return };
        let Some(pl) = self.remote.get(r).cloned() else { return };
        let Some(t) = self.tracks.get(i).cloned() else { return };
        self.tracks.remove(i);
        self.remote_cache.insert(pl.id.clone(), self.tracks.clone());
        self.bgn(move || match ytm::playlist_remove(&pl.id, &t.id, &t.set_id) {
            Ok(()) => Msg::Toast("Removed from playlist".into()),
            Err(e) => Msg::Toast(e),
        });
    }

    pub(crate) fn add_to_yt(&mut self, pl: usize, t: Track) {
        let Some(p) = self.remote.get(pl).cloned() else { return };
        self.remote_cache.remove(&p.id);
        self.bgn(move || match ytm::playlist_add(&p.id, &[t.id.clone()]) {
            Ok(()) => Msg::Toast(format!("Added to {}", p.title)),
            Err(e) => Msg::Toast(format!("Couldn’t add: {e}")),
        });
    }

    pub(crate) fn create_yt_playlist(&mut self, name: String, ids: Vec<String>) {
        let tx = self.msg_tx.clone();
        std::thread::spawn(move || match ytm::create_playlist(&name, &ids) {
            Ok(_) => {
                let _ = tx.send(Msg::Toast(format!("Created “{name}” on YouTube Music")));
                let _ = tx.send(Msg::LibraryChanged);
            }
            Err(e) => {
                let _ = tx.send(Msg::Toast(format!("Couldn’t create playlist: {e}")));
            }
        });
    }

    pub(crate) fn export_playlists(&mut self) {
        let path = config_dir().join("playlists-export.json");
        let ok = serde_json::to_string_pretty(&self.cfg.playlists)
            .ok()
            .map_or(false, |s| std::fs::write(&path, s).is_ok());
        let msg = if ok { format!("Exported to {}", path.display()) } else { "Export failed".to_string() };
        self.toast = Some((msg, Instant::now()));
    }

    pub(crate) fn import_playlists(&mut self) {
        let path = config_dir().join("playlists-export.json");
        let msg = match std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str::<Vec<Playlist>>(&s).ok()) {
            Some(list) => {
                let n = list.len();
                for p in list {
                    if let Some(ex) = self.cfg.playlists.iter_mut().find(|x| x.name == p.name) {
                        for t in p.tracks {
                            if !ex.tracks.iter().any(|x| x.id == t.id) {
                                ex.tracks.push(t);
                            }
                        }
                    } else {
                        self.cfg.playlists.push(p);
                    }
                }
                self.cfg.save();
                format!("Imported {n} playlists")
            }
            None => format!("Nothing to import at {}", path.display()),
        };
        self.toast = Some((msg, Instant::now()));
    }

    // ---------------- per-frame housekeeping ----------------

    pub(crate) fn housekeeping(&mut self, ctx: &egui::Context) {
        if let Some((k, id)) = self.open_detail_later.take() {
            self.open_detail(ctx, k, id);
        }
        let cur = self.snap.current.as_ref().map(|t| t.id.clone()).unwrap_or_default();
        // Once enabled, lyrics stay enabled as the queue advances to each new song.
        if self.view == View::Now && self.lyrics_enabled && !cur.is_empty() && cur != self.lyrics_for {
            self.fetch_lyrics(ctx);
        }
        // The background process counts plays, so history / stats are re-read while they are open.
        if matches!(self.view, View::History | View::Stats) && self.store_t.elapsed() > Duration::from_secs(3) {
            store::reload();
            self.store_t = Instant::now();
        }
    }

    pub(crate) fn shortcuts(&mut self, ctx: &egui::Context) {
        use egui::Key;
        let typing = ctx.wants_keyboard_input();
        let k = ctx.input(|i| {
            let plain = !i.modifiers.command && !i.modifiers.alt;
            [
                i.modifiers.command && i.key_pressed(Key::K),
                (i.modifiers.command && i.key_pressed(Key::H)) || i.key_pressed(Key::F1),
                plain && i.key_pressed(Key::Space),
                plain && i.key_pressed(Key::ArrowLeft),
                plain && i.key_pressed(Key::ArrowRight),
                i.modifiers.command && i.key_pressed(Key::ArrowLeft),
                i.modifiers.command && i.key_pressed(Key::ArrowRight),
                plain && i.key_pressed(Key::ArrowUp),
                plain && i.key_pressed(Key::ArrowDown),
                plain && i.key_pressed(Key::Q),
                plain && i.key_pressed(Key::L),
                plain && i.key_pressed(Key::S),
                plain && i.key_pressed(Key::R),
                plain && i.key_pressed(Key::Y),
                plain && i.key_pressed(Key::F),
            ]
        });
        if k[0] {
            self.back.clear();
            self.go(ctx, View::Search);
        }
        if k[1] {
            self.help_open = !self.help_open;
        }
        if typing {
            return;
        }
        if k[2] {
            let _ = self.cmd.send(Cmd::Toggle);
        }
        if k[5] {
            let _ = self.cmd.send(Cmd::Prev);
        } else if k[3] {
            let _ = self.cmd.send(Cmd::SeekBy(-5.0));
        }
        if k[6] {
            let _ = self.cmd.send(Cmd::Next);
        } else if k[4] {
            let _ = self.cmd.send(Cmd::SeekBy(5.0));
        }
        if k[7] || k[8] {
            let d = if k[7] { 5.0 } else { -5.0 };
            self.cfg.volume = (self.cfg.volume + d).clamp(0.0, 100.0);
            let _ = self.cmd.send(Cmd::Volume(self.cfg.volume));
            self.cfg.save();
        }
        if k[9] {
            self.show_queue = !self.show_queue;
        }
        if k[10] {
            if let Some(t) = self.snap.current.clone() {
                if t.local.is_empty() {
                    self.toggle_like(&t);
                }
            }
        }
        if k[11] {
            self.cfg.shuffle = !self.snap.shuffle;
            let _ = self.cmd.send(Cmd::Shuffle(self.cfg.shuffle));
            self.cfg.save();
        }
        if k[12] {
            let r = match self.snap.repeat {
                Repeat::Off => Repeat::All,
                Repeat::All => Repeat::One,
                Repeat::One => Repeat::Off,
            };
            self.cfg.repeat = r;
            let _ = self.cmd.send(Cmd::SetRepeat(r));
            self.cfg.save();
        }
        if k[13] {
            let v = if self.view == View::Now { View::Home } else { View::Now };
            self.go(ctx, v);
        }
        if k[14] {
            self.fx_open = !self.fx_open;
        }
    }

    // ---------------- overlays ----------------

    pub(crate) fn toast_draw(&mut self, ctx: &egui::Context) {
        let expired = self.toast.as_ref().map_or(false, |(_, t)| t.elapsed() > Duration::from_millis(3500));
        if expired {
            self.toast = None;
        }
        if let Some((msg, _)) = &self.toast {
            egui::Area::new(egui::Id::new("toast"))
                .anchor(Align2::CENTER_BOTTOM, [0.0, -108.0])
                .interactable(false)
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.label(RichText::new(msg).size(14.0));
                    });
                });
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }

    pub(crate) fn help_window(&mut self, ctx: &egui::Context) {
        let mut open = self.help_open;
        egui::Window::new("Keyboard shortcuts").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            for (k, d) in [
                ("Space", "Play / pause"),
                ("← / →", "Seek 5 seconds"),
                ("Ctrl + ← / →", "Previous / next song"),
                ("↑ / ↓", "Volume"),
                ("Ctrl + K", "Search"),
                ("Q", "Queue panel"),
                ("Y", "Lyrics (Now playing)"),
                ("F", "Sound: tempo, pitch, equaliser, sleep timer"),
                ("L", "Like / unlike the current song"),
                ("S", "Shuffle"),
                ("R", "Repeat: off / all / one"),
                ("Ctrl + H  or  F1", "This help"),
                ("Right-click a song", "Play next, queue, like, radio, artist, album"),
            ] {
                ui.horizontal(|ui| {
                    ui.add_sized([150.0, 20.0], egui::Label::new(RichText::new(k).strong()));
                    ui.label(d);
                });
            }
        });
        self.help_open = open;
    }

    pub(crate) fn fx_window(&mut self, ctx: &egui::Context) {
        if !self.fx_open {
            return;
        }
        let th = self.theme;
        let mut open = true;
        let mut fx = self.cfg.fx.clone();
        let mut sleep: Option<Option<u64>> = None;
        let mut sleep_eot: Option<bool> = None;
        egui::Window::new("Sound").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.set_min_width(420.0);
            ui.horizontal(|ui| {
                ui.label("Tempo");
                ui.add(egui::Slider::new(&mut fx.speed, 0.5..=2.0).step_by(0.05).suffix("×"));
            });
            ui.horizontal(|ui| {
                ui.label("Pitch");
                ui.add(egui::Slider::new(&mut fx.pitch, -12.0..=12.0).step_by(0.5).suffix(" st"));
            });
            ui.horizontal(|ui| {
                ui.label("Reverb");
                for (i, n) in REVERBS.iter().enumerate() {
                    ui.selectable_value(&mut fx.reverb, i as u8, *n);
                }
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut fx.eight_d, "8D audio (sound circles around your head - use headphones)");
            });
            if fx.eight_d {
                ui.horizontal(|ui| {
                    ui.label("8D speed");
                    ui.add(egui::Slider::new(&mut fx.eight_d_hz, 0.05..=0.5).step_by(0.01).suffix(" Hz"));
                });
            }
            ui.checkbox(&mut fx.normalize, "Loudness normalisation (same volume for every song)");
            ui.separator();
            ui.checkbox(&mut fx.eq_on, "Equaliser");
            ui.horizontal(|ui| {
                for (name, v) in [
                    ("Flat", [0.0f32; 10]),
                    ("Bass boost", [6.0, 5.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
                    ("Vocal", [-2.0, -2.0, -1.0, 1.0, 3.0, 4.0, 3.0, 1.0, 0.0, -1.0]),
                    ("Treble", [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 4.0, 5.0, 6.0]),
                    ("Rock", [5.0, 4.0, 2.0, -1.0, -2.0, -1.0, 2.0, 4.0, 5.0, 5.0]),
                ] {
                    if ui.button(name).clicked() {
                        fx.eq = v;
                        fx.eq_on = name != "Flat";
                    }
                }
            });
            ui.horizontal(|ui| {
                let labels = ["31", "62", "125", "250", "500", "1k", "2k", "4k", "8k", "16k"];
                for i in 0..10 {
                    ui.vertical(|ui| {
                        ui.add(egui::Slider::new(&mut fx.eq[i], -12.0..=12.0).vertical().show_value(false));
                        ui.label(RichText::new(labels[i]).size(11.0).color(th.dim));
                    });
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Slowed + reverb").clicked() {
                    fx.speed = 0.85;
                    fx.reverb = 2;
                }
                if ui.button("Reset all").clicked() {
                    fx = Fx::default();
                }
            });
            ui.separator();
            ui.label(RichText::new("Sleep timer").strong());
            if let Some(left) = self.snap.sleep_left {
                ui.label(format!("Pausing in {} min {:02} s", left / 60, left % 60));
            } else if self.snap.sleep_eot {
                ui.label("Stopping at the end of this song");
            }
            ui.horizontal_wrapped(|ui| {
                for m in [15u64, 30, 45, 60, 90] {
                    if ui.button(format!("{m} min")).clicked() {
                        sleep = Some(Some(m));
                    }
                }
                if ui.button("End of song").clicked() {
                    sleep_eot = Some(true);
                }
                if ui.button("Off").clicked() {
                    sleep = Some(None);
                    sleep_eot = Some(false);
                }
            });
        });
        if fx != self.cfg.fx {
            self.cfg.fx = fx.clone();
            let _ = self.cmd.send(Cmd::SetFx(fx));
            self.cfg.save();
        }
        if let Some(s) = sleep {
            let _ = self.cmd.send(Cmd::Sleep(s));
        }
        if let Some(b) = sleep_eot {
            let _ = self.cmd.send(Cmd::SleepEndOfTrack(b));
        }
        self.fx_open = open;
    }

    pub(crate) fn queue_panel(&mut self, ctx: &egui::Context) {
        if !self.show_queue {
            return;
        }
        let th = self.theme;
        let mut jump: Option<usize> = None;
        let mut remove: Option<usize> = None;
        let mut mv: Option<(usize, usize)> = None;
        let mut clear = false;
        let mut save = false;
        let q = self.snap.queue.clone();
        let idx = self.snap.idx;
        egui::SidePanel::right("queue")
            .exact_width(330.0)
            .resizable(false)
            .frame(egui::Frame::none().fill(th.panel).inner_margin(Margin::symmetric(14.0, 16.0)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Queue").size(20.0).strong().color(th.text));
                    ui.label(RichText::new(format!("{} songs", q.len())).size(12.0).color(th.dim));
                });
                ui.horizontal(|ui| {
                    if ui.small_button("Clear upcoming").clicked() {
                        clear = true;
                    }
                    if ui.small_button("Save as playlist").clicked() {
                        save = true;
                    }
                });
                ui.add_space(6.0);
                if q.is_empty() {
                    ui.label(RichText::new("Nothing queued. Right-click a song → Add to queue.").color(th.dim));
                    return;
                }
                egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, 46.0, q.len(), |ui, range| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for i in range {
                        let t = &q[i];
                        let now = i == idx;
                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 46.0), Sense::click());
                        let p = ui.painter();
                        if now {
                            p.rect_filled(rect, 8.0, th.accent.gamma_multiply(0.14));
                        } else if resp.hovered() {
                            p.rect_filled(rect, 8.0, th.card);
                        }
                        let clip = Rect::from_min_max(rect.min, Pos2::new(rect.right() - 78.0, rect.bottom()));
                        let pc = p.with_clip_rect(clip);
                        pc.text(
                            Pos2::new(rect.left() + 8.0, rect.center().y - 8.0),
                            Align2::LEFT_CENTER,
                            &t.title,
                            FontId::proportional(13.5),
                            if now { th.accent } else { th.text },
                        );
                        pc.text(Pos2::new(rect.left() + 8.0, rect.center().y + 10.0), Align2::LEFT_CENTER, &t.artist, FontId::proportional(12.0), th.dim);
                        let mut hit = false;
                        for (k, (label, dx)) in [("^", 66.0f32), ("v", 44.0), ("x", 22.0)].iter().enumerate() {
                            let c = Pos2::new(rect.right() - dx, rect.center().y);
                            let br = Rect::from_center_size(c, Vec2::splat(22.0));
                            let r = ui.interact(br, ui.make_persistent_id(("qb", i, k)), Sense::click());
                            let col = if r.hovered() { th.accent } else { th.dim };
                            ui.painter().text(c, Align2::CENTER_CENTER, *label, FontId::proportional(15.0), col);
                            if r.clicked() {
                                hit = true;
                                match k {
                                    0 if i > 0 => mv = Some((i, i - 1)),
                                    1 if i + 1 < q.len() => mv = Some((i, i + 1)),
                                    2 => remove = Some(i),
                                    _ => {}
                                }
                            }
                        }
                        if resp.clicked() && !hit {
                            jump = Some(i);
                        }
                    }
                });
            });
        if let Some(i) = jump {
            let _ = self.cmd.send(Cmd::Jump(i));
        }
        if let Some(i) = remove {
            let _ = self.cmd.send(Cmd::RemoveAt(i));
        }
        if let Some((a, b)) = mv {
            let _ = self.cmd.send(Cmd::MoveItem(a, b));
        }
        if clear {
            let _ = self.cmd.send(Cmd::ClearUpcoming);
        }
        if save && !q.is_empty() {
            let n = self.cfg.playlists.len() + 1;
            self.cfg.playlists.push(Playlist { name: format!("Queue {n}"), tracks: q.to_vec() });
            self.cfg.save();
            self.toast = Some((format!("Saved as playlist “Queue {n}”"), Instant::now()));
        }
    }

    // ---------------- pages ----------------

    pub(crate) fn page_search(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let th = self.theme;
        let mut go = false;
        let mut pick: Option<String> = None;
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 150.0).min(900.0);
            let resp = egui::Frame::none()
                .fill(th.card2)
                .rounding(26.0)
                .inner_margin(Margin::symmetric(22.0, 12.0))
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.query)
                            .hint_text("Search songs, albums, artists — or paste a YouTube link…")
                            .font(FontId::proportional(17.0))
                            .frame(false)
                            .desired_width(w)
                            .min_size(Vec2::new(w, 34.0)),
                    )
                })
                .inner;
            if self.focus_once {
                resp.request_focus();
                self.focus_once = false;
            }
            if resp.changed() {
                self.sug_dirty = true;
                self.sug_t = Instant::now();
                self.show_sug = true;
            }
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                go = true;
            }
            if ui.add_sized([120.0, 58.0], accent_button(th, "Search")).clicked() {
                go = true;
            }
        });
        // suggestions as you type (debounced)
        if self.sug_dirty && self.sug_t.elapsed() > Duration::from_millis(220) {
            self.sug_dirty = false;
            let q = self.query.trim().to_string();
            if q.chars().count() >= 2 {
                self.sug_req += 1;
                let id = self.sug_req;
                self.bg(ctx, move || match ytm::suggestions(&q) {
                    Ok(v) => Msg::Suggest(id, v),
                    Err(_) => Msg::Suggest(id, vec![]),
                });
            } else {
                self.suggestions.clear();
            }
        } else if self.sug_dirty {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        if self.show_sug && !self.suggestions.is_empty() {
            ui.add_space(4.0);
            egui::Frame::none().fill(th.card).rounding(12.0).inner_margin(Margin::symmetric(6.0, 6.0)).show(ui, |ui| {
                ui.set_width(ui.available_width().min(560.0));
                for s in self.suggestions.iter() {
                    ui.horizontal(|ui| {
                        ui.add_space(6.0);
                        ui.label(RichText::new("⌕").color(th.dim));
                        if ui.add(egui::Button::new(RichText::new(s).size(14.5).color(th.text)).frame(false)).clicked() {
                            pick = Some(s.clone());
                        }
                    });
                }
            });
        } else if self.show_sug && self.query.trim().is_empty() && !self.cfg.recent_queries.is_empty() {
            // No typing yet: offer recent searches instead of an empty box.
            ui.add_space(4.0);
            ui.label(RichText::new("RECENT SEARCHES").size(9.5).strong().color(th.dim));
            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| {
                for s in self.cfg.recent_queries.iter().take(8) {
                    if self.pill(ui, s, false).clicked() {
                        pick = Some(s.clone());
                    }
                }
            });
        }
        if let Some(s) = pick {
            self.query = s;
            go = true;
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            for (i, n) in ["All", "Songs", "Albums", "Artists", "Videos"].iter().enumerate() {
                if ui.selectable_label(self.sfilter == i, *n).clicked() && self.sfilter != i {
                    self.sfilter = i;
                    if !self.query.trim().is_empty() {
                        go = true;
                    }
                }
            }
        });
        if go {
            self.run_search(ctx);
        }
        ui.add_space(10.0);
        self.header(ui);
        if self.sres.is_empty() {
            if !self.loading && self.status.is_empty() {
                self.empty(ui, "Search for a song to get started", "No ads. Just music.");
                ui.add_space(24.0);
                let mut chip: Option<&str> = None;
                ui.vertical_centered(|ui| {
                    ui.horizontal_wrapped(|ui| {
                        for s in ["Arijit Singh", "Anirudh Ravichander", "A. R. Rahman", "Lo-fi beats", "Taylor Swift", "The Weeknd", "Ilaiyaraaja", "Coke Studio"] {
                            let b = egui::Button::new(RichText::new(s).size(14.0)).rounding(18.0).min_size(Vec2::new(0.0, 34.0));
                            if ui.add(b).clicked() {
                                chip = Some(s);
                            }
                        }
                    });
                });
                if let Some(s) = chip {
                    self.query = s.to_string();
                    self.run_search(ctx);
                }
            }
            return;
        }
        let cur = self.snap.current.as_ref().map(|c| c.id.clone());
        let mut act: Option<Act> = None;
        let mut open: Option<ytm::Card> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if !self.sres.songs.is_empty() {
                let n = if self.sfilter == 0 { 6 } else { self.sres.songs.len() };
                ui.label(RichText::new("Songs").size(18.0).strong().color(th.text));
                ui.add_space(4.0);
                for (i, t) in self.sres.songs.iter().take(n).enumerate() {
                    track_row(ui, th, t, i, "sr", cur.as_deref(), false, &self.liked_ids, &mut act);
                }
            }
            for (title, list) in [("Albums", &self.sres.albums), ("Artists", &self.sres.artists), ("Playlists", &self.sres.playlists)] {
                if list.is_empty() {
                    continue;
                }
                section(ui, th, title, "");
                grid(ui, list.len().min(12), 160.0, |ui, i, w| {
                    if media_card(ui, th, &list[i], w).clicked() {
                        open = Some(list[i].clone());
                    }
                });
            }
            ui.add_space(20.0);
        });
        self.handle_act(act, None);
        if let Some(c) = open {
            self.open_card(ctx, c);
        }
    }

    pub(crate) fn page_detail(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let th = self.theme;
        let mut back = false;
        let mut play: Option<bool> = None;
        let mut save = false;
        let mut subscribe = false;
        let mut radio = false;
        let mut open: Option<ytm::Card> = None;
        let mut act: Option<Act> = None;
        if !self.back.is_empty() && ui.button("‹ Back").clicked() {
            back = true;
        }
        ui.add_space(6.0);
        match &self.detail {
            None => {
                if self.loading {
                    ui.horizontal(|ui| {
                        spin(ui, th);
                        ui.label(RichText::new("Loading…").color(th.dim));
                    });
                }
                if !self.status.is_empty() {
                    ui.label(RichText::new(&self.status).color(th.accent));
                }
            }
            Some(d) => {
                let artist_page = self.detail_kind == ytm::Kind::Artist;
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(150.0), Sense::hover());
                    if d.thumb.is_empty() {
                        ui.painter().rect_filled(r, 14.0, th.card2);
                    } else {
                        let rad = if artist_page { 75.0 } else { 14.0 };
                        egui::Image::new(ImageSource::Uri(d.thumb.clone().into())).fit_to_exact_size(r.size()).rounding(rad).paint_at(ui, r);
                    }
                    ui.add_space(14.0);
                    ui.vertical(|ui| {
                        ui.add_space(14.0);
                        ui.label(RichText::new(&d.title).size(30.0).strong().color(th.text));
                        let sub = if !d.artist.is_empty() { format!("{} · {}", d.artist, d.sub) } else { d.sub.clone() };
                        ui.label(RichText::new(sub).size(14.0).color(th.dim));
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            if !d.tracks.is_empty() {
                                if ui.add_sized([110.0, 36.0], accent_button(th, "Play")).clicked() {
                                    play = Some(false);
                                }
                                if ui.add_sized([110.0, 36.0], egui::Button::new("Shuffle")).clicked() {
                                    play = Some(true);
                                }
                            }
                            if artist_page {
                                if self.cfg.signed_in && ui.add_sized([110.0, 36.0], egui::Button::new("Subscribe")).clicked() {
                                    subscribe = true;
                                }
                                if let Some(t) = d.tracks.first() {
                                    let _ = t;
                                    if ui.add_sized([110.0, 36.0], egui::Button::new("Radio")).clicked() {
                                        radio = true;
                                    }
                                }
                            } else if self.cfg.signed_in && !d.playlist_id.is_empty() && ui.add_sized([150.0, 36.0], egui::Button::new("Save to library")).clicked() {
                                save = true;
                            }
                        });
                    });
                });
                ui.add_space(12.0);
                let cur = self.snap.current.as_ref().map(|c| c.id.clone());
                if artist_page {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        if !d.tracks.is_empty() {
                            ui.label(RichText::new("Top songs").size(18.0).strong().color(th.text));
                            for (i, t) in d.tracks.iter().take(10).enumerate() {
                                track_row(ui, th, t, i, "ar", cur.as_deref(), false, &self.liked_ids, &mut act);
                            }
                        }
                        for (si, shelf) in d.shelves.iter().enumerate() {
                            section(ui, th, &shelf.title, "");
                            ui.push_id(("ashelf", si), |ui| {
                                egui::ScrollArea::horizontal().auto_shrink([false, true]).show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        for c in shelf.cards.iter().take(16) {
                                            if media_card(ui, th, c, 150.0).clicked() {
                                                open = Some(c.clone());
                                            }
                                        }
                                    });
                                });
                            });
                        }
                        ui.add_space(24.0);
                    });
                } else {
                    act = draw_tracks(ui, th, &d.tracks, cur.as_deref(), self.yt_removable(), &self.liked_ids);
                }
            }
        }
        if back {
            if let Some(v) = self.back.pop() {
                self.detail = None;
                self.go(ctx, v);
            }
        }
        if let Some(shuffle) = play {
            let tr = self.detail.as_ref().map(|d| d.tracks.clone()).unwrap_or_default();
            if !tr.is_empty() {
                if shuffle != self.snap.shuffle {
                    self.cfg.shuffle = shuffle;
                    self.cfg.save();
                    let _ = self.cmd.send(Cmd::Shuffle(shuffle));
                }
                let i = if shuffle { (store::now() as usize) % tr.len() } else { 0 };
                self.record_recent(tr[i].clone());
                let _ = self.cmd.send(Cmd::Play(tr, i));
            }
        }
        if save {
            if let Some(pid) = self.detail.as_ref().map(|d| d.playlist_id.clone()) {
                self.bgn(move || match ytm::save_playlist(&pid, true) {
                    Ok(()) => Msg::Toast("Saved to your library".into()),
                    Err(e) => Msg::Toast(format!("Couldn’t save: {e}")),
                });
                self.fetch_cards(ctx, 0);
            }
        }
        if subscribe {
            let id = self.detail_id.clone();
            self.bgn(move || match ytm::subscribe(&id, true) {
                Ok(()) => Msg::Toast("Subscribed".into()),
                Err(e) => Msg::Toast(format!("Couldn’t subscribe: {e}")),
            });
        }
        if radio {
            if let Some(t) = self.detail.as_ref().and_then(|d| d.tracks.first().cloned()) {
                self.start_radio(&t.id);
            }
        }
        self.handle_act(act, None);
        if let Some(c) = open {
            self.open_card(ctx, c);
        }
    }

    pub(crate) fn page_cards(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, k: u8) {
        let th = self.theme;
        // Title row with Albums | Artists segmented tabs (one slim sidebar item).
        ui.horizontal(|ui| {
            ui.label(RichText::new(&self.label).size(24.0).strong().color(th.text));
            if self.loading {
                ui.add_space(8.0);
                spin(ui, th);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.pill(ui, "Artists", k == 1).clicked() && k != 1 {
                    self.go(ctx, View::Artists);
                }
                if self.pill(ui, "Albums", k == 0).clicked() && k != 0 {
                    self.go(ctx, View::Albums);
                }
            });
        });
        let msg = if !self.status.is_empty() { self.status.clone() } else { self.snap.error.clone() };
        if !msg.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new(msg).color(th.accent));
        }
        ui.add_space(10.0);
        if !self.cfg.signed_in {
            self.empty(ui, "Sign in to see your library", "Your saved albums and artists show up here.");
            return;
        }
        let list: Vec<ytm::Card> = if k == 0 { self.lib_albums.clone() } else { self.lib_artists.clone() };
        if list.is_empty() {
            if self.lib_loading {
                ui.horizontal(|ui| {
                    spin(ui, th);
                    ui.label(RichText::new("Loading…").color(th.dim));
                });
            } else if !self.lib_err.is_empty() {
                ui.label(RichText::new(&self.lib_err).color(th.accent));
                if ui.button("Retry").clicked() {
                    self.fetch_cards(ctx, k);
                }
            } else {
                self.empty(ui, "Nothing saved yet", "Save albums and subscribe to artists to see them here.");
            }
            return;
        }
        let mut open: Option<ytm::Card> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            grid(ui, list.len(), 160.0, |ui, i, w| {
                if media_card(ui, th, &list[i], w).clicked() {
                    open = Some(list[i].clone());
                }
            });
        });
        if let Some(c) = open {
            self.open_card(ctx, c);
        }
    }

    pub(crate) fn page_history(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let th = self.theme;
        self.header(ui);
        ui.horizontal(|ui| {
            if self.cfg.signed_in {
                if self.pill(ui, "▶▶ YouTube Music", !self.hist_local).clicked() && self.hist_local {
                    self.hist_local = false;
                }
                if self.pill(ui, "▤ On this device", self.hist_local).clicked() && !self.hist_local {
                    self.hist_local = true;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(egui::Button::new(RichText::new("◔  Listening stats").size(12.0).color(th.accent)).fill(th.card).rounding(12.0)).clicked() {
                    self.go(ctx, View::Stats);
                }
            });
        });
        ui.add_space(6.0);
        let groups: Vec<(String, Vec<Track>)> = if self.hist_local || !self.cfg.signed_in {
            let today = (store::now() as i64 + tz_offset()).div_euclid(86_400);
            let mut out: Vec<(String, Vec<Track>)> = vec![];
            for h in store::history(400) {
                let d = today - (h.ts as i64 + tz_offset()).div_euclid(86_400);
                let label = match d {
                    0 => "Today".to_string(),
                    1 => "Yesterday".to_string(),
                    n => format!("{n} days ago"),
                };
                match out.last_mut() {
                    Some((l, v)) if *l == label => v.push(h.track),
                    _ => out.push((label, vec![h.track])),
                }
            }
            out
        } else {
            if self.hist_loading {
                ui.horizontal(|ui| {
                    spin(ui, th);
                    ui.label(RichText::new("Loading…").color(th.dim));
                });
            }
            if !self.hist_err.is_empty() {
                ui.label(RichText::new(&self.hist_err).color(th.accent));
            }
            self.history.clone()
        };
        if groups.is_empty() {
            if !self.hist_loading {
                self.empty(ui, "No history yet", "Songs you listen to show up here.");
            }
            return;
        }
        let cur = self.snap.current.as_ref().map(|c| c.id.clone());
        let mut picked: Option<(Vec<Track>, Act)> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (gi, (label, tracks)) in groups.iter().enumerate() {
                ui.add_space(10.0);
                ui.label(RichText::new(label).size(18.0).strong().color(th.text));
                for (i, t) in tracks.iter().enumerate() {
                    let mut a: Option<Act> = None;
                    track_row(ui, th, t, i, &format!("h{gi}"), cur.as_deref(), false, &self.liked_ids, &mut a);
                    if let Some(a) = a {
                        picked = Some((tracks.clone(), a));
                    }
                }
            }
            ui.add_space(20.0);
        });
        if let Some((list, a)) = picked {
            self.tracks = list;
            self.handle_act(Some(a), None);
        }
    }

    pub(crate) fn page_stats(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        let th = self.theme;
        // Title + tabs are drawn by page_system_tabs.
        let (plays, secs, songs) = store::totals();
        if plays == 0 && secs < 1.0 {
            self.empty(ui, "No stats yet", "Rustym counts what you play on this device — nothing leaves your computer.");
            return;
        }
        let top = store::top_tracks(25);
        let artists = store::top_artists(12);
        let cur = self.snap.current.as_ref().map(|c| c.id.clone());
        let mut picked: Option<(Vec<Track>, Act)> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal(|ui| {
                for (big, small) in [(plays.to_string(), "plays"), (fmt_hms(secs), "listened"), (songs.to_string(), "different songs")] {
                    egui::Frame::none().fill(th.card).rounding(14.0).inner_margin(Margin::symmetric(22.0, 14.0)).show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new(big).size(28.0).strong().color(th.accent));
                            ui.label(RichText::new(small).size(13.0).color(th.dim));
                        });
                    });
                }
            });
            section(ui, th, "Top artists", "");
            for (i, (name, p, s)) in artists.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.add_sized([28.0, 20.0], egui::Label::new(RichText::new(format!("{}", i + 1)).color(th.dim)));
                    ui.label(RichText::new(name).size(15.0).color(th.text));
                    ui.label(RichText::new(format!("{p} plays · {}", fmt_hms(*s))).size(12.0).color(th.dim));
                });
            }
            section(ui, th, "Top songs", "");
            let list: Vec<Track> = top.iter().map(|s| s.track.clone()).collect();
            for (i, t) in list.iter().enumerate() {
                let mut a: Option<Act> = None;
                track_row(ui, th, t, i, "st", cur.as_deref(), false, &self.liked_ids, &mut a);
                if let Some(a) = a {
                    picked = Some((list.clone(), a));
                }
            }
            ui.add_space(20.0);
        });
        if let Some((list, a)) = picked {
            self.tracks = list;
            self.handle_act(Some(a), None);
        }
    }

    pub(crate) fn page_local(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let th = self.theme;
        self.header(ui);
        let mut rescan = false;
        let mut remove: Option<usize> = None;
        ui.horizontal_wrapped(|ui| {
            for (i, d) in self.cfg.local_dirs.iter().enumerate() {
                ui.label(RichText::new(d).color(th.dim2));
                if ui.small_button("remove").clicked() {
                    remove = Some(i);
                }
                ui.add_space(10.0);
            }
        });
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.local_input).hint_text("Add a music folder, e.g. ~/Music/Albums").desired_width(320.0));
            if ui.button("Add folder").clicked() && !self.local_input.trim().is_empty() {
                self.cfg.local_dirs.push(self.local_input.trim().to_string());
                self.local_input.clear();
                self.cfg.save();
                rescan = true;
            }
            if ui.button("Rescan").clicked() {
                rescan = true;
            }
            if !self.local_tracks.is_empty() && ui.add(accent_button(th, "Play all")).clicked() {
                let _ = self.cmd.send(Cmd::Play(self.local_tracks.clone(), 0));
            }
        });
        if let Some(i) = remove {
            self.cfg.local_dirs.remove(i);
            self.cfg.save();
            rescan = true;
        }
        if rescan {
            self.scan_local(ctx);
        }
        ui.add_space(8.0);
        if self.local_tracks.is_empty() {
            if !self.loading {
                self.empty(ui, "No music files found", "Add a folder with mp3 / flac / ogg / opus / m4a files.");
            }
            return;
        }
        self.tracks = self.local_tracks.clone();
        let cur = self.snap.current.as_ref().map(|c| c.id.clone());
        let act = draw_tracks(ui, th, &self.local_tracks, cur.as_deref(), false, &self.liked_ids);
        self.handle_act(act, None);
    }

    pub(crate) fn page_now(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        let th = self.theme;
        let Some(t) = self.snap.current.clone() else {
            self.empty(ui, "Nothing playing yet", "Choose a track from Search, Home or your library. Your player will appear here automatically.");
            return;
        };

        ui.add_space(8.0);

        let available = ui.available_width().max(320.0);
        let available_height = ui.available_height();
        let lyrics_shown = self.lyrics_enabled;
        let wide = available >= 980.0;
        // What goes into the right pane on wide windows:
        //  * the vinyl player by default (also when no lyrics were found),
        //  * the lyrics panel when lyrics are enabled and found (or being found),
        //  * the vinyl again whenever the user presses the vinyl ring button.
        let lyrics_ready = self.lyrics.as_ref().map_or(false, |l| !l.synced.is_empty() || !l.plain.trim().is_empty());
        let show_lyrics_pane = wide && lyrics_shown && (lyrics_ready || self.lyrics_loading);
        let show_vinyl_pane = wide && (!show_lyrics_pane || self.vinyl_open);
        let inner_width = (available - 40.0).max(240.0);

        // When the visualiser is hidden, use the full vertical space left by the
        // top bar and bottom player (small breathing margin only).
        let expanded_height = !self.cfg.show_visualiser;
        let card_content_height = if expanded_height { (available_height - 26.0).max(300.0) } else { 0.0 };
        let wide_pane = show_lyrics_pane || show_vinyl_pane;
        let art_side = if expanded_height && wide_pane {
            ((card_content_height - 40.0).min(inner_width * 0.36)).clamp(280.0, 420.0)
        } else if available >= 900.0 {
            280.0
        } else {
            (available * 0.38).clamp(150.0, 230.0)
        };
        let info_width = if wide_pane {
            340.0_f32.min((inner_width - art_side - 320.0).max(250.0))
        } else {
            (inner_width - art_side - 28.0).max(190.0)
        };
        // The hero row is: art | info | pane, joined by item spacing + our own
        // add_space calls (~70 px total). Size the pane to what really remains
        // so it can never push past the card edge.
        let pane_width = if wide_pane {
            (inner_width - art_side - info_width - 70.0).max(200.0)
        } else {
            0.0
        };
        let card_min_height = if expanded_height { card_content_height } else { 0.0 };
        let column_height = if expanded_height { card_content_height - 40.0 } else { art_side }.max(art_side);
        let playing = self.snap.playing && !self.snap.paused;
        let liked = self.liked_ids.contains(&t.id);

        egui::Frame::none()
            .fill(th.card)
            .rounding(24.0)
            .inner_margin(Margin::same(20.0))
            .show(ui, |ui| {
                // Ambient accent glow inside the card corners (paint only, no layout).
                let p = ui.painter();
                let r = ui.max_rect();
                p.circle_filled(r.left_top() + Vec2::new(30.0, 30.0), 190.0, th.accent.gamma_multiply(0.05));
                p.circle_filled(r.right_bottom() - Vec2::new(30.0, 30.0), 150.0, th.accent.gamma_multiply(0.04));
                if card_min_height > 0.0 {
                    ui.set_min_height(card_min_height - 40.0);
                }
                ui.set_width(inner_width);

                // ---------- info column (shared by every layout) ----------
                let info_column = |ui: &mut egui::Ui, s: &mut Self, col_h: f32| {
                    if expanded_height {
                        ui.set_min_height(col_h);
                    }
                    ui.add_space(6.0);
                    ui.label(RichText::new("N O W   P L A Y I N G").size(10.0).strong().color(th.accent));
                    ui.add_space(8.0);
                    // Title and artist wrap so long names are never cut off.
                    ui.add(egui::Label::new(RichText::new(&t.title).font(serif_id(34.0)).color(th.text)));
                    ui.add_space(6.0);
                    ui.add(egui::Label::new(RichText::new(&t.artist).font(serif_italic_id(19.0)).color(th.dim2)));
                    ui.add_space(12.0);
                    // Meta row: like state · album
                    let mut meta = if liked { "♥  Liked".to_string() } else { String::new() };
                    if !t.album.is_empty() {
                        if !meta.is_empty() {
                            meta.push_str("   ·   ");
                        }
                        meta.push_str(&t.album);
                    }
                    if !meta.is_empty() {
                        ui.label(RichText::new(meta).size(13.0).color(th.dim));
                    }
                    ui.add_space(16.0);
                    // Hero actions: big accent play pill + ring buttons.
                    ui.horizontal(|ui| {
                        let label = if playing { "❚❚  Pause" } else { "▶  Play" };
                        if ui.add(play_pill(th, label)).clicked() {
                            let _ = s.cmd.send(Cmd::Toggle);
                        }
                        if ring_btn(ui, th, "♥", liked).clicked() && t.local.is_empty() {
                            s.toggle_like(&t);
                        }
                        if ring_btn(ui, th, "♫", lyrics_shown).clicked() {
                            s.lyrics_enabled = !lyrics_shown;
                            s.cfg.auto_show_lyrics = s.lyrics_enabled;
                            s.cfg.save();
                        }
                        if ring_btn(ui, th, "◉", s.vinyl_open).clicked() {
                            s.vinyl_open = !s.vinyl_open;
                        }
                        if ring_btn(ui, th, "◔", s.cfg.show_visualiser).clicked() {
                            s.cfg.show_visualiser = !s.cfg.show_visualiser;
                            s.cfg.save();
                        }
                        if ring_btn(ui, th, "＋", false).clicked() {
                            s.adding = Some(t.clone());
                        }
                        if ring_btn(ui, th, "≡", s.show_queue).clicked() {
                            s.show_queue = !s.show_queue;
                        }
                    });
                    ui.add_space(14.0);
                    // Status anchored to the bottom of the column when the card is
                    // expanded, so no dead space is left under the actions.
                    if expanded_height {
                        ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                            if s.snap.loading {
                                ui.horizontal(|ui| {
                                    spin(ui, th);
                                    ui.label(RichText::new("Preparing your track…").size(12.5).color(th.dim));
                                });
                            } else if s.snap.paused {
                                ui.label(RichText::new("Paused").size(12.5).color(th.dim));
                            } else {
                                ui.label(RichText::new("Enjoying your music").size(12.5).color(th.dim));
                            }
                        });
                    } else if s.snap.loading {
                        ui.horizontal(|ui| {
                            spin(ui, th);
                            ui.label(RichText::new("Preparing your track…").size(12.5).color(th.dim));
                        });
                    } else if s.snap.paused {
                        ui.label(RichText::new("Paused").size(12.5).color(th.dim));
                    } else {
                        ui.label(RichText::new("Enjoying your music").size(12.5).color(th.dim));
                    }
                };

                if wide_pane {
                    // Hero: artwork | info | lyrics panel or vinyl player.
                    ui.horizontal_top(|ui| {
                        ui.allocate_ui_with_layout(Vec2::new(art_side, art_side), Layout::top_down(Align::Center), |ui| {
                            let (art, _) = ui.allocate_exact_size(Vec2::splat(art_side), Sense::hover());
                            glow_art(ui, th, &t.id, art);
                        });
                        ui.add_space(18.0);
                        ui.allocate_ui_with_layout(Vec2::new(info_width, column_height), Layout::top_down(Align::Min), |ui| {
                            info_column(ui, self, column_height);
                        });
                        ui.add_space(12.0);
                        ui.allocate_ui_with_layout(Vec2::new(pane_width, column_height), Layout::top_down(Align::Center), |ui| {
                            if show_vinyl_pane {
                                let vsize = pane_width.min(column_height - 90.0).max(150.0);
                                let disp = self.disp.clone();
                                vinyl(ui, th, vsize, playing, &disp);
                            } else {
                                egui::Frame::none().fill(th.panel).rounding(16.0)
                                    .inner_margin(Margin::same(14.0)).show(ui, |ui| {
                                        ui.set_width(pane_width - 28.0);
                                        if expanded_height { ui.set_min_height(column_height - 28.0); }
                                        ui.add_space(2.0);
                                        egui::ScrollArea::vertical()
                                            .id_salt("now-playing-lyrics-panel")
                                            .max_height(column_height - 34.0)
                                            .max_width(pane_width - 28.0)
                                            .auto_shrink([false, false])
                                            .show(ui, |ui| {
                                                ui.set_width(pane_width - 32.0);
                                                self.draw_lyrics(ui);
                                            });
                                    });
                            }
                        });
                    });
                } else {
                    // Compact: artwork and metadata share a bounded row; lyrics
                    // become a full-width, height-limited panel beneath it.
                    ui.horizontal_top(|ui| {
                        ui.allocate_ui_with_layout(Vec2::new(art_side, art_side), Layout::top_down(Align::Center), |ui| {
                            let (art, _) = ui.allocate_exact_size(Vec2::splat(art_side), Sense::hover());
                            glow_art(ui, th, &t.id, art);
                        });
                        ui.add_space(18.0);
                        ui.allocate_ui_with_layout(Vec2::new(info_width, art_side), Layout::top_down(Align::Min), |ui| {
                            info_column(ui, self, art_side);
                        });
                    });
                    if lyrics_shown {
                        ui.add_space(16.0);
                        let compact_lyrics_height = if expanded_height {
                            (card_content_height - art_side - 96.0).max(160.0)
                        } else {
                            260.0
                        };
                        egui::Frame::none().fill(th.panel).rounding(16.0)
                            .inner_margin(Margin::same(14.0)).show(ui, |ui| {
                                ui.set_width(inner_width - 28.0);
                                if expanded_height {
                                    ui.set_min_height(compact_lyrics_height - 28.0);
                                }
                                ui.add_space(2.0);
                                egui::ScrollArea::vertical()
                                    .id_salt("now-playing-lyrics-compact")
                                    .max_height(compact_lyrics_height - 48.0)
                                    .max_width(inner_width - 28.0)
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        ui.set_width(inner_width - 32.0);
                                        self.draw_lyrics(ui);
                                    });
                            });
                    }
                }
            });

        if self.cfg.show_visualiser {
            ui.add_space(18.0);
            let style = self.cfg.viz;
            let vw = (ui.available_width() - 28.0).max(200.0);
            let min_vh = if style == 2 { 148.0 } else { 116.0 };
            // Stretch the visualiser to fill whatever vertical space is left above the
            // bottom player (frame margins 24 + header ~14 + gap 8 + page bottom pad 6),
            // so no dead band remains between the visualiser and the player bar.
            let vh = (ui.available_height() - 24.0 - 14.0 - 8.0 - 6.0).max(min_vh);
            egui::Frame::none()
                .fill(th.panel)
                .rounding(18.0)
                .inner_margin(Margin::symmetric(14.0, 12.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("AUDIO VISUALISER").size(10.0).strong().color(th.dim));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            for (i, name) in ["Radial", "Mirror", "Bars"].iter().enumerate() {
                                let active = self.cfg.viz == 2 - i;
                                if ui.add(egui::Button::new(RichText::new(*name).size(10.5).color(if active { th.text } else { th.dim })).fill(if active { th.card2 } else { th.card }).rounding(8.0)).clicked() {
                                    self.cfg.viz = 2 - i;
                                    self.cfg.save();
                                }
                            }
                        });
                    });
                    ui.add_space(8.0);
                    let resp = draw_viz(ui, th, &self.disp, style, Vec2::new(vw, vh));
                    if resp.clicked() {
                        self.cfg.viz = (self.cfg.viz + 1) % 3;
                        self.cfg.save();
                    }
                });
        }
    }
    
    fn draw_lyrics(&mut self, ui: &mut egui::Ui) {
        let th = self.theme;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Lyrics").size(19.0).strong().color(th.text));
            if let Some(l) = &self.lyrics {
                ui.label(RichText::new(format!("via {}", l.source)).size(11.0).color(th.dim));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.lyrics.as_ref().map_or(false, |l| !l.synced.is_empty()) {
                    if self.cfg.lyrics_offset != 0 {
                        ui.label(RichText::new(format!("{:+} ms", self.cfg.lyrics_offset)).size(11.0).color(th.accent));
                    }
                    if ui.add(egui::Button::new(RichText::new("+0.5 s").size(10.5).color(th.dim2)).fill(th.card).rounding(8.0)).clicked() {
                        self.cfg.lyrics_offset += 500;
                        self.cfg.save();
                    }
                    if ui.add(egui::Button::new(RichText::new("−0.5 s").size(10.5).color(th.dim2)).fill(th.card).rounding(8.0)).clicked() {
                        self.cfg.lyrics_offset -= 500;
                        self.cfg.save();
                    }
                }
            });
        });
        ui.add_space(6.0);
        if self.lyrics_loading {
            ui.horizontal(|ui| {
                spin(ui, th);
                ui.label(RichText::new("Finding lyrics…").color(th.dim));
            });
            return;
        }
        let offset = self.cfg.lyrics_offset;
        let pos = self.snap.pos;
        let mut seek: Option<f64> = None;
        let mut new_cur = self.lyric_cur;
        match &self.lyrics {
            Some(l) if !l.synced.is_empty() => {
                let cur = lyrics::current(&l.synced, pos, offset);
                ui.add_space(12.0);
                for (i, line) in l.synced.iter().enumerate() {
                        let active = Some(i) == cur;
                        let txt = if line.text.trim().is_empty() { "…".to_string() } else { line.text.clone() };
                        let rt = RichText::new(txt).size(if active { 24.0 } else { 18.0 }).color(if active { th.text } else { th.dim });
                        let r = ui.add(egui::Label::new(if active { rt.strong() } else { rt }).sense(Sense::click()));
                        if active && cur != self.lyric_cur {
                            r.scroll_to_me(Some(Align::Center));
                        }
                        if r.clicked() {
                            seek = Some((line.t - offset as f64 / 1000.0).max(0.0));
                        }
                    ui.add_space(6.0);
                }
                ui.add_space(24.0);
                new_cur = cur;
            }
            Some(l) => {
                ui.label(RichText::new(&l.plain).size(16.0).color(th.text));
            }
            None => {
                let msg = if self.lyrics_err.is_empty() { "No lyrics yet." } else { self.lyrics_err.as_str() };
                ui.label(RichText::new(msg).color(th.dim));
            }
        }
        self.lyric_cur = new_cur;
        if let Some(t) = seek {
            let _ = self.cmd.send(Cmd::Seek(t));
        }
    }
}
