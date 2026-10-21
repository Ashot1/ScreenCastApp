# 06: Первый работающий путь помощника

Spec: `docs/features/assistant-behavior/spec.md`. Decisions: `docs/features/assistant-behavior/decisions.md`.

**What to build:** Сделать первый настоящий путь в macOS: пользователь задаёт намерение → помощник читает разрешённый текущий контекст → сохраняет нужную память одного дела → получает облачный вывод → показывает ненавязчивую карточку с основанием и копированием. Здесь создаётся и проверяется реальная связь ядра с оболочкой, без производственных имитаций.

**Blocked by (полная приёмка):** 01 — Облачный анализ и предельная стоимость; 03 — Общий доступ к приложениям.

**Начало независимой части (D56):** основа может начаться сразу: настоящее macOS-приложение → узкая C ABI граница → Rust → постоянное хранение одного намерения, его просмотр/изменение/удаление и сохранение ручной паузы. Перед первым хранилищем нужен ADR, перед типами — read-only Type Scaffolder. Облако явно недоступно, помощник остановлен; никакого наблюдения, голоса, предложений, фиктивного провайдера или Node runtime. Проверяются реальная запись/повторное открытие, ошибки, удаление и пауза через native boundary. После проверки основы можно начать независимую память 07; остальные критерии 06 остаются открытыми.

**Status:** in-progress — независимая основа D56 реализована и проверена; полная 06 остаётся открытой и зависит от 01/03 и интеграции ниже.

Пакет06D принят локально:108 тестов, app build/signature и review прошли. Подробная приёмка — в разделе «Независимый пакет 06D» ниже; полный06 остаётся открытым.

## Независимый пакет06F: помощь по выбранному сохранённому делу

**Спецификация:** [ADR-010](../../../decisions/0010-selected-task-assistance.md), Plan Simplicity Review/Evidence Audit приняты. **Blocked by локального начала:** принятые07B/06D/06E. Capture03 не нужен для уже введённого ручного текста; полный06 сохраняет01/03. **Status:** принято локально19 октября2026; тред01a15391-9ec8-71a2-bf74-74d06f062a15. Один владелец общей Core/ABI/providers/native границы, GPT-6.1 Sol xhigh.

- [x] Существующее окно даёт выбрать сохранённое дело/до32 ручных заметок, просмотреть точный снимок намерения и полного плана, включая checks/dependencies и до32 useful references, и отправить его одной явной кнопкой. Нет других дел, внешних источников или незаметной замены stale context.
- [x] Настоящий путь Swift–C–Rust → bounded Responses → проверенное предложение/уточнение/silence → Core receipt → точное копирование. Вывод не записывается как факт, не завершает шаг и не запускает действие во внешнем приложении.
- [x] Явная Low/High диагностика использует общий транспорт/ledger. Только успешный High Fast текущего process-local epoch после accounting commit открывает передачу рабочего текста. Нет готовности в Keychain/SQLite, нового credential owner или обхода полной остановки.
- [x] Согласованное чтение Core, revision/происхождение/полный план, пределы256KiB context/320KiB body и одноразовый consent проверяются до POST; очереди нет. Неполный план, включая Partial+0, не даёт ложный готовый шаг.
- [x] SSE механика/usage общие; purpose-specific Low check сохраняет256bytes, High помощь допускает32KiB и strict JSON форму/цитаты. Refusal/incomplete/ошибки/config mismatch не выдаются за silence/Fast. Нет повторов и fallback.
- [x] Model ABI2/schema5 реализованы и документированы, memory3/auth1 сохранены. Один журнал1024rows/1MiB, durable unknown до отправки; usage после отмены отдельно. Schema4/WAL отвергается без записи/миграции/сброса.
- [x] Поздний ответ, pause/logout/report withdrawal/смена данных/epoch не публикует старую помощь. Copy запрашивает актуальный текст у Core по receipt, не использует старую UI строку. Busy до actual cleanup, поколение проверяется после close, самостоятельный редактор сохраняется.
- [x] Mandatory specialists, meaningful TDD, Code Simplicity Review, EN/RU, полный verify/build/signatures и сохранность63 файлов семи checkpoints подтверждены. Собственные native consent/Fast, новая provider schema, UI, macOS14 hardware/Windows отдельно и честно непроверены.

