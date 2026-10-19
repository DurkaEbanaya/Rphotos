//! Full-page library preferences. Unsupported capabilities are explicit.
use super::{ui::Tokens, Appearance, Command, Language, Library};
use crate::settings::AppSettings;
use eframe::egui::{self, RichText};

impl Library {
    pub(super) fn preferences(
        &mut self,
        ctx: &egui::Context,
        settings: &mut AppSettings,
        t: &Tokens,
    ) {
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
        egui::TopBottomPanel::top("preferences_heading")
            .exact_height(64.0)
            .frame(
                egui::Frame::NONE
                    .fill(t.header)
                    .inner_margin(egui::Margin::symmetric(24, 0)),
            )
            .show(ctx, |ui| {
                ui.label(RichText::new(lang.text("Настройки", "Settings")).size(32.0));
            });
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
                        ui.radio_value(&mut settings.language, Language::Ru, "Русский");
                        ui.radio_value(&mut settings.language, Language::En, "English");
                        ui.add_space(20.0);
                         super::icons::heading(ui,lang.text("Оформление", "Appearance"),super::icons::Icon::Theme);
                        ui.radio_value(&mut settings.appearance, Appearance::Light, lang.text("Светлое", "Light"));
                        ui.radio_value(&mut settings.appearance, Appearance::Dark, lang.text("Тёмное", "Dark"));
                        ui.radio_value(&mut settings.appearance, Appearance::System, lang.text("Как в системе", "Use system setting"));
                        ui.add_space(20.0);
                         super::icons::heading(ui,lang.text("Колесо мыши", "Mouse wheel"),super::icons::Icon::Mouse);
                        ui.radio_value(&mut settings.mouse_wheel_zoom, false, lang.text("Следующее / предыдущее изображение", "View next or previous item"));
                        ui.radio_value(&mut settings.mouse_wheel_zoom, true, lang.text("Увеличение и уменьшение", "Zoom in and out"));
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
