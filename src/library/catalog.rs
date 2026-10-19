//! Album/folder scenes. All file mutations remain on the database worker.
use super::{ui::Tokens, Command, Language, Library, Section};
use crate::settings::AppSettings;
use eframe::egui::{self, pos2, vec2, Color32, Rect, RichText, Sense};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Default)]
pub(super) struct Catalog {
    pub jobs: crate::platform::jobs::Jobs,
    pub share: bool,
    pub trash: Option<Vec<PathBuf>>,
    pub trash_ready: Option<Vec<PathBuf>>,
    pub rename: Option<String>,
    pub remove: Option<i64>,
    pub picker: Option<AlbumPicker>,
    pub notice: Option<i64>,
    pub sort: u8,
    pub assign_new: bool,
    pub slideshow: bool,
}

pub(super) struct AlbumPicker {
    pub folders: bool,
    pub folder: Option<PathBuf>,
    pub density: u8,
    pub album: Option<i64>,
    pub selected: std::collections::HashSet<i64>,
    pub name: String,
    pub query: String,
}

impl Library {
    pub(crate) fn take_trash(&mut self) -> Option<Vec<PathBuf>> {
        self.catalog.trash_ready.take()
    }
    pub(super) fn selected_paths(&self) -> Vec<PathBuf> {
        self.state
            .photos
            .iter()
            .filter(|p| self.selection.contains(&p.id))
            .map(|p| p.path.clone())
            .collect()
    }
    pub(super) fn back_scene(&mut self) {
        if self.catalog.picker.take().is_some() {
            return;
        }
        if self.semantic_zoom {
            self.semantic_zoom = false;
            return;
        }
        if self.settings_open {
            self.settings_open = false;
            return;
        }
        if self.catalog.rename.take().is_some() {
            return;
        }
        self.album = None;
        self.folder = None;
        self.catalog.rename = None;
        self.cancel_selection();
        self.scroll_offset = 0.0;
        self.scroll_anchor = None;
    }

    pub(super) fn enter_album(&mut self, id: i64) {
        self.section = Section::Albums;
        self.album = Some(id);
        self.folder = None;
        self.cancel_selection();
        self.scroll_offset = 0.0;
        self.scroll_anchor = None;
        self.query.clear();
    }

    pub(super) fn start_picker(&mut self, album: Option<i64>) {
        self.catalog.picker = Some(AlbumPicker {
            folders: false,
            folder: None,
            density: 1,
            album,
            selected: Default::default(),
            name: String::new(),
            query: String::new(),
        });
    }

