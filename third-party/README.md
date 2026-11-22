# Third-party components

Short map of the third-party licences that matter for the packaged release.
The authoritative, longer notices live in the root [`NOTICE`](../NOTICE).

## llama.cpp

- Source: <https://github.com/ggml-org/llama.cpp>
- Pinned build: **b12535**
- License: **MIT**
- Full text: [`llama.cpp-LICENSE`](llama.cpp-LICENSE)
- Role: the `llama-server` sidecar that runs the OCR model locally.

## PDFium

- Source: <https://pdfium.googlesource.com/pdfium/> (binaries from
  <https://github.com/bblanchon/pdfium-binaries>)
- Pinned build: **chromium/8165** (PDFium 160.0.8165.0)
- License: **BSD-3-Clause**
- Full text: ships with the binaries at `runtime/<target>/pdfium/LICENSE`.
- Role: PDF page rendering.

## Chandra OCR 2 model weights

- Model card: <https://huggingface.co/datalab-to/chandra-ocr-2>
- License: **OpenRAIL-M**
- The weights are **not** included in this package; `ocr-tool models download`
  fetches them on demand.
- Use restrictions and disclaimers: see the root [`NOTICE`](../NOTICE).
