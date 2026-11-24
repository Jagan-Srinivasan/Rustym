//! Bounded image cache for egui's network images.
//!
//! Why this exists: egui_extras 0.29's `ImageCrateLoader` caches every decoded
//! image forever (its source literally says "TODO: automatic cache eviction"),
//! at the *native* decoded size. Requesting a 45 px row thumbnail downloaded
//! and kept a 1280x720 RGBA buffer (~3.7 MB) per track, so scrolling around
//! the app grew RSS by hundreds of MB and never released it.
//!
//! This loader fixes both halves:
//!   * decode + downscale to the requested size (capped), so a 45 px row
//!     holds a 45x45 buffer (~8 KB) instead of 3.7 MB;
//!   * an LRU with a total byte budget and stale-entry eviction.

use eframe::egui::{
    load::{ImageLoadResult, ImageLoader, ImagePoll, LoadError, SizeHint},
    mutex::Mutex,
    ColorImage,
};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

/// Total decoded-bytes budget for the whole cache.
const BUDGET: usize = 96 * 1024 * 1024;
/// Entries unused for this long are evicted by the app-side janitor.
pub const STALE: Duration = Duration::from_secs(45);
/// How often the loader runs its internal sweep.
const SWEEP: Duration = Duration::from_secs(3);
/// Failed downloads are retried after this long.
const RETRY: Duration = Duration::from_secs(30);
/// Never decode wider/taller than this (Now Playing art is at most ~420 px).
const MAX_DECODE_SIDE: u32 = 1280;
/// Minimum decode size, so tiny hints don't produce blurry upscales.
const MIN_SIDE: u32 = 96;

enum Entry {
    Img { img: Arc<ColorImage>, sw: u32, sh: u32, nw: u32, nh: u32, last_used: Instant },
    Err { at: Instant },
}

pub struct BoundedImageLoader {
    cache: Mutex<HashMap<String, Entry>>,
    last_sweep: Mutex<Instant>,
}

impl BoundedImageLoader {
    fn new() -> Self {
        Self { cache: Mutex::new(HashMap::default()), last_sweep: Mutex::new(Instant::now()) }
    }

    /// Installs the loader (tried first, before egui_extras' default one) and
    /// returns a handle for the app-side janitor.
    pub fn install(ctx: &eframe::egui::Context) -> Arc<Self> {
        let loader = Arc::new(Self::new());
        ctx.add_image_loader(loader.clone());
        loader
    }

    /// URIs not used for longer than `STALE`. The app also forgets the GPU
    /// texture and the raw-bytes cache for these, so everything is released.
    pub fn forget_stale(&self) -> Vec<String> {
        let mut cache = self.cache.lock();
        let mut stale = Vec::new();
        cache.retain(|uri, e| match e {
            Entry::Img { last_used, .. } => {
                let keep = last_used.elapsed() < STALE;
                if !keep {
                    stale.push(uri.clone());
                }
                keep
            }
            Entry::Err { at } => at.elapsed() < RETRY,
        });
        stale
    }

    fn sweep(&self, cache: &mut HashMap<String, Entry>) {
        let mut last = self.last_sweep.lock();
        if last.elapsed() < SWEEP {
            return;
        }
        *last = Instant::now();

        // 1) expired error entries may be retried
        cache.retain(|_, e| match e {
            Entry::Err { at } => at.elapsed() < RETRY,
            Entry::Img { .. } => true,
        });
        // 2) enforce the byte budget, evicting the least recently used first
        let total: usize = cache.values().map(entry_bytes).sum();
        if total <= BUDGET {
            return;
        }
        let mut order: Vec<(String, Instant)> = cache
            .iter()
            .filter_map(|(k, e)| match e {
                Entry::Img { last_used, .. } => Some((k.clone(), *last_used)),
                Entry::Err { .. } => None,
            })
            .collect();
        order.sort_by_key(|(_, t)| *t);
        let mut excess = total - BUDGET;
        for (uri, _) in order {
            if excess == 0 {
                break;
            }
            if let Some(e) = cache.remove(&uri) {
                excess = excess.saturating_sub(entry_bytes(&e));
            }
        }
    }
}

fn entry_bytes(e: &Entry) -> usize {
    match e {
        Entry::Img { img, .. } => img.pixels.len() * 4,
        Entry::Err { .. } => 0,
    }
}

fn hint_box(hint: SizeHint) -> (u32, u32) {
    match hint {
        SizeHint::Size(w, h) => (w, h),
        SizeHint::Width(w) => (w, w),
        SizeHint::Height(h) => (h, h),
        _ => (MAX_DECODE_SIDE / 2, MAX_DECODE_SIDE / 2), // Scale or unknown: a middle default (384)
    }
}

