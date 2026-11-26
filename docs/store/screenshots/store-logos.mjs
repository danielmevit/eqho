// Renders Microsoft Partner Center Store logo assets for Eqho:
//   - box-art-2160x2160.png, box-art-1080x1080.png (1:1 Box Art)
//   - poster-art-1440x2160.png, poster-art-720x1080.png (9:16 Poster Art, featuring the real Eqho pill)
//   - logo-mark-1080.png, app-tile-300x300.png, app-tile-150x150.png, app-tile-71x71.png
//
// Usage:
//   NODE_PATH="/mnt/d/Vibe Coding/glitch-lab/node_modules" node docs/store/screenshots/store-logos.mjs \
//     --caps <0.9.4-captures-dir> --out <dir>
import { createRequire } from 'node:module';
import { readFileSync, mkdirSync, existsSync } from 'node:fs';
import { dirname, resolve, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const require = createRequire(join(process.env.NODE_PATH?.split(':')[0] || process.cwd(), 'x.js'));
const { chromium } = require('playwright');
const here = dirname(fileURLToPath(import.meta.url));
const arg = (n, d) => { const i = process.argv.indexOf(n); return i > 0 ? process.argv[i + 1] : d; };
const caps = resolve(arg('--caps', join(here, '../../../dist/captures')));
const out = resolve(arg('--out', join(here, '../../../dist/store-assets/store-logos')));
mkdirSync(out, { recursive: true });

const markSvg = readFileSync(join(here, 'eqho-mark.svg'), 'utf8');
const realPillPath = join(caps, 'pill_dark_real.png');
const realPillHtml = existsSync(realPillPath)
  ? `<img src="data:image/png;base64,${readFileSync(realPillPath).toString('base64')}" width="260" height="48" style="margin-top:38px;display:block;filter:drop-shadow(0 16px 32px rgba(4,10,24,0.58))" alt="Eqho listening pill" />`
  : `<div class="pill-mini"></div>`;

const brandBg = 'radial-gradient(900px 700px at 25% 22%, rgba(88,166,255,0.22), transparent 68%), linear-gradient(135deg, #1E54B5 0%, #1A489B 32%, #102B63 68%, #09152E 100%)';

const tile = (px, shadow = '') => {
  const markPx = Math.round(px * 0.64);
  const rad = Math.round(px * 0.25);
  const svgSized = markSvg.replace(/width="512" height="512"/, `width="${markPx}" height="${markPx}"`);
  return `<div style="width:${px}px;height:${px}px;border-radius:${rad}px;background:linear-gradient(180deg,#18191D 0%,#101114 100%);border:${Math.max(1, Math.round(px * 0.008))}px solid rgba(88,166,255,0.28);box-sizing:border-box;display:grid;place-items:center;${shadow}">${svgSized}</div>`;
};

const lift = 'box-shadow:0 32px 80px rgba(4,10,24,0.55), 0 0 0 1px rgba(255,255,255,0.06) inset';
const W = '/mnt/c/Windows/Fonts';
const font = existsSync(join(W, 'seguisb.ttf'))
  ? `@font-face{font-family:"Eqho Segoe UI";font-weight:600;src:url(${pathToFileURL(join(W, 'seguisb.ttf')).href})}
     @font-face{font-family:"Eqho Segoe UI";font-weight:400;src:url(${pathToFileURL(join(W, 'segoeui.ttf')).href})}`
  : '';

const baseCss = `${font}
  html,body{margin:0;padding:0;overflow:hidden}
  body{font-family:"Eqho Segoe UI","Segoe UI","Inter",sans-serif;color:#F5F6F8;-webkit-font-smoothing:antialiased}
  .c{position:absolute;inset:0;display:flex;flex-direction:column;align-items:center;justify-content:center}
  .name{font-weight:600;letter-spacing:-0.02em;line-height:1;color:#FFFFFF}
  .line{font-weight:400;color:rgba(245,246,248,0.88);line-height:1.3;text-align:center}
  .pill-mini{margin-top:34px;width:140px;height:20px;border-radius:999px;background:linear-gradient(90deg,#1A489B 0%,#58A6FF 50%,#1A489B 100%);box-shadow:0 0 28px rgba(88,166,255,0.45);opacity:0.9}`;

const jobs = [
  ['box-art', 1080, 1080, `<div class="c" style="background:${brandBg}">${tile(440, lift)}
    <div class="name" style="font-size:152px;margin-top:60px">Eqho</div>
    <div class="line" style="font-size:40px;margin-top:20px">Offline voice dictation</div></div>`],
  ['poster-art', 720, 1080, `<div class="c" style="background:${brandBg}">${tile(330, lift)}
    <div class="name" style="font-size:132px;margin-top:52px">Eqho</div>
    <div class="line" style="font-size:38px;margin-top:22px;max-width:520px">Speak anywhere.<br>100% offline on your PC.</div>
    ${realPillHtml}</div>`],
];

const browser = await chromium.launch({ args: ['--allow-file-access-from-files'] });

for (const [stem, w, h, html] of jobs) {
  for (const s of [2, 1]) {
    const page = await browser.newPage({ viewport: { width: w, height: h }, deviceScaleFactor: s });
    await page.setContent(`<!doctype html><html><head><style>${baseCss}</style></head><body>${html}</body></html>`);
    await page.evaluate(() => document.fonts.ready);
    const file = join(out, `${stem}-${w * s}x${h * s}.png`);
    await page.screenshot({ path: file });
    console.log(file);
    await page.close();
  }
}

for (const [name, px] of [
  ['logo-mark-1080', 1080],
  ['app-tile-300x300', 300],
  ['app-tile-150x150', 150],
  ['app-tile-71x71', 71],
]) {
  const page = await browser.newPage({ viewport: { width: px, height: px } });
  await page.setContent(`<!doctype html><html><head><style>html,body{margin:0;background:transparent;overflow:hidden}</style></head><body>${tile(px)}</body></html>`);
  const file = join(out, `${name}.png`);
  await page.screenshot({ path: file, omitBackground: true });
  console.log(file);
  await page.close();
}

await browser.close();
