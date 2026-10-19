//! Explicit Save a copy and Open with actions; never edits the viewed original.
use anyhow::{bail, Context, Result};
use std::path::Path;

// Preserve lexical absolute paths even if the removable volume is offline.
fn reveal_target(path: &Path) -> Result<(std::path::PathBuf, bool)> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    if absolute.is_file() {
        return Ok((absolute, true));
    }
    let mut parent = absolute.parent();
    while let Some(folder) = parent {
        if folder.is_dir() {
            return Ok((folder.to_path_buf(), false));
        }
        parent = folder.parent();
    }
    bail!("No accessible parent directory for this photo")
}

pub(super) fn reveal(path: &Path) -> Result<()> {
    let (target, select) = reveal_target(path)?;
    #[cfg(target_os = "linux")]
    {
        if select {
            let selected: Result<()> = pollster::block_on(async {
                let connection = ashpd::zbus::Connection::session().await?;
                let proxy = ashpd::zbus::Proxy::new(
                    &connection,
                    "org.freedesktop.FileManager1",
                    "/org/freedesktop/FileManager1",
                    "org.freedesktop.FileManager1",
                )
                .await?;
                let uri = ashpd::url::Url::from_file_path(&target)
                    .map_err(|_| anyhow::anyhow!("Invalid file URI"))?;
                proxy
                    .call::<_, _, ()>("ShowItems", &(vec![uri.as_str()], ""))
                    .await?;
                Ok(())
            });
            if selected.is_ok() {
                return Ok(());
            }
        }
        let folder = if select {
            target.parent().context("Photo has no parent directory")?
        } else {
            target.as_path()
        };
        if !std::process::Command::new("xdg-open")
            .arg(folder)
            .status()?
            .success()
        {
            bail!("The file manager could not open this folder");
        }
    }
    #[cfg(target_os = "windows")]
    {
        let mut command = std::process::Command::new("explorer.exe");
        if select {
            let mut argument = std::ffi::OsString::from("/select,");
            argument.push(&target);
            command.arg(argument);
        } else {
            command.arg(&target);
        }
        command.spawn().context("Cannot start Explorer")?;
    }
    #[cfg(target_os = "macos")]
    {
        let mut command = std::process::Command::new("/usr/bin/open");
        if select {
            command.arg("-R");
        }
        if !command.arg(&target).status()?.success() {
            bail!("Finder could not reveal this photo");
        }
    }
    Ok(())
}

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
    fn reveal_selects_existing_file_and_falls_back_to_accessible_parent() {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().canonicalize().unwrap();
        let photo = folder.join("Фото & # file.jpg");
        std::fs::write(&photo, b"original").unwrap();
        assert_eq!(reveal_target(&photo).unwrap(), (photo.clone(), true));
        assert_eq!(
            reveal_target(&folder.join("offline/sub/file.jpg")).unwrap(),
            (folder.clone(), false)
        );
        assert_eq!(std::fs::read(photo).unwrap(), b"original");
    }

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
