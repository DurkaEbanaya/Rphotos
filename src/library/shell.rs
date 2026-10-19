//! Measured library shell. No wrapping command bars or placeholder integrations.
use eframe::egui::{self, pos2, vec2, Color32, Rect, Sense, Stroke};

use super::{ui::Tokens, Language, Library, Section};
use crate::settings::AppSettings;

#[derive(Clone, Copy)]
enum Icon {
    Plus,
    Select,
    Cancel,
    Share,
    Print,
    Copy,
    Trash,
}

fn command(ui: &mut egui::Ui, label: &str, icon: Icon, width: f32, text: bool) -> egui::Response {
    let (r, response) = ui.allocate_exact_size(vec2(width, 48.0), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    if response.is_pointer_button_down_on() {
        ui.painter().rect_filled(r, 0.0, crate::reveal::ACCENT);
    }
    let c = pos2(
        r.left() + if text { 20.0 } else { width / 2.0 },
        r.center().y,
    );
    let s = Stroke::new(1.0_f32, ui.visuals().text_color());
    let p = |x, y| c + vec2(x, y);
    let line = |a, b| {
        ui.painter().line_segment([a, b], s);
    };
    match icon {
        Icon::Plus => {
            line(p(-7.0, 0.0), p(7.0, 0.0));
            line(p(0.0, -7.0), p(0.0, 7.0));
        }
        Icon::Select => {
            ui.painter().rect_stroke(
                Rect::from_center_size(c, vec2(14.0, 14.0)),
                0.0,
                s,
                egui::StrokeKind::Inside,
            );
            line(p(-4.0, 0.0), p(-1.0, 3.0));
            line(p(-1.0, 3.0), p(5.0, -4.0));
        }
        Icon::Cancel => {
            line(p(-6.0, -6.0), p(6.0, 6.0));
            line(p(-6.0, 6.0), p(6.0, -6.0));
        }
        Icon::Share => {
            line(p(-7.0, 6.0), p(-7.0, -3.0));
            line(p(-7.0, 6.0), p(6.0, 6.0));
            line(p(-3.0, 1.0), p(5.0, -6.0));
            line(p(-1.0, -6.0), p(5.0, -6.0));
            line(p(5.0, 0.0), p(5.0, -6.0));
        }
        Icon::Print => {
            ui.painter().rect_stroke(
                Rect::from_center_size(c, vec2(16.0, 8.0)),
                0.0,
                s,
                egui::StrokeKind::Inside,
            );
            ui.painter().rect_stroke(
                Rect::from_min_max(p(-4.0, -9.0), p(4.0, -4.0)),
                0.0,
                s,
                egui::StrokeKind::Inside,
            );
            line(p(-4.0, 3.0), p(-4.0, 9.0));
            line(p(-4.0, 9.0), p(4.0, 9.0));
            line(p(4.0, 3.0), p(4.0, 9.0));
        }
        Icon::Copy => {
            for offset in [vec2(3.0, -3.0), vec2(-2.0, 2.0)] {
                ui.painter().rect_stroke(
                    Rect::from_center_size(c + offset, vec2(10.0, 12.0)),
                    0.0,
                    s,
                    egui::StrokeKind::Inside,
                );
            }
        }
        Icon::Trash => {
            ui.painter().rect_stroke(
                Rect::from_center_size(p(0.0, 2.0), vec2(10.0, 13.0)),
                0.0,
                s,
                egui::StrokeKind::Inside,
            );
            line(p(-8.0, -6.0), p(8.0, -6.0));
            line(p(-3.0, -9.0), p(3.0, -9.0));
        }
    }
    if text {
        let display = match label {
            "Добавить в альбом" => "Добавить в",
            "Add to album" => "Add to",
            _ => label,
        };
        let clip = Rect::from_min_max(pos2(r.left() + 36.0, r.top()), r.max - vec2(4.0, 0.0));
        ui.painter().with_clip_rect(clip).text(
            clip.left_center(),
            egui::Align2::LEFT_CENTER,
            display,
            egui::FontId::proportional(12.0),
            ui.visuals().text_color(),
        );
    }
    response.on_hover_text(label)
}

fn at<R>(ui: &mut egui::Ui, rect: Rect, f: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.set_clip_rect(rect.intersect(ui.clip_rect()));
            ui.spacing_mut().item_spacing.x = 0.0;
            f(ui)
        },
    )
    .inner
}

