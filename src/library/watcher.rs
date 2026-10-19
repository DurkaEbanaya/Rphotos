//! Native filesystem notifications, debounced off the UI thread. No original writes.
use notify::{EventKind, RecursiveMode, Watcher};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};

pub(super) struct Watch {
    roots: Vec<PathBuf>,
    commands: mpsc::SyncSender<Vec<PathBuf>>,
    dirty: Arc<AtomicBool>,
    error: mpsc::Receiver<String>,
}
impl Watch {
    pub fn new(wake: impl Fn() + Send + 'static) -> Self {
        let (commands, input) = mpsc::sync_channel::<Vec<PathBuf>>(1);
        let (errors, error) = mpsc::sync_channel(1);
        let dirty = Arc::new(AtomicBool::new(false));
        let flag = dirty.clone();
        std::thread::spawn(move || {
            let (events, received) = mpsc::sync_channel(1);
            let mut watcher =
                match notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                    if event.as_ref().map_or(true, |event| {
                        !matches!(event.kind, EventKind::Access(_))
                            && (matches!(
                                event.kind,
                                EventKind::Any
                                    | EventKind::Other
                                    | EventKind::Create(notify::event::CreateKind::Folder)
                                    | EventKind::Remove(notify::event::RemoveKind::Folder)
                                    | EventKind::Modify(notify::event::ModifyKind::Name(_))
                            ) || event.paths.iter().any(|p| {
                                p.extension().is_none() || crate::file_io::is_supported_image(p)
                            }))
                    }) {
                        let _ = events.try_send(());
                    }
                }) {
                    Ok(w) => w,
                    Err(e) => {
                        let _ = errors.try_send(e.to_string());
                        wake();
                        return;
                    }
                };
            let mut roots: Vec<PathBuf> = Vec::new();
            let mut pending = None;
            let mut first = None;
            let mut failed = Vec::new();
            let mut retry = Instant::now();
            loop {
                match input.try_recv() {
                    Ok(next) => {
                        for root in &roots {
                            let _ = watcher.unwatch(root);
                        }
                        roots = next;
                        failed.clear();
                        for root in &roots {
                            if let Err(e) = watcher.watch(root, RecursiveMode::Recursive) {
                                failed.push(root.clone());
                                let _ = errors.try_send(format!("{}: {e}", root.display()));
                                wake();
                            }
                        }
                    }
                    Err(mpsc::TryRecvError::Disconnected) => break,
                    Err(mpsc::TryRecvError::Empty) => {}
                }
                if retry.elapsed() >= Duration::from_secs(2) {
                    retry = Instant::now();
                    // Detect disappearing roots too: some backends silently
                    // discard registrations when a mount or directory goes away.
                    for root in &roots {
                        if !root.is_dir() && !failed.contains(root) {
                            let _ = watcher.unwatch(root);
                            failed.push(root.clone());
                        }
                    }
                    failed.retain(|root| {
                        if watcher.watch(root, RecursiveMode::Recursive).is_ok() {
                            flag.store(true, Ordering::Relaxed);
                            wake();
                            false
                        } else {
                            true
                        }
                    });
                }
                match received.recv_timeout(Duration::from_millis(100)) {
                    Ok(()) => {
                        let now = Instant::now();
                        pending = Some(now);
                        first.get_or_insert(now);
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                if pending.is_some_and(|t: Instant| t.elapsed() >= Duration::from_millis(800))
                    || first.is_some_and(|t: Instant| t.elapsed() >= Duration::from_secs(5))
                {
                    flag.store(true, Ordering::Relaxed);
                    pending = None;
                    first = None;
                    wake();
                }
            }
        });
        Self {
            roots: Vec::new(),
            commands,
            dirty,
            error,
        }
    }
    pub fn roots(&mut self, mut paths: Vec<PathBuf>) {
        paths.sort();
        paths.dedup();
        // Overlapping source roots only need one native recursive watch.
        let minimal: Vec<_> = paths
            .iter()
            .filter(|p| {
                !paths
                    .iter()
                    .any(|other| other != *p && p.starts_with(other))
            })
            .cloned()
            .collect();
        if self.roots != minimal && self.commands.try_send(minimal.clone()).is_ok() {
            self.roots = minimal;
        }
    }
    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::Relaxed)
    }
    pub fn error(&self) -> Option<String> {
        self.error.try_recv().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initially_missing_root_is_registered_when_it_returns() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("returning");
        let mut watch = Watch::new(|| {});
        watch.roots(vec![root.clone()]);
        std::thread::sleep(Duration::from_millis(250));
        std::fs::create_dir(&root).unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        while !watch.take_dirty() {
            assert!(Instant::now() < deadline, "root was never registered");
            std::thread::sleep(Duration::from_millis(100));
        }
        std::fs::write(root.join("new.png"), b"fixture").unwrap();
        while !watch.take_dirty() {
            assert!(
                Instant::now() < deadline,
                "new root events were not delivered"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    #[test]
    fn native_watcher_reports_create_without_touching_original() {
        let temp = tempfile::tempdir().unwrap();
        let mut watch = Watch::new(|| {});
        watch.roots(vec![temp.path().to_owned()]);
        // Registration is asynchronous; repeated fixture writes avoid racing it.
        let deadline = Instant::now() + Duration::from_secs(6);
        let mut n = 0;
        while !watch.take_dirty() {
            assert!(
                Instant::now() < deadline,
                "native watcher did not report fixture change: {:?}",
                watch.error()
            );
            if n < 4 {
                std::fs::write(temp.path().join(format!("{n}.png")), "synthetic").unwrap();
                n += 1;
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        assert_eq!(
            std::fs::read_to_string(temp.path().join("0.png")).unwrap(),
            "synthetic"
        );
    }
}
