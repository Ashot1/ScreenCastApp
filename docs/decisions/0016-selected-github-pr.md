# ADR-016: выбранный GitHub PR и обсуждение

**Поправка ADR-020 (23B принят 20 октября 2026).** GitHub работает на общем движке интеграций: схема 12 и ABI интеграций v1 (вид 3) заменяют schema9/GitHub1; секрет хранится в Keychain service `dev.ambient.foundation.integrations`, привязанный к экземпляру базы. Выравнивания: канонический порядок допуска (профиль, allowlist до версии и паузы), проверка часов по концу интервала, `server_deadline_known` следует победившему сроку, сохранение профиля отменяет только чтение этого профиля, запись 401 и паузы игнорирует удалённый профиль, но не скрывает другие ошибки хранилища. Предметная семантика ниже не меняется.

Приёмка реализации20 октября2026:13A принят локально, schema9/GitHub1. Verify8/186/72, feature111 и последующие GitHub feature6/Core27, review/Clippy/build/signatures PASS. Контрольная сборка сохранена. [Проверка управляющего](../../target/verification/orchestration/13a-parent-inspection.md). Реальные аккаунт/UI/Fast/Windows и full13/G6 отдельно. Ниже сохранён согласованный контракт.

Статус: спецификация13A прошла итоговые Plan Review и Evidence Audit20 октября2026. 08B принят: schema8/model4 и окончательный source/retention контракт сверены;14A принят ранее. Зависимости для локального запуска выполнены. Исследование API и независимого варианта: [GitHub research](../features/assistant-behavior/evidence/13-github-integration-research.md). Это самостоятельный ручной путь полной13 по D56, не завершённый проактивный коннектор.

## Problem Statement

Обсуждение PR пока нельзя сохранить как проверяемые внешние сведения дела. После ухода со страницы помощнику недоступны выбранные замечания, их авторство и ограничения актуальности. Требуется явное подключение выбранного репозитория и PR без автоматического чтения всего аккаунта или записи в GitHub.

## Solution

Пользователь подключает собственный fine-grained PAT к явному профилю GitHub.com, разрешает конкретные репозитории и выбирает PR. Приложение читает описание, reviews, inline comments и обычные комментарии. Пользователь выбирает до32 объектов текущего окна, сохраняет их одной операцией в дело и получает помощь прежним путём 08B. После ухода со страницы факты остаются связаны с делом. Сохранённое обсуждение не выдаётся за актуальную полную историю PR.

Это объектный источник, а не обработчик сценария «исправить PR». Обсуждение своего или чужого PR не становится поручением изменить код, статус PR не завершает дело. Один источник может использоваться вместе с уже сохранённой целью или инструкциями в обычном модельном контексте.

Полная задача13 остаётся открытой: Checks/commit statuses, resolved/outdated через GraphQL, автоматическое обнаружение и настоящая G6-проверка — отдельные части. Для Checks документация PAT противоречива; 13A не получает ложной готовности из успешного чтения комментариев. Не добавляются polling, webhooks, произвольные поиски, автоматический bind или запись в GitHub.

## User Stories

1. Я подключаю собственный доступ только к разрешённым репозиториям GitHub.com.
2. Выбираю PR и вижу описание и доступные обсуждения с указанием недоступных частей.
3. Выбираю несколько замечаний, проверяю весь набор и сохраняю его в дело одним действием.
4. После ухода со страницы использую сохранённые замечания вместе с моей целью через существующую помощь.
5. Чужое ревью не становится поручением исправить чужой код, а approval не завершает мою задачу.
6. Обновление не выдаёт старое или неполное обсуждение за проверенное текущее состояние.
7. Отзываю источник или заменяю ключ; старые предложения не обходят отзыв после перезапуска.
8. При ошибке доступа или квоте вижу причину и не получаю автоматические повторные запросы.

## Владение

