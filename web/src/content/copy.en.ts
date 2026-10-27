/**
 * All landing copy lives here, decoupled from components (Spanish lives in copy.es.ts with the same shape; see src/i18n.ts).
 * Source of truth for facts: FUNCIONALIDADES.md in the Gachapon repository.
 */
export interface Beat {
  h: string;
  p?: string;
}
export interface ActCopy {
  id: string;
  kicker: string;
  /** Accessible name of the scene illustration. */
  alt: string;
  /** Plain-text version of the flow shown in the artwork (also shown in static layouts). */
  flow?: string;
  beats: Beat[];
}

export const site = {
  lang: 'en',
  name: 'Gachapon',
  title: 'Gachapon — Open-source AI agent orchestrator',
  description:
    'Open-source orchestrator for AI agents: a capable planner thinks, a local model does the mechanical work, and every step stays sandboxed, logged and yours.',
  ogImage: '/og.png',
  ogAlt: 'Gachapon logo, an orange and white capsule, above the headline “Turn AI agents into a system.” and the line “The expensive model thinks. The local model works.”',
  repo: 'https://github.com/jesus13g/gachapon',
  docs: 'https://github.com/jesus13g/gachapon#readme',
  license: 'MIT License',
};

export const ui = {
  skip: 'Skip to content',
  top: 'Gachapon — top',
  home: 'Gachapon — home page',
  navProject: 'Project',
  /** The header button that switches to the other language. */
  langSwitch: 'Ver en español',
  scroll: 'Scroll',
  soundNote: '',
  railLabel: 'Story chapters',
  footer: 'Gachapon is open source (MIT). It runs on your machine; only the planner’s model may live in the cloud.',
  ctaPrimary: 'View on GitHub',
  ctaSecondary: 'Read the docs',
  hero: {
    kicker: 'Agent orchestration · local first',
    h1: 'Turn AI agents',
    h1em: 'into a system.',
    subStrong: 'The expensive model thinks. The local model works.',
    sub: 'Gachapon coordinates them — and you keep control.',
    boot: ['planner   idle', 'executor  idle', 'bus       ok', 'sandbox   armed'],
  },
  final: {
    kicker: 'Job closed · verified',
    h: 'Don’t just run agents.\nOrchestrate them.',
    sub: 'Let capable models think, local models work, and Gachapon keep everything under control.',
    done: 'DONE',
  },
};

