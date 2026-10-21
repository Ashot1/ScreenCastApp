# Jira: проверенные возможности и открытые вопросы

Проверено 20 октября 2026. Это исследование для задачи12, не утверждённый
контракт реализации и не результат подключения аккаунта. GitHub13A уже принят
локально. Исследователь использовал только публичные первичные источники;
управляющий отдельно сверил ключевые страницы авторизации.

## Авторизация и тип установки

Jira Cloud и Data Center требуют разных контрактов. Тип личной Jira пользователя
не установлен; наличие коннектора Codex не предоставляет собственному приложению
Ambient его сессию или разрешения.

Для Cloud документирован личный scoped API token, email и HTTP Basic через
`https://api.atlassian.com/ex/jira/{cloudId}/rest/api/3/...`.
Срок токена — от одного дня до года; отзыв и замена явные, OAuth refresh у
такого токена нет. Scopes ограничивают операции, но не заменяют allowlist задач
и проектов приложения. Права пользователя и ограничения организации остаются
в силе. [Управление API tokens](https://support.atlassian.com/atlassian-account/docs/manage-api-tokens-for-your-atlassian-account/).

Личные scripts/manual calls с API token документированы. На странице Basic auth
одновременно есть более широкое предупреждение против приложений, собирающих
токены клиентов. Нельзя превращать это в доказанный запрет личной автоматизации
владельца; нельзя также объявлять любой распространяемый desktop-продукт
одобренным исключением. [Basic auth](https://developer.atlassian.com/cloud/jira/platform/basic-auth-for-rest-apis/).

Требования Marketplace и их enforcement явно адресованы Marketplace cloud apps.
Пояснение Atlassian рассматривает сторонние сервисы, хранящие токены клиентов,
и рекомендует единое распространяемое OAuth-приложение. Отдельная классификация
частного native-приложения единственного владельца в проверенных источниках
не найдена. Предыдущая гипотеза о блокере скорректирована; юридическое заключение
не делалось.
[Требования](https://developer.atlassian.com/platform/marketplace/security-requirements/),
[enforcement](https://developer.atlassian.com/platform/marketplace/marketplace-security-enforcement-policy/#personal-access-tokens),
[FAQ](https://developer.atlassian.com/platform/marketplace/security-requirements-faq/#are-apps-prohibited-from-collecting-atlassian-user-account-api-tokens-even-after-taking-the-users-consent),
[пояснение Atlassian](https://www.atlassian.com/blog/developers/building-secure-and-scalable-integrations-our-guidance-for-third-party-apps).

Cloud REST OAuth3LO использует `auth.atlassian.com/authorize` и
`auth.atlassian.com/oauth/token`; опубликованный обмен/refresh требует
`client_secret`. Встраивать такой секрет в desktop-приложение нельзя считать
решением. Публичный REST PKCE/device grant не подтверждён. Для OAuth cloudId
получают из `api.atlassian.com/oauth/token/accessible-resources`, выбирая
соответствующий сайт; организационный ID ему не равен.
[OAuth3LO](https://developer.atlassian.com/cloud/jira/platform/oauth-2-3lo-apps/).

Для API-token сценария Support документирует cloudId из настроек либо
`https://SITE.atlassian.net/_edge/tenant_info`. Это не доказательство
стабильного версионированного REST-контракта этого вспомогательного адреса.
[Cloud ID](https://support.atlassian.com/jira/kb/retrieve-my-atlassian-sites-cloud-id/).

Data Center PAT использует Bearer и собственный base URL. Поддержка указана для
Jira Core/Software8.14+ и JSM4.15+; версия сервера, context path и политика
администратора должны быть установлены отдельно. Cloud v3/ADF/scopes не
переносятся автоматически.
[Data Center PAT](https://confluence.atlassian.com/enterprise/using-personal-access-tokens-1026032365.html).

## Собственное MCP-подключение — отдельный кандидат

Atlassian документирует сторонние MCP-клиенты, включая desktop/local tools,
OAuth2.1, PKCE и localhost callbacks. Текущий опубликованный адрес:
`https://mcp.atlassian.com/v2/mcp`. Это собственное подключение Ambient, а не
повторное использование доступа Codex.
[Начало работы](https://support.atlassian.com/atlassian-ai-gateway/docs/get-started-with-the-atlassian-remote-mcp-server/),
[OAuth2.1](https://support.atlassian.com/atlassian-ai-gateway/docs/configure-oauth-2-1/),
[официальный анонс PKCE](https://www.atlassian.com/blog/company-news/team26-europe-atlassian-mcp),
[домены и localhost](https://support.atlassian.com/security-and-access-policies/docs/available-atlassian-mcp-server-domains/).

Не установлены полный контракт регистрации нового произвольного клиента,
discovery/refresh/secret требования и эквивалентность инструментов выбранным
REST issue/comments/changelog с нужной полнотой. Настройки администратора могут
запрещать домен, localhost или IP. Документация также упоминает Rovo credits для
некоторых retrieval/enriched вызовов: нельзя считать MCP бесплатным без
проверки конкретных инструментов.
[Административные настройки](https://support.atlassian.com/security-and-access-policies/docs/control-atlassian-mcp-server-settings/).

## Чтение Cloud REST

Операции ниже документируют classic scope `read:jira-work`; опубликованные
granular sets нужно сверять для каждого endpoint. Sparse fields сами по себе
не доказывают возможность сократить требуемые scopes.

| Данные | Операция и существенная граница |
| --- | --- |
| Сохранённая задача | `GET /rest/api/3/issue/{issueIdOrKey}`; по умолчанию все поля, поэтому нужен явный выбор. `updateHistory=true` меняет историю просмотренного — для чтения не включать. `failFast=false` допускает частичный набор полей |
| Комментарии | `GET .../issue/{issueIdOrKey}/comment`; отдельная пагинация, только видимые данному пользователю комментарии |
| История | `GET .../issue/{issueIdOrKey}/changelog`; отдельная пагинация от старых событий. `expand=changelog` не доказывает полную историю |
| Связь | `GET /rest/api/3/issueLink/{linkId}`; доступ к связанным задачам и настройка issue linking влияют на результат. Название/направление связи не равно автоматически зависимости плана Ambient |

[Issues](https://developer.atlassian.com/cloud/jira/platform/rest/v3/api-group-issues/),
[comments](https://developer.atlassian.com/cloud/jira/platform/rest/v3/api-group-issue-comments/),
[links](https://developer.atlassian.com/cloud/jira/platform/rest/v3/api-group-issue-links/).

Кандидат минимального набора полей: summary, description, status, issuetype,
project, created, updated, parent, subtasks, issuelinks. Это исследовательский
вариант, не уже выбранная спецификация.

Ключ задачи может измениться; API способен найти перемещённую задачу по старому
ключу без HTTP redirect. Site-scoped ID предпочтительнее как идентичность;
сохранность ID при межсайтовой миграции не доказана. `updated` не является
транзакционной ревизией задачи вместе со всеми обсуждениями и историей.

Серверный размер страницы и total могут меняться, возможна пустая страница.
401 указывает проблему credentials, 404 не отличает отсутствие от недоступности.
403 может отражать scopes/политику/app access. Нельзя делать вывод, что
невидимой задачи, комментария или зависимости не существует.
[REST introduction](https://developer.atlassian.com/cloud/jira/platform/rest/v3/intro/).

API-token traffic имеет burst limits; описанные новые points quotas относятся
к Forge/Connect/3LO. Числовые примеры RPS в документации расходятся; следует
опираться на реальные ответы и собственные ограничения, не обещать постоянный
RPS. 429 содержит Retry-After и rate-limit headers; отдельные503 также могут
содержать Retry-After. Автоматические повторы пока не проектировались.
[Rate limiting](https://developer.atlassian.com/cloud/jira/platform/rate-limiting/).

## ADF и полнота

Description/comment body Cloud v3 используют Atlassian Document Format.
Ограниченное текстовое представление возможно, но оно не равно исходному
документу. Нужно явно учитывать потерю структуры, неизвестные узлы и marks;
не превращать упоминание или карточку без текста в выдуманное содержание.
Media содержит идентичность ресурса, а не сам файл. Никакого неявного чтения
вложений или внешних ссылок.
[Структура ADF](https://developer.atlassian.com/cloud/jira/platform/apis/document/structure/),
[mention](https://developer.atlassian.com/cloud/jira/platform/apis/document/nodes/mention/),
[inlineCard](https://developer.atlassian.com/cloud/jira/platform/apis/document/nodes/inlineCard/),
[media](https://developer.atlassian.com/cloud/jira/platform/apis/document/nodes/media/).

Официальные библиотеки перечислены для JavaScript/Java; готовый официальный
Rust/Swift plain-text parser не подтверждён. Node остаётся tooling, не desktop
runtime. Выбор существующей библиотеки или ограниченного преобразования требует
отдельной проверки Librarian.
[Библиотеки ADF](https://developer.atlassian.com/cloud/jira/platform/apis/document/libs/).

Исследователь проверил официальный OpenAPI:
`https://dac-static.atlassian.com/cloud/jira/platform/swagger-v3.v3.json?_v=1.8516.140`,
OpenAPI3.0.1, info.version
`1001.0.0-SNAPSHOT-e73e298945c7d2f7b579779a90edd5ffcceff065`,
2509957 bytes, SHA256
`73a2afdb0230726ff11fdbdee8e250beeb1ec73c090568deeb53c63b792dee4e`.
URL изменяемый. Управляющий сохранил и независимо сверил snapshot:
`target/verification/orchestration/jira-openapi-2026-10-20.json`;
число байтов и SHA256 совпали. Выбранные GET/schema выдержки находятся рядом
в `jira-openapi-extract.json`. Python TLS trust store не смог проверить
сертификат; системный curl загрузил документ с включённой проверкой TLS.
Схема IssueBean/Comment/Changelog слабо
ограничивает required fields, `fields` — произвольный объект. Тип SDK не
доказывает полноту/валидность полученного объекта.

## Перед реализацией

Нужны отдельные решения о поддерживаемой установке и авторизации, ограничениях
чтения/пагинации, текстовом представлении ADF, идентичности и сохранении связей.
Переиспользование памяти, отзыва доступа и помощи проверяется по принятому коду
13A после его завершения. Общий коннекторный framework заранее не выбран.

Сохранённые API-данные не дают несохранённый текст редактора и не закрывают
наблюдение, инициативное сопоставление или полную приёмку12/G6. Аккаунты,
credentials, OAuth, MCP tools и платные вызовы при исследовании не использовались.

После приёмки13A Scout подтвердил: переиспользуются общие SourceKey/SourceEvidence,
write_source и byte_room в существующем Store, source citations и08B lifecycle.
GitHub profile/types/namespace/Bearer/pagination остаются конкретными примерами,
не универсальными Jira-контрактами. Нужны отдельные typed metadata/fence/ABI.
Pattern Librarian `/root/reminder_patterns` проверил ADF и установленные
сетевые механизмы. Независимый Plan Reviewer `/root/reminder_alternative`
вернул альтернативу: личный Jira Cloud scoped-token профиль с явно заданным
cloudId; минимальный identity/project запрос перед содержимым; отдельные
comments/changelog, embedded links без обхода графа; bounded ADF-проекция с
явными потерями → единое окно/preview/batch →08B. Неизменный issue.updated
не доказывает атомарность. Link/status не создают зависимости и не завершают
дело Ambient. Cloud-срез не предполагает тип установки пользователя и не
закрывает полную12. Для проектирования такого среза обязательного нового
вопроса пользователю reviewer не нашёл.

## Уточнения после проверки библиотек и OpenAPI

Исследователь `/root/fast_route_facts` и Librarian завершили предметную проверку:

- Документация именно личных API tokens ссылается на Jira classic scopes.
  `read:jira-work` указан как Current для issue/comments/changelog; granular
  alternatives в snapshot помечены Beta. Это наименьшее число документированных
  scopes, а не доказательство наименьшего охвата данных. Реальная selectable UI
  аккаунта не проверена; заголовок связанного справочника scopes всё ещё
  OAuth/Forge, что не следует скрывать.
- Issue, project, comment, changelog и issue-link IDs — строки, без числового
  формата. Пример link содержит ID `10004L`. Нужные identity поля reader обязан
  потребовать сам: optional OpenAPI не разрешает привязку без идентичности.
- Авторы могут быть удалены, повреждены или недоступны. Display name не identity;
  `unknown`/пустые значения не объединяют авторов. Missing author и null требуют
  явной политики, а не фиктивного пользователя.
- Официальные даты comments/changelog содержат offset `+0000`, например
  `2021-01-17T12:34:00.000+0000`; одного строгого RFC3339 parser недостаточно.
  Точность дробной части не закреплена. Issue fields произвольны в OpenAPI:
  обязательность и тип `updated` из этой схемы не следуют.
- Comments поддерживают `orderBy=-created`. Changelog всегда oldest-first:
  tail offset возможен относительно наблюдённого total, но не является
  атомарным последним окном. Нет гарантии стабильного tie-break комментариев.
  Сервер может менять total/размер страницы и возвращать пустую страницу.
- `fields=project&updateHistory=false&failFast=true` — допустимая предварительная
  форма, но project возвращается целиком. Поддержка `project.id` как nested
  projection не установлена. `versionedRepresentations` использовать нельзя:
  он заставляет API игнорировать fields.
- **Предварительная проверка project и финальная проверка не гарантируют
  исключение проекта до чтения.** Между ними задачу могут переместить, включая
  A→B→A. Документированный условный content GET с проверкой project не найден.
  Project allowlist пока был только вариантом проектирования, не принятым
  пользовательским требованием; D40 требует исключения приложений/сайтов/папок.
  Перед спецификацией отдельно проверяется достаточность site-scoped доступа
  и надёжная связь cloudId с сайтом. Гарантия приватности не ослаблена.

Официальная ADF schema (публичный `https://go.atlassian.com/adf-json-schema`):
73655bytes, SHA256
`5128562b75278c8a83e7e3619a570205bc80d59696985ec31a7a7883cff66fbe`,
draft04. Doc требует type=doc, version=1, content=array. Comment.body в
OpenAPI не ограничен схемой, description находится в произвольном fields.
Missing/null/корректный пустой ADF различаются; отсутствие текста не доказывает
отказ доступа. Невидимые комментарии могут просто отсутствовать в выдаче.

Установленный serde_json1.0.154 разбирает JSON с обычным пределом глубины128,
но не выполняет ADF-проекцию и не ограничивает её размер. Нужны отдельные
границы исходного body, обхода дерева и выходного текста. Готового ADF parser
в зависимостях нет. reqwest0.12.28 предоставляет basic_auth с sensitive
Authorization; url2.5.8 — безопасную сборку path/query; httpdate1.0.3 —
Retry-After HTTP-date. GitHub cooldown и лимиты не становятся Jira-политикой
автоматически. Header bound после HTTP parsing не ограничивает предшествующее
выделение памяти самим Hyper.

Общая память уже ограничивает SourceKey namespace/scope/object_id
64/256/2048bytes, одну запись16KiB, Store64MiB; write_source/byte_room
переиспользуются. Type Scaffolder проверяет конкретную карту нового контракта
только на чтение. Plan Reviewer проверяет границу сайта, Researcher —
публичный serverInfo для связи cloudId/адреса. Готовой спецификации/задачи12A
и разрешения запуска реализации пока нет. Аккаунты и платные вызовы не использовались.

### Выбранная граница для проекта12A

Повторный независимый Plan Reviewer подтвердил: site scope по точному Cloud ID
с явным сохранённым исключением достаточен для этого среза. Project allowlist
был добавленным вариантом, не требованием пользователя; он не принят. Нового
глобального hostname resolver/table для выбранного API-источника не требуется.
Неизвестная связь hostname/Cloud ID не станет разрешением в будущей интеграции
межприложенческих исключений. Полный D40 не объявляется завершённым.

Researcher дополнительно подтвердил официальный анонимный
`GET https://api.atlassian.com/ex/jira/{cloudId}/rest/api/3/serverInfo`.
Публичный [Forge guide](https://developer.atlassian.com/platform/forge/remote-agents-in-jira/#fetching-the-jira-base-url)
указывает именно этот gateway для получения baseUrl.
[REST Server info](https://developer.atlassian.com/cloud/jira/platform/rest/v3/api-group-server-info/)
допускает anonymous/permissions None; schema ServerInformation имеет optional
baseUrl/displayUrl/deploymentType. Ответ не содержит echo cloudId, всех aliases
или revision привязки; rename между запросами не исключён. Поэтому serverInfo
не добавлен как ненужная и недостаточная проверка авторизации.

Type Scaffolder read-only подтвердил отдельный typed source/fence и bounded
FFI на прежнем Store. Его предложение отдельного объекта Project не принято:
проект достаточно сохранить как metadata задачи. Root отдельно сверил
embedded issuelinks в официальном GET issue example: только одна сторона
inwardIssue либо outwardIssue, хотя общая IssueLink schema требует обе.
Reader должен учитывать конкретную embedded форму, а не механически отвергать
документированный ответ.

Создан [проект ADR-017](../../../decisions/0017-selected-jira-issue.md).
Сейчас идут итоговые Plan Review и Evidence Audit; задача12A и implementer
ещё не созданы.

### Уточнение subtasks

Researcher проверил публичные первичные источники, root отдельно сверил request
пример: [Atlassian Support](https://support.atlassian.com/jira-align/kb/jira-aling-how-to-check-if-tempo-work-log-is-being-populated-in-a-jira-task/)
от26 сентября2025 включает `subtasks` в fields RESTv2 search.
[Cloud announcement, ответ47](https://community.developer.atlassian.com/t/deprecation-of-the-epic-link-parent-link-and-other-related-fields-in-rest-apis-and-webhooks/54048/47)
называет subtasks системным полем. [Официальная интеграция](https://support.atlassian.com/opsgenie/docs/integrate-opsgenie-with-jira-service-management/)
имеет старый v2 empty-response `subtasks:[]`; root web fetch этой страницы
завершился timeout, поэтому самостоятельной повторной проверки здесь нет.

Непустой Cloudv3 subtask response в проверенных источниках строго не описан.
Pinned Get issue example содержит sub-tasks; он также содержит updated:1 и
не задаёт строгую схему произвольных fields. Не установлено, что sub-tasks
является alias subtasks. Проект12A выбирает request/response key subtasks
и ограниченную проверяемую форму embedded issue objects; неизвестную форму
помечает partial, не «подзадач нет». Это ограничение reader и открытая
реальная12/G6 проверка, а не доказанная полноценная интеграция. Решение
передано Evidence Auditor на предмет допустимости завершения локального
контракта без ложного подтверждения провайдерской схемы.
