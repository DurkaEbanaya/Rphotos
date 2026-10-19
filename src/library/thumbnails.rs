//! Prioritized parallel thumbnails, directional prefetch and bounded GPU LRU.

use super::thumbnail_jobs::{Jobs, Request, MAX_QUEUED};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use eframe::egui;
use image::{DynamicImage, ImageFormat};

use super::store::{cache_key, Photo};

const MAX_TEXTURES: usize = 2048;
// Keep a substantial backscroll cache. The current/prefetch working set remains
// protected even above the soft byte budget, up to the hard texture-count guard.
const MAX_VISIBLE_TEXTURES: usize = 4096;
const GPU_BUDGET: usize = 512 * 1024 * 1024;

pub struct Thumbnails {
    jobs: Jobs,
    textures: HashMap<String, egui::TextureHandle>,
    frost: HashMap<String, egui::TextureHandle>,
    order: VecDeque<String>,
    pending: HashSet<String>,
    failed: HashSet<String>,
    aspects: HashMap<String, f32>,
    changed: HashMap<String, f32>,
    touched: HashMap<String, u64>,
    pass: u64,
    protected: HashSet<String>,
    last_plan: Vec<String>,
    visible_count: usize,
    scroll: f32,
    forward: bool,
    pub(super) revision: u64,
    gpu_bytes: usize,
    sizes: HashMap<String, usize>,
    last_use: HashMap<String, u64>,
    availability: HashMap<String, (bool, std::time::Instant)>,
    availability_changed: bool,
    offline: HashMap<String, PathBuf>,
}

