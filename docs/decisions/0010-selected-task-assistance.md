# ADR-010: помощь по выбранному сохранённому делу

Статус:06F реализован и принят локально19 октября2026. Полный verify:8 tooling/83 Rust/48 Swift; отдельный Swift–C–Rust/HTTP/SSE прогон8 tests, build/signature/review PASS. ModelABI2/schema5, память3/auth1 сохранены. Частичная реализация согласованных06/07/08 по D56/D57; наблюдение, инициатива и полная приёмка не заменены ручным запросом. Настоящий High Fast, native consent/no-credit, новая SIWC схема и полезность модели отдельно не подтверждены.

> **Уточнение 22 октября 2026 (D91, D96, ADR-025, 27A):** уровень обработки модели больше не проверяется, а только показывается. Успешная проверка High на любом выданном уровне, включая `default` и отсутствующий, открывает помощь; запрос по-прежнему просит `priority`.

## Problem Statement

Основа уже сохраняет дела, заметки и планы, подключается к ChatGPT и проверяет синтетический ответ. Она ещё не помогает с содержанием дела. Следующий законченный путь должен дать обоснованный следующий шаг или уточнение из выбранных сохранённых данных без новых внешних источников, имитации модели и изменения исходных данных.

## Solution

В существующем окне памяти пользователь выбирает открытое дело и сохранённые ручные заметки. Приложение показывает точный подготовленный набор для отправки: намерение, выбранные заметки и весь сохранённый план с проверками/зависимостями. Явная кнопка отправляет именно этот набор одним запросом Luna High Fast. Возвращается предложение с основанием и текстом для копирования, уточняющий вопрос либо сообщение, что обоснованного предложения пока нет. Ответ на уточнение можно сохранить существующим редактором заметки и запросить помощь заново.

Облачный анализ доступен только после собственного входа, действующего пользовательского сообщения о no-credit настройке и подтверждённой синтетической High Fast проверки этого подключения. Настройки/сохранённая память остаются доступны при остановке; рабочий текст не используется для проверки недоступного облака.

## User Stories

1. Я выбираю своё дело и вижу точный сохранённый текст, который будет передан, без скрытого чтения приложений или других дел.
2. Я выбираю нужные ручные заметки; неизвестное происхождение и внешние источники пока не допускаются этим путём.
3. Я получаю совет с понятным основанием и могу скопировать подготовленный текст по нажатию.
4. Если план неполон, помощник уточняет его, а не объявляет зависимый шаг готовым.
5. Если достаточных оснований нет, помощник сообщает об этом, не выдумывает успех или новую задачу.
6. Изменение дела, заметки или плана делает прежний вывод недоступным для действия; редактор моего текста не затирается.
7. Пауза, выход, отмена и ошибка подключения запрещают позднему ответу вернуть старое предложение, но расход остаётся учтённым.
8. Режим default или неизвестный режим не считается Fast; автоматической смены модели или повтора нет.

## Implementation Decisions

### Один владелец и два явных назначения

- Core сохраняет владение допуском, памятью, готовностью шагов, учётом и актуальностью результата. Native исполняет факты сессии/выбор пользователя/clipboard; существующий ChatGPTCoordinator остаётся единственным владельцем credentials и времени жизни auth/model операций.
- Обобщить существующий узкий журнал попыток ровно на два назначения: synthetic connectivity и selected-task assistance. Connectivity допускает фиксированные Low или High; assistance только High. Expected и actual model/effort/tier хранятся отдельно. Старый Low путь сохраняет своё поведение. Никаких произвольных моделей, промптов ABI, fallback или второго журнала.
- Схема SQLite5 заменяет4 по существующей политике: несовместимая4/WAL отвергается без записи, миграции, сброса или смены пути. Базовые memory ABI3 и auth ABI1 не меняются. Model ABI2 заменяет1 целиком, без совместимого дублирования; версия/layout/ownership документируются перед кодом. Имена частных модулей могут быть приведены к двум реальным назначениям, без широкого рефакторинга.
- Один активный model operation на Store и coordinator. Новый запрос запрещён до фактического transport/runtime/ledger cleanup. Сохранить ledger1024rows/1MiB, checked aggregates, durable possible-send unknown до POST, crash interrupted без повторов и независимый учёт валидного usage даже при отклонённом результате. При расширении fixed record пересчитать консервативный byte charge; неизвестный расход никогда не удаляется ради новой попытки.

