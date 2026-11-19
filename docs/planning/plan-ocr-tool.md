# План: ocr-tool (Rust UI/CLI, Chandra OCR 2)

Дата: 2026-11-19. Владелец: пользователь. Рабочий каталог: `E:\projects\ai\ocr-tool`.

## Цель

Кроссплатформенное приложение (Windows/Linux/macOS) с GUI и CLI на Rust: распознаёт PDF и популярные изображения (PNG/JPEG/TIFF/WebP/BMP) с русским текстом, выдаёт `.md` и `.html`. Движок — Chandra OCR 2 (веса `datalab-to/chandra-ocr-2`), локальный инференс через llama.cpp; GPU (Vulkan/CUDA/Metal) с откатом на CPU.

## Проверенные факты (эмпирика 2026-11-19)

- `llama.cpp b12535` (Win x64 Vulkan) + `mradermacher/chandra-ocr-2-GGUF` Q4_K_M + mmproj-Q8_0 **работает**: страница счёта (2496×3508) распознана за ~20 с на RTX 3060, русский текст и таблица корректны.
- CPU-сборка `llama-mtmd-cli` b12535 работает: ~3.3 ток/с, страница ~10–15 мин; поддержка CPU обязательна как fallback.
- Сервер: `llama-server --mmproj ... --reasoning off`; OpenAI-совместимый `POST /v1/chat/completions` с base64 image, ответ в `message.content`. Без `--reasoning off` HTML уходит в `reasoning_content`.
- Модель: 5.3B, BF16 10.6 ГБ; GGUF Q4_K_M 3.2 ГБ, mmproj 0.5 ГБ. Не gated. Лицензия весов — OpenRAIL-M (не Apache): нужен NOTICE, веса не бандлить.
- Официальный промпт `ocr_layout` — https://github.com/datalab-to/chandra/blob/master/chandra/prompts.py
- PDFium binaries: `bblanchon/pdfium-binaries` chromium/8165.
- Образцы: `samples/` (4 файла, 11.7 МБ), реальные документы; в них есть персональные данные — локально, не публиковать.

## Архитектура

Cargo workspace:
- `crates/ocr-core` — типы заданий, события прогресса, промпт, HTML→Markdown, санитайз, имена выходных файлов.
- `crates/ocr-pdf` — рендер PDF через `pdfium-render` (pdfium.dll/.so/.dylib рядом с exe), DPI 192 по умолчанию, лимит пикселей, диапазон страниц, пароль.
- `crates/ocr-engine` — менеджер sidecar `llama-server`: поиск бинарника (GPU→CPU), свободный порт, health-check, запрос, таймауты, kill process tree, парсинг `content`/`reasoning_content`.
- `crates/ocr-cli` — бинарь `ocr-tool` (clap): `ocr`, `models`, `doctor`, `gui`.
- `crates/ocr-gui` — бинарь `ocr-tool-gui` (eframe/egui + rfd): выбор файлов, формат, устройство, прогресс, лог, открыть результат.
- Общая библиотека `ocr-app` для job-runner, конфиг, загрузка модели.

Поток: вход → (PDF: страницы в PNG; image: как есть) → постранично `llama-server` → HTML-блоки → склейка → `.md` (htmd) + `.html` (ammonia + обёртка UTF-8).

## Runtime и модель

- Поиск `llama-server`/`llama-mtmd-cli`: `OCR_LLAMA_SERVER`, `--llama-dir`, `<exe_dir>/runtime/<os-arch>/`, PATH.
- Модель: `OCR_MODEL_DIR`, `--model-dir`, иначе `%LOCALAPPDATA%/ocr-tool/models/chandra-ocr-2/` (Linux `~/.local/share`, macOS `~/Library/Application Support`).
- `ocr-tool models download` — скачивание Q4_K_M + mmproj с HF, SHA-256 (Q4 `29ec72bf…d3efb`, mmproj `709be8ae…48bc3`), прогресс, докачка, отказ при несовпадении.
- `ocr-tool doctor` — проверяет exe, модель, pdfium, GPU/CPU, печатает версии.
- GPU: Windows/Linux — Vulkan-сборка, при ошибке — CPU; macOS — Metal-сборка. Флаг `--device auto|gpu|cpu`.
- `--reasoning off` обязателен; fallback на `reasoning_content`.

## CLI (минимум)

`ocr-tool ocr <inputs...> -o <dir> [--format md|html|both] [--device auto|gpu|cpu] [--dpi 192] [--pages 1-5,7] [--model-dir PATH] [--llama-dir PATH] [--json]`
- несколько файлов и каталогов, рекурсия по флагу;
- прогресс в stderr, итог в stdout; коды выхода 0/2/3;
- при ошибке одной страницы — продолжить, отметить в отчёте.

## GUI (минимум)

Один экран: добавить файлы/папку, список, выходная папка, формат, устройство, DPI, кнопка «Распознать», прогресс-бар по страницам, лог, «Открыть папку». Работа в фоновом потоке, отмена. Без сети, кроме загрузки модели.

## Кроссплатформенность и упаковка

- `scripts/fetch-runtime.ps1` и `.sh`: скачивают pinned llama.cpp b12535 (win-vulkan, win-cpu, ubuntu-vulkan, ubuntu-cpu, macos-arm64) и pdfium chromium/8165 (win-x64, linux-x64, mac-univ); складывают в `runtime/<os>-<arch>/`.
- `scripts/package.ps1`/`.sh`: собирают zip: exe(ы), runtime, pdfium, README, NOTICE; модель не входит.
- `.github/workflows/release.yml`: матрица windows-latest/ubuntu-22.04/macos-14, build, package, upload artifact; без подписи (отметить в README).
- Честно указать: «single exe» = exe + pdfium + llama-server + модель (скачивается). Статическая сборка PDFium не требуется.

## Тесты и приёмка

1. `cargo test` — unit: промпт, HTML→MD, имена файлов, парсер ответа, конфиг.
2. `cargo build --release` — без ошибок.
3. CLI на `samples/invoice_scan_ru.jpg` → `.md`+`.html`, проверить подстроки: `Внимание`, `Счет № 99`, `Дедморозим`, `43,200.00`; HTML — UTF-8, `lang="ru"`.
4. CLI на `samples/contract_ru.pdf` (2 страницы) → md/html; проверить русский текст.
5. CPU: `--device cpu` на кропе/странице с малым лимитом токенов — процесс завершается, текст непустой (медленно, зафиксировать время).
6. GUI: процесс `ocr-tool-gui.exe` стартует и живёт ≥3 с; smoke-флаг, если нужен.
7. `ocr-tool doctor` — отчёт о компонентах.

## Ограничения

- Не публиковать, не пушить, не создавать релиз без подтверждения владельца.
- Не бандлить веса Chandra (OpenRAIL-M), не обещать коммерческое использование.
- Не включать персональные данные из `samples/` в публикуемые артефакты; для CI — синтетический образец или скачивание обезличенного.
- Один писатель в рабочем каталоге; root код не правит.
- После сборки — аудиторский цикл до ready (факт-чек → эмпирик ∥ инвариант → арбитр → правки → re-audit).
