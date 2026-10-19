//! Client-side title bar with platform backdrop or an opaque fallback.
use eframe::egui::{self, pos2, vec2, Color32, Rect, Sense, Stroke};

use crate::library::{ui::Tokens, Appearance, Language};

#[derive(Clone, Copy)]
pub(crate) enum BackStyle {
    Solid,
    Transparent,
}

pub(crate) fn show(
    ctx: &egui::Context,
    title: &str,
    back: Option<&str>,
    appearance: Appearance,
    lang: Language,
    blocked: bool,
) -> bool {
    show_with_back_style(
        ctx,
        title,
        back,
        appearance,
        lang,
        blocked,
        BackStyle::Transparent,
    )
}

pub(crate) fn show_with_back_style(
    ctx: &egui::Context,
    title: &str,
    back: Option<&str>,
    appearance: Appearance,
    lang: Language,
    blocked: bool,
    back_style: BackStyle,
) -> bool {
    let (fullscreen, maximized) = ctx.input(|i| {
        (
            i.viewport().fullscreen.unwrap_or(false),
            i.viewport().maximized.unwrap_or(false),
        )
    });
    if fullscreen {
        return false;
    }
    let tokens = Tokens::new(appearance, ctx);
    tokens.apply(ctx);
    let mut go_back = false;
    egui::TopBottomPanel::top("rphotos_title")
        .show_separator_line(false)
        .exact_height(32.0)
        .frame(egui::Frame::NONE.fill(tokens.header))
        .show(ctx, |ui| {
            let r = ui.max_rect();
            let left = r.left() + if back.is_some() { 48.0 } else { 0.0 };
            let drag_rect =
                Rect::from_min_max(pos2(left, r.top()), pos2(r.right() - 138.0, r.bottom()));
            let drag = ui.interact(drag_rect, ui.id().with("drag"), Sense::click_and_drag());
            if drag.drag_started() {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            if drag.double_clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            }
            ui.painter()
                .with_clip_rect(drag_rect.shrink2(vec2(12.0, 0.0)))
                .text(
                    pos2(left + 12.0, r.center().y),
                    egui::Align2::LEFT_CENTER,
                    title,
                    egui::FontId::proportional(12.0),
                    tokens.text,
                );
            if let Some(label) = back {
                let rect = Rect::from_min_size(r.min, vec2(48.0, 32.0));
                let response = ui.interact(
                    rect,
                    ui.id().with("back"),
                    if blocked {
                        Sense::hover()
                    } else {
                        Sense::click()
                    },
                );
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, !blocked, label)
                });
                // Settings leave the acrylic title bar visible through Back.
                // Only pointer-down gets a blue fill; hover/focus do not add
                // another translucent coat that would change header opacity.
                if !blocked && response.is_pointer_button_down_on() {
                    ui.painter().rect_filled(
                        rect,
                        0.0,
                        if blocked { tokens.muted } else { tokens.accent },
                    );
                }
                let c = rect.center();
                let s = Stroke::new(
                    1.0_f32,
                    match back_style {
                        BackStyle::Transparent => Color32::WHITE,
                        BackStyle::Solid => tokens.text,
                    },
                );
                ui.painter()
                    .line_segment([c + vec2(-6.0, 0.0), c + vec2(6.0, 0.0)], s);
                ui.painter()
                    .line_segment([c + vec2(-6.0, 0.0), c + vec2(-1.0, -5.0)], s);
                ui.painter()
                    .line_segment([c + vec2(-6.0, 0.0), c + vec2(-1.0, 5.0)], s);
                go_back = response.on_hover_text(label).clicked() && !blocked;
            }
            for (n, label) in [
                lang.text("Свернуть", "Minimize"),
                lang.text("Развернуть / восстановить", "Maximize / restore"),
                lang.text("Закрыть окно", "Close window"),
            ]
            .into_iter()
            .enumerate()
            {
                let rect = Rect::from_min_size(
                    pos2(r.right() - 138.0 + n as f32 * 46.0, r.top()),
                    vec2(46.0, 32.0),
                );
                let response = ui.interact(rect, ui.id().with(("window", n)), Sense::click());
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label)
                });
                if response.is_pointer_button_down_on() {
                    ui.painter().rect_filled(
                        rect,
                        0.0,
                        if n == 2 {
                            Color32::from_rgb(196, 43, 28)
                        } else {
                            tokens.accent
                        },
                    );
                }
                let c = rect.center();
                let s = Stroke::new(
                    1.0_f32,
                    if response.is_pointer_button_down_on() && n == 2 {
                        Color32::WHITE
                    } else {
                        tokens.text
                    },
                );
                match n {
                    0 => {
                        ui.painter()
                            .line_segment([c + vec2(-5.0, 0.0), c + vec2(5.0, 0.0)], s);
                    }
                    1 => {
                        maximize_icon(ui.painter(), c, s, maximized);
                    }
                    _ => {
                        ui.painter()
                            .line_segment([c + vec2(-5.0, -5.0), c + vec2(5.0, 5.0)], s);
                        ui.painter()
                            .line_segment([c + vec2(-5.0, 5.0), c + vec2(5.0, -5.0)], s);
                    }
                }
                if response.on_hover_text(label).clicked() {
                    ctx.send_viewport_cmd(match n {
                        0 => egui::ViewportCommand::Minimized(true),
                        1 => egui::ViewportCommand::Maximized(!maximized),
                        _ => egui::ViewportCommand::Close,
                    });
                }
            }
        });
    if !maximized {
        resize_edges(ctx);
    }
    go_back
}

