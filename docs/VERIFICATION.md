# Проверка Rphotos — 2026-10-19

Проверено локально на Linux / KDE Wayland, Rust 1.98.1.
Это запись фактически выполненных проверок, не матрица обещанной поддержки.

## Результаты

| Проверка | Результат |
| --- | --- |
| `cargo build --locked` | Успешно |
| `cargo test --locked --all-targets` | 208 passed, 0 failed, 11 ignored |
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

Поворот/отражение: все8 EXIF orientations ×4 transforms совпадают с преобразованием
уже отображаемых pixels; little/big-endian EXIF и сохранность прежних TIFF offsets.
JPEG/PNG после сохранения имеют те же decoder pixels до EXIF (без re-encoding);
undo восстанавливает точные байты. Проверены повреждённые/animated containers,
read-only/symbolic/hard links, concurrent source changes, single in-flight worker
и сохранность прежнего undo после неудачного нового edit. Kittest: rotate→flip→undo→
rotate-left, async reload правильных размеров, блокировка перехода при сохранении.
Тесты изменяют только сгенерированные фото; личные файлы не редактировались.
Resize: PNG/JPEG dimensions, white alpha, no-clobber исходника/существующего
destination, safety limits и ориентация, применённая к пикселям без повторного
EXIF-поворота. Kittest: presets/formats/cancel/modal navigation guard. Native
save chooser не вызывается автотестом и остаётся отдельной ручной проверкой.
Narrow overflow: прокрутка достигает «Настройки», wheel над меню не листает фото.
GPU viewer capture PASS:9 synthetic состояний в `/tmp/opencode/rphotos-edit-final-captures`;
поворот, transform menu, resize и narrow resize/menu реально просмотрены.
Debug/opt-dev builds PASS; нативные library и synthetic PNG viewer по5с без
раннего выхода, isolated XDG `/tmp/opencode/rphotos-transform-smoke-n928zxs2`.
Предыдущий library/cache push `be1d64f` прошёл CI на Linux/Windows/macOS:
https://github.com/DurkaEbanaya/Rphotos/actions/runs/39264202034 .

Latest wide-layout change: window-title separator removed (paint regression),
desktop library/search/tabs gutter16px instead120px; date rail88px separate from
photo content. Shell tests now assert search/tabs/photo x16 and density x1409 at1500.
Cache error modal wraps full path/reason and shrinks actual content; regression
asserts Close lies<40px below a single reason (no empty fixed82px slots).
GPU captures3tests PASS `/tmp/opencode/rphotos-wide-layout-captures`; collection,
albums and compact error modal actually viewed, synthetic data only.
Final195/0/11, clippy only8 unchanged upstream warnings; debug/opt-dev builds PASS.
Native opt-dev isolated startup5s PASS `/tmp/opencode/rphotos-wide-layout-smoke-rzrjfmmo`.

Latest unavailable behavior: no placeholder/label/cell. Background presentation
snapshot checks originals before exposing startup photos; index/cache identities
retained. Availability changes invalidate grouped grid and pager preview, remove
offline selection; Collection/Favorites/album/folder and embedded picker filter them.
Offline identities have independent background reconnect probes even when no widget
calls get(); return restores rows and reuses PNG. Regression disconnects18-photo
source with cached PNG before startup: zero rows/months/photo widgets, empty picker,
unchanged index18; restores source and all18 return with identical cached bytes.

### Offline originals, persistent compressed previews, Reveal

PNG cache hit больше не обходит readable-original/key validation. Missing/permission/
replaced original — typed Unavailable, не failed decode: bulk increments skipped,
disk PNG сохраняется. Background metadata/open probes (~2s visible/resident) удаляют
GPU+frost unavailable photo, без UI filesystem I/O. F5 resets status; reconnect reuses
unchanged PNG without original re-decode. This is eventual availability, not atomic
with filesystem disappearance. Incomplete offline scans keep source associations
and suppress missing/permission warnings, not other unexpected errors.
Tests: hide resident/PNG preview after rename; no error/cache deletion; restart and
restore original→same cached bytes; bulk unavailable80/failed0; index survives offline.

