//! Local library scene. UI owns presentation state; the worker owns SQLite.
mod analysis;
mod cache_status;
mod catalog;
mod discovery;
mod facts;
mod gallery;
mod gestures;
pub(crate) mod icons;
mod layout;
mod navigation;
mod preferences;
mod semantic;
mod shell;
pub mod store;
mod thumbnail_jobs;
mod thumbnails;
pub(crate) mod ui;
mod watcher;
mod worker;

use std::collections::HashSet;
use std::path::PathBuf;

use crate::settings::AppSettings;
use eframe::egui;
use serde::{Deserialize, Serialize};

use store::Snapshot;
use worker::{Command, Worker};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    Ru,
    En,
}

impl Language {
    pub fn text(self, ru: &'static str, en: &'static str) -> &'static str {
        match self {
            Self::Ru => ru,
            Self::En => en,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
enum Section {
    Collection,
    Albums,
    Folders,
    Favorites,
}

pub struct OpenPhoto {
    pub paths: Vec<PathBuf>,
    pub index: usize,
    pub slideshow: bool,
}

#[derive(Clone)]
enum GridRow {
    Month(usize),
    Photos(layout::Row),
}

#[derive(Clone)]
struct Month {
    key: String,
    indices: Vec<usize>,
    offset: f32,
}

type LayoutKey = (u32, u32, u64, String, Section, Option<i64>, Option<PathBuf>);

pub struct Library {
    #[cfg(test)]
    timeline_marker: Option<f32>,
    #[cfg(test)]
    timeline_date: Option<String>,
    watcher: watcher::Watch,
    watch_initial_refresh: bool,
    watch_refresh_pending: bool,
    focused_photo: Option<i64>,
    focused_photo_widget: Option<egui::Id>,
    content_search: semantic::Search,
    discovery: discovery::Discovery,
    ready: bool,
    pager: gestures::Pager,
    catalog: catalog::Catalog,
    worker: Worker,
    state: Snapshot,
    thumbnails: thumbnails::Thumbnails,
    section: Section,
    album: Option<i64>,
    folder: Option<PathBuf>,
    query: String,
    rows: Vec<GridRow>,
    months: Vec<Month>,
    semantic_zoom: bool,
    scroll_to_month: Option<String>,
    scroll_offset: f32,
    scroll_anchor: Option<(i64, f32)>,
    row_offsets: Vec<f32>,
    filtered: Vec<usize>,
    layout_key: Option<LayoutKey>,
    selection: HashSet<i64>,
    selecting: bool,
    popup_open_last_frame: bool,
    scanning: bool,
    error: Option<String>,
    settings_open: bool,
    album_name: String,
    create_album_open: bool,
    cache_errors_open: bool,
    assign_album_open: bool,
    remove_source: Option<i64>,
    picker: Option<std::sync::mpsc::Receiver<Option<PathBuf>>>,
    #[cfg(test)]
    grid_right: f32,
    #[cfg(test)]
    search_rect: Option<egui::Rect>,
}

impl Library {
    pub(crate) fn is_favorite(&self, path: &std::path::Path) -> bool {
        self.state
            .photos
            .iter()
            .any(|p| p.path == path && self.state.favorites.contains(&p.id))
    }
    pub(crate) fn toggle_favorite(&mut self, path: &std::path::Path) -> bool {
        if let Some(photo) = self.state.photos.iter().find(|p| p.path == path) {
            self.send(Command::Favorite(photo.id));
            true
        } else {
            false
        }
    }
    pub(crate) fn restore_scene(&mut self, storage: Option<&dyn eframe::Storage>) {
        if let Some(scene) =
            storage.and_then(|s| eframe::get_value::<SavedScene>(s, "rphotos.library.scene.v1"))
        {
            self.section = scene.section;
            self.album = scene.album;
            self.folder = scene.folder;
            self.query = scene.query;
            self.scroll_offset = if scene.scroll_offset.is_finite() {
                scene.scroll_offset.max(0.0)
            } else {
                0.0
            };
            self.discovery.hidden = scene.cards_hidden;
        }
    }
    pub(crate) fn save_scene(&self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(
            storage,
            "rphotos.library.scene.v1",
            &SavedScene {
                section: self.section,
                album: self.album,
                folder: self.folder.clone(),
                query: self.query.clone(),
                scroll_offset: self.scroll_offset,
                cards_hidden: self.discovery.hidden,
            },
        );
    }
    pub(crate) fn open_settings(&mut self) {
        self.settings_open = true;
    }
    pub(crate) fn settings_visible(&self) -> bool {
        self.settings_open
    }
    fn cancel_selection(&mut self) {
        self.selecting = false;
        self.selection.clear();
    }

    fn select_all(&mut self) {
        self.selecting = true;
        self.selection = self
            .filtered
            .iter()
            .map(|&i| self.state.photos[i].id)
            .collect();
    }
    pub(crate) fn add_viewed_to_album(&mut self, path: &std::path::Path) -> bool {
        let Some(photo) = self.state.photos.iter().find(|photo| photo.path == path) else {
            return false;
        };
        self.selection.clear();
        self.selection.insert(photo.id);
        self.assign_album_open = true;
        true
    }

    pub(crate) fn viewer_dialogs(&mut self, ctx: &egui::Context, settings: &mut AppSettings) {
        self.dialogs(ctx, settings, settings.language);
    }

    pub(crate) fn has_dialog(&self) -> bool {
        self.cache_errors_open
            || self.assign_album_open
            || self.error.is_some()
            || self.settings_open
            || self.catalog.share
            || self.catalog.trash.is_some()
            || self.catalog.jobs.message.is_some()
    }
    pub fn new(ctx: &egui::Context) -> Self {
        let data = dirs::data_local_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join(crate::identity::APP_ID);
        let cache = dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join(crate::identity::APP_ID)
            .join("thumbnails-v1");
        let mut library = Self::with_paths(ctx, data.join("library.sqlite3"), cache);
        library.watch_initial_refresh = false;
        library
    }

    fn with_paths(ctx: &egui::Context, database: PathBuf, cache: PathBuf) -> Self {
        let wake = ctx.clone();
        let watch_wake = ctx.clone();
        Self {
            #[cfg(test)]
            timeline_marker: None,
            #[cfg(test)]
            timeline_date: None,
            watcher: watcher::Watch::new(move || watch_wake.request_repaint()),
            // Isolated fixtures can use pre-populated metadata without a rescan.
            // The public native constructor enables one startup refresh.
            watch_initial_refresh: true,
            watch_refresh_pending: false,
            focused_photo: None,
            focused_photo_widget: None,
            content_search: Default::default(),
            discovery: Default::default(),
            ready: false,
            pager: Default::default(),
            catalog: Default::default(),
            worker: Worker::new(database, move || wake.request_repaint()),
            state: Snapshot::default(),
            thumbnails: thumbnails::Thumbnails::new(cache, ctx),
            section: Section::Collection,
            album: None,
            folder: None,
            query: String::new(),
            rows: Vec::new(),
            months: Vec::new(),
            semantic_zoom: false,
            scroll_to_month: None,
            scroll_offset: 0.0,
            scroll_anchor: None,
            row_offsets: Vec::new(),
            filtered: Vec::new(),
            layout_key: None,
            selection: HashSet::new(),
            selecting: false,
            popup_open_last_frame: false,
            scanning: false,
            error: None,
            settings_open: false,
            album_name: String::new(),
            create_album_open: false,
            cache_errors_open: false,
            assign_album_open: false,
            remove_source: None,
            picker: None,
            #[cfg(test)]
            grid_right: 0.0,
            #[cfg(test)]
            search_rect: None,
        }
    }

    fn send(&mut self, command: Command) {
        if let Err(error) = self.worker.send(command) {
            self.error = Some(error.into());
        }
    }

    pub fn add_source(&mut self, path: PathBuf) {
        let ticket = self.worker.ticket();
        self.scanning = true;
        self.send(Command::AddSource(path, ticket));
    }

    pub fn refresh(&mut self) {
        let ticket = self.worker.ticket();
        self.scanning = true;
        self.thumbnails.retry();
        self.send(Command::Refresh(ticket));
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        self.catalog.jobs.poll();
        while let Ok(update) = self.worker.results.try_recv() {
            self.ready = true;
            if let Some(id) = update.album {
                self.catalog.notice = Some(id);
            }
            if let Some(state) = update.snapshot {
                self.catalog.derived = None;
                if let Some(picker) = &mut self.catalog.picker {
                    picker.invalidate();
                }
                self.invalidate_pager_preview();
                self.watcher
                    .roots(state.sources.iter().map(|s| s.path.clone()).collect());
                if !self.watch_initial_refresh {
                    self.watch_initial_refresh = true;
                    self.watch_refresh_pending = !state.sources.is_empty();
                }
                self.content_search.hits = None;
                self.state = state;
                self.thumbnails.restore_snapshot_availability(
                    &self.state.photos,
                    &self.state.unavailable,
                    self.state.availability_verified,
                );
                self.thumbnails.restore_aspects(&self.state.aspects);
                self.discovery.rebuild(&self.state);
                self.thumbnails
                    .retain(&self.state.photos.iter().map(|p| p.key.clone()).collect());
                let present: HashSet<_> = self.state.photos.iter().map(|p| p.id).collect();
                self.selection.retain(|id| present.contains(id));
                self.layout_key = None;
                if self
                    .album
                    .is_some_and(|id| !self.state.albums.iter().any(|a| a.id == id))
                {
                    self.album = None;
                }
            }
            if update.error.is_some() {
                self.error = update.error;
            }
            if update.ticket == Some(self.worker.current_ticket()) {
                self.scanning = false;
                if let Some(matches) = update.matches {
                    if self.query == matches.query {
                        self.content_search.query = matches.query;
                        self.content_search.hits = Some(matches.ids.into_iter().collect());
                        self.layout_key = None;
                    }
                }
            }
        }
        if let Some(picker) = &self.picker {
            match picker.try_recv() {
                Ok(path) => {
                    self.picker = None;
                    if let Some(path) = path {
                        self.add_source(path);
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.picker = None,
                Err(_) => {}
            }
        }
        self.thumbnails.poll(ctx);
        if self.thumbnails.take_availability_changed() {
            self.catalog.derived = None;
            if let Some(picker) = &mut self.catalog.picker {
                picker.invalidate();
            }
            self.layout_key = None;
            self.invalidate_pager_preview();
            let available: HashSet<_> = self
                .state
                .photos
                .iter()
                .filter(|p| !self.thumbnails.unavailable(p))
                .map(|p| p.id)
                .collect();
            self.selection.retain(|id| available.contains(id));
        }
        self.thumbnails.persist_aspects(&self.worker);
        if self.watcher.take_dirty() {
            self.watch_refresh_pending = true;
        }
        if let Some(error) = self.watcher.error() {
            log::warn!("Library watcher: {error}; use F5 for unavailable folders");
        }
        if self.watch_refresh_pending && !self.scanning && self.ready && self.error.is_none() {
            self.watch_refresh_pending = false;
            self.refresh();
        }
    }

    fn pick_source(&mut self, ctx: &egui::Context) {
        if self.picker.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let wake = ctx.clone();
        // rfd requires the platform main thread on macOS; retain its supported
        // synchronous path there. Linux portals run off the UI thread.
        #[cfg(target_os = "macos")]
        {
            let _ = tx.send(rfd::FileDialog::new().pick_folder());
            wake.request_repaint();
        }
        #[cfg(not(target_os = "macos"))]
        std::thread::spawn(move || {
            let _ = tx.send(rfd::FileDialog::new().pick_folder());
            wake.request_repaint();
        });
        self.picker = Some(rx);
    }

    #[cfg(test)]
    fn rebuild_rows(&mut self, width: f32, density: u8) {
        self.rebuild_rows_at(
            width,
            match density {
                0 => 512.0,
                1 => 256.0,
                _ => 96.0,
            },
        );
    }

    fn rebuild_rows_at(&mut self, width: f32, size: f32) {
        let size = if size.is_finite() {
            size.clamp(64.0, 640.0)
        } else {
            256.0
        };
        let key = (
            width.to_bits(),
            size.to_bits(),
            self.thumbnails.revision,
            self.query.clone(),
            self.section,
            self.album,
            self.folder.clone(),
        );
        if self.layout_key.as_ref() == Some(&key) {
            return;
        }
        if self
            .layout_key
            .as_ref()
            .is_some_and(|old| old.3 != key.3 || old.4 != key.4 || old.5 != key.5 || old.6 != key.6)
        {
            self.scroll_anchor = None;
            self.scroll_offset = 0.0;
        }
        // Aspect arrivals / width changes only affect packing. Reusing filtered
        // month buckets avoids repeating Unicode search + local-time formatting
        // for every photo whenever a batch of thumbnails finishes.
        let regroup = self
            .layout_key
            .as_ref()
            .is_none_or(|old| old.3 != key.3 || old.4 != key.4 || old.5 != key.5 || old.6 != key.6);
        if regroup {
            let semantic_hits = self
                .content_search
                .hits
                .as_ref()
                .filter(|_| self.content_search.query == self.query);
            let query = self.query.trim().to_lowercase();
            let favorites = query.split_whitespace().any(|word| word == "is:favorite");
            let query = query
                .split_whitespace()
                .filter(|word| *word != "is:favorite")
                .collect::<Vec<_>>()
                .join(" ");
            self.filtered = self
                .state
                .photos
                .iter()
                .enumerate()
                .filter_map(|(i, photo)| {
                    let matches = !self.thumbnails.unavailable(photo)
                        && (!(favorites || self.section == Section::Favorites)
                            || self.state.favorites.contains(&photo.id))
                        && self.album.is_none_or(|album| photo.albums.contains(&album))
                        && self
                            .folder
                            .as_ref()
                            .is_none_or(|folder| photo.path.parent() == Some(folder.as_path()))
                        && (if let Some(hits) = semantic_hits {
                            hits.contains(&photo.id)
                        } else {
                            query.is_empty()
                                || photo.path.to_string_lossy().to_lowercase().contains(&query)
                                || photo.text.to_lowercase().contains(&query)
                        });
                    matches.then_some(i)
                })
                .collect();
            self.months.clear();
            for &index in &self.filtered {
                let month = month_key(self.state.photos[index].date());
                if self.months.last().is_none_or(|m| m.key != month) {
                    self.months.push(Month {
                        key: month,
                        indices: Vec::new(),
                        offset: 0.0,
                    });
                }
                self.months.last_mut().unwrap().indices.push(index);
            }
        }
        self.rows.clear();
        let target = size;
        let gap = 6.0 + 6.0 * ((size - 96.0) / 160.0).clamp(0.0, 1.0);
        self.row_offsets.clear();
        self.row_offsets.push(0.0);
        for (index, month) in self.months.iter_mut().enumerate() {
            month.offset = *self.row_offsets.last().unwrap();
            self.rows.push(GridRow::Month(index));
            self.row_offsets.push(month.offset + 66.0);
            let items: Vec<_> = month
                .indices
                .iter()
                .map(|&i| (i, self.thumbnails.aspect(&self.state.photos[i])))
                .collect();
            for row in layout::rows(&items, width, target, gap) {
                self.row_offsets
                    .push(self.row_offsets.last().unwrap() + row.height + gap);
                self.rows.push(GridRow::Photos(row));
            }
        }
        // Preserve the first visible photo across asynchronous aspect discovery
        // and density/width changes. No synchronous metadata reads on the UI thread.
        if let Some((id, relative)) = self.scroll_anchor {
            if let Some((row,_))=self.rows.iter().enumerate().find(|(_,r)|matches!(r,GridRow::Photos(r) if r.cells.iter().any(|c|self.state.photos[c.index].id==id))) {
                self.scroll_offset=(self.row_offsets[row]+self.discovery_height()-relative).max(0.0);
            }
        }
        self.layout_key = Some(key);
    }

    fn open(&self, photo: usize) -> OpenPhoto {
        OpenPhoto {
            slideshow: false,
            paths: self
                .filtered
                .iter()
                .map(|&i| self.state.photos[i].path.clone())
                .collect(),
            index: self.filtered.iter().position(|&i| i == photo).unwrap_or(0),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct SavedScene {
    #[serde(default)]
    cards_hidden: bool,
    section: Section,
    album: Option<i64>,
    folder: Option<PathBuf>,
    query: String,
    scroll_offset: f32,
}

fn date_key(seconds: i64) -> String {
    chrono::DateTime::from_timestamp(seconds, 0)
        .map(|date| {
            date.with_timezone(&chrono::Local)
                .format("%Y-%m-%d")
                .to_string()
        })
        .unwrap_or_else(|| "—".into())
}

fn month_key(seconds: i64) -> String {
    date_key(seconds).chars().take(7).collect()
}

fn month_label(key: &str, lang: Language) -> String {
    let names = match lang {
        Language::Ru => [
            "Январь",
            "Февраль",
            "Март",
            "Апрель",
            "Май",
            "Июнь",
            "Июль",
            "Август",
            "Сентябрь",
            "Октябрь",
            "Ноябрь",
            "Декабрь",
        ],
        Language::En => [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ],
    };
    key.get(5..7)
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|n| (1..=12).contains(n))
        .map(|n| format!("{} {}", names[n - 1], &key[..4]))
        .unwrap_or_else(|| key.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::AppSettings;

    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("demo");
        std::fs::create_dir(&source).unwrap();
        // macOS /var -> /private/var and Windows extended paths are canonicalized
        // by the index. Expectations must use that same native representation.
        let source = source.canonicalize().unwrap();
        for n in 0..18 {
            let (width, height) = if n % 3 == 0 { (240, 360) } else { (480, 300) };
            let image = image::RgbImage::from_fn(width, height, |x, y| {
                image::Rgb([
                    ((x + n * 29) % 256) as u8,
                    ((y + n * 19) % 256) as u8,
                    (80 + n * 7) as u8,
                ])
            });
            image.save(source.join(format!("Фото-{n:02}.png"))).unwrap();
        }
        let database = temp.path().join("data/library.sqlite3");
        let mut db = store::connect(&database).unwrap();
        store::add_source(&db, &source).unwrap();
        store::scan(&mut db, &std::sync::atomic::AtomicU64::new(1), 1, |_| {}).unwrap();
        // Stable ordering independent of whether fixture generation straddled a
        // wall-clock second. Tests must not assume offscreen photos are visible.
        db.execute("UPDATE photos SET modified=1792324800", [])
            .unwrap();
        (temp, source, database)
    }

    /// Opt-in, metadata-only scale probe. No original images or model engines
    /// are needed; timings are observations, not machine-independent budgets.
    #[test]
    #[ignore = "100k-photo scale probe; run explicitly with --nocapture"]
    fn large_library_scale_probe() {
        use std::time::Instant;
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("synthetic");
        std::fs::create_dir(&source).unwrap();
        let source = source.canonicalize().unwrap();
        let database = temp.path().join("data/library.sqlite3");
        let mut db = store::connect(&database).unwrap();
        store::add_source(&db, &source).unwrap();
        let started = Instant::now();
        {
            let tx = db.transaction().unwrap();
            let mut photos = tx.prepare("INSERT INTO photos VALUES(?,?,?,?)").unwrap();
            let mut links = tx.prepare("INSERT INTO source_photos VALUES(1,?)").unwrap();
            let mut aspects = tx
                .prepare("INSERT INTO thumbnail_aspects VALUES(?,?)")
                .unwrap();
            for id in 1..=100_000_i64 {
                let path = source.join(format!("photo-{id:06}.jpg"));
                #[cfg(unix)]
                let bytes = {
                    use std::os::unix::ffi::OsStrExt;
                    path.as_os_str().as_bytes().to_vec()
                };
                #[cfg(windows)]
                let bytes = {
                    use std::os::windows::ffi::OsStrExt;
                    path.as_os_str()
                        .encode_wide()
                        .flat_map(u16::to_le_bytes)
                        .collect::<Vec<_>>()
                };
                let key = format!("{id:064x}");
                photos
                    .execute(rusqlite::params![
                        id,
                        bytes,
                        1_792_324_800_i64 - id * 300,
                        key
                    ])
                    .unwrap();
                links.execute([id]).unwrap();
                aspects
                    .execute(rusqlite::params![
                        key,
                        if id % 3 == 0 { 0.667 } else { 1.6 }
                    ])
                    .unwrap();
            }
            drop((photos, links, aspects));
            tx.commit().unwrap();
        }
        let insert_ms = started.elapsed().as_secs_f64() * 1000.0;
        println!("100k metadata insertion: {insert_ms:.2}ms; loading snapshot");
        let started = Instant::now();
        let state = store::snapshot(&db).unwrap();
        let snapshot_ms = started.elapsed().as_secs_f64() * 1000.0;
        println!("100k snapshot: {snapshot_ms:.2}ms; building layout");
        assert_eq!(state.photos.len(), 100_000);
        assert_eq!(state.aspects.len(), 100_000);
        drop(db);
        let ctx = egui::Context::default();
        let mut library = Library::with_paths(&ctx, database, temp.path().join("cache"));
        library.thumbnails.restore_aspects(&state.aspects);
        library.state = state;
        library.ready = true;
        let started = Instant::now();
        library.rebuild_rows(1260.0, 1);
        let layout_ms = started.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(library.filtered.len(), 100_000);
        let rows = library.rows.len();
        let mut settings = AppSettings::default();
        let mut times = Vec::new();
        let mut max_shapes = 0;
        for _ in 0..60 {
            let started = Instant::now();
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1500.0, 960.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    let _ = library.show(ctx, &mut settings);
                },
            );
            max_shapes = max_shapes.max(output.shapes.len());
            times.push(started.elapsed().as_secs_f64() * 1000.0);
            assert!(library.thumbnails.pending_count() <= 2104);
        }
        assert!(
            max_shapes < 1000,
            "offscreen photos must not become widgets/shapes"
        );
        times.sort_by(f64::total_cmp);
        library.query = "photo-099999".into();
        let started = Instant::now();
        library.rebuild_rows(1260.0, 1);
        let search_ms = started.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(library.filtered.len(), 1);
        println!("100k metadata probe: insert_ms={insert_ms:.2}, snapshot_ms={snapshot_ms:.2}, layout_ms={layout_ms:.2}, rows={rows}, warm_frame_p50_ms={:.2}, warm_frame_p95_ms={:.2}, max_shapes={max_shapes}, search_ms={search_ms:.2}", times[30], times[57]);
        library.query.clear();
        library.layout_key = None;
        library.rebuild_rows_at(1248.0, 96.0);
        settings.thumbnail_density = 2;
        let mut moving = Vec::new();
        for frame in 0..180 {
            library.scroll_offset = frame as f32 * 40.0;
            library.scroll_anchor = None;
            library.thumbnails.revision += 1; // Model a continuously arriving decoder batch.
            let start = Instant::now();
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1500.0, 960.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    let _ = library.show(ctx, &mut settings);
                },
            );
            moving.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        moving.sort_by(f64::total_cmp);
        println!("100k small-grid moving+aspect-arrivals CPU: p50={:.3}ms p95={:.3}ms max={:.3}ms (no GPU submit or disk decode)",moving[90],moving[171],moving[179]);
    }

    #[test]
    fn hover_heart_toggles_favorite_without_opening_and_new_tab_filters() {
        use egui_kittest::kittest::Queryable;
        let (temp, source, database) = fixture();
        let ctx = egui::Context::default();
        let lib = Library::with_paths(&ctx, database, temp.path().join("cache"));
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1500.0, 960.0))
            .build_state(
                |ctx, (lib, settings, open)| {
                    lib.poll(ctx);
                    *open = lib.show(ctx, settings);
                },
                (lib, AppSettings::default(), None::<OpenPhoto>),
            );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !h.state().0.ready {
            h.run_steps(2);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        h.run_steps(3);
        let rect = h.get_by_label("Фото-00.png").raw_bounds().unwrap();
        h.input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(
                rect.x1 as f32 - 20.0,
                rect.y0 as f32 + 20.0,
            )));
        h.run_steps(3);
        let heart = h
            .get_by_label("В избранное: Фото-00.png")
            .raw_bounds()
            .unwrap();
        let pos = egui::pos2(
            (heart.x0 + heart.x1) as f32 / 2.0,
            (heart.y0 + heart.y1) as f32 / 2.0,
        );
        h.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(1);
        h.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(2);
        while h.state().0.state.favorites.is_empty() {
            h.run_steps(2);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(h.state().2.is_none(), "heart click must not open photo");
        h.get_by_label("Избранное").click();
        h.run_steps(3);
        assert!(h.state().0.section == Section::Favorites);
        assert_eq!(h.state().0.filtered.len(), 1);
        h.get_by_label("Убрать из избранного: Фото-00.png").click();
        h.run_steps(2);
        while !h.state().0.state.favorites.is_empty() {
            h.run_steps(2);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        h.run_steps(2);
        assert!(h.state().0.filtered.is_empty());
        assert!(source.join("Фото-00.png").exists());
    }

    #[test]
    fn timeline_marker_moves_while_scrolling_within_one_month() {
        use egui_kittest::kittest::Queryable;
        let (temp, _, database) = fixture();
        let ctx = egui::Context::default();
        let lib = Library::with_paths(&ctx, database, temp.path().join("cache"));
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1500.0, 960.0))
            .build_state(
                |ctx, (lib, settings)| {
                    lib.poll(ctx);
                    let _ = lib.show(ctx, settings);
                },
                (lib, AppSettings::default()),
            );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !h.state().0.ready {
            h.run_steps(2);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        h.run_steps(3);
        let before = h.state().0.timeline_marker.unwrap();
        h.state_mut().0.scroll_offset = 300.0;
        h.state_mut().0.scroll_anchor = None;
        h.run_steps(3);
        assert_eq!(h.state().0.months.len(), 1);
        assert!(h.state().0.timeline_marker.unwrap() > before + 10.0);
        h.get_by_label("Текущая дата: Октябрь 2026");
        assert!(h.ctx.style().spacing.scroll.handle_min_length >= 64.0);
    }

    #[test]
    fn arriving_aspects_do_not_reflow_rows_during_active_scroll() {
        let (temp, _, database) = fixture();
        let ctx = egui::Context::default();
        let lib = Library::with_paths(&ctx, database, temp.path().join("cache"));
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1500.0, 960.0))
            .build_state(
                |ctx, (lib, settings)| {
                    // This is a layout/debounce test, not a decoder throughput
                    // deadline. Keep async results out after the initial snapshot
                    // so slow CI disks cannot keep changing the target revision.
                    if !lib.ready {
                        lib.poll(ctx);
                    }
                    let _ = lib.show(ctx, settings);
                },
                (lib, AppSettings::default()),
            );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !h.state().0.ready {
            h.run_steps(2);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let before = h.state().0.layout_key.as_ref().unwrap().2;
        let filtered = h.state().0.filtered.clone();
        for n in 1..12 {
            h.state_mut().0.thumbnails.revision += 1;
            h.state_mut().0.scroll_offset = n as f32 * 10.0;
            h.state_mut().0.scroll_anchor = None;
            h.run_steps(1);
            assert_eq!(
                h.state().0.layout_key.as_ref().unwrap().2,
                before,
                "decoder arrivals must not repack during scroll"
            );
        }
        assert_eq!(h.state().0.filtered, filtered);
        // Advance only the recorded quiet/reflow deadlines. Actual decoding is
        // covered separately; waiting for every pending PNG made this regression
        // depend on Windows runner load instead of the debounce contract.
        let elapsed = std::time::Instant::now() - std::time::Duration::from_secs(1);
        let offset = h.state().0.scroll_offset;
        h.ctx.data_mut(|d| {
            d.insert_temp(
                egui::Id::new("library.aspect_reflow"),
                (offset, elapsed, elapsed),
            )
        });
        h.run_steps(1);
        assert_eq!(
            h.state().0.layout_key.as_ref().unwrap().2,
            h.state().0.thumbnails.revision
        );
        assert_eq!(h.state().0.filtered, filtered);
    }

