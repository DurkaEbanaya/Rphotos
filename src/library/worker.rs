//! One database owner, bounded commands/results, cooperative scan cancellation.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};

use super::store::{self, Snapshot};

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
    pub results: mpsc::Receiver<Update>,
    generation: Arc<AtomicU64>,
    pub progress: Arc<AtomicUsize>,
    paused: Arc<AtomicBool>,
}

impl Worker {
    pub fn new(path: PathBuf, wake: impl Fn() + Send + 'static) -> Self {
        let (commands, input) = mpsc::sync_channel(16);
        let (output, results) = mpsc::sync_channel(2);
        let generation = Arc::new(AtomicU64::new(0));
        let cancel = generation.clone();
        let progress = Arc::new(AtomicUsize::new(0));
        let scanned_count = progress.clone();
        let paused = Arc::new(AtomicBool::new(false));
        let pause = paused.clone();
        std::thread::spawn(move || {
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
            let initial = store::snapshot(&db);
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
            while let Ok(command) = input.recv() {
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
                            matches = Some(super::semantic::search(&db, &model, query, || {
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
                                            let current =
                                                std::fs::metadata(&photo.path).ok().and_then(|m| {
                                                    m.modified().ok().map(|t| {
                                                        store::cache_key(&photo.path, m.len(), t)
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
                                            let current =
                                                std::fs::metadata(&photo.path).ok().and_then(|m| {
                                                    m.modified().ok().map(|t| {
                                                        store::cache_key(&photo.path, m.len(), t)
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
                        Command::Refresh(ticket) => store::scan(&mut db, &cancel, ticket, |n| {
                            scanned_count.store(n, Ordering::Relaxed);
                            wake();
                            wait_pause(&pause, &cancel, ticket);
                        }),
                        Command::AddSource(path, ticket) => {
                            store::add_source(&db, &path)?;
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
                let snapshot = store::snapshot(&db);
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
        Self {
            commands,
            results,
            generation,
            progress,
            paused,
        }
    }

    pub fn ticket(&self) -> u64 {
        self.paused.store(false, Ordering::Relaxed);
        self.progress.store(0, Ordering::Relaxed);
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
        self.commands.try_send(command).map_err(|e| match e {
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
