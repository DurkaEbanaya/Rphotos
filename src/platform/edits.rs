//! Lossless container orientation edits. Pixel streams and unrelated metadata
//! are preserved; replacement is staged in the same directory and atomic.
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::mpsc,
};

const MAX_FILE: u64 = 128 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(crate) enum Transform {
    Right,
    Left,
    Horizontal,
    Vertical,
}

pub(crate) struct Undo {
    path: PathBuf,
    original: Vec<u8>,
    expected: [u8; 32],
}

fn compose(old: u8, transform: Transform) -> u8 {
    let right = [0, 6, 7, 8, 5, 2, 3, 4, 1];
    match transform {
        Transform::Right => right[old as usize],
        Transform::Left => right[right[right[old as usize] as usize] as usize],
        Transform::Horizontal => [0, 2, 1, 4, 3, 6, 5, 8, 7][old as usize],
        Transform::Vertical => [0, 4, 3, 2, 1, 8, 7, 6, 5][old as usize],
    }
}

fn short(bytes: &[u8], at: usize, le: bool) -> Result<u16> {
    let b: [u8; 2] = bytes
        .get(at..at + 2)
        .context("Truncated EXIF")?
        .try_into()?;
    Ok(if le {
        u16::from_le_bytes(b)
    } else {
        u16::from_be_bytes(b)
    })
}
fn long(bytes: &[u8], at: usize, le: bool) -> Result<u32> {
    let b: [u8; 4] = bytes
        .get(at..at + 4)
        .context("Truncated EXIF")?
        .try_into()?;
    Ok(if le {
        u32::from_le_bytes(b)
    } else {
        u32::from_be_bytes(b)
    })
}
fn put_short(bytes: &mut [u8], at: usize, value: u16, le: bool) {
    bytes[at..at + 2].copy_from_slice(&if le {
        value.to_le_bytes()
    } else {
        value.to_be_bytes()
    });
}
fn put_long(bytes: &mut [u8], at: usize, value: u32, le: bool) {
    bytes[at..at + 4].copy_from_slice(&if le {
        value.to_le_bytes()
    } else {
        value.to_be_bytes()
    });
}

fn exif(mut bytes: Vec<u8>, transform: Transform) -> Result<Vec<u8>> {
    if bytes.is_empty() {
        bytes = b"II\x2a\0\x08\0\0\0\0\0\0\0\0\0".to_vec();
    }
    let le = match bytes.get(..2) {
        Some(b"II") => true,
        Some(b"MM") => false,
        _ => bail!("Invalid EXIF byte order"),
    };
    if short(&bytes, 2, le)? != 42 {
        bail!("Unsupported EXIF header");
    }
    let start = long(&bytes, 4, le)? as usize;
    if start < 8 {
        bail!("Invalid EXIF directory");
    }
    let count = short(&bytes, start, le)? as usize;
    let end = start
        .checked_add(2 + count * 12)
        .context("Invalid EXIF directory")?;
    let next = long(&bytes, end, le)?;
    let mut orientation = None;
    for n in 0..count {
        let at = start + 2 + n * 12;
        if short(&bytes, at, le)? == 0x112 {
            if orientation.is_some()
                || short(&bytes, at + 2, le)? != 3
                || long(&bytes, at + 4, le)? != 1
            {
                bail!("Ambiguous EXIF orientation");
            }
            let old = short(&bytes, at + 8, le)?;
            if !(1..=8).contains(&old) {
                bail!("Invalid EXIF orientation");
            }
            orientation = Some((at + 8, old as u8));
        }
    }
    if let Some((at, old)) = orientation {
        put_short(&mut bytes, at, compose(old, transform).into(), le);
    } else {
        if count >= u16::MAX as usize {
            bail!("EXIF directory is full");
        }
        // Appending a replacement IFD0 preserves every existing TIFF-relative
        // data/SubIFD/MakerNote offset. Only the header's IFD0 pointer changes.
        let mut entries: Vec<[u8; 12]> = bytes[start + 2..end].as_chunks::<12>().0.to_vec();
        let mut added = [0_u8; 12];
        put_short(&mut added, 0, 0x112, le);
        put_short(&mut added, 2, 3, le);
        put_long(&mut added, 4, 1, le);
        put_short(&mut added, 8, compose(1, transform).into(), le);
        entries.push(added);
        entries.sort_by_key(|entry| short(entry, 0, le).unwrap_or_default());
        if !bytes.len().is_multiple_of(2) {
            bytes.push(0);
        }
        let new = bytes.len();
        bytes.resize(new + 2 + (count + 1) * 12 + 4, 0);
        put_long(&mut bytes, 4, new.try_into()?, le);
        put_short(&mut bytes, new, (count + 1) as u16, le);
        for (n, entry) in entries.iter().enumerate() {
            bytes[new + 2 + n * 12..new + 2 + (n + 1) * 12].copy_from_slice(entry);
        }
        put_long(&mut bytes, new + 2 + entries.len() * 12, next, le);
    }
    Ok(bytes)
}

