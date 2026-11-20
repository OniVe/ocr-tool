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

Соберите (или распакуйте готовый дистрибутив) и запустите GUI — CLI не нужен:

```powershell
cargo build --release
.\target\release\ocr-tool-gui.exe
```

При первом запуске GUI показывает две независимые панели:

1. **Download model (~3.4 GB)** — закреплённая пара Chandra Q4_K_M + mmproj,
   с докачкой и проверкой SHA-256, в app-data (или редактируемый Model dir).
2. **Download runtime (~70 MB)** — llama-vulkan, llama-cpu и PDFium в
   редактируемую **Runtime folder** (по умолчанию
   `%LOCALAPPDATA%\ocr-tool\runtime\windows-x64`; на macOS ~25 MB).

У обеих загрузок есть прогресс и отмена; пока идёт любая, Recognize и вторая
кнопка недоступны. Чтобы держать всё в одной папке, задайте и Model dir, и
Runtime folder.

В Windows release GUI собран как Windows-subsystem приложение: двойной клик по
`ocr-tool-gui.exe` открывает только окно GUI, без отдельной консоли. Старт,
ошибки preflight/загрузок и паники (с backtrace) дописываются в
`%LOCALAPPDATA%\ocr-tool\logs\gui.log`
(`<app-data>/ocr-tool/logs/gui.log` в Linux/macOS) и остаются в логе окна;
debug-сборка сохраняет консоль для логов. `ocr-tool.exe` остаётся консольной
программой. Загрузки модели/runtime идут в фоновых потоках, а `llama-server` —
отдельный скрытый sidecar под управлением GUI (in-process inference вне рамок).

CLI-паритет (для роботов/скриптов):

```powershell
cargo run --release -p ocr-cli -- runtime download
cargo run --release -p ocr-cli -- models download
cargo run --release -p ocr-cli -- doctor
cargo run --release -p ocr-cli -- ocr samples\invoice_scan_ru.jpg -o out
cargo run --release -p ocr-cli -- ocr samples\contract_ru.pdf -o out --pages 1-2
```

GUI самодостаточен: если модели нет, он предлагает **Download model
(~3.4 GB)** (прогресс, отмена, докачка `.part`; уже лежащие файлы, в том числе
`chandra-q4.gguf` + `mmproj.gguf`, не перекачиваются), **Choose existing model
folder…** и **Open model folder**. Если нет runtime — поле **Runtime folder** и
кнопку **Download runtime (~70 MB)**. Обе загрузки продолжаются с `.part` и
узнают уже установленные компоненты, поэтому ничего не качается дважды. Если
файлы модели есть, но неверного размера, **Replace / Re-download…** (с
подтверждением) удаляет их и заново скачивает закреплённую пару; для готового
runtime в Advanced paths есть **Re-download runtime**.

Linux/macOS:

```bash
cargo build --release
cargo run --release -p ocr-cli -- runtime download
cargo run --release -p ocr-cli -- models download
cargo run --release -p ocr-cli -- doctor
```

Пути можно задать явно:

```powershell
$env:OCR_LLAMA_SERVER = "E:\llama\llama-server.exe"
$env:OCR_MODEL_DIR    = "E:\models\chandra-ocr-2"
$env:OCR_PDFIUM       = "E:\pdfium\pdfium.dll"
```

или флагами `--llama-dir`, `--model-dir`, `--pdfium`; в GUI — поля Model dir,
llama dir, PDFium в разделе «Advanced paths». Поле **Runtime folder** в панели
Runtime (и тот же выбор в Advanced) заполняет каталог llama-server и библиотеку
PDFium из `runtime/<os>-<arch>/{llama-*,pdfium}`; в эту же папку кнопка
**Download runtime** ставит закреплённые архивы. Папки модели и runtime
независимы — задайте обе, если хотите держать всё вместе.

## CLI