Локальный результат19 октября2026: `yarn verify` — exit0,8 tooling/83 Rust/48 Swift; отдельная test-support Swift–C–Rust/HTTP/SSE сборка — exit0,8 тестов (7 дополнительных), включая восемь режимов ответа/ошибок. `yarn build:macos -quiet` и строгая подпись app — exit0. В каждом из трёх executable Mach-O images отсутствуют `ambient_model_test_dispatch` и прежние model v1 symbols; все11 v2 exports присутствуют в Debug dylib. Все63 файла семи checkpoints совпали по полному inventory/SHA256 и прошли codesign после сборки. Новая контрольная сборка не создавалась.

Type Scaffolder работал только на чтение; важные типы/логика написаны исполнителем. Endpoint Registry Mechanic добавил High-константы, Localization Mechanic обновил EN/RU. Scout/Librarian/Researcher переиспользованы по актуальным принятым выводам ADR. Code Simplicity Review и целевые повторы — PASS после исправлений commit/cancel, конечного ABI outcome и сообщения об отсутствии результата; refresh UI и закрытие неиспользованного preview проверены RED→GREEN. Старый тест80ms мог зависнуть в `join/accept`; стек подтвердил причину, test-only ожидание ограничено и запрос к потоку теперь проверяется явно. Production60s не менялся. Полный verify повторён после исправления; Swift cleanup regression сохранён и прошёл за65s.

Во время реализации app/UI, native consent/TCC, ServiceManagement registration, live model/token POST и Jev не запускались. Новая SIWC схема/настоящий High Fast, собственные native consent/no-credit, полезность модели, macOS14 hardware и Windows остаются непроверенными. Пользовательские БД не открывались; schema4/WAL отказ проверен только на временной SQLite, без миграции/сброса. Полные06/08 и G5 не приняты.

## Независимый пакет06E: управление автозапуском

Спецификация: [ADR-009](../../../decisions/0009-login-item-settings.md), Plan Simplicity Review/Evidence Audit PASS. Реализован после принятого06D; один новый владелец native Settings, Core/ABI/providers не меняются. Полный06/D28 и интеграция первого включения остаются открытыми.

- [x] Settings показывает настоящий SMAppService.mainApp.status и явные enable/disable/open System Settings; системный отказ и requiresApproval не становятся enabled.
- [x] Снимок обновляется при показе/возврате/операции; нет своего постоянного флага, polling, helper/plist или повторного register на старте. Пауза и анализ не меняются.
- [x] Реальные API закреплены в production, test-only seam проверяет ошибки/отзыв/отсутствие автоматических действий. EN/RU обновлены вместе.
- [x] Meaningful TDD, code simplicity review, полный verify/build/signature и сохранность шести checkpoints подтверждены.
- [x] Фактическая регистрация/login/UI не выполнялись и не выданы за проверенные; системные настройки пользователя не менялись.

Проверка06E, 2026-10-19: восемь новых Swift tests прошли RED → GREEN. Они проверяют initial/повторный refresh без регистрации, внешний отзыв, ровно enable/disable с последующим системным чтением, requiresApproval, ошибки/явный retry, notFound/будущий неизвестный статус, открытие System Settings без register, сохранённую паузу и очистку ошибки только явным действием. Production использует настоящие API; register/unregister/open System Settings во время разработки не выполнялись.

Code Simplicity Review: одно замечание об очистке старой ошибки кнопкой refresh исправлено с отдельным RED → GREEN; узкая повторная проверка — PASS. Localization Mechanic добавил14 ключей EN/RU; принятые Scout/Librarian/Researcher переиспользованы после проверки актуальных Settings/Package/tests и SDK27 header. Не-UI domain/transport/FFI declarations не менялись, Type Scaffolder не требовался.

Окончательный `yarn verify` с изолированным Rust — exit0:8 tooling/62 Rust/46 Swift, включая прежнюю native cleanup regression64,7s. `yarn build:macos -quiet` и `codesign --verify --deep --strict` текущего app — exit0. Все54 файла шести checkpoints побайтово совпали с baseline; их подписи тоже прошли. Xcode сообщил прежние системные CoreDevice/CoreSimulator diagnostics; macOS build успешен, compiler/linker warnings о deployment target нет. SwiftPM потребовал обычный разрешённый запуск вне файловой песочницы; её запрет не обходился.

