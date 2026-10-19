//! One database owner, bounded commands/results, cooperative scan cancellation.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};

use super::store::{self, Snapshot};

fn presentation_snapshot(
    db: &rusqlite::Connection,
    availability: &std::sync::Mutex<std::collections::HashSet<String>>,
    verified: bool,
) -> anyhow::Result<Snapshot> {
    // Every SELECT belongs to one consistent WAL read snapshot, even when
    // the other lane commits a short interactive mutation concurrently.
    let read = db.unchecked_transaction()?;
    let mut state = store::snapshot(&read)?;
    read.commit()?;
    state.unavailable = availability
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    state.availability_verified = verified;
    state.suggestions = super::discovery::suggestions(&state);
    Ok(state)
}

pub enum Command {
    Favorite(i64),
    Aspects(Vec<(String, f32)>),
    Refresh(u64),
    AddSource(PathBuf, u64),
    RemoveSource(i64),
    CreateAlbum(String),
    AssignAlbum(i64, Vec<i64>),
    CreateAlbumPhotos(String, Vec<i64>),
    RenameAlbum(i64, String),
    RemoveAlbum(i64),
    CoverAlbum(i64, i64),
    UnassignAlbum(i64, Vec<i64>),
    AutoAlbums,
    SaveQuery(String, String),
    RemoveQuery(i64),
    Ocr(u64),
    Describe(u64, String),
    Embed(u64, String),
    SemanticSearch(u64, String, String),
    ContentAlbums(u64, String),
    ClearEmbeddings,
}

pub struct Update {
    pub matches: Option<super::semantic::Matches>,
    pub snapshot: Option<Snapshot>,
    pub error: Option<String>,
    pub ticket: Option<u64>,
    pub album: Option<i64>,
}

pub struct Worker {
    commands: mpsc::SyncSender<Command>,
    interactive: mpsc::SyncSender<Command>,
    pub results: mpsc::Receiver<Update>,
    generation: Arc<AtomicU64>,
    pub progress: Arc<AtomicUsize>,
    /// MAX while discovering; MAX-1 for jobs without a filesystem denominator.
    pub total: Arc<AtomicUsize>,
    paused: Arc<AtomicBool>,
}

