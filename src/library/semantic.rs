//! Opt-in text embeddings of OCR/descriptions. Never uploads original images.
use super::store;
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};

#[derive(Default)]
pub(super) struct Search {
    pub query: String,
    pub hits: Option<std::collections::HashSet<i64>>,
}

pub(super) struct Matches {
    pub query: String,
    pub ids: Vec<i64>,
}

fn fingerprint(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

pub(super) fn normalize(mut vector: Vec<f32>) -> Result<Vec<f32>> {
    if vector.is_empty() || vector.len() > 8192 || vector.iter().any(|v| !v.is_finite()) {
        bail!("Invalid embedding vector");
    }
    let norm = vector
        .iter()
        .map(|v| f64::from(*v).powi(2))
        .sum::<f64>()
        .sqrt();
    if norm <= 1e-12 {
        bail!("Empty embedding vector");
    }
    for v in &mut vector {
        *v = (f64::from(*v) / norm) as f32;
    }
    Ok(vector)
}

fn parse(bytes: &[u8]) -> Result<Vec<f32>> {
    let data: serde_json::Value = serde_json::from_slice(bytes)?;
    let array = data
        .get("embeddings")
        .and_then(|v| v.get(0))
        .and_then(|v| v.as_array())
        .context("Local model returned no embedding")?;
    if array.len() > 8192 {
        bail!("Embedding dimension exceeds limit");
    }
    normalize(
        array
            .iter()
            .map(|v| {
                v.as_f64()
                    .map(|v| v as f32)
                    .context("Non-numeric embedding")
            })
            .collect::<Result<Vec<_>>>()?,
    )
}

fn embed(model: &str, text: &str, cancel: impl Fn() -> bool) -> Result<Vec<f32>> {
    embed_at(model, text, cancel, "http://127.0.0.1:11434/api/embed")
}

fn embed_at(model: &str, text: &str, cancel: impl Fn() -> bool, url: &str) -> Result<Vec<f32>> {
    if model.trim().is_empty() || model.len() > 200 {
        bail!("Choose an installed local embedding model");
    }
    let input: String = text.chars().take(12000).collect();
    if input.trim().is_empty() {
        bail!("No text for embedding; run OCR or content descriptions first");
    }
    let body = serde_json::json!({"model":model.trim(),"input":input,"truncate":true,"keep_alive":0,"options":{"num_thread":1}});
    parse(&super::analysis::request_json(&body, url, cancel)?)
}

fn pack(vector: &[f32]) -> Vec<u8> {
    vector.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn unpack(bytes: &[u8]) -> Result<Vec<f32>> {
    let (chunks, rest) = bytes.as_chunks::<4>();
    if !rest.is_empty() {
        bail!("Corrupt embedding data");
    }
    normalize(chunks.iter().map(|b| f32::from_le_bytes(*b)).collect())
}
fn cosine(a: &[f32], b: &[f32]) -> Option<f32> {
    (a.len() == b.len()).then(|| {
        a.iter()
            .zip(b)
            .map(|(a, b)| a * b)
            .sum::<f32>()
            .clamp(-1.0, 1.0)
    })
}

pub(super) fn index(
    db: &Connection,
    model: &str,
    cancel: impl Fn() -> bool,
    progress: impl Fn(usize),
) -> Result<()> {
    for (n, photo) in store::snapshot(db)?.photos.iter().enumerate() {
        if cancel() {
            break;
        }
        if !photo.text.trim().is_empty() {
            let hash = fingerprint(&photo.text);
            let exists: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM photo_embeddings WHERE photo_id=? AND cache_key=? AND model=? AND text_hash=?)",params![photo.id,photo.key,model.trim(),hash],|r|r.get(0))?;
            if !exists {
                let vector = embed(model, &photo.text, &cancel)?;
                if cancel() {
                    break;
                }
                // Reject a file changed while the model was working, before persisting.
                let current = std::fs::metadata(&photo.path).ok().and_then(|m| {
                    m.modified()
                        .ok()
                        .map(|t| store::cache_key(&photo.path, m.len(), t))
                });
                if current.as_ref() == Some(&photo.key) {
                    db.execute("INSERT INTO photo_embeddings VALUES(?,?,?,?,?) ON CONFLICT(photo_id) DO UPDATE SET cache_key=excluded.cache_key,model=excluded.model,text_hash=excluded.text_hash,vector=excluded.vector", params![photo.id,photo.key,model.trim(),hash,pack(&vector)])?;
                }
            }
        }
        progress(n + 1);
    }
    Ok(())
}

fn vectors(db: &Connection, model: &str) -> Result<Vec<(store::Photo, Vec<f32>)>> {
    let mut stmt = db.prepare("SELECT vector FROM photo_embeddings WHERE photo_id=? AND cache_key=? AND model=? AND text_hash=? AND length(vector)<=32768")?;
    let mut result = Vec::new();
    let mut used = 0usize;
    for photo in store::snapshot(db)?.photos {
        use rusqlite::OptionalExtension;
        let bytes: Option<Vec<u8>> = stmt
            .query_row(
                params![photo.id, photo.key, model.trim(), fingerprint(&photo.text)],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(bytes) = bytes {
            used += bytes.len();
            if used > 256 * 1024 * 1024 {
                bail!("Semantic vectors exceed the current 256 MiB memory budget");
            }
            if let Ok(vector) = unpack(&bytes) {
                result.push((photo, vector));
            }
        }
    }
    Ok(result)
}

pub(super) fn search(
    db: &Connection,
    model: &str,
    query: String,
    cancel: impl Fn() -> bool,
) -> Result<Matches> {
    if query.len() > 4096 || query.trim().is_empty() {
        bail!("Enter a search query (up to 4096 bytes)");
    }
    let vectors = vectors(db, model)?;
    if vectors.is_empty() {
        bail!("Build the semantic index with this embedding model first");
    }
    let query_vector = embed(model, &query, &cancel)?;
    if cancel() {
        bail!("Semantic search cancelled");
    }
    if !vectors.iter().any(|(_, v)| v.len() == query_vector.len()) {
        bail!("Model embedding dimension changed; rebuild the semantic index");
    }
    let scores = rank(
        &query_vector,
        vectors.iter().map(|(p, v)| (p.id, v.as_slice())),
    );
    Ok(Matches { query, ids: scores })
}

fn rank<'a>(query: &[f32], vectors: impl Iterator<Item = (i64, &'a [f32])>) -> Vec<i64> {
    let mut scores: Vec<_> = vectors
        .filter_map(|(id, v)| cosine(v, query).filter(|s| *s >= 0.3).map(|s| (id, s)))
        .collect();
    scores.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    scores.into_iter().take(100).map(|(id, _)| id).collect()
}