Фактическая регистрация с ad-hoc подписью, следующий macOS login, UI, macOS14 hardware и Windows не проверялись. Первое автоматическое включение после полноценной настройки, рабочий анализ и полные D28/06 остаются открытыми. Неиспользуемый hook не добавлен; общие продуктовые документы синхронизирует координатор.

**Проверенная независимая часть, 2026-10-19:**

- [x] Настоящий Xcode app target со SwiftUI-окнами памяти/настроек и ресурсами EN/RU. Собран `target/macos/Build/Products/Debug/Ambient.app`: arm64, macOS 14, development bundle ID `dev.ambient.foundation`, ad hoc подпись проверена.
- [x] Ручной ввод → Swift actor → C ABI v1 → Rust/SQLite → снимок для отображения. Read/save/edit/delete и повторное открытие проверены настоящими native integration tests, без production doubles.
- [x] `NeverSet` / `Present {text, revision}` / `Deleted {revision}`; удалённая отметка не содержит текста. Ревизия растёт атомарно, пауза хранится отдельно и переживает повторное открытие. Ошибки/чужая или повреждённая схема не становятся пустой памятью и не сбрасывают БД.
- [x] Облако явно недоступно, помощник остановлен; снятие ручной паузы не меняет это состояние. Наблюдение, голос, таймеры, предложения, провайдеры и Node runtime не добавлены. Независимый черновик сохраняется при ошибке и повторном чтении, включая пустой ввод и разные точные UTF-8 записи одного символа.
- [x] `yarn verify`: agent check, 8 tooling tests, Rust check/rustfmt/Clippy и 15 Rust tests, Swift build/strict format и 8 Swift behavior/integration tests. `yarn build:macos -quiet` и `codesign --verify --deep --strict` успешны. Code Simplicity Reviewer: единственное замечание о сравнении точных UTF-8 байтов исправлено, целевая повторная проверка замечаний не выявила.

Сборка использовала официальный Rust 1.99.1 в изолированном `target/` с проверенным загрузчиком; глобальный Rust/PATH не менялись. Инструкции воспроизведения и контракт: `src/platforms/macos/README.md`, `src/bridge/README.md`, `src/core/README.md`. Xcode сообщил о несовместимых системных CoreDevice/CoreSimulator, но macOS build завершился успешно; compiler/linker warnings о повышении минимальной macOS нет. Визуальный/ручной UI-прогон, запуск на устройстве с macOS 14 и Windows CI не выполнялись.

Полные критерии 06 ниже остаются открытыми. Нужны собственный подписочный вход/продление и Luna Fast Low/High, разрешённое наблюдение по 03, настоящее облачное решение и карточка, бюджет/квота, поздние результаты/потери событий, автозапуск и восстановление, полный реальный сценарий и синхронизация общих продуктовых документов. Эта основа сама по себе не подтверждает G5 или полную G7.

