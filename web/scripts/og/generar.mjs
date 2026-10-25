/**
 * Builds the share cards and raster icons in public/ from the real copy and the favicon.
 *
 *   npm run og        (from web/; needs Node ≥ 23 to import the .ts copy, plus chromium, rsvg-convert and magick)
 *
 * Outputs:
 *   public/og.png, public/og-es.png      1200×630 Open Graph / X cards, one per language
 *   public/apple-touch-icon.png          180×180, opaque #090b0e background
 *   public/favicon-32.png                32×32
 *
 * Texts come from src/content/copy.{en,es}.ts (site.name, ui.hero.h1/h1em/subStrong), so the card always
 * matches the hero. Override the browser with CHROMIUM=/path/to/chromium.
 */
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const web = resolve(here, '../..');
const pub = join(web, 'public');
const chromium = process.env.CHROMIUM || '/usr/bin/chromium';
const bg = '#090b0e';

const font = (pkg, file) => pathToFileURL(join(web, 'node_modules', pkg, 'files', file)).href;
const fonts = {
  fontDisplay800: font('@fontsource/big-shoulders-display', 'big-shoulders-display-latin-800-normal.woff2'),
  fontDisplay900: font('@fontsource/big-shoulders-display', 'big-shoulders-display-latin-900-normal.woff2'),
  fontSans: font('@fontsource-variable/inter-tight', 'inter-tight-latin-wght-normal.woff2'),
};

const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const template = readFileSync(join(here, 'plantilla.html'), 'utf8');
const favicon = readFileSync(join(pub, 'favicon.svg'), 'utf8');

const cards = [
  { lang: 'en', out: 'og.png' },
  { lang: 'es', out: 'og-es.png' },
];

const work = mkdtempSync(join(tmpdir(), 'gachapon-og-'));
try {
  for (const { lang, out } of cards) {
    const { site, ui } = await import(pathToFileURL(join(web, 'src/content', `copy.${lang}.ts`)).href);
    const values = {
      ...fonts,
      lang: site.lang,
      icon: favicon, // trusted local SVG, inlined as markup
      name: esc(site.name),
      h1: esc(ui.hero.h1),
      h1em: esc(ui.hero.h1em),
      support: esc(ui.hero.subStrong),
      h1Size: '92',
    };
    const html = template.replace(/\{\{(\w+)\}\}/g, (_, k) => {
      if (!(k in values)) throw new Error(`plantilla.html: unknown placeholder {{${k}}}`);
      return values[k];
    });
    const page = join(work, `${lang}.html`);
    const shot = join(work, `${lang}.png`);
    writeFileSync(page, html);
    execFileSync(chromium, [
      '--headless',
      '--disable-gpu',
      '--hide-scrollbars',
      '--force-device-scale-factor=1',
      '--virtual-time-budget=2000', // let the local fonts load before the capture
      '--window-size=1200,630',
      `--screenshot=${shot}`,
      pathToFileURL(page).href,
    ], { stdio: 'ignore' });
    // Drop metadata/alpha and recompress; the card is flat colour, so this stays well under 300 KB.
    const dest = join(pub, out);
    execFileSync('magick', [shot, '-strip', '-background', bg, '-alpha', 'remove', '-alpha', 'off',
      '-define', 'png:compression-level=9', dest]);
    console.log(`${out}  ${Math.round(statSync(dest).size / 1024)} KB`);
  }

  // Icons from the favicon: the touch icon gets an opaque background and some margin around the art.
  const fav = join(pub, 'favicon.svg');
  const big = join(work, 'icon-148.png');
  execFileSync('rsvg-convert', ['-w', '148', '-h', '148', '-o', big, fav]);
  execFileSync('magick', [big, '-background', bg, '-gravity', 'center', '-extent', '180x180',
    '-alpha', 'remove', '-alpha', 'off', '-strip', join(pub, 'apple-touch-icon.png')]);
  execFileSync('rsvg-convert', ['-w', '32', '-h', '32', '-o', join(pub, 'favicon-32.png'), fav]);
  execFileSync('magick', [join(pub, 'favicon-32.png'), '-strip', join(pub, 'favicon-32.png')]);
  console.log('apple-touch-icon.png, favicon-32.png');
} finally {
  rmSync(work, { recursive: true, force: true });
}