### Доступность и полная остановка

- В Settings существующая явная проверка подключения получает закрытый выбор Low/High. Один запуск — один синтетический POST без рабочих данных, точный AMBIENT_OK и проверка выбранного профиля. Нет фоновой проверки или автоматического второго запроса.
- Core хранит только временную capability успешного High check, привязанную к текущему opaque connection epoch. Native создаёт новый epoch при новом подключении/процессе, независимо от operation generation; успешный refresh той же проверенной identity сохраняет epoch. Epoch не секрет, не сохраняется в Keychain/SQLite, не отправляется поставщику. Не вводить fingerprint личности, постоянный cloud-ready флаг или новую копию identity.
- Native передаёт текущие факты своей сессии/credit report в Core по узкой границе. Core принимает capability только после полного High/Fast результата, успешного commit учёта и совпадения текущего epoch. Устаревший ответ не может установить её. Каждая отправка проверяет valid current session fact, report, capability, отсутствие паузы и отмены. Native дополнительно проверяет действительные credentials непосредственно перед отправкой; токен остаётся только у native и authorized transport.
- Logout/new login/restart, недействительная или неоднозначная сессия, отзыв report, ошибка облачного пути и неправильный actual config сбрасывают capability. При refresh той же identity она не сбрасывается без причины. Локальный неверный ввод/stale context не объявляет облако недоступным. Пауза закрывает допуск, отменяет активную операцию и инвалидирует готовое предложение; снятие паузы само запрос не запускает.
- Пока нет настоящей capability, возможны только управление подключением и локальная память. Не добавлять test-only обход в production, постоянный закрытый gate или production mock. При разработке путь проверяется локальным HTTP через настоящий transport; настоящий High Fast всё ещё не подтверждён.

### Снимок и согласие передачи

- Core собирает один immutable снимок из TaskId/ожидаемой revision и выбранных manual EvidenceId в согласованном чтении. Не принимать текст запроса от native вместо сохранённых данных. Все включённые заметки принадлежат делу, live и Origin::User. Нет других дел, links, внешних source keys, чтения URL/файлов или скрытого расширения области.
- Полный сохранённый план этого дела включает шаги, их prompts/reference, checks/dependencies, completeness и отдельные сохранённые useful_references (до32 записей), которые также видны в preview; readiness вычисляется существующей логикой. Это разрешённый пользователем ручной текст, ссылки не разыменовываются. Показанные UI первые страницы не считаются полным планом; Core собирает полную ограниченную совокупность. Сохранить различие факта/предположения/подтверждения и неполноты.
- Не более32 выбранных заметок, прежние64steps/8checks на шаг/512edges, не более256KiB сериализованного контекста вместе с его идентификаторами/метаданными. При превышении — явная ошибка до POST, без усечения/замены. Дополнительно полный HTTP body ограничен320KiB вместе с фиксированными instructions/schema. IDs уникальны; одинаковые/чужие/удалённые IDs отвергаются.
- Native показывает подготовленный exact snapshot в раскрываемом preview и одной явной кнопкой разрешает его отправку. Не требовать повторного no-credit report на каждый запрос. Consent относится к этому immutable набору и одноразовому dispatch, не к будущему произвольному чтению. Изменение revision/выбора до dispatch отменяет подготовленный набор, не подменяет его свежим незаметно. Нет отправки несохранённого editor draft; UI ясно помечает saved content и сохраняет самостоятельный черновик.

### Настоящий результат

