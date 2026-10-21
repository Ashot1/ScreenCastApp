# Windows / C#

Зарезервирована граница нативной оболочки; `.csproj`, WinUI/WPF выбор и приложение ещё не созданы. Это явный этап проекта, а не заглушка, возвращающая фиктивный результат.

Оболочка использует тот же Rust core через C ABI. Она владеет Windows Graphics Capture, UI Automation, системными credentials, окнами и display scaling. .NET/SDK/UI baseline фиксируется перед первым native срезом после проверки инструментов на Windows.

Текущий `yarn verify:portable` проверяет Rust и инструменты правил. Даже зелёный этот прогон не проверяет C# UI, FFI или capture.
