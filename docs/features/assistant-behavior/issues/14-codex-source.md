# 14: Codex как источник текущей работы

Spec: `docs/features/assistant-behavior/spec.md`. Decisions: `docs/features/assistant-behavior/decisions.md`.

**What to build:** Добавить подтверждённое чтение Codex в общий цикл: разговор, точный промпт, план и завершение хода помогают выбрать следующий шаг, пока запуск агента остаётся у пользователя.

**Blocked by (полная приёмка):** 02 — Чтение работающего Codex; 07 — Несколько дел и связи между ними.

**Начало независимой части (D56):** после проверки используемой памяти и контракта источников 07; не требуется ждать модельной приёмки всего 07. Для 14 также нужен подтверждённый результат 02. Общие изменяемые контракты сначала завершает их владелец. Можно реализовать и проверить собственный реальный путь, но инициативные функции в остановленном приложении не работают. Полная приёмка сохраняет перечисленные зависимости и все критерии ниже; синтетический тест не заменяет настоящие подключения.

**Status:** ready-for-agent

- [ ] Использован подтверждённый результат 02 на совместимой версии; незакрытый G1 не обойдён запуском другого сервера с изменением профиля или продолжением агента.
- [ ] Существующие чаты/ходы, доступный разговор, реальные ссылки и промпты читаются с происхождением и полнотой. Сохранённое завершение различается с неизвестным статусом/активностью другого процесса.
- [ ] Наблюдаемый план и выполненные зависимости позволяют общей логике подготовить следующий применимый промпт с точной ссылкой; номер волны не увеличивается механически после любого завершения.
- [ ] Если план или подтверждение проверки отсутствуют, помощник уточняет или честно обозначает недостаток данных, не выдумывая содержание задачи.
- [ ] Пользователь копирует промпт и сам запускает следующего агента. Копирование не считается запуском; отдельное событие или сообщение пользователя подтверждает продолжение.
- [ ] Чтение не запускает, не возобновляет, не редактирует и не архивирует чаты/агентов. Обновление версии, задержка сохранения и потеря доступа дают видимое ограничение.
- [ ] Проверены переход между приложениями, завершение одного шага без закрытия дела, пауза/удаление во время чтения и поздний результат. G6 для источника подтверждён.
- [ ] Полный пример нескольких волн вместе с Jira и повторными напоминаниями оставлен итоговой совместной проверке; здесь нет зависимости от ещё не добавленных источников и нет отдельного движка этого сценария.


## 14A: выбранная сохранённая история Codex

**Status:** implemented-local-pass —20 октября2026; тред `01a1587d-76d4-7031-ac72-bec377dd97fa`, Sol xhigh. Ожидает приёмки координатором; полный14/G6 не закрыт.

**Договор:** [ADR-014](../../../decisions/0014-selected-codex-history.md). Finished Plan Review и Evidence Audit PASS19 октября2026; [исследование](../evidence/14-codex-integration-research.md). Это реальный ограниченный источник→память→помощь, не завершение полного14/G6.

**Входы:** принятые07A/07B,06F,11A,08A;04/10A сохраняются без регрессии; результат02 применяется только к pinnedформату, без переноса PASS на новый reader. Один Sol-владелец Core/bridge/native границы.

**Что реализовать:** выбранный rollout → bounded native FD read/window → Core paginated parser/projection → explicit bind в прежний Store → существующие preview/Send/result/copy. Пользователь явно читает «Ранее»/«Обновить»; автоматического поиска, polling, индекса, записи/запускаCodex нет.