- Reuse reqwest/Tokio bounded transport и SSE decoder. Разделить payload и конечную semantic validation connectivity/assistance. Текущий decoder с AMBIENT_OK/Low/256bytes требует явного расширения для High и32KiB assistance; механика SSE и независимого usage остаётся общей, Low check сохраняет256bytes. Один POST, fixed endpoint/model, priority, high, store=false, stream=true, tools=[]; не добавлять запрещённый SIWC max_output_tokens. Deadline60s/connect5s/read10s, stream2MiB/frame512KiB/events4096/items8/64KiB остаются; текст assistance не более32KiB. Эти пределы не обещают ограничения удалённого расхода.
- Один strict text.format json_schema: корневой object, additionalProperties=false, все поля required. Поля: kind=suggestion|clarification|silence; message:string|null; rationale:string; evidence_ids:string[]; step_id:string|null. Использовать простую форму, доказанную существующей SIWC пробой, без новых conditional keywords. Новая схема отдельно не проверена live. Неподдерживаемая схема — ошибка, без prompt-only повторного POST.
- Provider строго проверяет форму JSON/неизвестные поля/дубли/размеры/UTF-8/конечный status; Core проверяет смысл и принадлежность оснований immutable snapshot. Message для suggestion/clarification непустое, не более16KiB; silence имеет null message и null step. Rationale непустое, не более8KiB; citations не более32 уникальных идентификаторов, только из переданного набора. Для suggestion нужна хотя бы одна ссылка на основание, для clarification допускается ссылка на намерение; silence может иметь пустой список. Каждая ссылка разрешается в сохранённый элемент, а не в произвольный URL модели.
- Core выдаёт имена оснований из собственного снимка: task, note, step/check. При наличии сохранённых шагов suggestion следующего действия требует step_id существующего готового шага и Complete плана. При Partial плане, включая ноль шагов после удаления последнего, — только clarification или silence. При Unknown плане с шагами — тоже только clarification или silence; не выводить номер волны из шаблона. Без шагов при Unknown или Complete suggestion может опираться на намерение/заметки с null step, не объявляя переход плана проверенным. При переданном step_id его готовность всегда проверяется. Валидная ссылка подтверждает происхождение, не истинность всего вывода.
- Refusal, incomplete, failed, malformed, quota и config mismatch — ошибки, не silence. Actual Luna/high/Fast проверяется заново для каждого ответа независимо от формы/usage. Полезность и новые ситуации потребуют настоящей последующей оценки; локальный JSON test её не доказывает.

### Владение результатом и действия

- Не более одного подготовленного снимка, одной выполняемой операции и одного временного Core-owned typed результата; очереди нет. Во время выполняемой операции новое приготовление отклоняется как busy; новый явный prepare после её окончания заменяет прежний снимок/результат. Результат связан с request ID, task/revision, included IDs и connection epoch. Native хранит только display projection. Не сохранять generated output как факт/заметку и не вводить durable latest table без текущего требования. Будущая08 сможет потреблять тот же тип; история предложений/повторы остаются её открытой интеграцией.
- Core проверяет сохранённую revision, open lifecycle, current connection, pause/cancel и generation при публикации и перед выдачей точного текста для копирования. Любая зависимая правка/удаление/завершение делает receipt stale. Смена выбора в UI не переносит старый ответ на другое дело; готовый editor draft сохраняется. Все пути invalidation освобождают временный контекст и результат в установленный срок.
- Только явное нажатие копирует полученный у Core актуальный текст; проверка Core — точка принятия действия, native вызывает существующий NSPasteboard путь и сообщает ошибку. Копирование не завершает шаг/дело, не запускает агента и не изменяет внешние приложения.
- Cancellation/signout/shutdown повторяют принятый06D lifecycle: закрыть допуск немедленно, сохранить generation fence до actualclose, поздний usage принять отдельно, не публиковать поздний текст. Не вводить второй native deadline или callback после disposal.
- Новый UI и сообщения одновременно EN/RU; строка меню честно отделяет пока недоступное наблюдение от проверки/помощи в окне. Нельзя объявить весь помощник работающим по одному ручному результату.

## Testing Decisions

