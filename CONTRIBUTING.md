# Contributing

Thanks for improving ocr-tool. Bug reports, documentation fixes and focused
pull requests are welcome.

## Prerequisites

- **Rust 1.82+** (stable). The workspace builds with Cargo; no nightly features
  are required.
- **Windows:** Rust MSVC toolchain plus the Visual Studio Build Tools with the
  “Desktop development with C++” workload. The GUI uses eframe/egui and links as
  a Windows-subsystem executable.
- **Linux:** the same packages the CI installs:

  ```bash
  sudo apt-get install -y libgtk-3-dev libxkbcommon-x11-dev libwayland-dev libx11-dev
  ```

- **macOS:** Xcode Command Line Tools.

Model weights and the llama.cpp/PDFium runtime are **not** required to build or
run the unit tests. They are only needed for a real OCR run; the GUI and
`runtime download` / `models download` fetch the pinned versions.

## Checks before a pull request

Run from the repository root:

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo build --release
```

`cargo test --workspace` is the expected baseline (currently 200 tests). If your
change affects the GUI, include a screenshot or a short description of the
visible result. Do not run the GUI or OCR in CI just to prove a docs change.

## What not to commit

- Anything under `samples/` except `samples/SOURCES.md` — the fixtures contain
  personal data and are deliberately gitignored.
- Model weights (`*.gguf`), runtime binaries/libraries, downloaded archives,
  `.part` files, `runtime/`, `dist/`, `target/`, local output folders.
- Secrets, tokens, passwords, private paths or documents with PII.
- Internal planning notes, audit dumps or generated reports (`docs/planning/`,
  `reports/`, `audit-*`, `handoff-*`, `decisions-*`).

If a commit accidentally includes any of the above, remove it from the branch
history before opening the PR — a follow-up deletion commit is not enough for
leaked personal data.

## Style and pull requests

- Keep changes focused; separate refactors from behaviour changes.
- Follow the existing structure, naming and rustfmt style of the surrounding
  code. Prefer small, testable functions and keep user-visible strings Russian
  in the GUI.
- Add or update tests for behaviour changes and keep existing tests green.
- Update `README.md` / `README.ru.md` together when user-facing behaviour or
  commands change, and add a `CHANGELOG.md` entry for notable changes.
- Describe the problem, the approach and any trade-offs in the PR. Link the
  issue when one exists.
- By contributing you agree that your contribution is licensed under the
  project's MIT license (see [LICENSE](LICENSE)).
