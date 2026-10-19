//! Photos-style single-photo scene. The decoder/cache remain owned by Pane.

use eframe::egui::{self, RichText};

use crate::{
    library::{ui::Tokens, Appearance, Language},
    pane::Pane,
    settings::AppSettings,
};

#[derive(Default)]
pub(crate) struct Viewer {
    resize: Option<Resize>,
    edits: crate::platform::edits::Edits,
    transform_requested: Option<crate::platform::edits::Transform>,
    undo_requested: bool,
    overflow_open_last_frame: bool,
    jobs: crate::platform::jobs::Jobs,
    share_open: bool,
    pub info_open: bool,
    zoom_open: bool,
    settings_open: bool,
    delete_path: Option<std::path::PathBuf>,
    slideshow_at: Option<f64>,
    image_rect: Option<egui::Rect>,
    zoom_anchor: Option<egui::Rect>,
}

struct Resize {
    path: std::path::PathBuf,
    original: [u32; 2],
    width: u32,
    height: u32,
    linked: bool,
    quality: u8,
    jpeg: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{kittest::Queryable, Harness};

    fn fixture() -> (tempfile::TempDir, Pane) {
        let temp = tempfile::tempdir().unwrap();
        let mut paths = Vec::new();
        for n in 0..3 {
            let path = temp.path().join(format!("Фото-{n}.png"));
            image::RgbImage::from_fn(960, 600, |x, y| {
                image::Rgb([(x % 256) as u8, (y % 256) as u8, 80 + n * 40])
            })
            .save(&path)
            .unwrap();
            paths.push(path);
        }
        let ctx = egui::Context::default();
        let mut pane = Pane::new(&ctx, 2, 64, 1, false, true, 0);
        pane.open_library(paths, 0, &ctx);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while pane.current_texture.is_none() {
            pane.poll_cache();
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        (temp, pane)
    }

    fn harness(pane: Pane) -> Harness<'static, (Viewer, Pane, AppSettings, Action)> {
        Harness::builder()
            .with_size(egui::vec2(1000.0, 760.0))
            .build_state(
                |ctx, (viewer, pane, settings, action)| {
                    pane.poll_cache();
                    *action = viewer.show(ctx, pane, settings, false);
                },
                (
                    Viewer::default(),
                    pane,
                    AppSettings::default(),
                    Action::None,
                ),
            )
    }

    #[test]
    fn narrow_overflow_scroll_reaches_settings() {
        let (_temp, pane) = fixture();
        let mut h = harness(pane);
        h.set_size(egui::vec2(500.0, 700.0));
        h.run_steps(3);
        h.get_by_label("…").click();
        h.run_steps(3);
        h.input_mut().events.extend([
            egui::Event::PointerMoved(egui::pos2(430.0, 350.0)),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -1200.0),
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        h.run_steps(15);
        h.get_by_label("Настройки").click();
        h.run_steps(1);
        assert_eq!(h.state().3, Action::Settings);
        assert_eq!(
            h.state().1.current_index,
            0,
            "wheel over menu must not navigate photo"
        );
    }

    #[test]
    fn resize_modal_presets_formats_cancel_and_navigation_guard() {
        let (_temp, pane) = fixture();
        let path = pane.image_paths[0].clone();
        let before = std::fs::read(&path).unwrap();
        let mut h = harness(pane);
        h.run_steps(3);
        h.get_by_label("…").click();
        h.run_steps(2);
        h.get_by_label("Изменить размер…").click();
        h.run_steps(2);
        assert!(h.state().0.resize.is_some());
        h.get_by_label("50%").click();
        h.run_steps(2);
        let resize = h.state().0.resize.as_ref().unwrap();
        assert_eq!((resize.width, resize.height), (480, 300));
        h.get_by_label("JPEG").click();
        h.run_steps(2);
        assert!(h.state().0.resize.as_ref().unwrap().jpeg);
        h.press_key(egui::Key::ArrowRight);
        h.run_steps(2);
        assert_eq!(h.state().1.current_index, 0);
        h.get_by_label("Отмена").click();
        h.run_steps(2);
        assert!(h.state().0.resize.is_none());
        assert_eq!(std::fs::read(path).unwrap(), before);
    }

    #[test]
    fn rotate_flip_and_undo_reload_pixels() {
        let (_temp, pane) = fixture();
        let path = pane.image_paths[0].clone();
        let original = std::fs::read(&path).unwrap();
        let mut h = harness(pane);
        h.run_steps(3);
        assert!(!h.get_by_label("Повернуть вправо").is_disabled());
        h.get_by_label("Повернуть вправо").click();
        h.run_steps(1);
        if h.state().0.edits.busy() {
            h.press_key(egui::Key::ArrowRight);
            h.run_steps(1);
            assert_eq!(
                h.state().1.current_index,
                0,
                "navigation must not race a save"
            );
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while h.state().0.edits.busy()
            || h.state()
                .1
                .current_record
                .as_ref()
                .and_then(|r| r.dimensions)
                != Some([600, 960])
        {
            h.run_steps(2);
            std::thread::sleep(std::time::Duration::from_millis(5));
            assert!(
                std::time::Instant::now() < deadline,
                "{:?}",
                h.state().0.edits.error
            );
        }
        assert_eq!(h.state().1.current_index, 0);
        h.get_by_label("…").click();
        h.run_steps(2);
        assert!(!h.get_by_label("Показать в проводнике").is_disabled());
        h.get_by_label("Отразить по горизонтали").click();
        h.run_steps(1);
        while h.state().0.edits.busy() {
            h.run_steps(2);
            std::thread::sleep(std::time::Duration::from_millis(5));
            assert!(std::time::Instant::now() < deadline);
        }
        h.run_steps(3);
        h.get_by_label("…").click();
        h.run_steps(2);
        h.get_by_label("Отменить последний поворот / отражение")
            .click();
        h.run_steps(1);
        while h.state().0.edits.busy() {
            h.run_steps(2);
            std::thread::sleep(std::time::Duration::from_millis(5));
            assert!(std::time::Instant::now() < deadline);
        }
        assert_eq!(
            crate::file_io::load_image(&path).orientation,
            image::metadata::Orientation::Rotate90
        );
        while h.state().1.current_texture.is_none() {
            h.run_steps(2);
            std::thread::sleep(std::time::Duration::from_millis(5));
            assert!(std::time::Instant::now() < deadline);
        }
        h.run_steps(3);
        h.get_by_label("…").click();
        h.run_steps(2);
        assert!(!h.get_by_label("Повернуть влево").is_disabled());
        h.get_by_label("Повернуть влево").click();
        h.run_steps(1);
        while h.state().0.edits.busy() {
            h.run_steps(2);
            std::thread::sleep(std::time::Duration::from_millis(5));
            assert!(std::time::Instant::now() < deadline);
        }
        assert_eq!(
            crate::file_io::load_image(&path).orientation,
            image::metadata::Orientation::NoTransforms
        );
        assert_eq!(
            image::load_from_memory(&std::fs::read(&path).unwrap()).unwrap(),
            image::load_from_memory(&original).unwrap()
        );
    }

    #[test]
    fn photo_commands_edges_and_delete_confirmation() {
        let (_temp, pane) = fixture();
        let mut h = harness(pane);
        h.run_steps(3);
        h.get_by_label("Следующая фотография").click();
        h.run_steps(2);
        assert_eq!(
            h.state().1.current_index,
            1,
            "edge click must not be eaten by image"
        );
        h.get_by_label("Предыдущая фотография").click();
        h.run_steps(2);
        assert_eq!(h.state().1.current_index, 0);
        h.press_key(egui::Key::ArrowRight);
        h.run_steps(2);
        assert_eq!(
            h.state().1.current_index,
            1,
            "keyboard remains active after focusing an edge button"
        );
        h.press_key(egui::Key::ArrowLeft);
        h.run_steps(2);
        assert_eq!(h.state().1.current_index, 0);
        h.press_key(egui::Key::I);
        h.run_steps(2);
        assert!(h.state().0.info_open);
        h.get_by_label("Масштаб").click();
        h.run_steps(2);
        assert!(h.state().0.zoom_open);
        h.get_by_label("Удалить в корзину").click();
        h.run_steps(2);
        assert!(h.state().0.delete_path.is_some());
        h.get_by_label("Следующая фотография").click();
        h.run_steps(2);
        assert_eq!(h.state().1.current_index, 0, "modal blocks navigation");
        h.get_by_label("Отмена").click();
        h.run_steps(2);
        assert!(h.state().0.delete_path.is_none());
        assert!(h.state().1.image_paths[0].exists());
    }

    #[test]
    fn overflow_closes_before_escape_can_leave_and_blocks_navigation() {
        let (_temp, pane) = fixture();
        let mut h = harness(pane);
        h.run_steps(3);
        h.get_by_label("…").click();
        h.run_steps(3);
        assert!(h.state().0.overflow_open_last_frame);
        assert!(h.query_by_label("Сохранить копию…").is_some());
        assert!(h.query_by_label("Открыть с помощью…").is_some());
        h.press_key(egui::Key::End);
        h.run_steps(2);
        assert_eq!(h.state().1.current_index, 0);
        h.press_key(egui::Key::Escape);
        h.run_steps(2);
        assert_eq!(h.state().3, Action::None);
        assert!(!h.state().0.overflow_open_last_frame);
        assert!(h.query_by_label("Сохранить копию…").is_none());
        // Do not send another Escape here: it is the real leave-viewer action.
        assert!(h.state().1.image_paths[0].exists());
    }

    #[test]
    fn measured_viewport_and_zoom_popup_do_not_reflow_photo() {
        let (_temp, pane) = fixture();
        let mut h = harness(pane);
        h.run_steps(3);
        let before = h.state().0.image_rect.unwrap();
        assert_eq!(before.min, egui::pos2(32.0, 80.0));
        assert_eq!(before.max, egui::pos2(968.0, 728.0));
        let anchor = h.state().0.zoom_anchor.unwrap();
        assert!((anchor.center().x - 401.0).abs() < 0.01);
        assert_eq!(anchor.height(), 48.0);
        h.get_by_label("Масштаб").click();
        h.run_steps(3);
        assert!(h.state().0.zoom_open);
        assert_eq!(
            h.state().0.image_rect,
            Some(before),
            "flyout must not resize the photo viewport"
        );
        h.press_key(egui::Key::Escape);
        h.run_steps(2);
        assert!(!h.state().0.zoom_open);
        assert_eq!(
            h.state().3,
            Action::None,
            "Escape closes zoom before leaving viewer"
        );
        h.press_key(egui::Key::I);
        h.run_steps(3);
        let info = h.state().0.image_rect.unwrap();
        assert!(
            (info.left() - 376.0).abs() < 0.1,
            "info occupies exactly 344 logical units: {info:?}"
        );
        h.get_by_label("Назад в библиотеку").click();
        h.run_steps(1);
        assert_eq!(h.state().3, Action::Back);
    }

    #[test]
    fn normalized_zoom_is_bounded_and_round_trips() {
        assert_eq!(zoom_from_position(0.0), 1.0);
        assert_eq!(zoom_from_position(100.0), 20.0);
        assert_eq!(zoom_position(0.5), 0.0);
        for p in [0.0, 10.0, 25.0, 50.0, 100.0] {
            assert!((zoom_position(zoom_from_position(p)) - p).abs() < 0.001);
        }
    }

    #[test]
    fn fullscreen_hides_title_commands_and_info() {
        let (_temp, pane) = fixture();
        let mut h = harness(pane);
        h.state_mut().0.info_open = true;
        h.input_mut()
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .unwrap()
            .fullscreen = Some(true);
        h.run_steps(3);
        assert_eq!(h.state().0.image_rect.unwrap().min, egui::pos2(32.0, 0.0));
        assert!(h.query_by_label("Назад в библиотеку").is_none());
    }

    #[test]
    fn wheel_only_navigates_over_photo_and_ctrl_wheel_zooms() {
        let (_temp, pane) = fixture();
        let mut h = harness(pane);
        h.run_steps(3);
        let wheel = || egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -120.0),
            modifiers: egui::Modifiers::NONE,
        };
        h.input_mut()
            .events
            .extend([egui::Event::PointerMoved(egui::pos2(500.0, 400.0)), wheel()]);
        h.run_steps(2);
        assert_eq!(h.state().1.current_index, 1);
        h.input_mut()
            .events
            .extend([egui::Event::PointerMoved(egui::pos2(500.0, 20.0)), wheel()]);
        h.run_steps(2);
        assert_eq!(
            h.state().1.current_index,
            1,
            "toolbar scrolling cannot navigate"
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while h.state().1.current_texture.is_none() {
            h.run_steps(2);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        h.run_steps(60); // Let the previous unmodified wheel's smooth tail settle.
        h.input_mut().modifiers = egui::Modifiers::CTRL | egui::Modifiers::COMMAND;
        h.input_mut().events.extend([
            egui::Event::PointerMoved(egui::pos2(500.0, 400.0)),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 120.0),
                modifiers: egui::Modifiers::CTRL | egui::Modifiers::COMMAND,
            },
        ]);
        h.run_steps(2);
        assert_eq!(h.state().1.current_index, 1);
        assert!(
            h.state().1.zoom > 1.0,
            "zoom = {}, texture = {}",
            h.state().1.zoom,
            h.state().1.current_texture.is_some()
        );
    }