Meaningful RED→GREEN на реально достижимом пути Swift–C–Rust → локальный HTTP/SSE → SQLite → результат. Временные test-only сервер/credentials разрешены; production всегда настоящий provider. Проверить готовое предложение/copy, clarification при неполноте, включая Partial с нулём шагов, silence, wrong citations/step/JSON/refusal/incomplete, превышение контекста до POST, чужой/external note, High/Fast mismatch, Low не открывает High, epoch/logout/report/pause/cancel, stale до dispatch/после ответа/перед copy, сохранность editor draft, quota/обрыв/unknown и restart accounting. Старые memory/auth/Low/cleanup регрессии сохраняются. Не добавлять snapshot-тесты переводов или невозможные матрицы.

Обязательны read-only Type Scaffolder до типов, Localization Mechanic, Endpoint Registry Mechanic для High профильных констант, Code Simplicity Review готовой логики. Full yarn verify, app build/signature и побайтовая сохранность всех63 файлов семи checkpoints. Schema4/WAL read-only refusal проверить отдельно. Windows/macOS14 hardware/visual UI/native consent/реальный Fast явно вне подтверждённого локального результата.

## Reference Points

На 19 октября2026, после принятого06E; Git отсутствует, implementer сверяет актуальность перед правками.

- `src/core/src/memory_types.rs`, `memory_store.rs::bump`, `plan_store.rs`: common task revision, Origin::User, plan readiness и реальные пределы. `MemorySession.load` показывает страницы, не весь план.
- `src/core/src/check_store.rs`, `check_types.rs`, `schema.sql`: одноразовый допуск, exactschema4 и durable attempt accounting.
- `src/providers/src/model.rs`, `model_tests.rs`, `model_registry.rs`: настоящий SSE, usage/config validation и fixed low connectivity payload.
- `src/bridge/src/model.rs`, `include/ambient_model.h`: model ABI1; registry leases/close и раздельные memory3/auth1.
- `src/platforms/macos/Sources/AmbientMacOS/ChatGPTCoordinator.swift`, `ChatGPTModel.swift`, `MemorySession.swift`, `AssistantModel.swift::copy`, `ConnectionCheckView.swift`: единый owner, generation и защищённый editor.
- `tests/feasibility/01-cloud/response-schema.json`, `probe.mjs::buildRequest`, recorded attempt103b41cb в evidence01: strictschema принят Luna High/SIWC, actual default. Новых live POST для планирования нет.
- [SIWC ограничения](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations), [поток](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference), [Structured Outputs](https://developers.openai.com/api/docs/guides/structured-outputs#supported-schemas). Researcher проверил19 октября; общиеAPI docs не выдаются за гарантиюSIWC любой схемы.
- Scout подтвердил:03 нужен для новых внешних источников, не для выбранного уже введённого текста. Librarian подтвердил installed reqwest/Tokio/serde_json и текущие места. Type Scaffolder предложил Core receipt; постоянный binding в Keychain не принят — достаточен process-local epoch. Независимый Plan Reviewer подтвердил временный результат и High capability без второго session owner.

## Out of Scope

Новые коннекторы/наблюдение, работа вне сохранённого ручного контекста, проактивный запуск, краевая лента, повторные напоминания, голос, Jev, автоматический запуск агентов/правок, production mocks, CLI credentials, новые live запросы во время реализации, UI-прогон и системные настройки. Полные06/07/08 и G5/G2/G6 остаются открытыми. Продуктовое ограничение Fast и правило полной остановки сохраняются.

## Приёмка спецификации

Plan Simplicity Review: исправлена неоднозначность Partial с нулём шагов, целевой повтор PASS. Evidence Audit: локальная реализация допустима; уточнено включение32 useful_references и явное расширение Low/256bytes decoder для High/32KiB без второго транспорта. Постраничные API не выдаются за atomic snapshot: согласованное чтение реализуется в Core. Copy получает текст по действующему Core receipt. Live gate не считается пройденным. Новых запросов и продуктовых уступок нет.
