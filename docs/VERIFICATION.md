# Проверка Rphotos — 2026-10-19

Проверено локально на Linux / KDE Wayland, Rust 1.98.1.
Это запись фактически выполненных проверок, не матрица обещанной поддержки.

## Результаты

| Проверка | Результат |
| --- | --- |
| `cargo build --locked` | Успешно |
| `cargo test --locked --all-targets` | 166 passed, 0 failed, 7 ignored |
| `cargo check --locked --all-features` | Успешно |
| `cargo clippy --locked --all-targets --all-features --message-format short` | Успешно; 8 предупреждений в неизменённых upstream `decode`/`raw` |
| `git diff --check` | Успешно |
| `python3 scripts/test_package.py` | 2 passed; Linux/macOS/Windows archive fixtures, allowlist, notices, SHA256 |
| GPU offscreen capture-тесты | Успешно; библиотека и пять видов просмотрщика, включая zoom-popup |
| Нативный запуск без аргументов | Окно жило 6 секунд без аварии; затем процесс остановлен тестом |
| Повторный нативный smoke после смены просмотрщика | Библиотека и просмотр synthetic PNG по 4 секунды без аварии |
| Smoke R1 с клиентским chrome | Библиотека и synthetic PNG по 5 секунд без аварии, изолированные XDG |
| Smoke R2 shell | Библиотека 5 секунд без аварии, изолированные XDG; не полный native workflow |
| Smoke R3 gallery | 30 synthetic фото в изолированном индексе/кеше, 5 секунд без аварии |
| Smoke R4–R6 | Библиотека и synthetic PNG по5с, изолированные XDG; EXT blur protocol подключён |
| Оптимизированный portable build | `cargo build --locked --profile opt-dev`; обе сцены по5с без аварии |

Нативный smoke-запуск использовал отдельные XDG data/config/cache в тестовом
временном каталоге, без индексации личных папок. Он не подтверждает весь
интерактивный сценарий на настоящем дисплее. В журнале есть предупреждение об
отсутствии CJK-шрифта; кириллица на GPU-снимках отображается корректно.

## Что покрывают новые тесты

Последний срез R4–R6: SQLite migrations v1→v10; create/assign/rename/cover/unassign/
remove album с rollback; папки только текущего уровня; auto date events и saved
queries; OCR/cache-key invalidation; сохранение thumbnail aspect и избранного;
round-trip scene/query/scroll/Hide cards без transient selection/dialogs.
Kittest: full-page picker/settings и album lifecycle, настоящие album cards,
Hide/Show, dropdown сохранённого поиска, pager axis-lock/vertical escape и
блокировка открытия фото во время свайпа. Локальные OCR/vision adapters проверены
fake Tesseract и loopback HTTP mock; реальные движки не установлены.

Дерево папок: пустые/nested каталоги, dedup overlapping sources, restart,
offline/cancel preservation и удаление только source associations. Kittest открывает
пустую папку и её ребёнка в библиотеке и embedded album picker; чужие фото не
попадают в текущий уровень. EXIF date: настоящий сгенерированный JPEG с DateTimeOriginal,
offset/invalid date guards, date sorting, restart/cache-key invalidation, без изменения
оригинала. Viewer dimensions проверены на9000px PNG: record сохраняет9000, GPU8192;
orientation90° корректно меняет стороны и у JPEG/PNG с trailing EXIF.

Semantic: loopback `/api/embed` mock проверяет, что передаётся текст, не images;
нормализация, NaN/Inf/zero/dimension guards, cosine ranking/top100, persisted
vectors/model+text-hash+cache-key invalidation. Content grouping test проверяет
раздельную пересборку date/content albums и сохранность manualized альбома.
UI-filter test принимает semantic hits без literal substring; редактирование
запроса возвращает keyword search. Настоящие multilingual retrieval/cluster quality
не проверены: Ollama/модели на машине не установлены.
Keyboard regression: End достигает последнего, изначально невидимого фото с
автопрокруткой; Space выделяет, Enter открывает нужный индекс, Home при focus
в поиске не перехватывается сеткой. Полный screen-reader workflow не принят.
Native watcher проверен на создании synthetic файла через настоящий notify backend
Linux; debounce800ms, bounded event queue, максимальное ожидание5с при непрерывном
потоке изменений. Проверка больших/сетевых папок и Windows/macOS ещё остаётся.

Дополнительные gesture regression: vertical touch+pointer drag действительно
прокручивает галерею и сохраняет kinetic tail после release, не открывая thumbnail
и не меняя section. Для скорости используется kittest60fps: default4fps стирает
100ms pointer velocity history и не подходит для проверки инерции. Two-finger
pinch увеличивает фото и переносит centroid даже при wheel-navigation mode,
не переключая кадр. Это всё ещё synthetic events, не реальный touchscreen.

Add-to modal: проверены центрирование и внешний размер362×650 при client1500×960,
выбор альбома, завершение worker assignment и сохранность исходного файла.
Панель содержит настоящие обложки и inline-создание альбома; это не точная
приёмка каждого состояния исходного Photos.