- [ ] Использованы подтверждённые результаты 01 и 03. Запускается настоящее macOS-приложение, а не только библиотека; Rust владеет памятью, правилами, бюджетом и допуском, оболочка — UI, ОС и ключами. G7 подтверждён для этого пути.
- [ ] Работают явное включение, выбор разрешённой области/облачной передачи и исключения для затронутых источников. Собственное подписочное подключение Ambient по D52 подтверждено отдельно от CLI-пробы; секреты хранятся системно. Отказ/отзыв прав и неизвестная область не превращаются в разрешённый контент.
- [ ] По D54 используется GPT-6 Luna Fast: Low для быстрых задач, High для обдумывания. Доступность модели и фактический Fast подтверждены для подписочного подключения; запрошенные параметры отличаются в учёте от возвращённых. Sol и автоматическая замена запрещены; отказ или неизвестный режим явно останавливают выбранный путь, а не маскируются успешным Fast.
- [ ] По D55 собственный вход переживает перезапуск без повторной браузерной авторизации. Токены находятся в системном хранилище; продление последовательно сохраняет проверенную личность. Отзыв, потеря ответа при продлении и отказ хранилища обработаны без утечки секретов, повторного использования неопределённого токена и обхода ограничений расходов.
- [ ] Намерение и необходимый контекст одного дела переживают смену приложения и перезапуск. Есть удаление содержания и защита от возврата позднего результата; снимки очищаются после обработки/ошибки/отмены, скрытого архива и личных логов нет.
- [ ] Настоящий облачный вывод приводит к предложению, уточнению или молчанию. Видны источник/основание и непроверенность; текст источника или модели не получает полномочий. Нет обработчика специально под демонстрационный сценарий.
- [ ] Карточка появляется без ожидания паузы в работе и не перехватывает фокус; пользователь может ответить текстом и скопировать предложенное. Копирование не означает запуск или завершение, внешние данные не изменяются.
- [ ] В подписочном подключении проверены авторизация, доступная модель, ограничения поставщика, отсутствие дополнительных списаний и платного fallback; включённая квота не подменяется вымышленным долларовым резервом. Для платных попыток сохраняются месячный предел, резерв до отправки, конкурентные вызовы, повторы, предупреждение при 80%, сверка usage и неизвестный расход через перезапуск. По D53 проверки реального приложения имеют отдельный совокупный потолок Jev $1,50 и предупреждение $1,20; предварительный $1 и личные $2,50 им недоступны. До каждой платной проверки фиксируются её необходимость и верхний резерв, после — фактический или неизвестный расход. Эти суммы не становятся месячным бюджетом продукта; единый владелец допуска и учёта — Rust.
- [ ] Индикатор и полная остановка работают при ручной паузе, недоступности облака, подписки/квоты или бюджета платного подключения. Новые чтения/отправки запрещены, локальная работа реально прекращается, поздний ответ не создаёт действие; готовый черновик сохраняется.
- [ ] Автозапуск после первого включения и сохранение ручной паузы проверены. Восстановление облака/бюджета возобновляет работу лишь без ручной паузы и после проверки свежего контекста; ограниченного офлайн-помощника нет.
- [ ] Полный путь проверен на настоящем источнике и модели, а правила — воспроизводимо на границе наблюдение → память → предложение/эффект. Короткие события не теряются молча, очереди ограничены числом и байтами.
- [ ] Обновлены затронутые продуктовые/платформенные документы и выполнены применимые обязательные проверки. Расширения нескольких дел, полной ленты, голоса, подключений и подсветки оставлены своим согласованным задачам.

## Независимый пакет 06B: нативный вход и сохранение сессии

**Контракт:** [ADR-006](../../../decisions/0006-native-chatgpt-session.md), проверен Plan Simplicity Reviewer и Evidence Auditor после исправлений. Начало после приёмки 07B только из-за общей границы Rust–C–Swift. Fast/G5 и разрешения чтения экрана не блокируют этот пакет. Полные критерии выше остаются открытыми.

**Результат:** реальный Continue with ChatGPT в настройках, собственная регистрация/проверка личности, сохранение одной сессии в Keychain, повторный запуск без браузера, явные refresh/cancel/local sign-out. Сохранённый вход не включает анализ; model POST/Responses, Jev и чтения рабочего контекста здесь нет.

**Владелец:** один GPT-6.1 Sol xhigh для `src/providers/**`, auth-части `src/bridge/**`, `src/platforms/macos/**`, необходимых Cargo/build/tests и этого билета. Существующие память/план/schema3/memory ABI3 сохраняются. Общие docs и agent-sources обновляет координатор; cloud-стенд и checkpoints не изменять. Важные протокол/credentials/ABI/lifecycle пишет Sol; Type Scaffolder только на чтение.

- [x] Rust OIDC/PKCE с openidconnect 4.0.1 и ограниченным reqwest 0.12.28 transport; точные проверенные endpoints, без redirect/retry/собственного JWT parser. Реальные HTTP тесты доказывают пределы/таймауты и максимум один token POST.
- [x] Native IPv4 loopback listener и CFHTTPMessage: ограниченные bytes/connections/deadlines, точный callback/state, закрытие всех соединений, отдельный auth executor вне UI и памяти. Фактическое завершение отличается от запроса отмены.
- [x] `ext_agent_host_id` одинаков в initial/returning. Выданный client ID сохраняется до обмена; ошибки/неизвестная регистрация/права не превращаются в ready. Проверяется подпись/issuer/aud/azp/expiry/nonce/sub и выданные scopes.
- [x] Один generic-password Keychain envelope: service `dev.ambient.foundation.chatgpt`, account `installation`, host внутри, без синхронизации. Постоянный адрес обеспечивает поиск после перезапуска; ошибки/повреждение не создают новую регистрацию. Собственные test-only fixtures, без чтения Codex/стенда/пользовательских записей.
- [x] Refresh prepare сохраняет старую запись при ошибке; перед token POST атомарно удаляются прежние credentials из envelope. Новый набор сохраняется вместе, старый неизвестный rotating token не повторяется. Logout/TTL/cancel блокируют поздний commit, занятый handle живёт до настоящего завершения.
- [x] Auth ABI v1 отдельна от memory ABI3: typed spans/caller buffers, проверка вместимости до одноразового POST, никаких secrets/raw SDK errors в диагностике. Native UI EN/RU отличает сохранённый вход, истёкшую сессию и остановленный анализ.
- [x] Type Scaffolder read-only, Localization Mechanic, Endpoint Registry Mechanic и Code Simplicity Review выполнены; TDD по ADR, `yarn verify`, macOS build/codesign и сохранность трёх checkpoints подтверждены.

