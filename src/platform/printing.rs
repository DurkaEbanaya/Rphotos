//! Offline photo-to-PDF rendering and explicit native printing adapters.
//! Rendering never alters originals. A job is submitted only after the native dialog.
use anyhow::{bail, Context, Result};
use std::path::Path;

#[derive(Clone, Copy)]
pub(crate) struct Page {
    pub width: f64,
    pub height: f64,
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
}

impl Default for Page {
    fn default() -> Self {
        Self {
            width: 210.0,
            height: 297.0,
            left: 10.0,
            right: 10.0,
            top: 10.0,
            bottom: 10.0,
        }
    }
}

/// Minimal PDF 1.4, one JPEG XObject per page. Binary lengths/xref offsets are
/// counted in bytes. Full oriented pixels are used, not the viewer's GPU texture.
pub(crate) fn pdf(paths: &[std::path::PathBuf], page: Page) -> Result<Vec<u8>> {
    if paths.is_empty() {
        bail!("No photos selected");
    }
    if paths.len() > 100 {
        bail!("Select at most 100 photos per PDF job");
    }
    if ![
        page.width,
        page.height,
        page.left,
        page.right,
        page.top,
        page.bottom,
    ]
    .iter()
    .all(|v| v.is_finite() && *v >= 0.0)
        || page.width <= page.left + page.right
        || page.height <= page.top + page.bottom
    {
        bail!("Invalid paper dimensions or margins");
    }
    let pt = 72.0 / 25.4;
    let w = page.width * pt;
    let h = page.height * pt;
    let mut objects: Vec<Vec<u8>> = vec![b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(), Vec::new()];
    let mut kids = String::new();
    for path in paths {
        let loaded = crate::file_io::load_image(path);
        let mut image = loaded
            .image
            .with_context(|| format!("Cannot decode {}", path.display()))?;
        image.apply_orientation(loaded.orientation);
        // Printing at up to ~600 dpi on A4. Cap memory/output on huge images.
        if image.width() > 7000 || image.height() > 7000 {
            image = image.thumbnail(7000, 7000);
        }
        let rgb = paper_rgb(image);
        let (iw, ih) = rgb.dimensions();
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95).encode(
            rgb.as_raw(),
            iw,
            ih,
            image::ExtendedColorType::Rgb8,
        )?;
        let scale = ((w - (page.left + page.right) * pt) / iw as f64)
            .min((h - (page.top + page.bottom) * pt) / ih as f64);
        let dw = iw as f64 * scale;
        let dh = ih as f64 * scale;
        let x = page.left * pt + (w - (page.left + page.right) * pt - dw) / 2.0;
        let y = page.bottom * pt + (h - (page.top + page.bottom) * pt - dh) / 2.0;
        let id = objects.len() + 1;
        let image_id = id + 1;
        let content_id = id + 2;
        kids.push_str(&format!("{id} 0 R "));
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w:.4} {h:.4}] /Resources << /XObject << /Photo {image_id} 0 R >> >> /Contents {content_id} 0 R >>").into_bytes());
        let mut image_obj=format!("<< /Type /XObject /Subtype /Image /Width {iw} /Height {ih} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",jpeg.len()).into_bytes();
        image_obj.extend(jpeg);
        image_obj.extend(b"\nendstream");
        objects.push(image_obj);
        if objects.iter().map(|o| o.len()).sum::<usize>() > 128 * 1024 * 1024 {
            bail!("PDF job exceeds the 128 MiB output budget; select fewer photos");
        }
        let content = format!("q {dw:.4} 0 0 {dh:.4} {x:.4} {y:.4} cm /Photo Do Q\n");
        objects.push(
            format!(
                "<< /Length {} >>\nstream\n{content}endstream",
                content.len()
            )
            .into_bytes(),
        );
    }
    objects[1] = format!("<< /Type /Pages /Count {} /Kids [{kids}] >>", paths.len()).into_bytes();
    let mut output = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, obj) in objects.iter().enumerate() {
        offsets.push(output.len());
        output.extend(format!("{} 0 obj\n", i + 1).as_bytes());
        output.extend(obj);
        output.extend(b"\nendobj\n");
    }
    let start = output.len();
    output.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for offset in offsets {
        output.extend(format!("{offset:010} 00000 n \n").as_bytes());
    }
    output.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    Ok(output)
}

