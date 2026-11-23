<a id="readme-top"></a>

<!-- Бейджи -->
[![CI][ci-shield]][ci-url]
[![Release][release-shield]][release-url]
[![License: MIT][license-shield]][license-url]
[![Rust 1.82+][rust-shield]][rust-url]
[![Платформа][platform-shield]][platform-url]
[![100% локально][offline-shield]][offline-url]

<br />

<div align="center">
  <a href="https://github.com/onive/ocr-tool">
    <img src="assets/icon-256.png" alt="логотип ocr-tool" width="96" height="96">
  </a>

  <h3 align="center">ocr-tool</h3>

  <p align="center">
    Локальное OCR для PDF и изображений — офлайн, с упором на русский язык.
    <br />
    PDFium рендерит страницы, локальный llama.cpp запускает Chandra OCR 2, результат сохраняется в Markdown, HTML и JSON.
    <br /><br />
    <a href="README.md">English</a> · <strong>Русский</strong>
    <br /><br />
    <a href="#начало-работы">Быстрый старт</a>
    ·
    <a href="docs/screenshots/pixel-ui/">Скриншоты</a>
    ·
    <a href="https://github.com/onive/ocr-tool/releases">Скачать</a>
    ·
    <a href="https://github.com/onive/ocr-tool/issues/new?labels=bug">Сообщить об ошибке</a>
    ·
    <a href="https://github.com/onive/ocr-tool/issues/new?labels=enhancement">Предложить идею</a>
  </p>
</div>

<details>
  <summary>Содержание</summary>
  <ol>
    <li>
      <a href="#о-проекте">О проекте</a>
      <ul>
        <li><a href="#стек">Стек</a></li>
      </ul>
    </li>
    <li>
      <a href="#начало-работы">Начало работы</a>
      <ul>
        <li><a href="#требования">Требования</a></li>
        <li><a href="#установка">Установка</a></li>
      </ul>
    </li>
    <li><a href="#использование">Использование</a></li>
    <li><a href="#как-это-работает">Как это работает</a></li>
    <li><a href="#планы">Планы</a></li>
    <li><a href="#участие">Участие</a></li>
    <li><a href="#лицензия">Лицензия</a></li>
    <li><a href="#контакты">Контакты</a></li>
    <li><a href="#благодарности">Благодарности</a></li>
  </ol>
</details>

<!-- О ПРОЕКТЕ -->
## О проекте

[![GUI ocr-tool в состоянии готовности][hero-screenshot]](docs/screenshots/pixel-ui/)

