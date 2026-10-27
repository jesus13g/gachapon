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
    'Gachapon is a local Rust orchestrator that splits work between two roles: a planner that formalises and reviews, and an executor that does the mechanical work inside a sandbox. They talk through a file bus, never directly.',
    'These eight diagrams walk through the whole design, from the overall map down to the database and the three operating systems it runs on.',
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
      h: 'The whole system at a glance',
      p: 'The planner turns your request into an order with a success criterion and drops it on the file bus. The orchestrator takes it from the queue, runs the executor in a sandbox and checks the criterion itself. Around that core sit the CLI, the web panel, the MCP server and the knowledge vault.',
      points: [
        'Planner (Claude Code by default) and executor (Qwen Code on a local model) are separate roles, never the same agent.',
        'The bus is plain files on disk: queues, job state and exclusive locks per worktree.',
        'ea, ea-web and ea-mcp read the same state; everything listens on 127.0.0.1 only.',
        'The success criterion is a shell command run by the system, never by the agent.',
      ],
    },
    {
      slug: 'workspace',
      kicker: '02 · Workspace',
      h: 'A Rust workspace of small crates',
      p: 'The repository is a Cargo workspace where each crate has one job and the dependency graph stays shallow. Settings sit at the base, and every platform difference is confined to a single crate. Three binaries and one web panel are built on top.',
      points: [
        'ea-ajustes holds the settings and depends on no other ea-* crate; ea-biblioteca depends only on it.',
        'ea-plataforma isolates everything that differs between Linux, macOS and Windows.',
        'ea-store (SQLite), ea-bus (file queues and state machine) and ea-boveda (the only code that writes to the vault) are the data layer.',
        'The desktop app and the Windows installer live outside the workspace, with their own manifests.',
      ],
    },
    {
      slug: 'mission-flow',
      kicker: '03 · Missions and works',
      h: 'Three ways to hand over work',
      p: 'A mission iterates towards one goal until its criterion passes. A work (obra) follows a long plan phase by phase, each with its own criterion. A routine repeats a known process from a versioned TOML file. The diagram shows how each one travels from the planner to the executor and back.',
      points: [
        'A mission returns to the planner when it finishes or gets blocked.',
        'A work returns when it finishes, or every N phases to be reviewed.',
        'A failed routine leaves a correction package waiting until you confirm it.',
        'Rephrasing a work restarts it from a chosen phase; earlier phases are kept.',
      ],
    },
    {
      slug: 'planner-executor',
      kicker: '04 · Planner and executor',
      h: 'Planner and executor, in sequence',
      p: 'The sequence shows who talks to whom and when. The planner writes the order; the orchestrator locks the worktree, launches the executor, runs the criterion after every attempt and feeds the evidence of any failure back in. Only then does the report and diff return for review.',
      points: [
        'Each role can be filled by Claude Code, Qwen Code, Codex, OpenCode or Antigravity, within what the catalog allows.',
        'The executor always runs against a local model; a remote model is only valid for the planner.',
        'Attempts are capped by time, turns and iterations, and the whole process tree dies at the limit.',
        'You, or the planner with /revisar, close, rephrase or continue the job.',
      ],
    },
    {
      slug: 'job-lifecycle',
      kicker: '05 · Job lifecycle',
      h: 'The life of a job',
      p: 'Every job moves through a small state machine that the code validates as a pure function. A job is queued, runs, waits for review and is closed; it can also be cancelled, or wait as a correction package. An illegal transition is rejected rather than written.',
      points: [
        'Main path: en_cola, ejecutando, en_revision, cerrado.',
        'Cancelling a running job really stops the agent: SIGTERM, then SIGKILL after 5 seconds.',
        'What the agent already wrote in the worktree is not undone.',
        'State files are written atomically, so a crash never leaves a half-written job.',
      ],
    },
    {
      slug: 'actions-flow',
      kicker: '06 · Supervised actions',
      h: 'Agents propose, you decide',
      p: 'Anything that changes the system goes through one catalog of actions. An agent can only simulate it or queue a proposal; it cannot confirm it. You review the simulation and accept, and only then is the action revalidated and executed, with every step recorded in the journal.',
      points: [
        'The MCP server and the orchestrator only receive a read-only layer.',
        'Irreversible actions require a real terminal and typing a confirmation.',
        'The journal is append-only at the engine level; a mistake is fixed with another event.',
        'The vault of notes is written by a single owner, ea-boveda.',
      ],
    },
    {
      slug: 'database',
      kicker: '07 · Database',
      h: 'One SQLite file for history',
      p: 'The bus holds the live state as files; SQLite keeps the history. It stores the journal, proposals, the vault index, jobs, phases and routines. There is a single migration, and an old schema is rejected instead of silently upgraded.',
      points: [
        'The journal table is protected by triggers, so rows cannot be updated or deleted.',
        'Jobs link to their phases; proposals carry the structured action and its precondition.',
        'The database path is configurable with EA_DB_PATH, which tests use to stay isolated.',
      ],
    },
    {
      slug: 'platform',
      kicker: '08 · Platform and safety',
      h: 'Linux, macOS and Windows, same guarantees',
      p: 'The executor runs inside the sandbox of each system: bubblewrap on Linux, Seatbelt on macOS. Windows has no native sandbox, so with sandbox enabled the executor refuses to start and WSL2 is recommended. Services, paths and terminals are adapted per system, and everything stays on your machine.',
      points: [
        'Hard limits kill the agent and its descendants when time runs out.',
        'The agent has no web tools and auto-approval applies only inside the sandbox.',
        'Services run as systemd --user, launchd or scheduled tasks, installed by ea instalar.',
        'Local only: every server listens on 127.0.0.1 and the web panel filters the Host header.',
      ],
    },
  ],
};
