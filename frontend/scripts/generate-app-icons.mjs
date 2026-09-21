#!/usr/bin/env node
// Regenerates frontend/assets/*.png from the shared repo-wide logo
// (assets/logo.svg by default). Re-run this whenever the logo changes —
// don't hand-edit the PNGs.
//
// Usage:
//   node scripts/generate-app-icons.mjs [--source <path/to/logo.svg>]
//                                        [--out <dir>] [--background <#hex>]

import { mkdir, readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import sharp from 'sharp';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, '../..');

function parseArgs(argv) {
  const args = {
    source: path.join(repoRoot, 'assets/logo.svg'),
    outDir: path.join(__dirname, '../assets'),
    background: '#faf8f3', // the app's cream background (see screen styles)
  };
  for (let i = 0; i < argv.length; i++) {
    const value = argv[i + 1];
    if (argv[i] === '--source') args.source = path.resolve(value);
    else if (argv[i] === '--out') args.outDir = path.resolve(value);
    else if (argv[i] === '--background') args.background = value;
  }
  return args;
}

function hexToRgba(hex) {
  const clean = hex.replace('#', '');
  const full =
    clean.length === 3
      ? clean
          .split('')
          .map((c) => c + c)
          .join('')
      : clean;
  const int = parseInt(full.slice(0, 6), 16);
  return {
    r: (int >> 16) & 255,
    g: (int >> 8) & 255,
    b: int & 255,
    alpha: full.length === 8 ? parseInt(full.slice(6, 8), 16) / 255 : 1,
  };
}

const TRANSPARENT = { r: 0, g: 0, b: 0, alpha: 0 };

// `padding` is the fraction of the shorter edge left empty around the logo.
// Android adaptive-icon layers (foreground/monochrome) need a much bigger
// margin — the OS crops to a mask (circle, squircle, teardrop, ...) and only
// guarantees the inner ~66% survives every shape.
//
// `outDir` overrides the default (frontend/assets) for targets that belong
// elsewhere, e.g. the repo-root social preview banner.
const TARGETS = [
  { file: 'icon.png', width: 1024, height: 1024, mode: 'pad', padding: 0.02 },
  { file: 'splash-icon.png', width: 1024, height: 1024, mode: 'pad-transparent', padding: 0.08 },
  { file: 'favicon.png', width: 48, height: 48, mode: 'pad', padding: 0.02 },
  {
    file: 'android-icon-foreground.png',
    width: 512,
    height: 512,
    mode: 'pad-transparent',
    padding: 0.17,
  },
  {
    file: 'android-icon-monochrome.png',
    width: 432,
    height: 432,
    mode: 'monochrome',
    padding: 0.17,
  },
  { file: 'android-icon-background.png', width: 512, height: 512, mode: 'solid' },
  {
    file: 'social-preview.png',
    width: 1280,
    height: 640,
    mode: 'pad',
    padding: 0.12,
    outDir: path.join(repoRoot, 'assets'),
  },
];

/**
 * Rasterizes the SVG at high resolution and trims its own transparent
 * margin — the source `viewBox` has empty padding baked in around the
 * artwork, which would otherwise show up as extra margin no matter what
 * `padding` each target below asks for.
 */
async function loadTrimmedLogo(svgBuffer) {
  return sharp(svgBuffer)
    .resize(2048, 2048, { fit: 'inside', background: TRANSPARENT })
    .png()
    .trim()
    .toBuffer();
}

/** Fits the (already-trimmed) logo into a transparent `size`x`size` square, centered. */
async function renderLogo(trimmedLogoBuffer, size) {
  return sharp(trimmedLogoBuffer)
    .resize(size, size, { fit: 'contain', background: TRANSPARENT })
    .png()
    .toBuffer();
}

/** Android's themed-icon layer: a solid-white silhouette, alpha preserved. */
async function toMonochrome(pngBuffer) {
  const { data, info } = await sharp(pngBuffer)
    .ensureAlpha()
    .raw()
    .toBuffer({ resolveWithObject: true });
  for (let i = 0; i < data.length; i += 4) {
    data[i] = 255;
    data[i + 1] = 255;
    data[i + 2] = 255;
  }
  return sharp(data, { raw: { width: info.width, height: info.height, channels: 4 } })
    .png()
    .toBuffer();
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const backgroundRgba = hexToRgba(args.background);
  const svgBuffer = await readFile(args.source);
  const trimmedLogo = await loadTrimmedLogo(svgBuffer);

  for (const target of TARGETS) {
    const outDir = target.outDir ?? args.outDir;
    await mkdir(outDir, { recursive: true });
    const outPath = path.join(outDir, target.file);
    const { width, height } = target;

    if (target.mode === 'solid') {
      await sharp({ create: { width, height, channels: 4, background: backgroundRgba } })
        .png()
        .toFile(outPath);
      console.log(`wrote ${target.file} (${width}x${height}, solid ${args.background})`);
      continue;
    }

    const contentSize = Math.round(Math.min(width, height) * (1 - target.padding * 2));
    let logo = await renderLogo(trimmedLogo, contentSize);
    if (target.mode === 'monochrome') logo = await toMonochrome(logo);

    const canvasBackground = target.mode === 'pad' ? backgroundRgba : TRANSPARENT;
    await sharp({ create: { width, height, channels: 4, background: canvasBackground } })
      .composite([{ input: logo, gravity: 'center' }])
      .png()
      .toFile(outPath);
    console.log(`wrote ${target.file} (${width}x${height})`);
  }
}

main().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});
