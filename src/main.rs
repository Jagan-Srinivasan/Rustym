//! Rustym - simple, ad-free YouTube Music player (UI).

mod distro;
mod engine;
mod imgcache;
mod local;
mod lyrics;
mod mpris;
mod pages;
mod store;
mod ytm;

use eframe::egui::{
    self, Align, Align2, Color32, FontData, FontDefinitions, FontFamily, FontId, ImageSource, Layout, Margin,
    Pos2, Rect, RichText, Sense, Shape, Stroke, UiBuilder, Vec2,
};
use engine::*;
use eframe::egui::load::{BytesLoader, ImageLoader};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering::SeqCst},
        mpsc::{channel, Receiver, Sender},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

// ---------------- themes ----------------

#[derive(Clone, Copy)]
struct Theme {
    name: &'static str,
    bg: Color32,
    panel: Color32,
    card: Color32,
    card2: Color32,
    accent: Color32,
    text: Color32,
    dim: Color32,
    dim2: Color32,
    light: bool,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

const THEMES: [Theme; 7] = [
    Theme { name: "Crimson", bg: rgb(11, 11, 14), panel: rgb(18, 18, 23), card: rgb(28, 28, 36), card2: rgb(44, 44, 58),
            accent: rgb(255, 61, 94), text: rgb(240, 240, 245), dim: rgb(140, 140, 155), dim2: rgb(175, 175, 190), light: false },
    Theme { name: "Ocean", bg: rgb(8, 12, 20), panel: rgb(13, 19, 31), card: rgb(22, 31, 48), card2: rgb(34, 48, 72),
            accent: rgb(62, 153, 255), text: rgb(236, 242, 250), dim: rgb(130, 148, 172), dim2: rgb(168, 184, 205), light: false },
    Theme { name: "Emerald", bg: rgb(8, 14, 12), panel: rgb(13, 21, 18), card: rgb(22, 34, 29), card2: rgb(34, 52, 45),
            accent: rgb(30, 215, 130), text: rgb(236, 248, 242), dim: rgb(126, 152, 140), dim2: rgb(166, 190, 178), light: false },
    Theme { name: "Violet", bg: rgb(12, 10, 18), panel: rgb(19, 16, 29), card: rgb(30, 26, 46), card2: rgb(46, 40, 70),
            accent: rgb(168, 120, 255), text: rgb(242, 238, 250), dim: rgb(146, 136, 168), dim2: rgb(184, 176, 205), light: false },
    Theme { name: "Sunset", bg: rgb(16, 11, 10), panel: rgb(25, 17, 15), card: rgb(40, 28, 24), card2: rgb(60, 42, 36),
            accent: rgb(255, 140, 50), text: rgb(250, 242, 236), dim: rgb(166, 146, 136), dim2: rgb(204, 184, 172), light: false },
    Theme { name: "Snow", bg: rgb(245, 245, 248), panel: rgb(255, 255, 255), card: rgb(236, 236, 242), card2: rgb(220, 220, 230),
            accent: rgb(255, 45, 85), text: rgb(22, 22, 30), dim: rgb(110, 110, 125), dim2: rgb(75, 75, 90), light: true },
    // Warm near-black with a cream accent - the signature Rustym look.
    Theme { name: "Midnight", bg: rgb(22, 17, 14), panel: rgb(15, 12, 10), card: rgb(34, 27, 22), card2: rgb(51, 41, 32),
            accent: rgb(242, 201, 162), text: rgb(247, 240, 231), dim: rgb(158, 143, 130), dim2: rgb(199, 183, 167), light: false },
];

/// Serif display text (page titles, Now Playing hero). Falls back silently to
/// the default font when no system serif is installed.
fn serif_id(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("rustym-serif".into()))
}

fn serif_italic_id(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("rustym-serif-italic".into()))
}

fn apply_theme(ctx: &egui::Context, th: &Theme) {
    let mut v = if th.light { egui::Visuals::light() } else { egui::Visuals::dark() };
    v.panel_fill = th.bg;
    v.window_fill = th.panel;
    v.extreme_bg_color = th.card;
    v.selection.bg_fill = th.accent;
    v.widgets.noninteractive.fg_stroke.color = th.text;
    v.widgets.inactive.fg_stroke.color = th.text;
    v.widgets.hovered.fg_stroke.color = th.text;
    v.widgets.active.fg_stroke.color = th.text;
    v.widgets.inactive.weak_bg_fill = th.card;
    v.widgets.inactive.bg_fill = th.card;
    v.widgets.hovered.weak_bg_fill = th.card2;
    v.widgets.hovered.bg_fill = th.card2;
    v.widgets.inactive.rounding = 8.0.into();
    v.widgets.hovered.rounding = 8.0.into();
    v.widgets.active.rounding = 8.0.into();
    v.window_rounding = 16.0.into();
    v.menu_rounding = 12.0.into();
    v.widgets.noninteractive.bg_stroke = Stroke::NONE;
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.hovered.bg_stroke = Stroke::NONE;
    v.widgets.hovered.expansion = 0.0;
    v.widgets.active.expansion = 0.0;
    v.window_stroke = Stroke::new(1.0_f32, th.card2);
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = Vec2::new(10.0, 8.0);
        s.spacing.button_padding = Vec2::new(14.0, 7.0);
        s.spacing.scroll = egui::style::ScrollStyle::floating();
    });
}

// ---------------- helpers ----------------

enum Msg {
    Results(u64, Vec<Track>),
    Error(u64, String),
    Trending(Vec<Track>),
    TrendingErr(String),
    Remote(Vec<RemotePl>),
    RemoteErr(String),
    /// Some(browser) when a browser login was found, None when cookies.txt was used.
    LoginOk(Option<String>),
    LoginFail(String),
    LoginProgress(String),
    Home(Vec<ytm::Shelf>),
    HomeErr(String),
    SearchRes(u64, ytm::SearchOut),
    Suggest(u64, Vec<String>),
    Detail(u64, ytm::Detail),
    DetailErr(u64, String),
    /// 0 = albums, 1 = artists
    Cards(u8, Vec<ytm::Card>),
    CardsErr(u8, String),
    History(Vec<(String, Vec<Track>)>),
    HistoryErr(String),
    Lyrics(String, lyrics::Lyrics),
    LyricsErr(String, String),
    LikedIds(Vec<String>),
    /// Google account name + avatar URL.
    Account(String, String),
    /// Spotify import finished: (playlist name, matched tracks).
    SpotifyImported(String, Vec<Track>),
    /// User-facing notice shown as a toast.
    Toast(String),
    PlayNow(Vec<Track>),
    Local(Vec<Track>),
    /// Something changed in the YouTube library: reload the playlist list.
    LibraryChanged,
    /// Finished updating yt-dlp / deno from Settings > Dependencies.
    Deps(String),
}

#[derive(PartialEq, Clone, Copy)]
enum View {
    Home,
    Mix(usize),
    Remote(usize),
    Search,
    Liked,
    Now,
    Playlists,
    Playlist(usize),
    Settings,
    Logs,
    Albums,
    Artists,
    History,
    Stats,
    Local,
    Detail,
}

enum Act {
    Play(usize),
    Add(usize),
    Remove(usize),
    PlayNext(usize),
    Enqueue(usize),
    Like(usize),
    Radio(usize),
    ToArtist(usize),
    ToAlbum(usize),
}

#[derive(Clone, Copy, PartialEq)]
enum Icon {
    Play,
    Pause,
    Next,
    Prev,
    Shuffle,
    Repeat,
    Heart,
}


struct Mix {
    title: &'static str,
    sub: &'static str,
    query: &'static str,
    c1: Color32,
    c2: Color32,
}

static MIXES: [Mix; 8] = [
    Mix { title: "Top Hits", sub: "The biggest songs right now", query: "top hits 2026 official audio", c1: rgb(255, 61, 94), c2: rgb(120, 20, 80) },
    Mix { title: "Tamil Hits", sub: "Kollywood favourites", query: "tamil hit songs 2026", c1: rgb(255, 150, 40), c2: rgb(160, 40, 30) },
    Mix { title: "Hindi Hits", sub: "Bollywood chartbusters", query: "latest hindi songs 2026", c1: rgb(250, 90, 160), c2: rgb(90, 30, 140) },
    Mix { title: "Chill Vibes", sub: "Relax and unwind", query: "chill songs playlist relaxing", c1: rgb(60, 190, 200), c2: rgb(30, 70, 140) },
    Mix { title: "Workout Energy", sub: "Power through your set", query: "workout gym motivation songs", c1: rgb(255, 100, 60), c2: rgb(130, 20, 40) },
    Mix { title: "Lo-fi Focus", sub: "Beats to work and study to", query: "lofi beats songs focus", c1: rgb(130, 120, 255), c2: rgb(40, 30, 110) },
    Mix { title: "Throwback", sub: "Classics you know by heart", query: "best 2000s hit songs", c1: rgb(240, 190, 60), c2: rgb(140, 70, 20) },
    Mix { title: "Late Night Drive", sub: "Your after-hours soundtrack", query: "late night drive songs", c1: rgb(40, 120, 255), c2: rgb(20, 20, 70) },
];

static PAL: [(Color32, Color32); 5] = [
    (rgb(255, 61, 94), rgb(90, 20, 90)),
    (rgb(62, 153, 255), rgb(30, 40, 120)),
    (rgb(30, 215, 130), rgb(10, 80, 70)),
    (rgb(255, 140, 50), rgb(130, 40, 30)),
    (rgb(168, 120, 255), rgb(60, 30, 120)),
];

fn lerp_col(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// Filled rounded rectangle with a diagonal two-colour gradient.
fn rounded_gradient(p: &egui::Painter, r: Rect, rad: f32, c1: Color32, c2: Color32) {
    use std::f32::consts::{FRAC_PI_2, PI};
    let rad = rad.min(r.height() / 2.0).min(r.width() / 2.0);
    let corners = [
        (Pos2::new(r.right() - rad, r.top() + rad), -FRAC_PI_2),
        (Pos2::new(r.right() - rad, r.bottom() - rad), 0.0),
        (Pos2::new(r.left() + rad, r.bottom() - rad), FRAC_PI_2),
        (Pos2::new(r.left() + rad, r.top() + rad), PI),
    ];
    let mut m = egui::Mesh::default();
    m.colored_vertex(r.center(), lerp_col(c1, c2, 0.5));
    let mut n = 0u32;
    for (c, a0) in corners {
        for k in 0..=6 {
            let a = a0 + FRAC_PI_2 * k as f32 / 6.0;
            let pt = Pos2::new(c.x + rad * a.cos(), c.y + rad * a.sin());
            let t = ((pt.x - r.left()) / r.width().max(1.0)) * 0.35 + ((pt.y - r.top()) / r.height().max(1.0)) * 0.65;
            m.colored_vertex(pt, lerp_col(c1, c2, t.clamp(0.0, 1.0)));
            n += 1;
        }
    }
    for i in 1..n {
        m.add_triangle(0, i, i + 1);
    }
    m.add_triangle(0, n, 1);
    p.add(Shape::mesh(m));
}

#[derive(Clone, Copy)]
enum Glyph {
    Home,
    Search,
    Heart,
    Wave,
    List,
    Gear,
}

fn draw_glyph(p: &egui::Painter, g: Glyph, c: Pos2, col: Color32) {
    let st = Stroke::new(1.8_f32, col);
    let line = |pts: &[Pos2]| {
        for w in pts.windows(2) {
            p.line_segment([w[0], w[1]], st);
        }
    };
    let o = |x: f32, y: f32| Pos2::new(c.x + x, c.y + y);
    match g {
        Glyph::Home => {
            line(&[o(-9.0, -1.0), o(0.0, -9.0), o(9.0, -1.0)]);
            line(&[o(-6.0, -3.0), o(-6.0, 8.0), o(6.0, 8.0), o(6.0, -3.0)]);
        }
        Glyph::Search => {
            p.circle_stroke(o(-2.0, -2.0), 6.0, st);
            line(&[o(2.5, 2.5), o(8.0, 8.0)]);
        }
        Glyph::Heart => {
            let pts: Vec<Pos2> = (0..=28)
                .map(|k| {
                    let t = k as f32 / 28.0 * std::f32::consts::TAU;
                    let x = 16.0 * t.sin().powi(3);
                    let y = -(13.0 * t.cos() - 5.0 * (2.0 * t).cos() - 2.0 * (3.0 * t).cos() - (4.0 * t).cos());
                    o(x * 0.55, y * 0.55 + 1.0)
                })
                .collect();
            line(&pts);
        }
        Glyph::Wave => {
            for (dx, h) in [(-6.0f32, 5.0f32), (0.0, 10.0), (6.0, 7.0)] {
                p.rect_filled(Rect::from_center_size(o(dx, 0.0), Vec2::new(3.0, h * 1.6)), 1.5, col);
            }
        }
        Glyph::List => {
            line(&[o(-8.0, -6.0), o(8.0, -6.0)]);
            line(&[o(-8.0, 0.0), o(8.0, 0.0)]);
            line(&[o(-8.0, 6.0), o(2.0, 6.0)]);
        }
        Glyph::Gear => {
            p.circle_stroke(c, 5.0, st);
            for k in 0..8 {
                let a = k as f32 / 8.0 * std::f32::consts::TAU;
                let (s, co) = a.sin_cos();
                p.line_segment([o(co * 7.5, s * 7.5), o(co * 10.0, s * 10.0)], Stroke::new(2.2_f32, col));
            }
        }
    }
}

fn section(ui: &mut egui::Ui, th: Theme, title: &str, sub: &str) {
    ui.add_space(26.0);
    ui.label(RichText::new(title).font(serif_id(23.0)).color(th.text));
    if !sub.is_empty() {
        ui.label(RichText::new(sub).size(12.5).color(th.dim));
    }
    ui.add_space(10.0);
}

/// Lays `n` cards out in a responsive grid; `f(ui, index, card_width)` draws one card.
fn grid(ui: &mut egui::Ui, n: usize, min_w: f32, mut f: impl FnMut(&mut egui::Ui, usize, f32)) {
    let gap = 16.0;
    let avail = ui.available_width();
    let cols = (((avail + gap) / (min_w + gap)).floor() as usize).max(1);
    let w = ((avail - gap * (cols as f32 - 1.0)) / cols as f32).floor();
    let mut i = 0;
    while i < n {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for j in i..(i + cols).min(n) {
                f(ui, j, w);
            }
        });
        ui.add_space(gap - 8.0);
        i += cols;
    }
}

fn play_bubble(p: &egui::Painter, th: Theme, c: Pos2, k: f32) {
    if k < 0.02 {
        return;
    }
    let r = 19.0 * (0.6 + 0.4 * k);
    p.circle_filled(c + Vec2::new(0.0, 2.0), r + 2.0, Color32::from_black_alpha((70.0 * k) as u8));
    p.circle_filled(c, r, th.accent.gamma_multiply(k));
    let s = 6.5 * k;
    p.add(Shape::convex_polygon(
        vec![
            Pos2::new(c.x - s * 0.7, c.y - s),
            Pos2::new(c.x - s * 0.7, c.y + s),
            Pos2::new(c.x + s * 1.1, c.y),
        ],
        Color32::from_white_alpha((255.0 * k) as u8),
        Stroke::NONE,
    ));
}

