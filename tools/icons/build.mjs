// Builds every VYRA system icon from one geometry: two rounded bars that cross into a V.
// Run: npm run build (inside tools/icons). Output goes to tools/icons/dist.

import { Resvg } from '@resvg/resvg-js';
import { mkdir, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const root = path.dirname(fileURLToPath(import.meta.url));
const dist = path.join(root, 'dist');

const COLOR = {
  light: '#b3a6dc',
  deep: '#6a5aa8',
  deepOpacity: 0.78,
  sheet: '#efe9de',
  fold: '#d9d1c2',
  sheetEdge: '#cbc2b1',
  // On the ivory sheet the pale bar would vanish, so file icons use a darker pair.
  onSheetLight: '#9d8fd6',
  onSheetDeep: '#5f4f9e',
  band: '#6a5aa8',
  label: '#efe9de',
};

const ANGLE = 23;
const ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 256];
const TRAY_SIZES = [16, 20, 24, 32];
const FORMATS = ['MP3', 'FLAC', 'WAV', 'AIFF', 'M4A', 'AAC', 'OGG', 'OPUS', 'WMA', 'APE', 'WV', 'DSF', 'DFF', 'AC3', 'DTS'];

// Small sizes get less margin and thicker bars so the V survives at 16 px.
// margin: empty pixels around the mark; weight: bar radius as a share of mark height.
const MARK_TUNING = {
  16: { margin: 0.5, weight: 0.19 },
  20: { margin: 1, weight: 0.19 },
  24: { margin: 1, weight: 0.185 },
  32: { margin: 1.5, weight: 0.18 },
  40: { margin: 2, weight: 0.172 },
  48: { margin: 3, weight: 0.166 },
  64: { margin: 4, weight: 0.162 },
  256: { margin: 16, weight: 0.162 },
};

const n = (v) => Number(v.toFixed(3));

function svg(size, body) {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 ${size} ${size}">${body}</svg>`;
}

// Fits the V into a box and returns the shared pivot, bar radius and bar length.
function fitMark(box, weight) {
  const a = (ANGLE * Math.PI) / 180;
  let height = box.h;
  let r = weight * height;
  let length = (height - 2 * r) / Math.cos(a);
  const width = 2 * (length * Math.sin(a) + r);
  if (width > box.w) {
    const s = box.w / width;
    height *= s;
    r *= s;
    length *= s;
  }
  const top = box.y + (box.h - height) / 2;
  return { r, length, cx: box.x + box.w / 2, pivotY: top + height - r };
}

function bar(g, angle, attrs) {
  const x = g.cx - g.r;
  const y = g.pivotY - g.length - g.r;
  return `<rect x="${n(x)}" y="${n(y)}" width="${n(2 * g.r)}" height="${n(g.length + 2 * g.r)}" rx="${n(g.r)}" transform="rotate(${angle} ${n(g.cx)} ${n(g.pivotY)})" ${attrs}/>`;
}

function markBody(g, light, deep, deepOpacity) {
  return bar(g, -ANGLE, `fill="${light}"`) + bar(g, ANGLE, `fill="${deep}" fill-opacity="${deepOpacity}"`);
}

function markSvg(size) {
  const t = MARK_TUNING[size];
  const side = size - 2 * t.margin;
  const g = fitMark({ x: t.margin, y: t.margin, w: side, h: side }, t.weight);
  return svg(size, markBody(g, COLOR.light, COLOR.deep, COLOR.deepOpacity));
}

// One-colour mark for the tray. From 24 px a thin gap keeps the upper bar readable as a
// separate layer; below that the gap turns into noise, so the V is solid.
function traySvg(size, color) {
  const margin = size <= 20 ? 0.5 : 1;
  const side = size - 2 * margin;
  const g = fitMark({ x: margin, y: margin, w: side, h: side }, 0.19);
  if (size < 24) return svg(size, bar(g, -ANGLE, `fill="${color}"`) + bar(g, ANGLE, `fill="${color}"`));
  const gap = size * 0.05;
  const cut = bar(g, ANGLE, `fill="#000" stroke="#000" stroke-width="${n(2 * gap)}"`);
  return svg(
    size,
    `<mask id="cut" maskUnits="userSpaceOnUse" x="0" y="0" width="${size}" height="${size}"><rect width="${size}" height="${size}" fill="#fff"/>${cut}</mask>` +
      `<g mask="url(#cut)">${bar(g, -ANGLE, `fill="${color}"`)}</g>` +
      bar(g, ANGLE, `fill="${color}"`),
  );
}

