/**
 * Language switch: English lives at the root, Spanish under /es/ (Astro i18n routing, see astro.config.mjs).
 * Components read their copy through copyFor(Astro.currentLocale); SVG labels go through tr(), keyed by the English text.
 */
import * as en from './content/copy.en';
import * as es from './content/copy.es';

export const langs = ['en', 'es'] as const;
export type Lang = (typeof langs)[number];

const copies = { en, es } satisfies Record<Lang, typeof en>;

export const langOf = (locale?: string): Lang => (locale === 'es' ? 'es' : 'en');

export const copyFor = (locale?: string) => copies[langOf(locale)];

/** Translator for the labels drawn inside the artwork; unknown strings stay in English. */
export const tr = (locale?: string) => {
  const dict: Record<string, string> = copyFor(locale).svg;
  return (s: string) => dict[s] ?? s;
};

/** Pages that exist in every language. */
export type Page = 'landing' | 'architecture';

/** Root-relative path of a page (the landing by default) in a given language, honouring the deploy base. */
export const pathFor = (lang: Lang, page: Page = 'landing') => {
  const base = import.meta.env.BASE_URL.replace(/\/$/, '');
  const prefix = lang === 'en' ? base : `${base}/${lang}`;
  return page === 'architecture' ? `${prefix}/architecture/` : `${prefix}/`;
};