Прежний Core Store владеет профилями, разрешёнными repos, версиями, одноразовым допуском чтения, нормализацией и привязкой фактов. Native владеет отдельным Keychain credential и UI. Rust providers исполняет ограниченные GET с предоставленным краткоживущим секретом; PAT не передаётся в SQLite, модельный запрос или журнал.

После принятой schema8 используется новая schema9 и отдельный GitHub ABI1 на прежнем memory handle. Memory3/auth1/source1/Codex1/model4 сохраняются. Старое хранилище отклоняется без миграции/reset/изменения файлов по принятому механизму допуска. Эти исходные версии сверены с принятым08B; schema9/GitHub1 относятся к новой реализации13A.

GitHub получает конкретный вариант source fence. Изменение profile settings или credential generation очищает зависимые области 08B в той же Core-транзакции, включая непрoцитированные основания. Прежние сохранённые SourceFacts остаются историческими, но использовать их через отозванный источник нельзя. Подмена второго Store, памяти или бюджета запрещена.

## Профиль и секрет

До8 профилей; имя до256 UTF-8 bytes; до32 разрешённых пар owner/repository на профиль. Совокупные настройки до1MiB; размер одной записи до16KiB. GitHub.com фиксирован: произвольного API URL нет. Owner/repository — непустые ASCII path segments до100 bytes каждый, только буквы/цифры/дефис/подчёркивание/точка, без отдельных `.`/`..`. Сравнение allowlist без учёта ASCII-регистра; отображение сохраняет исходное написание. URL PR принимается только как https://github.com/{owner}/{repository}/pull/{positive-number}, без credentials, query/fragment или дополнительных сегментов. Можно ввести те же поля отдельно. Номер ограничен положительным i64.

Настройки не выполняют запросов. Список repos является собственным ограничением Ambient, даже если PAT имеет более широкий доступ к публичным данным. UI объясняет fine-grained PAT, выбранные repos и Pull requests:read; не предлагает classic `repo` как равнозначный минимальный доступ. Принадлежность аккаунта и реальные permissions не выводятся из имени профиля или строки токена.

Profile ID, settings revision и credential generation выделяются прежним монотонным Core source clock, не переиспользуются после удаления. Settings revision и credential generation отдельны: редактирование списка repos не требует повторного ввода того же секрета.

Native хранит один envelope в отдельном generic-password service: версия, profile ID, credential generation и PAT. PAT — непустой ASCII до16KiB, без control characters/whitespace; не проверять формат по выдуманному префиксу. Значение не показывается обратно и не копируется в логи. Ключ ChatGPT, его OIDC envelope и refresh не переиспользуются.

Замена: Core сначала транзакционно повышает generation/revision и отзывает старые производные, затем единственный native credential owner пишет новый envelope. Несовпадение после ошибки/crash означает отсутствие подходящего секрета, без автоматического возврата к старому. Ошибка первой Core-транзакции означает, что замена не состоялась. Успешная Keychain запись до падения UI остаётся пригодной после restart при точном совпадении generation.

Удаление: Core отзывает generation; native подтверждает удаление Keychain item; Core удаляет профиль с ожидаемой revision. Неудачная очистка остаётся видимой и доступной для повтора; UI не объявляет профиль полностью удалённым раньше. Все native credential mutations сериализованы до фактического завершения, поздний результат проверяет expected generation/revision. Новая запись не может быть удалена хвостом старой операции. Отдельный durable журнал между SQLite и Keychain не нужен.

## Чтение одного PR

Общий допуск прежний: актуальные session/report/High Fast, отсутствие паузы, выбранное незавершённое дело и актуальные настройки. Один GitHub read-job и одно текущее окно на Store, без очереди. Начало фиксирует TaskVersion, profile ID/settings revision/credential generation, repo path, PR number и process epoch. Смена источника или дела, пауза и потеря сессии отменяют job; слот освобождается после фактического завершения transport. Отмена не доказывает отмену серверного GET.

