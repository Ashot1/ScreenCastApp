# 25F: Вкладки заметок, связей, источников и помощи

Spec: [ADR-023](../../../decisions/0023-ui-redesign-liquid-glass.md). Decisions: [D72–D78](../decisions.md). Evidence: [опись дизайна](../evidence/25-ui-design-inventory.md), [Liquid Glass](../evidence/25-liquid-glass-research.md), [опись интерфейса](../../../design/ui-inventory.md).

**What to build:** Заметки с меню «⋯», связи со статусом и датой, второй уровень вкладок источников, помощь — в оформлении дизайна.

**Blocked by:** приёмка 25E.

**Status:** accepted 21 October 2026 ([приёмка](../../../../target/verification/orchestration/25f-parent-inspection.md)); handoff `target/verification/25f/handoff.txt`

- [x] `NotesTab` (из `EvidenceRow`) с меню «⋯»: изменить, перенести, удалить — прежние действия.
- [x] `LinksTab` со статусом цели и датой `actionAtUTC`.
- [x] Второй уровень вкладок источников (только имена, без статусов — D73); источники оформлены стилем корня; `PageControls` в источниках.
- [x] Вкладка «Помощь» — прежний `SelectedAssistanceView` с защитой предпросмотра.
- [x] Отмена чтений не возвращается в экраны источников (D76).
- [x] Перечень проверок (`rg` из ADR-023) снят до и после; каждое перенесённое или исчезнувшее выражение объяснено в handoff.
- [x] Сначала перенос без изменений с проверкой сборки, затем оформление.
- [x] Новые строки EN/RU — через Localization Mechanic; чувствительные тексты утверждает оркестратор.
- [x] `yarn verify`, `yarn lint:swift`, `yarn build:macos`, подписи, контрольные сборки по базе оркестратора; приложение Ambient с данными пользователя не запускается (D77).
- [x] Обязательные специалисты, TDD для логики, Code Simplicity Review; изменения общих документов перечислены в handoff.

**Не входит:** статусы источников и время чтения.
