# Результаты проверки основы

## Приёмка 25E — 21 октября 2026

Лента и лента у края ([ADR-023](decisions/0023-ui-redesign-liquid-glass.md), [приёмка](../target/verification/orchestration/25e-parent-inspection.md)).

- **Исполнитель:**
  - yarn verify 99 Swift, feature Swift 147;
  - RED→GREEN `FeedFocusTests` (#20);
  - проба E1: стекло заполняет 420×600 вместе с заголовком, панель при появлении не становится ключевой, ввод работает;
  - перечень проверок 108 → 112, охраны кнопок ленты прежние;
  - аудит и подписи.
- **Оркестратор:**
  - свежий yarn verify 19:47–19:50Z, exit 0, 358 Rust / 99 Swift;
  - в `EdgeCardController` изменён только блок создания окна, `EdgeCardTests` не изменены;
  - хэши совпадают;
  - 26 прежних контрольных сборок (466 файлов) целы;
  - новая `Ambient-feed-25e.app`.
- **Не проверено:** вид стекла и карточек (смотрит пользователь, D77), настоящий ввод в пробе E1 (был синтетический; в R2 25A — настоящий), CI на `macos-26`.

## Приёмка 25D — 21 октября 2026

Страница шага, страницы редакторов и D81 ([ADR-023](decisions/0023-ui-redesign-liquid-glass.md), [приёмка](../target/verification/orchestration/25d-parent-inspection.md)).

- **Исполнитель:**
  - yarn verify 98 Swift, feature Swift 146;
  - `MemorySessionTests` переписаны на `editorKind`;
  - `CasePlanTests`: приоритет страниц; D81 — на второй странице готовый шаг есть, а блока нет; поведенческий RED для D81 наблюдался;
  - перечень проверок 107 → 108, все изменения объяснены;
  - аудит и подписи.
- **Оркестратор:**
  - свежий yarn verify 19:21–19:24Z, exit 0, 358 Rust / 98 Swift;
  - хэши исходников и Mach-O совпадают;
  - 25 прежних контрольных сборок (444 файла) не изменились;
  - новая `Ambient-steps-25d.app`.
- **Не проверено:** вид страниц (смотрит пользователь, D77), CI на `macos-26`, устройство с macOS 26.

## Приёмка 25C — 21 октября 2026

Вкладки дела, план и отмена чтений в модели ([ADR-023](decisions/0023-ui-redesign-liquid-glass.md), [приёмка](../target/verification/orchestration/25c-parent-inspection.md)).

- **Исполнитель:**
  - yarn verify 96 Swift, feature Swift 144;
  - RED→GREEN `CasePlanTests`: «следующий шаг» с границей страницы; «ждёт шаг N»; смена дела, ревизии и паузы отменяет все пять источников, а шаг и редакторы не отменяют;
  - перечень проверок 114 → 107, все изменения объяснены;
  - аудит и подписи.
- **Оркестратор:**
  - свежий yarn verify 18:47–18:50Z, exit 0, 358 Rust / 96 Swift;
  - хэши исходников и Mach-O совпадают;
  - 24 прежние контрольные сборки (422 файла) не изменились;
  - новая `Ambient-tabs-25c.app`.
- **Не проверено:** вид вкладок (смотрит пользователь, D77), CI на `macos-26`, устройство с macOS 26.

## Приёмка 25B — 21 октября 2026

Каркас главного окна ([ADR-023](decisions/0023-ui-redesign-liquid-glass.md), [приёмка](../target/verification/orchestration/25b-parent-inspection.md)).

- **Исполнитель:**
  - yarn verify 93 Swift, feature Swift 141;
  - RED→GREEN `CaseGroupsTests`;
  - проба R5: после «Отмены» список возвращается к строке модели, 3 из 3 запусков;
  - перечень проверок 111 → 114, все изменения объяснены;
  - аудит и подписи.
- **Оркестратор:**
  - свежий yarn verify 16:56–17:00Z, exit 0, 358 Rust / 93 Swift;
  - хэши исходников и Mach-O совпадают;
  - 23 прежние контрольные сборки (400 файлов) не изменились;
  - новая `Ambient-shell-25b.app`.
- **Не проверено:** вид окна (смотрит пользователь, D77), CI на `macos-26`, устройство с macOS 26.

## Приёмка 25A — 21 октября 2026

Пробы, macOS 26 и слой оформления ([ADR-023](decisions/0023-ui-redesign-liquid-glass.md), [приёмка](../target/verification/orchestration/25a-parent-inspection.md)).

- **Пробы R1–R4** — основные пути прошли:
  - настройки с боковой панелью;
  - лента у края со стеклом и вводом, приложение пользователя остаётся впереди;
  - `.onDisappear` корня главного окна при закрытии;
  - иконки пунктов меню на macOS 27.

  Снимки сделаны через computer-use приложения Claude с разрешения пользователя.
- **Исполнитель:**
  - yarn verify 358 Rust / 92 Swift, feature Swift 140, Clippy;
  - аудит: экспорты в `Contents/MacOS/Ambient`, нет `Ambient.debug.dylib`, minos 26.0 у всех Mach-O, подписи;
  - перечень проверок 111 = 111.
- **Оркестратор:**
  - свежий yarn verify 16:17–16:21Z, exit 0, 358 Rust / 92 Swift;
  - хэши Mach-O совпадают с доказательством исполнителя;
  - 22 прежние контрольные сборки (378 файлов) не изменились;
  - новая `Ambient-design-25a.app`.
- **Не проверено:** интерфейс самого Ambient (смотрит пользователь, D77), CI на `macos-26` (нет удалённого репозитория), устройство с macOS 26.

## Исправление запуска на macOS 27 — 21 октября 2026

Пользователь впервые открыл собранное приложение, и оно не запустилось. В отчёте macOS о сбое загрузчик отказался грузить `@rpath/Ambient.debug.dylib`: «code signature … not valid for use in process: mapping process and mapped file (non-platform) have different Team IDs».

Причина:
- сборка Debug выносила код приложения в отдельную библиотеку `Ambient.debug.dylib` (умолчание Xcode `ENABLE_DEBUG_DYLIB`);
- обе части подписывались ad-hoc с Hardened Runtime;
- проверка библиотек macOS 27 такую библиотеку не допускает.

Раньше дефект не проявлялся, потому что агенты по правилам не запускали приложение, а проверки ограничивались `codesign --verify`.

Проверка гипотезы: сборка в отдельную папку с `ENABLE_DEBUG_DYLIB=NO` запустилась и проработала больше 3 s без ошибок загрузчика.

Исправление: `ENABLE_DEBUG_DYLIB = NO` для Debug в `Ambient.xcodeproj`. Подпись ad-hoc и Hardened Runtime сохранены; `Contents/MacOS` содержит один файл `Ambient`; 56 экспортов ABI в нём; codesign strict/deep PASS. Полный `yarn verify` после исправления (15:32–15:35Z): exit 0, Swift 91 тест.

**Последствие для аудита:** дальнейшие аудиты ищут экспорты в `Contents/MacOS/Ambient`, а не в `Ambient.debug.dylib`, и проверяют, что отладочной библиотеки нет. Старые контрольные сборки с `Ambient.debug.dylib` на macOS 27 не запускаются и остаются только эталонами файлов.

## Приёмка 17A — 21 октября 2026

Принят Todoist на общем движке интеграций ([ADR-018](decisions/0018-selected-todoist-task.md)):
- схема 16, пятый переключатель;
- запись аккаунта `Step::Pin` (D65);
- пауза из заголовка и тела (D66);
- запросы без конечного `/user` (D67);
- ABI интеграций v1 — 18 экспортов;
- интерфейс источника в macOS.

Свежий yarn verify оркестратора:
- первый прогон уронил давний тест голоса `voiceCancellationAndDeadlineKillIgnoredTERMAndWaitForActualExit` (окно 5 s, 6,7 s под нагрузкой); 17A его не трогал, отдельно тест прошёл 6 из 6;
- повторный полный прогон: 8 tooling/358 Rust/91 Swift, exit 0.

У исполнителя:
- feature Swift 139, bridge 36;
- аудит: 18 экспортов интеграций, 770 строк на язык;
- 64+64 прогонов под нагрузкой без отказов;
- эталоны GitHub и Jira 18/18 побайтно прежние;
- 15/15 мутаций.

Сохранена Ambient-todoist-17a.app; всего 22 приложения/378 файлов. Настоящий аккаунт Todoist, полная 17/G6 и UI не проверены. [Handoff](../target/verification/17a/handoff.txt), [приёмка](../target/verification/orchestration/17a-parent-inspection.md).

## Приёмка 24B (локальный объём) — 21 октября 2026

Принят Google через управляемый `workspace-mcp` ([ADR-022](decisions/0022-local-mcp-servers.md)):
- встроенный пакет и «Добавить Google»;
- `Managed`/`ManagedServerId` и список из 8 инструментов;
- установка через `uv` по хэшам;
- провод loopback HTTP;
- OAuth со встроенным при сборке клиентом;
- отзыв в Google при выходе.

Свежий yarn verify оркестратора: 8 tooling/331 Rust/90 Swift, exit 0.

У исполнителя:
- feature Swift 136, bridge 33;
- all-features Clippy, сборка;
- аудит: плагины v2 — 21 экспорт, клиент Google есть только в бинарном файле;
- 16/16 мутаций;
- 64+64 прогонов под нагрузкой, после исправления дефекта выбора порта.

Оркестратор сверил 237 исходников и проверил, что секрета клиента нет в 682 файлах для git и в файлах проверок. Сохранена Ambient-google-24b.app; всего 21 приложение/354 файла.

**Ручные проверки на этом Mac (без аккаунта Google):**
- установка 22,7–25,1 s, около 250 MB;
- подменённые хэши колеса и сборки Python отвергаются;
- сервер слушает только 127.0.0.1;
- вне частной папки ничего не пишется;
- холодный старт около 2 s;
- `tools/list` — 29,3 KB.

**Ждёт пользователя:** живой пилот (`target/verification/24b/live-pilot-plan.md`) — вход, точные права в согласии, вызовы инструментов Luna на подписке (подтверждение D64), реальные размеры результатов, длинный документ, отзыв при выходе, журнал расходов.

**Чек-лист выпуска сервера Google** (при каждом повышении версии `workspace-mcp`):
1. `uv lock --check` тега.
2. `uv export --format requirements.txt --no-emit-project --no-dev --frozen`.
3. Хэш колеса `workspace-mcp` с PyPI.
4. Колёса arm64 для закреплённого CPython.
5. Сверка списка инструментов с `tools/list`.
6. Просмотр изменений кода.

Новый файл требований меняет имя папки установки, поэтому пользователь увидит «Обновить».

[Handoff](../target/verification/24b/handoff.txt), [приёмка](../target/verification/orchestration/24b-parent-inspection.md).

## Приёмка 24C — 21 октября 2026

Приняты локальные серверы для любых плагинов ([ADR-022](decisions/0022-local-mcp-servers.md)):
- `ServerKind`/`Target` Remote/Local;
- разбор локального сервера с запретом системных имён переменных;
- `local_host.rs`: группы процессов, очищенное окружение, фиксированный PATH, частные папки, провод stdio с потоками чтения и записи;
- последовательность остановки с пределами по 2 s и итогом «остановка не подтверждена»;
- SIGPIPE отключён при запуске приложения;
- секрет `env(map)`;
- запас по видам серверов;
- модель ABI v6, `ambient_plugin.h` v2 с описателем операции;
- просмотр и обнаружение в macOS.

Эталон запроса без плагинов прежний.

Свежий yarn verify оркестратора: 8 tooling/301 Rust/85 Swift, exit 0.

У исполнителя:
- feature Swift 130, bridge 29;
- all-features Clippy, rustfmt, Swift lint, сборка;
- аудит: модель v6 — 17 экспортов, плагины v2 — 14, без моделей v1–v5 и плагинов v1;
- strict/deep подписи, 656 строк на язык;
- providers 64+64 прогонов без отказов;
- 15/15 мутаций пойманы;
- RED SIGPIPE: без вызова запуска тестовый процесс убит сигналом 13.

Ручная проверка `uvx mcp-server-time@2026.10.19` (временная база, без аккаунта):
- холодное обнаружение 10,8–12,1 s, тёплое 0,38–0,41 s;
- вызов 0,36–0,38 s;
- отмена 0,31–0,34 s без оставшихся процессов.

Плагины через `npx` на этом Mac не проверить: Homebrew-node не запускается.

Оркестратор сверил 221 исходник, код на границах, подпись и хэши. Сохранена Ambient-local-servers-24c.app: 24 файла совпали, codesign PASS; всего 20 приложений/330 файлов.

[Handoff](../target/verification/24c/handoff.txt), [приёмка](../target/verification/orchestration/24c-parent-inspection.md).

## Приёмка 24A — 21 октября 2026

Принят механизм пользовательских плагинов ([ADR-021](decisions/0021-plugins-layer.md)):
- схема 14, пакеты обоих форматов;
- закрепление инструментов «только чтение»;
- цикл модели внутри одного `dispatch` (до 5 POST и 8 вызовов, последний POST без инструментов);
- автосохранение процитированного, основание 08B `Plugin`;
- журнал вызовов и сворачивание журнала попыток;
- модель ABI v5, `ambient_plugin.h` v1;
- настройки плагинов в macOS.

Эталон запроса без плагинов побайтно прежний. Давний нестабильный тест ChatGPT исправлен: было 9 отказов из 116, после исправления 0 из 304.

Свежий yarn verify оркестратора: 8 tooling/285 Rust/82 Swift, exit 0.

У исполнителя:
- feature Swift 127, bridge 27;
- all-features Clippy, rustfmt, Swift lint, сборка;
- аудит: 4 Mach-O, модель v5 — 17 экспортов, плагины — 11, интеграции — 17, без моделей v1–v4 и тестовых точек;
- strict/deep подписи, 630 строк на язык;
- providers 64+64 прогонов под нагрузкой без отказов;
- 8/8 мутаций кода пойманы.

Оркестратор сверил 214 исходников, код на границах, подпись и хэши. Сохранена Ambient-plugins-24a.app: 24 файла совпали, codesign PASS; всего 19 приложений/306 файлов.

Не проверены настоящие аккаунты и серверы MCP, OAuth/Google, живой вызов инструментов моделью на подписке, UI. [Handoff](../target/verification/24a/handoff.txt), [приёмка](../target/verification/orchestration/24a-parent-inspection.md).

## Приёмка 23C — 20 октября 2026

Приняты переключатели встроенных интеграций (D61): схема 13 с таблицей ровно из четырёх переключателей, ABI интеграций v1 — 17 экспортов (`enabled`/`set_enabled` для видов 1..4), раздел «Настройки › Интеграции» и заглушка выключенной интеграции в окне дела. Переключатель проверяется при допуске, в fence операций, в транзакции сохранения, в обоих fence оснований 08B и при построении нового предпросмотра; то же значение не запускает `Store::change`. Свежий yarn verify оркестратора: 8 tooling/254 Rust/79 Swift, exit 0. У исполнителя: feature Swift 123, bridge 22, all-features Clippy, rustfmt, Swift lint, сборка, аудит 4 Mach-O/17 экспортов/582 строки на язык, strict/deep подписи; RED — ошибки компиляции и три мутации кода, каждая поймана нужным тестом. Оркестратор сверил 181 исходник, код на границах, подпись и хэши. Сохранена Ambient-switches-23c.app: 24 файла совпали, codesign PASS; всего 18 приложений/282 файла. [Handoff](../target/verification/23c/handoff.txt), [приёмка](../target/verification/orchestration/23c-parent-inspection.md).

## Приёмка 23B — 20 октября 2026

Принят перевод GitHub на общий движок интеграций: схема 12, ABI интеграций v1 с видом 3 (15 экспортов), общий транспорт с `bearer_auth`, общий владелец секретов и каркас Swift; прежние хранилище, транспорт, ABI, заголовок и Swift-копии GitHub удалены, основание 08B только `Integration`. Свежий yarn verify оркестратора: 8 tooling/244 Rust/77 Swift, exit 0. У исполнителя: feature Swift 120, bridge 21, all-features Clippy, rustfmt, сборка, аудит 4 Mach-O/15 экспортов/0 GitHub и Jira, strict/deep подписи; эталоны GitHub побайтно прежние, кроме названных изменений; 27 перенесённых и 6 новых сценариев. Оркестратор сверил 175 исходников, аудит удаления, код адаптера, подпись и хэш библиотеки. Сохранена Ambient-github-engine-23b.app: 24 файла совпали, codesign PASS; всего 17 приложений/258 файлов. Давний тест транспорта ChatGPT нестабилен под нагрузкой — исправление поручено 24A. [Handoff](../target/verification/23b/handoff.txt), [приёмка](../target/verification/orchestration/23b-parent-inspection.md).

## Приёмка 23A — 20 октября 2026

Принят общий движок интеграций ([ADR-020](decisions/0020-integration-layer-contract.md)) с Jira на нём: схема 11, ABI интеграций v1, один HTTP-транспорт интеграций, один владелец секретов Swift с привязкой к экземпляру базы. Первый свежий yarn verify оркестратора выявил нестабильный тест тестового HTTP-сервера (принятый сокет наследовал неблокирующий режим); исполнитель воспроизвёл и исправил его в тестовом модуле. Второй свежий yarn verify: 8 tooling/232 Rust (+1 игнорируемый сборщик эталонов)/78 Swift, exit 0. У исполнителя: feature Swift 122, bridge 18, all-features Clippy, сборка, аудит 4 Mach-O/14 экспортов интеграций/0 Jira, strict/deep подписи; эталоны GitHub 9/9 и Jira (окно, предпросмотр, `data`, сохранённые метаданные, контекст модели) побайтно прежние, изменены только конверт профилей, цитата и JSON основания 08B. Оркестратор сверил 167 исходников, код на границах, подпись и хэш библиотеки. Сохранена Ambient-integrations-23a.app: 24 файла совпали, codesign strict/deep PASS; всего 16 приложений/234 файла. Наблюдение вне 23A: давний тест транспорта ChatGPT `actual_http_oversize_and_quota_errors_remain_bounded` возвращает Timeout при чрезмерной искусственной перегрузке. [Handoff](../target/verification/23a/handoff.txt), [приёмка](../target/verification/orchestration/23a-parent-inspection.md).

Настоящие Jira/GitHub, UI, Windows, High Fast и полные 12/13/G6 не подтверждены. Новых model/Jev вызовов нет; 7 POST/1021 known tokens/3 unknown сохранены.

## Приёмка12A — 20 октября2026

Принят локальный путь Jira Cloud → выбранные сведения → тот же Store →08B. Свежие verify8 tooling/224 Rust/75 Swift и feature Swift119 (один real-ASR тест явно пропущен), all-features Clippy, build и подписи PASS. Обязательные специалисты и Code Simplicity Review выполнены; две находки исправлены и повторно проверены. API Inspector14/14 и отдельная проверка history probe прошли. Управляющий сверил180 source hashes/47 изменённых файлов,4 Mach-O, свежую подпись и неизменность186 файлов14 прежних checkpoints. Сохранена Ambient-jira-12a.app:24 файла совпали, strict/deep codesign PASS; всего15apps/210files. [Handoff](../target/verification/12a/handoff.txt), [приёмка](../target/verification/orchestration/12a-parent-inspection.md).

Настоящие Jira/права/Cloudv3 subtask relations, UI, macOS14 hardware, Windows, High Fast и full12/G6 не подтверждены. Новых model/Jev вызовов нет;7POST/1021known tokens/3unknown сохранены.

## Приёмка13A — 20 октября2026

Принят локальный GitHub PR → выбранные объекты → память → помощь08B. Full verify8 tooling/186 Rust/72 Swift; feature Swift111 с публичным WAV. После последних исправлений пересобран feature bridge, GitHub feature6 и Core27 прошли; all-features Clippy/build/signatures PASS. Full111 не выдаётся за повтор после этих исправлений. Review и focused corrections PASS; API Inspector9/9 на fixtures, не UI. Управляющий сверил169 current source hashes/49 изменённых файлов,4 Mach-O и162 старых checkpoint файла. Сохранён Ambient-github-13a.app:24 файла совпали, strict/deep codesign PASS; всего14apps/186files. [Handoff](../target/verification/13a/handoff.txt), [проверка управляющего](../target/verification/orchestration/13a-parent-inspection.md).

Реальные PAT permissions, UI, серверная квота, live Fast, Windows и full13/G6 не проверены. Новых live model/Jev вызовов нет;7POST/1021known tokens/3unknown неизменны.

## Приёмка08B — 20 октября2026

Свежие verify8 tooling/154 Rust/71 Swift и расширенный Swift105 с настоящим Parakeet на публичном WAV — PASS. All-features Clippy, build, подписи и независимый обзор прошли. Управляющий сверил50 source hashes,4 Mach-O и138 прежних checkpoint-файлов; последняя правка кода предшествовала итоговым проверкам. Сохранена Ambient-feed-08b.app с проверкой совпадения и strict/deep подписи; теперь13 checkpoints/162 файла. [Handoff](../target/verification/08b/handoff.txt), [проверки](../target/verification/08b/verification-summary.json), [приёмка](../target/verification/orchestration/08b-parent-inspection.md).

Исправлены гонки preview/отмены, сохранение stale-обсуждения при обновлении Codex и принадлежность сохранённого StepId. Реальные UI/focus/scroll/Spaces, микрофон/TCC, устройство macOS14, Windows и live High Fast/semantic ID quality не проверены. Новых model/Jev вызовов нет.

## Приёмка 14A — 20 октября 2026

Свежий verify:8 tooling/135 Rust/70 Swift; feature Swift101 (real ASR намеренно не повторялся); all-features Clippy, app build, strict/deep signatures и Mach-O audit — exit0. Управляющий проверил реальные команды/логи,44 изменённых source hashes,4 Mach-O и114 файлов11 прежних сборок. Сохранена Ambient-codex-14a.app; теперь12 checkpoints/138files. [Handoff](../target/verification/14a/handoff.txt), [результаты](../target/verification/14a/verification-summary.json), [приёмка управляющего](../target/verification/orchestration/14a-parent-inspection.md).

Scout/Librarian/Type Scaffolder read-only/Researcher/Localizer и Code Simplicity Review выполнены. Замечания ordinal/cancel/preview и SQLite закрыты с RED/GREEN и focused review. Отдельный живой тестовый процесс прежней версии подтверждает побайтовый отказ от старой WAL-базы; ограничение raw writable SQLite в том же процессе сохранено в [отчёте](features/assistant-behavior/evidence/14-sqlite-admission-review.md). Первый feature-прогон упёрся в sandbox тестового Keychain/pasteboard; полный прогон с требуемым native-доступом прошёл. UI, реальные истории, Windows/macOS14 hardware, live Fast и full14/G6 не проверены. Новых model/Jev вызовов нет.

## Приёмка 10A — 20 октября 2026

Свежие логи подтверждают 8 tooling / 105 Rust / 69 Swift, feature Swift95, all-features Clippy, app build и strict signatures. Настоящий packaged ASR → editable draft → exact preview → один localHTTP Send прошёл. Отдельный helper smoke без сети: 8/8; отмена в этой пробе проверяет живой startup, не активный CoreML. Process tests отдельно покрывают ignored TERM, deadline, parent EOF и гонки. Parent сверил реальные логи, 78 source hashes и 90 прежних checkpoint hashes; создал Ambient-voice-10a.app, strict signature PASS. Всего11 сборок/114 файлов.

[Handoff](../target/verification/10a/handoff.txt), [JSON и команды](../target/verification/10a/verification-summary.json). Исторические /tmp логи19 октября утрачены; восстановленный текст и прежние tool outputs отделены от свежих проверок. Runtime upstream получен напрямую из официального репозитория на прежнем commit, временный mirror удалён. Specialist review19 октября применим: логика Swift/C не менялась. UI/TCC/микрофон/live Fast/Jev/macOS14 hardware/Windows не проверялись. Новых live POST нет; прежние unknown остаются unknown.

## Карточка и текстовый ответ08A / ADR-012 — 19 октября2026

Full yarn verify exit0:8 tooling/105 Rust/51 Swift; отдельный feature Swift72, cleanup65.084s. All-features Clippy, app build/strict signatures и Code Simplicity Review с focused rechecks PASS. Проверены exact HTTP bytes, clarification→reply→ещёreply→suggestion/copy, одноразовый Send, отсутствие DB/ledger при preview, полные source fences, bounds, expiry/discard/late-result races и независимость memory editor. Type Scaffolder read-only; Scout→Librarian; Localization Mechanic21keys EN/RU (324/324).

Управляющий прочитал handoff, реальные log tails, Core getter/наследование snapshot и native NSPanel ownership. Все81 прежних файла сохранены; создан и проверен Ambient-reply-08a.app. Теперь10 checkpoints/90files. Все3 Mach-O без hooks/modelv1/v2,14modelv3 exports. Schema6/memory3/auth1/source1 неизменны. Handoff: `/tmp/ambient-08a-handoff.txt`, proof/logs: `/tmp/ambient-08a-logs/`.

UI/focus/Spaces/масштабы, macOS14hardware, Windows и live auth/Fast/Jev не проверены. TCC, реальные проекты и пользовательские данные не затрагивались. Полные07/08/09/10 остаются открыты.

## Выбранные локальные ветки11A / ADR-011 — 19 октября2026

Полный yarn verify exit0:8 tooling/99 Rust/50 Swift. Отдельный feature Swift-прогон65 tests (15 дополнительных,8 новых для source), all-features Clippy и app build/signature PASS. Code Simplicity Review и точечный повтор PASS; Type Scaffolder дважды read-only, Localization Mechanic EN/RU43keys (303 в каталоге). Исправлены native SIGBUS на256files (два flat buffers), неподтверждённые packed OID при потерях loose refs, symbolic HEAD validation и отмена, ожидавшая Store lock.

Команды/результаты: /tmp/ambient-11a-logs/verification-summary.json; verify-unsandboxed.log; feature-swift-unsandboxed.log; app-checkpoint-proof.json. Координатор сверил журналы, native FD lifetime/flat buffers и Core source fences. Скопирована и отдельно проверена подпись target/checkpoints/Ambient-branches-11a.app; прежние72файла неизменны, теперь9сборок/81файл. Во всех3Mach-O нет test hooks/старых ABI. Schema6 read-only отвергает5/WAL.

Production-каталог отсутствует по metadata-only проверке. UI/TCC, реальные проекты/БД, native consent, live Fast, Jev, macOS14 hardware и Windows не проверялись. Полная11/G6 и автоматическое обнаружение незавершённости не заявлены.

## Помощь по сохранённому делу06F / ADR-010 — 19 октября2026

Финальный verify exit0:8 tooling/83 Rust/48 Swift; отдельный Swift–C–Rust/local HTTP/SSE прогон8 tests (7 дополнительных) exit0. Build/signature PASS. Логи `/tmp/ambient-06f-logs/verify-accepted.log`, `swift-http-accepted.log`, `build-accepted.log`, `production-symbols.json`. В3Mach-O нет test hook/modelv1, все11v2exports присутствуют. Code Review и целевые исправления PASS; TypeRO/RegistryMechanic/LocalizationMechanic выполнены, EN/RU262keys. Исправлены commit/cancel/ABI-final гонки, refresh/errors и test-only unbounded accept; productiondeadline60s сохранён, Swiftcleanup65s прошёл.

Координатор сверил команды/ключевые границы, сохранил и отдельно проверил `target/checkpoints/Ambient-assistance-06f.app`. Все63старых файла неизменны; теперь8checkpoints/72files. Schema5 reject4/WAL проверен без записи. Productionкаталог/БД отсутствуют; реальный native consent/no-credit/High Fast/новаяSIWCschema/modelusefulness/UI/macOS14hardware/Windows не проверены. Нет live/Jev вызовов или системных регистраций. Полные06/08/G5 открыты.

## Управление автозапуском06E / ADR-009 — 19 октября2026

Финальный yarn verify:8 tooling/62 Rust/46 Swift, exit0. App build и строгая подпись exit0. Логи: /tmp/ambient-06e-verify-final.log и /tmp/ambient-06e-build.log. Восемь новых тестов прошли RED → GREEN; исправлено замечание Code Simplicity Review по явному refresh, повтор PASS. Localization Mechanic обновил14 ключей EN/RU; актуальные Scout/Librarian/Researcher findings переиспользованы, новых non-UI типов нет.

Координатор проверил финальные команды, статус/операции native owner и SwiftUI refresh. Все54 файла шести прежних checkpoints побайтово сохранены, подписи PASS. Сохранён target/checkpoints/Ambient-login-06e.app, отдельная подпись PASS. System registration/login/UI не выполнялись; macOS14 hardware и Windows не проверены. Нет model/Jev вызовов и изменений системных настроек. Полные D28/06 открыты.

## Нативная проверка подключения06D / ADR-008 — 19 октября2026

Принят bounded Settings → model ABI1 → Rust HTTP → Core/SQLite journal → UI путь. Финальный `yarn verify` exit0:8 tooling,62 Rust,38 Swift tests; strict format/Clippy/agent generation прошли. `yarn build:macos -quiet` и codesign текущей app — exit0; minimumOS14 сохранён во всех3 executable Mach-O images. Логи: `/tmp/ambient-06d-verify.log`, `/tmp/ambient-06d-build.log`. Это локальные проверки, не настоящий Fast/consent/G5.

Scout/Librarian/Type Scaffolder read-only/Endpoint Registry Mechanic/Localization Mechanic/Code Simplicity Reviewer выполнены; Researcher evidence переиспользован из ADR. EN/RU по219 ключей,40 новых. Review исправления: история перечитывается без устаревшей ошибки, provider timeout не скрывается вторым native сроком. Оба случая сначала воспроизведены RED, затем GREEN; целевое повторное review чистое. Реальный длительный local cleanup test занимает более60s, без провайдера.

HTTP проверен локальным сервером; SQLite и Swift–C–Rust настоящие, тестовые зависимости изолированы. Схема3/WAL не изменяются при отказе новой схемы4. Registry/Store leases, одинPOST, pause/cancel/signout/shutdown, SSE/config/usage/error limits покрыты. Auth protocol/allowlist/headers/Keychain/loopback остались прежними. Проверка inventory сначала ошиблась в выборе корня; исправленная и итоговая проверки подтвердили все45 файлов пяти checkpoints и их подписи. Xcode выдал прежние CoreDevice/CoreSimulator warnings, build успешен; SwiftPM sandbox escalation прошла штатно.

Координатор сверил команды/exit codes и критические границы, сохранил `target/checkpoints/Ambient-connection-06d.app`; подпись отдельной копии exit0. Production каталог/БД отсутствуют и после тестов. Стенд01 по-прежнему имеет7 попыток/1021known tokens/3unknown; новый live model/token POST, Jev, TCC, native consent, UI-запуск не выполнялись. macOS14 hardware/Windows не проверены.

## Текущее управление06C / ADR-007 — 19 октября 2026

Тред `Основа 06C · Меню, пауза и разрешения` завершён; координатор прочитал итоговые команды, общий tracked operation/app shutdown, pause-only reader и permission preflight с явным false. `yarn verify` exit0: 8 tooling, 46 Rust, 33 Swift; строгие lint/format пройдены. `yarn build:macos -quiet` exit0; codesign текущей app и checkpoints06/07A/07B/06B —5/5PASS. Все36 файлов прежних checkpoints побайтово сохранены. Координатор создал и отдельно проверил `target/checkpoints/Ambient-controls-06c.app`.

Шесть новых Swift tests проверяют старт меню/окна, pause без content load, сохранность черновика/страниц, read/write ошибки, busy/close и локальное обновление permission state; close имеет два параметрических случая. Исходный meaningful RED воспроизвёл замену snapshot старым toggle. Scout refreshed06B, Librarian, read-only Type Scaffolder, Localization Mechanic(EN/RU), Researcher и Code Simplicity Reviewer выполнены, review без замечаний.

C-accessor системного ключа static-inline, ownership аннотирован; native memory/auth ABI не меняются. Первая nm-проверка смотрела только app launcher и ошибочно не нашла native import. Повтор на всех трёх executable Mach-O images завершился exit0: native APIs присутствуют, экспортируемого accessor нет. Это не визуальная проверка UI. Известные Xcode warnings не сорвали build. Ни app, ни реальные TCC/consent/refresh/ACL, macOS14 hardware и Windows не проверялись; model/Jev calls отсутствуют.

## Предыдущий вход06B / ADR-006 — 19 октября 2026

Тред `Основа 06B · Вход ChatGPT и сессия` завершён. Координатор прочитал последние verify/build/codesign commands с exit0: 8 tooling, 46 Rust и 27 Swift tests. Проверены сохранение до exchange/refresh, generation/deadline fence, busy до фактического возврата, shutdown всех native connections и app delegate, который ждёт память и auth. Локальный подписывающий IdP проходит настоящий HTTP; отдельный временный Keychain проверяет запись и повторное открытие сессии. Пользовательские credentials не использовались.

Обязательные Type Scaffolder(read-only), Endpoint Registry Mechanic, Localization Mechanic, Code Simplicity Reviewer и узкие Librarian/Researcher вызовы выполнены. Review исправления: UI не застревает busy при ошибке хранилища, callback response ждёт send completion, idle browser timeout сообщает expiry. Целевые rechecks без замечаний. Полный прогон выполнен после исправлений.

Финальная `.app`: arm64, minimumOS14.0, codesign PASS, как и три предыдущих checkpoints. Координатор создал `target/checkpoints/Ambient-session-06b.app` и отдельно проверил подпись. Известные Xcode CoreDevice/CoreSimulator и test-only deprecated Keychain API warnings не скрывались. Native consent/пользовательский ACL/реальный refresh, visual UI, macOS14 hardware, Windows и Fast не проверены; полная06 открыта.

## Предыдущая память 07B / ADR-005 — 19 октября 2026

Тред `Основа 07B · План и завершение дел` завершён; координатор прочитал итоговые команды verify/build/codesign с exit0 и ключевую логику планов/очистки. `yarn verify`: 8 tooling, 32 Rust, 13 Swift tests, strict lint/format. `.app` и подписи текущей сборки/checkpoints06/07A прошли; отдельно создан и проверен `target/checkpoints/Ambient-plan-07b.app`.

Проверены ручные зависимости/проверки, инвалидизация потомков, пределы, ревизии страниц, атомарное завершение/rollback, точный набор затронутых дел, сохранность старых схем/WAL, reopen/забывание, черновики и отдельный тестовый clipboard. Code Simplicity Reviewer нашёл сброс страницы и лишнее чтение шага; оба исправлены, целевой recheck PASS. Type Scaffolder read-only и Localization Mechanic вызваны; применимые Scout/Librarian/Researcher выводы приняты.

Native clipboard тест требовал разрешённого запуска вне файловой песочницы; user clipboard и данные не использовались. Известные CoreDevice/CoreSimulator warnings не помешали build. Visual UI, macOS14 hardware, Windows CI, перенос userDB и автоматический помощник не проверены. Следующий06B реализуется, его готовность этим прогоном не подтверждается.

## Предыдущая память 07A / ADR-004 — 19 октября 2026

Тред `Основа 07 · Несколько дел и память источников` завершил независимую часть. Координатор проверил реальные команды с exit 0: полный `yarn verify` (8 tooling, 17 Rust и 8 Swift tests), `yarn build:macos -quiet`, подпись новой `.app` и checkpoint 06. Проверены несколько дел, заметки/связи, ревизии/rollback, ограниченные страницы/байты, забывание известных источников, сохранность старой схемы и WAL, черновик при переключении/ошибке/другой странице. Code Simplicity Review после исправлений принят. Type Scaffolder работал на чтение, Localization Mechanic обновил EN/RU; Scout/Librarian/Researcher результаты переиспользованы.

Сборка сохранена также в `target/checkpoints/Ambient-memory-07a.app`, её подпись отдельно проверена координатором. Проверено только существование известного пользовательского пути — каталог/БД отсутствуют; пользовательское содержание не читалось. Реальное восстановление через старую сборку и перенос данных не проверялись. Xcode CoreDevice/CoreSimulator warnings не сорвали macOS-сборку. UI визуально, устройство macOS 14 и Windows CI не проверялись. Это проверка памяти, не готовности инициативного помощника.

## Предыдущая основа 06 — 19 октября 2026

Тред `Основа 06 · Нативное приложение и память` завершил независимую часть D56. Координатор проверил итоговые команды и контракт хранения/границы. `yarn verify` завершился с кодом 0: 8 tooling, 15 Rust и 8 Swift tests, Rust check/rustfmt/Clippy, Swift build и strict format. Тесты используют настоящую временную SQLite и путь Swift–C–Rust; проверены повторное открытие, изменение/удаление, ревизии, пауза и ошибки. После исправления точного сравнения UTF-8 повторный полный прогон также успешен.

`yarn build:macos -quiet` и `codesign --verify --deep --strict` завершились с кодом 0. Собран `target/macos/Build/Products/Debug/Ambient.app`: arm64, минимум macOS 14, ресурсы EN/RU, ad hoc подпись. Использован официальный Rust 1.99.1 в `target/` с проверенным SHA-256 загрузчика. Системные настройки не менялись. Xcode сообщил о несовместимых CoreDevice/CoreSimulator, но macOS-сборка успешна; предупреждений линковщика о повышении minimum deployment target нет.

В реализации участвовали read-only Type Scaffolder, Localization Mechanic, External Researcher и Code Simplicity Reviewer; применимые результаты Scout/Pattern Librarian переиспользованы. Замечание reviewer об UTF-8 исправлено и повторно принято. Визуальный UI-прогон не запрашивался; работа на устройстве macOS 14 и Windows CI не проверялась. Облако, capture, карточки и полная 06 ещё не приняты. Это доказательство работоспособности первой памяти и нативной границы, а не готовности помощника.

## История — пустой каркас 18 октября 2026

18 октября 2026 года. Основа установлена в `/Users/gojda/Projects/ScreenCastApp`; существующие файлы не перезаписывались.

## Финальный рабочий каталог

`yarn verify` завершился с кодом 0 за 20.14 секунды. Пройдены agents:check, 8/8 тестов генератора, Rust check/rustfmt/Clippy/test и Swift build/strict format lint. Команда выполнена с обычными настройками SwiftPM после разрешения доступа инструменту к системному кэшу и запуску его sandbox. Изменений продуктовых исходников для обхода ошибки среды не потребовалось.

Дополнительно проверены все относительные ссылки новой документации и совпадение двух исходных brief. Пробел replay scope исправлен и повторно проверен Code Simplicity Reviewer. Windows и удалённый CI остаются непроверенными.

## Подготовленная копия

| Проверка | Результат |
| --- | --- |
| Генератор native инструкций | Созданы 122 файла; check без drift |
| Vitest generator suite | 8/8 passed |
| Rust check / rustfmt / Clippy с warnings denied | Passed, rustc/cargo 1.99.0 |
| Rust test | Passed, 0 tests — продуктовой логики ещё нет |
| Swift package build | Passed, Apple Swift 6.4, Swift 6 concurrency, warnings-as-errors |
| Swift project format lint | Passed |
| Provenance | 58 записей: 43 точные копии, 15 локальных адаптаций; 6 общих разделов дословно |
| Два исходных документа | Byte-for-byte совпадают с Downloads |
| Codex budget | Все root-to-scope цепочки помещаются в 65536 bytes |
| Workflow YAML | Оба файла успешно разобраны |

SwiftPM сначала не мог записать SDK cache и запустить вложенную sandbox в ограниченной среде агента. Проверенная команда использовала временные cache/config/security/scratch paths и `--disable-sandbox` только для SwiftPM subprocess sandbox. Это ограничение среды выполнения, не исправление исходников приложения. Обычные команды для разработчика остались стандартными.

## Независимые проверки

Вызваны Codebase Scout, Pattern Librarian, External Researcher, Plan Simplicity Reviewer, Evidence Auditor и Code Simplicity Reviewer. Архитектурных противоречий не обнаружено. Найдено и исправлено отсутствие явного подключения replay rules для fixtures в Codex/Cursor/OpenCode.

Type/endpoint/localization mechanics не требовались: runtime domain/transport declarations, provider registries и переводы не создавались. Visual Inspector не вызывался: визуальная проверка пользователем не запрошена, нового UI нет. Generic implementation workers не создавались.

## Границы проверки

Windows run и GitHub Actions не выполнялись. C# shell, C ABI, UI, capture, provider calls, replay corpus и качество моделей пока не реализованы/не проверялись. Native discovery всех четырёх клиентов не запускался; проверены generated formats и их согласованность. Local Rust 1.99.0 не равен pinned CI 1.99.1; это явно отражено в development. Commit и публикация не выполнялись.
