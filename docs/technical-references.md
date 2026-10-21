# Технические источники

Проверены при подготовке основы 17–18 октября 2026 года. Подтверждают возможности API, а не качество или скорость Ambient.

| Решение/ограничение | Первичный источник | Следствие |
| --- | --- | --- |
| Rust `staticlib` / `cdylib` для foreign linking | [Rust Reference](https://doc.rust-lang.org/reference/linkage.html) | Один core с отдельной узкой C boundary |
| Swift импортирует C | [Swift interop](https://www.swift.org/documentation/cxx-interop/) | Native wrapper не требует второго domain core |
| .NET native interop | [Microsoft ABI](https://learn.microsoft.com/dotnet/standard/native-interop/abi-support) | LibraryImport и явное владение unmanaged data |
| Swift Windows toolchain существует | [Swift Windows](https://www.swift.org/install/windows/) | Альтернатива рассмотрена; пользователь выбрал Rust |
| ScreenCaptureKit и доступ пользователя | [Apple WWDC](https://developer.apple.com/videos/play/wwdc2022/10156/), [Picker](https://developer.apple.com/documentation/screencapturekit/sccontentsharingpicker) | Native consent/capture остаётся в Swift |
| Windows capture support, frame handling, HDR | [Microsoft screen capture](https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture) | Нужны platform integration tests, не только replay |
| Swift formatter и strict lint | [swift-format](https://github.com/swiftlang/swift-format/blob/main/README.md) | Своя `.swift-format`, Swift 6 compiler concurrency checks |
| Jev vision экспериментален; PNG/JPEG 280–1568 px на сторону | [Images](https://docs.typesafe.ai/images) | Размеры/preprocessing проверяются перед отправкой, provider заменяем |
| Box может существовать без цели | [Box](https://docs.typesafe.ai/primitives/box) | Нужна отдельная проверка присутствия; box не разрешает действие |
| Rust 1.99.1 исправляет native FFI/linker defects | [Rust release](https://blog.rust-lang.org/2026/10/15/Rust-1.99.1/) | CI закрепляет 1.99.1; локальная 1.99.0 проверка пока не включает FFI |
| Swift 6.4 официальный пакет | [Swift macOS](https://www.swift.org/install/macos/) | CI устанавливает официальный пакет 6.4; host Apple Swift recorded отдельно |

Перед включением Jev повторно проверить version ID, preview header, ограничения входа и фактический usage. Тариф не зашит в foundation. Совместимость local model/helper и права на веса здесь не проверены; выбор отложен до экспериментального этапа.

`swift-actions/setup-swift@v2` не используется: его проверенный список версий ещё не содержит 6.4. [Исходник проверки](https://github.com/swift-actions/setup-swift/blob/v2/src/swift-versions.ts).
