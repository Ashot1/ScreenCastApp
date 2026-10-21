# ADR-017: выбранная задача Jira Cloud

**Поправка ADR-020 (23A принят 20 октября 2026).** Jira работает на общем движке интеграций: схема 11 и ABI интеграций v1 заменяют schema10 и Jira ABI1; секрет хранится в Keychain service `dev.ambient.foundation.integrations`, привязанный к экземпляру базы. Выравнивания: второй ответ сервера без срока паузы сохраняет уже записанный срок и его `server_deadline_known`; повторная проверка паузы на каждом шаге удалена (она читала снимок профиля и не действовала); первый код ошибки начала чтения следует каноническому порядку ADR-020. Предметная семантика ниже не меняется.

Статус:12A реализован и принят локально20 октября2026 после Finished Plan Review/Evidence Audit, реализации, code review и проверки управляющим. Verify8/224/75, feature Swift119 (real ASR пропущен), build/signatures PASS; настоящая совместимость/права/Cloudv3 subtask relations и полная12/G6 остаются открытыми. GitHub13A и08B приняты, зависимостей от активного владельца нет.

## Problem Statement

Сохранённое обсуждение реализации пока нельзя связать с проверяемыми сведениями Jira. Нужен собственный путь выбранная задача → доступные сведения и история → память дела → существующая помощь, сохраняющий происхождение и неполноту данных.

## Solution

Пользователь явно подключает сайт Jira Cloud через собственные email/scoped API token и Cloud ID, выбирает key или ID задачи, читает ограниченное окно и сохраняет выбранные сведения одной операцией. Помощь08B использует их вместе с остальной памятью. Нет специальных сценариев Jira и автоматического изменения задачи.

Это Cloud-срез, не предположение о типе личной Jira пользователя. Фактическую установку и права проверяют при настоящем подключении. Data Center, несохранённый редактор, автоматическое обнаружение и полная12/G6 остаются открытыми.

## User Stories

1. Подключаю конкретный сайт своим ключом и вижу область разрешённого чтения.
2. Выбираю задачу без передачи приложению всей истории аккаунта.
3. Вижу сохранённое описание, новые доступные комментарии и ограниченное окно истории.
4. Вижу наблюдённые связи с другими задачами без автоматического обхода этих задач.
5. Различаю отсутствие данных, пустой текст, потерю форматирования и недостаток прав.
6. Выбираю1–32 объекта, проверяю точный набор и сохраняю его в дело.
7. Использую сведения после ухода со страницы в обычном обсуждении08B.
8. Исключаю сайт; чтение и использование прежних сведений прекращаются.
9. Заменяю/удаляю ключ без восстановления старого доступа после ошибки или restart.
10. Ошибка API или изменение задачи не превращается в ложную актуальную полную историю.

## Implementation Decisions

### Владение и область доступа

Один прежний Core Store владеет профилями, исключениями сайта, допуском, окном чтения, проекцией, памятью и source fences. Native владеет UI и отдельным Keychain envelope. Providers исполняет только разрешённые GET. Нет общего connector framework, второго Store или отдельного scheduler.

После принятой schema9 — schema10 и отдельный Jira ABI1 на существующем memory handle. Memory3/auth1/model4/source1/Codex1/GitHub1 не меняют согласованную семантику. Старые схемы отклоняются принятым admission-механизмом без миграции/reset/изменения файлов.

Канонический site scope — точный Cloud ID в пути фиксированного gateway. До8 профилей; одновременно только один профиль на Cloud ID. Cloud ID неизменяем в существующем профиле: смена сайта создаёт новый профиль. Имя до256 UTF-8 bytes — подпись пользователя, не подтверждение hostname/аккаунта. Cloud ID — непустой ASCII segment до128bytes, только буквы/цифры/дефис; это поддерживаемое ограничение ввода, не заявление о полном формате всех Atlassian IDs. Регистр сохраняется, никакого угадывания aliases.

