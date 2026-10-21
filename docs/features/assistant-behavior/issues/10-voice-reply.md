# 10: Голосовой ответ в карточке

Spec: `docs/features/assistant-behavior/spec.md`. Decisions: `docs/features/assistant-behavior/decisions.md`.

**What to build:** Добавить в существующую карточку локальный голосовой ответ: запись → галочка или отмена → Parakeet → редактируемый текст → отдельное подтверждение отправки.

**Blocked by (полная приёмка):** 04 — Локальный Parakeet; 06 — Первый работающий путь помощника.

**Начало независимой части (D56):** результат 04 и проверенная нативная основа 06 позволяют реализовать и изолированно проверить запись/отмену/распознавание/черновик после фиксации общего контракта ответа и остановки. В остановленном приложении голос не запускается; рабочий голосовой ответ и полная приёмка требуют полного 06 и соответствующих системных прав.

**Status:** partial — независимый10A готов к запуску по ADR-013; полная приёмка с настоящим микрофоном/UI/live Fast остаётся открытой.

- [ ] Используется проверенный в 04 вариант Parakeet. Кнопка «Записать» начинает запись и показывает галочку/крестик; речь не транскрибируется и не редактируется во время записи.
- [ ] Галочка завершает запись и запускает локальное распознавание, но не отправляет ответ. Готовый текст можно исправить; отправка происходит только отдельным подтверждением.
- [ ] Крестик отменяет запись без отправки. Ошибка микрофона/распознавания, тишина и пустой результат не создают подтверждённого ответа; текстовый ввод доступен независимо.
- [ ] Аудио не покидает компьютер и удаляется после обработки, ошибки и отмены; в облако может уйти лишь подтверждённый текст по общим правилам доступа и бюджета.
- [ ] Полная остановка отменяет запись и обработку с подтверждённым окончанием локальной работы, очищает звук и сохраняет уже готовый текстовый черновик. Поздний результат не затирает его и не отправляется.
- [ ] Облачная маршрутизация не ждёт локального распознавания/завершения отмены; UI сохраняет отзывчивость и адресата ответа.
- [ ] Проверен полный поток с русской и английской записью, редактированием, двумя подтверждениями, отменой, отказом разрешения и паузой; записаны реальные результаты и применимые проверки.


## Независимый пакет10A — локальный голос поверх08A

Договор: [ADR-013](../../../decisions/0013-local-voice-reply.md). Блокирующие входы: принятые04 и08A, подтверждённый read-only result getter и неизменённая build/link проба FluidAudio. Все входы проверены; итоговые Plan Review/Evidence Audit PASS19 октября2026. Реальное разрешение микрофона не требуется для разработки и публичного WAV-прогона; полная10 его сохраняет.

**Status:** local-DONE, ожидает приёмки управляющим — тред01a1540c-62a4-7533-9a20-84e9e5e4490a, GPT-6.1 Sol xhigh. Один владелец общей native границы. Код и локальные проверки выполнены; настоящий микрофон/UI/live Fast остаются вне этой приёмки.

**Результат:** из текущего уточнения начать локальную запись, закончить её галочкой, распознать в отдельном настоящем helper, применить текст к прежнему общему черновику08A и отдельно отправить. В коде есть настоящий microphone adapter, но без TCC/UI-прогона он не объявляется фактически принятым.

**Владение:** src/platforms/macos, необходимые существующие build/package/Cargo файлы и узкие Core/bridge/providers follow-up только для интеграции/тестов без изменения модели/auth/расходов. Module README и эта секция10A. Shared docs/ADR/spec/execution и checkpoints принадлежат управляющему. Schema6/memory3/auth1/source1/model3 сохраняются; новый Store/coordinator/ledger/scheduler/ABI и production mock не нужны.

- [x] Один native voice owner на карточку и общую панель; текущий receipt/clarification проверяется через existing getter, черновик имеет собственное поколение. Правка/отправка не выполняются во время записи; отмена не портит старый текст, замена непустого текста явная.
- [x] Настоящие AVAudioApplication/AVAudioRecorder и usage description; permission только после нажатия. Позднее разрешение после cancel/shutdown не начинает запись, выход не ждёт callback без recorder/helper.
- [x] PCM16 mono16kHz,60s/2MiB; закрытый WAV до helper. На пределе запись ждёт галочки, транскрипции во время записи нет.
- [x] Подписанный вложенный helper с неизменённым FluidAudio0.17.7 exact commit/traits[], offline loadLocal/v3, streamingEnabled:false и проверкой21файла inventory04 внутри общего300s deadline. Модели выбираются явно, не скачиваются/не перераспространяются. Точные notices входят в app, Nemo не линкуется/не поставляется.
- [x] Результат ограничен16KiB UTF8/32KiB channel; silence/error/oversize не отправляются и не обрезаются. Нет личного текста/аудио/путей в логах и неограниченных pipes. Готовый текст идёт в existing preview/Send, без автоматической записи в task memory.
- [x] Отмена SIGTERM→250ms→SIGKILL ждёт фактического выхода. Voice не занимает ChatGPTCoordinator; независимый HTTP заканчивается во время stop локального child. Shutdown ждёт реальные native ресурсы, MainActor не блокируется.
- [x] Independent flock leases и parent-only EOF exit; cleanup после exit и при следующем запуске только собственных проверенных каталогов. Ошибка очистки возвращает текстовый ввод, запрещает новую запись до успешной очистки. Проверка root ограничена64каталогами/256MiB и резервирует место для новой операции; неполный обход не считается успехом.
- [x] TDD: реальные child process успех/ошибка/oversized output/deadline/отказSIGTERM/exit race/смерть родителя при зависшем compute; удаление и замена inode до helper lock; pause/revoke/target/draft fences; поздние permission/ASR; bounded storage и реальная cleanup failure.
- [x] Настоящий упакованный helper проходит публичные RU/EN WAV04, silence/invalid/cancel, отсутствие unowned raw derivatives и сохранность public fixtures. Существующие испытанные веса читаются только по точному явно заданному пути; production не ищет cache самостоятельно.
- [x] Сквозной временный Store→C/Swift→localHTTP/SSE clarification→реальный ASR→editable draft→exact preview→один Send. Никаких live API/Jev и фактического микрофона при этом прогоне.
- [x] Full yarn verify, feature Swift suite, app build/strict signatures, все Mach-O без test hooks/старыхABI/Nemo, audit link/resources/notices. Сохранять полные build logs с upstream warnings в файлы до краткого вывода. Все90файлов10 прежних checkpoints неизменны.
- [x] Обязательные Scout→Librarian, Type Scaffolder read-only, Localization Mechanic EN/RU и Code Simplicity Review выполнены. Важную логику/типы пишетSol. Итог честно отделяет локальные проверки от реального микрофона/UI/macOS14hardware/Windows/live Fast.

