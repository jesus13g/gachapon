import { defineConfig } from 'astro/config';
import sitemap from '@astrojs/sitemap';

// Set SITE_URL at build time to the real deployment origin (used for canonical, OG and sitemap).
const site = process.env.SITE_URL ?? 'https://jesus13g.github.io';
const base = process.env.SITE_BASE ?? '/gachapon';

export default defineConfig({
  site,
  base,
  trailingSlash: 'ignore',
  integrations: [sitemap()],
  build: { inlineStylesheets: 'auto' },
  vite: {
    build: { chunkSizeWarningLimit: 700 },
  },
});