Есть явное сохранённое действие «Исключить сайт» по Cloud ID, отдельное от удаления профиля/памяти. До128 исключённых ID, совокупные настройки до1MiB. Исключение переживает удаление профиля и замену ключа; новый профиль не обходит его. Повторное разрешение — отдельное явное действие. Исключение закрывает begin/каждый GET/принятие результата/preview/bind/все08B-действия и атомарно очищает зависимую помощь, включая непрoцитированные основания. Исторические SourceFacts остаются до удаления памяти. Снятие исключения не восстанавливает старую помощь автоматически; новый read/bind требуется для актуальных оснований.

Изменение excluded↔allowed для существующего профиля в той же транзакции повышает его settings revision из прежнего source clock и инвалидирует текущие job/window/preview. Поэтому поздний ответ, начатый до цикла exclude→allow, не становится допустимым снова. Credential generation от одного переключения сайта не меняется; сохранённый секрет не требует повторного ввода. При отсутствии профиля exclusion хранится отдельно, а созданный позже профиль получает новый непереиспользуемый ID/revision.

Project — наблюдённое поле, не граница доступа. Project allowlist не вводится: предварительный GET не защищает от перемещения между запросами. Весь доступ остаётся внутри явно разрешённого site и выбранной задачи. Parent/subtask/link — данные ответа выбранной задачи, не разрешение следующего GET.

Ввод browser URL/hostname-правил и разрешение aliases в12A отсутствуют. `api.atlassian.com` не показывается как идентичность сайта. serverInfo может дать текущий адрес, но не доказывает все aliases/атомарность rename, поэтому не нужен в этом пути. Межприложенческие D40-исключения остаются открытыми; будущая интеграция hostname-правил обязана блокировать доступ при неизвестном соответствии Cloud ID. Нельзя объявлять эту часть универсальной защитой наблюдения за браузером.

### Авторизация

Личный scoped API token + email, HTTP Basic, фиксированный gateway `https://api.atlassian.com/ex/jira/{cloudId}/rest/api/3`. Документированный classic scope для выбранных трёх GET — `read:jira-work`. UI объясняет, что scope шире одной задачи и не отменяет разрешения Jira; реальная доступность scope и аккаунта не доказана локальными тестами. Не собирать OAuth client secret, не использовать сессию коннектора Codex, MCP/refresh/автоматическое получение Cloud ID не добавлять.

Native envelope: собственная версия, profile ID, credential generation, email и token; отдельный несинхронизируемый Keychain service. Email непустой ASCII до254bytes без whitespace/control/colon; token непустой ASCII до16KiB без whitespace/control. Не проверять выдуманный token prefix. Секреты/адрес email не хранятся в Core JSON/SQLite/логах/модельном контексте и не возвращаются в UI. Basic строится штатным sensitive-header API.

Монотонные profile ID/settings revision/credential generation берутся из прежнего Core source clock, не переиспользуются. Замена сначала повышает Core generation и отзывает производные, затем сериализованный native owner записывает envelope. Несовпадение после crash/ошибки закрывает доступ без возврата старого ключа. Удаление: Core revoke → подтверждённый Keychain delete → Core delete с expected revision. Ошибка очистки видна и допускает повтор; поздняя операция не удаляет новую запись. Scope exclusion не зависит от наличия Keychain envelope.

### Ограниченное чтение

Общий допуск сохраняет текущие session/report/High Fast, отсутствие паузы, выбранное незавершённое дело. Настройки не выполняют запросов. Только явные Read/Refresh. Один job и одно ephemeral окно на Store; без очереди/polling. Fences: TaskVersion, profile ID/settings revision/credential generation, Cloud ID, выбранная identity, process epoch. Отмена освобождает слот только после фактического завершения transport.

