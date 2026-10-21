# 01 — Одна проба через подписку ChatGPT

**Реальная High-проба подтвердила completed-items parser; G5 BLOCKED из-за default tier.** `103b41cb-2e0a-419e-84cb-6813b956d1e4`,19 октября03:07:33 МСК: text-suggestion/Luna high/priority → model/effort matching,actualtier=default,configuration_mismatch. output_source=completed_items,proposal=suggestion,evidenceIDsvalid/count1. Terminal output=[], но два done items успешно восстановлены. Смысловая оценка отдельно не проводилась, raw text не сохранялся.

HTTP200,3414ms,usage **171+117=288**,reasoning53,cache0. Ровно1 modelPOST в отдельном max1 пакете; GETmodels1/tokenPOST0/browser0. Все21 прежний liveJSON побайтово неизменны; старые3unknown сохраняются. Код не менялся, переиспользованы **234 tests PASS** и syntax/plan; High-plan/fixture проверены отдельно. Общий verify не повторялся по указанию управляющего. Новых POST нет.

[Полный отчёт](report.md).

## Проверить подготовку без аккаунта

Из корня репозитория, Node 22 и Swift 6:

```sh
npm ci --prefix tests/feasibility/01-cloud --ignore-scripts --no-audit --no-fund
swift build --package-path tests/feasibility/01-cloud/native-keychain -c release -Xswiftc -warnings-as-errors -Xswiftc -strict-concurrency=complete
node tests/feasibility/01-cloud/probe.mjs plan
yarn vitest run tests/feasibility/01-cloud
swift test --package-path tests/feasibility/01-cloud/native-keychain -Xswiftc -warnings-as-errors -Xswiftc -strict-concurrency=complete
swift format lint --strict --configuration .swift-format --recursive tests/feasibility/01-cloud/native-keychain/Package.swift tests/feasibility/01-cloud/native-keychain/Sources tests/feasibility/01-cloud/native-keychain/Tests
```

Helper собирается один раз после изменения его исходников. Node запускает готовый `native-keychain/.build/release/keychain-helper` по постоянному пути, не пересобирает его при входе и не использует `security` CLI. Отсутствующий binary или ошибка Keychain останавливают путь. macOS может показать запрос доступа к Keychain; нулевое число системных диалогов не обещается, ACL не расширяется.

`plan` только перечисляет случаи и не обращается к Keychain или аккаунту. Node-тесты используют тестовые подписи, подставленный транспорт и loopback listener. Swift-тест действительно выполняет Add/read/update/delete в собственном временном Keychain с синтетическими токенами: `kSecUseKeychain` задаёт Add destination, `kSecMatchSearchList` ограничивает каждый read/update/delete. Системный search list и default Keychain не меняются: тест сравнивает их metadata до/после создания, CRUD и удаления временного хранилища без печати имён/путей. Только создание/удаление временного файла требует legacy API в явно помеченном test fixture; helper использует SecItem API. [Детали помощника](../../../../../tests/feasibility/01-cloud/native-keychain/README.md).

Две MIT-библиотеки закреплены scoped lockfile: `openid-client@6.8.8` для code/refresh grant и проверки подписи ID token через `enableNonRepudiationChecks`; `jose@6.2.12` подписывает тестовые токены. Корневые зависимости не менялись.

Стенд фиксирует `gpt-6-luna`, Fast с каноническим `service_tier: priority`, reasoning effort `low` по умолчанию либо явный `high`. Последняя реальная проба вернула Luna/low, tier default и пустой output с reported usage; Fast и корректное предложение ещё не подтверждены.

Fast остаётся пользовательским названием. `priority` — фактически отправляемый ID в payload, plan и новых requested_configuration; возвращённые fast/priority оба допустимы. Управляющий сверил account service_tiers.id и официальный OpenAI Codex config_types.rs `ServiceTier::Fast.request_value()`; legacy additional_speed_tiers не задаёт канонический ID. Это не fallback, не новая опция CLI и не доказанное исправление HTTP 400. Две исторические попытки отправили fast; новая priority-попытка описана выше. Повтор без конкретной проверяемой гипотезы остановлен.