**Проверенная реализация 06B, 2026-10-19:** Rust crate `ambient-providers`, отдельная auth ABI1 и нативные `ChatGPTCoordinator/Bridge/Loopback/Keychain/Model` подключены к настоящим настройкам приложения EN/RU. Системный CFHTTPMessage parser, реальный IPv4 listener и собственный временный Keychain проверены программно. Подписывающий тестовый IdP выполняет discovery/JWKS/exchange через настоящий локальный HTTP; другие тесты проверяют claims/grants, таймауты, пределы, отсутствие retry/redirect, DNS cleanup, сохранение до POST, refresh unknown, отмену/TTL/выход и фактическое завершение. App delegate ждёт память и auth shutdown. Сохранённый вход не включает анализ.

`yarn verify` — exit 0: 8 tooling, 46 Rust, 27 Swift tests; agent generation check, Rust check/rustfmt/Clippy и строгий Swift format прошли. `yarn build:macos -quiet` — exit 0 после исправления видимости startup-read метода между модулями; новая app arm64, bundle ID `dev.ambient.foundation`, minimumOS 14.0 в plist и Mach-O. `codesign --verify --deep --strict` — exit 0 для новой app и всех checkpoints 06/07A/07B. Xcode выдал известные системные CoreDevice/CoreSimulator предупреждения; minimumOS не повышен. Изолированный C fixture сохраняет видимыми два предупреждения SDK о deprecated SecKeychainCreate/Delete; production использует SecItem, диагностики не отключались.

Type Scaffolder read-only, Endpoint Registry Mechanic, Localization Mechanic и Code Simplicity Reviewer выполнены. Два замечания review исправлены: UI выходит из busy при ошибке Keychain без подмены ошибки пустым входом; ответ браузеру ждёт send completion до закрытия. Целевая повторная проверка замечаний не выявила. Узкие Librarian/Researcher проверки подтвердили transport correction ADR006 и выбор file-based SecItem Keychain для текущей ad hoc подписи: Data Protection access groups требуют отсутствующих provisioning entitlements. Один fixed service/account, без синхронизации и без ослабления системного ACL; после пересборки ad hoc система может снова спросить доступ.

Новых model/Responses/Jev вызовов, capture, рабочего контекста и Node runtime нет. Пользовательские credentials и БД не использовались; приложение визуально не запускалось, checkpoints не изменялись. Общие продуктовые/архитектурные документы синхронизирует координатор по этим результатам. Полная 06, Fast/G5 и пользовательский native consent/refresh остаются открытыми.

Отдельно отметить непроверенные native consent/пользовательский Keychain ACL/реальный refresh, visual UI, macOS14 hardware и Windows. Изолированный временный Keychain и локальный тестовый IdP не считаются настоящим входом аккаунта. Если возникнет блокер, сначала сообщить координатору своим final и дождаться ответа; не обходить права и не заменять механизм производственной имитацией.


## Независимый пакет 06C: меню, пауза и локальная проверка прав

**Контракт:** [ADR-007](../../../decisions/0007-menu-bar-and-local-permissions.md). Спецификация прошла Plan Simplicity Review и Evidence Audit; условие аудита выполнено:06B принят, координатор прочитал actual app shutdown и финальные команды. Общая native граница освобождена. Fast и TCC-права не нужны для реализации этой части; полного анализа пока нет.

**Результат:** постоянный MenuBarExtra, пауза без загрузки содержания дел, открытие памяти/настроек, штатный выход. Явная кнопка проверяет Screen Capture/Accessibility без запроса прав и чтения контента.

