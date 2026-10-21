# 25D: Страница шага и страницы редактора

Spec: [ADR-023](../../../decisions/0023-ui-redesign-liquid-glass.md). Decisions: [D72–D78](../decisions.md). Evidence: [опись дизайна](../evidence/25-ui-design-inventory.md), [Liquid Glass](../evidence/25-liquid-glass-research.md), [опись интерфейса](../../../design/ui-inventory.md).

**What to build:** У шага своя страница; редакторы дела, шага, проверки, зависимости, завершения и нового дела — отдельные страницы, один за раз; план завершённого дела — только для чтения.

**Blocked by:** приёмка 25C.

**Status:** accepted 21 October 2026 ([приёмка](../../../../target/verification/orchestration/25d-parent-inspection.md)); handoff `target/verification/25d/handoff.txt`

- [x] `editorKind` — проекция только для чтения существующего `Editor`; приоритет страниц: `editorKind`, затем `selectedStep`, затем вкладки.
- [x] Страница шага: проверки через `Menu`, дата `confirmedAtUTC`, если есть, меню шага; «Назад к плану» — `requestSelection(.task(current))`.
- [x] Признаки `editingTask`/`editingStep` в `@State` `AssistantView`; поля видны и при несохранённом черновике; правила сброса из ADR-023.
- [x] `MemorySessionTests.swift:202, 329, 332` переписаны на `editorKind` в том же изменении; выражение `.onChange` (`:57`) с прежним значением.
- [x] Тест: путь «выбрать шаг → editorKind → страница», включая приоритет проверки над шагом; `PageControls` в `PlanDraftFields`.
- [x] D81: блок «Следующий шаг» показывается только на первой странице шагов; тест на вторую страницу.
- [x] Перечень проверок (`rg` из ADR-023) снят до и после; каждое перенесённое или исчезнувшее выражение объяснено в handoff.
- [x] Сначала перенос без изменений с проверкой сборки, затем оформление.
- [x] Новые строки EN/RU — через Localization Mechanic; чувствительные тексты утверждает оркестратор.
- [x] `yarn verify`, `yarn lint:swift`, `yarn build:macos`, подписи, контрольные сборки по базе оркестратора; приложение Ambient с данными пользователя не запускается (D77).
- [x] Обязательные специалисты, TDD для логики, Code Simplicity Review; изменения общих документов перечислены в handoff.

**Не входит:** новые данные проверок (текст, дата сверх `confirmedAtUTC`).
