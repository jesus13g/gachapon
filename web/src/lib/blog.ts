import { getCollection } from 'astro:content';

/** Published articles, newest first (drafts only in `astro dev`). */
export const posts = async () =>
  (await getCollection('blog', (p) => import.meta.env.DEV || !p.data.draft)).sort(
    (a, b) => b.data.date.getTime() - a.data.date.getTime() || b.id.localeCompare(a.id),
  );

/** One static page per article, for the `[slug].astro` routes of both languages. */
export const blogPaths = async () => (await posts()).map((p) => ({ params: { slug: p.id } }));
