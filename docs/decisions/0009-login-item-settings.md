# ADR-009: управление автозапуском macOS

Статус: независимый06E реализован и принят19 октября2026 после06D. Plan Simplicity Review, Evidence Audit и Code Simplicity Review пройдены. Требование — D28 и§7 спецификации поведения. Первое автоматическое включение после полноценной настройки помощника остаётся отдельной интеграцией06.

## Результат

В Settings пользователь видит системное состояние запуска Ambient при входе в macOS, может явно включить/выключить его и открыть системную страницу для подтверждения. Настройки доступны при остановленном помощнике. Автозапуск приложения никогда не снимает ручную паузу и не включает неподтверждённый анализ.

Этот пакет реализует управление действительной регистрацией mainApp; не заменяет её своим постоянным флагом. Во время разработки не регистрировать сборку на компьютере пользователя и не запускать UI. Реальная регистрация и запуск после входа остаются непроверенными до отдельного испытания.

## Решение

- Использовать ServiceManagement.SMAppService.mainApp, доступный с macOS13 при minimummacOS14. Новые packages, helper, LaunchAgent plist, UserDefaults и Core/FFI изменения не нужны.
- Единственный потребитель — Settings. Его небольшой @MainActor UI owner хранит только текущий снимок native SMAppService.Status и ошибку UI. Не добавлять ещё одного владельца в AppDelegate без второго фактического потребителя. Системная регистрация остаётся владельцем постоянного значения.
- Читать status при появлении Settings, после enable/disable и возвращении Settings в active через штатный SwiftUI scenePhase. Нет polling, KVO-подписки или своего наблюдателя ОС. На старте не вызывать register и не отменять системный отказ.
- Для notRegistered — явное включение. Для enabled — явное выключение. Для requiresApproval — понятно сообщить о необходимом действии в macOS, предоставить переход в System Settings и возможность убрать регистрацию. Для notFound и неизвестного будущего enum case — честное недоступное состояние, без выдуманного enabled. Можно перечитать статус; автоматических повторов нет.
- register/unregister — штатные синхронные throwing API. После успеха и ошибки перечитать фактический status: успешный возврат не заменяет чтение состояния. Ошибка действия отображается локализованным кратким сообщением, не сырым NSError/путём; ни ошибка, ни открытие системных настроек не означают enabled. Следующее явное действие снимает старую ошибку.
- openSystemSettingsLoginItems вызывается только кнопкой пользователя. Не конструировать URL вручную и не изменять настройки через автоматизацию.
- Состояния и действия локализованы одновременно EN/RU. Использовать прямой native enum, не вводить transport/domain копию. Если для тестов нужен seam, он внутренний и узкий, аналогичный имеющимся injected closures; production всегда использует настоящий SMAppService.

## Проверка и границы

Meaningful TDD с test-only службой/closures: начальный/повторный refresh не регистрирует; enable/disable вызывает ровно выбранную операцию и показывает фактический status; ошибка сохраняет правду о status и позволяет повторить явное действие; requiresApproval не трактуется как enabled; внешний отзыв обнаруживается при refresh; переход в настройки не вызывает register. Не тестировать десятки копий текста или сам SwiftUI setter.

Полный yarn verify и app build/signature после интеграции. Сохранить регрессии памяти,06B/06C/06D и все принятые checkpoints. Code Simplicity Review готовой логики; Localization Mechanic; Scout/Librarian/Researcher результаты ниже переиспользуются после проверки актуальных мест06D. Type Scaffolder нужен только если фактически меняются не-UI domain/transport/FFI declarations; здесь они не запланированы.

Не менять credentials, Core память/паузу/журнал, сбор данных, облачный transport или расходные лимиты. Не делать авто register после sign-in: вход ChatGPT сам по себе не означает полноценное первое включение помощника и выдачу всех разрешений. Этот будущий hook не подменяется мёртвым кодом или скрытым side effect. Не утверждать полный D28/06, пока actual login и интеграция first enable не проверены.

Текущая ad-hoc подпись и расположение development app не доказывают ни успех, ни невозможность mainApp registration. Apple требует подпись; обязательность /Applications или notarization именно для mainApp не подтверждена. Не вводить такие блокировки по аналогии с LaunchDaemon. Реальные OS ошибки должны отображаться честно.

## Основания

Проверено19 октября2026; Git отсутствует. Места реализации повторно проверяются после06D, сейчас его владелец имеет исключительный scope.

- `src/platforms/macos/Sources/AmbientMacOS/AssistantSettingsView.swift`: Settings, единственный будущий потребитель. `Package.swift`, `Tests/AmbientMacOSTests`: native module/test seams.
- `src/platforms/macos/App/AmbientApp.swift`: один delegate и запуск чтения сохранённой паузы, который не меняется этим пакетом.
- `src/platforms/macos/Ambient.xcodeproj/project.pbxproj`: minimummacOS14, code signing ad hoc, bundle dev.ambient.foundation, без sandbox.
- Installed Xcode macOS27 SDK `ServiceManagement.framework/Headers/SMAppService.h`: API13+, Status cases, throwing register/unregister, requirement code signed. Librarian успешно typechecked точные вызовы/четыре enumcases для deploymenttarget14 без выполнения системных операций.
- [Apple mainApp](https://developer.apple.com/documentation/servicemanagement/smappservice/mainapp), [register](https://developer.apple.com/documentation/servicemanagement/smappservice/register()), [unregister](https://developer.apple.com/documentation/servicemanagement/smappservice/unregister()), [status](https://developer.apple.com/documentation/servicemanagement/smappservice/status-swift.property), [requiresApproval](https://developer.apple.com/documentation/servicemanagement/smappservice/status-swift.enum/requiresapproval), [системная страница login items](https://developer.apple.com/documentation/servicemanagement/updating-helper-executables-from-earlier-versions-of-macos).

## Результат реализации

Settings использует LoginItemView и локальный LoginItemModel; системный статус не сохраняется второй копией. Восемь новых Swift tests прошли RED → GREEN. Замечание code review об очистке ошибки явным refresh исправлено и проверено повторно; Localization Mechanic добавил14 ключей EN/RU. Принятые Scout/Librarian/Researcher findings сверены с текущим кодом/SDK; non-UI контракты не менялись.

Финальные verify/build/codesign — exit0:8 tooling/62 Rust/46 Swift tests. Сохранность54 файлов шести прежних checkpoints и их подписи подтверждены. Координатор сохранил седьмую сборку target/checkpoints/Ambient-login-06e.app и отдельно проверил подпись. Фактическая регистрация/login, UI, macOS14 hardware и Windows не проверены; системных изменений и модельных запросов не было. Полные D28/06 остаются открытыми.
