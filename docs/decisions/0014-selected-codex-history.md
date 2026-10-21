# ADR-014: выбранная сохранённая история Codex

Статус: принят для14A от19 октября2026. Finished Plan Review и Evidence Audit, включая targeted corrections, PASS. 10A и14A приняты локально20 октября;14A verify8/135/70, feature Swift101, build/signature/review PASS. Доказательства: target/verification/14a/. Основание — D56/D57 и подтверждённый ограниченный результат02. Полный14 сохраняет автоматическое связывание, инициативу и фактическую G6-приёмку.

## Problem Statement

Ассистент уже использует выбранные сведения дела и локальные ветки, но не сохранённый разговор Codex. Нужны точные сообщения, планы как текст и наблюдённые завершения ходов с происхождением. Конец ответа агента не доказывает выполнение задачи, проверку PR или запуск следующей волны. Длинный текущий чат нельзя отвергать только потому, что он вырос за предел16MiB пробника02.

## Solution

Пользователь выбирает один локальный rollout, явно загружает ограниченное окно его сохранённых записей, выбирает элемент и сохраняет его в дело. Сохранённое сведение идёт в существующий06F/08A context/preview/Send. «Ранее» и «Обновить» читают новые окна только по нажатию; история целиком не копируется и не индексируется.

Native владеет выбранным файловым расположением и ограниченным I/O. Core — настройками доступа/ревизиями, разбором закреплённой схемы, проекцией событий, привязкой и допуском. Используются прежние Store/SourceKey/SourceEvidence и model transport. Нет Codex CLI/app-server, доступа к SQLite Codex, запуска/продолжения агентов, автоматического поиска sessions, общего connector framework или движка волн.

## User Stories

1. Я явно выбираю файл чата и вижу, какая версия и история доступны.
2. Я могу посмотреть свежие сообщения большого чата и по запросу перейти к более ранним.
3. Я сохраняю точный доступный prompt или ответ в выбранное дело; большой текст не обрезается молча.
4. Я различаю сообщение из Codex, делегированный ввод, текст агента и наблюдённое завершение хода.
5. Я вижу неполноту окна/истории и не получаю выдуманный статус «работает» из отсутствия terminal.
6. Удаление источника прекращает использование его сведений в помощи, сохраняя ранее записанную историю.
7. Копия файла не меняет происхождение уже привязанного события без моего решения.
8. Я получаю помощь существующим путём и сам копирую/запускаю следующий prompt; копирование не считается запуском.

## Implementation Decisions

### Граница и версии

Один новый конкретный адаптер Codex. Существующие Git-настройки/local-source ABI1 не переименовываются в универсальный framework. В том же Core Store добавляются Codex settings, временное чтение/window и persisted association выбранного evidence. Узкий `ambient_codex_v1` C ABI использует существующий memory handle. Memory3/auth1/source1/model3 сохраняются; schema7 заменяет6, старое хранилище отвергается без записи/миграции/reset по принятому подходу проекта.

Source settings fence становится типизированным для Git и Codex; каждое основание сохраняет ограничения своего реального источника. Это два конкретных варианта, не открытый plugin registry. При изменениях/удалении/паузе/session reset отменяются оба вида source read; нельзя потерять проверенные Git fences при добавлении второго вида.

10A должен завершить запись до реализации. Core остаётся владельцем разрешений источника; native не создаёт второй persistent mapping/ledger. Native передаёт каноническое расположение как OS-факт, Core сохраняет выбранный путь и revision, native сверяет фактический файл перед I/O и результатом.

### Выбор и файловое чтение

Настройки: до64 выбранных файлов, до16KiB записи и1MiB всех Codex settings; монотонные revisions используют прежний Core clock. Выбор/переименование настройки не запускают чтение автоматически. Файл открывается только по явному чтению после текущего High Fast/report/session/unpaused допуска. В production не ищутся файлы в ~/.codex и не открываются referenced rollouts из содержимого.