```
ocr-tool ocr <inputs...> -o <dir>
    [--format md|html|both] [--device auto|gpu|cpu] [--dpi 192]
    [--pages 1-5,7] [--password PASS] [--model-dir PATH] [--llama-dir PATH]
    [--pdfium PATH] [--recursive] [--json]
    [--max-pixels N] [--max-tokens N] [--timeout SECONDS]

ocr-tool doctor [--model-dir PATH] [--llama-dir PATH] [--json]
ocr-tool models download [--dir PATH] [--force]
ocr-tool models verify [--dir PATH] [--deep]
ocr-tool runtime download [--dir PATH] [--force]
ocr-tool runtime verify [--dir PATH]
ocr-tool gui
```

- Вход — файлы и/или каталоги; каталоги сканируются на один уровень, с
  `--recursive` — рекурсивно.
- Номера страниц считаются с 1; диапазон обрезается по длине документа.
  Некорректный диапазон (включая `0`) отсекается на preflight с кодом `2`.
- `--password` открывает зашифрованные PDF; пароль передаётся только PDFium и
  не попадает в логи, `_meta.json` или лог GUI.
- Прогресс — в **stderr**, итог или JSON-отчёт — в **stdout**. Если выбрана
  часть страниц, печатается `page 9/11 (selected 1/3)`, а не `page 9/3`.
- Коды выхода: `0` успех, `2` ошибка аргументов/конфигурации (нет входа, модели,
  PDFium; несуществующий явный `--llama-dir` — предупреждение с откатом на
  найденный runtime), `3` ошибка обработки/отмена. Частичные ошибки страниц не
  прерывают документ, успешные результаты сохраняются, но код будет `3`.
- Ошибка одной страницы не прерывает остальные.
- `models verify` проверяет размеры закреплённых файлов без чтения весов;
  `--deep` дополнительно считает SHA-256 (читает ~3,4 ГБ). `models download`
  узнаёт те же переименованные пары, что и OCR (`chandra-q4.gguf` +
  `mmproj.gguf`), и не качает их повторно.
- `runtime download` ставит закреплённые сборки llama.cpp b12535
  (Vulkan/Metal GPU + CPU fallback) и PDFium 8165 в `--dir` (по умолчанию
  `%LOCALAPPDATA%\ocr-tool\runtime\<tag>`). Архивы проверяются по размеру и
  SHA-256 до распаковки, `.part` продолжается с места отмены, компонент
  распаковывается во временный каталог и переименовывается на место.
  `runtime verify` проверяет установленную папку офлайн и выходит с кодом `2`,
  если компонента нет; ошибка `runtime download` — код `3`. Обе команды
  принимают `--dir PATH`; модель и runtime ставятся независимо.

Имена: `<stem>.md`, `<stem>.html`; при коллизии — `-2`, `-3`. Зарезервированные
имена Windows (`CON`, `NUL`, `COM1`, …) получают суффикс `_`. Для каждого
документа пишется `<stem>_meta.json`, для задания — `_meta.json`; параллельные
задания в одном `-o` объединяют документы под файловой блокировкой. Страницы PDF
разделяются `<!-- page N -->` в Markdown и `<section data-page="N">` в HTML.
HTML очищается ammonia, содержит `<meta charset="utf-8">` и `lang="ru"`.

## GUI

```powershell
cargo run --release -p ocr-gui
# или target\release\ocr-tool-gui.exe без аргументов
```

GUI самодостаточен при первом запуске — CLI и переменные окружения не нужны.

- **Без консольного окна** — release-бинарник использует Windows GUI subsystem,
  поэтому запуск из Explorer создаёт только окно приложения. Диагностика
  пишется в `%LOCALAPPDATA%\ocr-tool\logs\gui.log` (Linux/macOS:
  `<app-data>/ocr-tool/logs/gui.log`); при фатальной ошибке до появления окна
  открывается диалог с путём к логу. Debug-сборка сохраняет консоль.