Disk usage timestamps refreshed off-thread (hourly), age90d expiry; quota prunes oldest
only>=7d, recently used may exceed soft quota. Unlimited still has age90d expiry.
Tests renew old touch, expire91d, preserve20d with unlimited, overquota prune20d but
preserve recent; foreign files untouched. Compressed format remains existing PNG,
no wholesale recompression or cache invalidation.

ПКМ → «Показать в проводнике»: Linux FileManager1.ShowItems (fallback parent xdg-open),
Windows explorer /select, macOS open -R; offline nearest existing parent. Tests verify
target resolution and actual secondary-click menu entry; no system file manager was
launched by automated tests. Win/mac native workflow remains unaccepted.
Final193/0/11; clippy только8 unchanged upstream warnings; opt-dev build и3GPU
capture tests PASS (`/tmp/opencode/rphotos-offline-captures`). Existing private
library/cache не пересканировались и не чистились этим тестированием.

### Единый спокойный status row (последний UI-срез)

И scanning, и full thumbnail build теперь используют один48px компонент:
фон сцены вместо серой плашки, тонкий separator, оригинальный outline photo-stack,
13px title/11px details, короткая1px шкала максимум280px под текстом.
Нет spinner/default rounded ProgressBar/blue edge через всю ширину окна.
Running/paused/completed/error/stopped сохраняют40px targets и error-report click.
Unknown count не показывает выдуманный процент; paused progress нейтральный,
после завершения шкала скрыта. Tests: narrow/wide actions, local hairline geometry,
scanning counts/pause/stop. Старые записи про2px full-width strip ниже исторические.
Final189/0/11, opt-dev build и clippy успешны (8 unchanged upstream warnings).
Capture3tests PASS `/tmp/opencode/rphotos-status-restyle-final`; light active,
narrow paused, dark errors и integrated scanning PNG просмотрены.

### Cache failures и fast-scroll ready priority

Read-only диагностика18 отсутствующих previews пользовательского индекса:
14 JPG (7 пустых0-byte,7 без JPEG start marker) и4 DNG без embedded JPEG.
Оригиналы не изменялись. Paths/reasons сохранены только в private local report,
не в git/stdout: `~/.cache/rphotos/thumbnails-v1/last-build-errors.json`.
Новый loader различает metadata/change/decode/panic/cache-write failures; interactive
view остаётся доступен при write error, bulk честно сообщает failure. Full build
сохраняет report, background startup его восстанавливает. Modal из меню и clickable
error strip показывает файл/path/reason; F5/background actions заблокированы.
Tests: decode/stale key/unwritable cache и original preservation; persisted report,
restart/modal/close, wide/narrow error-strip click. Private diagnostic — отдельный
ignored opt-in test с explicit DB/cache/report environment, не часть обычного CI.

32-slot FIFO заменена bounded64 ready queue с выбором visible-first; смена viewport
выбрасывает obsolete decoded results, не только queued disk jobs. Full ready queue
не блокирует все workers на speculative output: visible вытесняет ahead result.
Во время motion upload до нового plan откладывается; новые visible pixels
загружаются после plan перед paint. Регрессия заполняет64-ready без UI drain и
резко меняет viewport на10 других cached PNG/offline originals; все10 получены.
Это исправляет head-of-line задержки, но не гарантирует zero placeholders при
произвольном скачке в холодную GPU/disk область: чтение/PNG decode/upload нужны.
Final checks:188/0/11, all-features clippy только8 upstream warnings; debug/opt-dev
build успешны. Capture3tests прошли (`/tmp/opencode/rphotos-errors-captures`),
25library+5viewer+5task strips; error modal PNG просмотрен, только synthetic paths.

Cache task redesign: shared48px flat strip,2px square accent edge; accessible
40x40 pause/resume/stop/dismiss actions, wide/narrow states, no zero-error text.
Tests проверяют geometry/actions, отсутствие rounded progress fill. Полная UI
прогрузка18/18 теперь dismiss обнуляет только notification; в preferences нет
кнопки запуска (только главное menu…). Separate opt-in capture_cache_task_strip
рендерит active/paused narrow/completed/error dark/stopped без пользовательских фото.
Capture `/tmp/opencode/rphotos-taskstrip-captures`: все5 strips просмотрены,
включая wide active/narrow paused/dark errors и интегрированный library completion.
Opt-dev native smoke5с без раннего выхода (isolated XDG):
`/tmp/opencode/rphotos-taskstrip-smoke-koq_bs2o`. Decode timing regression теперь
ждёт empty pending и итоговую debounced revision, а не фиксированный sleep650ms.

