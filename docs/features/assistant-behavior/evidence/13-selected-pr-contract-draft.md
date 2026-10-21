# Подготовка контракта 13A

20 октября2026. Законченный контракт перенесён в [ADR-016](../../../decisions/0016-selected-github-pr.md), чтобы у реализации был один нормативный документ.

Plan Simplicity Review — PASS после уточнения отзыва credential generation при401 и различения terminal stop/category404. Evidence Audit — PASS с зависимостью от приёмки08B и её окончательного source/retention контракта. Type Scaffolder работал только на чтение; отдельная durable read generation отвергнута как дублирование существующих fences. Batch1–32 сохраняется атомарно, source timestamp переиспользует прежнее поле.

[Исследование API](13-github-integration-research.md) сохраняет первичные источники, pinned OpenAPI и ограничения. Окончательная13A ещё не реализована и не подтверждает полную13/G6.
