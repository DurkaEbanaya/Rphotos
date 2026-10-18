//! Virtualized aspect-aware gallery, month chooser and calendar rail.
use super::{date_key, month_label, ui::Tokens, GridRow, Library, OpenPhoto};
use crate::settings::AppSettings;
use eframe::egui::{self, pos2, vec2, Color32, Rect, Sense, Stroke};

impl Library {
    pub(super) fn gallery(
        &mut self,
        ui: &mut egui::Ui,
        settings: &AppSettings,
        t: &Tokens,
        margin: f32,
    ) -> Option<OpenPhoto> {
        let lang = settings.language;
        let prefix = self.discovery_height();
        let width = (ui.available_width() - 2.0 * margin - 12.0).max(1.0);
        self.rebuild_rows(width, settings.thumbnail_density);
        let keyboard_open = self.grid_keyboard(ui, prefix);
        if self.semantic_zoom {
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::ScrollArea::vertical()
                .id_salt("month_chooser")
                .show_rows(ui, 48.0, self.months.len(), |ui, range| {
                    for i in range {
                        let m = &self.months[i];
                        let label = month_label(&m.key, lang);
                        let r = Rect::from_min_size(
                            ui.cursor().min + vec2(margin, 0.0),
                            vec2(width, 48.0),
                        );
                        let response = ui.allocate_rect(r, Sense::click());
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                ui.is_enabled(),
                                &label,
                            )
                        });
                        ui.painter().text(
                            r.left_center(),
                            egui::Align2::LEFT_CENTER,
                            label,
                            egui::FontId::proportional(22.0),
                            t.accent,
                        );
                        if response.clicked() {
                            self.scroll_to_month = Some(m.key.clone());
                            self.semantic_zoom = false;
                        }
                    }
                });
            return None;
        }
        if self.filtered.is_empty() {
            ui.add_space(24.0);
            ui.horizontal(|ui| {
                ui.add_space(margin);
                ui.label(lang.text("Здесь пока нет фотографий", "No matching photos yet"));
            });
            if self.state.sources.is_empty() {
                ui.horizontal(|ui| {
                    ui.add_space(margin);
                    if ui
                        .add_sized(
                            [160.0, 40.0],
                            egui::Button::new(lang.text("+ Добавить папку", "+ Add folder")),
                        )
                        .clicked()
                    {
                        self.pick_source(ui.ctx());
                    }
                });
            }
        }
        if let Some(key) = self.scroll_to_month.take() {
            if let Some(m) = self.months.iter().find(|m| m.key == key) {
                self.scroll_offset = m.offset + prefix;
            }
        }
        let mut opened = keyboard_open;
        let output = egui::ScrollArea::vertical()
            .id_salt(("photo_grid", self.section, self.album, &self.folder))
            .auto_shrink([false, false])
            .vertical_scroll_offset(self.scroll_offset)
            .show_viewport(ui, |ui, viewport| {
                ui.set_height(prefix + self.row_offsets.last().copied().unwrap_or(0.0));
                let base = ui.max_rect().min;
                if prefix > 0.0 && viewport.min.y < prefix {
                    ui.scope_builder(
                        egui::UiBuilder::new().max_rect(Rect::from_min_size(
                            base,
                            vec2(ui.available_width(), prefix),
                        )),
                        |ui| self.discovery_cards(ui, lang, t, margin),
                    );
                }
                let origin = base + vec2(margin, prefix);
                let start = self
                    .row_offsets
                    .partition_point(|y| *y + prefix <= viewport.min.y)
                    .saturating_sub(1);
                let end = self
                    .row_offsets
                    .partition_point(|y| *y + prefix < viewport.max.y)
                    .min(self.rows.len());
                self.scroll_anchor = None;
                for row in start..end {
                    let y = origin.y + self.row_offsets[row];
                    match self.rows[row].clone() {
                        GridRow::Month(index) => {
                            let m = &self.months[index];
                            let label = month_label(&m.key, lang);
                            let rect = Rect::from_min_size(pos2(origin.x, y), vec2(width, 30.0));
                            let response =
                                ui.interact(rect, ui.id().with(("month", &m.key)), Sense::click());
                            response.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    ui.is_enabled(),
                                    &label,
                                )
                            });
                            ui.painter().text(
                                rect.left_center(),
                                egui::Align2::LEFT_CENTER,
                                label,
                                egui::FontId::proportional(22.0),
                                t.accent,
                            );
                            if response.clicked() && !self.selecting {
                                self.semantic_zoom = true;
                            }
                            let newest = m
                                .indices
                                .first()
                                .map(|&i| date_key(self.state.photos[i].date()))
                                .unwrap_or_default();
                            let oldest = m
                                .indices
                                .last()
                                .map(|&i| date_key(self.state.photos[i].date()))
                                .unwrap_or_default();
                            let dates = if newest == oldest {
                                newest
                            } else {
                                format!("{oldest} — {newest}")
                            };
                            let sub =
                                Rect::from_min_size(pos2(origin.x, y + 30.0), vec2(width, 24.0));
                            if self.selecting {
                                let all = m
                                    .indices
                                    .iter()
                                    .all(|&i| self.selection.contains(&self.state.photos[i].id));
                                let label = if all {
                                    lang.text("Снять выделение группы", "Unselect group").into()
                                } else {
                                    format!(
                                        "{} {}",
                                        lang.text("Выбрать группу", "Select group"),
                                        m.indices.len()
                                    )
                                };
                                let response = ui.interact(
                                    sub,
                                    ui.id().with(("select_month", index)),
                                    Sense::click(),
                                );
                                response.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::WidgetType::Button,
                                        ui.is_enabled(),
                                        &label,
                                    )
                                });
                                ui.painter().text(
                                    sub.left_center(),
                                    egui::Align2::LEFT_CENTER,
                                    label,
                                    egui::FontId::proportional(14.0),
                                    t.accent,
                                );
                                if response.clicked() {
                                    for &i in &m.indices {
                                        let id = self.state.photos[i].id;
                                        if all {
                                            self.selection.remove(&id);
                                        } else {
                                            self.selection.insert(id);
                                        }
                                    }
                                }
                            } else {
                                ui.painter().text(
                                    sub.left_center(),
                                    egui::Align2::LEFT_CENTER,
                                    format!(
                                        "{dates} · {} {}",
                                        m.indices.len(),
                                        lang.text("снимков", "photos")
                                    ),
                                    egui::FontId::proportional(14.0),
                                    t.muted,
                                );
                            }
                        }
                        GridRow::Photos(r) => {
                            if self.scroll_anchor.is_none() {
                                if let Some(c) = r.cells.first() {
                                    self.scroll_anchor = Some((
                                        self.state.photos[c.index].id,
                                        self.row_offsets[row] + prefix - viewport.min.y,
                                    ));
                                }
                            }
                            for c in r.cells {
                                let rect = Rect::from_min_size(
                                    pos2(origin.x + c.x, y),
                                    vec2(c.width, r.height),
                                );
                                if !ui.is_rect_visible(rect) {
                                    continue;
                                }
                                let photo = self.state.photos[c.index].clone();
                                let response = ui.interact(
                                    rect,
                                    ui.id().with(("photo", photo.id)),
                                    Sense::click(),
                                );
                                response.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::WidgetType::Button,
                                        ui.is_enabled(),
                                        photo
                                            .path
                                            .file_name()
                                            .unwrap_or_default()
                                            .to_string_lossy(),
                                    )
                                });
                                if response.has_focus() {
                                    self.focused_photo_widget = Some(response.id);
                                    self.focused_photo = Some(photo.id);
                                }
                                ui.painter().rect_filled(rect, 0.0, t.tile);
                                if let Some(texture) = self.thumbnails.get(&photo) {
                                    // Bound pathological aspect ratios in layout, center-crop only those.
                                    let size = texture.size_vec2();
                                    let scale = (rect.width() / size.x).max(rect.height() / size.y);
                                    let uv_size = rect.size() / (size * scale);
                                    let uv = Rect::from_center_size(pos2(0.5, 0.5), uv_size);
                                    ui.painter().image(texture.id(), rect, uv, Color32::WHITE);
                                } else if self.thumbnails.failed(&photo) {
                                    ui.painter().text(
                                        rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        lang.text("Нет миниатюры", "No preview"),
                                        egui::FontId::proportional(12.0),
                                        t.muted,
                                    );
                                }
                                let selected = self.selection.contains(&photo.id);
                                if selected
                                    || response.hovered()
                                    || response.has_focus()
                                    || self.focused_photo == Some(photo.id)
                                {
                                    ui.painter().rect_stroke(
                                        rect,
                                        0.0,
                                        Stroke::new(
                                            if selected { 4.0_f32 } else { 1.0_f32 },
                                            t.accent,
                                        ),
                                        egui::StrokeKind::Inside,
                                    );
                                }
                                if self.selecting {
                                    let check = Rect::from_min_size(
                                        rect.right_top() + vec2(-28.0, 4.0),
                                        vec2(24.0, 24.0),
                                    );
                                    ui.painter().rect_filled(
                                        check,
                                        0.0,
                                        if selected { t.accent } else { t.background },
                                    );
                                    if selected {
                                        let p = check.center();
                                        let s = Stroke::new(1.5_f32, Color32::WHITE);
                                        ui.painter().line_segment(
                                            [p + vec2(-6.0, 0.0), p + vec2(-1.0, 5.0)],
                                            s,
                                        );
                                        ui.painter().line_segment(
                                            [p + vec2(-1.0, 5.0), p + vec2(7.0, -5.0)],
                                            s,
                                        );
                                    } else {
                                        ui.painter().rect_stroke(
                                            check,
                                            0.0,
                                            Stroke::new(1.0_f32, t.muted),
                                            egui::StrokeKind::Inside,
                                        );
                                    }
                                }
                                if response.clicked() {
                                    self.focused_photo = Some(photo.id);
                                    if self.selecting || ui.input(|i| i.modifiers.command) {
                                        self.selecting = true;
                                        if !self.selection.insert(photo.id) {
                                            self.selection.remove(&photo.id);
                                        }
                                    } else {
                                        opened = Some(self.open(c.index));
                                    }
                                }
                                response.context_menu(|ui| {
                                    self.popup_open_last_frame = true;
                                    if ui
                                        .button(if self.state.favorites.contains(&photo.id) {
                                            lang.text(
                                                "Убрать из избранного",
                                                "Remove from favorites",
                                            )
                                        } else {
                                            lang.text("Добавить в избранное", "Add to favorites")
                                        })
                                        .clicked()
                                    {
                                        self.send(super::Command::Favorite(photo.id));
                                        ui.close_menu();
                                    }
                                    if ui.button(lang.text("Открыть", "Open")).clicked() {
                                        opened = Some(self.open(c.index));
                                        ui.close_menu();
                                    }
                                    if ui
                                        .button(lang.text("Добавить в альбом", "Add to album"))
                                        .clicked()
                                    {
                                        self.selection.clear();
                                        self.selection.insert(photo.id);
                                        self.assign_album_open = true;
                                        ui.close_menu();
                                    }
                                    if ui.button(lang.text("Поделиться…", "Share…")).clicked()
                                    {
                                        self.selection.clear();
                                        self.selection.insert(photo.id);
                                        self.catalog.share = true;
                                        ui.close_menu();
                                    }
                                    if ui
                                        .add_enabled(
                                            !self.catalog.jobs.busy(),
                                            egui::Button::new(
                                                lang.text("Копировать файл", "Copy file"),
                                            ),
                                        )
                                        .clicked()
                                    {
                                        self.catalog.jobs.start(
                                            crate::platform::jobs::Job::Copy(vec![photo
                                                .path
                                                .clone()]),
                                            ui.ctx(),
                                        );
                                        ui.close_menu();
                                    }
                                    if ui
                                        .button(lang.text("Копировать путь", "Copy path"))
                                        .clicked()
                                    {
                                        ui.ctx()
                                            .copy_text(photo.path.to_string_lossy().into_owned());
                                        ui.close_menu();
                                    }
                                    if ui
                                        .add_enabled(
                                            !self.catalog.jobs.busy(),
                                            egui::Button::new(lang.text("Печать…", "Print…")),
                                        )
                                        .clicked()
                                    {
                                        self.catalog.jobs.start(
                                            crate::platform::jobs::Job::Print(vec![photo
                                                .path
                                                .clone()]),
                                            ui.ctx(),
                                        );
                                        ui.close_menu();
                                    }
                                    if ui
                                        .button(lang.text("Удалить в корзину", "Move to Trash"))
                                        .clicked()
                                    {
                                        self.catalog.trash = Some(vec![photo.path.clone()]);
                                        ui.close_menu();
                                    }
                                    if let Some(album) = self.album {
                                        if ui
                                            .button(lang.text("Сделать обложкой", "Set as cover"))
                                            .clicked()
                                        {
                                            self.send(super::Command::CoverAlbum(album, photo.id));
                                            ui.close_menu();
                                        }
                                    }
                                });
                                response.on_hover_text(photo.path.to_string_lossy());
                            }
                        }
                    }
                }
            });
        self.scroll_offset = output.state.offset.y;
        #[cfg(test)]
        {
            self.grid_right = output.inner_rect.right();
        }
        if !self.months.is_empty() {
            let rail = Rect::from_min_max(
                pos2(
                    output.inner_rect.right() - margin.min(72.0) + 12.0,
                    output.inner_rect.top() + 12.0,
                ),
                pos2(
                    output.inner_rect.right() - 12.0,
                    output.inner_rect.bottom() - 12.0,
                ),
            );
            if rail.width() > 20.0 && rail.height() > 48.0 {
                let x = rail.right() - 8.0;
                ui.painter()
                    .vline(x, rail.y_range(), Stroke::new(1.0_f32, t.muted));
                let active = self
                    .months
                    .partition_point(|m| m.offset + prefix <= self.scroll_offset + 1.0)
                    .saturating_sub(1);
                let count =
                    ((rail.height() / 32.0).floor().max(2.0) as usize).min(self.months.len());
                let span = (rail.height() - 32.0).max(1.0);
                // Dense histories sample their dots, but dragging addresses every
                // month, including ones with no rendered label.
                let drag = ui.interact(
                    Rect::from_min_max(pos2(x - 6.0, rail.top()), pos2(x + 6.0, rail.bottom())),
                    ui.id().with("timeline_scrub"),
                    Sense::drag(),
                );
                if drag.dragged() {
                    if let Some(p) = drag.interact_pointer_pos() {
                        let fraction = ((p.y - rail.top() - 16.0) / span).clamp(0.0, 1.0);
                        let index = (fraction * self.months.len().saturating_sub(1) as f32).round()
                            as usize;
                        self.scroll_to_month = Some(self.months[index].key.clone());
                        ui.ctx().request_repaint();
                    }
                }
                let mut previous_year = None;
                for n in 0..count {
                    let index = if count <= 1 {
                        0
                    } else {
                        n * (self.months.len() - 1) / (count - 1)
                    };
                    let y = rail.top()
                        + 16.0
                        + if self.months.len() <= count {
                            n as f32 * 32.0
                        } else {
                            n as f32 / (count - 1) as f32 * span
                        };
                    let m = &self.months[index];
                    let hit =
                        Rect::from_center_size(pos2(rail.center().x, y), vec2(rail.width(), 32.0));
                    let label = month_label(&m.key, lang);
                    let response =
                        ui.interact(hit, ui.id().with(("timeline", index)), Sense::click());
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            ui.is_enabled(),
                            format!("{} {label}", lang.text("Перейти к", "Jump to")),
                        )
                    });
                    ui.painter().circle_filled(
                        pos2(x, y),
                        if index == active { 5.0 } else { 2.0 },
                        if index == active { t.accent } else { t.muted },
                    );
                    if previous_year != m.key.get(..4) {
                        ui.painter().text(
                            pos2(x - 8.0, y),
                            egui::Align2::RIGHT_CENTER,
                            m.key.get(..4).unwrap_or("—"),
                            egui::FontId::proportional(12.0),
                            t.muted,
                        );
                    }
                    previous_year = m.key.get(..4);
                    if response.on_hover_text(label).clicked() {
                        self.scroll_to_month = Some(m.key.clone());
                        ui.ctx().request_repaint();
                    }
                }
                if self.months.len() > count {
                    let y = rail.top()
                        + 16.0
                        + active as f32 / self.months.len().saturating_sub(1).max(1) as f32 * span;
                    ui.painter().circle_filled(pos2(x, y), 5.0, t.accent);
                }
            }
        }
        opened
    }
}