PDF: byte xref/page count, white alpha compositing, atomic export, original-path
guard и сохранение старого destination при decode error. `pdfinfo` независимо
прочитал synthetic двухстраничный A4 PDF1.4. Настоящий Print dialog/job не проверен.
File clipboard: тестируется URI escaping, включая Unicode, `%`, `#`, newline и
не-UTF-8 пути. Автотесты не меняют системный clipboard пользователя. Windows
FileDropList/macOS aliases написаны, но не собраны/приняты на соответствующих ОС.
Save copy: byte-exact, original/symlink guard, no-clobber любого существующего
destination, отсутствие частичного результата при ошибке. Меню viewer проверяет,
что Escape сначала закрывает overflow и не покидает просмотр; Home/End под меню
не переключают фото. Linux OpenWith portal с ask(true) скомпилирован, нативный
выбор приложения ещё не принят; это отдельная команда, не системный Share.
Thumbnail quota удаляет только generated64hex.png; соседний файл тест сохраняет.

- SQLite: повторное открытие индекса, сохранение альбомов, канонизация и
  дедупликация пересекающихся источников, исключение видео, пути с не-UTF-8
  байтами на Unix, проверка версии схемы и названий альбомов.
- Источники: недоступная папка не стирает прежний индекс; отмена посреди
  сканирования откатывает текущую транзакцию; удаление источника не трогает
  оригиналы. Удаление отсутствующих связей выполняется только после полного
  успешного обхода источника.
- Миниатюры: дисковый кеш повторно используется, устаревший ключ не записывается,
  размеры ограничиваются 400 px, оригинал не меняется.
- Сцена: фильтры имени/пути, альбома и папки, порядок открытия, компактная
  геометрия заголовков дат.
- `egui_kittest`: открытие миниатюры, выделение, переключение разделов,
  открытие диалога альбома, блокировка фонового клика, закрытие через Esc.
- Просмотрщик: открытие библиотечного списка без синхронного декодирования,
  получение текстуры из фонового кеша, повторное открытие с заменой предыдущей
  задачи, закрытие без восстановления старого изображения.
- Новый одиночный просмотр: клики по большим боковым зонам (не теряются за
  drag-областью изображения), масштаб/сведения/подтверждение корзины,
  блокировка навигации под диалогом, колесо только над фото, Ctrl+колесо,
  touch-свайп на fit и pan на увеличении синтетическими touch-событиями.
  Быстрые фоновые прыжки показывают последний выбранный файл; 100% учитывает DPI.
- Геометрия: правая граница scroll viewport у края окна, а не у внутреннего
  отступа галереи (регрессионная проверка).
- R1: title32/Back48×32/window controls46×32 и команда сворачивания,
  viewer viewport начинается в (32,80), zoom-popup не сдвигает фото,
  Info занимает344, fullscreen скрывает верхние панели и Info.
- R2: search outer(120,38)396×36 в viewport1500, commands48/tabs48,
  density25×44 справа, selection не добавляет панель; скрывает search/соседние
  вкладки, включает Add to только при выборе, Cancel/Esc сохраняет фильтр.
  Проверены выбор/снятие группы, modal Add to и graphical density buttons.
  Accessibility TextInput bounds в egui описывают inner text area, поэтому
  regression outer search slot проверяется отдельно от его accessible bounds.
- R3: mixed-aspect layout сохраняет порядок, положительные размеры и границы
  при разных ширинах/плотностях; последняя строка не растягивается.
  Month chooser/Back/jump, блокировка команд, timeline click и anchor после
  смены плотности проверены через kittest. Высота photo response совпадает с
  высотой картинки, filename footer отсутствует. Fixture timestamps фиксированы,
  чтобы тесты не зависели от перехода wall-clock секунды и порядка видимых фото.
  Scrub реализован, но его реальный drag/touch ещё не проверен.

R3 offscreen captures: `/tmp/opencode/rphotos-r3-captures` — восемь состояний
галереи (medium/large/small, месяц-chooser, selection, narrow, dark/HiDPI).
После смены density/данных capture дополнительно ждёт завершения новых видимых
thumbnail requests, чтобы не выдавать пустые placeholders за готовый результат.

## Визуальная проверка

**Историческая оценка пользователя до R1:** интерфейс слишком далёк от Photos на планшете.
Перечисленные ниже успешные сборки и capture-тесты подтверждают работоспособность,
но **не подтверждают визуальное соответствие**. Повторный подробный аудит и
приоритеты переделки: [PHOTOS-2019-AUDIT.md](PHOTOS-2019-AUDIT.md).

Синтетические PNG генерируются самим тестом, пользовательских фото нет:

```sh
RPHOTOS_CAPTURE_DIR=/tmp/opencode/rphotos-library-captures \
  cargo test --locked capture_ -- --ignored --nocapture
```

Проверены светлая RU 1440×900, светлая EN 640×760 и тёмная RU 900×650
при масштабе 2×. Проверены кириллица, перенос панели в узком окне,
пропорции портретных/альбомных миниатюр, подписи и отсутствие огромного
пустого отступа перед датой. Это реальный offscreen-рендер wgpu, но не
скриншот системного окна и не pixel-perfect сравнение с Photos 2019.