Выбор key/ID: непустая строка до256 UTF-8bytes без control/NUL, кодируется как один URL segment. Сначала GET issue закрепляет возвращённый ID; следующие запросы только по нему. ID/key из ответа не обязаны совпадать с введённым старым key. Финальный GET обязан вернуть тот же ID. Данные первого/последнего ответа не склеиваются: issue и embedded relations берутся из конечного ответа.

Разрешены только GET:

- `issue/{issueIdOrKey}` с `fields=summary,description,status,issuetype,project,created,updated,parent,subtasks,issuelinks`, `updateHistory=false`, `failFast=false`; без expand/properties.
- `issue/{id}/comment` с `startAt`, `maxResults=50`, `orderBy=-created`.
- `issue/{id}/changelog` с `startAt`, `maxResults`.

failFast=false сохраняет доступные поля при ошибке загрузки другого поля. Missing не позволяет отличить ошибку загрузки от иной недоступности: сохраняется partial/missing-field, без догадки о причине. Нужные для identity anchor id/key/fields всё равно обязательны; без них окно непригодно. Полнота каждой выбранной группы проверяется независимо.

Никаких redirect/retry/cookies/proxy по окружению, произвольных base URL, `self`/`nextPage` переходов, загрузки media/вложений, issueLink endpoint или связанных задач. URL собирается из валидированных частей; query через библиотечную сериализацию.

Весь job:30секунд, до13 GET, body до1MiB на ответ/8MiB всего, окно до512 объектов/512KiB. Принимаемые headers до32KiB после HTTP parsing; ранние пределы allocation остаются пределами установленного Hyper, а не этой проверкой. Chunk bytes считаются фактически. До1MiB и одного GET резервируются для конечной проверки issue; deadline не продлевается. Если final не выполнен, bind недоступен.

Комментарии: до5 страниц по50, начиная с0. История: начальный GET с maxResults=1/startAt=0 даёт наблюдённый total; затем до5 страниц по50 с startAt=max(total−250,0). При total0 отдельных tail-запросов нет; при total1 первый элемент можно переиспользовать. Это окно относительно наблюдённого total, не доказательство последних250 событий. Изменившийся total и нестабильный порядок отмечаются. Общий предел13 учитывает initial/final issue,5comments,1history probe,5history tail.

Требовать корректные массивы, совпадение ответного startAt с запросом, неотрицательные integers для paging и положительный maxResults не выше запрошенного. Comments startAt ограничен i64, changelog startAt — i32; maxResults обеих категорий — положительный i32. Total принимается в0..i64::MAX; tail offset вне i32 не отправляется, история явно partial/offset-limit. Продвижение — startAt + число реально возвращённых объектов с checked arithmetic и проверкой диапазона соответствующей операции. Пустая/аномальная/повторная страница прекращает категорию с явной partial-причиной. nextPage не отправляется. Конец наблюдённой выдачи не равен всей истории Jira; невидимые записи не определяются. История с tail offset>0 всегда имеет omitted-prefix. Достижение лимита не обрезает объект и не объявляет категорию полной.

401 отзывает соответствующую credential generation даже при устаревшем TaskVersion; новая generation не затрагивается. Initial/final issue403/404 не дают пригодного окна. Category403/404 означает недоступную часть, не отсутствие. 429 всегда останавливает job, в том числе без Retry-After;503 с Retry-After также останавливает дополнительные запросы. Final не обходит cooldown. HTTP-date/delta-seconds обрабатываются с checked arithmetic, известный срок сохраняется на профиль; при неизвестном сроке показывается явная временная недоступность без автоматического retry. Последующее чтение возможно только новым действием после применимого cooldown. Сырые error body не показываются и не сохраняются.

### Данные и ADF

