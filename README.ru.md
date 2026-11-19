# ocr-tool

Локальное OCR для PDF и изображений (PNG/JPEG/TIFF/WebP/BMP) с упором на
русские документы. PDFium рендерит страницы, локальный `llama-server`
(llama.cpp) запускает Chandra OCR 2, результат сохраняется в Markdown и
самостоятельный HTML в UTF-8. Есть CLI (`ocr-tool`) и GUI (`ocr-tool-gui`).
После загрузки runtime и модели всё работает офлайн.

> **Это не один exe.** Нужны: бинарник `ocr-tool` + библиотека PDFium +
> `llama-server` + файлы модели (~3,4 ГБ). Веса модели никогда не входят в
> поставку; во время распознавания сеть не используется.

## Статус

Локально собрано и проверено на **Windows 11 x64**, Rust 1.99 MSVC, RTX 3060
12 ГБ (Vulkan), llama.cpp **b12535**, PDFium **chromium/8165**, Chandra OCR 2
**Q4_K_M** + **mmproj Q8_0**. Сборка для Linux/macOS описана в
`.github/workflows/release.yml`, но локально в этой версии **не проверялась**.

## Быстрый старт (Windows)

```powershell
# 1. Runtime: llama-server (Vulkan + CPU fallback) и PDFium -> runtime/windows-x64/
.\scripts\fetch-runtime.ps1

# 2. Модель (~3,4 ГБ, докачка, проверка SHA-256)
cargo run --release -p ocr-cli -- models download

# 3. Проверка компонентов
cargo run --release -p ocr-cli -- doctor

# 4. Распознавание
cargo run --release -p ocr-cli -- ocr samples\invoice_scan_ru.jpg -o out
cargo run --release -p ocr-cli -- ocr samples\contract_ru.pdf -o out --pages 1-2
```

Linux/macOS:

```bash
./scripts/fetch-runtime.sh
cargo run --release -p ocr-cli -- models download
cargo run --release -p ocr-cli -- doctor
```

Пути можно задать явно:

```powershell
$env:OCR_LLAMA_SERVER = "E:\llama\llama-server.exe"
$env:OCR_MODEL_DIR    = "E:\models\chandra-ocr-2"
$env:OCR_PDFIUM       = "E:\pdfium\pdfium.dll"
```

или флагами `--llama-dir`, `--model-dir`; в GUI — раздел «Advanced paths».

## CLI

```
ocr-tool ocr <inputs...> -o <dir>
    [--format md|html|both] [--device auto|gpu|cpu] [--dpi 192]
    [--pages 1-5,7] [--model-dir PATH] [--llama-dir PATH]
    [--recursive] [--json] [--max-pixels N] [--max-tokens N] [--timeout SECONDS]

ocr-tool doctor [--model-dir PATH] [--llama-dir PATH] [--json]
ocr-tool models download [--dir PATH] [--force]
ocr-tool gui
```

- Вход — файлы и/или каталоги; каталоги сканируются на один уровень, с
  `--recursive` — рекурсивно.
- Номера страниц считаются с 1; диапазон обрезается по длине документа.
- Прогресс — в **stderr**, итог или JSON-отчёт — в **stdout**.
- Коды выхода: `0` успех, `2` ошибка аргументов/конфигурации (нет входа, модели,
  PDFium), `3` ошибка обработки/отмена. Частичные ошибки страниц не прерывают
  документ, успешные результаты сохраняются, но код будет `3`.
- Ошибка одной страницы не прерывает остальные.

Имена: `<stem>.md`, `<stem>.html`; при коллизии — `-2`, `-3`. Для каждого
документа пишется `<stem>_meta.json`, для задания — `_meta.json`. Страницы PDF
разделяются `<!-- page N -->` в Markdown и `<section data-page="N">` в HTML.
HTML очищается ammonia, содержит `<meta charset="utf-8">` и `lang="ru"`.