Каждый сетевой шаг получает Core-разрешение для точной цели. Native раскрывает PAT только при совпадении envelope с generation. После сети Core повторно проверяет допуск перед принятием страницы/результата. Ссылки и инструкции из PR, комментариев и HTTP body — данные, а не полномочия.

Production authority только https://api.github.com:443. API version2026-03-10; Accept application/vnd.github+json; явный User-Agent. Только GET:

- `/repos/{owner}/{repository}/pulls/{number}`;
- `/repos/{owner}/{repository}/pulls/{number}/reviews`;
- `/repos/{owner}/{repository}/pulls/{number}/comments`;
- `/repos/{owner}/{repository}/issues/{number}/comments`.

Endpoints строятся из Core-validated segments. Нет redirect, retry, cookie jar, ссылок из body, content download, fork-repo traversal или переадресации секрета на Enterprise. Тестовый loopback route существует только под feature test support и отсутствует в production Mach-O.

Сначала PR detail подтверждает base repository ID/full_name и PR ID/number. Переименование/перенос за allowlist даёт явный конфликт; redirect не выполняется. Далее по очереди читаются reviews, inline comments и обычные comments; после них повторно PR detail. Если repo/PR identity или head SHA изменились, всё текущее окно непригодно для bind. Первый PR detail служит identity anchor; описание PR в окне берётся из конечного ответа, а не смешивается с первой версией. Неизменность identity/head не доказывает атомарность обсуждения или неизменность каждого текста; observed interval и non-atomic coverage сохраняются явно.

Пределы одного job:30 секунд, до17 GET (первый/последний PR и до5 страниц каждой из3 категорий), per_page100, до1MiB body отдельного ответа и8MiB фактически прочитанных body всего. Принимаемые заголовки до32KiB суммарно, Link до8KiB. Это проверка после HTTP parsing: установленный reqwest0.12.28 не выставляет меньший предварительный HTTP/1 bound. Его Hyper1.12.0 ограничивает parser100 заголовками и буфером417792 bytes; HTTP/2/3 сейчас выключены. Не обещать32KiB как максимум transport allocation и не менять HTTP-клиент только для этого. Deadline ограничивает всю операцию, а не перезапускается для каждой страницы. Content-Length не заменяет счётчик chunked bytes. До512 нормализованных объектов/512KiB окна; элементы не обрезаются для помещения. Достижение bound прекращает дополнительные category GET, оставляет причину partial и допускает только конечную проверку PR в пределах общего deadline/bytes; если она не состоялась, bind недоступен.

Пагинация следует только проверенному `rel=next`: точные scheme/host/path текущей категории, только per_page=100 и следующий положительный page, без fragment/credentials/неизвестных query. Endpoint формируется заново; сырой URL не отправляется. Повторные/регрессирующие/двусмысленные страницы отклоняются. Отсутствие next после успешной страницы означает конец доступной выдачи категории в этом чтении, не полную историю сервиса. Duplicate object ID с различающимся содержимым делает объект неоднозначным и недоступным для bind; одинаковые дубликаты не размножаются.

Не сохраняются raw JSON, HTTP body, ETag cache или весь просмотр. Новый явный Refresh начинает отдельное наблюдение; старое окно не сливается с ним автоматически.

## Ошибки и API-квота

401 от проверенного endpoint прекращает job без refresh/retry и отзывает соответствующую credential generation прежней Core-транзакцией с очисткой зависимых08B областей. Сравниваются существующий profile ID и точная credential generation операции: поздний401 старого PAT не отзывает новый. Это обработка недействительности секрета, а не публикация результата по делу; смена TaskVersion сама по себе не сохраняет заведомо отвергнутый credential. После restart старый Keychain envelope не совпадает с generation, повторное чтение/copy/reply через него запрещены; нужна явная замена PAT. Отдельный durable статус или журнал не добавляется. 403/404 — недоступность, не пустая выдача и не доказательство удаления; 404 PR блокирует весь результат. Любой403/429 и явный rate limit прекращают дальнейшие GET. Ошибки transport/схемы/лимита сохраняют закрытый код и категорию без текста ответа сервера.