#[cfg(target_os = "linux")]
pub(crate) fn print(paths: &[std::path::PathBuf]) -> Result<()> {
    validate_job(paths)?;
    use ashpd::desktop::print::{Orientation, PrintProxy};
    pollster::block_on(async {
        let proxy = PrintProxy::new()
            .await
            .context("Print portal is unavailable")?;
        let prepared = proxy
            .prepare_print(
                None,
                "Rphotos",
                Default::default(),
                Default::default(),
                None,
                true,
            )
            .await?;
        let prepared = match prepared.response() {
            Ok(value) => value,
            Err(ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled)) => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        let setup = prepared.page_setup;
        let mut page = Page {
            width: setup.width.unwrap_or(210.0),
            height: setup.height.unwrap_or(297.0),
            left: setup.margin_left.unwrap_or(10.0),
            right: setup.margin_right.unwrap_or(10.0),
            top: setup.margin_top.unwrap_or(10.0),
            bottom: setup.margin_bottom.unwrap_or(10.0),
        };
        if matches!(
            setup.orientation,
            Some(Orientation::Landscape | Orientation::ReverseLandscape)
        ) {
            std::mem::swap(&mut page.width, &mut page.height);
        }
        let bytes = pdf(paths, page)?;
        let mut file = tempfile::tempfile()?;
        use std::io::{Seek, SeekFrom, Write};
        file.write_all(&bytes)?;
        file.seek(SeekFrom::Start(0))?;
        let request = proxy
            .print(None, "Rphotos", &file, Some(prepared.token), true)
            .await?;
        match request.response() {
            Ok(()) => {}
            Err(ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled)) => {}
            Err(error) => return Err(error.into()),
        }
        Ok(())
    })
}

