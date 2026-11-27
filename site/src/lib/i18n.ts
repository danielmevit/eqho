import en from '../i18n/en.json';

/** The 33 site languages, in menu order: tag, endonym, Open Graph locale. */
export const SITE_LANGS: { tag: string; name: string; og: string }[] = [
  ['en', 'English', 'en_US'], ['es', 'Español', 'es_ES'], ['zh-Hans', '简体中文', 'zh_CN'], ['ja', '日本語', 'ja_JP'],
  ['ko', '한국어', 'ko_KR'], ['vi', 'Tiếng Việt', 'vi_VN'], ['ar', 'العربية', 'ar_AR'], ['uk', 'Українська', 'uk_UA'],
  ['fr', 'Français', 'fr_FR'], ['de', 'Deutsch', 'de_DE'], ['pt-BR', 'Português (Brasil)', 'pt_BR'],
  ['ru', 'Русский', 'ru_RU'], ['it', 'Italiano', 'it_IT'], ['nl', 'Nederlands', 'nl_NL'], ['pl', 'Polski', 'pl_PL'],
  ['tr', 'Türkçe', 'tr_TR'], ['sv', 'Svenska', 'sv_SE'], ['da', 'Dansk', 'da_DK'], ['nb', 'Norsk bokmål', 'nb_NO'],
  ['fi', 'Suomi', 'fi_FI'], ['cs', 'Čeština', 'cs_CZ'], ['ro', 'Română', 'ro_RO'], ['hu', 'Magyar', 'hu_HU'],
  ['el', 'Ελληνικά', 'el_GR'], ['bg', 'Български', 'bg_BG'], ['hr', 'Hrvatski', 'hr_HR'], ['sk', 'Slovenčina', 'sk_SK'],
  ['ca', 'Català', 'ca_ES'], ['hi', 'हिन्दी', 'hi_IN'], ['id', 'Bahasa Indonesia', 'id_ID'],
  ['ms', 'Bahasa Melayu', 'ms_MY'], ['th', 'ไทย', 'th_TH'], ['he', 'עברית', 'he_IL'],
].map(([tag, name, og]) => ({ tag, name, og }));

export const DEFAULT_LANG = 'en';
export const SITE = 'https://eqho.damt.xyz';
export type Lang = string;

// Every src/i18n/<tag>.json is picked up automatically; adding a file is enough to build that language.
const files = import.meta.glob<Record<string, string>>('../i18n/*.json', { eager: true, import: 'default' });
const TABLES: Record<string, Record<string, string>> = { en };
for (const [p, table] of Object.entries(files)) {
  const tag = p.split('/').pop()!.replace(/\.json$/, '');
  if (tag === 'en') continue;
  if (!SITE_LANGS.some((l) => l.tag === tag)) {
    throw new Error(`src/i18n/${tag}.json: "${tag}" is not a site language. Use one of: ${SITE_LANGS.map((l) => l.tag).join(', ')}`);
  }
  TABLES[tag] = table;
}

/** English first, then the other languages that have a JSON file, in SITE_LANGS order. */
export const LANGS: string[] = SITE_LANGS.map((l) => l.tag).filter((tag) => tag in TABLES);

export function languageEndonym(lang: Lang): string {
  return SITE_LANGS.find((l) => l.tag === lang)?.name ?? lang;
}
export function ogLocaleFor(lang: Lang): string {
  return SITE_LANGS.find((l) => l.tag === lang)?.og ?? 'en_US';
}

const RTL = new Set(['ar', 'he']);

/** Translated string with `{name}` interpolation; falls back to English, then throws. */
export function t(lang: Lang, key: string, vars?: Record<string, string | number>): string {
  const raw = TABLES[lang]?.[key] ?? TABLES[DEFAULT_LANG][key];
  if (raw === undefined) throw new Error(`Missing i18n key "${key}"`);
  return vars ? raw.replace(/\{(\w+)\}/g, (m, n) => (n in vars ? String(vars[n]) : m)) : raw;
}

/** Splits a string around one `{placeholder}` so markup (a link) can sit in the middle. */
export function tSplit(lang: Lang, key: string, name = 'link'): [string, string] {
  const s = t(lang, key);
  const i = s.indexOf(`{${name}}`);
  return i < 0 ? [s, ''] : [s.slice(0, i), s.slice(i + name.length + 2)];
}

function esc(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

/**
 * Safe inline markup: the text is HTML-escaped, then only **bold** and `code` become tags.
 * `links` fills `{name}` placeholders with <a> elements (text escaped too).
 */
export function tRich(lang: Lang, key: string, links: Record<string, { href: string; text: string }> = {}): string {
  let s = esc(t(lang, key));
  s = s.replace(/`([^`]+)`/g, '<code>$1</code>').replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
  return s.replace(/\{(\w+)\}/g, (m, n) => (links[n] ? `<a href="${esc(links[n].href)}">${esc(links[n].text)}</a>` : m));
}

export function dir(lang: Lang): 'rtl' | 'ltr' {
  return RTL.has(lang.split('-')[0]) ? 'rtl' : 'ltr';
}

/** Page path for a language: `/`, `/privacy/` for English; `/de/`, `/pt-BR/privacy/` for others. */
export function pagePath(lang: Lang, page = ''): string {
  const p = page ? `${page.replace(/^\/|\/$/g, '')}/` : '';
  return lang === DEFAULT_LANG ? `/${p}` : `/${lang}/${p}`;
}

/** Pages that exist in every language (the changelog stays English only). */
export const LOCALIZED_PAGES = ['', 'privacy'];

export function otherLangPaths() {
  return LANGS.filter((l) => l !== DEFAULT_LANG).map((lang) => ({ params: { lang }, props: { lang } }));
}

/** Flag file (public/assets/flags/<code>.svg) for a site language tag, using the app's flag set. */
export function flagFor(lang: Lang): string {
  const map: Record<string, string> = { 'zh-Hans': 'zh', nb: 'no', 'pt-BR': 'br' };
  return `/assets/flags/${map[lang] ?? lang}.svg`;
}

/** Short tag for compact buttons: "EN", "ZH", "PT". */
export function shortTag(lang: Lang): string {
  return lang.split('-')[0].toUpperCase();
}