Обработка доступных частей не должна скрывать ошибки. Если категория вернула404 или непригодное представление, другие можно дочитать в прежнем общем лимите; конечная PR-проверка обязательна. Любой403 или429 прекращает job без дальнейшего GET и устанавливает ожидание: secondary rate limit не всегда имеет отличительные заголовки. Причина403 остаётся «сервер отказал», а не выдуманное подтверждение rate limit. Rate-limit/403 stop не обходится конечным GET, результат остаётся непригоден для bind. UI показывает состояние каждой категории и временной охват, а не общий зелёный «всё прочитано».

Остановка по403/rate limit записывает Core `not_before_utc` в профиль; новый клик и restart не снимают этот срок. Он относится к доступу профиля, не меняет settings revision, не стирает исторические сведения и не становится бюджетом модели. Удаление профиля удаляет этот local state; приложение не обещает знать общую серверную квоту аккаунта или совпадение аккаунтов разных PAT.

Минимальное ожидание —60 секунд. Из валидного Retry-After принимается delta-seconds либо HTTP-date; при exhausted primary quota принимается X-RateLimit-Reset. Выбирается позднейший срок. Переполнение/двусмысленность/невалидные значения не дают немедленного повтора: остаётся60 секунд и пометка, что срок сервера не определён. Никакого фонового пробуждения или автоматической отправки после срока. Длительный валидный срок не сокращается клиентом.

Librarian проверил: установленный `time0.3.55` Rfc2822 не принимает оба устаревших HTTP-date формата. Для этого конкретного заголовка подходит небольшой `httpdate1.0.3` с `parse_http_date`, поддерживающий все три HTTP-формата; новый прямой dependency оправдан отказом от собственного парсера. Delta-seconds и epoch проверяются обычным checked integer parsing. Точная стратегия при недостоверном времени ОС остаётся обычной ошибкой допуска, без обещания обходоустойчивости к ручной смене системного времени.

## Нормализованные факты и память

Поддержаны четыре typed kind: PR description, review, inline review comment, general comment. Внешний author — подпись источника, никогда UserConfirmation. Review state не означает состояние каждого обсуждения. Resolved/outdated неизвестны; head SHA mismatch сам по себе не доказывает исправление замечания. Pending/hidden/deleted content не предполагается доступным.

SourceKey: namespace `github-com-rest-v1`; scope — repository numeric ID; object — kind + stable numeric object ID. PR number/path и profile metadata не подменяют identity. ID положительный i64, как в проверенной versioned OpenAPI; SQLite/FFI не преобразуют его через floating point. Версия — хеш нормализованного содержания и typed metadata; не только updated_at или head SHA. Время источника отдельно от времени чтения. Hash не включает локальную настройку, окно и observed interval.

Для привязки выбираются от1 до32 различных видимых объектов текущего окна. Exact preview показывает весь набор до отдельного Save; смена выбора не сохраняет данные. Core сохраняет отдельные SourceFact, source key/version и GitHub-specific metadata на прежних binding IDs. Текст комментария сохраняется точно, без обрезки/очистки «инструкций» и без исполнения. Для структурированных полей Core формирует отдельно помеченную проекцию факта, а не выдаёт её за цитату. Пустые или oversized текстовые объекты не получают выдуманного body. Каждая выбранная строка вместе со всеми переменными metadata/key/version помещается в прежние16KiB; общие64MiB включают metadata. Saved metadata paging50/256KiB включает весь возвращаемый framing.

Researcher сверил required/nullability с versioned OpenAPI2026-03-10; итоговые поля перечислены ниже. Текст body/title не дублируется в SourceFact и metadata. Для пустого review body используется помеченная Core-проекция состояния review; обычный пустой comment не получает фиктивного текста.

