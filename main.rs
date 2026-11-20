//! Rustym - simple, ad-free YouTube Music player (UI).

mod engine;

use eframe::egui::{
    self, Align, Align2, Color32, FontData, FontDefinitions, FontFamily, FontId, ImageSource, Layout, Margin,
    Pos2, Rect, RichText, Sense, Shape, Stroke, Vec2, ViewportCommand,
};
use engine::*;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering::SeqCst},
        mpsc::{channel, Receiver, Sender},
        Arc, Mutex,
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

const THEMES: [Theme; 6] = [
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
];

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
}

enum Act {
    Play(usize),
    Add(usize),
    Remove(usize),
}

#[derive(Clone, Copy, PartialEq)]
enum Icon {
    Play,
    Pause,
    Next,
    Prev,
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
    ui.label(RichText::new(title).size(22.0).strong().color(th.text));
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

fn track_card(ui: &mut egui::Ui, th: Theme, t: &Track, w: f32) -> egui::Response {
    let ih = ((w - 20.0) * 9.0 / 16.0).floor();
    let h = ih + 20.0 + 50.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
    let k = ui.ctx().animate_bool(resp.id, resp.hovered());
    let p = ui.painter();
    p.rect_filled(rect, 14.0, lerp_col(th.card, th.card2, k));
    let img = Rect::from_min_size(rect.min + Vec2::splat(10.0), Vec2::new(w - 20.0, ih));
    thumb(ui, &t.id, img, 10.0);
    play_bubble(p, th, img.right_bottom() + Vec2::new(-28.0, -28.0 + 6.0 * (1.0 - k)), k);
    let clip = Rect::from_min_max(Pos2::new(rect.left() + 10.0, rect.top()), Pos2::new(rect.right() - 10.0, rect.bottom()));
    let tc = p.with_clip_rect(clip);
    tc.text(Pos2::new(rect.left() + 12.0, img.bottom() + 17.0), Align2::LEFT_CENTER, &t.title, FontId::proportional(14.5), th.text);
    tc.text(Pos2::new(rect.left() + 12.0, img.bottom() + 37.0), Align2::LEFT_CENTER, &t.artist, FontId::proportional(12.5), th.dim);
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
    let clip = p.with_clip_rect(r.shrink(2.0));
    clip.text(Pos2::new(r.left() + 16.0, r.bottom() - 36.0), Align2::LEFT_CENTER, title, FontId::proportional(19.0), Color32::WHITE);
    clip.text(Pos2::new(r.left() + 16.0, r.bottom() - 15.0), Align2::LEFT_CENTER, sub, FontId::proportional(12.0), Color32::from_white_alpha(205));
    play_bubble(p, th, Pos2::new(r.right() - 32.0, r.bottom() - 32.0 + 6.0 * (1.0 - k)), k);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
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
    egui::Image::new(ImageSource::Uri(format!("https://i.ytimg.com/vi/{id}/mqdefault.jpg").into()))
        .fit_to_exact_size(rect.size())
        .rounding(radius)
        .paint_at(ui, rect);
}

fn icon_btn(ui: &mut egui::Ui, th: Theme, icon: Icon, size: f32, circle: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let hot = resp.hovered();
    let c = rect.center();
    let p = ui.painter();
    let fg;
    if circle {
        let fill = if th.light { th.text } else if hot { Color32::WHITE } else { Color32::from_gray(232) };
        p.circle_filled(c, size / 2.0, fill);
        fg = if th.light { Color32::WHITE } else { Color32::BLACK };
    } else {
        fg = if hot { th.text } else { th.dim2 };
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
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::click());
    let k = ui.ctx().animate_bool(resp.id, resp.hovered());
    let p = ui.painter();
    if active {
        p.rect_filled(rect, 10.0, th.card2);
        p.rect_filled(
            Rect::from_min_size(Pos2::new(rect.left(), rect.center().y - 10.0), Vec2::new(3.0, 20.0)),
            2.0,
            th.accent,
        );
    } else if k > 0.01 {
        p.rect_filled(rect, 10.0, th.card.gamma_multiply(k));
    }
    let col = if active { th.accent } else { lerp_col(th.dim2, th.text, k) };
    draw_glyph(p, icon, Pos2::new(rect.left() + 24.0, rect.center().y), col);
    p.text(
        Pos2::new(rect.left() + 46.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(15.0),
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

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let paths = [
        "/usr/share/fonts/noto/NotoSansTamil-Regular.ttf",
        "/usr/share/fonts/noto/NotoSansDevanagari-Regular.ttf",
        "/usr/share/fonts/noto/NotoSansTelugu-Regular.ttf",
        "/usr/share/fonts/noto/NotoSansMalayalam-Regular.ttf",
        "/usr/share/fonts/noto/NotoSansKannada-Regular.ttf",
        "/usr/share/fonts/noto/NotoSansArabic-Regular.ttf",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
    ];
    for (i, p) in paths.iter().enumerate() {
        if let Ok(bytes) = std::fs::read(p) {
            let name = format!("sysfont{i}");
            fonts.font_data.insert(name.clone(), FontData::from_owned(bytes));
            if let Some(f) = fonts.families.get_mut(&FontFamily::Proportional) {
                f.push(name);
            }
        }
    }
    ctx.set_fonts(fonts);
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

fn draw_tracks(ui: &mut egui::Ui, th: Theme, tracks: &[Track], cur: Option<&str>, removable: bool) -> Option<Act> {
    let row_h = 58.0;
    let mut act: Option<Act> = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, row_h, tracks.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for i in range {
                let t = &tracks[i];
                let playing = cur == Some(t.id.as_str());
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::click());
                let hovered = resp.hovered();
                let p = ui.painter();
                if playing {
                    p.rect_filled(rect, 10.0, th.accent.gamma_multiply(0.12));
                } else if hovered {
                    p.rect_filled(rect, 10.0, th.card);
                }
                let img = Rect::from_min_size(rect.min + Vec2::new(8.0, 6.5), Vec2::new(80.0, 45.0));
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
                pc.text(
                    Pos2::new(tx, rect.center().y + 11.0),
                    Align2::LEFT_CENTER,
                    &t.artist,
                    FontId::proportional(13.0),
                    th.dim,
                );
                if let Some(d) = t.dur {
                    p.text(
                        Pos2::new(rect.right() - 14.0, rect.center().y),
                        Align2::RIGHT_CENTER,
                        fmt_time(d),
                        FontId::proportional(13.0),
                        th.dim,
                    );
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
                    act = Some(if removable { Act::Remove(i) } else { Act::Add(i) });
                } else if resp.clicked() {
                    act = Some(Act::Play(i));
                }
            }
        });
    act
}

// ---------------- app ----------------

struct Rustym {
    cfg: Config,
    theme: Theme,
    cmd: Sender<Cmd>,
    shared: Arc<Mutex<Shared>>,
    viz: Arc<Mutex<VizData>>,
    win: Arc<WinCtl>,
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
    msg_tx: Sender<Msg>,
    msg_rx: Receiver<Msg>,
}

impl Rustym {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let cfg = Config::load();
        let theme = THEMES[cfg.theme.min(THEMES.len() - 1)];
        egui_extras::install_image_loaders(&cc.egui_ctx);
        setup_fonts(&cc.egui_ctx);
        apply_theme(&cc.egui_ctx, &theme);