// Sheet with a folded corner. From 48 px up it carries a band with the format name.
function fileSvg(size, label) {
  const left = Math.round(size * 0.17);
  const right = size - left;
  const top = Math.round(size * 0.05);
  const bottom = size - top;
  const rad = Math.max(1, size * 0.055);
  const fold = Math.round(size * 0.2);
  const labelled = size >= 48;
  const bandH = labelled ? Math.round(size * 0.27) : Math.max(2, Math.round(size * 0.12));
  const bandTop = bottom - bandH;
  const edge = Math.max(1, Math.round(size / 128));
  const i = edge / 2;

  const outline = (l, t, r, b, rr) =>
    `M${n(l + rr)} ${n(t)} H${n(r - fold)} L${n(r)} ${n(t + fold)} V${n(b - rr)} a${n(rr)} ${n(rr)} 0 0 1 ${n(-rr)} ${n(rr)} H${n(l + rr)} a${n(rr)} ${n(rr)} 0 0 1 ${n(-rr)} ${n(-rr)} V${n(t + rr)} a${n(rr)} ${n(rr)} 0 0 1 ${n(rr)} ${n(-rr)} Z`;

  const sheet = `<path d="${outline(left, top, right, bottom, rad)}" fill="${COLOR.sheet}"/>`;
  const band = `<path d="M${left} ${bandTop} H${right} V${n(bottom - rad)} a${n(rad)} ${n(rad)} 0 0 1 ${n(-rad)} ${n(rad)} H${n(left + rad)} a${n(rad)} ${n(rad)} 0 0 1 ${n(-rad)} ${n(-rad)} Z" fill="${COLOR.band}"/>`;
  const foldShape = `<path d="M${right - fold} ${top} L${right} ${top + fold} H${right - fold} Z" fill="${COLOR.fold}"/>`;
  const rim = `<path d="${outline(left + i, top + i, right - i, bottom - i, Math.max(0.5, rad - i))}" fill="none" stroke="${COLOR.sheetEdge}" stroke-width="${edge}" stroke-opacity="0.9"/>`;

  const pad = labelled ? size * 0.1 : 1;
  const box = {
    x: left + pad,
    y: top + (labelled ? size * 0.15 : fold * 0.55),
    w: right - left - 2 * pad,
    h: 0,
  };
  box.h = bandTop - (labelled ? size * 0.07 : 1) - box.y;
  const g = fitMark(box, labelled ? 0.17 : 0.2);
  const mark = markBody(g, COLOR.onSheetLight, COLOR.onSheetDeep, 0.85);

  let text = '';
  if (labelled) {
    const maxByWidth = ((right - left) * 0.84) / (label.length * 0.64);
    const fontSize = Math.min(bandH * 0.6, maxByWidth);
    const baseline = bandTop + bandH / 2 + fontSize * 0.355;
    text = `<text x="${n(size / 2)}" y="${n(baseline)}" text-anchor="middle" font-family="Segoe UI" font-weight="600" font-size="${n(fontSize)}" letter-spacing="${n(fontSize * 0.03)}" fill="${COLOR.label}">${label}</text>`;
  }

  // The rim goes under the band, otherwise a pale line shows around the band at small sizes.
  return svg(size, sheet + rim + band + foldShape + mark + text);
}

function render(source, size) {
  const image = new Resvg(source, {
    fitTo: { mode: 'width', value: size },
    font: { loadSystemFonts: true, defaultFontFamily: 'Segoe UI' },
  }).render();
  return { size, png: image.asPng(), rgba: Buffer.from(image.pixels) };
}

// ICO container: PNG for 256 px, 32-bit bitmaps for the rest, which every Windows shell path reads.
function ico(images) {
  const blobs = images.map((im) => (im.size >= 256 ? im.png : dib(im)));
  const head = Buffer.alloc(6 + 16 * images.length);
  head.writeUInt16LE(1, 2);
  head.writeUInt16LE(images.length, 4);
  let offset = head.length;
  images.forEach((im, index) => {
    const at = 6 + 16 * index;
    const dim = im.size >= 256 ? 0 : im.size;
    head.writeUInt8(dim, at);
    head.writeUInt8(dim, at + 1);
    head.writeUInt16LE(1, at + 4);
    head.writeUInt16LE(32, at + 6);
    head.writeUInt32LE(blobs[index].length, at + 8);
    head.writeUInt32LE(offset, at + 12);
    offset += blobs[index].length;
  });
  return Buffer.concat([head, ...blobs]);
}

