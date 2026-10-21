# 25E: Лента и лента у края

Spec: [ADR-023](../../../decisions/0023-ui-redesign-liquid-glass.md). Decisions: [D72–D78](../decisions.md). Evidence: [опись дизайна](../evidence/25-ui-design-inventory.md), [Liquid Glass](../evidence/25-liquid-glass-research.md), [опись интерфейса](../../../design/ui-inventory.md).

**What to build:** Карточки ленты сворачиваются в строку, со смысловыми метками, блоками ошибок и ходом голоса; лента у края — стеклянная панель, по-прежнему без активации и с печатью ответа.

**Blocked by:** приёмка 25D.

**Status:** accepted 21 October 2026 ([приёмка](../../../../target/verification/orchestration/25e-parent-inspection.md)); handoff `target/verification/25e/handoff.txt`. Вторая точка просмотра для пользователя (D77)

- [x] `AssistanceCardView`: сворачиваемые строки (развёрнута цель ответа, иначе явное нажатие, иначе первая не устаревшая), метки, `Callout`; `VoiceReplyView`; `WorkRow`.
- [x] `EdgeCardController`: в `render` меняется только блок создания окна (прозрачный скрытый заголовок, `.fullSizeContentView`, прозрачный фон, основной путь `.glassEffect`, запасной `NSGlassEffectView` по R2); логика показа и активации, наблюдатели, `placement()` и `EdgeCardTests` — побайтно прежние.
- [x] Проверки «устарело / занято / готово» в кнопках ленты сохранены (перечень проверок).
- [x] Перечень проверок (`rg` из ADR-023) снят до и после; каждое перенесённое или исчезнувшее выражение объяснено в handoff.
- [x] Сначала перенос без изменений с проверкой сборки, затем оформление.
- [x] Новые строки EN/RU — через Localization Mechanic; чувствительные тексты утверждает оркестратор.
- [x] `yarn verify`, `yarn lint:swift`, `yarn build:macos`, подписи, контрольные сборки по базе оркестратора; приложение Ambient с данными пользователя не запускается (D77).
- [x] Обязательные специалисты, TDD для логики, Code Simplicity Review; изменения общих документов перечислены в handoff.

**Не входит:** изменение правил показа ленты. Точка просмотра для пользователя после приёмки (D77).
