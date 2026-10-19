//! Full-page library preferences. Unsupported capabilities are explicit.
use super::{ui::Tokens, Appearance, Command, Language, Library};
use crate::settings::AppSettings;
use eframe::egui::{self, RichText};

fn radio<T: PartialEq>(ui: &mut egui::Ui, current: &mut T, value: T, label: &str) {
    let response = ui.radio_value(current, value, label);
    // Radio strokes extend half a pixel outside their layout rectangle.
    // Leave space around both the circle and the complete translated label.
    crate::reveal::bounds(&response, response.rect.expand(3.0));
}

#[cfg(test)]
#[test]
fn reveal_contains_the_whole_radio_circle_and_long_label() {
    use egui_kittest::{kittest::Queryable, Harness};
    let label = "Следующее / предыдущее изображение";
    let mut h = Harness::builder()
        .with_size(egui::vec2(700.0, 300.0))
        .build_state(
            |ctx, value| {
                let t = Tokens::new(Appearance::Dark, ctx);
                t.apply(ctx);
                egui::CentralPanel::default().show(ctx, |ui| radio(ui, value, false, label));
            },
            true,
        );
    h.run_steps(3);
    let b = h.get_by_label(label).raw_bounds().unwrap();
    h.input_mut()
        .events
        .push(egui::Event::PointerMoved(egui::pos2(
            b.x0 as f32 + 6.0,
            (b.y0 + b.y1) as f32 / 2.0,
        )));
    h.run_steps(2);
    let glow = h
        .output()
        .shapes
        .iter()
        .find(|shape| matches!(&shape.shape,egui::Shape::Mesh(mesh) if mesh.vertices.len()==42))
        .unwrap();
    let circle = h
        .output()
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Circle(circle) if circle.radius > 5.0 => Some(circle),
            _ => None,
        })
        .unwrap();
    assert!(
        glow.clip_rect.left() < circle.center.x - circle.radius,
        "radio edge cannot be cut by Reveal clip"
    );
    assert!(
        glow.clip_rect.right() > b.x1 as f32,
        "complete label needs right padding"
    );
}

fn heading(ctx: &egui::Context, lang: Language, t: &Tokens) {
    egui::TopBottomPanel::top("preferences_heading")
        .exact_height(crate::platform::backdrop::PREFERENCES_HEADING_HEIGHT)
        .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(24, 0)))
        .show(ctx, |ui| {
            let rect = ui.max_rect().expand2(egui::vec2(24.0, 0.0));
            let boundary = rect.top() + crate::platform::backdrop::PREFERENCES_TRANSLUCENT_HEIGHT;
            // The 64px heading keeps its layout, but only its first 48px
            // belong to acrylic. The final 16px are opaque, like the page.
            ui.painter().rect_filled(
                egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), boundary)),
                0.0,
                t.header,
            );
            ui.painter().rect_filled(
                egui::Rect::from_min_max(egui::pos2(rect.left(), boundary), rect.max),
                0.0,
                t.background,
            );
            ui.label(RichText::new(lang.text("Настройки", "Settings")).size(32.0));
        });
}

#[cfg(test)]
#[test]
fn settings_heading_is_opaque_below_the_library_acrylic_boundary() {
    for appearance in [Appearance::Light, Appearance::Dark] {
        for focused in [true, false] {
            let ctx = egui::Context::default();
            ctx.data_mut(|d| d.insert_temp(egui::Id::new("rphotos.backdrop"), true));
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 800.0),
                )),
                ..Default::default()
            };
            input
                .viewports
                .get_mut(&egui::ViewportId::ROOT)
                .unwrap()
                .focused = Some(focused);
            let output = ctx.run(input, |ctx| {
                let t = Tokens::new(appearance, ctx);
                egui::TopBottomPanel::top("title")
                    .exact_height(32.0)
                    .frame(egui::Frame::NONE.fill(t.header))
                    .show(ctx, |_| {});
                heading(ctx, Language::Ru, &t);
            });
            let fills: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Rect(rect) if rect.fill.a() > 0 => Some(rect),
                    _ => None,
                })
                .collect();
            for point in [
                egui::pos2(1.0, 88.0),
                egui::pos2(600.0, 88.0),
                egui::pos2(1199.0, 88.0),
            ] {
                let fill = fills
                    .iter()
                    .rev()
                    .find(|rect| rect.rect.contains(point))
                    .expect("bottom strip must be painted, including side margins");
                assert_eq!(fill.fill.a(), 255, "no transparency below y=80");
                assert_eq!(fill.rect.top(), 80.0);
                assert_eq!(fill.rect.bottom(), 96.0);
            }
            let header = fills
                .iter()
                .rev()
                .find(|rect| rect.rect.contains(egui::pos2(600.0, 70.0)))
                .unwrap();
            assert_eq!(header.rect.bottom(), 80.0);
            assert_eq!(header.fill.a(), if focused { 190 } else { 255 });
            assert!(fills
                .iter()
                .all(|rect| rect.fill.a() == 255 || rect.rect.bottom() <= 80.0));
        }
    }
}