        let c2 = cc.egui_ctx.clone();
        let repaint: Repaint = Arc::new(move || c2.request_repaint());
        let (cmd, shared) = spawn_engine(cfg.volume, repaint.clone());
        let viz = spawn_viz();
        let win = Arc::new(WinCtl::new());
        spawn_tray(cmd.clone(), win.clone(), repaint.clone());
        listen_instance(win.clone(), repaint);

        let (msg_tx, msg_rx) = channel();
        let trending = cfg.trending.clone();
        let remote = cfg.remote.clone();
        let signed = cfg.signed_in;
        let mut app = Rustym {
            cfg,
            theme,
            cmd,
            shared,
            viz,
            win,
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
        };
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
            let m = match run_ytdlp(args) {
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
        let args = vec![
            "--flat-playlist".to_string(),
            "--dump-json".to_string(),
            "--no-warnings".to_string(),
            format!("ytsearch30:{}", m.query),
        ];
        self.start(ctx, m.title.to_string(), args);
    }

    fn sign_out(&mut self) {
        self.cfg.signed_in = false;
        forget_login();
        self.remote.clear();
        self.cfg.remote.clear();
        self.remote_cache.clear();
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

    fn start(&mut self, ctx: &egui::Context, label: String, args: Vec<String>) {
        self.start_chain(ctx, label, vec![args]);
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

    fn start_chain(&mut self, ctx: &egui::Context, label: String, chains: Vec<Vec<String>>) {
        self.loading = true;
        self.status.clear();
        self.label = label;
        self.req += 1;
        let id = self.req;
        let tx = self.msg_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let m = match run_chain(chains) {
                Ok(t) => Msg::Results(id, t),
                Err(e) => Msg::Error(id, e),
            };
            let _ = tx.send(m);
            ctx.request_repaint();
        });
    }