    #[test]
    fn scan_status_shows_discovery_and_known_denominator() {
        use egui_kittest::kittest::Queryable;
        let (temp, _, database) = fixture();
        let ctx = egui::Context::default();
        let lib = Library::with_paths(&ctx, database, temp.path().join("cache"));
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1500.0, 960.0))
            .build_state(
                |ctx, (lib, settings)| {
                    lib.poll(ctx);
                    let _ = lib.show(ctx, settings);
                },
                (lib, AppSettings::default()),
            );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !h.state().0.ready {
            h.run_steps(2);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        h.state_mut().0.scanning = true;
        h.state()
            .0
            .worker
            .total
            .store(usize::MAX, std::sync::atomic::Ordering::Relaxed);
        h.state()
            .0
            .worker
            .progress
            .store(128, std::sync::atomic::Ordering::Relaxed);
        h.run_steps(2);
        h.get_by_label("Найдено: 128 · подсчёт файлов…");
        h.state()
            .0
            .worker
            .total
            .store(30323, std::sync::atomic::Ordering::Relaxed);
        h.state()
            .0
            .worker
            .progress
            .store(1000, std::sync::atomic::Ordering::Relaxed);
        h.run_steps(2);
        h.get_by_label("Обработано: 1000 / 30323");
    }

    #[test]
    fn offline_photos_are_absent_from_grid_picker_and_viewer_order_then_return() {
        use egui_kittest::kittest::Queryable;
        let (temp, source, database) = fixture();
        let db = store::connect(&database).unwrap();
        let photo = store::snapshot(&db).unwrap().photos[0].clone();
        let cache = temp.path().join("cache");
        thumbnails::load_detailed(&cache, &photo.path, &photo.key).unwrap();
        let png = cache.join(format!("{}.png", photo.key));
        let bytes = std::fs::read(&png).unwrap();
        drop(db);
        let offline = temp.path().join("offline");
        std::fs::rename(&source, &offline).unwrap();
        let ctx = egui::Context::default();
        let library = Library::with_paths(&ctx, database, cache);
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1500.0, 960.0))
            .build_state(
                |ctx, (lib, settings)| {
                    lib.poll(ctx);
                    let _ = lib.show(ctx, settings);
                },
                (library, AppSettings::default()),
            );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while !h.state().0.ready {
            h.run_steps(2);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        h.run_steps(3);
        assert_eq!(
            h.state().0.state.photos.len(),
            18,
            "index retains offline identities"
        );
        assert!(h.state().0.filtered.is_empty());
        assert!(
            h.state().0.rows.is_empty(),
            "no empty cells or month headers"
        );
        assert!(h.query_by_label("Фото-00.png").is_none());
        assert!(h.query_by_label("Файл недоступен").is_none());
        h.state_mut().0.start_picker(None);
        h.run_steps(3);
        assert!(h.query_by_label("Фото-00.png").is_none());
        h.press_key(egui::Key::Escape);
        h.run_steps(3);
        std::fs::rename(&offline, &source).unwrap();
        while h.state().0.filtered.len() != 18 {
            h.run_steps(2);
            assert!(
                std::time::Instant::now() < deadline,
                "offline items did not return"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(h.state().0.open(0).paths.len(), 18);
        assert_eq!(std::fs::read(png).unwrap(), bytes);
    }

    #[test]
    fn photo_context_menu_offers_file_manager_without_launching_it() {
        use egui_kittest::kittest::Queryable;
        let (temp, _, database) = fixture();
        let ctx = egui::Context::default();
        let library = Library::with_paths(&ctx, database, temp.path().join("cache"));
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1500.0, 960.0))
            .build_state(
                |ctx, (lib, settings)| {
                    lib.poll(ctx);
                    let _ = lib.show(ctx, settings);
                },
                (library, AppSettings::default()),
            );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !h.state().0.ready {
            h.run_steps(2);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        h.run_steps(4);
        let r = h.get_by_label("Фото-00.png").raw_bounds().unwrap();
        let pos = egui::pos2((r.x0 + r.x1) as f32 / 2.0, (r.y0 + r.y1) as f32 / 2.0);
        h.input_mut().events.push(egui::Event::PointerMoved(pos));
        for pressed in [true, false] {
            h.input_mut().events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            h.run_steps(1);
        }
        h.run_steps(3);
        h.get_by_label("Показать в проводнике");
        assert!(!h.state().0.catalog.jobs.busy());
    }

    #[test]
    fn cache_error_report_reopens_from_disk_and_blocks_background_actions() {
        use egui_kittest::kittest::Queryable;
        let (temp, source, database) = fixture();
        let cache = temp.path().join("cache");
        std::fs::create_dir(&cache).unwrap();
        let failures = vec![thumbnail_jobs::Failure {
            path: source.join("empty.jpg"),
            reason: "Original file is empty (0 bytes)".into(),
        }];
        std::fs::write(
            cache.join("last-build-errors.json"),
            serde_json::to_vec(&failures).unwrap(),
        )
        .unwrap();
        let ctx = egui::Context::default();
        let lib = Library::with_paths(&ctx, database, cache);
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1000.0, 760.0))
            .build_state(
                |ctx, (lib, settings)| {
                    lib.poll(ctx);
                    let _ = lib.show(ctx, settings);
                },
                (lib, AppSettings::default()),
            );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !h.state().0.ready || h.state().0.thumbnails.failures().is_empty() {
            h.run_steps(2);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        h.get_by_label("…").click();
        h.run_steps(3);
        h.get_by_label("Ошибки кеша миниатюр").click();
        h.run_steps(3);
        assert!(h.state().0.cache_errors_open);
        h.get_by_label("empty.jpg");
        let reason = h
            .get_by_label("Original file is empty (0 bytes)")
            .raw_bounds()
            .unwrap();
        let close = h.get_by_label("Закрыть").raw_bounds().unwrap();
        assert!(
            close.y0 - reason.y1 < 40.0,
            "error modal must shrink to its actual contents, not reserve empty rows"
        );
        h.press_key(egui::Key::F5);
        h.run_steps(2);
        assert!(!h.state().0.scanning);
        h.get_by_label("Закрыть").click();
        h.run_steps(3);
        assert!(!h.state().0.cache_errors_open);
        assert!(source.join("Фото-00.png").is_file());
    }

    #[test]
    fn cache_all_button_builds_entire_library_even_when_search_filters_one_photo() {
        use egui_kittest::kittest::Queryable;
        let (temp, source, database) = fixture();
        let ctx = egui::Context::default();
        let lib = Library::with_paths(&ctx, database, temp.path().join("cache"));
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1500.0, 960.0))
            .build_state(
                |ctx, (lib, settings)| {
                    lib.poll(ctx);
                    let _ = lib.show(ctx, settings);
                },
                (lib, AppSettings::default()),
            );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !h.state().0.ready {
            h.run_steps(2);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        h.state_mut().0.query = "Фото-00".into();
        h.run_steps(3);
        assert_eq!(h.state().0.filtered.len(), 1);
        h.get_by_label("…").click();
        h.run_steps(3);
        h.get_by_label("Прогрузить все миниатюры").click();
        h.run_steps(3);
        assert_eq!(h.state().0.thumbnails.bulk_status().total, 18);
        assert_eq!(h.state().1.thumbnail_disk_gib, 0);
        while h.state().0.thumbnails.bulk_status().running {
            h.run_steps(2);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(h.state().0.thumbnails.bulk_status().failed, 0);
        for photo in &h.state().0.state.photos {
            assert!(temp
                .path()
                .join("cache")
                .join(format!("{}.png", photo.key))
                .is_file());
        }
        assert!(source.join("Фото-00.png").is_file());
        // The background job can complete just after the last rendered frame.
        h.run_steps(3);
        h.get_by_label("Скрыть уведомление").click();
        h.run_steps(2);
        assert_eq!(h.state().0.thumbnails.bulk_status().total, 0);
        h.state_mut().0.settings_open = true;
        h.run_steps(3);
        assert!(
            h.query_by_label("Прогрузить все миниатюры").is_none(),
            "task launch belongs to the main menu, not preferences"
        );
    }

    #[test]
    fn grid_zoom_is_continuous_pinch_compatible_and_presets_reset_it() {
        use egui_kittest::kittest::Queryable;
        let (temp, _, database) = fixture();
        let ctx = egui::Context::default();
        let lib = Library::with_paths(&ctx, database, temp.path().join("cache"));
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1500.0, 960.0))
            .build_state(
                |ctx, (lib, settings)| {
                    lib.poll(ctx);
                    let _ = lib.show(ctx, settings);
                },
                (lib, AppSettings::default()),
            );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !h.state().0.ready {
            h.run_steps(2);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        h.run_steps(3);
        let count = h.state().0.filtered.len();
        h.input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(650.0, 450.0)));
        h.input_mut().events.push(egui::Event::Zoom(1.13));
        h.run_steps(1);
        assert!(
            (h.state().1.thumbnail_size - 289.28).abs() < 1.0,
            "{}",
            h.state().1.thumbnail_size
        );
        assert_eq!(h.state().0.filtered.len(), count);
        assert_eq!(h.state().0.section, Section::Collection);
        assert_eq!(
            h.state().0.layout_key.as_ref().unwrap().1,
            h.state().1.thumbnail_size.to_bits()
        );
        h.input_mut().events.push(egui::Event::Zoom(1.07));
        h.run_steps(1);
        assert!(h.state().1.thumbnail_size > 300.0);
        h.get_by_label("Средние миниатюры").click();
        h.run_steps(2);
        assert_eq!(h.state().1.thumbnail_size, 0.0);
        assert_eq!(
            h.state().0.layout_key.as_ref().unwrap().1,
            256.0_f32.to_bits()
        );
        let ctrl = egui::Modifiers {
            ctrl: true,
            ..Default::default()
        };
        h.input_mut().events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, 90.0),
            modifiers: ctrl,
        });
        h.run_steps(1);
        assert!(
            h.state().1.thumbnail_size > 256.0,
            "Ctrl+wheel must resize photos, not the entire app"
        );
        assert_eq!(h.ctx.zoom_factor(), 1.0);
        h.get_by_label("Средние миниатюры").click();
        h.run_steps(2);
        // Real multi-touch events (seed the pointer used by egui's gesture state).
        h.input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(500.0, 450.0)));
        h.run_steps(1);
        let touch = |id, phase, x| egui::Event::Touch {
            device_id: egui::TouchDeviceId(1),
            id: egui::TouchId(id),
            phase,
            pos: egui::pos2(x, 450.0),
            force: None,
        };
        h.input_mut().events.extend([
            touch(1, egui::TouchPhase::Start, 500.0),
            touch(2, egui::TouchPhase::Start, 700.0),
        ]);
        h.run_steps(1);
        h.input_mut().events.extend([
            touch(1, egui::TouchPhase::Move, 460.0),
            touch(2, egui::TouchPhase::Move, 740.0),
        ]);
        h.run_steps(1);
        assert!(h.state().1.thumbnail_size > 320.0);
        assert_eq!(h.state().0.section, Section::Collection);
        h.input_mut().events.extend([
            touch(1, egui::TouchPhase::End, 460.0),
            touch(2, egui::TouchPhase::End, 740.0),
        ]);
        h.run_steps(2);
        assert!(h.state().0.selection.is_empty());
    }

    #[test]
    fn overflow_has_flat_reference_rows_and_favorites_is_only_a_tab() {
        use egui_kittest::kittest::Queryable;
        let (temp, _, database) = fixture();
        let ctx = egui::Context::default();
        let lib = Library::with_paths(&ctx, database, temp.path().join("cache"));
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1500.0, 960.0))
            .build_state(
                |ctx, (lib, settings)| {
                    lib.poll(ctx);
                    let _ = lib.show(ctx, settings);
                },
                (lib, AppSettings::default()),
            );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !h.state().0.ready {
            h.run_steps(2);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        h.run_steps(3);
        h.get_by_label("…").click();
        h.run_steps(3);
        let refresh = h.get_by_label("Обновить · F5").raw_bounds().unwrap();
        let settings = h.get_by_label("Настройки").raw_bounds().unwrap();
        assert_eq!(refresh.height(), 32.0);
        assert!((refresh.width() - 160.0).abs() < 2.0, "{refresh:?}");
        assert_eq!(settings.y0 - refresh.y0, 64.0);
        let preload = h
            .get_by_label("Прогрузить все миниатюры")
            .raw_bounds()
            .unwrap();
        let x = (settings.x0 + settings.x1) as f32 / 2.0;
        for y in [
            settings.y1 as f32 + 0.5,
            ((settings.y1 + preload.y0) / 2.0) as f32,
            preload.y0 as f32 - 0.5,
        ] {
            h.input_mut()
                .events
                .push(egui::Event::PointerMoved(egui::pos2(x, y)));
            h.run_steps(2);
            assert!(!h.output().shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Mesh(mesh) if mesh.vertices.len()==42)),"flyout separator: settings={settings:?} preload={preload:?} pointer=({x},{y}) meshes={:?}",h.output().shapes.iter().filter_map(|s|matches!(&s.shape,egui::Shape::Mesh(m) if m.vertices.len()==42).then_some(s.clip_rect)).collect::<Vec<_>>());
        }
        for bounds in [settings, preload] {
            h.input_mut()
                .events
                .push(egui::Event::PointerMoved(egui::pos2(
                    (bounds.x0 + bounds.x1) as f32 / 2.0,
                    (bounds.y0 + bounds.y1) as f32 / 2.0,
                )));
            h.run_steps(2);
            assert_eq!(h.output().shapes.iter().filter(|shape|matches!(&shape.shape,egui::Shape::Mesh(mesh) if mesh.vertices.len()==42)).count(),1,"only one hovered menu action may glow");
        }
        let favorite = h.get_by_label("Избранное").raw_bounds().unwrap();
        assert!(
            favorite.y0 < 128.0,
            "favorite must be a section tab, not a flyout item"
        );
        h.press_key(egui::Key::Escape);
        h.run_steps(2);
        assert!(h.query_by_label("Настройки").is_none());
    }

    #[test]
    fn empty_folders_are_navigable_in_library_and_album_picker() {
        use egui_kittest::kittest::Queryable;
        let (temp, source, database) = fixture();
        let empty = source.join("Empty folder");
        let nested = empty.join("Nested empty");
        std::fs::create_dir_all(&nested).unwrap();
        let mut db = store::connect(&database).unwrap();
        store::scan(&mut db, &std::sync::atomic::AtomicU64::new(1), 1, |_| {}).unwrap();
        drop(db);
        let ctx = egui::Context::default();
        let lib = Library::with_paths(&ctx, database, temp.path().join("cache"));
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1500.0, 960.0))
            .build_state(
                |ctx, (lib, settings)| {
                    lib.poll(ctx);
                    let _ = lib.show(ctx, settings);
                },
                (lib, AppSettings::default()),
            );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !h.state().0.ready {
            h.run_steps(2);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        h.get_by_label("Папки").click();
        h.run_steps(3);
        h.get_by_label(&format!("Открыть папку {}", source.display()))
            .click();
        h.run_steps(3);
        h.get_by_label(&format!("Открыть папку {}", empty.display()))
            .click();
        h.run_steps(3);
        assert_eq!(h.state().0.folder.as_ref(), Some(&empty));
        assert!(h.query_by_label("Фото-00.png").is_none());
        h.get_by_label(&format!("Открыть папку {}", nested.display()))
            .click();
        h.run_steps(3);
        assert_eq!(h.state().0.folder.as_ref(), Some(&nested));
        assert!(h.state().0.filtered.is_empty());

        h.state_mut().0.start_picker(None);
        h.run_steps(3);
        h.get_by_label("Папки").click();
        h.run_steps(3);
        h.get_by_label(&source.to_string_lossy()).click();
        h.run_steps(3);
        h.get_by_label("Empty folder").click();
        h.run_steps(3);
        h.get_by_label("Nested empty").click();
        h.run_steps(3);
        assert_eq!(
            h.state().0.catalog.picker.as_ref().unwrap().folder.as_ref(),
            Some(&nested)
        );
        assert!(h.query_by_label("Фото-00.png").is_none());
        assert!(source.join("Фото-00.png").exists());
    }

    #[test]
    fn native_watch_updates_library_without_manual_refresh() {
        let (temp, source, database) = fixture();
        let ctx = egui::Context::default();
        let mut library = Library::with_paths(&ctx, database, temp.path().join("cache"));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while library.state.photos.len() != 18 {
            library.poll(&ctx);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let path = source.join("new-arrival.png");
        // Allow native registration to settle; a small write burst is debounced.
        for _ in 0..4 {
            image::RgbImage::new(24, 16).save(&path).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        while library.state.photos.len() != 19 {
            library.poll(&ctx);
            assert!(
                std::time::Instant::now() < deadline,
                "watch did not trigger indexing: {:?}",
                library.error
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(library.state.photos.iter().any(|p| p.path == path));
        assert_eq!(image::open(path).unwrap().width(), 24);
    }

    #[test]
    fn keyboard_reaches_virtualized_photos_and_does_not_steal_search_input() {
        use egui_kittest::kittest::Queryable;
        let (temp, _, database) = fixture();
        let ctx = egui::Context::default();
        let library = Library::with_paths(&ctx, database, temp.path().join("cache"));
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1000.0, 760.0))
            .build_state(
                |ctx, (lib, settings, opened)| {
                    lib.poll(ctx);
                    if let Some(photo) = lib.show(ctx, settings) {
                        *opened = Some(photo);
                    }
                },
                (library, AppSettings::default(), None::<OpenPhoto>),
            );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while harness.state().0.state.photos.len() != 18 {
            harness.run_steps(2);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        harness.run_steps(3);
        harness.get_by_label("Фото-00.png").focus();
        harness.run_steps(2);
        assert!(
            harness.ctx.wants_keyboard_input(),
            "real thumbnail focus must not disable grid navigation"
        );
        harness.press_key(egui::Key::End);
        harness.run_steps(2);
        let id = harness.state().0.state.photos[17].id;
        assert_eq!(harness.state().0.focused_photo, Some(id));
        assert!(harness.state().0.scroll_offset > 0.0);
        harness.press_key(egui::Key::Space);
        harness.run_steps(2);
        assert!(harness.state().0.selection.contains(&id));
        harness.press_key(egui::Key::Escape);
        harness.run_steps(2);
        harness.press_key(egui::Key::Enter);
        harness.run_steps(2);
        assert_eq!(harness.state().2.as_ref().unwrap().index, 17);
        harness.get_by_label("Поиск по имени или пути").focus();
        harness.run_steps(2);
        harness.press_key(egui::Key::Home);
        harness.run_steps(2);
        assert_eq!(harness.state().0.focused_photo, Some(id));
    }

    #[test]
    fn library_buttons_select_open_and_switch_sections() {
        use egui_kittest::kittest::Queryable;
        let (temp, _source, database) = fixture();
        let ctx = egui::Context::default();
        let mut library = Library::with_paths(&ctx, database, temp.path().join("cache"));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while library.state.photos.len() < 18 {
            library.poll(&ctx);
            assert!(
                std::time::Instant::now() < deadline,
                "worker did not deliver snapshot"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let state = (library, AppSettings::default(), None::<OpenPhoto>);
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_state(
                |ctx, (library, settings, opened)| {
                    library.poll(ctx);
                    if let Some(photo) = library.show(ctx, settings) {
                        *opened = Some(photo);
                    }
                },
                state,
            );
        harness.run_steps(3);
        assert!(
            harness.state().0.grid_right > 1260.0,
            "scrollbar must stay at window edge, outside gallery margin"
        );
        harness.get_by_label("Фото-00.png").click();
        harness.run_steps(2);
        assert_eq!(harness.state().2.as_ref().unwrap().paths.len(), 18);
        harness.get_by_label("Выбрать").click();
        harness.run_steps(2);
        harness.get_by_label("Фото-00.png").click();
        harness.run_steps(2);
        assert_eq!(harness.state().0.selection.len(), 1);
        assert!(
            harness.query_by_label("Альбомы").is_none(),
            "selection only shows current section"
        );
        harness.get_by_label("Отмена").click();
        harness.run_steps(2);
        harness.get_by_label("Альбомы").click();
        harness.run_steps(2);
        assert!(harness.state().0.section == Section::Albums);
        harness.get_by_label("+ Новый альбом").click();
        harness.run_steps(2);
        assert!(harness.state().0.catalog.picker.is_some());
        assert!(
            harness.query_by_label("Коллекция").is_some(),
            "picker embeds Collection/Folders without changing the underlying section"
        );
        harness.get_by_label("Папки").click();
        harness.run_steps(2);
        assert!(harness.state().0.catalog.picker.as_ref().unwrap().folders);
        assert!(
            harness.query_by_label("Фото-00.png").is_none(),
            "folder root shows sources, not recursive photos"
        );
        let source = harness.state().0.state.sources[0]
            .path
            .to_string_lossy()
            .into_owned();
        harness.get_by_label(&source).click();
        harness.run_steps(2);
        assert!(harness.query_by_label("Фото-00.png").is_some());
        assert!(harness.state().0.section == Section::Albums);
        harness.input_mut().events.push(egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run_steps(2);
        assert!(harness.state().0.catalog.picker.is_none());
        assert!(harness.state().0.section == Section::Albums);
    }

    #[test]
    fn discovery_cards_and_saved_search_dropdown_use_real_data() {
        use egui_kittest::kittest::Queryable;
        let (temp, _source, database) = fixture();
        let db = store::connect(&database).unwrap();
        store::create_album(&db, "Прогулки").unwrap();
        db.execute(
            "INSERT INTO saved_queries(name,query) VALUES ('Любимый поиск','Фото-00')",
            [],
        )
        .unwrap();
        let ctx = egui::Context::default();
        let mut lib = Library::with_paths(&ctx, database, temp.path().join("cache"));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !lib.ready {
            lib.poll(&ctx);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1500.0, 960.0))
            .build_state(
                |ctx, (lib, settings)| {
                    lib.poll(ctx);
                    lib.show(ctx, settings);
                },
                (lib, AppSettings::default()),
            );
        h.run_steps(3);
        assert!(h.query_by_label("Рекомендация: Прогулки").is_some());
        h.get_by_label("Скрыть рекомендации").click();
        h.run_steps(3);
        assert!(h.query_by_label("Рекомендация: Прогулки").is_none());
        h.get_by_label("Показать рекомендации").click();
        h.run_steps(3);
        h.get_by_label("Рекомендация: Прогулки").click();
        h.run_steps(3);
        assert!(h.state().0.album.is_some());
        h.state_mut().0.back_scene();
        h.state_mut().0.section = Section::Collection;
        h.run_steps(3);
        h.get_by_label("Поиск по имени или пути").focus();
        h.run_steps(3);
        h.get_by_label("Любимый поиск").click();
        h.run_steps(3);
        assert_eq!(h.state().0.query, "Фото-00");
        assert_eq!(h.state().0.filtered.len(), 1);
        assert!(!h.state().0.discovery.search_open);
        assert!(h.query_by_label("Рекомендация: Прогулки").is_none());
    }

    #[test]
    fn shell_geometry_selection_replacement_and_narrow_layout() {
        use egui_kittest::kittest::Queryable;
        let (temp, _source, database) = fixture();
        let ctx = egui::Context::default();
        let mut library = Library::with_paths(&ctx, database, temp.path().join("cache"));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while library.state.photos.len() < 18 {
            library.poll(&ctx);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1500.0, 960.0))
            .build_state(
                |ctx, (lib, settings)| {
                    lib.show(ctx, settings);
                },
                (library, AppSettings::default()),
            );
        h.run_steps(3);
        let search = h.state().0.search_rect.unwrap();
        assert_eq!(
            (search.left(), search.top(), search.width(), search.height()),
            (16.0, 38.0, 396.0, 36.0)
        );
        let tabs = h.get_by_label("Коллекция").raw_bounds().unwrap();
        assert_eq!((tabs.x0, tabs.y0, tabs.height()), (16.0, 80.0, 48.0));
        // The actual shell's padded hit targets must not shift Reveal away
        // from their centered titles, in either direction.
        for label in ["Коллекция", "Альбомы", "Папки", "Избранное"] {
            let bounds = h.get_by_label(label).raw_bounds().unwrap();
            h.input_mut()
                .events
                .push(egui::Event::PointerMoved(egui::pos2(
                    (bounds.x0 + bounds.x1) as f32 / 2.0,
                    (bounds.y0 + bounds.y1) as f32 / 2.0,
                )));
            h.run_steps(2);
            let text = h
                .output()
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == label => {
                        Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
                    }
                    _ => None,
                })
                .unwrap();
            let glow=h.output().shapes.iter().find(|shape|matches!(&shape.shape,egui::Shape::Mesh(mesh) if mesh.vertices.len()==42)).unwrap();
            assert!(
                glow.clip_rect.center().distance(text.center()) < 0.01,
                "{label}: Reveal must be centered on the title"
            );
            assert!(glow.clip_rect.left() < text.left() && glow.clip_rect.right() > text.right());
        }
        for point in [
            search.center(),
            egui::pos2(1450.0, h.state().0.timeline_marker.unwrap()),
        ] {
            h.input_mut().events.push(egui::Event::PointerMoved(point));
            h.run_steps(2);
            assert!(!h.output().shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Mesh(mesh) if mesh.vertices.len()==42)),"search and active date must not have Reveal artifacts");
        }
        h.state_mut().0.settings_open = true;
        h.run_steps(2);
        let bounds = h.get_by_label("Источники").raw_bounds().unwrap();
        h.input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(
                (bounds.x0 + bounds.x1) as f32 / 2.0,
                (bounds.y0 + bounds.y1) as f32 / 2.0,
            )));
        h.run_steps(2);
        assert!(
            !h.output().shapes.iter().any(
                |shape| matches!(&shape.shape,egui::Shape::Mesh(mesh) if mesh.vertices.len()==42)
            ),
            "settings category heading is informational"
        );
        h.state_mut().0.settings_open = false;
        h.run_steps(2);
        let first_photo = h.get_by_label("Фото-00.png").raw_bounds().unwrap();
        assert_eq!(
            first_photo.x0, 16.0,
            "gallery must use the same slim gutter as commands/tabs"
        );
        let density = h.get_by_label("Крупные миниатюры").raw_bounds().unwrap();
        assert_eq!(
            (density.x0, density.y0, density.width(), density.height()),
            (1409.0, 82.0, 25.0, 44.0)
        );
        h.get_by_label("Выбрать").click();
        h.run_steps(2);
        assert!(h.query_by_label("Поиск по имени или пути").is_none());
        assert!(h.query_by_label("Альбомы").is_none());
        assert!(h.query_by_label("Папки").is_none());
        let tabs = h.get_by_label("Коллекция").raw_bounds().unwrap();
        assert_eq!(
            (tabs.y0, tabs.height()),
            (80.0, 48.0),
            "selection replaces commands, not an extra strip"
        );
        assert!(h.get_by_label("Добавить в альбом").is_disabled());
        h.get_by_label("Выбрать группу 18").click();
        h.run_steps(2);
        assert_eq!(h.state().0.selection.len(), 18);
        h.get_by_label("Снять выделение группы").click();
        h.run_steps(2);
        assert!(h.state().0.selection.is_empty());
        h.get_by_label("Фото-00.png").click();
        h.run_steps(2);
        assert!(!h.get_by_label("Добавить в альбом").is_disabled());
        h.get_by_label("Добавить в альбом").click();
        h.run_steps(2);
        assert!(h.state().0.assign_album_open);
        h.press_key(egui::Key::Escape);
        h.run_steps(2);
        assert!(!h.state().0.assign_album_open);
        h.get_by_label("Отмена").click();
        h.run_steps(2);
        assert!(h.state().0.selection.is_empty());
        assert!(h.query_by_label("Поиск по имени или пути").is_some());
        h.set_size(egui::vec2(500.0, 700.0));
        h.run_steps(3);
        let select = h.get_by_label("Выбрать").raw_bounds().unwrap();
        assert_eq!((select.y0, select.height()), (32.0, 48.0));
        let search = h
            .get_by_label("Поиск по имени или пути")
            .raw_bounds()
            .unwrap();
        assert!(
            search.x1 < select.x0,
            "narrow search must not overlap commands"
        );
        let density = h.get_by_label("Мелкие миниатюры").raw_bounds().unwrap();
        assert!(density.x1 <= 484.0);
        h.get_by_label("…").click();
        h.run_steps(2);
        assert!(h.query_by_label("Новый альбом").is_some());
        assert!(h.query_by_label("Добавить папку").is_some());
        h.press_key(egui::Key::Escape);
        h.run_steps(2);
        h.get_by_label("Выбрать").click();
        h.run_steps(2);
        h.state_mut().0.folder = Some(PathBuf::from("/keep-this-filter"));
        let cancel = h.get_by_label("Отмена").raw_bounds().unwrap();
        assert_eq!((cancel.y0, cancel.height()), (32.0, 48.0));
        h.get_by_label("…").click();
        h.run_steps(2);
        assert!(
            h.query_by_label("Выбрать всё").is_some(),
            "selection overflow opens"
        );
        assert!(h.state().0.selecting, "still in selection before Escape");
        assert!(h.state().0.popup_open_last_frame, "menu state tracked");
        h.press_key(egui::Key::Escape);
        h.run_steps(2);
        assert!(
            h.state().0.selecting,
            "Escape closes overflow before cancelling selection"
        );
        h.press_key(egui::Key::Escape);
        h.run_steps(2);
        assert!(!h.state().0.selecting);
        assert_eq!(
            h.state().0.folder,
            Some(PathBuf::from("/keep-this-filter")),
            "cancel selection must retain scene filters"
        );
    }

    #[test]
    fn month_navigation_semantic_zoom_and_reflow_anchor() {
        use egui_kittest::kittest::Queryable;
        let (temp, _source, database) = fixture();
        let state = store::snapshot(&store::connect(&database).unwrap()).unwrap();
        let ctx = egui::Context::default();
        let mut library = Library::with_paths(&ctx, database.clone(), temp.path().join("cache"));
        library.state = state;
        library.ready = true;
        for (i, photo) in library.state.photos.iter_mut().enumerate() {
            photo.modified = chrono::NaiveDate::from_ymd_opt(2026, 10 - i as u32 / 6, 15)
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap()
                .and_utc()
                .timestamp();
        }
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1500.0, 960.0))
            .build_state(
                |ctx, (lib, settings)| {
                    lib.show(ctx, settings);
                },
                (library, AppSettings::default()),
            );
        h.run_steps(3);
        assert_eq!(h.state().0.months.len(), 3);
        assert_eq!(h.state().0.months[0].key, "2026-10");
        assert_eq!(h.state().0.months[2].key, "2026-08");
        let first_name = h.state().0.state.photos[0]
            .path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let first = h.get_by_label(&first_name).raw_bounds().unwrap();
        let first_height = h
            .state()
            .0
            .rows
            .iter()
            .find_map(|row| {
                if let GridRow::Photos(row) = row {
                    Some(row.height)
                } else {
                    None
                }
            })
            .unwrap();
        assert!(
            (first.y1 - first.y0 - first_height as f64).abs() < 0.01,
            "photo response has no filename footer"
        );
        h.get_by_label("Октябрь 2026").click();
        h.run_steps(2);
        assert!(h.state().0.semantic_zoom);
        assert!(h.query_by_label("Назад к фотографиям").is_some());
        assert!(h.query_by_label("Фото-00.png").is_none());
        assert!(h.get_by_label("Выбрать").is_disabled());
        let month = h.get_by_label("Октябрь 2026").raw_bounds().unwrap();
        h.input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(
                1200.0,
                (month.y0 + month.y1) as f32 / 2.0,
            )));
        h.run_steps(2);
        assert!(
            !h.output().shapes.iter().any(
                |shape| matches!(&shape.shape,egui::Shape::Mesh(mesh) if mesh.vertices.len()==42)
            ),
            "blank right side of month row must not glow"
        );
        assert!(
            !h.get_by_label("…").is_disabled(),
            "overflow must work in month chooser"
        );
        h.get_by_label("…").click();
        h.run_steps(2);
        assert!(h.query_by_label("Настройки").is_some());
        h.press_key(egui::Key::Escape);
        h.run_steps(2);
        assert!(
            h.state().0.semantic_zoom,
            "Escape closes overflow before month chooser"
        );
        h.get_by_label("Август 2026").click();
        h.run_steps(3);
        assert!(!h.state().0.semantic_zoom);
        assert!(h.state().0.scroll_offset > 0.0);
        h.get_by_label("Перейти к Октябрь 2026").click();
        h.run_steps(3);
        assert!(h.state().0.scroll_offset < 1.0);
        h.get_by_label("Октябрь 2026").click();
        h.run_steps(2);
        h.get_by_label("Назад к фотографиям").click();
        h.run_steps(2);
        assert!(!h.state().0.semantic_zoom);
        h.input_mut().events.extend([
            egui::Event::PointerMoved(egui::pos2(700.0, 500.0)),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -240.0),
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        h.run_steps(12);
        assert!(
            h.state().0.scroll_offset > 0.0,
            "wheel scroll must not be reset by explicit offset"
        );
        let lib = &mut h.state_mut().0;
        let anchor_id = lib.state.photos[8].id;
        lib.scroll_anchor = Some((anchor_id, 20.0));
        lib.rebuild_rows(1260.0, 2);
        let row=lib.rows.iter().position(|r|matches!(r,GridRow::Photos(r) if r.cells.iter().any(|c|lib.state.photos[c.index].id==anchor_id))).unwrap();
        assert!((lib.row_offsets[row] - lib.scroll_offset - 20.0).abs() < 0.01);
        assert_eq!(month_label("2026-08", Language::En), "August 2026");
    }

    #[test]
    fn wheel_scroll_crosses_month_boundaries_without_layout_or_position_jump() {
        use egui_kittest::kittest::Queryable;
        let (temp, _, database) = fixture();
        let mut state = store::snapshot(&store::connect(&database).unwrap()).unwrap();
        state.albums.push(store::Album {
            id: 1,
            name: "Recommendations".into(),
            cover: None,
            modified: 0,
            automatic: false,
        });
        for (i, photo) in state.photos.iter_mut().enumerate() {
            photo.modified = chrono::NaiveDate::from_ymd_opt(2026, 10 - i as u32 / 6, 15)
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap()
                .and_utc()
                .timestamp();
        }
        let ctx = egui::Context::default();
        let mut library = Library::with_paths(&ctx, database, temp.path().join("cache"));
        library.thumbnails.restore_aspects(
            &state
                .photos
                .iter()
                .map(|p| {
                    let name = p.path.file_stem().unwrap().to_string_lossy();
                    let n = name.rsplit('-').next().unwrap().parse::<usize>().unwrap();
                    (p.key.clone(), if n % 3 == 0 { 2.0 / 3.0 } else { 1.6 })
                })
                .collect(),
        );
        library.state = state;
        library.ready = true;
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(900.0, 500.0))
            .with_step_dt(1.0 / 60.0)
            .build_state(
                |ctx, (lib, settings)| {
                    lib.thumbnails.poll(ctx);
                    lib.show(ctx, settings);
                },
                (library, AppSettings::default()),
            );
        h.run_steps(5);
        let offsets = h.state().0.row_offsets.clone();
        let mut previous = h.state().0.scroll_offset;
        let mut dates = HashSet::new();
        let mut previous_positions = std::collections::HashMap::<String, f64>::new();
        let mut previous_marker = None::<f32>;
        for frame in 0..360 {
            let down = frame < 180;
            h.input_mut().events.extend([
                egui::Event::PointerMoved(egui::pos2(400.0, 300.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, if down { -12.0 } else { 12.0 }),
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
            h.run_steps(1);
            let next = h.state().0.scroll_offset;
            if let Some((id, relative)) = h.state().0.scroll_anchor {
                let lib = &h.state().0;
                let r=lib.rows.iter().position(|r|matches!(r,GridRow::Photos(r) if r.cells.iter().any(|c|lib.state.photos[c.index].id==id))).unwrap();
                assert!(
                    (lib.row_offsets[r] + lib.discovery_height() - next - relative).abs() < 0.1,
                    "anchor lag at offset {next}: row {} relative {relative}",
                    lib.row_offsets[r]
                );
            }
            assert!(
                (next - previous).abs() <= 13.0,
                "scroll jumped {previous} -> {next}"
            );
            if frame != 180 {
                assert!(
                    if down {
                        next >= previous
                    } else {
                        next <= previous
                    },
                    "scroll reversed"
                );
            }
            assert_eq!(h.state().0.row_offsets, offsets);
            let geometry = h
                .ctx
                .data(|d| {
                    d.get_temp::<(egui::Vec2, egui::Rect)>(egui::Id::new(
                        "library.grid.test_geometry",
                    ))
                })
                .unwrap();
            assert!(
                (geometry.0.y
                    - h.state().0.row_offsets.last().unwrap()
                    - h.state().0.discovery_height())
                .abs()
                    < 0.1,
                "wrong content height {geometry:?}"
            );
            if let (Some(old), Some(new)) = (previous_marker, h.state().0.timeline_marker) {
                assert!((new - old).abs() < 13.0, "timeline jumped {old} -> {new}");
            }
            previous_marker = h.state().0.timeline_marker;
            for photo in &h.state().0.state.photos {
                let name = photo
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                if let Some(node) = h.query_by_label(&name) {
                    let b = node.raw_bounds().unwrap();
                    if let Some(old) = previous_positions.insert(name.clone(), b.y0) {
                        assert!(
                            (b.y0 - old).abs() <= 13.0,
                            "photo jumped {name}: {old} -> {}",
                            b.y0
                        );
                    }
                } else {
                    previous_positions.remove(&name);
                }
            }
            for key in ["Октябрь 2026", "Сентябрь 2026", "Август 2026"] {
                if let Some(node) = h.query_by_label(key) {
                    let b = node.raw_bounds().unwrap();
                    assert!((b.height() - 30.0).abs() < 0.01);
                    dates.insert(key);
                }
            }
            previous = next;
            // A metadata-only layout revision must not rewind the last wheel
            // step, even when the anchor moved to the next month's first row.
            if frame % 17 == 0 {
                let lib = &mut h.state_mut().0;
                let offset = lib.scroll_offset;
                lib.thumbnails.revision += 1;
                lib.rebuild_rows_at(768.0, 256.0);
                assert!(
                    (lib.scroll_offset - offset).abs() < 0.1,
                    "reflow rewound wheel at frame {frame}"
                );
            }
        }
        assert!(dates.len() >= 2);
        // Once scrolling rests, a background aspect arrival can reflow rows.
        // Its anchor must describe the final offset, not the pre-wheel viewport.
        let lib = &mut h.state_mut().0;
        let (id, relative) = lib.scroll_anchor.unwrap();
        let row=lib.rows.iter().position(|r|matches!(r,GridRow::Photos(r) if r.cells.iter().any(|c|lib.state.photos[c.index].id==id))).unwrap();
        assert!(
            (lib.row_offsets[row] + lib.discovery_height() - lib.scroll_offset - relative).abs()
                < 0.1,
            "anchor is one wheel step behind final scroll offset"
        );
    }

    /// Real GPU-rendered captures, never personal photos. Run explicitly with
    /// RPHOTOS_CAPTURE_DIR=/tmp/opencode/... cargo test capture_library -- --ignored.
    #[test]
    #[ignore = "requires an offscreen wgpu adapter and RPHOTOS_CAPTURE_DIR"]
    fn capture_library() {
        let output = PathBuf::from(
            std::env::var_os("RPHOTOS_CAPTURE_DIR").expect("set RPHOTOS_CAPTURE_DIR"),
        );
        std::fs::create_dir_all(&output).unwrap();
        let (temp, _source, database) = fixture();
        {
            let mut db = store::connect(&database).unwrap();
            let photos = store::snapshot(&db).unwrap().photos;
            store::create_album_with_photos(
                &mut db,
                "Осенние прогулки",
                &photos.iter().take(8).map(|p| p.id).collect::<Vec<_>>(),
            )
            .unwrap();
            db.execute(
                "INSERT INTO saved_queries(name,query) VALUES ('Осенние снимки','Фото')",
                [],
            )
            .unwrap();
        }
        for (name, size, scale, language, appearance) in [
            (
                "cache-errors",
                [1000.0, 760.0],
                1.0,
                Language::Ru,
                Appearance::Light,
            ),
            (
                "collection-cache-all",
                [1500.0, 960.0],
                1.0,
                Language::Ru,
                Appearance::Light,
            ),
            (
                "collection-scan-progress",
                [1500.0, 960.0],
                1.0,
                Language::Ru,
                Appearance::Light,
            ),
            (
                "collection-continuous-zoom",
                [1500.0, 960.0],
                1.0,
                Language::Ru,
                Appearance::Light,
            ),
            (
                "collection-menu",
                [1500.0, 960.0],
                1.0,
                Language::Ru,
                Appearance::Light,
            ),
            (
                "collection-heart",
                [1500.0, 960.0],
                1.0,
                Language::Ru,
                Appearance::Light,
            ),
            (
                "favorites",
                [1500.0, 960.0],
                1.0,
                Language::Ru,
                Appearance::Light,
            ),
            (
                "collection-light",
                [1440.0, 900.0],
                1.0,
                Language::Ru,
                Appearance::Light,
            ),
            (
                "collection-narrow",
                [640.0, 760.0],
                1.0,
                Language::En,
                Appearance::Light,
            ),
            (
                "collection-large",
                [1500.0, 960.0],
                1.0,
                Language::Ru,
                Appearance::Light,
            ),
            (
                "collection-small-months",
                [1500.0, 960.0],
                1.0,
                Language::En,
                Appearance::Light,
            ),
            (
                "collection-month-chooser",
                [1500.0, 960.0],
                1.0,
                Language::Ru,
                Appearance::Light,
            ),
            (
                "collection-selection",
                [1500.0, 960.0],
                1.0,
                Language::Ru,
                Appearance::Light,
            ),
            (
                "collection-narrow-selection",
                [500.0, 700.0],
                1.0,
                Language::En,
                Appearance::Light,
            ),
            (
                "collection-dark-hidpi",
                [900.0, 650.0],
                2.0,
                Language::Ru,
                Appearance::Dark,
            ),
            (
                "albums",
                [1500.0, 960.0],
                1.0,
                Language::Ru,
                Appearance::Light,
            ),
            (
                "album-detail",
                [1500.0, 960.0],
                1.0,
                Language::Ru,
                Appearance::Light,
            ),
            (
                "album-picker",
                [700.0, 800.0],
                1.0,
                Language::En,
                Appearance::Light,
            ),
            (
                "folders",
                [1500.0, 960.0],
                1.0,
                Language::Ru,
                Appearance::Light,
            ),
            (
                "folder-empty",
                [1500.0, 960.0],
                1.0,
                Language::Ru,
                Appearance::Light,
            ),
            (
                "add-to-album",
                [1500.0, 960.0],
                1.0,
                Language::Ru,
                Appearance::Light,
            ),
            (
                "collection-search",
                [1500.0, 960.0],
                1.0,
                Language::Ru,
                Appearance::Light,
            ),
            (
                "collection-semantic",
                [1500.0, 960.0],
                1.0,
                Language::Ru,
                Appearance::Light,
            ),
            (
                "section-swipe",
                [900.0, 760.0],
                1.0,
                Language::Ru,
                Appearance::Light,
            ),
            (
                "preferences",
                [1100.0, 900.0],
                1.0,
                Language::Ru,
                Appearance::Light,
            ),
            (
                "collection-reveal",
                [1500.0, 960.0],
                1.0,
                Language::Ru,
                Appearance::Light,
            ),
            (
                "preferences-reveal",
                [1100.0, 900.0],
                1.0,
                Language::Ru,
                Appearance::Light,
            ),
            (
                "collection-tabs-reveal",
                [1500.0, 960.0],
                1.0,
                Language::Ru,
                Appearance::Light,
            ),
            (
                "collection-search-reveal",
                [1500.0, 960.0],
                1.0,
                Language::Ru,
                Appearance::Light,
            ),
            (
                "collection-date-reveal",
                [1500.0, 960.0],
                1.0,
                Language::Ru,
                Appearance::Light,
            ),
            (
                "preferences-heading-reveal",
                [1100.0, 900.0],
                1.0,
                Language::Ru,
                Appearance::Light,
            ),
            (
                "month-blank-reveal",
                [900.0, 760.0],
                1.0,
                Language::Ru,
                Appearance::Dark,
            ),
            (
                "month-menu-reveal",
                [1500.0, 960.0],
                1.0,
                Language::Ru,
                Appearance::Dark,
            ),
            (
                "menu-separator-reveal",
                [1500.0, 960.0],
                1.0,
                Language::Ru,
                Appearance::Dark,
            ),
            (
                "preferences-radio-reveal",
                [1100.0, 900.0],
                1.0,
                Language::Ru,
                Appearance::Dark,
            ),
        ] {
            let ctx = egui::Context::default();
            let library = Library::with_paths(&ctx, database.clone(), temp.path().join("cache"));
            let settings = AppSettings {
                language,
                appearance,
                ..Default::default()
            };
            let mut harness = egui_kittest::Harness::builder()
                .with_size(egui::vec2(size[0], size[1]))
                .with_pixels_per_point(scale)
                .wgpu()
                .build_state(
                    |ctx, (library, settings)| {
                        library.poll(ctx);
                        library.show(ctx, settings);
                    },
                    (library, settings),
                );
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while std::time::Instant::now() < deadline {
                harness.run_steps(2);
                let library = &harness.state().0;
                if library.state.photos.len() == 18 && library.thumbnails.pending_count() == 0 {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            harness.run_steps(4);
            assert_eq!(harness.state().0.state.photos.len(), 18);
            match name {
                "albums" => harness.state_mut().0.section = Section::Albums,
                "favorites" => {
                    let lib = &mut harness.state_mut().0;
                    lib.state.favorites.insert(lib.state.photos[0].id);
                    lib.section = Section::Favorites;
                    lib.layout_key = None;
                }
                "album-detail" => {
                    let id = harness.state().0.state.albums[0].id;
                    harness.state_mut().0.enter_album(id);
                }
                "album-picker" => harness.state_mut().0.start_picker(None),
                "folders" => harness.state_mut().0.section = Section::Folders,
                "add-to-album" => {
                    let lib = &mut harness.state_mut().0;
                    lib.selection.insert(lib.state.photos[0].id);
                    lib.selecting = true;
                    lib.assign_album_open = true;
                }
                "folder-empty" => {
                    let lib = &mut harness.state_mut().0;
                    let folder = lib.state.sources[0].path.join("Empty folder");
                    lib.state.directories.push(folder.join("Nested empty"));
                    lib.section = Section::Folders;
                    lib.folder = Some(folder);
                }
                "preferences"
                | "preferences-reveal"
                | "preferences-heading-reveal"
                | "preferences-radio-reveal" => harness.state_mut().0.settings_open = true,
                "collection-search" => harness.state_mut().0.discovery.search_open = true,
                "collection-semantic" => {
                    let lib = &mut harness.state_mut().0;
                    lib.query = "Природа и отдых".into();
                    lib.content_search.query = lib.query.clone();
                    lib.content_search.hits =
                        Some(lib.state.photos.iter().take(3).map(|p| p.id).collect());
                    lib.layout_key = None;
                }
                _ => {}
            }
            if name.ends_with("selection") {
                harness.state_mut().0.selecting = true;
                let id = harness.state().0.state.photos[0].id;
                harness.state_mut().0.selection.insert(id);
                harness.run_steps(4);
            }
            if name == "collection-continuous-zoom" {
                harness.state_mut().1.thumbnail_size = 177.0;
            }
            if name == "collection-scan-progress" {
                let lib = &mut harness.state_mut().0;
                lib.scanning = true;
                lib.worker
                    .total
                    .store(30323, std::sync::atomic::Ordering::Relaxed);
                lib.worker
                    .progress
                    .store(12000, std::sync::atomic::Ordering::Relaxed);
            }
            if name == "collection-cache-all" {
                harness.state_mut().1.thumbnail_disk_gib = 0;
                let lib = &mut harness.state_mut().0;
                lib.thumbnails.disk_quota(0);
                lib.thumbnails.build_all(&lib.state.photos);
            }
            if name == "collection-large" {
                harness.state_mut().1.thumbnail_density = 0;
            }
            if matches!(
                name,
                "collection-small-months"
                    | "collection-month-chooser"
                    | "month-blank-reveal"
                    | "month-menu-reveal"
            ) {
                for (i, p) in harness.state_mut().0.state.photos.iter_mut().enumerate() {
                    p.modified = chrono::NaiveDate::from_ymd_opt(2026, 10 - i as u32 / 6, 15)
                        .unwrap()
                        .and_hms_opt(12, 0, 0)
                        .unwrap()
                        .and_utc()
                        .timestamp();
                }
                harness.state_mut().0.layout_key = None;
                harness.state_mut().1.thumbnail_density = 2;
                harness.state_mut().0.semantic_zoom = name != "collection-small-months";
            }
            harness.run_steps(4);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                harness.run_steps(3);
                if harness.state().0.thumbnails.pending_count() == 0 {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "thumbnail capture did not settle"
                );
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            if name == "section-swipe" {
                for (phase, x) in [
                    (egui::TouchPhase::Start, 700.0),
                    (egui::TouchPhase::Move, 330.0),
                ] {
                    harness.input_mut().events.push(egui::Event::Touch {
                        device_id: egui::TouchDeviceId(1),
                        id: egui::TouchId(1),
                        phase,
                        pos: egui::pos2(x, 300.0),
                        force: None,
                    });
                    harness.run_steps(1);
                }
                assert_eq!(harness.state().0.pager.adjacent, Some(1));
            }
            if matches!(
                name,
                "collection-menu" | "month-menu-reveal" | "menu-separator-reveal"
            ) {
                use egui_kittest::kittest::Queryable;
                harness.get_by_label("…").click();
                harness.run_steps(3);
            }
            if name == "cache-errors" {
                let errors = vec![
                    thumbnail_jobs::Failure {
                        path: PathBuf::from("/synthetic/empty.jpg"),
                        reason: "Original file is empty (0 bytes); no image data to decode".into(),
                    },
                    thumbnail_jobs::Failure {
                        path: PathBuf::from("/synthetic/without-preview.dng"),
                        reason: "Cannot decode original image: RAW file without an embedded JPEG"
                            .into(),
                    },
                ];
                let cache = harness.state().0.thumbnails.directory().to_owned();
                std::fs::write(
                    cache.join("last-build-errors.json"),
                    serde_json::to_vec(&errors).unwrap(),
                )
                .unwrap();
                // A fresh worker loads the saved report, as on application restart.
                let thumbnails = thumbnails::Thumbnails::new(cache, &harness.ctx);
                harness.state_mut().0.thumbnails = thumbnails;
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                while harness.state().0.thumbnails.failures().is_empty() {
                    assert!(std::time::Instant::now() < deadline);
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                harness.state_mut().0.cache_errors_open = true;
                harness.run_steps(4);
            }
            if name == "collection-heart" {
                use egui_kittest::kittest::Queryable;
                let rect = harness.get_by_label("Фото-00.png").raw_bounds().unwrap();
                harness
                    .input_mut()
                    .events
                    .push(egui::Event::PointerMoved(egui::pos2(
                        rect.x1 as f32 - 20.0,
                        rect.y0 as f32 + 20.0,
                    )));
                harness.run_steps(3);
            }
            if name == "collection-reveal" || name == "preferences-reveal" {
                use egui_kittest::kittest::Queryable;
                let label = if name == "collection-reveal" {
                    "Фото-00.png"
                } else {
                    "Добавить папку"
                };
                let rect = harness.get_by_label(label).raw_bounds().unwrap();
                harness
                    .input_mut()
                    .events
                    .push(egui::Event::PointerMoved(egui::pos2(
                        rect.x0 as f32 + 20.0,
                        rect.y0 as f32 + 20.0,
                    )));
                harness.run_steps(3);
            }
            if matches!(
                name,
                "collection-tabs-reveal"
                    | "collection-search-reveal"
                    | "preferences-heading-reveal"
                    | "collection-date-reveal"
            ) {
                use egui_kittest::kittest::Queryable;
                let label = match name {
                    "collection-tabs-reveal" => "Альбомы",
                    "collection-search-reveal" => "Поиск по имени или пути",
                    "preferences-heading-reveal" => "Источники",
                    _ => "Текущая дата: Октябрь 2026",
                };
                let rect = harness.get_by_label(label).raw_bounds().unwrap();
                harness
                    .input_mut()
                    .events
                    .push(egui::Event::PointerMoved(egui::pos2(
                        (rect.x0 + rect.x1) as f32 / 2.0,
                        (rect.y0 + rect.y1) as f32 / 2.0,
                    )));
                harness.run_steps(3);
            }
            if name == "month-blank-reveal" {
                use egui_kittest::kittest::Queryable;
                let r = harness.get_by_label("Октябрь 2026").raw_bounds().unwrap();
                harness
                    .input_mut()
                    .events
                    .push(egui::Event::PointerMoved(egui::pos2(
                        740.0,
                        (r.y0 + r.y1) as f32 / 2.0,
                    )));
                harness.run_steps(2);
            }
            if name == "menu-separator-reveal" {
                use egui_kittest::kittest::Queryable;
                let a = harness.get_by_label("Настройки").raw_bounds().unwrap();
                let b = harness
                    .get_by_label("Прогрузить все миниатюры")
                    .raw_bounds()
                    .unwrap();
                harness
                    .input_mut()
                    .events
                    .push(egui::Event::PointerMoved(egui::pos2(
                        (a.x0 + a.x1) as f32 / 2.0,
                        (a.y1 + b.y0) as f32 / 2.0,
                    )));
                harness.run_steps(2);
            }
            if name == "preferences-radio-reveal" {
                use egui_kittest::kittest::Queryable;
                let r = harness
                    .get_by_label("Следующее / предыдущее изображение")
                    .raw_bounds()
                    .unwrap();
                harness
                    .input_mut()
                    .events
                    .push(egui::Event::PointerMoved(egui::pos2(
                        r.x0 as f32 + 6.0,
                        (r.y0 + r.y1) as f32 / 2.0,
                    )));
                harness.run_steps(2);
            }
            let image = harness.render().expect("offscreen render failed");
            image.save(output.join(format!("{name}.png"))).unwrap();
        }
    }

    #[test]
    fn rows_filter_album_and_folder_and_keep_open_order() {
        let ctx = egui::Context::default();
        // Test only the pure layout state, not filesystem workers.
        let temp = tempfile::tempdir().unwrap();
        let mut library = Library {
            timeline_marker: None,
            timeline_date: None,
            watcher: watcher::Watch::new(|| {}),
            watch_initial_refresh: true,
            watch_refresh_pending: false,
            focused_photo: None,
            focused_photo_widget: None,
            content_search: Default::default(),
            discovery: Default::default(),
            ready: true,
            pager: Default::default(),
            catalog: Default::default(),
            worker: Worker::new(temp.path().join("db/index"), || {}),
            state: Snapshot {
                photos: vec![
                    store::Photo {
                        id: 1,
                        path: "/photos/Кот.png".into(),
                        modified: 0,
                        taken: None,
                        key: "a".into(),
                        albums: vec![9],
                        text: "Распознанная надпись".into(),
                    },
                    store::Photo {
                        id: 2,
                        path: "/other/dog.png".into(),
                        modified: 0,
                        taken: None,
                        key: "b".into(),
                        albums: vec![],
                        text: String::new(),
                    },
                ],
                ..Default::default()
            },
            thumbnails: thumbnails::Thumbnails::new(temp.path().join("cache"), &ctx),
            section: Section::Collection,
            album: None,
            folder: None,
            query: "КОТ".into(),
            rows: vec![],
            months: vec![],
            semantic_zoom: false,
            scroll_to_month: None,
            scroll_offset: 0.0,
            scroll_anchor: None,
            row_offsets: vec![],
            filtered: vec![],
            layout_key: None,
            selection: HashSet::new(),
            selecting: false,
            popup_open_last_frame: false,
            scanning: false,
            error: None,
            settings_open: false,
            album_name: String::new(),
            create_album_open: false,
            assign_album_open: false,
            cache_errors_open: false,
            remove_source: None,
            picker: None,
            grid_right: 0.0,
            search_rect: None,
        };
        library.rebuild_rows(600.0, 1);
        assert_eq!(library.filtered, [0]);
        assert_eq!(library.row_offsets, [0.0, 66.0, 334.0]);
        assert_eq!(library.open(0).paths, [PathBuf::from("/photos/Кот.png")]);
        library.query.clear();
        library.state.favorites.insert(2);
        library.query = "is:favorite".into();
        library.rebuild_rows(600.0, 1);
        assert_eq!(library.filtered, [1]);
        library.query = "is:favorite dog".into();
        library.rebuild_rows(600.0, 1);
        assert_eq!(library.filtered, [1]);
        library.query = "Питомец на прогулке".into();
        library.content_search.query = library.query.clone();
        library.content_search.hits = Some([2].into_iter().collect());
        library.rebuild_rows(600.0, 1);
        assert_eq!(library.filtered, [1]); // semantic hit, no literal substring
        library.query = "Кот".into();
        library.rebuild_rows(600.0, 1);
        assert_eq!(library.filtered, [0]); // editing a query immediately restores keyword filtering
        library.query.clear();
        library.query = "НАДПИСЬ".into();
        library.rebuild_rows(600.0, 1);
        assert_eq!(library.filtered, [0]);
        library.query.clear();
        library.album = Some(9);
        library.rebuild_rows(600.0, 1);
        assert_eq!(library.filtered, [0]);
        library.album = None;
        library.folder = Some("/photos".into());
        library.rebuild_rows(600.0, 1);
        assert_eq!(library.filtered, [0]);
        library.state.photos.push(store::Photo {
            id: 3,
            path: "/photos/nested/cat.png".into(),
            modified: 0,
            taken: None,
            key: "c".into(),
            albums: vec![],
            text: String::new(),
        });
        library.layout_key = None;
        library.rebuild_rows(600.0, 1);
        assert_eq!(
            library.filtered,
            [0],
            "folder view must not include descendants"
        );
    }

    #[test]
    fn catalog_picker_rename_remove_and_preferences_transitions() {
        use egui_kittest::kittest::Queryable;
        let (temp, _source, database) = fixture();
        let ctx = egui::Context::default();
        let mut lib = Library::with_paths(&ctx, database, temp.path().join("cache"));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while lib.state.photos.len() < 18 {
            lib.poll(&ctx);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        lib.section = Section::Albums;
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1500.0, 960.0))
            .build_state(
                |ctx, (lib, settings)| {
                    lib.poll(ctx);
                    lib.show(ctx, settings);
                },
                (lib, AppSettings::default()),
            );
        h.run_steps(3);
        h.get_by_label("+ Новый альбом").click();
        h.run_steps(3);
        assert!(h.state().0.catalog.picker.is_some());
        h.state_mut().0.catalog.picker.as_mut().unwrap().name = "Test album".into();
        h.get_by_label("Фото-00.png").click();
        h.run_steps(2);
        assert_eq!(
            h.state().0.catalog.picker.as_ref().unwrap().selected.len(),
            1
        );
        h.get_by_label("Добавить").click();
        h.run_steps(2);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while h.state().0.state.albums.is_empty() {
            h.run_steps(2);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let id = h.state().0.state.albums[0].id;
        assert_eq!(
            h.state()
                .0
                .state
                .photos
                .iter()
                .filter(|p| p.albums.contains(&id))
                .count(),
            1
        );
        h.get_by_label("Test album").click();
        h.run_steps(3);
        assert_eq!(h.state().0.album, Some(id));
        h.get_by_label("Test album").click();
        h.run_steps(2);
        assert!(h.state().0.catalog.rename.is_some());
        h.state_mut().0.catalog.rename = Some("Renamed album".into());
        h.get_by_label("Готово").click();
        h.run_steps(2);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while h.state().0.state.albums[0].name != "Renamed album" {
            h.run_steps(2);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        h.state_mut().0.catalog.remove = Some(id);
        h.run_steps(3);
        h.get_by_label("Отмена").click();
        h.run_steps(2);
        assert_eq!(h.state().0.state.albums.len(), 1);
        h.state_mut().0.catalog.remove = Some(id);
        h.run_steps(3);
        // Heading and confirmation can share text: click the button role.
        h.get_by_label("Удалить альбом").click();
        h.run_steps(2);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !h.state().0.state.albums.is_empty() {
            h.run_steps(2);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(h.state().0.album.is_none());
        assert!(h.state().0.state.photos.iter().all(|p| p.path.exists()));
        h.state_mut().0.settings_open = true;
        h.run_steps(3);
        assert!(h.query_by_label("Коллекция").is_none());
        h.get_by_label("Назад").click();
        h.run_steps(2);
        assert!(!h.state().0.settings_open);
    }

    #[test]
    fn add_to_album_modal_has_reference_geometry_and_assigns_without_touching_files() {
        use egui_kittest::kittest::Queryable;
        let (temp, source, database) = fixture();
        let mut db = store::connect(&database).unwrap();
        let id = store::create_album_with_photos(&mut db, "Target album", &[]).unwrap();
        drop(db);
        let ctx = egui::Context::default();
        let lib = Library::with_paths(&ctx, database, temp.path().join("cache"));
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1500.0, 960.0))
            .build_state(
                |ctx, (lib, settings, rect)| {
                    lib.poll(ctx);
                    let _ = lib.show(ctx, settings);
                    *rect = ctx.memory(|m| m.area_rect(egui::Id::new("assign_album")));
                },
                (lib, AppSettings::default(), None::<egui::Rect>),
            );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !h.state().0.ready {
            h.run_steps(2);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let photo = h.state().0.state.photos[0].id;
        h.state_mut().0.selection.insert(photo);
        h.state_mut().0.selecting = true;
        h.state_mut().0.assign_album_open = true;
        h.run_steps(4);
        let rect = h.state().2.unwrap();
        assert!((rect.width() - 362.0).abs() < 2.0, "modal width {rect:?}");
        assert!((rect.height() - 650.0).abs() < 2.0, "modal height {rect:?}");
        assert!((rect.center() - egui::pos2(750.0, 480.0)).length() < 2.0);
        h.get_by_label("Target album").click();
        h.run_steps(2);
        while !h
            .state()
            .0
            .state
            .photos
            .iter()
            .find(|p| p.id == photo)
            .unwrap()
            .albums
            .contains(&id)
        {
            h.run_steps(2);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!h.state().0.assign_album_open);
        assert!(source.join("Фото-00.png").exists());
    }

    #[test]
    fn vertical_touch_drag_scrolls_with_inertia_without_opening_or_switching() {
        let (temp, _, database) = fixture();
        let ctx = egui::Context::default();
        let lib = Library::with_paths(&ctx, database, temp.path().join("cache"));
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(640.0, 760.0))
            .with_step_dt(1.0 / 60.0)
            .build_state(
                |ctx, (lib, settings, opened)| {
                    lib.poll(ctx);
                    if let Some(photo) = lib.show(ctx, settings) {
                        *opened = Some(photo);
                    }
                },
                (lib, AppSettings::default(), None::<OpenPhoto>),
            );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !h.state().0.ready {
            h.run_steps(2);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        for (i, photo) in h.state_mut().0.state.photos.iter_mut().enumerate() {
            photo.modified = chrono::NaiveDate::from_ymd_opt(2026, 10 - i as u32 / 6, 15)
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap()
                .and_utc()
                .timestamp();
        }
        h.state_mut().0.layout_key = None;
        h.run_steps(4);
        // Start shortly before a month boundary; the drag and inertial tail
        // must keep the same final-coordinate anchor through that boundary.
        let boundary = h.state().0.months[1].offset;
        h.state_mut().0.scroll_offset = (boundary - 100.0).max(0.0);
        h.state_mut().0.scroll_anchor = None;
        h.run_steps(2);
        let touch = |phase, pos| egui::Event::Touch {
            device_id: egui::TouchDeviceId(1),
            id: egui::TouchId(1),
            phase,
            pos,
            force: None,
        };
        let start = egui::pos2(320.0, 650.0);
        h.input_mut().events.extend([
            touch(egui::TouchPhase::Start, start),
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        h.run_steps(1);
        for y in [610.0, 560.0, 510.0, 460.0, 410.0] {
            let pos = egui::pos2(320.0, y);
            h.input_mut().events.extend([
                touch(egui::TouchPhase::Move, pos),
                egui::Event::PointerMoved(pos),
            ]);
            h.run_steps(1);
            let lib = &h.state().0;
            if let Some((id, relative)) = lib.scroll_anchor {
                let r=lib.rows.iter().position(|r|matches!(r,GridRow::Photos(r) if r.cells.iter().any(|c|lib.state.photos[c.index].id==id))).unwrap();
                assert!(
                    (lib.row_offsets[r] + lib.discovery_height() - lib.scroll_offset - relative)
                        .abs()
                        < 0.1,
                    "touch anchor lag at month boundary"
                );
            }
        }
        let dragged = h.state().0.scroll_offset;
        assert!(dragged > 100.0, "vertical touch did not scroll: {dragged}");
        assert!(
            dragged > boundary,
            "touch must actually cross a month boundary"
        );
        let end = egui::pos2(320.0, 410.0);
        h.input_mut().events.extend([
            touch(egui::TouchPhase::End, end),
            egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        h.run_steps(4);
        assert!(
            h.state().0.scroll_offset > dragged + 1.0,
            "no kinetic tail after release: {dragged} -> {}, pointer velocity {:?}",
            h.state().0.scroll_offset,
            h.ctx.input(|i| i.pointer.velocity())
        );
        assert!(h.state().0.section == Section::Collection);
        assert!(h.state().2.is_none(), "a drag must not open a thumbnail");
    }

    #[test]
    fn section_touch_pager_locks_axis_and_blocks_photo_open() {
        use egui_kittest::kittest::Queryable;
        let (temp, _, database) = fixture();
        let ctx = egui::Context::default();
        let mut lib = Library::with_paths(&ctx, database, temp.path().join("cache"));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !lib.ready {
            lib.poll(&ctx);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(900.0, 760.0))
            .build_state(
                |ctx, (lib, settings, open)| {
                    lib.poll(ctx);
                    *open = lib.show(ctx, settings);
                },
                (lib, AppSettings::default(), None::<OpenPhoto>),
            );
        h.run_steps(3);
        let touch = |phase, x, y| egui::Event::Touch {
            device_id: egui::TouchDeviceId(1),
            id: egui::TouchId(1),
            phase,
            pos: egui::pos2(x, y),
            force: None,
        };
        h.input_mut()
            .events
            .push(touch(egui::TouchPhase::Start, 700.0, 300.0));
        h.run_steps(1);
        h.input_mut()
            .events
            .push(touch(egui::TouchPhase::Move, 450.0, 304.0));
        h.run_steps(1);
        assert!(h.state().0.pager.offset < -200.0);
        assert!(h.state().2.is_none());
        // Both real scenes are painted while dragging; neighboring controls
        // cannot start a picker or steal the gesture.
        let adjacent = h.get_by_label("+ Новый альбом");
        assert!(adjacent.is_disabled());
        let rect = adjacent.raw_bounds().unwrap();
        assert!(rect.x0 >= 640.0 && rect.x0 < 900.0, "{rect:?}");
        let filtered = h.state().0.filtered.clone();
        assert_eq!(filtered.len(), 18, "preview must not replace grid state");
        h.input_mut()
            .events
            .push(touch(egui::TouchPhase::End, 450.0, 304.0));
        h.run_steps(2);
        assert!(h.state().0.section == Section::Albums);
        assert!(h.state().0.pager.offset > 0.0);
        h.run_steps(45);
        h.input_mut().events.extend([
            touch(egui::TouchPhase::Start, 700.0, 300.0),
            touch(egui::TouchPhase::Move, 690.0, 500.0),
            touch(egui::TouchPhase::End, 400.0, 510.0),
        ]);
        h.run_steps(2);
        assert!(h.state().0.section == Section::Albums);
        for (phase, x) in [
            (egui::TouchPhase::Start, 700.0),
            (egui::TouchPhase::Move, 400.0),
            (egui::TouchPhase::End, 400.0),
        ] {
            h.input_mut().events.push(touch(phase, x, 300.0));
            h.run_steps(1);
        }
        assert!(h.state().0.section == Section::Folders);
        h.run_steps(45);
        for (phase, x) in [
            (egui::TouchPhase::Start, 700.0),
            (egui::TouchPhase::Move, 400.0),
            (egui::TouchPhase::End, 400.0),
        ] {
            h.input_mut().events.push(touch(phase, x, 300.0));
            h.run_steps(1);
        }
        assert!(h.state().0.section == Section::Favorites);
        h.run_steps(45);
        for (phase, x) in [
            (egui::TouchPhase::Start, 700.0),
            (egui::TouchPhase::Move, 400.0),
            (egui::TouchPhase::End, 400.0),
        ] {
            h.input_mut().events.push(touch(phase, x, 300.0));
            h.run_steps(1);
        }
        assert!(
            h.state().0.section == Section::Favorites,
            "no wrap at last tab"
        );
        assert!(h.state().0.pager.adjacent.is_none());
        assert!(h.state().2.is_none());
    }
}
#[test]
fn scene_storage_round_trip_without_selection_or_transient_dialogs() {
    #[derive(Default)]
    struct Storage(std::collections::HashMap<String, String>);
    impl eframe::Storage for Storage {
        fn get_string(&self, k: &str) -> Option<String> {
            self.0.get(k).cloned()
        }
        fn set_string(&mut self, k: &str, v: String) {
            self.0.insert(k.into(), v);
        }
        fn flush(&mut self) {}
    }
    let temp = tempfile::tempdir().unwrap();
    let ctx = egui::Context::default();
    let mut lib = Library::with_paths(&ctx, temp.path().join("data/db"), temp.path().join("cache"));
    lib.section = Section::Folders;
    lib.folder = Some("/photos/nested".into());
    lib.query = "кот".into();
    lib.scroll_offset = 432.0;
    lib.discovery.hidden = true;
    lib.selection.insert(99);
    lib.selecting = true;
    lib.error = Some("transient".into());
    let mut storage = Storage::default();
    lib.save_scene(&mut storage);
    let mut restored = Library::with_paths(
        &ctx,
        temp.path().join("other/db"),
        temp.path().join("other/cache"),
    );
    restored.restore_scene(Some(&storage));
    assert!(restored.section == Section::Folders);
    assert_eq!(restored.folder, lib.folder);
    assert_eq!(restored.query, "кот");
    assert_eq!(restored.scroll_offset, 432.0);
    assert!(restored.discovery.hidden);
    assert!(restored.selection.is_empty());
    assert!(!restored.selecting);
    assert!(restored.error.is_none());
}