Chrome microfix: maximize/restore icon только outline, без повторной заливки
полупрозрачным header. Restore задний контур обрезан геометрией, а не opaque mask.
Regression проверяет отсутствие fill у обеих icon shapes; chrome geometry/commands
сохранены. Полный прогон183/0/9, opt-dev build успешен.

### Полная прогрузка и большая дисковая квота

Кнопка в overflow/настройках: worker получает snapshot всех indexed photos,
независимо от query/album. Отдельная bulk очередь не вытесняется viewport plans,
работает после visible/ahead, progress/pause/cancel generation. Результаты bulk
не отправляются в interactive ready queue и не занимают texture cache. Проверка
81entries (80generated PNG+missing):done81/errors1 без UI poll, кеш переживает
prune(budget0) и последующий restart с перемещёнными оригиналами. UI regression:
query оставляет1photo, кнопка всё равно кеширует18/18, оригинал сохранён.
Queue regression проверяет pause/cancel и сохранность bulk при смене viewport.

Default disk quota16GiB, AppSettings.thumbnail_disk_gib (0=unlimited), serde default
сохраняет совместимость старых settings. Полная прогрузка ставит0 и сохраняет
marker под private cache; maintenance сериализован с marker writes. Возврат к
finite quota инвалидирует marker без удаления чужих файлов; протестирован.
Startup не удаляет кеш по устаревшей квоте до применения settings. Не выполнена
полная прогрузка пользовательской библиотеки — только synthetic originals.
Write/decode errors учитываются; диск может заполниться, гарантии free space нет.
Capture `/tmp/opencode/rphotos-bulk-captures`:24 library states, full-cache status
18/18/errors0 PNG просмотрен. Нативная interactive прогрузка пользовательской
библиотеки не запускалась — кнопка оставлена пользователю.

### Большая библиотека: отклик, бегунок, знаменатель сканирования

Текущий пользовательский индекс прочитан только для агрегатов:30323 photos,
1695 aspects; пути/содержимое не выводились и не менялись. Исправлены повторные
Unicode/date grouping при aspect reflow, reflow во время движения, копирование
OCR/description в каждой видимой Photo и линейный LRU retain() на каждом draw.
Aspect batches применяются после180ms покоя, не чаще600ms; geometry/filter
changes немедленные. Upload CPU budget2ms/4MiB во время ввода,5ms/16MiB в покое.
Eviction ограничен1ms/32texture за кадр. Кеш2048 resident/soft512MiB/hard4096,
prefetch8screens вперед/3назад и очередь2048 — за счёт большей памяти.

Regressions: aspect arrivals не меняют layout revision во время scroll, idle
reflow сохраняет filtered order; recently revisited texture переживает oldest-first
eviction, byte accounting совпадает; UI handle_min_length64px; filesystem count
совпадает со scan и отмена не меняет index; status показывает discovery и1000/30323.
Opt-dev synthetic100k: snapshot129ms/layout35ms; moving small-grid с искусственным
aspect arrival каждый кадр,180frames: CPU p50=0.209ms/p95=0.224ms/max=0.332ms.
Это metadata/UI-only без GPU submit/реального decode, не гарантия native frametime.
Лог `/tmp/opencode/rphotos-responsive-moving-100k.log`.
Capture `/tmp/opencode/rphotos-responsive-captures`:23 library states, включая
status12000/30323 с39%; PNG просмотрен. Native opt-dev library/viewer по5с,
isolated XDG, без раннего выхода: `/tmp/opencode/rphotos-responsive-smoke-42j8oypa`.
Исходная пользовательская библиотека не пересканировалась тестом и не изменялась.

Total сначала неизвестен: count_images делает отдельный filesystem pass, UI пишет
«подсчёт файлов». После него denominator и percent реальные для этого обхода;
недоступные каталоги не считаются, live mutations могут изменить total. Ошибки
основного scan по-прежнему сохраняют старые associations, отмена rollback прежний.

### Предзагрузка, постоянная дата и непрерывный масштаб