impl Thumbnails {
    #[cfg(test)]
    pub(super) fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn new(directory: PathBuf, ctx: &egui::Context) -> Self {
        let jobs = Jobs::new(directory, ctx);
        Self {
            jobs,
            textures: HashMap::new(),
            frost: HashMap::new(),
            order: VecDeque::new(),
            pending: HashSet::new(),
            failed: HashSet::new(),
            aspects: HashMap::new(),
            changed: HashMap::new(),
            touched: HashMap::new(),
            pass: 0,
            protected: HashSet::new(),
            last_plan: Vec::new(),
            visible_count: 0,
            scroll: 0.0,
            forward: true,
            revision: 0,
            gpu_bytes: 0,
            sizes: HashMap::new(),
            last_use: HashMap::new(),
            availability: HashMap::new(),
            availability_changed: false,
            offline: HashMap::new(),
        }
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        for (key, path) in &self.offline {
            if self.availability.get(key).is_some_and(|(ok, time)| {
                !ok && time.elapsed() >= std::time::Duration::from_secs(2)
            }) {
                self.jobs.probe(Request {
                    key: key.clone(),
                    path: path.clone(),
                });
            }
        }
        if !self.offline.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_secs(2));
        }
        self.upload(ctx, false);
    }

    pub(super) fn take_availability_changed(&mut self) -> bool {
        std::mem::take(&mut self.availability_changed)
    }
    pub(super) fn restore_availability(&mut self, photos: &[Photo], unavailable: &HashSet<String>) {
        for photo in photos {
            let available = !unavailable.contains(&photo.key);
            self.availability
                .insert(photo.key.clone(), (available, std::time::Instant::now()));
            if available {
                self.offline.remove(&photo.key);
            } else {
                self.offline.insert(photo.key.clone(), photo.path.clone());
            }
        }
        self.availability_changed = true;
    }

    // Called after the current viewport plan, before painting its photos.
    pub(super) fn upload_visible(&mut self, ctx: &egui::Context) {
        self.upload(ctx, true);
    }

    fn upload(&mut self, ctx: &egui::Context, visible_only: bool) {
        for (key, available) in self.jobs.availability() {
            let was_unavailable = self.availability.get(&key).is_some_and(|(ok, _)| !ok);
            if was_unavailable != !available {
                self.availability_changed = true;
                ctx.request_repaint();
            }
            self.availability
                .insert(key.clone(), (available, std::time::Instant::now()));
            if !available {
                self.textures.remove(&key);
                self.frost.remove(&key);
                self.order.retain(|k| k != &key);
                self.gpu_bytes = self
                    .gpu_bytes
                    .saturating_sub(self.sizes.remove(&key).unwrap_or(0));
                self.failed.remove(&key);
            } else if was_unavailable {
                self.offline.remove(&key);
                self.failed.remove(&key);
                self.last_plan.clear();
            }
        }
        self.pass = ctx.cumulative_pass_nr();
        let started = std::time::Instant::now();
        // No artificial two-images-per-frame throttle. Upload a ready batch,
        // bounded by both time and bytes to keep input/scroll responsive.
        let mut bytes = 0;
        let interacting = ctx.input(|i| {
            i.pointer.any_down()
                || i.smooth_scroll_delta.length_sq() > 0.1
                || i.multi_touch().is_some()
        });
        let upload_ms = if interacting { 2 } else { 5 };
        let upload_bytes = if interacting {
            4 * 1024 * 1024
        } else {
            16 * 1024 * 1024
        };
        // During motion the previous frame's visible set may already be obsolete.
        // Gallery submits its new plan and uploads visible results before draw.
        let batch = if interacting && !visible_only { 0 } else { 64 };
        for _ in 0..batch {
            if started.elapsed() > std::time::Duration::from_millis(upload_ms)
                || bytes >= upload_bytes
            {
                break;
            }
            let Some(ready) = self.jobs.next_ready(visible_only) else {
                break;
            };
            self.jobs.complete(&ready.key);
            self.pending.remove(&ready.key);
            if self.availability.get(&ready.key).is_some_and(|(ok, _)| !ok) != ready.unavailable {
                self.availability_changed = true;
                ctx.request_repaint();
            }
            self.availability.insert(
                ready.key.clone(),
                (!ready.unavailable, std::time::Instant::now()),
            );
            if let Some(pixels) = ready.pixels {
                bytes += pixels.pixels.len() * 4;
                let aspect = pixels.size[0] as f32 / pixels.size[1].max(1) as f32;
                if self.aspects.insert(ready.key.clone(), aspect) != Some(aspect) {
                    self.changed.insert(ready.key.clone(), aspect);
                    self.revision = self.revision.wrapping_add(1);
                }
                let size = pixels.pixels.len() * 4
                    + ready.frost.as_ref().map_or(0, |p| p.pixels.len() * 4);
                let texture = ctx.load_texture(&ready.key, pixels, egui::TextureOptions::LINEAR);
                self.touched.insert(ready.key.clone(), self.pass);
                self.last_use.insert(ready.key.clone(), self.pass);
                if let Some(old) = self.sizes.insert(ready.key.clone(), size) {
                    self.gpu_bytes = self.gpu_bytes.saturating_sub(old);
                }
                self.gpu_bytes += size;
                self.order.retain(|key| key != &ready.key);
                self.order.push_back(ready.key.clone());
                self.textures.insert(ready.key, texture);
                if let Some(pixels) = ready.frost {
                    let key = self.order.back().unwrap().clone();
                    self.frost.insert(
                        key.clone(),
                        ctx.load_texture(
                            format!("frost-{key}"),
                            pixels,
                            egui::TextureOptions::LINEAR,
                        ),
                    );
                }
            } else if !ready.unavailable {
                // Bound failed-item bookkeeping too. Refresh allows retrying.
                if self.failed.len() >= 4096 {
                    self.failed.clear();
                }
                self.failed.insert(ready.key);
            }
        }
        let evict_started = std::time::Instant::now();
        let mut evicted = 0;
        while self.order.len() > MAX_TEXTURES || self.gpu_bytes > GPU_BUDGET {
            if evicted >= 32 || evict_started.elapsed() > std::time::Duration::from_millis(1) {
                ctx.request_repaint();
                break;
            }
            let Some(index) = self
                .order
                .iter()
                .enumerate()
                .filter(|(_, key)| {
                    self.touched
                        .get(*key)
                        .is_none_or(|&pass| pass.saturating_add(2) < self.pass)
                        && !self.protected.contains(*key)
                })
                .min_by_key(|(_, key)| self.last_use.get(*key).copied().unwrap_or(0))
                .map(|(i, _)| i)
                .or_else(|| {
                    (self.order.len() > MAX_VISIBLE_TEXTURES)
                        .then(|| {
                            self.order
                                .iter()
                                .position(|key| !self.protected.contains(key))
                        })
                        .flatten()
                })
            else {
                break;
            };
            let key = self.order.remove(index).unwrap();
            self.textures.remove(&key);
            self.gpu_bytes = self
                .gpu_bytes
                .saturating_sub(self.sizes.remove(&key).unwrap_or(0));
            self.last_use.remove(&key);
            self.frost.remove(&key);
            self.touched.remove(&key);
            evicted += 1;
        }
        self.touched
            .retain(|key, _| self.textures.contains_key(key) || self.pending.contains(key));
        self.pending = self.jobs.pending();
        if !self.pending.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }

    pub fn retry(&mut self) {
        self.failed.clear();
        self.last_plan.clear();
        self.availability.clear();
    }

    pub fn get(&mut self, photo: &Photo) -> Option<egui::TextureHandle> {
        // Retain only identities for unavailable entries so they can recover
        // while absent from every rendered row (no get() calls after filtering).
        if self.unavailable(photo) {
            self.offline.insert(photo.key.clone(), photo.path.clone());
        }
        let status = self.availability.get(&photo.key).copied();
        if status.is_none_or(|(_, time)| time.elapsed() >= std::time::Duration::from_secs(2))
            && (self.textures.contains_key(&photo.key) || status.is_some_and(|(ok, _)| !ok))
        {
            self.offline.insert(photo.key.clone(), photo.path.clone());
            self.jobs.probe(Request {
                path: photo.path.clone(),
                key: photo.key.clone(),
            });
        }
        self.jobs.recheck_after(std::time::Duration::from_secs(2));
        if status.is_some_and(|(ok, _)| !ok) {
            return None;
        }
        if self.textures.contains_key(&photo.key) && status.is_none() {
            return None;
        }
        if self.touched.len() < MAX_VISIBLE_TEXTURES || self.touched.contains_key(&photo.key) {
            self.touched.insert(photo.key.clone(), self.pass);
        }
        if let Some(texture) = self.textures.get(&photo.key) {
            // Avoid O(resident textures) retain() for every visible image/frame.
            // The eviction timestamp is refreshed without walking the LRU list.
            self.last_use.insert(photo.key.clone(), self.pass);
            return Some(texture.clone());
        }
        if self.textures.len() + self.pending.len() < MAX_VISIBLE_TEXTURES
            && !self.pending.contains(&photo.key)
            && !self.failed.contains(&photo.key)
        {
            self.offline.insert(photo.key.clone(), photo.path.clone());
            self.jobs.visible(Request {
                path: photo.path.clone(),
                key: photo.key.clone(),
            });
            self.pending.insert(photo.key.clone());
        }
        None
    }

    pub(super) fn configure(&mut self, workers: usize) {
        self.jobs.configure(workers);
    }
    pub(super) fn build_all(&mut self, photos: &[Photo]) {
        self.jobs.build_all(
            photos
                .iter()
                .map(|p| Request {
                    path: p.path.clone(),
                    key: p.key.clone(),
                })
                .collect(),
        );
    }
    pub(super) fn disk_quota(&self, gib: u32) {
        self.jobs.disk_quota(gib);
    }
    pub(super) fn bulk_status(&self) -> super::thumbnail_jobs::BulkStatus {
        self.jobs.bulk_status()
    }
    pub(super) fn failures(&self) -> Vec<super::thumbnail_jobs::Failure> {
        self.jobs.failures()
    }
    pub(super) fn dismiss_bulk(&self) {
        self.jobs.dismiss_bulk();
    }
    pub(super) fn directory(&self) -> &Path {
        self.jobs.directory()
    }
    pub(super) fn pause_bulk(&self) {
        self.jobs.pause_bulk();
    }
    pub(super) fn cancel_bulk(&self) {
        self.jobs.cancel_bulk();
    }
    pub(super) fn forward(&mut self, offset: f32) -> bool {
        if (offset - self.scroll).abs() > 0.5 {
            self.forward = offset > self.scroll;
            self.scroll = offset;
        }
        self.forward
    }

    pub(super) fn plan(&mut self, photos: &[Photo], visible: &[usize], ahead: &[usize]) {
        let keys: Vec<_> = visible
            .iter()
            .chain(ahead)
            .take(MAX_QUEUED)
            .map(|&i| photos[i].key.clone())
            .collect();
        // Re-submit unfinished work even when the viewport didn't move, but
        // never rebuild an already-running identical queue every UI frame.
        if self.last_plan == keys && self.visible_count == visible.len() {
            return;
        }
        self.last_plan = keys;
        self.protected = self.last_plan.iter().cloned().collect();
        self.visible_count = visible.len();
        let request = |i: usize| {
            let p = &photos[i];
            (!self.textures.contains_key(&p.key)
                && !self.failed.contains(&p.key)
                && !self.unavailable(p))
            .then(|| Request {
                path: p.path.clone(),
                key: p.key.clone(),
            })
        };
        self.jobs.plan(
            visible.iter().filter_map(|&i| request(i)).collect(),
            ahead
                .iter()
                .take(MAX_QUEUED.saturating_sub(visible.len()))
                .filter_map(|&i| request(i))
                .collect(),
        );
        self.pending = self.jobs.pending();
    }

    pub fn failed(&self, photo: &Photo) -> bool {
        self.failed.contains(&photo.key)
    }
    pub(super) fn unavailable(&self, photo: &Photo) -> bool {
        self.availability.get(&photo.key).is_some_and(|(ok, _)| !ok)
    }

    pub(super) fn frost(&self, photo: &Photo) -> Option<&egui::TextureHandle> {
        if self.unavailable(photo) {
            return None;
        }
        self.frost.get(&photo.key)
    }

    pub(super) fn aspect(&self, photo: &Photo) -> f32 {
        self.aspects.get(&photo.key).copied().unwrap_or(1.5)
    }

    pub(super) fn restore_aspects(&mut self, saved: &HashMap<String, f32>) {
        for (key, &aspect) in saved {
            if aspect.is_finite() && aspect > 0.0 && !self.aspects.contains_key(key) {
                self.aspects.insert(key.clone(), aspect);
                self.revision = self.revision.wrapping_add(1);
            }
        }
    }

    pub(super) fn persist_aspects(&mut self, worker: &super::Worker) {
        if !self.changed.is_empty()
            && worker
                .send(super::Command::Aspects(
                    self.changed.iter().map(|(k, v)| (k.clone(), *v)).collect(),
                ))
                .is_ok()
        {
            self.changed.clear();
        }
    }

    pub(super) fn retain(&mut self, keys: &HashSet<String>) {
        self.aspects.retain(|key, _| keys.contains(key));
        self.availability.retain(|key, _| keys.contains(key));
        self.offline.retain(|key, _| keys.contains(key));
    }
}