После повторной проверки Type Scaffolder отдельная durable read generation исключена. Единственное окно имеет process-local observation ID; страницы, preview и bind связаны с ним, TaskVersion и profile/settings/credential fences. Успешный Save меняет TaskVersion и закрывает окно; после restart окно/receipt не восстанавливаются. Эти проверки исключают позднюю привязку без дополнительной записи в DB при начале чтения. Новый локальный номер всё равно не доказывал бы свежесть серверного ответа. Существующий SourceKey/content hash определяет совпадение или явное обновление факта, а не порядок серверных версий.

Canonical SHA-256 включает type tag и length-prefixed поля в фиксированном порядке, явное различие null/пустого, точный отображаемый факт и его содержательные metadata. updated_at входит как факт, но не используется для сравнения версий. Profile/settings/credential generation, локальный интервал и paging в content hash не входят. Порядок полей совпадает с перечнями вариантов ниже; для optional сначала записывается0/1, затем при наличии значение. Integer — i64 big-endian, bool —0/1; UTF-8 строка имеет u64 big-endian длину перед bytes. В начале version tag1, kind tag и SourceKey fields с length prefix. Затем SourceFact.text, projection flag и перечисленные содержательные поля соответствующего kind. Hash записывается lowercase hex. Core повторно проверяет этот digest по сохранённым факту/metadata при открытии.

В 13A другой профиль для уже связанного SourceKey даёт явный conflict, как выбранный Codex-источник; второй binding не создаётся. Замена PAT у прежнего профиля и новое явное чтение/Save могут обновить provenance. Предложенное Type Scaffolder свободное перепривязывание через другой профиль не принято: для первого пути оно добавляло бы отдельное управление переносом, которое пока не требуется. User-edited/forgotten защита сохраняется.

Bind, metadata, версия и task revision — одна прежняя транзакция. Обычное добавление/обновление факта делает прежнюю08B область stale по её договору; не стирает обсуждение. Очистка всей зависимой области выполняется только при предусмотренных08B забывании/удалении/завершении/изменении source settings или credential generation, в той же соответствующей mutation transaction. User-edited protection, forgotten digest tombstone и запрет восстановления через другую настройку сохраняются. Delete evidence/task очищает GitHub metadata и текст; отзыв профиля сохраняет историческую память, но отнимает дальнейшее использование. Все отправленные основания, включая непрoцитированные, проверяют профиль при prepare/Send/result/copy/reply.

Пакет выбранных объектов сохраняется целиком: ошибка одного элемента, лимит, user protection или tombstone откатывают весь набор и его task revisions. Внутри одной Store transaction можно переиспользовать существующий write_source с последовательной актуальной revision; наружу публикуются только конечная TaskVersion и полный список EvidenceIds. После успеха обычная invalidation закрывает окно. Это позволяет сохранить несколько замечаний без повторных API-чтений на каждую строку; отдельного batch coordinator или частичного успеха нет.

## Native UI и проверка

Настройки: профиль, разрешённые repos, SecureField для замены PAT, состояние наличия подходящего секрета и явное удаление. Источник: выбор профиля/PR, Read/Cancel/Refresh, доступные категории и их ограничения, выбор1–32 объектов, точный preview всего набора и Save to task. Нет автоматического чтения при открытии Settings или копировании URL. EN/RU вместе. Модельная помощь и голос используют существующую08B область, без отдельного GitHub-чата.

Итоговые Finished Plan Review и Evidence Audit пройдены;08B принят и его окончательный контракт сверён. Состав операций/данных определён ниже; имена structs и внутренние модули остаются реализационным выбором без изменения владельцев и границ. Проверки реализации: настоящие temporary SQLite → C → Swift → изолированный Keychain → loopback HTTP → source bind →08B помощь; race отмены/отзыва/удаления/позднего ответа, несовпадение credential generation, отсутствие секрета в DB/log/context; pagination/partial/redirect/auth/rate/identity/bounds/tombstones;401→restart→старый envelope непригоден, прежние copy/reply закрыты, поздний401 не отзывает заменённый PAT. Полный verify, feature Swift, all-features Clippy, app build/signatures/Mach-O audit и сохранность checkpoints.