    fn search(&mut self, ctx: &egui::Context) {
        let q = self.query.trim().to_string();
        if q.is_empty() {
            return;
        }
        let args = vec![
            "--flat-playlist".into(),
            "--dump-json".into(),
            "--no-warnings".into(),
            format!("ytsearch30:{q}"),
        ];
        self.start(ctx, format!("Results for “{q}”"), args);
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
    }

    fn start_login(&mut self, ctx: &egui::Context) {
        if self.login_waiting {
            return;
        }
        self.login_waiting = true;
        self.login_msg.clear();
        let cancel = Arc::new(AtomicBool::new(false));
        self.login_cancel = cancel.clone();
        let browser = self.cfg.browser.clone();
        open_login(&browser);
        let tx = self.msg_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let tp = tx.clone();
            let cp = ctx.clone();
            let progress = move |t: String| {
                let _ = tp.send(Msg::LoginProgress(t));
                cp.request_repaint();
            };
            let m = match wait_for_login(&browser, &cancel, &progress) {
                Ok(b) => Msg::LoginOk(Some(b)),
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
            View::Logs => {
                self.label = "Debug logs".into();
                if health_snapshot().is_empty() && !health_busy() {
                    run_health_checks(false);
                }
            }
            _ => {}
        }
    }

    fn hide(&mut self, ctx: &egui::Context) {
        self.win.hidden.store(true, SeqCst);
        if is_hypr() {
            hide_window();
        } else {
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
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
                    self.tracks = t;
                }
                Msg::Error(id, e) => {
                    log(2, "fetch", format!("{}: {e}", self.label));
                    if id != self.req {
                        continue;
                    }
                    self.loading = false;
                    self.tracks.clear();
                    self.status = if self.view == View::Liked {
                        format!("Couldn’t load liked songs ({e}). Try signing out and in again, and keep yt-dlp up to date (sudo pacman -Syu yt-dlp).")
                    } else {
                        e
                    };
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
                    if let Some(b) = b {
                        self.cfg.browser = b;
                    }
                    self.cfg.signed_in = true;
                    self.cfg.save();
                    self.mix_cache.clear();
                    self.remote_err.clear();
                    self.fetch_remote(ctx);
                    if self.view == View::Liked {
                        self.load_liked(ctx);
                    }
                }
                Msg::LoginProgress(t) => {
                    if self.login_waiting {
                        self.login_msg = t;
                    }
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
        let target: Vec<f32> = {
            let v = self.viz.lock().unwrap();
            if v.live && v.updated.elapsed() < Duration::from_millis(1200) {
                v.levels.clone()
            } else if playing {
                fake_levels(self.t0.elapsed().as_secs_f32())
            } else {
                vec![0.0; BANDS]
            }
        };
        for i in 0..BANDS {
            let t = target[i];
            let d = &mut self.disp[i];
            *d = if t > *d { *d * 0.35 + t * 0.65 } else { *d * 0.88 };
        }
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
            }
            Some(Act::Add(i)) => self.adding = list.get(i).cloned(),
            Some(Act::Remove(i)) => {
                if let Some(p) = src {
                    if let Some(pl) = self.cfg.playlists.get_mut(p) {
                        if i < pl.tracks.len() {
                            pl.tracks.remove(i);
                        }
                    }
                    self.cfg.save();
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
                        Color32::WHITE,
                        Stroke::NONE,
                    ));
                    ui.label(RichText::new("Rustym").size(22.0).strong().color(th.text));
                });
                ui.add_space(22.0);
                let items: [(&str, View, Glyph); 7] = [
                    ("Home", View::Home, Glyph::Home),
                    ("Search", View::Search, Glyph::Search),
                    ("Liked songs", View::Liked, Glyph::Heart),
                    ("Now playing", View::Now, Glyph::Wave),
                    ("Playlists", View::Playlists, Glyph::List),
                    ("Settings", View::Settings, Glyph::Gear),
                    ("Debug logs", View::Logs, Glyph::List),
                ];
                for (name, v, g) in items {
                    let active = match (self.view, v) {
                        (View::Playlist(_), View::Playlists) => true,
                        (View::Mix(_), View::Home) => true,
                        (View::Remote(_), View::Playlists) => true,
                        (a, b) => a == b,
                    };
                    if nav_item(ui, th, g, name, active).clicked() {
                        self.go(ctx, v);
                    }
                }

                ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                    if self.cfg.signed_in {
                        if ui.add(egui::Button::new(RichText::new("Sign out").size(12.0).color(th.dim)).frame(false)).clicked() {
                            self.sign_out();
                        }
                        ui.label(RichText::new("● Signed in").size(13.0).color(th.accent));
                    } else if self.login_waiting {
                        if ui.add(egui::Button::new(RichText::new("Cancel").size(12.0).color(th.dim)).frame(false)).clicked() {
                            self.cancel_login();
                        }
                        ui.horizontal(|ui| {
                            spin(ui, th);
                            ui.label(RichText::new("Finish signing in in your browser…").size(12.0).color(th.dim));
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
            .exact_height(92.0)
            .frame(egui::Frame::none().fill(th.panel).inner_margin(Margin::symmetric(20.0, 10.0)))
            .show(ctx, |ui| {
                let top = ui.max_rect().top() - 10.0;
                ui.painter().line_segment(
                    [Pos2::new(ui.max_rect().left() - 20.0, top), Pos2::new(ui.max_rect().right() + 20.0, top)],
                    Stroke::new(1.0_f32, th.card2),
                );
                ui.columns(3, |cols| {
                    let cur = self.snap.current.clone();
                    cols[0].horizontal(|ui| match &cur {
                        Some(t) => {
                            let (r, rr) = ui.allocate_exact_size(Vec2::new(96.0, 54.0), Sense::click());
                            thumb(ui, &t.id, r, 8.0);
                            if rr.clicked() {
                                self.view = View::Now;
                            }
                            ui.vertical(|ui| {
                                ui.add_space(8.0);
                                ui.add(
                                    egui::Label::new(RichText::new(&t.title).size(15.0).strong().color(th.text))
                                        .wrap_mode(egui::TextWrapMode::Truncate),
                                );
                                ui.add(
                                    egui::Label::new(RichText::new(&t.artist).size(13.0).color(th.dim))
                                        .wrap_mode(egui::TextWrapMode::Truncate),
                                );
                            });
                        }
                        None => {
                            ui.add_space(8.0);
                            ui.label(RichText::new("Nothing playing").color(th.dim));
                        }
                    });

                    let ui = &mut cols[1];
                    ui.horizontal(|ui| {
                        ui.add_space(((ui.available_width() - 130.0) / 2.0).max(0.0));
                        ui.spacing_mut().item_spacing.x = 14.0;
                        if icon_btn(ui, th, Icon::Prev, 30.0, false).clicked() {
                            let _ = self.cmd.send(Cmd::Prev);
                        }
                        let ic = if self.snap.paused || !self.snap.playing { Icon::Play } else { Icon::Pause };
                        if icon_btn(ui, th, ic, 40.0, true).clicked() {
                            let _ = self.cmd.send(Cmd::Toggle);
                        }
                        if icon_btn(ui, th, Icon::Next, 30.0, false).clicked() {
                            let _ = self.cmd.send(Cmd::Next);
                        }
                    });
                    ui.horizontal(|ui| {
                        let dur = self.snap.dur;
                        let frac = self.drag.unwrap_or(if dur > 0.0 { (self.snap.pos / dur) as f32 } else { 0.0 });
                        ui.label(RichText::new(fmt_time(frac as f64 * dur)).size(12.0).color(th.dim));
                        let w = ui.available_width() - 48.0;
                        let resp = bar(ui, th, frac, w);
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
                        ui.label(RichText::new(fmt_time(dur)).size(12.0).color(th.dim));
                    });

                    cols[2].with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let resp = bar(ui, th, self.cfg.volume / 100.0, 110.0);
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
                        ui.label(RichText::new("Vol").size(12.0).color(th.dim));
                    });
                });
            });
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        let th = self.theme;
        ui.horizontal(|ui| {
            ui.label(RichText::new(&self.label).size(24.0).strong().color(th.text));
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
        let time = ui.input(|i| i.time) as f32;
        let loading_shimmer = self.trending.is_empty() && self.trending_loading;
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
                &self.greeting,
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
                    let h = ((w - 20.0) * 9.0 / 16.0).floor() + 70.0;
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

        if let Some((q, i)) = play {
            if let Some(t) = q.get(i).cloned() {
                self.record_recent(t);
            }
            let _ = self.cmd.send(Cmd::Play(q, i));
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
        }
        if self.tracks.is_empty() {
            return;
        }
        let cur = self.snap.current.as_ref().map(|c| c.id.clone());
        let act = draw_tracks(ui, th, &self.tracks, cur.as_deref(), false);
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
        }
        if self.tracks.is_empty() {
            return;
        }
        let cur = self.snap.current.as_ref().map(|c| c.id.clone());
        let act = draw_tracks(ui, th, &self.tracks, cur.as_deref(), false);
        self.handle_act(act, None);
    }

    fn page_search(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let th = self.theme;
        let mut go = false;
        ui.horizontal(|ui| {
            let w = ui.available_width() - 96.0;
            let resp = ui
                .scope(|ui| {
                    ui.spacing_mut().interact_size.y = 40.0;
                    ui.add(
                        egui::TextEdit::singleline(&mut self.query)
                            .hint_text("Search songs, artists…")
                            .font(FontId::proportional(16.0))
                            .desired_width(w),
                    )
                })
                .inner;
            if self.focus_once {
                resp.request_focus();
                self.focus_once = false;
            }
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                go = true;
            }
            if ui.add_sized([80.0, 40.0], accent_button(th, "Search")).clicked() {
                go = true;
            }
        });
        if go {
            self.search(ctx);
        }
        ui.add_space(18.0);
        self.header(ui);
        if self.tracks.is_empty() {
            if !self.loading && self.status.is_empty() {
                self.empty(ui, "Search for a song to get started", "No ads. Just music.");
                ui.add_space(24.0);
                let mut pick: Option<&str> = None;
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("Try searching for").size(13.0).color(th.dim));
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        for s in ["Arijit Singh", "Anirudh Ravichander", "A. R. Rahman", "Lo-fi beats", "Taylor Swift", "The Weeknd", "Ilaiyaraaja", "Coke Studio"] {
                            let b = egui::Button::new(RichText::new(s).size(14.0)).rounding(18.0).min_size(Vec2::new(0.0, 34.0));
                            if ui.add(b).clicked() {
                                pick = Some(s);
                            }
                        }
                    });
                });
                if let Some(s) = pick {
                    self.query = s.to_string();
                    self.search(ctx);
                }
            }
            return;
        }
        let cur = self.snap.current.as_ref().map(|c| c.id.clone());
        let act = draw_tracks(ui, th, &self.tracks, cur.as_deref(), false);
        self.handle_act(act, None);
    }

    fn page_liked(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let th = self.theme;
        if !self.cfg.signed_in {
            ui.vertical_centered(|ui| {
                ui.add_space(110.0);
                ui.label(RichText::new("Sign in to see your liked songs").size(22.0).strong().color(th.text));
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!(
                        "Rustym opens Google sign-in in {} and detects it automatically.",
                        self.cfg.browser
                    ))
                    .color(th.dim),
                );
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
        let cur = self.snap.current.as_ref().map(|c| c.id.clone());
        let act = draw_tracks(ui, th, &self.tracks, cur.as_deref(), false);
        self.handle_act(act, None);
    }

    fn page_now(&mut self, ui: &mut egui::Ui) {
        let th = self.theme;
        let Some(t) = self.snap.current.clone() else {
            self.empty(ui, "Nothing playing yet", "Pick a song from Search, Liked songs or a playlist.");
            return;
        };
        let style = self.cfg.viz;
        ui.vertical_centered(|ui| {
            ui.add_space(4.0);
            let w = (ui.available_width() * 0.55).min(500.0);
            let (r, _) = ui.allocate_exact_size(Vec2::new(w, w * 9.0 / 16.0), Sense::hover());
            ui.painter().rect_filled(r.expand(8.0), 20.0, th.accent.gamma_multiply(0.16));
            thumb(ui, &t.id, r, 14.0);
            ui.add_space(18.0);
            ui.label(RichText::new(&t.title).size(26.0).strong().color(th.text));
            ui.label(RichText::new(&t.artist).size(16.0).color(th.dim));
            ui.add_space(14.0);
            let vw = ui.available_width().min(820.0);
            let vh = if style == 2 { 280.0 } else { 150.0 };
            let resp = draw_viz(ui, th, &self.disp, style, Vec2::new(vw, vh));
            if resp.clicked() {
                self.cfg.viz = (self.cfg.viz + 1) % 3;
                self.cfg.save();
            }
            ui.label(RichText::new("click the visualiser to change style").size(11.0).color(th.dim));
        });
    }

    fn page_playlists(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let th = self.theme;
        ui.label(RichText::new("Playlists").size(24.0).strong().color(th.text));
        ui.add_space(12.0);
        let mut create = false;
        ui.horizontal(|ui| {
            let w = ui.available_width() - 120.0;
            let r = ui.scope(|ui| {
                ui.spacing_mut().interact_size.y = 38.0;
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_pl)
                        .hint_text("New playlist name…")
                        .desired_width(w),
                )
            });
            if r.inner.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                create = true;
            }
            if ui.add_sized([110.0, 38.0], accent_button(th, "Create")).clicked() {
                create = true;
            }
        });
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
        }
        if count == 0 {
            self.empty(ui, "This playlist is empty", "Use the + button on any song to add it here.");
            return;
        }
        let cur = self.snap.current.as_ref().map(|c| c.id.clone());
        let act = draw_tracks(ui, th, &self.cfg.playlists[i].tracks, cur.as_deref(), true);
        self.handle_act(act, Some(i));
    }

    fn page_logs(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let th = self.theme;
        let red = Color32::from_rgb(255, 92, 92);
        let amber = Color32::from_rgb(255, 184, 76);
        let green = Color32::from_rgb(80, 210, 130);
        ctx.request_repaint_after(Duration::from_millis(500));
        let logs = logs_snapshot();
        let health = health_snapshot();
        let busy = health_busy();
        let n_err = logs.iter().filter(|e| e.level == 2).count();
        let n_warn = logs.iter().filter(|e| e.level == 1).count();
        let fid = egui::Id::new("log_filter");
        let mut filter: u8 = ui.data(|d| d.get_temp::<u8>(fid)).unwrap_or(0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.label(RichText::new("Debug logs").size(24.0).strong().color(th.text));
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

    fn page_settings(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let th = self.theme;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.label(RichText::new("Settings").size(24.0).strong().color(th.text));
            ui.add_space(16.0);

            ui.label(RichText::new("Theme").size(16.0).strong().color(th.text));
            ui.add_space(6.0);
            let mut pick = None;
            ui.horizontal_wrapped(|ui| {
                for (i, t) in THEMES.iter().enumerate() {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(116.0, 64.0), Sense::click());
                    let sel = self.cfg.theme == i;
                    let p = ui.painter();
                    p.rect_filled(rect, 12.0, t.panel);
                    p.rect_stroke(rect, 12.0, Stroke::new(if sel { 2.0_f32 } else { 1.0_f32 }, if sel { t.accent } else { t.card2 }));
                    p.circle_filled(Pos2::new(rect.left() + 22.0, rect.center().y), 9.0, t.accent);
                    p.text(Pos2::new(rect.left() + 38.0, rect.center().y), Align2::LEFT_CENTER, t.name,
                           FontId::proportional(14.0), t.text);
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

            ui.add_space(22.0);
            ui.label(RichText::new("Visualiser style").size(16.0).strong().color(th.text));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (i, n) in ["Bars", "Mirror", "Radial"].iter().enumerate() {
                    if ui.selectable_value(&mut self.cfg.viz, i, *n).changed() {
                        self.cfg.save();
                    }
                }
            });

            ui.add_space(22.0);
            ui.label(RichText::new("Browser used for Google sign-in").size(16.0).strong().color(th.text));
            ui.label(RichText::new("The browser you sign in with. Rustym reads its login to load liked songs.").size(12.0).color(th.dim));
            ui.add_space(6.0);
            let before = self.cfg.browser.clone();
            ui.horizontal_wrapped(|ui| {
                for b in BROWSERS {
                    ui.selectable_value(&mut self.cfg.browser, b.to_string(), b);
                }
            });
            if self.cfg.browser != before {
                self.cfg.signed_in = false;
                forget_login();
                self.cfg.save();
            }

            ui.add_space(22.0);
            ui.label(RichText::new("Account").size(16.0).strong().color(th.text));
            ui.add_space(6.0);
            if self.cfg.signed_in {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("● Signed in").color(th.accent));
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
                ui.label(RichText::new(&self.login_msg).size(12.0).color(th.accent));
            }
            ui.label(
                RichText::new(format!(
                    "Sign-in not picked up? Export your YouTube cookies (Netscape cookies.txt) to {} and press “Use cookies.txt”.",
                    cookie_file().display()
                ))
                .size(12.0)
                .color(th.dim),
            );

            ui.add_space(22.0);
            ui.label(RichText::new("Window").size(16.0).strong().color(th.text));
            ui.add_space(6.0);
            if ui
                .checkbox(&mut self.cfg.background, "Keep playing in the background when the window is closed (needs a system tray)")
                .changed()
            {
                self.cfg.save();
            }

            ui.add_space(22.0);
            if ui.button("Quit Rustym").clicked() {
                self.cfg.save();
                let _ = self.cmd.send(Cmd::Quit);
            }
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
                match self.view {
                    View::Home => self.page_home(ui, ctx),
                    View::Mix(i) => self.page_mix(ui, i),
                    View::Remote(i) => self.page_remote(ui, i),
                    View::Search => self.page_search(ui, ctx),
                    View::Liked => self.page_liked(ui, ctx),
                    View::Now => self.page_now(ui),
                    View::Playlists => self.page_playlists(ui, ctx),
                    View::Playlist(i) => self.page_playlist(ui, i),
                    View::Settings => self.page_settings(ui, ctx),
                    View::Logs => self.page_logs(ui, ctx),
                }
            });
    }

    fn add_modal(&mut self, ctx: &egui::Context) {
        let Some(t) = self.adding.clone() else { return };
        let th = self.theme;
        let mut close = false;
        let mut chosen: Option<usize> = None;
        let mut create = false;
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
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.new_pl).hint_text("New playlist…").desired_width(190.0));
                    if ui.add(accent_button(th, "Create & add")).clicked() {
                        create = true;
                    }
                });
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
            self.add_to_playlist(i, t);
            close = true;
        }
        if close {
            self.adding = None;
        }
    }
}