// A small, genuinely blurred copy of the photo, never of the desktop. Prepared
// on the decode worker and sampled only underneath the translucent heart tile.
pub(super) fn frosted_pixels(pixels: &egui::ColorImage) -> egui::ColorImage {
    let rgba: Vec<u8> = pixels
        .pixels
        .iter()
        .flat_map(|p| p.to_srgba_unmultiplied())
        .collect();
    let image =
        image::RgbaImage::from_raw(pixels.size[0] as u32, pixels.size[1] as u32, rgba).unwrap();
    let small = DynamicImage::ImageRgba8(image).thumbnail(64, 64).to_rgba8();
    let blurred = image::imageops::blur(&small, 2.0);
    egui::ColorImage::from_rgba_unmultiplied(
        [blurred.width() as usize, blurred.height() as usize],
        blurred.as_raw(),
    )
}

pub(super) fn prune(directory: &Path, budget: u64) -> std::io::Result<()> {
    let _guard = DISK_MAINTENANCE.lock().unwrap_or_else(|e| e.into_inner());
    // Explicit full-library builds must not evict their own earlier thumbnails,
    // including after application restart. This affects only disk previews.
    let budget = if std::fs::read(directory.join(".keep-all")).is_ok_and(|bytes| bytes == KEEP_ALL)
    {
        u64::MAX
    } else {
        budget
    };
    let mut files = Vec::new();
    let mut total = 0_u64;
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // Only files generated by this cache, never arbitrary neighboring files.
        if !name.is_ascii()
            || name.len() != 68
            || !name.ends_with(".png")
            || !name[..64].bytes().all(|b| b.is_ascii_hexdigit())
        {
            continue;
        }
        let metadata = entry.metadata()?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        total = total.saturating_add(metadata.len());
        files.push((
            metadata.modified().unwrap_or(std::time::UNIX_EPOCH),
            entry.path(),
            metadata.len(),
        ));
    }
    files.sort_by_key(|(time, _, _)| *time);
    for (used, path, size) in files {
        let age = used.elapsed().unwrap_or_default();
        // A quota is soft for recently used previews. Unlimited keeps the same
        // 90-day expiry rather than retaining abandoned previews forever.
        if age < std::time::Duration::from_secs(7 * 86400) {
            continue;
        }
        if total <= budget && age < std::time::Duration::from_secs(90 * 86400) {
            continue;
        }
        if std::fs::remove_file(path).is_ok() {
            // rphotos-cache-prune: generated thumbnails only
            total = total.saturating_sub(size);
        }
    }
    Ok(())
}