Один Codex read job и одно текущее окно на Store, без очереди страниц. Job фиксирует task/revision, setting/revision, epoch и request identity. Native открывает обычный файл, не следует symlink-компонентам, сверяет фактический выбранный путь и file identity до чтения и перед результатом; FIFO/device/directory отклоняются. Использовать установленные native bounded read/seek/FD механизмы. Не запускать shell/Codex и не изменять исходный файл. Отмена прекращает дальнейшее чтение, но slot освобождается только после реального возврата I/O и закрытия FD.

Сначала читается только первая законченная строка session_meta, максимум256KiB+1байт. Для окна читается не более1MiB исходных байтов и1байта проверки границы. Допускается файл большего размера; в памяти не существует его полного содержимого. Одна законченная JSONL-строка ограничена256KiB, просмотр —4096 завершёнными строками и128 нормализованными элементами/256KiB их суммарного текста и метаданных. Все пределы проверяются до аллокаций/FFI копий по размеру внешних данных.

Свежее окно ограничивается наблюдённым размером файла. Cursor — opaque Core token текущего выбранного файла, его settings revision, file identity, header fingerprint и byte boundary, не пользовательский offset. «Ранее» заканчивается перед самой ранней разобранной физической JSONL-строкой, независимо от появления элемента в UI. Служебные, отфильтрованные и неподдержанные строки также продвигают cursor. При item/byte limit курсор не перескакивает непринятые строки. Частичный leading fragment перечитывается следующим окном без отдельного накопителя и не теряется молча. Если целой записи нет, курсор всё равно продвигается назад на прочитанный диапазон с явной потерей. Повтор без продвижения запрещён. При достижении item/byte bounds ранние непринятые записи остаются доступны через «Ранее»; невозможность показать oversized record отмечается отдельно.

Незаконченная последняя строка не является событием. Append за наблюдённую границу становится видимым только после «Обновить». Замена inode, обнаруженное усечение, изменение header или настройки инвалидируют cursor/window. Окно — датированное наблюдение, не атомарный снимок активно меняемого Codex; проверки не обещают защиту от конкурентного переписывания произвольного прежнего участка другим процессом. Перед bind снова проверяются актуальные Core gates/настройки и принадлежность текущему window.

### Поддержанный формат и наследование

Требуется точное session_meta.cli_version == `0.163.0-alpha.10.2`; отсутствие, неверный тип или несовпадение дают unsupported. Поддерживается только эта версия, `history_mode=paginated`, schema подтверждённого commit4ebc485ca321243727539187169425fcf972f2c4. Проверять session_meta.id, не подставлять session_id, filename или предполагаемый UUID. Первая запись должна быть законченной session_meta; иной engine/legacy/malformed header дают явный unsupported, не пустую историю.

14A не открывает history_base и не поддерживает forked_from_id либо forked_from_ordinal_exclusive: любое non-null из этих трёх полей (включая ordinal0) даёт `unsupported_inherited_history`, включая отдельный fork ordinal без остальных полей. Отдельно поддерживается известный subagent_history_start_ordinal без этих трёх полей: каждый record обязан иметь embedded ordinal, записи ниже границы не проецируются. До тестов этого случая нельзя переносить на него прежний PASS02. Отсутствие/null boundary означает отсутствие границы; неверный тип любого inheritance-поля — malformed/unsupported, а не отсутствие. history_mode должен явно быть строкой paginated, без default. При отсутствии boundary действует та же обязательность ordinal. Внутри окна duplicate/regressed ordinal и gaps отмечаются как неполнота; неоднозначные записи не становятся source evidence.

Canonical источник — event_msg.item_completed с точными thread_id == header.id, turn_id и item.id. Поддержаны UserMessage, AgentMessage, Plan, FunctionCallOutput только namespace=codex_app/name=create_thread либоsend_message_to_thread. Повторные snapshots одного item внутри окна обновляют одну запись по ordinal. Конфликт типа/identity не создаёт два сообщения. Raw response_item не используется как второй prompt; без контекста он не имеет надёжной turn identity.

