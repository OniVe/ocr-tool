# Fonts

The four font families below are licensed under the SIL Open Font License 1.1
and are embedded into the GUI binary at build time. Ark Pixel is a fallback for
CJK glyphs only; Latin text keeps Press Start 2P and IBM Plex Mono metrics.

| File | Family | Source |
|---|---|---|
| `PressStart2P-Regular.ttf` | Press Start 2P | https://github.com/google/fonts/tree/main/ofl/pressstart2p |
| `IBMPlexMono-Regular.ttf` | IBM Plex Mono | https://github.com/google/fonts/tree/main/ofl/ibmplexmono |
| `IBMPlexMono-Bold.ttf` | IBM Plex Mono Bold | https://github.com/google/fonts/tree/main/ofl/ibmplexmono |
| `ArkPixel-12px-zh_hans.ttf` | Ark Pixel 12px monospaced (zh_hans), version 2026.11.10 | https://github.com/TakWolf/ark-pixel-font/releases/tag/2026.11.10 |
| `OFL.txt` | Press Start 2P license | SIL OFL 1.1 |
| `OFL-IBMPlexMono.txt` | IBM Plex Mono license | SIL OFL 1.1 |
| `OFL-ArkPixel.txt` | Ark Pixel Font license | SIL OFL 1.1 |

Press Start 2P: Copyright 2012 The Press Start 2P Project Authors
(cody@zone38.net), with Reserved Font Name "Press Start 2P".

IBM Plex Mono: Copyright © 2017 IBM Corp. with Reserved Font Name "Plex".

Ark Pixel Font: Copyright (c) 2021, TakWolf (https://takwolf.com). The bundled
`ArkPixel-12px-zh_hans.ttf` is the `ark-pixel-12px-monospaced-zh_hans.ttf` file
from `ark-pixel-font-12px-monospaced-ttf-v2026.11.10.zip`.

Cyrillic coverage was checked against U+0410–U+044F, U+0401/U+0451, №, —, ·, ×
and … in both Latin families. Chinese UI strings (including `中文`, `文档`,
`识别`, `保存`, `结果`) are covered by the Ark Pixel fallback.
