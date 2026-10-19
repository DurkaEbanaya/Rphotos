//! Bounded, reprioritizable preview work. UI never waits for an I/O lock held
//! during decode. Visible jobs always precede speculative neighboring screens.
use super::thumbnails::{frosted_pixels, keep_all, load_detailed, prune};
use eframe::egui;
use std::{
    collections::{HashSet, VecDeque},
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, Condvar, Mutex,
    },
    thread,
};

pub(super) const MAX_QUEUED: usize = 2048;
const MAX_WORKERS: usize = 24;
#[derive(Clone)]
pub(super) struct Request {
    pub path: PathBuf,
    pub key: String,
}
pub(super) struct Ready {
    pub key: String,
    pub pixels: Option<egui::ColorImage>,
    pub frost: Option<egui::ColorImage>,
    pub unavailable: bool,
}
#[derive(Default)]
struct Queue {
    visible: VecDeque<Request>,
    ahead: VecDeque<Request>,
    running: HashSet<String>,
    stop: bool,
    active: usize,
    bulk: VecDeque<Request>,
    bulk_status: BulkStatus,
    bulk_generation: u64,
    quota_change: Option<u64>,
    ready: VecDeque<Ready>,
    wanted: HashSet<String>,
    visible_keys: HashSet<String>,
    failures: Vec<Failure>,
    probes: VecDeque<Request>,
    probing: HashSet<String>,
    availability: std::collections::HashMap<String, bool>,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(super) struct Failure {
    pub path: PathBuf,
    pub reason: String,
}
#[derive(Clone, Copy, Default, Debug)]
pub(super) struct BulkStatus {
    pub total: usize,
    pub done: usize,
    pub failed: usize,
    pub skipped: usize,
    pub running: bool,
    pub paused: bool,
}
struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
    completed: AtomicUsize,
    disk_budget: AtomicU64,
}
pub(super) struct Jobs {
    shared: Arc<Shared>,
    directory: PathBuf,
    ctx: egui::Context,
    spawned: usize,
}

pub(super) fn auto_workers() -> usize {
    thread::available_parallelism()
        .map_or(2, usize::from)
        .saturating_sub(1)
        .clamp(1, 16)
}

fn save_failures(directory: &std::path::Path, failures: &[Failure]) -> anyhow::Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(directory)?;
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    file.write_all(&serde_json::to_vec_pretty(failures)?)?;
    file.persist(directory.join("last-build-errors.json"))?;
    Ok(())
}

