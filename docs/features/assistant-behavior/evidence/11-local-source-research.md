# Подготовка локального источника11 — 19 октября2026

Это исследования следующего фронта, не реализация11. Они выполнялись параллельно06F, который затем принят локально; его изменяющийся код для первоначальных выводов не использовался. Пользовательские файлы/репозитории не сканировались, новые модельные запросы не выполнялись.

## Проверенные зависимости

Scout подтвердил: локальные файлы/ветки не требуют capture03. Используемая память07 уже имеет SourceKey, provenance, task revision, отдельные bind/ingest и защиту забытых ключей. SourceKey задаёт идентичность, а не разрешение чтения. Production чтение по-прежнему запрещено при ручной паузе или недоступном облачном пути; локальные временные файлы и HTTP позволяют разрабатывать реальный адаптер без Jev/TCC.

Готового file/Git адаптера, NSOpenPanel/bookmark integration или Git-библиотеки в проекте не было. App Sandbox выключен. Полная11 остаётся открытой: обнаружение забытой работы, ветки, файлы, частые изменения, исключения и настоящий модельный результат.

## Native средства и пределы

Librarian и External Researcher проверили Foundation/AppKit/Darwin в SDK macOS27 при deployment target14:

- NSOpenPanel выбирает файл или папку. После allowedContentTypes явно установить canChooseFiles/canChooseDirectories; allowsMultipleSelection=false, resolvesAliases=false. Панель не создаёт системной файловой границы при выключенном App Sandbox.
- URL bookmarks/security scope существуют, но постоянное восстановление разрешений не обязательно для одного явного импорта. Успешный startAccessingSecurityScopedResource балансируется stopAccessing.
- Покомпонентный openat относительно закреплённого directory FD с O_NOFOLLOW/O_DIRECTORY/O_CLOEXEC защищает от следования symlink в каждом проверяемом компоненте. Финальный O_NONBLOCK и fstat позволяют отвергнуть FIFO/необычный файл до чтения. Проверка st_nlink==1 отвергает существующие дополнительные hard links в момент проверки.
- Это не атомарная файловая песочница: каталог с открытым FD можно переместить, hard link можно создать после проверки. Нельзя обещать устранение всех таких гонок только по факту использования openat. O_UNIQUE/O_RESOLVE_BENEATH не подтверждены для macOS14 и не должны становиться предпосылкой.
- Для текстового импорта достаточно чтения не более16KiB+1byte, строгого UTF-8 и явного отказа при превышении; не усекать и не заменять повреждённые символы.
- До/после чтения можно сравнить dev/ino, size, nanosecond mtime/ctime и отклонить обнаруженное изменение. Равенство не доказывает атомарный снимок; digest описывает прочитанные байты. FileResourceIdentifier/volumeIdentifier не гарантированы через reboot; volumeUUID persistent, но может отсутствовать. SourceKey по локации и grant должны быть разными понятиями; смена root/grant не должна незаметно обходить forgotten-key защиту.
- Cancel не гарантирует прекращение synchronous read. Даже MNT_LOCAL не даёт строгой задержки. Worker остаётся занят до возврата I/O/закрытия FD; поздний результат не принимается после отзыва/паузы.

