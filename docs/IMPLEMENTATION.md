# Rphotos: рабочая карта реализации

Этот файл — точка входа после потери/сжатия контекста. Не заменять измеренный
референс общим представлением о Windows Photos.

## Источники истины

1. [PRODUCT.md](PRODUCT.md): согласованный объём и исключения.
2. [PHOTOS-2019-AUDIT.md](PHOTOS-2019-AUDIT.md): подробные размеры, состояния,
   переходы и пределы проверки на планшете. Раздел 9 — визуальный backlog.
3. [PHOTOS-2019.md](PHOTOS-2019.md): короткое описание референса/backdrop.
4. [VERIFICATION.md](VERIFICATION.md): только действительно выполненные проверки.

Приоритет: Linux/KDE Wayland, затем Windows/macOS. RU/EN, touch+мышь+клавиатура.
Без видео, People, OneDrive, preview, USB/camera import; редактор отложен.
Локальные OCR/content descriptions и date auto albums, Linux print/PDF/share
подключены в базовом виде; ограничения и непроверенные интеграции — ниже.
Приватные скриншоты/деревья с планшета не добавлять в Git.

## Где что находится

- `src/library/{store,worker,thumbnails}.rs`: SQLite, фоновый scan, thumbnail cache.
- `src/library/facts.rs`: bounded header-only EXIF date probe, не декодирует pixels.
- `src/library/{mod,ui}.rs`: состояние и UI коллекции/альбомов/папок.
- `src/library/{layout,gallery}.rs`: независимая раскладка строк, виртуализированная
  галерея, выбор месяца и timeline.
- `src/library/navigation.rs`: keyboard focus, arrows/Home/End/Enter/Space,
  прокрутка к виртуализированным снимкам без перехвата search/toolbar input.
- `src/library/watcher.rs`: notify native recursive watch,800ms debounce,
  bounded event queue/atomic dirty, пересканирование после текущей задачи.
- `src/library/catalog.rs`: album/folder tiles, hero, picker, rename/cover/remove,
  контекстные действия и уведомления; `preferences.rs`: полностраничные настройки.
- `src/library/analysis.rs`: Tesseract и opt-in loopback Ollama; `semantic.rs`:
  persisted embeddings, cosine search/content albums; `gestures.rs`: pager.
- `src/platform/{printing,jobs,clipboard,backdrop}.rs`: PDF/Print portal, share jobs,
  file clipboard и Wayland blur; `src/library/discovery.rs`: album cards/search suggestions.
- `src/platform/files.rs`: Save copy с atomic no-clobber и Linux OpenFile portal
  ask(true). Не подменяет Share и не меняет оригинал.
- `src/viewer.rs`: одиночный viewer. `src/pane.rs`, `src/cache.rs`: decode/render.
- `src/app.rs`: маршрутизация сцен; comparison/bench пока используют старую UI.
- `src/settings.rs`: persistent settings с обратносовместимыми defaults.
- `src/chrome.rs`: общая клиентская строка заголовка и window controls.

## Этапы визуальной переделки

| Этап | Объём | Статус |
| --- | --- | --- |
| R1 | Общий title32/Back/window controls; viewer48, zoom-popup, info344, viewport/edges | Базовая реализация и regression/capture готовы; нативная интерактивная приёмка остаётся |
| R2 | Library commands48/tabs48/search/density; selection заменяет commands | Реализован базовый shell; regression/captures, без полноценных bulk integrations |
| R3 | Aspect-aware grid без filename footer; месяцы, группы, semantic zoom/timeline | Базовая реализация; пропорции уточняются фоновыми миниатюрами, точная Photos-раскладка не заявлена |
| R4 | Tiles Albums/Folders, album detail/rename/cover/picker, вложенные каталоги | Базовые сцены и DB lifecycle реализованы; picker ещё проще референса |
| R5 | Full-page Settings, notification/modal transitions | Реализованы; viewer возвращается из общих Settings |
| R6 | Platform acrylic, реальные touch, print/share, content search/auto albums | Частично: Wayland blur, synthetic pager, Print portal/PDF, local OCR/descriptions и date events; native/device/ML quality приёмка остаётся |

Для каждого этапа: сначала геометрические и interaction regression tests,
затем offscreen captures одинаковых synthetic файлов/viewport/DPI и нативный
smoke. Сборка не доказывает визуальное соответствие. Не объявлять feature
завершённой по наличию нерабочей кнопки. Неподключённые действия обозначать явно.

## Историческая исходная точка перед R1

Нативная OS title + Back в command bar; zoom расширяет отдельную строку;
info320 с произвольным набором полей; равные gallery cells/filename footers;
нет timeline, album hero и полноэкранных Settings. Acrylic отсутствует.
Текущий UI пользователь признал слишком далёким от референса.

## Что изменено в R1

