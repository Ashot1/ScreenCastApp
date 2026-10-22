# Задачи первой личной версии Ambient

Последняя приёмка: [Todoist 17A](17a-selected-todoist-task.md) на общем движке; до неё [24B](24b-plugins-oauth-google.md) (локально), [24C](24c-local-plugin-servers.md), [24A](24a-plugins-mechanism.md). Verify 8 tooling/358 Rust/91 Swift, feature Swift 139, 22 контрольные сборки. По слову пользователя (D80) идёт новый интерфейс [25A–25H](25a-platform-and-design-layer.md): [25A](25a-platform-and-design-layer.md) принята 21 октября (пробы R1–R4, macOS 26, слой оформления; verify 358 Rust/92 Swift, 23 контрольные сборки), [25B](25b-main-window-shell.md) принята 21 октября (главное окно со стеклянной боковой панелью; verify 358 Rust/93 Swift, 24 контрольные сборки) — первая точка просмотра для пользователя; [25C](25c-case-tabs-and-plan.md) принята 21 октября (вкладки дела, план, отмена чтений в модели; 25 контрольных сборок); [25D](25d-step-page-and-editors.md) принята 21 октября (страница шага, страницы редакторов, D81; 26 контрольных сборок); [25E](25e-feed-and-edge.md) принята 21 октября (сворачиваемая лента, стеклянная панель у края; 27 контрольных сборок) — вторая точка просмотра; [25F](25f-notes-links-sources-help.md) принята 21 октября (вкладки заметок, связей, источников и помощи; 28 контрольных сборок); [25G](25g-settings.md) принята 21 октября (настройки с боковой панелью; 29 контрольных сборок); [25H](25h-menu-bar-and-quit.md) принята 22 октября (строка меню и выход; 30 контрольных сборок) — новый интерфейс 25A–25H завершён, финальная точка просмотра у пользователя; следующий фронт (D82) — спецификация наблюдения за работой и проактивных предложений после просмотра интерфейса; ранее — [24B](24b-plugins-oauth-google.md) по [ADR-021](../../../decisions/0021-plugins-layer.md) и [ADR-022](../../../decisions/0022-local-mcp-servers.md), пересмотр [Todoist17A](17a-selected-todoist-task.md) на общем движке принят 21 октября (реализация после 24A). Ранее: Старый контракт17A прошёл два обзора, но теперь заблокирован до нового общего контракта и приёмки; исполнитель не создан. Контракт общего слоя — [ADR-020](../../../decisions/0020-integration-layer-contract.md), оба обзора PASS; задачи [23A](23a-integration-engine-jira.md) → [23B](23b-github-on-integration-engine.md) → [23C](23c-integration-switches.md) выполняются последовательно одним владельцем общей границы. Управляет оркестратор, исполнители — сессии Claude Code (D59). Полные08/10/11/12/13/14 остаются открытыми.

22 задачи созданы по согласованной разбивке 2026-10-18; порядок разработки пересмотрен по прямому поручению пользователя 2026-10-19 (D56/D57). Основа — [спецификация](../spec.md) и [журнал решений](../decisions.md), включая D51.

Уточнения D52/D53: облачная проверка адаптируется к подписке ChatGPT. Jev: $1 на предварительные проверки, $1,50 на проверки реального приложения во время/после разработки, $2,50 на личное использование. Пределы раздельные, без переноса денег между этапами; текущий стенд допускает только первый доллар. Эти изменения не подтверждают зависимости сами по себе; доступная без ключа Jev и прав macOS подготовка продолжается в 01/03/05.

В каждом файле есть результат, блокирующие задачи и критерии приёмки. Статус `ready-for-agent` означает готовность описания к передаче исполнителю, а не выполнение зависимостей или начало реализации. Фактические состояния и созданные треды собраны в [журнале выполнения](../execution.md); критерии отмечаются только по проверенным результатам.

