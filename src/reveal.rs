//! Shared, bounded Fluent-inspired feedback for every egui hit target.
//! Uses egui's existing hit testing: no duplicate widgets, input capture or
//! per-photo registry, and never samples the desktop behind the application.
use eframe::egui::{self, Color32, Id, Rect, Stroke};

pub(crate) const ACCENT: Color32 = Color32::from_rgb(0, 120, 215);

#[derive(Clone, Default)]
struct Rules {
    pass: u64,
    bounds: Vec<(Id, Rect)>,
    excluded: Vec<Rect>,
    ignored: Vec<Id>,
}

pub(crate) fn ignore(response: &egui::Response) {
    let pass = response.ctx.cumulative_pass_nr();
    response.ctx.data_mut(|d| {
        let rules = d.get_temp_mut_or_default::<Rules>(Id::new("rphotos.reveal.rules"));
        if rules.pass != pass {
            *rules = Rules {
                pass,
                ..Default::default()
            };
        }
        rules.ignored.push(response.id);
    });
}

/// Override visual bounds without moving or shrinking the actual hit target.
pub(crate) fn bounds(response: &egui::Response, rect: Rect) {
    let pass = response.ctx.cumulative_pass_nr();
    response.ctx.data_mut(|d| {
        let rules = d.get_temp_mut_or_default::<Rules>(Id::new("rphotos.reveal.rules"));
        if rules.pass != pass {
            *rules = Rules {
                pass,
                ..Default::default()
            };
        }
        rules.bounds.push((response.id, rect));
    });
}

pub(crate) fn exclude(ctx: &egui::Context, rect: Rect) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let rules = d.get_temp_mut_or_default::<Rules>(Id::new("rphotos.reveal.rules"));
        if rules.pass != pass {
            *rules = Rules {
                pass,
                ..Default::default()
            };
        }
        rules.excluded.push(rect);
    });
}

pub(crate) fn install(ctx: &egui::Context) {
    let installed = ctx.data_mut(|d| {
        let id = Id::new("rphotos.reveal.installed");
        let installed = d.get_temp::<bool>(id).unwrap_or(false);
        d.insert_temp(id, true);
        installed
    });
    if !installed {
        ctx.on_end_pass("rphotos.reveal", std::sync::Arc::new(paint_interactions));
    }
}

fn paint_interactions(ctx: &egui::Context) {
    let pass = ctx.cumulative_pass_nr();
    let rules = ctx
        .data(|d| d.get_temp::<Rules>(Id::new("rphotos.reveal.rules")))
        .filter(|r| r.pass == pass)
        .unwrap_or_default();
    let mut ids = ctx.interaction_snapshot(|i| {
        let mut ids = i.hovered.iter().copied().collect::<Vec<_>>();
        if let Some(dragged) = i.dragged {
            if !ids.contains(&dragged) {
                ids.push(dragged);
            }
        }
        ids
    });
    if let Some(focus) = ctx.memory(|m| m.focused()) {
        if !ids.contains(&focus) {
            ids.push(focus);
        }
    }
    // Hit testing normally returns one or two targets. Bound work even for
    // pathological nested/overlapping UIs; never traverse the photo catalogue.
    for id in ids.into_iter().take(16) {
        let Some(response) = ctx.read_response(id) else {
            continue;
        };
        if rules.ignored.contains(&id) {
            continue;
        }
        if !response.enabled() || !response.sense.senses_click() {
            continue;
        }
        let rect = rules
            .bounds
            .iter()
            .find(|(id, _)| *id == response.id)
            .map_or(response.rect, |(_, rect)| *rect);
        if !rect.is_positive() || !rect.is_finite() {
            continue;
        }
        let pointer = ctx.pointer_hover_pos();
        // egui's hover hit slop can include two adjacent controls across a
        // separator. Reveal follows their actual painted bounds, not that slop.
        let hovered = response.hovered()
            && pointer.is_some_and(|p| response.rect.contains(p) && rect.contains(p));
        if !hovered && !response.has_focus() && !response.is_pointer_button_down_on() {
            continue;
        }
        if rules.excluded.iter().any(|excluded| {
            excluded.contains(rect.center())
                || pointer.is_some_and(|p| excluded.contains(p) && rect.contains(p))
        }) {
            continue;
        }
        // Text editing has its own caret, selection, frame and focus stroke.
        // Do not draw meshes over glyphs or individual insertion positions.
        let editing =
            response.sense.senses_drag() && ctx.output(|o| o.cursor_icon == egui::CursorIcon::Text);
        if editing {
            continue;
        }
        // The title-bar click-and-drag surface is not an action button.
        if response.sense.senses_drag() && rect.width() > 400.0 && rect.height() <= 36.0 {
            continue;
        }
        let painter = ctx
            .layer_painter(response.layer_id)
            .with_clip_rect(rect.intersect(response.interact_rect.expand(3.0)));
        let focused = response.has_focus();
        let pressed = response.is_pointer_button_down_on();
        if pressed {
            // A translucent accent also works over thumbnails without hiding
            // their content, and does not alter idle/selected semantics.
            painter.rect_filled(rect, 0.0, ACCENT.gamma_multiply(0.32));
        }
        if hovered {
            if let Some(pointer) = ctx.pointer_hover_pos() {
                glow(&painter, rect, pointer, ctx.style().visuals.dark_mode);
            }
        }
        if focused {
            focus(&painter, rect);
        }
    }
}