export const acts: ActCopy[] = [
  {
    id: 'boot',
    kicker: 'Boot',
    alt: 'A dormant Gachapon machine wakes up and releases a capsule.',
    beats: [],
  },
  {
    id: 'problem',
    kicker: '01 · The problem',
    alt: 'Isolated agent terminals with tangled lines, which realign around a single Gachapon core.',
    flow: 'many agents → no shared queue → no state → Gachapon',
    beats: [
      { h: 'More agents isn’t a system.', p: 'Terminals everywhere. Premium models doing mechanical chores. Nobody knows what is running, or what finished.' },
      { h: 'Put a core in the middle.', p: 'Gachapon gives every agent a place, a queue and a state.' },
    ],
  },
  {
    id: 'roles',
    kicker: '02 · Planner + executor',
    alt: 'An objective enters the planner, becomes a job in Gachapon, runs on a local-model executor and returns as evidence.',
    flow: 'OBJECTIVE → PLANNER → JOB → GACHAPON → EXECUTOR → EVIDENCE → GACHAPON → PLANNER',
    beats: [
      { h: 'Spend intelligence where it matters.', p: 'The planner understands the goal, designs the plan and judges the result. Nothing else.' },
      { h: 'Let local models do the work.', p: 'The executor handles every step that can be described and verified — unattended, on your machine.' },
    ],
  },
  {
    id: 'bus',
    kicker: '03 · The job bus',
    alt: 'Inside Gachapon, a capsule travels through the stations queued, running, review and closed, with side bays for waiting and cancelled.',
    flow: 'QUEUED → RUNNING → REVIEW → CLOSED · WAITING · CANCELLED',
    beats: [
      { h: 'Every job has a place.', p: 'One queue for the executor, one for the planner’s review. Work is never lost — it is always in a known state.' },
    ],
  },
  {
    id: 'locks',
    kicker: '04 · Locks + worktrees',
    alt: 'A gate locks a git worktree for one writer; a second job waits, then gets its own branch and worktree to run in parallel.',
    flow: 'job → LOCK → one writer · second job BLOCKED → own worktree → parallel',
    beats: [
      { h: 'One writer. Zero collisions.', p: 'A lock gives a single agent the working tree and shows who holds it and since when.' },
      { h: 'Need parallel? Branch it.', p: 'A job can run in its own git worktree, on its own branch, beside the others.' },
    ],
  },
  {
    id: 'modes',
    kicker: '05 · Three ways to work',
    alt: 'The flow splits into three lanes: mission loops until a real check passes, build advances by phases with reviews, routine becomes a versioned template run on a calendar.',
    flow: 'MISSION: try → verify → fail → evidence → retry · BUILD: 01 → 02 → 03 → review → 04 → 05 · ROUTINE: define once → run on a schedule',
    beats: [
      { h: 'Mission: iterate until it’s true.', p: 'The agent doesn’t decide it succeeded. The system verifies it — with a command that has to pass.' },
      { h: 'Build: big plan, small phases.', p: 'A snapshot after each phase. A review every N. Continue, or rewrite from phase four.' },
      { h: 'Routine: define once.', p: 'Run it whenever needed — by hand or on a calendar. Every version is explicit.' },
    ],
  },
  {
    id: 'control',
    kicker: '06 · Human control',
    alt: 'An agent’s change stops at a barrier as a proposal; you simulate it and accept or reject. Irreversible actions are diverted to a real terminal.',
    flow: 'CHANGE → PROPOSAL → SIMULATE → ACCEPT / REJECT · IRREVERSIBLE → REAL TERMINAL REQUIRED',
    beats: [
      { h: 'Agents can propose. You decide.', p: 'Anything that changes the system is simulated first and applied only when you accept.' },
      { h: 'No agent approves itself.', p: 'Irreversible actions are diverted to a real terminal, with an explicit confirmation.' },
    ],
  },
  {
    id: 'teams',
    kicker: '07 · Teams',
    alt: 'A planner wakes a team: three executors and a reviewer start one by one, each job on its own route and worktree, then the team goes back to sleep.',
    flow: 'WAKE TEAM → planner → executor A / B / C → reviewer → SLEEP TEAM',
    beats: [
      { h: 'From two agents to a team.', p: 'Wake a team for a goal. The planner assigns jobs to specific executors and a reviewer.' },
      { h: 'Parallel when useful. Isolated when necessary.', p: 'Teams are woken and put to sleep by you — never by an agent, never on their own.' },
    ],
  },
  {
    id: 'routines',
    kicker: '08 · Routines',
    alt: 'A successful run is saved as routine version one, runs on a calendar, gets version two, and a failure produces a correction package that waits for your confirmation before version three.',
    flow: 'run → ROUTINE v1 → calendar → v2 · FAILED → CORRECTION PACKAGE → waits for you → planner → v3',
    beats: [
      { h: 'Turn a good run into a habit.', p: 'Save it as a versioned routine and schedule it. Retire it and restore it any time.' },
      { h: 'Automation without surprises.', p: 'When a run fails, a correction package waits for you. Nothing rewrites itself.' },
    ],
  },
  {
    id: 'knowledge',
    kicker: '09 · Vault · skills · library',
    alt: 'A vault with a private region per agent and a shared region; skills inserted into the executor for one job only; and a wall of tools: skills, plugins, subagents, MCP servers, commands and extensions.',
    flow: 'VAULT: private notes → promoted by you → SHARED · SKILLS: per job · LIBRARY: skills / plugins / subagents / MCP / commands / extensions',
    beats: [
      { h: 'Memory with a gatekeeper.', p: 'Each agent writes in its own region. A note becomes shared only when you promote it.' },
      { h: 'Only the context this job needs.', p: 'Skills are injected for one job, recorded with it, and gone afterwards.' },
      { h: 'Know what every agent can use.', p: 'One inventory of skills, plugins, subagents, MCP servers, commands and extensions.' },
    ],
  },
  {
    id: 'mcp',
    kicker: '10 · MCP + permissions',
    alt: 'Agent calls reach Gachapon’s MCP door: reads pass, reversible mutations become proposals, irreversible ones are blocked and need a real terminal. Editing the prompt does not move the barrier.',
    flow: 'READ → pass · REVERSIBLE MUTATION → proposal / simulation · IRREVERSIBLE → blocked, real terminal',
    beats: [
      { h: 'Security lives in the system, not in the prompt.', p: 'Permissions follow the agent’s role and are re-read on every call. Rewriting the prompt moves nothing.' },
    ],
  },
  {
    id: 'sandbox',
    kicker: '11 · Sandbox · local first',
    alt: 'The executor sits inside a translucent sandbox: the worktree is writable, the rest of the filesystem read-only, the internet blocked, and the model reached only through localhost. Hard limits stop the mechanism when reached.',
    flow: 'WORKTREE write · rest read-only · internet blocked · localhost → local model · limits: turns / time / tool calls / iterations',
    beats: [
      { h: 'Local execution. Hard limits. Real isolation.', p: 'Filesystem read-only except the worktree. No web tools. Turns, time, tool calls and iterations all have a ceiling.' },
      { h: 'Your work stays on your machine.', p: 'Linux uses bubblewrap, macOS a Seatbelt profile. Windows has no native sandbox: use WSL2.' },
    ],
  },
  {
    id: 'journal',
    kicker: '12 · Observability',
    alt: 'Events from every part of the system flow into an append-only journal. A correction is added as a new event linked to the original; nothing is erased.',
    flow: 'EVENT → JOURNAL (append-only) → CORRECTION is a new event',
    beats: [
      { h: 'Everything leaves a trace.', p: 'Jobs, routines, proposals, settings, notes. Who did it, with which model and skills, and when.' },
      { h: 'The past can’t be rewritten.', p: 'The journal is append-only. A mistake is fixed by adding a correction, never by deleting.' },
    ],
  },
  {
    id: 'interfaces',
    kicker: '13 · Interfaces',
    alt: 'The same system seen as a web panel, a command-line terminal and a read-only terminal UI.',
    flow: 'WEB PANEL · CLI (ea status, ea cola, ea agentes, ea semaforos) · TUI (read-only)',
    beats: [
      { h: 'One system. Many ways to control it.', p: 'A local web panel, a full CLI, and a read-only TUI for a safe glance.' },
    ],
  },
  {
    id: 'system',
    kicker: '14 · The whole system',
    alt: 'The complete architecture running at once: planner, executor, queues, worktrees, locks, teams, vault, journal, routines, tools, MCP and the user.',
    flow: 'planner · executor · queues · worktrees · locks · teams · vault · journal · routines · tools · MCP · you',
    beats: [
      { h: 'It was never one machine.', p: 'It’s an ecosystem — and every part of it is on your machine.' },
    ],
  },
  {
    id: 'final',
    kicker: 'Done',
    alt: 'A capsule reaches the end of the machine and opens, showing DONE.',
    beats: [],
  },
];