По D54 модель анализа внутри приложения — GPT-6 Luna Fast: Low для быстрых задач, High для обдумывания, без Sol и автоматической подмены. Это не выбор модели разработчика: пользователь поручил выполнять волны разработки в тредах **GPT-6.1 Sol** с подходящей глубиной рассуждений. Fast через подписочный вход ещё требует настоящего подтверждения. Jev закреплён на актуальном `jev-1.14.0` со зрением/Box; настройки допуска и бюджет не меняются.

По D55 собственный вход ChatGPT сохраняется между запусками через системное хранилище и штатное продление сессии. Эта доработка относится к 01 и будущему нативному подключению 06; её реализация и проверка не предполагаются выполненными по самому факту согласования.

Начальный фронт теперь включает **основу 06 одновременно с 01 и доступными проверками 03/05**. 02 и 04 уже имеют свои проверенные результаты. Ожидания окончания всей волны нет: открываем следующую независимую часть после проверки используемого результата. Окончание треда с BLOCKED не подтверждает зависимость.

## Волны запуска агентов

Таблица показывает фронты разработки. Полная приёмка функций сохраняет реальные облачные, системные и бюджетные проверки. Каждая сессия получает конкретную часть и одного владельца; менять общий Rust/ABI/Swift-контракт одновременно нельзя.