    pub(super) fn catalog_commands(
        &mut self,
        ctx: &egui::Context,
        settings: &AppSettings,
        t: &Tokens,
        blocked: bool,
    ) -> bool {
        let lang = settings.language;
        if self.catalog.picker.is_some() {
            return false;
        }
        if self.section != Section::Albums || self.album.is_none() || self.selecting {
            return false;
        }
        let id = self.album.unwrap();
        let width = ctx.screen_rect().width();
        egui::TopBottomPanel::top("album_commands")
            .exact_height(48.0)
            .frame(egui::Frame::NONE.fill(t.header))
            .show(ctx, |ui| {
                if blocked || self.semantic_zoom {
                    ui.disable();
                }
                ui.spacing_mut().interact_size.y = 48.0;
                ui.spacing_mut().button_padding = vec2(16.0, 0.0);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    if self.catalog.rename.is_some() {
                        if ui
                            .add_enabled(
                                self.catalog.rename.as_ref().is_some_and(|n| {
                                    !n.trim().is_empty() && n.chars().count() <= 120
                                }),
                                egui::Button::new(lang.text("Готово", "Done")),
                            )
                            .clicked()
                        {
                            if let Some(name) = self.catalog.rename.take() {
                                self.send(Command::RenameAlbum(id, name));
                            }
                        }
                        if ui.button(lang.text("Отмена", "Cancel")).clicked() {
                            self.catalog.rename = None;
                        }
                    } else {
                        if width >= 900.0
                            && ui.button(lang.text("Показ слайдов", "Slideshow")).clicked()
                        {
                            self.catalog.slideshow = true;
                        }
                        if ui
                            .button(lang.text("Добавить фотографии", "Add photos"))
                            .clicked()
                        {
                            self.start_picker(Some(id));
                        }
                        if width >= 900.0 && ui.button(lang.text("Поделиться…", "Share…")).clicked()
                        {
                            self.selection = self
                                .state
                                .photos
                                .iter()
                                .filter(|p| p.albums.contains(&id))
                                .map(|p| p.id)
                                .collect();
                            self.catalog.share = true;
                        }
                        if ui.button(lang.text("Выбрать", "Select")).clicked() {
                            self.selecting = true;
                            self.selection.clear();
                        }
                        ui.menu_button("…", |ui| {
                            self.popup_open_last_frame = true;
                            if width < 900.0 {
                                if ui.button(lang.text("Показ слайдов", "Slideshow")).clicked()
                                {
                                    self.catalog.slideshow = true;
                                    ui.close_menu();
                                }
                                if ui.button(lang.text("Поделиться…", "Share…")).clicked()
                                {
                                    self.selection = self
                                        .state
                                        .photos
                                        .iter()
                                        .filter(|p| p.albums.contains(&id))
                                        .map(|p| p.id)
                                        .collect();
                                    self.catalog.share = true;
                                    ui.close_menu();
                                }
                            }
                            if ui.button(lang.text("Переименовать", "Rename")).clicked()
                            {
                                self.catalog.rename = self
                                    .state
                                    .albums
                                    .iter()
                                    .find(|a| a.id == id)
                                    .map(|a| a.name.clone());
                                ui.close_menu();
                            }
                            if ui
                                .button(lang.text("Удалить альбом", "Remove album"))
                                .clicked()
                            {
                                self.catalog.remove = Some(id);
                                ui.close_menu();
                            }
                        });
                    }
                });
            });
        true
    }

    pub(super) fn notice(&mut self, ctx: &egui::Context, lang: Language, t: &Tokens) {
        if let Some(id) = self.catalog.notice {
            let name = self
                .state
                .albums
                .iter()
                .find(|a| a.id == id)
                .map(|a| a.name.as_str())
                .unwrap_or("");
            let mut view = false;
            let mut close = false;
            egui::TopBottomPanel::top("album_notice")
                .exact_height(44.0)
                .frame(egui::Frame::NONE.fill(t.tile))
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(24.0);
                        ui.label(format!(
                            "{} {name}",
                            lang.text("Альбом обновлён:", "Album updated:")
                        ));
                        view = ui
                            .button(lang.text("Посмотреть альбом", "View album"))
                            .clicked();
                        close = ui
                            .button(lang.text("Закрыть уведомление", "Close notification"))
                            .clicked();
                    });
                });
            if view {
                self.enter_album(id);
            }
            if view || close {
                self.catalog.notice = None;
            }
        }
    }

    pub(super) fn albums(&mut self, ui: &mut egui::Ui, lang: Language, t: &Tokens, margin: f32) {
        ui.horizontal(|ui| {
            ui.add_space(margin);
            ui.heading(format!(
                "{} ({})",
                lang.text("Альбомы", "Albums"),
                self.state.albums.len()
            ));
            egui::ComboBox::from_id_salt("album_sort")
                .selected_text(match self.catalog.sort {
                    1 => lang.text("Название", "Name"),
                    2 => lang.text("Сначала старые", "Oldest first"),
                    _ => lang.text("Изменённые", "Modified"),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.catalog.sort,
                        0,
                        lang.text("Изменённые", "Modified"),
                    );
                    ui.selectable_value(&mut self.catalog.sort, 1, lang.text("Название", "Name"));
                    ui.selectable_value(
                        &mut self.catalog.sort,
                        2,
                        lang.text("Сначала старые", "Oldest first"),
                    );
                });
        });
        let width = (ui.available_width() - 2.0 * margin - 12.0).max(1.0);
        let columns = ((width + 12.0) / 315.0).floor().max(1.0) as usize;
        let tile_width = ((width - 12.0 * (columns - 1) as f32) / columns as f32).min(360.0);
        let mut albums = self.state.albums.clone();
        match self.catalog.sort {
            1 => albums.sort_by_key(|a| a.name.to_lowercase()),
            2 => albums.sort_by_key(|a| (a.modified, a.id)),
            _ => {}
        }
        let rows = (albums.len() + 1).div_ceil(columns);
        ui.spacing_mut().item_spacing.y = 0.0;
        egui::ScrollArea::vertical()
            .id_salt("albums")
            .show_rows(ui, 210.0, rows, |ui, range| {
                for row in range {
                    let origin = ui.cursor().min + vec2(margin, 0.0);
                    ui.allocate_space(vec2(width, 210.0));
                    for col in 0..columns {
                        let slot = row * columns + col;
                        if slot > albums.len() {
                            break;
                        }
                        let rect = Rect::from_min_size(
                            origin + vec2(col as f32 * (tile_width + 12.0), 0.0),
                            vec2(tile_width, 198.0),
                        );
                        if slot == 0 {
                            if new_tile(
                                ui,
                                rect,
                                lang.text("+ Новый альбом", "+ New album"),
                                lang.text("Соберите любимые снимки", "Bring your photos together"),
                                t,
                            ) {
                                self.start_picker(None);
                            }
                        } else {
                            let album = &albums[slot - 1];
                            let indices: Vec<_> = self
                                .state
                                .photos
                                .iter()
                                .enumerate()
                                .filter(|(_, p)| {
                                    p.albums.contains(&album.id) && !self.thumbnails.unavailable(p)
                                })
                                .map(|(i, _)| i)
                                .collect();
                            let cover = album
                                .cover
                                .and_then(|id| {
                                    indices
                                        .iter()
                                        .find(|&&i| self.state.photos[i].id == id)
                                        .copied()
                                })
                                .or_else(|| indices.first().copied());
                            let image = Rect::from_min_size(rect.min, vec2(rect.width(), 140.0));
                            self.paint_cover(ui, image, cover, t);
                            ui.painter().rect_filled(
                                Rect::from_min_max(image.left_bottom(), rect.max),
                                0.0,
                                t.tile,
                            );
                            text(
                                ui,
                                rect.min + vec2(12.0, 153.0),
                                &album.name,
                                16.0,
                                t.text,
                                rect,
                            );
                            text(
                                ui,
                                rect.min + vec2(12.0, 176.0),
                                &format!(
                                    "{} {}{}",
                                    indices.len(),
                                    lang.text("снимков", "photos"),
                                    if album.automatic {
                                        lang.text(" · авто", " · auto")
                                    } else {
                                        ""
                                    }
                                ),
                                12.0,
                                t.muted,
                                rect,
                            );
                            let response = ui.interact(
                                rect,
                                ui.id().with(("album", album.id)),
                                Sense::click(),
                            );
                            response.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    ui.is_enabled(),
                                    &album.name,
                                )
                            });
                            if response.clicked() {
                                self.enter_album(album.id);
                            }
                        }
                    }
                }
            });
    }

    pub(super) fn album_heading(
        &mut self,
        ui: &mut egui::Ui,
        lang: Language,
        t: &Tokens,
        margin: f32,
    ) {
        let Some(album) = self
            .state
            .albums
            .iter()
            .find(|a| Some(a.id) == self.album)
            .cloned()
        else {
            return;
        };
        let width = (ui.available_width() - 2.0 * margin - 12.0).max(1.0);
        if ui.available_height() > 600.0 {
            ui.add_space(36.0);
        }
        ui.horizontal(|ui| {
            ui.add_space(margin);
            ui.allocate_ui_with_layout(
                vec2(width, 76.0),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    if let Some(name) = &mut self.catalog.rename {
                        ui.add(
                            egui::TextEdit::singleline(name).font(egui::FontId::proportional(36.0)),
                        );
                    } else if ui
                        .add(egui::Button::new(RichText::new(&album.name).size(42.0)).frame(false))
                        .on_hover_text(lang.text("Переименовать", "Rename"))
                        .clicked()
                    {
                        self.catalog.rename = Some(album.name.clone());
                    }
                },
            );
        });
        let cover = album
            .cover
            .and_then(|id| {
                self.state.photos.iter().position(|p| {
                    p.id == id && p.albums.contains(&album.id) && !self.thumbnails.unavailable(p)
                })
            })
            .or_else(|| {
                self.state
                    .photos
                    .iter()
                    .position(|p| p.albums.contains(&album.id) && !self.thumbnails.unavailable(p))
            });
        if !self.selecting {
            let r = Rect::from_min_size(
                ui.cursor().min + vec2(margin + 24.0, 0.0),
                vec2(
                    (width - 48.0).max(1.0),
                    (width * 0.31)
                        .min(ui.available_height() * 0.55)
                        .clamp(60.0, 426.0),
                ),
            );
            ui.allocate_space(vec2(width, r.height() + 16.0));
            self.paint_cover(ui, r, cover, t);
        }
    }

    pub(super) fn folders(&mut self, ui: &mut egui::Ui, lang: Language, t: &Tokens, margin: f32) {
        let mut folders: BTreeMap<PathBuf, Option<usize>> = BTreeMap::new();
        if let Some(current) = &self.folder {
            folders = self.state.child_folders(current);
            ui.horizontal(|ui| {
                ui.add_space(margin);
                ui.label(current.to_string_lossy());
            });
            ui.horizontal(|ui| {
                ui.add_space(margin);
                if ui
                    .link(lang.text("Вернуться ко всем папкам", "Go back to all folders"))
                    .clicked()
                {
                    self.folder = None;
                } else if ui
                    .link(lang.text("На уровень выше", "Parent folder"))
                    .clicked()
                {
                    self.folder = self
                        .folder
                        .as_ref()
                        .and_then(|f| f.parent())
                        .filter(|parent| {
                            self.state
                                .sources
                                .iter()
                                .any(|s| parent.starts_with(&s.path))
                        })
                        .map(|p| p.to_path_buf());
                }
            });
        } else {
            for source in &self.state.sources {
                folders.insert(
                    source.path.clone(),
                    self.state
                        .photos
                        .iter()
                        .position(|p| p.path.starts_with(&source.path)),
                );
            }
        }
        let width = (ui.available_width() - 2.0 * margin - 12.0).max(1.0);
        let columns = ((width + 12.0) / 318.0).floor().max(1.0) as usize;
        let tw = ((width - 12.0 * (columns - 1) as f32) / columns as f32).min(380.0);
        let extra = usize::from(self.folder.is_none());
        let count = folders.len() + extra;
        let entries: Vec<_> = folders.into_iter().collect();
        let rows = count.div_ceil(columns);
        let height = if self.folder.is_some() {
            (rows as f32 * 140.0)
                .min(ui.available_height() * 0.4)
                .max(1.0)
        } else {
            ui.available_height().max(1.0)
        };
        egui::ScrollArea::vertical()
            .id_salt(("folder_tiles", &self.folder))
            .max_height(height)
            .show_rows(ui, 140.0, rows, |ui, range| {
                for row in range {
                    let origin = ui.cursor().min + vec2(margin, 0.0);
                    ui.allocate_space(vec2(width, 140.0));
                    for col in 0..columns {
                        let slot = row * columns + col;
                        if slot >= count {
                            break;
                        }
                        let rect = Rect::from_min_size(
                            origin + vec2(col as f32 * (tw + 12.0), 0.0),
                            vec2(tw, 128.0),
                        );
                        if slot < extra {
                            if new_tile(
                                ui,
                                rect,
                                lang.text("+ Добавить папку", "+ Add folder"),
                                "",
                                t,
                            ) {
                                self.pick_source(ui.ctx());
                            }
                        } else {
                            let (path, cover) = &entries[slot - extra];
                            self.paint_cover(
                                ui,
                                Rect::from_min_size(rect.min, vec2(90.0, 128.0)),
                                *cover,
                                t,
                            );
                            ui.painter().rect_filled(
                                Rect::from_min_max(rect.min + vec2(90.0, 0.0), rect.max),
                                0.0,
                                t.tile,
                            );
                            text(
                                ui,
                                rect.min + vec2(102.0, 30.0),
                                &path.file_name().unwrap_or_default().to_string_lossy(),
                                16.0,
                                t.text,
                                rect,
                            );
                            text(
                                ui,
                                rect.min + vec2(102.0, 64.0),
                                &path.to_string_lossy(),
                                12.0,
                                t.muted,
                                rect,
                            );
                            let label = format!(
                                "{} {}",
                                lang.text("Открыть папку", "Open folder"),
                                path.display()
                            );
                            let response =
                                ui.interact(rect, ui.id().with(("folder", path)), Sense::click());
                            response.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    ui.is_enabled(),
                                    &label,
                                )
                            });
                            if response.clicked() {
                                self.folder = Some(path.clone());
                                self.scroll_anchor = None;
                                self.scroll_offset = 0.0;
                            }
                        }
                    }
                }
            });
    }

    pub(super) fn paint_cover(
        &mut self,
        ui: &egui::Ui,
        rect: Rect,
        index: Option<usize>,
        t: &Tokens,
    ) {
        ui.painter().rect_filled(rect, 0.0, t.tile);
        if let Some(texture) = index.and_then(|i| self.thumbnails.get(&self.state.photos[i])) {
            let size = texture.size_vec2();
            let scale = (rect.width() / size.x).max(rect.height() / size.y);
            ui.painter().image(
                texture.id(),
                rect,
                Rect::from_center_size(pos2(0.5, 0.5), rect.size() / (size * scale)),
                Color32::WHITE,
            );
        }
    }

    pub(super) fn album_picker(
        &mut self,
        ctx: &egui::Context,
        settings: &AppSettings,
        t: &Tokens,
        blocked: bool,
    ) {
        let lang = settings.language;
        let mut picker = self.catalog.picker.take().unwrap();
        let mut done = false;
        let mut cancel = false;
        egui::TopBottomPanel::top("album_picker_commands")
            .exact_height(48.0)
            .frame(egui::Frame::NONE.fill(t.header))
            .show(ctx, |ui| {
                if blocked {
                    ui.disable();
                }
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    ui.label(
                        RichText::new(if picker.album.is_some() {
                            lang.text("Добавление фотографий", "Add photos to album")
                        } else {
                            lang.text("Создание альбома", "Create new album")
                        })
                        .size(20.0),
                    );
                    done = ui
                        .add_enabled(
                            !picker.selected.is_empty()
                                && (picker.album.is_some()
                                    || (!picker.name.trim().is_empty()
                                        && picker.name.chars().count() <= 120)),
                            egui::Button::new(lang.text("Добавить", "Add")),
                        )
                        .clicked();
                    cancel = ui.button(lang.text("Отмена", "Cancel")).clicked();
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(t.background).inner_margin(24))
            .show(ctx, |ui| {
                if blocked {
                    ui.disable();
                }
                if picker.album.is_none() {
                    ui.add(
                        egui::TextEdit::singleline(&mut picker.name)
                            .hint_text(lang.text("Название альбома", "Album name")),
                    );
                }
                ui.add_sized(
                    [396.0_f32.min(ui.available_width()), 36.0],
                    egui::TextEdit::singleline(&mut picker.query)
                        .hint_text(lang.text("Найти фотографию", "Find photo")),
                );
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut picker.folders,
                        false,
                        lang.text("Коллекция", "Collection"),
                    );
                    ui.selectable_value(&mut picker.folders, true, lang.text("Папки", "Folders"));
                    for (value, label) in [
                        (0, lang.text("Крупные", "Large")),
                        (1, lang.text("Средние", "Medium")),
                        (2, lang.text("Мелкие", "Small")),
                    ] {
                        ui.selectable_value(&mut picker.density, value, label);
                    }
                });
                if picker.folders {
                    ui.horizontal_wrapped(|ui| {
                        if let Some(folder) = picker.folder.clone() {
                            ui.label(folder.to_string_lossy());
                            if ui
                                .button(lang.text("Назад к источникам", "Back to sources"))
                                .clicked()
                            {
                                picker.folder = None;
                            }
                            for child in self.state.child_folders(&folder).into_keys() {
                                if ui
                                    .button(child.file_name().unwrap_or_default().to_string_lossy())
                                    .clicked()
                                {
                                    picker.folder = Some(child);
                                }
                            }
                        } else {
                            for source in &self.state.sources {
                                if ui.button(source.path.to_string_lossy()).clicked() {
                                    picker.folder = Some(source.path.clone());
                                }
                            }
                        }
                    });
                }
                ui.label(format!(
                    "{}: {}",
                    lang.text("Выбрано", "Selected"),
                    picker.selected.len()
                ));
                let query = picker.query.to_lowercase();
                let indices: Vec<_> = self
                    .state
                    .photos
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| {
                        !self.thumbnails.unavailable(p)
                            && (!picker.folders
                                || picker.folder.as_ref().is_some_and(|folder| {
                                    p.path.parent() == Some(folder.as_path())
                                }))
                            && (p.path.to_string_lossy().to_lowercase().contains(&query)
                                || p.text.to_lowercase().contains(&query))
                    })
                    .map(|(i, _)| i)
                    .collect();
                let items: Vec<_> = indices
                    .iter()
                    .map(|&i| (i, self.thumbnails.aspect(&self.state.photos[i])))
                    .collect();
                let gap = if picker.density == 2 { 6.0 } else { 12.0 };
                let target = match picker.density {
                    0 => 512.0,
                    1 => 256.0,
                    _ => 96.0,
                };
                let rows = super::layout::rows(&items, ui.available_width() - 12.0, target, gap);
                let mut offsets = vec![0.0];
                for row in &rows {
                    offsets.push(offsets.last().unwrap() + row.height + gap);
                }
                egui::ScrollArea::vertical()
                    .id_salt("album_photo_picker")
                    .auto_shrink([false, false])
                    .show_viewport(ui, |ui, viewport| {
                        ui.set_height(*offsets.last().unwrap());
                        let origin = ui.max_rect().min;
                        let start = offsets
                            .partition_point(|y| *y <= viewport.min.y)
                            .saturating_sub(1);
                        let end = offsets
                            .partition_point(|y| *y < viewport.max.y)
                            .min(rows.len());
                        for row in start..end {
                            for cell in &rows[row].cells {
                                let i = cell.index;
                                let r = Rect::from_min_size(
                                    origin + vec2(cell.x, offsets[row]),
                                    vec2(cell.width, rows[row].height),
                                );
                                let response = ui.interact(
                                    r,
                                    ui.id().with(("picker_photo", self.state.photos[i].id)),
                                    Sense::click(),
                                );
                                self.paint_cover(ui, r, Some(i), t);
                                let p = &self.state.photos[i];
                                response.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::WidgetType::Button,
                                        ui.is_enabled(),
                                        p.path.file_name().unwrap_or_default().to_string_lossy(),
                                    )
                                });
                                if picker.selected.contains(&p.id) {
                                    ui.painter().rect_stroke(
                                        r,
                                        0.0,
                                        egui::Stroke::new(4.0_f32, t.accent),
                                        egui::StrokeKind::Inside,
                                    );
                                }
                                if response.clicked() && !picker.selected.insert(p.id) {
                                    picker.selected.remove(&p.id);
                                }
                            }
                        }
                    });
            });
        if done {
            let photos = picker.selected.into_iter().collect();
            if let Some(id) = picker.album {
                self.send(Command::AssignAlbum(id, photos));
            } else {
                self.send(Command::CreateAlbumPhotos(picker.name, photos));
            }
        } else if !cancel {
            self.catalog.picker = Some(picker);
        }
    }

    pub(super) fn catalog_dialogs(&mut self, ctx: &egui::Context, lang: Language) {
        self.catalog.jobs.show(ctx, lang);
        if self.catalog.share {
            let paths = self.selected_paths();
            if self.catalog.jobs.share(ctx, lang, &paths) {
                self.catalog.share = false;
            }
        }
        if let Some(paths) = self.catalog.trash.clone() {
            let response = egui::Modal::new(egui::Id::new("library_trash")).show(ctx, |ui| {
                ui.set_max_width(360.0);
                ui.heading(lang.text("Переместить в корзину?", "Move to Trash?"));
                ui.label(format!(
                    "{} {}",
                    paths.len(),
                    lang.text("снимков", "photos")
                ));
                let yes = ui.button(lang.text("В корзину", "Move to Trash")).clicked();
                let no = ui.button(lang.text("Отмена", "Cancel")).clicked();
                (yes, no)
            });
            if response.inner.0 {
                self.catalog.trash_ready = Some(paths);
                self.cancel_selection();
            }
            if response.inner.0 || response.inner.1 || response.should_close() {
                self.catalog.trash = None;
            }
        }
        if let Some(id) = self.catalog.remove {
            let response = egui::Modal::new(egui::Id::new("remove_album")).show(ctx, |ui| {
                ui.set_max_width(360.0);
                ui.heading(lang.text("Удалить альбом?", "Remove album?"));
                ui.label(lang.text(
                    "Фотографии на диске не будут удалены.",
                    "Photos on disk will not be deleted.",
                ));
                let remove = ui
                    .button(lang.text("Удалить альбом", "Remove album"))
                    .clicked();
                let cancel = ui.button(lang.text("Отмена", "Cancel")).clicked();
                (remove, cancel)
            });
            if response.inner.0 {
                self.send(Command::RemoveAlbum(id));
            }
            if response.inner.0 || response.inner.1 || response.should_close() {
                self.catalog.remove = None;
            }
        }
    }
}

fn text(ui: &egui::Ui, pos: egui::Pos2, value: &str, size: f32, color: Color32, clip: Rect) {
    ui.painter().with_clip_rect(clip.shrink(4.0)).text(
        pos,
        egui::Align2::LEFT_TOP,
        value,
        egui::FontId::proportional(size),
        color,
    );
}
fn new_tile(ui: &mut egui::Ui, rect: Rect, title: &str, subtitle: &str, t: &Tokens) -> bool {
    ui.painter().rect_filled(rect, 0.0, t.accent);
    text(
        ui,
        rect.min + vec2(16.0, 32.0),
        title,
        20.0,
        Color32::WHITE,
        rect,
    );
    text(
        ui,
        rect.min + vec2(16.0, 74.0),
        subtitle,
        14.0,
        Color32::WHITE,
        rect,
    );
    let response = ui.interact(rect, ui.id().with(title), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), title)
    });
    response.clicked()
}