UserMessage маркируется как канонический ввод Codex с неподтверждённым человеческим происхождением: heartbeat тоже способен его создать. Делегированный prompt обозначается отдельно. Никакой из этих видов не становится UserConfirmation. Plan/обычный текст плана не записываются автоматически в Core plan, AgentMessage final phase не завершает ход.

Из content сохраняются только проверенные текстовые части в исходном порядке без нормализации. Если присутствовали нетекстовые части, это явно text-only projection, не полный prompt. Делегированный output принимается только в подтверждённой строковой форме; иная форма даёт unsupported item. Пробник02, который извлекал raw delegated input, не копируется как production-реализация.

Terminal task_complete/alias turn_complete принимается с явным turn_id после header/ordinal/inheritance checks. error=None → observed completed, иначе observed failed; raw error/last_agent_message не становятся отдельным каноническим текстом. turn_aborted требует явный turn_id, иначе сохраняется только причина неполноты без назначения ходу. task_started может дать отдельный observed start. Предыдущий start для самостоятельного canonical/terminal не обязателен; отсутствие start/terminal в окне и покрытие сохраняются, «сейчас работает» не выводится.

compacted отмечается как служебный факт, replacement_history не становится вводом. Известные непроецируемые world_state/turn_context/token/прочие служебные записи не меняют сообщения. Неизвестный record/event, неверныйUTF8/JSON, oversized record или отсутствующий ordinal показывают неполноту; нельзя тихо принять будущую схему как старую. Самостоятельные проверенные элементы остальных строк могут остаться доступны, если header/identity окна действительны.

### Source identity, сохранение и отзыв

SourceKey namespace `codex-local-jsonl-v1`, scope — проверенный thread ID. Object ID для item строится из length-prefixed turn_id + item.id и SHA256 с префиксом `item:`; terminal и start имеют отдельные префиксы и hash turn_id. Путь, byte offset и время чтения не заменяют identity. Ограниченные исходные IDs, kind и source time входят в каноническое содержимое события. Версия — hash этого содержимого, включая ограничения самой text-only projection, а не mtime. Coverage окна, время чтения и файловое расположение сохраняются отдельно и не входят в event hash: одно событие не меняет версию при другом окне. Ordinal сохраняется отдельно для порядка updates.

Результат выбора — existing SourceEvidence <=16KiB вместе с метаданными. Большой prompt целиком не сохраняется: видимый отказ, без автоматической выдержки/усечения/summary. Этот пакет не добавляет редактор excerpt. Охват остаётся local_records_only/window, даже если выбранная текстовая часть точна. Raw JSONL/window не сохраняются в SQLite и не попадают в обычные логи.

Явный bind сохраняет на конкретной связи task/evidence источник `selection_id + settings_revision + accepted_ordinal`. Такая связь отдельна от SourceKey и не выводится из уникальности thread ID. То же событие из другой настройки даёт conflict, не перепривязывает старое evidence. Пользователь может изменить файл у той же настройки (новая revision), прочитать его заново и явно bind проверенный элемент: только это обновляет provenance revision. Изменение пути само по себе не авторизует старую историю.

Для одного SourceKey порядок accepted_ordinal/event_hash сохраняется между settings revisions. Новая revision меняет разрешённое происхождение, но не обнуляет порядок: более старый ordinal отклоняется; тот же ordinal с другим event hash — conflict; тот же ordinal с тем же hash допускает явное обновление provenance. Проверенное более новое событие может обновить existing source evidence по прежним правилам ingest. Пользовательски отредактированное evidence и forgotten/tombstone rules сохраняются. Повторный bind в удалённую пару task/source не восстанавливает её. Явная новая revision настройки не является обходом tombstone. Изменение provenance повышает task revision и инвалидирует подготовленную помощь даже при равном тексте.

Bind/ingest, association, accepted_ordinal/event_hash и повышение task revision выполняются одной существующей транзакцией Store. Ошибка, conflict, лимит или tombstone не оставляют частичных изменений; protected-user правила не обходятся. Отдельный координатор транзакций не нужен.

