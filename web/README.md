# Gachapon · página del proyecto

Página narrativa, guiada por el desplazamiento, que presenta Gachapon.

```bash
npm install
npm run dev      # astro dev
npm run build    # astro check + compilación de producción
SITE_URL=https://example.com SITE_BASE=/ npm run build   # origen real para canonical / OG / sitemap
```

Opcional: `PUBLIC_GOOGLE_SITE_VERIFICATION` y `PUBLIC_BING_SITE_VERIFICATION` añaden las etiquetas meta de Search Console y de Bing Webmaster. El workflow de Pages las lee de las variables del repositorio `GOOGLE_SITE_VERIFICATION` y `BING_SITE_VERIFICATION` (Settings → Secrets and variables → Actions → Variables); si no están, no se emite ninguna etiqueta.

- `src/content/copy.en.ts`, `copy.es.ts`: todos los textos, en inglés en `/` y en español en `/es/` (rutas i18n de Astro). Los dos tienen la misma forma; `svg` asocia cada etiqueta en inglés de las ilustraciones con su traducción.
- `src/i18n.ts`: `copyFor(Astro.currentLocale)` para los textos, `tr()` para las etiquetas de las ilustraciones y `pathFor(lang, page = 'landing', slug?)` para los botones de idioma y de vista de la cabecera (`page` es `'landing'`, `'architecture'` o `'blog'`, y `slug` elige un artículo del blog; el botón de idioma de la portada conserva además el capítulo actual). Para un idioma nuevo: añadir `copy.xx.ts`, darlo de alta en `i18n.ts` y en `astro.config.mjs`, y añadir `src/pages/xx/index.astro`.
- `src/components/Hud.astro`: la cabecera fija de arriba, común a las páginas (`<Hud page="landing" />` / `page="architecture"`). Lleva la marca, botones en píldora a las otras dos vistas (Story / Historia, Architecture / Arquitectura, Blog), el cambio de idioma (la misma página en el otro idioma) y GitHub. `Footer.astro` es el pie común.
- `src/components/Landing.astro`: la página en sí, que comparten `src/pages/index.astro` y `src/pages/es/index.astro`.
- `src/lib/tokens.ts`: tiempos de la historia, transiciones de cámara y constantes de movimiento.
- `src/lib/motion.ts`: la gramática del movimiento (transfer, lock, success, failure, wake, sleep, verify…).
- `src/components/`: piezas SVG reutilizables (Capsule, AgentNode, LockGate, Worktree, Barrier, Terminal…) y `scenes/Act*.astro`.
- `src/scripts/`: `director.ts` (escenario fijo, cámara y avance con el desplazamiento) y `acts-*.ts` (un constructor por acto).
- Escritorio ≥900px: escenario cinematográfico fijo (y una cápsula WebGL diferida ≥1100px). En pantallas estrechas, escenas apiladas que se reproducen al entrar. Con `prefers-reduced-motion`, composición estática apilada.

## Página de arquitectura

`/architecture/` (inglés) y `/es/architecture/` (español), las dos generadas por `src/components/Architecture.astro` desde `src/pages/architecture.astro` y `src/pages/es/architecture.astro`. Es una página con desplazamiento normal (sin escenario fijo ni `scripts/main`; `Base` solo añade el script `is-cinema` con `page="landing"`) con una portada, un índice de chips fijo y ocho secciones, cada una con su texto y un `<iframe>` diferido y enmarcado con un diagrama, más un enlace para «abrir a pantalla completa».

- Textos: la exportación `architecture` de `copy.en.ts` / `copy.es.ts` (título, descripción, etiquetas y `sections[]` con `slug`, `kicker`, `h`, `p`, `points`). Orden de las secciones y sus slugs: `system`, `workspace`, `mission-flow`, `planner-executor`, `job-lifecycle`, `actions-flow`, `database`, `platform`.
- `Base.astro` recibe `page` para que canonical, las alternativas `hreflang` y `x-default` apunten a la página equivalente en cada idioma.

### Contrato de los diagramas

- Fuentes (especificaciones de archify): `web/architecture/<slug>.<lang>.json`, una por sección e idioma.
- Resultado: `web/public/architecture/<lang>/<slug>.html`, HTML interactivo autónomo que Pages sirve tal cual.
- La página incrusta `${BASE_URL}/architecture/<lang>/<slug>.html?theme=dark&embed=1` y enlaza el mismo fichero sin `embed=1` para la pantalla completa. Una sección nueva necesita su slug en los dos ficheros de textos y los dos HTML.
- El HTML se regenera desde las fuentes JSON con la skill del proyecto `.claude/skills/archify`.

## Blog

`/blog/` y `/es/blog/`, generados por `src/components/Blog.astro`: la lista de artículos a la izquierda (fija en pantallas anchas, arriba en las estrechas) y el artículo elegido a la derecha. `/blog/` abre el artículo más reciente; cada artículo tiene además su propia página, `/blog/<slug>/` y `/es/blog/<slug>/` (`src/pages/blog/[slug].astro`). Textos: la exportación `blog` de `copy.en.ts` / `copy.es.ts`.

Los artículos no se escriben desde la web: cada uno es un fichero Markdown en `src/blog/` (colección en `src/content.config.ts`) y se publica cuando llega a `main` y el workflow de Pages recompila el sitio.

1. Copiar `src/blog/_plantilla.md` a `src/blog/<slug>.md` (p. ej. `2026-10-27-dia-1.md`; el nombre del fichero es la URL; los que empiezan por `_` no se publican).
2. Rellenar la cabecera: `title`, `date` (los más recientes primero), `summary` opcional (lista y meta description), `tags`, `lang` (`es` por defecto; cada artículo sale en los dos idiomas, marcado cuando no coincide con el de la página) y `draft` (solo visible con `npm run dev`).
3. Escribir el cuerpo en Markdown. Las imágenes van en `public/blog/` y se enlazan como `/gachapon/blog/<fichero>`.