## GUI

```powershell
cargo run --release -p ocr-gui
# или target\release\ocr-tool-gui.exe без аргументов
```

Один экран: файлы/папка (можно перетащить), выходная папка, формат, устройство,
DPI, диапазон страниц, рекурсия; кнопка «Recognize» запускает фоновый поток,
есть прогресс, лог, отмена и «Open output folder». Отмена убивает запрос и
дерево процессов `llama-server`.

## Как работает

- Один `llama-server` на задание, свободный локальный порт, health-check до
  180 с, таймаут запроса 600 с, обязательный `--reasoning off`.
- `--device auto`: сначала GPU-сборка (Vulkan/Metal), при ошибке — CPU.
- Модель ищется в `--model-dir` → `OCR_MODEL_DIR` → app-data
  (`%LOCALAPPDATA%\ocr-tool\models\chandra-ocr-2`, Linux `~/.local/share/...`,
  macOS `~/Library/Application Support/...`).
- `llama-server`: `OCR_LLAMA_SERVER` → `--llama-dir` → `runtime/<os>-<arch>/llama-vulkan`
  (и `llama-metal`, `llama-cuda`, `llama-cpu`) рядом с exe или в текущем каталоге → `PATH`.
- PDFium: `OCR_PDFIUM` → рядом с exe → `runtime/<os>-<arch>/pdfium/{bin,lib}` →
  текущий каталог. Если `OCR_PDFIUM` задан неверно — понятная ошибка, без паники.

## Сборка, тесты, упаковка

```bash
cargo test --workspace
cargo build --release
```

Бинарники: `target/release/ocr-tool` и `target/release/ocr-tool-gui`.
Упаковка (модель не входит): `scripts/package.ps1` или `scripts/package.sh`.

`scripts/fetch-runtime.ps1|.sh` скачивают закреплённые llama.cpp b12535 и
PDFium 8165 в `runtime/<target>/`. `scripts/package.ps1|.sh` складывают exe,
runtime, README и NOTICE в `dist/`. GitHub Actions собирает артефакты для
Windows/Ubuntu/macOS и не публикует релиз; подписи кода нет.

## Производительность

- GPU (RTX 3060, Q4_K_M): примерно **20–50 с на страницу A4**; локально
  измерено 25–27 с для счёта 2496×3508 и 43–48 с для двух страниц договора.
- CPU: около **3,3 ток/с генерации**, обычная страница — **10–15 минут**;
  кроп 1248×351 со 96 токенами — 39–45 с (плюс запуск сервера). Для быстрой
  проверки используйте `--pages`, меньшее `--dpi` или кроп.
- Диск: ~3,4 ГБ модель + ~250 МБ runtime + результаты.

CPU — аварийный fallback, а не рабочий режим для пакетной обработки.

## Ограничения

- Не single-exe: нужны PDFium, llama-server и модель.
- CPU очень медленный.
- Текстовый слой PDF игнорируется — все выбранные страницы растрируются.
- Сложные таблицы могут неточно конвертироваться в Markdown (в HTML точнее).
- Качество рукописи/печатей/плохих сканов зависит от Chandra; проверяйте
  финансовые и юридические документы вручную.
- Страницы обрабатываются последовательно, без параллелизма.
- Linux/macOS локально не проверялись; CI-конфигурация приложена, но не
  запускалась.
- В GUI нет загрузчика модели — используйте `models download`.
- Лицензия весов OpenRAIL-M; коммерческое использование не гарантируется.
- `samples/` содержит персональные данные и исключён из git, кроме `SOURCES.md`.

## Приватность и лицензии

Распознавание полностью локальное. Сеть используется только для явных
`models download` (Hugging Face) и `fetch-runtime` (GitHub). Телеметрии нет.
Код на Rust — MIT; сторонние компоненты сохраняют свои лицензии, см. [NOTICE](NOTICE).