    #[test]
    fn two_finger_pinch_zooms_and_translates_without_photo_navigation() {
        let (_temp, pane) = fixture();
        let mut h = harness(pane);
        h.run_steps(3);
        // Navigation wheel mode must not disable pinch zoom.
        h.state_mut().2.mouse_wheel_zoom = false;
        h.input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(400.0, 400.0)));
        h.run_steps(1);
        let touch = |id, phase, x, y| egui::Event::Touch {
            device_id: egui::TouchDeviceId(1),
            id: egui::TouchId(id),
            phase,
            pos: egui::pos2(x, y),
            force: None,
        };
        h.input_mut().events.extend([
            touch(1, egui::TouchPhase::Start, 400.0, 400.0),
            touch(2, egui::TouchPhase::Start, 600.0, 400.0),
        ]);
        h.run_steps(1);
        h.input_mut().events.extend([
            touch(1, egui::TouchPhase::Move, 350.0, 430.0),
            touch(2, egui::TouchPhase::Move, 650.0, 430.0),
        ]);
        h.run_steps(1);
        assert!(
            h.state().1.zoom > 1.4,
            "pinch not applied: {}",
            h.state().1.zoom
        );
        assert!(h.state().1.pan.y.abs() > 1.0, "centroid movement must pan");
        h.input_mut().events.extend([
            touch(1, egui::TouchPhase::End, 350.0, 430.0),
            touch(2, egui::TouchPhase::End, 650.0, 430.0),
        ]);
        h.run_steps(2);
        assert_eq!(h.state().1.current_index, 0);
        assert_eq!(h.state().3, Action::None);
    }

    #[test]
    fn touch_swipe_at_fit_navigates_but_zoomed_drag_pans() {
        let (_temp, pane) = fixture();
        let mut h = harness(pane);
        h.run_steps(3);
        let swipe = |h: &mut Harness<'static, (Viewer, Pane, AppSettings, Action)>| {
            let a = egui::pos2(650.0, 400.0);
            let b = egui::pos2(450.0, 410.0);
            let touch = |phase, pos| egui::Event::Touch {
                device_id: egui::TouchDeviceId(1),
                id: egui::TouchId(1),
                phase,
                pos,
                force: None,
            };
            h.input_mut().events.extend([
                touch(egui::TouchPhase::Start, a),
                egui::Event::PointerMoved(a),
                egui::Event::PointerButton {
                    pos: a,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
            h.run_steps(1);
            h.input_mut().events.extend([
                touch(egui::TouchPhase::Move, b),
                egui::Event::PointerMoved(b),
            ]);
            h.run_steps(1);
            h.input_mut().events.extend([
                touch(egui::TouchPhase::End, b),
                egui::Event::PointerButton {
                    pos: b,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
            h.run_steps(2);
        };
        swipe(&mut h);
        assert_eq!(h.state().1.current_index, 1);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while h.state().1.current_texture.is_none() {
            h.run_steps(2);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        h.state_mut().1.set_zoom(2.0);
        swipe(&mut h);
        assert_eq!(h.state().1.current_index, 1);
        assert!(h.state().1.pan.x < -100.0);
    }

    #[test]
    #[ignore = "requires RPHOTOS_CAPTURE_DIR and offscreen GPU"]
    fn capture_viewer() {
        let output = std::path::PathBuf::from(
            std::env::var_os("RPHOTOS_CAPTURE_DIR").expect("set RPHOTOS_CAPTURE_DIR"),
        );
        std::fs::create_dir_all(&output).unwrap();
        for (name, size, lang, appearance, info) in [
            (
                "viewer-light",
                [1440.0, 900.0],
                Language::Ru,
                Appearance::Light,
                false,
            ),
            (
                "viewer-info",
                [1440.0, 900.0],
                Language::Ru,
                Appearance::Light,
                true,
            ),
            (
                "viewer-narrow",
                [500.0, 700.0],
                Language::En,
                Appearance::Light,
                false,
            ),
            (
                "viewer-zoom",
                [1500.0, 960.0],
                Language::En,
                Appearance::Light,
                false,
            ),
            (
                "viewer-dark",
                [1100.0, 760.0],
                Language::Ru,
                Appearance::Dark,
                false,
            ),
            (
                "viewer-rotated",
                [1440.0, 900.0],
                Language::Ru,
                Appearance::Light,
                false,
            ),
            (
                "viewer-transform-menu",
                [1500.0, 960.0],
                Language::Ru,
                Appearance::Light,
                false,
            ),
            (
                "viewer-resize",
                [1100.0, 850.0],
                Language::Ru,
                Appearance::Light,
                false,
            ),
            (
                "viewer-narrow-menu",
                [500.0, 700.0],
                Language::En,
                Appearance::Light,
                false,
            ),
            (
                "viewer-resize-narrow",
                [500.0, 700.0],
                Language::Ru,
                Appearance::Light,
                false,
            ),
        ] {
            let (_temp, mut pane) = fixture();
            if name == "viewer-rotated" {
                let ctx = egui::Context::default();
                let mut edits = crate::platform::edits::Edits::default();
                edits.start(
                    pane.image_paths[0].clone(),
                    Some(crate::platform::edits::Transform::Right),
                    &ctx,
                );
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                while edits.busy() {
                    edits.poll();
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    assert!(std::time::Instant::now() < deadline);
                }
                assert!(edits.error.is_none());
                pane.current_record = Some(crate::file_io::load_image(&pane.image_paths[0]).record);
            }
            let settings = AppSettings {
                language: lang,
                appearance,
                ..Default::default()
            };
            let viewer = Viewer {
                info_open: info,
                zoom_open: name == "viewer-zoom",
                ..Default::default()
            };
            let mut h = Harness::builder()
                .with_size(egui::vec2(size[0], size[1]))
                .wgpu()
                .build_state(
                    |ctx, (viewer, pane, settings, uploaded)| {
                        ctx.data_mut(|d| {
                            d.insert_temp(egui::Id::new("rphotos.viewer.favorite"), true)
                        });
                        // The fixture's decode context is not the harness renderer's
                        // texture registry. Upload synthetic pixels into this context.
                        if !*uploaded {
                            let loaded = crate::file_io::load_image(&pane.image_paths[0]);
                            pane.current_texture = Some(ctx.load_texture(
                                "synthetic-photo",
                                crate::decode::image_to_color_image(
                                    loaded.image.unwrap(),
                                    loaded.orientation,
                                ),
                                egui::TextureOptions::LINEAR,
                            ));
                            *uploaded = true;
                        }
                        viewer.show(ctx, pane, settings, false);
                    },
                    (viewer, pane, settings, false),
                );
            h.run_steps(5);
            if matches!(
                name,
                "viewer-transform-menu"
                    | "viewer-narrow-menu"
                    | "viewer-resize"
                    | "viewer-resize-narrow"
            ) {
                h.get_by_label("…").click();
                h.run_steps(3);
                if matches!(name, "viewer-resize" | "viewer-resize-narrow") {
                    h.get_by_label("Изменить размер…").click();
                    h.run_steps(3);
                    h.get_by_label("50%").click();
                    h.run_steps(2);
                }
            }
            h.render()
                .unwrap()
                .save(output.join(format!("{name}.png")))
                .unwrap();
        }
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum Action {
    Edited(std::path::PathBuf),
    Favorite,
    Back,
    AddToAlbum,
    Trash(std::path::PathBuf),
    Compare,
    Settings,
    None,
}

#[derive(Clone, Copy)]
enum Icon {
    Plus,
    Zoom,
    Trash,
    Favorite,
    Rotate,
    Crop,
    Edit,
    Share,
    Print,
    Close,
    Fullscreen,
}

fn icon_button(ui: &mut egui::Ui, icon: Icon, label: &str, size: egui::Vec2) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    let painter = ui.painter_at(rect);
    if response.is_pointer_button_down_on() {
        painter.rect_filled(rect, 0.0, crate::reveal::ACCENT);
    }
    let center = rect.center();
    let stroke = egui::Stroke::new(1.0_f32, ui.visuals().text_color());
    let p = |x, y| center + egui::vec2(x, y);
    let line = |a, b| {
        painter.line_segment([a, b], stroke);
    };
    match icon {
        Icon::Plus => {
            line(p(-8.0, 0.0), p(8.0, 0.0));
            line(p(0.0, -8.0), p(0.0, 8.0));
        }
        Icon::Zoom => {
            painter.circle_stroke(p(-2.0, -2.0), 6.0, stroke);
            line(p(3.0, 3.0), p(9.0, 9.0));
            line(p(-5.0, -2.0), p(1.0, -2.0));
            line(p(-2.0, -5.0), p(-2.0, 1.0));
        }
        Icon::Trash => {
            painter.rect_stroke(
                egui::Rect::from_min_max(p(-6.0, -5.0), p(6.0, 8.0)),
                0.0,
                stroke,
                egui::StrokeKind::Inside,
            );
            line(p(-9.0, -7.0), p(9.0, -7.0));
            line(p(-3.0, -10.0), p(3.0, -10.0));
            line(p(-2.0, -2.0), p(-2.0, 5.0));
            line(p(2.0, -2.0), p(2.0, 5.0));
        }
        Icon::Favorite => {
            let favorite = ui.ctx().data(|d| {
                d.get_temp::<bool>(egui::Id::new("rphotos.viewer.favorite"))
                    .unwrap_or(false)
            });
            crate::library::icons::paint(
                &painter,
                egui::Rect::from_center_size(center, egui::vec2(24.0, 24.0)),
                crate::library::icons::Icon::Heart,
                if favorite {
                    egui::Color32::from_rgb(220, 35, 55)
                } else {
                    stroke.color
                },
                favorite,
            );
        }
        Icon::Rotate => {
            let points = (0..=18)
                .map(|n| {
                    let a = std::f32::consts::PI + n as f32 / 18.0 * std::f32::consts::PI * 1.5;
                    p(a.cos() * 7.0, a.sin() * 7.0)
                })
                .collect();
            painter.add(egui::Shape::line(points, stroke));
            line(p(0.0, 7.0), p(-4.0, 3.0));
            line(p(0.0, 7.0), p(4.0, 3.0));
        }
        Icon::Crop => {
            line(p(-5.0, -9.0), p(-5.0, 5.0));
            line(p(-5.0, 5.0), p(9.0, 5.0));
            line(p(-9.0, -5.0), p(5.0, -5.0));
            line(p(5.0, -5.0), p(5.0, 9.0));
            line(p(-8.0, 8.0), p(8.0, -8.0));
        }
        Icon::Edit => {
            painter.add(egui::Shape::closed_line(
                vec![
                    p(-8.0, 4.0),
                    p(3.0, -7.0),
                    p(7.0, -3.0),
                    p(-4.0, 8.0),
                    p(-8.0, 8.0),
                ],
                stroke,
            ));
            line(p(0.0, -4.0), p(4.0, 0.0));
            line(p(-7.0, -8.0), p(8.0, 7.0));
        }
        Icon::Share => {
            painter.add(egui::Shape::line(
                vec![p(-8.0, -3.0), p(-8.0, 7.0), p(7.0, 7.0), p(7.0, 2.0)],
                stroke,
            ));
            painter.add(egui::Shape::line(
                vec![p(-4.0, 2.0), p(0.0, -4.0), p(7.0, -4.0)],
                stroke,
            ));
            line(p(3.0, -8.0), p(7.0, -4.0));
            line(p(3.0, 0.0), p(7.0, -4.0));
        }
        Icon::Print => {
            painter.rect_stroke(
                egui::Rect::from_min_max(p(-8.0, -3.0), p(8.0, 5.0)),
                0.0,
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.rect_stroke(
                egui::Rect::from_min_max(p(-4.0, -9.0), p(4.0, -3.0)),
                0.0,
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.rect_filled(
                egui::Rect::from_min_max(p(-4.0, 1.0), p(4.0, 9.0)),
                0.0,
                ui.visuals().panel_fill,
            );
            painter.rect_stroke(
                egui::Rect::from_min_max(p(-4.0, 1.0), p(4.0, 9.0)),
                0.0,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        Icon::Close => {
            line(p(-6.0, -6.0), p(6.0, 6.0));
            line(p(-6.0, 6.0), p(6.0, -6.0));
        }
        Icon::Fullscreen => {
            line(p(-8.0, 8.0), p(8.0, -8.0));
            line(p(0.0, -8.0), p(8.0, -8.0));
            line(p(8.0, 0.0), p(8.0, -8.0));
            line(p(-8.0, 0.0), p(-8.0, 8.0));
            line(p(0.0, 8.0), p(-8.0, 8.0));
        }
    }
    response.on_hover_text(label)
}

fn zoom_position(zoom: f32) -> f32 {
    (zoom.max(1.0).ln() / 20.0_f32.ln() * 100.0).clamp(0.0, 100.0)
}

// Fixed-width command slots must not inherit a right-to-left label layout:
// that lets long translated labels paint into adjacent commands.
fn command_label(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    width: f32,
    hint: &str,
) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 48.0), egui::Sense::hover());
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.set_clip_rect(rect.intersect(ui.clip_rect()));
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add_enabled_ui(false, |ui| {
                icon_button(ui, icon, hint, egui::vec2(32.0, 48.0));
            });
            let text_rect = egui::Rect::from_min_max(
                rect.min + egui::vec2(32.0, 0.0),
                rect.max - egui::vec2(4.0, 0.0),
            );
            ui.painter().with_clip_rect(text_rect).text(
                text_rect.left_center(),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::proportional(12.0),
                ui.visuals().weak_text_color(),
            );
        },
    );
    let response = ui.interact(rect, ui.id().with(label), egui::Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), hint));
    response.on_hover_text(hint)
}

fn zoom_from_position(position: f32) -> f32 {
    20.0_f32.powf(position.clamp(0.0, 100.0) / 100.0)
}

fn can_transform(pane: &Pane) -> bool {
    pane.current_texture.is_some()
        && pane
            .current_record
            .as_ref()
            .is_some_and(|r| matches!(r.format.as_deref(), Some("JPEG" | "PNG")))
}

impl Viewer {
    pub(crate) fn start_slideshow(&mut self, ctx: &egui::Context) {
        self.slideshow_at = Some(ctx.input(|i| i.time) + 4.0);
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
    }
    pub fn modal_open(&self) -> bool {
        self.resize.is_some()
            || self.edits.busy()
            || self.edits.error.is_some()
            || self.delete_path.is_some()
            || self.settings_open
            || self.share_open
            || self.jobs.message.is_some()
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        pane: &mut Pane,
        settings: &mut AppSettings,
        blocked: bool,
    ) -> Action {
        let lang = settings.language;
        self.jobs.poll();
        let tokens = Tokens::new(settings.appearance, ctx);
        tokens.apply(ctx);
        let before = settings.clone();
        let mut action = if let Some(path) = self.edits.poll() {
            pane.reload_edited(&path, ctx);
            Action::Edited(path)
        } else {
            Action::None
        };
        let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
        let now = ctx.input(|i| i.time);
        let disabled = blocked || self.modal_open();
        let mut direction = 0;
        let title = pane
            .image_paths
            .get(pane.current_index)
            .map(|path| {
                format!(
                    "Rphotos - {}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                )
            })
            .unwrap_or_else(|| "Rphotos".into());
        if crate::chrome::show(
            ctx,
            &title,
            Some(lang.text("Назад в библиотеку", "Back to library")),
            settings.appearance,
            lang,
            disabled,
        ) {
            action = Action::Back;
        }

        let overflow_was_open = std::mem::take(&mut self.overflow_open_last_frame);
        let text_input = ctx.wants_keyboard_input();
        if !disabled && !overflow_was_open && !ctx.memory(|m| m.any_popup_open()) {
            ctx.input(|i| {
                if i.key_pressed(egui::Key::Escape) {
                    if fullscreen {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                        self.slideshow_at = None;
                    } else if self.zoom_open {
                        self.zoom_open = false;
                    } else if self.slideshow_at.take().is_none() {
                        action = Action::Back;
                    }
                }
                if i.key_pressed(egui::Key::ArrowRight)
                    && !(self.zoom_open && ctx.wants_keyboard_input())
                {
                    direction = 1;
                }
                if i.key_pressed(egui::Key::ArrowLeft)
                    && !(self.zoom_open && ctx.wants_keyboard_input())
                {
                    direction = -1;
                }
                if i.key_pressed(egui::Key::Home) {
                    pane.jump_background(0, ctx);
                }
                if i.key_pressed(egui::Key::End) {
                    pane.jump_background(pane.image_paths.len().saturating_sub(1), ctx);
                }
                if i.key_pressed(egui::Key::F11) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
                }
                if i.key_pressed(egui::Key::I) {
                    self.info_open = !self.info_open;
                }
                if !text_input && i.modifiers.command {
                    if i.key_pressed(egui::Key::R) && !self.jobs.busy() && can_transform(pane) {
                        self.transform_requested = Some(if i.modifiers.shift {
                            crate::platform::edits::Transform::Left
                        } else {
                            crate::platform::edits::Transform::Right
                        });
                    }
                    if i.key_pressed(egui::Key::Z)
                        && !self.jobs.busy()
                        && pane
                            .image_paths
                            .get(pane.current_index)
                            .is_some_and(|p| self.edits.can_undo(p))
                    {
                        self.undo_requested = true;
                    }
                }
                if i.events.iter().any(|e| {
                    matches!(
                        e,
                        egui::Event::Key {
                            key: egui::Key::Delete,
                            pressed: true,
                            repeat: false,
                            ..
                        }
                    )
                }) {
                    self.delete_path = pane.image_paths.get(pane.current_index).cloned();
                }
                if !settings.mouse_wheel_zoom
                    && !i.modifiers.command
                    && i.zoom_delta() == 1.0
                    && self.image_rect.is_some_and(|rect| {
                        i.pointer.hover_pos().is_some_and(|pos| rect.contains(pos))
                    })
                {
                    let delta: f32 = i
                        .events
                        .iter()
                        .filter_map(|e| match e {
                            egui::Event::MouseWheel { delta, .. } => Some(delta.y),
                            _ => None,
                        })
                        .sum();
                    if delta.abs() >= 1.0 {
                        direction = if delta > 0.0 { -1 } else { 1 };
                    }
                }
            });
        }

        // Fullscreen has no commands; only the exit affordance remains over the image.
        let reveal = !fullscreen;
        if reveal {
            egui::TopBottomPanel::top("photo_commands")
                .exact_height(48.0)
                .frame(egui::Frame::default().fill(tokens.background))
                .show(ctx, |ui| {
                    if disabled {
                        ui.disable();
                    }
                    let width = ui.available_width();
                    let narrow = width < 900.0;
                    let bar = ui.max_rect();
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(egui::Rect::from_min_size(bar.min, egui::vec2(108.0, 48.0)))
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            if icon_button(
                                ui,
                                Icon::Plus,
                                lang.text("Добавить в альбом", "Add to album"),
                                egui::vec2(36.0, 48.0),
                            )
                            .clicked()
                            {
                                action = Action::AddToAlbum;
                            }
                            if ui
                                .add(
                                    egui::Button::new(lang.text("Добавить в", "Add to"))
                                        .frame(false)
                                        .min_size(egui::vec2(48.0, 48.0)),
                                )
                                .clicked()
                            {
                                action = Action::AddToAlbum;
                            }
                        },
                    );
                    let cluster = egui::Rect::from_min_size(
                        egui::pos2(bar.center().x - 123.75, bar.top()),
                        egui::vec2(247.5, 48.0),
                    );
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(cluster)
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            let zoom = icon_button(
                                ui,
                                Icon::Zoom,
                                lang.text("Масштаб", "Zoom"),
                                egui::vec2(49.5, 48.0),
                            );
                            self.zoom_anchor = Some(zoom.rect);
                            if zoom.clicked() {
                                self.zoom_open = !self.zoom_open;
                            }
                            if icon_button(
                                ui,
                                Icon::Trash,
                                lang.text("Удалить в корзину", "Move to Trash"),
                                egui::vec2(49.5, 48.0),
                            )
                            .clicked()
                            {
                                self.delete_path =
                                    pane.image_paths.get(pane.current_index).cloned();
                                self.zoom_open = false;
                            }
                            let favorite = ctx.data(|d| {
                                d.get_temp::<bool>(egui::Id::new("rphotos.viewer.favorite"))
                                    .unwrap_or(false)
                            });
                            let response = icon_button(
                                ui,
                                Icon::Favorite,
                                if favorite {
                                    lang.text("Убрать из избранного", "Remove from favorites")
                                } else {
                                    lang.text("Добавить в избранное", "Add to favorites")
                                },
                                egui::vec2(49.5, 48.0),
                            );
                            if response.clicked() {
                                action = Action::Favorite;
                            }
                            ui.add_enabled_ui(!self.jobs.busy() && can_transform(pane), |ui| {
                                if icon_button(
                                    ui,
                                    Icon::Rotate,
                                    lang.text("Повернуть вправо", "Rotate right"),
                                    egui::vec2(49.5, 48.0),
                                )
                                .clicked()
                                {
                                    self.transform_requested =
                                        Some(crate::platform::edits::Transform::Right);
                                }
                            });
                            ui.add_enabled_ui(false, |ui| {
                                icon_button(
                                    ui,
                                    Icon::Crop,
                                    lang.text(
                                        "Обрезка — редактор отложен",
                                        "Crop — editor deferred",
                                    ),
                                    egui::vec2(49.5, 48.0),
                                );
                            });
                        },
                    );
                    let right_width = if narrow { 112.0 } else { 317.0 };
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(egui::Rect::from_min_max(
                                egui::pos2(bar.right() - right_width, bar.top()),
                                bar.max,
                            ))
                            .layout(egui::Layout::right_to_left(egui::Align::Center)),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.style_mut().spacing.button_padding = egui::vec2(12.0, 0.0);
                            ui.style_mut().spacing.interact_size = egui::vec2(48.0, 48.0);
                            ui.menu_button("…", |ui| {
                                self.overflow_open_last_frame = true;
                                egui::ScrollArea::vertical()
                                    .max_height((ctx.screen_rect().height() - 100.0).max(100.0))
                                    .show(ui, |ui| {
                                        ui.set_min_width(180.0);
                                        ui.style_mut().spacing.interact_size.y = 32.0;
                                        ui.style_mut().spacing.button_padding =
                                            egui::vec2(12.0, 6.0);
                                        ui.add_enabled_ui(
                                            !self.jobs.busy() && can_transform(pane),
                                            |ui| {
                                                for (transform, label) in [
                                        (
                                            crate::platform::edits::Transform::Left,
                                            lang.text("Повернуть влево", "Rotate left"),
                                        ),
                                        (
                                            crate::platform::edits::Transform::Horizontal,
                                            lang.text("Отразить по горизонтали", "Flip horizontal"),
                                        ),
                                        (
                                            crate::platform::edits::Transform::Vertical,
                                            lang.text("Отразить по вертикали", "Flip vertical"),
                                        ),
                                    ] {
                                        if ui.button(label).clicked() {
                                            self.transform_requested = Some(transform);
                                            ui.close_menu();
                                        }
                                    }
                                            },
                                        );
                                        if ui
                                            .add_enabled(
                                                pane.image_paths
                                                    .get(pane.current_index)
                                                    .is_some_and(|p| self.edits.can_undo(p))
                                                    && !self.jobs.busy(),
                                                egui::Button::new(lang.text(
                                                    "Отменить последний поворот / отражение",
                                                    "Undo last rotate / flip",
                                                )),
                                            )
                                            .clicked()
                                        {
                                            self.undo_requested = true;
                                            ui.close_menu();
                                        }
                                        ui.separator();
                                        if ui
                                            .add_enabled(
                                                !self.jobs.busy() && pane.current_texture.is_some(),
                                                egui::Button::new(
                                                    lang.text("Изменить размер…", "Resize…"),
                                                ),
                                            )
                                            .clicked()
                                        {
                                            if let (Some(path), Some(dimensions)) = (
                                                pane.image_paths.get(pane.current_index),
                                                pane.current_record
                                                    .as_ref()
                                                    .and_then(|r| r.dimensions),
                                            ) {
                                                self.resize = Some(Resize {
                                                    path: path.clone(),
                                                    original: dimensions,
                                                    width: dimensions[0],
                                                    height: dimensions[1],
                                                    linked: true,
                                                    quality: 90,
                                                    jpeg: false,
                                                });
                                                self.slideshow_at = None;
                                            }
                                            ui.close_menu();
                                        }
                                        if ui
                                            .add_enabled(
                                                !self.jobs.busy(),
                                                egui::Button::new(lang.text(
                                                    "Показать в проводнике",
                                                    "Show in file manager",
                                                )),
                                            )
                                            .clicked()
                                        {
                                            if let Some(path) =
                                                pane.image_paths.get(pane.current_index)
                                            {
                                                self.jobs.start(
                                                    crate::platform::jobs::Job::Reveal(
                                                        path.clone(),
                                                    ),
                                                    ctx,
                                                );
                                            }
                                            ui.close_menu();
                                        }
                                        if narrow {
                                            if ui
                                                .button(lang.text("Поделиться…", "Share…"))
                                                .clicked()
                                            {
                                                self.share_open = true;
                                                ui.close_menu();
                                            }
                                            if ui
                                                .add_enabled(
                                                    !self.jobs.busy(),
                                                    egui::Button::new(
                                                        lang.text("Печать…", "Print…"),
                                                    ),
                                                )
                                                .clicked()
                                            {
                                                if let Some(path) =
                                                    pane.image_paths.get(pane.current_index)
                                                {
                                                    self.jobs.start(
                                                        crate::platform::jobs::Job::Print(vec![
                                                            path.clone(),
                                                        ]),
                                                        ctx,
                                                    );
                                                }
                                                ui.close_menu();
                                            }
                                            ui.separator();
                                        }
                                        if ui
                                            .button(lang.text("Показ слайдов", "Slideshow"))
                                            .clicked()
                                        {
                                            self.slideshow_at = Some(now + 4.0);
                                            self.zoom_open = false;
                                            ctx.send_viewport_cmd(
                                                egui::ViewportCommand::Fullscreen(true),
                                            );
                                            ui.close_menu();
                                        }
                                        ui.separator();
                                        for (label, kind) in [
                                            (lang.text("Сохранить копию…", "Save a copy…"), 0),
                                            (lang.text("Копировать файл", "Copy file"), 1),
                                            (lang.text("Открыть с помощью…", "Open with…"), 2),
                                        ] {
                                            if ui
                                                .add_enabled(
                                                    !self.jobs.busy()
                                                        && (kind != 2 || cfg!(target_os = "linux")),
                                                    egui::Button::new(label),
                                                )
                                                .clicked()
                                            {
                                                if let Some(path) =
                                                    pane.image_paths.get(pane.current_index)
                                                {
                                                    use crate::platform::jobs::Job;
                                                    let job = match kind {
                                                        0 => Job::SaveCopy(path.clone()),
                                                        1 => Job::Copy(vec![path.clone()]),
                                                        _ => Job::OpenWith(path.clone()),
                                                    };
                                                    self.jobs.start(job, ctx);
                                                }
                                                ui.close_menu();
                                            }
                                        }
                                        ui.separator();
                                        if ui
                                            .button(lang.text("По размеру окна", "Fit to window"))
                                            .clicked()
                                        {
                                            pane.fit_view();
                                            ui.close_menu();
                                        }
                                        if ui
                                            .button(lang.text(
                                                "Фактический размер · 100%",
                                                "Actual size · 100%",
                                            ))
                                            .clicked()
                                        {
                                            if let Some(rect) = self.image_rect {
                                                pane.actual_size(rect, ctx.pixels_per_point());
                                            }
                                            ui.close_menu();
                                        }
                                        if ui
                                            .button(
                                                lang.text("Сведения о файле · I", "File info · I"),
                                            )
                                            .clicked()
                                        {
                                            self.info_open = !self.info_open;
                                            ui.close_menu();
                                        }
                                        if ui
                                            .button(
                                                lang.text(
                                                    "Сравнить два снимка",
                                                    "Compare two photos",
                                                ),
                                            )
                                            .clicked()
                                        {
                                            action = Action::Compare;
                                            ui.close_menu();
                                        }
                                        if ui.button(lang.text("Настройки", "Settings")).clicked()
                                        {
                                            action = Action::Settings;
                                            ui.close_menu();
                                        }
                                    });
                            });
                            if !narrow {
                                ui.add_enabled_ui(!self.jobs.busy(), |ui| {
                                    if icon_button(
                                        ui,
                                        Icon::Print,
                                        lang.text("Печать…", "Print…"),
                                        egui::vec2(48.0, 48.0),
                                    )
                                    .clicked()
                                    {
                                        if let Some(path) = pane.image_paths.get(pane.current_index)
                                        {
                                            self.jobs.start(
                                                crate::platform::jobs::Job::Print(vec![
                                                    path.clone()
                                                ]),
                                                ctx,
                                            );
                                        }
                                    }
                                    if command_label(
                                        ui,
                                        Icon::Share,
                                        lang.text("Поделиться", "Share"),
                                        82.0,
                                        lang.text("Поделиться…", "Share…"),
                                    )
                                    .clicked()
                                    {
                                        self.share_open = true;
                                    }
                                });
                            }
                            ui.add_enabled_ui(false, |ui| {
                                if narrow {
                                    icon_button(
                                        ui,
                                        Icon::Edit,
                                        lang.text("Редактирование отложено", "Editor deferred"),
                                        egui::vec2(48.0, 48.0),
                                    );
                                } else {
                                    command_label(
                                        ui,
                                        Icon::Edit,
                                        lang.text("Редактирование", "Edit & Create"),
                                        139.0,
                                        lang.text("Редактирование отложено", "Editor deferred"),
                                    );
                                }
                            });
                        },
                    );
                });
        }
        if self.info_open && !fullscreen {
            self.info_panel(ctx, pane, lang, &tokens, disabled);
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(tokens.background))
            .show(ctx, |ui| {
                if disabled {
                    ui.disable();
                }
                let rect = ui.available_rect_before_wrap();
                let image = egui::Rect::from_min_max(
                    rect.min + egui::vec2(32.0, 0.0),
                    rect.max - egui::vec2(32.0, 32.0),
                );
                self.image_rect = Some(image);
                let mut image_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(image)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                image_ui.set_clip_rect(image);
                pane.show_content(&mut image_ui);
                let edge = (rect.width() * 0.12).clamp(48.0, 100.0);
                // Overlay the large click zones after the image's drag response.
                for (forward, enabled) in [
                    (false, pane.can_navigate_backward()),
                    (true, pane.can_navigate_forward()),
                ] {
                    if !enabled {
                        continue;
                    }
                    let x = if forward {
                        rect.right() - edge
                    } else {
                        rect.left()
                    };
                    let zone = egui::Rect::from_min_max(
                        egui::pos2(x, rect.top() + 48.0),
                        egui::pos2(x + edge, rect.bottom() - 64.0),
                    );
                    let response = ui.interact(
                        zone,
                        ui.id().with(("photo_edge", forward)),
                        egui::Sense::click(),
                    );
                    let label = if forward {
                        lang.text("Следующая фотография", "Next photo")
                    } else {
                        lang.text("Предыдущая фотография", "Previous photo")
                    };
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
                    });
                    if response.clicked() {
                        direction = if forward { 1 } else { -1 };
                    }
                    if response.hovered() || response.has_focus() || ctx.input(|i| i.any_touches())
                    {
                        let arrow =
                            egui::Rect::from_center_size(zone.center(), egui::vec2(48.0, 64.0));
                        let d = if forward { 1.0 } else { -1.0 };
                        let p = arrow.center();
                        ui.painter().line_segment(
                            [
                                p + egui::vec2(-7.0 * d, -18.0),
                                p + egui::vec2(11.0 * d, 0.0),
                            ],
                            egui::Stroke::new(1.0_f32, tokens.text),
                        );
                        ui.painter().line_segment(
                            [
                                p + egui::vec2(11.0 * d, 0.0),
                                p + egui::vec2(-7.0 * d, 18.0),
                            ],
                            egui::Stroke::new(1.0_f32, tokens.text),
                        );
                    }
                }
                let full_rect = egui::Rect::from_min_size(
                    rect.right_bottom() - egui::vec2(60.0, 56.0),
                    egui::vec2(48.0, 48.0),
                );
                let full = ui
                    .allocate_new_ui(egui::UiBuilder::new().max_rect(full_rect), |ui| {
                        icon_button(
                            ui,
                            Icon::Fullscreen,
                            lang.text("Полноэкранный режим", "Fullscreen"),
                            full_rect.size(),
                        )
                    })
                    .inner;
                if full.clicked() {
                    self.zoom_open = false;
                    self.slideshow_at = None;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
                }
                if let Some(swipe) = pane.take_swipe() {
                    direction = swipe;
                }
            });

        if self.zoom_open && reveal {
            if let Some(anchor) = self.zoom_anchor {
                let pos = egui::pos2(
                    (anchor.center().x - 122.0)
                        .clamp(0.0, (ctx.screen_rect().width() - 244.0).max(0.0)),
                    anchor.bottom() + 4.0,
                );
                let popup = egui::Area::new(egui::Id::new("photo_zoom_flyout"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(pos)
                    .movable(false)
                    .show(ctx, |ui| {
                        if disabled {
                            ui.disable();
                        }
                        egui::Frame::NONE
                            .fill(tokens.tile)
                            .inner_margin(egui::Margin::symmetric(32, 0))
                            .show(ui, |ui| {
                                ui.set_width(180.0);
                                ui.set_min_height(44.0);
                                ui.spacing_mut().slider_width = 180.0;
                                ui.vertical_centered(|ui| {
                                    ui.add_space(8.0);
                                    // Normalized control, not a percent of native pixels.
                                    // Reference curve is unknown; bounded approximation is explicit.
                                    let mut position = zoom_position(pane.zoom);
                                    if ui
                                        .add(
                                            egui::Slider::new(&mut position, 0.0..=100.0)
                                                .show_value(false),
                                        )
                                        .on_hover_text(lang.text("Уровень масштаба", "Zoom level"))
                                        .changed()
                                    {
                                        pane.set_zoom(zoom_from_position(position));
                                    }
                                });
                            });
                    });
                if ctx.input(|i| {
                    i.pointer.any_pressed()
                        && i.pointer.interact_pos().is_some_and(|p| {
                            !popup.response.rect.contains(p) && !anchor.contains(p)
                        })
                }) {
                    self.zoom_open = false;
                }
            }
        }

        if disabled {
            direction = 0;
        }
        if let Some(deadline) = self.slideshow_at {
            if !disabled && now >= deadline {
                if pane.can_navigate_forward() {
                    direction = 1;
                    self.slideshow_at = Some(now + 4.0);
                } else {
                    self.slideshow_at = None;
                }
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if self.transform_requested.is_some() || self.undo_requested {
            direction = 0;
        }
        if direction != 0 {
            pane.navigate_background(direction, ctx);
        }
        if let Some(path) = pane.image_paths.get(pane.current_index).cloned() {
            if let Some(transform) = self.transform_requested.take() {
                self.slideshow_at = None;
                self.zoom_open = false;
                self.edits.start(path, Some(transform), ctx);
            } else if std::mem::take(&mut self.undo_requested) {
                self.edits.start(path, None, ctx);
            }
        }
        if let Some(resize) = &mut self.resize {
            let result=egui::Modal::new(egui::Id::new("photo_resize"))
                .frame(egui::Frame::NONE.fill(if ctx.style().visuals.dark_mode {egui::Color32::from_gray(40)}else{egui::Color32::from_gray(242)})
                    .stroke(egui::Stroke::new(1.0_f32,tokens.accent)).inner_margin(24))
                .backdrop_color(if ctx.style().visuals.dark_mode {egui::Color32::from_black_alpha(110)}else{egui::Color32::from_white_alpha(160)})
                .show(ctx,|ui|{
                ui.set_width(420.0_f32.min((ctx.screen_rect().width()-100.0).max(200.0)));ui.heading(lang.text("Изменить размер","Resize"));
                ui.label(format!("{} × {} px",resize.original[0],resize.original[1]));
                ui.horizontal(|ui|{
                    ui.label(lang.text("Ширина","Width"));
                    if ui.add(egui::DragValue::new(&mut resize.width).range(1..=16384)).changed() && resize.linked {
                        resize.height=(resize.width as f64*resize.original[1] as f64/resize.original[0].max(1) as f64).round().clamp(1.0,16384.0) as u32;
                    }
                    ui.label(lang.text("Высота","Height"));
                    if ui.add(egui::DragValue::new(&mut resize.height).range(1..=16384)).changed() && resize.linked {
                        resize.width=(resize.height as f64*resize.original[0] as f64/resize.original[1].max(1) as f64).round().clamp(1.0,16384.0) as u32;
                    }
                });
                ui.checkbox(&mut resize.linked,lang.text("Сохранять пропорции","Maintain aspect ratio"));
                ui.horizontal(|ui|{for (fraction,label) in [(1.0,"100%"),(0.5,"50%"),(0.25,"25%")] {
                    if ui.button(label).clicked(){resize.width=(resize.original[0] as f64*fraction).round().max(1.0) as u32;resize.height=(resize.original[1] as f64*fraction).round().max(1.0) as u32;}
                }});
                ui.horizontal(|ui|{ui.selectable_value(&mut resize.jpeg,false,"PNG");ui.selectable_value(&mut resize.jpeg,true,"JPEG");});
                if resize.jpeg {ui.add(egui::Slider::new(&mut resize.quality,10..=100).text(lang.text("Качество","Quality")));}
                ui.label(lang.text("Сохраняется новый файл. EXIF/ICC и другие метаданные не переносятся; ориентация применяется к пикселям. JPEG: прозрачность заменяется белым. Анимации/RAW: только первый кадр/превью.","Saves a new file. EXIF/ICC and other metadata are not copied; orientation is baked into pixels. JPEG composites transparency on white. Animation/RAW: first frame/preview only."));
                let valid=crate::platform::resize::Options{width:resize.width,height:resize.height,quality:resize.quality,jpeg:resize.jpeg}.validate().is_ok();
                if !valid {ui.colored_label(egui::Color32::RED,lang.text("1–16384 px по стороне, не более48 мегапикселей","1–16384 px per side, at most48 megapixels"));}
                let save=ui.add_enabled(valid&&!self.jobs.busy(),egui::Button::new(lang.text("Сохранить копию…","Save resized copy…"))).clicked();
                let cancel=ui.button(lang.text("Отмена","Cancel")).clicked();(save,cancel)
            });
            if result.inner.0 {
                let options = crate::platform::resize::Options {
                    width: resize.width,
                    height: resize.height,
                    quality: resize.quality,
                    jpeg: resize.jpeg,
                };
                self.jobs.start(
                    crate::platform::jobs::Job::Resize(resize.path.clone(), options),
                    ctx,
                );
                self.resize = None;
            } else if result.inner.1 || result.should_close() {
                self.resize = None;
            }
        }
        if self.edits.busy() {
            egui::Area::new(egui::Id::new("photo_edit_busy"))
                .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -24.0))
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.label(lang.text("Сохранение поворота…", "Saving transform…"));
                    });
                });
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        if let Some(error) = self.edits.error.clone() {
            let response = egui::Modal::new(egui::Id::new("photo_edit_error")).show(ctx, |ui| {
                ui.set_max_width(460.0);
                ui.heading(lang.text("Не удалось изменить фото", "Could not edit photo"));
                ui.label(error);
                ui.button(lang.text("Закрыть", "Close")).clicked()
            });
            if response.inner || response.should_close() {
                self.edits.error = None;
            }
        }
        if let Some(path) = self.delete_path.clone() {
            let response =
                egui::Modal::new(egui::Id::new("photo_trash_confirmation")).show(ctx, |ui| {
                    ui.set_max_width(360.0_f32.min(ctx.screen_rect().width() - 48.0));
                    ui.heading(lang.text("Переместить в корзину?", "Move to Trash?"));
                    ui.label(path.file_name().unwrap_or_default().to_string_lossy());
                    let yes = ui.button(lang.text("В корзину", "Move to Trash")).clicked();
                    let cancel = ui.button(lang.text("Отмена", "Cancel")).clicked();
                    (yes, cancel)
                });
            if response.inner.0 {
                action = Action::Trash(path);
                self.delete_path = None;
            } else if response.inner.1 || response.should_close() {
                self.delete_path = None;
            }
        }
        if self.settings_open {
            let response = egui::Modal::new(egui::Id::new("photo_settings")).show(ctx, |ui| {
                ui.heading(lang.text("Настройки просмотра", "Viewer settings"));
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut settings.language, Language::Ru, "Русский");
                    ui.selectable_value(&mut settings.language, Language::En, "English");
                });
                ui.horizontal(|ui| {
                    for (value, label) in [
                        (Appearance::System, lang.text("Системная", "System")),
                        (Appearance::Light, lang.text("Светлая", "Light")),
                        (Appearance::Dark, lang.text("Тёмная", "Dark")),
                    ] {
                        ui.selectable_value(&mut settings.appearance, value, label);
                    }
                });
                ui.label(lang.text("Колесо мыши", "Mouse wheel"));
                ui.radio_value(
                    &mut settings.mouse_wheel_zoom,
                    false,
                    lang.text("Следующий / предыдущий снимок", "Next / previous photo"),
                );
                ui.radio_value(
                    &mut settings.mouse_wheel_zoom,
                    true,
                    lang.text("Масштабирование", "Zoom"),
                );
                ui.label(lang.text(
                    "Ctrl + колесо всегда меняет масштаб",
                    "Ctrl + wheel always zooms",
                ));
                ui.button(lang.text("Закрыть", "Close")).clicked()
            });
            if response.inner || response.should_close() {
                self.settings_open = false;
            }
        }
        if self.share_open {
            let response = egui::Modal::new(egui::Id::new("share_photo")).show(ctx, |ui| {
                ui.set_max_width(360.0);
                ui.heading(lang.text("Поделиться", "Share"));
                ui.label(lang.text(
                    "Файл не отправляется автоматически. Выберите действие.",
                    "Nothing is sent automatically. Choose an action.",
                ));
                let email = ui
                    .add_enabled(
                        cfg!(target_os = "linux") && !self.jobs.busy(),
                        egui::Button::new(
                            lang.text("Черновик письма с вложением", "Email attachment draft"),
                        ),
                    )
                    .clicked();
                let export = ui
                    .add_enabled(
                        !self.jobs.busy(),
                        egui::Button::new(lang.text("Сохранить PDF…", "Save PDF…")),
                    )
                    .clicked();
                let folder = ui
                    .button(lang.text("Показать файл в папке", "Show file in folder"))
                    .clicked();
                ui.label(lang.text(
                    "Это действия Rphotos, не системный Share-флайаут.",
                    "These are Rphotos actions, not the system Share flyout.",
                ));
                let close = ui.button(lang.text("Отмена", "Cancel")).clicked();
                let copy = ui
                    .add_enabled(
                        !self.jobs.busy(),
                        egui::Button::new(lang.text("Копировать файл", "Copy file")),
                    )
                    .clicked();
                (email, export, folder, close, copy)
            });
            if let Some(path) = pane.image_paths.get(pane.current_index) {
                if response.inner.0 {
                    self.jobs
                        .start(crate::platform::jobs::Job::Email(vec![path.clone()]), ctx);
                }
                if response.inner.1 {
                    self.jobs
                        .start(crate::platform::jobs::Job::Export(vec![path.clone()]), ctx);
                }
                if response.inner.2 {
                    if let Some(parent) = path.parent() {
                        crate::file_io::open_in_file_explorer(&parent.to_string_lossy());
                    }
                }
                if response.inner.4 {
                    self.jobs
                        .start(crate::platform::jobs::Job::Copy(vec![path.clone()]), ctx);
                }
            }
            if response.inner.0
                || response.inner.1
                || response.inner.2
                || response.inner.3
                || response.inner.4
                || response.should_close()
            {
                self.share_open = false;
            }
        }
        self.jobs.show(ctx, lang);
        pane.mouse_wheel_zoom = settings.mouse_wheel_zoom;
        if *settings != before {
            settings.save();
        }
        action
    }

    fn info_panel(
        &mut self,
        ctx: &egui::Context,
        pane: &Pane,
        lang: Language,
        tokens: &Tokens,
        disabled: bool,
    ) {
        egui::SidePanel::left("photo_info")
            .exact_width(344.0_f32.min(ctx.screen_rect().width() - 160.0))
            .resizable(false)
            .frame(egui::Frame::default().fill(tokens.tile))
            .show(ctx, |ui| {
                ui.set_min_width(ui.available_width());
                if disabled {
                    ui.disable();
                }
                let header = egui::Rect::from_min_size(
                    ui.cursor().min,
                    egui::vec2(ui.available_width(), 48.0),
                );
                ui.allocate_rect(header, egui::Sense::hover());
                ui.painter().text(
                    header.left_center() + egui::vec2(24.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    lang.text("Сведения о файле", "File info"),
                    egui::FontId::proportional(20.0),
                    tokens.text,
                );
                ui.scope_builder(
                    egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                        header.right_top() - egui::vec2(48.0, 0.0),
                        egui::vec2(48.0, 48.0),
                    )),
                    |ui| {
                        if icon_button(
                            ui,
                            Icon::Close,
                            lang.text("Закрыть сведения", "Close file info"),
                            egui::vec2(48.0, 48.0),
                        )
                        .clicked()
                        {
                            self.info_open = false;
                        }
                    },
                );
                egui::ScrollArea::vertical()
                    .id_salt("photo_info_scroll")
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        egui::Frame::NONE
                            .inner_margin(egui::Margin::symmetric(24, 8))
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                if let Some(path) = pane.image_paths.get(pane.current_index) {
                                    info_field(
                                        ui,
                                        lang.text("Имя файла", "File name"),
                                        &path.file_name().unwrap_or_default().to_string_lossy(),
                                        tokens,
                                    );
                                }
                                let record = pane.current_record.as_ref();
                                let exif = record.and_then(|r| {
                                    if let crate::metadata::ExifData::Present(e) = &r.exif {
                                        Some(e.as_ref())
                                    } else {
                                        None
                                    }
                                });
                                info_field(
                                    ui,
                                    lang.text("Дата съёмки", "Date taken"),
                                    exif.and_then(|e| e.date_taken.as_deref()).unwrap_or("--"),
                                    tokens,
                                );
                                let size = record
                                    .and_then(|r| r.file_size)
                                    .map(|s| {
                                        if s < 1_000_000 {
                                            format!("{:.1} KB", s as f64 / 1000.0)
                                        } else {
                                            format!("{:.1} MB", s as f64 / 1_000_000.0)
                                        }
                                    })
                                    .unwrap_or_else(|| "--".into());
                                info_field(ui, lang.text("Размер", "Size"), &size, tokens);
                                let dimensions = record
                                    .and_then(|r| r.dimensions)
                                    .map(|[w, h]| format!("{w} × {h} px"))
                                    .unwrap_or_else(|| "--".into());
                                info_field(
                                    ui,
                                    lang.text("Разрешение изображения", "Image dimensions"),
                                    &dimensions,
                                    tokens,
                                );
                                let shot = exif
                                    .map(|e| {
                                        [
                                            e.aperture.as_deref(),
                                            e.shutter.as_deref(),
                                            e.focal_length.as_deref(),
                                        ]
                                        .into_iter()
                                        .flatten()
                                        .collect::<Vec<_>>()
                                        .join(" · ")
                                    })
                                    .filter(|s| !s.is_empty())
                                    .unwrap_or_else(|| "--".into());
                                info_field(ui, lang.text("Съёмка", "Shot"), &shot, tokens);
                                info_field(
                                    ui,
                                    "ISO",
                                    exif.and_then(|e| e.iso.as_deref()).unwrap_or("--"),
                                    tokens,
                                );
                                info_field(
                                    ui,
                                    lang.text("Устройство", "Device"),
                                    exif.and_then(|e| e.camera.as_deref()).unwrap_or("--"),
                                    tokens,
                                );
                                if let Some(parent) = pane
                                    .image_paths
                                    .get(pane.current_index)
                                    .and_then(|p| p.parent())
                                {
                                    info_field(
                                        ui,
                                        lang.text("Путь к папке", "Folder path"),
                                        &parent.to_string_lossy(),
                                        tokens,
                                    );
                                    if ui.link(lang.text("Открыть папку", "Open folder")).clicked()
                                    {
                                        crate::file_io::open_in_file_explorer(
                                            &parent.to_string_lossy(),
                                        );
                                    }
                                    ui.add_space(20.0);
                                }
                                info_field(
                                    ui,
                                    lang.text("Источник", "Source"),
                                    lang.text("Этот компьютер", "This computer"),
                                    tokens,
                                );
                                ui.label(
                                    RichText::new(lang.text(
                                        "Только чтение · оригинал не меняется",
                                        "Read only · original is unchanged",
                                    ))
                                    .size(12.0)
                                    .color(tokens.muted),
                                );
                            });
                    });
            });
    }
}

fn info_field(ui: &mut egui::Ui, label: &str, value: &str, tokens: &Tokens) {
    let top = ui.cursor().top();
    ui.label(RichText::new(label).size(14.0).color(tokens.muted));
    ui.add(egui::Label::new(RichText::new(value).size(14.0)).wrap());
    ui.add_space((60.0 - (ui.cursor().top() - top)).max(20.0));
}