**Не закрывает:** весь10, реальное качество новой диктовки/устройства/permission denial, UI/focus/Spaces, voice→live Fast, полные07/08/09 и автоматические источники. После DONE управляющий принимает отчёт и создаёт новый checkpoint.


**Историческая локальная проверка19 октября2026 (tool outputs восстановлены из чата; исходные `/tmp` логи утрачены):** `yarn verify` exit0 —8 tooling/105 Rust/69 Swift, прежний cleanup regression65.264s. Полный test-support Swift suite с настоящим упакованным ASR —95 tests exit0, cleanup64.739s; all-features Clippy и `yarn build:macos` — exit0. Отдельный packaged smoke под deny network: RU/EN short/long, silence, invalid, missing models, cancel —8/8; отрицательный network control EPERM, global raw0→0, actual exit/cleanup подтверждены. SHA256 всех21 model files и4 публичных WAV сохранены.

**Артефакт:** оба bundle имеют runtime signature; Audio Input entitlement только у main app, helper без entitlements. Debug main сохраняет штатный get-task-allow. Во всех4 Mach-O отсутствуют test hooks/modelv1/v2/Nemo;14 modelv3 exports определены. Link graph, оба resource bundles, точные Apache/BSD notices и351 EN/RU key проверены. Неизменённый upstream exact commit подтверждён в source mirror и build checkout. Все90 файлов10 прежних checkpoints, их inventory и strict deep signatures сохранены; новый checkpoint не создавался. Утраченные полные build logs не восстановлены; происхождение исторических результатов записано отдельно. Новый полный build log20 октября содержит неблокирующие системные диагностики Xcode CoreDevice/CoreSimulator/AppIntents.

**Специалисты:** Scout→Librarian, Type Scaffolder read-only, Localization Mechanic EN/RU, Code Simplicity Reviewer и узкая проверка Apple Audio Input entitlement — выполнены. После ревью исправлены привязка cancellation к конкретному child и main-app entitlement; дополнительно проверены startup cleanup и общий лимит вложенных каталогов. Итоговый focused review — PASS. Core/bridge/providers не менялись.

**Повторная приёмка20 октября2026:** full verify8 tooling/105 Rust/69 Swift; feature suite95 Swift с настоящим packaged ASR и cleanup65.032s; all-features Clippy/build/signatures/audit — PASS. Публичные WAV04 заново получены из pinned FLEURS и совпали по SHA256;21 model file не изменился. Packaged deny-network smoke8/8, raw0→0; отмена проверена при подтверждённо живом startup helper, без утверждения об active CoreML cancellation. Первая smoke-проба с задержкой0.5s завершилась ошибкой самой проверки, потому что helper уже успешно вышел; failure log и исходная проба сохранены.

Временный mirror удалён из активной конфигурации. Новая штатная резолюция официального URL получила неизменённый commit503b4bd1bbf7220882de39fe8ae6716aae4132da; identity в Package.resolved SwiftPM автоматически сменил с source на fluidaudio, revision не менялась. Swift/C логика не менялась; новых production исправлений нет, исторический specialist review остаётся применимым.90 нынешних checkpoint-файлов сохранены внутри повторного прогона по новому baseline20 октября. Историческое90unchanged — только прошлый tool output из чата, без нового исторического hash-сравнения.

**Передача:** `target/verification/10a/handoff.txt`; исходная область38 файлов — `target/verification/10a/10a-scope-files.txt` (восстановленный scope inventory, не новый Git diff); актуальные hashes — `target/verification/10a/source-hashes-current.json`; итоги/полные новые логи — `target/verification/10a/verification-summary.json`. Исторический handoff и tool outputs находятся отдельно в provenance. Shared product/privacy/spec/execution и итоговый checkpoint остаются управляющему после приёмки.
