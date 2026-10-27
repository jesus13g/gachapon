# Gachapon — landing

Narrative, scroll-driven landing for Gachapon (source of truth: `../ecosistema-asistente/FUNCIONALIDADES.md`).

```bash
npm install
npm run dev      # astro dev
npm run build    # astro check + production build
SITE_URL=https://example.com SITE_BASE=/ npm run build   # real origin for canonical / OG / sitemap
```

Optional: `PUBLIC_GOOGLE_SITE_VERIFICATION` and `PUBLIC_BING_SITE_VERIFICATION` add the Search Console / Bing Webmaster meta tags. The Pages workflow reads them from the repository variables `GOOGLE_SITE_VERIFICATION` and `BING_SITE_VERIFICATION` (Settings → Secrets and variables → Actions → Variables); unset, no tag is emitted.

- `src/content/copy.en.ts`, `copy.es.ts` — all copy, English at `/` and Spanish at `/es/` (Astro i18n routing). Same shape in both; `svg` maps each English label of the artwork to its translation
- `src/i18n.ts` — `copyFor(Astro.currentLocale)` for the copy, `tr()` for artwork labels, `pathFor(lang, page = 'landing')` for the language and view buttons in the header (`page` is `'landing'` or `'architecture'`; the landing's language button also keeps the current chapter). A new language: add `copy.xx.ts`, list it in `i18n.ts` and `astro.config.mjs`, and add `src/pages/xx/index.astro`
- `src/components/Hud.astro` — the fixed top header, shared by both pages (`<Hud page="landing" />` / `page="architecture"`). It holds the brand, a pill button to the other view (Architecture / Arquitectura on the landing, Story / Historia on the architecture page), the language switch (same page in the other language) and GitHub. `Footer.astro` is the shared footer
- `src/components/Landing.astro` — the page itself, shared by `src/pages/index.astro` and `src/pages/es/index.astro`
- `src/lib/tokens.ts` — story timing, camera transitions, motion constants
- `src/lib/motion.ts` — motion grammar (transfer, lock, success, failure, wake, sleep, verify…)
- `src/components/` — reusable SVG pieces (Capsule, AgentNode, LockGate, Worktree, Barrier, Terminal…) and `scenes/Act*.astro`
- `src/scripts/` — `director.ts` (pinned stage, camera, scrub), `acts-*.ts` (one builder per act)
- Desktop ≥900px: pinned cinematic stage (+ one lazy WebGL capsule ≥1100px). Narrow: stacked scenes played on entry. `prefers-reduced-motion`: static stacked composition.

## Architecture page

`/architecture/` (English) and `/es/architecture/` (Spanish), both rendered by `src/components/Architecture.astro` from `src/pages/architecture.astro` and `src/pages/es/architecture.astro`. It is a normal scrolling page (no pinned stage, no `scripts/main`; `Base` only adds the `is-cinema` script for `page="landing"`) with a hero, a sticky chip index and eight sections, each with copy and a framed lazy `<iframe>` of one diagram plus an "open full screen" link.

- Copy: the `architecture` export of `copy.en.ts` / `copy.es.ts` (title, description, labels and `sections[]` with `slug`, `kicker`, `h`, `p`, `points`). Section order and slugs: `system`, `workspace`, `mission-flow`, `planner-executor`, `job-lifecycle`, `actions-flow`, `database`, `platform`.
- `Base.astro` takes `page` so canonical, `hreflang` alternates and `x-default` point at the equivalent page in each language.

### Diagram contract

- Sources (archify specs): `web/architecture/<slug>.<lang>.json`, one per section and language.
- Rendered output: `web/public/architecture/<lang>/<slug>.html`, standalone interactive HTML served as-is by Pages.
- The page embeds `${BASE_URL}/architecture/<lang>/<slug>.html?theme=dark&embed=1` and links to the same file without `embed=1` for full screen. A new section needs its slug in both copy files and both HTML files.
- Regenerate the HTML from the JSON sources with the project skill `.claude/skills/archify`.