- При старте и при каждом изменении путей выполняются дешёвые проверки (без
  запуска OCR): каталог модели резолвится и размеры двух GGUF сверяются с
  закреплёнными; PDFium ищется, кандидаты `llama-server` перечисляются.
- **Модели нет или она неполная** — панель «Model not found in <path>» или
  «Model files incomplete in <path>» с кнопками **Download model (~3.4 GB)**
  (фоновый поток, байты/проценты по файлам, уже лежащие файлы пропускаются,
  отмена сохраняет `.part` для докачки), **Choose existing model folder…**
  (сразу перепроверка), **Open model folder** и **Replace / Re-download…**.
  Replace спрашивает подтверждение, удаляет существующие файлы модели и их
  `.part` и заново скачивает закреплённую пару; это выход GUI из состояния
  «incomplete / неверный размер», которое обычная загрузка намеренно не
  перезаписывает.
- **Нет runtime** — панель «Runtime not found» показывает редактируемое поле
  **Runtime folder** (подсказка — платформенная папка по умолчанию), кнопки
  **Choose folder…**, **Open folder** и **Download runtime (~70 MB; на macOS
  ~25 MB)**. Загрузка скачивает llama-vulkan, llama-cpu и PDFium для текущей
  платформы, сверяет закреплённый SHA-256 и ставит их в выбранную папку; видны
  прогресс, статус по компонентам и отмена, частичный `.part` продолжается.
  После успеха папка заполняет llama dir (`llama-vulkan` / `llama-cpu`) и
  PDFium (`pdfium/bin|lib/<lib>`), статус пишет
  «Runtime ready … (downloaded to <path>)». В дистрибутиве папка `runtime`
  рядом с exe по-прежнему находится автоматически. Сырой `os error 3` в
  главный статус не попадает.
- **Независимые загрузки** — у модели и runtime свои кнопки и отмена; пока идёт
  любая, Recognize и вторая кнопка недоступны. **Re-download runtime** (с
  подтверждением) находится в Advanced paths, когда runtime уже готов.
- **Источник PDFium** — «Runtime ready» показывает, откуда взята библиотека:
  выбранная папка runtime/путь, `OCR_PDFIUM` или автопоиск рядом с приложением.
  Если `OCR_PDFIUM` перекрывает встроенный runtime, выводится предупреждение.
- **Advanced paths** разворачивается автоматически, пока setup не завершён
  (нет модели, llama-server или PDFium); остаются опциональные поля Model dir /
  llama dir / PDFium / Runtime folder (то же поле, что в панели Runtime).
- Распознавание: файлы/папка (можно перетащить), выходная папка, формат,
  устройство, DPI, диапазон страниц, пароль PDF (скрытое поле), рекурсия;
  кнопка «Recognize» запускает фоновый поток, есть прогресс, лог, отмена и
  «Open output folder». Отмена убивает запрос и дерево процессов
  `llama-server`; закрытие окна отменяет задание и дожидается остановки
  sidecar.

## Как работает

- Один `llama-server` на задание, уникальный `--alias`, свободный локальный
  порт, health-check до 180 с, таймаут запроса 600 с, обязательный
  `--reasoning off`. Перед принятием сервера `/props` или `/v1/models`
  подтверждают alias/путь модели; при несовпадении порт перевыбирается.
- Процесс убивается деревом (`taskkill /T /F`) и Windows Job Object с
  `KILL_ON_JOB_CLOSE`; на Unix — группа процессов и `PR_SET_PDEATHSIG`. Поэтому
  sidecar не остаётся после `taskkill /F` CLI или закрытия GUI. На Windows
  дочерние процессы (`llama-server`, `taskkill`, `llama-server --version`)
  запускаются с `CREATE_NO_WINDOW`, чтобы у GUI без консоли не мигало ни одно
  окно; сам `llama-server` — отдельный скрытый sidecar, которым управляет GUI.