Wire JSON проверяется на входе. Opaque IDs issue/comment/changelog/link/project — строки до256bytes без control/NUL, не числа; accountId до128bytes. Missing/null/wrong type различаются. Нужные для bind ID не синтезируются. Автор может отсутствовать/быть недоступен; displayName до512bytes не identity, `unknown` не общий автор. Даты optional: отсутствующая остаётся неизвестной, неверная отмечается потерей. Принимать RFC3339 и Jira ISO дату с offset±HHmm, дробная часть0–9цифр; проверять календарь/offset/диапазон, нормализовать UTC, сохранять precision loss явно. Не трактовать issue.updated как транзакционную ревизию.

Исходные значения body: missing, null, валидный пустой ADF, ADF с текстом, malformed/unsupported. Null не считается отказом доступа. ADF doc требует type=doc/version=1/content array. serde_json обычный recursion limit не отключается; дополнительно обход до64уровней/10000узлов/marks и текст до16KiB. Выход за предел означает непригодность соответствующего объекта, не тихое усечение. Незнакомый узел может дать ограниченный видимый дочерний текст с loss flag, но не выдуманное содержимое.

Поддерживаемая проекция: text, paragraph/hardBreak/heading, bullet/ordered list/listItem, blockquote, codeBlock, table/row/cell/header с явными разделителями. Текст сохраняется; plain-text-потери marks/форматирования явно отражаются. Mention использует только присутствующий attrs.text; без него — отметка отсутствующей подписи, не имя из ID. Link mark не загружает URL; текст label сохраняется, утрата адреса отмечается. Media/card/unknown leaf не превращаются в содержимое; отмечается пропуск. Source text и инструкции внутри — недоверенные данные.

Типы сохраняемых объектов: issue, comment, changelog group, relation. Project не становится самостоятельным объектом памяти. Issue содержит summary/описание/status/type/project/даты как наблюдения. Comment — проекция body, доступный author/created/updated. Changelog group — bounded список items с wire fields field/fieldId/fieldtype/from/to/fromString/toString; fieldtype нормализуется как field_type, wire key type не является его alias. Missing/null сохраняются раздельно. Не угадывать новую семантику custom fields.

Контракт чтения выбранных полей ниже — явный ограниченный reader, а не утверждение, что OpenAPI определяет произвольный fields этой схемой. Unknown поля не интерпретируются; исходный JSON не сохраняется. Presence имеет варианты missing/null/value/invalid; malformed необязательное поле даёт loss, а не подставное пустое значение. Root issue должен быть object с пригодными string id/key и object fields; без этого initial/final anchor непригоден.

| Поле | Принимаемая форма и результат |
| --- | --- |
| summary | String до4096bytes. Missing/null/empty/invalid различаются; issue object bindable только с непустым summary. Остальные категории могут остаться пригодными |
| description | ADF object либо missing/null; правила проекции выше. Missing/null/invalid отражаются в issue losses; пригодный summary не заменяется вымышленным описанием |
| status, issuetype, project | Object с optional string id до256bytes и name до512bytes; project также optional key до256bytes. Все nested presence проверяются отдельно. Name без ID остаётся только отображаемым наблюдением, не идентичностью. Unknown enum/status names сохраняются как данные |
| created, updated | Optional string до128bytes с правилами даты выше. Null/missing остаются разными неизвестными значениями; неверная форма даёт loss. Необязательность — политика reader |
| parent | Object с пригодным string id; key optional string. Optional fields object может содержать summary/status по тем же правилам. Без ID relation не bindable; валидная часть остальных issue fields сохраняется |
| subtasks | Request field ID и принимаемый response key — `subtasks`. Array embedded issue objects с теми же id/key/fields правилами, что parent; это ограниченный reader-контракт, а не найденная строгая v3-schema. Missing/null не равны пустому массиву; `sub-tasks` не считается alias и помечает непроверенную форму/partial вместо пустой истории |
| issuelinks | Array объектов с id/type и одной направленной embedded issue стороной по следующему абзацу; type object принимает id/name/inward/outward как independently checked strings. Relation требует непустой направленный label до512bytes и target ID; ошибочный элемент даёт отдельную потерю |

