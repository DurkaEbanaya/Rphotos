//! Optional full sensor development through the installed LibRaw C API.
//! No downloads or bundled proprietary camera SDKs. A handle is worker-local.
use anyhow::{bail, Context, Result};
use std::{ffi::c_void, path::Path};

#[repr(C)]
struct Processed {
    kind: i32,
    height: u16,
    width: u16,
    colors: u16,
    bits: u16,
    size: u32,
    data: [u8; 1],
}

pub(crate) fn decode(path: &Path) -> Result<image::DynamicImage> {
    // Some LibRaw builds retain process-global codec state. Do not unload a
    // native decoder while another worker is entering it; serialize development
    // and keep the library resident for the process lifetime.
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
    static LIBRARY: std::sync::OnceLock<Option<libloading::Library>> = std::sync::OnceLock::new();
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    #[cfg(target_os = "linux")]
    let names = ["libraw_r.so.25", "libraw_r.so.23", "libraw_r.so"];
    #[cfg(target_os = "macos")]
    let names = [
        "/opt/homebrew/lib/libraw_r.dylib",
        "/usr/local/lib/libraw_r.dylib",
        "libraw_r.dylib",
    ];
    #[cfg(target_os = "windows")]
    let names = ["libraw.dll", "raw.dll", "libraw_r.dll"];
    let library = LIBRARY
        .get_or_init(|| {
            for name in names {
                // SAFETY: standard installed LibRaw C ABI; no app-supplied code paths.
                if let Ok(value) = unsafe { libloading::Library::new(name) } {
                    return Some(value);
                }
            }
            None
        })
        .as_ref()
        .context("Full RAW decoding needs an installed LibRaw shared library")?;
    let bytes = std::fs::read(path)?;
    // SAFETY: symbols have the documented LibRaw C ABI. `bytes` and `library`
    // outlive the handle. Processed memory is copied before clear/close.
    unsafe {
        let init = library.get::<unsafe extern "C" fn(u32) -> *mut c_void>(b"libraw_init\0")?;
        let open = library.get::<unsafe extern "C" fn(*mut c_void, *const c_void, usize) -> i32>(
            b"libraw_open_buffer\0",
        )?;
        let unpack = library.get::<unsafe extern "C" fn(*mut c_void) -> i32>(b"libraw_unpack\0")?;
        let process =
            library.get::<unsafe extern "C" fn(*mut c_void) -> i32>(b"libraw_dcraw_process\0")?;
        let output =
            library.get::<unsafe extern "C" fn(*mut c_void, i32)>(b"libraw_set_output_color\0")?;
        let depth =
            library.get::<unsafe extern "C" fn(*mut c_void, i32)>(b"libraw_set_output_bps\0")?;
        let make = library.get::<unsafe extern "C" fn(*mut c_void, *mut i32) -> *mut Processed>(
            b"libraw_dcraw_make_mem_image\0",
        )?;
        let clear =
            library.get::<unsafe extern "C" fn(*mut Processed)>(b"libraw_dcraw_clear_mem\0")?;
        let close = library.get::<unsafe extern "C" fn(*mut c_void)>(b"libraw_close\0")?;
        let handle = init(0);
        if handle.is_null() {
            bail!("Cannot allocate LibRaw decoder");
        }
        let result = (|| -> Result<_> {
            let code = open(handle, bytes.as_ptr().cast(), bytes.len());
            if code != 0 {
                bail!("LibRaw open failed ({code})");
            }
            let code = unpack(handle);
            if code != 0 {
                bail!("LibRaw unpack failed ({code})");
            }
            output(handle, 1);
            depth(handle, 8);
            let code = process(handle);
            if code != 0 {
                bail!("LibRaw development failed ({code})");
            }
            let mut code = 0;
            let pointer = make(handle, &mut code);
            if pointer.is_null() {
                bail!("LibRaw output failed ({code})");
            }
            let result = (|| -> Result<_> {
                let p = &*pointer;
                let expected = usize::from(p.width) * usize::from(p.height) * 3;
                if code != 0
                    || p.kind != 2
                    || p.colors != 3
                    || p.bits != 8
                    || expected == 0
                    || expected != p.size as usize
                {
                    bail!("Unsupported LibRaw output layout");
                }
                let pixels = std::slice::from_raw_parts(p.data.as_ptr(), expected).to_vec();
                Ok(image::DynamicImage::ImageRgb8(
                    image::RgbImage::from_raw(p.width.into(), p.height.into(), pixels)
                        .context("Invalid RAW output dimensions")?,
                ))
            })();
            clear(pointer);
            result
        })();
        close(handle);
        result
    }
}