| Фронт | Можно запускать параллельно | Что должно быть проверено |
| --- | --- | --- |
| **1 — основа и доступы** | [01 — облачное подключение](01-cloud-analysis-feasibility.md), доступные проверки [03](03-native-content-feasibility.md)/[05](05-jev-localization-feasibility.md). Независимая [основа06](06-first-working-assistant.md) уже принята | Основа не ждёт облака/прав: настоящий ввод → Rust → память, удаление и пауза проверены. Повторно реализовывать её не нужно |
| **2 — после основы** | Независимая память [07](07-task-memory-and-links.md); независимая интеграция [10](10-voice-reply.md), когда граница ответа/остановки стабильна | Проверен путь памяти 06; для 10 также 04. У частей должны быть непересекающиеся владельцы. Голос и предложения не работают в остановленном приложении |
| **3 — после контрактов памяти** | Независимые части [08](08-suggestion-feed-and-panel.md), [09](09-unfinished-work-reminders.md), источников [11](11-local-projects-and-files.md)–[19](19-apple-reminders-source.md) | Используемая память и контракт источника 07 проверены; для [14](14-codex-source.md) также 02. Реальное подключение каждого источника требует его доступа и проверки G6 |
| **Собственный вход06B — принят** | [06B по ADR-006](../../../decisions/0006-native-chatgpt-session.md) после освобождения native границы07B; облачная диагностика01 отдельно | Локальные OIDC/Keychain/refresh/cancel/shutdown проверки пройдены. Настоящий native consent/refresh проверяются отдельно; Fast/TCC не блокировали этот пакет |
| **Управление06C — принято** | [06C по ADR-007](../../../decisions/0007-menu-bar-and-local-permissions.md) после приёмки06B; облачная диагностика01 отдельно | Меню, пауза без загрузки дел, local no-prompt preflight и сохранённый shutdown06B проверены. Model/TCC вызовов не было; анализ остаётся остановленным |
| **Проверка подключения06D — принята** | [06D по ADR-008](../../../decisions/0008-native-chatgpt-connection-check.md) после06B/06C/07B, один владелец Core/ABI/providers/native | Настоящий синтетический Responses путь и durable учёт проверены локально без нового live POST. Fast/TCC/Jev не блокируют реализацию; источники и полный06 этим не включаются |
| **Управление автозапуском06E — принято** | [06E по ADR-009](../../../decisions/0009-login-item-settings.md) после06D, один владелец Settings | Настоящие SMAppService controls, без установки login item при разработке. Полная интеграция первого включения и реальный login отдельно |
| **Помощь по сохранённому делу06F — принята локально** | [06F по ADR-010](../../../decisions/0010-selected-task-assistance.md) после07B/06D/06E; один Core/ABI/providers/native владелец | Выбранный ручной контекст → High Fast → обоснованный результат и copy. Локальная реализация не ждёт03; настоящая передача требует собственного подключения/High Fast, полные06/08 открыты |
| **Локальные ветки11A — приняты локально** | [11A по ADR-011](../../../decisions/0011-selected-local-branches.md) после07A/07B/06F; один Core/ABI/native владелец | Явный bounded reader → source memory →06F; native/cloud live не заменяются локальными тестами, полная11 открыта |
| **Карточка и ответ08A — приняты локально** | [08A по ADR-012](../../../decisions/0012-edge-card-and-text-reply.md) после приёмки11A; один Core/ABI/native владелец | Текущий результат у края → общий текстовый черновик → точный preview → один Send. Временный диалог наследует все ограничения источников; полная лента/память/голос отдельно |
| **Локальный голос10A — принят локально** | [10A по ADR-013](../../../decisions/0013-local-voice-reply.md) после04/08A и build proof; один native/helper владелец | Запись → галочка → отдельный Parakeet → общий reply draft → прежний Send; публичный WAV/localHTTP проверяются без TCC, фактический микрофон/UI/live отдельно |
| **История Codex14A — принята локально** | [14A по ADR-014](../../../decisions/0014-selected-codex-history.md) после приёмки10A; один Core/ABI/native владелец | Выбранный файл → bounded окно → source memory → прежняя помощь. Plan Review/Evidence Audit PASS; liveG6/автоматическое связывание остаются полной14 |
| **Постоянная лента08B — принята локально** | [Задача08B](08b-persistent-proposal-feed.md), [ADR-015](../../../decisions/0015-persistent-proposal-feed.md); оба обзора PASS | Сохранённые предложения/диалог → краевая лента → обновление без дублей → текст/голос. Один владелец общего контракта после приёмки14A; full08/09 отдельно |
| **Выбранный GitHub PR13A — принят локально** | [Задача13A](13a-selected-github-pr.md), [ADR-016](../../../decisions/0016-selected-github-pr.md); Plan Review/Evidence Audit PASS | Явный PAT/allowlist → выбранный PR и обсуждения → атомарное сохранение →08B. Принят после08B; реальные права, Checks/автоматическое обнаружение/full13 отдельно |
| **Выбранная задача Jira12A — принята локально** | [Задача12A](12a-selected-jira-issue.md), [ADR-017](../../../decisions/0017-selected-jira-issue.md); Plan Review/Evidence Audit PASS,08B/13A приняты | Canonical site/ключ → issue/comments/history/relations → atomicSave→08B. Ограниченный Cloud reader, реальные права/subtask relations/full12 отдельно |
| **Слой интеграций 23 — по ADR-020** | [23A](23a-integration-engine-jira.md) движок и Jira → [23B](23b-github-on-integration-engine.md) GitHub и удаление прежнего кода → [23C](23c-integration-switches.md) переключатели. Последовательно, один владелец Core/providers/bridge/native | Эталоны JSON побайтно, перенесённые сценарии 12A/13A, отзыв и устаревание 08B, полный verify, аудит экспортов и checkpoints. Затем пересмотр Todoist17A |
| **Слой плагинов 24 — по ADR-021 и ADR-022** | [24A](24a-plugins-mechanism.md) механизм (токен, локальные тестовые серверы) → [24C](24c-local-plugin-servers.md) локальные серверы для любых плагинов → [24B](24b-plugins-oauth-google.md) Google Диск и Документы через `workspace-mcp`, вход OAuth и живой пилот. После 23C, один владелец | Побайтно прежний запрос без плагинов; цикл, автосохранение, fences, журналы на fixtures; 24C — тестовые stdio-серверы и ручная проверка настоящего сервера из Codex; живая проверка Google и Luna — в 24B с проектом Google Cloud пользователя |
| **Новый интерфейс 25 — по ADR-023** | [25A](25a-platform-and-design-layer.md) пробы, macOS 26, оформление → [25B](25b-main-window-shell.md) каркас окна → [25C](25c-case-tabs-and-plan.md) вкладки и план → [25D](25d-step-page-and-editors.md) шаг и редакторы → [25E](25e-feed-and-edge.md) лента → [25F](25f-notes-links-sources-help.md) вкладки → [25G](25g-settings.md) настройки → [25H](25h-menu-bar-and-quit.md) меню и выход. После 17A, последовательно | Перечень проверок до и после; без визуальных проверок Ambient (D77), пробы — снимками (D78); точки просмотра пользователя после 25B, 25E, 25H |
| **Точно по макету 26 — по ADR-024** | [26A](26a-visual-verification-build.md) сборка «Ambient Visual» и тестовые данные → [26B](26b-window-sidebar-menu.md) окно, панель, строка меню → [26C](26c-case-plan-step.md) дело, план, шаг → [26D](26d-notes-links-sources-help.md) вкладки → [26E](26e-feed.md) лента → [26F](26f-settings.md) настройки. Последовательно; параллельно с ними — Rust-задачи фонового режима (D86) | Сверка снимков «Ambient Visual» со страницами PDF (D84); отпечатки данных пользователя; точки просмотра после 26B, 26E, 26F |
| **Фоновый режим 27 — по ADR-025** | Линия ядра по одной задаче: [27A](27a-model-tier-gate.md) уровень модели → [27B](27b-observation-core-intake.md) приём и отправка → [27C](27c-observation-core-answer.md) ответ → [27D](27d-observation-provider-bridge.md) поставщик и мост; затем [27E](27e-observation-session.md) снимок и распознавание, [27F](27f-highlight-overlay.md) рамка → [27G](27g-observation-ui.md) интерфейс (после 26F) → [27H](27h-observation-live-check.md) живая проверка. Параллельно со Swift-линией 26, без копий дерева (D102) | Replay-эпизоды в ядре; HTTP-тест тела; живой запрос со снимком перенесён в 27H; живая проверка на Mac пользователя |
| **Облачная интеграция** | Завершение полного пути [06](06-first-working-assistant.md), затем полная приёмка 07–19 | Успешные 01 и 03 плюс уже работающая основа. Изменения общей границы выполняются последовательно. Успех CLI не заменяет нативное подключение |
| **Подсветка и поиск** | [20](20-screen-highlighting.md) после 05 и нужной нативной основы; затем [21](21-search-beyond-viewport.md) | 20 требует Jev и актуальной геометрии, полная приёмка — полного 06; 21 требует 03 и 20. Эти проверки не блокируют независимую память |
| **Итог** | [22 — приёмка новых ситуаций](22-first-version-acceptance.md) | Все функции 06–21 приняты полностью, G1–G7 подтверждены в своей области |

