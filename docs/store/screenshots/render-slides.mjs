// Renders 6 Microsoft Store presentation slides for Eqho (3840x2160 + 1920x1080 + contact-sheet.png).
// Guided onboarding-style introduction covering every major feature of Eqho 0.9.4 using:
//   - Real high-res iridescent pill PNGs rendered by `eqho_core::pillfx`
//   - Real Eqho tray icon monogram (`eqho-mark.svg`)
//   - Real Eqho 0.9.4 UI captures (Obsidian Dark & Crisp Light)
//   - Zero GitHub URLs, zero competitor names.
//
// Usage:
//   NODE_PATH="/mnt/d/Vibe Coding/glitch-lab/node_modules" node docs/store/screenshots/render-slides.mjs \
//     --caps <0.9.4-captures-dir> --out <output-dir>
import { createRequire } from 'node:module';
import { readFileSync, mkdirSync, existsSync } from 'node:fs';
import { dirname, resolve, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const require = createRequire(join(process.env.NODE_PATH?.split(':')[0] || process.cwd(), 'x.js'));
const { chromium } = require('playwright');
const here = dirname(fileURLToPath(import.meta.url));
const arg = (n, d) => { const i = process.argv.indexOf(n); return i > 0 ? process.argv[i + 1] : d; };
const caps = resolve(arg('--caps', join(here, '../../../dist/captures')));
const out = resolve(arg('--out', join(here, '../../../dist/store-assets/screenshots')));
mkdirSync(out, { recursive: true });

const markSvg = readFileSync(join(here, 'eqho-mark.svg'), 'utf8');
const capDataUri = (name) => {
  const p = join(caps, name);
  const b64 = readFileSync(p).toString('base64');
  return `data:image/png;base64,${b64}`;
};

const W = '/mnt/c/Windows/Fonts';
const fontCss = existsSync(join(W, 'segoeui.ttf')) ? `
  @font-face { font-family: "Eqho UI"; font-weight: 400; src: url(${pathToFileURL(join(W, 'segoeui.ttf')).href}); }
  @font-face { font-family: "Eqho UI"; font-weight: 600; src: url(${pathToFileURL(join(W, 'seguisb.ttf')).href}); }
  @font-face { font-family: "Eqho UI"; font-weight: 700; src: url(${pathToFileURL(join(W, 'segoeuib.ttf')).href}); }
` : '';

const brandChip = (dark, stepLabel) => {
  const svgSmall = markSvg.replace(/width="512" height="512"/, 'width="32" height="32"');
  return `<div class="brand-row">
    <div class="brand-tile ${dark ? 'dark' : 'light'}">${svgSmall}</div>
    <span class="brand-name ${dark ? 'dark' : 'light'}">Eqho</span>
    <span class="step-pill ${dark ? 'dark' : 'light'}">${stepLabel}</span>
  </div>`;
};

const flagSvg = (code) => {
  const maps = {
    en: `<rect width="21" height="14" fill="#112C66"/><path d="M0,0 L21,14 M21,0 L0,14" stroke="#F8FAFC" stroke-width="2.8"/><path d="M0,0 L21,14 M21,0 L0,14" stroke="#D82A2E" stroke-width="1.2"/><rect x="8.5" width="4" height="14" fill="#F8FAFC"/><rect y="5" width="21" height="4" fill="#F8FAFC"/><rect x="9.3" width="2.4" height="14" fill="#D82A2E"/><rect y="5.8" width="21" height="2.4" fill="#D82A2E"/>`,
    de: `<rect width="21" height="4.7" fill="#16181D"/><rect y="4.7" width="21" height="4.7" fill="#D82A2E"/><rect y="9.3" width="21" height="4.7" fill="#F2AE18"/>`,
    es: `<rect width="21" height="3.5" fill="#D82A2E"/><rect y="3.5" width="21" height="7" fill="#F6CA18"/><rect y="10.5" width="21" height="3.5" fill="#D82A2E"/>`,
    fr: `<rect width="7" height="14" fill="#112C66"/><rect x="7" width="7" height="14" fill="#F8FAFC"/><rect x="14" width="7" height="14" fill="#D82A2E"/>`,
    ja: `<rect width="21" height="14" fill="#F8FAFC"/><circle cx="10.5" cy="7" r="3.8" fill="#DC143C"/>`,
    bg: `<rect width="21" height="4.7" fill="#F8FAFC"/><rect y="4.7" width="21" height="4.7" fill="#168A42"/><rect y="9.3" width="21" height="4.7" fill="#D82A2E"/>`,
  };
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 21 14" width="24" height="16" style="border-radius:3px;box-shadow:0 0 0 1px rgba(255,255,255,0.16);flex-shrink:0">${maps[code] || maps.en}</svg>`;
};

// Real Eqho iridescent listening pill rendered directly by Rust `eqho_core::pillfx::render_premul_bgra`.
const realPillHtml = (variant = 'dark', w = 360) => {
  const h = Math.round((w * 132) / 720);
  const src = capDataUri(variant === 'light' ? 'pill_light_real.png' : 'pill_dark_real.png');
  return `<img class="real-pill ${variant}" src="${src}" width="${w}" height="${h}" alt="Eqho iridescent listening pill" />`;
};

// Authentic 0.9.4 flush pill-shaped under-pill micro-bar (matching `UnderbarTheme::for_theme`).
const underbarHtml = ({ lang = 'EN', mode = 'Batch', pinned = true, timer = '0:14', activeLang = false, light = false } = {}) => `
  <div class="underbar ${light ? 'light' : ''}">
    <div class="ub-btn ub-muted" title="Cancel">✕</div>
    <div class="ub-div"></div>
    <div class="ub-timer">${timer}</div>
    <div class="ub-chip ${activeLang ? 'active' : ''}"><span>${lang}</span><span class="ub-chev">▾</span></div>
    <div class="ub-chip mode">${mode}</div>
    <div class="ub-btn ${pinned ? 'ub-pin-on' : 'ub-muted'}">
      <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
        <path d="M9.5 2.5L13.5 6.5L11.5 8.5L10.5 11.5L4.5 5.5L7.5 4.5L9.5 2.5Z"/>
        <path d="M7.5 8.5L3 13"/>
      </svg>
    </div>
    <div class="ub-done">✓</div>
  </div>`;

const baseCss = `${fontCss}
  * { box-sizing: border-box; }
  html, body { margin: 0; padding: 0; width: 1920px; height: 1080px; overflow: hidden; }
  body { font-family: "Eqho UI", "Segoe UI", "Inter", sans-serif; -webkit-font-smoothing: antialiased; }
  .slide {
    position: relative; width: 1920px; height: 1080px; overflow: hidden;
    display: flex; align-items: center; padding: 84px 96px; gap: 68px;
  }
  .slide.dark {
    background: radial-gradient(980px 760px at 78% 40%, rgba(88,166,255,0.20), transparent 66%),
                radial-gradient(640px 540px at 14% 88%, rgba(26,72,155,0.28), transparent 65%),
                linear-gradient(135deg, #16397A 0%, #10244D 38%, #0E1320 74%, #0B0C10 100%);
    color: #F5F6F8;
  }
  .slide.light {
    background: radial-gradient(960px 760px at 76% 45%, rgba(9,105,218,0.09), transparent 68%),
                linear-gradient(135deg, #F8FAFE 0%, #EEF3FA 55%, #E5EDF8 100%);
    color: #16181D;
  }
  .copy { width: 690px; flex-shrink: 0; z-index: 5; }
  .brand-row { display: flex; align-items: center; gap: 15px; margin-bottom: 32px; }
  .brand-tile {
    width: 54px; height: 54px; border-radius: 14px; display: grid; place-items: center;
  }
  .brand-tile.dark {
    background: linear-gradient(180deg,#18191E 0%,#111216 100%);
    border: 1px solid rgba(88,166,255,0.34);
    box-shadow: 0 12px 28px rgba(4,10,24,0.38);
  }
  .brand-tile.light {
    background: #FFFFFF;
    border: 1px solid rgba(9,105,218,0.18);
    box-shadow: 0 10px 24px rgba(15,35,75,0.10);
  }
  .brand-name { font-size: 30px; font-weight: 600; letter-spacing: -0.02em; }
  .brand-name.dark { color: #FFFFFF; }
  .brand-name.light { color: #16181D; }
  .step-pill {
    margin-left: 6px; font-size: 16px; font-weight: 600; letter-spacing: 0.04em;
    text-transform: uppercase; padding: 7px 14px; border-radius: 999px;
  }
  .step-pill.dark {
    background: rgba(88,166,255,0.14); border: 1px solid rgba(88,166,255,0.34); color: #8AC4FF;
  }
  .step-pill.light {
    background: rgba(9,105,218,0.09); border: 1px solid rgba(9,105,218,0.22); color: #0969DA;
  }

  h1 {
    font-size: 70px; font-weight: 700; line-height: 1.08; letter-spacing: -0.03em;
    margin: 0 0 26px;
  }
  .slide.dark h1 em { font-style: normal; color: #79C0FF; }
  .slide.light h1 em { font-style: normal; color: #0969DA; }

  .sub {
    font-size: 27px; line-height: 1.46; font-weight: 400; margin: 0 0 34px;
    max-width: 650px;
  }
  .slide.dark .sub { color: rgba(245,246,248,0.84); }
  .slide.light .sub { color: #4A5160; }

  .facts { display: flex; flex-wrap: wrap; gap: 12px; }
  .fact {
    font-size: 19px; font-weight: 600; padding: 10px 20px; border-radius: 999px;
    display: inline-flex; align-items: center; gap: 8px;
  }
  .slide.dark .fact {
    background: rgba(255,255,255,0.09); border: 1px solid rgba(255,255,255,0.16); color: #FFFFFF;
  }
  .slide.light .fact {
    background: #FFFFFF; border: 1px solid #D4DCE8; color: #1A1C1F;
    box-shadow: 0 4px 12px rgba(15,35,75,0.05);
  }
  .fact-accent {
    background: #1A489B !important; border-color: rgba(88,166,255,0.55) !important; color: #FFFFFF !important;
  }

  .stage {
    position: relative; flex: 1; height: 900px;
    display: flex; flex-direction: column; align-items: center; justify-content: center;
  }

  /* Framed app screenshots */
  .app-frame {
    border-radius: 16px; overflow: hidden; background: #121215;
    border: 1px solid rgba(255,255,255,0.14);
    box-shadow: 0 36px 90px rgba(4,10,24,0.56), 0 0 0 1px rgba(88,166,255,0.12);
  }
  .slide.light .app-frame {
    border: 1px solid rgba(15,35,75,0.14);
    box-shadow: 0 32px 76px rgba(15,35,75,0.18);
  }
  .app-frame img { display: block; width: 100%; height: auto; }

  /* Mock desktop document on Slide 1 */
  .doc-win {
    width: 890px; border-radius: 18px; background: #121215;
    border: 1px solid rgba(255,255,255,0.13);
    box-shadow: 0 40px 100px rgba(3,8,20,0.62);
    overflow: hidden;
  }
  .doc-bar {
    height: 44px; background: #17171B; border-bottom: 1px solid #282830;
    display: flex; align-items: center; justify-content: space-between; padding: 0 20px;
    color: #9A9EA8; font-size: 15px; font-weight: 500;
  }
  .doc-win-btns { display: flex; gap: 18px; color: #7C818D; font-size: 16px; line-height: 1; }
  .doc-body { padding: 34px 40px 38px; min-height: 290px; }
  .doc-title { font-size: 26px; font-weight: 700; color: #FFFFFF; margin: 0 0 16px; }
  .doc-p { font-size: 22px; line-height: 1.62; color: #D8DCE4; margin: 0; }
  .doc-p .spoken { color: #FFFFFF; }
  .caret {
    display: inline-block; width: 3px; height: 26px; background: #58A6FF;
    vertical-align: -5px; margin-left: 3px; border-radius: 2px;
  }
  .hotkey-pill {
    display: inline-flex; align-items: center; gap: 10px;
    margin-bottom: 20px; padding: 7px 15px; border-radius: 9px;
    background: rgba(88,166,255,0.12); border: 1px solid rgba(88,166,255,0.36);
    color: #79C0FF; font-size: 16px; font-weight: 600;
  }

  /* Real Eqho Acrylic live text box + real iridescent pill + underbar */
  .overlay-stack { display: flex; flex-direction: column; align-items: center; gap: 11px; }
  .acrylic-box {
    max-width: 560px; padding: 13px 18px; border-radius: 10px;
    background: rgba(21,21,25,0.82); backdrop-filter: blur(22px);
    border: 1px solid rgba(255,255,255,0.14);
    box-shadow: 0 18px 44px rgba(3,8,20,0.48);
    color: #F5F6F8; font-size: 19px; line-height: 1.42; text-align: center;
  }
  .acrylic-box .dim { color: #8E929E; }
  .real-pill {
    display: block;
    filter: drop-shadow(0 18px 36px rgba(4,12,30,0.58));
  }
  .real-pill.light {
    filter: drop-shadow(0 16px 32px rgba(15,35,75,0.22));
  }
  .underbar {
    display: inline-flex; align-items: center; gap: 6px; height: 42px; padding: 0 7px;
    border-radius: 999px; background: rgba(22,22,26,0.92); border: 1px solid rgba(62,62,72,0.92);
    box-shadow: 0 14px 36px rgba(4,10,24,0.45); color: #F5F6F8; font-size: 16px; font-weight: 400;
  }
  .ub-btn {
    width: 30px; height: 30px; border-radius: 50%; display: grid; place-items: center;
    font-size: 15px;
  }
  .ub-muted { color: #9898A2; }
  .ub-pin-on { color: #58A6FF; }
  .ub-div { width: 1px; height: 18px; background: rgba(58,58,66,0.85); }
  .ub-timer { color: #9898A2; font-variant-numeric: tabular-nums; padding: 0 7px; }
  .ub-chip {
    display: inline-flex; align-items: center; gap: 5px; height: 30px; padding: 0 11px;
    border-radius: 999px; color: #F5F6F8;
  }
  .ub-chip.active { background: #26262D; color: #58A6FF; }
  .ub-chip.mode { color: #58A6FF; }
  .ub-chev { font-size: 12px; color: #9898A2; }
  .ub-done {
    width: 36px; height: 30px; border-radius: 999px; background: #1A489B; color: #FFFFFF;
    display: grid; place-items: center; font-size: 17px; font-weight: 600;
  }
  .underbar.light {
    background: rgba(255,255,255,0.95); border-color: #C8CBD2; color: #1A1C1F;
    box-shadow: 0 14px 34px rgba(15,35,75,0.12);
  }
  .underbar.light .ub-muted, .underbar.light .ub-timer, .underbar.light .ub-chev { color: #686A74; }
  .underbar.light .ub-div { background: #D4D6DC; }
  .underbar.light .ub-chip { color: #1A1C1F; }
  .underbar.light .ub-chip.active { background: #EEF0F4; color: #0969DA; }
  .underbar.light .ub-chip.mode, .underbar.light .ub-pin-on { color: #0969DA; }
  .underbar.light .ub-done { background: #0969DA; }

  /* Language popup card (Slide 2) */
  .lang-popup {
    width: 460px; border-radius: 14px; background: rgba(21,21,23,0.97); border: 1px solid #2A2A30;
    box-shadow: 0 32px 80px rgba(10,24,54,0.28); padding: 14px; color: #F5F6F8;
  }
  .lang-popup.light {
    background: rgba(255,255,255,0.97); border-color: #CFD3DC; color: #1A1C1F;
    box-shadow: 0 28px 68px rgba(15,35,75,0.16);
  }
  .lang-popup.light .lp-search { background: #F5F6F8; border-color: #0969DA; color: #1A1C1F; }
  .lang-popup.light .lp-row { color: #242730; }
  .lang-popup.light .lp-row.active { background: #EDF4FF; color: #0969DA; }
  .lang-popup.light .lp-tier { color: #6B707C; }
  .lang-popup.light .lp-recents { border-top-color: #E2E5EC; }
  .lang-popup.light .lp-chip { background: #F5F6F8; border-color: #D4D6DC; color: #1A1C1F; }
  .lang-popup.light .lp-chip.active { border-color: #0969DA; color: #0969DA; background: #EDF4FF; }
  .lp-search {
    height: 44px; border-radius: 9px; background: #1C1C21; border: 1px solid #58A6FF;
    display: flex; align-items: center; gap: 12px; padding: 0 14px; font-size: 18px; color: #F5F6F8;
    margin-bottom: 8px;
  }
  .lp-row {
    height: 44px; border-radius: 8px; padding: 0 12px; display: flex; align-items: center;
    justify-content: space-between; font-size: 18px; font-weight: 400; color: #E2E5EC;
  }
  .lp-row.active { background: #222229; color: #58A6FF; }
  .lp-row-l { display: flex; align-items: center; gap: 13px; }
  .lp-tier { font-size: 13px; color: #85858F; }
  .lp-recents {
    margin-top: 10px; padding-top: 10px; border-top: 1px solid #2A2A30;
    display: flex; gap: 8px;
  }
  .lp-chip {
    flex: 1 1 0; height: 38px; border-radius: 8px; background: #1E1E24; border: 1px solid #2A2A30;
    display: flex; align-items: center; justify-content: center; gap: 7px;
    font-size: 16px; color: #F5F6F8;
  }
  .lp-chip.active { border-color: #58A6FF; color: #58A6FF; }

  /* Trust metric cards (Slide 5) */
  .priv-grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 16px; width: 920px; margin-top: 20px; }
  .priv-card {
    background: rgba(18,18,21,0.92); border: 1px solid rgba(88,166,255,0.22);
    border-radius: 12px; padding: 16px 18px;
  }
  .priv-k { font-size: 13px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.06em; color: #79C0FF; margin-bottom: 5px; }
  .priv-v { font-size: 18px; font-weight: 500; color: #FFFFFF; line-height: 1.32; }
`;

const slides = [
  {
    id: '01-hero',
    theme: 'dark',
    step: '01 · Press Alt+Q Anywhere',
    headline: 'Speak anywhere.<br>It just <em>types</em>.',
    sub: 'Eqho waits quietly in your system tray. Press Alt+Q in any Windows app — email, docs, chat, or code — talk naturally, and your words appear right at your cursor.',
    facts: ['100% offline', 'No account needed', 'No subscription', '15-day free trial'],
    stage: `
      <div style="display:flex;flex-direction:column;align-items:center;gap:22px;">
        <div class="doc-win">
          <div class="doc-bar">
            <div>Project Notes — Any Windows App</div>
            <div class="doc-win-btns"><span>&#8722;</span><span>&#9633;</span><span>&#215;</span></div>
          </div>
          <div class="doc-body">
            <div class="hotkey-pill">Press Alt + Q to start or stop dictating</div>
            <div class="doc-title">Weekly team update</div>
            <p class="doc-p">
              <span class="spoken">Everything ran smoothly in today's review. Eqho starts in a blink, stays calm in the tray, and turns your voice into text completely offline on your own PC.</span><span class="caret"></span>
            </p>
          </div>
        </div>
        <div class="overlay-stack" style="margin-top:-4px;">
          <div class="acrylic-box">
            <span class="dim">Listening…</span> "…and turns your voice into text completely offline on your own PC."
          </div>
          ${realPillHtml('dark', 360)}
          ${underbarHtml({ lang: 'EN', mode: 'Batch', pinned: true, timer: '0:14' })}
        </div>
      </div>`,
  },
  {
    id: '02-languages-underbar',
    theme: 'light',
    step: '02 · Quick Floating Controls',
    headline: 'Switch across <em>99 languages</em> without leaving your app',
    sub: 'Hover the glowing pill — or pin its control bar open — to switch languages mid-sentence, cancel silently, or toggle Batch and Live delivery. Zero focus stealing.',
    facts: ['99 languages with flags', 'One-click recent chips', 'Never steals focus'],
    stage: `
      <div style="display:flex;flex-direction:column;align-items:center;gap:14px;">
        ${realPillHtml('light', 360)}
        ${underbarHtml({ lang: 'EN', mode: 'Live', pinned: true, timer: '0:22', activeLang: true, light: true })}
        <div class="lang-popup light">
          <div class="lp-search">
            <span style="color:#85858F">⌕</span>
            <span>Search 99 languages…</span>
          </div>
          <div class="lp-row active">
            <div class="lp-row-l">${flagSvg('en')}<span>English</span></div>
            <span>✓</span>
          </div>
          <div class="lp-row">
            <div class="lp-row-l">${flagSvg('de')}<span>German</span></div>
            <span class="lp-tier">Hand-tuned</span>
          </div>
          <div class="lp-row">
            <div class="lp-row-l">${flagSvg('es')}<span>Spanish</span></div>
            <span class="lp-tier">Hand-tuned</span>
          </div>
          <div class="lp-row">
            <div class="lp-row-l">${flagSvg('ja')}<span>Japanese</span></div>
            <span class="lp-tier">Hand-tuned</span>
          </div>
          <div class="lp-row">
            <div class="lp-row-l">${flagSvg('fr')}<span>French</span></div>
            <span class="lp-tier">Hand-tuned</span>
          </div>
          <div class="lp-row">
            <div class="lp-row-l">${flagSvg('bg')}<span>Bulgarian</span></div>
            <span class="lp-tier">Strong</span>
          </div>
          <div class="lp-recents">
            <div class="lp-chip active">${flagSvg('en')}<span>EN</span></div>
            <div class="lp-chip">${flagSvg('de')}<span>DE</span></div>
            <div class="lp-chip">${flagSvg('es')}<span>ES</span></div>
            <div class="lp-chip">${flagSvg('fr')}<span>FR</span></div>
          </div>
        </div>
      </div>`,
  },
  {
    id: '03-general-workflow',
    theme: 'dark',
    step: '03 · Dictate Your Way',
    headline: 'Your pace: <em>Batch or Live</em>',
    sub: 'Paste a whole thought cleanly when you finish, or deliver words phrase by phrase as you speak. Choose your mic, Toggle or Hold-to-Talk, custom vocabulary, and voice commands.',
    facts: ['Batch & Live modes', 'Toggle or Hold-to-Talk', 'Custom vocabulary', 'Voice commands'],
    stage: `
      <div class="app-frame" style="width:940px;">
        <img src="${capDataUri('01b_general_clean_dark_940x560.png')}" alt="Eqho General Dashboard"/>
      </div>`,
  },
  {
    id: '04-gpu-cpu-models',
    theme: 'light',
    step: '04 · Fast Local Models',
    headline: 'Fast on your GPU — or <em>any laptop CPU</em>',
    sub: 'Download a speech model once and dictate offline forever. Accelerates automatically on NVIDIA, AMD, and Intel graphics cards via Vulkan — or runs fast on multi-core CPUs from 60 MB.',
    facts: ['NVIDIA · AMD · Intel Vulkan', 'Laptop CPU from 60 MB', 'Downloads once, stays offline'],
    stage: `
      <div style="position:relative;width:940px;height:760px;">
        <div class="app-frame" style="position:absolute;top:0;left:0;width:780px;z-index:1;">
          <img src="${capDataUri('03_models_dark_940x560.png')}" alt="Eqho Models Tab"/>
        </div>
        <div class="app-frame" style="position:absolute;bottom:0;right:0;width:670px;z-index:2;border-color:rgba(88,166,255,0.42);">
          <img src="${capDataUri('09_onboarding_step4_models_link.png')}" alt="Eqho Guided Model Picker"/>
        </div>
      </div>`,
  },
  {
    id: '05-history-settings',
    theme: 'dark',
    step: '05 · Private & Never Lost',
    headline: 'Private by design. <em>Never lose a word.</em>',
    sub: 'Searchable local History with Trash protection and instant failed-paste recovery keeps every dictation safe. Tune overlay transparency live on screen, volume ducking, and per-app paste rules.',
    facts: ['Audio never uploaded', 'Failed-paste recovery', 'Live overlay preview', 'Stays out of Win+V'],
    stage: `
      <div style="display:flex;flex-direction:column;align-items:center;">
        <div style="position:relative;width:920px;height:660px;">
          <div class="app-frame" style="position:absolute;top:0;left:0;width:740px;z-index:1;">
            <img src="${capDataUri('05_history_recovery_dark_940x560.png')}" alt="Eqho Local History and Recovery"/>
          </div>
          <div class="app-frame" style="position:absolute;bottom:0;right:0;width:660px;z-index:2;border-color:rgba(88,166,255,0.42);">
            <img src="${capDataUri('06b_settings_modal_overlay_light.png')}" alt="Eqho Focused Settings with Live Overlay Preview"/>
          </div>
        </div>
        <div class="priv-grid">
          <div class="priv-card"><div class="priv-k">100% Local Audio</div><div class="priv-v">Processed on your PC, never uploaded</div></div>
          <div class="priv-card"><div class="priv-k">Zero Telemetry</div><div class="priv-v">No accounts, tracking, or ads</div></div>
          <div class="priv-card"><div class="priv-k">Featherweight</div><div class="priv-v">~3.4 MB idle in your system tray</div></div>
        </div>
      </div>`,
  },
  {
    id: '06-setup-themes-trial',
    theme: 'light',
    step: '06 · Ready in 60 Seconds',
    headline: 'Guided setup. Light & dark. <em>Buy once.</em>',
    sub: 'A calm first-run setup checks your mic and lets you try Alt+Q right away, followed by an optional interactive tour. Try every feature free for 15 days — then keep it with a one-time purchase.',
    facts: ['Interactive guide', '15-day free trial', 'One-time purchase', 'No subscription'],
    stage: `
      <div style="position:relative;width:940px;height:750px;">
        <div class="app-frame" style="position:absolute;top:8px;left:0;width:750px;z-index:1;">
          <img src="${capDataUri('01_general_dark_940x560.png')}" alt="Eqho Obsidian Dark Theme with Interactive Guide"/>
        </div>
        <div class="app-frame" style="position:absolute;bottom:8px;right:0;width:750px;z-index:2;">
          <img src="${capDataUri('02b_general_clean_light_940x560.png')}" alt="Eqho Crisp Light Theme"/>
        </div>
      </div>`,
  },
];

const renderHtml = (s) => {
  const isDark = s.theme === 'dark';
  const factTags = s.facts
    .map((f) => `<span class="fact ${f.includes('trial') ? 'fact-accent' : ''}">${f}</span>`)
    .join('');
  return `<!doctype html><html><head><meta charset="utf-8"><style>${baseCss}</style></head>
    <body>
      <div class="slide ${s.theme}">
        <div class="copy">
          ${brandChip(isDark, s.step)}
          <h1>${s.headline}</h1>
          <p class="sub">${s.sub}</p>
          <div class="facts">${factTags}</div>
        </div>
        <div class="stage">${s.stage}</div>
      </div>
    </body></html>`;
};

const browser = await chromium.launch({ args: ['--allow-file-access-from-files'] });
const previewFiles = [];

for (const scale of [2, 1]) {
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 }, deviceScaleFactor: scale });
  for (const s of slides) {
    await page.setContent(renderHtml(s), { waitUntil: 'load' });
    await page.evaluate(() => document.fonts.ready);
    const file = join(out, `eqho-${s.id}-${scale === 2 ? '3840x2160' : '1920x1080'}.png`);
    await page.screenshot({ path: file });
    if (scale === 1) previewFiles.push([s.id, file]);
    console.log(file);
  }
  await page.close();
}

// Contact sheet for rapid visual review.
const sheet = await browser.newPage({ viewport: { width: 1660, height: 100 } });
const cells = previewFiles
  .map(([id, f]) => {
    const uri = `data:image/png;base64,${readFileSync(f).toString('base64')}`;
    return `<figure><img src="${uri}"><figcaption>${id}</figcaption></figure>`;
  })
  .join('');
await sheet.setContent(`<!doctype html><html><head><style>
  body{margin:0;padding:24px;background:#111317;font:18px "Segoe UI",sans-serif;color:#D8DCE4;display:grid;grid-template-columns:repeat(2,794px);gap:24px}
  figure{margin:0}img{width:794px;display:block;border-radius:8px;border:1px solid #2A2A30}figcaption{padding-top:8px;font-weight:600}
</style></head><body>${cells}</body></html>`, { waitUntil: 'load' });
await sheet.evaluate(() => Promise.all([...document.images].map((i) => i.decode())));
const sheetPath = join(out, 'contact-sheet.png');
await sheet.screenshot({ path: sheetPath, fullPage: true });
console.log(sheetPath);
await browser.close();
