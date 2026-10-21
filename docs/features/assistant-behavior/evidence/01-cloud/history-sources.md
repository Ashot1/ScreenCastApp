# Проверенная конфигурация кандидата

**Архив прежнего оплачиваемого API-варианта.** Пользователь затем выбрал подписку ChatGPT. Эти тарифы и формула не описывают подписку, не разрешают платные запросы и не доказывают переносимость сохранённого входа CLI в Ambient. Текущее состояние и предложение адаптации — в report.md и runbook.md.

Проверено 2026-10-18 External Researcher; исполнитель отдельно сверил модель, ставки Standard, категории кэша, выходной предел и retention. Это документальные факты, а не доступ выбранного аккаунта.

## Маршрут

OpenAI `POST https://api.openai.com/v1/responses`, `model=gpt-6.1-sol`, `service_tier=default`, `reasoning.effort=low`, `background=false`, `store=false`, без инструментов, conversation и previous_response_id. `truncation=disabled`. Проба использует только собственные синтетические данные.

Документированы текстовый вход/выход, изображения на входе и Structured Outputs. Контекст — 1 050 000 токенов, выход — до 128 000. В эксперименте выход ограничен 2048, в пробе incomplete — 16. Датированный snapshot на странице не опубликован, поэтому alias и дата проверки не обеспечивают неизменность весов. Free API tier модель не поддерживает. [Карточка модели](https://developers.openai.com/api/docs/models/gpt-6.1-sol).

`max_output_tokens` ограничивает видимый вывод вместе с рассуждением. `store` по умолчанию включён; выбранное false нужно передавать явно. `service_tier=auto` использовал бы настройки проекта, поэтому не подходит фиксированному тарифу пробы. HTTP reference при проверке вернул Internal Error; прочитана официальная CLI-проекция контракта. [Создание Responses](https://developers.openai.com/api/reference/cli/resources/responses/methods/create).

Строгая схема ограничивает форму. Отказ и незавершённый ответ обрабатываются отдельно; правильность оснований и полезность проверяет человек. [Structured Outputs](https://developers.openai.com/api/docs/guides/structured-outputs).

## Стоимость

USD за миллион токенов для выбранного Standard:

| Вход | Обычный вход | Чтение кэша | Запись кэша | Выход |
| --- | ---: | ---: | ---: | ---: |
| ≤272000 | 2 | 0.10 | 2.50 | 10 |
| >272000, весь запрос | 4 | 0.20 | 5 | 15 |

Regional/FedRAMP endpoints добавляют 10%; эксперимент их не использует и не принимает override endpoint. Fast/Ultrafast, Batch/Flex и дополнительные инструменты не входят в эту конфигурацию. Индивидуальный договор, налоги и конвертация валют не проверены. До реального подключения нужно подтвердить применимость опубликованного тарифа аккаунту. [Pricing](https://developers.openai.com/api/docs/pricing).

Категории входа не складываются: обычные токены = input_tokens − cached_tokens − cache_write_tokens. Запись кэша — 125% обычного входа; рассуждение уже включено в output_tokens. Кэш включён по умолчанию. [Prompt caching](https://developers.openai.com/api/docs/guides/prompt-caching), [Reasoning](https://developers.openai.com/api/docs/guides/reasoning).

Консервативный предел одной попытки выводится из глобального потолка входа и самых дорогих применимых ставок:

```text
R = (1050000 × 5 + Omax × 15) / 1000000 USD
Omax=2048 → R=5.280720 USD
Omax=16   → R=5.250240 USD
```

Это расчёт верхнего резерва, не выбранный бюджет и не оценка обычной цены короткого запроса. Он намеренно избыточен: все возможные входные токены считаются длинной записью кэша. При превышении контекста запрос должен отклоняться, а не обрезаться. Предполагается соблюдение опубликованных ограничений API. Арифметика ведётся целыми микродолларами с округлением вверх.

Точный endpoint подсчёта входа существует, включая изображения и схемы, но явной гарантии его бесплатности и отдельных условий retention не найдено. Проба его не вызывает: нет дополнительного обращения с недоказанным счётом. [Counting tokens](https://developers.openai.com/api/docs/guides/token-counting), [count reference](https://developers.openai.com/api/reference/cli/resources/responses/subresources/input_tokens/methods/count).

## Отмена и данные

Синхронный запрос можно отменять закрытием соединения; API cancel предназначен для background. Закрытие не доказывает нулевой счёт или момент остановки сервера. Для store=false нельзя полагаться на последующее retrieval утраченного usage. Поэтому неизвестный расход удерживает полный резерв. [Background и cancellation](https://developers.openai.com/api/docs/guides/background), [Responses store](https://developers.openai.com/api/reference/cli/resources/responses/methods/create).

API-данные по умолчанию не используются для обучения; opt-in возможен. Abuse logs могут содержать запрос/ответ и обычно хранятся до 30 дней, с исключениями. store=false не отключает их. Кэш может хранить зашифрованные KV-тензоры до 24 часов; новый TTL 30m обозначает минимум, не обещание удаления. Проверка изображений на CSAM имеет исключения хранения. ZDR/MAM не предполагаются: их доступ конкретному аккаунту не проверен. Локальное удаление не удаляет облачные копии. [Data controls](https://developers.openai.com/api/docs/guides/your-data).

Доступ API и оплата отделены от подписки ChatGPT. Ни подписка, ни работа этой сессии не являются ключом Ambient. [Раздельный billing](https://help.openai.com/en/articles/9039756-managing-billing-for-chatgpt-and-the-api-platform).
