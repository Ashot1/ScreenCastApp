# Подготовка источника Codex14 — 19 октября2026

Это исследование следующего пакета, не реализация и не подтверждение подключения. Реальные личные sessions/credentials не читались; Codex/app-server не запускались. 10A отдельно владеет исходниками,14 пока только планируется.

## Проверенная отправная точка

[Отчёт02](02-codex/report.md) подтверждает независимое чтение выбранного JSONL для engine0.163.0-alpha.10.2, commit `4ebc485ca321243727539187169425fcf972f2c4`. Это local records only, не полнота прежних paginated страниц и не восстановление контекста модели. Terminal отсутствует — completion not observed, а не «сейчас работает» или «задача завершена».

Scout подтвердил существующие SourceKey/SourceEvidence и bind/ingest в едином Store. Evidence ограничен16KiB, страницы50элементами/256KiB, память64MiB. Но local-source ABI1 и evidence_root сейчас Git-specific; Codex не может просто пройти в existing snapshot. Type Scaffolder read-only рекомендует отдельные выбранный rollout/window/cursor и узкий Codex ABI внутри прежнего Store, с типизированным source-settings fence для двух реальных источников. Source permission и message identity — разные вещи; точная связь ещё прорабатывается.

Pattern Librarian подтвердил уже установленные serde_json1.0.154/sha2 вCore и штатные FileHandle bounded reads. Native владеет файловым I/O, Core — недоверенным JSONL/schema/identity. Не нужны общий JSONL-framework, второй Store или Python desktop runtime. StreamDeserializer сам не обеспечивает правило только newline-terminated records.

## Независимая альтернатива

Plan Reviewer предлагает полезный14A: явно выбранный rollout → ограниченное окно сохранённых сообщений → выбор/привязка к делу → существующая помощь06F/08A. Полные14, автоматическое связывание/опрос/инициатива остаются открытыми. Никакого правила «terminal → следующая волна», запускаCodex или записи вегоисторию.

Предел16MiB всегофайла в02 — защитный предел пробника, не согласованное ограничение продукта. Такой отказ исключил бы длинные текущие обсуждения. Предлагается ограниченный reader окон с явными «Ранее»/«Обновить», без накопления всейистории. Например1MiB window/256KiB record/128 normalizeditems; точныепределы иcursor ещё не приняты. Append/partialtail/replacement/truncation должны иметь явную семантику; атомарныйснимок активногоCodex не обещается. Новаяprojection должна пройти свои тесты, PASS02 её автоматически не доказывает.

## Канонические события закреплённой версии

External Researcher сверил официальные исходники того же commit. `event_msg.item_completed` содержит явные thread_id,turn_id,item.id и полный snapshot item; предыдущий task_started для такого item не нужен. Типы UserMessage, AgentMessage, Plan и FunctionCallOutput дают самостоятельные записи. Delegated input распознаётся по FunctionCallOutput namespace=codex_app/name=create_thread илиsend_message_to_thread; canonical ID совпадает с producer ID raw output. Использовать canonical представление, не дублировать raw response_item.

`UserMessage` означает канонический пользовательский ввод движка, но **не доказанное человеческое происхождение**: heartbeat тоже проходит этим producer. Поле origin отсутствует; допустима пометка canonical_user/human_unverified, не UserConfirmation. Assistant final phase не означает terminal хода. Text-only projection не означает, что исходное сообщение не имело других content types; это ограничение надо сохранить.

Raw `response_item` не имеет самостоятельных thread_id/turn_id; без предыдущего контекста нельзя надёжно привязать его к ходу. Его provider ID не равен canonical UserMessage ID. `compacted` в paginated visible-history projection не создаёт сообщений; replacement context не становится новым вводом. world_state/turn_context/legacy/inter-agent metadata тоже не самостоятельные canonical messages.

`task_complete`/alias turn_complete имеет turn_id, но не own event ID/thread_id. error=None означает completed, иначе failed. turn_aborted может не иметьturn_id, тогда его нельзя безконтекстно назначить ходу. Session_meta.id — выбранныйthread, session_id — rootlineage, не подменаid. history_base иsubagent_history_start_ordinal задают наследование: точная безопасная обработка terminal в tail безполногоprefix ещё исследуется и доfinishedspec должна бытьрешена.

