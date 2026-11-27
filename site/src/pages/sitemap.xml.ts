import type { APIRoute } from 'astro';
import { LANGS, LOCALIZED_PAGES, SITE, pagePath } from '../lib/i18n';

const LASTMOD = '2026-11-27';
const META: Record<string, { freq: string; prio: string }> = {
  '': { freq: 'monthly', prio: '1.0' },
  privacy: { freq: 'yearly', prio: '0.5' },
  changelog: { freq: 'weekly', prio: '0.6' },
};

function entry(loc: string, page: string, alts: string[]) {
  const m = META[page];
  const links = alts.length > 1
    ? alts.map((l) => `    <xhtml:link rel="alternate" hreflang="${l}" href="${SITE}${pagePath(l, page)}"/>`)
        .concat(`    <xhtml:link rel="alternate" hreflang="x-default" href="${SITE}${pagePath('en', page)}"/>`).join('\n') + '\n'
    : '';
  return `  <url>\n    <loc>${loc}</loc>\n${links}    <lastmod>${LASTMOD}</lastmod>\n    <changefreq>${m.freq}</changefreq>\n    <priority>${m.prio}</priority>\n  </url>`;
}

export const GET: APIRoute = () => {
  const urls: string[] = [];
  for (const page of LOCALIZED_PAGES) for (const l of LANGS) urls.push(entry(SITE + pagePath(l, page), page, LANGS));
  urls.push(entry(`${SITE}/changelog/`, 'changelog', ['en']));
  const body = `<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:xhtml="http://www.w3.org/1999/xhtml">
${urls.join('\n')}
</urlset>
`;
  return new Response(body, { headers: { 'Content-Type': 'application/xml; charset=utf-8' } });
};