fn validate_job(paths: &[std::path::PathBuf]) -> Result<()> {
    if paths.is_empty() || paths.len() > 100 {
        bail!("Select between1 and100 photos to print");
    }
    for path in paths {
        std::fs::File::open(path).with_context(|| format!("Cannot read {}", path.display()))?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub(crate) fn print(paths: &[std::path::PathBuf]) -> Result<()> {
    validate_job(paths)?;
    use std::io::Write;
    let mut file = tempfile::Builder::new().suffix(".pdf").tempfile()?;
    file.write_all(&pdf(paths, Page::default())?)?;
    file.as_file().sync_all()?;
    // PDFKit's native print operation runs on the helper's main thread. No
    // silent lp submission: the standard print panel is always shown.
    let script = r#"ObjC.import('AppKit'); ObjC.import('PDFKit');
function run(argv) {
  $.NSApplication.sharedApplication;
  var url=$.NSURL.fileURLWithPath(argv[0]);
  var doc=$.PDFDocument.alloc.initWithURL(url);
  if (!doc) throw Error('Cannot read print PDF');
  var op=doc.printOperationForPrintInfoScalingModeAutoRotate($.NSPrintInfo.sharedPrintInfo,0,true);
  if (!op) throw Error('Cannot create print operation');
  op.setShowsPrintPanel(true); op.setShowsProgressPanel(true); op.runOperation;
}"#;
    let status = std::process::Command::new("/usr/bin/osascript")
        .args(["-l", "JavaScript", "-e", script, "--"])
        .arg(file.path())
        .status()?;
    if !status.success() {
        bail!("Native macOS print panel failed");
    }
    Ok(())
}

#[cfg(target_os = "windows")]
pub(crate) fn print(paths: &[std::path::PathBuf]) -> Result<()> {
    validate_job(paths)?;
    // Windows PowerShell/WPF owns the STA and the native printer chooser.
    // The helper reads private JSON, not interpolated filenames or shell text.
    let directory = tempfile::tempdir()?;
    let mut images = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        let loaded = crate::file_io::load_image(path);
        let mut image = loaded.image.context("Cannot decode print photo")?;
        image.apply_orientation(loaded.orientation);
        if image.width() > 7000 || image.height() > 7000 {
            image = image.thumbnail(7000, 7000);
        }
        let output = directory.path().join(format!("{index}.png"));
        paper_rgb(image).save(&output)?;
        images.push(output);
    }
    let manifest = directory.path().join("photos.json");
    std::fs::write(&manifest, serde_json::to_vec(&images)?)?;
    let script = r#"param([string]$Manifest)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName PresentationFramework,PresentationCore,WindowsBase,ReachFramework,System.Printing
$paths=Get-Content -Raw -LiteralPath $Manifest | ConvertFrom-Json
$dialog=New-Object System.Windows.Controls.PrintDialog
if ($dialog.ShowDialog() -ne $true) { exit 0 }
$document=New-Object System.Windows.Documents.FixedDocument
foreach ($path in $paths) {
 $page=New-Object System.Windows.Documents.FixedPage
 $page.Width=$dialog.PrintableAreaWidth; $page.Height=$dialog.PrintableAreaHeight
 $bitmap=New-Object System.Windows.Media.Imaging.BitmapImage
 $bitmap.BeginInit(); $bitmap.CacheOption=[System.Windows.Media.Imaging.BitmapCacheOption]::OnLoad
 $bitmap.UriSource=[Uri]$path; $bitmap.EndInit(); $bitmap.Freeze()
 $image=New-Object System.Windows.Controls.Image
 $image.Source=$bitmap; $image.Width=$page.Width; $image.Height=$page.Height
 $image.Stretch=[System.Windows.Media.Stretch]::Uniform
 [void]$page.Children.Add($image)
 $content=New-Object System.Windows.Documents.PageContent
 ([System.Windows.Markup.IAddChild]$content).AddChild($page)
 [void]$document.Pages.Add($content)
}
$dialog.PrintDocument($document.DocumentPaginator,'Rphotos')
"#;
    let helper = directory.path().join("print.ps1");
    std::fs::write(&helper, script)?;
    let status = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-STA",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(helper)
        .arg(manifest)
        .status()?;
    if !status.success() {
        bail!("Native Windows print dialog failed");
    }
    Ok(())
}

pub(crate) fn export(paths: &[std::path::PathBuf], path: &Path) -> Result<()> {
    // The save dialog can select an existing image (or a symlink to it).
    // Export must never replace any original, even after explicit overwrite.
    if path.exists() {
        let destination = path.canonicalize()?;
        if paths
            .iter()
            .any(|source| source.canonicalize().is_ok_and(|p| p == destination))
        {
            bail!("Choose a PDF destination different from the original photos");
        }
    }
    if !path
        .extension()
        .is_some_and(|s| s.eq_ignore_ascii_case("pdf"))
    {
        bail!("The export destination must have a .pdf extension");
    }
    let bytes = pdf(paths, Page::default())?;
    // Atomic replacement: a failed render/write must not truncate an existing PDF.
    let parent = path.parent().context("No output folder")?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    std::io::Write::write_all(&mut file, &bytes)?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}

pub(super) fn paper_rgb(image: image::DynamicImage) -> image::RgbImage {
    if !image.color().has_alpha() {
        return image.to_rgb8();
    }
    let rgba = image.into_rgba8();
    image::RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let p = rgba.get_pixel(x, y);
        let a = p[3] as u32;
        image::Rgb([0, 1, 2].map(|c| ((p[c] as u32 * a + 255 * (255 - a) + 127) / 255) as u8))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_rejects_original_and_failed_render_preserves_destination() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("photo.png");
        image::RgbImage::new(12, 8).save(&source).unwrap();
        let original = std::fs::read(&source).unwrap();
        assert!(export(std::slice::from_ref(&source), &source).is_err());
        assert_eq!(std::fs::read(&source).unwrap(), original);
        let output = temp.path().join("output.pdf");
        std::fs::write(&output, b"old PDF").unwrap();
        assert!(export(&[temp.path().join("missing.png")], &output).is_err());
        assert_eq!(std::fs::read(&output).unwrap(), b"old PDF");
        export(&[source], &output).unwrap();
        assert!(std::fs::read(output).unwrap().starts_with(b"%PDF"));
    }
    #[test]
    fn transparent_pixels_print_on_white() {
        let rgba = image::RgbaImage::from_pixel(2, 1, image::Rgba([0, 0, 0, 0]));
        let rgb = paper_rgb(rgba.into());
        assert_eq!(rgb.get_pixel(0, 0).0, [255, 255, 255]);
    }
    #[test]
    fn pdf_offsets_pages_and_originals() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("photo.png");
        image::RgbImage::new(120, 80).save(&path).unwrap();
        let original = std::fs::read(&path).unwrap();
        let bytes = pdf(&[path.clone(), path.clone()], Page::default()).unwrap();
        assert!(bytes.starts_with(b"%PDF-1.4"));
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/Count 2"));
        let start = text
            .rsplit("startxref\n")
            .next()
            .unwrap()
            .split('\n')
            .next()
            .unwrap()
            .parse::<usize>()
            .unwrap();
        assert_eq!(&bytes[start..start + 4], b"xref");
        let xref = std::str::from_utf8(&bytes[start..]).unwrap();
        for (i, line) in xref.lines().skip(3).take(8).enumerate() {
            let offset = line[..10].parse::<usize>().unwrap();
            assert!(bytes[offset..].starts_with(format!("{} 0 obj", i + 1).as_bytes()));
        }
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(pdf(&[], Page::default()).is_err());
        assert!(pdf(
            &[path],
            Page {
                width: 0.0,
                ..Default::default()
            }
        )
        .is_err());
        if let Some(dir) = std::env::var_os("RPHOTOS_CAPTURE_DIR") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(std::path::PathBuf::from(dir).join("print-test.pdf"), bytes).unwrap();
        }
    }
}
