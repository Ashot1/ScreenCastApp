# Todoist17: исследование подключения

Проверено20 октября2026. Это публичное исследование и проект границы, не принятая спецификация, не подключение аккаунта и не полная17/G6. Исследователь /root/fast_route_facts; coordinator проверил selected OpenAPI paths/schema и SHA snapshot.

## Первичные источники

- [Todoist API v1, авторизация, pagination, operations и schemas](https://developer.todoist.com/api/v1/).
- [Личный API token, инструкция15октября2026](https://www.todoist.com/help/todoist/integrations/find-your-api-token-Jpzx9IIlB).
- [Официальный Doist CLI: read-only OAuth](https://github.com/Doist/todoist-cli/blob/main/README.md#setup).
- [SDK OAuth/public clients/refresh](https://github.com/Doist/todoist-sdk-typescript#oauth-with-custom-fetch).
- [Doist: закрытие Syncv9/RESTv2 с10февраля2026](https://groups.google.com/a/doist.com/g/todoist-api/c/brwENjfT_tk).
- [Python SDK migration3.0](https://github.com/Doist/todoist-api-python/blob/main/CHANGELOG.md#300---2025-04-11).
- [Ограничения тарифов](https://www.todoist.com/nl/help/account-and-billing/plans/todoist-plans-pricing-and-billing-faq-Vq2z0HWL6).

Официальный HTML скачан с обычной TLS-проверкой, embedded `const __redoc_state` извлечён JSONDecoder; `spec.data` canonical JSON sort_keys/separators:765721bytes, SHA256 `6bce9d0a5bbd3374003ab971233de397a1a531116615ad01609aaea6af6433c5`. Snapshot: [todoist-openapi-2026-10-20.json](../../../../target/verification/orchestration/todoist-openapi-2026-10-20.json), OpenAPI3.1.0/info.version1. HTML — рядом todoist-api-2026-10-20.html. Ни одного аккаунтного запроса не было.

## Доступ и ограничения

Личный token предназначен для собственных интеграций, но read-only scope для него не документирован. Ограничение приложения фиксированными read-операциями не сужает сам token. OAuth `data:read` существует; публичные PKCE clients с token_endpoint_auth_method:none документированы. Localhost redirect назван разрешённым для testing, поэтому production-native callback пока не подтверждён. Новые OAuth access tokens живут час, refresh вращается; grace60s может дать ответ без refresh_token, поздний reuse отзывает family. Нельзя переносить чужой OAuth-клиент или секрет.

Актуальный route — https://api.todoist.com/api/v1; старые RESTv2/Syncv9 не использовать. Account identity — GET/user.id, не displayname/email и не task.user_id (это владелец задачи, включая shared data). Account/tier пользователя неизвестны.

## Выбранные операции

| Операция | Проверенный контракт |
| --- | --- |
| GET/user | UserJSON; id/email/full_name required strings. OAuth user:read включён в data:read. Сохранять минимум identity; остальные личные поля не нужны для источника |
| GET/tasks/{task_id} | ItemSyncView, только active non-completed task. public_key optional, в предлагаемом personal path не нужен |
| GET/projects/{project_id} | AnyProjectSyncViewResponse union PersonalProjectSyncView/WorkspaceProjectSyncView |
| GET/comments?task_id=… | Ровно один task_id/project_id. results array, required next_cursor:string|null. limit default50/max200. cursor opaque, request schema pattern `^[0-9a-zA-Z_-]+\.[0-9a-zA-Z_-]+$`; прежние параметры сохраняются |
| GET/activities | object_type=item/object_id, интервалы, parent/child flags; отдельный путь, не полный аудит |
| GET/tasks/completed/by_completion_date | since inclusive/until exclusive, интервал≤3месяцев; items, next_cursor optional отсутствует в конце |
| GET/tasks/completed/by_due_date | Отдельный диапазон≤6недель |

Нет доказанной атомарности, версии окна или порядка комментариев. Active-task404 не доказывает удаление: возможно завершение или потеря доступа. ID task/project/comment — opaque string разных namespaces; tmp- placeholders не являются сохранёнными объектами. Activity ID nullable integer, поэтому generic ID-парсер не подходит.

## Поля selected models

ItemSyncView required strings id/user_id/project_id/content/description; nullable parent_id/section_id/added_by_uid/assigned_by_uid/responsible_uid/completed_by_uid; nullable timestamps added_at/updated_at/completed_at; checked/is_deleted booleans. due — nullable произвольный object; deadline/duration dictionaries, не строгие модели. note_count deprecated и всегда0; нельзя считать отсутствием комментариев.

NoteSyncView: required idstring, posted_uid nullable string, posted_at nullable date-time, is_deleted bool, file_attachment nullable object. content optional default empty поschema; отсутствие и пустое содержимое всё равно различимы в сохранённом происхождении. updated_at и parent task ID в selected schema не объявлены; связь комментария с задачей должна происходить из фиксированного запроса, не из выдуманного поля. Вложения не загружать.

Project: id/name/description strings, created_at/updated_at nullable; is_archived/is_deleted bool. Personal parent_id nullable; workspace parent_id null и workspace_id string. Дополнительные обязательные поля в полном snapshot не означают, что приложению необходимо хранить весь объект.

ActivityEvents: object_type/object_id/event_type/event_date required; ID nullable integer optional, initiator nullable string, extra_data object. Overview и extra_data расходятся по охвату updateevents; полноту истории не обещать.

## Quota/history

Документированы1000partial/100fullsync за15минут, но не универсальный лимит выбранных GET. Старые450 не переносить. HTTP error schema содержит error/error_code/error_tag/http_code/error_extra; retry_after возможен кроме429. Точные единицы и приоритет заголовка/тела ещё уточняются. Beginner history7дней, paid fullavailablehistory — не обещание бесконечной ретенции. Sync user_plan_limits.current содержит activity_log/activity_log_limit/comments/completed_tasks; is_premium недостаточно. Реальный тариф остаётся full17/G6.

## Существующий код и минимальный кандидат

Scout: общий SourceKey/SourceEvidence/write_source/byte_room; typed metadata/fences отдельные, памяти/scheduler/framework заново не создавать. NativeKeychain и serialization смотрим в JiraCredentialOwner; Jira-specific identity/auth/ADF не переносить. 08B проверяет все основания, включая непрoцитированные; ordinary refresh сохраняет stale history, revoke очищает зависимые данные.

Независимый Plan Reviewer предложил17A: явный active task + project metadata + boundedcomments → exactpreview1–32 → atomicSave →08B, без поллинга, модели при импорте, обхода соседних задач/ссылок. History/completed отдельно по доказанному API; это не закрытие полной17.

PatternLibrarian: reqwest0.12.28 bearer_auth marks header sensitive; url2.5.8 query/path encoding; time0.3.55 Date/OffsetDateTime, но нетIANA/DST базы и serde feature; serde_json Value различаетmissing/null. Markdown/HTML renderer не установлен. Сохранятьliteraltext безрендеринга — кандидат проще нового parser, формат надо сверить. date-only не превращать вUTC. TypeScaffolder read-only и проверка account-pinning ещё идут; finishedspec и оба обзора ещё не выполнены.

## Уточнения исследователя и типов

Bearer token формат/длина не доказаны: не требовать40hex или иной выдуманный формат. Локальный bound/header-validity — ограничение приложения, не обещание провайдера. Comment content может содержать Markdown; выбран literal text безрендеринга/переходов. Due prose различает date-only, floating local и fixedUTC+timezone, но embeddedfloatingexample опечатан (`2016-12-0…`), его нельзя использоватьfixture. Не вычислять UTC изfloating/IANA. Project response — объектunion безdiscriminator. RESTNoteSyncView parentID не обещает; SyncItemNoteSyncView — другаямодель.

`error_extra.retry_after` — integer секунды; новыйREST использует общийформатошибок. Header/body precedence недокументирован: локальнаяконсервативнаяполитика должна бытьявной.401 требует reauthorization, не тотжетокенповторно. Project move документирован; finalid/project/updatedне доказываютатомарность/A→B→A и pre-readprivacyprojectguard.

**Устарело по D65 (20 октября 2026):** аккаунт записывается один раз без повышения ревизии и без нового fence, см. ADR-018. Прежняя запись: TypeScaffolder read-only рекомендовал account pin припервомявномGET/user внутриCore сexactgeneration/settings/task/sessionfences; pin immutable,settingsrevповышается, текущийreadполучаетновыйfence, прочиеpreparedоперацииstale. DomainGETs толькопослеpin. FinaluserтотжеID; mismatchнетsave, другойаккаунтновыйprofile. Excludedaccount сохраняетсянезависимоотprofile; SourceKeyscopeaccountIDсохраняетtombstoneпослепересоздания. Это рекомендация дляADR, не реализация.

## Срок ожидания после 429/503 — исследование 20 октября 2026

Пользователь попросил точно выяснить, откуда Todoist API v1 даёт срок ожидания, прежде чем фиксировать правило. External Researcher проверил локальный OpenAPI-снимок (SHA-256 6bce9d0a…6433c5), публичные документы и код клиентов Doist; аккаунтных запросов и намеренного превышения лимита не было.

Документировано:

- В спецификации нет ни одного ответа 429 или 503 как объекта ответа; все упоминания — в текстовых описаниях.
- `error_extra.retry_after` — целые секунды; «может возвращаться» при ограничении частоты и при других ошибках, не только при 429.
- Пример 404 из раздела «Migrating to v1» содержит `retry_after: 3` в теле без заголовка.
- Единственный пример с заголовком — 401 в разделе OAuth: `Retry-After: 3` и `retry_after: 3` в теле; там же сказано, что на 401 ждать и повторять тот же токен не нужно.
- 503 описан только строкой таблицы статусов, без срока.
- Лимиты REST-чтения (`/user`, `/tasks/{id}`, `/projects/{id}`, `/comments`) не документированы; документированы только 1000 частичных и 100 полных sync-запросов за 15 минут.
- Формат заголовка для 429 не документирован, как и приоритет при расхождении заголовка и тела.

Клиенты Doist (клоны 20 октября 2026) срок не читают:

- `todoist-api-python` 07aba94 — повторов нет;
- `todoist-sdk-typescript` d729e5b — повторяет только сетевые ошибки;
- `todoist-ai` 0744ce7 — повторяет 502/503/504 со своей задержкой, 429 не повторяет;
- `todoist-cli` 518a67e — 429 даёт подсказку «Wait a moment and retry», а заголовок `retry-after` только пишет в отладочный журнал.

Наблюдённых ответов 429 Todoist API v1 с заголовками и телом в открытом доступе не найдено. Сторонний клиент `todoist-enhanced` читает заголовок как секунды; это его код, не снимок ответа. Ветка форума Obsidian (январь 2023) относится к старому API.

Вывод: открытые источники не позволяют утверждать, что заголовок всегда есть у 429, что поле тела всегда есть, или что они совпадают. Безопасно установить это можно только пассивным наблюдением настоящих ответов: статус, шаблон пути, строка заголовка, тип и значение поля тела, `error_code`/`error_tag`, без токена, содержимого и `event_id`. Намеренно вызывать ограничение частоты нельзя.