impl Worker {
    pub fn new(path: PathBuf, wake: impl Fn() + Send + Sync + 'static) -> Self {
        let (commands, input) = mpsc::sync_channel(16);
        let (interactive, fast_input) = mpsc::sync_channel(64);
        let (output, results) = mpsc::sync_channel(2);
        let generation = Arc::new(AtomicU64::new(0));
        let cancel = generation.clone();
        let progress = Arc::new(AtomicUsize::new(0));
        let scanned_count = progress.clone();
        let total = Arc::new(AtomicUsize::new(usize::MAX - 1));
        let scan_total = total.clone();
        let paused = Arc::new(AtomicBool::new(false));
        let pause = paused.clone();
        let wake = Arc::new(wake);
        let availability = Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
        let connect_guard = Arc::new(std::sync::Mutex::new(()));
        let publish_guard = Arc::new(std::sync::Mutex::new(()));
        for (lane, input) in [(0, fast_input), (1, input)] {
            let path = path.clone();
            let wake = wake.clone();
            let availability = availability.clone();
            let connect_guard = connect_guard.clone();
            let publish_guard = publish_guard.clone();
            let cancel = cancel.clone();
            let scanned_count = scanned_count.clone();
            let scan_total = scan_total.clone();
            let pause = pause.clone();
            let output = output.clone();
            std::thread::spawn(move || {
                let guard = connect_guard.lock().unwrap_or_else(|e| e.into_inner());
                let mut db = match store::connect(&path) {
                    Ok(db) => db,
                    Err(e) => {
                        let _ = output.send(Update {
                            matches: None,
                            snapshot: None,
                            error: Some(format!("{e:#}")),
                            ticket: None,
                            album: None,
                        });
                        wake();
                        return;
                    }
                };
                drop(guard);
                if lane == 0 {
                    // Initial availability is checked before the first presentation, but
                    // ordinary publications never hold this mutex across filesystem I/O.
                    if let Ok(state) = store::snapshot(&db) {
                        *availability.lock().unwrap_or_else(|e| e.into_inner()) = state
                            .photos
                            .iter()
                            .filter(|p| !super::thumbnails::original_available(&p.path, &p.key))
                            .map(|p| p.key.clone())
                            .collect();
                    }
                    let _publish = publish_guard.lock().unwrap_or_else(|e| e.into_inner());
                    let initial = presentation_snapshot(&db, &availability, true);
                    let _ = output.send(match initial {
                        Ok(snapshot) => Update {
                            matches: None,
                            snapshot: Some(snapshot),
                            error: None,
                            ticket: None,
                            album: None,
                        },
                        Err(e) => Update {
                            matches: None,
                            snapshot: None,
                            error: Some(format!("{e:#}")),
                            ticket: None,
                            album: None,
                        },
                    });
                    wake();
                }
                while let Ok(command) = input.recv() {
                    let verify = matches!(&command, Command::Refresh(_) | Command::AddSource(_, _));
                    if let Command::Aspects(aspects) = &command {
                        if let Err(error) = store::save_aspects(&mut db, aspects) {
                            log::warn!("Cannot persist thumbnail aspects: {error}");
                        }
                        continue;
                    }
                    // A newly requested generation supersedes queued scans/OCR too.
                    if matches!(&command,Command::Refresh(t)|Command::Ocr(t)|Command::Describe(t,_)|Command::Embed(t,_)|Command::SemanticSearch(t,_,_)|Command::ContentAlbums(t,_) if *t!=cancel.load(Ordering::Relaxed))
                    {
                        continue;
                    }
                    let ticket = match &command {
                        Command::Refresh(t)
                        | Command::AddSource(_, t)
                        | Command::Ocr(t)
                        | Command::Describe(t, _) => Some(*t),
                        Command::Embed(t, _)
                        | Command::SemanticSearch(t, _, _)
                        | Command::ContentAlbums(t, _) => Some(*t),
                        _ => None,
                    };
                    let mut changed_album = None;
                    let mut matches = None;
                    let result = (|| -> anyhow::Result<Vec<String>> {
                        match command {
                            Command::ClearEmbeddings => {
                                db.execute("DELETE FROM photo_embeddings", [])?;
                                Ok(vec![])
                            }
                            Command::Embed(ticket, model) => {
                                super::analysis::ensure_local_model(&model, "embedding", || {
                                    cancel.load(Ordering::Relaxed) != ticket
                                })?;
                                super::semantic::index(
                                    &db,
                                    &model,
                                    || cancel.load(Ordering::Relaxed) != ticket,
                                    |n| {
                                        scanned_count.store(n, Ordering::Relaxed);
                                        wake();
                                        wait_pause(&pause, &cancel, ticket);
                                    },
                                )?;
                                Ok(vec![])
                            }
                            Command::SemanticSearch(ticket, model, query) => {
                                super::analysis::ensure_local_model(&model, "embedding", || {
                                    cancel.load(Ordering::Relaxed) != ticket
                                })?;
                                matches =
                                    Some(super::semantic::search(&db, &model, query, || {
                                        cancel.load(Ordering::Relaxed) != ticket
                                    })?);
                                Ok(vec![])
                            }
                            Command::ContentAlbums(ticket, model) => {
                                super::semantic::albums(&mut db, &model, || {
                                    wait_pause(&pause, &cancel, ticket);
                                    cancel.load(Ordering::Relaxed) != ticket
                                })?;
                                Ok(vec![])
                            }
                            Command::Favorite(id) => {
                                store::toggle_favorite(&db, id)?;
                                Ok(vec![])
                            }
                            Command::Aspects(_) => {
                                unreachable!("handled without emitting a scene snapshot")
                            }
                            Command::Describe(ticket, model) => {
                                super::analysis::ensure_local_model(&model, "vision", || {
                                    cancel.load(Ordering::Relaxed) != ticket
                                })?;
                                let photos = store::snapshot(&db)?.photos;
                                let mut warnings = Vec::new();
                                for (n, photo) in photos.iter().enumerate() {
                                    wait_pause(&pause, &cancel, ticket);
                                    if cancel.load(Ordering::Relaxed) != ticket {
                                        break;
                                    }
                                    let indexed:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM photo_descriptions WHERE photo_id=? AND cache_key=? AND model=?)",rusqlite::params![photo.id,photo.key,model],|r|r.get(0))?;
                                    if !indexed {
                                        match super::analysis::describe(&photo.path, &model, || {
                                            cancel.load(Ordering::Relaxed) != ticket
                                        }) {
                                            Ok(text) => {
                                                let current = std::fs::metadata(&photo.path)
                                                    .ok()
                                                    .and_then(|m| {
                                                        m.modified().ok().map(|t| {
                                                            store::cache_key(
                                                                &photo.path,
                                                                m.len(),
                                                                t,
                                                            )
                                                        })
                                                    });
                                                if current.as_ref() == Some(&photo.key) {
                                                    db.execute("INSERT INTO photo_descriptions VALUES(?,?,?,?) ON CONFLICT(photo_id) DO UPDATE SET cache_key=excluded.cache_key,model=excluded.model,text=excluded.text",rusqlite::params![photo.id,photo.key,model,text])?;
                                                }
                                            }
                                            // A missing server/model would otherwise fail once for every photo.
                                            Err(error) => {
                                                warnings.push(format!(
                                                    "{}: {error:#}",
                                                    photo.path.display()
                                                ));
                                                break;
                                            }
                                        }
                                    }
                                    scanned_count.store(n + 1, Ordering::Relaxed);
                                    wake();
                                }
                                Ok(warnings)
                            }
                            Command::Ocr(ticket) => {
                                let available = super::analysis::languages()?;
                                let langs = ["rus", "eng"]
                                    .into_iter()
                                    .filter(|lang| available.iter().any(|v| v == lang))
                                    .collect::<Vec<_>>()
                                    .join("+");
                                if langs.is_empty() {
                                    anyhow::bail!("Install Tesseract rus/eng language data");
                                }
                                let photos = store::snapshot(&db)?.photos;
                                let mut warnings = Vec::new();
                                for (n, photo) in photos.iter().enumerate() {
                                    wait_pause(&pause, &cancel, ticket);
                                    if cancel.load(Ordering::Relaxed) != ticket {
                                        break;
                                    }
                                    let indexed:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM photo_text WHERE photo_id=? AND cache_key=?)",rusqlite::params![photo.id,photo.key],|r|r.get(0))?;
                                    if !indexed {
                                        match super::analysis::text(&photo.path, &langs, || {
                                            cancel.load(Ordering::Relaxed) != ticket
                                        }) {
                                            Ok(text) => {
                                                let current = std::fs::metadata(&photo.path)
                                                    .ok()
                                                    .and_then(|m| {
                                                        m.modified().ok().map(|t| {
                                                            store::cache_key(
                                                                &photo.path,
                                                                m.len(),
                                                                t,
                                                            )
                                                        })
                                                    });
                                                if current.as_ref() == Some(&photo.key) {
                                                    db.execute("INSERT INTO photo_text VALUES(?,?,?) ON CONFLICT(photo_id) DO UPDATE SET cache_key=excluded.cache_key,text=excluded.text",rusqlite::params![photo.id,photo.key,text])?;
                                                }
                                            }
                                            Err(error) => {
                                                if warnings.len() < 20 {
                                                    warnings.push(format!(
                                                        "{}: {error}",
                                                        photo.path.display()
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                    scanned_count.store(n + 1, Ordering::Relaxed);
                                    wake();
                                }
                                Ok(warnings)
                            }
                            Command::AutoAlbums => {
                                store::auto_albums(&mut db)?;
                                Ok(vec![])
                            }
                            Command::SaveQuery(name, query) => {
                                store::save_query(&db, &name, &query)?;
                                Ok(vec![])
                            }
                            Command::RemoveQuery(id) => {
                                store::remove_query(&db, id)?;
                                Ok(vec![])
                            }
                            Command::Refresh(ticket) => {
                                scan_total.store(usize::MAX, Ordering::Relaxed);
                                wake();
                                let total = store::count_images(&db, &cancel, ticket, |n| {
                                    scanned_count.store(n, Ordering::Relaxed);
                                    wake();
                                    wait_pause(&pause, &cancel, ticket);
                                })?;
                                scanned_count.store(0, Ordering::Relaxed);
                                scan_total.store(total, Ordering::Relaxed);
                                wake();
                                store::scan(&mut db, &cancel, ticket, |n| {
                                    scanned_count.store(n, Ordering::Relaxed);
                                    wake();
                                    wait_pause(&pause, &cancel, ticket);
                                })
                            }
                            Command::AddSource(path, ticket) => {
                                store::add_source(&db, &path)?;
                                scan_total.store(usize::MAX, Ordering::Relaxed);
                                wake();
                                let total = store::count_images(&db, &cancel, ticket, |n| {
                                    scanned_count.store(n, Ordering::Relaxed);
                                    wake();
                                    wait_pause(&pause, &cancel, ticket);
                                })?;
                                scanned_count.store(0, Ordering::Relaxed);
                                scan_total.store(total, Ordering::Relaxed);
                                wake();
                                store::scan(&mut db, &cancel, ticket, |n| {
                                    scanned_count.store(n, Ordering::Relaxed);
                                    wake();
                                    wait_pause(&pause, &cancel, ticket);
                                })
                            }
                            Command::RemoveSource(id) => {
                                store::remove_source(&mut db, id)?;
                                Ok(vec![])
                            }
                            Command::CreateAlbum(name) => {
                                changed_album = Some(store::create_album(&db, &name)?);
                                Ok(vec![])
                            }
                            Command::AssignAlbum(album, photos) => {
                                store::assign_album(&mut db, album, &photos)?;
                                changed_album = Some(album);
                                Ok(vec![])
                            }
                            Command::CreateAlbumPhotos(name, photos) => {
                                changed_album =
                                    Some(store::create_album_with_photos(&mut db, &name, &photos)?);
                                Ok(vec![])
                            }
                            Command::RenameAlbum(id, name) => {
                                store::rename_album(&db, id, &name)?;
                                Ok(vec![])
                            }
                            Command::RemoveAlbum(id) => {
                                store::remove_album(&db, id)?;
                                Ok(vec![])
                            }
                            Command::CoverAlbum(id, photo) => {
                                store::cover_album(&db, id, photo)?;
                                Ok(vec![])
                            }
                            Command::UnassignAlbum(id, photos) => {
                                store::unassign_album(&mut db, id, &photos)?;
                                Ok(vec![])
                            }
                        }
                    })();
                    let error = match result {
                        Ok(warnings) if !warnings.is_empty() => Some(warnings.join("\n")),
                        Ok(_) => None,
                        Err(e) => Some(format!("{e:#}")),
                    };
                    if verify {
                        if let Ok(state) = store::snapshot(&db) {
                            let unavailable = state
                                .photos
                                .iter()
                                .filter(|p| !super::thumbnails::original_available(&p.path, &p.key))
                                .map(|p| p.key.clone())
                                .collect();
                            *availability.lock().unwrap_or_else(|e| e.into_inner()) = unavailable;
                        }
                    }
                    // Serialize snapshot creation + publication, not the lengthy
                    // analysis itself. Older lane results cannot arrive after newer
                    // snapshots and undo an already-presented favorite/album edit.
                    let _publish = publish_guard.lock().unwrap_or_else(|e| e.into_inner());
                    let snapshot = presentation_snapshot(&db, &availability, verify);
                    let update = match snapshot {
                        Ok(snapshot) => Update {
                            matches,
                            snapshot: Some(snapshot),
                            error,
                            ticket,
                            album: changed_album,
                        },
                        Err(e) => Update {
                            matches: None,
                            snapshot: None,
                            error: Some(format!("{e:#}")),
                            ticket,
                            album: None,
                        },
                    };
                    if output.send(update).is_err() {
                        break;
                    }
                    wake();
                }
            });
        }
        Self {
            commands,
            interactive,
            results,
            generation,
            progress,
            total,
            paused,
        }
    }

    pub fn ticket(&self) -> u64 {
        self.paused.store(false, Ordering::Relaxed);
        self.progress.store(0, Ordering::Relaxed);
        self.total.store(usize::MAX - 1, Ordering::Relaxed);
        self.generation.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn cancel(&self) {
        self.paused.store(false, Ordering::Relaxed);
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    pub fn current_ticket(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }
    pub fn paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }
    pub fn toggle_pause(&self) {
        self.paused.fetch_xor(true, Ordering::Relaxed);
    }

    pub fn send(&self, command: Command) -> Result<(), &'static str> {
        let sender = if matches!(
            &command,
            Command::Refresh(_)
                | Command::AddSource(_, _)
                | Command::Ocr(_)
                | Command::Describe(_, _)
                | Command::Embed(_, _)
                | Command::SemanticSearch(_, _, _)
                | Command::ContentAlbums(_, _)
                | Command::AutoAlbums
        ) {
            &self.commands
        } else {
            &self.interactive
        };
        sender.try_send(command).map_err(|e| match e {
            mpsc::TrySendError::Full(_) => "Library queue is busy; try again",
            mpsc::TrySendError::Disconnected(_) => "Library worker is unavailable",
        })
    }
}

fn wait_pause(pause: &AtomicBool, cancel: &AtomicU64, ticket: u64) {
    while pause.load(Ordering::Relaxed) && cancel.load(Ordering::Relaxed) == ticket {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paused_analysis_does_not_block_favorites_or_recheck_originals() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        std::fs::create_dir(&source).unwrap();
        let photo = source.join("test.png");
        image::RgbImage::new(16, 12).save(&photo).unwrap();
        let path = temp.path().join("data/db");
        let mut db = store::connect(&path).unwrap();
        store::add_source(&db, &source).unwrap();
        store::scan(&mut db, &AtomicU64::new(1), 1, |_| {}).unwrap();
        let id = store::snapshot(&db).unwrap().photos[0].id;
        drop(db);
        let worker = Worker::new(path, || {});
        worker
            .results
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        // Disconnect AFTER the initial verified snapshot. An ordinary mutation
        // must use the availability cache, rather than open the file again.
        std::fs::rename(&source, temp.path().join("offline")).unwrap();
        let ticket = worker.ticket();
        worker.toggle_pause();
        worker.send(Command::Refresh(ticket)).unwrap();
        worker.send(Command::Favorite(id)).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            let update = worker
                .results
                .recv_timeout(std::time::Duration::from_millis(500))
                .unwrap();
            if let Some(state) = update.snapshot {
                if state.favorites.contains(&id) {
                    assert!(state.unavailable.is_empty());
                    break;
                }
            }
            assert!(std::time::Instant::now() < deadline);
        }
        assert!(worker.paused());
        worker.cancel();
    }
    #[test]
    fn cancellation_releases_paused_worker() {
        let temp = tempfile::tempdir().unwrap();
        let worker = Worker::new(temp.path().join("data/db"), || {});
        let _ = worker
            .results
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        let ticket = worker.ticket();
        worker.toggle_pause();
        assert!(worker.paused());
        let pause = worker.paused.clone();
        let cancel = worker.generation.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            wait_pause(&pause, &cancel, ticket);
            let _ = tx.send(());
        });
        assert!(rx
            .recv_timeout(std::time::Duration::from_millis(80))
            .is_err());
        worker.cancel();
        rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        assert!(!worker.paused());
    }
}
