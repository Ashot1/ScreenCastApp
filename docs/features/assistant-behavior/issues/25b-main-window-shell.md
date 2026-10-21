# 25B: Каркас главного окна

Spec: [ADR-023](../../../decisions/0023-ui-redesign-liquid-glass.md). Decisions: [D72–D78](../decisions.md). Evidence: [опись дизайна](../evidence/25-ui-design-inventory.md), [Liquid Glass](../evidence/25-liquid-glass-research.md), [опись интерфейса](../../../design/ui-inventory.md).

**What to build:** Главное окно на `NavigationSplitView` со стеклянной боковой панелью: «Предложения N», дела, «Завершённые», листание, настройки, капсула статуса; выбор дела через привязку от модели с прежним окном несохранённого. Содержимое дела пока прежнее, разделы подряд.

**Blocked by:** приёмка 25A.

**Status:** accepted 21 October 2026 ([приёмка](../../../../target/verification/orchestration/25b-parent-inspection.md)); handoff `target/verification/25b/handoff.txt`. Точка просмотра для пользователя (D77)

- [x] Перенос `AssistantView` в каркас (диалоги, проверки), `CaseSidebar`, `CaseDetailView` без изменений поведения.
- [x] Привязка выбора по ADR-023 (`get`/`set`, раздел по `.onChange` корня); тест не нужен для привязки, но путь «Отмена» в окне несохранённого не меняет раздел.
- [x] `Callout`, `StatusRing` (три статуса и «завершение», с символом), `PageControls` для рядов окна; пустые состояния — `ContentUnavailableView`.
- [x] `.defaultSize` сцены под боковую панель; `.disabled(model.busy || model.closing)` покрывает боковую панель.
- [x] Тест: деление на открытые и завершённые на границах страниц.
- [x] Перечень проверок (`rg` из ADR-023) снят до и после; каждое перенесённое или исчезнувшее выражение объяснено в handoff.
- [x] Сначала перенос без изменений с проверкой сборки, затем оформление.
- [x] Новые строки EN/RU — через Localization Mechanic; чувствительные тексты утверждает оркестратор.
- [x] `yarn verify`, `yarn lint:swift`, `yarn build:macos`, подписи, контрольные сборки по базе оркестратора; приложение Ambient с данными пользователя не запускается (D77).
- [x] Обязательные специалисты, TDD для логики, Code Simplicity Review; изменения общих документов перечислены в handoff.

**Не входит:** вкладки дела; страница шага; лента; настройки. Точка просмотра для пользователя после приёмки (D77).
