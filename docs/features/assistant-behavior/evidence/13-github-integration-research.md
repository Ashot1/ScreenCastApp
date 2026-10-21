# Подготовка GitHub — 20 октября 2026

Исследование следующего объектного источника после14A/08B, не утверждённый production-контракт. Аккаунт, credentials и личные PR не читались. Новых inference-запросов нет.

## Существующая основа

Scout подтвердил прежние Core SourceKey/SourceBinding/Evidence, явный bind, обновление той же пары и content-free tombstones. Source fences сейчас Git/Codex;14A меняет их, поэтому окончательный GitHub-контракт сверяется после приёмки.

Native ChatGPTKeychain использует SecItem generic-password, но service/account, OIDC envelope и refresh lifecycle принадлежат ChatGPT. Они не являются готовым GitHub credential owner. Rust providers transport/model имеют bounded reqwest, закрытые allowlists, запрет redirects/retries, deadline/cancel и короткие ошибки без секрета; их текущие полномочия также только ChatGPT. LocalProjectsModel показывает подходящий selected-read/preview/bind lifecycle, но файловый permit не разрешает сетевой запрос.

Существующие loopback HTTP/SSE, временная SQLite и Swift–C–Rust тесты подходят для проверки новой границы. Это не доказательство GitHub API или доступа пользователя.

Librarian подтвердил установленные reqwest0.12.28/Tokio1.53.3/serde1.0.229/serde_json1.0.154: доступны GET, условные заголовки/304, `chunk()` и отключение redirects/retry. Собственного GitHub pager, общего числа страниц/байтов и cache policy библиотека не задаёт. Текущий auth transport не прерывает execute одним cancel-флагом; отмена model future — полезный существующий образец локального прекращения ожидания, не доказательство отмены серверного запроса.

Для отдельного секрета подходит native generic-password, но нужен собственный service/account и lifecycle. OIDC envelope не переиспользуется как PAT. Нынешний Keychain-код не подтверждает Data Protection/WhenUnlockedThisDeviceOnly; это нельзя обещать без отдельного решения. `HeaderValue::set_sensitive` помогает только debug-представлению и не заменяет запрет журналирования credential. Новая библиотека для этих механизмов не требуется.

## Первичные источники

External Researcher проверил публичные GitHub Docs/OpenAPI20 октября2026.

