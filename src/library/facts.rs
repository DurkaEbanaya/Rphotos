//! Bounded header-only EXIF date probe on the scanner thread, never pixel decode.
//! Metadata outside the first 4 MiB falls back to the file modification date.
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use chrono::TimeZone;
use exif::{In, Tag, Value};

pub(super) fn taken(path: &Path) -> Option<i64> {
    let file = std::fs::File::open(path).ok()?;
    let end = file.metadata().ok()?.len().min(4 * 1024 * 1024);
    let mut input = BufReader::new(Prefix {
        file,
        end,
        position: 0,
    });
    let exif = exif::Reader::new().read_from_container(&mut input).ok()?;
    let ascii = |tag| match &exif.get_field(tag, In::PRIMARY)?.value {
        Value::Ascii(values) => std::str::from_utf8(values.first()?).ok(),
        _ => None,
    };
    for (date, offset) in [
        (Tag::DateTimeOriginal, Tag::OffsetTimeOriginal),
        (Tag::DateTimeDigitized, Tag::OffsetTimeDigitized),
        (Tag::DateTime, Tag::OffsetTime),
    ] {
        if let Some(timestamp) = ascii(date).and_then(|date| parse_date(date, ascii(offset))) {
            return Some(timestamp);
        }
    }
    None
}

// Seekable bounded prefix: the EXIF reader stops at the relevant header instead
// of reading 4 MiB from every ordinary JPEG. TIFF offsets cannot escape the cap.
struct Prefix {
    file: std::fs::File,
    end: u64,
    position: u64,
}
impl Read for Prefix {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let len = out
            .len()
            .min(self.end.saturating_sub(self.position) as usize);
        let read = self.file.read(&mut out[..len])?;
        self.position += read as u64;
        Ok(read)
    }
}
impl Seek for Prefix {
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
        let next = match from {
            SeekFrom::Start(n) => i128::from(n),
            SeekFrom::Current(n) => i128::from(self.position) + i128::from(n),
            SeekFrom::End(n) => i128::from(self.end) + i128::from(n),
        };
        if next < 0 || next > i128::from(self.end) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "EXIF offset exceeds header limit",
            ));
        }
        self.position = self.file.seek(SeekFrom::Start(next as u64))?;
        Ok(self.position)
    }
}

fn parse_date(date: &str, offset: Option<&str>) -> Option<i64> {
    let date = date.trim_matches(|c: char| c == '\0' || c.is_whitespace());
    let naive = chrono::NaiveDateTime::parse_from_str(date, "%Y:%m:%d %H:%M:%S").ok()?;
    if let Some(offset) = offset {
        let offset = offset.trim_matches(|c: char| c == '\0' || c.is_whitespace());
        // A malformed explicit offset must not silently change the interpretation.
        return chrono::DateTime::parse_from_str(
            &format!("{} {offset}", naive.format("%Y-%m-%d %H:%M:%S")),
            "%Y-%m-%d %H:%M:%S %:z",
        )
        .ok()
        .map(|date| date.timestamp());
    }
    chrono::Local
        .from_local_datetime(&naive)
        .earliest()
        .map(|date| date.timestamp())
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use image::{ExtendedColorType, ImageEncoder};

    pub(crate) fn jpeg(path: &Path) {
        // TIFF IFD points at the EXIF IFD; its date ASCII starts at byte 44.
        let mut exif = b"II\x2a\0\x08\0\0\0\x01\0".to_vec();
        exif.extend_from_slice(&[0x69, 0x87, 4, 0, 1, 0, 0, 0, 26, 0, 0, 0]);
        exif.extend_from_slice(&[0; 4]);
        exif.extend_from_slice(&[1, 0]);
        exif.extend_from_slice(&[0x03, 0x90, 2, 0, 20, 0, 0, 0, 44, 0, 0, 0]);
        exif.extend_from_slice(&[0; 4]);
        exif.extend_from_slice(b"2015:06:12 10:20:30\0");
        let mut encoder =
            image::codecs::jpeg::JpegEncoder::new(std::fs::File::create(path).unwrap());
        encoder.set_exif_metadata(exif).unwrap();
        encoder
            .write_image(&[90; 18], 3, 2, ExtendedColorType::Rgb8)
            .unwrap();
    }

    #[test]
    fn dates_offsets_invalid_values_and_header_only_jpeg() {
        assert_eq!(
            parse_date("2015:06:12 10:20:30", Some("+03:00")),
            Some(
                chrono::DateTime::parse_from_rfc3339("2015-06-12T07:20:30Z")
                    .unwrap()
                    .timestamp()
            )
        );
        assert!(parse_date("0000:00:00 00:00:00", None).is_none());
        assert!(parse_date("2015:02:30 10:20:30", None).is_none());
        assert!(parse_date("2015:06:12 10:20:30", Some("wrong")).is_none());
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("dated.jpg");
        jpeg(&path);
        let original = std::fs::read(&path).unwrap();
        assert_eq!(taken(&path), parse_date("2015:06:12 10:20:30", None));
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let broken = temp.path().join("broken.jpg");
        std::fs::write(&broken, b"not an image").unwrap();
        assert!(taken(&broken).is_none());
        let mut prefix = Prefix {
            file: std::fs::File::open(&path).unwrap(),
            end: 8,
            position: 0,
        };
        assert!(prefix.seek(SeekFrom::Start(9)).is_err());
        assert!(prefix.seek(SeekFrom::Current(-1)).is_err());
        assert_eq!(prefix.seek(SeekFrom::End(-2)).unwrap(), 6);
        let mut bytes = [0; 10];
        assert_eq!(prefix.read(&mut bytes).unwrap(), 2);
        assert_eq!(prefix.read(&mut bytes).unwrap(), 0);
    }
}
