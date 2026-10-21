# 25A: Пробы, macOS 26 и слой оформления

Spec: [ADR-023](../../../decisions/0023-ui-redesign-liquid-glass.md). Decisions: [D72–D78](../decisions.md). Evidence: [опись дизайна](../evidence/25-ui-design-inventory.md), [Liquid Glass](../evidence/25-liquid-glass-research.md), [опись интерфейса](../../../design/ui-inventory.md).

**What to build:** Ранние пробы R1–R4 в отдельных маленьких приложениях; приложение и голосовой помощник требуют macOS 26; появляется слой оформления, применённый на трёх корнях. Поведение не меняется.

**Blocked by:** приёмка [17A](17a-selected-todoist-task.md) — общие файлы macOS.

**Status:** accepted 21 October 2026 ([приёмка](../../../../target/verification/orchestration/25a-parent-inspection.md)); evidence `target/verification/25a/handoff.txt`. В Debug нет `Ambient.debug.dylib` (исправление запуска 21 октября): экспорты и аудит — в `Contents/MacOS/Ambient`

- [x] Пробы R1–R4 в `target/verification/25a/probes/`: компиляция стеклянных API при `.v26`; R1 настройки с боковой панелью; R2 лента у края со стеклом и вводом; R3 главное окно и `.onDisappear` при закрытии; R4 метка и иконки меню. Проверены программно и снимками экрана проб (D78); итог и снимки — в `target/verification/25a/`.
- [x] `Package.swift` 6.2 и `.v26`; `VoiceHelper/Package.swift` `.v26`; `VoiceHelper/Info.plist` 26.0; оба `MACOSX_DEPLOYMENT_TARGET` в xcodeproj; `scripts/macos.sh:6`; CI на раннере macOS 26+ (иначе BLOCKED).
- [x] Тексты о «минимум macOS 14» в README, `src/platforms/macos/README.md`, `docs/development.md` (через список для оркестратора там, где это общие документы).
- [x] `Design/Theme.swift` (четыре пары цветов через `Color(light:dark:)`, нужные роли шрифтов, радиусы) и `Design/Styles.swift` (`AmbientInsetGroupBoxStyle`, `.ambientTheme()`) на трёх корнях.
- [x] Тест равенства наборов ключей EN и RU в `Localizable.strings`.
- [x] Перечень проверок (`rg` из ADR-023) снят до и после; каждое перенесённое или исчезнувшее выражение объяснено в handoff.
- [x] Сначала перенос без изменений с проверкой сборки, затем оформление.
- [x] Новые строки EN/RU — через Localization Mechanic; чувствительные тексты утверждает оркестратор.
- [x] `yarn verify`, `yarn lint:swift`, `yarn build:macos`, подписи, контрольные сборки по базе оркестратора; приложение Ambient с данными пользователя не запускается (D77).
- [x] Обязательные специалисты, TDD для логики, Code Simplicity Review; изменения общих документов перечислены в handoff.

**Не входит:** компоненты без потребителей; перестройка экранов.
