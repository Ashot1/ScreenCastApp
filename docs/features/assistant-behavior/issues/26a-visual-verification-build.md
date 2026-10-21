# 26A: Сборка «Ambient Visual» и тестовые данные

Spec: [ADR-024](../../../decisions/0024-design-fidelity-pass.md); base [ADR-023](../../../decisions/0023-ui-redesign-liquid-glass.md). Decisions: [D83–D86](../decisions.md). Макет: `target/verification/design/ambient-design.pdf`.

**What to build:** Отдельная сборка для сверки с макетом на тестовых данных, без кода в ежедневной сборке и без доступа к данным пользователя.

**Blocked by:** —

**Status:** accepted 22 October 2026 ([приёмка](../../../../target/verification/orchestration/26a-27a-parent-inspection.md))

- [x] Пробная сборка и запуск: подмена идентификатора не ломает ресурсные пакеты SwiftPM, флаг `AMBIENT_VISUAL_FIXTURE` доходит до цели приложения; иначе — третья конфигурация или QUESTION.
- [x] `scripts/macos.sh visual`: сборка с `$(inherited) AMBIENT_VISUAL_FIXTURE`, `dev.ambient.foundation.visual`, «Ambient Visual», папка `target/verification/design/build`, запуск с `AMBIENT_VISUAL_ROOT`.
- [x] `AmbientAppDelegate.init` под флагом: папка только в `target/verification/`, иначе остановка; память; публичный `ChatGPTModel.init(session:chatGPTKeychainService:codexHome:)` со значениями по умолчанию; переименована только служба ChatGPT; домашняя папка Codex внутри папки сверки.
- [x] Тест наполнения (`AMBIENT_VISUAL_SEED_DIR`, `@testable`): пустая папка, испорченная память, дела по макету (план, шаги, проверки, зависимости, заметки, связи, сведения, завершённое дело).
- [x] Пробный снимок нынешнего интерфейса через computer-use (одобрение пользователя), отпечатки данных пользователя до и после; README: порядок сессии сверки и правила «не нажимать».
- [x] Приёмка: в ежедневных Debug и Release нет `AMBIENT_VISUAL_ROOT`; «Ambient Visual» без переменной и с неверной папкой завершается с ошибкой.
- [x] Перечень проверок ADR-023 снят до и после; каждое перенесённое или исчезнувшее выражение объяснено в handoff.
- [x] Новые и изменённые строки EN/RU — через Localization Mechanic; чувствительные тексты утверждает оркестратор; `docs/product.md`/`docs/privacy.md` — списком изменений.
- [x] `yarn verify`, `yarn lint:swift`, `yarn build:macos`, подписи, контрольные сборки по базе оркестратора.
- [x] Обязательные специалисты, Code Simplicity Review; изменения общих документов перечислены в handoff.

**Не входит:** изменение экранов; наполнение ленты (26E).
