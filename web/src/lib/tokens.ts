/** Shared constants: geometry, story timing, motion grammar. One place, no magic numbers in scenes. */

export const VB = { w: 1600, h: 900 } as const;

export type Transition = 'cut' | 'zoom' | 'zoomOut' | 'panL' | 'drop' | 'rotate' | 'flip';
export type Pt = readonly [number, number];

export interface ActCfg {
  id: string;
  /** Scroll length in vh. */
  len: number;
  /** How the camera arrives at this act from the previous one. */
  in: Transition;
  /** Where the story capsule is when the act begins / ends, in viewBox units. */
  start: Pt;
  end: Pt;
  /** Mobile crop of the artwork: x y w h. */
  crop: readonly [number, number, number, number];
}

export const STORY: ActCfg[] = [
  { id: 'boot', len: 70, in: 'cut', start: [1130, 330], end: [800, 450], crop: [560, 80, 900, 760] },
  { id: 'problem', len: 90, in: 'drop', start: [800, 450], end: [800, 450], crop: [200, 120, 1200, 700] },
  { id: 'roles', len: 110, in: 'zoom', start: [330, 450], end: [800, 450], crop: [80, 200, 1440, 500] },
  { id: 'bus', len: 90, in: 'zoom', start: [800, 450], end: [360, 600], crop: [200, 250, 1200, 440] },
  { id: 'locks', len: 100, in: 'panL', start: [200, 450], end: [1010, 450], crop: [120, 230, 1180, 590] },
  { id: 'modes', len: 150, in: 'panL', start: [520, 500], end: [1540, 540], crop: [760, 220, 840, 600] },
  { id: 'control', len: 100, in: 'drop', start: [240, 470], end: [1340, 470], crop: [100, 230, 1400, 600] },
  { id: 'teams', len: 110, in: 'zoomOut', start: [260, 520], end: [1480, 520], crop: [100, 190, 1460, 580] },
  { id: 'routines', len: 90, in: 'panL', start: [450, 450], end: [478, 512], crop: [200, 200, 1200, 560] },
  { id: 'knowledge', len: 130, in: 'rotate', start: [1050, 590], end: [1050, 590], crop: [500, 160, 800, 640] },
  { id: 'mcp', len: 80, in: 'panL', start: [200, 430], end: [1370, 430], crop: [100, 220, 1400, 520] },
  { id: 'sandbox', len: 100, in: 'zoom', start: [800, 535], end: [800, 535], crop: [380, 180, 840, 600] },
  { id: 'journal', len: 80, in: 'drop', start: [160, 560], end: [1465, 560], crop: [100, 220, 1400, 520] },
  { id: 'interfaces', len: 90, in: 'flip', start: [800, 450], end: [800, 450], crop: [260, 130, 1080, 640] },
  { id: 'system', len: 100, in: 'zoomOut', start: [800, 545], end: [800, 545], crop: [60, 100, 1480, 720] },
  { id: 'final', len: 80, in: 'zoom', start: [800, 545], end: [1130, 330], crop: [560, 80, 900, 760] },
];

/** Scroll feel: `stretch` multiplies the scroll distance of the whole story (more = slower),
 *  `scrub` is how many seconds the scene lags behind the wheel (more = heavier, smoother). */
export const SCROLL = { stretch: 1.7, scrub: 1.4 } as const;

/** Overlap (in vh) at each side of an act boundary where the camera moves between scenes. */
export const TR = 9;

/** Motion grammar — the same state always moves the same way. */
export const EASE = {
  /** transfer: a capsule moving along a conduit */
  transfer: 'power2.inOut',
  /** lock / mechanical clamp */
  lock: 'back.out(2.2)',
  /** wake: machines coming alive */
  wake: 'power3.out',
  /** sleep: powering down */
  sleep: 'power2.in',
  /** verification: stepped, deliberate */
  verify: 'steps(6)',
  /** approval: crisp and final */
  approve: 'expo.out',
  /** camera moves */
  camera: 'power3.inOut',
  linear: 'none',
} as const;

/** Durations are fractions of an act's usable scroll length, not seconds. */
export const DUR = {
  blip: 0.03,
  short: 0.06,
  step: 0.1,
  travel: 0.16,
  long: 0.25,
} as const;
