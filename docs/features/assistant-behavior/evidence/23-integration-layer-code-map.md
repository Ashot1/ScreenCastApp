# Карта повторов GitHub13A/Jira12A для общего слоя (ADR-019)

Состояние: 20 октября 2026, принятый код после 12A, SQLite schema10, ABI memory3/auth1/source1/Codex1/model4/GitHub1/Jira1. Git-репозитория нет; опоры ниже — файлы и символы на эту дату. Источник — два read-only Codebase Scout (Core; providers/bridge/macOS). Ключевые опоры оркестратор сверил по коду: `src/core/src/memory_store.rs:28-31`, `assistance_types.rs:197`, `schema.sql:184-210`, `github_store.rs:348-375`, `jira_store.rs:512-560`, `src/bridge/src/lib.rs:38-45`, `src/bridge/src/github.rs:199-270`, `GitHubSourcesModel.swift:38-50`, `JiraSourcesModel.swift:40-56`.

Классы: **ОДИНАКОВО** — одинаковая семантика; **ФОРМА** — одинаковая форма, разная семантика; **ИСТОЧНИК** — особенность источника.

## Core (`src/core/src`)

`github_store.rs` (1006 строк) и `jira_store.rs` (1146) повторяют один жизненный цикл чтения; по оценке Scout около 900–1000 строк структурно одинаковы.

| Механизм | GitHub | Jira | Класс |
| --- | --- | --- | --- |
| Профили | `github_profiles` (`schema.sql:184`): id, settings_revision, credential_generation, name, repositories JSON, not_before_utc, server_deadline_known | `jira_profiles` (`:197`): те же поля, `cloud_id` UNIQUE вместо repositories; `jira_excluded_sites` (`:206`) | ФОРМА |
| Часы ревизий | `clock()` (`github_store.rs:9`) поверх общего `local_source_clock` | `jira_store.rs:10`, тот же clock; его же используют `local_source_store.rs` и `codex_store.rs` | ОДИНАКОВО |
| Сохранение профиля | `save_github_profile` (`:120`): id0 создаёт, иначе expected revision; ≤8 профилей; новая revision; generation сохраняется | `save_jira_profile` (`:127`); Cloud ID неизменяем | ОДИНАКОВО + ИСТОЧНИК |
| Allowlist / исключение | allowlist repos ≤32 (`validate_profile` `:47`); `authorized` (`:805`) повторно проверяет repo | `set_jira_site_excluded` (`:162`) повышает settings_revision профилей сайта при exclude и allow; `allowed()` (`:57`) | ИСТОЧНИК |
| Замена/отзыв секрета | `replace_github_credential` (`:139`): revision=generation=clock после expected revision | `jira_store.rs:199` | ОДИНАКОВО |
| 401 | `accept_github_response` (`:348`): отзыв только точной generation до проверки дела | `accept_jira_response` (`:512`) | ОДИНАКОВО |
| Удаление профиля | `finish_github_profile_deletion(id,revision,generation)` (`:146`) | `:212` | ОДИНАКОВО |
| Допуск чтения | `begin_github_read` (`:161`): busy, assistance (pause/session/report/High), task fence, profile version, cooldown, session expiry | `begin_jira_read` (`:335`): те же проверки | ОДИНАКОВО; порядок проверок различается, что меняет первый код ошибки |
| Fence операции | `github_fence` (`:241`): job id, cancelled, revision+generation, epoch, task | `jira_fence` (`:395`) + exclusion и cloud_id | ОДИНАКОВО + ИСТОЧНИК |
| Слот операции | `Store.github_read: Option<GitHubReadOperation>` (`memory_store.rs:30`), id из `next_source` | `jira_read` (`:31`) | ОДИНАКОВО; local/codex имеют свои слоты (`:28-29`) |
| Отмена | `cancel_*_read`, `invalidate_stale_*` из `Store::change` (`memory_store.rs:102-106`); fan-out в `assistance_store.rs:20-24`, `memory_store.rs:121-125`, `check_store.rs:162-166,180-183` | то же | ОДИНАКОВО для четырёх источников |
| Занятость до конца transport | `io_active` до `finish_github_transport` (`:550`); `release_github_read` (`:739`) Busy | `:796`, `:895` | ОДИНАКОВО |
| Пределы | 30s, 1MiB/8MiB, окно 512/512KiB, страницы 50/256KiB, 17 GET с резервом финального | те же байты/окно, 13 GET с резервом финального | ОДИНАКОВО по байтам/окну; число GET — ИСТОЧНИК |
| Реакция на предел окна | `status=limit` и переход к финальному GET | причина `object_limit`/`window_byte_limit`, чтение продолжается | ФОРМА (разное наблюдаемое поведение) |
| Пауза сервера | 403/429/`X-RateLimit-Remaining=0`, минимум 60s, Retry-After/HTTP-date/reset (`cooldown` `:976`) | 429 и 503 с Retry-After/неоднозначным; без минимума; неизвестный срок допустим (`retry_deadline` `:908`) | ФОРМА: столбцы и CAS-обновление одинаковы, триггеры и срок — ИСТОЧНИК |
| 403 | пауза и провал чтения | категория — partial, issue — провал | ИСТОЧНИК (по ADR) |
| Окно/выбор/preview | `github_window_page` (`:589`), `selected_github` (`:617`), preview (`:640`) | `:835`, `:859`, `:882` | ОДИНАКОВО |
| Атомарный bind | `bind_github_objects` (`:653`): preview == indices, одна транзакция `change`, повтор допуска, `validate_object`, tombstone→Deleted, чужой профиль/UserConfirmation→InvalidInput, `write_source`, metadata insert, `byte_room`, TaskVersion+EvidenceIds | `bind_jira_objects` (`:988`) построчно так же | ОДИНАКОВО |
| Coverage в metadata | фиксированная строка `selected_objects` | полный вектор категорий в каждом объекте (`finalize_window` `:936`) | ФОРМА (разный сохранённый JSON) |
| 08B | `SourceSettingsFence {Git, Codex, GitHub, Jira}` (`assistance_types.rs:197`) хранится JSON в `assistant_workspaces.sources`; `source_settings_fence` (`assistance_store.rs:568`) — stale, `source_authorization_fence` (`:586`) — очистка через `clear_revoked` (`proposal_store.rs:339`) внутри каждого `Store::change` | то же | ОДИНАКОВО; добавление источника — ветка match примерно в шести местах |
| Проверка при открытии | `validate_github` (`:828`): ≤8 профилей, id/revision ≤ clock, binding↔metadata, пересборка объекта и `validate_object` | `validate_jira` (`:228`) + `validate_settings` (`:78`) | ОДИНАКОВО + ИСТОЧНИК |
| 16KiB/64MiB | `combined_bytes` в parser; CHECK `data` в schema; `byte_room` (`memory_store.rs:678`) суммирует `*_metadata` | то же | ОДИНАКОВО (функция per source) |
| Метаданные вне store-файлов | DELETE/UPDATE `*_metadata` в `memory_store.rs:173,298,324`, `plan_store.rs:534`; combined-bytes ветки `memory_store.rs:247-285` | то же | ОДИНАКОВО |

