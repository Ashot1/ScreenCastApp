# Liquid Glass на macOS 26/27 для нового интерфейса — исследование 21 октября 2026

Основа для спецификации внедрения дизайна (D72–D74). Источники:
- **SDK** Xcode 27.1 (MacOSX SDK 27A9275): `SwiftUICore`/`SwiftUI` swiftinterface, заголовки AppKit `NSGlassEffectView.h`, `NSStatusItem.h`, `NSViewCornerConfiguration.h`, `NSPanel.h`;
- **документы Apple** (JSON developer.apple.com): «Applying Liquid Glass to custom views», «Adopting Liquid Glass», HIG Materials/Color/Sidebars/Windows/Sheets/Alerts/Settings, примечания к выпускам macOS 26 и 27;
- **WWDC25** 310 и 323;
- **проверка типов** (`swiftc -typecheck -target arm64-apple-macos26.0`) пробного файла: панель `NSPanel` без активации с `NSGlassEffectView` и `NSHostingView` компилируется.

Отчёты: Pattern Librarian и External Researcher (оркестратор). Во время работы ничего не запускалось — поведение при запуске не проверено.

## SwiftUI (macOS 26+, если не сказано иное)

- **Стекло.**
  - `glassEffect(_ glass: Glass = .regular, in shape: some Shape = DefaultGlassEffectShape())` — форма по умолчанию капсула; применять после остальных модификаторов внешнего вида.
  - `Glass`: `.regular`, `.clear`, `.identity`, `tint(_:)`, `interactive(_:)`.
  - `GlassEffectContainer(spacing:)` — группирует стекло: стекло не может брать изображение у другого стекла, и так экономятся проходы отрисовки.
  - `glassEffectID(_:in:)`, `glassEffectUnion(id:namespace:)`, `glassEffectTransition(_:)` (`.matchedGeometry`, `.materialize`, `.identity`).
- **Кнопки.**
  - `.buttonStyle(.glass)`, `.glassProminent`; `GlassButtonStyle.init(_:)` требует macOS 26.1; статический `.glass(_:)` в SDK помечен 26.0 (поправка 25A, `target/verification/25a/p0-compile.txt`).
  - `.buttonBorderShape(.capsule)`.
  - На macOS кнопки размеров mini/small/medium остаются скруглёнными прямоугольниками, large и больше — капсулы; высоты элементов чуть больше, жёсткие высоты не задавать.
- **Края и формы.**
  - `backgroundExtensionEffect()`, `scrollEdgeEffectStyle(_:for:)` (`.automatic`/`.hard`/`.soft`), `scrollEdgeEffectHidden`.
  - `ConcentricRectangle`, `containerShape(_:)`, `Edge.Corner.Style.concentric`; `GeometryProxy.concentricCornerRadii` — только SDK macOS 27.
- **Окно и панель инструментов.**
  - `windowToolbarStyle` (`.unified`/`.unifiedCompact`/`.expanded`), `windowStyle(.hiddenTitleBar)`, `windowBackgroundDragBehavior` (macOS 15), `toolbar(removing:)`.
  - `ToolbarSpacer`, `sharedBackgroundVisibility(_:)`, `safeAreaBar(edge:)`, `containerBackground(_:for:)` (размещение `.window` — macOS 15).
- **Навигация.**
  - `NavigationSplitView` получает плавающую стеклянную боковую панель автоматически (WWDC 323).
  - `navigationSplitViewColumnWidth`, `List(selection:)`, `.listStyle(.sidebar)`, `Section(isExpanded:)`, `.badge`.
  - `TabView` (`.automatic`, `.tabBarOnly`, `.sidebarAdaptable` — macOS 15), `ContentUnavailableView`, `Form` + `.formStyle(.grouped)`.
- **Выбор строки списка.** Способа запретить смену выбора нет (есть только `selectionDisabled` для строки). Поддерживаемый путь — своя привязка `Binding(get: { model.selectedID }, set: { id in Task { await model.requestSelection(…) } })`. Список визуально возвращается назад, если пользователь отменил переход в окне несохранённых изменений. Новое состояние не нужно.
- **Строка меню.**
  - `MenuBarExtra(content:label:)` — метка любая `View`; стили `.menu` (по умолчанию) и `.window`.
  - Цветное кольцо статуса не гарантировано: метка может стать монохромным шаблоном. Нужна ранняя проба, запасной путь — нешаблонное `NSImage`.
  - Делегат развёрнутого интерфейса `NSStatusItem` — только macOS 27.
