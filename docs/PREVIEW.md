# Rphotos 0.1.0 development preview

Это предварительная версия, **не завершённая замена Photos 2019** и не stable release.
Документ описывает текущий `main`. Опубликованный Preview1 (`d089114`) исторический:
он не содержит последующих исправлений аудита, transforms и нового кеша.

## Основные возможности

- Коллекция с aspect-aware строками, тремя плотностями, месяцами и timeline.
- Ручные альбомы, обложки, избранное, источники и дерево пустых/вложенных папок.
- SQLite-индекс, native watcher, фоновые миниатюры, ограниченный кеш, пауза/отмена.
- Одиночный просмотр, zoom/pan, native100% viewport tiles, сведения, slideshow, фоновая корзина.
- JPEG/PNG rotate/flip, resize-copy и отдельный Save overwrite с подтверждением,
  private recovery для Undo после перезапуска при неизменённом файле.
- RU/EN, темы, Reveal; Wayland blur и новые Windows/macOS blur/vibrancy adapters.
- PDF, native Print/OpenWith adapters, file clipboard, сохранение копии и native Share Windows/macOS.
- Opt-in OCR, локальные описания, смысловой поиск и альбомы по датам/содержимому.

## Как запустить portable archive

Распакуйте **весь архив** в отдельный каталог. Linux: запустите `./rphotos`,
Windows: `rphotos.exe`, macOS: `./rphotos` из Terminal. Эти архивы не являются
подписанными installers, AppImage или macOS `.app`; OS может показывать предупреждения.
На Linux нужны рабочие GPU driver/Wayland либо X11 и системные shared libraries.
Готовая сборка не включает модели, Tesseract или photo library.

Без аргументов открывается библиотека. Выберите «Добавить папку». Источники
индексируются локально; удаление источника/альбома не удаляет фотографии.
Удаление фото выполняется отдельной командой с подтверждением.

Настройки и индекс находятся в стандартных пользовательских каталогах `rphotos`,
не в каталоге portable archive. Это не полностью автономный portable profile.
`BUILD-INFO.json` содержит platform/profile, SHA256 бинарника и packaging source commit;
SHA256 всего архива находится в соседнем `.sha256`.

## Важно перед проверкой

- Начните с копии небольшой тестовой папки. Это development build с развивающейся схемой индекса.
- Необходимые OCR/vision/embedding engines устанавливаются отдельно, только по вашему решению.
  Rphotos ничего не скачивает и не запускает анализ автоматически.
- Для моделей разрешён только local Ollama с capability metadata; стандартные cloud-backed
  aliases отклоняются до передачи фото/текста. Не используйте недоверенный локальный proxy.
- Качество RU/EN поиска и группировки настоящих моделей ещё не принято.
- Native touch, реальные printer jobs и полный clipboard workflow не приняты.
- Новые Windows/macOS native adapters прошли CI и требуют реальной проверки диалогов/дисплея;
  успешная компиляция не подтверждает визуальное поведение или отправку print job.
- Linux Share остаётся собственным меню: универсального OS Share portal нет.
- Нет crop/цветокоррекции, видео, OneDrive, People/face recognition и camera/USB import.
- Fit texture max8192, native100% поверх неё — tiles2048 с полным декодированием в RAM,
  не disk-backed engine. RGB ICC приводится к sRGB, monitor ICC/CMYK/16-bit pipeline нет.
- RAW предпочитает embedded JPEG; при его отсутствии используется установленный LibRaw,
  если доступен. Качество всех камер и режим полного сенсора вместо preview не приняты.
- Recovery хранит последний шаг на путь без автоквоты, не всю историю; замену нельзя
  считать atomic CAS против внешнего редактора. Сначала используйте тестовые копии.

Фактическая проверка: [VERIFICATION.md](VERIFICATION.md).
Оставшиеся задачи: [IMPLEMENTATION.md](IMPLEMENTATION.md).
История и авторство ViewSkater сохранены, лицензии MIT OR Apache-2.0.
