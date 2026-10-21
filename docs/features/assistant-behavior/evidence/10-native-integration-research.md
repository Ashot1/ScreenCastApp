# Подготовка голосового ввода10 — 19 октября2026

Это исследования, не принятый договор голосового ввода или его реализация. 06F и 11A приняты локально, 08A выполняется отдельно. Микрофон, TCC, UI, ASR и новые модельные запросы здесь не запускались. Отдельная проверка жизненного цикла использовала только тестовые дочерние процессы. Пользовательские записи не читались; веса не скачивались.

## Зависимость от ответа

Scout подтвердил:06F имеет сохранённый task context, exact preview, результат/receipt и copy. Редактор ручной заметки не является composer ответа конкретной карточке. Для10 нужен договор адресата/черновика/отдельного подтверждения, stale target и сохранения готового текста при паузе; карточка и проактивный цикл пока не готовы. ADR-010 позволяет сохранить уточнение ручной заметкой и повторно запросить помощь, но это не реализованный голосовой ответ. Полная10 сохраняет04/06 и реальные microphone permissions. Независимый Plan Reviewer рассматривает следующий полезный пакет после11A; решение ещё не принято.

## Нативная запись и процесс

Librarian выполнил только Swift typecheck с macOS14 target. AVAudioRecorder поддерживает WAV/linearPCM, record(forDuration:), stop() с закрытием файла и deleteRecording() после stop. AVAudioApplication.shared.recordPermission/requestRecordPermission доступны с macOS14; нужен NSMicrophoneUsageDescription, которого в текущем app target ещё нет. Это доказательство API, не работающего микрофона. Конкретные пределы записи/памяти и реакция на них ещё не выбраны.

Process.terminationHandler установлен до run; один владелец может ждать completion через continuation без блокировки MainActor. waitUntilExit блокирует и не гарантирует окончание handler. terminate посылаетSIGTERM; SIGKILL требует Darwin kill с проверенным владением PID/lifetime. Cancellation request не освобождает worker. Текущий Xcode проект имеет одну app target, helper/embedding ещё нет. Native executable может быть вложен в допустимый bundle path и подписан до внешней app; отдельный sandbox/XPC договор пока не выбран.

