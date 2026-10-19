# Исправления аудита 1–20

Это матрица реализации, не заявление о полной нативной приёмке. Лимиты памяти
(пункт10) и доработка поиска (пункт19) по решению пользователя не менялись.

| № | Реализация и оставшиеся ограничения |
|---|---|
| 1 | `ctx.input` только читает события; F11/fullscreen Esc/zoom arrows/Home-End проверены regression. |
| 2 | Unix owner/mode/xattrs, Windows ReplaceFile merge; fail closed при ошибке. До замены повторная проверка байтов. Последний TOCTOU check→replace остаётся; не atomic CAS против внешнего редактора. |
| 3 | Durable private recovery-v1 перед записью; Undo после restart при совпадении SHA256. Один последний шаг на путь, без автоквоты/удаления. Resize имеет Save copy и отдельное Save overwrite с подтверждением. |
| 4 | v11 file identity: перенос ID/favorites/albums/text при однозначном same-filesystem rename с совпадающим size/mtime. Не объединяет копии/hardlinks; cross-volume copy-delete не определяется. |
| 5 | Атомарные settings, сохранение corrupt YAML, UI error. Однократный VACUUM INTO backup перед миграцией. Полной recovery/export UI базы пока нет. |
| 6 | Обычные мутации используют cached availability, не открывают все оригиналы. Полная проверка осталась при initial/Refresh/AddSource; startup сетевых источников ещё может ждать. |
| 7 | Selection retention через HashSet вместо selection×photos. |
| 8 | Отдельные bounded interactive/analysis lanes, WAL connections; FS/EXIF/pause вне write transaction, атомарное применение наблюдений после обхода. Публикации сериализованы, SELECT snapshot согласован. Большое SQL-применение ещё может кратко задержать запись. |
| 9 | Trash worker/progress/stop queued files/error report; CLI/drop directory discovery и следующий decode фоновые. Close во время оригинального Save/Trash блокируется до конца. Начатую OS trash operation отмена не прерывает. |
| 10 | **Не менять** — существующие CPU/GPU/thumbnail limits сохранены. |
| 11 | Derived album/folder caches и picker layout cache; suggestions строятся worker, не UI. Ошибки кеша по50/страницу. Полный snapshot всё ещё O(N) и хранится в RAM. |
| 12 | Неудавшиеся/пропавшие watcher roots повторяются каждые2с, восстановление вызывает refresh. Реальный network mount acceptance остаётся. |
| 13 | Library overflow ограничен viewport и прокручивается. |
| 14 | Month header bounds/search popup container Reveal, album month chooser overflow доступен. |
| 15 | Настоящий focus thumbnail не отключает grid navigation, regression с focused photo → End. |
| 16 | Helpers с30с deadline; ожидание native chooser можно покинуть без обещания отмены другой программы. Save original нельзя бросить/закрыть. |
| 17 | Native100% viewport tiles2048, source dimensions; LittleCMS embedded RGB ICC→sRGB; optional installed LibRaw fallback при отсутствии JPEG. Не disk-backed decode, не monitor-ICC/CMYK/16-bit pipeline; preview при наличии по-прежнему предпочитается. |
| 18 | Print Windows WPF/macOS PDFKit, OpenWith Win32/macOS chooser, native Share WinRT/AppKit, blur/vibrancy через window-vibrancy. Linux portal/actions сохранены. Новые Win/Mac пути требуют CI и реальных диалогов/дисплея; Linux не имеет универсального Share portal. |
| 19 | **Позже** — поиск не расширялся; перенос vocabulary в worker не меняет алгоритм. |
| 20 | README/PRODUCT/IMPLEMENTATION/VERIFICATION обновляются этим срезом. Preview1 исторический, не содержит новых изменений; новый release только после native CI. |

## Проверка

Локально Linux:233 tests passed /12 ignored; clippy только8 прежних upstream
warnings. Native sensor path проверен оригинальным сгенерированным Bayer DNG64×64
через установленный LibRaw, без JPEG. Нельзя считать это проверкой всех камер.
Тесты edits/recovery используют только synthetic files и отдельную temporary recovery
directory; личные оригиналы/clipboard/printer не изменяются.

Новые зависимости: xattr, LittleCMS2 (static), window-vibrancy; LibRaw динамический,
его установка/лицензия относится к пользователю системы. Monitor color management,
реальные touch/print/share и всех file attributes на native OS требуют приёмки.