Опоры: [session_meta](https://github.com/openai/codex/blob/4ebc485ca321243727539187169425fcf972f2c4/codex-rs/protocol/src/protocol.rs#L3245), [item_completed](https://github.com/openai/codex/blob/4ebc485ca321243727539187169425fcf972f2c4/codex-rs/protocol/src/protocol.rs#L2057), [items](https://github.com/openai/codex/blob/4ebc485ca321243727539187169425fcf972f2c4/codex-rs/protocol/src/items.rs#L80), [delegation producer](https://github.com/openai/codex/blob/4ebc485ca321243727539187169425fcf972f2c4/codex-rs/core/src/hook_runtime.rs#L744), [terminal](https://github.com/openai/codex/blob/4ebc485ca321243727539187169425fcf972f2c4/codex-rs/protocol/src/protocol.rs#L2266), [visible projection](https://github.com/openai/codex/blob/4ebc485ca321243727539187169425fcf972f2c4/codex-rs/app-server-protocol/src/protocol/thread_history_projection.rs#L17), [user producer](https://github.com/openai/codex/blob/4ebc485ca321243727539187169425fcf972f2c4/codex-rs/core/src/session/mod.rs#L5218), [heartbeat origin](https://github.com/openai/codex/blob/4ebc485ca321243727539187169425fcf972f2c4/codex-rs/history/src/heartbeat.rs#L12).

Версионная совместимость за пределами закреплённогоформата не заявляется. Обычный текст плана остаётся текстом источника, не подтверждённым Core plan. Следующие обязательные шаги — разрешить identity/permissions/inheritance, зафиксировать договор, finished Plan Review+Evidence Audit, затем ticket и Sol implementation после10A.

## Уточнённые границы и привязка доступа

Ordinal действительно присутствует в paginated JSONL envelope и не равен byte offset. Parent отдельно прочитал RolloutLine и materialization закреплённойверсии: отсутствующий ordinal — аномалия; duplicate/regressed ordinal не проецируется; subagent prefix отбрасывается по ordinal до разбора item/terminal. Совпадение turn ID не заменяет этот фильтр. [Оболочка](https://github.com/openai/codex/blob/4ebc485ca321243727539187169425fcf972f2c4/codex-rs/history/src/lib.rs#L336), [фильтр](https://github.com/openai/codex/blob/4ebc485ca321243727539187169425fcf972f2c4/codex-rs/thread-store/src/local/thread_history_materialization.rs#L200).

Researcher рекомендует узкий начальный случай без history_base/fork и отдельно проверяемое сравнение embedded ordinal с subagent boundary. Referenced rollouts автоматически не открывать. Evidence02 не сохранил все inheritance fields, поэтому прежние живые пробы нельзя автоматически объявить проверкой каждого нового supported case. Terminal с явным turn_id допустим только после проверки header и принадлежности записи собственной области.

Type Scaffolder рекомендует persisted association на конкретной привязке evidence к выбранному источнику: selection_id + settings_revision, отдельно от canonical SourceKey. Копия файла с тем же thread/event ID не должна молча перепривязать ранее сохранённое сведение. Удаление/изменение настройки отзывает использование связанного evidence, сохраняя историю. Domain source settings остаются в Core, а native выбирает/канонизирует путь и владеет I/O; отдельная native база разрешений не нужна. Точные поля и поведение явной перепривязки ещё фиксируются в ADR.

## Finished review и закрытие уточнений

Plan Reviewer подтвердил PASS после четырёх поправок: cursor по физическим строкам, event hash без window metadata, порядок ordinal между settings revisions и атомарная запись evidence/association/task revision. Evidence Audit обнаружил отдельный forked_from_ordinal_exclusive и необходимость конкретного metadata-контракта. Parent сверил SessionMeta официального commit: поле fork ordinal независимо от history_base; любой non-null, включая0, отклоняется узким14A. cli_version проверяется точной строкой, history_mode явно paginated. Поддержка subagent boundary остаётся отдельным явно тестируемым случаем, не отвергается вместе с fork.

Type Scaffolder повторно проверил контракт read-only. Приняты reuse SourceFact/observed_at_utc/SourceBinding.version и Codex-specific metadata на существующей source binding; generic memory ABI не меняется. ADR задаёт IDs2048байт, совокупные16KiB на binding и учёт metadata в64MiB. Structured terminal/start получает помеченное фактическое описание, не выдуманную цитату. Window details остаются временными; постоянный охват этого адаптера всегда local_records_only/local_window. Целевые повторные Plan Review и Evidence Audit подтвердили PASS; новые таблицы/проверки остаются будущей реализацией14A.