Опоры: [AVAudioRecorder](https://developer.apple.com/documentation/avfaudio/avaudiorecorder), [permission](https://developer.apple.com/documentation/avfaudio/avaudioapplication/requestrecordpermission%28completionhandler%3A%29), [Process](https://developer.apple.com/documentation/foundation/process), [terminationHandler](https://developer.apple.com/documentation/foundation/process/terminationhandler), [Apple TN2206](https://developer.apple.com/library/archive/technotes/tn2206/_index.html).

## FluidAudio: испытанный runtime и упаковка

External Researcher подтвердил последний обозначенный release FluidAudio0.17.7, commit503b4bd1bbf7220882de39fe8ae6716aae4132da. Повторно проверены официальные API AsrModels.loadLocal(from:version:.v3,encoderPrecision:.int8), AsrManager/loadModels/transcribe completed WAV/cleanup и ASRConfig(streamingEnabled:false). ModelHub.offlineMode=true до загрузчиков запрещает библиотечные сетевые downloads, но не создаёт OS network sandbox. Runtime Apache2; weights/conversion metadata CC BY4, противоречивый Apache footer нельзя выдавать за Apache-only веса. Настоящие RU/EN/cleanup/quality результаты остаются в [отчёте04](04-parakeet/report.md).

У Swift>=6.2 traits:[] отключает Nemo linking, но объявленный unconditional binaryTarget может скачиваться SwiftPM даже без trait.04 убирал target только во временном upstream manifest; приложение этого патча не принимало. Currentmain имеет новые TTS/Diarizer traits, но unconditional Nemo target остаётся. Официальный способ гарантированно избежать unused build archive без правки пакета не подтверждён. Это вопрос воспроизводимой упаковки/лишнего build artifact, не опровержение работающего local ASR; выбирать runtime update или production patch по этому отчёту нельзя. Версия Rust wrapper0.14.8 не сохраняет испытанный runtime и не является обходом.

На этапе первоначального исследования до implementation требовались выбранный способ подключения без замены runtime и доказательство сборки. Теперь ADR-013 и приведённый ниже build proof закрывают эти условия. Конечные helper/signature/offline ownership и отсутствие лишнего artifact в Ambient app проверяются при реализации. Неподтверждённое отсутствие build download не следует само по себе считать запретом всей разработки или требованием менять продукт. Модельные веса не перераспространялись; Intel/universal/macOS14 hardware и установленный helper ещё не проверены.

Опоры: [release](https://github.com/FluidInference/FluidAudio/releases/tag/v0.17.7), [pinned manifest](https://github.com/FluidInference/FluidAudio/blob/503b4bd1bbf7220882de39fe8ae6716aae4132da/Package%40swift-6.2.swift), [local loader](https://github.com/FluidInference/FluidAudio/blob/v0.17.7/Sources/FluidAudio/ASR/Parakeet/SlidingWindow/TDT/AsrModels.swift), [ModelHub](https://github.com/FluidInference/FluidAudio/blob/v0.17.7/Sources/FluidAudio/Shared/Download/ModelHub.swift), [original weights](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3/blob/541d1f99c6b0c3cd0b11a95167540bb8edefd82b/README.md).

## Уточнение: штатная упаковка без правки upstream

Дополнительная проверка19 октября снимает прежнюю слишком сильную оценку build download как общего блокера. Неизменённый FluidAudio0.17.7 с `traits: []` — обоснованный кандидат для helper на Swift6.4. По закреплённому manifest связь цели FluidAudio с NemoTextProcessing условна по одноимённому trait; пустой набор отключает default traits. В исходниках вызовы защищены `canImport(CNemoTextProcessing)`. Если другой потребитель не включает trait, Nemo не должен линковаться или копироваться в конечный продукт. SwiftPM может отдельно скачать архив при разрешении зависимостей; это не означает поставку в приложении. На этом этапе конечная сборка ещё не была проверена; результат отдельной неизменённой сборки ниже — PASS.

NemoTextProcessing.xcframework.zip v0.3.1 — статическая библиотека нормализации текста, не ASR-веса:87267919байт, SHA256 `5fa8c10d4ec26c1bb2413125f351a7222a4c68a23b74476680fbada7e26fc6aa`. Размер подтверждён GitHub release metadata без скачивания архива. Наборы: macOS arm64/x86_64, iOS arm64, Simulator и Catalyst arm64/x86_64. [Метаданные](https://api.github.com/repos/FluidInference/text-processing-rs/releases/tags/v0.3.1), [релиз](https://github.com/FluidInference/text-processing-rs/releases/tag/v0.3.1), [Swift traits](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0450-swiftpm-package-traits.md).

У пакета нет внешних package dependencies. После opt-out остаются FluidAudio, внутренние FastClusterWrapper/MachTaskSelfWrapper и системные библиотеки. Безусловный TTS resource bundle содержит два словаря суммарно1019385исходных байт, не TTS-веса; учитывать его в аудите поставки. Runtime Apache2, fastcluster BSD2-Clause с уведомлением; прежние ASR-веса CC BY4. Deployment minimum macOS14, opt-out требует Swift>=6.2.

Следующая тогда запланированная проверка неизменённого пакета и link inputs выполнена ниже; содержимое конечного Ambient helper/app остаётся проверкой реализации. Для неё не нужны новые inference или платные вызовы. Новая версия runtime и production-патч manifest не требуются по имеющимся данным. В этом исследовании архивы/веса не скачивались, сборка и распознавание не запускались; полный аудит notices и установленный helper ещё впереди.

## Независимая альтернатива10A после08A

Plan Simplicity Reviewer предложил один законченный пакет: штатная запись → отдельный одноразовый Swift helper → существующий reply draft08A → прежний final Send. Предлагается сначала доказать упаковку helper, затем подключать запись и полный путь; отдельный продуктовый голосовой стенд не нужен. Это подготовка решения, не принятая спецификация или начало реализации.

Core проверяет адресата и общий допуск, native владеет recorder/процессом/временными файлами; voice не занимает ChatGPTCoordinator. Helper получает только закрытый WAV и явно выбранные проверенные модели, не credentials. Перед применением текста повторно проверяются адресат и версия черновика. Для первого пути достаточно локального каталога с inventory04; downloader/поставка483MB весов не обязательны.

Предложенные ресурсные пределы ещё требуют фиксации в договоре: запись60s monoPCM16/16kHz и2MiB WAV; helper deadline300s учитывает измеренный cold load140s. На пределе запись закрывается, но ASR ждёт галочки. Отмена использует SIGTERM, затем SIGKILL после grace и ожидание настоящего завершения; свободный slot нельзя объявлять раньше. Cloud не ждёт voice cleanup, app shutdown ждёт. После crash возможны остатки: очистка при старте должна касаться только собственных заведомо неактивных каталогов, без обещания физического стирания или удаления ещё работающего helper. Точный crash/ownership договор ещё не выбран.

Без разрешения микрофона можно проверить подписанный helper, настоящий законченный WAV, supervisor на тестовых child processes, target/draft fences и независимость HTTP во время остановки ASR. Это не заменяет реальный микрофон, маршруты устройств, качество новой диктовки, UI и voice→live Fast. Эти границы сохраняются в полном10.

## Проверка очистки после аварии

External Researcher проверил на текущем macOS 27 трёхпроцессный fixture в `/private/tmp`, используя стандартный Python `fcntl.flock`, который вызывает Darwin `flock`. Родитель и helper независимо открывали один lease-файл и удерживали shared lock. После аварийного выхода родителя cleaner не мог получить exclusive nonblocking lock, пока helper оставался жив. После фактического выхода helper lock стал доступен. Результат:

```text
PARENT_CONFIRMED_INDEPENDENT_HELPER_LOCK
PARENT_EXIT_CONFIRMED
CLEANER_WHILE_HELPER_ALIVE=BLOCKED errno=35
CLEANER_AFTER_HELPER_PROCESS_EXIT=ACQUIRED
PROOF=PASS
```

Это позволяет отличать ещё занятую операцию без проверки PID, но только для сотрудничающих процессов, соблюдающих протокол. Предлагаемый договор: свежий непереиспользуемый private каталог операции; родитель держит shared lock до реального завершения helper; helper независимо открывает существующий lease без создания и держит свой lock до выхода процесса. Cleaner удаляет только проверенный собственный каталог после успешного exclusive nonblocking lock и удерживает lock до конца удаления. Ошибка, занятый, отсутствующий или повреждённый lease не являются разрешением на удаление потенциально живой операции. Lease нельзя пересоздавать во время работы: новый inode создаёт другую область блокировки.

Между запуском helper и получением им lock возможна гонка с cleaner после аварии родителя. После shared lock helper должен подтвердить существование исходного каталога и идентичность lease по исходному пути, owner/type и `st_nlink == 1`, без следования symlink. Затем он открывает существующий WAV без создания. Lock на уже удалённом inode не разрешает обработку. Отдельный race fixture затем выполнен и прошёл: helper взял shared lock на уже отвязанном inode, но обнаружил `st_nlink=0`/отсутствующий путь и отказал до открытия WAV (0 попыток). В обратном порядке, когда helper успел взять shared lock первым, cleaner получил errno35 до фактического выхода helper. Код и лог сохранены в `/private/tmp/ambient-flock-race-proof.Hy21TP`; операционные каталоги удалены. Не проверены подмена каталога новым inode и несотрудничающий процесс.

Pipe EOF может дополнительно сообщить helper о смерти родителя. Writer принадлежит только родителю; унаследованная лишняя ссылка задержит EOF. EOF или SIGTERM сами по себе не доказывают завершение inference, поэтому lease сохраняется до выхода. Swift `FileHandle` должен иметь явного долгоживущего владельца: закрытие последней ссылки освобождает lock. Новые daemon/XPC для этой задачи не обоснованы.

Границы: advisory lock не защищает от несотрудничающего процесса того же пользователя. Проверены системный lock lifetime и локальная файловая система текущего хоста, не Swift Process интеграция, FluidAudio shutdown или macOS 14 hardware. Первый fixture ошибочно ожидал унаследованный stderr pipe; после исправления владения FD и handshake проверка прошла. Fixture удалён; личные данные, TCC и ASR не использовались.

Опоры: Apple [flock(2)](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/flock.2.html), [close(2)](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/close.2.html), [pipe(2)](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/pipe.2.html), установленный SDK `Foundation/NSFileHandle.h`.

## Результат неизменённой сборки

Pattern Librarian собрал отдельный SwiftPM proof с закреплённым commit `503b4bd1bbf7220882de39fe8ae6716aae4132da` и `traits: []`. Upstream manifest/исходники не менялись. После обрыва большого remote clone использовано локальное зеркало того же проверенного commit; это доказательство сборки и линковки, не надёжности GitHub clone. `Build complete! (92.49 s)`. Все материалы находятся в `/tmp/ambient-10-packaging-proof`.

Исполняемый arm64 `Proof` размером 10174744 байт действительно содержит FluidAudio; deployment minimum macOS14. Проверены `import FluidAudio` и компиляция создания AsrManager. В link inputs нет Nemo object/archive, в бинарнике нет его C API symbols или dynamic dependency. SwiftPM всё же скачал83.3MB XCFramework и распаковал около197MB — это build artifact, не связанная библиотека. Resource bundle содержит два TTS dictionary файла982385 и37000байт, около1MB. В bundle нет license notice; приложение должно отдельно включить необходимые уведомления.

Бинарник не запускался: веса не загружались, ASR/аудио/микрофон не использовались. Build на Swift6.4/SDK27 с target14 не подтверждает запуск на macOS14. Упаковка конечного Ambient helper, подписи, notices, точный ASR configuration и реальный публичный WAV остаются проверками10A. Эта проба снимает стартовое условие неизменённой сборки библиотеки, но не заменяет эти проверки.

Handoff: `/tmp/ambient-10-packaging-proof/handoff.txt`; executable: `build/out/Products/Release/Proof` внутри этого каталога. Управляющий отдельно проверил clean upstream checkout, SHA SwiftPM checkout, четыре действительных link inputs и LC_BUILD_VERSION minos14.0. Весь verbose build log не сохранён из-за усечения вывода, поэтому точное число upstream warnings неизвестно; «без предупреждений» не заявляется. В10A build log должен сохраняться в файл до вывода краткого результата.