fn jpeg(input: &[u8], transform: Transform) -> Result<Vec<u8>> {
    let mut out = input[..2].to_vec();
    let mut at = 2;
    let mut found = false;
    while at < input.len() {
        let start = at;
        if input[at] != 0xff {
            bail!("Invalid JPEG marker");
        }
        while input.get(at) == Some(&0xff) {
            at += 1;
        }
        let marker = *input.get(at).context("Truncated JPEG")?;
        at += 1;
        if marker == 0xda || marker == 0xd9 {
            if !found {
                append_exif(&mut out, exif(Vec::new(), transform)?)?;
            }
            out.extend_from_slice(&input[start..]);
            return Ok(out);
        }
        if marker == 1 || (0xd0..=0xd7).contains(&marker) {
            out.extend_from_slice(&input[start..at]);
            continue;
        }
        let len = u16::from_be_bytes(
            input
                .get(at..at + 2)
                .context("Truncated JPEG")?
                .try_into()?,
        ) as usize;
        if len < 2 {
            bail!("Invalid JPEG segment");
        }
        let end = at
            .checked_add(len)
            .filter(|&end| end <= input.len())
            .context("Truncated JPEG segment")?;
        let payload = &input[at + 2..end];
        if marker == 0xe1 && payload.starts_with(b"Exif\0\0") {
            if found {
                bail!("Multiple EXIF segments are not safe to edit");
            }
            append_exif(&mut out, exif(payload[6..].to_vec(), transform)?)?;
            found = true;
        } else {
            out.extend_from_slice(&input[start..end]);
        }
        at = end;
    }
    bail!("JPEG has no image data")
}
fn append_exif(out: &mut Vec<u8>, exif: Vec<u8>) -> Result<()> {
    let len: u16 = (exif.len() + 8)
        .try_into()
        .context("EXIF segment is too large; original unchanged")?;
    out.extend_from_slice(&[0xff, 0xe1]);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(b"Exif\0\0");
    out.extend(exif);
    Ok(())
}
fn crc(bytes: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320_u32 & 0_u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}
fn png_exif(out: &mut Vec<u8>, bytes: Vec<u8>) {
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    let at = out.len();
    out.extend_from_slice(b"eXIf");
    out.extend(bytes);
    let checksum = crc(&out[at..]);
    out.extend_from_slice(&checksum.to_be_bytes());
}
fn png(input: &[u8], transform: Transform) -> Result<Vec<u8>> {
    let mut chunks = Vec::new();
    let mut at = 8;
    let mut found = false;
    let mut ended = false;
    let mut pixels = false;
    while at < input.len() {
        let len = u32::from_be_bytes(input.get(at..at + 4).context("Truncated PNG")?.try_into()?)
            as usize;
        let end = at
            .checked_add(len)
            .and_then(|n| n.checked_add(12))
            .filter(|&n| n <= input.len())
            .context("Truncated PNG chunk")?;
        let kind = &input[at + 4..at + 8];
        if chunks.is_empty() && kind != b"IHDR" {
            bail!("PNG header is missing");
        }
        pixels |= kind == b"IDAT";
        if kind == b"acTL" {
            bail!("Animated PNG rotation is not supported yet");
        }
        if crc(&input[at + 4..end - 4]) != u32::from_be_bytes(input[end - 4..end].try_into()?) {
            bail!("Invalid PNG checksum; original unchanged");
        }
        if kind == b"eXIf" {
            if found {
                bail!("Multiple PNG EXIF chunks");
            }
            found = true;
        }
        chunks.push((at, end));
        at = end;
        if kind == b"IEND" {
            if len != 0 || at != input.len() {
                bail!("PNG trailing data is not safe to edit");
            }
            ended = true;
            break;
        }
    }
    if !ended || !pixels {
        bail!("PNG image data or end marker is missing");
    }
    let mut out = input[..8].to_vec();
    let mut inserted = false;
    for (at, end) in chunks {
        let kind = &input[at + 4..at + 8];
        if !found && !inserted && kind == b"IDAT" {
            png_exif(&mut out, exif(Vec::new(), transform)?);
            inserted = true;
        }
        if kind == b"eXIf" {
            png_exif(&mut out, exif(input[at + 8..end - 4].to_vec(), transform)?);
        } else {
            out.extend_from_slice(&input[at..end]);
        }
    }
    Ok(out)
}
fn transformed(input: &[u8], transform: Transform) -> Result<Vec<u8>> {
    if input.starts_with(b"\xff\xd8") {
        jpeg(input, transform)
    } else if input.starts_with(b"\x89PNG\r\n\x1a\n") {
        png(input, transform)
    } else {
        bail!("Lossless rotation currently supports JPEG and still PNG only")
    }
}
fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

