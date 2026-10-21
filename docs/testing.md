# Проверки

## Что проверяется сейчас

- Генератор инструкций: перенесённые 8 тестов на форматы, одинаковые общие правила, drift, удаление устаревшего и сохранение постороннего.
- `agents:check`: все сгенерированные файлы соответствуют исходникам; команда ничего не записывает.
- Rust: настоящее хранилище SQLite и C ABI проверяются на временных БД, включая повторное открытие, изменение/удаление, ревизии, пределы и ошибки. Формат и Clippy проверяются строго.
- Swift: build, собственный `.swift-format` и реальные Swift–C–Rust behavior tests; Xcode отдельно собирает `.app`. Эти проверки не заменяют визуальное испытание UI или capture.
- Windows CI: конфигурация проверяет общий Rust и agent tooling; удалённый запуск ещё не выполнен, C# shell отсутствует. Реальный C ABI пока подключён и испытан только на macOS.

Результаты текущего прогона находятся в [verification.md](verification.md). Команды — в [development.md](development.md).

## Тесты при реализации

| Уровень | Обязательные случаи |
| --- | --- |
| Core unit | Deadline, reservations, unknown usage, dedupe, bounded queues, task/goal revisions |
| Shared replay | Исчезающий alert; app switch; unchanged screen + timer; overload + ObservationLoss; no network; denied permission |
| Local supervisor | Busy/cold model; cancel tail; late completion; stuck/crashed helper; restart limit/generation; urgent cloud independent |
| Action validator | Changed target/window/geometry; revoked permission; expired proposal; valid historical fact but invalid action; failed postcondition |
| Native integration | Реальные capture coverage/coordinate transforms, multi-display/DPI, frame lifetime, resize/device loss, UI responsiveness |
| Provider contract | Malformed payload; absent object with box; pinned version; rate limit; retry budget; unknown cost |
| FFI | Round-trip and errors on Swift/C#; size limits; ownership/free; cancellation/disposal; no late callbacks |

50–100 подготовленных эпизодов — стартовая цель корпуса, не статистическое доказательство качества. Сохраняем отрицательные случаи и таймауты; не удаляем неудачи из p95.

## Качество и стоимость

Сравниваем полный эпизод с прямым облачным baseline и человеческой оценкой. Разделяем warm/cold, класс задачи, модель/preprocessing, устройство, сеть и нагрузку. Учитываем пропущенные полезные события и ложные подсказки; confidence не заменяет оценку качества.

Начальные ориентиры исходного проекта: p95 события → свидетельство ≤250 мс; срочный payload → dispatch ≤50 мс; narrow online answer ≤3 с; warm local ≤500 мс; фоновая quota 3 с/60 с; audit ≤4/активный час. **Ни один ориентир foundation не измерен и не гарантирован.** Допуск local требует неухудшения качества и измеримой экономии полного маршрута.

Живые платные испытания включаются отдельно с явным dataset, budget, consent и model version. Обычный `yarn verify` никогда не вызывает модели и не снимает экран.