fn density(ui: &mut egui::Ui, value: u8, current: &mut u8, lang: Language, t: &Tokens) -> bool {
    let label = match value {
        0 => lang.text("Крупные миниатюры", "Large thumbnails"),
        1 => lang.text("Средние миниатюры", "Medium thumbnails"),
        _ => lang.text("Мелкие миниатюры", "Small thumbnails"),
    };
    let (r, response) = ui.allocate_exact_size(vec2(25.0, 44.0), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    let n = usize::from(value) + 1;
    let gap = 2.0;
    let side = (17.0 - gap * (n - 1) as f32) / n as f32;
    let origin = r.center() - vec2(8.5, 8.5);
    for y in 0..n {
        for x in 0..n {
            ui.painter().rect_stroke(
                Rect::from_min_size(
                    origin + vec2(x as f32 * (side + gap), y as f32 * (side + gap)),
                    vec2(side, side),
                ),
                0.0,
                Stroke::new(1.0_f32, t.muted),
                egui::StrokeKind::Inside,
            );
        }
    }
    if *current == value {
        ui.painter().hline(
            r.x_range(),
            r.bottom() - 1.0,
            Stroke::new(2.0_f32, t.accent),
        );
    }
    let clicked = response.on_hover_text(label).clicked();
    if clicked {
        *current = value;
    }
    clicked
}

impl Library {
    pub(super) fn shell(
        &mut self,
        ctx: &egui::Context,
        settings: &mut AppSettings,
        t: &Tokens,
        margin: f32,
        blocked: bool,
    ) {
        let lang = settings.language;
        let selecting = self.selecting;
        let width = ctx.screen_rect().width();
        let narrow = width < 900.0;
        let mut menu_open = false;
        let mut focused_search = None;
        egui::TopBottomPanel::top("library_commands")
            .show_separator_line(false)
            .exact_height(48.0)
            .frame(egui::Frame::NONE.fill(if selecting {
                Color32::from_gray(51)
            } else {
                t.header
            }))
            .show(ctx, |ui| {
                if blocked {
                    ui.disable();
                }
                let r = ui.max_rect();
                ui.spacing_mut().interact_size = vec2(40.0, 48.0);
                ui.spacing_mut().button_padding = vec2(10.0, 0.0);
                if selecting {
                    *ui.visuals_mut() = egui::Visuals::dark();
                    ui.visuals_mut().override_text_color = Some(Color32::WHITE);
                    let right_width = if narrow { 168.0 } else { 400.0 };
                    let right = Rect::from_min_max(
                        pos2(r.right() - margin - right_width, r.top()),
                        pos2(r.right() - margin, r.bottom()),
                    );
                    let count = Rect::from_min_max(
                        pos2(margin, r.top()),
                        pos2(right.left() - 8.0, r.bottom()),
                    );
                    let label = if self.selection.is_empty() {
                        lang.text("Ничего не выбрано", "No item selected").into()
                    } else if self.selection.len() == 1 {
                        lang.text("Выбран 1 снимок", "1 item selected").into()
                    } else {
                        format!(
                            "{} {}",
                            self.selection.len(),
                            lang.text("выбрано", "items selected")
                        )
                    };
                    ui.painter().with_clip_rect(count).text(
                        count.left_center(),
                        egui::Align2::LEFT_CENTER,
                        label,
                        egui::FontId::proportional(14.0),
                        Color32::WHITE,
                    );
                    at(ui, right, |ui| {
                        if ui
                            .add_enabled_ui(!self.selection.is_empty(), |ui| {
                                command(
                                    ui,
                                    lang.text("Добавить в альбом", "Add to album"),
                                    Icon::Plus,
                                    if narrow { 48.0 } else { 120.0 },
                                    !narrow,
                                )
                            })
                            .inner
                            .clicked()
                        {
                            self.assign_album_open = true;
                        }
                        if !narrow {
                            for (icon, label) in [
                                (Icon::Share, lang.text("Поделиться…", "Share…")),
                                (Icon::Print, lang.text("Печать…", "Print…")),
                                (Icon::Copy, lang.text("Копировать файлы", "Copy files")),
                                (Icon::Trash, lang.text("Удалить в корзину", "Move to Trash")),
                            ] {
                                if ui
                                    .add_enabled_ui(
                                        !self.selection.is_empty() && !self.catalog.jobs.busy(),
                                        |ui| command(ui, label, icon, 40.0, false),
                                    )
                                    .inner
                                    .clicked()
                                {
                                    match icon {
                                        Icon::Share => self.catalog.share = true,
                                        Icon::Print => {
                                            let paths = self.selected_paths();
                                            self.catalog.jobs.start(
                                                crate::platform::jobs::Job::Print(paths),
                                                ctx,
                                            );
                                        }
                                        Icon::Copy => self.catalog.jobs.start(crate::platform::jobs::Job::Copy(self.selected_paths()), ctx),
                                        Icon::Trash => {
                                            self.catalog.trash = Some(self.selected_paths())
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        if command(ui, lang.text("Отмена", "Cancel"), Icon::Cancel, 80.0, true)
                            .clicked()
                        {
                            self.cancel_selection();
                        }
                        ui.spacing_mut().menu_margin=egui::Margin::symmetric(0,4);
                        ui.visuals_mut().menu_corner_radius=egui::CornerRadius::ZERO;
                        ui.visuals_mut().window_fill=if ui.visuals().dark_mode {Color32::from_gray(40)} else {Color32::from_gray(242)};
                        ui.visuals_mut().window_stroke=Stroke::new(1.0_f32,Color32::from_gray(if ui.visuals().dark_mode {75}else{205}));
                        super::icons::overflow(ui, |ui| {
                            menu_open = true;
                            // Photos command flyout: flat 32px rows, icon gutter,
                            // pale-gray surface and restrained square border.
                            ui.set_min_width(216.0);
                            ui.spacing_mut().item_spacing.y=0.0;
                            if ui.button(lang.text("Выбрать всё", "Select all")).clicked()
                            {
                                self.select_all();
                                ui.close_menu();
                            }
                            if ui
                                .button(lang.text("Снять выделение", "Unselect all"))
                                .clicked()
                            {
                                self.selection.clear();
                                ui.close_menu();
                            }
                            ui.separator();
                            if let Some(album) = self.album {
                                if ui
                                    .add_enabled(
                                        self.selection.len() == 1,
                                        egui::Button::new(
                                            lang.text("Сделать обложкой", "Set as cover"),
                                        ),
                                    )
                                    .clicked()
                                {
                                    if let Some(&photo) = self.selection.iter().next() {
                                        self.send(super::Command::CoverAlbum(album, photo));
                                    }
                                    ui.close_menu();
                                }
                                if ui
                                    .add_enabled(
                                        !self.selection.is_empty(),
                                        egui::Button::new(
                                            lang.text("Убрать из альбома", "Remove from album"),
                                        ),
                                    )
                                    .clicked()
                                {
                                    self.send(super::Command::UnassignAlbum(
                                        album,
                                        self.selection.iter().copied().collect(),
                                    ));
                                    self.cancel_selection();
                                    ui.close_menu();
                                }
                            }
                            if narrow {
                                if ui.add_enabled(!self.selection.is_empty() && !self.catalog.jobs.busy(), egui::Button::new(lang.text("Копировать файлы", "Copy files"))).clicked() {
                                    self.catalog.jobs.start(crate::platform::jobs::Job::Copy(self.selected_paths()), ctx);
                                    ui.close_menu();
                                }
                                if ui
                                    .add_enabled(
                                        !self.selection.is_empty(),
                                        egui::Button::new(lang.text("Поделиться…", "Share…")),
                                    )
                                    .clicked()
                                {
                                    self.catalog.share = true;
                                    ui.close_menu();
                                }
                                if ui
                                    .add_enabled(
                                        !self.selection.is_empty() && !self.catalog.jobs.busy(),
                                        egui::Button::new(lang.text("Печать…", "Print…")),
                                    )
                                    .clicked()
                                {
                                    let paths = self.selected_paths();
                                    self.catalog
                                        .jobs
                                        .start(crate::platform::jobs::Job::Print(paths), ctx);
                                    ui.close_menu();
                                }
                                if ui
                                    .add_enabled(
                                        !self.selection.is_empty(),
                                        egui::Button::new(
                                            lang.text("Удалить в корзину", "Move to Trash"),
                                        ),
                                    )
                                    .clicked()
                                {
                                    self.catalog.trash = Some(self.selected_paths());
                                    ui.close_menu();
                                }
                            }
                        });
                    });
                } else {
                    let right_width = if narrow { 136.0 } else { 400.0 };
                    let right = Rect::from_min_max(
                        pos2(r.right() - margin - right_width, r.top()),
                        pos2(r.right() - margin, r.bottom()),
                    );
                    let search_width = (right.left() - margin - 16.0).clamp(96.0, 396.0);
                    let search =
                        Rect::from_min_size(pos2(margin, r.top() + 6.0), vec2(search_width, 36.0));
                    at(ui, search, |ui| {
                        if self.semantic_zoom { ui.disable(); }
                        let response = ui.add_sized(
                            search.size(),
                            egui::TextEdit::singleline(&mut self.query)
                                .id(egui::Id::new("library_search"))
                                .margin(vec2(10.0, 9.0))
                                .hint_text(
                                    lang.text("Поиск по имени, пути или тексту", "Search filename, path or text"),
                                ),
                        );
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::TextEdit,
                                ui.is_enabled(),
                                lang.text("Поиск по имени или пути", "Search filename or path"),
                            )
                        });
                        crate::reveal::exclude(ui.ctx(), search);
                        if response.changed() {
                            self.content_search.hits = None;
                            self.layout_key = None;
                        }
                        if response.gained_focus() || response.clicked() {
                            self.discovery.search_open = true;
                        }
                        if self.discovery.search_open && !blocked && !self.semantic_zoom {
                            if ctx.input(|i| i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Enter)) {
                                self.discovery.search_open = false;
                                response.surrender_focus();
                            } else {
                                focused_search = Some(search);
                            }
                        }
                        response.on_hover_text(lang.text("Имя и путь; текст на фото — после локального OCR в настройках.","Filename and path; image text becomes searchable after local OCR in Settings."));
                        // TextEdit response bounds cover its inner text region;
                        // retain the measured outer slot for geometry regression.
                        #[cfg(test)]
                        {
                            self.search_rect = Some(search);
                        }
                    });
                    at(ui, right, |ui| {
                        if !narrow
                            && ui.add_enabled_ui(!self.semantic_zoom, |ui| command(
                                ui,
                                lang.text("Новый альбом", "New album"),
                                Icon::Plus,
                                120.0,
                                true,
                            )).inner
                            .clicked()
                        {
                            self.start_picker(None);
                        }
                        if ui.add_enabled_ui(!self.semantic_zoom, |ui| command(ui, lang.text("Выбрать", "Select"), Icon::Select, 96.0, true)).inner
                            .clicked()
                        {
                            self.selecting = true;
                            self.selection.clear();
                        }
                        if !narrow
                            && ui.add_enabled_ui(!self.semantic_zoom, |ui| command(
                                ui,
                                lang.text("Добавить папку", "Add folder"),
                                Icon::Plus,
                                144.0,
                                true,
                            )).inner
                            .clicked()
                        {
                            self.pick_source(ctx);
                        }
                        ui.spacing_mut().menu_margin=egui::Margin::symmetric(0,4);
                        ui.visuals_mut().menu_corner_radius=egui::CornerRadius::ZERO;
                        ui.visuals_mut().window_fill=if ui.visuals().dark_mode {Color32::from_gray(40)} else {Color32::from_gray(242)};
                        ui.visuals_mut().window_stroke=Stroke::new(1.0_f32,Color32::from_gray(if ui.visuals().dark_mode {75}else{205}));
                        super::icons::overflow(ui, |ui| {
                            menu_open = true;
                            if narrow {
                                if ui.button(lang.text("Новый альбом", "New album")).clicked()
                                {
                                    self.start_picker(None);
                                    ui.close_menu();
                                }
                                if ui
                                    .button(lang.text("Добавить папку", "Add folder"))
                                    .clicked()
                                {
                                    self.pick_source(ctx);
                                    ui.close_menu();
                                }
                                ui.separator();
                            }
                            if super::icons::menu_item(ui,lang.text("Обновить · F5", "Refresh · F5"),super::icons::Icon::Refresh).clicked()
                            {
                                self.refresh();
                                ui.close_menu();
                            }
                            if super::icons::menu_item(ui,lang.text("Слайд-шоу", "Slideshow"),super::icons::Icon::Slideshow).clicked() {
                                self.catalog.slideshow=true;
                                ui.close_menu();
                            }
                            if super::icons::menu_item(ui,lang.text("Настройки", "Settings"),super::icons::Icon::Settings).clicked() {
                                self.settings_open = true;
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.add_enabled(!self.state.photos.is_empty()&&!self.thumbnails.bulk_status().running,egui::Button::new(lang.text("Прогрузить все миниатюры", "Cache all thumbnails"))).on_hover_text(lang.text("Вся библиотека. Дисковый кеш без ограничения; потребуется дополнительное место. Оригиналы не изменяются.","Entire library. Unlimited disk cache; requires additional disk space. Originals are unchanged.")).clicked(){
                                settings.thumbnail_disk_gib=0;self.thumbnails.disk_quota(0);
                                self.thumbnails.configure(settings.thumbnail_workers);
                                 self.thumbnails.build_all(&self.state.photos);
                                 ui.memory_mut(|m|m.close_popup());
                             }
                             if ui.button(lang.text("Ошибки кеша миниатюр", "Thumbnail cache errors")).clicked() {
                                 self.cache_errors_open = true;
                                 ui.memory_mut(|m|m.close_popup());
                             }
                            if !self.query.trim().is_empty() && ui.add_enabled(!self.scanning && !settings.local_embedding_model.trim().is_empty(), egui::Button::new(lang.text("Смысловой поиск по этому запросу", "Semantic search for this query"))).clicked() {
                                let ticket = self.worker.ticket();
                                self.scanning = true;
                                self.section = Section::Collection;
                                self.album = None;
                                self.folder = None;
                                self.send(super::Command::SemanticSearch(ticket, settings.local_embedding_model.clone(), self.query.clone()));
                                ui.close_menu();
                            }
                            if self.content_search.hits.is_some() && ui.button(lang.text("Вернуться к поиску по словам", "Return to keyword search")).clicked() {
                                self.content_search.hits = None;
                                self.layout_key = None;
                                ui.close_menu();
                            }
                            if !self.query.trim().is_empty() && ui.button(lang.text("Сохранить поиск", "Save search")).clicked()
                            {
                                self.send(super::Command::SaveQuery(
                                    self.query.chars().take(120).collect(),
                                    self.query.clone(),
                                ));
                                ui.close_menu();
                            }
                            let has_query=!self.query.trim().is_empty();
                            for search in self.state.searches.clone().into_iter().filter(|_| has_query) {
                                if ui
                                    .button(format!(
                                        "{}: {}",
                                        lang.text("Поиск", "Search"),
                                        search.name
                                    ))
                                    .clicked()
                                {
                                    self.query = search.query;
                                    self.section = Section::Collection;
                                    self.album = None;
                                    self.folder = None;
                                    ui.close_menu();
                                }
                            }
                        });
                    });
                }
            });
        // Legacy egui menus do not report visibility through any_popup_open;
        // record the actual content callback so Escape cannot also leave selection.
        self.popup_open_last_frame = menu_open;
        egui::TopBottomPanel::top("library_tabs")
            .exact_height(48.0)
            .frame(egui::Frame::NONE.fill(t.background))
            .show(ctx, |ui| {
                if blocked || self.semantic_zoom {
                    ui.disable();
                }
                let r = ui.max_rect();
                at(
                    ui,
                    Rect::from_min_max(
                        pos2(margin, r.top()),
                        pos2(r.right() - margin - 87.0, r.bottom()),
                    ),
                    |ui| {
                        egui::ScrollArea::horizontal()
                            .id_salt("section_tabs_scroll")
                            .scroll_bar_visibility(
                                egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                            )
                            .max_height(48.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.with_layout(
                                    egui::Layout::left_to_right(egui::Align::Min),
                                    |ui| {
                                        ui.spacing_mut().item_spacing.x = 0.0;
                                        for (section, title) in [
                                            (
                                                Section::Collection,
                                                lang.text("Коллекция", "Collection"),
                                            ),
                                            (Section::Albums, lang.text("Альбомы", "Albums")),
                                            (Section::Folders, lang.text("Папки", "Folders")),
                                            (
                                                Section::Favorites,
                                                lang.text("Избранное", "Favorites"),
                                            ),
                                        ] {
                                            if selecting && self.section != section {
                                                continue;
                                            }
                                            let galley = ui.painter().layout_no_wrap(
                                                title.into(),
                                                egui::FontId::proportional(20.0),
                                                t.text,
                                            );
                                            let (rect, response) = ui.allocate_exact_size(
                                                vec2(galley.size().x + 24.0, 48.0),
                                                Sense::click(),
                                            );
                                            response.widget_info(|| {
                                                egui::WidgetInfo::labeled(
                                                    egui::WidgetType::Button,
                                                    ui.is_enabled(),
                                                    title,
                                                )
                                            });
                                            let text_pos = pos2(
                                                rect.left() + 12.0,
                                                rect.center().y - galley.size().y / 2.0,
                                            );
                                            // Keep the existing generously sized hit target,
                                            // but balance Reveal padding around the text.
                                            crate::reveal::bounds(
                                                &response,
                                                Rect::from_center_size(
                                                    rect.center(),
                                                    vec2(galley.size().x + 16.0, 40.0),
                                                ),
                                            );
                                            let text_width = galley.size().x;
                                            ui.painter().galley(text_pos, galley, t.text);
                                            if self.section == section {
                                                ui.painter().hline(
                                                    (rect.left() + 12.0)
                                                        ..=(rect.left() + 12.0 + text_width),
                                                    rect.bottom() - 3.0,
                                                    Stroke::new(2.0_f32, t.accent),
                                                );
                                            }
                                            if response.clicked() && !selecting {
                                                self.section = section;
                                                self.semantic_zoom = false;
                                                self.scroll_offset = 0.0;
                                                self.scroll_anchor = None;
                                                self.album = None;
                                                self.folder = None;
                                            }
                                        }
                                    },
                                );
                            });
                    },
                );
                if matches!(self.section, Section::Collection | Section::Favorites)
                    || self.album.is_some()
                    || self.folder.is_some()
                {
                    at(
                        ui,
                        Rect::from_min_size(
                            pos2(r.right() - margin - 75.0, r.top() + 2.0),
                            vec2(75.0, 44.0),
                        ),
                        |ui| {
                            for value in 0..=2 {
                                if density(ui, value, &mut settings.thumbnail_density, lang, t) {
                                    settings.thumbnail_size = 0.0;
                                }
                            }
                        },
                    );
                }
            });
        if let Some(search) = focused_search {
            self.search_suggestions(ctx, search, lang);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{kittest::Queryable, Harness};

    #[test]
    fn graphical_density_switch_changes_value_without_settings_io() {
        let mut h = Harness::builder().build_state(
            |ctx, current| {
                let t = Tokens::new(super::super::Appearance::Light, ctx);
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        for value in 0..=2 {
                            density(ui, value, current, Language::En, &t);
                        }
                    });
                });
            },
            1_u8,
        );
        h.run_steps(2);
        h.get_by_label("Small thumbnails").click();
        h.run_steps(2);
        assert_eq!(*h.state(), 2);
        h.get_by_label("Large thumbnails").click();
        h.run_steps(2);
        assert_eq!(*h.state(), 0);
    }
}
