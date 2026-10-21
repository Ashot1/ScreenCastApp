# 25H: Строка меню и выход

Spec: [ADR-023](../../../decisions/0023-ui-redesign-liquid-glass.md). Decisions: [D72–D78](../decisions.md). Evidence: [опись дизайна](../evidence/25-ui-design-inventory.md), [Liquid Glass](../evidence/25-liquid-glass-research.md), [опись интерфейса](../../../design/ui-inventory.md).

**What to build:** В строке меню статус — символ SF Symbol, меню системное с иконками (по R4); при выходе меню показывает «Завершение…», а открытое главное окно — полосу хода без затемнения.

**Blocked by:** приёмка 25G.

**Status:** ready — 25G принята; исполнитель запускается оркестратором (D80)

- [ ] Метка `MenuBarExtra` (символ по состоянию и «Ambient»), явный `.menuBarExtraStyle(.menu)`, пункты через `Label`.
- [ ] `AssistantModel.beginClosing()` вызывается первым в `applicationShouldTerminate`; тест: после него `canToggleManualPause == false`, `requestSelection` ничего не делает.
- [ ] Полоса хода завершения только при открытом окне; своего окна и затемнения нет (D75).
- [ ] Итоговые изменения `docs/product.md`, `docs/design/ui-inventory.md` и README — списком для оркестратора.
- [ ] Перечень проверок (`rg` из ADR-023) снят до и после; каждое перенесённое или исчезнувшее выражение объяснено в handoff.
- [ ] Сначала перенос без изменений с проверкой сборки, затем оформление.
- [ ] Новые строки EN/RU — через Localization Mechanic; чувствительные тексты утверждает оркестратор.
- [ ] `yarn verify`, `yarn lint:swift`, `yarn build:macos`, подписи, контрольные сборки по базе оркестратора; приложение Ambient с данными пользователя не запускается (D77).
- [ ] Обязательные специалисты, TDD для логики, Code Simplicity Review; изменения общих документов перечислены в handoff.

**Не входит:** своё меню; цветное кольцо в строке меню. Точка просмотра для пользователя после приёмки (D77).