Уже общие с 11A/14A: `local_source_clock`, `next_source`, хук `Store::change`, `SourceSettingsFence`. Локальные файлы и Codex не имеют `dispatched/pending/stage/gets/body_bytes` и HTTP. Generic `bind_source` отвергает namespaces GitHub/Jira/Codex (`memory_store.rs:427-429`).

## Providers (`src/providers/src`)

Общего HTTP-транспорта GitHub/Jira нет; `transport.rs` обслуживает ChatGPT/OIDC.

| Механизм | GitHub | Jira | Класс |
| --- | --- | --- | --- |
| Ошибки transport | `github.rs:11-24` | `jira.rs:11-24` | ОДИНАКОВО |
| Клиент | `:52-62`: redirect none, retry never, no_proxy, connect 5s, read 10s, без cookie store; runtime current_thread | `:61-71` | ОДИНАКОВО (третья копия флагов в `transport.rs:111-117`) |
| Секрет | `Bearer` HeaderValue `set_sensitive` (`:45-47,111`) | `basic_auth(email, token)` (`:125`), email/token как String | ФОРМА |
| Заголовки запроса | Accept vnd.github+json, UA, API version (`github_registry.rs:3-6`) | Accept json, UA (`jira_registry.rs:3-5`) | ИСТОЧНИК |
| URL | конкатенация из validated segments (`github_registry.rs:12-42`) | `reqwest::Url` из registry (`jira_registry.rs:10-89`) | ИСТОЧНИК |
| Ожидание и отмена | `waiting()` 25ms slices, cancel flag и deadline (`:192-217`) | `:184-209` | ОДИНАКОВО |
| Предел заголовков 32KiB | `:121-127` | `:134-140` | ОДИНАКОВО; набор «неоднозначных» статусов при переполнении разный (401/403/429 против 401/429/503) |
| Извлекаемые заголовки | link, retry-after, x-ratelimit-* (`:139-169`) | retry-after (`:149-161`) | ИСТОЧНИК |
| Чтение тела | только 200, chunked с checked_add, тела ошибок игнорируются (`:171-183`) | `:163-175` | ОДИНАКОВО |
| Loopback для тестов | `test()` (`:71-90`) | `:81-101` | ОДИНАКОВО |

## Bridge (`src/bridge`)

`ambient_github.h` — 13 production exports, `ambient_jira.h` — 14 (+ `site_excluded`), плюс test-only `*_test_dispatch`. Раскладки ProfileRef 24, Read 64, Batch 280 bytes одинаковы.