- **Автоматически получают стекло** панели инструментов, листы, всплывающие окна, меню и диалоги. Свои фоны в боковых панелях, листах и панелях инструментов Apple советует убрать: они мешают стеклу.
  - Лист затемняет родительское окно.
  - В SDK 27 значения среды формы кнопок сбрасываются внутри листов и всплывающих окон.
- **Радиус окна.** Публичного API нет: точный радиус 26 из дизайна недостижим. Содержимое вкладывается концентрическими формами.
- **Содержимое.** Стекло не используется в слое содержимого (HIG): белый лист содержимого — обычный слой, не стекло.

## AppKit

- **`NSGlassEffectView`** (macOS 26): `contentView`, `cornerRadius`, `tintColor`, `style` (`.regular`/`.clear`); `effectIsInteractive` — только macOS 27. Внутри стекла гарантированно находится только `contentView`.
- **`NSGlassEffectContainerView`**: `contentView`, `spacing`.
- **Панель ленты у края.** Работает вариант `NSPanel` + `NSGlassEffectView(cornerRadius: 30)` + `NSHostingView`, при этом `isOpaque = false`, `backgroundColor = .clear`.
  - У панели без рамки `canBecomeKey` по умолчанию, по-видимому, `false`, и поле ответа не получит ввод. Нужен подкласс с `canBecomeKey` или прозрачный заголовок вместо отсутствия рамки (проверить пробой).
  - Более простой путь: `.glassEffect` на корне SwiftUI внутри прежнего `NSHostingView`.
- **Панели и ввод.** По HIG панель становится ключевым окном только при нажатии на элемент, которому нужен ввод с клавиатуры.

## Цвета, темы, доступность

- Системные стили (`.primary`, `.secondary`, `.tint`, фоны) сами адаптируются к тёмной теме.
- Немногие фирменные и статусные цвета хранятся в каталоге `Colors.xcassets` (Any/Dark) в `Resources` пакета AmbientMacOS. `.process("Resources")` их обрабатывает, манифест не меняется. Доступ — `Color("name", bundle: .module)`.
- Стекло без собственного цвета; оттенок — только для смысла (главное действие, статус), умеренно. `.regular` — для поверхностей с текстом, `.clear` — только над насыщенными медиа.
- «Уменьшить прозрачность», «Увеличить контраст» и «Уменьшить движение» меняют эффекты. Стандартные элементы адаптируются сами, свои нужно проверять.
- Статус только цветом недостаточен: нужен символ или подпись.

## Платформа

- **Цель сборки.** `Package.swift` (`.macOS(.v14)`) и обе конфигурации `MACOSX_DEPLOYMENT_TARGET = 14.0` в `Ambient.xcodeproj` поднимаются до 26 вместе.
- **API новее 26.0** без нового повышения не используются: `GlassButtonStyle.init(_:)` (26.1), `effectIsInteractive`, делегат развёрнутого интерфейса `NSStatusItem`, `NSViewCornerConfiguration`, `concentricCornerRadii` (27).
- **Изменения macOS 27.** Иконки пунктов меню скрыты по умолчанию — `NSMenuItem.preferredImageVisibility`, `labelStyle(.titleAndIcon)`. Исправлено состояние наведения у кнопок `.glass` вне панели инструментов.

## Не проверено при запуске

- Стекло в панели без активации и ввод в ней.
- Цветное кольцо в строке меню и вид метки `MenuBarExtra`.
- `.window`-стиль меню.
- `NavigationSplitView` или `TabView(.sidebarAdaptable)` в сцене `Settings`.
- Вид листов и диалогов на macOS 27.
- Поведение при «Уменьшить прозрачность».
- Сборка `.xcassets` в конвейере `yarn check:swift`.
- Порог производительности числа стеклянных элементов.