| Объект | REST GET / источник | Что сохранять как ограничение |
| --- | --- | --- |
| PR | `/repos/{owner}/{repo}/pulls/{number}`; [документация](https://docs.github.com/en/rest/pulls/pulls#get-a-pull-request) | `id/node_id`, repo-local number, head/base SHA, state/merged; nullable mergeable неизвестно |
| Общие комментарии | `/repos/{owner}/{repo}/issues/{number}/comments`; [документация](https://docs.github.com/en/rest/issues/comments#list-issue-comments) | Отдельны от inline comments; права Pull requests:read достаточны как альтернативное разрешение |
| Review comments | `/repos/{owner}/{repo}/pulls/{number}/comments`; [документация](https://docs.github.com/en/rest/pulls/comments#list-review-comments-on-a-pull-request) | IDs, reply/review IDs, commit/path/line; нет надёжного REST resolved-state |
| Reviews | `/repos/{owner}/{repo}/pulls/{number}/reviews`; [документация](https://docs.github.com/en/rest/pulls/reviews#list-reviews-for-a-pull-request) | Review state не равен состоянию каждого thread; commit может быть недоступен |
| Checks | `/repos/{owner}/{repo}/commits/{sha}/check-runs`; [документация](https://docs.github.com/en/rest/checks/runs#list-check-runs-for-a-git-reference) | Привязка к repo+SHA; filter latest/all и предел1000 suites; не полный набор обязательных merge-проверок |
| Commit statuses | `/repos/{owner}/{repo}/commits/{sha}/status` и `/statuses`; [документация](https://docs.github.com/en/rest/commits/statuses) | Отдельная система от Checks; pending возможен без statuses |
| Resolved/outdated | GraphQL reviewThreads/comments; [документация](https://docs.github.com/en/graphql/reference/pulls#pullrequestreviewthread) | Нужны обе пагинации; outdated не означает resolved/исправлено |

REST paging по Link, до100 объектов на страницу; страницы не атомарный снимок. ETag относится к конкретному URL/представлению, ETag PR не валидирует все обсуждения/проверки. IDs не являются ревизией; head SHA не является ревизией текста PR. 404 может скрывать отсутствие доступа, а недоступный API не является пустой выборкой. Учитывать реальные rate-limit/Retry-After заголовки без немедленного повтора.

Опоры: [pagination](https://docs.github.com/en/rest/using-the-rest-api/using-pagination-in-the-rest-api), [conditional requests](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api#use-conditional-requests), [404](https://docs.github.com/en/rest/using-the-rest-api/troubleshooting-the-rest-api#404-not-found-for-an-existing-resource), [rate limits](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api).

## Не выбранная пока авторизация

[Fine-grained PAT](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens) даёт один resource owner, выбранные repos, срок и read permissions; организация может требовать approval или запрещать PAT. Публичный доступ может быть шире выбора — приложению всё равно нужен собственный список разрешённых repos.

**Документальный конфликт:** endpoint Check runs перечисляет fine-grained PAT с Checks:read, но руководство ограничений PAT называет Checks API неподдержанным, а отдельная таблица PAT permissions не содержит Checks. Без проверки нельзя обещать полное чтение проверок этим способом.

GitHub App поддерживает выбранные repos и категории read permissions, но требует регистрации/установки. Для native доступен device flow без client secret на начальном шаге; refresh expiring user token требует client secret, который нельзя вшивать в общий desktop binary. Жизненный цикл ещё не выбран. OAuth App/classic PAT с приватным `repo` даёт широкие права, включая запись, и не является эквивалентом минимального read-only доступа.

Опоры: [Apps vs OAuth](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/differences-between-github-apps-and-oauth-apps), [device flow](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-a-user-access-token-for-a-github-app#using-the-device-flow-to-generate-a-user-access-token), [scopes](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/scopes-for-oauth-apps).

## Deployment и граница следующего решения

GitHub.com использует api.github.com; Enterprise Cloud отдельного домена и Enterprise Server имеют другие base URLs. Секрет нельзя автоматически переносить между ними. [REST versions](https://docs.github.com/en/rest/about-the-rest-api/api-versions) перечисляет2026-03-10 и2022-11-28; конкретные endpoint/version/allowlist ещё не закреплены.

Минимальный кандидат — выбранный PR и доступные связанные объекты → явная привязка фактов → прежняя помощь. Полная история, hidden/pending reviews, обязательные merge checks, конкретные права и актуальность каждого замечания не доказаны. До выбора auth/deployment и bounded read-контракта новый тикет13A не выпускается.

## Независимый вариант для будущей спецификации

Plan Reviewer предложил явный профиль **GitHub.com + fine-grained PAT**, выбранные repos и один PR. Это поддерживаемый способ личного подключения, не заглушка до GitHub App. Профиль не предполагает фактический deployment пользователя и не переносит секрет в Enterprise.

Категории читаются раздельно: PR, reviews, inline/общие comments, Checks и commit statuses. Checks можно реализовать по endpoint-контракту, но реальный доступ остаётся неизвестным до проверки подключения; недоступность — отдельный результат категории, не пустой список. Resolved/outdated без GraphQL неизвестны. Обязательный полный набор merge checks не выводится из доступных runs/statuses.

Один bounded read-job, только выбранные GET endpoints/authority/repo, без polling/webhook/произвольных Link URL, redirects/retry или загрузки ссылок из текста. Каждый шаг проверяет профиль, credential generation, allowlist, паузу и общий допуск помощника. Один PAT имеет отдельный native Keychain owner без OIDC refresh. Замена/удаление credentials отзывает допуск; аккаунтная принадлежность и permissions не выдумываются по пользовательскому описанию.

Источник факта — deployment/repository ID/object ID; версия содержания отдельно. Перед окончанием проверяется PR/repo/head identity, но это не обещание атомарной истории комментариев. Исходные raw ответы не образуют архив; в память попадают явно выбранные нормализованные сведения и границы их полноты.

Вариант принят как основа следующего проектирования. Точные bounds, формы фактов, политика rate-limit, операции credential/settings и ABI ещё не закреплены. Если настоящая проверка выявит недоступный Checks через PAT, нужен отдельный договор дополнительной авторизации; full13 не закрывается локальным тестом.

## Проверка границы SQLite / Keychain

Type Scaffolder работал только на чтение. Для профиля предложены отдельные settings revision и credential generation: изменение списка repos не требует заново вводить PAT. Native envelope содержит версию, profile ID, credential generation и секрет; Core хранит только ожидаемую привязку, без PAT или его хеша.

Замена: сначала Core атомарно повышает generation/revision, отзывая старую привязку, затем единственный native owner записывает новый envelope. После ошибки/crash старый секрет может физически оставаться, но несовпадение generation запрещает его использование. Если envelope записался до падения UI, совпадение после restart позволяет использовать текущий секрет. Ошибка первой Core-транзакции означает, что замена ещё не произошла.

Удаление: Core сначала отзывает generation, native подтверждает удаление Keychain item, затем удаляется профиль с ожидаемой revision. Неудачная очистка остаётся видимой и не объявляется завершённой. Нужны сериализация всех native credential writes до их фактического окончания и проверка позднего результата, чтобы старая операция не удаляла новый секрет.

Отдельный durable journal не требуется. Отсутствие/несовпадение envelope — отсутствие подходящего секрета, а не автоматическое восстановление старого. Существующий Core source clock можно использовать для новых generation/ID после проверки окончательной14A; удаление профиля не сбрасывает clock. Это предложенный контракт для ADR, не уже реализованная функция.

## Закреплённая схема обсуждений для 13A

Рабочий срез [13A](13-selected-pr-contract-draft.md) ограничен PR и обсуждениями. Checks/GraphQL и полнота13 остаются отдельно; это не отказ от них в приложении.

Researcher сверил именно [versioned REST OpenAPI2026-03-10](https://github.com/github/rest-api-description/blob/755ee33b4a81d51f6f07a902cce26cbec3b822f2/descriptions/api.github.com/api.github.com.2026-03-10.json), commit `755ee33b4a81d51f6f07a902cce26cbec3b822f2`. Исходный файл13389403 bytes, SHA-256 `dc477f0866b67a12f1f9f6adadf721a23ad87e2289980c46c5e6fa4dbc7c4492`. OpenAPI3.0.3/info.version1.1.4 — версия описания, не REST API.

Управляющий независимо проверил размер/SHA-256 и required/nullability основных полей. Компактная выдержка с исходным URL сохранена в `target/verification/orchestration/github-2026-03-10-schema-extract.json`; аккаунтные API не использовались.

| Тип | Обязательные поля | Optional / nullable |
| --- | --- | --- |
| pull-request | id:int64, number, title, body, user, created_at, updated_at, state(open/closed), merged, head/base | body nullable; user non-null, login required. base.repo non-null, его id/full_name required; head/base.sha required string без заданной длины |
| pull-request-review | id:int64, state:string без enum, body:string, user, commit_id | user и commit_id nullable; submitted_at optional date-time, не nullable |
| pull-request-review-comment | id:int64, body:string, user, created_at/updated_at, pull_request_review_id, path, commit_id/original_commit_id | user/review ID nullable. in_reply_to_id, line/original_line, side optional, не nullable; start_line/original_start_line/start_side optional nullable; subject_type optional line/file |
| issue-comment | id:int64, user, created_at/updated_at | user nullable; body optional string, не nullable |

Если nullable user является объектом, его login обязателен. Missing body, null body и пустая строка не взаимозаменяемы. Представления raw/text/html/full возвращают разные body/body_text/body_html; для выбранного raw-пути отсутствие body обычного комментария означает недоступный текст, не прочитанный пустой комментарий.

Review.state — открытая строка. Добавление значений enum GitHub считает совместимым расширением; неизвестные PR.state/side/subject_type нельзя превращать в известное значение. Опубликованная схема не разрешает null у optional line/original_line; фактические аккаунтные ответы не проверялись, расширение parser-контракта нельзя приписывать этой схеме.

Librarian также проверил HTTP-границы: текущие reqwest0.12.28/rustls без HTTP2/3 используют Hyper1.12.0 с пределом100 HTTP/1 headers и417792-byte parsing buffer; reqwest не выставляет меньший header limit. Проверка32KiB после Response ограничивает принятую информацию, не предварительную аллокацию. Для полного HTTP-date Retry-After подходит [httpdate1.0.3](https://docs.rs/httpdate/1.0.3/httpdate/fn.parse_http_date.html); установленный time/Rfc2822 отвергает два старых допустимых HTTP-формата. Зависимость ещё не добавлена.