**Владелец:** один GPT-6.1 Sol xhigh, `src/platforms/macos/**`, необходимые native build/test файлы, module README и этот раздел. Core/schema3/memory ABI3/auth ABI1/provider protocol не менять. Shared docs, ADR и agent-sources у координатора. Checkpoints06/07A/07B/06B защищены.

- [x] MenuBarExtra и существующие окна используют один delegate.model; доступные текстовые состояния различают паузу, неизвестную паузу и остановленный анализ. Сохранённый вход не показывает Running.
- [x] Пауза читается/сохраняется через прежний MemorySession/ABI без загрузки страниц памяти; одна Bool? проекция, поле удалено из MemorySnapshot. Ошибка не становится ложным false/успехом и не стирает черновик.
- [x] Начальная загрузка меню и окна присоединяется к одному принятому чтению паузы; window load не теряется из-за busy. Параметры reload/замены черновика вычисляются после await. Единый tracked lifecycle ждёт принятую работу при закрытии и не публикует поздний результат.
- [x] Pause command одинакова в меню/окне/настройках, отключена при unknown/busy/closing. Open memory сохраняет выбранное дело, страницы и независимый черновик. Обычный Quit сохраняет проверенный memory+auth shutdown06B.
- [x] Permission preflight запускается только кнопкой; до неё unchecked. CGPreflightScreenCaptureAccess и AXIsProcessTrustedWithOptions(prompt=false) дают granted/notgranted без выдуманных причин. Нет запросов прав, чтений содержимого, Keychain, polling или сохранения результатов на диск.
- [x] EN/RU через Localization Mechanic, non-UI declarations через read-only Type Scaffolder, применимые Scout/Librarian findings приняты. Code Simplicity Review готовой логики и meaningful TDD выполнены.
- [x] Полный yarn verify, native build и codesign текущей app/четырёх checkpoints прошли; результаты и ограничения записаны. Нет model/Jev/token POST, browser consent, capture, нового ABI/БД/admission, production mocks или visual app запуска.

Полная06 остаётся открытой. Native visual/menu/focus/Spaces, настоящий consent/refresh, macOS14 hardware и Windows не считать проверенными по сборке. Вопрос/блокер передать координатору final QUESTION/BLOCKED и дождаться ответа.


**Реализовано и проверено 2026-10-19:** MenuBarExtra использует тот же delegate.model; отдельная Bool? пауза больше не входит в MemorySnapshot и не перечитывает содержание. Начальное чтение из applicationDidFinishLaunching разделяется с окном/Settings. Принятая операция включает применение результата и освобождение busy; close ждёт её без поздней публикации. Reload принимает решение о замене черновика после ожидания. Штатный memory+auth shutdown06B сохранён.

Preflight остаётся явным prompt=false. SDK27 объявляет kAXTrustedCheckOptionPrompt изменяемой C-глобальной переменной, которую strict Swift6 не разрешает читать даже на MainActor. Локальный `CLocalPermissions` даёт только static-inline accessor заимствованного ключа с ownership-аннотацией; словарь с false и AXIsProcessTrustedWithOptions остаются в Swift. Type Scaffolder проверил тип/владение, compiler подтвердил импорт CFString. Новый экспортируемый ABI не введён: nm проверен на всех трёх executable Mach-O images app, включая Debug dylib; нативные preflight imports присутствуют. Core/schema3/memory ABI3/auth ABI1 не менялись. Nil-options не принят: Researcher проверил первичные Apple docs/JSON, гарантии nil=prompt=false там нет.

Scout refreshed актуальные места06B; Librarian подтвердил штатные API и ограничение импорта SDK; read-only Type Scaffolder проверил удаление поля/ABI-reader и C-accessor; Localization Mechanic обновил EN/RU (14 добавленных, 2 удалённых устаревших ключа, по179 ключей); Code Simplicity Reviewer готовой логики — без замечаний. Generic workers не использовались. TDD: до правки подтверждён ожидаемый провал сохранности snapshot при старом toggle; затем6 новых тестов (close имеет2 параметрических случая) прошли на реальной временной SQLite. Временный test-only gate проверяет совместный старт, busy/closing и ожидание принятой операции; permission tests используют локальные значения, без TCC-вызова.

