# Gachapon — landing

Narrative, scroll-driven landing for Gachapon (source of truth: `../ecosistema-asistente/FUNCIONALIDADES.md`).

```bash
npm install
npm run dev      # astro dev
npm run build    # astro check + production build
SITE_URL=https://example.com SITE_BASE=/ npm run build   # real origin for canonical / OG / sitemap
```

- `src/content/copy.en.ts` — all copy (i18n-ready: add `copy.xx.ts` with the same shape)
- `src/lib/tokens.ts` — story timing, camera transitions, motion constants
- `src/lib/motion.ts` — motion grammar (transfer, lock, success, failure, wake, sleep, verify…)
- `src/components/` — reusable SVG pieces (Capsule, AgentNode, LockGate, Worktree, Barrier, Terminal…) and `scenes/Act*.astro`
- `src/scripts/` — `director.ts` (pinned stage, camera, scrub), `acts-*.ts` (one builder per act)
- Desktop ≥900px: pinned cinematic stage (+ one lazy WebGL capsule ≥1100px). Narrow: stacked scenes played on entry. `prefers-reduced-motion`: static stacked composition.
