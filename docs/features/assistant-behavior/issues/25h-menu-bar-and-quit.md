# 25H: Строка меню и выход

Spec: [ADR-023](../../../decisions/0023-ui-redesign-liquid-glass.md). Decisions: [D72–D78](../decisions.md). Evidence: [опись дизайна](../evidence/25-ui-design-inventory.md), [Liquid Glass](../evidence/25-liquid-glass-research.md), [опись интерфейса](../../../design/ui-inventory.md).

**What to build:** В строке меню статус — символ SF Symbol, меню системное с иконками (по R4); при выходе меню показывает «Завершение…», а открытое главное окно — полосу хода без затемнения.

**Blocked by:** приёмка 25G.

**Status:** accepted 22 October 2026 ([приёмка](../../../../target/verification/orchestration/25h-parent-inspection.md)); handoff `target/verification/25h/handoff.txt`. Финальная точка просмотра для пользователя (D77)

- [x] Метка `MenuBarExtra` (символ по состоянию и «Ambient»), явный `.menuBarExtraStyle(.menu)`, пункты через `Label`.
- [x] `AssistantModel.beginClosing()` вызывается первым в `applicationShouldTerminate`; тест: после него `canToggleManualPause == false`, `requestSelection` ничего не делает.
- [x] Полоса хода завершения только при открытом окне; своего окна и затемнения нет (D75).
- [x] Итоговые изменения `docs/product.md`, `docs/design/ui-inventory.md` и README — списком для оркестратора.
- [x] Перечень проверок (`rg` из ADR-023) снят до и после; каждое перенесённое или исчезнувшее выражение объяснено в handoff.
- [x] Сначала перенос без изменений с проверкой сборки, затем оформление.
- [x] Новые строки EN/RU — через Localization Mechanic; чувствительные тексты утверждает оркестратор.
- [x] `yarn verify`, `yarn lint:swift`, `yarn build:macos`, подписи, контрольные сборки по базе оркестратора; приложение Ambient с данными пользователя не запускается (D77).
- [x] Обязательные специалисты, TDD для логики, Code Simplicity Review; изменения общих документов перечислены в handoff.

**Не входит:** своё меню; цветное кольцо в строке меню. Точка просмотра для пользователя после приёмки (D77).