static DISK_MAINTENANCE: std::sync::Mutex<()> = std::sync::Mutex::new(());
const KEEP_ALL: &[u8] =
    b"Rphotos explicit full thumbnail cache; automatic size eviction disabled\n";
pub(super) fn keep_all(directory: &Path) -> std::io::Result<()> {
    let _guard = DISK_MAINTENANCE.lock().unwrap_or_else(|e| e.into_inner());
    std::fs::create_dir_all(directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
    }
    std::fs::write(directory.join(".keep-all"), KEEP_ALL)
}
pub(super) fn apply_disk_quota(directory: &Path, budget: u64) {
    {
        let _guard = DISK_MAINTENANCE.lock().unwrap_or_else(|e| e.into_inner());
        let marker = directory.join(".keep-all");
        if marker.is_file() {
            let _ = std::fs::write(
                marker,
                if budget == 0 {
                    KEEP_ALL
                } else {
                    b"Rphotos finite disk cache quota enabled\n"
                },
            );
        }
    }
    if budget > 0 {
        let _ = prune(directory, budget);
    }
}

pub(super) struct Loaded {
    pub pixels: egui::ColorImage,
    pub write_error: Option<String>,
}

#[derive(Debug)]
pub(super) struct Unavailable;
impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Original is currently unavailable")
    }
}
impl std::error::Error for Unavailable {}

pub(super) fn original_available(path: &Path, key: &str) -> bool {
    std::fs::File::open(path)
        .and_then(|file| file.metadata())
        .is_ok_and(|m| {
            m.is_file()
                && m.modified()
                    .is_ok_and(|time| cache_key(path, m.len(), time) == key)
        })
}

pub(super) fn touch_cached(directory: &Path, key: &str) {
    let path = directory.join(format!("{key}.png"));
    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(path) {
        if file
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age.as_secs() > 3600)
        {
            let _ = file.set_modified(std::time::SystemTime::now());
        }
    }
}