Клиентский заголовок 32, Back48×32, controls46×32, drag/double-click и resize
через viewport commands. Общая строка подключена к библиотеке/viewer/comparison;
OS decorations отключены, minimum client500×360. В R1 acrylic ещё отсутствовал;
текущая Wayland-реализация описана ниже.
Viewer commands48: Add to слева, пять центральных слотов49.5, edit/share/print/…
справа; неподключённые действия disabled. Убраны счётчик и центральная Info.
Zoom — локальный flyout, не меняет viewport; control0..100, приблизительная
логарифмическая кривая fit..20× (кривая Photos неизвестна, совпадение не заявлено).
Photo inset32 по бокам/снизу, без дополнительного top32. Info344 фиксированная,
read-only, лейблы/пустые значения/вертикальный ритм. Тонкие edge chevrons без
плашек; fullscreen/slideshow скрывают title/commands/info.

Regression tests проверяют измеренные прямоугольники, отсутствие reflow при zoom,
ширину Info, fullscreen и отправку window commands. Настоящие compositor drag,
resize/snap и touch с клиентским chrome ещё требуют ручной проверки на каждой OS.

## Что изменено в R2

`src/library/shell.rs` отделяет measured shell от галереи/диалогов. Title32,
commands48, tabs48; большой viewport1500 даёт search(120,38)396×36,
content margin120, density справа тремя векторными значками25×44. При узком
окне commands не переносятся: New album/Add folder уходят в overflow.
Selection заменяет commands charcoal-панелью, скрывает search/соседние tabs,
сохраняет высоту/фильтры; count, Add to, Cancel, select/unselect all в overflow.
Esc сначала закрывает overflow, следующий Esc отменяет selection: хранится
popup-state прошлого кадра, поскольку egui может закрыть меню до кода сцены.
Групповой выбор доступен у даты; checkbox/checkmark рисуются векторами.
В R2 Print/share/copy/bulk-delete были явно disabled; сейчас подключены backend-ы
R6, с ограничениями ниже.
Bottom status показывается только при scan, постоянного footer нет.

R2 **не** включает переделку album modal, OCR dropdown, discovery cards,
album/folder tiles, сетки и timeline. Они разрабатываются отдельно.

## Что изменено в R3

Чистая раскладка `layout::rows`: целевые высоты512/256/96, gap12/12/6,
подбор переноса с общей высотой строки, неполная последняя строка без растяжения.
Пропорции — из ориентированных thumbnail pixels; пока неизвестны, используется
1.5. При декодировании revision инвалидирует layout; anchor(id,relative_y)
удерживает видимое фото при reflow. Пропорции хранятся в RAM и сохраняются worker-ом
в SQLite по cache key; после рестарта известные значения доступны до decode. Редкие экстремальные
пропорции ограничены0.2..5 и center-crop, обычные фото не искажаются.
Никаких filesystem/decode на UI thread. Layout на весь индекс O(N) при изменении,
рендер только видимых строк (binary search по offsets).

Month headers по EXIF capture date с fallback на local modified month,
range/count или group-select под ними.
Filename только tooltip/accessibility. Semantic zoom: клик месяца → список48,
titleBack/Esc или выбор месяца возвращает фото; команды/вкладки disabled в chooser.
Timeline в широком поле справа: year/dots/active, click-jump и scrub ко всем месяцам;
при очень длинной истории точки семплируются. В узком окне rail скрыт, chooser
остаётся доступен. Это базовая версия, не воспроизведение неизвестного алгоритма
Photos и группировки нескольких дней внутри месяца.

## R4–R6: реализованное и оставшееся

Альбомы: tiles303×198, заголовок/hero, rename Done/Cancel, выбор обложки из
выделенного снимка, add/remove membership, remove album без filesystem delete.
Полностраничный picker использует Collection/Folders, поиск по имени/тексту и
общую aspect-aware раскладку трёх плотностей; точная геометрия референса ещё
не заявлена. Папки выводят вложенные каталоги перед файлами
именно текущего уровня; включая пустые каталоги из фонового индекса. Один helper
child_folders используется библиотекой и picker-ом, без filesystem на UI thread.
Пересекающиеся sources deduplicate дерево; offline/incomplete/cancel сохраняют
последний успешный обход. Удаление source удаляет только его DB associations.
SQLite migrations v1→v10 сохраняют ручные данные; auto events/date based,
saved queries, OCR и descriptions с cache-key invalidation. Изменённый пользователем
автоальбом превращается в ручной. Пересборка нетронутых auto albums меняет их ID.

EXIF DateTimeOriginal → Digitized → DateTime, offset при наличии, иначе local time.
Scanner читает не более4MiB на новый/изменённый cache key, в том числе сохраняет
отрицательный результат; metadata за этим пределом/неподдержанный контейнер дают
fallback на modified. Dates cached отдельно, cache key invalidation; порядок, месяцы
и event albums используют одну эффективную дату. Viewer Info хранит upright source
dimensions до8192 GPU cap; у RAW это размеры embedded preview, не sensor raster.

