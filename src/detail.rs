//! On-demand native-pixel viewport over the fit texture. Decode/crop off UI;
//! never upload a texture larger than2048. Only one original is retained.
use eframe::egui;
use std::{path::PathBuf, sync::mpsc};
#[derive(Clone, PartialEq)]
struct Request {
    path: PathBuf,
    rect: [u32; 4],
}
struct Ready {
    request: Request,
    tiles: Vec<([u32; 4], egui::ColorImage)>,
}
type Tiles = Vec<([u32; 4], egui::TextureHandle)>;
pub(crate) struct Detail {
    input: mpsc::SyncSender<Request>,
    output: std::sync::Arc<std::sync::Mutex<Option<Ready>>>,
    requested: Option<Request>,
    ready: Option<(Request, Tiles)>,
}

#[cfg(test)]
#[test]
fn native_crop_retains_source_pixels_and_discards_old_viewports() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("wide.png");
    image::RgbImage::from_fn(9000, 4, |x, _| {
        image::Rgb([if x % 2 == 0 { 255 } else { 0 }, 0, 0])
    })
    .save(&path)
    .unwrap();
    let ctx = egui::Context::default();
    let detail = Detail::new(&ctx);
    let request = Request {
        path: path.clone(),
        rect: [8500, 0, 200, 4],
    };
    detail.input.send(request.clone()).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let ready = loop {
        if let Some(ready) = detail.output.lock().unwrap().take() {
            break ready;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    assert!(ready.request == request);
    assert_eq!(ready.tiles[0].1.size, [200, 4]);
    assert_eq!(ready.tiles[0].1.pixels[0], egui::Color32::RED);
    assert_eq!(ready.tiles[0].1.pixels[1], egui::Color32::BLACK);
    assert_eq!(image::image_dimensions(path).unwrap(), (9000, 4));
}
impl Detail {
    pub fn new(ctx: &egui::Context) -> Self {
        let (input, rx) = mpsc::sync_channel::<Request>(1);
        let output = std::sync::Arc::new(std::sync::Mutex::new(None));
        let ready = output.clone();
        let wake = ctx.clone();
        std::thread::spawn(move || {
            let mut current: Option<(PathBuf, image::DynamicImage)> = None;
            while let Ok(mut request) = rx.recv() {
                while let Ok(next) = rx.try_recv() {
                    request = next;
                }
                if current.as_ref().is_none_or(|(p, _)| *p != request.path) {
                    let loaded = crate::file_io::load_image(&request.path);
                    let Ok(mut image) = loaded.image else {
                        continue;
                    };
                    image.apply_orientation(loaded.orientation);
                    current = Some((request.path.clone(), image));
                }
                let image = &current.as_ref().unwrap().1;
                let [x, y, w, h] = request.rect;
                if x >= image.width() || y >= image.height() {
                    continue;
                }
                let right = x + w.min(image.width() - x);
                let bottom = y + h.min(image.height() - y);
                let mut tiles = Vec::new();
                // Tile rather than downscale on a high-DPI / >4K display.
                // Native 100% retains exactly one texel per source pixel.
                for top in (y..bottom).step_by(2048) {
                    for left in (x..right).step_by(2048) {
                        let rect = [
                            left,
                            top,
                            (right - left).min(2048),
                            (bottom - top).min(2048),
                        ];
                        let rgba = image
                            .crop_imm(rect[0], rect[1], rect[2], rect[3])
                            .into_rgba8();
                        let pixels = egui::ColorImage::from_rgba_unmultiplied(
                            [rgba.width() as usize, rgba.height() as usize],
                            rgba.as_raw(),
                        );
                        tiles.push((rect, pixels));
                    }
                }
                *ready.lock().unwrap_or_else(|e| e.into_inner()) = Some(Ready { request, tiles });
                wake.request_repaint();
            }
        });
        Self {
            input,
            output,
            requested: None,
            ready: None,
        }
    }
    pub fn paint(
        &mut self,
        ui: &egui::Ui,
        path: &std::path::Path,
        dimensions: [u32; 2],
        display: egui::Rect,
        clip: egui::Rect,
    ) {
        let visible = display.intersect(clip);
        if !visible.is_positive() {
            return;
        }
        let [w, h] = dimensions;
        let x = ((visible.left() - display.left()) / display.width() * w as f32)
            .floor()
            .max(0.0) as u32;
        let y = ((visible.top() - display.top()) / display.height() * h as f32)
            .floor()
            .max(0.0) as u32;
        let right = (((visible.right() - display.left()) / display.width() * w as f32).ceil()
            as u32)
            .min(w);
        let bottom = (((visible.bottom() - display.top()) / display.height() * h as f32).ceil()
            as u32)
            .min(h);
        let request = Request {
            path: path.to_owned(),
            rect: [
                x,
                y,
                right.saturating_sub(x).max(1),
                bottom.saturating_sub(y).max(1),
            ],
        };
        if self.requested.as_ref() != Some(&request) && self.input.try_send(request.clone()).is_ok()
        {
            self.requested = Some(request.clone());
        }
        if let Some(result) = self.output.lock().unwrap_or_else(|e| e.into_inner()).take() {
            if self.requested.as_ref() == Some(&result.request) {
                let tiles = result
                    .tiles
                    .into_iter()
                    .map(|(rect, pixels)| {
                        (
                            rect,
                            ui.ctx().load_texture(
                                "native_detail",
                                pixels,
                                egui::TextureOptions::LINEAR,
                            ),
                        )
                    })
                    .collect();
                self.ready = Some((result.request, tiles));
            }
        }
        if let Some((ready, tiles)) = &self.ready {
            if ready.path == path {
                for (rect, texture) in tiles {
                    let [x, y, rw, rh] = *rect;
                    let min = display.min
                        + egui::vec2(
                            x as f32 / w as f32 * display.width(),
                            y as f32 / h as f32 * display.height(),
                        );
                    let size = egui::vec2(
                        rw as f32 / w as f32 * display.width(),
                        rh as f32 / h as f32 * display.height(),
                    );
                    ui.painter_at(clip).image(
                        texture.id(),
                        egui::Rect::from_min_size(min, size),
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }
            }
        }
    }
}