Опорные источники: installed SDK NSOpenPanel.h:24, NSURL.h:428–455/514/603, man2/open.2:103, man2/read.2:77, sys/mount.h:222; [Apple open](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/open.2.html), [Apple stat](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/stat.2.html), [NSOpenPanel](https://developer.apple.com/documentation/appkit/nsopenpanel), [FileHandle](https://developer.apple.com/documentation/foundation/filehandle).

## Git: обычный read-only запуск недостаточен

Системный Git2.54.0 установлен. Исследованы также git2 0.21.0/libgit2 и gix0.88.0; это не решение добавить dependency.

- Фиксированный for-each-ref даёт refs/heads, вершины и commit metadata; возраст не доказывает незавершённость, локальные refs не доказывают отсутствие PR.
- --no-optional-locks запрещает только необязательные lock-операции; --no-lazy-fetch отдельно запрещает загрузку отсутствующих объектов. Нужно исключать fsmonitor, pager/filter/diff/signature и исполнение проектных команд.
- --no-includes относится к git config, не ко всем командам. В2.54.0 локальные includes читаются до command-line overrides. Поэтому набор -c не доказывает отсутствие чтения вне granted roots.
- Пустой GIT_ALTERNATE_OBJECT_DIRECTORIES не отключает objects/info/alternates. .git, commondir и связанные worktree также могут вывести чтение за выбранный root.
- git2 open flags NO_SEARCH/NO_DOTGIT/BARE не являются файловыми разрешениями; BARE лишь откладывает чтение config.
- gix Options::isolated отключает global/system/env config и includes, но не все локальные config/commondir/alternates чтения. Это не filesystem sandbox.

Источники: [Git layout](https://git-scm.com/docs/gitrepository-layout), [for-each-ref](https://git-scm.com/docs/git-for-each-ref), [Git2.54 options/security](https://git-scm.com/docs/git/2.54.0), [config2.54](https://git-scm.com/docs/git-config/2.54.0), [Git config source](https://github.com/git/git/blob/v2.54.0/config.c#L1515), [Git alternates source](https://github.com/git/git/blob/v2.54.0/odb.c#L520), [git2 flags](https://docs.rs/git2/0.21.0/git2/struct.RepositoryOpenFlags.html), [gix isolated permissions](https://github.com/GitoxideLabs/gitoxide/blob/gix-v0.88.0/gix/src/open/permissions.rs#L48), [gix commondir](https://github.com/GitoxideLabs/gitoxide/blob/gix-v0.88.0/gix/src/open/repository.rs#L197), [gix alternates](https://github.com/GitoxideLabs/gitoxide/blob/gix-v0.88.0/gix-odb/src/alternate/mod.rs#L58).

## Уточнение: Git-парсеры без открытия репозитория

External Researcher затем подтвердил публичные byte-only входы Gitoxide: `gix-ref0.69.0` — `file::loose::Reference::try_from_path(FullName, &[u8], Kind)` и `packed::Iter::new(&[u8], Kind)`; `gix-config0.62.0` — `File::from_bytes_no_includes`; `gix-hash0.28.0` — `ObjectId::from_hex`. Несмотря на имя `try_from_path`, этот вход получает уже прочитанные байты, а не открывает путь. `from_bytes_owned` применять нельзя: он может разрешать includes. Публичность и совместимость диапазонов зависимостей проверены по исходникам; отдельная пробная компиляция ещё выполняется. Наличие файловых и записывающих API в самих crates не даёт права использовать их.

Librarian затем скомпилировал изолированную пробу в `/tmp` на Rust1.99.1: три теста loose symbolic/direct refs, packed refs и config с include/includeIf прошли. `gix-ref` и `gix-hash`: default-features=false, features sha1/sha256; `gix-config`: default-features=false, без дополнительных features. `File::from_bytes_no_includes(bytes, Metadata::api(), Options::default())` не разрешает includes; `section_by_key` и обход sections дают доступ к значениям. Packed iterator предполагает сортировку, каждый элемент fallible; limits и проверка сортировки/дублей остаются обязанностью потребителя. Проба не проверяла файловый доступ/обход/отмену и не меняла проект.

Источники: [loose refs](https://github.com/GitoxideLabs/gitoxide/blob/gix-ref-v0.69.0/gix-ref/src/store/file/loose/reference/decode.rs), [packed refs](https://github.com/GitoxideLabs/gitoxide/blob/gix-ref-v0.69.0/gix-ref/src/store/packed/iter.rs), [config без includes](https://github.com/GitoxideLabs/gitoxide/blob/gix-config-v0.62.0/gix-config/src/file/init/mod.rs), [OID](https://github.com/GitoxideLabs/gitoxide/blob/gix-ref-v0.69.0/gix-hash/src/object_id.rs).

Это позволяет отделить ограниченное native чтение от разбора Git в Core без собственного Git-парсера, CLI, Repository::open и ODB. Неподдерживаемые gitdir/commondir/reftable, неполнота и ошибки не должны выглядеть пустым репозиторием. OID не доказывает наличие объекта, возраст коммита, незавершённость или отсутствие PR. Байтовый парсер также не решает ограничения файловых разрешений и гонок, описанные выше.

## Независимый вариант, ещё не принятый контракт

Plan Reviewer независимо предложил один выбранный UTF-8 рабочий файл → существующее дело → помощь06F, без обхода дерева/Git/watchers. Native владеет FD и чтением, Core — допуском/ревизией/связью и результатом; нет второго хранилища. Это полезно для планов/черновиков вне программирования, но не закрывает забытые ветки и полную11.

После доказательства публичных byte-only парсеров Reviewer пересмотрел рекомендацию: первым ограниченный обзор локальных веток выбранного проекта → сохранённое дело → помощь06F. Поддерживаемая первая форма — обычный рабочий каталог с непосредственным `.git`; только HEAD, локальные refs, packed-refs и необходимая config, без объектов/индекса/рабочих файлов. Это наблюдение за интервал чтения с явной полнотой и источником. Требуется отдельный контракт grant/SourceKey/исключений и проверки перед чтением, сохранением и использованием результата. Рекомендация ещё не является разрешением на реализацию или чтение настоящих проектов.

До билета нужно закрепить точный смысл grant/exclusions при переименованиях, SourceKey независимо от нового разрешения, правила устаревания источника, границы синхронного I/O и применение существующего06F к разрешённому внешнему evidence. Не выдавать предположения о файловой защите или ещё не реализованном допуске за доказательства. Смена App Sandbox или новый helper не приняты. Общая разрешённая область разработки11 не означает разрешения сканировать настоящие файлы во время исследования.

## Уточнения договора и отдельная native проба

Type Scaffolder read-only исправил своё первоначальное требование постоянного физического ID: ADR-004 определяет identity конкретным адаптером и обещает защиту известных точных ключей, явно не обещая неизвестные aliases. Поэтому расположение+fullref допустимы при устойчивом ключе повторного выбора того же пути и отсутствии автоматической привязки после переноса. Та же удалённая пара task/key терминальна даже для bind_source; явная привязка того же ключа допустима к другому живому делу. Folder exclusions по D40 не обещают физического следования после rename. Эти уточнения закреплены в ADR-011, не расширяют исходные гарантии.

External Researcher проверил `fcntl(fd, F_GETPATH, typedPointer)` в Swift6 fixture в `/private/tmp`: warnings-as-errors и deployment arm64-macos14.0 прошли, Mach-O minos14. Фактический запуск macOS27.0, не macOS14 hardware. MAXPATHLEN=1024/F_GETPATH=50. Rename изменил выдаваемый путь; replacement имел другой dev/ino. После rmdir удерживаемого root F_GETPATH успешно вернул старое имя: один этот вызов не доказывает существование по пути. Для check нужно сравнить повторный F_GETPATH с исходным и заново покомпонентно nofollow открыть исходный путь, сравнив dev/ino. Ошибка/отсутствие/различие отвергает результат; move-out-and-back между проверками и post-check race не исключены. Buffer требует bounded NUL и strict UTF-8. Все fixture данные удалены; никакие реальные проекты не читались.

Опоры: [Apple fcntl/F_GETPATH](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/fcntl.2.html), installed SDK sys/fcntl.h:259 и man2/fcntl.2:155. Optional baseAddress пришлось раскрыть через guard let, чтобы Swift выбрал доступный typed-pointer overload; C-wrapper не требуется.

Finished Plan Reviewer выявил отдельную важную границу: изменение/удаление настройки должно запрещать отправку старого сохранённого source context, а не только новое чтение. ADR-011 теперь проверяет current root settings/исключения на prepare/dispatch и включает их revision в snapshot/receipt/copy. Память исторического факта остаётся; watcher/новое чтение для этой проверки не нужны. Evidence Audit после конкретных поправок — PASS локального перехода к билету, без объявления full11 или macOS14 hardware пройденными.