Для запуска передавайте агенту номер задачи **и конкретную разрешённую часть**. Например: «Исполни основу 06 по D56: настоящий ввод, Rust, постоянная память и пауза; облачная интеграция остаётся отдельным незавершённым критерием». В отчёте нужны проверенный результат, оставшиеся критерии и фактические проверки. Нельзя отмечать весь билет done ради запуска следующего.

Историческое состояние на2026-10-19: приняты основа06, память07A/07B, собственный вход06B, управление06C, проверка подключения06D, автозапуск06E, помощь06F, локальные ветки11A и карточка/ответ08A. Последний полный verify8/105/51, feature Swift72, app build/signatures PASS; десять контрольных сборок. Native consent/refresh/ACL, visual UI и полный автоматический помощник этим не подтверждены.

Следующие части запускаются по своим фактическим входам. Для рабочего облачного пути остаются подтверждение Fast в01 (сейчас сервер возвращает default), собственный native consent/refresh и права/проверка чтения03. Для20 нужен Jev05. Предложения08, напоминания09, голос10 и источники11–19 требуют своих рабочих контрактов допуска/наблюдения/ответа: принятый ручной план сам по себе их не заменяет. Полные06/07 и08–22 не отмечены done, незавершённые функции не заполнены production mocks.

Для приложения выбрана Luna Low/High Fast; код важных частей выполняют **GPT-6.1 Sol**. Исполнители сообщают управляющему только вопрос, проблему или завершение. Разрешённый владельцем канал final → wait_threads/read_thread → ответ управляющего работает без обратного send от исполнителя; потеря всех каналов связи — остановка, а не разрешение продолжать наугад.

