# 26B: Окно, боковая панель и строка меню по макету

Spec: [ADR-024](../../../decisions/0024-design-fidelity-pass.md); base [ADR-023](../../../decisions/0023-ui-redesign-liquid-glass.md). Decisions: [D83–D86](../decisions.md). Макет: `target/verification/design/ambient-design.pdf`.

**What to build:** Стеклянное окно с белым листом-карточкой, боковая панель по F10, пустая память и ошибка по F18, строка меню по странице 14.

**Blocked by:** приёмка 26A

**Status:** ready — 26A принята; исполнитель запускается оркестратором (D102)

- [ ] Шаг 1 — проба по F10 (`NavigationSplitView`, фон окна, лист, сцена `Settings`); расхождение — QUESTION, без ручного стекла.
- [ ] `columnVisibility: .constant(.all)`, `.toolbar(removing: .title)`, `.toolbar(removing: .sidebarToggle)` на `List`; значок настроек в самой панели.
- [ ] Панель: «Предложения N», «Дела» с «+», строки, «Завершённые N», компактная капсула статуса; значковые кнопки сохраняют текст в `accessibilityLabel`/`.help` и прежние охраны.
- [ ] Пустая память и ошибка открытия по F18; `.tint(Theme.ink)` уходит из `ambientTheme()`, главная кнопка — своя капсула.
- [ ] Строка меню: четыре состояния страницы 14; диалог смены дела по странице 20.
- [ ] Шесть текстов о «проверке High Fast» (`status.cloudUnavailable`, `source.gate`, `check.help`, `feed.unavailable`, `assistance.gate`, `assistance.error.capability`) — по смыслу «успешная проверка подключения High», без Fast (D96), во всех языках.
- [ ] Сверка: снимки «Ambient Visual» на тестовых данных рядом со страницами PDF в `target/verification/26b/compare/`, список совпадений и объяснённых отличий; отпечатки данных пользователя до и после.
- [ ] Перечень проверок ADR-023 снят до и после; каждое перенесённое или исчезнувшее выражение объяснено в handoff.
- [ ] Новые и изменённые строки EN/RU — через Localization Mechanic; чувствительные тексты утверждает оркестратор; `docs/product.md`/`docs/privacy.md` — списком изменений.
- [ ] `yarn verify`, `yarn lint:swift`, `yarn build:macos`, подписи, контрольные сборки по базе оркестратора.
- [ ] Обязательные специалисты, Code Simplicity Review; изменения общих документов перечислены в handoff.

**Не входит:** экраны дела, лента, настройки (26C–26F); будущие состояния наблюдения.
