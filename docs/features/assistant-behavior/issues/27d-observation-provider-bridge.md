# 27D: Поставщик и мост наблюдения

Spec: [ADR-025](../../../decisions/0025-background-observation.md). Decisions: [D82, D86–D104](../decisions.md).

**What to build:** `observation_body` с изображениями и строгой схемой, `ModelPurpose::Observation`, ветка `dispatch_with` без инструментов, заголовок `ambient_observe.h` v1 вместе с `highlight` и владением буферами; живой запрос со снимком переносится в 27H: он требует сессии ChatGPT пользователя.

**Blocked by:** приёмка 27C (общие мост и ядро, D102)

**Status:** accepted — 22 октября 2026 ([приёмка](../../../../target/verification/orchestration/27d-parent-inspection.md))

- [x] HTTP-тест тела с изображением через `ambient_model_test_dispatch`.
- [x] Предел размера тела запроса; заголовок документирует владение буферами.
- [x] TDD для логики; обязательные специалисты; Code Simplicity Review; изменения общих документов — списком в handoff.
- [x] `yarn verify`, Clippy со всеми признаками, аудит экспортов и подписей, контрольные сборки по базе оркестратора. Всё зелёное, кроме `lint:swift` в незавершённых файлах 26D (см. handoff, раздел 5); у оркестратора — тот же итог.

**Не входит:** Swift (27E).
