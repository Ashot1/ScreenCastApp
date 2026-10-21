# Разработка

## Инструменты

- Node 22.23.1, Yarn 1.22.22 — только генерация правил и её тесты. Зависимости закреплены в package.json/yarn.lock.
- Rust: CI закрепляет 1.99.1 с rustfmt/Clippy. Cargo edition 2024, `rust-version = 1.99`. Основа проверена официальным 1.99.1 из изолированного `target/`; глобальные настройки не менялись. Локальный Homebrew toolchain имеет std для более новой macOS и не подходит для приложения с минимумом macOS 26. [Установка и команды](../src/platforms/macos/README.md).
- macOS: Xcode SDK + Swift 6.4, language mode 6. `swift format` входит в toolchain; полный project config — `.swift-format`. `.swift-version` записывает baseline, но сам по себе не переключает Xcode.
- Windows native shell: .NET/SDK ещё не выбраны; переносимость Rust проверяется отдельно.

## Команды из корня

```sh
yarn install --frozen-lockfile
yarn agents:generate
yarn agents:check
yarn test:tooling
yarn check:core
yarn lint:core
yarn test:core
yarn check:swift
yarn lint:swift
yarn test:swift
yarn verify
yarn build:macos
```

Windows: `yarn verify:portable` выполняет общие Rust/tooling checks. На macOS `yarn verify` добавляет Swift build, строгий lint и настоящие Swift–C–Rust tests на временной SQLite. `yarn build:macos` отдельно создаёт `target/macos/Build/Products/Debug/Ambient.app` через Xcode. Эти команды не снимают экран и не обращаются к модели.

`swift format format --in-place --configuration .swift-format <files>` форматирует выбранные Swift-файлы. `cargo fmt --all` форматирует Rust. Проверки не запускают автоисправление.

## CI

`portable.yml`: macOS + Windows, Node/Yarn и Rust 1.99.1, генерация/check/tests/Clippy. `macos.yml`: macOS SDK, официальный Swift 6.4 toolchain, native build и lint. Workflow-файлы подготовлены, но удалённые runs не запускались: репозиторий не опубликован.

Swift CI загружает подписанный официальный пакет Swift и проверяет подпись installer перед установкой. Отдельные runtime models, Python, Node и `.NET` не добавляются в desktop app автоматически.