impl Library {
    pub(super) fn preferences(
        &mut self,
        ctx: &egui::Context,
        settings: &mut AppSettings,
        t: &Tokens,
    ) {
        crate::platform::backdrop::painted_preferences(ctx);
        let lang = settings.language;
        let blocked =
            self.remove_source.is_some() || self.error.is_some() || self.cache_errors_open;
        let status = self.thumbnails.bulk_status();
        if status.total > 0 {
            egui::TopBottomPanel::bottom("preferences_cache_status")
                .exact_height(48.0)
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    if blocked {
                        ui.disable();
                    }
                    let action = super::cache_status::show(ui, status, lang, t, 24.0);
                    self.cache_task_action(action);
                });
        }
        heading(ctx, lang, t);
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(t.background))
            .show(ctx, |ui| {
                if blocked { ui.disable(); }
                egui::ScrollArea::vertical().id_salt("preferences").show(ui, |ui| {
                    egui::Frame::NONE.inner_margin(24).show(ui, |ui| {
                        ui.set_max_width(532.0);
                        ui.spacing_mut().item_spacing.y = 12.0;
                         super::icons::heading(ui,lang.text("Источники", "Sources"),super::icons::Icon::Folder);
                        ui.label(lang.text("Выберите папки, фотографии из которых будут показаны в коллекции.", "Choose folders whose photos appear in Collection."));
                        for source in self.state.sources.clone() {
                            ui.label(source.path.to_string_lossy());
                            if ui.button(lang.text("Убрать источник", "Remove source")).clicked() {
                                self.remove_source = Some(source.id);
                            }
                        }
                        if ui.button(lang.text("Добавить папку", "Add folder")).clicked() { self.pick_source(ctx); }
                        ui.add_space(20.0);
                         super::icons::heading(ui,"Язык / Language",super::icons::Icon::Language);
                         radio(ui,&mut settings.language, Language::Ru, "Русский");
                         radio(ui,&mut settings.language, Language::En, "English");
                        ui.add_space(20.0);
                         super::icons::heading(ui,lang.text("Оформление", "Appearance"),super::icons::Icon::Theme);
                         radio(ui,&mut settings.appearance, Appearance::Light, lang.text("Светлое", "Light"));
                         radio(ui,&mut settings.appearance, Appearance::Dark, lang.text("Тёмное", "Dark"));
                         radio(ui,&mut settings.appearance, Appearance::System, lang.text("Как в системе", "Use system setting"));
                        ui.add_space(20.0);
                         super::icons::heading(ui,lang.text("Колесо мыши", "Mouse wheel"),super::icons::Icon::Mouse);
                         radio(ui,&mut settings.mouse_wheel_zoom, false, lang.text("Следующее / предыдущее изображение", "View next or previous item"));
                         radio(ui,&mut settings.mouse_wheel_zoom, true, lang.text("Увеличение и уменьшение", "Zoom in and out"));
                         ui.label(lang.text("Ctrl + колесо всегда меняет масштаб.", "Ctrl + wheel always zooms."));
                         ui.add_space(20.0);
                         super::icons::heading(ui,lang.text("Предзагрузка миниатюр", "Thumbnail preloading"),super::icons::Icon::Analysis);
                         ui.horizontal(|ui|{ui.label(lang.text("Потоки (0 — автоматически)","Workers (0 — automatic)"));ui.add(egui::DragValue::new(&mut settings.thumbnail_workers).range(0..=24));});
                         ui.add_enabled_ui(!self.thumbnails.bulk_status().running,|ui|{ui.horizontal(|ui|{ui.label(lang.text("Дисковый кеш, ГиБ (0 — без ограничения)","Disk cache, GiB (0 — unlimited)"));ui.add(egui::DragValue::new(&mut settings.thumbnail_disk_gib).range(0..=4096));});});
                          ui.label(format!("{}: {}",lang.text("Каталог кеша","Cache directory"),self.thumbnails.directory().display()));
                          ui.label(lang.text("Полная прогрузка: главное меню «… → Прогрузить все миниатюры». Кеш сохраняется после перезапуска; команда отключает его дисковый лимит. Для полного размера по-прежнему нужен оригинал.","Full caching: main menu … → Cache all thumbnails. Cache survives restart; the command disables its disk limit. Full-size viewing still needs the original."));
                          ui.label(lang.text("Миниатюры хранятся сжатыми PNG между запусками. Недоступный оригинал скрывает превью, но не удаляет его кеш. Неиспользованные 90 дней превью удаляются; при превышении квоты — самые старые, кроме использованных за последние 7 дней.","Compressed PNG previews persist between sessions. Unavailable originals hide previews without deleting their cache. Previews unused for 90 days expire; over quota, the oldest are removed except those used in the past 7 days."));
                         ui.label(lang.text("Видимый экран — первым, до восьми экранов вперёд и трёх назад. Расширенный кеш сохраняет недавно просмотренные фото. Авто использует число CPU-потоков минус один (до 16); вручную — до 24. Крупные оригиналы декодируются последовательно для ограничения памяти. На HDD меньше потоков может быть быстрее.","Visible screen first, up to eight screens ahead and three behind. The expanded cache retains recently viewed photos. Auto uses CPU threads minus one (up to 16); manual up to 24. Large originals decode sequentially to limit memory. Fewer workers can be faster on HDD."));
                        self.analysis_preferences(ui, settings, lang);
                        ui.add_space(20.0);
                         super::icons::heading(ui,lang.text("В разработке", "In development"),super::icons::Icon::Info);
                         ui.label(lang.text("Linux: печать через системный portal; экспорт PDF и черновик письма доступны через «Поделиться». Размытие шапки — на Wayland с поддержкой compositor, иначе непрозрачный фон. Полноценный системный Share ещё в разработке.", "Linux printing uses the system portal; PDF export and email drafts are available in Share. Header blur uses a supported Wayland compositor, otherwise an opaque fallback. Full system Share remains in development."));
                        ui.label("Rphotos · MIT OR Apache-2.0 · based on ViewSkater");
                    });
                });
            });
    }

    fn analysis_preferences(
        &mut self,
        ui: &mut egui::Ui,
        settings: &mut AppSettings,
        lang: Language,
    ) {
        ui.add_space(20.0);
        super::icons::heading(
            ui,
            lang.text("Локальные данные", "Local data"),
            super::icons::Icon::Analysis,
        );
        if ui
            .add_enabled(
                !self.scanning,
                egui::Button::new(lang.text(
                    "Распознать текст локально (OCR)",
                    "Recognize text locally (OCR)",
                )),
            )
            .clicked()
        {
            let ticket = self.worker.ticket();
            self.scanning = true;
            self.send(Command::Ocr(ticket));
        }
        ui.label(lang.text("Нужен установленный Tesseract и языки rus/eng. Один поток CPU, таймаут 45 с/фото, без облака. Результаты сохраняются локально и участвуют в поиске.", "Requires installed Tesseract and rus/eng data. One CPU thread, 45 s/photo timeout, no cloud. Results stay local and become searchable."));
        ui.label(lang.text(
            "Описание содержимого (необязательный локальный Ollama)",
            "Content descriptions (optional local Ollama)",
        ));
        ui.add(
            egui::TextEdit::singleline(&mut settings.local_vision_model).hint_text("qwen2.5vl:3b"),
        );
        if ui
            .add_enabled(
                !self.scanning && !settings.local_vision_model.trim().is_empty(),
                egui::Button::new(lang.text(
                    "Описать фото локальной моделью",
                    "Describe photos with local model",
                )),
            )
            .clicked()
        {
            let ticket = self.worker.ticket();
            self.scanning = true;
            self.send(Command::Describe(
                ticket,
                settings.local_vision_model.clone(),
            ));
        }
        ui.label(lang.text("Только после нажатия: уменьшенная копия каждого фото отправляется на 127.0.0.1:11434. Нужны curl, запущенный Ollama и уже установленная vision-модель. Ничего не скачивается автоматически. Описания RU/EN попадают в текстовый поиск; это не векторный semantic search. Ответ модели может ошибаться. Отмена прерывает клиент; сервер может закончить текущий запрос.", "Only after clicking: a resized copy of each photo is sent to 127.0.0.1:11434. Requires curl, a running Ollama and an already-installed vision model. Nothing is downloaded automatically. RU/EN descriptions become text-searchable; this is not vector semantic search. Model descriptions can be wrong. Cancellation stops the client; the server may finish the current request."));
        ui.add_space(12.0);
        ui.label(lang.text(
            "Модель смыслового поиска (local Ollama)",
            "Semantic search model (local Ollama)",
        ));
        ui.add(egui::TextEdit::singleline(&mut settings.local_embedding_model).hint_text("bge-m3"));
        let enabled = !self.scanning && !settings.local_embedding_model.trim().is_empty();
        if ui
            .add_enabled(
                enabled,
                egui::Button::new(lang.text("Построить смысловой индекс", "Build semantic index")),
            )
            .clicked()
        {
            let ticket = self.worker.ticket();
            self.scanning = true;
            self.send(Command::Embed(
                ticket,
                settings.local_embedding_model.clone(),
            ));
        }
        if ui
            .add_enabled(
                !self.scanning,
                egui::Button::new(lang.text("Очистить смысловой индекс", "Clear semantic index")),
            )
            .clicked()
        {
            self.send(Command::ClearEmbeddings);
        }
        if ui
            .add_enabled(
                enabled,
                egui::Button::new(lang.text(
                    "Собрать альбомы похожего содержимого",
                    "Build similar-content albums",
                )),
            )
            .clicked()
        {
            let ticket = self.worker.ticket();
            self.scanning = true;
            self.send(Command::ContentAlbums(
                ticket,
                settings.local_embedding_model.clone(),
            ));
        }
        ui.label(lang.text("Сначала OCR или описания, затем индекс. Только по нажатию: текст передаётся на 127.0.0.1:11434 уже установленной embedding-модели. Поиск: введите запрос → … → «Смысловой поиск». До 100 результатов с cosine ≥ 0.3, показаны по датам. Автоальбомы: группы ≥ 3 фото с похожестью к представителю ≥ 0.75, максимум 64 группы / 10000 фото. Это сходство текстовых описаний, не прямой image embedding; качество и русский язык зависят от модели. Ничего не скачивается автоматически.", "Run OCR or descriptions, then build the index. Only on clicking: text is sent to an already-installed embedding model at 127.0.0.1:11434. Search: type a query → … → Semantic search. Up to 100 matches with cosine ≥ 0.3, displayed by date. Automatic albums: groups of ≥ 3 photos with representative similarity ≥ 0.75, at most 64 groups / 10000 photos. This compares text descriptions, not direct image embeddings; quality and Russian support depend on the model. No automatic downloads."));
        if self.scanning {
            if ui
                .button(lang.text("Остановить анализ / сканирование", "Stop analysis / scan"))
                .clicked()
            {
                self.worker.cancel();
                self.scanning = false;
            }
            if self.scanning
                && ui
                    .button(if self.worker.paused() {
                        lang.text("Продолжить задачу", "Resume task")
                    } else {
                        lang.text("Приостановить задачу", "Pause task")
                    })
                    .clicked()
            {
                self.worker.toggle_pause();
            }
        }
        if ui
            .add_enabled(
                !self.scanning,
                egui::Button::new(lang.text(
                    "Собрать автоальбомы по датам",
                    "Build date-based automatic albums",
                )),
            )
            .clicked()
        {
            self.send(Command::AutoAlbums);
        }
        ui.label(lang.text("События: минимум 3 фото, разрыв до 2 дней. Дата съёмки EXIF, иначе дата файла; переименованный автоальбом становится ручным.", "Events: at least 3 photos, gaps up to 2 days. EXIF capture date, otherwise file date; renaming an automatic album makes it manual."));
        for search in self.state.searches.clone() {
            ui.horizontal(|ui| {
                ui.label(search.name);
                if ui
                    .button(lang.text("Удалить поиск", "Remove search"))
                    .clicked()
                {
                    self.send(Command::RemoveQuery(search.id));
                }
            });
        }
        ui.label(lang.text("Индекс и миниатюры сохраняются локально. Оригиналы не изменяются при индексировании или работе с альбомами.", "The index and thumbnails stay local. Indexing and album operations do not modify originals."));
        if ui
            .button(lang.text("Обновить индекс", "Refresh index"))
            .clicked()
        {
            self.refresh();
        }
    }
}
