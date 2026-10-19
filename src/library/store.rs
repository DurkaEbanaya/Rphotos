//! Persistent library data. This module never decodes or changes originals.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};

// Materialize the photo key set once. A correlated EXISTS can rescan photos
// for every aspect on SQLite versions without a transient covering index.
const ASPECTS_QUERY: &str = "SELECT cache_key,aspect FROM thumbnail_aspects WHERE cache_key IN (SELECT cache_key FROM photos)";

#[derive(Clone, Debug)]
pub struct Photo {
    pub id: i64,
    pub path: PathBuf,
    pub modified: i64,
    pub taken: Option<i64>,
    pub key: String,
    pub albums: Vec<i64>,
    pub text: String,
}

impl Photo {
    pub fn date(&self) -> i64 {
        self.taken.unwrap_or(self.modified)
    }
}

#[derive(Clone, Debug)]
pub struct Source {
    pub id: i64,
    pub path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct Album {
    pub id: i64,
    pub name: String,
    pub cover: Option<i64>,
    pub modified: i64,
    pub automatic: bool,
}

#[derive(Default, Debug)]
pub struct Snapshot {
    pub unavailable: HashSet<String>,
    pub directories: Vec<PathBuf>,
    pub favorites: HashSet<i64>,
    pub aspects: std::collections::HashMap<String, f32>,
    pub sources: Vec<Source>,
    pub photos: Vec<Photo>,
    pub albums: Vec<Album>,
    pub searches: Vec<SavedQuery>,
}

#[derive(Clone, Debug)]
pub struct SavedQuery {
    pub id: i64,
    pub name: String,
    pub query: String,
}

impl Snapshot {
    /// Immediate children only; the persisted tree also includes empty folders.
    /// Derive missing ancestors from photos while an older index awaits its scan.
    pub fn child_folders(
        &self,
        current: &Path,
    ) -> std::collections::BTreeMap<PathBuf, Option<usize>> {
        let mut children = std::collections::BTreeMap::new();
        for path in &self.directories {
            if path.parent() == Some(current) {
                children.insert(path.clone(), None);
            }
        }
        for (index, photo) in self.photos.iter().enumerate() {
            if let Ok(relative) = photo.path.strip_prefix(current) {
                let mut parts = relative.components();
                if let (Some(first), Some(_)) = (parts.next(), parts.next()) {
                    let cover = children.entry(current.join(first)).or_insert(None);
                    if cover.is_none() {
                        *cover = Some(index);
                    }
                }
            }
        }
        children
    }
}

pub fn connect(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    let db = Connection::open(path)?;
    db.busy_timeout(std::time::Duration::from_secs(3))?;
    let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version > 10 {
        bail!("Library database was created by a newer Rphotos version");
    }
    db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;")?;
    // Opening a migrated database must not start a schema write transaction:
    // readers and the owner worker can connect concurrently (SQLITE_BUSY_SNAPSHOT
    // cannot be repaired by busy_timeout after upgrading a deferred transaction).
    if version == 10 {
        return Ok(db);
    }
    db.execute_batch(
        "BEGIN IMMEDIATE;
         CREATE TABLE IF NOT EXISTS sources(id INTEGER PRIMARY KEY, path BLOB NOT NULL UNIQUE);
         CREATE TABLE IF NOT EXISTS photos(
             id INTEGER PRIMARY KEY, path BLOB NOT NULL UNIQUE,
             modified INTEGER NOT NULL, cache_key TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS source_photos(
             source_id INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
             photo_id INTEGER NOT NULL REFERENCES photos(id) ON DELETE CASCADE,
             PRIMARY KEY(source_id, photo_id));
         CREATE TABLE IF NOT EXISTS albums(id INTEGER PRIMARY KEY, name TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS album_photos(
             album_id INTEGER NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
             photo_id INTEGER NOT NULL REFERENCES photos(id) ON DELETE CASCADE,
              PRIMARY KEY(album_id, photo_id));
          CREATE INDEX IF NOT EXISTS source_photos_by_photo ON source_photos(photo_id);
          CREATE INDEX IF NOT EXISTS album_photos_by_photo ON album_photos(photo_id);
          COMMIT;",
    )?;
    if version < 2 {
        db.execute_batch("BEGIN; ALTER TABLE albums ADD COLUMN cover_id INTEGER; ALTER TABLE albums ADD COLUMN modified INTEGER NOT NULL DEFAULT 0; PRAGMA user_version=2; COMMIT;")?;
    }
    if version < 3 {
        db.execute_batch("BEGIN; ALTER TABLE albums ADD COLUMN auto_key TEXT; CREATE UNIQUE INDEX albums_auto_key ON albums(auto_key); CREATE TABLE saved_queries(id INTEGER PRIMARY KEY,name TEXT NOT NULL,query TEXT NOT NULL); PRAGMA user_version=3; COMMIT;")?;
    }
    db.execute_batch(
        "BEGIN;
         CREATE TABLE IF NOT EXISTS photo_text(photo_id INTEGER PRIMARY KEY REFERENCES photos(id) ON DELETE CASCADE,cache_key TEXT NOT NULL,text TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS photo_descriptions(photo_id INTEGER PRIMARY KEY REFERENCES photos(id) ON DELETE CASCADE,cache_key TEXT NOT NULL,model TEXT NOT NULL,text TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS thumbnail_aspects(cache_key TEXT PRIMARY KEY,aspect REAL NOT NULL);
         CREATE TABLE IF NOT EXISTS favorites(photo_id INTEGER PRIMARY KEY REFERENCES photos(id) ON DELETE CASCADE);
         CREATE TABLE IF NOT EXISTS photo_embeddings(photo_id INTEGER PRIMARY KEY REFERENCES photos(id) ON DELETE CASCADE,cache_key TEXT NOT NULL,model TEXT NOT NULL,text_hash TEXT NOT NULL,vector BLOB NOT NULL);
         CREATE TABLE IF NOT EXISTS source_directories(source_id INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,path BLOB NOT NULL,PRIMARY KEY(source_id,path));
         CREATE TABLE IF NOT EXISTS photo_dates(photo_id INTEGER PRIMARY KEY REFERENCES photos(id) ON DELETE CASCADE,cache_key TEXT NOT NULL,taken INTEGER);
         PRAGMA user_version=10; COMMIT;",
    )?;
    Ok(db)
}

// Native paths are BLOBs, not lossy Unicode strings. Databases are local to the OS.
#[cfg(unix)]
fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(unix)]
fn bytes_path(bytes: Vec<u8>) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    std::ffi::OsString::from_vec(bytes).into()
}

#[cfg(windows)]
fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(windows)]
fn bytes_path(bytes: Vec<u8>) -> PathBuf {
    use std::os::windows::ffi::OsStringExt;
    let wide: Vec<_> = bytes
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    std::ffi::OsString::from_wide(&wide).into()
}

pub fn add_source(db: &Connection, path: &Path) -> Result<()> {
    let path = path.canonicalize().context("Cannot open source folder")?;
    if !path.is_dir() {
        bail!("Source is not a folder");
    }
    db.execute(
        "INSERT OR IGNORE INTO sources(path) VALUES (?)",
        [path_bytes(&path)],
    )?;
    Ok(())
}

pub fn remove_source(db: &mut Connection, id: i64) -> Result<()> {
    let tx = db.transaction()?;
    tx.execute("DELETE FROM sources WHERE id=?", [id])?;
    // Keep album membership for temporarily disconnected sources. Only the index
    // association is removed; no original or thumbnail file is ever deleted.
    tx.commit()?;
    Ok(())
}

pub fn create_album(db: &Connection, name: &str) -> Result<i64> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 120 {
        bail!("Album name must contain 1–120 characters");
    }
    db.execute(
        "INSERT INTO albums(name,modified) VALUES (?,?)",
        params![name, chrono::Utc::now().timestamp()],
    )?;
    Ok(db.last_insert_rowid())
}

pub fn rename_album(db: &Connection, id: i64, name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 120 {
        bail!("Album name must contain 1–120 characters");
    }
    db.execute(
        "UPDATE albums SET name=?,modified=?,auto_key=NULL WHERE id=?",
        params![name, chrono::Utc::now().timestamp(), id],
    )?;
    Ok(())
}

pub fn remove_album(db: &Connection, id: i64) -> Result<()> {
    db.execute("DELETE FROM albums WHERE id=?", [id])?;
    Ok(())
}

pub fn cover_album(db: &Connection, id: i64, photo: i64) -> Result<()> {
    if !db.query_row(
        "SELECT EXISTS(SELECT 1 FROM album_photos WHERE album_id=? AND photo_id=?)",
        params![id, photo],
        |r| r.get::<_, bool>(0),
    )? {
        bail!("Cover must be a member of the album");
    }
    db.execute(
        "UPDATE albums SET cover_id=?,modified=?,auto_key=NULL WHERE id=?",
        params![photo, chrono::Utc::now().timestamp(), id],
    )?;
    Ok(())
}

pub fn unassign_album(db: &mut Connection, album: i64, photos: &[i64]) -> Result<()> {
    let tx = db.transaction()?;
    for photo in photos {
        tx.execute(
            "DELETE FROM album_photos WHERE album_id=? AND photo_id=?",
            params![album, photo],
        )?;
    }
    tx.execute("UPDATE albums SET cover_id=CASE WHEN EXISTS(SELECT 1 FROM album_photos WHERE album_id=? AND photo_id=cover_id) THEN cover_id ELSE NULL END,modified=?,auto_key=NULL WHERE id=?",params![album,chrono::Utc::now().timestamp(),album])?;
    tx.commit()?;
    Ok(())
}

pub fn create_album_with_photos(db: &mut Connection, name: &str, photos: &[i64]) -> Result<i64> {
    let tx = db.transaction()?;
    let id = create_album(&tx, name)?;
    for photo in photos {
        tx.execute(
            "INSERT OR IGNORE INTO album_photos VALUES(?,?)",
            params![id, photo],
        )?;
    }
    tx.commit()?;
    Ok(id)
}

pub fn assign_album(db: &mut Connection, album: i64, photos: &[i64]) -> Result<()> {
    let tx = db.transaction()?;
    for photo in photos {
        tx.execute(
            "INSERT OR IGNORE INTO album_photos VALUES (?, ?)",
            params![album, photo],
        )?;
    }
    tx.execute(
        "UPDATE albums SET modified=?,auto_key=NULL WHERE id=?",
        params![chrono::Utc::now().timestamp(), album],
    )?;
    tx.commit()?;
    Ok(())
}

pub fn snapshot(db: &Connection) -> Result<Snapshot> {
    let sources = db
        .prepare("SELECT id,path FROM sources ORDER BY id")?
        .query_map([], |r| {
            Ok(Source {
                id: r.get(0)?,
                path: bytes_path(r.get(1)?),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let albums = db
        .prepare(
            "SELECT id,name,cover_id,modified,auto_key FROM albums ORDER BY modified DESC,id DESC",
        )?
        .query_map([], |r| {
            Ok(Album {
                id: r.get(0)?,
                name: r.get(1)?,
                cover: r.get(2)?,
                modified: r.get(3)?,
                automatic: r.get::<_, Option<String>>(4)?.is_some(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut photos = db
        .prepare(
            "SELECT id,path,modified,cache_key FROM photos
         WHERE EXISTS(SELECT 1 FROM source_photos WHERE photo_id=photos.id)
         ORDER BY modified DESC, path ASC",
        )?
        .query_map([], |r| {
            Ok(Photo {
                id: r.get(0)?,
                path: bytes_path(r.get(1)?),
                modified: r.get(2)?,
                taken: None,
                key: r.get(3)?,
                albums: Vec::new(),
                text: String::new(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let dates: std::collections::HashMap<i64, i64> = db.prepare("SELECT photo_id,taken FROM photo_dates JOIN photos ON photos.id=photo_dates.photo_id WHERE photos.cache_key=photo_dates.cache_key AND taken IS NOT NULL")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    for photo in &mut photos {
        photo.taken = dates.get(&photo.id).copied();
    }
    photos.sort_by(|a, b| b.date().cmp(&a.date()).then_with(|| a.path.cmp(&b.path)));
    let positions: std::collections::HashMap<_, _> =
        photos.iter().enumerate().map(|(i, p)| (p.id, i)).collect();
    for row in db
        .prepare("SELECT photo_id,album_id FROM album_photos")?
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?
    {
        let (photo, album) = row?;
        if let Some(&i) = positions.get(&photo) {
            photos[i].albums.push(album);
        }
    }
    for row in db.prepare("SELECT photo_id,text FROM photo_text JOIN photos ON photos.id=photo_text.photo_id WHERE photos.cache_key=photo_text.cache_key")?.query_map([],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?)))? {
        let (id,text)=row?;if let Some(&i)=positions.get(&id){photos[i].text=text;}
    }
    for row in db.prepare("SELECT photo_id,text FROM photo_descriptions JOIN photos ON photos.id=photo_descriptions.photo_id WHERE photos.cache_key=photo_descriptions.cache_key")?.query_map([],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?)))? {
        let (id,text)=row?;if let Some(&i)=positions.get(&id){photos[i].text.push('\n');photos[i].text.push_str(&text);}
    }
    Ok(Snapshot {
        unavailable: HashSet::new(),
        directories: db
            .prepare("SELECT DISTINCT path FROM source_directories ORDER BY path")?
            .query_map([], |r| Ok(bytes_path(r.get(0)?)))?
            .collect::<rusqlite::Result<_>>()?,
        favorites: db
            .prepare("SELECT photo_id FROM favorites")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?,
        aspects: db
            .prepare(ASPECTS_QUERY)?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?,
        sources,
        photos,
        albums,
        searches: db
            .prepare("SELECT id,name,query FROM saved_queries ORDER BY name,id")?
            .query_map([], |r| {
                Ok(SavedQuery {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    query: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?,
    })
}

pub fn save_aspects(db: &mut Connection, aspects: &[(String, f32)]) -> Result<()> {
    let tx = db.transaction()?;
    for (key, aspect) in aspects {
        if key.len() == 64
            && key.bytes().all(|b| b.is_ascii_hexdigit())
            && aspect.is_finite()
            && *aspect > 0.0
        {
            tx.execute("INSERT INTO thumbnail_aspects VALUES (?,?) ON CONFLICT(cache_key) DO UPDATE SET aspect=excluded.aspect", params![key, aspect])?;
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn toggle_favorite(db: &Connection, id: i64) -> Result<()> {
    if db.execute("DELETE FROM favorites WHERE photo_id=?", [id])? == 0 {
        db.execute("INSERT INTO favorites(photo_id) VALUES (?)", [id])?;
    }
    Ok(())
}

pub fn save_query(db: &Connection, name: &str, query: &str) -> Result<()> {
    if name.trim().is_empty()
        || name.chars().count() > 120
        || query.trim().is_empty()
        || query.len() > 4096
    {
        bail!("Invalid saved search");
    }
    db.execute(
        "INSERT INTO saved_queries(name,query) VALUES(?,?)",
        params![name.trim(), query.trim()],
    )?;
    Ok(())
}
pub fn remove_query(db: &Connection, id: i64) -> Result<()> {
    db.execute("DELETE FROM saved_queries WHERE id=?", [id])?;
    Ok(())
}

/// Explicit local event grouping by capture date (file modification fallback).
/// Rebuilding touches only automatic albums. Renaming one turns it into manual.
pub fn auto_albums(db: &mut Connection) -> Result<()> {
    let photos = snapshot(db)?.photos;
    let mut groups: Vec<Vec<&Photo>> = Vec::new();
    for photo in &photos {
        if groups
            .last()
            .and_then(|g| g.last())
            .is_none_or(|previous| previous.date().saturating_sub(photo.date()) > 2 * 86400)
        {
            groups.push(Vec::new());
        }
        groups.last_mut().unwrap().push(photo);
    }
    let tx = db.transaction()?;
    tx.execute("DELETE FROM albums WHERE auto_key LIKE 'event:%'", [])?;
    for group in groups.into_iter().filter(|g| g.len() >= 3) {
        let newest = group.first().unwrap().date();
        let oldest = group.last().unwrap().date();
        let date = chrono::DateTime::from_timestamp(oldest, 0)
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| oldest.to_string());
        let end = chrono::DateTime::from_timestamp(newest, 0)
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| newest.to_string());
        let name = if date == end {
            date
        } else {
            format!("{date} — {end}")
        };
        tx.execute(
            "INSERT INTO albums(name,modified,auto_key) VALUES(?,?,?)",
            params![name, newest, format!("event:{oldest}:{newest}")],
        )?;
        let id = tx.last_insert_rowid();
        for photo in group {
            tx.execute(
                "INSERT INTO album_photos VALUES(?,?)",
                params![id, photo.id],
            )?;
        }
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
#[test]
fn automatic_events_and_saved_queries_survive_restart() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("data/library");
    let mut db = connect(&path).unwrap();
    db.execute("INSERT INTO sources VALUES(1,?)", [path_bytes(temp.path())])
        .unwrap();
    for id in 1..=7 {
        db.execute(
            "INSERT INTO photos VALUES(?,?,?,?)",
            params![
                id,
                path_bytes(&temp.path().join(format!("{id}.jpg"))),
                if id <= 3 {
                    1000000
                } else if id <= 6 {
                    2000000
                } else {
                    3000000
                },
                id.to_string()
            ],
        )
        .unwrap();
        db.execute("INSERT INTO source_photos VALUES(1,?)", [id])
            .unwrap();
    }
    auto_albums(&mut db).unwrap();
    let state = snapshot(&db).unwrap();
    assert_eq!(state.albums.len(), 2);
    assert!(state.albums.iter().all(|a| a.automatic));
    let preserved = state.albums[0].id;
    rename_album(&db, preserved, "Keep forever").unwrap();
    auto_albums(&mut db).unwrap();
    let state = snapshot(&db).unwrap();
    assert_eq!(state.albums.len(), 3);
    assert!(state
        .albums
        .iter()
        .any(|a| a.id == preserved && !a.automatic));
    save_query(&db, "Кот", "кот").unwrap();
    assert!(save_query(&db, "", "query").is_err());
    drop(db);
    let db = connect(&path).unwrap();
    let state = snapshot(&db).unwrap();
    assert_eq!(state.searches[0].query, "кот");
    remove_query(&db, state.searches[0].id).unwrap();
    assert!(snapshot(&db).unwrap().searches.is_empty());
}

#[cfg(test)]
#[test]
fn ocr_index_invalidates_with_content_key() {
    let temp = tempfile::tempdir().unwrap();
    let db = connect(&temp.path().join("data/db")).unwrap();
    db.execute("INSERT INTO sources VALUES(1,?)", [path_bytes(temp.path())])
        .unwrap();
    db.execute(
        "INSERT INTO photos VALUES(1,?,0,'old')",
        [path_bytes(&temp.path().join("photo.png"))],
    )
    .unwrap();
    db.execute("INSERT INTO source_photos VALUES(1,1)", [])
        .unwrap();
    db.execute("INSERT INTO photo_text VALUES(1,'old','Привет world')", [])
        .unwrap();
    assert_eq!(snapshot(&db).unwrap().photos[0].text, "Привет world");
    db.execute("UPDATE photos SET cache_key='new' WHERE id=1", [])
        .unwrap();
    assert!(snapshot(&db).unwrap().photos[0].text.is_empty());
}

pub fn cache_key(path: &Path, size: u64, modified: SystemTime) -> String {
    let mut hash = Sha256::new();
    hash.update(b"rphotos-thumbnail-v1");
    hash.update(path_bytes(path));
    hash.update(size.to_le_bytes());
    // SystemTime Debug includes nanoseconds and represents dates before the epoch.
    hash.update(format!("{modified:?}"));
    format!("{:x}", hash.finalize())
}

fn sources(db: &Connection) -> Result<Vec<Source>> {
    Ok(db
        .prepare("SELECT id,path FROM sources ORDER BY id")?
        .query_map([], |r| {
            Ok(Source {
                id: r.get(0)?,
                path: bytes_path(r.get(1)?),
            })
        })?
        .collect::<rusqlite::Result<_>>()?)
}

/// Discovery pass: count the same source associations as scan, without decoding
/// or collecting every path in memory. Until this finishes the total is unknown.
pub fn count_images(
    db: &Connection,
    generation: &AtomicU64,
    ticket: u64,
    mut progress: impl FnMut(usize),
) -> Result<usize> {
    let mut count = 0;
    for source in sources(db)? {
        let mut stack = vec![source.path];
        while let Some(dir) = stack.pop() {
            if generation.load(Ordering::Relaxed) != ticket {
                return Ok(count);
            }
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            for entry in entries {
                if generation.load(Ordering::Relaxed) != ticket {
                    return Ok(count);
                }
                let Ok(entry) = entry else {
                    continue;
                };
                if entry.file_name().to_string_lossy().starts_with('.') {
                    continue;
                }
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                if kind.is_symlink() {
                    continue;
                }
                if kind.is_dir() {
                    stack.push(entry.path());
                } else if kind.is_file() && crate::file_io::is_supported_image(&entry.path()) {
                    count += 1;
                    if count % 128 == 0 {
                        progress(count);
                    }
                }
            }
        }
    }
    progress(count);
    Ok(count)
}

/// A cancelled or incomplete traversal never prunes the last successful index.
/// Symlinks and hidden files/directories are intentionally not followed.
pub fn scan(
    db: &mut Connection,
    generation: &AtomicU64,
    ticket: u64,
    mut progress: impl FnMut(usize),
) -> Result<Vec<String>> {
    let sources = sources(db)?;
    let mut warnings = Vec::new();
    let mut count = 0;
    for source in sources {
        let tx = db.transaction()?;
        let mut stack = vec![source.path.clone()];
        let mut seen = HashSet::new();
        let mut seen_directories = HashSet::new();
        let mut complete = true;
        while let Some(dir) = stack.pop() {
            if generation.load(Ordering::Relaxed) != ticket {
                return Ok(warnings);
            }
            let entries = match std::fs::read_dir(&dir) {
                Ok(e) => e,
                Err(e) => {
                    if !matches!(
                        e.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
                    ) && warnings.len() < 20
                    {
                        warnings.push(format!("{}: {e}", dir.display()));
                    }
                    complete = false;
                    continue;
                }
            };
            let bytes = path_bytes(&dir);
            tx.execute(
                "INSERT OR IGNORE INTO source_directories VALUES(?,?)",
                params![source.id, bytes],
            )?;
            seen_directories.insert(bytes);
            for entry in entries {
                if generation.load(Ordering::Relaxed) != ticket {
                    return Ok(warnings);
                }
                let result = (|| -> Result<()> {
                    let entry = entry?;
                    if entry.file_name().to_string_lossy().starts_with('.') {
                        return Ok(());
                    }
                    let kind = entry.file_type()?;
                    if kind.is_symlink() {
                        return Ok(());
                    }
                    let path = entry.path();
                    if kind.is_dir() {
                        stack.push(path);
                        return Ok(());
                    }
                    if !kind.is_file() || !crate::file_io::is_supported_image(&path) {
                        return Ok(());
                    }
                    let metadata = entry.metadata()?;
                    let modified = metadata.modified()?;
                    let seconds = chrono::DateTime::<chrono::Utc>::from(modified).timestamp();
                    let key = cache_key(&path, metadata.len(), modified);
                    tx.execute(
                        "INSERT INTO photos(path,modified,cache_key) VALUES (?,?,?)
                         ON CONFLICT(path) DO UPDATE SET modified=excluded.modified,cache_key=excluded.cache_key",
                        params![path_bytes(&path), seconds, key],
                    )?;
                    let id: i64 = tx.query_row(
                        "SELECT id FROM photos WHERE path=?",
                        [path_bytes(&path)],
                        |r| r.get(0),
                    )?;
                    let dated: bool = tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM photo_dates WHERE photo_id=? AND cache_key=?)",
                        params![id, key],
                        |r| r.get(0),
                    )?;
                    if !dated {
                        let taken = super::facts::taken(&path);
                        // Ignore metadata read across a concurrent file replacement.
                        let current = std::fs::metadata(&path)?;
                        if cache_key(&path, current.len(), current.modified()?) == key {
                            tx.execute("INSERT INTO photo_dates VALUES(?,?,?) ON CONFLICT(photo_id) DO UPDATE SET cache_key=excluded.cache_key,taken=excluded.taken", params![id, key, taken])?;
                        }
                    }
                    tx.execute(
                        "INSERT OR IGNORE INTO source_photos VALUES (?,?)",
                        params![source.id, id],
                    )?;
                    seen.insert(id);
                    count += 1;
                    if count % 128 == 0 {
                        progress(count);
                    }
                    Ok(())
                })();
                if let Err(e) = result {
                    complete = false;
                    let unavailable = e.downcast_ref::<std::io::Error>().is_some_and(|e| {
                        matches!(
                            e.kind(),
                            std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
                        )
                    });
                    if !unavailable && warnings.len() < 20 {
                        warnings.push(format!("{}: {e}", dir.display()));
                    }
                }
            }
        }
        if generation.load(Ordering::Relaxed) != ticket {
            return Ok(warnings);
        }
        if complete {
            let old_directories = tx
                .prepare("SELECT path FROM source_directories WHERE source_id=?")?
                .query_map([source.id], |r| r.get::<_, Vec<u8>>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for path in old_directories {
                if !seen_directories.contains(&path) {
                    tx.execute(
                        "DELETE FROM source_directories WHERE source_id=? AND path=?",
                        params![source.id, path],
                    )?;
                }
            }
            let old = tx
                .prepare("SELECT photo_id FROM source_photos WHERE source_id=?")?
                .query_map([source.id], |r| r.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for id in old {
                if !seen.contains(&id) {
                    tx.execute(
                        "DELETE FROM source_photos WHERE source_id=? AND photo_id=?",
                        params![source.id, id],
                    )?;
                }
            }
        }
        tx.commit()?;
    }
    progress(count);
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    #[test]
    fn discovery_total_matches_scan_and_can_be_cancelled_without_changing_index() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("images");
        std::fs::create_dir(&root).unwrap();
        for n in 0..260 {
            std::fs::write(root.join(format!("{n}.png")), b"synthetic header").unwrap();
        }
        std::fs::write(root.join("video.mp4"), b"ignored").unwrap();
        std::fs::write(root.join(".hidden.jpg"), b"ignored").unwrap();
        let mut db = connect(&temp.path().join("library.sqlite3")).unwrap();
        add_source(&db, &root).unwrap();
        let generation = AtomicU64::new(1);
        let mut found = 0;
        assert_eq!(
            count_images(&db, &generation, 1, |n| found = n).unwrap(),
            260
        );
        assert_eq!(found, 260);
        assert!(snapshot(&db).unwrap().photos.is_empty());
        let mut processed = 0;
        scan(&mut db, &generation, 1, |n| processed = n).unwrap();
        assert_eq!(processed, found);
        let before = snapshot(&db).unwrap().photos.len();
        count_images(&db, &generation, 1, |_| {
            generation.store(2, Ordering::Relaxed);
        })
        .unwrap();
        assert_eq!(snapshot(&db).unwrap().photos.len(), before);
    }

    #[test]
    fn aspect_snapshot_materializes_keys_without_correlated_photo_scan() {
        let temp = tempfile::tempdir().unwrap();
        let db = connect(&temp.path().join("library.sqlite3")).unwrap();
        db.execute(
            "INSERT INTO photos VALUES(1,x'61',0,'shared'),(2,x'62',0,'shared')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO thumbnail_aspects VALUES('shared',1.5),('stale',0.5)",
            [],
        )
        .unwrap();
        let state = snapshot(&db).unwrap();
        assert_eq!(state.aspects.len(), 1);
        assert_eq!(state.aspects["shared"], 1.5);
        let details = db
            .prepare(&format!("EXPLAIN QUERY PLAN {ASPECTS_QUERY}"))
            .unwrap()
            .query_map([], |row| row.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert!(
            details.iter().all(|detail| !detail.contains("CORRELATED")),
            "{details:?}"
        );
    }

    use super::*;
    use std::sync::atomic::AtomicU64;
    use std::time::UNIX_EPOCH;

    #[test]
    fn capture_date_is_persisted_sorted_and_invalidated_without_changing_original() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("photos");
        std::fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let dated = root.join("dated.jpg");
        super::super::facts::tests::jpeg(&dated);
        let original = std::fs::read(&dated).unwrap();
        let plain = root.join("plain.png");
        image::RgbImage::new(4, 3).save(&plain).unwrap();
        let path = temp.path().join("data/db");
        let mut db = connect(&path).unwrap();
        add_source(&db, &root).unwrap();
        scan(&mut db, &AtomicU64::new(1), 1, |_| {}).unwrap();
        // Both files were copied in 2026, but the EXIF photo belongs to 2015.
        let state = snapshot(&db).unwrap();
        assert_eq!(state.photos[0].path, plain);
        assert_eq!(state.photos[1].path, dated);
        let taken = state.photos[1].taken.unwrap();
        assert!(taken < state.photos[1].modified);
        assert_eq!(std::fs::read(&dated).unwrap(), original);
        drop(db);
        let mut db = connect(&path).unwrap();
        assert_eq!(snapshot(&db).unwrap().photos[1].taken, Some(taken));
        db.execute(
            "UPDATE photos SET cache_key='changed' WHERE path=?",
            [path_bytes(&dated)],
        )
        .unwrap();
        assert!(snapshot(&db)
            .unwrap()
            .photos
            .iter()
            .find(|p| p.path == dated)
            .unwrap()
            .taken
            .is_none());
        scan(&mut db, &AtomicU64::new(1), 1, |_| {}).unwrap();
        assert_eq!(snapshot(&db).unwrap().photos[1].taken, Some(taken));
    }

    #[test]
    fn empty_directory_tree_survives_restart_offline_cancel_and_overlapping_roots() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("photos");
        let empty = root.join("empty/deep");
        std::fs::create_dir_all(&empty).unwrap();
        let root = root.canonicalize().unwrap();
        let empty = root.join("empty/deep");
        std::fs::create_dir(root.join(".private")).unwrap();
        std::fs::write(root.join("original.png"), b"untouched").unwrap();
        let path = temp.path().join("data/library");
        let mut db = connect(&path).unwrap();
        add_source(&db, &root).unwrap();
        add_source(&db, &root.join("empty")).unwrap();
        let generation = AtomicU64::new(1);
        scan(&mut db, &generation, 1, |_| {}).unwrap();
        drop(db);
        let mut db = connect(&path).unwrap();
        let state = snapshot(&db).unwrap();
        assert_eq!(state.directories.len(), 3);
        assert_eq!(
            state.child_folders(&root).into_keys().collect::<Vec<_>>(),
            [root.join("empty")]
        );
        assert_eq!(
            state
                .child_folders(&root.join("empty"))
                .into_keys()
                .collect::<Vec<_>>(),
            std::slice::from_ref(&empty)
        );
        assert!(state.child_folders(&empty).is_empty());

        // An unavailable source keeps its previously indexed empty tree.
        let offline = temp.path().join("offline");
        std::fs::rename(&root, &offline).unwrap();
        assert!(scan(&mut db, &generation, 1, |_| {}).unwrap().is_empty());
        assert_eq!(snapshot(&db).unwrap().directories, state.directories);
        std::fs::rename(&offline, &root).unwrap();

        // A cancelled scan must not publish newly discovered directories.
        std::fs::create_dir(root.join("cancelled")).unwrap();
        for n in 0..128 {
            std::fs::write(root.join("cancelled").join(format!("{n}.png")), b"fixture").unwrap();
        }
        scan(&mut db, &generation, 1, |n| {
            if n >= 128 {
                generation.store(2, Ordering::Relaxed);
            }
        })
        .unwrap();
        assert_eq!(generation.load(Ordering::Relaxed), 2);
        assert_eq!(snapshot(&db).unwrap().directories, state.directories);
        generation.store(1, Ordering::Relaxed);
        scan(&mut db, &generation, 1, |_| {}).unwrap();
        assert!(snapshot(&db)
            .unwrap()
            .directories
            .contains(&root.join("cancelled")));

        // Removing one source preserves descendants belonging to an overlap.
        let id = snapshot(&db)
            .unwrap()
            .sources
            .iter()
            .find(|s| s.path == root)
            .unwrap()
            .id;
        remove_source(&mut db, id).unwrap();
        assert_eq!(snapshot(&db).unwrap().directories.len(), 2);
        std::fs::rename(&empty, root.join("moved")).unwrap();
        scan(&mut db, &generation, 1, |_| {}).unwrap();
        assert_eq!(snapshot(&db).unwrap().directories, [root.join("empty")]);
        assert_eq!(
            std::fs::read(root.join("original.png")).unwrap(),
            b"untouched"
        );
    }

    #[test]
    fn index_survives_restart_and_prunes_only_missing_files() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("photos");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("one.JPG"), b"indexing does not decode").unwrap();
        std::fs::write(source.join("movie.mp4"), b"not indexed").unwrap();
        let db_path = temp.path().join("index/library.sqlite3");
        let mut db = connect(&db_path).unwrap();
        add_source(&db, &source).unwrap();
        add_source(&db, &source.join(".")).unwrap();
        let gen = AtomicU64::new(1);
        scan(&mut db, &gen, 1, |_| {}).unwrap();
        let state = snapshot(&db).unwrap();
        assert_eq!(state.sources.len(), 1);
        assert_eq!(state.photos.len(), 1);
        create_album(&db, "  Holiday  ").unwrap();
        let album = snapshot(&db).unwrap().albums[0].id;
        assign_album(&mut db, album, &[state.photos[0].id]).unwrap();
        drop(db);
        let mut db = connect(&db_path).unwrap();
        assert_eq!(snapshot(&db).unwrap().photos[0].albums, [album]);
        std::fs::rename(source.join("one.JPG"), temp.path().join("moved-out.JPG")).unwrap();
        scan(&mut db, &gen, 1, |_| {}).unwrap();
        assert!(snapshot(&db).unwrap().photos.is_empty());
    }

    #[test]
    fn missing_source_and_cancel_preserve_the_index() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("photos");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("one.png"), b"photo").unwrap();
        let mut db = connect(&temp.path().join("db/index")).unwrap();
        add_source(&db, &source).unwrap();
        let gen = AtomicU64::new(1);
        scan(&mut db, &gen, 1, |_| {}).unwrap();
        std::fs::rename(&source, temp.path().join("offline")).unwrap();
        assert!(scan(&mut db, &gen, 1, |_| {}).unwrap().is_empty());
        assert_eq!(snapshot(&db).unwrap().photos.len(), 1);
        scan(&mut db, &gen, 0, |_| {}).unwrap();
        assert_eq!(snapshot(&db).unwrap().photos.len(), 1);
        let id = snapshot(&db).unwrap().sources[0].id;
        remove_source(&mut db, id).unwrap();
        assert!(snapshot(&db).unwrap().photos.is_empty());
        assert!(temp.path().join("offline/one.png").exists());
    }

    #[test]
    fn overlapping_sources_deduplicate_and_mid_scan_cancel_rolls_back() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("photos");
        let nested = source.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("one.png"), b"photo").unwrap();
        let mut db = connect(&temp.path().join("db/index")).unwrap();
        add_source(&db, &source).unwrap();
        add_source(&db, &nested).unwrap();
        let generation = AtomicU64::new(1);
        scan(&mut db, &generation, 1, |_| {}).unwrap();
        let state = snapshot(&db).unwrap();
        assert_eq!(state.photos.len(), 1);
        remove_source(&mut db, state.sources[1].id).unwrap();
        assert_eq!(snapshot(&db).unwrap().photos.len(), 1);
        for n in 0..130 {
            std::fs::write(nested.join(format!("new-{n}.png")), b"photo").unwrap();
        }
        scan(&mut db, &generation, 1, |count| {
            if count >= 128 {
                generation.store(2, Ordering::Relaxed);
            }
        })
        .unwrap();
        assert_eq!(
            snapshot(&db).unwrap().photos.len(),
            1,
            "cancel rolls back partial inserts"
        );
        scan(&mut db, &generation, 2, |_| {}).unwrap();
        assert_eq!(snapshot(&db).unwrap().photos.len(), 131);
    }

    #[test]
    fn cache_identity_invalidates_on_metadata_changes() {
        let path = Path::new("photo.png");
        assert_ne!(
            cache_key(path, 1, UNIX_EPOCH),
            cache_key(path, 2, UNIX_EPOCH)
        );
        assert_ne!(
            cache_key(path, 1, UNIX_EPOCH),
            // Windows SystemTime is quantized to 100 ns, unlike Unix timespec.
            cache_key(path, 1, UNIX_EPOCH + std::time::Duration::from_nanos(100))
        );
    }

    #[test]
    fn album_names_are_validated_and_newer_schema_is_not_overwritten() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("db/index");
        let db = connect(&path).unwrap();
        assert!(create_album(&db, "  ").is_err());
        assert!(create_album(&db, &"a".repeat(121)).is_err());
        db.pragma_update(None, "user_version", 99).unwrap();
        drop(db);
        assert!(connect(&path).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn non_unicode_paths_round_trip() {
        use std::os::unix::ffi::OsStringExt;
        let path = PathBuf::from(std::ffi::OsString::from_vec(b"/photos/\xff.png".to_vec()));
        assert_eq!(bytes_path(path_bytes(&path)), path);
    }
}
#[test]
fn album_lifecycle_migrates_preserves_originals_and_rolls_back() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("db/library");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let old = Connection::open(&path).unwrap();
    old.execute_batch("CREATE TABLE albums(id INTEGER PRIMARY KEY,name TEXT NOT NULL); INSERT INTO albums VALUES(1,'Legacy'); PRAGMA user_version=1;").unwrap();
    drop(old);
    let mut db = connect(&path).unwrap();
    assert_eq!(snapshot(&db).unwrap().albums[0].name, "Legacy");
    let original = temp.path().join("original.jpg");
    std::fs::write(&original, b"untouched").unwrap();
    db.execute(
        "INSERT INTO photos(id,path,modified,cache_key) VALUES(1,?,0,'key')",
        [path_bytes(&original)],
    )
    .unwrap();
    let id = create_album_with_photos(&mut db, "Trip", &[1]).unwrap();
    rename_album(&db, id, "  Renamed  ").unwrap();
    cover_album(&db, id, 1).unwrap();
    assert!(cover_album(&db, id, 2).is_err());
    let album = snapshot(&db)
        .unwrap()
        .albums
        .into_iter()
        .find(|a| a.id == id)
        .unwrap();
    assert_eq!(album.name, "Renamed");
    assert_eq!(album.cover, Some(1));
    unassign_album(&mut db, id, &[1]).unwrap();
    assert_eq!(
        snapshot(&db)
            .unwrap()
            .albums
            .into_iter()
            .find(|a| a.id == id)
            .unwrap()
            .cover,
        None
    );
    let before = snapshot(&db).unwrap().albums.len();
    assert!(create_album_with_photos(&mut db, "Broken", &[999]).is_err());
    assert_eq!(snapshot(&db).unwrap().albums.len(), before);
    remove_album(&db, id).unwrap();
    assert_eq!(std::fs::read(&original).unwrap(), b"untouched");
    drop(db);
    let db = connect(&path).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        10
    );
}

#[test]
fn thumbnail_aspects_round_trip_and_reject_invalid_values() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("data/index");
    let mut db = connect(&path).unwrap();
    let key = "a".repeat(64);
    db.execute(
        "INSERT INTO photos(path,modified,cache_key) VALUES (?,0,?)",
        params![path_bytes(Path::new("photo.png")), key],
    )
    .unwrap();
    let photo = db.last_insert_rowid();
    toggle_favorite(&db, photo).unwrap();
    assert!(snapshot(&db).unwrap().favorites.contains(&photo));
    save_aspects(
        &mut db,
        &[
            (key.clone(), 0.75),
            ("b".repeat(64), f32::NAN),
            ("bad".into(), 1.0),
        ],
    )
    .unwrap();
    drop(db);
    let db = connect(&path).unwrap();
    assert!(snapshot(&db).unwrap().favorites.contains(&photo));
    toggle_favorite(&db, photo).unwrap();
    assert!(!snapshot(&db).unwrap().favorites.contains(&photo));
    assert_eq!(snapshot(&db).unwrap().aspects.get(&key), Some(&0.75));
    assert_eq!(
        db.query_row("SELECT count(*) FROM thumbnail_aspects", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    db.execute("UPDATE photos SET cache_key=?", ["c".repeat(64)])
        .unwrap();
    assert!(snapshot(&db).unwrap().aspects.is_empty());
}