## Зависимости полной приёмки

| Задача | Блокируется задачами |
| --- | --- |
| [01 — Облачный анализ и предельная стоимость](01-cloud-analysis-feasibility.md) | Нет |
| [02 — Чтение работающего Codex](02-codex-read-feasibility.md) | Нет |
| [03 — Общий доступ к приложениям](03-native-content-feasibility.md) | Нет |
| [04 — Локальный Parakeet](04-parakeet-feasibility.md) | Нет |
| [05 — Локализация через Jev](05-jev-localization-feasibility.md) | Нет |
| [06 — Первый работающий путь помощника](06-first-working-assistant.md) | [01](01-cloud-analysis-feasibility.md), [03](03-native-content-feasibility.md) |
| [07 — Несколько дел и связи между ними](07-task-memory-and-links.md) | [06](06-first-working-assistant.md) |
| [08 — Полная лента и общая панель](08-suggestion-feed-and-panel.md) | [07](07-task-memory-and-links.md) |
| [09 — Возврат к незавершённому делу](09-unfinished-work-reminders.md) | [07](07-task-memory-and-links.md) |
| [10 — Голосовой ответ в карточке](10-voice-reply.md) | [04](04-parakeet-feasibility.md), [06](06-first-working-assistant.md) |
| [11 — Локальные проекты и файлы](11-local-projects-and-files.md) | [07](07-task-memory-and-links.md) |
| [12 — Jira](12-jira-source.md) | [07](07-task-memory-and-links.md) |
| [13 — GitHub](13-github-source.md) | [07](07-task-memory-and-links.md) |
| [14 — Codex как источник текущей работы](14-codex-source.md) | [02](02-codex-read-feasibility.md), [07](07-task-memory-and-links.md) |
| [15 — Gmail](15-gmail-source.md) | [07](07-task-memory-and-links.md) |
| [16 — Google Календарь](16-google-calendar-source.md) | [07](07-task-memory-and-links.md) |
| [17 — Todoist](17-todoist-source.md) | [07](07-task-memory-and-links.md) |
| [18 — Apple Заметки](18-apple-notes-source.md) | [07](07-task-memory-and-links.md) |
| [19 — Apple Напоминания](19-apple-reminders-source.md) | [07](07-task-memory-and-links.md) |
| [20 — Подсветка на текущем экране](20-screen-highlighting.md) | [05](05-jev-localization-feasibility.md), [06](06-first-working-assistant.md) |
| [21 — Поиск за пределами видимого экрана](21-search-beyond-viewport.md) | [03](03-native-content-feasibility.md), [20](20-screen-highlighting.md) |
| [23A — Общий движок интеграций и Jira](23a-integration-engine-jira.md) — **принят** | Нет (ADR-020 принят) |
| [23B — GitHub на общем движке](23b-github-on-integration-engine.md) — **принят** | [23A](23a-integration-engine-jira.md) |
| [23C — Переключатели интеграций](23c-integration-switches.md) — **принят** | [23B](23b-github-on-integration-engine.md) |
| [24A — Механизм пользовательских плагинов](24a-plugins-mechanism.md) — **принят** | [23C](23c-integration-switches.md) |
| [24C — Локальные серверы для любых плагинов](24c-local-plugin-servers.md) — **принят** | [24A](24a-plugins-mechanism.md) |
| [24B — Google Диск и Документы через workspace-mcp](24b-plugins-oauth-google.md) — **принят локально, пилот ждёт пользователя** | [24C](24c-local-plugin-servers.md) |
| [25A: Пробы, macOS 26 и слой оформления](25a-platform-and-design-layer.md) | [17A](17a-selected-todoist-task.md) |
| [25B: Каркас главного окна](25b-main-window-shell.md) | [25A](25a-platform-and-design-layer.md) |
| [25C: Вкладки дела, план и отмена источников в модели](25c-case-tabs-and-plan.md) | [25B](25b-main-window-shell.md) |
| [25D: Страница шага и страницы редактора](25d-step-page-and-editors.md) | [25C](25c-case-tabs-and-plan.md) |
| [25E: Лента и лента у края](25e-feed-and-edge.md) | [25D](25d-step-page-and-editors.md) |
| [25F: Вкладки заметок, связей, источников и помощи](25f-notes-links-sources-help.md) | [25E](25e-feed-and-edge.md) |
| [25G: Настройки с боковой панелью](25g-settings.md) | [25F](25f-notes-links-sources-help.md) |
| [25H: Строка меню и выход](25h-menu-bar-and-quit.md) | [25G](25g-settings.md) |
| [26A: Сборка «Ambient Visual» и тестовые данные](26a-visual-verification-build.md) | — |
| [26B: Окно, боковая панель и строка меню по макету](26b-window-sidebar-menu.md) | [26A](26a-visual-verification-build.md) |
| [26C: Дело, план, шаг и завершение по макету](26c-case-plan-step.md) | [26B](26b-window-sidebar-menu.md) |
| [26D: Заметки, связи, источники и помощь по макету](26d-notes-links-sources-help.md) | [26C](26c-case-plan-step.md) |
| [26E: Лента в окне и у края по макету](26e-feed.md) | [26D](26d-notes-links-sources-help.md) |
| [26F: Настройки по макету](26f-settings.md) | [26E](26e-feed.md) |
| [27A: Помощь на выдаваемом уровне модели](27a-model-tier-gate.md) | — |
| [27B: Ядро наблюдения — приём кадров и отправка](27b-observation-core-intake.md) | [27A](27a-model-tier-gate.md) |
| [27C: Ядро наблюдения — ответ модели](27c-observation-core-answer.md) | [27B](27b-observation-core-intake.md) |
| [27D: Поставщик и мост наблюдения](27d-observation-provider-bridge.md) | [27C](27c-observation-core-answer.md) |
| [27E: ObservationSession — снимок, распознавание, цикл](27e-observation-session.md) | [27D](27d-observation-provider-bridge.md), [26A](26a-visual-verification-build.md) |
| [27F: Рамка подсветки](27f-highlight-overlay.md) | [27D](27d-observation-provider-bridge.md) |
| [27G: Интерфейс фонового режима](27g-observation-ui.md) | [26F](26f-settings.md), [27E](27e-observation-session.md) |
| [27H: Живая проверка фонового режима](27h-observation-live-check.md) | [27C](27c-observation-core-answer.md), [27E](27e-observation-session.md), [27F](27f-highlight-overlay.md), [27G](27g-observation-ui.md) |
| [22 — Приёмка всей первой версии и новых ситуаций](22-first-version-acceptance.md) | [08](08-suggestion-feed-and-panel.md), [09](09-unfinished-work-reminders.md), [10](10-voice-reply.md), [11](11-local-projects-and-files.md), [12](12-jira-source.md), [13](13-github-source.md), [14](14-codex-source.md), [15](15-gmail-source.md), [16](16-google-calendar-source.md), [17](17-todoist-source.md), [18](18-apple-notes-source.md), [19](19-apple-reminders-source.md), [20](20-screen-highlighting.md), [21](21-search-beyond-viewport.md) |

Эта таблица сохраняет зависимости полной приёмки. Условия начала независимых частей указаны выше и в каждом билете; незавершённая облачная приёмка 06 больше не блокирует разработку проверенной памяти. Совместимость одновременных изменений определяется фактическими границами реализации.

Проверки G1–G7 обозначены в спецификации. 01 проверяет G5, 02 — G1, 03 — G2, 04 — G3, 05 — G4; G7 начинается в 06 и проверяется в нативных срезах. Каждый источник 11–19 включает свой G6. Итоговая задача 22 сопоставляет все доказательства и проверяет новые ситуации без подгонки.