function dib({ size, rgba }) {
  const maskRow = Math.ceil(size / 32) * 4;
  const pixelBytes = size * size * 4;
  const out = Buffer.alloc(40 + pixelBytes + maskRow * size);
  out.writeUInt32LE(40, 0);
  out.writeInt32LE(size, 4);
  out.writeInt32LE(size * 2, 8);
  out.writeUInt16LE(1, 12);
  out.writeUInt16LE(32, 14);
  out.writeUInt32LE(pixelBytes + maskRow * size, 20);
  for (let y = 0; y < size; y++) {
    const srcRow = size - 1 - y;
    for (let x = 0; x < size; x++) {
      const src = (srcRow * size + x) * 4;
      const dst = 40 + (y * size + x) * 4;
      const alpha = rgba[src + 3];
      // resvg hands back premultiplied RGBA, ICO bitmaps want straight alpha.
      const un = (c) => (alpha === 0 ? 0 : Math.min(255, Math.round((c * 255) / alpha)));
      out[dst] = un(rgba[src + 2]);
      out[dst + 1] = un(rgba[src + 1]);
      out[dst + 2] = un(rgba[src]);
      out[dst + 3] = alpha;
      if (alpha === 0) out[40 + pixelBytes + y * maskRow + (x >> 3)] |= 0x80 >> (x & 7);
    }
  }
  return out;
}

const dataUri = (png) => `data:image/png;base64,${Buffer.from(png).toString('base64')}`;

// Contact sheet for checking the set by eye: real sizes plus enlarged small ones.
function previewSvg(sets) {
  const W = 1180;
  const rows = [];
  let y = 0;
  const strip = (bg, fg, title, height, draw) => {
    let body = `<rect x="0" y="${y}" width="${W}" height="${height}" fill="${bg}"/>`;
    body += `<text x="24" y="${y + 26}" font-family="Segoe UI" font-size="13" fill="${fg}">${title}</text>`;
    body += draw(y + 44);
    rows.push(body);
    y += height;
  };
  const img = (im, x, top, scale = 1, crisp = false) =>
    `<image href="${dataUri(im.png)}" x="${x}" y="${top}" width="${im.size * scale}" height="${im.size * scale}"${crisp ? ' image-rendering="optimizeSpeed"' : ''}/>`;

  const appRow = (top) => {
    let x = 24;
    let out = '';
    for (const s of [256, 64, 48, 40, 32, 24, 20, 16]) {
      const im = sets.app.find((i) => i.size === s);
      const shown = s === 256 ? 0.5 : 1;
      out += img(im, x, top + (128 - s * shown), shown);
      x += s * shown + 20;
    }
    for (const s of [32, 24, 16]) {
      const im = sets.app.find((i) => i.size === s);
      const scale = s === 32 ? 4 : s === 24 ? 5 : 8;
      out += img(im, x, top + (128 - s * scale), scale, true);
      x += s * scale + 20;
    }
    return out;
  };
  strip('#202020', '#a1a1a6', 'App icon on the dark taskbar: 256 at half size, 64 to 16, then 32, 24 and 16 enlarged', 196, appRow);
  strip('#f3f3f3', '#525252', 'App icon on the light taskbar', 196, appRow);

  const trayRow = (key) => (top) => {
    let x = 24;
    let out = '';
    for (const im of sets[key]) {
      out += img(im, x, top + (32 - im.size));
      x += im.size + 16;
    }
    out += img(sets[key][0], x + 8, top - 4, 5, true);
    return out;
  };
  strip('#202020', '#a1a1a6', 'Tray, white: 16, 20, 24, 32 and 16 enlarged', 136, trayRow('trayWhite'));
  strip('#f3f3f3', '#525252', 'Tray, black', 136, trayRow('trayBlack'));

  const fileRow = (top) => {
    let x = 24;
    let out = '';
    const flac = sets.files.FLAC;
    for (const s of [256, 64, 48, 32, 24, 16]) {
      const im = flac.find((i) => i.size === s);
      const shown = s === 256 ? 0.5 : 1;
      out += img(im, x, top + (128 - s * shown), shown);
      x += s * shown + 16;
    }
    out += img(flac.find((i) => i.size === 48), x, top + (128 - 48 * 2.5), 2.5, true);
    x += 48 * 2.5 + 16;
    out += img(flac.find((i) => i.size === 16), x, top, 8, true);
    return out;
  };
  strip('#191919', '#a1a1a6', 'File icon in dark Explorer: 256 at half size, 64 to 16, then 48 and 16 enlarged', 196, fileRow);
  strip('#ffffff', '#525252', 'File icon in light Explorer', 196, fileRow);

  strip('#191919', '#a1a1a6', 'All formats at 64 px', 132, (top) => {
    let x = 24;
    let out = '';
    for (const format of FORMATS) {
      out += img(sets.files[format].find((i) => i.size === 64), x, top);
      x += 64 + 12;
    }
    return out;
  });

  return `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${y}" viewBox="0 0 ${W} ${y}">${rows.join('')}</svg>`;
}