/// Draws `text` on one line at `pos` (left-centre) and ends it with "…" when it is wider than
/// `max_w`, instead of being chopped off at the card edge.
fn ellipsis_text(p: &egui::Painter, pos: Pos2, text: &str, font: FontId, color: Color32, max_w: f32) {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping {
        max_width: max_w.max(1.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    let galley = p.layout_job(job);
    let top_left = Pos2::new(pos.x, pos.y - galley.size().y / 2.0);
    p.galley(top_left, galley, color);
}

/// UV rectangle that crops an image of `img` pixels so it *covers* a box of `boxs` without
/// stretching (centre crop).
fn cover_uv(img: Vec2, boxs: Vec2) -> Rect {
    if img.x <= 0.0 || img.y <= 0.0 || boxs.x <= 0.0 || boxs.y <= 0.0 {
        return Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    }
    let ia = img.x / img.y;
    let ba = boxs.x / boxs.y;
    if ia > ba {
        let w = ba / ia;
        Rect::from_min_max(Pos2::new((1.0 - w) / 2.0, 0.0), Pos2::new((1.0 + w) / 2.0, 1.0))
    } else {
        let h = ia / ba;
        Rect::from_min_max(Pos2::new(0.0, (1.0 - h) / 2.0), Pos2::new(1.0, (1.0 + h) / 2.0))
    }
}

/// Network picture that fills `rect` (cropped, never stretched). Used by the home page cards.
pub(crate) fn cover_image(ui: &egui::Ui, uri: &str, rect: Rect, radius: f32) {
    let img = egui::Image::new(ImageSource::Uri(uri.to_owned().into()));
    let uv = match img.load_for_size(ui.ctx(), rect.size()) {
        Ok(egui::load::TexturePoll::Ready { texture }) => cover_uv(texture.size, rect.size()),
        _ => Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
    };
    img.uv(uv).fit_to_exact_size(rect.size()).rounding(radius).paint_at(ui, rect);
}

fn track_card(ui: &mut egui::Ui, th: Theme, t: &Track, w: f32) -> egui::Response {
    // Square artwork (like the shelves above). The old 16:9 box stretched the square crop of the
    // thumbnail sideways, which is why album art looked squashed.
    let side = (w - 20.0).floor();
    let h = side + 20.0 + 50.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
    let k = ui.ctx().animate_bool(resp.id, resp.hovered());
    let p = ui.painter();
    p.rect_filled(rect, 14.0, lerp_col(th.card, th.card2, k));
    let img = Rect::from_min_size(rect.min + Vec2::splat(10.0), Vec2::splat(side));
    thumb(ui, &t.id, img, 10.0);
    let p = ui.painter();
    play_bubble(p, th, img.right_bottom() + Vec2::new(-28.0, -28.0 + 6.0 * (1.0 - k)), k);
    let tw = w - 24.0;
    ellipsis_text(p, Pos2::new(rect.left() + 12.0, img.bottom() + 17.0), &t.title, FontId::proportional(14.5), th.text, tw);
    ellipsis_text(p, Pos2::new(rect.left() + 12.0, img.bottom() + 37.0), &t.artist, FontId::proportional(12.5), th.dim, tw);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn mix_card(ui: &mut egui::Ui, th: Theme, title: &str, sub: &str, c1: Color32, c2: Color32, w: f32) -> egui::Response {
    let h = (w * 0.64).floor();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h + 8.0), Sense::click());
    let k = ui.ctx().animate_bool(resp.id, resp.hovered());
    let r = Rect::from_min_size(rect.min + Vec2::new(0.0, 8.0 - 6.0 * k), Vec2::new(w, h));
    let p = ui.painter();
    if k > 0.01 {
        p.rect_filled(r.translate(Vec2::new(0.0, 6.0)).expand(-4.0), 16.0, Color32::from_black_alpha((60.0 * k) as u8));
    }
    rounded_gradient(p, r, 16.0, c1, c2);
    let dec = Color32::from_white_alpha(22);
    p.circle_filled(Pos2::new(r.right() - w * 0.2, r.top() + h * 0.36), h * 0.26, dec);
    p.circle_stroke(Pos2::new(r.right() - w * 0.2, r.top() + h * 0.36), h * 0.34, Stroke::new(1.5_f32, Color32::from_white_alpha(30)));
    p.circle_filled(Pos2::new(r.left() + w * 0.12, r.bottom() - h * 0.1), h * 0.12, dec);
    let clip = p.with_clip_rect(Rect::from_min_max(r.left_top(), Pos2::new(r.right() - 58.0, r.bottom())).shrink(2.0));
    clip.text(Pos2::new(r.left() + 16.0, r.bottom() - 36.0), Align2::LEFT_CENTER, title, FontId::proportional(19.0), Color32::WHITE);
    clip.text(Pos2::new(r.left() + 16.0, r.bottom() - 15.0), Align2::LEFT_CENTER, sub, FontId::proportional(12.0), Color32::from_white_alpha(205));
    play_bubble(p, th, Pos2::new(r.right() - 32.0, r.bottom() - 32.0 + 6.0 * (1.0 - k)), k);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// First `n` characters of a name, with "…" when it was cut.
fn short_name(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

/// Round Google account picture (falls back to the first letter of the name).
fn account_avatar(ui: &mut egui::Ui, th: Theme, photo: &str, name: &str, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    if photo.is_empty() {
        let p = ui.painter();
        p.circle_filled(rect.center(), size / 2.0, th.accent);
        let letter = name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into());
        p.text(rect.center(), Align2::CENTER_CENTER, letter, FontId::proportional(size * 0.46), Color32::WHITE);
    } else {
        ui.painter().circle_filled(rect.center(), size / 2.0, th.card2);
        cover_image(ui, photo, rect, size / 2.0);
    }
}

fn greeting() -> String {
    let h = std::process::Command::new("date")
        .arg("+%H")
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u32>().ok())
        .unwrap_or(12);
    let g = match h {
        5..=11 => "Good morning",
        12..=16 => "Good afternoon",
        17..=21 => "Good evening",
        _ => "Late-night listening",
    };
    g.to_string()
}

/// Spinner that repaints at ~30fps instead of every frame.
fn spin(ui: &mut egui::Ui, th: Theme) {
    let (r, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
    let t = ui.input(|i| i.time) as f32;
    let c = r.center();
    for k in 0..10 {
        let a = k as f32 / 10.0 * std::f32::consts::TAU;
        let age = ((t * 1.6 - k as f32 / 10.0).rem_euclid(1.0)).powf(0.6);
        let (sn, cs) = a.sin_cos();
        ui.painter().line_segment(
            [Pos2::new(c.x + cs * 4.5, c.y + sn * 4.5), Pos2::new(c.x + cs * 8.0, c.y + sn * 8.0)],
            Stroke::new(1.8_f32, th.accent.gamma_multiply(0.15 + 0.85 * (1.0 - age))),
        );
    }
    ui.ctx().request_repaint_after(Duration::from_millis(33));
}

fn fmt_time(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

fn vgradient(p: &egui::Painter, r: Rect, top: Color32, bot: Color32) {
    let mut m = egui::Mesh::default();
    m.colored_vertex(r.left_top(), top);
    m.colored_vertex(r.right_top(), top);
    m.colored_vertex(r.right_bottom(), bot);
    m.colored_vertex(r.left_bottom(), bot);
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    p.add(Shape::mesh(m));
}

fn thumb(ui: &egui::Ui, id: &str, rect: Rect, radius: f32) {
    if id.starts_with("local:") {
        ui.painter().rect_filled(rect, radius, ui.visuals().extreme_bg_color);
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, "FILE", FontId::proportional(11.0), ui.visuals().weak_text_color());
        return;
    }
    // YouTube video thumbnails are 16:9. Crop the left/right edges so the
    // poster artwork fills this square tile instead of letterboxing/stretching.
    // Size-appropriate variant: mqdefault (320x180, always exists) for small
    // tiles and rows, maxresdefault only for the large Now Playing artwork.
    // Together with the bounded image loader this keeps decoded thumbnails
    // tiny; maxres was 1280x720 (~3.7 MB decoded) per tile and stayed cached
    // forever.
    let side = rect.width().max(rect.height());
    // Size-appropriate variant: mqdefault (320x180, always exists) for small
    // tiles and rows, maxresdefault for anything bigger so square tiles stay
    // sharp (mq's 180 px height would need upscaling above ~200 px). Together
    // with the bounded cover-aware image loader this keeps decoded thumbnails
    // tiny; maxres was 1280x720 (~3.7 MB decoded) per tile and stayed cached
    // forever before.
    let variant = if side <= 200.0 { "mqdefault.jpg" } else { "maxresdefault.jpg" };
    egui::Image::new(ImageSource::Uri(format!("https://i.ytimg.com/vi/{id}/{variant}").into()))
        .uv(Rect::from_min_max(Pos2::new(0.21875, 0.0), Pos2::new(0.78125, 1.0)))
        .fit_to_exact_size(rect.size())
        .rounding(radius)
        .paint_at(ui, rect);
}

fn icon_btn(ui: &mut egui::Ui, th: Theme, icon: Icon, size: f32, circle: bool, active: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let hot = resp.hovered();
    let c = rect.center();
    let p = ui.painter();
    let fg;
    if circle {
        // Accent-filled circle with a dark glyph - the signature play button.
        let fill = if hot { th.accent.gamma_multiply(0.85) } else { th.accent };
        p.circle_filled(c, size / 2.0, fill);
        fg = if th.light { Color32::WHITE } else { rgb(20, 15, 11) };
    } else {
        if hot {
            // Soft hover disc behind the glyph.
            p.circle_filled(c, size * 0.5, th.card2);
        }
        fg = if active { th.accent } else if hot { th.text } else { th.dim2 };
    }
    let s = size * if circle { 0.26 } else { 0.22 };
    match icon {
        Icon::Play => {
            p.add(Shape::convex_polygon(
                vec![
                    Pos2::new(c.x - s * 0.7, c.y - s * 1.1),
                    Pos2::new(c.x - s * 0.7, c.y + s * 1.1),
                    Pos2::new(c.x + s * 1.1, c.y),
                ],
                fg,
                Stroke::NONE,
            ));
        }
        Icon::Pause => {
            for dx in [-0.55f32, 0.55] {
                p.rect_filled(
                    Rect::from_center_size(Pos2::new(c.x + s * dx, c.y), Vec2::new(s * 0.6, s * 2.0)),
                    1.0,
                    fg,
                );
            }
        }
        Icon::Next => {
            p.add(Shape::convex_polygon(
                vec![
                    Pos2::new(c.x - s, c.y - s),
                    Pos2::new(c.x - s, c.y + s),
                    Pos2::new(c.x + s * 0.6, c.y),
                ],
                fg,
                Stroke::NONE,
            ));
            p.rect_filled(
                Rect::from_center_size(Pos2::new(c.x + s * 0.95, c.y), Vec2::new(s * 0.3, s * 2.0)),
                1.0,
                fg,
            );
        }
        Icon::Prev => {
            p.add(Shape::convex_polygon(
                vec![
                    Pos2::new(c.x + s, c.y - s),
                    Pos2::new(c.x + s, c.y + s),
                    Pos2::new(c.x - s * 0.6, c.y),
                ],
                fg,
                Stroke::NONE,
            ));
            p.rect_filled(
                Rect::from_center_size(Pos2::new(c.x - s * 0.95, c.y), Vec2::new(s * 0.3, s * 2.0)),
                1.0,
                fg,
            );
        }
        Icon::Shuffle => {
            // Two crossing arrows with solid wedge arrowheads in the top-right
            // and bottom-right corners.
            let w = s * 1.6;  // half width
            let h = s * 1.0;  // half height
            let a = s * 0.72; // corner arrowhead size
            let (l, r, t, b) = (c.x - w, c.x + w, c.y - h, c.y + h);
            p.line_segment([Pos2::new(l, b), Pos2::new(r, t)], Stroke::new(1.5_f32, fg));
            p.line_segment([Pos2::new(l, t), Pos2::new(r, b)], Stroke::new(1.5_f32, fg));
            // top-right wedge (arrow pointing up-right into the corner)
            p.add(Shape::convex_polygon(
                vec![Pos2::new(r - a, t), Pos2::new(r, t), Pos2::new(r, t + a)],
                fg,
                Stroke::NONE,
            ));
            // bottom-right wedge (arrow pointing down-right into the corner)
            p.add(Shape::convex_polygon(
                vec![Pos2::new(r - a, b), Pos2::new(r, b), Pos2::new(r, b - a)],
                fg,
                Stroke::NONE,
            ));
        }
        Icon::Repeat => {
            // Loop of two opposing arrows: top edge points right, bottom edge
            // points left, joined by stubs on opposite sides.
            let w = s * 1.6;   // half width
            let h = s * 0.95;  // half height
            let a = s * 0.62;  // arrowhead length
            let aw = s * 0.48; // arrowhead half-width
            let (l, r, t, b) = (c.x - w, c.x + w, c.y - h, c.y + h);
            let st = Stroke::new(1.5_f32, fg);
            p.line_segment([Pos2::new(l, t), Pos2::new(r - a, t)], st);
            p.line_segment([Pos2::new(r, b), Pos2::new(l + a, b)], st);
            p.line_segment([Pos2::new(l, t), Pos2::new(l, c.y)], st);
            p.line_segment([Pos2::new(r, b), Pos2::new(r, c.y)], st);
            p.add(Shape::convex_polygon(
                vec![Pos2::new(r - a, t - aw), Pos2::new(r - a, t + aw), Pos2::new(r, t)],
                fg,
                Stroke::NONE,
            ));
            p.add(Shape::convex_polygon(
                vec![Pos2::new(l + a, b - aw), Pos2::new(l + a, b + aw), Pos2::new(l, b)],
                fg,
                Stroke::NONE,
            ));
        }
        Icon::Heart => {
            let r = s * 1.1;
            p.circle_filled(Pos2::new(c.x - r * 0.5, c.y - r * 0.35), r * 0.62, fg);
            p.circle_filled(Pos2::new(c.x + r * 0.5, c.y - r * 0.35), r * 0.62, fg);
            p.add(Shape::convex_polygon(
                vec![Pos2::new(c.x - r * 1.05, c.y - r * 0.15), Pos2::new(c.x + r * 1.05, c.y - r * 0.15), Pos2::new(c.x, c.y + r * 1.15)],
                fg,
                Stroke::NONE,
            ));
        }
    }
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn bar(ui: &mut egui::Ui, th: Theme, frac: f32, width: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width.max(20.0), 18.0), Sense::click_and_drag());
    let y = rect.center().y;
    let rail = Rect::from_min_max(Pos2::new(rect.left(), y - 2.0), Pos2::new(rect.right(), y + 2.0));
    let x = rect.left() + rect.width() * frac.clamp(0.0, 1.0);
    let hot = resp.hovered() || resp.dragged();
    let p = ui.painter();
    p.rect_filled(rail, 2.0, th.card2);
    p.rect_filled(
        Rect::from_min_max(rail.min, Pos2::new(x, rail.max.y)),
        2.0,
        if hot { th.accent } else { th.text },
    );
    if hot {
        p.circle_filled(Pos2::new(x, y), 6.0, th.text);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn nav_item(ui: &mut egui::Ui, th: Theme, icon: Glyph, label: &str, active: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 38.0), Sense::click());
    let k = ui.ctx().animate_bool(resp.id, resp.hovered());
    let p = ui.painter();
    if active {
        p.rect_filled(rect, 10.0, th.accent.gamma_multiply(0.15));
    } else if k > 0.01 {
        p.rect_filled(rect, 10.0, th.accent.gamma_multiply(0.08 * k));
    }
    let col = if active { th.accent } else { lerp_col(th.dim2, th.text, k) };
    draw_glyph(p, icon, Pos2::new(rect.left() + 24.0, rect.center().y), col);
    p.text(
        Pos2::new(rect.left() + 46.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(14.5),
        if active { th.text } else { lerp_col(th.dim2, th.text, k) },
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn accent_button(th: Theme, text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_string()).strong().color(Color32::WHITE))
        .fill(th.accent)
        .rounding(8.0)
}

/// Load system fonts as fallbacks for scripts egui's built-in font lacks
/// (Bengali, Gurmukhi, Gujarati, Odia, Sinhala, Tamil, Devanagari, Telugu,
/// Kannada, Malayalam, Arabic, Thai, CJK ...). Looks in several common
/// distro locations and also asks fontconfig, so it works on Arch, Debian/Ubuntu,
/// Fedora, etc.
fn setup_fonts(ctx: &egui::Context) {
    use std::path::{Path, PathBuf};
    let mut fonts = FontDefinitions::default();

    let dirs = [
        "/usr/share/fonts/noto",
        "/usr/share/fonts/truetype/noto",
        "/usr/share/fonts/opentype/noto",
        "/usr/share/fonts/noto-cjk",
        "/usr/share/fonts/google-noto",
        "/usr/share/fonts/google-noto-vf",
        "/usr/share/fonts/google-noto-cjk",
        "/usr/share/fonts/truetype/lohit",
        "/usr/share/fonts/truetype/freefont",
        "/usr/share/fonts/TTF",
        "/usr/share/fonts/truetype",
        "/usr/share/fonts/opentype",
    ];
    // File-name stems, in priority order. Bengali first. (The heavy CJK
    // fallbacks are discovered below but only loaded when needed.)
    let stems = [
        "NotoSansBengali", "NotoSansBengaliUI",
        "NotoSansDevanagari", "NotoSansTamil", "NotoSansTelugu", "NotoSansKannada",
        "NotoSansMalayalam", "NotoSansGujarati", "NotoSansGurmukhi", "NotoSansOriya",
        "NotoSansSinhala", "NotoSansArabic", "NotoSansThai",
    ];
    let mut found: Vec<PathBuf> = Vec::new();
    let push = |p: PathBuf, found: &mut Vec<PathBuf>| {
        if p.is_file() && !found.contains(&p) {
            found.push(p);
        }
    };
    for stem in stems {
        for d in dirs {
            for ext in ["-Regular.ttf", "-Regular.otf", "[wdth,wght].ttf", "-VF.ttf", ".ttf", "-Regular.ttc"] {
                push(Path::new(d).join(format!("{stem}{ext}")), &mut found);
            }
        }
    }
    // Fontconfig lookups cover fonts in unusual locations or with other names.
    // The Indic / Arabic / Thai fallbacks together are only ~2 MB, so they stay
    // part of the startup set; the ~19 MB CJK .ttc is discovered here but kept
    // for lazy loading (CJK_FONT_FILES), see `font_cjk_needed` / `load_cjk_fonts`.
    let mut cjk: Vec<PathBuf> = Vec::new();
    let push_cjk = |p: PathBuf, cjk: &mut Vec<PathBuf>| {
        if p.is_file() && !cjk.contains(&p) {
            cjk.push(p);
        }
    };
    for stem in ["NotoSansCJK-Regular", "NotoSansCJKjp-Regular"] {
        for d in dirs {
            for ext in ["-Regular.ttc", ".ttc", ".ttf", ".otf", "-Regular.otf"] {
                push_cjk(Path::new(d).join(format!("{stem}{ext}")), &mut cjk);
            }
        }
    }
    for query in [
        ":lang=bn", ":lang=hi", ":lang=ta", ":lang=te", ":lang=kn", ":lang=ml",
        ":lang=gu", ":lang=pa", ":lang=or", ":lang=si", ":lang=ar", ":lang=th",
    ] {
        if let Ok(out) = std::process::Command::new("fc-match")
            .args(["-f", "%{file}", query])
            .output()
        {
            if out.status.success() {
                let f = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !f.is_empty() {
                    push(PathBuf::from(f), &mut found);
                }
            }
        }
    }
    // A CJK font for the lazy loader, even when the stems above did not match.
    if let Ok(out) = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", ":lang=ja"])
        .output()
    {
        if out.status.success() {
            let f = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !f.is_empty() {
                push_cjk(PathBuf::from(f), &mut cjk);
            }
        }
    }
    let _ = CJK_FONT_FILES.set(cjk.iter().map(|p| p.to_string_lossy().to_string()).collect());

    for (i, p) in found.iter().enumerate() {
        if let Ok(bytes) = std::fs::read(p) {
            let name = format!("sysfont{i}");
            fonts.font_data.insert(name.clone(), FontData::from_owned(bytes));
            for fam in [FontFamily::Proportional, FontFamily::Monospace] {
                if let Some(f) = fonts.families.get_mut(&fam) {
                    f.push(name.clone());
                }
            }
        }
    }
    // Serif display family for titles (Noto Serif on most distros, DejaVu fallback).
    // Missing glyphs fall through to the default proportional family.
    let tail: Vec<String> = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    for (fam, candidates) in [
        ("rustym-serif", ["NotoSerif-Regular", "NotoSerif-Medium", "DejaVuSerif", "LiberationSerif-Regular", "FreeSerif"].as_slice()),
        ("rustym-serif-italic", ["NotoSerif-Italic", "DejaVuSerif-Italic", "LiberationSerif-Italic"].as_slice()),
    ] {
        let mut loaded: Vec<String> = Vec::new();
        for stem in candidates {
            for d in dirs.iter().chain(["/usr/share/fonts/dejavu", "/usr/share/fonts/truetype/dejavu", "/usr/share/fonts/truetype/liberation"].iter()) {
                let p = Path::new(d).join(format!("{stem}.ttf"));
                if p.is_file() {
                    if let Ok(bytes) = std::fs::read(&p) {
                        let name = format!("{stem}-display");
                        fonts.font_data.insert(name.clone(), FontData::from_owned(bytes));
                        loaded.push(name);
                        break;
                    }
                }
            }
            if !loaded.is_empty() {
                break;
            }
        }
        if !loaded.is_empty() {
            let mut list = loaded;
            list.extend(tail.iter().cloned());
            fonts.families.insert(FontFamily::Name(fam.into()), list);
        }
    }
    // Last-resort fallback bundled in the binary, so Bengali always renders even when
    // no system Bengali font is installed. System fonts above take priority.
    fonts.font_data.insert(
        "bundled-bengali".to_owned(),
        FontData::from_static(include_bytes!("../assets/fonts/FreeSans-Bengali-subset.ttf")),
    );
    for fam in [FontFamily::Proportional, FontFamily::Monospace] {
        if let Some(f) = fonts.families.get_mut(&fam) {
            f.push("bundled-bengali".to_owned());
        }
    }
    eprintln!("[rustym] fonts: {} system fallback fonts loaded: {:?}", found.len(), found);
    let _ = BASE_FONTS.set(fonts.clone());
    ctx.set_fonts(fonts);
}

/// CJK codepoints that need the heavy NotoSansCJK fallback. That .ttc is
/// ~19 MB, so it is only loaded once CJK characters actually appear in the
/// displayed text (titles, artists, lyrics, account names…).
fn font_cjk_needed(s: &str) -> bool {
    s.chars().any(|c| {
        let u = c as u32;
        (0x2E80..=0x9FFF).contains(&u) // CJK radicals, kana, ideographs
            || (0xAC00..=0xD7AF).contains(&u) // Hangul syllables
            || (0xF900..=0xFAFF).contains(&u) // CJK compatibility ideographs
            || (0xFF00..=0xFFEF).contains(&u) // fullwidth forms (＋, 　, …)
    })
}

static CJK_FONT_FILES: OnceLock<Vec<String>> = OnceLock::new();
static CJK_FONTS_LOADED: AtomicBool = AtomicBool::new(false);
/// Font definitions without the heavy CJK fallbacks (the startup set).
static BASE_FONTS: OnceLock<FontDefinitions> = OnceLock::new();

/// Registers the discovered CJK fallback fonts (once). Rebuilding the font
/// definitions is safe any time; egui just rebuilds its glyph atlas.
fn load_cjk_fonts(ctx: &egui::Context) {
    use std::path::Path;
    if CJK_FONTS_LOADED.swap(true, SeqCst) {
        return;
    }
    let Some(paths) = CJK_FONT_FILES.get() else { return };
    let Some(base) = BASE_FONTS.get() else { return };
    let mut fonts = base.clone();
    for (i, p) in paths.iter().enumerate() {
        if let Ok(bytes) = std::fs::read(Path::new(p)) {
            let name = format!("sysfont-cjk-{i}");
            fonts.font_data.insert(name.clone(), FontData::from_owned(bytes));
            for fam in [FontFamily::Proportional, FontFamily::Monospace] {
                if let Some(f) = fonts.families.get_mut(&fam) {
                    f.push(name.clone());
                }
            }
        }
    }
    ctx.set_fonts(fonts);
    log(0, "fonts", "CJK fallback fonts loaded");
}

fn fake_levels(t: f32) -> Vec<f32> {
    (0..BANDS)
        .map(|i| {
            let x = i as f32;
            let v = 0.38 + 0.34 * (t * 2.3 + x * 0.35).sin() * (t * 1.4 + x * 0.12).sin();
            (v * (1.0 - x / BANDS as f32 * 0.5)).clamp(0.04, 1.0)
        })
        .collect()
}

fn draw_viz(ui: &mut egui::Ui, th: Theme, lv: &[f32], style: usize, size: Vec2) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter_at(rect);
    let n = lv.len();
    match style {
        0 => {
            let gap = 4.0;
            let bw = ((rect.width() - gap * (n as f32 - 1.0)) / n as f32).max(1.0);
            for (i, &v) in lv.iter().enumerate() {
                let h = (v * rect.height()).max(3.0);
                let x = rect.left() + i as f32 * (bw + gap);
                let r = Rect::from_min_max(Pos2::new(x, rect.bottom() - h), Pos2::new(x + bw, rect.bottom()));
                p.rect_filled(r, bw / 2.0, th.accent.gamma_multiply(0.45 + 0.55 * v));
            }
        }
        1 => {
            let gap = 4.0;
            let bw = ((rect.width() - gap * (n as f32 - 1.0)) / n as f32).max(1.0);
            let mid = rect.center().y;
            for (i, &v) in lv.iter().enumerate() {
                let h = (v * rect.height() * 0.5).max(2.0);
                let x = rect.left() + i as f32 * (bw + gap);
                let r = Rect::from_min_max(Pos2::new(x, mid - h), Pos2::new(x + bw, mid + h));
                p.rect_filled(r, bw / 2.0, th.accent.gamma_multiply(0.45 + 0.55 * v));
            }
        }
        _ => {
            let c = rect.center();
            let m = rect.height().min(rect.width());
            let r0 = m * 0.22;
            let rmax = m * 0.5 - 4.0;
            let spokes = 96usize;
            for i in 0..spokes {
                let k = if i < 48 { i } else { 95 - i };
                let v = lv[(k * n / 48).min(n - 1)];
                let a = i as f32 / spokes as f32 * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
                let (s, co) = a.sin_cos();
                let r1 = r0 + 3.0 + v * (rmax - r0);
                p.line_segment(
                    [Pos2::new(c.x + co * r0, c.y + s * r0), Pos2::new(c.x + co * r1, c.y + s * r1)],
                    Stroke::new(3.0_f32, th.accent.gamma_multiply(0.4 + 0.6 * v)),
                );
            }
            p.circle_stroke(c, r0 - 2.0, Stroke::new(2.0_f32, th.card2));
        }
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// One song row. Left click plays, "+" adds to a playlist, right click opens the song menu.
#[allow(clippy::too_many_arguments)]
fn track_row(
    ui: &mut egui::Ui,
    th: Theme,
    t: &Track,
    i: usize,
    salt: &str,
    cur: Option<&str>,
    removable: bool,
    liked: &HashSet<String>,
    act: &mut Option<Act>,
) {
    let row_h = 58.0;
    ui.push_id((salt, i), |ui| {
        let playing = cur == Some(t.id.as_str());
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::click());
        let hovered = resp.hovered();
        let p = ui.painter();
        if playing {
            p.rect_filled(rect, 10.0, th.accent.gamma_multiply(0.12));
        } else if hovered {
            p.rect_filled(rect, 10.0, th.card);
        }
        let img = Rect::from_min_size(rect.min + Vec2::new(8.0, 6.5), Vec2::new(45.0, 45.0));
        thumb(ui, &t.id, img, 8.0);
        let tx = img.right() + 14.0;
        let clip = Rect::from_min_max(Pos2::new(tx, rect.top()), Pos2::new(rect.right() - 116.0, rect.bottom()));
        let pc = p.with_clip_rect(clip);
        pc.text(
            Pos2::new(tx, rect.center().y - 10.0),
            Align2::LEFT_CENTER,
            &t.title,
            FontId::proportional(15.0),
            if playing { th.accent } else { th.text },
        );
        let sub = if t.album.is_empty() { t.artist.clone() } else { format!("{} · {}", t.artist, t.album) };
        pc.text(Pos2::new(tx, rect.center().y + 11.0), Align2::LEFT_CENTER, sub, FontId::proportional(13.0), th.dim);
        if let Some(d) = t.dur {
            p.text(Pos2::new(rect.right() - 14.0, rect.center().y), Align2::RIGHT_CENTER, fmt_time(d), FontId::proportional(13.0), th.dim);
        }
        // + (add to playlist) or x (remove) button
        let bc = Pos2::new(rect.right() - 76.0, rect.center().y);
        let br = Rect::from_center_size(bc, Vec2::splat(30.0));
        let bresp = ui.interact(br, ui.make_persistent_id(("rowbtn", i)), Sense::click());
        let col = if bresp.hovered() { th.accent } else if hovered { th.dim2 } else { th.dim };
        let st = Stroke::new(2.0_f32, col);
        let p = ui.painter();
        if removable {
            p.line_segment([bc + Vec2::new(-5.0, -5.0), bc + Vec2::new(5.0, 5.0)], st);
            p.line_segment([bc + Vec2::new(-5.0, 5.0), bc + Vec2::new(5.0, -5.0)], st);
        } else {
            p.line_segment([bc + Vec2::new(-6.0, 0.0), bc + Vec2::new(6.0, 0.0)], st);
            p.line_segment([bc + Vec2::new(0.0, -6.0), bc + Vec2::new(0.0, 6.0)], st);
        }
        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if bresp.clicked() {
            *act = Some(if removable { Act::Remove(i) } else { Act::Add(i) });
        } else if resp.clicked() {
            *act = Some(Act::Play(i));
        }
        let is_liked = liked.contains(&t.id);
        let local = !t.local.is_empty();
        resp.context_menu(|ui| {
            if ui.button("Play next").clicked() {
                *act = Some(Act::PlayNext(i));
                ui.close_menu();
            }
            if ui.button("Add to queue").clicked() {
                *act = Some(Act::Enqueue(i));
                ui.close_menu();
            }
            if ui.button("Add to playlist…").clicked() {
                *act = Some(Act::Add(i));
                ui.close_menu();
            }
            if !local {
                if ui.button(if is_liked { "Unlike" } else { "Like" }).clicked() {
                    *act = Some(Act::Like(i));
                    ui.close_menu();
                }
                if ui.button("Start radio").clicked() {
                    *act = Some(Act::Radio(i));
                    ui.close_menu();
                }
                if !t.artist_id.is_empty() && ui.button("Go to artist").clicked() {
                    *act = Some(Act::ToArtist(i));
                    ui.close_menu();
                }
                if !t.album_id.is_empty() && ui.button("Go to album").clicked() {
                    *act = Some(Act::ToAlbum(i));
                    ui.close_menu();
                }
            }
            if removable && ui.button("Remove from this playlist").clicked() {
                *act = Some(Act::Remove(i));
                ui.close_menu();
            }
        });
    });
}

fn draw_tracks(ui: &mut egui::Ui, th: Theme, tracks: &[Track], cur: Option<&str>, removable: bool, liked: &HashSet<String>) -> Option<Act> {
    let row_h = 58.0;
    let mut act: Option<Act> = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, row_h, tracks.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for i in range {
                track_row(ui, th, &tracks[i], i, "tr", cur, removable, liked, &mut act);
            }
        });
    act
}

