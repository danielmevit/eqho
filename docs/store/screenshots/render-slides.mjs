// Renders 6 Microsoft Store presentation slides for Eqho (3840x2160 + 1920x1080 + contact-sheet.png).
// Zero GitHub URLs, zero competitor names, calm Deep Navy Blend (#1A489B / #58A6FF) + Crisp Light rhythm.
//
// Usage:
//   NODE_PATH="/mnt/d/Vibe Coding/glitch-lab/node_modules" node docs/store/screenshots/render-slides.mjs \
//     --caps <0.9.0-captures-dir> --out <output-dir>
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

const brandChip = (dark) => {
  const svgSmall = markSvg.replace(/width="512" height="512"/, 'width="32" height="32"');
  return `<div class="brand-row">
    <div class="brand-tile ${dark ? 'dark' : 'light'}">${svgSmall}</div>
    <span class="brand-name ${dark ? 'dark' : 'light'}">Eqho</span>
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
    it: `<rect width="7" height="14" fill="#168A42"/><rect x="7" width="7" height="14" fill="#F8FAFC"/><rect x="14" width="7" height="14" fill="#D82A2E"/>`,
    pt: `<rect width="8.5" height="14" fill="#0E6034"/><rect x="8.5" width="12.5" height="14" fill="#D82A2E"/>`,
    uk: `<rect width="21" height="7" fill="#38A5E0"/><rect y="7" width="21" height="7" fill="#F6CA18"/>`,
  };
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 21 14" width="24" height="16" style="border-radius:3px;box-shadow:0 0 0 1px rgba(255,255,255,0.16);flex-shrink:0">${maps[code] || maps.en}</svg>`;
};

const iridescentPill = (w = 340, h = 62) => `
  <div class="eqho-pill" style="width:${w}px;height:${h}px">
    <svg viewBox="0 0 340 62" width="${w}" height="${h}" style="display:block">
      <defs>
        <linearGradient id="pillBg" x1="0%" y1="0%" x2="100%" y2="100%">
          <stop offset="0%" stop-color="#0E182B"/>
          <stop offset="50%" stop-color="#132549"/>
          <stop offset="100%" stop-color="#0B1324"/>
        </linearGradient>
        <linearGradient id="waveA" x1="0%" y1="0%" x2="100%" y2="0%">
          <stop offset="0%" stop-color="#1A489B" stop-opacity="0.1"/>
          <stop offset="30%" stop-color="#38A5E0" stop-opacity="0.9"/>
          <stop offset="55%" stop-color="#8FD3FF" stop-opacity="1"/>
          <stop offset="80%" stop-color="#58A6FF" stop-opacity="0.85"/>
          <stop offset="100%" stop-color="#1A489B" stop-opacity="0.1"/>
        </linearGradient>
        <linearGradient id="waveB" x1="0%" y1="0%" x2="100%" y2="0%">
          <stop offset="0%" stop-color="#58A6FF" stop-opacity="0.05"/>
          <stop offset="40%" stop-color="#58A6FF" stop-opacity="0.7"/>
          <stop offset="65%" stop-color="#C9E7FF" stop-opacity="0.9"/>
          <stop offset="100%" stop-color="#1A489B" stop-opacity="0.05"/>
        </linearGradient>
      </defs>
      <rect x="1" y="1" width="338" height="60" rx="30" fill="url(#pillBg)" stroke="rgba(130,190,255,0.45)" stroke-width="1.5"/>
      <path d="M 24 34 Q 68 20, 112 31 T 192 27 T 268 35 T 316 30" fill="none" stroke="url(#waveA)" stroke-width="3.4" stroke-linecap="round"/>
      <path d="M 32 31 Q 84 44, 134 29 T 218 36 T 308 31" fill="none" stroke="url(#waveB)" stroke-width="2.2" stroke-linecap="round"/>
      <path d="M 50 32 Q 110 14, 170 33 T 290 31" fill="none" stroke="rgba(255,255,255,0.45)" stroke-width="1.2" stroke-linecap="round"/>
    </svg>
  </div>`;

