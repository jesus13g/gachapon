import { defineConfig } from 'astro/config';
import sitemap from '@astrojs/sitemap';

// Set SITE_URL at build time to the real deployment origin (used for canonical, OG and sitemap).
const site = process.env.SITE_URL ?? 'https://jesus13g.github.io';
const base = process.env.SITE_BASE ?? '/gachapon';

export default defineConfig({
  site,
  base,
  trailingSlash: 'ignore',
  // English at the root, Spanish under /es/ (copy in src/content/copy.{en,es}.ts, switch in src/i18n.ts)
  i18n: { locales: ['en', 'es'], defaultLocale: 'en', routing: { prefixDefaultLocale: false } },
  integrations: [sitemap({ i18n: { defaultLocale: 'en', locales: { en: 'en', es: 'es' } } })],
  build: { inlineStylesheets: 'auto' },
  vite: {
    build: { chunkSizeWarningLimit: 700 },
  },
});