// Create/remove a standard XDG autostart entry for Linux desktop sessions.
fn set_desktop_autostart(enabled: bool) -> std::io::Result<()> {
    let base = match std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        Some(v) => std::path::PathBuf::from(v),
        None => std::env::var_os("HOME")
            .map(|h| std::path::PathBuf::from(h).join(".config"))
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "HOME is not set"))?,
    };
    let dir = base.join("autostart");
    let file = dir.join("rustym.desktop");
    if enabled {
        std::fs::create_dir_all(&dir)?;
        let exe = std::env::current_exe()?;
        let escaped = exe.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
        // X-GNOME / X-KDE / X-MATE keys make the entry honoured by every common desktop.
        std::fs::write(
            file,
            format!(
                "[Desktop Entry]\nType=Application\nName=Rustym\nComment=Ad-free YouTube Music\nExec=\"{escaped}\" --show\nIcon=rustym\nTerminal=false\nX-GNOME-Autostart-enabled=true\nX-KDE-autostart-after=panel\nX-MATE-Autostart-enabled=true\n"
            ),
        )?;
    } else if file.exists() {
        std::fs::remove_file(file)?;
    }
    Ok(())
}

// ---------------- app ----------------

struct Rustym {
    cfg: Config,
    theme: Theme,
    cmd: Sender<Cmd>,
    shared: Arc<Mutex<Shared>>,
    viz: Option<Arc<Mutex<VizData>>>,
    snap: Shared,
    query: String,
    label: String,
    status: String,
    view: View,
    tracks: Vec<Track>,
    loading: bool,
    login_waiting: bool,
    login_msg: String,
    login_cancel: Arc<AtomicBool>,
    req: u64,
    trending: Vec<Track>,
    trending_loading: bool,
    trending_err: String,
    mix_cache: HashMap<usize, Vec<Track>>,
    remote: Vec<RemotePl>,
    remote_loading: bool,
    remote_err: String,
    remote_cache: HashMap<String, Vec<Track>>,
    greeting: String,
    drag: Option<f32>,
    focus_once: bool,
    new_pl: String,
    adding: Option<Track>,
    disp: Vec<f32>,
    t0: Instant,
    /// Bounded decoded-image cache (see imgcache.rs).
    img: std::sync::Arc<imgcache::BoundedImageLoader>,
    /// Last time the image-cache janitor ran.
    img_t: Instant,
    /// Cached logs page snapshot (avoids cloning the log vec every frame).
    logs_cache: (Instant, std::sync::Arc<Vec<LogEntry>>),
    msg_tx: Sender<Msg>,
    msg_rx: Receiver<Msg>,
    // ---- v0.4 ----
    shelves: Vec<ytm::Shelf>,
    home_loading: bool,
    sres: ytm::SearchOut,
    sfilter: usize,
    suggestions: Vec<String>,
    sug_req: u64,
    sug_dirty: bool,
    sug_t: Instant,
    show_sug: bool,
    detail: Option<ytm::Detail>,
    detail_kind: ytm::Kind,
    detail_id: String,
    back: Vec<View>,
    lib_albums: Vec<ytm::Card>,
    lib_artists: Vec<ytm::Card>,
    lib_loading: bool,
    lib_err: String,
    history: Vec<(String, Vec<Track>)>,
    hist_loading: bool,
    hist_err: String,
    hist_local: bool,
    lyrics_for: String,
    lyrics: Option<lyrics::Lyrics>,
    lyrics_loading: bool,
    lyrics_err: String,
    lyric_cur: Option<usize>,
    liked_ids: HashSet<String>,
    toast: Option<(String, Instant)>,
    local_tracks: Vec<Track>,
    local_input: String,
    /// Settings > Dependencies: detected versions + paths of the runtime deps.
    deps_info: Vec<engine::DepRow>,
    deps_loaded: bool,
    deps_busy: bool,
    deps_report: String,
    show_queue: bool,
    fx_open: bool,
    help_open: bool,
    open_detail_later: Option<(ytm::Kind, String)>,
    /// Whether lyrics are enabled in Now Playing; remains on as the queue advances.
    lyrics_enabled: bool,
    /// Whether the Now Playing right pane shows the vinyl player (a ring button toggles it).
    vinyl_open: bool,
    /// Spotify playlist link typed into the import box.
    spotify_url: String,
    /// Last time listening stats were re-read from disk (the background process writes them).
    store_t: Instant,
    tray_ok: bool,
    tray_checked: Instant,
    last_current_id: String,
    /// Set when the track changes or text is typed: run the lazy CJK font check.
    cjk_check: bool,
}

impl Rustym {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let cfg = Config::load();
        let theme = THEMES[cfg.theme.min(THEMES.len() - 1)];
        egui_extras::install_image_loaders(&cc.egui_ctx);
        // Our bounded loader is tried first for decoded images (LRU + size-aware
        // decode); egui_extras' loader stays installed as the fallback/bytes fetcher.
        let img = imgcache::BoundedImageLoader::install(&cc.egui_ctx);
        setup_fonts(&cc.egui_ctx);
        apply_theme(&cc.egui_ctx, &theme);
        cc.egui_ctx.set_zoom_factor((cfg.ui_scale / 100.0).clamp(0.80, 1.25));

        let c2 = cc.egui_ctx.clone();
        let repaint: Repaint = Arc::new(move || c2.request_repaint());
        // This is the window process. The engine, tray icon and media keys live in the background
        // process; the window just talks to it, so closing the window never interrupts playback.
        let (cmd, shared) = connect_backend(repaint.clone());
        // Only capture audio (parec) when the visualiser is actually shown.
        let viz = if cfg.show_visualiser { Some(spawn_viz()) } else { None };