await mkdir(path.join(dist, 'files'), { recursive: true });
await mkdir(path.join(dist, 'tray'), { recursive: true });

const sets = { app: [], trayWhite: [], trayBlack: [], files: {} };

sets.app = ICO_SIZES.map((s) => render(markSvg(s), s));
await writeFile(path.join(dist, 'vyra.ico'), ico(sets.app));
await writeFile(path.join(dist, 'vyra-256.png'), sets.app.at(-1).png);
await writeFile(path.join(dist, 'mark.svg'), markSvg(256));

sets.trayWhite = TRAY_SIZES.map((s) => render(traySvg(s, '#ffffff'), s));
sets.trayBlack = TRAY_SIZES.map((s) => render(traySvg(s, '#1b1b1b'), s));
await writeFile(path.join(dist, 'tray', 'tray-white.ico'), ico(sets.trayWhite));
await writeFile(path.join(dist, 'tray', 'tray-black.ico'), ico(sets.trayBlack));

for (const format of FORMATS) {
  sets.files[format] = ICO_SIZES.map((s) => render(fileSvg(s, format), s));
  await writeFile(path.join(dist, 'files', `${format.toLowerCase()}.ico`), ico(sets.files[format]));
}

const sheet = previewSvg(sets);
const preview = new Resvg(sheet, { font: { loadSystemFonts: true, defaultFontFamily: 'Segoe UI' } }).render();
await writeFile(path.join(dist, 'preview.png'), preview.asPng());

// Images for the repository page.
const assets = path.resolve(root, '..', '..', 'docs', 'assets');
await mkdir(assets, { recursive: true });
await writeFile(path.join(assets, 'mark.svg'), markSvg(256));

const fontOptions = { font: { loadSystemFonts: true, defaultFontFamily: 'Segoe UI' } };

const rowFormats = ['FLAC', 'MP3', 'WAV', 'OPUS', 'M4A', 'DSF'];
const tile = 192;
const tileGap = 40;
const rowWidth = rowFormats.length * tile + (rowFormats.length - 1) * tileGap;
const rowSvg =
  `<svg xmlns="http://www.w3.org/2000/svg" width="${rowWidth}" height="${tile}" viewBox="0 0 ${rowWidth} ${tile}">` +
  rowFormats
    .map((format, index) => {
      const png = render(fileSvg(tile, format), tile).png;
      return `<image href="${dataUri(png)}" x="${index * (tile + tileGap)}" y="0" width="${tile}" height="${tile}"/>`;
    })
    .join('') +
  '</svg>';
await writeFile(path.join(assets, 'file-icons.png'), new Resvg(rowSvg, fontOptions).render().asPng());

// GitHub social preview, 1280x640. Uploaded by hand in the repository settings.
const socialMark = render(markSvg(256), 256).png;
const socialSvg =
  '<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="640" viewBox="0 0 1280 640">' +
  '<rect width="1280" height="640" fill="#141415"/>' +
  `<image href="${dataUri(socialMark)}" x="208" y="192" width="256" height="256"/>` +
  '<text x="528" y="336" font-family="Segoe UI" font-weight="600" font-size="128" letter-spacing="6" fill="#ededed">VYRA</text>' +
  '<text x="534" y="408" font-family="Segoe UI" font-size="40" fill="#a1a1a6">Your music. Your space.</text>' +
  '</svg>';
await writeFile(path.join(assets, 'social-preview.png'), new Resvg(socialSvg, fontOptions).render().asPng());

console.log(`app icon: ${ICO_SIZES.length} sizes`);
console.log(`tray: 2 colours x ${TRAY_SIZES.length} sizes`);
console.log(`file types: ${FORMATS.length} formats x ${ICO_SIZES.length} sizes`);
console.log(`preview: ${preview.width}x${preview.height}`);
