//! Small original line icons shared by Photos-inspired menus/preferences.
use eframe::egui::{self, pos2, vec2, Color32, Rect, Stroke};

#[derive(Clone, Copy)]
pub(super) enum Icon {
    Refresh,
    Slideshow,
    Settings,
    Folder,
    Language,
    Theme,
    Mouse,
    Analysis,
    Info,
    Heart,
}

pub(super) fn paint(p: &egui::Painter, r: Rect, icon: Icon, color: Color32, filled: bool) {
    let c = r.center();
    let s = r.width().min(r.height()) / 24.0;
    let pt = |x: f32, y: f32| c + vec2(x * s, y * s);
    let stroke = Stroke::new(1.2 * s, color);
    let line = |a, b| {
        p.line_segment([a, b], stroke);
    };
    let rect = |a, b| {
        p.rect_stroke(
            Rect::from_min_max(a, b),
            0.0,
            stroke,
            egui::StrokeKind::Inside,
        );
    };
    match icon {
        Icon::Heart => {
            let mut points = Vec::new();
            for i in 0..=48 {
                let t = i as f32 / 48.0 * std::f32::consts::TAU;
                points.push(pt(
                    0.55 * 16.0 * t.sin().powi(3),
                    -0.55
                        * (13.0 * t.cos()
                            - 5.0 * (2.0 * t).cos()
                            - 2.0 * (3.0 * t).cos()
                            - (4.0 * t).cos()),
                ));
            }
            if filled {
                let mut mesh = egui::Mesh::default();
                mesh.colored_vertex(c, color);
                for point in &points {
                    mesh.colored_vertex(*point, color);
                }
                for i in 1..points.len() {
                    mesh.add_triangle(0, i as u32, i as u32 + 1);
                }
                p.add(egui::Shape::mesh(mesh));
            } else {
                p.add(egui::Shape::line(points, stroke));
            }
        }
        Icon::Refresh => {
            let points = (0..=24)
                .map(|i| {
                    let a = 0.4 + i as f32 / 24.0 * 5.0;
                    pt(a.cos() * 8.0, a.sin() * 8.0)
                })
                .collect();
            p.add(egui::Shape::line(points, stroke));
            line(pt(7.5, -7.0), pt(7.5, -1.0));
            line(pt(7.5, -1.0), pt(1.5, -1.0));
        }
        Icon::Slideshow => {
            rect(pt(-10.0, -7.0), pt(10.0, 7.0));
            p.add(egui::Shape::closed_line(
                vec![pt(-3.0, -4.0), pt(4.0, 0.0), pt(-3.0, 4.0)],
                stroke,
            ));
        }
        Icon::Settings => {
            p.circle_stroke(c, 4.0 * s, stroke);
            let points = (0..32)
                .map(|i| {
                    let a = i as f32 / 32.0 * std::f32::consts::TAU;
                    let radius = if i % 4 == 0 || i % 4 == 3 { 10.0 } else { 7.5 };
                    pt(a.cos() * radius, a.sin() * radius)
                })
                .collect();
            p.add(egui::Shape::closed_line(points, stroke));
        }
        Icon::Folder => {
            p.add(egui::Shape::closed_line(
                vec![
                    pt(-10.0, -7.0),
                    pt(-2.0, -7.0),
                    pt(0.0, -4.0),
                    pt(10.0, -4.0),
                    pt(10.0, 8.0),
                    pt(-10.0, 8.0),
                ],
                stroke,
            ));
        }
        Icon::Language => {
            p.circle_stroke(c, 9.0 * s, stroke);
            line(pt(-9.0, 0.0), pt(9.0, 0.0));
            p.add(egui::Shape::ellipse_stroke(c, vec2(4.0, 9.0) * s, stroke));
        }
        Icon::Theme => {
            p.circle_stroke(c, 8.0 * s, stroke);
            line(pt(0.0, -8.0), pt(0.0, 8.0));
            for y in -5..=5 {
                let x = (64.0 - (y * y) as f32).sqrt();
                line(pt(0.0, y as f32), pt(x, y as f32));
            }
        }
        Icon::Mouse => {
            p.rect_stroke(
                Rect::from_center_size(c, vec2(13.0, 21.0) * s),
                6.0 * s,
                stroke,
                egui::StrokeKind::Inside,
            );
            line(pt(0.0, -7.0), pt(0.0, -2.0));
        }
        Icon::Analysis => {
            rect(pt(-8.0, -8.0), pt(8.0, 8.0));
            for n in [-5.0, 0.0, 5.0] {
                line(pt(n, -11.0), pt(n, -8.0));
                line(pt(n, 8.0), pt(n, 11.0));
                line(pt(-11.0, n), pt(-8.0, n));
                line(pt(8.0, n), pt(11.0, n));
            }
        }
        Icon::Info => {
            p.circle_stroke(c, 9.0 * s, stroke);
            line(pt(0.0, -1.0), pt(0.0, 6.0));
            p.circle_filled(pt(0.0, -5.0), s, color);
        }
    }
}

