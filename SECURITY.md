# Security policy

## Supported versions

Security fixes are provided for the **0.1.x** release line. Older builds and
locally modified copies are not supported.

## Reporting a vulnerability

Please report suspected vulnerabilities **privately** through GitHub Security
Advisories:

<https://github.com/onive/ocr-tool/security/advisories/new>

Do **not** open a public issue, discussion or pull request for an unfixed
vulnerability. Please include:

- affected version (for example `0.1.0`) and platform;
- a minimal reproduction or a clear description of the attack path;
- the expected and observed behaviour, and the impact you see;
- relevant logs with paths, names and document contents redacted.

Never attach real documents, model weights, passwords or other personal data.
Use a synthetic or redacted sample.

## Scope

In scope: the Rust code in this repository — the CLI, GUI, setup and download
logic, PDF/image handling wrappers, output writing, and the release packaging
scripts.

Out of scope: vulnerabilities in third-party components (llama.cpp,
`llama-server`, PDFium, the Chandra OCR 2 model or its GGUF quantization, and
Rust dependencies). Report those to their upstream projects. Model behaviour and
OCR accuracy are not security issues.

## What to expect

This is a volunteer project with no SLA, bounty or guaranteed response time.
Reports are reviewed on a best-effort basis; valid issues are fixed in the
0.1.x line and credited in the release notes unless you ask to stay anonymous.