        let (msg_tx, msg_rx) = channel();
        let trending = cfg.trending.clone();
        let remote = cfg.remote.clone();
        let signed = cfg.signed_in;
        let auto_show_lyrics = cfg.auto_show_lyrics;
        let mut app = Rustym {
            cfg,
            theme,
            cmd,
            shared,
            viz,
            img,
            img_t: Instant::now(),
            logs_cache: (Instant::now(), Arc::new(Vec::new())),
            snap: Shared::default(),
            query: String::new(),
            label: "Home".into(),
            status: String::new(),
            view: View::Home,
            tracks: vec![],
            loading: false,
            login_waiting: false,
            login_msg: String::new(),
            login_cancel: Arc::new(AtomicBool::new(false)),
            req: 0,
            trending,
            trending_loading: false,
            trending_err: String::new(),
            mix_cache: HashMap::new(),
            remote,
            remote_loading: false,
            remote_err: String::new(),
            remote_cache: HashMap::new(),
            greeting: greeting(),
            drag: None,
            focus_once: false,
            new_pl: String::new(),
            adding: None,
            disp: vec![0.0; BANDS],
            t0: Instant::now(),
            msg_tx,
            msg_rx,
            shelves: vec![],
            home_loading: false,
            sres: ytm::SearchOut::default(),
            sfilter: 0,
            suggestions: vec![],
            sug_req: 0,
            sug_dirty: false,
            sug_t: Instant::now(),
            show_sug: false,
            detail: None,
            detail_kind: ytm::Kind::Album,
            detail_id: String::new(),
            back: vec![],
            lib_albums: vec![],
            lib_artists: vec![],
            lib_loading: false,
            lib_err: String::new(),
            history: vec![],
            hist_loading: false,
            hist_err: String::new(),
            hist_local: false,
            lyrics_for: String::new(),
            lyrics: None,
            lyrics_loading: false,
            lyrics_err: String::new(),
            lyric_cur: None,
            // saved liked songs make the Like buttons right before the network answers
            liked_ids: if signed { liked_cache_load().into_iter().map(|t| t.id).collect() } else { HashSet::new() },
            toast: None,
            local_tracks: vec![],
            local_input: String::new(),
            deps_info: vec![],
            deps_loaded: false,
            deps_busy: false,
            deps_report: String::new(),
            show_queue: false,
            fx_open: false,
            help_open: false,
            open_detail_later: None,
            lyrics_enabled: auto_show_lyrics,
            store_t: Instant::now().checked_sub(Duration::from_secs(10)).unwrap_or_else(Instant::now),
            vinyl_open: false,
            spotify_url: String::new(),
            tray_ok: true,
            tray_checked: Instant::now(),
            last_current_id: String::new(),
            cjk_check: true,
        };
        let _ = app.cmd.send(Cmd::Shuffle(app.cfg.shuffle));
        let _ = app.cmd.send(Cmd::SetRepeat(app.cfg.repeat));
        let _ = app.cmd.send(Cmd::Autoplay(app.cfg.autoplay));
        let _ = app.cmd.send(Cmd::SetFx(app.cfg.fx.clone()));
        // Restore the preferred start page from Settings.
        match app.cfg.start_view {
            1 => {
                app.view = View::Search;
                app.label = "Search".into();
                app.focus_once = true;
            }
            2 => {
                app.view = View::Liked;
                app.label = "Liked songs".into();
                app.load_liked(&cc.egui_ctx);
            }
            _ => {}
        }
        app.fetch_home(&cc.egui_ctx);
        if signed {
            app.fetch_liked_ids(&cc.egui_ctx);
            app.fetch_account(&cc.egui_ctx);
        }
        app.fetch_trending(&cc.egui_ctx);
        if signed {
            app.fetch_remote(&cc.egui_ctx);
        }
        app
    }

    // ----- actions -----

    fn fetch_trending(&mut self, ctx: &egui::Context) {
        if self.trending_loading {
            return;
        }
        self.trending_loading = true;
        self.trending_err.clear();
        let tx = self.msg_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let args: Vec<String> = vec![
                "--flat-playlist".into(),
                "--dump-json".into(),
                "--no-warnings".into(),
                "ytsearch16:top songs this week official audio".into(),
            ];
            let native = ytm::search("top songs this week", 1).map(|o| o.songs);
            let m = match native.or_else(|_| run_ytdlp(args)) {
                Ok(t) => Msg::Trending(t),
                Err(e) => Msg::TrendingErr(e),
            };
            let _ = tx.send(m);
            ctx.request_repaint();
        });
    }

    fn fetch_remote(&mut self, ctx: &egui::Context) {
        if self.remote_loading || !self.cfg.signed_in {
            return;
        }
        self.remote_loading = true;
        self.remote_err.clear();
        let auth = auth_args(&self.cfg);
        let mk = |url: &str| -> Vec<String> {
            let mut a = auth.clone();
            a.extend(["--flat-playlist", "--dump-json", "--no-warnings", "--socket-timeout", "15", url].map(String::from));
            a
        };
        let chains = vec![
            mk("https://www.youtube.com/feed/playlists"),
            mk("https://www.youtube.com/feed/library"),
        ];
        let tx = self.msg_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let m = match api_playlists().or_else(|e1| run_playlists(chains).map_err(|e2| format!("{e1} | yt-dlp: {e2}"))) {
                Ok(v) => Msg::Remote(v),
                Err(e) => Msg::RemoteErr(e),
            };
            let _ = tx.send(m);
            ctx.request_repaint();
        });
    }

    fn open_remote(&mut self, ctx: &egui::Context, i: usize) {
        let Some(r) = self.remote.get(i).cloned() else { return };
        self.view = View::Remote(i);
        self.status.clear();
        self.req += 1;
        self.loading = false;
        self.label = r.title.clone();
        if let Some(t) = self.remote_cache.get(&r.id) {
            self.tracks = t.clone();
            return;
        }
        self.tracks.clear();
        let mut args = auth_args(&self.cfg);
        args.extend(
            [
                "--flat-playlist",
                "--dump-json",
                "--no-warnings",
                "--playlist-end",
                "300",
                "--socket-timeout",
                "15",
            ]
            .map(String::from),
        );
        args.push(format!("https://www.youtube.com/playlist?list={}", r.id));
        self.start_api(ctx, r.title, "playlist", r.id, vec![args]);
    }

    fn record_recent(&mut self, t: Track) {
        self.cfg.recent.retain(|x| x.id != t.id);
        self.cfg.recent.insert(0, t);
        self.cfg.recent.truncate(12);
        self.cfg.save();
    }

    fn open_mix(&mut self, ctx: &egui::Context, i: usize) {
        let m = &MIXES[i];
        self.view = View::Mix(i);
        self.status.clear();
        self.req += 1;
        self.loading = false;
        self.label = m.title.to_string();
        if let Some(t) = self.mix_cache.get(&i) {
            self.tracks = t.clone();
            return;
        }
        self.tracks.clear();
        self.start_query(ctx, m.title.to_string(), m.query.to_string());
    }

    fn sign_out(&mut self) {
        self.cfg.signed_in = false;
        self.cfg.account_name.clear();
        self.cfg.account_photo.clear();
        forget_login();
        self.remote.clear();
        self.cfg.remote.clear();
        self.remote_cache.clear();
        liked_cache_clear();
        self.liked_ids.clear();
        self.cfg.save();
        if self.view == View::Liked {
            self.tracks.clear();
        }
    }

    fn cancel_login(&mut self) {
        self.login_cancel.store(true, SeqCst);
        self.login_waiting = false;
    }

    fn use_cookie_file(&mut self, ctx: &egui::Context) {
        if self.login_waiting {
            return;
        }
        self.login_waiting = true;
        self.login_msg.clear();
        let tx = self.msg_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let m = match check_cookie_file() {
                Ok(()) => Msg::LoginOk(None),
                Err(e) => Msg::LoginFail(e),
            };
            let _ = tx.send(m);
            ctx.request_repaint();
        });
    }

    /// Fetches tracks from the YouTube Music API, falling back to yt-dlp (`chains`).
    fn start_api(&mut self, ctx: &egui::Context, label: String, mode: &'static str, arg: String, chains: Vec<Vec<String>>) {
        self.loading = true;
        self.status.clear();
        self.label = label;
        self.req += 1;
        let id = self.req;
        let tx = self.msg_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let r = api_tracks(mode, &arg).or_else(|e1| run_chain(chains).map_err(|e2| format!("{e1} | yt-dlp: {e2}")));
            let m = match r {
                Ok(t) => Msg::Results(id, t),
                Err(e) => Msg::Error(id, e),
            };
            let _ = tx.send(m);
            ctx.request_repaint();
        });
    }

    fn load_liked(&mut self, ctx: &egui::Context) {
        if !self.cfg.signed_in {
            self.tracks.clear();
            return;
        }
        let auth = auth_args(&self.cfg);
        let mk = |url: &str| -> Vec<String> {
            let mut a = auth.clone();
            a.extend(["--flat-playlist", "--dump-json", "--no-warnings", "--playlist-end", "200", "--socket-timeout", "15", url].map(String::from));
            a
        };
        // Liked Music first, then plain "Liked videos" as a fallback.
        let chains = vec![
            mk("https://www.youtube.com/playlist?list=LM"),
            mk("https://music.youtube.com/playlist?list=LM"),
            mk("https://www.youtube.com/playlist?list=LL"),
        ];
        self.start_api(ctx, "Liked songs".into(), "liked", String::new(), chains);
        // Saved copy first: the page opens instantly (and works offline); the fetch above then
        // refreshes it silently and updates the saved copy.
        let saved = liked_cache_load();
        if !saved.is_empty() {
            self.tracks = saved;
        }
    }

    fn start_login(&mut self, ctx: &egui::Context) {
        if self.login_waiting {
            return;
        }
        self.login_waiting = true;
        self.login_msg.clear();
        let cancel = Arc::new(AtomicBool::new(false));
        self.login_cancel = cancel.clone();
        if let Err(e) = open_login() {
            self.login_waiting = false;
            self.login_msg = e;
            return;
        }
        let tx = self.msg_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let tp = tx.clone();
            let cp = ctx.clone();
            let progress = move |t: String| {
                let _ = tp.send(Msg::LoginProgress(t));
                cp.request_repaint();
            };
            let m = match wait_for_login(&cancel, &progress) {
                Ok(()) => Msg::LoginOk(None),
                Err(e) => Msg::LoginFail(e),
            };
            let _ = tx.send(m);
            ctx.request_repaint();
        });
    }

    fn go(&mut self, ctx: &egui::Context, v: View) {
        self.view = v;
        self.status.clear();
        self.req += 1;
        self.loading = false;
        if v == View::Settings {
            self.deps_loaded = false; // re-detect versions on entry
        }
        match v {
            View::Home => self.label = "Home".into(),
            View::Playlists => {
                if self.cfg.signed_in && self.remote.is_empty() && self.remote_err.is_empty() {
                    self.fetch_remote(ctx);
                }
            }
            View::Search => {
                self.label = "Search".into();
                self.tracks.clear();
                self.focus_once = true;
            }
            View::Liked => {
                self.label = "Liked songs".into();
                self.tracks.clear();
                self.load_liked(ctx);
            }
            View::Albums => {
                self.label = "Albums".into();
                if self.lib_albums.is_empty() {
                    self.fetch_cards(ctx, 0);
                }
            }
            View::Artists => {
                self.label = "Artists".into();
                if self.lib_artists.is_empty() {
                    self.fetch_cards(ctx, 1);
                }
            }
            View::History => {
                self.label = "History".into();
                self.fetch_history(ctx);
            }
            View::Stats => self.label = "Listening stats".into(),
            View::Local => {
                self.label = "Local files".into();
                if self.local_tracks.is_empty() {
                    self.scan_local(ctx);
                }
            }
            View::Logs => {
                self.label = "Debug logs".into();
                if health_snapshot().is_empty() && !health_busy() {
                    run_health_checks(false);
                }
            }
            _ => {}
        }
    }

    fn pump(&mut self, ctx: &egui::Context) {
        while let Ok(m) = self.msg_rx.try_recv() {
            match m {
                Msg::Results(id, t) => {
                    if id != self.req {
                        continue;
                    }
                    self.loading = false;
                    if let View::Mix(i) = self.view {
                        self.mix_cache.insert(i, t.clone());
                    }
                    if let View::Remote(i) = self.view {
                        if let Some(r) = self.remote.get(i) {
                            self.remote_cache.insert(r.id.clone(), t.clone());
                        }
                    }
                    if self.view == View::Liked {
                        self.liked_ids = t.iter().map(|x| x.id.clone()).collect();
                        liked_cache_save(&t);
                    }
                    self.tracks = t;
                }
                Msg::Error(id, e) => {
                    log(2, "fetch", format!("{}: {e}", self.label));
                    if id != self.req {
                        continue;
                    }
                    self.loading = false;
                    if self.view == View::Liked && !self.tracks.is_empty() {
                        // keep the saved copy on screen instead of an empty page
                        self.status = "Couldn’t refresh liked songs right now, so this is your saved copy.".into();
                    } else {
                        self.tracks.clear();
                        self.status = if self.view == View::Liked {
                            format!("Couldn’t load liked songs ({e}). Try signing out and in again, and keep yt-dlp up to date (run: yt-dlp -U, or update it with your package manager).")
                        } else {
                            e
                        };
                    }
                }
                Msg::Trending(t) => {
                    self.trending_loading = false;
                    self.cfg.trending = t.clone();
                    self.trending = t;
                    self.cfg.save();
                }
                Msg::Remote(v) => {
                    self.remote_loading = false;
                    self.cfg.remote = v.clone();
                    self.remote = v;
                    self.cfg.save();
                }
                Msg::RemoteErr(e) => {
                    self.remote_loading = false;
                    log(2, "playlists", &e);
                    self.remote_err = e;
                }
                Msg::TrendingErr(e) => {
                    self.trending_loading = false;
                    log(1, "home", &e);
                    self.trending_err = e;
                }
                Msg::LoginOk(b) => {
                    self.login_waiting = false;
                    self.login_msg.clear();
                    let _ = b;
                    self.cfg.signed_in = true;
                    self.cfg.save();
                    self.mix_cache.clear();
                    self.remote_err.clear();
                    self.fetch_remote(ctx);
                    self.fetch_home(ctx);
                    self.fetch_liked_ids(ctx);
                    self.fetch_account(ctx);
                    if self.view == View::Liked {
                        self.load_liked(ctx);
                    }
                }
                Msg::LoginProgress(t) => {
                    if self.login_waiting {
                        self.login_msg = t;
                    }
                }
                Msg::Home(v) => {
                    self.home_loading = false;
                    self.shelves = v;
                }
                Msg::HomeErr(e) => {
                    self.home_loading = false;
                    log(1, "home", &e);
                }
                Msg::SearchRes(id, o) => {
                    if id == self.req {
                        self.loading = false;
                        self.tracks = o.songs.clone();
                        self.sres = o;
                    }
                }
                Msg::Suggest(id, v) => {
                    if id == self.sug_req {
                        self.suggestions = v;
                    }
                }
                Msg::Detail(id, d) => {
                    if id == self.req {
                        self.loading = false;
                        self.label = d.title.clone();
                        self.tracks = d.tracks.clone();
                        self.detail = Some(d);
                    }
                }
                Msg::DetailErr(id, e) => {
                    log(2, "detail", &e);
                    if id == self.req {
                        self.loading = false;
                        self.status = e;
                    }
                }
                Msg::Cards(k, v) => {
                    self.lib_loading = false;
                    if k == 0 {
                        self.lib_albums = v;
                    } else {
                        self.lib_artists = v;
                    }
                }
                Msg::CardsErr(kind, e) => {
                    self.lib_loading = false;
                    log(2, "library", &e);
                    let section = if kind == 0 { "Albums" } else { "Artists" };
                    self.lib_err = format!("{section}: {e}");
                }
                Msg::History(h) => {
                    self.hist_loading = false;
                    self.history = h;
                }
                Msg::HistoryErr(e) => {
                    self.hist_loading = false;
                    log(1, "history", &e);
                    self.hist_err = e;
                }
                Msg::Lyrics(id, l) => {
                    if id == self.lyrics_for {
                        self.lyrics_loading = false;
                        self.lyrics = Some(l);
                    }
                }
                Msg::LyricsErr(id, e) => {
                    if id == self.lyrics_for {
                        self.lyrics_loading = false;
                        self.lyrics_err = e;
                    }
                }
                Msg::LikedIds(v) => self.liked_ids = v.into_iter().collect(),
                Msg::Account(n, ph) => {
                    if !self.cfg.signed_in {
                        continue;
                    }
                    if !n.is_empty() {
                        self.cfg.account_name = n;
                    }
                    if !ph.is_empty() {
                        self.cfg.account_photo = ph;
                    }
                    self.cfg.save();
                }
                Msg::Toast(t) => self.toast = Some((t, Instant::now())),
                Msg::Deps(report) => {
                    self.deps_busy = false;
                    self.deps_report = report;
                    self.deps_info = engine::dep_report();
                }
                Msg::SpotifyImported(name, tracks) => {
                    if tracks.is_empty() {
                        self.toast = Some(("No Spotify tracks could be matched".into(), Instant::now()));
                    } else {
                        let n = tracks.len();
                        self.cfg.playlists.push(Playlist { name: name.clone(), tracks });
                        self.cfg.save();
                        self.toast = Some((format!("Imported “{name}” with {n} tracks"), Instant::now()));
                    }
                }
                Msg::PlayNow(v) => {
                    if let Some(t) = v.first().cloned() {
                        self.record_recent(t);
                        let _ = self.cmd.send(Cmd::Play(v, 0));
                        self.go_now();
                    }
                }
                Msg::Local(v) => {
                    self.loading = false;
                    if self.view == View::Local {
                        self.tracks = v.clone();
                    }
                    self.local_tracks = v;
                }
                Msg::LibraryChanged => {
                    self.remote_loading = false;
                    self.fetch_remote(ctx);
                }
                Msg::LoginFail(e) => {
                    self.login_waiting = false;
                    if e != "Cancelled." {
                        log(2, "login", &e);
                        self.login_msg = format!("Sign-in not detected. {e}");
                    }
                }
            }
        }
    }

    fn update_levels(&mut self) {
        let playing = self.snap.playing && !self.snap.paused;
        let target: Vec<f32> = match &self.viz {
            Some(v) => {
                let v = v.lock().unwrap();
                if v.live && v.updated.elapsed() < Duration::from_millis(1200) {
                    v.levels.clone()
                } else if playing {
                    fake_levels(self.t0.elapsed().as_secs_f32())
                } else {
                    vec![0.0; BANDS]
                }
            }
            // Visualiser is off: skip the capture thread entirely (no parec),
            // only animate the fake bars while music is playing.
            None if playing => fake_levels(self.t0.elapsed().as_secs_f32()),
            None => Vec::new(),
        };
        if target.is_empty() {
            for d in self.disp.iter_mut() {
                *d *= 0.88;
            }
            return;
        }
        for i in 0..BANDS {
            let t = target[i];
            let d = &mut self.disp[i];
            *d = if t > *d { *d * 0.35 + t * 0.65 } else { *d * 0.88 };
        }
    }

    fn go_now(&mut self) {
        self.view = View::Now;
        self.label = "Now playing".into();
        self.status.clear();
        self.back.clear();
    }

    fn handle_act(&mut self, act: Option<Act>, src: Option<usize>) {
        if act.is_none() {
            return;
        }
        let list: Vec<Track> = match src {
            Some(p) => self.cfg.playlists.get(p).map(|p| p.tracks.clone()).unwrap_or_default(),
            None => self.tracks.clone(),
        };
        match act {
            Some(Act::Play(i)) => {
                if let Some(t) = list.get(i).cloned() {
                    self.record_recent(t);
                }
                let _ = self.cmd.send(Cmd::Play(list, i));
                self.go_now();
            }
            Some(Act::Add(i)) => self.adding = list.get(i).cloned(),
            Some(Act::PlayNext(i)) => {
                if let Some(t) = list.get(i).cloned() {
                    let _ = self.cmd.send(Cmd::PlayNext(vec![t]));
                    self.toast = Some(("Will play next".into(), Instant::now()));
                }
            }
            Some(Act::Enqueue(i)) => {
                if let Some(t) = list.get(i).cloned() {
                    let _ = self.cmd.send(Cmd::Enqueue(vec![t]));
                    self.toast = Some(("Added to queue".into(), Instant::now()));
                }
            }
            Some(Act::Like(i)) => {
                if let Some(t) = list.get(i).cloned() {
                    self.toggle_like(&t);
                }
            }
            Some(Act::Radio(i)) => {
                if let Some(t) = list.get(i).cloned() {
                    self.start_radio(&t.id);
                }
            }
            Some(Act::ToArtist(i)) => {
                if let Some(t) = list.get(i) {
                    let id = t.artist_id.clone();
                    self.open_detail_later = Some((ytm::Kind::Artist, id));
                }
            }
            Some(Act::ToAlbum(i)) => {
                if let Some(t) = list.get(i) {
                    let id = t.album_id.clone();
                    self.open_detail_later = Some((ytm::Kind::Album, id));
                }
            }
            Some(Act::Remove(i)) => {
                if let Some(p) = src {
                    if let Some(pl) = self.cfg.playlists.get_mut(p) {
                        if i < pl.tracks.len() {
                            pl.tracks.remove(i);
                        }
                    }
                    self.cfg.save();
                } else if self.yt_removable() {
                    self.remove_from_yt(i);
                }
            }
            None => {}
        }
    }

    fn add_to_playlist(&mut self, pl: usize, t: Track) {
        if let Some(p) = self.cfg.playlists.get_mut(pl) {
            if !p.tracks.iter().any(|x| x.id == t.id) {
                p.tracks.push(t);
            }
        }
        self.cfg.save();
    }

    // ----- panels -----

    fn sidebar(&mut self, ctx: &egui::Context) {
        let th = self.theme;
        egui::SidePanel::left("side")
            .exact_width(214.0)
            .resizable(false)
            .frame(egui::Frame::none().fill(th.panel).inner_margin(Margin::symmetric(14.0, 20.0)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 13.0, th.accent);
                    ui.painter().add(Shape::convex_polygon(
                        vec![
                            Pos2::new(r.center().x - 4.0, r.center().y - 6.0),
                            Pos2::new(r.center().x - 4.0, r.center().y + 6.0),
                            Pos2::new(r.center().x + 6.0, r.center().y),
                        ],
                        // Dark glyph on the accent dot, like the accent play button
                        rgb(20, 15, 11),
                        Stroke::NONE,
                    ));
                    ui.label(RichText::new("R U S T Y M").size(17.5).strong().color(th.text));
                });
                ui.add_space(20.0);

                // Slim navigation: everything else lives as tabs inside the pages
                // (Albums/Artists -> tabs, Stats/Debug logs -> Settings tabs,
                // Now playing -> opens automatically and via the player bar).
                let is_library = matches!(self.view, View::Albums | View::Artists);
                let is_system = matches!(self.view, View::Settings | View::Stats | View::Logs);
                let sections: [(&str, Vec<(&str, bool, Glyph, View)>); 3] = [
                    (
                        "MENU",
                        vec![
                            ("Home", self.view == View::Home || matches!(self.view, View::Mix(_)), Glyph::Home, View::Home),
                            ("Search", self.view == View::Search, Glyph::Search, View::Search),
                        ],
                    ),
                    (
                        "LIBRARY",
                        vec![
                            ("Liked songs", self.view == View::Liked, Glyph::Heart, View::Liked),
                            ("Playlists", self.view == View::Playlists || matches!(self.view, View::Playlist(_) | View::Remote(_)), Glyph::List, View::Playlists),
                            ("Albums & Artists", is_library, Glyph::List, View::Albums),
                            ("History", self.view == View::History, Glyph::Wave, View::History),
                            ("Local files", self.view == View::Local, Glyph::List, View::Local),
                        ],
                    ),
                    (
                        "SYSTEM",
                        vec![("Settings", is_system, Glyph::Gear, View::Settings)],
                    ),
                ];
                let nav_h = (ui.available_height() - 120.0).max(160.0);
                egui::ScrollArea::vertical().max_height(nav_h).auto_shrink([false, true]).show(ui, |ui| {
                    for (section, items) in sections {
                        ui.add_space(4.0);
                        ui.label(RichText::new(section).size(9.5).strong().color(th.dim));
                        ui.add_space(4.0);
                        for (name, active, g, v) in items {
                            if nav_item(ui, th, g, name, active).clicked() {
                                self.back.clear();
                                self.go(ctx, v);
                            }
                        }
                        ui.add_space(6.0);
                    }
                });

                // Your local playlists, right in the sidebar.
                if !self.cfg.playlists.is_empty() {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("YOUR PLAYLISTS").size(9.5).strong().color(th.dim));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.add(egui::Button::new(RichText::new("+").size(14.0).color(th.accent)).frame(false)).clicked() {
                                self.back.clear();
                                self.go(ctx, View::Playlists);
                            }
                        });
                    });
                    ui.add_space(4.0);
                    let max_shown = 5.min(self.cfg.playlists.len());
                    for i in 0..max_shown {
                        let p = &self.cfg.playlists[i];
                        let active = matches!(self.view, View::Playlist(v) if v == i);
                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::click());
                        let k = ui.ctx().animate_bool(resp.id, resp.hovered());
                        if active {
                            ui.painter().rect_filled(rect, 9.0, th.accent.gamma_multiply(0.15));
                        } else if k > 0.01 {
                            ui.painter().rect_filled(rect, 9.0, th.accent.gamma_multiply(0.07 * k));
                        }
                        let img = Rect::from_min_size(Pos2::new(rect.left() + 7.0, rect.center().y - 12.0), Vec2::splat(24.0));
                        match p.tracks.first() {
                            Some(t) => thumb(ui, &t.id, img, 6.0),
                            None => {
                                ui.painter().rect_filled(img, 6.0, th.card2);
                            }
                        }
                        ui.painter().text(
                            Pos2::new(img.right() + 10.0, rect.center().y),
                            Align2::LEFT_CENTER,
                            short_name(&p.name, 18),
                            FontId::proportional(13.5),
                            if active { th.text } else { th.dim2 },
                        );
                        if resp.clicked() {
                            self.back.clear();
                            self.go(ctx, View::Playlist(i));
                        } else if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                    }
                }

                ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                    if self.cfg.signed_in {
                        // One aligned row: avatar | name | sign out (right).
                        let w = ui.available_width();
                        ui.allocate_ui_with_layout(Vec2::new(w, 40.0), Layout::left_to_right(Align::Center), |ui| {
                            account_avatar(ui, th, &self.cfg.account_photo, &self.cfg.account_name, 34.0);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 1.0;
                                let nm = if self.cfg.account_name.is_empty() { "Signed in".to_string() } else { short_name(&self.cfg.account_name, 16) };
                                ui.label(RichText::new(nm).size(13.0).strong().color(th.text));
                                ui.label(RichText::new("● Google account").size(10.5).color(th.accent));
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.add(egui::Button::new(RichText::new("Sign out").size(11.0).color(th.dim)).frame(false)).clicked() {
                                    self.sign_out();
                                }
                            });
                        });
                    } else if self.login_waiting {
                        if ui.add(egui::Button::new(RichText::new("Cancel").size(12.0).color(th.dim)).frame(false)).clicked() {
                            self.cancel_login();
                        }
                        ui.horizontal(|ui| {
                            spin(ui, th);
                            ui.label(RichText::new("Finish signing in in the Rustym window…").size(12.0).color(th.dim));
                        });
                        if !self.login_msg.is_empty() {
                            ui.label(RichText::new(&self.login_msg).size(10.5).color(th.dim));
                        }
                    } else {
                        if ui
                            .add_sized([ui.available_width(), 40.0], accent_button(th, "Sign in with Google"))
                            .clicked()
                        {
                            self.start_login(ctx);
                        }
                        if !self.login_msg.is_empty() {
                            ui.label(RichText::new(&self.login_msg).size(11.0).color(th.accent));
                        }
                    }
                });
            });
    }

    fn player(&mut self, ctx: &egui::Context) {
        let th = self.theme;
        egui::TopBottomPanel::bottom("player")
            .exact_height(96.0)
            .frame(egui::Frame::none().fill(th.panel).inner_margin(Margin::symmetric(24.0, 12.0)))
            .show(ctx, |ui| {
                let top = ui.max_rect().top() - 12.0;
                ui.painter().line_segment(
                    [Pos2::new(ui.max_rect().left() - 24.0, top), Pos2::new(ui.max_rect().right() + 24.0, top)],
                    Stroke::new(1.0_f32, th.card2),
                );
                let full = ui.max_rect();
                let cur = self.snap.current.clone();
                // Centre the controls on the whole app window (sidebar included),
                // not just this panel. The widest centred row (the seek bar) decides
                // how much room the sides get; they shrink on narrow windows.
                let win_cx = ui.ctx().screen_rect().center().x;
                const HALF_ROW: f32 = 200.0; // half of the widest centred row
                let left_w = (win_cx - HALF_ROW - 12.0 - full.left()).clamp(0.0, 300.0);
                let right_w = (full.right() - win_cx - HALF_ROW - 12.0).clamp(0.0, 360.0);
                let cw = ((win_cx - full.left() - left_w - 12.0) * 2.0)
                    .min((full.right() - right_w - 12.0 - win_cx) * 2.0)
                    .clamp(230.0, 480.0);
                let left_rect = Rect::from_min_max(full.left_top(), Pos2::new(full.left() + left_w, full.bottom()));
                let right_rect = Rect::from_min_max(Pos2::new(full.right() - right_w, full.top()), full.right_bottom());
                let center_rect = Rect::from_min_size(Pos2::new(win_cx - cw / 2.0, full.top()), Vec2::new(cw, full.height()));

                // ---- left: artwork, track info and a like heart ----
                ui.allocate_new_ui(UiBuilder::new().max_rect(left_rect), |ui| {
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        if left_w < 150.0 {
                            // No room beside the transport on narrow windows.
                        } else if let Some(t) = &cur {
                            let (r, rr) = ui.allocate_exact_size(Vec2::new(56.0, 56.0), Sense::click());
                            thumb(ui, &t.id, r, 10.0);
                            if rr.clicked() {
                                self.view = View::Now;
                            }
                            ui.add_space(10.0);
                            ui.vertical(|ui| {
                                // Clicking the track info opens the Now Playing page
                                if ui.add(egui::Label::new(RichText::new(&t.title).size(14.5).strong().color(th.text)).wrap_mode(egui::TextWrapMode::Truncate).sense(Sense::click())).clicked() {
                                    self.view = View::Now;
                                    self.label = "Now playing".into();
                                    self.status.clear();
                                    self.back.clear();
                                }
                                if ui.add(egui::Label::new(RichText::new(&t.artist).size(12.5).color(th.dim)).wrap_mode(egui::TextWrapMode::Truncate).sense(Sense::click())).clicked() {
                                    self.view = View::Now;
                                    self.label = "Now playing".into();
                                    self.status.clear();
                                    self.back.clear();
                                }
                            });
                            // like / unlike
                            if t.local.is_empty() {
                                ui.add_space(8.0);
                                let liked = self.liked_ids.contains(&t.id);
                                if icon_btn(ui, th, Icon::Heart, 22.0, false, liked).clicked() {
                                    self.toggle_like(t);
                                }
                            }
                        } else {
                            ui.label(RichText::new("Nothing playing").size(13.0).color(th.dim));
                        }
                    });
                });

                // ---- centre: transport controls above the seek bar, centred on
                // the app window. egui lays row widgets out sequentially, so each
                // row gets an exact-size child that the parent centres for us.
                ui.allocate_new_ui(UiBuilder::new().max_rect(center_rect), |ui| {
                    ui.with_layout(Layout::top_down(Align::Center).with_main_align(Align::Min), |ui| {
                        ui.add_space(2.0);
                        ui.allocate_ui_with_layout(Vec2::new(230.0, 42.0), Layout::left_to_right(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 18.0;
                            if icon_btn(ui, th, Icon::Shuffle, 28.0, false, self.snap.shuffle).clicked() {
                                let on = !self.snap.shuffle;
                                self.cfg.shuffle = on;
                                self.cfg.save();
                                let _ = self.cmd.send(Cmd::Shuffle(on));
                            }
                            if icon_btn(ui, th, Icon::Prev, 30.0, false, false).clicked() {
                                let _ = self.cmd.send(Cmd::Prev);
                            }
                            let ic = if self.snap.paused || !self.snap.playing { Icon::Play } else { Icon::Pause };
                            if icon_btn(ui, th, ic, 42.0, true, false).clicked() {
                                let _ = self.cmd.send(Cmd::Toggle);
                            }
                            if icon_btn(ui, th, Icon::Next, 30.0, false, false).clicked() {
                                let _ = self.cmd.send(Cmd::Next);
                            }
                            let rr = icon_btn(ui, th, Icon::Repeat, 28.0, false, self.snap.repeat != Repeat::Off);
                            if rr.clicked() {
                                let r = match self.snap.repeat {
                                    Repeat::Off => Repeat::All,
                                    Repeat::All => Repeat::One,
                                    Repeat::One => Repeat::Off,
                                };
                                self.cfg.repeat = r;
                                self.cfg.save();
                                let _ = self.cmd.send(Cmd::SetRepeat(r));
                            }
                            // "repeat one" badge in the middle of the loop
                            if matches!(self.snap.repeat, Repeat::One) {
                                let bc = rr.rect.center();
                                let p1 = ui.painter();
                                p1.circle_filled(bc, 6.0, th.panel);
                                p1.text(bc, Align2::CENTER_CENTER, "1", FontId::monospace(9.0), th.accent);
                            }
                        });
                        ui.add_space(6.0);
                        let dur = self.snap.dur;
                        let frac = self.drag.unwrap_or(if dur > 0.0 { (self.snap.pos / dur) as f32 } else { 0.0 });
                        let t1 = fmt_time(frac as f64 * dur);
                        let t2 = fmt_time(dur);
                        let mono = FontId::monospace(11.5);
                        let (w1, w2) = ui.ctx().fonts(|f| {
                            (
                                f.layout_no_wrap(t1.clone(), mono.clone(), Color32::WHITE).rect.width(),
                                f.layout_no_wrap(t2.clone(), mono.clone(), Color32::WHITE).rect.width(),
                            )
                        });
                        let bar_w = (cw * 0.66).max(120.0);
                        let row_w = w1 + w2 + bar_w + 20.0;
                        ui.allocate_ui_with_layout(Vec2::new(row_w, 18.0), Layout::left_to_right(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;
                            ui.label(RichText::new(&t1).size(11.5).color(th.dim).font(mono.clone()));
                            let resp = bar(ui, th, frac, bar_w);
                            if resp.dragged() || resp.clicked() {
                                if let Some(p) = resp.interact_pointer_pos() {
                                    self.drag = Some(((p.x - resp.rect.left()) / resp.rect.width()).clamp(0.0, 1.0));
                                }
                            }
                            if resp.drag_stopped() || resp.clicked() {
                                if let Some(f) = self.drag.take() {
                                    if dur > 0.0 {
                                        let _ = self.cmd.send(Cmd::Seek(f as f64 * dur));
                                    }
                                }
                            }
                            ui.label(RichText::new(&t2).size(11.5).color(th.dim).font(mono));
                        });
                    });
                });

                // ---- right: mode toggles above the volume slider ----
                ui.allocate_new_ui(UiBuilder::new().max_rect(right_rect), |ui| {
                    ui.with_layout(Layout::top_down(Align::Max).with_main_align(Align::Min), |ui| {
                        let cur_id = cur.as_ref().map(|c| c.id.clone()).unwrap_or_default();
                        let lyrics_shown = !cur_id.is_empty() && self.lyrics_enabled;
                        let bass_on = self.cfg.fx.eq_on && self.cfg.fx.eq == BASS_EQ;
                        let mut fx = self.cfg.fx.clone();
                        let narrow = right_w < 340.0;
                        let tight = right_w < 240.0; // no room for the volume slider
                        if narrow {
                            // Two compact rows so nothing overlaps the transport.
                            ui.add_space(4.0);
                            ui.allocate_ui_with_layout(Vec2::new(right_w, 26.0), Layout::right_to_left(Align::Center), |ui| {
                                if self.pill(ui, "Sound", self.fx_open || self.snap.sleep_left.is_some() || self.snap.sleep_eot).clicked() {
                                    self.fx_open = !self.fx_open;
                                }
                                if self.pill(ui, "Queue", self.show_queue).clicked() {
                                    self.show_queue = !self.show_queue;
                                }
                                if self.pill(ui, "Lyrics", lyrics_shown).clicked() {
                                    self.lyrics_enabled = !self.lyrics_enabled;
                                    self.cfg.auto_show_lyrics = self.lyrics_enabled;
                                    self.cfg.save();
                                    if self.lyrics_enabled && self.view != View::Now {
                                        self.go(ui.ctx(), View::Now);
                                    }
                                }
                            });
                            ui.add_space(6.0);
                            ui.allocate_ui_with_layout(Vec2::new(right_w, 26.0), Layout::right_to_left(Align::Center), |ui| {
                                if !tight {
                                    let resp = bar(ui, th, self.cfg.volume / 100.0, (right_w * 0.38).max(70.0));
                                    if resp.dragged() || resp.clicked() {
                                        if let Some(p) = resp.interact_pointer_pos() {
                                            let f = ((p.x - resp.rect.left()) / resp.rect.width()).clamp(0.0, 1.0);
                                            self.cfg.volume = f * 100.0;
                                            let _ = self.cmd.send(Cmd::Volume(self.cfg.volume));
                                        }
                                    }
                                    if resp.drag_stopped() || resp.clicked() {
                                        self.cfg.save();
                                    }
                                    ui.add_space(8.0);
                                    ui.label(RichText::new("Vol").size(11.5).color(th.dim));
                                    ui.add_space(8.0);
                                }
                                if self.pill(ui, "8D", self.cfg.fx.eight_d).clicked() {
                                    fx.eight_d = !fx.eight_d;
                                }
                                if self.pill(ui, "Bass", bass_on).clicked() {
                                    if bass_on {
                                        fx.eq_on = false;
                                        fx.eq = [0.0; 10];
                                    } else {
                                        fx.eq_on = true;
                                        fx.eq = BASS_EQ;
                                    }
                                }
                                if fx != self.cfg.fx {
                                    self.cfg.fx = fx.clone();
                                    let _ = self.cmd.send(Cmd::SetFx(fx));
                                    self.cfg.save();
                                }
                            });
                        } else {
                            ui.add_space(6.0);
                            ui.allocate_ui_with_layout(Vec2::new(right_w, 26.0), Layout::right_to_left(Align::Center), |ui| {
                                if self.pill(ui, "Sound", self.fx_open || self.snap.sleep_left.is_some() || self.snap.sleep_eot).clicked() {
                                    self.fx_open = !self.fx_open;
                                }
                                if self.pill(ui, "Queue", self.show_queue).clicked() {
                                    self.show_queue = !self.show_queue;
                                }
                                if self.pill(ui, "Lyrics", lyrics_shown).clicked() {
                                    self.lyrics_enabled = !self.lyrics_enabled;
                                    self.cfg.auto_show_lyrics = self.lyrics_enabled;
                                    self.cfg.save();
                                    if self.lyrics_enabled && self.view != View::Now {
                                        self.go(ui.ctx(), View::Now);
                                    }
                                }
                                if self.pill(ui, "Bass", bass_on).clicked() {
                                    if bass_on {
                                        fx.eq_on = false;
                                        fx.eq = [0.0; 10];
                                    } else {
                                        fx.eq_on = true;
                                        fx.eq = BASS_EQ;
                                    }
                                }
                                if self.pill(ui, "8D", self.cfg.fx.eight_d).clicked() {
                                    fx.eight_d = !fx.eight_d;
                                }
                                if fx != self.cfg.fx {
                                    self.cfg.fx = fx.clone();
                                    let _ = self.cmd.send(Cmd::SetFx(fx));
                                    self.cfg.save();
                                }
                            });
                            ui.add_space(8.0);
                            ui.allocate_ui_with_layout(Vec2::new(right_w, 18.0), Layout::right_to_left(Align::Center), |ui| {
                                let resp = bar(ui, th, self.cfg.volume / 100.0, 120.0);
                                if resp.dragged() || resp.clicked() {
                                    if let Some(p) = resp.interact_pointer_pos() {
                                        let f = ((p.x - resp.rect.left()) / resp.rect.width()).clamp(0.0, 1.0);
                                        self.cfg.volume = f * 100.0;
                                        let _ = self.cmd.send(Cmd::Volume(self.cfg.volume));
                                    }
                                }
                                if resp.drag_stopped() || resp.clicked() {
                                    self.cfg.save();
                                }
                                ui.add_space(10.0);
                                ui.label(RichText::new("Vol").size(11.5).color(th.dim));
                            });
                        }
                    });
                });
            });
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        let th = self.theme;
        ui.horizontal(|ui| {
            ui.label(RichText::new(&self.label).font(serif_id(25.0)).color(th.text));
            if self.loading {
                ui.add_space(8.0);
                spin(ui, th);
            }
        });
        let msg = if !self.status.is_empty() { self.status.clone() } else { self.snap.error.clone() };
        if !msg.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new(msg).color(th.accent));
        }
        ui.add_space(10.0);
    }

    fn empty(&self, ui: &mut egui::Ui, big: &str, small: &str) {
        let th = self.theme;
        ui.vertical_centered(|ui| {
            ui.add_space(110.0);
            ui.label(RichText::new(big).size(18.0).color(th.dim));
            ui.label(RichText::new(small).size(13.0).color(th.dim));
        });
    }

    fn page_home(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let th = self.theme;
        let mut open_mix: Option<usize> = None;
        let mut play: Option<(Vec<Track>, usize)> = None;
        let mut goto: Option<View> = None;
        let mut login = false;
        let mut retry = false;
        let mut open_card: Option<ytm::Card> = None;
        let time = ui.input(|i| i.time) as f32;
        let loading_shimmer = self.trending.is_empty() && self.trending_loading;
        let greet = match self.cfg.account_name.split_whitespace().next() {
            Some(n) if self.cfg.signed_in => format!("{}, {}", self.greeting, n),
            _ => self.greeting.clone(),
        };
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // ---- hero ----
            let hero_h = 176.0;
            let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), hero_h), Sense::hover());
            rounded_gradient(ui.painter(), rect, 22.0, th.accent, lerp_col(th.accent, th.bg, 0.8));
            let p = ui.painter();
            p.circle_filled(Pos2::new(rect.right() - 110.0, rect.center().y), 120.0, Color32::from_white_alpha(14));
            p.circle_stroke(Pos2::new(rect.right() - 110.0, rect.center().y), 150.0, Stroke::new(1.5_f32, Color32::from_white_alpha(22)));
            p.circle_filled(Pos2::new(rect.right() - 60.0, rect.top() + 40.0), 22.0, Color32::from_white_alpha(18));
            p.text(
                Pos2::new(rect.left() + 32.0, rect.top() + 44.0),
                Align2::LEFT_CENTER,
                &greet,
                FontId::proportional(34.0),
                Color32::WHITE,
            );
            p.text(
                Pos2::new(rect.left() + 32.0, rect.top() + 80.0),
                Align2::LEFT_CENTER,
                "Ad-free music, exactly the way you like it.",
                FontId::proportional(15.0),
                Color32::from_white_alpha(215),
            );
            let b1 = Rect::from_min_size(Pos2::new(rect.left() + 32.0, rect.bottom() - 62.0), Vec2::new(150.0, 40.0));
            let play_btn = egui::Button::new(RichText::new("Play trending").strong().size(15.0).color(Color32::BLACK))
                .fill(Color32::WHITE)
                .rounding(20.0);
            if ui.put(b1, play_btn).clicked() && !self.trending.is_empty() {
                play = Some((self.trending.clone(), 0));
            }
            if !self.cfg.signed_in && !self.login_waiting {
                let b2 = Rect::from_min_size(Pos2::new(b1.right() + 12.0, b1.top()), Vec2::new(190.0, 40.0));
                let btn = egui::Button::new(RichText::new("Sign in with Google").strong().size(15.0).color(Color32::WHITE))
                    .fill(Color32::from_white_alpha(38))
                    .rounding(20.0);
                if ui.put(b2, btn).clicked() {
                    login = true;
                }
            }

            // ---- YouTube Music home shelves ----
            for (si, shelf) in self.shelves.iter().enumerate().take(8) {
                section(ui, th, &shelf.title, "");
                ui.push_id(("shelf", si), |ui| {
                    egui::ScrollArea::horizontal().auto_shrink([false, true]).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for c in shelf.cards.iter().take(14) {
                                if pages::media_card(ui, th, c, 150.0).clicked() {
                                    open_card = Some(c.clone());
                                }
                            }
                        });
                    });
                });
            }

            // ---- jump back in ----
            if !self.cfg.recent.is_empty() {
                section(ui, th, "Jump back in", "Pick up where you left off");
                let n = self.cfg.recent.len().min(8);
                let list = &self.cfg.recent;
                grid(ui, n, 190.0, |ui, i, w| {
                    if track_card(ui, th, &list[i], w).clicked() {
                        play = Some((list.clone(), i));
                    }
                });
            }

            // ---- trending ----
            section(ui, th, "Trending now", "Popular right now");
            if loading_shimmer {
                let a = 0.5 + 0.5 * (time * 3.0).sin();
                grid(ui, 4, 190.0, |ui, _i, w| {
                    let h = (w - 20.0).floor() + 70.0;
                    let (r, _) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
                    ui.painter().rect_filled(r, 14.0, lerp_col(th.card, th.card2, a));
                });
                ui.ctx().request_repaint_after(Duration::from_millis(33));
            } else if self.trending.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Couldn’t load trending songs.").color(th.dim));
                    if ui.button("Retry").clicked() {
                        retry = true;
                    }
                });
                if !self.trending_err.is_empty() {
                    ui.label(RichText::new(&self.trending_err).size(12.0).color(th.accent));
                }
            } else {
                let n = self.trending.len().min(8);
                let list = &self.trending;
                grid(ui, n, 190.0, |ui, i, w| {
                    if track_card(ui, th, &list[i], w).clicked() {
                        play = Some((list.clone(), i));
                    }
                });
            }

            // ---- mixes ----
            section(ui, th, "Made for you", "Mixes for every mood");
            grid(ui, MIXES.len(), 200.0, |ui, i, w| {
                let m = &MIXES[i];
                if mix_card(ui, th, m.title, m.sub, m.c1, m.c2, w).clicked() {
                    open_mix = Some(i);
                }
            });

            // ---- library ----
            section(ui, th, "Your library", "");
            let pls = self.cfg.playlists.len();
            let nrem = self.remote.len();
            let signed = self.cfg.signed_in;
            let total = 1 + pls + nrem;
            let pal = [
                (rgb(255, 61, 94), rgb(90, 20, 90)),
                (rgb(62, 153, 255), rgb(30, 40, 120)),
                (rgb(30, 215, 130), rgb(10, 80, 70)),
                (rgb(255, 140, 50), rgb(130, 40, 30)),
                (rgb(168, 120, 255), rgb(60, 30, 120)),
            ];
            grid(ui, total, 200.0, |ui, i, w| {
                if i == 0 {
                    let sub = if signed { "Your liked songs" } else { "Sign in to unlock" };
                    if mix_card(ui, th, "Liked Songs", sub, rgb(110, 90, 255), rgb(40, 20, 100), w).clicked() {
                        goto = Some(View::Liked);
                    }
                } else if i > pls {
                    let r = &self.remote[i - 1 - pls];
                    let (c1, c2) = pal[(i - 1) % pal.len()];
                    if mix_card(ui, th, &r.title, "YouTube playlist", c1, c2, w).clicked() {
                        goto = Some(View::Remote(i - 1 - pls));
                    }
                } else {
                    let pl = &self.cfg.playlists[i - 1];
                    let (c1, c2) = pal[(i - 1) % pal.len()];
                    let sub = format!("{} songs", pl.tracks.len());
                    if mix_card(ui, th, &pl.name, &sub, c1, c2, w).clicked() {
                        goto = Some(View::Playlist(i - 1));
                    }
                }
            });
            ui.add_space(30.0);
        });

        if let Some(c) = open_card {
            self.open_card(ctx, c);
        }
        if let Some((q, i)) = play {
            if let Some(t) = q.get(i).cloned() {
                self.record_recent(t);
            }
            let _ = self.cmd.send(Cmd::Play(q, i));
            self.go_now();
        }
        if let Some(i) = open_mix {
            self.open_mix(ctx, i);
        }
        if let Some(v) = goto {
            match v {
                View::Playlist(_) => self.view = v,
                View::Remote(i) => self.open_remote(ctx, i),
                _ => self.go(ctx, v),
            }
        }
        if login {
            self.start_login(ctx);
        }
        if retry {
            self.fetch_trending(ctx);
        }
    }

    fn page_remote(&mut self, ui: &mut egui::Ui, i: usize) {
        let th = self.theme;
        let Some(r) = self.remote.get(i).cloned() else {
            self.view = View::Playlists;
            return;
        };
        let (c1, c2) = PAL[i % PAL.len()];
        let mut back = false;
        let mut play = false;
        if ui.button("‹ Back").clicked() {
            back = true;
        }
        ui.add_space(6.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 130.0), Sense::hover());
        rounded_gradient(ui.painter(), rect, 20.0, c1, c2);
        let p = ui.painter();
        p.circle_filled(Pos2::new(rect.right() - 90.0, rect.center().y), 90.0, Color32::from_white_alpha(16));
        let clip = p.with_clip_rect(rect.shrink(4.0));
        clip.text(Pos2::new(rect.left() + 28.0, rect.top() + 38.0), Align2::LEFT_CENTER, &r.title, FontId::proportional(30.0), Color32::WHITE);
        clip.text(Pos2::new(rect.left() + 28.0, rect.top() + 70.0), Align2::LEFT_CENTER, "YouTube playlist", FontId::proportional(14.0), Color32::from_white_alpha(215));
        let b = Rect::from_min_size(Pos2::new(rect.left() + 28.0, rect.bottom() - 48.0), Vec2::new(130.0, 36.0));
        let btn = egui::Button::new(RichText::new("Play all").strong().size(15.0).color(Color32::BLACK))
            .fill(Color32::WHITE)
            .rounding(18.0);
        if ui.put(b, btn).clicked() {
            play = true;
        }
        ui.add_space(10.0);
        if self.loading {
            ui.horizontal(|ui| {
                spin(ui, th);
                ui.label(RichText::new("Loading songs…").color(th.dim));
            });
        }
        if !self.status.is_empty() {
            ui.label(RichText::new(&self.status).color(th.accent));
        }
        if back {
            let c = ui.ctx().clone();
            self.go(&c, View::Playlists);
            return;
        }
        if play && !self.tracks.is_empty() {
            let _ = self.cmd.send(Cmd::Play(self.tracks.clone(), 0));
            self.go_now();
        }
        if self.tracks.is_empty() {
            return;
        }
        let cur = self.snap.current.as_ref().map(|c| c.id.clone());
        let act = draw_tracks(ui, th, &self.tracks, cur.as_deref(), self.yt_removable(), &self.liked_ids);
        self.handle_act(act, None);
    }

    fn page_mix(&mut self, ui: &mut egui::Ui, i: usize) {
        let th = self.theme;
        let i = i.min(MIXES.len() - 1);
        let m = &MIXES[i];
        let mut back = false;
        let mut play = false;
        if ui.button("‹ Back").clicked() {
            back = true;
        }
        ui.add_space(6.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 150.0), Sense::hover());
        rounded_gradient(ui.painter(), rect, 20.0, m.c1, m.c2);
        let p = ui.painter();
        p.circle_filled(Pos2::new(rect.right() - 90.0, rect.center().y), 100.0, Color32::from_white_alpha(16));
        p.circle_stroke(Pos2::new(rect.right() - 90.0, rect.center().y), 128.0, Stroke::new(1.5_f32, Color32::from_white_alpha(24)));
        p.text(Pos2::new(rect.left() + 28.0, rect.top() + 40.0), Align2::LEFT_CENTER, m.title, FontId::proportional(34.0), Color32::WHITE);
        p.text(Pos2::new(rect.left() + 28.0, rect.top() + 74.0), Align2::LEFT_CENTER, m.sub, FontId::proportional(15.0), Color32::from_white_alpha(215));
        let b = Rect::from_min_size(Pos2::new(rect.left() + 28.0, rect.bottom() - 54.0), Vec2::new(130.0, 38.0));
        let btn = egui::Button::new(RichText::new("Play all").strong().size(15.0).color(Color32::BLACK))
            .fill(Color32::WHITE)
            .rounding(19.0);
        if ui.put(b, btn).clicked() {
            play = true;
        }
        ui.add_space(10.0);
        if self.loading {
            ui.horizontal(|ui| {
                spin(ui, th);
                ui.label(RichText::new("Loading songs…").color(th.dim));
            });
        }
        if !self.status.is_empty() {
            ui.label(RichText::new(&self.status).color(th.accent));
        }
        if back {
            self.view = View::Home;
            self.label = "Home".into();
            return;
        }
        if play && !self.tracks.is_empty() {
            let _ = self.cmd.send(Cmd::Play(self.tracks.clone(), 0));
            self.go_now();
        }
        if self.tracks.is_empty() {
            return;
        }
        let cur = self.snap.current.as_ref().map(|c| c.id.clone());
        let act = draw_tracks(ui, th, &self.tracks, cur.as_deref(), self.yt_removable(), &self.liked_ids);
        self.handle_act(act, None);
    }

    fn page_liked(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let th = self.theme;
        if !self.cfg.signed_in {
            ui.vertical_centered(|ui| {
                ui.add_space(110.0);
                ui.label(RichText::new("Sign in to see your liked songs").size(22.0).strong().color(th.text));
                ui.add_space(6.0);
                ui.label(RichText::new("Rustym opens its own Google sign-in window and detects it automatically.").color(th.dim));
                ui.add_space(18.0);
                if self.login_waiting {
                    spin(ui, th);
                    ui.label(RichText::new("Waiting for you to finish signing in…").color(th.dim));
                    if ui.button("Cancel").clicked() {
                        self.cancel_login();
                    }
                    if !self.login_msg.is_empty() {
                        ui.label(RichText::new(&self.login_msg).size(12.0).color(th.dim));
                    }
                } else if ui.add_sized([220.0, 42.0], accent_button(th, "Sign in with Google")).clicked() {
                    self.start_login(ctx);
                }
                if !self.login_msg.is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new(&self.login_msg).size(12.0).color(th.accent));
                }
                if !self.status.is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new(&self.status).color(th.accent));
                }
            });
            return;
        }
        self.header(ui);
        if self.tracks.is_empty() {
            if !self.loading && self.status.is_empty() {
                self.empty(ui, "No liked songs found", "Like songs on music.youtube.com and they show up here.");
            }
            return;
        }
        // Hero card: count and a one-click Play all.
        egui::Frame::none().fill(th.card).rounding(20.0).inner_margin(Margin::same(20.0)).show(ui, |ui| {
            let p = ui.painter();
            let r = ui.max_rect();
            p.circle_filled(r.right_top() - Vec2::new(20.0, 20.0), 120.0, th.accent.gamma_multiply(0.05));
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Your liked songs").font(serif_id(26.0)).color(th.text));
                    ui.label(
                        RichText::new(format!("{} songs · synced from YouTube Music", self.tracks.len()))
                            .size(12.5)
                            .color(th.dim),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if self.pill(ui, "⤮  Shuffle all", false).clicked() {
                        let _ = self.cmd.send(Cmd::Shuffle(true));
                        let _ = self.cmd.send(Cmd::Play(self.tracks.clone(), 0));
                    }
                    if ui.add(pages::play_pill(th, "▶  Play all")).clicked() {
                        let _ = self.cmd.send(Cmd::Play(self.tracks.clone(), 0));
                    }
                });
            });
        });
        ui.add_space(12.0);
        let cur = self.snap.current.as_ref().map(|c| c.id.clone());
        let act = draw_tracks(ui, th, &self.tracks, cur.as_deref(), self.yt_removable(), &self.liked_ids);
        self.handle_act(act, None);
    }

    fn page_playlists(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let th = self.theme;
        ui.label(RichText::new("Playlists").size(24.0).strong().color(th.text));
        ui.add_space(12.0);
        let mut create = false;
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(Vec2::new(460.0, 44.0), Layout::left_to_right(Align::Center), |ui| {
                egui::Frame::none().fill(th.card).rounding(22.0).inner_margin(Margin::symmetric(16.0, 8.0)).show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_pl)
                            .hint_text("New playlist name…")
                            .frame(false)
                            .font(FontId::proportional(15.0))
                            .desired_width(400.0),
                    );
                });
            });
            if ui.add_sized([120.0, 44.0], accent_button(th, "Create")).clicked() {
                create = true;
            }
        });
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
        if enter && !self.new_pl.trim().is_empty() {
            create = true;
        }
        ui.add_space(8.0);
        // Spotify playlist import (public playlists, matched to YouTube Music).
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(Vec2::new(460.0, 44.0), Layout::left_to_right(Align::Center), |ui| {
                egui::Frame::none().fill(th.card).rounding(22.0).inner_margin(Margin::symmetric(16.0, 8.0)).show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.spotify_url)
                            .hint_text("Paste a Spotify playlist link (open.spotify.com/playlist/…)")
                            .frame(false)
                            .font(FontId::proportional(13.5))
                            .desired_width(400.0),
                    );
                });
            });
            if ui.add_sized([120.0, 44.0], accent_button(th, "Import")).clicked() {
                if self.spotify_url.trim().is_empty() {
                    self.toast = Some(("Paste a Spotify playlist link first".into(), Instant::now()));
                } else {
                    let url = self.spotify_url.trim().to_string();
                    self.toast = Some(("Importing Spotify playlist (matching each track on YouTube Music)…".into(), Instant::now()));
                    self.bg(ctx, move || match engine::spotify_import(&url) {
                        Ok((name, tracks)) => Msg::SpotifyImported(name, tracks),
                        Err(e) => Msg::Toast(format!("Spotify import failed: {e}")),
                    });
                }
            }
        });
        ui.label(RichText::new("Works with public playlists. Each Spotify track is searched on YouTube Music and added to a new local playlist.").size(11.5).color(th.dim));
        if create && !self.new_pl.trim().is_empty() {
            self.cfg.playlists.push(Playlist { name: self.new_pl.trim().to_string(), tracks: vec![] });
            self.new_pl.clear();
            self.cfg.save();
        }
        ui.add_space(14.0);
        let mut open = None;
        let mut open_remote: Option<usize> = None;
        let mut refresh = false;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if self.cfg.signed_in {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("From YouTube").size(18.0).strong().color(th.text));
                    if self.remote_loading {
                        spin(ui, th);
                    } else if ui.small_button("Refresh").clicked() {
                        refresh = true;
                    }
                });
                ui.add_space(8.0);
                if self.remote.is_empty() {
                    if !self.remote_loading {
                        let msg = if self.remote_err.is_empty() { "No YouTube playlists found.".to_string() } else { self.remote_err.clone() };
                        ui.label(RichText::new(msg).size(12.5).color(th.dim));
                    }
                } else {
                    let n = self.remote.len();
                    let list = &self.remote;
                    grid(ui, n, 200.0, |ui, i, w| {
                        let (c1, c2) = PAL[i % PAL.len()];
                        if mix_card(ui, th, &list[i].title, "YouTube playlist", c1, c2, w).clicked() {
                            open_remote = Some(i);
                        }
                    });
                }
                ui.add_space(14.0);
                ui.label(RichText::new("On this device").size(18.0).strong().color(th.text));
                ui.add_space(8.0);
            }
            if self.cfg.playlists.is_empty() {
                ui.label(RichText::new("No local playlists yet. Create one above, then use the + button on any song.").size(12.5).color(th.dim));
            }
            for (i, p) in self.cfg.playlists.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 68.0), Sense::click());
                let pt = ui.painter();
                pt.rect_filled(rect, 12.0, if resp.hovered() { th.card2 } else { th.card });
                let art = Rect::from_min_size(rect.min + Vec2::new(10.0, 10.0), Vec2::splat(48.0));
                vgradient(&pt.with_clip_rect(art), art, th.accent, th.accent.gamma_multiply(0.35));
                pt.text(art.center(), Align2::CENTER_CENTER, p.name.chars().next().unwrap_or('♪').to_uppercase().to_string(),
                        FontId::proportional(22.0), Color32::WHITE);
                pt.text(Pos2::new(art.right() + 16.0, rect.center().y - 9.0), Align2::LEFT_CENTER, &p.name,
                        FontId::proportional(16.0), th.text);
                pt.text(Pos2::new(art.right() + 16.0, rect.center().y + 11.0), Align2::LEFT_CENTER,
                        format!("{} songs", p.tracks.len()), FontId::proportional(13.0), th.dim);
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if resp.clicked() {
                    open = Some(i);
                }
                ui.add_space(6.0);
            }
        });
        if refresh {
            self.remote_err.clear();
            self.fetch_remote(ctx);
        }
        if let Some(i) = open_remote {
            self.open_remote(ctx, i);
            return;
        }
        if let Some(i) = open {
            self.view = View::Playlist(i);
        }
    }

    fn page_playlist(&mut self, ui: &mut egui::Ui, i: usize) {
        let th = self.theme;
        if i >= self.cfg.playlists.len() {
            self.view = View::Playlists;
            return;
        }
        let name = self.cfg.playlists[i].name.clone();
        let count = self.cfg.playlists[i].tracks.len();
        let mut delete = false;
        let mut play_all = false;
        let mut back = false;
        ui.horizontal(|ui| {
            if ui.button("‹ Back").clicked() {
                back = true;
            }
            ui.label(RichText::new(&name).size(24.0).strong().color(th.text));
            ui.label(RichText::new(format!("{count} songs")).color(th.dim));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Delete playlist").clicked() {
                    delete = true;
                }
                if count > 0 && ui.add(accent_button(th, "Play all")).clicked() {
                    play_all = true;
                }
            });
        });
        ui.add_space(12.0);
        if back {
            self.view = View::Playlists;
            return;
        }
        if delete {
            self.cfg.playlists.remove(i);
            self.cfg.save();
            self.view = View::Playlists;
            return;
        }
        if play_all {
            let q = self.cfg.playlists[i].tracks.clone();
            let _ = self.cmd.send(Cmd::Play(q, 0));
            self.go_now();
        }
        if count == 0 {
            self.empty(ui, "This playlist is empty", "Use the + button on any song to add it here.");
            return;
        }
        let cur = self.snap.current.as_ref().map(|c| c.id.clone());
        let act = draw_tracks(ui, th, &self.cfg.playlists[i].tracks, cur.as_deref(), true, &self.liked_ids);
        self.handle_act(act, Some(i));
    }

    fn page_logs(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let th = self.theme;
        let red = Color32::from_rgb(255, 92, 92);
        let amber = Color32::from_rgb(255, 184, 76);
        let green = Color32::from_rgb(80, 210, 130);
        ctx.request_repaint_after(Duration::from_millis(500));
        // Snapshot the log at most once per second instead of cloning up to
        // 1500 entries on every frame.
        if self.logs_cache.0.elapsed() > Duration::from_secs(1) {
            self.logs_cache = (Instant::now(), Arc::new(logs_snapshot()));
        }
        let logs = self.logs_cache.1.clone();
        let health = health_snapshot();
        let busy = health_busy();
        let n_err = logs.iter().filter(|e| e.level == 2).count();
        let n_warn = logs.iter().filter(|e| e.level == 1).count();
        let fid = egui::Id::new("log_filter");
        let mut filter: u8 = ui.data(|d| d.get_temp::<u8>(fid)).unwrap_or(0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // Title + tabs are drawn by page_system_tabs.
            ui.label(
                RichText::new(format!(
                    "{n_err} errors · {n_warn} warnings · {} entries · also saved to ~/.config/rustym/debug.log",
                    logs.len()
                ))
                .size(12.0)
                .color(if n_err > 0 { red } else { th.dim }),
            );
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(!busy, accent_button(th, "Run diagnostics")).clicked() {
                    run_health_checks(false);
                }
                if ui.add_enabled(!busy, egui::Button::new("Test playback")).clicked() {
                    run_health_checks(true);
                }
                if ui.button("Copy report").clicked() {
                    ctx.copy_text(logs_as_text());
                }
                if ui.button("Clear log").clicked() {
                    logs_clear();
                    self.logs_cache = (Instant::now(), Arc::new(Vec::new()));
                }
                if busy {
                    spin(ui, th);
                    ui.label(RichText::new("Checking…").color(th.dim));
                }
            });

            ui.add_space(16.0);
            ui.label(RichText::new("Diagnostics").size(16.0).strong().color(th.text));
            ui.label(
                RichText::new("Everything that can stop Rustym from running: mpv, its libraries, yt-dlp, python, deno, your Google login and YouTube Music itself.")
                    .size(12.0)
                    .color(th.dim),
            );
            ui.add_space(6.0);
            if health.is_empty() {
                ui.label(RichText::new("No results yet - press “Run diagnostics”.").color(th.dim));
            }
            for c in &health {
                let col = match c.sev {
                    0 => green,
                    1 => amber,
                    _ => red,
                };
                egui::Frame::none()
                    .fill(th.card)
                    .rounding(8.0)
                    .inner_margin(Margin::symmetric(12.0, 8.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new("●").color(col));
                            ui.label(RichText::new(c.name.clone()).strong().color(th.text));
                            ui.label(RichText::new(c.detail.clone()).color(th.dim2));
                        });
                        if !c.fix.is_empty() {
                            ui.label(RichText::new(format!("Fix: {}", c.fix)).monospace().size(12.0).color(col));
                        }
                    });
                ui.add_space(4.0);
            }

            ui.add_space(16.0);
            ui.label(RichText::new("Last mpv / yt-dlp output").size(16.0).strong().color(th.text));
            let mt = mpv_log_tail(30);
            ui.label(
                RichText::new(if mt.is_empty() { "(nothing yet - play a song)".to_string() } else { mt })
                    .monospace()
                    .size(11.5)
                    .color(th.dim2),
            );

            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Log").size(16.0).strong().color(th.text));
                ui.add_space(8.0);
                ui.selectable_value(&mut filter, 0u8, "All");
                ui.selectable_value(&mut filter, 1u8, "Warnings + errors");
                ui.selectable_value(&mut filter, 2u8, "Errors");
            });
            ui.add_space(4.0);
            let shown: Vec<&LogEntry> = logs.iter().rev().filter(|e| e.level >= filter).take(400).collect();
            if shown.is_empty() {
                ui.label(RichText::new("Nothing logged at this level.").color(th.dim));
            }
            for e in shown {
                let col = match e.level {
                    0 => th.dim2,
                    1 => amber,
                    _ => red,
                };
                let tag = ["INFO", "WARN", "ERR "][e.level.min(2) as usize];
                ui.label(
                    RichText::new(format!("{}  {}  [{}]  {}", e.t, tag, e.src, e.msg))
                        .monospace()
                        .size(11.5)
                        .color(col),
                );
            }
        });
        ui.data_mut(|d| d.insert_temp(fid, filter));
    }

    /// Settings, Listening stats and Debug logs share one page with tabs, so
    /// the sidebar stays slim. The active tab follows `self.view`.
    fn page_system_tabs(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let th = self.theme;
        let (tab, title) = match self.view {
            View::Stats => (1, "Listening stats"),
            View::Logs => (2, "Debug logs"),
            _ => (0, "Settings"),
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(title).size(24.0).strong().color(th.text));
            if self.loading && tab == 0 {
                ui.add_space(8.0);
                spin(ui, th);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.pill(ui, "Debug logs", tab == 2).clicked() && tab != 2 {
                    self.go(ctx, View::Logs);
                }
                if self.pill(ui, "Listening stats", tab == 1).clicked() && tab != 1 {
                    self.go(ctx, View::Stats);
                }
                if self.pill(ui, "General", tab == 0).clicked() && tab != 0 {
                    self.go(ctx, View::Settings);
                }
            });
        });
        let msg = if !self.status.is_empty() { self.status.clone() } else { self.snap.error.clone() };
        if !msg.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new(msg).color(th.accent));
        }
        ui.add_space(10.0);
        match tab {
            1 => self.page_stats(ui, ctx),
            2 => self.page_logs(ui, ctx),
            _ => self.page_settings(ui, ctx),
        }
    }

    /// A titled settings section rendered as one rounded card.
    fn settings_group(ui: &mut egui::Ui, th: Theme, title: &str, f: impl FnOnce(&mut egui::Ui)) {
        egui::Frame::none().fill(th.card).rounding(16.0).inner_margin(Margin::same(18.0)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).font(serif_id(18.0)).color(th.text));
            ui.add_space(8.0);
            f(ui);
        });
        ui.add_space(10.0);
    }

    /// Checkbox row with a short description underneath.
    fn settings_toggle(ui: &mut egui::Ui, th: Theme, on: &mut bool, title: &str, sub: &str) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            if ui.checkbox(on, RichText::new(title).size(14.0).color(th.text)).changed() {
                changed = true;
            }
        });
        if !sub.is_empty() {
            ui.label(RichText::new(sub).size(11.5).color(th.dim));
        }
        ui.add_space(6.0);
        changed
    }

    fn page_settings(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let th = self.theme;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(2.0);

            // ---------- Appearance ----------
            Self::settings_group(ui, th, "Appearance", |ui| {
                let mut pick = None;
                ui.horizontal_wrapped(|ui| {
                    for (i, t) in THEMES.iter().enumerate() {
                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(112.0, 56.0), Sense::click());
                        let sel = self.cfg.theme == i;
                        let p = ui.painter();
                        p.rect_filled(rect, 12.0, t.panel);
                        p.rect_stroke(rect, 12.0, Stroke::new(if sel { 2.0_f32 } else { 1.0_f32 }, if sel { t.accent } else { t.card2 }));
                        p.circle_filled(Pos2::new(rect.left() + 22.0, rect.center().y), 9.0, t.accent);
                        p.text(Pos2::new(rect.left() + 38.0, rect.center().y), Align2::LEFT_CENTER, t.name,
                               FontId::proportional(13.5), t.text);
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if resp.clicked() {
                            pick = Some(i);
                        }
                    }
                });
                if let Some(i) = pick {
                    self.cfg.theme = i;
                    self.theme = THEMES[i];
                    apply_theme(ctx, &self.theme);
                    self.cfg.save();
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Interface scale").size(13.5).color(th.dim2));
                    ui.add_sized([260.0, 26.0], egui::Slider::new(&mut self.cfg.ui_scale, 80.0..=125.0).suffix("%").show_value(true));
                    if ui.input(|i| i.pointer.any_released()) || ui.ctx().input(|i| i.time) as i64 != 0 {
                        // saved on release below
                    }
                    ctx.request_repaint();
                });
                if ui.input(|i| i.pointer.any_released()) {
                    self.cfg.save();
                }
                if Self::settings_toggle(ui, th, &mut self.cfg.compact_mode, "Compact layout", "Tighter spacing and smaller controls everywhere.") {
                    self.cfg.save();
                }
            });

            // ---------- Playback & startup ----------
            Self::settings_group(ui, th, "Playback & startup", |ui| {
                ui.label(RichText::new("Show this page when Rustym starts").size(13.5).color(th.dim2));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    for (i, name) in ["Home", "Search", "Liked songs"].iter().enumerate() {
                        if self.pill(ui, name, self.cfg.start_view == i as u8).clicked() {
                            self.cfg.start_view = i as u8;
                            self.cfg.save();
                        }
                    }
                });
                ui.add_space(10.0);
                if Self::settings_toggle(ui, th, &mut self.cfg.autoplay, "Autoplay", "Keep the queue going with YouTube Music radio when it ends.") {
                    let _ = self.cmd.send(Cmd::Autoplay(self.cfg.autoplay));
                    self.cfg.save();
                }
                ui.label(RichText::new("Visualiser style").size(13.5).color(th.dim2));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    for (i, n) in ["Bars", "Mirror", "Radial"].iter().enumerate() {
                        if self.pill(ui, n, self.cfg.viz == i).clicked() {
                            self.cfg.viz = i;
                            self.cfg.save();
                        }
                    }
                });
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Lyrics timing offset").size(13.5).color(th.dim2));
                    if ui
                        .add_sized([240.0, 24.0], egui::Slider::new(&mut self.cfg.lyrics_offset, -3000..=3000).suffix(" ms"))
                        .drag_stopped()
                    {
                        self.cfg.save();
                    }
                });
                ui.label(RichText::new("Tempo, pitch, equaliser, reverb, normalisation and the sleep timer are under “Sound” in the player bar.").size(11.5).color(th.dim));
            });

            // ---------- Interface & window ----------
            Self::settings_group(ui, th, "Interface & window", |ui| {
                if Self::settings_toggle(ui, th, &mut self.cfg.auto_show_lyrics, "Keep Lyrics enabled on startup", "The Lyrics button also updates this preference.") {
                    self.lyrics_enabled = self.cfg.auto_show_lyrics;
                    self.cfg.save();
                }
                if Self::settings_toggle(ui, th, &mut self.cfg.background, "Close to tray", "Closing the window keeps the music playing in the background (X11 and Wayland alike).") {
                    self.cfg.save();
                }
                if self.cfg.background {
                    let (txt, col) = if self.tray_ok {
                        ("● Tray icon available".to_string(), th.dim)
                    } else {
                        (
                            format!(
                                "● No system tray detected: the window still closes and music keeps playing; reopen it by launching Rustym again or running `rustym --show`. {}",
                                distro::tray_advice()
                            ),
                            th.accent,
                        )
                    };
                    ui.label(RichText::new(txt).size(12.0).color(col));
                }
                if Self::settings_toggle(ui, th, &mut self.cfg.start_minimised, "Start minimised", "Starts in the background with just the tray icon — nothing pops up on the desktop, at sign-in or when you launch it. Open the window from the tray.") {
                    self.cfg.save();
                }
                let auto_before = self.cfg.start_at_login;
                if Self::settings_toggle(ui, th, &mut self.cfg.start_at_login, "Launch automatically at sign-in", "Uses your desktop's XDG autostart entry.") {
                    match set_desktop_autostart(self.cfg.start_at_login) {
                        Ok(()) => {
                            self.cfg.save();
                            self.toast = Some((if self.cfg.start_at_login { "Desktop auto-start enabled" } else { "Desktop auto-start disabled" }.into(), Instant::now()));
                        }
                        Err(e) => {
                            self.cfg.start_at_login = auto_before;
                            self.toast = Some((format!("Could not update auto-start: {e}"), Instant::now()));
                        }
                    }
                }
                ui.label(RichText::new("Control from a terminal or keybindings: rustym --toggle | --play-pause | --next | --prev | --quit").size(11.5).color(th.dim));
            });

            // ---------- Account ----------
            Self::settings_group(ui, th, "Account", |ui| {
                if self.cfg.signed_in {
                    ui.horizontal(|ui| {
                        account_avatar(ui, th, &self.cfg.account_photo, &self.cfg.account_name, 44.0);
                        ui.vertical(|ui| {
                            let nm = if self.cfg.account_name.is_empty() { "Signed in to Google" } else { self.cfg.account_name.as_str() };
                            ui.label(RichText::new(nm).size(15.0).strong().color(th.text));
                            ui.label(RichText::new("● Signed in").size(12.0).color(th.accent));
                        });
                        ui.add_space(10.0);
                        if ui.button("Sign out").clicked() {
                            self.sign_out();
                        }
                    });
                } else if self.login_waiting {
                    ui.horizontal(|ui| {
                        spin(ui, th);
                        ui.label(RichText::new("Waiting for you to finish signing in…").color(th.dim));
                        if ui.button("Cancel").clicked() {
                            self.cancel_login();
                        }
                    });
                } else {
                    ui.horizontal(|ui| {
                        if ui.add(accent_button(th, "Sign in with Google")).clicked() {
                            self.start_login(ctx);
                        }
                        if ui.button("Use cookies.txt").clicked() {
                            self.use_cookie_file(ctx);
                        }
                    });
                }
                if !self.login_msg.is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new(&self.login_msg).size(12.0).color(th.accent));
                }
            });

            // ---------- Dependencies ----------
            Self::settings_group(ui, th, "Dependencies", |ui| {
                if !self.deps_loaded {
                    self.deps_info = engine::dep_report();
                    self.deps_loaded = true;
                }
                ui.label(
                    RichText::new("Runtime tools Rustym and mpv need for YouTube. Bundled and user-updated copies always beat the distro's.")
                        .size(11.5)
                        .color(th.dim),
                );
                ui.add_space(8.0);
                let green = Color32::from_rgb(80, 210, 130);
                let red = Color32::from_rgb(255, 92, 92);
                for (name, ver, path, missing) in &self.deps_info {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("●").color(if *missing { red } else { green }));
                        ui.label(RichText::new(*name).strong().size(13.5).color(th.text));
                        ui.label(RichText::new(if *missing { "not found".into() } else { ver.clone() })
                            .size(13.0).color(if *missing { red } else { th.dim2 }));
                        if !path.is_empty() {
                            ui.label(RichText::new(path.clone()).monospace().size(11.0).color(th.dim));
                        }
                    });
                }
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(!self.deps_busy, accent_button(th, "Update yt-dlp & deno")).clicked() {
                        self.deps_busy = true;
                        self.deps_report = "Downloading the newest yt-dlp and deno…".into();
                        self.bg(ctx, || Msg::Deps(engine::update_runtime_deps()));
                    }
                    if self.deps_busy {
                        ui.add_space(4.0);
                        spin(ui, th);
                        ui.label(RichText::new("Updating…").color(th.dim));
                    }
                });
                if !self.deps_report.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new(&self.deps_report).monospace().size(11.5).color(th.dim2));
                }
                ui.add_space(6.0);
                ui.label(
                    RichText::new("Installs the newest yt-dlp and deno into ~/.local/bin — no root needed, works on any distro (Arch, Debian/Ubuntu, Fedora). They take priority over the system copies.")
                        .size(11.5)
                        .color(th.dim),
                );
            });
            ui.add_space(10.0);

            // ---------- Maintenance ----------
            Self::settings_group(ui, th, "Maintenance", |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Clear artwork cache").clicked() {
                        self.img.forget_all();
                        let loaders = ctx.loaders();
                        loaders.include.forget_all();
                        for b in loaders.bytes.lock().iter() {
                            b.forget_all();
                        }
                        for l in loaders.texture.lock().iter() {
                            l.forget_all();
                        }
                        self.toast = Some(("Artwork cache cleared".into(), Instant::now()));
                    }
                    if ui.button("Clear saved liked songs").clicked() {
                        liked_cache_clear();
                        self.toast = Some(("Saved liked songs cleared".into(), Instant::now()));
                    }
                    if ui.button("Export local playlists").clicked() {
                        self.export_playlists();
                    }
                    if ui.button("Import local playlists").clicked() {
                        self.import_playlists();
                    }
                    if ui.button("Clear listening stats").clicked() {
                        store::clear();
                        self.toast = Some(("Listening stats cleared".into(), Instant::now()));
                    }
                });
                ui.add_space(6.0);
                ui.label(RichText::new("The artwork cache keeps decoded cover art in RAM bounded; clearing it frees memory immediately if Rustym has been open for days.").size(11.5).color(th.dim));
                ui.label(RichText::new(format!("Playlists export file: {}", config_dir().join("playlists-export.json").display())).size(11.5).color(th.dim));
            });

            // ---------- About ----------
            Self::settings_group(ui, th, "About", |ui| {
                ui.label(
                    RichText::new(format!("Rustym v{}", env!("CARGO_PKG_VERSION")))
                        .font(serif_id(19.0))
                        .color(th.text),
                );
                ui.label(RichText::new("An ad-free YouTube Music player for Linux — pure Rust + egui on top of mpv and yt-dlp.").size(12.0).color(th.dim));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("License: GNU GPL v3.0").size(12.0).color(th.dim2));
                    let repo = "https://github.com/Jagan-Srinivasan/Rustym";
                    if ui.button("Copy project link").clicked() {
                        ui.ctx().copy_text(repo.to_string());
                        self.toast = Some(("Project link copied".into(), Instant::now()));
                    }
                });
                ui.label(
                    RichText::new("Rustym is an unofficial third-party client. Not affiliated with or endorsed by Google or YouTube. Thanks to mpv, yt-dlp, egui and LRCLIB.")
                        .size(11.5)
                        .color(th.dim),
                );
                ui.add_space(8.0);
                if ui.button("Quit Rustym").clicked() {
                    self.cfg.save();
                    let _ = self.cmd.send(Cmd::Quit);
                }
            });
        });
    }
    
    fn main_panel(&mut self, ctx: &egui::Context) {
        let th = self.theme;
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(th.bg).inner_margin(Margin::symmetric(28.0, 22.0)))
            .show(ctx, |ui| {
                let full = ui.max_rect().expand2(Vec2::new(28.0, 22.0));
                vgradient(
                    ui.painter(),
                    Rect::from_min_size(full.min, Vec2::new(full.width(), 280.0)),
                    th.accent.gamma_multiply(0.16),
                    Color32::TRANSPARENT,
                );
                self.top_bar(ui, ctx);
                ui.add_space(6.0);
                match self.view {
                    View::Home => self.page_home(ui, ctx),
                    View::Mix(i) => self.page_mix(ui, i),
                    View::Remote(i) => self.page_remote(ui, i),
                    View::Search => self.page_search(ui, ctx),
                    View::Liked => self.page_liked(ui, ctx),
                    View::Now => self.page_now(ui, ctx),
                    View::Playlists => self.page_playlists(ui, ctx),
                    View::Playlist(i) => self.page_playlist(ui, i),
                    View::Settings | View::Stats | View::Logs => self.page_system_tabs(ui, ctx),
                    View::Albums => self.page_cards(ui, ctx, 0),
                    View::Artists => self.page_cards(ui, ctx, 1),
                    View::History => self.page_history(ui, ctx),
                    View::Local => self.page_local(ui, ctx),
                    View::Detail => self.page_detail(ui, ctx),
                }
            });
    }

    /// Floating top bar: back navigation and a rounded search pill that jumps
    /// into the Search page (like the mock's top-right search field).
    fn top_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let th = self.theme;
        ui.horizontal(|ui| {
            // Back button (uses the app's real navigation history).
            let can_back = !self.back.is_empty();
            let (r, resp) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::click());
            let p = ui.painter();
            if can_back {
                p.circle_filled(r.center(), 16.0, th.card);
                p.text(r.center(), Align2::CENTER_CENTER, "‹", FontId::proportional(19.0), th.text);
                if resp.clicked() {
                    if let Some(v) = self.back.pop() {
                        self.go(ctx, v);
                    }
                } else if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
            } else {
                p.circle_filled(r.center(), 16.0, th.card.gamma_multiply(0.55));
                p.text(r.center(), Align2::CENTER_CENTER, "‹", FontId::proportional(19.0), th.dim);
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // The Search page has its own input; don't show the pill twice.
                if self.view == View::Search {
                    return;
                }
                let w = (ui.available_width() * 0.42).clamp(240.0, 420.0);
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 34.0), Sense::click());
                let p = ui.painter();
                // Lighter fill + a visible stroke so the pill stands out on any theme.
                p.rect_filled(rect, 17.0, th.card2);
                p.rect_stroke(rect, 17.0, Stroke::new(1.0_f32, lerp_col(th.card2, th.text, 0.25)));
                p.text(
                    Pos2::new(rect.left() + 16.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    "⌕",
                    FontId::proportional(15.0),
                    th.dim2,
                );
                p.text(
                    Pos2::new(rect.left() + 36.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    "Search songs, artists, playlists…",
                    FontId::proportional(13.0),
                    th.dim,
                );
                if resp.clicked() {
                    self.go(ctx, View::Search);
                } else if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
            });
        });
    }

    fn add_modal(&mut self, ctx: &egui::Context) {
        let Some(t) = self.adding.clone() else { return };
        let th = self.theme;
        let mut close = false;
        let mut chosen: Option<usize> = None;
        let mut create = false;
        let mut chosen_yt: Option<usize> = None;
        let mut create_yt = false;
        egui::Window::new("Add to playlist")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_min_width(320.0);
                ui.label(RichText::new(&t.title).strong().color(th.text));
                ui.add_space(8.0);
                for (i, p) in self.cfg.playlists.iter().enumerate() {
                    if ui.add_sized([ui.available_width(), 32.0], egui::Button::new(&p.name)).clicked() {
                        chosen = Some(i);
                    }
                }
                if self.cfg.signed_in && !self.remote.is_empty() && t.local.is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new("YouTube Music playlists").size(12.0).color(th.dim));
                    egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                        for (i, r) in self.remote.iter().enumerate() {
                            if ui.add_sized([ui.available_width(), 30.0], egui::Button::new(&r.title)).clicked() {
                                chosen_yt = Some(i);
                            }
                        }
                    });
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.new_pl).hint_text("New playlist…").desired_width(190.0));
                    if ui.add(accent_button(th, "Create & add")).clicked() {
                        create = true;
                    }
                });
                if self.cfg.signed_in && t.local.is_empty() && ui.button("Create as new YouTube Music playlist").clicked() {
                    create_yt = true;
                }
                ui.add_space(6.0);
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        if create && !self.new_pl.trim().is_empty() {
            self.cfg.playlists.push(Playlist { name: self.new_pl.trim().to_string(), tracks: vec![] });
            self.new_pl.clear();
            chosen = Some(self.cfg.playlists.len() - 1);
        }
        if let Some(i) = chosen {
            self.add_to_playlist(i, t.clone());
            close = true;
        }
        if let Some(i) = chosen_yt {
            self.add_to_yt(i, t.clone());
            close = true;
        }
        if create_yt && !self.new_pl.trim().is_empty() {
            let name = self.new_pl.trim().to_string();
            self.new_pl.clear();
            self.create_yt_playlist(name, vec![t.id.clone()]);
            close = true;
        }
        if close {
            self.adding = None;
        }
    }
}