Локальные fixtures не подтверждают фактические permissions пользователя, GitHub quota, качество модели, UI или full13/G6. Реальные подключения проверяются отдельно с существующими разрешениями пользователя и учётом затрат; Jev этому источнику не нужен.

## Контракт данных и ABI

Проверенная версия OpenAPI и таблица required/nullability находятся в исследовании13. Numeric IDs в этой версии — положительные int64; Core/SQLite/FFI могут использовать i64 с проверкой >0, без преобразования через floating point. Номер PR также положительный i64. SHA — непрозрачная непустая строка до256 bytes, не предположение о40 символах. Author login — строка до256 bytes; path — до4096; открытые state/side/subject_type значения — до256. Эти bounds относятся к принимаемому представлению, не являются утверждением о максимумах GitHub.

Общая metadata привязки: profile ID/settings revision/credential generation, наблюдённые repo full_name, parent PR ID/number, interval start/end, kind и provenance/text projection flag. Repository ID и object ID уже в SourceKey и второй раз не сохраняются. Даты источника проверяются стандартным RFC3339 parser; optional/null различаются на входной границе согласно OpenAPI, после проверки эквивалентное отсутствие необязательного поля может нормализоваться в None. Canonical content hash использует нормализованные содержательные поля; local interval/profile/generation исключены.

Минимальные логические поля вариантов (source updated/submitted time размещается в прежнем SourceEvidence.observed_at_utc, не дублируется в metadata):

- PR: state (известное open/closed либо явно unknown raw), merged, created/updated time, head/base SHA, обязательный author login, признак nullable body. SourceFact.text — помеченная Core-проекция точных title/body, с явным отсутствием nullable body; title/body повторно в metadata не сохраняются. Проекция: `GitHub PR title:\n{title}\n\nGitHub PR description:\n{body}`, при null body вместо тела фиксированное `(not provided)`. Title/body сохраняются точно внутри помеченной проекции; labels не парсятся обратно. UI отдельно локализует typed kind/отсутствие body, а не принимает весь текст за цитату.
- Review: открытая state string, nullable author login и commit ID, optional submitted_at, flag исходного пустого body. Непустой SourceFact.text равен body. При пустом body текст Core-проекции равен `Observed GitHub review with an empty body.`; наблюдённая state находится в metadata и обязательно включается в assistance context. Пустая state вместе с пустым body не создаёт выдуманный факт.
- Inline: nullable author, created/updated time, nullable review ID, optional reply ID, path, required commit/original commit IDs; optional line/original line/side/subject_type; optional nullable start/original-start line/start side. Неизвестные enum values явно unknown, без значения по умолчанию. Body required string, сохраняется точно; пустой body не получает фиктивного текста. Diff hunk, code files и URLs не загружаются.
- General: nullable author, created/updated time. Body optional string: отсутствие означает недоступный текст; null нарушает опубликованную схему. Непустой body сохраняется точно; пустой/недоступный не создаёт выдуманный факт. Review/reply/path/line/commit полей у этого варианта нет.

Review User nullable, но само поле user обязательно; при объекте login обязателен. То же для inline/general. PR user non-null. Неверный тип/missing required поля делает объект непригодным для bind и увеличивает явный счётчик потерь категории. Не сохраняются raw invalid objects. Неподходящее raw-представление не подменяется body_text/body_html. Unknown additions к объекту игнорируются в пределах общего response bound, а не становятся полномочиями.

Минимальная узкая ABI-поверхность:

1. bounded read профилей/allowlist и mutation с expected revision;
2. begin credential replacement/revoke, возвращающий ожидаемую generation, и finish удаления профиля после подтверждённой native очистки;
3. begin read выбранного PR с точными gates/identity, native credential generation binding без PAT в Store;
4. provider transport start/poll/cancel, cleanup/release; callback не держит Store lock на время сети;
5. bounded window summary/category coverage и страницы объектов на одном immutable observation ID;
6. preview выбранных1–32 индексов и одноразовый atomic bind с тем же observation/TaskVersion/profile fence; output — EvidenceIds и конечная TaskVersion;
7. saved metadata paging, сохраняющее историю и источник после restart.

