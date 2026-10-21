# ADR-002: система инструкций как на фронтенде

Статус: требование пользователя, реализуется в foundation.

Источник: `/Users/gojda/Projects/Frontend/harmonyhunter-frontend`. Общие правила работы, роли, модели, Matt skills и процесс планирования сохраняются; локальные правила заменяются под Ambient, Rust, Swift и Windows.

Единый `agent-sources` генерирует Codex/Claude/Cursor/OpenCode адаптеры. Генератор и его тесты перенесены без изменения. Идентификаторы `harmony-*` оставлены для точного сохранения общих ссылок и workflow; это историческое имя набора правил, не имя продукта.

Frontend-local release publishing, React examples, ESLint/Biome, revalidation и devkit правила не являются общими. Их замены и точность переноса перечислены в [описании системы](../agent-rules-architecture.md) и машинном [реестре происхождения](../../agent-sources/provenance.json).

Superpowers нужны только в перечне, разрешённом исходными общими правилами. Планирование новых функций идёт `grill-with-docs → to-spec → to-tickets`; bootstrap не переносит global brainstorming flow вместо этого.