#### Конкретные метаданные Codex

Generic SourceEvidence и memory ABI3 не получают полей Codex. Одна Codex-specific metadata запись принадлежит существующему live source binding (его ID связывает task/evidence); новый Codex ABI возвращает её вместе с evidence ID. Поля: selection_id/settings_revision/accepted_ordinal; typed event identity — turn_id и поддержанный event kind, для item дополнительно item_id и item kind, для delegated output один из двух поддержанных вариантов вызова. Thread ID уже хранится в SourceKey.scope и не дублируется. Каждый turn_id/item_id — непустой UTF-8 до2048байт, thread scope сохраняет существующий предел256байт. Никаких произвольных JSON metadata blobs.

Существующие поля переиспользуются: kind=SourceFact, observed_at_utc — доступное время источника, SourceBinding.version — event hash. Происхождение выводится из typed kind: canonical UserMessage всегда human_unverified, delegated input отдельно, остальные agent/structured observation; ни одно не UserConfirmation. Completeness отражает text-only/неполноту выбранного record. Для всей сохранённой Codex-привязки охват всегда local_records_only/local_window, никогда полная история: это неизменное правило данного адаптера, которое Core добавляет в assistance context и новый ABI. Подробные byte boundaries/причины неполноты текущего окна остаются в bounded window receipt и не переживают его. Так после перезапуска не требуется восстанавливать окно для честной маркировки сохранённого источника.

Start/terminal с проверенным turn ID можно привязать как SourceFact. Core формирует детерминированное фактическое описание наблюдённого структурированного события, явно помеченное как проекция приложения, не цитата Codex. Например, наблюдённое завершение хода без ошибки; это не утверждение выполнения пользовательской задачи. Локализованный UI получает typed event kind/ID, а не пытается разобрать sentence. Неполное событие без обязательной identity не создаёт такой текст. Raw error/last_agent_message не подставляются. Пустой обычный text item не получает выдуманного текста и не привязывается как сообщение.

Совокупные UTF-8 bytes evidence text, source key/version и переменных Codex metadata не превышают16KiB на привязку; metadata учитываются и в существующем64MiB retained-content лимите. Fixed-width поля ограничены существующим row cap. Новое чтение сохранённых metadata использует существующие50записей/256KiB page bounds, включая все возвращаемые поля. SourceEvidence не получает отдельный дублирующий event hash/time. Generic edit сохраняет provenance и user-edited protection; автоматический ingest его не перезаписывает. Удаление evidence/task очищает текст и Codex metadata в той же транзакции, сохраняя прежний content-free digest tombstone. Отзыв/изменение настройки сохраняет исторические metadata, но запрещает использование; явная перепривязка следует тем же ordinal/hash/conflict/tombstone правилам.

Удаление/замена настройки блокирует использование связанного evidence, сохраняя исторический текст. Prepare/dispatch/result/copy и follow-up08A проверяют конкретную сохранённую association и текущую revision, включая основания, которые модель не процитировала. Снятая настройка не восстанавливается через копию файла или поздний read result. Новый Store или импорт всего архива не вводятся.

### UI и допуск

Нативная секция выбранных Codex-файлов, текущая страница и явные выбор/bind. Настройки источника и выбор дела не меняют самостоятельный reply draft08A/10A. Окно ограничено прокруткой, причины неполноты видны рядом с ним. Нет auto poll, перехвата фокуса, записи в Codex или автоматического запуска волн.

Source bytes читаются только при общем допуске помощи. Native settings/metadata preflight не подменяет этот gate. Ошибки permission/version/limit/cancel видимы; cached content после invalidation очищается. EN/RU обновляются вместе. Упоминания scope/ordinal/ABI не нужны в повседневном UI; пользователю достаточно происхождения, времени и ограничений полноты.

## Testing Decisions

Scout→Librarian и Type Scaffolder read-only выполнены для планирования; implementer обновляет их по принятому10A до зависимых правок. Важный код/типы пишетSol; Localizer EN/RU; Code Simplicity Review готовойлогики. Никаких genericworkers для формального разделения общего контракта.

