# 24B: Google Диск и Документы через workspace-mcp

Spec: [ADR-022](../../../decisions/0022-local-mcp-servers.md), [ADR-021](../../../decisions/0021-plugins-layer.md). Decisions: [D63, D64, D68, D69, D71](../decisions.md). Evidence: [Google и workspace-mcp](../evidence/23-plugins-mcp-research.md), живая проверка входа `target/verification/google-spike/findings.md`.

**What to build:**
1. Пользователь нажимает «Добавить Google» и проходит обычный просмотр встроенного плагина «Google Диск и Документы».
2. Ставит сервер `workspace-mcp` кнопкой «Установить» (`uv` уже установлен пользователем) строго по закреплённым версиям и хэшам.
3. Входит через Google (встроенный Desktop-клиент; права `openid`, email, `drive.readonly`, `documents.readonly`).
4. Во время помощи модель сама ищет и читает его документы, ссылается на них, а процитированное сохраняется в память дела.

Сервер работает только на 127.0.0.1, без своих учётных данных, на свежем токене от Ambient. Недельное истечение тестового режима показывается честно; повторный вход не отменяет сохранённое.

**Blocked by:** [24C](24c-local-plugin-servers.md) — принят оркестратором 21 октября 2026. Действие пользователя для живой проверки: включить обычные Google Drive API и Google Docs API в проекте Google Cloud (без Preview); OAuth-клиент Desktop app у пользователя есть.

**Status:** in-progress — локальная часть выполнена (`target/verification/24b/`); живой пилот отложен пользователем (pending: user). Baseline контрольных сборок `target/verification/orchestration/24b-checkpoints-baseline.json` (20 приложений, 330 файлов).

- [x] Вариант `Managed` и `ManagedServerId`:
  - исчерпывающие `match` списка (Core), рецепта (providers) и реестра OAuth;
  - встроенный пакет в Core (`include_bytes!`) и `builtin_package_hash()`;
  - происхождение `Managed` проверяется при просмотре и установке, при открытии — только структура;
  - `ambient_managed` вне встроенного пакета — предупреждение и пропуск.
- [x] Положительный список инструментов Google в Core проверяется при одобрении и при выдаче разрешения на вызов; итоговые имена сверены с `tools/list` закреплённой версии.
- [x] Установка (`managed_install` через описатель) и `managed_status` (`not_installed | installed | outdated | uv_missing`):
  - встроенный файл требований arm64 с хэшами (`uv export --format requirements.txt …` плюс строка `workspace-mcp==2.1.1` с PyPI);
  - `uv` в четырёх фиксированных местах;
  - `uv venv --python cpython@3.12.<патч>` с управляемым Python;
  - `--require-hashes --only-binary :all: --no-deps`;
  - очищенное окружение установки;
  - уборка `.partial`, проверка `python -P -c "import main"`, переименование, удаление старых папок;
  - статус «Нужно обновить сервер Google»;
  - удаление папки в macOS при свободном `busy`.
- [x] Запуск: `python -P -m main --transport streamable-http --read-only --tools drive docs`, окружение ADR-022, случайный порт с пробной привязкой, stdout → `/dev/null`, готовность по `initialize`.
- [x] Провод loopback HTTP только из ветви `Managed`; `allowed()` только для `Remote`; ответ больше 256 KiB — ошибка вызова без `Failed`.
- [x] Вход OAuth (Endpoint Registry Mechanic для реестра):
  - `oauth2` 5.0.0, PKCE S256, loopback 127.0.0.1;
  - `state`, проверка `iss` = `https://accounts.google.com`;
  - `client_secret`, без `resource`;
  - обновление без ротации и с ротацией в fixture;
  - `invalid_grant` → отзыв ключа и «Войдите снова»;
  - токен как `Authorization: Bearer` к серверу;
  - секрет `oauth(…)` в Связке ключей.
- [x] Тексты просмотра и установки для Google по ADR-022 (что скачивается, защита и её предел, сеть, права, данные, предупреждения); EN/RU, чувствительные строки утверждает оркестратор.
- [x] Тесты ADR-022 для 24B на тестовом HTTP-сервере и тестовом `uv` (точки подстановки только в test-support).
- [ ] Ручные проверки приёмки (в handoff для `docs/verification.md`):
  - настоящий `uv` с подменённым хэшем;
  - сервер не отвечает по адресу локальной сети;
  - stateless-режим не пишет вне частной папки;
  - холодный запуск, размер и время установки;
  - переименованная venv запускается;
  - длинный Doc даёт честную ошибку;
  - размер `tools/list`;
  - проверяет ли `uv` хэш сборки Python (если нет — вопрос пользователю).
- [ ] Живой пилот с проектом пользователя:
  - вход, поиск и чтение документа, ответ со ссылкой, автосохранение;
  - реальные размеры результатов, повторный вход;
  - проверки пересылки рассуждения (`include`, элементы с `id` при `store:false`, предел 64 KiB);
  - это подтверждение вызова инструментов Luna на подписке (D64).
- [ ] Специалисты, TDD, Code Simplicity Review и полный набор проверок с базой контрольных сборок после 24C; список изменений общих документов.

**Не входит:** официальные MCP-серверы Google (Preview); запись в Drive/Docs; песочница; Intel Mac; обнаружение RFC 9728/8414, CIMD и DCR; проверка приложения Google для распространения; Windows.
