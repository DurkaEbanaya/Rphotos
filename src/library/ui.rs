//! Photos-inspired shell: real actions, native chrome, opaque backdrop fallback.
//! Token/state approach informed by Silver Native GUI Lab (MIT OR Apache-2.0);
//! no Silver source/assets or version-specific 0.36 widgets are embedded.

use std::sync::atomic::Ordering;

use eframe::egui::{self, Color32};

use super::{Appearance, Command, Language, Library, OpenPhoto, Section};
use crate::settings::AppSettings;

pub(crate) struct Tokens {
    pub(crate) background: Color32,
    pub(crate) header: Color32,
    pub(crate) tile: Color32,
    pub(crate) text: Color32,
    pub(crate) muted: Color32,
    pub(crate) accent: Color32,
    dark: bool,
}

impl Tokens {
    pub(crate) fn new(appearance: Appearance, ctx: &egui::Context) -> Self {
        let dark = match appearance {
            Appearance::System => ctx.input(|i| i.raw.system_theme == Some(egui::Theme::Dark)),
            Appearance::Light => false,
            Appearance::Dark => true,
        };
        let translucent = crate::platform::backdrop::active(ctx);
        Self {
            background: if dark {
                Color32::from_gray(24)
            } else {
                Color32::WHITE
            },
            header: if dark {
                if translucent {
                    Color32::from_rgba_unmultiplied(40, 40, 40, 190)
                } else {
                    Color32::from_gray(40)
                }
            } else {
                if translucent {
                    Color32::from_rgba_unmultiplied(238, 238, 238, 190)
                } else {
                    Color32::from_gray(238)
                }
            },
            tile: if dark {
                Color32::from_gray(45)
            } else {
                Color32::from_gray(242)
            },
            text: if dark {
                Color32::from_gray(245)
            } else {
                Color32::from_gray(30)
            },
            muted: if dark {
                Color32::from_gray(175)
            } else {
                Color32::from_gray(100)
            },
            accent: Color32::from_rgb(0, 120, 215),
            dark,
        }
    }

    pub(crate) fn apply(&self, ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();
        style.visuals = if self.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        style.visuals.panel_fill = self.background;
        style.visuals.override_text_color = Some(self.text);
        style.visuals.selection.bg_fill = self.accent;
        style.visuals.selection.stroke.color = Color32::WHITE;
        style.visuals.hyperlink_color = self.accent;
        style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::ZERO;
        style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::ZERO;
        style.visuals.widgets.active.corner_radius = egui::CornerRadius::ZERO;
        style.spacing.interact_size.y = 36.0;
        style.spacing.button_padding = egui::vec2(12.0, 8.0);
        // Keep a usable drag target even when content is millions of points tall.
        style.spacing.scroll.handle_min_length = 64.0;
        style.spacing.scroll.bar_width = 12.0;
        ctx.set_style(style);
    }
}

impl Library {
    #[cfg(test)]
    pub fn show(&mut self, ctx: &egui::Context, settings: &mut AppSettings) -> Option<OpenPhoto> {
        self.show_blocked(ctx, settings, false)
    }