impl eframe::App for Rustym {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Closing the window just ends this process; the background process keeps playing
        // (or quits, when "close to tray" is off).
        if self.view == View::Settings && self.tray_checked.elapsed() > Duration::from_secs(3) {
            self.tray_ok = tray_host_running();
            self.tray_checked = Instant::now();
        }

        // Image-cache janitor: release decoded images, GPU textures and raw
        // bytes for thumbnails that have not been drawn for a while.
        if self.img_t.elapsed() > Duration::from_secs(15) {
            self.img_t = Instant::now();
            let stale = self.img.forget_stale();
            if !stale.is_empty() {
                let loaders = ctx.loaders();
                for u in &stale {
                    loaders.include.forget(u);
                    for b in loaders.bytes.lock().iter() {
                        b.forget(u);
                    }
                    for l in loaders.texture.lock().iter() {
                        l.forget(u);
                    }
                }
            }
            // glibc keeps freed pages in the arena, so the RSS never came down
            // even after every cache released its buffers. Hand freed pages
            // back to the OS on every janitor pass.
            #[cfg(target_os = "linux")]
            unsafe {
                libc::malloc_trim(0);
            }
        }
        // The ~19 MB CJK fallback font is loaded lazily, the first time CJK
        // text can appear (new track, lyrics, typed text, account names…).
        // Checked on track changes and each janitor pass; the scan is cheap.
        if !CJK_FONTS_LOADED.load(SeqCst)
            && (self.cjk_check || self.img_t.elapsed() > Duration::from_secs(15))
        {
            self.cjk_check = false;
            let mut texts: Vec<&str> = Vec::new();
            if let Some(t) = &self.snap.current {
                texts.push(&t.title);
                texts.push(&t.artist);
                texts.push(&t.album);
            }
            texts.push(self.query.as_str());
            texts.push(self.cfg.account_name.as_str());
            for t in &self.tracks {
                texts.push(&t.title);
                texts.push(&t.artist);
            }
            for p in &self.cfg.playlists {
                texts.push(&p.name);
            }
            if let Some(l) = &self.lyrics {
                texts.push(&l.plain);
                for s in &l.synced {
                    texts.push(&s.text);
                }
            }
            if texts.iter().any(|s| font_cjk_needed(s)) {
                load_cjk_fonts(ctx);
            }
        }
        // Visualiser lifecycle: run the audio capture only while shown.
        if self.cfg.show_visualiser && self.viz.is_none() {
            self.viz = Some(spawn_viz());
        } else if !self.cfg.show_visualiser && self.viz.is_some() {
            stop_viz();
            self.viz = None;
        }