Settings: Sources/RU-EN/Theme/Wheel/local jobs. Scene сохраняется в eframe storage,
loading guard не даёт пустому начальному snapshot сбросить scroll до загрузки.
Thumbnail disk quota512MiB удаляет только 64hex.png в своём каталоге, CPU worker;
guard запрета удаления оригиналов имеет ровно одно reviewed cache exception.

Print: ashpd PreparePrint → oriented PDF → Print. Нативная отправка не проверена.
PDF white alpha compositing, atomic replacement, destination-original guard,
100-photo/128MiB output limits. Share — свой modal, не системный.
File clipboard: Linux wl-copy или xclip/text-uri-list, Windows STA FileDropList,
macOS osascript aliases (две последние платформы не проверены). Отдельно остаётся
copy path. Clipboard пользователя не изменялся при автоматических тестах.
Linux email открывает черновик, не отправляет.
OCR требует установленного Tesseract; descriptions — curl+local Ollama vision
на фиксированном loopback, явный opt-in, без download. Real OCR/model quality
не проверены. Keyword search остаётся по умолчанию. Opt-in embedding-модель
на том же fixed loopback индексирует OCR/descriptions; векторы проверяются по
photo cache key, text hash и имени модели, нормализуются и хранятся в SQLite.
Запрос из `… → Смысловой поиск`: cosine≥0.3/top100, display date order;
изменение query отключает semantic hits. Clear index нужен при замене модели
с тем же именем. Memory budget256MiB. Content albums: ≥3, representative cosine≥0.75,
до64 групп/10000 фото, отдельный namespace auto_key content: vs event:.
Пересборка не трогает manualized albums; отмена до commit откатывает замену.
Это embeddings текста, не прямые image vectors; real model quality не проверена.

Backdrop: guest Wayland connection из raw handles, EXT protocol с capability
tracking, fallback KDE, область только80px library/32px viewer, inactive opaque.
Windows/macOS/X11 пока opaque. EXT подключение подтверждено native smoke на KDE;
проверка цветного backdrop/focus/resize с compositor ещё остаётся.
Pager axis lock/vertical escape/cancel проверен synthetic events; настоящий
touch, inertia и показ двух соседних страниц одновременно ещё не приняты.

Collection cards открывают настоящие альбомы, Hide/Show сохраняется в scene.
Карточки прокручиваются вместе с сеткой, не занимают фиксированную область.
Search dropdown — saved queries и bounded vocabulary из OCR/descriptions, без
вымышленных AI-тегов; Enter/Esc/outside click закрывают его. Избранное хранится
отдельно, доступно из viewer/context menu и через `is:favorite`.

**Оставшийся backlog:** точный встроенный album picker и modal geometry, system Share,
Windows/macOS adapters/builds, реальные touch/print/OCR/model, EXIF container coverage,
приёмка watcher на больших/сетевых библиотеках, content albums/vector search
на реальной модели, release packaging.

## Проверки и инфраструктура

`cargo test --locked --all-targets`, `cargo check --locked --all-features`,
`cargo build --locked`; capture: `RPHOTOS_CAPTURE_DIR=/tmp/opencode/... cargo test
--locked capture_ -- --ignored --nocapture`. Только новые файлы rustfmt;
upstream имеет несвязанные отклонения глобального fmt. Ничего не сбрасывать:
проект содержит незакоммиченные изменения предыдущих этапов.

Референсный helper на Windows остановлен и удалён; он **не активен**.
SSH — через amude-lan jump, не напрямую. При новом исследовании использовать
временный interactive helper с последующей очисткой, не открывать сетевые порты.
GitHub auth — по глобальному AGENTS.md, никогда не читать/выводить token.

## Packaging groundwork

Оригинальный artwork assets/rphotos.svg, PNG/ICO/ICNS генерирует
scripts/generate-icons.py (Pillow). Upstream assets сохранены, но application/bundle
уже используют Rphotos icon. scripts/package.py пакует готовый native binary,
README/license/verification/Cargo.lock и dependency notices в tar.gz/zip + SHA256.
Strict allowlist: никаких settings/index/captures/credentials. Dependency inventory
консервативно включает все targets/dev и тексты из crates, включая font OFL/UFL;
76 crates не содержат отдельного license file — отмечены с source/repository URL.
Это не полный юридический аудит или стабильный release. Нативные зависимости OS
и source-offer obligations перед публичным релизом требуют отдельной проверки.
CI делает opt-dev builds + upload artifact для3OS. Репозиторий опубликован:
https://github.com/DurkaEbanaya/Rphotos . Первый remote run скомпилировал macOS,
но обнаружил различие `/var`→`/private/var` в test expectations; fixtures приведены
к canonical paths, как и настоящий индекс. Результаты следующего run фиксировать
в VERIFICATION, не выдавать compile за нативную приёмку.
`python3 scripts/test_package.py` проверяет3OS archive branches на синтетических
входах: strict allowlist, SHA256, font/MPL notices и отсутствие архива при missing input.