Названия/раскладка structs ещё требуют Type Scaffolder после принятия08B, но операции и владельцы не должны превращаться в generic command framework или второй scheduler. PAT передаётся только отдельным bounded secret argument transport-владельцу, не частью persisted command/metadata/model context. Все raw C/provider inputs проверяются на границе.

Конечный PR GET резервирует1 запрос и1MiB body из общих17/8MiB. Category GET не начинается, если этого резерва и ещё1MiB для новой страницы уже нет. По исчерпанию object/window bound новые категории не читаются, конечная проверка использует резерв. Отдельного продления общего30s deadline нет; если времени не хватило, bind не разрешается. Terminal stop (401/403/429, rate или transport failure) не обходится конечным GET. Категория404 остаётся отдельно описанным partial, при котором другие категории и обязательный конечный GET разрешены.

Весь immutable window вместе с category summary/framing/metadata ограничен512KiB/512 объектов. Его чтение идёт страницами до50 объектов/256KiB точной сериализации; непоместившиеся элементы не теряются, cursor относится к observation ID. Exact preview выбранных1–32 объектов также512KiB включая framing. До публикации и bind проверяются реальные размеры, не только сумма body strings. Invalid/oversized объекты учитываются фиксированными счётчиками причин категории, без массива raw отклонённых записей.

Source timestamps нормализуются стандартным parser в UTC seconds i64; hash относится к нормализованному представлению, а не сырым JSON байтам. Generic SourceEvidence.observed_at_utc переиспользуется для updated_at PR/comments либо submitted_at review; отсутствие submitted_at оставляет unknown. Created time хранится только там, где требуется вариантом. Нет двойного сохранения той же normalized source time в metadata. Источник времени local interval отделён от source timestamp и не включён в content hash. Parent head SHA остаётся в ephemeral read receipt; в сохранённой PR metadata head/base SHA являются собственными полями PR, а у комментариев хранится только их собственный commit ID, без вывода outdated.

Во всех model preview/context/render/copy paths источник маркируется `selected_objects / non_atomic_observation / current_state_unverified`. Подробная category pagination и её ошибки остаются в ephemeral window; отсутствие whole-PR coverage не превращается после restart в полную историю. SourceFact всегда external SourceFact, ни review approval, ни явный Save не превращают содержание в UserConfirmation или разрешение исправить код.

## Решения обзора и пределы доказательств

Scout, Librarian, External Researcher и Type Scaffolder (только чтение) проверили места расширения, native transport/Keychain механизмы и versioned OpenAPI. Независимый Plan Reviewer подтвердил один source-owner и отдельный native credential-owner; после уточнения401/terminal GET focused review — PASS. Evidence Audit — PASS с обязательной зависимостью от приёмки08B.

Не добавляется durable read generation: существующие окно/TaskVersion/source fences уже исключают поздний Save, а локальный счётчик не доказывает серверную свежесть. Не добавляется cross-store journal: несовпадение credential generation оставляет безопасное неподключённое состояние. Source metadata не дублирует body/title/source timestamp. GitHub metadata обязательно входит в combined16KiB/global64MiB и DB-open validation; существующий счётчик ещё не знает новый вариант.

Компактная OpenAPI-выдержка не содержит original_start_line; nullable этого поля отдельно проверено аудитором по закреплённому первоисточнику. Полный список полей и исходный commit указаны в исследовании. Это ограничение сохранённой выдержки, не отсутствие контракта поля.

08B принят перед запуском13A; schema8/model4 подтверждены. Новая реализация13A использует schema9/model4/GitHub1. Реальные PAT permissions, UI, server quota, live High Fast и полная13/G6 остаются отдельной приёмкой.
