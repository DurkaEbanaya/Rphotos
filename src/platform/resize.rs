//! Explicit resized copies, not in-place editing. Metadata is deliberately not
//! copied: orientation is baked into pixels and the UI discloses metadata loss.
use anyhow::{bail, Context, Result};
use std::{io::Write, path::Path};

#[derive(Clone, Copy)]
pub(crate) struct Options {
    pub width: u32,
    pub height: u32,
    pub quality: u8,
    pub jpeg: bool,
}
impl Options {
    pub(crate) fn validate(self) -> Result<()> {
        if self.width == 0
            || self.height == 0
            || self.width > 16384
            || self.height > 16384
            || u64::from(self.width) * u64::from(self.height) > 48_000_000
        {
            bail!("Choose dimensions between 1 and 16384 pixels, at most 48 megapixels");
        }
        if !(10..=100).contains(&self.quality) {
            bail!("JPEG quality must be between 10 and 100");
        }
        Ok(())
    }
}

pub(crate) fn save(source: &Path, destination: &Path, options: Options) -> Result<()> {
    options.validate()?;
    let source = source.canonicalize()?;
    if destination.canonicalize().is_ok_and(|p| p == source) {
        bail!("Choose a new file; resizing never overwrites the original");
    }
    let extension = destination
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    if !(if options.jpeg {
        matches!(extension.as_str(), "jpg" | "jpeg")
    } else {
        extension == "png"
    }) {
        bail!("Choose a filename matching the selected JPEG/PNG format");
    }
    let before = std::fs::metadata(&source)?;
    let loaded = crate::file_io::load_image(&source);
    let mut image = loaded.image.context("Cannot decode photo")?;
    image.apply_orientation(loaded.orientation);
    let image = image.resize_exact(
        options.width,
        options.height,
        image::imageops::FilterType::Lanczos3,
    );
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    if options.jpeg {
        let rgb = super::printing::paper_rgb(image);
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut staged, options.quality).encode(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )?;
    } else {
        image.write_to(&mut staged, image::ImageFormat::Png)?;
    }
    staged.flush()?;
    staged.as_file().sync_all()?;
    let after = std::fs::metadata(&source)?;
    if before.len() != after.len() || before.modified()? != after.modified()? {
        bail!("Source changed during resizing; no copy was saved");
    }
    staged
        .persist_noclobber(destination)
        .map_err(|e| e.error)
        .context("Cannot save resized copy (choose a new filename)")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resized_copy_bakes_orientation_preserves_original_and_never_clobbers() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.png");
        image::RgbaImage::from_pixel(3, 2, image::Rgba([255, 0, 0, 0]))
            .save(&source)
            .unwrap();
        let before = std::fs::read(&source).unwrap();
        let output = temp.path().join("copy.jpg");
        let options = Options {
            width: 8,
            height: 12,
            quality: 90,
            jpeg: true,
        };
        save(&source, &output, options).unwrap();
        let decoded = image::open(&output).unwrap().to_rgb8();
        assert_eq!(decoded.dimensions(), (8, 12));
        assert!(decoded.get_pixel(0, 0).0.iter().all(|v| *v > 250));
        let copy = std::fs::read(&output).unwrap();
        assert!(save(&source, &output, options).is_err());
        assert_eq!(std::fs::read(&output).unwrap(), copy);
        assert!(save(
            &source,
            &source,
            Options {
                jpeg: false,
                ..options
            }
        )
        .is_err());
        assert_eq!(std::fs::read(&source).unwrap(), before);
        assert!(save(
            &source,
            &temp.path().join("bad.png"),
            Options {
                width: 0,
                ..options
            }
        )
        .is_err());
        assert!(save(
            &source,
            &temp.path().join("bad.png"),
            Options {
                width: 16384,
                height: 16384,
                jpeg: false,
                ..options
            }
        )
        .is_err());
        assert!(save(&source, &temp.path().join("wrong.png"), options).is_err());
    }

    #[test]
    fn oriented_png_copy_has_upright_pixels_and_no_double_orientation() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.png");
        image::RgbImage::from_fn(6, 4, |x, y| image::Rgb([x as u8 * 30, y as u8 * 50, 80]))
            .save(&source)
            .unwrap();
        let ctx = eframe::egui::Context::default();
        let mut edits = super::super::edits::Edits::default();
        edits.start(
            source.clone(),
            Some(super::super::edits::Transform::Right),
            &ctx,
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while edits.busy() {
            edits.poll();
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(edits.error.is_none());
        let before = std::fs::read(&source).unwrap();
        let loaded = crate::file_io::load_image(&source);
        let mut expected = loaded.image.unwrap();
        expected.apply_orientation(loaded.orientation);
        let output = temp.path().join("copy.png");
        save(
            &source,
            &output,
            Options {
                width: 4,
                height: 6,
                quality: 90,
                jpeg: false,
            },
        )
        .unwrap();
        let actual = crate::file_io::load_image(&output);
        assert_eq!(
            actual.orientation,
            image::metadata::Orientation::NoTransforms
        );
        assert_eq!(actual.image.unwrap(), expected);
        assert_eq!(std::fs::read(&source).unwrap(), before);
    }

    #[test]
    fn invalid_source_leaves_existing_destination_untouched() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("broken.jpg");
        let destination = temp.path().join("keep.png");
        std::fs::write(&source, b"not a photo").unwrap();
        std::fs::write(&destination, b"existing file").unwrap();
        assert!(save(
            &source,
            &destination,
            Options {
                width: 30,
                height: 20,
                quality: 90,
                jpeg: false
            }
        )
        .is_err());
        assert_eq!(std::fs::read(&destination).unwrap(), b"existing file");
        assert_eq!(std::fs::read(&source).unwrap(), b"not a photo");
    }
}
