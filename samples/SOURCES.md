# Russian OCR test samples

Collected 2026-11-19 for local OCR testing. Public URLs, no login/paywall. Total 11.7 MB.
Two source documents; invoice/act pages were extracted from a public 22-page charity report.

| Local file | Source URL | Format | Size | Pages / dimensions | Text layer |
|---|---|---|---|---|---|
| `contract_ru.pdf` | https://www.tyumsmu.ru/upload/iblock/504/453_OT_1.PDF | PDF 1.3 | 7.71 MB | 11 pp, A4 (595x842 pt); page images 2480x3508 | Yes, Russian text layer (55,302 chars via pdftotext) |
| `invoice_ru.pdf` | https://dobro.mail.ru/media/m/files/reports/6a/d2/30/-7958165347299578590.pdf (p.1 of 22) | PDF 1.7 | 1.25 MB | 1 p, A4; scan 2496x3508 | No (0 chars; 1 image) |
| `act_ru.pdf` | same report (p.8 of 22) | PDF 1.7 | 1.47 MB | 1 p, A4; scan 2807x3892 | No (0 chars; 1 image) |
| `invoice_scan_ru.jpg` | same report (p.1, embedded JPEG) | JPEG | 1.25 MB | 2496x3508 px | n/a (raster) |

## Sources, licensing, privacy

- **contract_ru.pdf** — official blank form "Договор возмездного оказания консультационных услуг" with the approving order, published by ФГБОУ ВО «Тюменский ГМУ». No explicit license is stated; public web document, keep attribution, local test use.
- **invoice/act/JPG** — public charity report of Некоммерческий БФ «Дедморозим» on dobro.mail.ru. No explicit license. The pages contain real company details, signatures, stamps and one recipient's name; treat as personal data: local testing only, do not redistribute.
- `invoice_scan_ru.jpg` is the original embedded JPEG from `invoice_ru.pdf`; SHA-256 matches the PDF image exactly.

## Verification

- `file` → `PDF 1.3` / `PDF 1.7, 1 page(s)` / `JPEG ... 2496x3508`; magic bytes: `%PDF-1.3`, `%PDF-1.7`, `%PDF-1.7`, `FFD8FFE0`.
- `pdftotext -enc UTF-8 <file> - | wc -c` → contract `55302`; invoice `0`; act `0` (only a form feed). Invoice/act are image-only scans.
- PyMuPDF: contract 11 pages; invoice 1 page/1 image; act 1 page/1 image; JPG decodes as RGB 2496x3508.
- SHA-256: `1a88c440…2314de` (contract), `ae23e628…54be4` (invoice), `f5172c84…8f263` (act), `4b377034…5bbc3` (JPG).

## Privacy warning

These files are **local-only test fixtures** and contain real personal and company data
(names, addresses, bank accounts, signatures, stamps). Do not commit, publish, upload,
attach to issues, or include them in release artifacts. The repository `.gitignore`
excludes everything in `samples/` except this file. Use a synthetic or redacted sample
for CI and public demonstrations. Delete local copies when they are no longer needed.
