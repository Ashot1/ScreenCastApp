# 12A: Выбранная задача Jira Cloud

Spec: [ADR-017](../../../decisions/0017-selected-jira-issue.md), [полная спецификация](../spec.md). Evidence: [Jira API](../evidence/12-jira-integration-research.md).

**What to build:** Собственные email/scoped API token и Cloud ID сайта → выбранная задача, доступные комментарии/история/связи → ограниченная текстовая проекция → точный preview выбранных1–32 объектов → атомарное сохранение в дело → существующая помощь08B.

**Blocked by:** локальная приёмка08B/13A и проверенный контракт памяти/source fences/credential lifecycle. Выполнено: обе части приняты; schema9/model4/GitHub1 подтверждены. Новая часть добавляет schema10/Jira1. Один исполнитель владеет связанной Core/provider/ABI/native границей.

**Status:** implemented-local-pass;20 октября2026. Связанный12A реализован и локально проверен; передача координатору для приёмки. Полная12/G6 и настоящие подключения/UI остаются открытыми.

- [x] Профиль явно задаёт canonical Cloud ID и отдельный native Keychain email/token. Нет автоматического обнаружения, общего API hostname вместо site identity или reuse credentials Codex.
- [x] Исключение сайта сохраняется независимо от профиля и проверяется перед чтением/результатом/bind/08B. Удаление/новый профиль/замена ключа не обходят exclusion. Exclude→allow повышает settings revision; поздний ответ не оживает.
- [x] Core-first credential replace/revoke, сериализация native mutations и подтверждённая очистка Keychain сохраняют границы crash/restart/late-result. Секреты не попадают в Core JSON/SQLite/log/model.
- [x] Один ограниченный job использует только три GET операции ADR, updateHistory=false/failFast=false, без redirect/retry/proxy/cookies/bodylinks/фонового чтения. Сохранены все task/profile/session/report/High/pause fences и фактическое завершение cancellation.
- [x] Initial issue ID закрепляет последующие запросы, final ID совпадает; issue/relations берутся из конечного ответа. История/комментарии имеют раздельную проверенную пагинацию, partial/omitted-prefix/non-atomic provenance. Нет ложной полной/актуальной истории.
- [x] Реализованы все13GET/30seconds/body/header/window/count/byte пределы, checked i32/i64 offsets и reserved final GET. Последний запрос не обходит cooldown/deadline. Нет усечения объекта ради помещения.
- [x] 401 отзывает только соответствующую generation, включая устаревший TaskVersion; поздний401 не портит замену.403/404/429/503 обрабатываются по категории, не как отсутствие данных. Ошибки полей дают explicit partial с неизвестной причиной.
- [x] Opaque IDs, +0000 даты, selected-fields shape/presence, fieldtype и bounded ADF проверяются по ADR. Unknown nodes/marks/media/mentions не превращаются в выдуманное содержание.
- [x] Parent/subtask/issue links остаются наблюдёнными связями, без traversal/завершения дел/изменения плана. Subtasks reader ограничен; sub-tasks не подменяется alias. Неизвестная форма отмечается partial; fixture не выдаётся за полную Cloudv3 совместимость.
- [x] Source identity tags разных relations не сталкиваются; canonical content hash отделён от provenance/generation/interval/coverage. Saved metadata входит в прежние16KiB/global64MiB/row bounds и open-time validation.
- [x] Exact preview1–32 ничего не записывает; Save атомарно переиспользует прежнюю память и защищает ручные изменения/tombstones/cross-profile bindings. Возвращаются только полный набор EvidenceIds и конечная TaskVersion.
- [x] Ordinary refresh делает08B stale с разрешённым retention; revoke/exclusion очищает все зависимые области, включая uncited grounds, в той же транзакции. Проверены реальные prepare/send/result/copy/reply/reopen и restart пути.
- [x] Native настройки, выбранная задача, coverage/errors/read/cancel/refresh/preview/save/exclusion работают через прежнюю панель и08B; EN/RU обновлены. Импорт сам не вызывает модель.
- [x] Настоящий временный SQLite→C→Swift→isolated test Keychain→loopback HTTP→batchSave→08B проверен. Пройдены специалисты, TDD, Code Simplicity Review, fullverify, нужные featureSwift/Clippy/build/signature/ABI checks и сохранность14 прежних checkpoints; source hashes и реальные логи сохранены.
- [x] Реальные Jira installation/scopes/permissions/subtask relations, UI/Fast/Windows/full12/G6 не объявлены проверенными. Новых account/model/Jev вызовов нет, прежнее неизвестное потребление не обнулено.

**Не входит:** Data Center, OAuth/MCP/backend, browser/unsaved editor/hostname aliases, project exclusions, JQL/search/polling/webhooks, вложения/media, переходы к связанным задачам, любые записи в Jira. Эти ограничения среза не отменяют полную задачу12.

Локальные доказательства20 октября2026: `yarn verify` exit0 —8 tooling/224 Rust/75 Swift; полный test-support Swift —119 tests exit0, один настоящий ASR test явно отключён. Jira Core22, parser5, provider/registry8 и ABI buffer/layout test проходят. Настоящий временный SQLite→C→Swift→isolated Keychain→loopback Jira→batchSave→08B путь и access/cancel/ABA/restart проверены. Code Simplicity Review — PASS после исправлений empty history probe/nonnegative tail offset и missing/null/invalid presentation; API Inspector14/14 + focused probe supplement — PASS.

All-features Clippy, app build, strict/deep signatures и audit4 Mach-O — PASS. JiraABI1 имеет14 production exports; memory3/auth1/source1/Codex1/model4/GitHub1 headers и публичные exports сохранены. Test hooks/старые ABI/Nemo/weights отсутствуют. EN/RU:599 ключей, из них120 Jira, без дублей;10 согласованных sensitive pairs совпадают точно. Все186 файлов14 прежних checkpoints совпадают с preflight baseline, подписи действительны; новый checkpoint не создавался.

Команды/UTC/окружение/exit codes, исходные логи, source hashes и ограничения: [verification-summary.json](../../../../target/verification/12a/verification-summary.json), [handoff.txt](../../../../target/verification/12a/handoff.txt). Промежуточные RED/compile/sandbox failures сохранены отдельно от финальных PASS. Реальные Jira Cloud permissions/непустые subtask shapes, production UI/focus/Spaces, TCC/mic/ServiceManagement, live Fast/Jev, macOS14 hardware и Windows не проверялись. Новых live POST и model weights нет; ledger7POST/1021known/3unknown не изменён. Shared docs и финальный checkpoint — область координатора.