const underbarHtml = ({ lang = 'EN', flag = 'en', mode = 'Batch', pinned = true, timer = '0:14', activeLang = false } = {}) => `
  <div class="underbar">
    <div class="ub-btn ub-muted" title="Cancel">✕</div>
    <div class="ub-div"></div>
    <div class="ub-timer">${timer}</div>
    <div class="ub-chip ${activeLang ? 'active' : ''}">${flagSvg(flag)}<span>${lang}</span><span class="ub-chev">▾</span></div>
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
    display: flex; align-items: center; padding: 92px 104px; gap: 76px;
  }
  .slide.navy {
    background: radial-gradient(960px 760px at 78% 42%, rgba(88,166,255,0.22), transparent 68%),
                linear-gradient(135deg, #1E54B5 0%, #163E87 38%, #0E234E 74%, #081226 100%);
    color: #F5F6F8;
  }
  .slide.light {
    background: radial-gradient(960px 760px at 76% 45%, rgba(9,105,218,0.09), transparent 68%),
                linear-gradient(135deg, #F7FAFF 0%, #EEF3FA 55%, #E5EDF8 100%);
    color: #16181D;
  }
  .copy { width: 700px; flex-shrink: 0; z-index: 5; }
  .brand-row { display: flex; align-items: center; gap: 16px; margin-bottom: 36px; }
  .brand-tile {
    width: 56px; height: 56px; border-radius: 15px; display: grid; place-items: center;
  }
  .brand-tile.dark {
    background: linear-gradient(180deg,#18191D 0%,#101114 100%);
    border: 1px solid rgba(88,166,255,0.34);
    box-shadow: 0 12px 28px rgba(4,10,24,0.35);
  }
  .brand-tile.light {
    background: #FFFFFF;
    border: 1px solid rgba(9,105,218,0.18);
    box-shadow: 0 10px 24px rgba(15,35,75,0.10);
  }
  .brand-name { font-size: 32px; font-weight: 600; letter-spacing: -0.02em; }
  .brand-name.dark { color: #FFFFFF; }
  .brand-name.light { color: #16181D; }

  h1 {
    font-size: 76px; font-weight: 700; line-height: 1.07; letter-spacing: -0.03em;
    margin: 0 0 28px;
  }
  .slide.navy h1 em { font-style: normal; color: #79C0FF; }
  .slide.light h1 em { font-style: normal; color: #0969DA; }

  .sub {
    font-size: 30px; line-height: 1.44; font-weight: 400; margin: 0 0 38px;
    max-width: 650px;
  }
  .slide.navy .sub { color: rgba(245,246,248,0.84); }
  .slide.light .sub { color: #4A5160; }

  .facts { display: flex; flex-wrap: wrap; gap: 14px; }
  .fact {
    font-size: 21px; font-weight: 600; padding: 11px 22px; border-radius: 999px;
    display: inline-flex; align-items: center; gap: 10px;
  }
  .slide.navy .fact {
    background: rgba(255,255,255,0.10); border: 1px solid rgba(255,255,255,0.18); color: #FFFFFF;
  }
  .slide.light .fact {
    background: #FFFFFF; border: 1px solid #D4DCE8; color: #1A1C1F;
    box-shadow: 0 4px 12px rgba(15,35,75,0.05);
  }
  .fact-accent {
    background: #1A489B !important; border-color: rgba(88,166,255,0.5) !important; color: #FFFFFF !important;
  }

  .stage {
    position: relative; flex: 1; height: 880px;
    display: flex; flex-direction: column; align-items: center; justify-content: center;
  }

  /* Framed app screenshots */
  .app-frame {
    border-radius: 18px; overflow: hidden; background: #111215;
    border: 1px solid rgba(255,255,255,0.14);
    box-shadow: 0 36px 90px rgba(4,10,24,0.52), 0 0 0 1px rgba(88,166,255,0.14);
  }
  .slide.light .app-frame {
    border: 1px solid rgba(15,35,75,0.14);
    box-shadow: 0 32px 76px rgba(15,35,75,0.18);
  }
  .app-frame img { display: block; width: 100%; height: auto; }

  /* Mock desktop document on Slide 1 */
  .doc-win {
    width: 880px; border-radius: 20px; background: #13151A;
    border: 1px solid rgba(255,255,255,0.14);
    box-shadow: 0 40px 100px rgba(3,8,20,0.58);
    overflow: hidden;
  }
  .doc-bar {
    height: 46px; background: #1A1C22; border-bottom: 1px solid #2A2A30;
    display: flex; align-items: center; justify-content: space-between; padding: 0 20px;
    color: #9A9EA8; font-size: 16px; font-weight: 500;
  }
  .doc-win-btns { display: flex; gap: 18px; color: #7C818D; font-size: 16px; line-height: 1; }
  .doc-body { padding: 38px 44px 48px; min-height: 360px; }
  .doc-title { font-size: 28px; font-weight: 700; color: #FFFFFF; margin: 0 0 20px; }
  .doc-p { font-size: 24px; line-height: 1.62; color: #D8DCE4; margin: 0 0 18px; }
  .doc-p .spoken { color: #FFFFFF; }
  .caret {
    display: inline-block; width: 3px; height: 28px; background: #58A6FF;
    vertical-align: -5px; margin-left: 3px; border-radius: 2px;
  }
  .hotkey-pill {
    display: inline-flex; align-items: center; gap: 10px;
    margin-bottom: 24px; padding: 8px 16px; border-radius: 10px;
    background: rgba(88,166,255,0.12); border: 1px solid rgba(88,166,255,0.35);
    color: #79C0FF; font-size: 17px; font-weight: 600;
  }

  /* Pill + underbar */
  .overlay-stack { display: flex; flex-direction: column; align-items: center; gap: 12px; }
  .eqho-pill { filter: drop-shadow(0 18px 34px rgba(5,16,38,0.5)); }
  .underbar {
    display: inline-flex; align-items: center; gap: 8px; height: 46px; padding: 0 8px;
    border-radius: 999px; background: rgba(21,21,23,0.96); border: 1px solid #2A2A30;
    box-shadow: 0 18px 44px rgba(4,10,24,0.45); color: #F5F6F8; font-size: 18px; font-weight: 600;
  }
  .ub-btn {
    width: 34px; height: 34px; border-radius: 50%; display: grid; place-items: center;
    font-size: 17px;
  }
  .ub-muted { color: #8C909C; }
  .ub-pin-on { color: #58A6FF; }
  .ub-div { width: 1px; height: 22px; background: #2A2A30; }
  .ub-timer { color: #9A9EA8; font-variant-numeric: tabular-nums; padding: 0 8px; font-weight: 500; }
  .ub-chip {
    display: inline-flex; align-items: center; gap: 8px; height: 34px; padding: 0 12px;
    border-radius: 999px; color: #F5F6F8;
  }
  .ub-chip.active { background: #26262D; color: #58A6FF; }
  .ub-chip.mode { color: #58A6FF; }
  .ub-chev { font-size: 14px; color: #8C909C; }
  .ub-done {
    width: 40px; height: 34px; border-radius: 999px; background: #1A489B; color: #FFFFFF;
    display: grid; place-items: center; font-size: 19px; font-weight: 700;
  }

  /* Language popup card (Slide 2) */
  .lang-popup {
    width: 460px; border-radius: 18px; background: #151517; border: 1px solid #2A2A30;
    box-shadow: 0 32px 80px rgba(10,24,54,0.30); padding: 16px; color: #F5F6F8;
  }
  .lp-search {
    height: 46px; border-radius: 10px; background: #1C1C21; border: 1px solid #58A6FF;
    display: flex; align-items: center; gap: 12px; padding: 0 14px; font-size: 19px; color: #F5F6F8;
    margin-bottom: 10px;
  }
  .lp-row {
    height: 48px; border-radius: 10px; padding: 0 12px; display: flex; align-items: center;
    justify-content: space-between; font-size: 20px; color: #E2E5EC;
  }
  .lp-row.active { background: #222229; color: #58A6FF; font-weight: 600; }
  .lp-row-l { display: flex; align-items: center; gap: 14px; }
  .lp-tier { font-size: 13px; color: #85858F; font-weight: 500; }
  .lp-recents {
    margin-top: 12px; padding-top: 12px; border-top: 1px solid #2A2A30;
    display: flex; gap: 8px;
  }
  .lp-chip {
    flex: 1 1 0; height: 40px; border-radius: 9px; background: #1E1E24; border: 1px solid #2A2A30;
    display: flex; align-items: center; justify-content: center; gap: 8px;
    font-size: 17px; font-weight: 600; color: #F5F6F8;
  }
  .lp-chip.active { border-color: #58A6FF; color: #58A6FF; }

  /* Privacy metric cards (Slide 5) */
  .priv-grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 18px; width: 880px; margin-top: 22px; }
  .priv-card {
    background: rgba(17,19,24,0.88); border: 1px solid rgba(88,166,255,0.24);
    border-radius: 14px; padding: 18px 20px;
  }
  .priv-k { font-size: 15px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.06em; color: #79C0FF; margin-bottom: 6px; }
  .priv-v { font-size: 20px; font-weight: 600; color: #FFFFFF; line-height: 1.3; }
`;