        // Apply the saved interface preferences before laying out this frame.
        ctx.set_zoom_factor((self.cfg.ui_scale / 100.0).clamp(0.80, 1.25));
        ctx.style_mut(|style| {
            style.spacing.item_spacing = if self.cfg.compact_mode { Vec2::new(7.0, 5.0) } else { Vec2::new(10.0, 8.0) };
            style.spacing.button_padding = if self.cfg.compact_mode { Vec2::new(10.0, 5.0) } else { Vec2::new(14.0, 7.0) };
        });
        self.pump(ctx);
        self.snap = self.shared.lock().unwrap().clone();
        let current_id = self.snap.current.as_ref().map(|t| t.id.clone()).unwrap_or_default();
        if !current_id.is_empty() && current_id != self.last_current_id {
            self.last_current_id = current_id;
            self.cjk_check = true; // new track: CJK may have appeared in the text
            self.back.clear();
            self.view = View::Now;
            self.label = "Now playing".into();
        }
        // Minimise only once, after the first backend snapshot has arrived.
        // Note: the window process never self-minimises any more. Wayland
        // compositors ignore an app's own minimize request before the window is
        // mapped (so "start minimised" looked broken — the window appeared
        // anyway), and a fresh window process spawned by the tray restore would
        // have minimised itself again, making "show window" look dead. The
        // background process owns this decision: with "Start minimised" it
        // simply never spawns the window at sign-in.
        self.update_levels();

