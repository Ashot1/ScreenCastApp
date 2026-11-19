# 27F: Рамка подсветки

Spec: [ADR-025](../../../decisions/0025-background-observation.md). Decisions: [D82, D86–D104](../decisions.md).

**What to build:** Новый файл `HighlightOverlay`: панель без рамки и активации, не принимает мышь, на всех рабочих столах; перевод нормализованного прямоугольника в экранные координаты.

**Blocked by:** приёмка 27D (принята)

**Status:** accepted locally — принята оркестратором 23 октября 2026 (D105): 4 теста 27F, полный verify (8 tooling / 394 Rust / 117 Swift), all-features Clippy, сборка и аудит PASS. Доказательства: `target/verification/27f/handoff.txt`; живая проверка — 27H. Новый checkpoint не создан из-за параллельных незавершённых пакетов.

- [x] Договор с 27E: `@MainActor final class HighlightOverlay` с `show(normalizedRect:displayKey:)` и `hide()`; `displayKey` — UUID дисплея строкой; прямоугольник нормализован, как у Vision.
- [x] Тест перевода координат (Retina, несколько дисплеев).
- [x] TDD для логики; обязательные специалисты; Code Simplicity Review; изменения общих документов — списком в handoff.
- [x] `yarn verify`, Clippy со всеми признаками, аудит экспортов и подписей, контрольные сборки по базе оркестратора.

**Не входит:** интерфейс (27G).