`ocr-tool` — локальное приложение OCR для PDF и изображений
(PNG/JPEG/TIFF/WebP/BMP) с упором на русские документы. PDFium рендерит
выбранные страницы, [Chandra OCR 2](https://github.com/datalab-to/chandra) под
управлением локального `llama-server` (llama.cpp) распознаёт их, а приложение
сохраняет Markdown, самостоятельный HTML в UTF-8 и/или JSON с блоками и bbox.
Есть CLI (`ocr-tool`) и GUI (`ocr-tool-gui`). После загрузки runtime и модели
всё работает офлайн.

> **Это не один exe.** Рабочая установка — это бинарник `ocr-tool` + библиотека
> PDFium + `llama-server` + файлы модели (~3,4 ГБ). Веса модели никогда не
> входят в поставку; это не облачный сервис, и во время распознавания сеть не
> используется.

### Ключевые возможности

- **100% локально.** Распознавание не обращается к сети, телеметрии нет. Сеть
  нужна только для явных загрузок модели и runtime.
- **CLI и GUI.** GUI самодостаточен при первом запуске; CLI даёт тот же
  конвейер для скриптов и пакетной обработки.
- **PDF и изображения.** PDF, PNG, JPEG, TIFF, WebP и BMP; страницы
  рендерятся PDFium с выбранным DPI.
- **Markdown, HTML и JSON.** Самостоятельный HTML в UTF-8 (`lang="ru"`, очистка
  ammonia), Markdown с маркерами страниц и JSON с bbox, метками и id блоков.
- **Блоки и assets.** Опциональные PNG-кропы блоков и типизированные файлы
  (`.md`, `.csv`, `.tex`, `.html`, `.txt`) со ссылками из документа.
- **Профили Качество и Быстро.** 4096 или 2048 токенов изображения; Качество
  остаётся профилем по умолчанию для юридических и финансовых документов.
- **Две страницы одновременно.** `--parallel-pages 2` перекрывает запросы PDF
  на GPU и даёт ускорение пакета в 1,5–1,9 раза на многостраничных документах.
- **Предзагрузка модели.** GUI держит `llama-server` и модель в памяти между
  заданиями (включено по умолчанию, ~5 ГБ VRAM/RAM).
- **Русский pixel-art интерфейс.** Frameless-окно, собственная шапка, русские
  подписи и палитра cream/ink.
- **Интерфейс EN/RU/ZH.** Язык интерфейса выбирается по локали системы при
  первом запуске и переключается в шапке или в окне «Дополнительно»; выбор
  сохраняется.
- **Сохранённые настройки.** Пути, формат, устройство, DPI, параметры скорости
  и предзагрузка восстанавливаются при следующем запуске. Пароли и содержимое
  документов не сохраняются.

### Статус

Версия **0.1.0**. Локально собрано и проверено на **Windows 11 x64**, Rust 1.99
MSVC, NVIDIA RTX 3060 12 ГБ (сборка Vulkan), llama.cpp **b12535**, PDFium
**chromium/8165**, Chandra OCR 2 **Q4_K_M** + **mmproj Q8_0**. **216** тестов
workspace проходят на Windows; Linux и macOS проверяются в CI-матрице
(`.github/workflows/ci.yml`), а не на локальном железе. Подпись релизов
опциональна: пайплайн поддерживает Windows Authenticode и macOS Developer
ID/notarization (см. [docs/signing.md](docs/signing.md)), но текущие архивы
**без подписи**, пока не настроены секреты. Установщика по-прежнему нет.

### Стек

[![Rust][rust-stack-shield]][rust-stack-url]
[![egui/eframe][egui-shield]][egui-url]
[![llama.cpp b12535][llama-shield]][llama-url]
[![PDFium 8165][pdfium-shield]][pdfium-url]
[![Chandra OCR 2][chandra-shield]][chandra-url]

Приложение написано на Rust с **egui/eframe 0.31**. Инференс идёт в закреплённом
sidecar **llama.cpp b12535**; PDF рендерит **PDFium chromium/8165** (PDFium
160.0.8165.0) из pdfium-binaries; модель — **Chandra OCR 2** (Q4_K_M + mmproj
Q8_0).

<p align="right">(<a href="#readme-top">наверх</a>)</p>

<!-- НАЧАЛО РАБОТЫ -->
## Начало работы

### Требования

- **Windows 11 x64, Linux или macOS.** Windows 11 x64 проверен локально; Linux
  и macOS проверяются в CI-матрице.
- **Rust 1.82+** для сборки из исходников. В Windows нужен toolchain MSVC, в
  Linux — обычные dev-пакеты GTK/eframe. Пакеты платформ и проверки перечислены
  в [CONTRIBUTING.md](CONTRIBUTING.md).
- **GPU не обязателен.** При наличии используется сборка Vulkan/Metal, иначе
  автоматически включается CPU. CPU-OCR работает очень медленно (см.
  [Производительность](#производительность)).
- **Диск:** ~3,4 ГБ модель плюс ~70 МБ runtime (на macOS ~25 МБ); пик — примерно
  3,4 ГБ модель + 250 МБ runtime + результаты.
- **Установщик и права администратора** для релизного архива не нужны.

Модель не входит ни в репозиторий, ни в релизный архив. GUI и CLI скачивают её
только по явному запросу.

### Установка

1. Скачайте последний релиз из
   [Releases](https://github.com/onive/ocr-tool/releases):
   `ocr-tool-0.1.0-windows-x64.zip` для Windows, `.tar.gz` — для Linux/macOS.
2. Распакуйте в любую папку — установщика нет, права администратора не нужны.
3. Запустите `ocr-tool-gui.exe` (Windows) или GUI-бинарник из распакованной
   папки.

При первом запуске GUI показывает панель **ПОДГОТОВКА** с двумя независимыми
строками:

1. **СКАЧАТЬ МОДЕЛЬ (~3,4 ГБ)** — закреплённая пара Chandra Q4_K_M + mmproj,
   с докачкой и проверкой SHA-256, в app-data (или папку, выбранную через
   **ПАПКА МОДЕЛИ**).
2. **СКАЧАТЬ ДВИЖОК (~70 МБ)** — llama-vulkan, llama-cpu и PDFium в
   редактируемую папку движка (по умолчанию
   `%LOCALAPPDATA%\ocr-tool\runtime\windows-x64`; на macOS ~25 МБ).

Релизный архив уже содержит `runtime/`; движок GUI скачивает, только если его
нет — например, при сборке из исходников или после удаления папки. Строка
модели помечает состояние `НЕТ` или `НЕПОЛНАЯ`. У обеих загрузок есть прогресс
и отмена; пока идёт любая, **РАСПОЗНАТЬ** и вторая кнопка недоступны. Загрузки
продолжаются с `.part` и узнают уже установленные компоненты, поэтому ничего не
качается дважды. Если файлы модели есть, но неверного размера, **ЗАМЕНИТЬ**
(с подтверждением) удаляет их вместе с остатками `.part` и заново скачивает
закреплённую пару; для готового движка есть **ПЕРЕУСТАНОВИТЬ**. Отсутствие
runtime показывается понятным состоянием подготовки, а не сырым `os error 3` в
главном статусе. Чтобы держать всё в одном месте, выберите обе папки внутри
общей родительской через **ПАПКА МОДЕЛИ** / **ПАПКА ДВИЖКА**.

После этого распознавание идёт локально, CLI и сеть не нужны.

#### Сборка из исходников

```powershell
git clone https://github.com/onive/ocr-tool.git
cd ocr-tool
cargo build --release
cargo run -p ocr-gui
```

Бинарники: `target/release/ocr-tool` и `target/release/ocr-tool-gui`
(`.exe` в Windows). В сборке из исходников нет папки `runtime/`: перед первым
заданием скачайте движок и модель через CLI (или кнопками GUI).

```powershell
cargo run --release -p ocr-cli -- runtime download
cargo run --release -p ocr-cli -- models download
cargo run --release -p ocr-cli -- doctor
```

```bash
# Linux/macOS
cargo build --release
cargo run --release -p ocr-cli -- runtime download
cargo run --release -p ocr-cli -- models download
cargo run --release -p ocr-cli -- doctor
cargo run --release -p ocr-cli -- ocr path/to/document.pdf -o out
```

Публичный репозиторий **не содержит тестовых документов**. `samples/SOURCES.md`
описывает только происхождение локальных фикстур; сами файлы содержат
персональные данные и не публикуются. Используйте свой файл, например
`C:\docs\document.pdf` (Windows) или `path/to/document.pdf` (Linux/macOS).

<p align="right">(<a href="#readme-top">наверх</a>)</p>

<!-- ИСПОЛЬЗОВАНИЕ -->
## Использование

### GUI

Запустите `ocr-tool-gui.exe` (релиз) или `cargo run --release -p ocr-gui` из
исходников; CLI и переменные окружения не нужны.

[![GUI ocr-tool после завершения задания][gui-done]](docs/screenshots/pixel-ui/)

- **Добавление файлов.** **ДОБАВИТЬ ФАЙЛЫ** открывает выбор файлов,
  **ДОБАВИТЬ ПАПКУ** добавляет каталог, работает и drag and drop. Каталоги
  сканируются на один уровень, если не включена рекурсия.
- **Параметры задания.** Папка сохранения (**ПАПКА СОХРАНЕНИЯ**), формат (`MD`,
  `HTML`, `JSON`, `ОБА`, `ВСЁ`), устройство, DPI, диапазон страниц, пароль PDF
  (скрытое поле), рекурсия и режим **Извлечение блоков** (выкл / только рисунки
  / все блоки).
- **Скорость.** Карточка **СКОРОСТЬ** выбирает **КАЧЕСТВО** (4096 токенов
  изображения) или **БЫСТРО** (2048; примерно на 15% быстрее, строки таблиц
  могут сдвигаться), опцию **2 страницы одновременно (PDF; GPU)** и
  **ПРЕДЗАГРУЗКА МОДЕЛИ**.
- **Предзагрузка.** Включена по умолчанию: после подготовки GUI запускает
  `llama-server` и держит модель в памяти, поэтому первый документ не тратит
  время на старт движка. Сервер переиспользуется между заданиями, статусы —
  `ЗАГРУЖАЕМ МОДЕЛЬ…` и `МОДЕЛЬ В ПАМЯТИ`; занято примерно **5 ГБ VRAM/RAM**.
  Отключается, если память нужна другим приложениям; ошибка предзагрузки не
  блокирует распознавание.
- **Запуск.** **РАСПОЗНАТЬ** работает в фоновом потоке с прогрессом, статусом
  страниц, логом и отменой. Отмена убивает запрос и дерево процессов
  `llama-server`; закрытие окна отменяет задание и дожидается остановки sidecar.
- **Результат.** **ОТКРЫТЬ ПАПКУ**, **ОТКРЫТЬ HTML**, **ОТКРЫТЬ MD**,
  **ОТКРЫТЬ JSON**. Кнопка файла появляется только когда файл существует,
  поэтому **ОТКРЫТЬ MD** нет у задания только с HTML или JSON.
- **Горячие клавиши.** `Ctrl+O` — добавить файлы, `Ctrl+Enter` — распознать,
  `Esc` — отменить.
- **Сохранённые настройки.** Output/format/device/DPI/blocks/fast/parallel,
  `model_dir`, `runtime_dir`, `llama_dir`, `pdfium`, `pages`, `recursive`,
  `max_pixels`, `max_tokens`, таймаут запроса (`timeout_secs`) и
  `preload_model` хранятся в `gui-prefs.json` в app-data. Пароль, имена
  документов и их содержимое не сохраняются.
- **Дешёвые проверки.** При старте и при каждом изменении путей GUI резолвит
  каталог модели, сверяет размеры двух GGUF с закреплёнными, ищет PDFium и
  перечисляет кандидатов `llama-server` — без запуска OCR.
- **Frameless-окно.** Нативное оформление отключено, шапка заменяет title bar:
  за неё окно перетаскивается, двойной клик разворачивает/восстанавливает,
  края служат для resize, справа — кнопки `—` / `□` / `×`. Минимальный размер —
  **1000×720** (по умолчанию 1180×840); окно остаётся resizable, сохраняет
  иконку и видно на панели задач.
- **Без консольного окна.** Релизный бинарник Windows использует GUI subsystem
  (PE subsystem 2; у CLI — 3), поэтому запуск из Explorer создаёт только окно
  приложения. Старт, ошибки preflight/загрузок и паники (с backtrace)
  дописываются в `%LOCALAPPDATA%\ocr-tool\logs\gui.log`
  (`<app-data>/ocr-tool/logs/gui.log` в Linux/macOS) и остаются в логе окна;
  фатальная ошибка до появления окна открывает диалог с путём к логу.
  Debug-сборка сохраняет консоль, а `ocr-tool.exe` остаётся консольной
  программой.
- **Примерное время до окончания с самого начала.** До первой готовой страницы
  GUI показывает намеренно осторожную оценку: ~12 мин на каждую выбранную
  страницу плюс ~20 с на запуск для CPU (Быстро: ~10 мин на страницу); ~30 с на
  страницу плюс ~10 с на запуск для GPU (Быстро: ~25 с). При двух страницах
  одновременно ETA считает пачки по две. Auto следует найденному runtime:
  оценка GPU, если есть GPU-кандидат, иначе осторожная оценка CPU. После первых
  страниц оценка уточняется по фактическому среднему. Для нескольких файлов
  подпись явно говорит «по текущему файлу»; пока число страниц неизвестно, ETA
  не показывается.
- **Pixel-art.** Все видимые подписи русские (бренд и технические имена вроде
  `OCR TOOL`, `PDFium`, `DPI`, `HTML/JSON/MD` остаются как есть), палитра cream
  `#F2EFE7` / ink `#111111`, пиксельные шрифты. Оба Windows-exe несут одну и ту
  же иконку (`assets/ocr-tool.ico`, размеры 16–256); окно GUI использует
  `assets/icon-256.png`. Редактируемый SVG — `assets/ocr-tool.svg`; ICO/PNG
  пересобираются командой `bun scripts/gen-icon.ts`. PNG также используется для
  иконок окна в Linux/macOS; `.icns` для macOS при необходимости генерируется в
  CI из того же исходника. Больше экранов:
  [docs/screenshots/pixel-ui](docs/screenshots/pixel-ui/).

### CLI

```powershell
cargo run --release -p ocr-cli -- runtime download
cargo run --release -p ocr-cli -- models download
cargo run --release -p ocr-cli -- doctor
cargo run --release -p ocr-cli -- ocr C:\docs\document.pdf -o out --format all
```

```
ocr-tool ocr <inputs...> -o <dir>
    [--format md|html|json|both|all]
    [--extract-blocks none|images|all]
    [--device auto|gpu|cpu]
    [--dpi 192]
    [--pages 1-5,7]
    [--password PASS]
    [--model-dir PATH] [--llama-dir PATH] [--pdfium PATH]
    [--recursive] [--json]
    [--max-pixels N] [--max-tokens N] [--timeout SECONDS]
    [--fast | --image-max-tokens N] [--parallel-pages 1|2]

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

### Форматы вывода и файлы блоков

Имена: `<stem>.md`, `<stem>.html`, `<stem>.json` (только запрошенные форматы);
при коллизии — `-2`, `-3`. `--format both` сохраняет прежнее поведение
(md+html), `all` — md+html+json, `json` — только JSON. Зарезервированные имена
Windows (`CON`, `NUL`, `COM1`, …) получают суффикс `_`. Для каждого документа
пишется `<stem>_meta.json`, для задания — `_meta.json`; параллельные задания в
одном `-o` объединяют документы под файловой блокировкой. Страницы PDF
разделяются `<!-- page N -->` в Markdown и `<section data-page="N">` в HTML.
HTML очищается ammonia, содержит `<meta charset="utf-8">` и `lang="ru"`.

`<stem>.json` повторяет форму официального `_chunks.json` (bbox в пикселях
страницы, label, внутренний HTML) и добавляет заголовок, стабильные id блоков и
пути к файлам:

```json
{
  "schema": "chandra-json/v1",
  "source": "contract_ru.pdf",
  "generator": "ocr-tool",
  "pages": [{
    "page_num": 1,
    "page_box": [0, 0, 2480, 3508],
    "chunks": [{
      "block_id": "p1-01",
      "bbox": [100, 200, 2400, 500],
      "label": "Text",
      "content": "<p>…</p>",
      "text": "…",
      "assets": {"png": "contract_ru_blocks/page-1-01-text.png",
                 "md": "contract_ru_blocks/page-1-01-text.md"}
    }]
  }]
}
```

- Нормализованный `data-bbox` 0–1000 переводится в пиксели; `page_box` —
  `[0, 0, width, height]` JPEG, который видела модель (после DPI рендера PDF или
  уменьшения изображения).
- `content` — очищенный внутренний HTML блока; вложенные `data-bbox`/`data-label`
  удаляются. `text` — plain text (включая `img alt`).
- `block_id` — `p<страница>-<NN>`; его получает каждый top-level блок.
- Если layout-разметки нет, создаётся один блок `block` на всю страницу;
  `Blank-Page` пропускается.
- `assets` перечисляет только реально созданные файлы относительно JSON.
  Невалидный/крошечный bbox оставляет chunk в JSON, но PNG не создаётся.

`--extract-blocks` управляет файлами блоков (по умолчанию `none`):

| режим | файлы |
|---|---|
| `none` | файлов блоков нет; JSON с chunks/bbox/text всё равно пишется при `--format json\|all` |
| `images` | PNG только для `Image`/`Figure` (прежний `--extract-images`) |
| `all` | PNG каждого непустого блока + типизированные файлы ниже |

`--extract-images` сохранён как устаревший alias для `--extract-blocks images`.

Типизированные файлы для `all` пишутся в
`<stem>_blocks/page-<n>-<idx>-<label>.<ext>`:

| label | файлы |
|---|---|
| `Text`, `Section-Header`, `Caption`, `Footnote`, `List-Group`, `Table-Of-Contents`, `Bibliography`, `Page-Header`, `Page-Footer`, `Complex-Block` | `.md`, `.png` |
| `Table` | `.html` (HTML блока), `.csv` (развёрнутые строки), `.png` |
| `Equation-Block` | `.tex` (текст из `<math>`), `.png` |
| `Code-Block` | `.txt`, `.png` |
| `Form` | `.html` (inputs/checkboxes), `.png` |
| `Image`, `Figure`, `Diagram` | `.png`, `.txt` (alt/описание) |
| `Chemical-Block` | `.html`, `.txt` (SMILES/chem), `.png` |
| `Blank-Page` | пропускается |

CSV берёт первую top-level таблицу; `colspan`/`rowspan` разворачиваются пустыми
ячейками, поля экранируются по RFC 4180. Вложенные таблицы уплощаются
best-effort. PNG меньше 8×8 px или с невалидным bbox пропускается. После 2000
файлов на документ извлечение останавливается с предупреждением; JSON остаётся.

При включённом извлечении HTML/Markdown размечаются:

- HTML оборачивает каждый встроенный блок в
  `<section class="ocr-block" id="block-p1-01" data-block-id="p1-01" data-label="Text">`
  и добавляет компактные ссылки `<a class="ocr-asset">PNG</a>`;
  `Image`/`Figure`/`Diagram` получают настоящий `<img src="…png" alt="…">`.
- Markdown добавляет `<!-- block: p1-01 Text -->` и ссылки на assets;
  визуальные блоки — `![alt](…png)`.
- `images` размечает только `Image`/`Figure`; `none` оставляет прежние
  Markdown/HTML без изменений.

Печати, логотипы и подписи извлекаются **только если Chandra разметила их как
`Image`/`Figure`**. Мелкая печать может оказаться `Text`/`Form`, а рукописная
подпись — текстом; гарантии нет. В режиме `all` PNG и типизированный файл
получает любой label, который выбрала модель. Список файлов пишется в
`<stem>_meta.json` (`images`, а также `assets` со всеми файлами).

### Профили скорости

- **Качество (по умолчанию):** 4096 токенов изображения, страницы по одной.
  Лучшая точность таблиц и мелкого текста. На эталоне RTX 3060 CUDA измерено
  **19,65 с** на страницу A4.
- **Быстро:** `--fast` (то же, что `--image-max-tokens 2048`). Prefill картинки
  сокращается с 4,1 до 1,7 с, страница — с **19,65 до 16,53 с (−16%)**; в
  выборочной проверке на счёте ключевые поля сохранились. На других документах
  возможны отличия: на договоре из двух страниц быстрый профиль изменил строку
  с номером/датой приказа, поэтому для юридических и финансовых документов
  по умолчанию остаётся Качество. Ограничение: подписи строк таблиц
  (`БИК`, `Сч. №`) могут сместиться на строку вниз — проверяйте банковские
  реквизиты и таблицы перед автоматизацией. `--image-max-tokens N` принимает
  1–4096 для экспериментов; при одновременном указании побеждает `--fast`.
  Ограничение передаётся sidecar через `LLAMA_ARG_IMAGE_MAX_TOKENS`, поэтому
  работает и со сборками без одноимённого флага.
- **Две страницы одновременно:** `--parallel-pages 2` запускает `llama-server`
  с `-np 2 -c 20000` и распознаёт страницы PDF парами. На том же эталоне
  измерено **13,18 с/страница** при 4096 токенах и **10,22 с/страница** при
  2048 (две страницы за 20,4 с, ~5,1 ГБ VRAM): ускорение пакета в 1,5–1,9 раза.
  Одна страница быстрее не становится — один запрос занимает один слот; порядок
  страниц в результатах сохраняется. На CPU оба запроса делят одни ядра,
  ускорения может не быть. Изображения (не PDF) по-прежнему обрабатываются по
  одному.

### Дополнительные пути

Если runtime и модель уже лежат в другом месте, укажите их явно:

```powershell
$env:OCR_LLAMA_SERVER = "C:\tools\llama\llama-server.exe"
$env:OCR_MODEL_DIR    = "C:\models\chandra-ocr-2"
$env:OCR_PDFIUM       = "C:\tools\pdfium\pdfium.dll"
```

или используйте флаги `--llama-dir`, `--model-dir`, `--pdfium`; в GUI — поля
model dir, llama dir, PDFium в разделе **ДОПОЛНИТЕЛЬНО**. Папка движка в панели
подготовки (и тот же выбор в **ДОПОЛНИТЕЛЬНО**) заполняет каталог llama-server
и библиотеку PDFium из `runtime/<os>-<arch>/{llama-vulkan,llama-cpu,pdfium}`;
GUI использует `llama-vulkan`/`llama-cpu` и `pdfium/bin|lib/<lib>`. В эту же
папку кнопка **СКАЧАТЬ ДВИЖОК** ставит закреплённые архивы. Папки модели и
runtime независимы — задайте обе, если хотите держать всё вместе. **ДОПОЛНИТЕЛЬНО**
разворачивается автоматически, пока setup не завершён (нет модели,
`llama-server` или PDFium), и хранит опциональные переопределения. Полный
порядок поиска — в разделе [Порядок поиска](#порядок-поиска).

### Производительность

Эталон: RTX 3060 12 ГБ, Vulkan b12535, Q4_K_M.

- GPU: примерно **20–50 с на страницу A4** (доминирует обработка image prompt).
  Локально измерено с Vulkan: счёт 2496×3508 занял **25–27 с**; две страницы A4
  договора — **43–48 с** после ~7 с запуска движка.
- CUDA (WSL2, та же карта и модель): **19,65 с/страница** в Качестве,
  **16,53 с/страница** в Быстром профиле, **13,18 с/страница** при двух
  страницах и 4096 токенах и **10,22 с/страница** при Быстро + две страницы.
  Два потока занимали ~5,1 ГБ VRAM. CUDA быстрее Vulkan на 4–14% на тёплых
  заданиях, но не в 2 раза.
- CPU: около **3,3 ток/с генерации**, обычная страница — **10–15 минут**;
  кроп 1248×351 со 96 токенами — **39–45 с** (плюс запуск сервера). Для быстрой
  проверки используйте `--pages`, `--dpi 120` или кроп.
- Загрузка: mmproj на 367 МБ скачался за ~30 с на тестовом подключении и
  продолжился с `.part` на 50 МБ; файл Q4 на 3,07 ГБ идёт тем же путём.
- Пик на диске: ~3,4 ГБ модель + ~250 МБ runtime + результаты.

CPU — аварийный fallback, а не рабочий режим для пакетной обработки на обычных
ноутбуках.

### Ограничения

- Не single-exe: нужны PDFium, llama-server и модель.
- CPU очень медленный; отдельной квантованной оптимизации под CPU нет.
- Текстовый слой PDF игнорируется — все выбранные страницы растрируются. Это
  медленнее извлечения текста, зато одинаково работает со сканами.
- Таблицы сохраняются как HTML-таблицы и конвертируются в Markdown через
  `htmd`; сложные объединённые ячейки могут неточно переноситься в Markdown.
- Изображения до двух `--max-pixels` декодируются и уменьшаются; всё, что
  больше, отсекается до выделения битмапа (PNG 6000×6000 не создаёт 354 МБ
  буфера при малом `--max-pixels`).
- Зашифрованные PDF требуют `--password` (или скрытого поля в GUI); поддержку
  схем шифрования определяет PDFium.
- Качество печатного/рукописного русского зависит от Chandra; печати, рукопись и
  плохие сканы могут содержать ошибки. Всегда проверяйте финансовые и
  юридические результаты.
- `--extract-blocks images` вырезает только блоки, размеченные моделью как
  `Image`/`Figure`; печать или подпись, распознанная как `Text`/`Form`, не
  кропится. `all` вырезает любой label, но PNG — это прямоугольник bbox модели,
  а не семантическая маска.
- Быстрый профиль (2048 токенов изображения) может сдвинуть подписи строк
  таблиц на строку вниз. Это режим скорости, а не гарантия точности таблиц:
  проверяйте банковские реквизиты и суммы.
- `--parallel-pages 2` перекрывает запросы, но не рендеринг PDF, и требует
  около 5 ГБ VRAM. Одна страница быстрее не становится; на CPU ускорения
  может не быть. Изображения (не PDF) по-прежнему обрабатываются по одному.
- Linux/macOS проверяются в CI-матрице, а не на локальном железе в этой версии;
  локально проверен только Windows 11 x64.
- GUI и CLI сами скачивают и модель, и runtime (llama-server + PDFium);
  дистрибутив дополнительно содержит `runtime/` рядом с exe. Скрытых скриптов
  установки нет.
- Лицензия весов OpenRAIL-M; коммерческое использование не гарантируется. См.
  [NOTICE](NOTICE).
- Публичный репозиторий не содержит тестовых документов. `samples/SOURCES.md`
  описывает только происхождение локальных фикстур; сами файлы содержат
  персональные данные и не публикуются (вся папка, кроме `SOURCES.md`,
  исключена из git).

<p align="right">(<a href="#readme-top">наверх</a>)</p>

<!-- КАК ЭТО РАБОТАЕТ -->
## Как это работает

```
input ─┬─ image ──────────────► JPEG (downscale to --max-pixels)
       └─ PDF → PDFium (DPI) ──► JPEG per selected page
                                     │
                         llama-server /v1/chat/completions
                         official Chandra ocr_layout prompt
                         temperature=0, max_tokens=4096
                                     │
                     HTML blocks ──► ammonia ──► .html
                              ├────► htmd ─────► .md
                              └────► layout ───► .json (+ _blocks/*.png|md|…)
```

- Один `llama-server` на задание, переиспользуется для всех страниц. Он
  стартует на свободном localhost-порту с уникальным `--alias`, проходит
  health-check до 180 с и убивается через `taskkill /T /F` и Windows Job Object
  с `KILL_ON_JOB_CLOSE` (Unix: группа процессов и `PR_SET_PDEATHSIG`) при
  Drop/отмене/падении, поэтому sidecar не остаётся после `taskkill /F` CLI или
  закрытия GUI. Перед принятием сервера `/props` или `/v1/models`
  подтверждают alias или путь модели, поэтому процесс, выигравший гонку за
  свободный порт, обнаруживается. Sidecar всегда получает `-np 1` или `-np 2`;
  для двух слотов используется `-c 20000`, а лимит токенов изображения
  передаётся через `LLAMA_ARG_IMAGE_MAX_TOKENS`.
- Sidecar — отдельный скрытый процесс под управлением GUI (in-process inference
  вне рамок). На Windows дочерние процессы (`llama-server`, `taskkill`,
  `llama-server --version`) запускаются с `CREATE_NO_WINDOW`, чтобы у GUI без
  консоли не мигало ни одно окно.
- `--device auto`: сначала GPU-сборка (Vulkan/Metal), при ошибке — CPU; если
  `--llama-dir` указывает на родителя `llama-vulkan` и `llama-cpu`,
  перечисляются обе сборки.
- Sidecar получает `--reasoning off`; если сборка всё же положила ответ в
  `message.reasoning_content`, клиент читает его.
- Таймаут запроса — 600 с на страницу по умолчанию (`--timeout`).
- `doctor` печатает версию `llama-server --version` (таймаут 5 с) для каждого
  кандидата и FileVersion библиотеки PDFium.
- Логи sidecar лежат в `%TEMP%/ocr-tool`; пустые удаляются, хранятся 20
  последних.

### Закреплённые компоненты

| Компонент | Закреплённая версия |
|---|---|
| llama.cpp | b12535 |
| PDFium (pdfium-binaries) | chromium/8165 (PDFium 160.0.8165.0) |
| Модель | `mradermacher/chandra-ocr-2-GGUF` Q4_K_M 3,07 ГБ + mmproj Q8_0 367 МБ |
| Rust | 1.82+ (проверено на 1.99) |

### Порядок поиска

`llama-server`:

1. `OCR_LLAMA_SERVER` (файл или каталог)
2. `--llama-dir` (файл, каталог или родитель `llama-vulkan`/`llama-cpu`)
3. `<exe-dir>/runtime/<os>-<arch>/llama-vulkan`, `llama-metal`, `llama-cuda`, `llama-cpu`
4. то же в текущем каталоге
5. `PATH`

Каталог модели:

1. `--model-dir`
2. `OCR_MODEL_DIR`
3. app-data: `%LOCALAPPDATA%\ocr-tool\models\chandra-ocr-2` (Windows),
   `~/.local/share/ocr-tool/models/chandra-ocr-2` (Linux),
   `~/Library/Application Support/ocr-tool/models/chandra-ocr-2` (macOS)

PDFium:

1. `--pdfium` / папка runtime из GUI (файл библиотеки или каталог)
2. `OCR_PDFIUM` (файл библиотеки или каталог)
3. рядом с exe
4. `runtime/<os>-<arch>/pdfium/{bin,lib}`, `runtime/<os>/...`
5. текущий каталог

Если `OCR_PDFIUM` задан неверно, приложение сообщает об ошибке, а не загружает
молча другую библиотеку. Отсутствие PDFium — обычная ошибка, не паника. Статус
runtime в GUI и `doctor` показывают, какой источник сработал (папка runtime/путь,
`OCR_PDFIUM` или автопоиск); GUI предупреждает, если `OCR_PDFIUM` перекрывает
встроенный runtime.

### Приватность и сеть

Распознавание полностью локальное. Сеть используется только для явных загрузок
модели (кнопка GUI **СКАЧАТЬ МОДЕЛЬ** или `models download`, Hugging Face) и
runtime (кнопка GUI **СКАЧАТЬ ДВИЖОК** или `runtime download`, GitHub), а также
упаковочных `fetch-runtime`. Телеметрии нет. Выходные файлы могут содержать
конфиденциальный текст — защищайте их соответственно.

<p align="right">(<a href="#readme-top">наверх</a>)</p>

<!-- ПЛАНЫ -->
## Планы

- [x] 0.1.0 — офлайн-конвейер CLI + GUI, вывод MD/HTML/JSON, файлы блоков,
  профили Качество/Быстро, две страницы одновременно, предзагрузка модели.
- [x] CI-матрица тестов и сборки на Windows, Ubuntu и macOS; релизные архивы с
  закреплённым runtime.
- [ ] Укрепление release workflow: воспроизводимые закреплённые загрузки и
  хранение артефактов.
- [x] Опциональный пайплайн подписи Authenticode/Developer ID.
- [ ] Включить подпись для публичных релизов (сертификаты + секреты репозитория).
- [ ] Установщики (MSI/NSIS в Windows, `.dmg` в macOS).
- [ ] Универсальный бинарник для macOS.
- [ ] Дополнительные языки интерфейса и документация.
- [ ] Публичные бенчмарки на более широком наборе железа.

Предложения и известные проблемы ведутся в
[GitHub Issues](https://github.com/onive/ocr-tool/issues).

<p align="right">(<a href="#readme-top">наверх</a>)</p>

<!-- УЧАСТИЕ -->
## Участие

Мы рады любому вкладу: сообщениям об ошибках, исправлениям, документации и
идеям. Любая помощь **очень ценится**.

1. Сделайте fork проекта.
2. Создайте ветку (`git checkout -b feature/AmazingFeature`).
3. Закоммитьте изменения (`git commit -m 'Add some AmazingFeature'`).
4. Запушьте ветку (`git push origin feature/AmazingFeature`).
5. Откройте pull request.

Перед PR прочитайте [CONTRIBUTING.md](CONTRIBUTING.md): там требования к
окружению, пакеты платформ, стиль кода, проверки и правила PR. Об уязвимостях
сообщайте приватно, как описано в [SECURITY.md](SECURITY.md). История версий —
в [CHANGELOG.md](CHANGELOG.md). Проект новый, стены контрибьюторов пока нет —
каждый PR виден в истории репозитория.

### Проверки при разработке

```bash
cargo fmt --all -- --check
cargo test --workspace   # 216 тестов
cargo build --release
```

Упаковка (модель не входит):

```powershell
.\scripts\package.ps1
```

```bash
./scripts/package.sh
```

`scripts/fetch-runtime.ps1|.sh` скачивают закреплённые llama.cpp и PDFium в
`runtime/<target>/` для упаковки; пользователям GUI/CLI они не нужны
(**СКАЧАТЬ ДВИЖОК** / `runtime download` ставят тот же layout).
`scripts/package.ps1|.sh` складывают exe, `runtime/<target>`,
`README.md`/`README.ru.md`, `LICENSE`, `CHANGELOG.md` и `NOTICE` в `dist/`.
GitHub Actions прогоняет тесты и сборку на Windows/Ubuntu/macOS и готовит
релизные архивы. Пайплайн подписи опционален ([docs/signing.md](docs/signing.md));
текущие assets без подписи, поэтому Windows SmartScreen может предупредить при
первом запуске.

`scripts/acceptance.ps1` — локальный acceptance-тест, использованный при
разработке. Он ожидает `.tmp/models`, `.tmp/llama`, `.tmp/llama-cpu` и
`.tmp/pdfium`, а также sample-файлы; проверяет распознанные подстроки, страницы
PDF, CPU fallback, `doctor`, запуск GUI, PE subsystem обоих exe
(`scripts/check-gui-subsystem.ps1`: GUI=2 Windows GUI, CLI=3 console), файловый
лог GUI и офлайн-команды `runtime --help` / `runtime verify`.
`scripts/acceptance-runtime.ps1` выполняет реальный E2E загрузки runtime
(~69 МБ): SHA-256 архивов, `llama-server --version`, `doctor`, CPU OCR.

<p align="right">(<a href="#readme-top">наверх</a>)</p>

<!-- ЛИЦЕНЗИЯ -->
## Лицензия

Код на Rust — **MIT** (см. [LICENSE](LICENSE)). Встроенные шрифты — под SIL
Open Font License (`assets/fonts/OFL*.txt`). Веса модели Chandra OCR 2 —
**OpenRAIL-M**, llama.cpp и PDFium сохраняют свои лицензии; скачиваемые
компоненты не входят в лицензию исходников этого репозитория. Подробности — в
[NOTICE](NOTICE).

<p align="right">(<a href="#readme-top">наверх</a>)</p>

<!-- КОНТАКТЫ -->
## Контакты

Для багов и предложений — [GitHub Issues](https://github.com/onive/ocr-tool/issues).
По остальным вопросам открывайте discussion или пишите мейнтейнеру через
профиль [@OniVe](https://github.com/OniVe).

Ссылка на проект: [https://github.com/onive/ocr-tool](https://github.com/onive/ocr-tool)

<p align="right">(<a href="#readme-top">наверх</a>)</p>

<!-- БЛАГОДАРНОСТИ -->
## Благодарности

- [Chandra OCR 2](https://github.com/datalab-to/chandra) — модель OCR и
  layout-промпт.
- [llama.cpp](https://github.com/ggml-org/llama.cpp) — закреплённый движок
  инференса `llama-server`.
- [PDFium](https://pdfium.googlesource.com/pdfium/) и
  [pdfium-binaries](https://github.com/bblanchon/pdfium-binaries) — рендеринг
  PDF.
- [egui/eframe](https://github.com/emilk/egui) — immediate-mode GUI.
- [Press Start 2P](https://fonts.google.com/specimen/Press+Start+2P) и
  [IBM Plex Mono](https://github.com/IBM/plex) — пиксельный и моноширинный
  шрифты.
- [shields.io](https://shields.io) — бейджи README.
- [Best-README-Template](https://github.com/othneildrew/Best-README-Template) —
  структура этого документа.

<p align="right">(<a href="#readme-top">наверх</a>)</p>

<!-- ССЫЛКИ И ИЗОБРАЖЕНИЯ -->
[ci-shield]: https://img.shields.io/github/actions/workflow/status/onive/ocr-tool/ci.yml?branch=master&style=for-the-badge
[ci-url]: https://github.com/onive/ocr-tool/actions/workflows/ci.yml
[release-shield]: https://img.shields.io/github/v/release/onive/ocr-tool?style=for-the-badge&label=release
[release-url]: https://github.com/onive/ocr-tool/releases
[license-shield]: https://img.shields.io/badge/license-MIT-blue?style=for-the-badge
[license-url]: LICENSE
[rust-shield]: https://img.shields.io/badge/rust-1.82%2B-orange?style=for-the-badge&logo=rust&logoColor=white
[rust-url]: https://www.rust-lang.org/
[platform-shield]: https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-lightgrey?style=for-the-badge
[platform-url]: https://github.com/onive/ocr-tool/releases
[offline-shield]: https://img.shields.io/badge/offline-100%25%20local-brightgreen?style=for-the-badge
[offline-url]: #как-это-работает
[rust-stack-shield]: https://img.shields.io/badge/Rust-1.82%2B-000000?style=for-the-badge&logo=rust&logoColor=white
[rust-stack-url]: https://www.rust-lang.org/
[egui-shield]: https://img.shields.io/badge/egui%2Feframe-0.31-FF5E00?style=for-the-badge
[egui-url]: https://github.com/emilk/egui
[llama-shield]: https://img.shields.io/badge/llama.cpp-b12535-2C2C2C?style=for-the-badge
[llama-url]: https://github.com/ggml-org/llama.cpp
[pdfium-shield]: https://img.shields.io/badge/PDFium-8165-6A5ACD?style=for-the-badge
[pdfium-url]: https://github.com/bblanchon/pdfium-binaries
[chandra-shield]: https://img.shields.io/badge/Chandra%20OCR-2-0A7E8C?style=for-the-badge
[chandra-url]: https://github.com/datalab-to/chandra
[hero-screenshot]: docs/screenshots/pixel-ui/02-ready.png
[gui-done]: docs/screenshots/pixel-ui/04-done.png
