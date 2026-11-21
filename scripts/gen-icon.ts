#!/usr/bin/env bun
/**
 * Regenerate the pixel-art application icon (assets/icon-256.png,
 * assets/ocr-tool.ico, assets/ocr-tool.svg) and the review previews under
 * reports/icon-pixel/.
 *
 * The artwork lives on a 16x16 grid and uses only the Pixel Poetry palette
 * (cream, ink). Every raster size is a nearest-neighbour resample of that
 * grid, so the assets stay crisp and reproducible.
 *
 * Deterministic and dependency-free: `bun scripts/gen-icon.ts`.
 */
import { deflateSync } from "node:zlib";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

const ROOT = join(import.meta.dir, "..");
const ASSETS = join(ROOT, "assets");
const REPORTS = join(ROOT, "reports", "icon-pixel");

const GRID = 16;
const SVG_SIZE = 1024;

type Rgba = readonly [number, number, number, number];

const CREAM: Rgba = [0xf2, 0xef, 0xe7, 0xff];
const INK: Rgba = [0x11, 0x11, 0x11, 0xff];

/** Preview-only background colours; never used by the icon itself. */
const PREVIEW_LIGHT: Rgba = [0xff, 0xff, 0xff, 0xff];
const PREVIEW_DARK: Rgba = [0x1e, 0x1e, 0x1e, 0xff];

/**
 * `#` is ink, `.` is cream.
 *
 * A solid document with a two-step cut in its top-right corner (x4..11,
 * y3..11), three cream text rows, and a scan beam at y13 separated from the
 * sheet by a cream row.
 */
const MASTER: readonly string[] = [
  "################",
  "#..............#",
  "#..............#",
  "#...######.....#",
  "#...#######....#",
  "#...##....##...#",
  "#...########...#",
  "#...##.....#...#",
  "#...########...#",
  "#...##...###...#",
  "#...########...#",
  "#...########...#",
  "#..............#",
  "#.############.#",
  "#..............#",
  "################",
];

interface Bitmap {
  width: number;
  height: number;
  data: Buffer;
}

function validate(): void {
  if (MASTER.length !== GRID) {
    throw new Error(`icon grid: expected ${GRID} rows, got ${MASTER.length}`);
  }
  MASTER.forEach((row, y) => {
    if (row.length !== GRID) {
      throw new Error(`icon grid: row ${y} has ${row.length} cells, expected ${GRID}`);
    }
    for (const cell of row) {
      if (cell !== "#" && cell !== ".") {
        throw new Error(`icon grid: row ${y} contains ${JSON.stringify(cell)}`);
      }
    }
  });
}

/** Nearest-neighbour resample of the 16x16 grid to any square size. */
function render(size: number): Bitmap {
  validate();
  const data = Buffer.alloc(size * size * 4);
  for (let y = 0; y < size; y++) {
    const srcY = Math.min(GRID - 1, Math.floor(((y + 0.5) * GRID) / size));
    for (let x = 0; x < size; x++) {
      const srcX = Math.min(GRID - 1, Math.floor(((x + 0.5) * GRID) / size));
      const colour = MASTER[srcY][srcX] === "#" ? INK : CREAM;
      const at = (y * size + x) * 4;
      data[at] = colour[0];
      data[at + 1] = colour[1];
      data[at + 2] = colour[2];
      data[at + 3] = colour[3];
    }
  }
  return { width: size, height: size, data };
}

// --- PNG -------------------------------------------------------------------

const CRC_TABLE = (() => {
  const table = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) {
      c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    }
    table[n] = c >>> 0;
  }
  return table;
})();

function crc32(data: Buffer): number {
  let crc = 0xffffffff;
  for (const byte of data) {
    crc = CRC_TABLE[(crc ^ byte) & 0xff] ^ (crc >>> 8);
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function pngChunk(type: string, payload: Buffer): Buffer {
  const length = Buffer.alloc(4);
  length.writeUInt32BE(payload.length, 0);
  const body = Buffer.concat([Buffer.from(type, "ascii"), payload]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body), 0);
  return Buffer.concat([length, body, crc]);
}

function encodePng(bitmap: Bitmap): Buffer {
  const { width, height, data } = bitmap;
  const stride = width * 4;
  const raw = Buffer.alloc((stride + 1) * height);
  for (let y = 0; y < height; y++) {
    raw[y * (stride + 1)] = 0; // no filter
    data.copy(raw, y * (stride + 1) + 1, y * stride, (y + 1) * stride);
  }

  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // truecolour + alpha
  ihdr[10] = 0; // deflate
  ihdr[11] = 0; // adaptive filtering
  ihdr[12] = 0; // no interlace

  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    pngChunk("IHDR", ihdr),
    pngChunk("IDAT", deflateSync(raw, { level: 9 })),
    pngChunk("IEND", Buffer.alloc(0)),
  ]);
}

// --- ICO -------------------------------------------------------------------

const ICO_SIZES = [16, 24, 32, 48, 64, 128, 256] as const;