/// Fixed-width right-aligned flyout, independent of command-bar sizing.
pub(super) fn overflow(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let id = ui.id().with("photos_overflow");
    let response = ui.add_sized([40.0, 48.0], egui::Button::new("…").frame(false));
    if response.clicked() {
        ui.memory_mut(|m| m.toggle_popup(id));
    }
    if !ui.memory(|m| m.is_popup_open(id)) {
        return;
    }
    let dark = ui.visuals().dark_mode;
    let area = egui::Area::new(id)
        .enabled(ui.is_enabled())
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::RIGHT_TOP)
        .fixed_pos(response.rect.right_bottom())
        .show(ui.ctx(), |ui| {
            egui::Frame::NONE
                .fill(Color32::from_gray(if dark { 40 } else { 242 }))
                .stroke(Stroke::new(
                    1.0_f32,
                    Color32::from_gray(if dark { 75 } else { 205 }),
                ))
                .shadow(egui::epaint::Shadow {
                    offset: [0, 5],
                    blur: 18,
                    spread: 0,
                    color: Color32::from_black_alpha(40),
                })
                .inner_margin(egui::Margin::symmetric(0, 4))
                .show(ui, |ui| {
                    ui.set_width(160.0);
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    ui.spacing_mut().interact_size.y = 32.0;
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), add);
                });
        });
    if ui.input(|i| {
        i.key_pressed(egui::Key::Escape)
            || i.key_pressed(egui::Key::Enter)
            || i.key_pressed(egui::Key::Space)
    }) || (ui.input(|i| i.pointer.any_released()) && !response.clicked())
        || (response.clicked_elsewhere() && area.response.clicked_elsewhere())
    {
        ui.memory_mut(|m| m.close_popup());
    }
}

pub(super) fn menu_item(ui: &mut egui::Ui, label: &str, icon: Icon) -> egui::Response {
    let (r, response) = ui.allocate_exact_size(
        vec2(ui.available_width().max(160.0), 32.0),
        egui::Sense::click(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    if response.hovered() || response.has_focus() {
        ui.painter()
            .rect_filled(r, 0.0, ui.visuals().widgets.hovered.weak_bg_fill);
    }
    let color = ui.visuals().text_color();
    paint(
        ui.painter(),
        Rect::from_center_size(pos2(r.left() + 20.0, r.center().y), vec2(20.0, 20.0)),
        icon,
        color,
        false,
    );
    ui.painter().text(
        pos2(r.left() + 46.0, r.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(15.0),
        color,
    );
    if response.clicked() {
        ui.memory_mut(|m| m.close_popup());
    }
    response
}

pub(super) fn heading(ui: &mut egui::Ui, label: &str, icon: Icon) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(24.0, 28.0), egui::Sense::hover());
        paint(
            ui.painter(),
            r.shrink(2.0),
            icon,
            ui.visuals().text_color(),
            false,
        );
        ui.heading(label);
    });
}