impl Jobs {
    pub fn new(directory: PathBuf, ctx: &egui::Context) -> Self {
        // Don't prune before saved user quota has been applied by the UI.
        let mut jobs = Self {
            shared: Arc::new(Shared {
                queue: Mutex::new(Queue::default()),
                wake: Condvar::new(),
                completed: AtomicUsize::new(0),
                disk_budget: AtomicU64::new(u64::MAX),
            }),
            directory,
            ctx: ctx.clone(),
            spawned: 0,
        };
        jobs.configure(0);
        jobs
    }
    pub fn configure(&mut self, workers: usize) {
        let count = if workers == 0 {
            auto_workers()
        } else {
            workers.clamp(1, MAX_WORKERS)
        };
        self.shared.queue.lock().unwrap().active = count;
        while self.spawned < count {
            let index = self.spawned;
            self.spawned += 1;
            let shared = self.shared.clone();
            let directory = self.directory.clone();
            let ctx = self.ctx.clone();
            thread::Builder::new()
                .name(format!("rphotos-preview-{index}"))
                .spawn(move || {
                    if index == 0 {
                        let _ = prune(&directory, u64::MAX);
                        if let Ok(bytes) = std::fs::read(directory.join("last-build-errors.json")) {
                            if bytes.len() <= 4 * 1024 * 1024 {
                                if let Ok(failures) = serde_json::from_slice::<Vec<Failure>>(&bytes)
                                {
                                    let mut q = shared.queue.lock().unwrap();
                                    if q.bulk_generation == 0 {
                                        q.failures = failures.into_iter().take(4096).collect();
                                    }
                                }
                            }
                        }
                    }
                    loop {
                        let request = {
                            let mut q = shared.queue.lock().unwrap();
                            loop {
                                if q.stop {
                                    return;
                                }
                                if let Some(budget) = q.quota_change.take() {
                                    drop(q);
                                    super::thumbnails::apply_disk_quota(&directory, budget);
                                    q = shared.queue.lock().unwrap();
                                    continue;
                                }
                                if index < q.active {
                                    if let Some(request) = q.probes.pop_front() {
                                        drop(q);
                                        let available = super::thumbnails::original_available(
                                            &request.path,
                                            &request.key,
                                        );
                                        if available {
                                            super::thumbnails::touch_cached(
                                                &directory,
                                                &request.key,
                                            );
                                        }
                                        q = shared.queue.lock().unwrap();
                                        q.probing.remove(&request.key);
                                        q.availability.insert(request.key, available);
                                        ctx.request_repaint();
                                        continue;
                                    }
                                    if let Some(request) =
                                        q.visible.pop_front().or_else(|| q.ahead.pop_front())
                                    {
                                        q.running.insert(request.key.clone());
                                        break (request, None);
                                    }
                                    if q.bulk_status.running && !q.bulk_status.paused {
                                        if let Some(index) =
                                            q.bulk.iter().position(|r| !q.running.contains(&r.key))
                                        {
                                            let request = q.bulk.remove(index).unwrap();
                                            q.running.insert(request.key.clone());
                                            let generation = q.bulk_generation;
                                            break (request, Some(generation));
                                        }
                                    }
                                }
                                q = shared.wake.wait(q).unwrap();
                            }
                        };
                        let (request, bulk) = request;
                        // Limit concurrent full-size decodes of huge/unknown images.
                        // Small previews still saturate the pool; this prevents a
                        // folder of giant TIFFs multiplying peak memory by24.
                        let cached = directory.join(format!("{}.png", request.key)).is_file();
                        let large = !cached
                            && (std::fs::metadata(&request.path)
                                .map_or(true, |m| m.len() > 64 * 1024 * 1024)
                                || image::image_dimensions(&request.path)
                                    .map_or(true, |(w, h)| {
                                        u64::from(w) * u64::from(h) > 48_000_000
                                    }));
                        static LARGE: Mutex<()> = Mutex::new(());
                        let guard = large.then(|| LARGE.lock().unwrap_or_else(|e| e.into_inner()));
                        let decoded = std::panic::catch_unwind(|| -> anyhow::Result<_> {
                            if bulk.is_some() && !directory.join(".keep-all").is_file() {
                                keep_all(&directory).map_err(|e| {
                                    anyhow::anyhow!("Cannot preserve full cache: {e}")
                                })?;
                            }
                            let loaded = load_detailed(&directory, &request.path, &request.key)?;
                            let frost = if bulk.is_none() {
                                Some(frosted_pixels(&loaded.pixels))
                            } else {
                                None
                            };
                            Ok((loaded, frost))
                        })
                        .unwrap_or_else(|_| Err(anyhow::anyhow!("Image decoder panicked")));
                        drop(guard);
                        let key = request.key;
                        if let Some(generation) = bulk {
                            // load() deliberately tolerates cache write failures
                            // for interactive viewing. A bulk build must report
                            // those as failures, not falsely claim persistence.
                            let unavailable = decoded
                                .as_ref()
                                .err()
                                .is_some_and(|e| e.is::<super::thumbnails::Unavailable>());
                            let failure = match decoded {
                                Ok((loaded, _)) => loaded.write_error,
                                Err(_) if unavailable => None,
                                Err(error) => Some(format!("{error:#}")),
                            };
                            let mut q = shared.queue.lock().unwrap();
                            q.running.remove(&key);
                            if generation == q.bulk_generation {
                                q.bulk_status.done += 1;
                                if unavailable {
                                    q.bulk_status.skipped += 1;
                                }
                                if let Some(reason) = failure {
                                    q.bulk_status.failed += 1;
                                    if q.failures.len() < 4096 {
                                        q.failures.push(Failure {
                                            path: request.path,
                                            reason,
                                        });
                                    }
                                }
                                if q.bulk_status.done == q.bulk_status.total {
                                    q.bulk_status.running = false;
                                }
                            }
                            // Persist outside the queue lock and without publishing
                            // private file paths in application logs.
                            let report = (!q.bulk_status.running
                                && generation == q.bulk_generation)
                                .then(|| q.failures.clone());
                            drop(q);
                            if let Some(report) = report {
                                let _ = save_failures(&directory, &report);
                            }
                            shared.wake.notify_all();
                            ctx.request_repaint();
                            continue;
                        }
                        let unavailable = decoded
                            .as_ref()
                            .err()
                            .is_some_and(|e| e.is::<super::thumbnails::Unavailable>());
                        let (pixels, frost) = decoded
                            .map(|(loaded, frost)| (Some(loaded.pixels), frost))
                            .unwrap_or((None, None));
                        {
                            let mut q = shared.queue.lock().unwrap();
                            while q.ready.len() >= 64 && q.wanted.contains(&key) && !q.stop {
                                // Prefetch completions must never occupy every worker
                                // while a freshly visible disk hit is waiting to run.
                                if let Some(index) = q
                                    .ready
                                    .iter()
                                    .position(|r| !q.visible_keys.contains(&r.key))
                                {
                                    let old = q.ready.remove(index).unwrap();
                                    q.running.remove(&old.key);
                                    break;
                                }
                                if !q.visible_keys.contains(&key) {
                                    q.running.remove(&key);
                                    break;
                                }
                                q = shared.wake.wait(q).unwrap();
                            }
                            if q.stop {
                                return;
                            }
                            if q.wanted.contains(&key) && q.ready.len() < 64 {
                                q.ready.push_back(Ready {
                                    key,
                                    pixels,
                                    frost,
                                    unavailable,
                                });
                            } else {
                                q.running.remove(&key);
                            }
                        }
                        ctx.request_repaint();
                        if shared.completed.fetch_add(1, Ordering::Relaxed) % 128 == 127 {
                            static PRUNE: Mutex<()> = Mutex::new(());
                            if let Ok(_guard) = PRUNE.try_lock() {
                                let budget = shared.disk_budget.load(Ordering::Relaxed);
                                let _ =
                                    prune(&directory, if budget == 0 { u64::MAX } else { budget });
                            }
                        }
                    }
                })
                .expect("thumbnail worker");
        }
        self.shared.wake.notify_all();
    }
    pub fn visible(&self, request: Request) {
        let mut q = self.shared.queue.lock().unwrap();
        q.wanted.insert(request.key.clone());
        q.visible_keys.insert(request.key.clone());
        if q.running.contains(&request.key) || q.visible.iter().any(|r| r.key == request.key) {
            return;
        }
        q.ahead.retain(|r| r.key != request.key);
        if q.visible.len() >= MAX_QUEUED {
            q.visible.pop_back();
        }
        q.visible.push_front(request);
        while q.visible.len() + q.ahead.len() > MAX_QUEUED {
            q.ahead.pop_back();
        }
        drop(q);
        self.shared.wake.notify_all();
    }
    pub fn probe(&self, request: Request) {
        let mut q = self.shared.queue.lock().unwrap();
        if q.probing.len() < MAX_QUEUED && q.probing.insert(request.key.clone()) {
            q.probes.push_back(request);
            drop(q);
            self.shared.wake.notify_all();
        }
    }
    pub fn recheck_after(&self, delay: std::time::Duration) {
        self.ctx.request_repaint_after(delay);
    }
    pub fn availability(&self) -> std::collections::HashMap<String, bool> {
        std::mem::take(&mut self.shared.queue.lock().unwrap().availability)
    }
    /// Replace speculative work, not append: fast scroll/turnaround never has
    /// to drain a FIFO full of no-longer-relevant images first.
    pub fn plan(&self, visible: Vec<Request>, ahead: Vec<Request>) {
        let mut q = self.shared.queue.lock().unwrap();
        q.visible_keys = visible.iter().map(|r| r.key.clone()).collect();
        q.wanted = visible
            .iter()
            .chain(&ahead)
            .take(MAX_QUEUED)
            .map(|r| r.key.clone())
            .collect();
        // Remove already-decoded obsolete pages too, not just queued disk jobs.
        let mut ready = std::mem::take(&mut q.ready);
        ready.retain(|r| {
            if q.wanted.contains(&r.key) {
                true
            } else {
                q.running.remove(&r.key);
                false
            }
        });
        q.ready = ready;
        let mut keys = q.running.clone();
        q.visible = visible
            .into_iter()
            .filter(|r| keys.insert(r.key.clone()))
            .take(MAX_QUEUED)
            .collect();
        q.ahead = ahead
            .into_iter()
            .filter(|r| keys.insert(r.key.clone()))
            .take(MAX_QUEUED.saturating_sub(q.visible.len()))
            .collect();
        drop(q);
        self.shared.wake.notify_all();
    }
    pub fn pending(&self) -> HashSet<String> {
        let q = self.shared.queue.lock().unwrap();
        q.running
            .iter()
            .cloned()
            .chain(q.visible.iter().chain(&q.ahead).map(|r| r.key.clone()))
            .collect()
    }
    pub fn complete(&self, key: &str) {
        self.shared.queue.lock().unwrap().running.remove(key);
        self.shared.wake.notify_all();
    }
    pub fn next_ready(&self, visible_only: bool) -> Option<Ready> {
        let mut q = self.shared.queue.lock().unwrap();
        let index = q
            .ready
            .iter()
            .position(|r| q.visible_keys.contains(&r.key))
            .or_else(|| (!visible_only && !q.ready.is_empty()).then_some(0))?;
        let ready = q.ready.remove(index);
        drop(q);
        self.shared.wake.notify_all();
        ready
    }
    pub fn failures(&self) -> Vec<Failure> {
        self.shared.queue.lock().unwrap().failures.clone()
    }
    pub fn build_all(&self, requests: Vec<Request>) {
        let mut q = self.shared.queue.lock().unwrap();
        q.bulk_generation += 1;
        q.failures.clear();
        q.bulk_status = BulkStatus {
            total: requests.len(),
            running: !requests.is_empty(),
            ..Default::default()
        };
        q.bulk = requests.into();
        drop(q);
        self.shared.wake.notify_all();
    }
    pub fn disk_quota(&self, gib: u32) {
        let budget = u64::from(gib) * 1024 * 1024 * 1024;
        if self.shared.disk_budget.swap(budget, Ordering::Relaxed) != budget {
            self.shared.queue.lock().unwrap().quota_change = Some(budget);
            self.shared.wake.notify_all();
        }
    }
    pub fn bulk_status(&self) -> BulkStatus {
        self.shared.queue.lock().unwrap().bulk_status
    }
    pub fn dismiss_bulk(&self) {
        let mut q = self.shared.queue.lock().unwrap();
        if !q.bulk_status.running {
            q.bulk_status = BulkStatus::default();
        }
    }
    pub fn directory(&self) -> &std::path::Path {
        &self.directory
    }
    pub fn pause_bulk(&self) {
        let mut q = self.shared.queue.lock().unwrap();
        q.bulk_status.paused = !q.bulk_status.paused;
        drop(q);
        self.shared.wake.notify_all();
    }
    pub fn cancel_bulk(&self) {
        let mut q = self.shared.queue.lock().unwrap();
        q.bulk_generation += 1;
        q.bulk.clear();
        q.bulk_status.running = false;
        drop(q);
        self.shared.wake.notify_all();
    }
}
impl Drop for Jobs {
    fn drop(&mut self) {
        let mut q = self.shared.queue.lock().unwrap();
        q.stop = true;
        q.visible.clear();
        q.ahead.clear();
        q.bulk.clear();
        drop(q);
        self.shared.wake.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_ready_queue_cannot_starve_a_new_cached_viewport() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let jobs = Jobs::new(temp.path().into(), &ctx);
        let requests: Vec<_> = (0..120)
            .map(|i| {
                let key = format!("{i:064x}");
                image::RgbImage::from_pixel(32, 24, image::Rgb([i as u8, 90, 180]))
                    .save(temp.path().join(format!("{key}.png")))
                    .unwrap();
                let path = temp.path().join(format!("original-{i}.jpg"));
                std::fs::write(&path, b"original").unwrap();
                let m = path.metadata().unwrap();
                let valid_key =
                    super::super::store::cache_key(&path, m.len(), m.modified().unwrap());
                std::fs::rename(
                    temp.path().join(format!("{key}.png")),
                    temp.path().join(format!("{valid_key}.png")),
                )
                .unwrap();
                Request {
                    key: valid_key,
                    path,
                }
            })
            .collect();
        jobs.plan(requests[..8].to_vec(), requests[8..110].to_vec());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while jobs.shared.queue.lock().unwrap().ready.len() < 64 {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        // No output drain before the jump: old speculative completions were the
        // source of head-of-line blocking even when every PNG existed on disk.
        jobs.plan(requests[110..].to_vec(), vec![]);
        let mut received = HashSet::new();
        while received.len() < 10 {
            if let Some(ready) = jobs.next_ready(true) {
                assert!(requests[110..].iter().any(|r| r.key == ready.key));
                assert!(ready.pixels.is_some());
                jobs.complete(&ready.key);
                received.insert(ready.key);
            }
            assert!(
                std::time::Instant::now() < deadline,
                "new viewport starved behind old ready queue"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(jobs.shared.queue.lock().unwrap().ready.len() <= 64);
    }

    fn request(key: &str) -> Request {
        Request {
            key: key.into(),
            path: PathBuf::from(key),
        }
    }
    #[test]
    fn plans_replace_stale_prefetch_and_promote_visible_without_duplicates() {
        let jobs = Jobs {
            shared: Arc::new(Shared {
                queue: Mutex::new(Queue::default()),
                wake: Condvar::new(),
                completed: AtomicUsize::new(0),
                disk_budget: AtomicU64::new(16 * 1024 * 1024 * 1024),
            }),
            directory: PathBuf::new(),
            ctx: egui::Context::default(),
            spawned: 0,
        };
        jobs.shared
            .queue
            .lock()
            .unwrap()
            .running
            .insert("running".into());
        jobs.plan(
            vec![request("visible"), request("running"), request("visible")],
            vec![request("old"), request("next")],
        );
        jobs.visible(request("next"));
        {
            let q = jobs.shared.queue.lock().unwrap();
            assert_eq!(
                q.visible.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(),
                ["next", "visible"]
            );
            assert_eq!(q.ahead.len(), 1);
        }
        jobs.plan(
            vec![request("new")],
            (0..4000).map(|n| request(&n.to_string())).collect(),
        );
        let pending = jobs.pending();
        assert!(!pending.contains("old"));
        assert!(!pending.contains("visible"));
        assert!(pending.contains("running"));
        assert!(pending.len() <= MAX_QUEUED + 1);
        let q = jobs.shared.queue.lock().unwrap();
        assert_eq!(q.visible.front().unwrap().key, "new");
        drop(q);
        jobs.complete("running");
        assert!(!jobs.pending().contains("running"));
        {
            let mut q = jobs.shared.queue.lock().unwrap();
            for key in ["old-page", "prefetch", "visible-now"] {
                q.running.insert(key.into());
                q.ready.push_back(Ready {
                    key: key.into(),
                    pixels: None,
                    frost: None,
                    unavailable: false,
                });
            }
        }
        jobs.plan(vec![request("visible-now")], vec![request("prefetch")]);
        assert_eq!(
            jobs.next_ready(true).unwrap().key,
            "visible-now",
            "visible uploads bypass prefetched ready pixels"
        );
        assert!(jobs.next_ready(true).is_none());
        assert_eq!(jobs.next_ready(false).unwrap().key, "prefetch");
        jobs.complete("visible-now");
        jobs.complete("prefetch");
        assert!(
            !jobs.pending().contains("old-page"),
            "obsolete ready pixels cannot block the new viewport"
        );
        jobs.build_all(vec![request("library-a"), request("library-b")]);
        assert_eq!(jobs.bulk_status().total, 2);
        jobs.pause_bulk();
        assert!(jobs.bulk_status().paused);
        jobs.plan(vec![request("visible-now")], vec![]);
        {
            let q = jobs.shared.queue.lock().unwrap();
            assert_eq!(
                q.bulk.len(),
                2,
                "viewport plans never replace explicit full-library work"
            );
            assert_eq!(q.visible.front().unwrap().key, "visible-now");
        }
        jobs.cancel_bulk();
        assert!(!jobs.bulk_status().running);
        assert!(jobs.shared.queue.lock().unwrap().bulk.is_empty());
    }
}
