/**
 * Content collections. The blog lives in src/blog/*.md: one Markdown file per article, published by pushing it
 * to GitHub (the Pages workflow rebuilds the site). Files starting with "_" are ignored (see src/blog/_plantilla.md).
 */
import { defineCollection } from 'astro:content';
import { glob } from 'astro/loaders';
import { z } from 'astro/zod';

const blog = defineCollection({
  loader: glob({ base: './src/blog', pattern: '**/[^_]*.md' }),
  schema: z.object({
    title: z.string(),
    date: z.coerce.date(),
    summary: z.string().optional(),
    tags: z.array(z.string()).default([]),
    /** Language the article is written in; every article shows in both versions of the site. */
    lang: z.enum(['en', 'es']).default('es'),
    /** Drafts only show with `npm run dev`. */
    draft: z.boolean().default(false),
  }),
});

export const collections = { blog };
