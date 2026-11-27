// Fails if a t()/tSplit() key used in src/ is missing from en.json, or an en.json key is unused.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');
const en = JSON.parse(fs.readFileSync(path.join(root, 'src/i18n/en.json'), 'utf8'));
const files = [];
(function walk(d) {
  for (const e of fs.readdirSync(d, { withFileTypes: true })) {
    const p = path.join(d, e.name);
    if (e.isDirectory()) { if (e.name !== 'i18n') walk(p); }
    else if (/\.(astro|ts|mjs|js)$/.test(e.name)) files.push(p);
  }
})(path.join(root, 'src'));

const used = new Set();
const re = /\b(?:t|tSplit)\(\s*[\w.]+\s*,\s*[`'"]([^`'"$]+)[`'"]/g;
// Keys built in loops are declared with a `// i18n-keys: a b c` comment.
const decl = /i18n-keys:\s*([^\n*]+)/g;
for (const f of files) {
  const src = fs.readFileSync(f, 'utf8');
  for (const m of src.matchAll(re)) used.add(m[1]);
  for (const m of src.matchAll(decl)) m[1].trim().split(/\s+/).forEach((k) => used.add(k));
}
const missing = [...used].filter((k) => !(k in en));
const unused = Object.keys(en).filter((k) => !used.has(k));
if (missing.length) console.error('Missing from en.json:', missing.join(', '));
if (unused.length) console.error('Unused in en.json:', unused.join(', '));
let bad = missing.length > 0 || unused.length > 0;
if (!bad) console.log(`check-i18n: en ${Object.keys(en).length} keys, ${used.size} used in src/, none missing or unused.`);

// Every other language must mirror en.json: same keys, same {placeholders}, same ** and ` markup counts.
const ph = (s) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join(',');
const count = (s, m) => s.split(m).length - 1;
const dir = path.join(root, 'src/i18n');
for (const f of fs.readdirSync(dir).filter((n) => n.endsWith('.json') && n !== 'en.json').sort()) {
  const tag = f.replace(/\.json$/, '');
  const tr = JSON.parse(fs.readFileSync(path.join(dir, f), 'utf8'));
  const errs = [];
  for (const k of Object.keys(en)) {
    if (!(k in tr)) { errs.push(`missing ${k}`); continue; }
    if (typeof tr[k] !== 'string') { errs.push(`${k}: not a string`); continue; }
    if (ph(tr[k]) !== ph(en[k])) errs.push(`${k}: placeholders {${ph(tr[k])}} vs en {${ph(en[k])}}`);
    if (count(tr[k], '**') !== count(en[k], '**')) errs.push(`${k}: ** count differs`);
    if (count(tr[k], '`') !== count(en[k], '`')) errs.push(`${k}: backtick count differs`);
  }
  for (const k of Object.keys(tr)) if (!(k in en)) errs.push(`extra ${k}`);
  if (errs.length) { bad = true; console.error(`${tag}: ${errs.length} problem(s)\n  ` + errs.slice(0, 40).join('\n  ')); }
  else console.log(`${tag}: ok (${Object.keys(tr).length} keys)`);
}
if (bad) process.exit(1);