Сквозной путь: настоящий временный JSONL → native bounded FD reader → Core → временная SQLite → local HTTP/SSE assistance → result/copy. Только тестовый HTTP; никакого live/model/Jev и личной истории в этомпакете. Проверить immutable source hashes и отсутствие записи/запускаCodex, полную schema6 read-only rejection без sidecars/reset, сохранность Git/voice/reply.

SQLite admission проверяется с текущей и старой схемой только в WAL, а также с живым отдельным процессом прежней версии. Используется WAL-aware readonly_shm preflight закреплённой bundled SQLite; отсутствие пригодного SHM может дать Storage без записи, но не ложный UnsupportedSchema или пустую память. Гарантия соответствует нынешнему app-owned Store path (private connection, DELETE journal), не произвольному embedding рядом с raw writable WAL connection в том же процессе. Доказательства и известное ограничение shared unixShmNode: [проверка SQLite](../features/assistant-behavior/evidence/14-sqlite-admission-review.md).

Обязательные случаи: большойфайл со свежими полезными records; границыUTF8/newline/partialtail; oversized line/header, многослужебныхстрок, item/byte limits и продвижениеcursor, включая окно без нормализованных элементов и остановку по каждому пределу; append/refresh/усечение/replacement/header change; canonicalduplicate/update и rawдубликат; thread/turn mismatch; ordinalmissing/regressed/gap; subagentboundary; unsupported fork/history_base/отдельный forked_from_ordinal_exclusive; cli_version missing/type/mismatch; human provenance иnontextcontent; terminalбезstart/безturnID/error/aborted; compaction неuserinput.

Bind/permission: metadata round-trip после reopen, truthful terminal/start projection и empty-text denial, metadata accounting/delete/tombstone; exacttext+metadata <=16KiB, no silenttruncate; duplicatefile conflict; explicit same-setting rebind; oldordinaldeny между revisions; одинаковый ordinal/hash из разных окон; атомарный отказ без частичных bind/association/revision изменений; useredited/forgotten rules; pause/delete/settingschange во времяI/O; FDclose доrelease; sourceотзыв послеpreview/доdispatch/послерезультата; uncitedsource и follow-up не обходятотзыв. Проверить потерюконтекста отдельно отсохранности самостоятельного пользовательского черновика.

Full yarn verify, feature Swift suite, all-features Clippy, app build/strictsignatures, ABI/hookaudit всехMachO и hashes всех прежних checkpoints. Логи содержат только закрытые причины/размеры/IDs тестовых данных. Фактические UI/фокус, личныеrollouts/совместимостьдругихверсий, macOS14hardware/Windows/live Fast и полнаяG6 остаются открытыми.

## Reference Points

- [Issue14](../features/assistant-behavior/issues/14-codex-source.md), [evidence02](../features/assistant-behavior/evidence/02-codex/report.md), [исследование14](../features/assistant-behavior/evidence/14-codex-integration-research.md).
- `src/core/src/memory_types.rs`, `memory_store.rs`: SourceKey/SourceEvidence, bind/ingest и tombstones.
- `src/core/src/local_source_store.rs`, `assistance_store.rs`: текущий Git-only admission/evidence_root и snapshot fences, которые нужно сохранить.
- `src/bridge/include/ambient_source.h`: Git ABI1; Codex имеет отдельную узкую границу, тот же Store handle.
- Official pinned RolloutLine/materialization/projection/item producers перечислены в исследовании14; root сверил embedded ordinal и фильтр subagent prefix.

## Out of Scope

Автоматическое обнаружение sessions, app-server/SQLiteCodex, запись/запуск/продолжениеагентов, cloud/fork/base history resolution, весьархив/полнаяmodelhistory, новыеCodexверсии, автоматическоеподтверждениеличныхнамерений, сценарийволнкакспециальныйдвижок, общаяconnectorплатформа, полные07/09/14. Пользовательские продуктовые требования сохраняются; это первый проверяемый путь Codex→память→помощь.