Одинаковые операции: profiles JSON, credential_begin, profile_delete_finish, cancel, abort, release, window (256KiB), preview (512KiB), bind, saved_metadata. Различаются: profile_save (repos против cloud_id), begin target (`pr_url` разбирается в Rust против `issue` строки), dispatch (token против email+token), шаговый предел цикла 17/13 (константы в bridge, `github.rs:227`, `jira.rs:197`). Dispatch-цикл permit → get → accept → finish под переподхватом Store lock, `drop(transport)` ждёт cleanup runtime до `finish_*_transport` (`github.rs:199-270`). Отдельные mutex отмены на источник в `StoreSlot` (`lib.rs:38-45`). Приватные helpers `now/capacity/json_output/indices` продублированы (`github.rs:43-72`, `jira.rs:37-70`). Bridge-тест есть только у Jira (`tests/jira_abi.rs:9`); раскладки GitHub проверяет Swift (`GitHubSourcesTests.swift:10-12`).

## macOS (`src/platforms/macos/Sources/AmbientMacOS`)

| Механизм | GitHub | Jira | Класс |
| --- | --- | --- | --- |
| Envelope | `{version, profileID, generation, token}` | `+ email` | ФОРМА |
| Keychain | service `dev.ambient.foundation.github`, account `profile-<id>`, не синхронизируется; SecItemUpdate→Add | `.jira`; дополнительно `data.count <= 65536` | ОДИНАКОВО + различие проверки |
| Владелец секрета | actor на serial executor, флаг `mutating`; replace: Core-first → Keychain → повторная сверка Core; delete: Core revoke → Keychain delete → finish deletion; read: сверка revision/generation, иначе abort | то же + email | ОДИНАКОВО |
| Операция/мост/окно/preview | `GitHubSources.swift` (429 строк) | `JiraSources.swift` (471) | ОДИНАКОВО, кроме типов объекта/metadata/coverage (GitHub `OpenValue`, Jira `Presence<T>`) |
| Модель | `GitHubSourcesModel.swift` (228) | `JiraSourcesModel.swift` (247) | ОДИНАКОВО; у Jira есть generation guards в `loadProfiles` (`:40-56`), у GitHub нет (`:38-50`) — вероятная гонка GitHub, тестом не проверена |
| Вид | `GitHubSourcesView.swift` (320) | `JiraSourcesView.swift` (386) | одинаковый каркас; поля metadata, email и исключение — ИСТОЧНИК |
| Связь с помощью | `ChatGPTModel.swift:278-292`, `ChatGPTCoordinator.swift:344-418` — по два метода begin/bind на источник; namespace-ветки в `AssistantView`, `AssistantSettingsView`, `SelectedAssistanceView`, `MemorySnapshot`, `SelectedAssistance.swift:95-101`, `AssistanceCardView.swift:129-135` | то же | ОДИНАКОВО |
| Локализация | 83 `github.*` | 120 `jira.*`; 49 общих суффиксов, 24 с одинаковым текстом в EN/RU | частично ОДИНАКОВО |

Swift-тесты: `GitHubSourcesTests.swift` (445 строк; :9, :218, :254, :292 loopback→08B, :369, :416 отмена) и `JiraSourcesTests.swift` (510; :9, :37, :161, :199, :326, :399, :454, :504); каждый со своим Keychain fixture и python HTTP-сервером. Loopback-путь требует `AMBIENT_MODEL_TEST_SUPPORT`.

## Регрессионные тесты Core

`src/core/tests/github_source.rs` (24 теста, :24–:1145) и `src/core/tests/jira_source.rs` (около 20, :8–:1072) покрывают профили/поколения, 401 ABA, паузу сервера, отмену/занятость, окно/preview/batch, отзыв всех оснований 08B, stale после обычного обновления, защиту ручных правок и tombstones, проверку при открытии и отказ старой схемы. Общие сценарии помощи: `tests/selected_assistance.rs`, `tests/proposal_workspace.rs`, `src/assistance_tests.rs`; локальные/Codex: `tests/local_sources.rs`, `tests/codex_source.rs`.

## Где объединение по внешнему сходству добавит сложность

- Один `accept_response` для разных схем пагинации (Link-заголовок против offset/total) потребует общий автомат стадий; разбор ответа и стадии остаются в адаптере.
- Правила паузы сервера (триггеры, минимум 60s, 403/503, «известный срок») различаются по ADR; общей может быть только запись срока.
- Coverage и metadata имеют разный сохранённый JSON и бюджет 16KiB; изменение типов меняет схему.
- Реакция на предел окна различается и проверяется тестами (`github_source.rs:754`, `jira_source.rs:841`).
- Локальные файлы и Codex не имеют HTTP-состояния; общим может быть только тонкий слот операции.
- Порядок проверок `begin` определяет первый код ошибки ABI и ожидания тестов.
- Единая структура секрета с optional `email` повторит расхождение; типизированные мутации ABI не следует заменять JSON-строками атрибутов.
- Различия, где одна сторона сильнее (generation guards Jira в Swift, `data.count` guard, checked deadline), при объединении меняют поведение GitHub и требуют теста.

## Не проверено

Тесты и сборка не запускались. Внутренности парсеров, полные тела Swift decode types и views, скрипты аудита, подсчитывающие экспорты, прочитаны не полностью. Чувствительность заголовка `basic_auth` в reqwest не подтверждена.
