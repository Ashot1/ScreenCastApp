# Стандарт OpenAI plugins и MCP для Ambient

Исследование 20 октября 2026: External Researcher, read-only, без входа в аккаунты и без чтения файлов с секретами. Веб-страницы прочитаны через WebFetch (пересказ страниц), поэтому детали полей — вторичные. Локальные манифесты плагинов оркестратор сверил сам.

## Что такое плагин

- Портативный плагин — папка: обязательный `plugin.json`, необязательные `skills/<name>/SKILL.md` (инструкции для модели), `mcp.json` (адреса и способ запуска MCP-серверов), `hooks/`, `assets/`, `.app.json` (ссылка на закрытый коннектор OpenAI). Старый формат `.codex-plugin/plugin.json` принимается для совместимости. Источник: https://developers.openai.com/codex/plugins/build.
- Переносимая часть оформлена как открытый стандарт Agent Plugins 1.0.0 (https://agent-plugins.org/specification), в руководстве участвуют Amazon, Cursor, Microsoft, OpenAI, Vercel. Совместимый клиент поддерживает хотя бы skills или MCP; для MCP — stdio или streamable-http. Спецификация не описывает OAuth и хранение секретов: это задача клиента; плагины не должны содержать секреты. OpenAI-специфичное (`apps`, `interface`, `auth`) лежит в `extensions.com.openai` и другими клиентами игнорируется.
- Публичные плагины проходят каталог OpenAI для ChatGPT/Codex; локальная установка — `~/.codex/plugins/cache/<marketplace>/<plugin>/<version>/`. Отдельного API каталога для сторонних приложений не найдено.

## Что может использовать отдельное приложение

| Часть плагина | Можно ли использовать в Ambient |
| --- | --- |
| Skills | Да, это текстовые инструкции; они рассчитывают на наличие соответствующих инструментов |
| Удалённый MCP-сервер (HTTPS) | Да, сервер требует собственную авторизацию приложения |
| Локальный MCP-сервер (stdio) | Технически да, но это запуск стороннего кода на Mac |
| Коннекторы OpenAI (`.app.json`) | Нет как готовое подключение: только внутренние ID. Через Responses API доступны лишь несколько коннекторов (Dropbox, Gmail, Google Calendar/Drive, Microsoft) с API-ключом и собственным OAuth-токеном вызывающего; GitHub и Atlassian в этом списке нет; `connector_id` устаревает для моделей после 2026-09-01 (https://developers.openai.com/api/docs/guides/tools-connectors-mcp) |
| Входы и токены Codex/ChatGPT | Нет. Официального способа передать их другому приложению не найдено; каждый клиент подключает аккаунт сам |

Локальная проверка (оркестратор, 20 октября 2026): `~/.codex/plugins/cache/openai-curated/github/*/.mcp.json` указывает `https://api.githubcopilot.com/mcp/` с `bearer_token_env_var: GITHUB_PAT_TOKEN`; `.app.json` — только `connector_…` ID; skills: github, gh-fix-ci, yeet, gh-address-comments. `atlassian-rovo` содержит только `.app.json` с ID коннектора, skills и assets, без `.mcp.json` и адреса сервера. Манифесты используют старый формат, не строгий Agent Plugins 1.0.0.

## Авторизация MCP

Спецификация 2025-11-25 (текущая ревизия 2026-07-28, раздел авторизации в ней не прочитан): для HTTP — OAuth 2.1 с обязательным PKCE, обнаружение метаданных (RFC 9728/8414), обязательный `resource` (RFC 8707), Bearer в каждом запросе. Регистрация клиента: заранее, CIMD (client_id как HTTPS-URL), DCR или ввод пользователем. Хранение и обновление токенов — задача клиента; передавать чужие токены запрещено. Codex использует loopback-callback на 127.0.0.1.

## Официальные MCP-серверы

| Сервис | Адрес и вход | Только чтение | Нужные инструменты | Структурированный ответ |
| --- | --- | --- | --- | --- |
| GitHub (MIT) | `https://api.githubcopilot.com/mcp/`; OAuth или PAT как Bearer | Заголовок `X-MCP-Readonly` или суффикс `/readonly`; выбор toolsets | `pull_request_read` (get, get_reviews, get_review_comments, get_comments, …), `issue_read` | Для протокола 2026-07-28 многие инструменты дают `outputSchema`/`structuredContent`; для чтения PR не подтверждено |
| Atlassian Rovo (Apache-2.0) | `https://mcp.atlassian.com/v2/mcp`; OAuth 2.1 или API token, который включает администратор; расход в Rovo credits | Переключателя нет; администратор выдаёт группы прав (`read_jira`, `write_jira`) | `getJiraIssue`, `listJiraIssueComments`, `listJiraIssueChangelogs` | Не документировано |
| Todoist (MIT, ранняя стадия) | `https://ai.todoist.net/mcp` (OAuth) или локальный пакет с API key | Не подтверждено; много инструментов записи | `fetch-object`, `find-comments`, `find-projects`, … | Не подтверждено |

## Протокол MCP для границ Ambient

- Текущая ревизия 2026-07-28. `outputSchema` необязателен; при его наличии сервер обязан соответствовать, клиент должен проверять. Аннотации вроде `readOnlyHint` клиент обязан считать недоверенными. Предела размера результата в спецификации нет — ограничения ставит клиент.
- Отмена: для HTTP — закрытие потока; сервер может проигнорировать отмену, подтверждения нет. Это совпадает с правилом проекта «запрос отмены не доказывает остановку».
- Rust SDK `rmcp` 3.6.0 (Apache-2.0, активный, клиент, streamable-HTTP на reqwest, OAuth, отмена, пагинация; спецификация 2026-07-28). Swift SDK — до 1.0 и на ревизии 2025-11-25.

## Не проверено

Лицензия текста Agent Plugins; структурированный ответ инструментов чтения PR, Jira и Todoist; лимиты запросов серверов; изменения авторизации в ревизии 2026-07-28; возможность вызывать инструменты через подписочный вход ChatGPT вместо API-ключа.

## Дополнение 20 октября 2026: путь помощи, подписочный маршрут, авторизация и риски

Источники: Codebase Scout (medium), Pattern Librarian (medium), External Researcher (high), read-only. Оркестратор сверил опоры кода: `src/providers/src/model.rs:51-73` (инструкции запрещают tools, `tools: []`), `:565-576` (принимаются только `message`/`reasoning`), `src/core/src/check_store.rs:7-9` (журнал 1024 записи / 1 MiB, очистки нет). Живые проверки — только неавторизованные GET/POST к метаданным серверов; входа, регистрации и вызовов моделей не было.

### Текущий путь помощи 08B

- Строго один запрос модели на действие пользователя: prepare (без сети) → begin → одна отметка попытки до POST → один POST → учёт → атомарная публикация. Поддержки вызова инструментов нет ни в запросе, ни в разборе.
- Цикл инструментов затрагивает все слои: допуск и журнал попыток Core (отметка до каждого POST; ёмкость 1024 строки без очистки), разбор ответа (`function_call`, отделение «ответ» от «ожидаются вызовы»), предел тела 320 KiB на каждый шаг, обещание точного предпросмотра, цитаты оснований (результат инструмента — не сохранённый SourceFact), ABI модели v4 (один блокирующий dispatch, без callbacks), единственный владелец `ChatGPTCoordinator`.
- Пауза, отмена, epoch сессии и правило «занято до фактического завершения» должны проверяться на каждом шаге и вокруг каждого вызова инструмента.

### Библиотеки

- `rmcp` не подходит: нужен reqwest 0.13 (второй HTTP/TLS-стек), есть автоповторы и повторная инициализация, OAuth сам управляет браузером; опубликован 20 октября 2026. Собственный минимальный клиент поверх общего транспорта ADR-020 достаточен.
- `oauth2` 5.0.0 (уже в lock, через `openidconnect` 4.0.1): PKCE, обмен кода, refresh, `add_extra_param` для `resource`. RFC 9728/8414 и регистрация клиента — собственный код. Loopback `ChatGPTLoopback.swift` переиспользуется после обобщения. SSE-разбор `model.rs` можно вынести в общий помощник. Валидатора JSON Schema в зависимостях нет.

### Подписочный маршрут (SIWC)

- Маршрут — `POST https://api.openai.com/v1/responses` с OAuth Bearer; `store:false`, `stream:true`, полная история в `input`, без `previous_response_id` (https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations).
- Поддерживаются клиентские function/custom tools, сгруппированные в namespaces или переданные через `additional_tools`; встроенные MCP/коннекторы, file search, code interpreter не поддерживаются. Неподдерживаемое — 400 `subscription_sharing_unsupported_capability`.
- Reasoning-элементы с `encrypted_content` и все `function_call`/`function_call_output` с последнего сообщения пользователя пересылаются обратно.
- Вызов инструментов именно моделью `gpt-6-luna` на этом маршруте не подтверждён — нужен настоящий запрос.

### Авторизация MCP 2026-07-28

- Транспорт без состояния: нет `initialize` и `Mcp-Session-Id`; запросы сервера к клиенту приходят как `input_required`; sampling/roots/logging устарели.
- DCR устарел (но доступен); рекомендуется CIMD — описание клиента по HTTPS-адресу приложения. PKCE S256, `resource` (RFC 8707), RFC 9728, проверка `iss` (RFC 9207), привязка учётных данных клиента к issuer, step-up при `insufficient_scope`, ротация refresh.
- Redirect — `localhost` или HTTPS; нужна проверка схемы адреса авторизации.

### Серверы (проверка метаданных 20 октября 2026)

| Сервер | Авторизация | Можно ли подключить без одобрения вендора |
| --- | --- | --- |
| GitHub `api.githubcopilot.com/mcp/` | AS `github.com/login/oauth`, нет DCR и CIMD; PAT как Bearer документирован | Да: личный токен пользователя или собственное OAuth/GitHub App (у OAuth App обмен кода требует секрета; device flow не проверен) |
| Atlassian `mcp.atlassian.com/v2/mcp` | CIMD и DCR, S256; API token — только если включил администратор | Вероятно да; подтверждает только настоящее подключение |
| Todoist `ai.todoist.net/mcp` | CIMD и DCR, S256, `resource` | Да; единственный scope `data:read_write` (с записью) |

### Риски и рекомендации спецификации

- Человек должен иметь возможность отклонить вызов; клиенту следует показывать аргументы до вызова (утечка данных), проверять результаты, ставить сроки и вести журнал.
- Аннотации (`readOnlyHint`) недоверенные; механизма закрепления описаний инструментов в протоколе нет — клиент закрепляет сам.
- Предела размера результата нет — ограничивает клиент.
- Клиент без заявленных возможностей elicitation/sampling таких запросов не получает.

## Кандидаты пилота плагинов (20 октября 2026)

External Researcher (настроенный medium), read-only; живые проверки — только неавторизованные POST (401 и метаданные). Условия пилота по D60/D62: сервис вне списка интеграций, официальный удалённый MCP, вход личным токеном `Bearer` на первом шаге, режим только чтения.

| | Linear | Notion | Sentry | Figma | Slack | Stripe |
| --- | --- | --- | --- | --- | --- | --- |
| Удалённый MCP | `https://mcp.linear.app/mcp` | `https://mcp.notion.com/mcp` | `https://mcp.sentry.dev/mcp` | `https://mcp.figma.com/mcp` | `https://mcp.slack.com/mcp` | `https://mcp.stripe.com` |
| Личный токен как Bearer | Да: API key или OAuth token (https://linear.app/docs/mcp) | Только «MCP tokens» на Business/Enterprise, создаёт владелец | Нет: `Sentry-Bearer` или OAuth | Нет: только каталогизированные клиенты | Нет: OAuth и одобрение приложения | Да: agent API key |
| Только чтение | Да: `/mcp/readonly` (метаданные: `scopes_supported: ["read"]`), ключ с правом `Read`, OAuth scope `read` | Режима нет | Не документировано | Не документировано | Нет | Через права ключа |
| Плагин OpenAI | `openai/plugins/plugins/linear` (`.mcp.json` на полный `/mcp`) | Есть | Есть | Есть | Есть | Есть |
| OAuth позже | CIMD, DCR, S256, `iss` | CIMD, DCR, S256 | OAuth | — | — | — |

Лимиты Linear для API key: 2 500 запросов/час и 3 000 000 очков сложности/час на пользователя (https://linear.app/developers/rate-limiting); отдельные лимиты MCP не найдены. Инструменты Linear часто меняются (changelog: `list_customer_needs` с 2026-10-08).

Не проверено: аннотации `readOnlyHint`, `outputSchema`/`structuredContent` инструментов Linear (нужен авторизованный `tools/list`); работает ли API key именно на `/mcp/readonly`; метка GA. Плагин Linear указывает полный `/mcp` без токена — Ambient при импорте должен использовать `/readonly` и свой заголовок.

## Пилот Google Docs (20 октября 2026)

External Researcher (встроенная настройка), read-only; живые проверки — неавторизованные `initialize`/`tools/list`/`tools/call` и `.well-known`, без входа.

- Официальные удалённые MCP Google, Developer Preview (Google Workspace Developer Preview Program): Docs `https://docsmcp.googleapis.com/mcp/v1`, Drive `https://drivemcp.googleapis.com/mcp/v1`, недокументированный Workspace `https://workspacemcp.googleapis.com/mcp/v1`. Без состояния, протокол 2025-06-18. В своём проекте Cloud нужно включить `docs.googleapis.com`/`docsmcp.googleapis.com` и `drive.googleapis.com`/`drivemcp.googleapis.com` (https://developers.google.com/workspace/docs/api/guides/configure-mcp-server, https://developers.google.com/workspace/drive/api/guides/configure-mcp-server).
- `initialize` и `tools/list` работают без входа; `tools/call` — 401 с `resource_metadata`, оканчивающимся именем инструмента (`…/oauth-protected-resource/read_doc`), а `resource` в нём — `https://docsmcp.googleapis.com/mcp` (не совпадает с `/mcp/v1`; строгая проверка RFC 9728/8707 потребует считать настроенный адрес сервера авторитетным).
- Инструменты (аннотации серверные, недоверенные): Docs `read_doc` (readOnly, комментарии флагом), `update_doc` (запись); Drive `search_files`, `list_recent_files`, `get_file_metadata`, `get_file_permissions`, `read_file_content` (текстовое представление, флаг комментариев), `download_file_content` (readOnly, без outputSchema), `copy_file`/`create_file` (запись); Workspace `search_corpus`. У инструментов чтения `readOnlyHint: true` и `outputSchema`.
- Авторизация: AS `https://accounts.google.com/`; нет DCR и CIMD; PKCE S256; `iss` (RFC 9207); есть device flow. Scopes: Drive — `drive`, `drive.readonly`, `drive.file`; Docs — `documents.readonly`, `documents`, `drive.readonly`, `drive`.
- Настольный клиент (https://developers.google.com/identity/protocols/oauth2/native-app): тип «Desktop app», loopback `127.0.0.1:<port>`, PKCE, публичный клиент, refresh при `access_type=offline`. Документация MCP описывает Web-клиенты с флагом «Use this client for an AI-powered agent»; работает ли Desktop-клиент с MCP-серверами — не проверено.
- Классы прав: `documents(.readonly)` — sensitive; `drive`, `drive.readonly`, `drive.metadata.readonly` — restricted; `drive.file` — non-sensitive (только выбранные пользователем файлы). Тестовый режим: до 100 тестовых пользователей, согласие и refresh для непростых прав истекают через 7 дней (https://support.google.com/cloud/answer/15549945). Для личного использования проверка Google не нужна; публичное распространение потребует проверки (CASA для restricted при передаче данных через серверы).
- Плагин OpenAI `openai/plugins/plugins/google-drive` v0.1.15 (MIT): `.mcp.json` на `drivemcp…/mcp/v1` с правами записи и заглушками client_id/secret; навыки рассчитаны на запись и инструменты Codex — для Ambient пригодны только адрес и имена инструментов.

Не проверено: работа Desktop OAuth-клиента с MCP Google; нужна ли запись в Developer Preview для личного проекта; размеры ответов `read_doc`/`read_file_content`; снимает ли публикация «In production» без проверки 7-дневный срок для sensitive/restricted прав.

## Живая проверка Google, 21 октября 2026

Отдельная сессия выполнила разовую проверку со своим OAuth-клиентом пользователя (Desktop app), только стандартной библиотекой Python; секреты, токены, тексты и ID документов не сохранялись, грант отозван. Итог — частичный: OAuth и `initialize`/`tools/list` подтверждены, все `tools/call` отклонены «The caller does not have permission». Подробности и выводы для ADR-021 — `target/verification/google-spike/findings.md`, `summary.json`; сводка — ADR-021, раздел «Живая проверка Google». Ранее записанный issuer `https://accounts.google.com/` уточнён: `iss` приходит без завершающей косой черты.

## Доступность официального MCP Google и замены — исследование 21 октября 2026

После сообщения пользователя («Preview… обычный Gmail в эту программу не принимают») External Researcher проверил первичные источники. Страницы читались через сводку WebFetch; перед цитированием в ADR их нужно открыть заново.

Официальные серверы Google:

- Drive MCP и Docs MCP — Developer Preview, не GA. Предварительное условие — «Membership in the Developer Preview Program»: https://developers.google.com/workspace/drive/api/guides/configure-mcp-server, https://developers.google.com/workspace/docs/api/guides/configure-mcp-server, https://developers.google.com/workspace/preview.
- Запись в программу — по адресу электронной почты и проекту Google Cloud. Заявка просит аккаунт Google Workspace и рассматривается пару дней.
- Условия Preview запрещают давать доступ пользователям вне своего домена или компании без разрешения Google. Распространять пилот другим пользователям до GA нельзя.
- В проекте включаются `drive.googleapis.com`, `drivemcp.googleapis.com`, `docs.googleapis.com`, `docsmcp.googleapis.com`. OAuth-клиент отмечается «Use this client for an AI-powered agent». Аудитория по умолчанию — Internal, то есть организация Workspace.
- Блог Workspace (май 2026) называет публичную preview «для всех клиентов Google Workspace». Даты GA нет.
- Права обычного Gmail явно не описаны, но всё указывает на организацию Workspace. Симптом проверки 21 октября («The caller does not have permission» на каждом `tools/call` при работающих OAuth и `tools/list`) совпадает с отзывом поставщика Aembit: до записи в программу Google отвечает ошибкой прав на каждый вызов. Это вывод, а не документированный признак.

Замены через обычные Drive API v3 / Docs API:

- Только между пользователем и Google, но локально (stdio или свой HTTP-сервер). Все три — MIT, со своим OAuth-клиентом:
  - архивный эталонный `servers-archived/gdrive` — `drive.readonly`, архив с 29 мая 2025;
  - `isaacphi/mcp-gdrive` — `drive.readonly` и `spreadsheets`, есть запись в Sheets;
  - `taylorwilsdon/google_workspace_mcp` — активен, флаг `--read-only`, поддерживает бесплатные аккаунты, README предупреждает о prompt injection.
- Удалённые через посредника: Zapier MCP, Composio, Pipedream. Токены и содержимое Drive проходят через стороннюю компанию, обычно под её OAuth-клиентом.
- Удалённого HTTPS-сервера, который работает с обычным Gmail и передаёт данные только между пользователем и Google, не найдено.

Прямой доступ из приложения:

- Drive API v3 (`files.list` с `q`, `files.export` Google Docs в `text/plain`/`text/markdown`, предел 10 MB) и Docs API `documents.get` работают с `drive.readonly` без записи в Preview.
- Обычный Gmail работает тестовым пользователем OAuth-клиента в режиме Testing (до 100 пользователей). В этом режиме разрешение и refresh token истекают через 7 дней.
- `drive.readonly` — restricted: для публичного распространения нужна проверка Google.

## google_workspace_mcp (D68) — устройство, 21 октября 2026

External Researcher прочитал код тега v2.1.1 (коммит 7096dd9d, PyPI `workspace-mcp` 2.1.1, MIT, Python ≥3.10) без установки и запуска.

**Запуск и версии.**
- Самодостаточного бинарного файла нет: нужны Python и `uv`. Пакет `.mcpb` тоже вызывает `uv run`.
- Зависимости: 18 основных и `uv.lock` с sha256 всех артефактов. Закрепление с хэшами возможно только через lock или экспорт requirements с `--require-hashes`.
- Подписей нет, attestation PyPI для колеса 2.1.1 отсутствует (404).
- Фактически один сопровождающий, несколько выпусков в неделю; 2.0.0 вышел 3 октября 2026.

**Транспорты.**
- stdio по умолчанию или streamable HTTP. Порт — `WORKSPACE_MCP_PORT`, хост — `WORKSPACE_MCP_HOST`.
- В режиме OAuth 2.1 без явного хоста сервер слушает 0.0.0.0.
- В обычном режиме HTTP никакой проверки у локальной точки нет.

**Вход.**
- Собственный OAuth сервера хранит refresh token и `client_secret` открытым JSON в `~/.google_workspace_mcp/credentials` (0600). Связки ключей нет (issue #706 открыт).
- Есть режим внешнего провайдера: `MCP_ENABLE_OAUTH21=true`, `EXTERNAL_OAUTH21_PROVIDER=true`, только streamable HTTP. В нём клиент передаёт `Authorization: Bearer ya29…`, а сервер проверяет токен через userinfo Google.
- В этом режиме нужен `GOOGLE_OAUTH_CLIENT_ID`, а `client_secret` не нужен. Обновлять токен сервер не умеет — свежий токен должен передавать клиент.
- `WORKSPACE_MCP_STATELESS_MODE=true` отключает файл журнала; README обещает отсутствие записи на диск. Это не проверено.

**Только чтение.**
- `--read-only` запрашивает только права чтения и убирает инструменты, которым нужны другие права. Для Drive и Docs это `drive.readonly`, `documents.readonly` и базовые права профиля.
- `--tools drive docs` ограничивает сервер Drive и Docs.
- У инструментов есть `ToolAnnotations(readOnlyHint=…)`, но с двумя оговорками:
  - `get_drive_file_download_url` помечен как только чтение, хотя пишет файл на локальный диск;
  - `start_google_auth` помечен как запись, но фильтр только чтения его не убирает (в режиме OAuth 2.1 он удаляется сам).
- Поэтому закреплять нужно по списку имён, а не только по отметке.

**Инструменты и результаты.**
- В режиме только чтения Drive+Docs остаётся около 15 инструментов: `search_drive_files`, `get_drive_file_content`, `list_drive_items`, `search_docs`, `get_doc_content`, `list_docs_in_folder`, `get_doc_as_markdown`, `list_document_comments` и диагностические.
- Результаты — простой текст.
- Предела размера по умолчанию нет: `WORKSPACE_MCP_MAX_FILE_BYTES` не задан.

**Сеть.**
- В основном коде, по README, сервер обращается только к хостам Google; телеметрии и проверки обновлений там не найдено. Зависимости (fastmcp, uvicorn) не проверены.
- OpenTelemetry выключен, пока не задан `OTEL_EXPORTER_OTLP_ENDPOINT`.
- Отладочный журнал `~/.google_workspace_mcp/logs/mcp_server_debug.log` пишется, если не включён stateless-режим.
- Чтение локальных файлов отключает `WORKSPACE_MCP_DISABLE_LOCAL_FILES=true`.

**Безопасность.** Опубликованных advisories нет. README прямо предупреждает о prompt injection.

**Архивный `server-gdrive`.** Слабая замена: только stdio, один инструмент `search` без отметок, токен в открытом файле, SDK 1.0.1.

## Поправка External Researcher при 24A (20 октября 2026)

Страница ограничений preview SIWC не упоминает `subscription_sharing_unsupported_capability` и правила повторной отправки reasoning/function_call — на этой странице оба не подтверждены. Страница говорит, что инструменты должны быть сгруппированы в namespace или переданы элементами `additional_tools`.

## workspace-mcp 2.1.1 при запуске здесь (24B, 21 октября 2026)

- Без секрета клиента сервер требует `FASTMCP_SERVER_AUTH_GOOGLE_JWT_SIGNING_KEY`, иначе завершается при старте.
- `tools/list` (stdio, обычный режим): 29,3 KB, 16 инструментов; в режиме OAuth 2.1 — 15.
- В режиме внешнего провайдера пишет `<HOME>/Library/Application Support/fastmcp/oauth-proxy/<id>` — внутри частной папки.
- `uv` проверяет sha256 архива python-build-standalone: подменённое зеркало отвергнуто.