pub(super) fn read_original(path: &Path) -> Result<Vec<u8>> {
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        bail!("Choose a regular file, not a symbolic link");
    }
    if meta.permissions().readonly() {
        bail!("Photo is read-only");
    }
    if meta.len() > MAX_FILE {
        bail!("Photo exceeds the 128 MiB edit safety limit");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.nlink() > 1 {
            bail!("Photos with hard links are not edited in place");
        }
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        };
        let file = std::fs::File::open(path)?;
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: file owns a valid handle; the output points to initialized,
        // correctly sized writable storage for the duration of the call.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        if info.nNumberOfLinks > 1 {
            bail!("Photos with hard links are not edited in place");
        }
    }
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_FILE + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE {
        bail!("Photo grew beyond the edit safety limit");
    }
    Ok(bytes)
}
pub(super) fn replace(path: &Path, before: &[u8], after: &[u8]) -> Result<()> {
    if after.len() as u64 > MAX_FILE {
        bail!("Edited photo exceeds the 128 MiB safety limit");
    }
    let meta = std::fs::metadata(path)?;
    let mut staged = tempfile::NamedTempFile::new_in(
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?;
    staged.write_all(after)?;
    #[cfg(unix)]
    {
        use std::os::{fd::AsRawFd, unix::fs::MetadataExt};
        use xattr::FileExt;
        let input = std::fs::File::open(path)?;
        // Preserve ownership first: chown can clear mode bits/ACLs.
        // SAFETY: staged owns this live descriptor; IDs are from source metadata.
        if unsafe { libc::fchown(staged.as_file().as_raw_fd(), meta.uid(), meta.gid()) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        for name in input.list_xattr()? {
            if let Some(value) = input.get_xattr(&name)? {
                staged.as_file().set_xattr(&name, &value)?;
            }
        }
    }
    staged.as_file().set_permissions(meta.permissions())?;
    staged.as_file().sync_all()?;
    // Validate the complete staged container before replacing the original.
    crate::file_io::load_image(staged.path())
        .image
        .context("Cannot validate edited photo")?;
    if read_original(path)? != before {
        bail!("Photo changed during editing; original was not replaced");
    }
    // Recovery must be durable BEFORE replacing the original. A backup failure
    // aborts the edit rather than leaving an irreversible successful save.
    super::recovery::save(path, before, after)?;
    if read_original(path)? != before {
        bail!("Photo changed while saving recovery; original was not replaced");
    }
    #[cfg(not(target_os = "windows"))]
    staged
        .persist(path)
        .map_err(|e| e.error)
        .context("Cannot atomically replace photo")?;
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        let destination = path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        // ReplaceFile needs delete access to the replacement. NamedTempFile's
        // open handle does not share deletion on Windows: retain its TempPath
        // cleanup guard but close the File before calling the native operation.
        let staged_path = staged.into_temp_path();
        let replacement = staged_path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        // ReplaceFile preserves destination security/streams and fails closed if
        // it cannot merge attributes; do not use IGNORE_MERGE_ERRORS.
        if unsafe {
            windows_sys::Win32::Storage::FileSystem::ReplaceFileW(
                destination.as_ptr(),
                replacement.as_ptr(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                std::ptr::null(),
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    #[cfg(unix)]
    std::fs::File::open(path.parent().unwrap_or(Path::new(".")))?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn upright(path: &Path) -> image::DynamicImage {
        let loaded = crate::file_io::load_image(path);
        let mut image = loaded.image.unwrap();
        image.apply_orientation(loaded.orientation);
        image
    }
    #[test]
    fn all_exif_orientations_compose_as_displayed_pixels() {
        use image::metadata::Orientation;
        for old in 1..=8 {
            for transform in [
                Transform::Right,
                Transform::Left,
                Transform::Horizontal,
                Transform::Vertical,
            ] {
                let original =
                    image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(3, 2, |x, y| {
                        image::Rgb([x as u8, y as u8, 80])
                    }));
                let mut expected = original.clone();
                expected.apply_orientation(Orientation::from_exif(old).unwrap());
                expected = match transform {
                    Transform::Right => expected.rotate90(),
                    Transform::Left => expected.rotate270(),
                    Transform::Horizontal => expected.fliph(),
                    Transform::Vertical => expected.flipv(),
                };
                let mut actual = original;
                actual.apply_orientation(Orientation::from_exif(compose(old, transform)).unwrap());
                assert_eq!(actual, expected, "orientation {old}");
            }
        }
    }
    #[test]
    fn jpeg_and_png_rotate_without_changing_pixel_stream_and_undo_exactly() {
        for format in ["jpg", "png"] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join(format!("Фото.{format}"));
            image::RgbImage::from_fn(3, 2, |x, y| image::Rgb([x as u8 * 70, y as u8 * 100, 80]))
                .save(&path)
                .unwrap();
            let original = std::fs::read(&path).unwrap();
            let expected = upright(&path).rotate90();
            let edit = apply(&path, Transform::Right).unwrap();
            assert_eq!(upright(&path), expected);
            let output = std::fs::read(&path).unwrap();
            // Decoder pixels (before EXIF) remain identical, including JPEG quantization.
            assert_eq!(
                image::load_from_memory(&original).unwrap(),
                image::load_from_memory(&output).unwrap()
            );
            undo(edit).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), original);
            for _ in 0..4 {
                apply(&path, Transform::Right).unwrap();
            }
            assert_eq!(upright(&path), image::load_from_memory(&original).unwrap());
        }
    }
    #[test]
    fn exif_without_orientation_preserves_offsets_metadata_and_endianness() {
        for le in [true, false] {
            let mut tiff = vec![0; 40];
            tiff[..2].copy_from_slice(if le { b"II" } else { b"MM" });
            put_short(&mut tiff, 2, 42, le);
            put_long(&mut tiff, 4, 8, le);
            put_short(&mut tiff, 8, 1, le);
            put_short(&mut tiff, 10, 0x10e, le);
            put_short(&mut tiff, 12, 2, le);
            put_long(&mut tiff, 14, 7, le);
            put_long(&mut tiff, 18, 26, le);
            tiff[26..33].copy_from_slice(b"retain\0");
            let edited = exif(tiff.clone(), Transform::Right).unwrap();
            assert_eq!(&edited[8..40], &tiff[8..40]);
            let parsed = exif::Reader::new().read_raw(edited.clone()).unwrap();
            assert_eq!(
                parsed
                    .get_field(exif::Tag::ImageDescription, exif::In::PRIMARY)
                    .unwrap()
                    .display_value()
                    .to_string(),
                "\"retain\""
            );
            assert_eq!(
                image::metadata::Orientation::from_exif_chunk(&edited)
                    .unwrap()
                    .to_exif(),
                6
            );
        }
    }
    #[test]
    fn concurrent_change_readonly_and_invalid_files_are_not_overwritten() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("photo.png");
        image::RgbImage::new(3, 2).save(&path).unwrap();
        let edit = apply(&path, Transform::Right).unwrap();
        std::fs::write(&path, b"changed elsewhere").unwrap();
        assert!(undo(edit).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"changed elsewhere");
        let original = std::fs::read(&path).unwrap();
        assert!(apply(&path, Transform::Right).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        image::RgbImage::new(3, 2).save(&path).unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(replace(&path, b"not current", &before).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&path, permissions).unwrap();
        assert!(apply(&path, Transform::Right).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        #[cfg(windows)]
        {
            let mut permissions = std::fs::metadata(&path).unwrap().permissions();
            permissions.set_readonly(false);
            std::fs::set_permissions(&path, permissions).unwrap();
        }
    }
    #[test]
    fn malformed_and_animated_containers_are_rejected() {
        assert!(transformed(b"GIF89a", Transform::Right).is_err());
        assert!(jpeg(b"\xff\xd8\xff\xe1\0\xff", Transform::Right).is_err());
        assert!(exif(b"MMbad".to_vec(), Transform::Right).is_err());
        let mut animated = b"\x89PNG\r\n\x1a\n".to_vec();
        animated.extend_from_slice(&0_u32.to_be_bytes());
        animated.extend_from_slice(b"acTL");
        animated.extend_from_slice(&0_u32.to_be_bytes());
        assert!(png(&animated, Transform::Right).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn original_owner_mode_and_user_xattrs_survive_edit_and_undo() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("photo.png");
        image::RgbImage::new(6, 4).save(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        #[cfg(target_os = "linux")]
        xattr::set(&path, "user.rphotos-test", b"retained").unwrap();
        let before = std::fs::metadata(&path).unwrap();
        let undo = apply(&path, Transform::Right).unwrap();
        let after = std::fs::metadata(&path).unwrap();
        assert_eq!(
            (after.uid(), after.gid(), after.mode()),
            (before.uid(), before.gid(), before.mode())
        );
        #[cfg(target_os = "linux")]
        assert_eq!(
            xattr::get(&path, "user.rphotos-test").unwrap().unwrap(),
            b"retained"
        );
        super::undo(undo).unwrap();
        #[cfg(target_os = "linux")]
        assert_eq!(
            xattr::get(&path, "user.rphotos-test").unwrap().unwrap(),
            b"retained"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn native_replacement_preserves_alternate_streams_through_edit_and_undo() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("photo.png");
        image::RgbImage::new(6, 4).save(&path).unwrap();
        let mut stream = path.as_os_str().to_os_string();
        stream.push(":rphotos-test");
        std::fs::write(&stream, b"retained alternate stream").unwrap();
        let edit = apply(&path, Transform::Right).unwrap();
        assert_eq!(
            std::fs::read(&stream).unwrap(),
            b"retained alternate stream"
        );
        undo(edit).unwrap();
        assert_eq!(
            std::fs::read(&stream).unwrap(),
            b"retained alternate stream"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_and_hardlinks_cannot_redirect_atomic_edit() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("photo.png");
        image::RgbImage::new(3, 2).save(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let symbolic = temp.path().join("alias.png");
        symlink(&path, &symbolic).unwrap();
        assert!(apply(&symbolic, Transform::Right).is_err());
        let linked = temp.path().join("hard.png");
        std::fs::hard_link(&path, &linked).unwrap();
        assert!(apply(&path, Transform::Right).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(std::fs::read(&linked).unwrap(), bytes);
    }

    #[test]
    fn failed_new_edit_preserves_previous_undo_and_busy_serializes_requests() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("photo.png");
        image::RgbImage::new(3, 2).save(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let ctx = eframe::egui::Context::default();
        let mut edits = Edits::default();
        edits.start(path.clone(), Some(Transform::Right), &ctx);
        edits.start(path.clone(), Some(Transform::Right), &ctx);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while edits.busy() {
            edits.poll();
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(
            crate::file_io::load_image(&path).orientation,
            image::metadata::Orientation::Rotate90
        );
        edits.start(
            temp.path().join("missing.jpg"),
            Some(Transform::Right),
            &ctx,
        );
        while edits.busy() {
            edits.poll();
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(edits.error.is_some());
        assert!(edits.can_undo(&path));
        edits.error = None;
        edits.start(path.clone(), None, &ctx);
        while edits.busy() {
            edits.poll();
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}
fn apply(path: &Path, transform: Transform) -> Result<Undo> {
    let original = read_original(path)?;
    // Validate original too: do not convert damaged containers into plausible edits.
    crate::file_io::load_image(path)
        .image
        .context("Cannot decode original")?;
    let output = transformed(&original, transform)?;
    replace(path, &original, &output)?;
    Ok(Undo {
        path: path.to_path_buf(),
        original,
        expected: digest(&output),
    })
}
fn undo(edit: Undo) -> Result<()> {
    let current = read_original(&edit.path)?;
    if digest(&current) != edit.expected {
        bail!("Photo changed after rotation; undo will not overwrite it");
    }
    replace(&edit.path, &current, &edit.original)
}

type EditResult = Result<(PathBuf, Option<Undo>), String>;

#[derive(Default)]
pub(crate) struct Edits {
    pending: Option<mpsc::Receiver<EditResult>>,
    undo: Option<Undo>,
    pub error: Option<String>,
    recovery_probe: Option<mpsc::Receiver<(PathBuf, bool)>>,
    recovery_path: Option<(PathBuf, bool)>,
}
impl Edits {
    pub fn invalidate_recovery(&mut self) {
        self.recovery_path = None;
    }
    pub fn external_save_completed(&mut self) {
        self.undo = None;
        self.recovery_path = None;
    }
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn can_undo(&self, path: &Path) -> bool {
        (self.undo.as_ref().is_some_and(|u| u.path == path)
            || self
                .recovery_path
                .as_ref()
                .is_some_and(|(p, ok)| p == path && *ok))
            && !self.busy()
    }
    pub fn probe_recovery(&mut self, path: &Path, ctx: &eframe::egui::Context) {
        if let Some(rx) = &self.recovery_probe {
            if let Ok(result) = rx.try_recv() {
                self.recovery_path = Some(result);
                self.recovery_probe = None;
            }
        }
        if self.busy()
            || self.recovery_probe.is_some()
            || self.recovery_path.as_ref().is_some_and(|(p, _)| p == path)
        {
            return;
        }
        let (tx, rx) = mpsc::sync_channel(1);
        let path = path.to_owned();
        let ctx = ctx.clone();
        self.recovery_probe = Some(rx);
        std::thread::spawn(move || {
            let ok = super::recovery::available(&path);
            let _ = tx.send((path, ok));
            ctx.request_repaint();
        });
    }
    pub fn start(
        &mut self,
        path: PathBuf,
        transform: Option<Transform>,
        ctx: &eframe::egui::Context,
    ) {
        if self.busy() {
            return;
        }
        if transform.is_none() && !self.can_undo(&path) {
            return;
        }
        let undo = if transform.is_none() {
            self.undo.take()
        } else {
            None
        };
        let (tx, rx) = mpsc::sync_channel(1);
        self.pending = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(|| {
                if let Some(t) = transform {
                    apply(&path, t).map(|edit| (path, Some(edit)))
                } else {
                    undo.map_or_else(
                        || {
                            let current = read_original(&path)?;
                            let original = super::recovery::load(&path, &current)?;
                            replace(&path, &current, &original).map(|()| (path.clone(), None))
                        },
                        |edit| super::edits::undo(edit).map(|()| (path.clone(), None)),
                    )
                }
            })
            .unwrap_or_else(|_| Err(anyhow::anyhow!("Photo edit worker failed")))
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
    pub fn poll(&mut self) -> Option<PathBuf> {
        let result = self.pending.as_ref()?.try_recv();
        match result {
            Ok(result) => {
                self.pending = None;
                match result {
                    Ok((path, undo)) => {
                        self.recovery_path = Some((path.clone(), undo.is_some()));
                        self.undo = undo;
                        Some(path)
                    }
                    Err(e) => {
                        self.error = Some(e);
                        None
                    }
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.pending = None;
                self.error = Some("Photo edit worker disconnected".into());
                None
            }
            Err(mpsc::TryRecvError::Empty) => None,
        }
    }
}
