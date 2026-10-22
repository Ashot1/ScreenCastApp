# 27D: Поставщик и мост наблюдения

Spec: [ADR-025](../../../decisions/0025-background-observation.md). Decisions: [D82, D86–D104](../decisions.md).

**What to build:** `observation_body` с изображениями и строгой схемой, `ModelPurpose::Observation`, ветка `dispatch_with` без инструментов, заголовок `ambient_observe.h` v1 вместе с `highlight` и владением буферами; живой запрос со снимком.

**Blocked by:** приёмка 27C (общие мост и ядро, D102)

**Status:** ready — 27C принята; исполнитель запускается оркестратором (D102)

- [ ] HTTP-тест тела с изображением через `ambient_model_test_dispatch`.
- [ ] Предел размера тела запроса; заголовок документирует владение буферами.
- [ ] TDD для логики; обязательные специалисты; Code Simplicity Review; изменения общих документов — списком в handoff.
- [ ] `yarn verify`, Clippy со всеми признаками, аудит экспортов и подписей, контрольные сборки по базе оркестратора.

**Не входит:** Swift (27E).