pub(super) fn load_detailed(directory: &Path, path: &Path, key: &str) -> anyhow::Result<Loaded> {
    use anyhow::{bail, Context};
    if !original_available(path, key) {
        return Err(Unavailable.into());
    }
    let cached = directory.join(format!("{key}.png"));
    if let Ok(image) = image::open(&cached) {
        if image.width() <= 400 && image.height() <= 400 {
            if !original_available(path, key) {
                return Err(Unavailable.into());
            }
            touch_cached(directory, key);
            return Ok(Loaded {
                pixels: crate::decode::image_to_thumbnail(
                    image,
                    image::metadata::Orientation::NoTransforms,
                ),
                write_error: None,
            });
        }
    }
    let metadata = path
        .metadata()
        .context("Cannot read original file metadata")?;
    if metadata.len() == 0 {
        bail!("Original file is empty (0 bytes); no image data to decode");
    }
    if cache_key(
        path,
        metadata.len(),
        metadata
            .modified()
            .context("Cannot read original modification time")?,
    ) != key
    {
        bail!("Original changed since indexing; refresh the library (F5)");
    }
    let loaded = crate::file_io::load_image(path);
    let pixels = crate::decode::image_to_thumbnail(
        loaded.image.context("Cannot decode original image")?,
        loaded.orientation,
    );
    let metadata = path
        .metadata()
        .context("Cannot recheck original after decoding")?;
    if cache_key(
        path,
        metadata.len(),
        metadata
            .modified()
            .context("Cannot recheck original modification time")?,
    ) != key
    {
        bail!("Original changed while decoding; refresh the library (F5)");
    }
    // Cache failure does not prevent viewing. Never write in the source folder.
    let write = || -> anyhow::Result<()> {
        std::fs::create_dir_all(directory).context("Cannot create thumbnail cache directory")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
                .context("Cannot protect thumbnail cache directory")?;
        }
        let rgba: Vec<u8> = pixels
            .pixels
            .iter()
            .flat_map(|c| c.to_srgba_unmultiplied())
            .collect();
        let image = image::RgbaImage::from_raw(pixels.size[0] as u32, pixels.size[1] as u32, rgba)
            .context("Invalid thumbnail pixel dimensions")?;
        let mut temporary = tempfile::NamedTempFile::new_in(directory)
            .context("Cannot create temporary cache file")?;
        DynamicImage::ImageRgba8(image)
            .write_to(temporary.as_file_mut(), ImageFormat::Png)
            .context("Cannot encode/write cached thumbnail (check free space and permissions)")?;
        temporary
            .persist(cached)
            .context("Cannot persist cached thumbnail")?;
        Ok(())
    };
    let write_error = write().err().map(|e| format!("{e:#}"));
    Ok(Loaded {
        pixels,
        write_error,
    })
}

