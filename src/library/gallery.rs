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
        // Only a slim date rail remains reserved; no centered 120px gutters.
        let rail_width = if ui.available_width() >= 900.0 {
            88.0
        } else {
            0.0
        };
        let width = (ui.available_width() - 2.0 * margin - 12.0 - rail_width).max(1.0);
        self.thumbnails.configure(settings.thumbnail_workers);
        let target = if settings.thumbnail_size > 0.0 {
            settings.thumbnail_size
        } else {
            match settings.thumbnail_density {
                0 => 512.0,
                1 => 256.0,
                _ => 96.0,
            }
        };
        // Decode arrivals must not reflow a 30k/100k-photo grid on every frame.
        // Batch aspect discovery, and never move rows under an active scroll.
        let id = egui::Id::new("library.aspect_reflow");
        let now = std::time::Instant::now();
        let (mut last_scroll, mut quiet, mut applied) = ui
            .ctx()
            .data(|d| d.get_temp::<(f32, std::time::Instant, std::time::Instant)>(id))
            .unwrap_or((self.scroll_offset, now, now));
        if (last_scroll - self.scroll_offset).abs() > 0.5
            || ui.input(|i| i.smooth_scroll_delta.y.abs() > 0.1 || i.pointer.any_down())
        {
            last_scroll = self.scroll_offset;
            quiet = now;
        }
        let geometry_changed = self.layout_key.as_ref().is_none_or(|k| {
            k.0 != width.to_bits()
                || k.1 != target.clamp(64.0, 640.0).to_bits()
                || k.3 != self.query
                || k.4 != self.section
                || k.5 != self.album
                || k.6 != self.folder
        });
        let aspects_changed = self
            .layout_key
            .as_ref()
            .is_some_and(|k| k.2 != self.thumbnails.revision);
        if geometry_changed
            || (aspects_changed
                && quiet.elapsed().as_millis() >= 180
                && applied.elapsed().as_millis() >= 600)
        {
            self.rebuild_rows_at(width, target);
            applied = now;
        } else if aspects_changed {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(200));
        }
        ui.ctx()
            .data_mut(|d| d.insert_temp(id, (last_scroll, quiet, applied)));
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
                        let galley = ui.painter().layout_no_wrap(
                            label.clone(),
                            egui::FontId::proportional(22.0),
                            t.accent,
                        );
                        let text_rect = Rect::from_min_size(
                            pos2(r.left(), r.center().y - galley.size().y / 2.0),
                            galley.size(),
                        );
                        crate::reveal::bounds(&response, text_rect.expand2(vec2(0.0, 8.0)));
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                ui.is_enabled(),
                                &label,
                            )
                        });
                        ui.painter().galley(text_rect.min, galley, t.accent);
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
                // Two/three screens ahead and one behind. New viewport plans
                // replace speculative queues immediately when direction changes.
                let down = self.thumbnails.forward(viewport.min.y);
                let ahead_height = viewport.height() * 8.0;
                let before = if down {
                    viewport.height() * 3.0
                } else {
                    ahead_height
                };
                let after = if down {
                    ahead_height
                } else {
                    viewport.height() * 3.0
                };
                let first = self
                    .row_offsets
                    .partition_point(|y| *y + prefix < viewport.min.y - before)
                    .saturating_sub(1);
                let last = self
                    .row_offsets
                    .partition_point(|y| *y + prefix < viewport.max.y + after)
                    .min(self.rows.len());
                let mut visible = Vec::new();
                let mut ahead = Vec::new();
                for row in start..end {
                    if let GridRow::Photos(row) = &self.rows[row] {
                        visible.extend(row.cells.iter().map(|c| c.index));
                    }
                }
                let ranges: Vec<usize> = if down {
                    (end..last).chain((first..start).rev()).collect()
                } else {
                    (first..start).rev().chain(end..last).collect()
                };
                for row in ranges {
                    if let GridRow::Photos(row) = &self.rows[row] {
                        ahead.extend(row.cells.iter().map(|c| c.index));
                    }
                }
                self.thumbnails.plan(&self.state.photos, &visible, &ahead);
                self.thumbnails.upload_visible(ui.ctx());
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
                            let text = ui.painter().layout_no_wrap(
                                label.clone(),
                                egui::FontId::proportional(22.0),
                                t.accent,
                            );
                            crate::reveal::bounds(
                                &response,
                                Rect::from_min_size(rect.min, text.size() + vec2(0.0, 8.0)),
                            );
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
                                // Drawing needs identity/path, not a potentially
                                // 256KiB OCR/description string on every frame.
                                let source = &self.state.photos[c.index];
                                if self.thumbnails.unavailable(source) {
                                    continue;
                                }
                                let photo = super::store::Photo {
                                    id: source.id,
                                    path: source.path.clone(),
                                    key: source.key.clone(),
                                    modified: source.modified,
                                    taken: source.taken,
                                    albums: Vec::new(),
                                    text: String::new(),
                                };
                                let favorite = self.state.favorites.contains(&photo.id);
                                let heart_rect = Rect::from_min_size(
                                    rect.right_top() + vec2(-40.0, 4.0),
                                    vec2(36.0, 36.0),
                                );
                                let available = !self.thumbnails.unavailable(&photo);
                                let show_heart = available
                                    && !self.selecting
                                    && (favorite
                                        || ui.rect_contains_pointer(rect)
                                        || self.focused_photo == Some(photo.id));
                                let response = ui.interact(
                                    rect,
                                    ui.id().with(("photo", photo.id)),
                                    Sense::click(),
                                );
                                // Overlay is registered last so pointer hit testing gives
                                // it priority over the underlying photo response.
                                let heart = show_heart.then(|| {
                                    let response = ui.interact(
                                        heart_rect,
                                        ui.id().with(("favorite", photo.id)),
                                        Sense::click(),
                                    );
                                    response.widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::WidgetType::Button,
                                            ui.is_enabled(),
                                            format!(
                                                "{} {}",
                                                lang.text(
                                                    if favorite {
                                                        "Убрать из избранного:"
                                                    } else {
                                                        "В избранное:"
                                                    },
                                                    if favorite {
                                                        "Remove favorite:"
                                                    } else {
                                                        "Favorite:"
                                                    }
                                                ),
                                                photo
                                                    .path
                                                    .file_name()
                                                    .unwrap_or_default()
                                                    .to_string_lossy()
                                            ),
                                        )
                                    });
                                    response
                                });
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
                                if selected || self.focused_photo == Some(photo.id) {
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
                                if self.focused_photo == Some(photo.id) && ui.is_enabled() {
                                    crate::reveal::focus(ui.painter(), rect);
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
                                if response.clicked()
                                    && available
                                    && !heart.as_ref().is_some_and(|r| r.clicked() || r.hovered())
                                    && !ui.input(|i| i.multi_touch().is_some())
                                {
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
                                if let Some(heart) = heart {
                                    if let Some(frost) = self.thumbnails.frost(&photo) {
                                        let size = frost.size_vec2();
                                        let scale =
                                            (rect.width() / size.x).max(rect.height() / size.y);
                                        let uv_size = rect.size() / (size * scale);
                                        let uv = Rect::from_center_size(pos2(0.5, 0.5), uv_size);
                                        let relative = (heart_rect.min - rect.min) / rect.size();
                                        let extent = heart_rect.size() / rect.size();
                                        let min = uv.min + relative * uv.size();
                                        ui.painter().image(
                                            frost.id(),
                                            heart_rect,
                                            Rect::from_min_size(min, extent * uv.size()),
                                            Color32::WHITE,
                                        );
                                    }
                                    ui.painter().rect_filled(
                                        heart_rect,
                                        3.0,
                                        Color32::from_rgba_unmultiplied(245, 245, 245, 130),
                                    );
                                    ui.painter().rect_stroke(
                                        heart_rect,
                                        3.0,
                                        Stroke::new(1.0_f32, Color32::from_white_alpha(130)),
                                        egui::StrokeKind::Inside,
                                    );
                                    super::icons::paint(
                                        ui.painter(),
                                        heart_rect.shrink(6.0),
                                        super::icons::Icon::Heart,
                                        if favorite {
                                            Color32::from_rgb(220, 35, 55)
                                        } else {
                                            Color32::from_gray(70)
                                        },
                                        favorite,
                                    );
                                    if heart.clicked() {
                                        self.send(super::Command::Favorite(photo.id));
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
                                    if ui
                                        .add_enabled(
                                            available,
                                            egui::Button::new(lang.text("Открыть", "Open")),
                                        )
                                        .clicked()
                                    {
                                        opened = Some(self.open(c.index));
                                        ui.close_menu();
                                    }
                                    if ui
                                        .add_enabled(
                                            !self.catalog.jobs.busy(),
                                            egui::Button::new(lang.text(
                                                "Показать в проводнике",
                                                "Show in file manager",
                                            )),
                                        )
                                        .clicked()
                                    {
                                        self.catalog.jobs.start(
                                            crate::platform::jobs::Job::Reveal(photo.path.clone()),
                                            ui.ctx(),
                                        );
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
        ui.ctx()
            .data_mut(|d| d.insert_temp(egui::Id::new("library.grid.viewport"), output.inner_rect));
        #[cfg(test)]
        {
            self.grid_right = output.inner_rect.right();
        }
        if !self.months.is_empty() && rail_width > 0.0 {
            let rail = Rect::from_min_max(
                pos2(
                    output.inner_rect.right() - rail_width,
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
                for n in 0..count {
                    let index = if count <= 1 {
                        0
                    } else {
                        n * (self.months.len() - 1) / (count - 1)
                    };
                    let y = rail.top()
                        + 16.0
                        + if count > 1 {
                            n as f32 / (count - 1) as f32 * span
                        } else {
                            0.0
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
                        if index == 0
                            || m.key.get(..4)
                                != self.months.get(index + 1).and_then(|m| m.key.get(..4))
                        {
                            3.5
                        } else {
                            1.5
                        },
                        t.muted,
                    );
                    if index + 1 == self.months.len()
                        || m.key.get(..4) != self.months.get(index + 1).and_then(|m| m.key.get(..4))
                    {
                        ui.painter().text(
                            pos2(x - 8.0, y),
                            egui::Align2::RIGHT_CENTER,
                            m.key.get(..4).unwrap_or("—"),
                            egui::FontId::proportional(12.0),
                            t.muted,
                        );
                    }
                    if response.on_hover_text(label).clicked() {
                        self.scroll_to_month = Some(m.key.clone());
                        ui.ctx().request_repaint();
                    }
                }
                // Interpolate *within* the current date interval: no static dot
                // stuck on its month while hundreds of images are scrolling.
                let month_start = self.months[active].offset + prefix;
                let month_end = self
                    .months
                    .get(active + 1)
                    .map(|m| m.offset + prefix)
                    .unwrap_or(prefix + self.row_offsets.last().copied().unwrap_or(0.0));
                let within = ((self.scroll_offset - month_start)
                    / (month_end - month_start).max(1.0))
                .clamp(0.0, 1.0);
                let max_scroll = (prefix + self.row_offsets.last().copied().unwrap_or(0.0)
                    - output.inner_rect.height())
                .max(1.0);
                let fraction = if prefix + self.row_offsets.last().copied().unwrap_or(0.0)
                    <= output.inner_rect.height()
                {
                    0.0
                } else if self.months.len() == 1 {
                    (self.scroll_offset / max_scroll).clamp(0.0, 1.0)
                } else if self.scroll_offset >= max_scroll - 1.0 {
                    1.0
                } else {
                    ((active as f32 + within) / self.months.len().saturating_sub(1).max(1) as f32)
                        .clamp(0.0, 1.0)
                };
                let y = rail.top() + 16.0 + fraction * span;
                ui.painter().circle_filled(pos2(x, y), 6.0, t.background);
                ui.painter()
                    .circle_stroke(pos2(x, y), 6.0, Stroke::new(1.0_f32, t.text));
                let current_date = month_label(&self.months[active].key, lang);
                // The active date is a label, not a hover-only tooltip. Give it
                // its own backing so year ticks cannot make it unreadable.
                let label_rect = Rect::from_min_size(pos2(x - 72.0, y - 17.0), vec2(65.0, 34.0));
                crate::reveal::exclude(ui.ctx(), label_rect);
                ui.painter().rect_filled(label_rect, 0.0, t.background);
                ui.painter().text(
                    label_rect.right_center() - vec2(3.0, 0.0),
                    egui::Align2::RIGHT_CENTER,
                    current_date.rsplit_once(' ').map_or_else(
                        || current_date.clone(),
                        |(month, year)| format!("{month}\n{year}"),
                    ),
                    egui::FontId::proportional(12.0),
                    t.text,
                );
                let marker = ui.interact(
                    Rect::from_center_size(pos2(x, y), vec2(24.0, 24.0)),
                    ui.id().with("timeline_current_date"),
                    Sense::hover(),
                );
                marker.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Label,
                        ui.is_enabled(),
                        format!(
                            "{}: {current_date}",
                            lang.text("Текущая дата", "Current date")
                        ),
                    )
                });
                marker.on_hover_text(&current_date);
                #[cfg(test)]
                {
                    self.timeline_marker = Some(y);
                    self.timeline_date = Some(current_date);
                }
            }
        }
        opened
    }
}