Queue regression: visible promotion/dedup/running preservation, replacement stale
prefetch и bounded2048 queue. Integration:80 generated originals, только10 visible,
остальные70 готовы до первого get(), исходник не изменён. LRU200-visible regression
остаётся зелёным. Kittest проверяет промежуточные zoom1.13/1.07, настоящий
Ctrl-wheel без изменения app zoom, multi-touch pinch без смены section, повторный
preset reset. Текущая дата доступна без hover (label month/year).

Opt-in `thumbnail_throughput_probe`:120 generated JPEG1200×800, opt-dev на
i5-14600KF/20 logical CPU, fresh thumbnail dirs, warm OS file cache:
1 worker1.716s (~70/s),16 workers0.186s (~646/s), ~9.2×. Включает decode/resize/
frost/PNG write/texture registration; не включает GPU submit и cold-media latency.
Это синтетический замер, не гарантия для HDD/RAW/large photos. Команда:
`cargo test --locked --profile opt-dev thumbnail_throughput_probe -- --ignored --nocapture`.

Новый GPU capture: `/tmp/opencode/rphotos-preload-verified-captures`,22 states,
включая intermediate size177px и постоянно видимую двухстрочную дату. Synthetic
PNG просмотрены; дата не требует hover. Opt-dev native smoke library/viewer по5с:
`/tmp/opencode/rphotos-preload-smoke-2agzbv0m`, процессы остановлены тестом.

### Внешний вид: шкала, меню, избранное и small-grid cache

Повторно просмотрен Photos2019 на Windows-планшете через временный interactive SSH
helper: верх списка, промежуточная прокрутка, нижняя дата и flyout «…». Полый
маркер меняет позицию; flyout имеет светло-серую квадратную поверхность, строки32px,
иконки слева. Private captures остались только в `/tmp/opencode`, не в репозитории.
Photos возвращён к началу; helper/task удалены, отсутствие обоих проверено.

В Rphotos: интерполируемый полый marker, вкладка Favorites после Folders (включая
pager/Ctrl4), hover heart с blurred-photo backing, красная заливка выбранного,
flat flyout160px со строками32px и line icons в разделах preferences.
Это приближение к reference, не pixel-perfect acceptance.

Small-grid flicker: idle LRU128 вытеснял всё ещё видимые textures при плотной сетке.
Первое исправление защищало current/previous visible working set (ceiling1024,
idle128); текущий expanded cache2048/4096 описан выше.
Regression держит200 visible textures20 кадров без reload, затем проверяет eviction
offscreen. Остальные новые tests: heart click не открывает фото, worker favorite
persist/filter/unfavorite (включая настоящие pointer press/release, не только
accessibility invoke), marker движется внутри одного месяца, menu geometry,
Escape и отсутствие Favorites в overflow. Полный прогон173/0/8.

Offscreen captures: `/tmp/opencode/rphotos-visual-final`,21 library states,
в том числе flyout/hover heart/Favorites/preferences; только generated gradients.
Search/ML implementation в этой итерации не расширялись.
Opt-dev native smoke: library/viewer по5с в isolated XDG,
`/tmp/opencode/rphotos-appearance-verified-smoke-3fstclxa`, без аварии; процессы остановлены тестом.

Последний срез R4–R6: SQLite migrations v1→v10; create/assign/rename/cover/unassign/
remove album с rollback; папки только текущего уровня; auto date events и saved
queries; OCR/cache-key invalidation; сохранение thumbnail aspect и избранного;
round-trip scene/query/scroll/Hide cards без transient selection/dialogs.
Kittest: full-page picker/settings и album lifecycle, настоящие album cards,
Hide/Show, dropdown сохранённого поиска, pager axis-lock/vertical escape и
блокировка открытия фото во время свайпа. Локальные OCR/vision adapters проверены
fake Tesseract и loopback HTTP mock; реальные движки не установлены.

Pager regression проверяет две одновременно нарисованные реальные сцены,
координаты disabled соседнего tile, сохранность filtered collection, переход
Collection→Albums→Folders и отсутствие wrap на краю. Release сохраняет исходную
позицию страницы перед snap; layout/scroll/focus preview не заменяют текущую сцену.
GPU capture `section-swipe` дополнительно фиксирует промежуточный кадр.
Короткий swipe, TouchCancel и открытие modal не переключают tab; snap возвращается
в нулевой offset и не оставляет скрытую блокировку кликов.

