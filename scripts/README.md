# Repository tooling

`generate-agent-sources.mjs` и `generate-agent-sources.test.js` перенесены из frontend byte-for-byte. Генератор владеет только объявленными native outputs и помеченными generated files. Scoped outputs размещены под `src` для сохранения исходного поиска устаревших правил.

`yarn agents:generate` записывает адаптеры, `yarn agents:check` только сравнивает. `yarn test:tooling` запускает ровно перенесённый suite.

`macos.sh` выполняет native `check`, `test`, `bridge` или `app`. Он закрепляет deployment target 26.0, передаёт Swift строгие compiler checks и держит build/cache в `target/` / Swift package `.build/`. `test` сначала собирает настоящий Rust staticlib; `app` использует Xcode target с Cargo build phase. Скрипт не является runtime-зависимостью приложения. Проверки и toolchain: [macOS README](../src/platforms/macos/README.md).