Просмотрщик: светлый RU 1440×900, сведения слева, узкий EN 500×700,
тёмный RU. Для снимков synthetic-текстура загружается в контекст renderer-а,
а не в отдельный контекст fixture: это важно, иначе тест мог бы сохранить
пустую область при существующем TextureHandle.

Повторная SSH-проверка референса подтвердила wheel-переход, геометрию боковых
зон, 48 px панель, всплывающий zoom и узкий режим. Временная задача/помощник
на планшете остановлены и удалены, файлы фотографий не менялись.

После этой проверки проведён более подробный аудит: 119 UI-деревьев, 75 исходных
снимков экрана и их preview вне репозитория. Переключатели с изменением настроек
возвращены в исходное состояние; операции поворота/избранного/удаления проверены
только на сгенерированных JPEG, создание/rename/remove — на временном альбоме.
Исходные пользовательские фотографии и альбом не изменялись. Временные источники,
тестовый каталог и remote helper/tasks удалены. Сенсорная инъекция не сработала;
этот аудит не подтверждает реальные жесты. В рамках самого аудита код UI Rphotos
не менялся. После него выполнена базовая переделка R1 (см.
[IMPLEMENTATION.md](IMPLEMENTATION.md)); таблица результатов обновлена для неё.
Новые capture-файлы: `/tmp/opencode/rphotos-r1-captures`, только synthetic pixels.
Это всё ещё не заявление о полном визуальном соответствии: Library grid/tiles,
Settings и acrylic на тот момент оставались следующими этапами. Интерактивные drag/resize/snap
настоящего окна не покрываются offscreen-тестами viewport commands.

R2 библиотечный shell: `/tmp/opencode/rphotos-r2-captures`, пять GPU-снимков:
светлый RU1440×900, EN640×760, dark RU900×650 при2×, selection RU1500×960,
selection EN500×700. На них проверены высоты панелей, доступность Cancel на
узком viewport, замена search на selection и векторные checkmarks.
На исторических R2-captures ещё видны filename footers и равные клетки; в R3 они
заменены aspect-aware раскладкой. Print/share/copy/bulk-delete добавлены после R2.

## Ограничения проверки

Текущие GPU captures: `/tmp/opencode/rphotos-current-captures`, 17 сцен библиотеки
и5 viewer (светлая/тёмная/narrow/HiDPI, cards/search, albums/detail/picker/folders/
settings, selection, zoom/info). Оба capture-теста пройдены; ключевые PNG прочитаны
визуально. Это synthetic изображения, не личные фотографии и не proof pixel-perfect.

Нативный smoke `/tmp/opencode/rphotos-current-smoke-49tgb5tx`: library/viewer
по5с без раннего выхода. Отдельный EXT blur smoke подтвердил подключение протокола
и alpha190 только в шапке; цветной backdrop, focus/resize переходы ещё не приняты.
Wayland blur не тестируется offscreen kittest и не заявлен для Windows/macOS/X11.

Packaging: scripts/package.py проверен на настоящем Linux debug binary;
tar.gz12 entries, SHA256 перепроверен независимо, пути relative без traversal,
нет credentials/settings/index/private captures. В archive dependency inventory
и license texts из crates, font OFL/UFL и MPL option-ext. Это предварительный
license inventory, не полный юридический аудит (76 crates без отдельного license
file отмечены source URLs). Собственные PNG/ICO/ICNS сгенерированы; PNG просмотрен.
Windows ZIP/macOS packaging и remote CI ещё не выполнены.

- Windows/macOS локально не собирались; CI-конфигурация подготовлена,
  удалённых результатов ещё нет.
- Реальные touch/pinch/swipe (в отличие от синтетических событий), клавиатурная доступность всей сетки и
  screen-reader workflow ещё не проверены.
- Нет замеров на библиотеке в десятки/сотни тысяч файлов. Сетка рисует только
  видимые строки; полный снимок индекса всё ещё загружается в память.
- Есть ограниченные очереди, один thumbnail-декодер, LRU128 GPU-текстур и квота
  дискового кеша512MiB. Не проверены adaptive load и прерывание уже начатого decode.
- Print/Email/file clipboard требуют native workflow проверки; OCR/Ollama quality,
  vector semantic search и auto albums по содержимому не приняты на реальной модели.
- Полный нативный сценарий выбора папки через portal, открытия фото и возврата
  пока требует ручной приёмки. Проверки по отдельным слоям его не заменяют.
- Глобальный `cargo fmt --all --check` не является зелёным: в upstream уже есть
  большие расхождения форматирования. Новые модули форматируются отдельно,
  массовый несвязанный reformat не выполнялся.

## Дополнительный архитектурный референс

Просмотрен Silver Native GUI Lab (`rust-gui-silver-bullet`): полезны разделение
состояния/команд/UI, токены оформления, ограниченные очереди и виртуализация.
Его отчёт сам помечает сборку/запуск/PNG как непроверенные; он не считается
подтверждённой готовой GUI-библиотекой. Код и ресурсы оттуда не копировались;
переход с egui 0.31 на 0.36 в рамках этой итерации не выполнялся.