Массивы relations и changelog items ограничены100 элементами на один объект: превышение делает соответствующий агрегат непригодным, не усечённым. Changelog items принимаются только как object; field/fieldId/fieldtype — optional strings до256bytes, from/to/fromString/toString — optional strings до4096bytes; явный null сохраняется как null (это допускаемое reader-расширение), wrong type делает группу непригодной. Непустая пригодная группа сохраняется целиком, без отбрасывания отдельных изменений. Общие16KiB/512KiB bounds применяются также после нормализации.

Имя системного поля subtasks подтверждено официальными Atlassian источниками; публичные request/empty-response examples относятся к RESTv2. Полная непустая форма именно Cloudv3 не установлена, а закреплённый Get issue example использует sub-tasks. Это зафиксированный пробел документации, не основание молча смешивать aliases или объявлять Jira несовместимой целиком. Unknown shape/отсутствие пригодных child IDs отражаются отдельным partial reason; остальные пригодные сведения выбранной задачи могут сохраняться. Локальные fixture-тесты доказывают только описанную границу reader; реальная проверка parent/subtask relations обязательна для полной12/G6.

Relation — embedded issue link либо parent/subtask. Для link требуются ID, type и ровно один inward/outward endpoint относительно текущей задачи; официальные embedded examples имеют только одну сторону, несмотря на общий required обеих в IssueLink schema. Направление и label берутся из соответствующей стороны type, не переводятся в план Ambient. Parent/subtask используют наблюдённый ID связанной задачи; направление сохраняется как структурная связь. Никакого follow-up по self. Неизвестные/невалидные relation отмечаются как потери и не связываются. Status/Done в Jira не завершают дело Ambient.

### Память, preview и помощь

Source namespace `jira-cloud-rest-v1`, scope=Cloud ID. Object ID — однозначное кодирование tuple(identity tag,parent issue ID,object ID). Tags: issue, comment, changelog, issue_link, parent, subtask; для parent/subtask последний компонент — target ID. Разные пространства relation IDs не объединяются общим tag relation: issueLink ID, равный parent target ID, остаётся отдельным фактом. Не склеивать разделителем, допустимым внутри opaque IDs. Лимиты SourceKey64/256/2048bytes соблюдаются. Исторические ключи не зависят от profile name, текущего key или task.

Нормализованная content version — hash с version tag и однозначным canonical encoding: identity tag, устойчивые IDs, нормализованный текст/содержательные поля (включая наблюдённый issue key и даты источника), presence/null и проекционные потери. Profile/settings/credential generation, локальный interval и paging/coverage исключены: Refresh и замена ключа сами по себе не меняют содержание. Hash пересчитывается при open-time validation. Typed saved metadata хранит profile/revisions/generation, parent ID, наблюдённый key, interval, category coverage, body/projection loss, доступные даты/автора/поля. Интервал не означает атомарный snapshot. Отдельная Jira metadata включена в прежние16KiB на запись/64MiB Store/100000rows, включая open-time validation.

Дубликаты одного ID с различным содержимым неоднозначны и не bindable; одинаковые дедуплицируются. Объект без полезного текста не превращается в фиктивный SourceFact; его отсутствие видно в coverage. До1–32 уникальных bindable объектов текущего окна; preview показывает весь точный набор и потери. Bind одной транзакцией повторяет допуск/fences/preview identity, использует прежние write_source/byte_room и возвращает новый TaskVersion/EvidenceIds. Защита ручных правок/забывания/другого binding сохраняется; при конфликте не сохраняется половина набора. Preview не меняет память.

Source authorization и content freshness различаются. Обычное обновление делает прежнюю помощь stale, сохраняя разрешённые пользовательские drafts по08B. Исключение site, credential revoke/replace, удаление profile закрывают старые authorization fences и в той же транзакции очищают все зависимые08B области, даже без цитат. После restart эти правила остаются. Site exclusion не обходится через другой profileID или одинаковый SourceKey. Нет автоматического переноса старых исторических фактов под новую generation.