#[cfg(test)]
fn load(directory: &Path, path: &Path, key: &str) -> Option<egui::ColorImage> {
    load_detailed(directory, path, key)
        .ok()
        .map(|loaded| loaded.pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_original_hides_resident_and_persistent_preview_then_recovers_without_decode() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("photo.png");
        image::RgbImage::from_pixel(80, 60, image::Rgb([20, 100, 180]))
            .save(&path)
            .unwrap();
        let metadata = path.metadata().unwrap();
        let photo = Photo {
            id: 1,
            path: path.clone(),
            key: cache_key(&path, metadata.len(), metadata.modified().unwrap()),
            modified: 0,
            taken: None,
            albums: vec![],
            text: String::new(),
        };
        let cache = temp.path().join("cache");
        let ctx = egui::Context::default();
        let mut thumbs = Thumbnails::new(cache.clone(), &ctx);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        loop {
            thumbs.poll(&ctx);
            if thumbs.get(&photo).is_some() {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let cached = cache.join(format!("{}.png", photo.key));
        let bytes = std::fs::read(&cached).unwrap();
        let offline = temp.path().join("offline.png");
        std::fs::rename(&path, &offline).unwrap();
        thumbs.availability.get_mut(&photo.key).unwrap().1 =
            std::time::Instant::now() - std::time::Duration::from_secs(3);
        loop {
            let _ = thumbs.get(&photo);
            thumbs.poll(&ctx);
            if thumbs.unavailable(&photo) {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(thumbs.get(&photo).is_none());
        assert!(!thumbs.failed(&photo));
        assert!(thumbs.frost(&photo).is_none());
        assert!(load_detailed(&cache, &path, &photo.key)
            .err()
            .unwrap()
            .is::<Unavailable>());
        assert_eq!(std::fs::read(&cached).unwrap(), bytes);
        drop(thumbs);
        std::fs::rename(&offline, &path).unwrap();
        let mut restarted = Thumbnails::new(cache.clone(), &ctx);
        loop {
            restarted.poll(&ctx);
            if restarted.get(&photo).is_some() {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(std::fs::read(&cached).unwrap(), bytes);
    }

    #[test]
    fn disk_cache_expires_only_old_entries_and_touch_renews_usage() {
        let temp = tempfile::tempdir().unwrap();
        let now = std::time::SystemTime::now();
        for (key, days) in [("a", 91), ("b", 20), ("c", 0), ("d", 91)] {
            let path = temp.path().join(format!("{}.png", key.repeat(64)));
            std::fs::write(&path, b"compressed preview").unwrap();
            std::fs::OpenOptions::new()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(now - std::time::Duration::from_secs(days * 86400))
                .unwrap();
        }
        touch_cached(temp.path(), &"d".repeat(64));
        keep_all(temp.path()).unwrap();
        prune(temp.path(), 0).unwrap();
        assert!(!temp.path().join(format!("{}.png", "a".repeat(64))).exists());
        for key in ["b", "c", "d"] {
            assert!(temp.path().join(format!("{}.png", key.repeat(64))).exists());
        }
        apply_disk_quota(temp.path(), 1);
        assert!(!temp.path().join(format!("{}.png", "b".repeat(64))).exists());
        for key in ["c", "d"] {
            assert!(temp.path().join(format!("{}.png", key.repeat(64))).exists());
        }
    }

    #[test]
    fn detailed_failures_distinguish_decode_change_and_cache_write_without_touching_original() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("image.png");
        image::RgbImage::new(16, 12).save(&path).unwrap();
        let original = std::fs::read(&path).unwrap();
        let m = path.metadata().unwrap();
        let key = cache_key(&path, m.len(), m.modified().unwrap());
        let blocked = temp.path().join("not-a-directory");
        std::fs::write(&blocked, b"unrelated file").unwrap();
        let loaded = load_detailed(&blocked, &path, &key).unwrap();
        assert!(loaded
            .write_error
            .unwrap()
            .contains("Cannot create thumbnail cache directory"));
        assert_eq!(loaded.pixels.size, [16, 12]);
        let error = load_detailed(&temp.path().join("cache"), &path, "stale")
            .err()
            .unwrap();
        assert!(error.is::<Unavailable>());
        let corrupt = temp.path().join("corrupt.jpg");
        std::fs::write(&corrupt, b"not a JPEG").unwrap();
        let m = corrupt.metadata().unwrap();
        let key = cache_key(&corrupt, m.len(), m.modified().unwrap());
        assert!(format!(
            "{:#}",
            load_detailed(&temp.path().join("cache"), &corrupt, &key)
                .err()
                .unwrap()
        )
        .contains("Cannot decode original"));
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(std::fs::read(&blocked).unwrap(), b"unrelated file");
    }

    #[test]
    #[ignore = "explicit private-index diagnostic; RPHOTOS_DIAGNOSE_DATABASE/CACHE required"]
    fn diagnose_missing_cached_thumbnails() {
        let database =
            std::env::var_os("RPHOTOS_DIAGNOSE_DATABASE").expect("explicit database required");
        let cache = PathBuf::from(
            std::env::var_os("RPHOTOS_DIAGNOSE_CACHE").expect("explicit cache required"),
        );
        let db = rusqlite::Connection::open_with_flags(
            database,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let state = super::super::store::snapshot(&db).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let mut summary = std::collections::BTreeMap::<String, usize>::new();
        let mut failures = Vec::new();
        for photo in state.photos {
            if cache.join(format!("{}.png", photo.key)).is_file() {
                continue;
            }
            let result =
                std::panic::catch_unwind(|| load_detailed(temp.path(), &photo.path, &photo.key));
            let reason = match result {
                Ok(Ok(loaded)) => loaded
                    .write_error
                    .unwrap_or_else(|| "Decode succeeded; cache was missing".into()),
                Ok(Err(e)) => format!("{e:#}"),
                Err(_) => "Image decoder panicked".into(),
            };
            // Private paths belong only in the local report, never stdout.
            let category = reason.replace(&photo.path.to_string_lossy().to_string(), "<original>");
            *summary.entry(category).or_default() += 1;
            failures.push(super::super::thumbnail_jobs::Failure {
                path: photo.path,
                reason,
            });
        }
        let report =
            std::env::var_os("RPHOTOS_DIAGNOSE_REPORT").expect("explicit private report required");
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(report)
            .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))
                .unwrap();
        }
        serde_json::to_writer_pretty(file, &failures).unwrap();
        println!("Missing preview diagnostic (originals unchanged): {summary:?}");
    }

    #[test]
    fn entire_library_build_persists_beyond_quota_and_reuses_without_originals() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let originals = temp.path().join("originals");
        std::fs::create_dir(&originals).unwrap();
        let mut photos: Vec<_> = (0..80)
            .map(|id| {
                let path = originals.join(format!("{id}.png"));
                image::RgbImage::from_pixel(120, 80, image::Rgb([id as u8, 100, 190]))
                    .save(&path)
                    .unwrap();
                let metadata = path.metadata().unwrap();
                Photo {
                    id,
                    path: path.clone(),
                    modified: 0,
                    taken: None,
                    key: cache_key(&path, metadata.len(), metadata.modified().unwrap()),
                    albums: vec![],
                    text: String::new(),
                }
            })
            .collect();
        photos.push(Photo {
            id: 80,
            path: originals.join("missing.jpg"),
            modified: 0,
            taken: None,
            key: format!("{:064x}", 81),
            albums: vec![],
            text: String::new(),
        });
        let cache = temp.path().join("cache");
        let thumbnails = Thumbnails::new(cache.clone(), &ctx);
        thumbnails.jobs.build_all(
            photos
                .iter()
                .map(|p| Request {
                    path: p.path.clone(),
                    key: p.key.clone(),
                })
                .collect(),
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        // No poll/GPU upload: full builds must not block on the32-result channel.
        while thumbnails.bulk_status().running {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let status = thumbnails.bulk_status();
        assert_eq!(status.done, 81);
        assert_eq!(status.failed, 0);
        assert_eq!(status.skipped, 1);
        let failures = thumbnails.failures();
        assert!(failures.is_empty());
        let deadline_report = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !cache.join("last-build-errors.json").is_file() {
            assert!(std::time::Instant::now() < deadline_report);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let report: Vec<super::super::thumbnail_jobs::Failure> =
            serde_json::from_slice(&std::fs::read(cache.join("last-build-errors.json")).unwrap())
                .unwrap();
        assert!(report.is_empty());
        assert!(thumbnails.textures.is_empty());
        prune(&cache, 0).unwrap(); // Explicit cache survives even a zero quota.
        assert!(cache.join(format!("{}.png", photos[0].key)).is_file());
        apply_disk_quota(&cache, 16 * 1024 * 1024 * 1024);
        assert_ne!(
            std::fs::read(cache.join(".keep-all")).unwrap(),
            KEEP_ALL,
            "finite quota can be restored after a full build"
        );
        keep_all(&cache).unwrap();
        std::fs::rename(&originals, temp.path().join("offline-originals")).unwrap();
        for photo in &photos[..80] {
            assert!(load(&cache, &photo.path, &photo.key).is_none());
            assert!(cache.join(format!("{}.png", photo.key)).is_file());
        }
        drop(thumbnails);
        let restarted = Thumbnails::new(cache.clone(), &ctx);
        restarted.jobs.build_all(
            photos[..80]
                .iter()
                .map(|p| Request {
                    path: p.path.clone(),
                    key: p.key.clone(),
                })
                .collect(),
        );
        while restarted.bulk_status().running {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(restarted.bulk_status().failed, 0);
        assert_eq!(restarted.bulk_status().skipped, 80);
        std::fs::rename(temp.path().join("offline-originals"), &originals).unwrap();
        for photo in &photos[..80] {
            assert!(load(&cache, &photo.path, &photo.key).is_some());
        }
    }

    #[test]
    fn resident_cache_eviction_keeps_recent_backscroll_and_accounting() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut thumbs = Thumbnails::new(temp.path().join("cache"), &ctx);
        for id in 0..2200 {
            let key = format!("{id:064x}");
            thumbs.textures.insert(
                key.clone(),
                ctx.load_texture(
                    &key,
                    egui::ColorImage::new([2, 2], egui::Color32::WHITE),
                    egui::TextureOptions::LINEAR,
                ),
            );
            thumbs.sizes.insert(key.clone(), 16);
            thumbs.gpu_bytes += 16;
            thumbs.order.push_back(key.clone());
            thumbs.last_use.insert(key, if id == 0 { 100 } else { 0 });
        }
        for _ in 0..400 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| thumbs.poll(ctx));
            if thumbs.textures.len() == MAX_TEXTURES {
                break;
            }
        }
        assert_eq!(thumbs.textures.len(), MAX_TEXTURES);
        assert!(
            thumbs.textures.contains_key(&format!("{:064x}", 0)),
            "recently revisited item survives oldest-first eviction"
        );
        assert_eq!(thumbs.gpu_bytes, thumbs.sizes.values().sum::<usize>());
        assert_eq!(thumbs.gpu_bytes, MAX_TEXTURES * 16);
    }

    #[test]
    fn parallel_preload_finishes_offscreen_photos_before_they_are_requested() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let photos: Vec<_> = (0..80)
            .map(|id| {
                let path = temp.path().join(format!("{id}.png"));
                image::RgbImage::from_pixel(120, 80, image::Rgb([id as u8, 90, 150]))
                    .save(&path)
                    .unwrap();
                let m = path.metadata().unwrap();
                Photo {
                    id,
                    path: path.clone(),
                    key: cache_key(&path, m.len(), m.modified().unwrap()),
                    modified: 0,
                    taken: None,
                    albums: vec![],
                    text: String::new(),
                }
            })
            .collect();
        let mut thumbnails = Thumbnails::new(temp.path().join("cache"), &ctx);
        thumbnails.configure(4);
        thumbnails.plan(
            &photos,
            &(0..10).collect::<Vec<_>>(),
            &(10..80).collect::<Vec<_>>(),
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while thumbnails.textures.len() < 80 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| thumbnails.poll(ctx));
            assert!(
                std::time::Instant::now() < deadline,
                "preload stalled: {} textures",
                thumbnails.textures.len()
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        // No get() has ever been issued for these future rows.
        for photo in &photos[10..] {
            assert!(thumbnails.get(photo).is_some());
        }
        assert_eq!(thumbnails.pending_count(), 0);
        assert_eq!(image::open(&photos[0].path).unwrap().width(), 120);
    }

    #[test]
    #[ignore = "thumbnail throughput probe; explicitly run with --nocapture"]
    fn thumbnail_throughput_probe() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let photos: Vec<_> = (0..120)
            .map(|id| {
                let path = temp.path().join(format!("{id}.jpg"));
                image::RgbImage::from_fn(1200, 800, |x, y| {
                    image::Rgb([
                        ((x + id) % 256) as u8,
                        ((y + id * 11) % 256) as u8,
                        ((x / 3 + y / 2) % 256) as u8,
                    ])
                })
                .save(&path)
                .unwrap();
                let m = path.metadata().unwrap();
                Photo {
                    id: i64::from(id),
                    path: path.clone(),
                    key: cache_key(&path, m.len(), m.modified().unwrap()),
                    modified: 0,
                    taken: None,
                    albums: vec![],
                    text: String::new(),
                }
            })
            .collect();
        for workers in [1, super::super::thumbnail_jobs::auto_workers()] {
            let mut thumbnails =
                Thumbnails::new(temp.path().join(format!("cache-{workers}")), &ctx);
            thumbnails.configure(workers);
            let start = std::time::Instant::now();
            thumbnails.plan(
                &photos,
                &(0..24).collect::<Vec<_>>(),
                &(24..120).collect::<Vec<_>>(),
            );
            while thumbnails.textures.len() < 120 {
                let _ = ctx.run(egui::RawInput::default(), |ctx| thumbnails.poll(ctx));
                assert!(start.elapsed() < std::time::Duration::from_secs(120));
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            println!("preview workers={workers}, 120 JPEG1200x800, fresh thumbnail cache: {:.3}s, {:.1}photos/s (warm OS cache, includes resize/frost/PNG write/texture registration, no GPU submit)",start.elapsed().as_secs_f64(),120.0/start.elapsed().as_secs_f64());
        }
    }

    #[test]
    fn small_grid_keeps_more_than_128_visible_textures_without_reload_cycle() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut thumbnails = Thumbnails::new(temp.path().join("cache"), &ctx);
        let photos: Vec<_> = (0..200)
            .map(|i| Photo {
                id: i,
                path: temp.path().join(format!("{i}.png")),
                modified: 0,
                taken: None,
                key: format!("{i:064x}"),
                albums: vec![],
                text: String::new(),
            })
            .collect();
        for photo in &photos {
            thumbnails.textures.insert(
                photo.key.clone(),
                ctx.load_texture(
                    &photo.key,
                    egui::ColorImage::new([2, 2], egui::Color32::WHITE),
                    egui::TextureOptions::LINEAR,
                ),
            );
            thumbnails
                .availability
                .insert(photo.key.clone(), (true, std::time::Instant::now()));
            thumbnails.order.push_back(photo.key.clone());
        }
        for frame in 0..20 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                if frame > 0 {
                    thumbnails.poll(ctx);
                }
                for photo in &photos {
                    assert!(
                        thumbnails.get(photo).is_some(),
                        "visible texture evicted on frame {frame}"
                    );
                }
            });
            assert_eq!(thumbnails.pending_count(), 0);
        }
        assert_eq!(thumbnails.textures.len(), 200);
        for _ in 0..5 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                thumbnails.poll(ctx);
            });
        }
        assert_eq!(
            thumbnails.textures.len(),
            200,
            "recent offscreen photos remain resident for immediate backscroll"
        );
    }

    #[test]
    fn disk_cache_reuses_pixels_and_stale_key_is_not_written() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("original.png");
        image::RgbImage::from_pixel(800, 200, image::Rgb([90, 120, 150]))
            .save(&path)
            .unwrap();
        let meta = path.metadata().unwrap();
        let key = cache_key(&path, meta.len(), meta.modified().unwrap());
        let cache = temp.path().join("cache");
        let first = load(&cache, &path, &key).unwrap();
        assert_eq!(first.size, [400, 100]);
        assert!(cache.join(format!("{key}.png")).exists());
        assert!(load(&cache, &path, "invalid-key").is_none());
        let second = load(&cache, &path, &key).unwrap();
        assert_eq!(first.pixels, second.pixels);
        assert_eq!(image::open(path).unwrap().width(), 800);
    }
}
#[test]
fn disk_budget_only_removes_our_thumbnails() {
    let t = tempfile::tempdir().unwrap();
    let private = t.path().join("personal.png");
    std::fs::write(&private, b"preserve").unwrap();
    let cache = t.path().join(format!("{}.png", "a".repeat(64)));
    std::fs::write(&cache, b"cache").unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&cache)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(91 * 86400))
        .unwrap();
    prune(t.path(), 0).unwrap();
    assert!(!cache.exists());
    assert!(private.exists());
}