        self.housekeeping(ctx);
        self.shortcuts(ctx);
        self.sidebar(ctx);
        self.player(ctx);
        self.queue_panel(ctx);
        self.main_panel(ctx);
        self.add_modal(ctx);
        self.fx_window(ctx);
        self.help_window(ctx);
        self.toast_draw(ctx);

        let playing = self.snap.playing && !self.snap.paused;
        let ms = if self.view == View::Now && playing {
            33
        } else if playing {
            200
        } else {
            400
        };
        ctx.request_repaint_after(Duration::from_millis(ms));
    }
}

impl Drop for Rustym {
    fn drop(&mut self) {
        // Only the window is going away. The engine lives in the background process.
        self.cfg.save();
    }
}

const HELP: &str = "Rustym - ad-free YouTube Music player

USAGE: rustym [OPTION]

  (no option)     open the window (or raise the running one)
  --background    start without a window (tray only)
  --show          open / raise the window
  --hide          close the window, keep playing in the background
  --toggle        show if hidden, hide if shown
  --play-pause    toggle playback of the running instance
  --next, --prev  skip tracks in the running instance
  --quit          quit the running instance
  -V, --version   print the version

The control options talk to the running instance, so they work from any shell,
launcher or desktop keybinding on X11 and Wayland alike.";