**Граница записи:** src/core/**, src/bridge/**, src/platforms/macos/**; src/providers/** только нужный source context и тестовая интеграция, без смены auth/endpoints/model/tier/budget. Необходимые manifests/build scripts, module READMEs и эта секция14A. Shared docs/ADRs/execution/agent-sources/checkpoints принадлежат координатору. prototype, feasibility и личные sessions вне доступа.

### Критерии14A

- [x] Core owns settings/revisions/admission/parser/bind; native owns bounded FD I/O, без второго Store. Новый narrow Codex ABI1 использует тот же memory handle; Git ABI1/memory3/auth1/model3 сохраняются. Schema7 отвергает6 read-only без sidecars/reset/migration.
- [x] Exact cli_version0.163.0-alpha.10.2 и explicit paginated; header.id — thread identity. history_base/forked_from_id/forked_from_ordinal_exclusive non-null, включая ordinal0, дают unsupported. Subagent boundary проверяется по embedded ordinal до projection. Wrong types/missingversion/unknownschema не подменяются default.
- [x] Header256KiB+1, window1MiB+1, line256KiB/4096lines,128items/256KiB. Physical-line cursor продвигается без нормализованныхitems и не перескакивает непринятые строки при лимитах. Большой файл не загружается целиком. Partialtail, append/refresh, replacement/truncation/headerchange проверены.
- [x] Canonical item_completed thread/turn/item identity, dedupe/snapshot updates; rawresponse_item не второй prompt. Human_unverified/delegated/agent/plan/structured statuses различаются. Final phase не terminal; отсутствиеterminal не running. Plan не подтверждённый Core plan.
- [x] Codex metadata принадлежит source binding, возвращается с evidence ID через Codex ABI; SourceFact/time/version переиспользуются. IDs<=2048bytes, combined evidence/key/version/metadata<=16KiB, metadata учитываются в64MiB и50rows/256KiB paging. Local_window scope сохраняется после reopen без rawwindow storage.
- [x] Start/terminal используют честную помеченную projection structured fact, не выдуманную цитату; пустой обычный item не получает fabricatedtext. Rawerror/last_agent_message не импортируются.
- [x] Evidence/association/ordinal/hash/taskrevision записываются одной Store transaction. Failure/conflict/limit/tombstone не оставляют частичных изменений. Monotonic ordinal действует между settingsrevisions; одинаковыйevent из другогоwindow не ложныйconflict. Duplicatefile не меняетprovenance; protecteduseredits/forgottenkeys сохранены.
- [x] Pause/delete/revoke/settingschange отменяют оба source jobs; actualFDclose доslotrelease. Late read/result не возвращает доступ. Все prepare/dispatch/result/copy/follow-up проверки охватываютuncitedsource. Самостоятельный replydraft08A/10A сохраняет своё поведение.
- [x] Настоящий tempJSONL → native reader → Core/SQLite → localHTTP/SSE → result/copy проверен. Sourcehashes неизменны, никакого live/model/Jev, личнойистории, TCC или запуска productionapp.
- [x] Scout→Librarian, Type Scaffolder READ-ONLY, Localizer EN/RU и Code Simplicity Review выполнены. Важный код/типы пишетSol. Все требуемые ADR014 cases покрыты meaningful tests RED→GREEN.
- [x] Full yarn verify, featureSwift, all-featuresClippy, appbuild/strictsignatures, allMachO ABI/hook audit и сохранность всех старыхcheckpoints PASS. Отдельно перечислены непроверенные UI/macOS14hardware/Windows/liveFast/fullG6.

**Проверено20 октября:** `yarn verify` exit0 —8 tooling/135 Rust/70 Swift; feature Swift exit0 —101tests (real ASR намеренно skipped); all-features Clippy, appbuild и artifact audit exit0. Четыре Mach-O,15 CodexABI1/14 modelABI3 exports, production `testAfter*`/oldABI отсутствуют; EN/RU382keys/31Codexkeys. Подписи currentapp/helper и11 прежних checkpoints PASS,114 checkpointfiles неизменны. Новый checkpoint не создавался. Code Simplicity Review принял P2 и P1 после исправлений. Ограничение same-process rawSQLite и app-owned гарантия описаны в [SQLite review](../evidence/14-sqlite-admission-review.md); исходный synthetic repro сохранён, исправленным не объявлен. UI/macOS14hardware/Windows/liveFast/realASR/fullG6 не проверены.

**Отчёт:** `target/verification/14a/handoff.txt`, `verification-summary.json`, `changed-files-current.json`, `specialists-and-review.json`, `app-checkpoint-proof.json`; копия handoff: `/tmp/ambient-14a-handoff.txt`. Все команды имеют отдельные logs/JSON с exitcode и временем. QUESTION/BLOCKED/DONE через собственныйfinal; координатор читает wait/read и отвечает. Владелец проекта явно разрешил ответы координатору. Если этот канал недоступен — остановка. Не делать обратныйsend вroot, commits/PR/gitinit или самостоятельныйзапуск следующегоimplementer.
