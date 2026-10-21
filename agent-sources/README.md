# Agent sources

Единственный источник правил, ролей и project skills. Native environments получают сгенерированные файлы, а не загружают эту папку автоматически.

- `manifest.json`: output paths, scoped rules и переменные окружений.
- `rules/universal.md`: общие правила из фронтенда с локальными изменениями Ambient.
- `rules/*.md`: Swift, Rust, Windows, bridge, providers и replay.
- `agents/*.md` + `*.json`: тело роли, разрешения и native settings.
- `models.json`: исходные frontend model/effort tiers без изменения.
- `skills/harmony-*`: bootstrap, orchestration, локальные simplicity examples.
- `entrypoints`, `environments`: загрузка правил четырьмя средами.
- `provenance.json`: hashes оригиналов и адаптаций, список точных копий.
- `licenses`, `THIRD-PARTY.md`: сохранённая атрибуция исходных ролей.

`yarn agents:generate` создаёт native copies; `yarn agents:check` сравнивает без записи. Генератор и его 8 fixture tests скопированы без изменения. Scoped outputs расположены под `src/**`, поскольку именно эту область исходный генератор проверяет на удалённые инструкции.

`.agents/skills` содержит 9 исходных Matt skills. Они скопированы без изменения; Claude получает generated copies. Системный Superpowers plugin — внешняя prerequisite, его исходники не включены в этот репозиторий.

Все four native agent directories, `.claude/rules` и `.cursor/rules` принадлежат генератору. Редактировать следует этот источник. Непомеченные посторонние файлы вне принадлежащих генератору каталогов не удаляются.