pub(crate) fn focus(painter: &egui::Painter, rect: Rect) {
    painter.rect_stroke(
        rect.shrink(1.0),
        0.0,
        Stroke::new(2.0_f32, ACCENT),
        egui::StrokeKind::Inside,
    );
    painter.rect_stroke(
        rect.shrink(3.0),
        0.0,
        Stroke::new(1.0_f32, Color32::from_white_alpha(180)),
        egui::StrokeKind::Inside,
    );
}

fn glow(painter: &egui::Painter, rect: Rect, pointer: egui::Pos2, dark: bool) {
    let radius = 88.0;
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(
        pointer,
        if dark {
            Color32::from_white_alpha(32)
        } else {
            // White-on-white glow washes out labels. A faint cool tint keeps
            // light-theme text legible while the brighter edge follows input.
            Color32::from_rgba_unmultiplied(0, 120, 215, 14)
        },
    );
    const SEGMENTS: u32 = 40;
    for i in 0..=SEGMENTS {
        let angle = i as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
        mesh.colored_vertex(
            pointer + egui::vec2(angle.cos(), angle.sin()) * radius,
            Color32::TRANSPARENT,
        );
        if i > 0 {
            mesh.add_triangle(0, i, i + 1);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
    let rect = rect.shrink(0.75);
    let edge_color = if dark {
        Color32::WHITE
    } else {
        Color32::from_rgb(30, 110, 180)
    };
    let mut border = egui::Mesh::default();
    for (from, to) in [
        (rect.left_top(), rect.right_top()),
        (rect.right_top(), rect.right_bottom()),
        (rect.right_bottom(), rect.left_bottom()),
        (rect.left_bottom(), rect.left_top()),
    ] {
        // Local highlight follows the pointer along the border, not a flat
        // hover rectangle. Limit segmentation independently of widget size.
        let steps = ((from.distance(to) / 12.0).ceil() as usize).clamp(1, 64);
        let direction = (to - from).normalized();
        let normal = egui::vec2(-direction.y, direction.x) * 0.6;
        let base = border.vertices.len() as u32;
        for step in 0..=steps {
            let point = from.lerp(to, step as f32 / steps as f32);
            let strength = (1.0 - pointer.distance(point) / radius).clamp(0.0, 1.0);
            let color = edge_color.gamma_multiply(strength * 0.75);
            border.colored_vertex(point - normal, color);
            border.colored_vertex(point + normal, color);
            if step > 0 {
                let i = base + step as u32 * 2;
                border.add_triangle(i - 2, i - 1, i);
                border.add_triangle(i - 1, i + 1, i);
            }
        }
    }
    // Continuous strips, not separately antialiased short line segments:
    // those produced bright joint dots inside small/text input rectangles.
    painter.add(egui::Shape::mesh(border));
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;
    use egui_kittest::{kittest::Queryable, Harness};

    fn has_glow(output: &egui::FullOutput, target: Rect) -> bool {
        output.shapes.iter().any(|shape| {
            shape.clip_rect == target
                && matches!(&shape.shape,egui::Shape::Mesh(mesh) if mesh.vertices.len()==42)
        })
    }

    #[test]
    fn shared_hook_covers_standard_widgets_custom_tiles_and_keyboard_focus() {
        let mut h = Harness::builder()
            .with_size(egui::vec2(600.0, 500.0))
            .build_state(
                |ctx, (checked, text)| {
                    crate::library::ui::Tokens::new(crate::library::Appearance::Light, ctx)
                        .apply(ctx);
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let _ = ui.button("Button");
                        ui.checkbox(checked, "Setting");
                        ui.add(egui::TextEdit::singleline(text).hint_text("Input"));
                        let (rect, response) =
                            ui.allocate_exact_size(egui::vec2(200.0, 100.0), egui::Sense::click());
                        ui.painter().rect_filled(rect, 0.0, Color32::from_gray(80));
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Photo tile")
                        });
                        ui.add_enabled(false, egui::Button::new("Disabled"));
                    });
                },
                (false, String::new()),
            );
        h.run_steps(3);
        for label in ["Button", "Setting", "Photo tile"] {
            let bounds = h.get_by_label(label).raw_bounds().unwrap();
            let rect = Rect::from_min_max(
                pos2(bounds.x0 as f32, bounds.y0 as f32),
                pos2(bounds.x1 as f32, bounds.y1 as f32),
            );
            h.input_mut()
                .events
                .push(egui::Event::PointerMoved(rect.center()));
            h.run_steps(2);
            assert!(has_glow(h.output(), rect), "No Reveal on {label}");
            assert_eq!(h.output().shapes.iter().filter(|shape|shape.clip_rect==rect && matches!(&shape.shape,egui::Shape::Mesh(mesh) if mesh.vertices.len()==42)).count(),1,"installing Reveal each frame must not accumulate callbacks");
        }
        let bounds = h.get_by_label("Disabled").raw_bounds().unwrap();
        let rect = Rect::from_min_max(
            pos2(bounds.x0 as f32, bounds.y0 as f32),
            pos2(bounds.x1 as f32, bounds.y1 as f32),
        );
        h.input_mut()
            .events
            .push(egui::Event::PointerMoved(rect.center()));
        h.run_steps(2);
        assert!(!has_glow(h.output(), rect));
        h.input_mut()
            .events
            .push(egui::Event::PointerMoved(pos2(590.0, 490.0)));
        h.press_key(egui::Key::Tab);
        h.run_steps(2);
        assert!(h.output().shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Rect(rect) if rect.stroke.width==2.0 && rect.stroke.color==ACCENT)),"keyboard focus must have a luminous outline");
        assert!(
            !h.output()
                .shapes
                .iter()
                .any(|shape| matches!(&shape.shape,egui::Shape::Rect(rect) if rect.fill==ACCENT)),
            "focus must not leave a solid pressed background"
        );
    }

    #[test]
    fn pressed_widgets_get_accent_without_changing_click_or_keyboard_actions() {
        let mut h = Harness::builder()
            .with_size(egui::vec2(600.0, 500.0))
            .build_state(
                |ctx, clicks| {
                    crate::library::ui::Tokens::new(crate::library::Appearance::Dark, ctx)
                        .apply(ctx);
                    egui::CentralPanel::default().show(ctx, |ui| {
                        if ui.button("Action").clicked() {
                            *clicks += 1;
                        }
                    });
                },
                0,
            );
        h.run_steps(3);
        let bounds = h.get_by_label("Action").raw_bounds().unwrap();
        let point = pos2(
            (bounds.x0 + bounds.x1) as f32 / 2.0,
            (bounds.y0 + bounds.y1) as f32 / 2.0,
        );
        h.input_mut().events.push(egui::Event::PointerMoved(point));
        h.run_steps(2);
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        h.run_steps(1);
        assert_eq!(
            *h.state(),
            0,
            "press must not invoke an action before release"
        );
        assert!(h.output().shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Rect(rect) if rect.fill==ACCENT.gamma_multiply(0.32))),"pressed overlay missing");
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        h.run_steps(1);
        assert_eq!(*h.state(), 1);
        h.press_key(egui::Key::Tab);
        h.run_steps(1);
        h.press_key(egui::Key::Enter);
        h.run_steps(1);
        assert_eq!(*h.state(), 2, "Reveal must not capture keyboard activation");
    }

    #[test]
    fn informational_labels_and_excluded_search_never_receive_reveal() {
        let mut h = Harness::builder()
            .with_size(egui::vec2(600.0, 500.0))
            .build_state(
                |ctx, text| {
                    crate::library::ui::Tokens::new(crate::library::Appearance::Light, ctx)
                        .apply(ctx);
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.heading("Settings heading");
                        ui.label("Current month");
                        let search = ui.add(egui::TextEdit::singleline(text).hint_text("Search"));
                        search.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Search")
                        });
                        exclude(ctx, search.rect.expand(2.0));
                        let _ = ui.button("Actual action");
                    });
                },
                String::new(),
            );
        h.run_steps(3);
        for label in ["Settings heading", "Current month", "Search"] {
            let bounds = h.get_by_label(label).raw_bounds().unwrap();
            h.input_mut().events.push(egui::Event::PointerMoved(pos2(
                (bounds.x0 + bounds.x1) as f32 / 2.0,
                (bounds.y0 + bounds.y1) as f32 / 2.0,
            )));
            h.run_steps(3);
            assert!(!h.output().shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Mesh(mesh) if mesh.vertices.len()==42)),"{label} must not get a Reveal mesh");
        }
        h.get_by_label("Search").focus();
        h.run_steps(2);
        assert!(!h.output().shapes.iter().any(
            |shape| matches!(&shape.shape,egui::Shape::Mesh(mesh) if mesh.vertices.len()==42)
        ));
    }

    #[test]
    fn visual_bounds_are_balanced_and_do_not_change_hit_testing() {
        let mut h = Harness::builder()
            .with_size(egui::vec2(600.0, 400.0))
            .build(|ctx| {
                crate::library::ui::Tokens::new(crate::library::Appearance::Light, ctx).apply(ctx);
                egui::CentralPanel::default().show(ctx, |ui| {
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(150.0, 48.0), egui::Sense::click());
                    let galley = ui.painter().layout_no_wrap(
                        "Collection".into(),
                        egui::FontId::proportional(20.0),
                        Color32::BLACK,
                    );
                    let text_rect = Rect::from_center_size(rect.center(), galley.size());
                    ui.painter().galley(text_rect.min, galley, Color32::BLACK);
                    bounds(
                        &response,
                        Rect::from_center_size(
                            text_rect.center(),
                            text_rect.size() + egui::vec2(16.0, 16.0),
                        ),
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Tab")
                    });
                });
            });
        h.run_steps(3);
        let target = h.get_by_label("Tab").raw_bounds().unwrap();
        let center = pos2(
            (target.x0 + target.x1) as f32 / 2.0,
            (target.y0 + target.y1) as f32 / 2.0,
        );
        h.input_mut().events.push(egui::Event::PointerMoved(center));
        h.run_steps(2);
        let glow = h
            .output()
            .shapes
            .iter()
            .find(|shape| matches!(&shape.shape,egui::Shape::Mesh(mesh) if mesh.vertices.len()==42))
            .unwrap();
        assert_eq!(glow.clip_rect.center(), center);
        assert!(glow.clip_rect.width() < 150.0);
        assert_eq!(h.get_by_label("Tab").raw_bounds().unwrap(), target);
    }

    #[test]
    #[ignore = "requires an offscreen wgpu adapter and RPHOTOS_CAPTURE_DIR"]
    fn capture_reveal() {
        let output = std::path::PathBuf::from(
            std::env::var_os("RPHOTOS_CAPTURE_DIR").expect("set RPHOTOS_CAPTURE_DIR"),
        );
        std::fs::create_dir_all(&output).unwrap();
        for (name, dark, pressed, keyboard) in [
            ("reveal-light", false, false, false),
            ("reveal-dark", true, false, false),
            ("reveal-pressed", true, true, false),
            ("reveal-focus", true, false, true),
        ] {
            let mut h = Harness::builder()
                .with_size(egui::vec2(700.0, 420.0))
                .wgpu()
                .build_state(
                    move |ctx, checked| {
                        crate::library::ui::Tokens::new(
                            if dark {
                                crate::library::Appearance::Dark
                            } else {
                                crate::library::Appearance::Light
                            },
                            ctx,
                        )
                        .apply(ctx);
                        egui::CentralPanel::default().show(ctx, |ui| {
                            ui.heading("Rphotos · Reveal");
                            ui.add_space(16.0);
                            ui.horizontal(|ui| {
                                let _ = ui.button("Добавить папку");
                                let _ = ui.button("Новый альбом");
                            });
                            ui.checkbox(checked, "Параметр настроек / Setting");
                            ui.add_enabled(false, egui::Button::new("Недоступно / Disabled"));
                            let (rect, response) = ui.allocate_exact_size(
                                egui::vec2(300.0, 180.0),
                                egui::Sense::click(),
                            );
                            ui.painter()
                                .rect_filled(rect, 0.0, Color32::from_rgb(25, 70, 105));
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "Фото / Photo",
                                egui::FontId::proportional(24.0),
                                Color32::WHITE,
                            );
                            response.widget_info(|| {
                                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Photo")
                            });
                        });
                    },
                    false,
                );
            h.run_steps(3);
            if keyboard {
                h.press_key(egui::Key::Tab);
                h.run_steps(2);
            } else {
                let bounds = h
                    .get_by_label(if name == "reveal-light" {
                        "Photo"
                    } else {
                        "Добавить папку"
                    })
                    .raw_bounds()
                    .unwrap();
                let point = pos2(bounds.x0 as f32 + 20.0, bounds.y0 as f32 + 15.0);
                h.input_mut().events.push(egui::Event::PointerMoved(point));
                h.run_steps(2);
                if pressed {
                    h.input_mut().events.push(egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    });
                    h.run_steps(1);
                }
            }
            h.render()
                .unwrap()
                .save(output.join(format!("{name}.png")))
                .unwrap();
        }
    }
    #[test]
    fn reveal_is_bounded_and_clipped_to_the_hit_target() {
        let ctx = egui::Context::default();
        install(&ctx);
        install(&ctx);
        let rect = Rect::from_min_max(pos2(20.0, 20.0), pos2(220.0, 60.0));
        let output = ctx.run(Default::default(), |ctx| {
            glow(
                &ctx.layer_painter(egui::LayerId::background())
                    .with_clip_rect(rect),
                rect,
                pos2(25.0, 40.0),
                false,
            )
        });
        assert!(output.shapes.len() < 150);
        assert!(output.shapes.iter().all(|shape| shape.clip_rect == rect));
        assert!(
            !output
                .shapes
                .iter()
                .any(|shape| matches!(shape.shape, egui::Shape::LineSegment { .. })),
            "border must be continuous strips without antialiased joint dots"
        );
        assert!(output.shapes.iter().any(
            |shape| matches!(&shape.shape,egui::Shape::Mesh(mesh) if mesh.vertices.len()==42)
        ));
    }
}
