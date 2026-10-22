# 26D: Заметки, связи, источники и помощь по макету

Spec: [ADR-024](../../../decisions/0024-design-fidelity-pass.md); base [ADR-023](../../../decisions/0023-ui-redesign-liquid-glass.md). Decisions: [D83–D86](../decisions.md). Макет: `target/verification/design/ambient-design.pdf`.

**What to build:** Вкладки заметок, связей, источников (GitHub, Jira, локальный проект, Codex) и помощи в обоих состояниях — по страницам 5–9.

**Blocked by:** приёмка 26C

**Status:** accepted — 22 октября 2026 ([приёмка](../../../../target/verification/orchestration/26d-parent-inspection.md))

- [x] Экраны по страницам 5–9; второй ряд источников; помощь: выбор сведений и предпросмотр.
- [x] Диалоги удаления заметки и настроек папки по странице 20.
- [x] D76 сохраняется: экраны источников не отменяют чтение.
- [x] Сверка: снимки «Ambient Visual» на тестовых данных рядом со страницами PDF в `target/verification/26d/compare/`, список совпадений и объяснённых отличий; отпечатки данных пользователя до и после.
- [x] Перечень проверок ADR-023 снят до и после; каждое перенесённое или исчезнувшее выражение объяснено в handoff.
- [x] Новые и изменённые строки EN/RU — через Localization Mechanic; чувствительные тексты утверждает оркестратор; `docs/product.md`/`docs/privacy.md` — списком изменений.
- [x] `yarn verify`, `yarn lint:swift`, `yarn build:macos`, подписи, контрольные сборки по базе оркестратора.
- [x] Обязательные специалисты, Code Simplicity Review; изменения общих документов перечислены в handoff.

**Не входит:** лента (26E), настройки (26F).