/** Benefit words that orbit the final architecture (act 14). */
export const benefits = [
  { w: 'Local', t: 'Your machine. Your data.' },
  { w: 'Observable', t: 'State always visible.' },
  { w: 'Reversible', t: 'Undo what can be undone.' },
  { w: 'Interchangeable', t: 'Swap agents and models.' },
  { w: 'Controlled', t: 'You hold the keys.' },
  { w: 'Auditable', t: 'Append-only trace.' },
];

/** Labels drawn inside the artwork, keyed by their English text (English needs no entries). */
export const svg: Record<string, string> = {};

/** The 404 page (one file for both languages on GitHub Pages, so it shows both copies). */
export const notFound = {
  title: 'Page not found — Gachapon',
  description: 'This page does not exist. Go back to the Gachapon home page.',
  kicker: 'Error 404 · job not found',
  h: 'Page not found.',
  p: 'The address you followed does not exist or has moved.',
  home: 'Go to the home page in English',
};

/** The architecture page: one embedded, interactive diagram per section (rendered HTML in public/architecture/<lang>/<slug>.html). */
export const architecture = {
  title: 'Architecture — how Gachapon is built · Gachapon',
  description:
    'The architecture of Gachapon in eight interactive diagrams: system map, workspace, mission flow, planner and executor, job lifecycle, supervised actions, database and platform.',
  nav: 'Architecture',
  back: 'Story',
  kicker: 'Architecture · design',
  h1: 'How Gachapon is built.',
  intro: [
    'This page shows how Gachapon is built, end to end. Gachapon is a local Rust orchestrator that splits work between two roles: a planner that formalises and reviews, and an executor that runs the work inside a sandbox. They talk through a file bus, never directly.',
    'Read it in order: the system map, the repository, the flow of a job, the planner–executor sequence, the job states, the supervised actions, the data, and finally the platform it runs on. Each section has an interactive diagram and the key facts written out beside it.',
  ],
  howTo: {
    label: 'How to read the diagrams',
    text: 'Every diagram is interactive: scroll or pinch to zoom, drag to pan, click a node to focus it and its neighbours, search to find a part, and export the result as an image or SVG. Open it full screen for more room.',
  },
  indexLabel: 'Diagrams',
  fullscreen: 'Open full screen',
  loading: 'Loading diagram…',
  diagramTitle: 'Interactive diagram',
  frameLabel: 'Interactive diagram',
  sections: [
    {
      slug: 'system',
      kicker: '01 · System map',
      short: '01 · Map',
      h: 'The whole system at a glance',
      p: 'The planner reaches Gachapon through `ea-mcp`, a read-only MCP server on `127.0.0.1:8765`, and creates jobs with the `ea` CLI. `ea-orchestrator` watches the `modelo-local` queue on the file bus, launches the executor in a sandbox that only talks to a local `llama-server`, and runs the success criterion itself. You accept changes only from `ea` in a real terminal or from the `ea-web` panel.',
      points: [
        '`ea-mcp` only receives the `SoloConsulta` layer: it can read and propose, never confirm.',
        'Accepting goes through simulate → propose → accept in `ea-acciones`, from `ea` (real TTY) or `ea-web` on `127.0.0.1:8092` (Host filter, session key).',
        'The executor runs under `bwrap` or Seatbelt and reaches only the local model; the only optional cloud piece is the planner\'s model.',
        'The file bus holds queues, job state, routines, the `flock` semaphore, teams and presence.',
        'Crates are layered: `ea-plataforma` at the base, then `ea-ajustes`, `ea-store`, `ea-bus`, `ea-acciones`, `ea-boveda` and `ea-biblioteca`.',
      ],
    },
    {
      slug: 'workspace',
      kicker: '02 · Workspace',
      short: '02 · Repo',
      h: 'A Cargo workspace of small crates',
      p: 'The repository is a Cargo workspace of 11 members, listed in dependency order from `ea-plataforma` and `ea-ajustes` up to `ea-cli`. Four binaries come out of it: `ea`, `ea-mcp`, `ea-orchestrator` and `ea-web`. Around the crates sit the desktop app, the installer, the skills and the project tooling. The tree is tall: scroll and zoom to walk it.',
      points: [
        'The workspace excludes `escritorio/` (`ea-escritorio`, tray and notifications) and `instalador/` (`gachapon-instalar.exe`); both have their own manifests.',
        '`instalar.sh` and `instalar.ps1` are the one-step installers.',
        '`claude/` holds the skills `/mision`, `/obra`, `/rutina`, `/revisar` and `/equipo`, plus the subagent hook.',
        '`agentes/` has the flow and the adapters for Codex, OpenCode and Antigravity; `qwen/skills` the short Qwen skills.',
        'Also in the tree: `web/` (this Astro landing), `bench/`, `docs/` and `.github/workflows` (`ci`, `pages`, `release`).',
      ],
    },
    {
      slug: 'mission-flow',
      kicker: '03 · Mission flow',
      short: '03 · Flow',
      h: 'From order to review',
      p: 'The planner formalises a goal plus a shell criterion with `/mision`, `/obra` or `/rutina` and runs `ea trabajo crear`; an empty criterion is rejected on creation. The orchestrator takes the oldest job, grabs the exclusive worktree semaphore, runs the executor in the sandbox and then runs the criterion itself with `sh -c`. The report comes back to the planner, who closes, rephrases or continues.',
      points: [
        'If the worktree semaphore is busy, the job goes back to the queue.',
        'A failing criterion retries with the evidence up to `iteraciones`; running out of iterations or hard limits yields a blocked report, and cancelling kills the process tree.',
        'Delivery writes `informe.json` and moves the job to `en_revision`; missions and works are queued in `colas/claude-code` for `/revisar`.',
        'Routines start from a timer or `ea rutina lanzar` with no planner. Success closes them; failure leaves them in `en_espera` with `paquete-correccion.md` until `ea trabajo confirmar`.',
        'A work (`/obra`) has a criterion and a tree snapshot per phase, and pauses every `revisar_cada` phases.',
      ],
    },
    {
      slug: 'planner-executor',
      kicker: '04 · Sequence',
      short: '04 · Sequence',
      h: 'Planner and executor, message by message',
      p: 'The sequence has nine participants and 39 messages. After `ea trabajo crear` the planner stops and hands you the job id: it does not loop. There are two asynchronous waits, the job in `colas/modelo-local` and the report in `colas/claude-code` until `/revisar`.',
      points: [
        '`ea-mcp` is read-only: `ea_status`, `ea_listar_trabajos`, `ea_ver_trabajo`. Creating and deciding goes through the CLI.',
        'CLI calls: `ea trabajo crear --orden`, `ea cola planificador --json`, `ea trabajo diff`, `ea trabajo cerrar|reformular|continuar`.',
        'Every bus write and every MCP call appends a journal event (`bus_crear`, `bus_tomar`, `bus_entregar`, `bus_cerrar`, `mcp_status`…) to the append-only `events` table.',
        'The executor talks to `llama-server`, an OpenAI-compatible API at `modelo.base_url`.',
        'On delivery you get a `notify-send` notification.',
      ],
    },
    {
      slug: 'job-lifecycle',
      kicker: '05 · Job states',
      short: '05 · States',
      h: 'Six states, thirteen transitions',
      p: 'A job is always in one of six states: `en_cola`, `ejecutando`, `en_revision`, `en_espera`, `cerrado` or `cancelado`. The last two are terminal. The state machine is a pure function and an illegal transition is rejected.',
      points: [
        'Transitions: `tomar` (`en_cola` → `ejecutando`), `entregar` (`ejecutando` → `en_revision`), `cerrar`, `reformular` (back to `en_cola`), `poner_en_espera` and `confirmar` (`en_espera` → `en_revision`).',
        '`cancelar` is allowed from any non-terminal state.',
        'There is no timeout state: exhausting the limits delivers a blocked report that also ends in `en_revision`; so do jobs interrupted by a restart (`recuperar`).',
        'Phases of a work live in `trabajo_fases`: `pendiente` → `hecha` or `bloqueada`, or `cancelada`.',
        '`reformular` sends the chosen phase and the following ones back to `pendiente`.',
      ],
    },
    {
      slug: 'actions-flow',
      kicker: '06 · Supervised actions',
      short: '06 · Actions',
      h: 'Agents propose, you decide',
      p: 'Anything that changes the system goes through the catalog of `ea-acciones`: 23 tools, 9 read-only, 13 reversible and 1 irreversible (`ea_eliminar_rutina`). Agents reach it through `ea-mcp` with the `SoloConsulta` layer, which only simulates and writes a pending row in `propuestas`. You accept, and only then is the action claimed, revalidated and applied.',
      points: [
        'Accept from the `ea` CLI or TUI in a real TTY (an irreversible action needs `confirmo <id>`) or from `ea-web`, where irreversible actions answer 403.',
        '`ejecutar()` claims the row atomically, revalidates the precondition and applies it; the result goes to `propuestas` and to the journal as `accion_ejecutada` or `accion_fallida`.',
        'The journal is append-only: triggers abort any UPDATE or DELETE.',
        '`ea-boveda` is the single vault writer (`flock` on `.ea-escritor.lock`); agents read with `ea_buscar_notas` and `ea_leer_nota`.',
        '`ea-ajustes` writes `ajustes.toml` from the CLI and `ea-web`; a key registry says who may change each one, and the orchestrator only reads.',
      ],
    },
    {
      slug: 'database',
      kicker: '07 · Database',
      short: '07 · Data',
      h: 'Seven tables, one migration',
      p: 'The history lives in SQLite: seven tables in five domains, created by a single migration, `0001_inicial.sql`. Only one foreign key is declared; the other links are logical and kept by the code.',
      points: [
        '`events` is the journal, append-only through the triggers `events_no_update` and `events_no_delete`.',
        '`trabajos` and `trabajo_fases` (primary key `trabajo_id` + `numero`); the one declared foreign key points to `trabajos.id`.',
        '`rutinas` has primary key `slug` + `version`; `propuestas` holds the action proposals.',
        '`boveda_notas` (unique `ruta`) and `boveda_fts`, the full-text index (`boveda_fts.rowid` = `boveda_notas.id`).',
        'Logical links without a foreign key: `events.trabajo` → `trabajos`, `trabajos.rutina_slug` and `version` → `rutinas`.',
      ],
    },
    {
      slug: 'platform',
      kicker: '08 · Platform',
      short: '08 · Platform',
      h: 'Install, sandbox and services on three systems',
      p: 'A tag `v*` makes `release.yml` publish to GitHub Releases, and `pages.yml` serves `instalar.sh` and `instalar.ps1`. `ea instalar` needs a real terminal, refuses root, shows a plan and asks for one confirmation. The sandbox is `bwrap` on Linux and Seatbelt on macOS; native Windows has none.',
      points: [
        '`instalar.sh` downloads the tar.gz and checks its SHA-256; `instalar.ps1` downloads `gachapon-instalar.exe`, which carries the Windows binaries. From a clone, run `./instalar.sh`.',
        'Services: `systemctl --user`, `launchctl` or `schtasks`; on Windows `--via wsl` offers WSL2.',
        'With `sandbox = true` the executor refuses to start on native Windows; WSL2 is recommended.',
        'The desktop tray app uses WebKitGTK on Linux and WebView2 on Windows; in WSL mode it reads the session key through `wsl.exe`.',
        'State lives in `~/.config/ecosistema-asistente`, `~/.local/share/ecosistema-asistente` and `~/.local/state/ecosistema-asistente`, overridable with `EA_CONFIG_DIR`, `EA_DB_PATH`, `EA_ESTADO_DIR` and `EA_BOVEDA_DIR`. `ea-plataforma` gathers the differences between systems.',
      ],
    },
  ],
};