- `--device auto`: сначала GPU-сборка (Vulkan/Metal), при ошибке — CPU; если
  `--llama-dir` указывает на родителя `llama-vulkan` и `llama-cpu`,
  перечисляются обе сборки.
- `doctor` печатает версию `llama-server --version` (таймаут 5 с) для каждого
  кандидата и FileVersion библиотеки PDFium.
- Модель ищется в `--model-dir` → `OCR_MODEL_DIR` → app-data
  (`%LOCALAPPDATA%\ocr-tool\models\chandra-ocr-2`, Linux `~/.local/share/...`,
  macOS `~/Library/Application Support/...`).
- `llama-server`: `OCR_LLAMA_SERVER` → `--llama-dir` → `runtime/<os>-<arch>/llama-vulkan`
  (и `llama-metal`, `llama-cuda`, `llama-cpu`) рядом с exe или в текущем каталоге → `PATH`.
- PDFium: `--pdfium` / папка runtime из GUI → `OCR_PDFIUM` → рядом с exe →
  `runtime/<os>-<arch>/pdfium/{bin,lib}` → текущий каталог. Если `OCR_PDFIUM`
  задан неверно — понятная ошибка, без паники. GUI и `doctor` показывают,
  какой источник сработал (папка runtime/путь, `OCR_PDFIUM` или автопоиск);
  GUI предупреждает, если `OCR_PDFIUM` перекрывает встроенный runtime.
- Пустые логи sidecar в `%TEMP%/ocr-tool` удаляются, хранятся 20 последних.

## Сборка, тесты, упаковка

```bash
cargo test --workspace
cargo build --release
```

Бинарники: `target/release/ocr-tool` и `target/release/ocr-tool-gui`.
Упаковка (модель не входит): `scripts/package.ps1` или `scripts/package.sh`.

`scripts/fetch-runtime.ps1|.sh` скачивают закреплённые llama.cpp b12535 и
PDFium 8165 в `runtime/<target>/` для упаковки; пользователям GUI/CLI они не
нужны (**Download runtime** / `runtime download` ставят тот же layout).
`scripts/package.ps1|.sh` складывают exe, runtime, README и NOTICE в `dist/`.
GitHub Actions собирает артефакты для Windows/Ubuntu/macOS и не публикует
релиз; подписи кода нет. `scripts/acceptance-runtime.ps1` — реальный E2E
загрузки runtime (~69 МБ): SHA-256 архивов, `llama-server --version`, `doctor`,
CPU OCR.

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
- Изображения до двух `--max-pixels` декодируются и уменьшаются; всё, что
  больше, отсекается до выделения битмапа (PNG 6000×6000 не создаёт 354 МБ
  буфера при малом `--max-pixels`).
- Зашифрованные PDF требуют `--password` (или скрытого поля в GUI); поддержку
  схем шифрования определяет PDFium.
- Качество рукописи/печатей/плохих сканов зависит от Chandra; проверяйте
  финансовые и юридические документы вручную.
- Страницы обрабатываются последовательно, без параллелизма.
- Linux/macOS локально не проверялись; CI-конфигурация приложена, но не
  запускалась.
- GUI и CLI сами скачивают и модель, и runtime (llama-server + PDFium);
  дистрибутив дополнительно содержит `runtime/` рядом с exe. Скрытых скриптов
  установки нет.
- Лицензия весов OpenRAIL-M; коммерческое использование не гарантируется.
- `samples/` содержит персональные данные и исключён из git, кроме `SOURCES.md`.

## Приватность и лицензии

Распознавание полностью локальное. Сеть используется только для явных
загрузок модели (кнопка GUI **Download model** или `models download`,
Hugging Face) и runtime (кнопка GUI **Download runtime** или
`runtime download`, GitHub), а также упаковочных `fetch-runtime`. Телеметрии
нет.
Код на Rust — MIT; сторонние компоненты сохраняют свои лицензии, см. [NOTICE](NOTICE).
