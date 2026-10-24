# Gachapon — landing

Narrative, scroll-driven landing for Gachapon (source of truth: `../ecosistema-asistente/FUNCIONALIDADES.md`).

```bash
npm install
npm run dev      # astro dev
npm run build    # astro check + production build
SITE_URL=https://example.com SITE_BASE=/ npm run build   # real origin for canonical / OG / sitemap
```

- `src/content/copy.en.ts`, `copy.es.ts` — all copy, English at `/` and Spanish at `/es/` (Astro i18n routing). Same shape in both; `svg` maps each English label of the artwork to its translation
- `src/i18n.ts` — `copyFor(Astro.currentLocale)` for the copy, `tr()` for artwork labels, `pathFor()` for the EN/ES switch in the header. A new language: add `copy.xx.ts`, list it in `i18n.ts` and `astro.config.mjs`, and add `src/pages/xx/index.astro`
- `src/components/Landing.astro` — the page itself, shared by `src/pages/index.astro` and `src/pages/es/index.astro`
- `src/lib/tokens.ts` — story timing, camera transitions, motion constants
- `src/lib/motion.ts` — motion grammar (transfer, lock, success, failure, wake, sleep, verify…)
- `src/components/` — reusable SVG pieces (Capsule, AgentNode, LockGate, Worktree, Barrier, Terminal…) and `scenes/Act*.astro`
- `src/scripts/` — `director.ts` (pinned stage, camera, scrub), `acts-*.ts` (one builder per act)
- Desktop ≥900px: pinned cinematic stage (+ one lazy WebGL capsule ≥1100px). Narrow: stacked scenes played on entry. `prefers-reduced-motion`: static stacked composition.
