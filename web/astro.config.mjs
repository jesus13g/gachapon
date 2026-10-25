import { defineConfig } from 'astro/config';
import sitemap from '@astrojs/sitemap';

// Set SITE_URL at build time to the real deployment origin (used for canonical, OG and sitemap).
const site = process.env.SITE_URL ?? 'https://jesus13g.github.io';
const base = process.env.SITE_BASE ?? '/gachapon';
// English home at the root: the sitemap's x-default, same URL the pages declare in <head>.
const xDefault = new URL(base.replace(/\/?$/, '/'), site).href;

export default defineConfig({
  site,
  base,
  trailingSlash: 'ignore',
  // English at the root, Spanish under /es/ (copy in src/content/copy.{en,es}.ts, switch in src/i18n.ts)
  i18n: { locales: ['en', 'es'], defaultLocale: 'en', routing: { prefixDefaultLocale: false } },
  integrations: [
    sitemap({
      i18n: { defaultLocale: 'en', locales: { en: 'en', es: 'es' } },
      // The 404 page is noindex: keep it out of the sitemap.
      filter: (page) => !/\/404\/?$/.test(page),
      // Mirror the pages' hreflang set: add x-default to the localized entries only.
      serialize(item) {
        if (item.links?.length && !item.links.some((l) => l.lang === 'x-default')) {
          item.links = [...item.links, { url: xDefault, lang: 'x-default' }];
        }
        return item;
      },
    }),
  ],
  // Inline all CSS so the first paint does not wait on a render-blocking stylesheet request.
  build: { inlineStylesheets: 'always' },
  vite: {
    build: { chunkSizeWarningLimit: 700 },
  },
});