#[cfg(test)]
#[test]
fn rejects_non_raw_without_modifying_it() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("not-raw.dng");
    std::fs::write(&path, b"not a raw file").unwrap();
    assert!(decode(&path).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"not a raw file");
}

#[cfg(test)]
#[test]
fn develops_generated_sensor_dng_without_a_jpeg_preview_when_libraw_is_installed() {
    // A complete, original 64x64 Bayer DNG fixture, not a downloaded camera file.
    // Runtime decoder availability is optional, just like the production path.
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("sensor.dng");
    let mut tags: Vec<(u16, u16, u32, Vec<u8>)> = Vec::new();
    let mut scalar =
        |tag, kind, value: u32| tags.push((tag, kind, 1, value.to_le_bytes().to_vec()));
    for (tag, kind, value) in [
        (256, 4, 64),
        (257, 4, 64),
        (258, 3, 16),
        (259, 3, 1),
        (262, 3, 32803),
        (273, 4, 0),
        (277, 3, 1),
        (278, 4, 64),
        (279, 4, 8192),
        (284, 3, 1),
        (50714, 4, 0),
        (50717, 4, 65535),
        (50778, 3, 21),
    ] {
        scalar(tag, kind, value);
    }
    for (tag, kind, count, bytes) in [
        (271, 2, 8, b"Rphotos\0".to_vec()),
        (272, 2, 10, b"Synthetic\0".to_vec()),
        (33421, 3, 2, vec![2, 0, 2, 0]),
        (33422, 1, 4, vec![0, 1, 1, 2]),
        (50706, 1, 4, vec![1, 4, 0, 0]),
        (50707, 1, 4, vec![1, 1, 0, 0]),
        (50708, 2, 18, b"Rphotos synthetic\0".to_vec()),
    ] {
        tags.push((tag, kind, count, bytes));
    }
    let matrix = [10000_i32, 0, 0, 0, 10000, 0, 0, 0, 10000]
        .into_iter()
        .flat_map(|n| n.to_le_bytes().into_iter().chain(10000_i32.to_le_bytes()))
        .collect();
    tags.push((50721, 10, 9, matrix));
    tags.sort_by_key(|t| t.0);
    let base = 8 + 2 + tags.len() * 12 + 4;
    let mut extra = Vec::new();
    let mut entries = Vec::new();
    for (tag, kind, count, mut bytes) in tags {
        let data = if bytes.len() > 4 {
            if extra.len() % 2 != 0 {
                extra.push(0);
            }
            let offset = base + extra.len();
            extra.extend(bytes);
            (offset as u32).to_le_bytes().to_vec()
        } else {
            bytes.resize(4, 0);
            bytes
        };
        entries.push((tag, kind, count, data));
    }
    let mut bytes = b"II*\0".to_vec();
    bytes.extend(8_u32.to_le_bytes());
    bytes.extend((entries.len() as u16).to_le_bytes());
    for (tag, kind, count, data) in entries {
        bytes.extend(tag.to_le_bytes());
        bytes.extend(kind.to_le_bytes());
        bytes.extend(count.to_le_bytes());
        bytes.extend(if tag == 273 {
            ((base + extra.len()) as u32).to_le_bytes().to_vec()
        } else {
            data
        });
    }
    bytes.extend([0; 4]);
    bytes.extend(extra);
    for i in 0..4096 {
        bytes.extend((10000 + (i % 64) * 600_u16).to_le_bytes());
    }
    std::fs::write(&path, &bytes).unwrap();
    match decode(&path) {
        Ok(image) => {
            assert_eq!(image.width(), 64);
            assert_eq!(image.height(), 64);
            assert!(image.to_rgb8().pixels().any(|p| p.0 != [0; 3]));
        }
        Err(error) if error.to_string().contains("installed LibRaw") => {
            eprintln!("Optional LibRaw is unavailable; sensor decode not executed")
        }
        Err(error) => panic!("Generated DNG failed: {error:#}"),
    }
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}
