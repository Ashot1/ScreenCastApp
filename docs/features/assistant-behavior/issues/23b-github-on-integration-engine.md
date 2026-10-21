# 23B: GitHub на общем движке и удаление прежнего кода

Spec: [ADR-020](../../../decisions/0020-integration-layer-contract.md), [ADR-016](../../../decisions/0016-selected-github-pr.md). Decisions: [D58–D61](../decisions.md). Evidence: [карта повторов](../evidence/23-integration-layer-code-map.md).

**What to build:** GitHub работает для пользователя как раньше — профиль с allowlist, ключ, чтение выбранного PR и обсуждений, точный предпросмотр, атомарное сохранение и помощь 08B — через тот же общий движок, транспорт, ABI интеграций и Swift-каркас, что и Jira. Весь прежний код GitHub и переходные остатки удалены; сохранение профиля GitHub больше не отменяет чтение другого профиля.

**Blocked by:** [23A](23a-integration-engine-jira.md) — принят оркестратором.

**Status:** accepted 20 октября 2026 — [приёмка](../../../../target/verification/orchestration/23b-parent-inspection.md), [handoff](../../../../target/verification/23b/handoff.txt). Контрольная сборка Ambient-github-engine-23b.app.

- [x] Адаптер GitHub реализует интерфейс ADR-020 без изменения сигнатур `Adapter`/`Reader` и без ветвления по виду внутри движка; иначе — остановка и вопрос оркестратору.
- [x] Предметная семантика ADR-016 сохранена: allowlist, якорь и head, Link-пагинация, реакция на предел окна, пауза (403/429/`remaining=0`, минимум 60 s), 404 категории, nullability, канонический hash.
- [x] Схема 12: строки `github` в профилях интеграций (`scope` NULL, `config` — allowlist); `github_profiles`/`github_metadata` удалены; схема 11 отклоняется только для чтения; fixture схемы 11 заморожена.
- [x] Переходные остатки 23A удалены: основание 08B только `Integration`, цитата только `integration`, учёт байтов и проверка открытия только по общей таблице; контекст модели для GitHub побайтно прежний.
- [x] ABI интеграций v1 дополнен видом GitHub и типизированным сохранением профиля GitHub; `ambient_github.h`, именованные mutex отмены и константы шагов удалены; bridge-тест раскладок и ошибок покрывает GitHub.
- [x] Swift: GitHub на общем владельце секретов (`githubToken`), каркасе и модели с защитой от устаревших ответов; 24 одинаковых ключа объединены в `integration.*` в EN/RU через Localization Mechanic.
- [x] Эталоны GitHub совпадают побайтно, кроме названных изменений; перенесены все 27 сценариев `github_source.rs`; новые тесты ADR-020 для 23B (сохранение профиля не отменяет чтение другого, единый `known` для GitHub, первый код ошибки по каноническому порядку) проходят.
- [x] Аудит удаления: нет `github_read`, `jira_read`, `*ReadOperation`, `ambient_github*`, `ambient_jira*`, именованных mutex отмены; измерено и записано чистое сокращение строк в Core, providers, bridge и macOS.
- [x] Пройдены специалисты, TDD, Code Simplicity Review и полный набор проверок 23A с новой базой checkpoints после 23A.

**Не входит:** переключатели (23C), Todoist, плагины/MCP, OAuth, Windows, настоящие подключения и полные 13/G6.