Model privacy preflight: проверены отказ cloud aliases, nested remote_host/model,
отсутствующих/неверных capabilities и отмена до network request. Worker проверяет
metadata до передачи фото или текста; старый сервер без capabilities отклоняется.
Не является защитой от заведомо недоверенного proxy, выдающего себя за local Ollama.

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
FileDropList/macOS aliases скомпилированы remote CI; нативный clipboard workflow
на этих ОС ещё не принят.
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

Текущие GPU captures: `/tmp/opencode/rphotos-pager-captures`, 18 сцен библиотеки
и5 viewer (светлая/тёмная/narrow/HiDPI, cards/search, albums/detail/picker/folders/
settings, selection, zoom/info). Оба capture-теста пройдены; ключевые PNG прочитаны
визуально. Это synthetic изображения, не личные фотографии и не proof pixel-perfect.

Нативный smoke `/tmp/opencode/rphotos-pager-smoke-kkokls13`: library/viewer
по5с без раннего выхода. Отдельный EXT blur smoke подтвердил подключение протокола
и alpha190 только в шапке; цветной backdrop, focus/resize переходы ещё не приняты.
Wayland blur не тестируется offscreen kittest и не заявлен для Windows/macOS/X11.

Packaging: scripts/package.py проверен на настоящих Linux debug/opt-dev binaries;
исходный tar.gz12 entries, SHA256 перепроверен независимо, пути relative без traversal,
нет credentials/settings/index/private captures. В archive dependency inventory
и license texts из crates, font OFL/UFL и MPL option-ext. Это предварительный
license inventory, не полный юридический аудит (76 crates без отдельного license
file отмечены source URLs). Собственные PNG/ICO/ICNS сгенерированы; PNG просмотрен.
В актуальный формат добавлены PREVIEW/связанные docs и BUILD-INFO.json (commit,
profile, OS/arch, binary SHA256, dirty flag при упаковке). Python regression
проверяет22 Linux /21 Windows/macOS entries на synthetic inputs. Binary из
Linux архива извлечён и запущен5с в isolated XDG без раннего выхода.

Remote CI выявил и помог исправить две платформенные ошибки тестов:
macOS canonical `/private/var` вместо alias `/var`, Windows SystemTime precision
100ns вместо1ns. Тесты не отключались. Полные runs39168800751 (`06d7821`) и
39169792775 (`d089114`) успешны на всех трёх ОС: tests, all-features checks,
opt-dev build, Python archive regression, package и upload artifacts.
Последний run: https://github.com/DurkaEbanaya/Rphotos/actions/runs/39169792775 .

- Windows/macOS локально не собирались; tests/build/package на GitHub runners
  прошли. Настоящий GUI workflow на пользовательском устройстве этим не принят.
- Реальные touch/pinch/swipe (в отличие от синтетических событий), клавиатурная доступность всей сетки и
  screen-reader workflow ещё не проверены.
- Metadata-only scale probe на100000 synthetic записей: snapshot422ms, layout173ms,
  filename search14ms; warm CPU frame p50/p950.37/0.39ms, максимум78 paint shapes.
  Это debug build на этой машине,60 кадров без GPU submit и декодирования оригиналов,
  не обещание скорости настоящей библиотеки/диска. Первый прогон был остановлен
  после95с: correlated aspect query заменён на materialized key set; повтор завершился
  за1.72с целиком. Query-plan regression запрещает correlated photo scan.
  Воспроизведение: `cargo test --locked large_library_scale_probe -- --ignored --nocapture`.
  Полный snapshot остаётся в памяти; реальный scan/decode/RSS/network volumes не замерены.

  Scale probe — восьмой opt-in ignored test, выполнен отдельно и прошёл.
  Последующий query-plan regression входит в обычные169 tests. Preview1 собран из
  `d089114` до этой оптимизации; performance fix находится в следующем main commit.
- Есть ограниченные очереди, parallel thumbnail pool, resident cache2048 GPU-текстур
  (защищённый working set, guard4096, soft512MiB) и квота
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
