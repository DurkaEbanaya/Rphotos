//! On-demand disk thumbnails, one decoder, bounded queue and GPU LRU.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use eframe::egui;
use image::{DynamicImage, ImageFormat};

use super::store::{cache_key, Photo};

const MAX_TEXTURES: usize = 128;
const MAX_REQUESTS: usize = 24;

struct Request {
    path: PathBuf,
    key: String,
}
struct Ready {
    key: String,
    pixels: Option<egui::ColorImage>,
}

pub struct Thumbnails {
    requests: mpsc::SyncSender<Request>,
    results: mpsc::Receiver<Ready>,
    textures: HashMap<String, egui::TextureHandle>,
    order: VecDeque<String>,
    pending: HashSet<String>,
    failed: HashSet<String>,
    aspects: HashMap<String, f32>,
    changed: HashMap<String, f32>,
    pub(super) revision: u64,
}

impl Thumbnails {
    #[cfg(test)]
    pub(super) fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn new(directory: PathBuf, ctx: &egui::Context) -> Self {
        let (requests, input) = mpsc::sync_channel::<Request>(MAX_REQUESTS);
        let (output, results) = mpsc::sync_channel(4);
        let wake = ctx.clone();
        std::thread::spawn(move || {
            // Conservative fixed 512 MiB disk budget, enforced off the UI thread.
            let _ = prune(&directory, 512 * 1024 * 1024);
            let mut written = 0;
            while let Ok(request) = input.recv() {
                // A decoder panic must complete this request as a failed preview,
                // not strand it in `pending` or terminate the only thumbnail worker.
                let pixels =
                    std::panic::catch_unwind(|| load(&directory, &request.path, &request.key))
                        .ok()
                        .flatten();
                written += 1;
                if written % 64 == 0 {
                    let _ = prune(&directory, 512 * 1024 * 1024);
                }
                if output
                    .send(Ready {
                        key: request.key,
                        pixels,
                    })
                    .is_err()
                {
                    break;
                }
                wake.request_repaint();
            }
        });
        Self {
            requests,
            results,
            textures: HashMap::new(),
            order: VecDeque::new(),
            pending: HashSet::new(),
            failed: HashSet::new(),
            aspects: HashMap::new(),
            changed: HashMap::new(),
            revision: 0,
        }
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        for _ in 0..2 {
            let Ok(ready) = self.results.try_recv() else {
                break;
            };
            self.pending.remove(&ready.key);
            if let Some(pixels) = ready.pixels {
                let aspect = pixels.size[0] as f32 / pixels.size[1].max(1) as f32;
                if self.aspects.insert(ready.key.clone(), aspect) != Some(aspect) {
                    self.changed.insert(ready.key.clone(), aspect);
                    self.revision = self.revision.wrapping_add(1);
                }
                let texture = ctx.load_texture(&ready.key, pixels, egui::TextureOptions::LINEAR);
                self.order.push_back(ready.key.clone());
                self.textures.insert(ready.key, texture);
                while self.order.len() > MAX_TEXTURES {
                    if let Some(key) = self.order.pop_front() {
                        self.textures.remove(&key);
                    }
                }
            } else {
                // Bound failed-item bookkeeping too. Refresh allows retrying.
                if self.failed.len() >= 4096 {
                    self.failed.clear();
                }
                self.failed.insert(ready.key);
            }
        }
        if !self.pending.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    pub fn retry(&mut self) {
        self.failed.clear();
    }

    pub fn get(&mut self, photo: &Photo) -> Option<egui::TextureHandle> {
        if let Some(texture) = self.textures.get(&photo.key) {
            self.order.retain(|k| k != &photo.key);
            self.order.push_back(photo.key.clone());
            return Some(texture.clone());
        }
        if self.pending.len() < MAX_REQUESTS
            && !self.pending.contains(&photo.key)
            && !self.failed.contains(&photo.key)
            && self
                .requests
                .try_send(Request {
                    path: photo.path.clone(),
                    key: photo.key.clone(),
                })
                .is_ok()
        {
            self.pending.insert(photo.key.clone());
        }
        None
    }

    pub fn failed(&self, photo: &Photo) -> bool {
        self.failed.contains(&photo.key)
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
    }
}

fn prune(directory: &Path, budget: u64) -> std::io::Result<()> {
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
    for (_, path, size) in files {
        if total <= budget {
            break;
        }
        if std::fs::remove_file(path).is_ok() {
            // rphotos-cache-prune: generated thumbnails only
            total = total.saturating_sub(size);
        }
    }
    Ok(())
}

fn load(directory: &Path, path: &Path, key: &str) -> Option<egui::ColorImage> {
    let cached = directory.join(format!("{key}.png"));
    if let Ok(image) = image::open(&cached) {
        if image.width() <= 400 && image.height() <= 400 {
            return Some(crate::decode::image_to_thumbnail(
                image,
                image::metadata::Orientation::NoTransforms,
            ));
        }
    }
    let metadata = path.metadata().ok()?;
    if cache_key(path, metadata.len(), metadata.modified().ok()?) != key {
        return None; // File changed since indexing: do not poison its previous key.
    }
    let loaded = crate::file_io::load_image(path);
    let pixels = crate::decode::image_to_thumbnail(loaded.image.ok()?, loaded.orientation);
    let metadata = path.metadata().ok()?;
    if cache_key(path, metadata.len(), metadata.modified().ok()?) != key {
        return None; // Also reject a file that changed while the decoder read it.
    }
    // Cache failure does not prevent viewing. Never write in the source folder.
    if std::fs::create_dir_all(directory).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700));
        }
        let rgba: Vec<u8> = pixels
            .pixels
            .iter()
            .flat_map(|c| c.to_srgba_unmultiplied())
            .collect();
        if let Some(image) =
            image::RgbaImage::from_raw(pixels.size[0] as u32, pixels.size[1] as u32, rgba)
        {
            let temporary = cached.with_extension("tmp");
            if DynamicImage::ImageRgba8(image)
                .save_with_format(&temporary, ImageFormat::Png)
                .is_ok()
            {
                let _ = std::fs::rename(temporary, cached);
            }
        }
    }
    Some(pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    prune(t.path(), 0).unwrap();
    assert!(!cache.exists());
    assert!(private.exists());
}
