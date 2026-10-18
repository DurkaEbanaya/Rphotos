//! Explicit Save a copy and Open with actions; never edits the viewed original.
use anyhow::{bail, Context, Result};
use std::path::Path;

pub(super) fn save_copy(source: &Path, destination: &Path) -> Result<()> {
    let source = source.canonicalize()?;
    if !source.is_file() {
        bail!("Source is not a file");
    }
    if destination.canonicalize().is_ok_and(|path| path == source) {
        bail!("Choose a different destination; Rphotos never overwrites the original");
    }
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut input = std::fs::File::open(&source)?;
    let before = input.metadata()?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    std::io::copy(&mut input, temporary.as_file_mut()).context("Cannot copy photo")?;
    let after = input.metadata()?;
    if before.len() != after.len() || before.modified()? != after.modified()? {
        bail!("Source changed while copying; destination was not replaced");
    }
    temporary.as_file().sync_all()?;
    // No-clobber protects *other* originals too, not only the currently viewed
    // path. A chooser overwrite confirmation does not waive this safety rule.
    temporary
        .persist_noclobber(destination)
        .map_err(|e| e.error)
        .context("Cannot save photo copy")?;
    Ok(())
}

#[cfg(target_os = "linux")]
pub(super) fn open_with(path: &Path) -> Result<()> {
    let file = std::fs::File::open(path)?;
    pollster::block_on(async {
        match ashpd::desktop::open_uri::OpenFileRequest::default()
            .ask(true)
            .writeable(false)
            .send_file(&file)
            .await?
            .response()
        {
            Ok(()) | Err(ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled)) => {
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    })
}

#[cfg(not(target_os = "linux"))]
pub(super) fn open_with(_path: &Path) -> Result<()> {
    bail!("The system application chooser is not implemented on this platform yet")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_is_byte_exact_atomic_and_cannot_replace_original() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("Фото оригинал.jpg");
        let bytes = b"arbitrary original bytes including EXIF\0\xff";
        std::fs::write(&source, bytes).unwrap();
        assert!(save_copy(&source, &source).is_err());
        assert_eq!(std::fs::read(&source).unwrap(), bytes);
        let output = temp.path().join("Copy.jpg");
        std::fs::write(&output, b"previous destination").unwrap();
        assert!(save_copy(&temp.path().join("missing.jpg"), &output).is_err());
        assert_eq!(std::fs::read(&output).unwrap(), b"previous destination");
        assert!(save_copy(&source, &output).is_err());
        assert_eq!(std::fs::read(&output).unwrap(), b"previous destination");
        let new_output = temp.path().join("New copy.jpg");
        save_copy(&source, &new_output).unwrap();
        assert_eq!(std::fs::read(&new_output).unwrap(), bytes);
        assert_eq!(std::fs::read(&source).unwrap(), bytes);
        #[cfg(unix)]
        {
            let alias = temp.path().join("alias.jpg");
            std::os::unix::fs::symlink(&source, &alias).unwrap();
            assert!(save_copy(&source, &alias).is_err());
        }
    }
}
