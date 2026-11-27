// Hero replay of one Live dictation, ~11 s loop. Without JS or with reduced motion the final state shows.
const LOOP = 11000, KEY1 = 1200, PILL_IN = 1400, SPEAK = 1600, SPEAK_END = 7500, KEY2 = 7800, CLEAR = 10500;

function phrases(text: string, lang: string): string[] {
  let segs: { s: string; word: boolean }[];
  try {
    const seg = new (Intl as any).Segmenter(lang, { granularity: 'word' });
    segs = Array.from(seg.segment(text) as Iterable<{ segment: string; isWordLike: boolean }>).map((x) => ({ s: x.segment, word: x.isWordLike }));
  } catch (e) {
    segs = text.split(/(\s+)/).filter(Boolean).map((s) => ({ s, word: /\S/.test(s) }));
  }
  const out: string[] = [];
  let cur = '', words = 0, closeNext = false;
  for (const g of segs) {
    if (closeNext && g.word) { out.push(cur); cur = ''; words = 0; closeNext = false; }
    cur += g.s;
    if (g.word) words++;
    else if (/[.,;:!?。、，！？]/.test(g.s) && words >= 2) closeNext = true;
    if (words >= 4 && !closeNext) closeNext = true;
  }
  if (cur) out.push(cur);
  return out;
}

function run() {
  const stage = document.getElementById('demo');
  const note = document.getElementById('note');
  const timer = document.getElementById('ub-timer');
  const toast = document.getElementById('keytoast');
  if (!stage || !note || !timer || !toast) return;
  const full = note.dataset.text || '';
  const lang = note.dataset.lang || document.documentElement.lang || 'en';
  const parts = phrases(full, lang);
  const gap = Math.min(800, Math.max(550, (SPEAK_END - SPEAK) / Math.max(1, parts.length)));
  const reduced = matchMedia('(prefers-reduced-motion: reduce)');
  let t0 = 0, timers: number[] = [], tick = 0;

  const caret = () => { const c = document.createElement('span'); c.className = 'caret'; return c; };
  const showFinal = () => {
    stage.classList.remove('live', 'pill-on', 'toast-on', 'toast-press');
    note.textContent = full; note.appendChild(caret()); timer.textContent = '0:06';
  };
  const later = (ms: number, fn: () => void) => { timers.push(window.setTimeout(fn, ms)); };
  const clearAll = () => { timers.forEach(clearTimeout); timers = []; clearInterval(tick); };
  const press = (at: number) => {
    later(at, () => stage.classList.add('toast-on', 'toast-press'));
    later(at + 150, () => stage.classList.remove('toast-press'));
    later(at + 900, () => stage.classList.remove('toast-on'));
  };

  (window as any).EqhoDemo = {
    speakingAt(): number | null {
      if (!t0) return reduced.matches ? null : null;
      const t = performance.now() - t0;
      return t >= SPEAK && t < SPEAK_END ? (t - SPEAK) / 1000 : null;
    },
  };

  const loop = () => {
    clearAll();
    t0 = performance.now();
    stage.classList.add('live'); stage.classList.remove('pill-on');
    note.textContent = '';
    const c = caret(); note.appendChild(c);
    timer.textContent = '0:00';
    press(KEY1);
    later(PILL_IN, () => {
      stage.classList.add('pill-on');
      const start = performance.now();
      tick = window.setInterval(() => {
        const s = Math.floor((performance.now() - start) / 1000);
        timer.textContent = `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
      }, 250);
    });
    parts.forEach((p, i) => later(SPEAK + i * gap, () => {
      const span = document.createElement('span'); span.className = 'ph'; span.textContent = p;
      note.insertBefore(span, c);
    }));
    press(KEY2);
    later(KEY2 + 100, () => { stage.classList.remove('pill-on'); clearInterval(tick); });
    later(CLEAR, () => { note.textContent = ''; note.appendChild(c); });
    later(LOOP, loop);
  };

  const start = () => { clearAll(); t0 = 0; if (reduced.matches) showFinal(); else loop(); };
  reduced.addEventListener?.('change', start);
  start();
}

run();
