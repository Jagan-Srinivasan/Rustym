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
const MAX_SIDE: u32 = 768;
/// Minimum decode size, so tiny hints don't produce blurry upscales.
const MIN_SIDE: u32 = 96;

enum Entry {
    Img { img: Arc<ColorImage>, served: u32, native: u32, last_used: Instant },
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

fn hint_side(hint: SizeHint) -> u32 {
    match hint {
        SizeHint::Size(w, h) => w.max(h).clamp(MIN_SIDE, MAX_SIDE),
        SizeHint::Width(w) | SizeHint::Height(w) => w.clamp(MIN_SIDE, MAX_SIDE),
        _ => MAX_SIDE / 2, // Scale or unknown: a middle default (384)
    }
}

fn decode(bytes: &[u8], target: u32) -> Option<(ColorImage, u32, u32)> {
    let im = image::load_from_memory(bytes).ok()?;
    let (w, h) = (im.width(), im.height());
    let big = w.max(h).max(1);
    // Downscale to `target` on the long side (never upscale) to bound memory.
    let scaled = if big > target { im.thumbnail(target, target) } else { im };
    let rgba = scaled.to_rgba8();
    let (w2, h2) = (rgba.width(), rgba.height());
    let ci = ColorImage::from_rgba_unmultiplied([w2 as usize, h2 as usize], rgba.as_raw());
    // served = the max side we actually decoded to; native = the source's own max side.
    // A hit is: served >= requested, or the source itself is no bigger than served.
    Some((ci, big.min(target), big))
}

impl ImageLoader for BoundedImageLoader {
    fn id(&self) -> &str {
        concat!("rustym/", "bounded-image-cache")
    }

    fn load(&self, ctx: &eframe::egui::Context, uri: &str, hint: SizeHint) -> ImageLoadResult {
        let target = hint_side(hint);
        let now = Instant::now();
        {
            let mut cache = self.cache.lock();
            if let Some(e) = cache.get_mut(uri) {
                match e {
                    Entry::Img { img, served, native, last_used } => {
                        if *served >= target || *native <= *served {
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
                let Some((img, served, native)) = decode(&bytes, target) else {
                    self.cache.lock().insert(uri.into(), Entry::Err { at: now });
                    return Err(LoadError::NotSupported);
                };
                let img = Arc::new(img);
                self.cache.lock().insert(uri.into(), Entry::Img {
                    img: img.clone(),
                    served,
                    native,
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
    fn decode_downscales_to_target() {
        let img = image::DynamicImage::new_rgb8(1280, 720);
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Jpeg).unwrap();
        let (ci, served, native) = decode(buf.get_ref(), 96).unwrap();
        assert_eq!(native, 1280);
        assert_eq!(served, 96);
        assert_eq!(ci.width().max(ci.height()), 96);
        // A 45 px row now costs ~17 KB decoded instead of ~3.7 MB.
        assert!(ci.pixels.len() * 4 < 100_000);
    }

    #[test]
    fn small_source_is_not_upscaled_in_cache() {
        let img = image::DynamicImage::new_rgb8(320, 180);
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Jpeg).unwrap();
        let (_, served, native) = decode(buf.get_ref(), 420).unwrap();
        assert_eq!(native, 320);
        assert_eq!(served, 320); // hit rule native <= served keeps this cached
    }
}