Проверки с изолированным Rust по заданной команде: `yarn verify` exit0 — 8 tooling / 46 Rust / 33 Swift, включая строгий Swift lint; `yarn build:macos -quiet` exit0. Xcode выдал системные предупреждения CoreDevice/CoreSimulator; macOS-сборка завершилась успешно. `codesign --verify --deep --strict` текущей app и четырёх checkpoints06/07A/07B/06B — все exit0. Все36 файлов checkpoints побайтово совпадают с начальной проверкой. Первый sandboxed SwiftPM запуск был блокирован sandbox-exec; обычный узкий require_escalated разрешён и дальнейшие тесты/сборка прошли. Проверка символов сначала смотрела только launcher и не нашла native import; повтор на всех executable Mach-O images подтвердил оба native API и отсутствие экспортируемого accessor.

Реальный аккаунт/consent/refresh/пользовательский Keychain ACL, menu/focus/Spaces visual UI, macOS14 hardware и Windows не проверялись. Приложение не запускалось; модель, Jev, удалённые token POST, browser consent, capture и запросы TCC отсутствовали. Полный06 остаётся открытым.

## Независимый пакет 06D: нативная проверка Luna Low Fast

**Контракт:** [ADR-008](../../../decisions/0008-native-chatgpt-connection-check.md), локальная реализация после принятых06B/06C/07B. Это только управление проверкой собственного подключения: полный06/G5, источники и работающий анализ остаются открытыми. Целевые subscription tests разрешены D57, но собственный native consent/no-credit ещё не подтверждены; разработка этого пакета выполнена без live POST.

**Владелец:** один GPT-6.1 Sol xhigh, Core/providers/bridge/macOS и module README. Важные типы и логику написал исполнитель; Type Scaffolder работал только на чтение. Shared docs/ADRs/status синхронизирует координатор. Historical evidence, стенд01, пять checkpoints, прочие платформы и agent rules не изменялись.

- [x] Реальный production путь Settings → существующий ChatGPTCoordinator → отдельная model ABI1 → Rust Responses transport → тот же Store/SQLite → native результат. Фиксированный синтетический Luna/low/priority запрос; нет рабочих данных, org/project headers, automatic dispatch/refresh/retry/fallback или Jev. Auth allowlist и прежние memory ABI3/auth ABI1 layouts сохранены.
- [x] Once-per-process user_reported сообщение о проверенном запрете дополнительных списаний принадлежит существующему coordinator, без новых Keychain/SQLite полей или копии identity. Новый вход/выход/перезапуск сбрасывают; refresh сохраняет только ту же проверенную привязку. UI явно не выдаёт сообщение за API-подтверждение и ведёт в официальный ChatGPT UI без обещания deep link.
- [x] Core владеет одним non-clone допуском. Пауза/отмена/закрытие запрещают dispatch; commit possible-send unknown происходит до HTTP. Отказ записи не отправляет POST. Отдельная последующая попытка не стирает старые unknown. Схема4 read-only отвергает прежнюю3 без миграции/сброса; schema3 и её WAL проверены побайтово.
- [x] Журнал хранит только fixed integer-поля, не более1024 строк и1MiB с консервативным начислением128bytes/строку. Агрегаты checked; ошибочный/отсутствующий usage не становится нулём, переполнение/ошибка итогового commit оставляют durable unknown. Reopen переводит незавершённые записи в interrupted/unknown без повторной отправки.
- [x] Узкий reqwest/Tokio transport использует Response.chunk, connect5s/read10s/total60s, bounded SSE2MiB/frame512KiB/4096events/8items64KiB/text256bytes/error16KiB. Проверены CR/LF/CRLF, multiline/UTF-8, empty terminal reconstruction, непрерывные уникальные items, unfinished deltas, malformed/duplicate/conflicting/failed/quota/EOF. Requested и actual разделены; default/absent/other не подтверждают Fast. Валидный usage сохраняется отдельно от функционального исхода.
- [x] Registry locks освобождаются до Store lock/HTTP. Тот же Store lock сериализует паузу и durable dispatch acceptance. Model operation удерживает Arc lease для accounting после memory close; busy живёт до настоящего Rust/runtime cleanup. Native generation fence запрещает старый успех после cancel/sign-out/shutdown. Модельный срок принадлежит Rust; auth-expiration не перекрывает timeout после долгой очистки.
- [x] EN/RU обновлены Localization Mechanic:40 новых ключей, parity/plutil PASS. UI показывает последний outcome, requested/actual, известные input/output/total и unknown попытки native журнала. Чтение журнала имеет отдельный Result; успешное перечитывание убирает read error, сохраняя отдельную ошибку записи выполненной попытки.
- [x] Scout, Librarian, Type Scaffolder read-only, Endpoint Registry Mechanic, Localization Mechanic и Code Simplicity Reviewer вызваны. Researcher evidence переиспользован из принятого ADR/планирования; новых внешних решений не требовалось. Два замечания review воспроизведены RED и исправлены; focused повторное review — без замечаний. Generic workers и Visual Inspector не использовались.