/// Bounded greedy similarity groups. No identities/faces, no inferred category labels.
pub(super) fn albums(db: &mut Connection, model: &str, cancel: impl Fn() -> bool) -> Result<()> {
    let data = vectors(db, model)?;
    if data.is_empty() {
        bail!("Build the semantic index first");
    }
    if data.len() > 10000 {
        bail!("Content album builder currently supports at most 10000 indexed photos");
    }
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (i, (_, v)) in data.iter().enumerate() {
        if cancel() {
            return Ok(());
        }
        let best = groups
            .iter()
            .enumerate()
            .filter_map(|(g, items)| cosine(v, &data[items[0]].1).map(|s| (g, s)))
            .filter(|(_, s)| *s >= 0.75)
            .max_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((g, _)) = best {
            groups[g].push(i);
        } else if groups.len() < 64 {
            groups.push(vec![i]);
        }
    }
    if cancel() {
        return Ok(());
    }
    let tx = db.transaction()?;
    tx.execute("DELETE FROM albums WHERE auto_key LIKE 'content:%'", [])?;
    for group in groups.iter().filter(|g| g.len() >= 3) {
        if cancel() {
            return Ok(());
        } // transaction rollback preserves existing albums
        let representative = &data[group[0]].0;
        let title: String = representative
            .path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .chars()
            .take(90)
            .collect();
        let id = store::create_album(&tx, &format!("≈ {title}"))?;
        tx.execute(
            "UPDATE albums SET auto_key=? WHERE id=?",
            params![
                format!("content:{}:{}", fingerprint(model), representative.id),
                id
            ],
        )?;
        for &i in group {
            tx.execute(
                "INSERT INTO album_photos VALUES(?,?)",
                params![id, data[i].0.id],
            )?;
        }
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vectors_validate_normalize_and_reject_mismatched_models() {
        let a = normalize(vec![3.0, 4.0]).unwrap();
        assert!((cosine(&a, &a).unwrap() - 1.0).abs() < 1e-6);
        assert_eq!(cosine(&a, &[1.0]), None);
        assert!(normalize(vec![f32::INFINITY]).is_err());
        assert!(normalize(vec![0.0]).is_err());
        assert!(unpack(&[1, 2, 3]).is_err());
        assert_eq!(unpack(&pack(&a)).unwrap(), a);
        assert!(parse(br#"{"embeddings":[[1,"bad"]]}"#).is_err());
        assert_eq!(parse(br#"{"embeddings":[[3,4]]}"#).unwrap(), a);
        assert_eq!(
            rank(
                &[1.0, 0.0],
                [
                    (9, [0.0, 1.0].as_slice()),
                    (3, [0.6, 0.8].as_slice()),
                    (2, [1.0, 0.0].as_slice()),
                    (1, [-1.0, 0.0].as_slice())
                ]
                .into_iter()
            ),
            [2, 3]
        );
    }

    #[cfg(unix)]
    #[test]
    fn embedding_adapter_uses_loopback_and_checks_cancellation() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/api/embed", server.local_addr().unwrap());
        let task = std::thread::spawn(move || {
            let (mut stream, _) = server.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut data = Vec::new();
            let mut buf = [0; 4096];
            loop {
                let n = stream.read(&mut buf).unwrap();
                assert!(n > 0);
                data.extend_from_slice(&buf[..n]);
                if let Some(start) = data.windows(4).position(|b| b == b"\r\n\r\n") {
                    let length = String::from_utf8_lossy(&data[..start])
                        .lines()
                        .find_map(|l| {
                            l.to_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap();
                    if data.len() >= start + 4 + length {
                        let body: serde_json::Value =
                            serde_json::from_slice(&data[start + 4..start + 4 + length]).unwrap();
                        assert_eq!(body["model"], "test-embedding");
                        assert_eq!(body["input"], "Кот на траве");
                        assert!(body.get("images").is_none());
                        break;
                    }
                }
            }
            let body = r#"{"embeddings":[[3,4]]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        assert_eq!(
            embed_at("test-embedding", "Кот на траве", || false, &url).unwrap(),
            vec![0.6, 0.8]
        );
        task.join().unwrap();
        assert!(embed_at("test-embedding", "text", || true, &url).is_err());
    }

    #[test]
    fn persisted_vectors_invalidate_and_content_albums_preserve_manual_events() {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("data/library");
        let mut db = store::connect(&database).unwrap();
        let source = temp.path().join("photos");
        std::fs::create_dir(&source).unwrap();
        for id in 0..6 {
            image::RgbImage::new(12, 8)
                .save(source.join(format!("{id}.png")))
                .unwrap();
        }
        store::add_source(&db, &source).unwrap();
        store::scan(&mut db, &std::sync::atomic::AtomicU64::new(0), 0, |_| {}).unwrap();
        let photos = store::snapshot(&db).unwrap().photos;
        for (i, p) in photos.iter().enumerate() {
            db.execute(
                "INSERT INTO photo_text VALUES(?,?,?)",
                params![p.id, p.key, format!("text {i}")],
            )
            .unwrap();
        }
        for (i, p) in store::snapshot(&db).unwrap().photos.iter().enumerate() {
            let vector = if i < 3 {
                vec![1.0, 0.0]
            } else {
                vec![0.0, 1.0]
            };
            db.execute(
                "INSERT INTO photo_embeddings VALUES(?,?,?,?,?)",
                params![p.id, p.key, "test", fingerprint(&p.text), pack(&vector)],
            )
            .unwrap();
        }
        assert_eq!(vectors(&db, "test").unwrap().len(), 6);
        assert!(vectors(&db, "other-model").unwrap().is_empty());
        albums(&mut db, "test", || false).unwrap();
        let content = store::snapshot(&db).unwrap().albums;
        assert_eq!(content.len(), 2);
        store::rename_album(&db, content[0].id, "Keep manual").unwrap();
        store::auto_albums(&mut db).unwrap();
        assert_eq!(store::snapshot(&db).unwrap().albums.len(), 3); // manual + content + date
        albums(&mut db, "test", || true).unwrap();
        assert_eq!(store::snapshot(&db).unwrap().albums.len(), 3);
        albums(&mut db, "test", || false).unwrap();
        let state = store::snapshot(&db).unwrap();
        assert_eq!(state.albums.len(), 4);
        assert!(state
            .albums
            .iter()
            .any(|a| a.name == "Keep manual" && !a.automatic));
        drop(db);
        let db = store::connect(&database).unwrap();
        assert_eq!(vectors(&db, "test").unwrap().len(), 6);
        db.execute(
            "UPDATE photo_text SET text='changed' WHERE photo_id=?",
            [photos[0].id],
        )
        .unwrap();
        assert_eq!(vectors(&db, "test").unwrap().len(), 5);
        db.execute(
            "UPDATE photos SET cache_key='changed' WHERE id=?",
            [photos[1].id],
        )
        .unwrap();
        assert_eq!(vectors(&db, "test").unwrap().len(), 4);
        assert!(source.join("0.png").exists());
    }
}