/// The window process: just the UI, connected to the background process that plays the music.
fn run_gui() -> ! {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([860.0, 520.0])
            .with_title("Rustym")
            .with_app_id("rustym"),
        // With vsync on, Wayland blocks the UI thread in swap_buffers while the window is
        // hidden / on another workspace, which makes some compositors show "Application Not Responding".
        vsync: false,
        ..Default::default()
    };
    let r = eframe::run_native("Rustym", opts, Box::new(|cc| Ok(Box::new(Rustym::new(cc)))));
    // Only the window ends here; the background process keeps the music going.
    stop_viz();
    if let Err(e) = r {
        eprintln!("rustym: {e}");
        log(2, "gui", format!("window error: {e}"));
        std::process::exit(1);
    }
    std::process::exit(0);
}

/// Desktop launchers do not always put ~/.local/bin (pip / yt-dlp release binary) or ~/.deno/bin
/// on PATH, even though a terminal does. Add them so mpv can always find yt-dlp and deno.
fn extend_path() {
    let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else { return };
    // Managed copies of yt-dlp / deno win over the distro's, which is often
    // years old and gets 403s from YouTube: first the user-updated ones in
    // ~/.local/bin (Settings > Dependencies), then the copies bundled into the
    // packages at /usr/lib/rustym. Every child process inherits this, including
    // mpv's ytdl_hook script that resolves the stream URLs.
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    for extra in [home.join(".local/bin"), "/usr/lib/rustym".into()] {
        if extra.is_dir() {
            dirs.push(extra);
        }
    }
    dirs.extend(std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default());
    for extra in [home.join(".deno/bin"), home.join(".cargo/bin"), "/usr/local/bin".into()] {
        if extra.is_dir() && !dirs.contains(&extra) {
            dirs.push(extra);
        }
    }
    if let Ok(joined) = std::env::join_paths(dirs) {
        std::env::set_var("PATH", joined);
    }
}

fn main() {
    extend_path();
    let mut start_hidden = false;
    let mut gui_mode = false;
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return;
            }
            "-V" | "--version" => {
                println!("rustym {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--gui" => gui_mode = true, // internal: the window process
            "--background" | "--hidden" => start_hidden = true,
            "--show" | "--toggle" | "--hide" | "--play-pause" | "--next" | "--prev" | "--quit" => {
                let word = a.trim_start_matches("--").replace('-', "");
                if send_instance(&word) {
                    return;
                }
                match a.as_str() {
                    "--show" | "--toggle" => {} // not running: start it normally
                    "--hide" | "--quit" => return,
                    _ => {
                        eprintln!("rustym is not running");
                        std::process::exit(1);
                    }
                }
            }
            other => {
                eprintln!("rustym: unknown option {other} (try --help)");
                std::process::exit(2);
            }
        }
    }
    if gui_mode {
        run_gui();
    }
    // Single instance: a second launch asks the running one to open its window.
    if start_hidden {
        if send_instance("ping") {
            return;
        }
    } else if send_instance("show") {
        return;
    }

    std::panic::set_hook(Box::new(|info| {
        log(2, "panic", format!("{info}"));
        eprintln!("rustym panic: {info}");
    }));
    log(0, "app", format!("Rustym {} starting", env!("CARGO_PKG_VERSION")));

    // ----- background process: engine + tray + media keys + control socket; windows are child processes -----
    let cfg = Config::load();
    // "Start minimised": a cold start stays in the background with just the
    // tray icon, so nothing pops up on the desktop at sign-in (the autostart
    // entry runs `rustym --show`, so this must win over --show too). The window
    // process cannot reliably minimise itself on Wayland — compositors ignore
    // an app's own minimize request before the window is mapped — so not
    // opening the window at all is the dependable behaviour. If no tray shows
    // up within 10 s the fallback below still opens a window, so the person is
    // never left with nothing.
    let start_minimised = cfg.start_minimised;
    if start_minimised {
        start_hidden = true;
    }
    let (cmd, shared) = spawn_engine(cfg.volume, Arc::new(|| {}));
    let _ = cmd.send(Cmd::Shuffle(cfg.shuffle));
    let _ = cmd.send(Cmd::SetRepeat(cfg.repeat));
    let _ = cmd.send(Cmd::Autoplay(cfg.autoplay));
    let _ = cmd.send(Cmd::SetFx(cfg.fx.clone()));
    let win = Arc::new(WinCtl::new());
    mpris::spawn(cmd.clone(), shared.clone(), win.clone());
    spawn_tray(cmd.clone(), win.clone());
    listen_instance(win.clone(), cmd.clone(), shared);

    let started = Instant::now();
    let mut first = true;
    let mut child: Option<std::process::Child> = None;
    loop {
        // The window process ended (window closed)?
        if let Some(c) = child.as_mut() {
            if let Ok(Some(st)) = c.try_wait() {
                child = None;
                win.gui_open.store(false, SeqCst);
                if Config::load().background {
                    log(0, "app", format!("window closed ({st}); running in the background"));
                    warn_no_tray();
                } else {
                    break; // close-to-tray is off: closing the window quits Rustym
                }
            }
        }
        // Hide request (tray / CLI / media-key raise toggle): end the window process, keep running.
        if win.want_hide.swap(false, SeqCst) {
            if let Some(mut c) = child.take() {
                let _ = c.kill();
                let _ = c.wait();
                win.gui_open.store(false, SeqCst);
            }
        }
        let mut open_now = win.want_show.swap(false, SeqCst);
        if first {
            if !start_hidden {
                open_now = true;
            } else if TRAY_UP.load(SeqCst) {
                first = false; // the tray is there to restore the window: stay hidden
            } else if started.elapsed() > Duration::from_secs(10) {
                open_now = true; // no tray showed up: never leave the person with nothing
            }
        }
        if open_now && child.is_none() {
            first = false;
            match spawn_gui() {
                Ok(c) => {
                    child = Some(c);
                    win.gui_open.store(true, SeqCst);
                }
                Err(e) => {
                    eprintln!("rustym: could not start the window process: {e}");
                    log(2, "app", format!("could not start window process: {e}"));
                    if start_hidden && !TRAY_UP.load(SeqCst) {
                        break;
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(150));
    }

    if let Some(mut c) = child {
        let _ = c.kill();
        let _ = c.wait();
    }
    // Quit: the engine stops mpv, removes its sockets and ends the process.
    let _ = cmd.send(Cmd::Quit);
    std::thread::sleep(Duration::from_millis(500));
    cleanup();
    std::process::exit(0);
}