**Проверки и границы, 2026-10-19:** полный повторный `yarn verify` — exit0:8tooling/62Rust/38Swift; agent generation, Rust check/rustfmt/Clippy и строгий Swift format прошли. Добавлены4Core/2bridge/10provider/5Swift тестов, прежние memory/auth/pause проверки сохранены. Реальные временные SQLite и локальный HTTP не считаются настоящим provider/account. Native Swift–C–Rust проверяет ABI/паузы/отмену, coordinator seams — sign-out/shutdown/поколение и настоящий61-секундный cleanup; это не UI-прогон и не собственный native consent.

Настоящие native consent/no-credit настройка/пользовательский Keychain ACL/refresh, live модель/Fast, visual UI, macOS14 hardware и Windows не проверены. Сохранённый вход и даже успешный синтетический check не включают анализ. Стенд остаётся7POST/1021known tokens/3unknown; Jev0. Для первого настоящего native запроса отдельно нужны подтверждённый собственный вход, пользовательская проверка no-credit и явный запуск через проверенные условия допуска. Повторно запрашивать общее разрешение на уже разрешённые D57 тесты не требуется.


Обе итоговые команды выполнены из корня с окружением:

```sh
RUSTUP_HOME=$PWD/target/toolchains/rustup CARGO_HOME=$PWD/target/cargo-home PATH=$PWD/target/cargo-home/bin:$PATH MACOSX_DEPLOYMENT_TARGET=14.0 yarn verify
RUSTUP_HOME=$PWD/target/toolchains/rustup CARGO_HOME=$PWD/target/cargo-home PATH=$PWD/target/cargo-home/bin:$PATH MACOSX_DEPLOYMENT_TARGET=14.0 yarn build:macos -quiet
codesign --verify --deep --strict target/macos/Build/Products/Debug/Ambient.app
```

Все exit0. Финальные логи: `/tmp/ambient-06d-verify.log`, `/tmp/ambient-06d-build.log`. Current app имеет bundleID `dev.ambient.foundation`; Info.plist и все3 executable Mach-O images сохраняют minimumOS14.0. Все5 model ABI symbols присутствуют в app Debug dylib. В собранных EN/RU ресурсах по219 ключей, включая40 новых check keys. Codesign пяти checkpoints06/07A/07B/06B/06C — exit0; полный набор45 файлов совпадает побайтово с parent baseline `/tmp/ambient-06d-checkpoints.json`, включая отсутствие новых файлов.

TDD RED→GREEN: schema4 тест сначала получил3; новые APIs отсутствовали; отдельный unfinished delta тест сначала ошибочно получал Confirmed; после исправлений они проходят. Review-регрессии сначала получили stale storage error и `.expired` после64.97s cleanup; в полном финальном verify оба проходят, timeout сохраняется после65s. Провал Clippy в test-server loop исправлен штатным while-let. Sandboxed SwiftPM упал на sandbox_apply; обычные узкие require_escalated были разрешены, проверки не обходили sandbox. Xcode сообщил прежние системные CoreDevice/CoreSimulator предупреждения, macOS build exit0. Первая команда inventory ошиблась при вычислении корня checkpoint; после исправления selector полный SHA256/inventory/codesign прогон прошёл и повторён после финальной сборки.

Локальных блокеров нет. Общие product/privacy/contracts/architecture/status, ADR и README корня/src требуют синхронизации координатором: schema4/attempts, отдельная model ABI1, bounded synthetic Responses путь, user_reported допуск, journal accounting/unknown, serialized shutdown и точные локальные результаты выше. Полный06/G5 остаётся открытым.
