//! Durable, private recovery records for explicit in-place edits. One record per
//! native path; no index or originals are removed when a viewer closes.
#[cfg(not(test))]
use anyhow::Context;
use anyhow::{bail, Result};
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

fn directory() -> Result<PathBuf> {
    #[cfg(test)]
    {
        static DIR: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
        Ok(DIR
            .get_or_init(|| tempfile::tempdir().unwrap())
            .path()
            .to_owned())
    }
    #[cfg(not(test))]
    Ok(dirs::data_local_dir()
        .context("No local data directory for photo recovery")?
        .join("rphotos/recovery-v1"))
}
fn name(path: &Path) -> String {
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
    format!("{:x}.recovery", Sha256::digest(bytes))
}
pub(super) fn save(path: &Path, before: &[u8], expected: &[u8]) -> Result<()> {
    save_in(&directory()?, path, before, expected)
}
fn save_in(dir: &Path, path: &Path, before: &[u8], expected: &[u8]) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    file.write_all(b"RPHOTOS-RECOVERY-1\n")?;
    file.write_all(&Sha256::digest(expected))?;
    file.write_all(before)?;
    file.as_file().sync_all()?;
    file.persist(dir.join(name(path))).map_err(|e| e.error)?;
    #[cfg(unix)]
    std::fs::File::open(dir)?.sync_all()?;
    Ok(())
}
pub(super) fn load(path: &Path, current: &[u8]) -> Result<Vec<u8>> {
    load_in(&directory()?, path, current)
}
fn load_in(dir: &Path, path: &Path, current: &[u8]) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(dir.join(name(path)))?
        .take(128 * 1024 * 1024 + 51)
        .read_to_end(&mut bytes)?;
    if bytes.len() < 51 || !bytes.starts_with(b"RPHOTOS-RECOVERY-1\n") {
        bail!("Invalid photo recovery record");
    }
    // Header is 19 bytes, followed by the expected edited-file SHA256.
    if bytes[19..51] != Sha256::digest(current)[..] {
        bail!("Photo changed since saving; recovery will not overwrite it");
    }
    Ok(bytes[51..].to_vec())
}
pub(super) fn available(path: &Path) -> bool {
    // Called only by an edit worker, never by the UI.
    // Most photos have never been edited: avoid reading the full original just
    // to find out there is no recovery record.
    let Ok(directory) = directory() else {
        return false;
    };
    if !directory.join(name(path)).is_file() {
        return false;
    }
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if meta.len() > 128 * 1024 * 1024 {
        return false;
    }
    std::fs::read(path)
        .ok()
        .is_some_and(|current| load(path, &current).is_ok())
}
#[cfg(test)]
#[test]
fn durable_record_is_byte_exact_and_refuses_external_changes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("Фото.png");
    save_in(temp.path(), &path, b"original", b"edited").unwrap();
    assert_eq!(load_in(temp.path(), &path, b"edited").unwrap(), b"original");
    assert!(load_in(temp.path(), &path, b"external edit").is_err());
}