    pub(crate) fn show_blocked(
        &mut self,
        ctx: &egui::Context,
        settings: &mut AppSettings,
        external_modal: bool,
    ) -> Option<OpenPhoto> {
        let lang = settings.language;
        let t = Tokens::new(settings.appearance, ctx);
        t.apply(ctx);
        self.thumbnails.disk_quota(settings.thumbnail_disk_gib);
        let before = (
            settings.language,
            settings.appearance,
            settings.thumbnail_density,
            settings.thumbnail_size,
            settings.thumbnail_workers,
            settings.thumbnail_disk_gib,
            settings.mouse_wheel_zoom,
            settings.local_vision_model.clone(),
            settings.local_embedding_model.clone(),
        );
        let scale_before = settings.thumbnail_size;
        if !external_modal {
            self.grid_scale_input(ctx, settings);
        }
        let mut opened = None;
        let margin = 16.0;

        let modal_open = external_modal
            || self.cache_errors_open
            || self.create_album_open
            || self.assign_album_open
            || self.remove_source.is_some()
            || self.catalog.remove.is_some()
            || self.catalog.share
            || self.catalog.trash.is_some()
            || self.catalog.jobs.message.is_some()
            || self.error.is_some();
        if crate::chrome::show(
            ctx,
            "Rphotos",
            if self.semantic_zoom {
                Some(lang.text("Назад к фотографиям", "Back to photos"))
            } else if self.settings_open
                || self.catalog.picker.is_some()
                || self.album.is_some()
                || self.folder.is_some()
            {
                Some(lang.text("Назад", "Back"))
            } else {
                None
            },
            settings.appearance,
            lang,
            modal_open,
        ) {
            self.back_scene();
        }
        if !self.ready {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(t.background))
                .show(ctx, |ui| {
                    ui.centered_and_justified(|ui| {
                        ui.spinner();
                    });
                });
            return None;
        }
        if self.settings_open {
            if !modal_open
                && !ctx.wants_keyboard_input()
                && ctx.input(|i| i.key_pressed(egui::Key::Escape))
            {
                self.settings_open = false;
            }
            self.preferences(ctx, settings, &t);
            self.dialogs(ctx, settings, lang);
            if before
                != (
                    settings.language,
                    settings.appearance,
                    settings.thumbnail_density,
                    settings.thumbnail_size,
                    settings.thumbnail_workers,
                    settings.thumbnail_disk_gib,
                    settings.mouse_wheel_zoom,
                    settings.local_vision_model.clone(),
                    settings.local_embedding_model.clone(),
                )
            {
                settings.save();
            }
            return None;
        }
        if self.catalog.picker.is_some() {
            if !modal_open
                && !ctx.wants_keyboard_input()
                && ctx.input(|i| i.key_pressed(egui::Key::Escape))
            {
                self.catalog.picker = None;
                ctx.request_repaint();
                return None;
            }
            self.album_picker(ctx, settings, &t, modal_open);
            self.dialogs(ctx, settings, lang);
            return None;
        }
        if !ctx.wants_keyboard_input()
            && !ctx.memory(|m| m.any_popup_open())
            && !self.popup_open_last_frame
            && !modal_open
            && !self.settings_open
        {
            if !self.semantic_zoom && ctx.input(|i| i.key_pressed(egui::Key::F5)) {
                self.refresh();
            }
            if !self.semantic_zoom
                && ctx.input(|i| i.key_pressed(egui::Key::O) && i.modifiers.command)
            {
                self.pick_source(ctx);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                if self.semantic_zoom {
                    self.semantic_zoom = false;
                } else if self.selecting {
                    self.cancel_selection();
                } else {
                    self.back_scene();
                }
            }
            if !self.semantic_zoom
                && ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::A))
            {
                self.select_all();
            }
            let section = ctx.input(|i| {
                if !i.modifiers.command || self.semantic_zoom {
                    return None;
                }
                if i.key_pressed(egui::Key::Num1) {
                    Some(Section::Collection)
                } else if i.key_pressed(egui::Key::Num2) {
                    Some(Section::Albums)
                } else if i.key_pressed(egui::Key::Num3) {
                    Some(Section::Folders)
                } else if i.key_pressed(egui::Key::Num4) {
                    Some(Section::Favorites)
                } else {
                    None
                }
            });
            if let Some(section) = section {
                self.cancel_selection();
                self.semantic_zoom = false;
                self.scroll_offset = 0.0;
                self.scroll_anchor = None;
                self.section = section;
                self.album = None;
                self.folder = None;
            }
        }

        self.popup_open_last_frame = false;
        if !self.catalog_commands(ctx, settings, &t, modal_open) {
            self.shell(ctx, settings, &t, margin, modal_open);
        }
        self.notice(ctx, lang, &t);

        if self.scanning {
            egui::TopBottomPanel::bottom("library_status")
                .exact_height(48.0)
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    if modal_open {
                        ui.disable();
                    }
                    let action = super::cache_status::scan(
                        ui,
                        self.worker.progress.load(Ordering::Relaxed),
                        self.worker.total.load(Ordering::Relaxed),
                        self.worker.paused(),
                        lang,
                        &t,
                        margin,
                    );
                    match action {
                        super::cache_status::Action::Stop => {
                            self.worker.cancel();
                            self.scanning = false;
                        }
                        super::cache_status::Action::Pause => self.worker.toggle_pause(),
                        _ => {}
                    }
                });
        }
        let cache = self.thumbnails.bulk_status();
        if cache.total > 0 {
            egui::TopBottomPanel::bottom("thumbnail_build_status")
                .exact_height(48.0)
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    if modal_open {
                        ui.disable();
                    }
                    let action = super::cache_status::show(ui, cache, lang, &t, margin);
                    self.cache_task_action(action);
                });
        }

        self.pager(ctx, 128.0, modal_open);
        let pager_offset = self.pager.offset;
        egui::CentralPanel::default()
            .frame(
                egui::Frame::default()
                    .fill(t.background)
                    .inner_margin(egui::Margin::symmetric(0, 16)),
            )
            .show(ctx, |ui| {
                self.pager_preview(ui, settings, &t, margin);
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(ui.max_rect().translate(egui::vec2(pager_offset, 0.0))),
                    |ui| {
                        if modal_open || self.pager.consumed {
                            ui.disable();
                        }
                        ui.set_clip_rect(ui.clip_rect().intersect(ui.max_rect()));
                        if self.content_search.query == self.query
                            && self.content_search.hits.is_some()
                        {
                            ui.horizontal(|ui| {
                                ui.add_space(margin);
                                ui.label(lang.text(
                                    "Смысловой поиск · результаты по датам",
                                    "Semantic search · results by date",
                                ));
                                if ui.button(lang.text("По словам", "Keywords")).clicked() {
                                    self.content_search.hits = None;
                                    self.layout_key = None;
                                }
                            });
                        }
                        opened = self.section_content(ui, settings, &t, margin);
                    },
                );
            });

        self.dialogs(ctx, settings, lang);
        if std::mem::take(&mut self.catalog.slideshow) {
            if let Some(&index) = self.filtered.first() {
                let mut photo = self.open(index);
                photo.slideshow = true;
                opened = Some(photo);
            }
        }
        if before
            != (
                settings.language,
                settings.appearance,
                settings.thumbnail_density,
                settings.thumbnail_size,
                settings.thumbnail_workers,
                settings.thumbnail_disk_gib,
                settings.mouse_wheel_zoom,
                settings.local_vision_model.clone(),
                settings.local_embedding_model.clone(),
            )
        {
            if settings.thumbnail_size != scale_before
                && ctx.input(|i| (i.zoom_delta() - 1.0).abs() > 0.0001)
            {
                ctx.data_mut(|d| {
                    d.insert_temp(
                        egui::Id::new("library.grid.save_after"),
                        std::time::Instant::now(),
                    )
                });
            } else {
                settings.save();
            }
        }
        let save_id = egui::Id::new("library.grid.save_after");
        if let Some(last) = ctx.data(|d| d.get_temp::<std::time::Instant>(save_id)) {
            if last.elapsed() >= std::time::Duration::from_millis(500) {
                settings.save();
                ctx.data_mut(|d| d.remove::<std::time::Instant>(save_id));
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(500));
            }
        }
        opened
    }

    pub(super) fn section_content(
        &mut self,
        ui: &mut egui::Ui,
        settings: &AppSettings,
        tokens: &Tokens,
        margin: f32,
    ) -> Option<OpenPhoto> {
        let lang = settings.language;
        if self.section == Section::Albums && self.album.is_none() {
            self.albums(ui, lang, tokens, margin);
        } else if self.section == Section::Folders {
            let has_folder = self.folder.is_some();
            self.folders(ui, lang, tokens, margin);
            if has_folder {
                return self.gallery(ui, settings, tokens, margin);
            }
        } else {
            if self.album.is_some() {
                self.album_heading(ui, lang, tokens, margin);
            }
            return self.gallery(ui, settings, tokens, margin);
        }
        None
    }

    pub(super) fn dialogs(
        &mut self,
        ctx: &egui::Context,
        settings: &mut AppSettings,
        lang: Language,
    ) {
        self.catalog_dialogs(ctx, lang);
        if self.cache_errors_open {
            let failures = self.thumbnails.failures();
            let mut close = false;
            let response = egui::Modal::new(egui::Id::new("thumbnail_errors")).show(ctx, |ui| {
                ui.set_width((ctx.screen_rect().width() - 48.0).clamp(240.0, 1000.0));
                ui.heading(lang.text("Ошибки кеша миниатюр", "Thumbnail cache errors"));
                ui.label(lang.text("Отчёт хранится локально. Исходные фотографии не изменены.", "The report stays local. Original photos are unchanged."));
                ui.label(self.thumbnails.directory().join("last-build-errors.json").to_string_lossy());
                if failures.is_empty() { ui.label(lang.text("Записанных ошибок пока нет. Для старой прогрузки без отчёта повторите команду кеширования.", "No recorded errors yet. For older builds without a report, run caching again.")); }
                egui::ScrollArea::vertical().auto_shrink([false,true]).max_height((ctx.screen_rect().height() - 230.0).max(100.0)).show(ui, |ui| {
                    for (i,error) in failures.iter().enumerate() {
                        ui.push_id(i, |ui| {
                            ui.label(egui::RichText::new(error.path.file_name().unwrap_or_default().to_string_lossy()).strong());
                            ui.add(egui::Label::new(error.path.to_string_lossy()).wrap());
                            ui.add(egui::Label::new(&error.reason).wrap());
                            if i+1<failures.len() { ui.separator(); }
                        });
                    }
                });
                close = ui.button(lang.text("Закрыть", "Close")).clicked();
            });
            if close || response.should_close() {
                self.cache_errors_open = false;
            }
        }

        if self.create_album_open {
            let mut save = false;
            let response = egui::Modal::new(egui::Id::new("create_album")).show(ctx, |ui| {
                ui.set_max_width((ctx.screen_rect().width() - 64.0).clamp(160.0, 400.0));
                ui.heading(lang.text("Новый альбом", "New album"));
                ui.add(
                    egui::TextEdit::singleline(&mut self.album_name)
                        .hint_text(lang.text("Название", "Name")),
                );
                save = ui
                    .add_enabled(
                        !self.album_name.trim().is_empty()
                            && self.album_name.chars().count() <= 120,
                        egui::Button::new(lang.text("Создать", "Create")),
                    )
                    .clicked();
            });
            if response.should_close() {
                self.create_album_open = false;
            }
            if save {
                let name = std::mem::take(&mut self.album_name);
                self.send(Command::CreateAlbum(name));
                self.create_album_open = false;
            }
        }
        if self.assign_album_open {
            let mut chosen = None;
            let width = (ctx.screen_rect().width() - 48.0).clamp(200.0, 360.0);
            let height = (ctx.screen_rect().height() - 64.0).clamp(200.0, 648.0);
            let dark = Tokens::new(settings.appearance, ctx).dark;
            let response = egui::Modal::new(egui::Id::new("assign_album"))
                .backdrop_color(if dark {
                    Color32::from_black_alpha(110)
                } else {
                    Color32::from_white_alpha(160)
                })
                .frame(
                    egui::Frame::NONE
                        .fill(if dark {
                            Color32::from_gray(40)
                        } else {
                            Color32::from_gray(242)
                        })
                        .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(0, 120, 215)))
                        .inner_margin(24),
                )
                .show(ctx, |ui| {
                    ui.set_width(width - 48.0);
                    ui.set_min_height(height - 48.0);
                    ui.heading(lang.text("Добавить в альбом", "Add to album"));
                    ui.label(format!(
                        "{}: {}",
                        lang.text("Выбрано", "Selected"),
                        self.selection.len()
                    ));
                    if !self.catalog.assign_new
                        && ui
                            .button(lang.text("+ Новый альбом", "+ New album"))
                            .clicked()
                    {
                        self.catalog.assign_new = true;
                        self.album_name.clear();
                    }
                    if self.catalog.assign_new {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.album_name)
                                .hint_text(lang.text("Название альбома", "Album name")),
                        );
                        if ui
                            .add_enabled(
                                !self.album_name.trim().is_empty()
                                    && self.album_name.chars().count() <= 120,
                                egui::Button::new(lang.text("Создать альбом", "Create album")),
                            )
                            .clicked()
                        {
                            let name = std::mem::take(&mut self.album_name);
                            self.send(Command::CreateAlbumPhotos(
                                name,
                                self.selection.iter().copied().collect(),
                            ));
                            self.assign_album_open = false;
                            self.catalog.assign_new = false;
                        }
                        if ui.button(lang.text("Отмена", "Cancel")).clicked() {
                            self.catalog.assign_new = false;
                        }
                    }
                    egui::ScrollArea::vertical()
                        .max_height(
                            (height
                                - if self.catalog.assign_new {
                                    260.0
                                } else {
                                    170.0
                                })
                            .max(80.0),
                        )
                        .show(ui, |ui| {
                            for album in self.state.albums.clone() {
                                let (rect, response) = ui.allocate_exact_size(
                                    egui::vec2(ui.available_width(), 56.0),
                                    egui::Sense::click(),
                                );
                                let cover = album
                                    .cover
                                    .and_then(|id| {
                                        self.state.photos.iter().position(|p| p.id == id)
                                    })
                                    .or_else(|| {
                                        self.state
                                            .photos
                                            .iter()
                                            .position(|p| p.albums.contains(&album.id))
                                    });
                                let t = Tokens::new(settings.appearance, ctx);
                                self.paint_cover(
                                    ui,
                                    egui::Rect::from_min_size(rect.min, egui::vec2(48.0, 48.0)),
                                    cover,
                                    &t,
                                );
                                ui.painter().with_clip_rect(rect).text(
                                    rect.left_center() + egui::vec2(60.0, 0.0),
                                    egui::Align2::LEFT_CENTER,
                                    &album.name,
                                    egui::FontId::proportional(14.0),
                                    t.text,
                                );
                                response.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::WidgetType::Button,
                                        true,
                                        &album.name,
                                    )
                                });
                                if response.clicked() {
                                    chosen = Some(album.id);
                                }
                            }
                        });
                });
            if response.should_close() {
                self.assign_album_open = false;
                self.catalog.assign_new = false;
            }
            if let Some(album) = chosen {
                self.send(Command::AssignAlbum(
                    album,
                    self.selection.iter().copied().collect(),
                ));
                self.assign_album_open = false;
            }
        }
        if let Some(id) = self.remove_source {
            let mut close = false;
            let mut remove = false;
            let response = egui::Modal::new(egui::Id::new("remove_source")).show(ctx, |ui| {
                ui.set_max_width((ctx.screen_rect().width() - 64.0).clamp(160.0, 400.0));
                ui.heading(lang.text("Убрать источник?", "Remove source?"));
                ui.label(lang.text(
                    "Из библиотеки исчезнут снимки этой папки. Файлы на диске не удаляются.",
                    "This folder's photos will leave the library. Files on disk are not deleted.",
                ));
                remove = ui
                    .button(lang.text("Убрать из библиотеки", "Remove from library"))
                    .clicked();
                close = ui.button(lang.text("Отмена", "Cancel")).clicked();
            });
            if remove {
                self.worker.cancel();
                self.scanning = false;
                self.send(Command::RemoveSource(id));
            }
            if close || remove || response.should_close() {
                self.remove_source = None;
            }
        }
        if let Some(error) = self.error.clone() {
            let response = egui::Modal::new(egui::Id::new("library_notice")).show(ctx, |ui| {
                ui.set_max_width((ctx.screen_rect().width() - 64.0).clamp(160.0, 500.0));
                ui.heading(lang.text("Сообщение библиотеки", "Library notice"));
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        ui.label(error);
                    });
                ui.button(lang.text("Закрыть", "Close")).clicked()
            });
            if response.inner || response.should_close() {
                self.error = None;
            }
        }
    }
}
