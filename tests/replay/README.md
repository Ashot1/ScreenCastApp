# Корпус эпизодов

Здесь лежат синтетические/обезличенные входы и ожидаемые решения. Реальные личные снимки не нужны для foundation. Список случаев: [testing](../../docs/testing.md).

## Наблюдение (`observation/`, ADR-025, 27B)

Эпизоды 2 (частота и накопление), 3 (незначимые изменения) и 6 (жизненный цикл без карточек) ADR-025. Их прогоняет `src/core/tests/observation_replay.rs` на временной SQLite; ответы подаются прямо в `finish_observation`, без HTTP. Каждый файл записывает `source`, `consent`, `schema_version`, `time_basis`, `geometry` и `summary`; шаги идут по `t` (секунды от начала) с ожиданиями: `policy`, `session`, `pause`, `connectivity_check`, `assistance_available`, `frame` (текст или `repeat`, исход и задержка), `tick`, `begin`, `dispatch` (кадры от новых к старым, потери, снимки), `finish`, `journal`, `restart`, `delete_log`. Снимки — восемь байт-заглушек, рамки слов не нужны до 27C. Зелёный replay не доказывает качество модели и живой захват.