impl eframe::App for Rustym {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.win.want_show.swap(false, SeqCst) {
            ctx.send_viewport_cmd(ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(ViewportCommand::Focus);
        }
        if self.win.want_hide.swap(false, SeqCst) {
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.cfg.background {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.hide(ctx);
        }
        if !ctx.wants_keyboard_input() && ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            let _ = self.cmd.send(Cmd::Toggle);
        }

        self.pump(ctx);
        self.snap = self.shared.lock().unwrap().clone();
        self.update_levels();

        self.sidebar(ctx);
        self.player(ctx);
        self.main_panel(ctx);
        self.add_modal(ctx);

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
        self.cfg.save();
        let _ = self.cmd.send(Cmd::Quit);
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn main() {
    if ping_existing() {
        return;
    }
    std::panic::set_hook(Box::new(|info| {
        log(2, "panic", format!("{info}"));
        eprintln!("rustym panic: {info}");
    }));
    log(0, "app", format!("Rustym {} starting", env!("CARGO_PKG_VERSION")));
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([860.0, 520.0])
            .with_title("Rustym")
            .with_app_id("rustym"),
        // With vsync on, Wayland blocks the whole UI thread in swap_buffers while the window is
        // hidden / on another workspace, which makes Hyprland show "Application Not Responding".
        vsync: false,
        ..Default::default()
    };
    let r = eframe::run_native("Rustym", opts, Box::new(|cc| Ok(Box::new(Rustym::new(cc)))));
    // The window is closed: stop mpv / parec / sockets and make sure the process really exits.
    cleanup();
    if let Err(e) = r {
        eprintln!("rustym: {e}");
        std::process::exit(1);
    }
    std::process::exit(0);
}