const slides = [
  {
    id: '01-hero',
    theme: 'navy',
    headline: 'Speak anywhere.<br>It just <em>types</em>.',
    sub: 'Press Alt+Q, talk naturally, and your words appear in any Windows app. 100% offline on your own PC.',
    facts: ['100% offline', '99 languages', 'No account', '15-day free trial'],
    stage: `
      <div style="display:flex;flex-direction:column;align-items:center;gap:28px;">
        <div class="doc-win">
          <div class="doc-bar">
            <div>Project Notes — Any Windows App</div>
            <div class="doc-win-btns"><span>&#8722;</span><span>&#9633;</span><span>&#215;</span></div>
          </div>
          <div class="doc-body">
            <div class="hotkey-pill">Press Alt + Q to dictate</div>
            <div class="doc-title">Weekly product update</div>
            <p class="doc-p">
              <span class="spoken">Everything ran smoothly in today's test. The new release starts instantly, stays quiet in the tray, and transcribes locally on your PC without touching the cloud.</span><span class="caret"></span>
            </p>
          </div>
        </div>
        <div class="overlay-stack" style="margin-top:-10px;">
          ${iridescentPill(360, 64)}
          ${underbarHtml({ lang: 'EN', flag: 'en', mode: 'Batch', pinned: true, timer: '0:14' })}
        </div>
      </div>`,
  },
  {
    id: '02-languages-underbar',
    theme: 'light',
    headline: 'Switch languages <em>without leaving</em> your app',
    sub: 'Hover the listening pill — or pin the bar open — for instant access to 99 languages, silent cancel, and Batch or Live mode. Zero focus stealing.',
    facts: ['99 languages with vector flags', 'Recent-language chips', 'Never steals focus'],
    stage: `
      <div style="display:flex;flex-direction:column;align-items:center;gap:18px;">
        ${iridescentPill(340, 60)}
        ${underbarHtml({ lang: 'EN', flag: 'en', mode: 'Live', pinned: true, timer: '0:22', activeLang: true })}
        <div class="lang-popup">
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
    id: '03-batch-live',
    theme: 'navy',
    headline: 'Your pace: <em>Batch or Live</em>',
    sub: 'Paste a whole thought cleanly when you stop, or deliver text phrase by phrase as you speak. Add custom vocabulary for names and technical terms.',
    facts: ['Batch & Live modes', 'Toggle or Hold-to-Talk', 'Private clipboard paste'],
    stage: `
      <div class="app-frame" style="width:920px;">
        <img src="${capDataUri('01_general_dark_940x560.png')}" alt="Eqho General Dashboard"/>
      </div>`,
  },
  {
    id: '04-gpu-cpu-models',
    theme: 'light',
    headline: 'Fast on your GPU — or <em>any laptop CPU</em>',
    sub: 'Vulkan acceleration across NVIDIA, AMD, and Intel graphics cards, plus compact Q5 models starting at 60 MB for fast multi-core CPU dictation.',
    facts: ['Vulkan GPU (NVIDIA / AMD / Intel)', 'Multi-core CPU + Q5 models', 'Downloads once, stays offline'],
    stage: `
      <div style="position:relative;width:920px;height:760px;">
        <div class="app-frame" style="position:absolute;top:0;left:0;width:780px;z-index:1;">
          <img src="${capDataUri('03_models_dark_940x560.png')}" alt="Eqho Models Tab"/>
        </div>
        <div class="app-frame" style="position:absolute;bottom:0;right:0;width:680px;z-index:2;border-color:rgba(88,166,255,0.4);">
          <img src="${capDataUri('09_onboarding_step4_models_link.png')}" alt="Eqho Model Picker"/>
        </div>
      </div>`,
  },
  {
    id: '05-privacy-controls',
    theme: 'navy',
    headline: 'Private by design, <em>always on your PC</em>',
    sub: 'No cloud servers, no subscriptions, no accounts. Customize overlay position, volume ducking, voice commands, and per-app paste rules.',
    facts: ['~3.4 MB idle RAM', 'Audio never saved or uploaded', 'Stays out of Win+V history'],
    stage: `
      <div style="display:flex;flex-direction:column;align-items:center;">
        <div class="app-frame" style="width:880px;">
          <img src="${capDataUri('06_settings_dark_940x560.png')}" alt="Eqho Settings"/>
        </div>
        <div class="priv-grid">
          <div class="priv-card"><div class="priv-k">Local Audio</div><div class="priv-v">Processed in RAM, never uploaded</div></div>
          <div class="priv-card"><div class="priv-k">Zero Telemetry</div><div class="priv-v">No tracking, ads, or accounts</div></div>
          <div class="priv-card"><div class="priv-k">Featherweight</div><div class="priv-v">~3.4 MB idle in system tray</div></div>
        </div>
      </div>`,
  },
  {
    id: '06-themes-trial',
    theme: 'light',
    headline: 'Light, dark, and <em>ready in a minute</em>',
    sub: 'Follows Windows automatically or lets you pick Deep Navy dark mode or crisp light mode. Try every feature free for 15 days — then buy once.',
    facts: ['15-day free trial', 'One-time purchase', 'No subscription'],
    stage: `
      <div style="position:relative;width:940px;height:740px;">
        <div class="app-frame" style="position:absolute;top:10px;left:0;width:740px;z-index:1;">
          <img src="${capDataUri('01_general_dark_940x560.png')}" alt="Eqho Dark Theme"/>
        </div>
        <div class="app-frame" style="position:absolute;bottom:10px;right:0;width:740px;z-index:2;">
          <img src="${capDataUri('02_general_light_940x560.png')}" alt="Eqho Light Theme"/>
        </div>
      </div>`,
  },
];

const renderHtml = (s) => {
  const isDark = s.theme === 'navy';
  const factTags = s.facts
    .map((f) => `<span class="fact ${f.includes('trial') ? 'fact-accent' : ''}">${f}</span>`)
    .join('');
  return `<!doctype html><html><head><meta charset="utf-8"><style>${baseCss}</style></head>
    <body>
      <div class="slide ${s.theme}">
        <div class="copy">
          ${brandChip(isDark)}
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