fn maximize_icon(painter: &egui::Painter, center: egui::Pos2, stroke: Stroke, maximized: bool) {
    if maximized {
        // Draw only the exposed part of the rear window. Never mask its hidden
        // edges with header paint: repeating translucent fill changes opacity.
        painter.add(egui::Shape::line(
            vec![
                center + vec2(-3.0, -3.0),
                center + vec2(-3.0, -5.0),
                center + vec2(5.0, -5.0),
                center + vec2(5.0, 3.0),
            ],
            stroke,
        ));
    }
    painter.rect_stroke(
        Rect::from_min_size(
            center + vec2(-5.0, if maximized { -3.0 } else { -5.0 }),
            vec2(10.0, 10.0),
        ),
        0.0,
        stroke,
        egui::StrokeKind::Inside,
    );
}

fn resize_edges(ctx: &egui::Context) {
    use egui::{CursorIcon as C, ResizeDirection as D};
    let r = ctx.screen_rect();
    let b = 4.0;
    let corner = 12.0;
    for (index, (direction, cursor, rect)) in [
        (
            D::NorthWest,
            C::ResizeNwSe,
            Rect::from_min_size(r.min, vec2(corner, b)),
        ),
        (
            D::NorthEast,
            C::ResizeNeSw,
            Rect::from_min_size(pos2(r.right() - corner, r.top()), vec2(corner, b)),
        ),
        (
            D::SouthWest,
            C::ResizeNeSw,
            Rect::from_min_size(pos2(r.left(), r.bottom() - corner), vec2(corner, corner)),
        ),
        (
            D::SouthEast,
            C::ResizeNwSe,
            Rect::from_min_size(r.max - vec2(corner, corner), vec2(corner, corner)),
        ),
        (
            D::North,
            C::ResizeVertical,
            Rect::from_min_max(
                pos2(r.left() + corner, r.top()),
                pos2(r.right() - corner, r.top() + b),
            ),
        ),
        (
            D::South,
            C::ResizeVertical,
            Rect::from_min_max(
                pos2(r.left() + corner, r.bottom() - b),
                pos2(r.right() - corner, r.bottom()),
            ),
        ),
        (
            D::West,
            C::ResizeHorizontal,
            Rect::from_min_max(
                pos2(r.left(), r.top() + b),
                pos2(r.left() + b, r.bottom() - corner),
            ),
        ),
        (
            D::East,
            C::ResizeHorizontal,
            Rect::from_min_max(
                pos2(r.right() - b, r.top() + b),
                pos2(r.right(), r.bottom() - corner),
            ),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        egui::Area::new(egui::Id::new(("window_resize", index)))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .movable(false)
            .show(ctx, |ui| {
                let (_, response) = ui.allocate_exact_size(rect.size(), Sense::drag());
                let response = response.on_hover_cursor(cursor);
                if response.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{kittest::Queryable, Harness};

    #[test]
    fn settings_back_has_no_fill_until_pressed_and_keeps_its_arrow_color() {
        for appearance in [Appearance::Light, Appearance::Dark] {
            let ctx = egui::Context::default();
            for (events, blocked, pressed) in [
                (vec![], false, false),
                (
                    vec![egui::Event::PointerMoved(pos2(24.0, 16.0))],
                    false,
                    false,
                ),
                (
                    vec![egui::Event::PointerButton {
                        pos: pos2(24.0, 16.0),
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    }],
                    false,
                    true,
                ),
                (
                    vec![egui::Event::PointerButton {
                        pos: pos2(24.0, 16.0),
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: Default::default(),
                    }],
                    false,
                    false,
                ),
                (
                    vec![egui::Event::PointerButton {
                        pos: pos2(24.0, 16.0),
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    }],
                    true,
                    false,
                ),
            ] {
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            vec2(1000.0, 760.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        show_with_back_style(
                            ctx,
                            "Rphotos",
                            Some("Back"),
                            appearance,
                            Language::En,
                            blocked,
                            BackStyle::Transparent,
                        );
                    },
                );
                let back_rect = Rect::from_min_size(egui::Pos2::ZERO, vec2(48.0, 32.0));
                let fills: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Rect(rect)
                            if rect.rect == back_rect && rect.fill.a() == 255 =>
                        {
                            Some(rect.fill)
                        }
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    fills.len(),
                    usize::from(pressed),
                    "idle, hover, release and disabled must have no button background"
                );
                if pressed {
                    assert_eq!(fills[0], Color32::from_rgb(0, 120, 215));
                }
                let arrow: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::LineSegment { points, stroke }
                            if points.iter().all(|point| back_rect.contains(*point))
                                && stroke.color == Color32::WHITE =>
                        {
                            Some(stroke.color)
                        }
                        _ => None,
                    })
                    .collect();
                assert_eq!(arrow.len(), 3);
                assert!(arrow.iter().all(|color| *color == Color32::WHITE));
            }
        }
    }

    #[test]
    fn title_has_no_separator_between_window_and_application_commands() {
        let ctx = egui::Context::default();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1500.0, 960.0))),
                ..Default::default()
            },
            |ctx| {
                show(ctx, "Rphotos", None, Appearance::Light, Language::En, false);
            },
        );
        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::LineSegment { points, stroke } if stroke.width>0.0 && stroke.color!=Color32::TRANSPARENT && (points[0].y-32.0).abs()<1.0 && (points[1].y-32.0).abs()<1.0 && (points[1].x-points[0].x).abs()>1000.0)), "title separator must not be painted");
    }

    #[test]
    fn maximize_and_restore_icons_have_no_interior_fill() {
        for maximized in [false, true] {
            let ctx = egui::Context::default();
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                maximize_icon(
                    &ctx.layer_painter(egui::LayerId::background()),
                    pos2(30.0, 16.0),
                    Stroke::new(1.0_f32, Color32::BLACK),
                    maximized,
                );
            });
            let mut outlines = 0;
            for shape in output.shapes {
                match shape.shape {
                    egui::Shape::Rect(rect) => {
                        assert_eq!(rect.fill, Color32::TRANSPARENT);
                        outlines += 1;
                    }
                    egui::Shape::Path(path) => assert_eq!(path.fill, Color32::TRANSPARENT),
                    egui::Shape::Noop => {}
                    other => panic!("Unexpected icon paint: {other:?}"),
                }
            }
            assert_eq!(outlines, 1);
        }
    }

    #[test]
    fn title_buttons_have_measured_geometry_and_emit_window_commands() {
        let mut h = Harness::builder()
            .with_size(vec2(1000.0, 760.0))
            .build(|ctx| {
                show(
                    ctx,
                    "Rphotos",
                    Some("Back"),
                    Appearance::Light,
                    Language::En,
                    false,
                );
                egui::CentralPanel::default().show(ctx, |_| {});
            });
        h.run_steps(3);
        for (label, x) in [
            ("Minimize", 862.0),
            ("Maximize / restore", 908.0),
            ("Close window", 954.0),
        ] {
            let bounds = h.get_by_label(label).raw_bounds().unwrap();
            let b = Rect::from_min_max(
                pos2(bounds.x0 as f32, bounds.y0 as f32),
                pos2(bounds.x1 as f32, bounds.y1 as f32),
            );
            assert_eq!(b, Rect::from_min_size(pos2(x, 0.0), vec2(46.0, 32.0)));
        }
        let back = h.get_by_label("Back").raw_bounds().unwrap();
        assert_eq!((back.x0, back.y0, back.x1, back.y1), (0.0, 0.0, 48.0, 32.0));
        h.get_by_label("Minimize").click();
        h.step();
        assert!(h.output().viewport_output[&egui::ViewportId::ROOT]
            .commands
            .iter()
            .any(|cmd| matches!(cmd, egui::ViewportCommand::Minimized(true))));
    }
}