/// Cover-crop-aware decode: the UI always paints pictures with a "fill the
/// box, crop the overflow" fit (egui's uv centre-crop / cover_uv), so decode
/// such that BOTH decoded dimensions are >= the requested box. Decoding to
/// `max side = box side` instead would shrink the cropped-away part too and
/// made square tiles blurry (the cropped region then got upscaled ~1.8x).
/// The result is sharp and still tiny: a 260 px tile costs ~0.5 MB decoded
/// instead of ~3.7 MB.
fn decode(bytes: &[u8], bw: u32, bh: u32) -> Option<(ColorImage, u32, u32, u32, u32)> {
    let im = image::load_from_memory(bytes).ok()?;
    let (w, h) = (im.width().max(1), im.height().max(1));
    // Cover the box (so the "fill box, crop overflow" paint is 1:1 and sharp),
    // but never upscale beyond the native size - a source with fewer pixels
    // than the box gains nothing from a bigger texture (the GPU stretches it
    // at paint time), and the hit rule keeps such entries cached forever.
    let mut scale = (bw.max(1) as f32 / w as f32).max(bh.max(1) as f32 / h as f32).min(1.0).max(1e-6);
    let (w2, h2) = ((w as f32 * scale).ceil() as u32, (h as f32 * scale).ceil() as u32);
    // Safety cap for gigantic sources
    let (mut w2, mut h2) = (w2, h2);
    if w2 > MAX_DECODE_SIDE || h2 > MAX_DECODE_SIDE {
        let k = (MAX_DECODE_SIDE as f32) / (w2.max(h2) as f32);
        w2 = (w2 as f32 * k) as u32;
        h2 = (h2 as f32 * k) as u32;
    }
    let scaled = if scale < 1.0 { im.resize_exact(w2, h2, image::imageops::FilterType::Triangle) } else { im };
    let rgba = scaled.to_rgba8();
    let (rw, rh) = (rgba.width(), rgba.height());
    let ci = ColorImage::from_rgba_unmultiplied([rw as usize, rh as usize], rgba.as_raw());
    Some((ci, rw, rh, w, h))
}

impl ImageLoader for BoundedImageLoader {
    fn id(&self) -> &str {
        concat!("rustym/", "bounded-image-cache")
    }

    fn load(&self, ctx: &eframe::egui::Context, uri: &str, hint: SizeHint) -> ImageLoadResult {
        let (mut bw, mut bh) = hint_box(hint);
        bw = bw.clamp(MIN_SIDE, MAX_DECODE_SIDE);
        bh = bh.clamp(MIN_SIDE, MAX_DECODE_SIDE);
        let now = Instant::now();
        {
            let mut cache = self.cache.lock();
            if let Some(e) = cache.get_mut(uri) {
                match e {
                    Entry::Img { img, sw, sh, nw, nh, last_used } => {
                        if (*sw >= bw && *sh >= bh) || (*nw <= *sw && *nh <= *sh) {
                            *last_used = now;
                            return Ok(ImagePoll::Ready { image: img.clone() });
                        }
                        // cached smaller than now needed: fall through to re-decode
                    }
                    Entry::Err { at } => {
                        if at.elapsed() < RETRY {
                            return Err(LoadError::Loading("previous attempt failed".into()));
                        }
                        // expired: retry below
                    }
                }
            }
        }

        match ctx.try_load_bytes(uri) {
            Ok(eframe::egui::load::BytesPoll::Ready { bytes, .. }) => {
                let Some((img, sw, sh, nw, nh)) = decode(&bytes, bw, bh) else {
                    self.cache.lock().insert(uri.into(), Entry::Err { at: now });
                    return Err(LoadError::NotSupported);
                };
                let img = Arc::new(img);
                self.cache.lock().insert(uri.into(), Entry::Img {
                    img: img.clone(),
                    sw,
                    sh,
                    nw,
                    nh,
                    last_used: now,
                });
                self.sweep(&mut self.cache.lock());
                Ok(ImagePoll::Ready { image: img })
            }
            Ok(eframe::egui::load::BytesPoll::Pending { size }) => Ok(ImagePoll::Pending { size }),
            Err(e) => {
                // remember the failure briefly so we don't hammer the network every frame
                self.cache.lock().insert(uri.into(), Entry::Err { at: now });
                Err(LoadError::Loading(format!("{e}")))
            }
        }
    }

    fn forget(&self, uri: &str) {
        self.cache.lock().remove(uri);
    }

    fn forget_all(&self) {
        self.cache.lock().clear();
    }

    fn byte_size(&self) -> usize {
        self.cache.lock().values().map(entry_bytes).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_cover_crop_sharp() {
        // 16:9 source painted into a square box: decode must keep height >= box
        // so the centre-cropped square is not upscaled (the blur bug).
        let img = image::DynamicImage::new_rgb8(1280, 720);
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Jpeg).unwrap();
        let (ci, sw, sh, nw, nh) = decode(buf.get_ref(), 260, 260).unwrap();
        assert_eq!((nw, nh), (1280, 720));
        assert!(sw >= 260 && sh >= 260, "decoded {sw}x{sh} must cover a 260x260 box");
        assert_eq!(ci.width().max(ci.height()) as u32, sw);
        // Still small: ~0.5 MB decoded instead of ~3.7 MB.
        assert!(ci.pixels.len() * 4 < 800_000);
    }

    #[test]
    fn small_source_is_not_upscaled_in_cache() {
        let img = image::DynamicImage::new_rgb8(320, 180);
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Jpeg).unwrap();
        let (_, sw, sh, nw, nh) = decode(buf.get_ref(), 420, 420).unwrap();
        assert_eq!((nw, nh), (320, 180));
        assert_eq!((sw, sh), (320, 180)); // native <= served keeps this cached
    }

    #[test]
    fn tiny_box_gets_min_decode() {
        let img = image::DynamicImage::new_rgb8(1280, 720);
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Jpeg).unwrap();
        let (ci, _, _, _, _) = decode(buf.get_ref(), 45, 45).unwrap();
        assert!(ci.width().max(ci.height()) <= 1280);
        assert!(ci.pixels.len() * 4 < 1_000_000);
    }
}