/** Classic 32bpp BMP DIB: BITMAPINFOHEADER + bottom-up BGRA + AND mask. */
function encodeDib(bitmap: Bitmap): Buffer {
  const { width, height, data } = bitmap;
  const xorSize = width * height * 4;
  const maskStride = Math.ceil(width / 32) * 4;
  const maskSize = maskStride * height;

  const header = Buffer.alloc(40);
  header.writeUInt32LE(40, 0);
  header.writeInt32LE(width, 4);
  header.writeInt32LE(height * 2, 8); // XOR + AND planes
  header.writeUInt16LE(1, 12); // planes
  header.writeUInt16LE(32, 14); // bits per pixel
  header.writeUInt32LE(0, 16); // BI_RGB
  header.writeUInt32LE(xorSize + maskSize, 20);
  header.writeInt32LE(0, 24);
  header.writeInt32LE(0, 28);
  header.writeUInt32LE(0, 32);
  header.writeUInt32LE(0, 36);

  const xor = Buffer.alloc(xorSize);
  for (let y = 0; y < height; y++) {
    const srcRow = (height - 1 - y) * width * 4;
    const dstRow = y * width * 4;
    for (let x = 0; x < width; x++) {
      const src = srcRow + x * 4;
      const dst = dstRow + x * 4;
      xor[dst] = data[src + 2]; // B
      xor[dst + 1] = data[src + 1]; // G
      xor[dst + 2] = data[src]; // R
      xor[dst + 3] = data[src + 3]; // A
    }
  }

  // The tile is fully opaque, so every AND-mask bit stays 0.
  const mask = Buffer.alloc(maskSize);
  return Buffer.concat([header, xor, mask]);
}

function encodeIco(): Buffer {
  const images = ICO_SIZES.map((size) => ({
    size,
    data: size === 256 ? encodePng(render(256)) : encodeDib(render(size)),
  }));

  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0); // reserved
  header.writeUInt16LE(1, 2); // icon
  header.writeUInt16LE(images.length, 4);

  const directory = Buffer.alloc(16 * images.length);
  let offset = header.length + directory.length;
  images.forEach(({ size, data }, index) => {
    const at = index * 16;
    directory[at] = size >= 256 ? 0 : size;
    directory[at + 1] = size >= 256 ? 0 : size;
    directory[at + 2] = 0; // palette
    directory[at + 3] = 0; // reserved
    directory.writeUInt16LE(1, at + 4); // planes
    directory.writeUInt16LE(32, at + 6); // bits per pixel
    directory.writeUInt32LE(data.length, at + 8);
    directory.writeUInt32LE(offset, at + 12);
    offset += data.length;
  });

  return Buffer.concat([header, directory, ...images.map((image) => image.data)]);
}

// --- SVG -------------------------------------------------------------------

function encodeSvg(): string {
  validate();
  const cell = SVG_SIZE / GRID;
  const rects: string[] = [];
  MASTER.forEach((row, y) => {
    let x = 0;
    while (x < GRID) {
      if (row[x] !== "#") {
        x++;
        continue;
      }
      const start = x;
      while (x < GRID && row[x] === "#") x++;
      rects.push(
        `  <rect x="${start * cell}" y="${y * cell}" width="${(x - start) * cell}" height="${cell}" fill="#111111"/>`,
      );
    }
  });

  return [
    '<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024" role="img" aria-label="ocr-tool" shape-rendering="crispEdges">',
    "  <title>ocr-tool</title>",
    '  <rect width="1024" height="1024" fill="#F2EFE7"/>',
    ...rects,
    "</svg>",
    "",
  ].join("\n");
}

// --- Preview ---------------------------------------------------------------

function blank(width: number, height: number, background: Rgba): Bitmap {
  const data = Buffer.alloc(width * height * 4);
  for (let i = 0; i < width * height; i++) {
    data[i * 4] = background[0];
    data[i * 4 + 1] = background[1];
    data[i * 4 + 2] = background[2];
    data[i * 4 + 3] = background[3];
  }
  return { width, height, data };
}

function paste(canvas: Bitmap, icon: Bitmap, x: number, y: number): void {
  for (let row = 0; row < icon.height; row++) {
    const src = row * icon.width * 4;
    const dst = ((y + row) * canvas.width + x) * 4;
    icon.data.copy(canvas.data, dst, src, src + icon.width * 4);
  }
}

/**
 * 1:1 icons (16, 24, 32, 48, 64, 128) on a light and a dark background, plus
 * 8x enlargements of the 16 and 32 px renders (the latter is the 256 px asset).
 */
function preview(): Bitmap {
  const width = 960;
  const panel = 320;
  const canvas = blank(width, panel * 2, PREVIEW_LIGHT);
  paste(canvas, blank(width, panel, PREVIEW_DARK), 0, panel);

  for (const [top, background] of [
    [0, PREVIEW_LIGHT],
    [panel, PREVIEW_DARK],
  ] as const) {
    const paper = blank(width, panel, background);
    // 1:1 renders, top-aligned.
    paste(paper, render(16), 24, 24);
    paste(paper, render(24), 64, 24);
    paste(paper, render(32), 112, 24);
    paste(paper, render(48), 168, 24);
    paste(paper, render(64), 240, 24);
    paste(paper, render(128), 328, 24);
    // Nearest-neighbour 8x enlargements.
    paste(paper, render(16 * 8), 520, 24);
    paste(paper, render(32 * 8), 680, 24);
    paste(canvas, paper, 0, top);
  }

  return canvas;
}

// --- Main ------------------------------------------------------------------

function write(path: string, data: Buffer | string): void {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, data);
}

write(join(ASSETS, "icon-256.png"), encodePng(render(256)));
write(join(ASSETS, "ocr-tool.ico"), encodeIco());
write(join(ASSETS, "ocr-tool.svg"), encodeSvg());
write(join(REPORTS, "preview.png"), encodePng(preview()));
write(join(REPORTS, "expected-32.png"), encodePng(render(32)));

console.log(`icon assets + previews -> ${ASSETS}, ${REPORTS}`);