Native UI использует существующую панель источников/памяти и08B, локализацию EN/RU. Выбор сайта/задачи, read/refresh/cancel, coverage, exactpreview/save, excluded state и ошибки очистки видимы. Пользователь сам запускает помощь существующей кнопкой; импорт не вызывает модель автоматически.

## Testing Decisions

Проверяется настоящий локальный путь SQLite → C → Swift и provider HTTP через isolated loopback feature; fixture response не становится production source. Native isolated Keychain tests проверяют сериализацию и Core-first revocation, не личную связку. Один связанный implementer владеет Core/provider/bridge/native контрактом.

Обязательные сценарии: key→ID move внутри site; другой finalID; исключение перед чтением/между страницами/передbind/перед08B и после restart; поздний ответ после exclude→allow не принимается; удаление/создание профиля не обходит exclusion; 401старойgeneration; ошибка Keychain; partial403/404/429/503; меняющийся total/пустая/duplicate page; latestcomments и omittedhistoryprefix; ADF неизвестные узлы/marks/таблицы/пределы; null/missing/emptybody; opaque IDs и+0000 даты; embedded one-sided links; одинаковые IDs разных relation kinds не сталкиваются; одинаковое содержание при новом interval/coverage/generation сохраняет content hash; exactbatch/conflict/tombstone; revoke всех grounds, ordinaryrefreshstale.

Нужны TDD для поведения, обязательные named specialists (Type Scaffolder read-only, важные типы пишет Sol), code simplicity review, стандартный fullverify, релевантные featureSwift/HTTP/ABI/Keychain проверки, production build/Mach-O audit/strictdeep signatures и текущие source hashes. Не повторять тяжёлый ASR без затронутой границы. Checkpoint создаёт root после приёмки; baseline14apps/186files должен сохраниться.

Никаких настоящих аккаунтов/credentials/UI/TCC/mic/ServiceManagement/model/Jev вызовов. Это не G6, не настоящее разрешение Jira, не проверка Fast/UI/Windows. Ledger7POST/1021known/3unknown остаётся.

## Reference Points

Планирование после принятого13A, без Git repository/commit. Implementer перепроверяет опоры:

- [Общая spec/D40](../features/assistant-behavior/spec.md), [решения D40/D56](../features/assistant-behavior/decisions.md), [полная12](../features/assistant-behavior/issues/12-jira-source.md).
- [Исследование12](../features/assistant-behavior/evidence/12-jira-integration-research.md), [OpenAPI snapshot](../../target/verification/orchestration/jira-openapi-2026-10-20.json), SHA25673a2afdb0230726ff11fdbdee8e250beeb1ec73c090568deeb53c63b792dee4e.
- SourceKey/SourceEvidence/write_source/byte_room: `src/core/src/memory_types.rs`, `memory_store.rs`; source fences: `assistance_types.rs`, `assistance_store.rs`, `proposal_store.rs`.
- GitHub ownership example: `src/core/src/github_types.rs`, `github_store.rs`, `src/providers/src/github.rs`; native serialized credential owner `src/platforms/macos/Sources/AmbientMacOS/GitHubCredentials.swift`; narrow ABI `src/bridge/include/ambient_github.h`.
- [13A acceptance](../../target/verification/orchestration/13a-parent-inspection.md), [checkpoint baseline](../../target/verification/orchestration/post13a-checkpoints-baseline.json).

## Out of Scope

Data Center, OAuth/MCP/backend, глобальные hostname aliases, project exclusions, browser capture/unsaved input, JQL/search/polling/webhooks, attachments/media, related-issue traversal, любые Jira writes, автоматическое создание зависимостей/завершение дел, G6/liveFast/Windows. Эти границы не отменяют полную задачу12.