## Подготовленная процедура — сейчас заблокирована

**Не выполнять сейчас:** старый root unresolved блокирует обычную команду до входа, отдельно сохранена вторая unknown-попытка. Разовое разрешение уже использовано; дальнейших POST не разрешено. Команда оставлена как описание обычной процедуры, не способ обхода ledger:

```sh
node tests/feasibility/01-cloud/probe.mjs auth text-none gpt-6-luna low
# либо явно high:
node tests/feasibility/01-cloud/probe.mjs auth text-none gpt-6-luna high
```

Один процесс выполняет следующую последовательность:

1. Берёт `.local/active.lock`. Неизвестный usage предыдущей пробы блокирует запуск до чтения кэша и входа. В `.local/installation.json` с правами 0600 остаются только host UUID и проверенная регистрация `{client_id, issuer, subject}`. До первой dynamic registration пишется `registration_pending`; сорванный callback без проверенной регистрации не снимает эту защиту. При наличии проверенной регистрации она сохраняется даже при отсутствии нужных scopes/refresh token. Регистрация не создаётся заново вместо известной.
2. Читает одну generic-password запись macOS file-based Keychain. Service фиксирован, account — SHA-256 от host UUID и полной регистрации. Secret JSON содержит access/refresh token, `expires_at_ms` в миллисекундах, подтверждённые scopes, исходный nonce и регистрацию. После чтения Node проверяет форму, лимиты и точное совпадение client ID/issuer/subject с installation. Действующий access token используется без браузера. Ошибка чтения/повреждённая привязка останавливают запуск без browser fallback.
3. Только при отсутствии записи выполняет browser OAuth. Listener слушает `127.0.0.1` на свободном порту, `/auth/callback`; печатается ссылка **Continue with ChatGPT**. Известный issued ID используется без `agent_name_hint` и `id_token_hint`; `dynamic_agent_client` допустим лишь при отсутствии регистрации. Библиотека проверяет state, PKCE, nonce, подпись, issuer, audience и срок ID token. При возврате проверяются прежние issuer/subject; другой callback client ID отклоняется. Нужны `openid`, `resource.invoke`, `chatgpt.tokens.use.direct` и refresh token. Полная проверенная сессия сохраняется в Keychain до model discovery. Ошибка сохранения прекращает запуск. CLI credentials и userinfo не используются, ID token/email не сохраняются.
4. Если access token истёк или ему осталось не более 15 секунд, до `RUN` выполняет один refresh. Сначала старую запись Keychain успешно удаляет; token остаётся лишь в памяти. Затем `openid-client.refreshTokenGrant` отправляет issued client ID, refresh token и `resource=https://api.openai.com/v1`; без scope, client secret, dynamic ID или PKCE. Новый access token, фактический `expires_in` и replacement refresh token обязательны. Отсутствующий scope сохраняет только ранее подтверждённые права; присутствующий ограничивает их и проверяется заново. Optional ID token проверяется по подписи, issuer/audience/expiry и прежнему subject; nonce сверяется, если он присутствует. Replacement сохраняется до использования. Потерянный ответ, неверные данные или ошибка записи не вызывают retry и не восстанавливают старый rotating token; браузер в этой попытке не открывается. Фиксированная ошибка требует нового входа при следующем запуске с тем же issued ID и не заявляет, что сеть доказала отзыв доступа. `earliest_refresh_at` не используется.
5. Показывает [Manage usage: инструкции OpenAI](https://help.openai.com/en/articles/20001542-using-your-chatgpt-plan-in-other-apps-and-sites) и ждёт подтверждения. В [ChatGPT](https://chatgpt.com/) откройте **Settings → Usage → App limits → Ambient Assistant Feasibility → Manage limits** (или имя при входе). Сохраните cap **ниже 100%**, например 1%, либо выключите общий **Allow apps to use credits**. Проверьте сохранённую настройку в том же аккаунте/рабочем пространстве. Баланс 0 и выключенное автопополнение сами по себе её не заменяют.
6. Человек вводит `RUN` только после подтверждения no-credit настройки и разрешения этой одной синтетической пробы. Иной ответ останавливает процесс. При управлении через Codex агент ждёт явного сообщения человека и только затем передаёт `RUN`. Это `user_reported`, не API-проверка лимита; настройки аккаунта программно не меняются. После долгого ожидания истёкший access token останавливает текущий запуск, не открывая дополнительный путь inference.
7. Выполняет один `GET /v1/models` тем же access token. Каталог ограничен 2 MiB распакованных байт до разбора JSON; превышение отменяет reader и завершает попытку до POST, в том числе при ошибочном HTTP. Используются только `models[].visibility="list"` и точный slug. Ошибка discovery не удаляет полезный login cache. Недоступная модель не заменяется, `data[]` не используется как fallback.
8. До единственного `POST /v1/responses` сохраняет attempt и отметку неизвестного расхода. Запрос содержит Luna, `reasoning.effort: low|high`, `service_tier: priority` для Fast, `tools:[]`, `store:false`, `stream:true`, строгую JSON-схему и выбранные синтетические данные. Возвращённые model/tier/effort пишутся отдельно. Иной/отсутствующий model или Fast tier (`fast`/`priority`), а также явное расхождение effort дают `configuration_mismatch` с фактическим usage. Отсутствующий/null effort остаётся null; это только метаданные, не доказательство внутренних рассуждений. Нет `max_output_tokens`, API-ключа, исполнения действий, повторов или перехода на кредиты. Ошибка `subscription_sharing_unsupported_capability` завершает попытку без смены режима.

Secret JSON передаётся helper только по приватным stdin/stdout pipes, до 64 KiB; Keychain data ограничены 48 KiB. Нет secret argv/env, plaintext temp-файлов или logs. Helper stdin читается ограниченным циклом до EOF или max+1. Node собирает stdout только в памяти, игнорирует stderr и выдаёт фиксированные ошибки. Для первого macOS ACL-диалога отведено до 120 секунд. Timeout, Ctrl-C, overflow и stdin error запрашивают остановку helper; active.lock освобождается только после `close`, подтверждающего завершение процесса.

Полный процесс ограничен 10 минутами, каждый metadata/token/JWKS/models запрос — 15 секундами, модельный — 60 секундами. Локальное закрытие потока не доказывает остановку облачного вычисления. После перезапуска действующая сохранённая сессия остаётся доступной. Не удаляйте installation, lock или usage ledger для обхода блокировок. Для отзыва доступа отключите приложение в настройках подключений ChatGPT; автоматический revoke/logout CLI здесь не добавлен. Несколько аккаунтов и смена выбранной личности не поддерживаются.

## Результат, остановка и повторный запуск

`live/<attempt-id>.json` хранит case, отдельно запрошенные модель/effort/tier и возвращённые модель/tier/effort-метаданные, время, SHA-256 запроса, источник подтверждения `user_reported`, исход и фактически сообщённый usage. Личные данные входа, токены и сырые ошибки провайдера не пишутся. Входы здесь только синтетические; вывод модели не исполняется.

После `RUN` ошибка до Responses POST возвращает `status: pre_post_failed`, `post_dispatch: not_sent`, `usage_status: not_applicable`, фиксированные `diagnostic.stage`/`reason` и только известные `http_status`, `response_shape` или allowlist `system_code`. Формы ответа классифицируются как `models_array`, `data_array`, `error_object`, `detail_only`, `other_json` или `non_json`; само тело и `Error.message` не сохраняются. Полученный HTTP-ответ подтверждает `post_dispatch: sent`; сбой транспорта без ответа обозначается `possibly_sent`. Ошибка финальной записи evidence не меняет уже известный статус отправки. Если usage неизвестен, выставляется `model_request_count_not_assumed: true`, попытка не повторяется и ledger сохраняется. Ответ `/v1/models` с `data[]` не преобразуется в модели и не запускает fallback.

POST отправляет Accept:text/event-stream; GET каталога этот заголовок не получает. MIME нормализуется до части перед точкой с запятой, trim/lowercase. Отсутствующий MIME и text/event-stream идут в строгий SSE parser; JSON/иной явный MIME — в ограниченную diagnostic ветку. При HTTP 200 с неподходящим MIME reason=unexpected_content_type, actual HTTP status не подменяется.

На каждом полученном ответе response_metadata содержит только закрытые значения http_status, media_type(absent/sse/json/other), body_present, body_read_outcome и byte counters. SSE исходы: terminal_event/eof/empty/limit_exceeded/read_error/invalid_utf8/invalid_json/invalid_event/cancelled; HTTP diagnostic: complete/empty/limit_exceeded/read_error/invalid_utf8/invalid_json. Bytes_observed насыщается byte_limit+1; byte_limit — 2 MiB или 16 KiB. Terminal_event означает остановку на терминальном событии, не EOF и не успешное предложение. Метаданные остаются рядом с diagnostic при ошибке финализации evidence; отказ диска не позволяет гарантировать запись, но возвращаемый результат их сохраняет. Fetch без полученного Response не выдумывает metadata.

В ветке HTTP/non-SSE отказа полученные headers уже доказывают `sent`. Локальный reader накапливает не более 16 KiB распакованных байт; overflow chunk не сохраняется и не разбирается, чтение отменяется. Полный допустимый body декодируется строгим UTF-8 и JSON. Новые optional поля `diagnostic`:

- `response_shape`: только прежние фиксированные категории `models_array`, `data_array`, `error_object`, `detail_only`, `other_json`, `non_json`.
- `provider_code`: только `subscription_sharing_usage_limit_exceeded`, `subscription_sharing_usage_unavailable`, `subscription_sharing_unsupported_capability`; другое значение — фиксированное `provider_failure`.
- `provider_param`: только точные `model`, `service_tier`, `reasoning`, `reasoning.effort`, `tools`, `input`, `instructions`, `store`, `stream`, `text`, `text.format`; другое значение опускается.

В обычном режиме message/detail, raw body, произвольные code/param, error.type и x-request-id не сохраняются. Программный diagnosticMode имеет строгие исключения, описанные выше. Malformed UTF-8/JSON даёт только `non_json`; при overflow, read failure или отсутствующем body поля тела опускаются. Read/cancel failures остаются внутри диагностики и не меняют `http_error`, HTTP status, `sent`, unknown usage или ledger. Даже признанный код/параметр не доказывает нулевой расход. Строгие SSE/schema/config проверки, refresh и выбранные Luna Low/Fast сохранены. Parser дополнен только закрытыми исходами чтения и счётчиком; Accept/MIME исправлены по контракту devkit. Диагностика старого 400 не восстанавливается.

Успех разбора требует `response.completed` и совпадения фактически возвращённых модели и Fast tier; затем проверяется предложение: `kind`, сообщение/молчание, основание и известные evidence IDs. Наличие правильной структуры не доказывает смысловую правильность — её оценивает человек по `cases.json`. Отказ, `response.failed`, `response.incomplete`, неверный JSON, EOF, ошибка квоты, неподдерживаемая возможность и недоступность сервиса учитываются отдельно. Известные коды подписочных ограничений сохраняются; произвольные сырые сообщения — нет.

stream_event_metadata сохраняет только фиксированные event counts, bounded indices/counts/types/status/text bytes. Delta-текст не накапливается и не валидируется отдельным путём. В RAM удерживаются канонические validator-relevant поля done items, максимум8 по индексам0..7 и64 KiB суммарного JSON. Потеря/дубликат/overflow отбрасывают снимки, но чтение terminal usage продолжается. При completed с пустым output допустимы только уникальные непрерывные done indexes с нуля без pending added; reasoning done status может отсутствовать, сообщения проходят прежний validator. Непустой terminal имеет приоритет, наблюдаемые done content conflicts отвергаются, IDs/extra fields игнорируются. output_source фиксирует terminal/completed_items/unavailable; неизвестный/неполный terminal не заменяется догадкой. RAM очищается при завершении, отмене и ошибке; текст/IDs не записываются в diagnosticMode. Reasoning_tokens — только целое0..output_tokens, без reasoning text.

validation_metadata вычисляется в существующих ветвях отклонения до privacy projection. proposal_rejection — только закрытый код первого сработавшего условия; нет текста модели, evidence IDs или произвольных ключей. Configuration классифицирует model/effort как matching/missing/null/different; tier — missing/null/fast/priority/auto/default/flex/standard/other_string/other_type. Это факты диагностики, не новые допустимые режимы. Terminal_fields и message_fields содержат только фиксированные JSON-kind flags; счётчики ограничены 1000 (1000 или больше); proposal_keys содержит только kind/message/rationale/evidence_ids/unexpected booleans. При raw terminal output=[] результат теперь зависит от полной завершённой последовательности done items; после её восстановления JSON проверяется обычным валидатором. Исторический message_count не переписывается.

Если терминальное событие сообщило корректный usage, локальная неопределённость этой попытки снята. Это не точный остаток общей подписки и не оценка цены в USD. Без такого usage остаётся `live/unresolved.json`; все следующие пробы блокируются. При аварии процесса также может остаться `.local/active.lock`. Не удаляйте эти отметки для обхода остановки: сначала подтвердите завершение процесса и разберите потребление через Usage/поддержку. Автоматической сверки или команды «обнулить расход» нет. Нулевой расход не предполагается.

Проверка исчерпания квоты сейчас только локальная: специально расходовать подписку до лимита не требуется. После настоящих проб фиксируются доступная модель, смысл результата, actual usage, задержка и видимые ограничения. Не запускайте всю матрицу автоматически и не повторяйте неизвестную попытку.

## Матрица будущих проб

| Случай | Проверка | Сейчас |
| --- | --- | --- |
| text-suggestion | Предложение проверить источники, известное основание | BLOCKED: нового POST не разрешено |
| text-none | Молчание без нового обязательства | Две POST-попытки: HTTP 400, sent, usage unknown; дальнейшие POST не разрешены |
| image-suggestion | Красный квадрат слева, ссылка image-1 | BLOCKED: нового POST не разрешено |
| image-none | Отсутствующий зелёный треугольник → none | BLOCKED: нового POST не разрешено |
| refusal | Безопасный отказ или none; специальный refusal-блок не гарантирован | BLOCKED: нового POST не разрешено |
| incomplete | Только локальная обработка terminal incomplete; выходной cap не передаётся | Локально проверено, настоящий исход не наблюдался |
| cancel | Abort через 1 секунду; ранний ответ не доказывает отмену | BLOCKED: нового POST не разрешено |

При входе OpenAI обрабатывает данные аккаунта/разрешений/устройства. Для inference передаются только синтетический вход и инструкции; `store:false` не обещает отсутствия журналов, кэша или обучения. Применимость конкретных сроков хранения и настроек обучения к direct SIWC не подтверждена. Удаление локальных файлов не доказывает удаления копий OpenAI.

Публичный SIWC-контракт и условия данных — [sources.md](sources.md). Исторический платный путь и обсуждение CLI — [history-report.md](history-report.md); их команды и расчёты не являются действующей процедурой.
