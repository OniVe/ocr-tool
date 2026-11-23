## [0.1.0](https://github.com/OniVe/ocr-tool/releases/tag/v0.1.0)

Первый публичный релиз: локальное OCR для PDF и изображений на Chandra OCR 2 — CLI и GUI, офлайн, EN/RU/ZH.

**Что нового**

- **PDF и изображения**: PDF через PDFium и PNG/JPEG/TIFF/WebP/BMP; диапазон страниц, DPI, пароль PDF, рекурсивный обход папок
- **Форматы вывода**: Markdown, standalone HTML и layout JSON; опциональные ассеты по блокам — PNG-кропы и HTML/CSV/TEX/TXT
- **Скорость**: профили Quality (4096 image tokens) и Fast (2048), две страницы PDF распознаются параллельно
- **Модель в памяти**: preload включён по умолчанию — модель остаётся загруженной между задачами
- **Интерфейс**: пиксельный UI с кастомным заголовком, панель первичной настройки и сохранение предпочтений
- **Языки**: EN/RU/ZH для интерфейса и распознавания, включая русские документы
- **Офлайн**: runtime (llama.cpp) и PDFium уже в архиве, модель скачивается один раз с проверкой SHA-256

**Установка**

- Windows: скачать `ocr-tool-0.1.0-windows-x64.zip`, распаковать, запустить `ocr-tool-gui.exe`.
- Linux/macOS: скачать `.tar.gz`, распаковать, запустить `ocr-tool-gui`.
- Модель скачивается при первом запуске (~3.4 GB); runtime/PDFium уже в архиве.
- Контрольные суммы: `certutil -hashfile <файл> SHA256` (Windows) или `sha256sum -c <файл>.sha256` / `sha256sum -c SHA256SUMS` (Linux/macOS).
- Аттестации: `gh attestation verify <файл> -R onive/ocr-tool` — для релизов, собранных обновлённым workflow; у `v0.1.0` аттестаций пока нет, доступны checksums.
- Архивы **unsigned** (Authenticode/Developer ID не настроены); SHA256/attestation не убирают SmartScreen/Gatekeeper.

Артефакты — три платформы: 3 архива, общий `SHA256SUMS` и `.sha256` sidecar для каждого архива;
у `v0.1.0` sidecars и `SHA256SUMS` добавлены вручную, аттестаций нет.

Подробности — в [CHANGELOG.md](https://github.com/OniVe/ocr-tool/blob/master/CHANGELOG.md).
