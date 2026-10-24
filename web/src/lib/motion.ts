import { gsap } from 'gsap';
import { DrawSVGPlugin } from 'gsap/DrawSVGPlugin';
import { MotionPathPlugin } from 'gsap/MotionPathPlugin';
import { EASE, TR } from './tokens';

gsap.registerPlugin(DrawSVGPlugin, MotionPathPlugin);

/**
 * The motion grammar. Scenes never tween raw numbers: they call these verbs, so a state always moves the same way.
 *   enter/exit      → appear / vanish
 *   transfer        → a capsule travelling along a conduit
 *   lock / unlock   → a gate closing / opening
 *   success         → pulse + open + advance
 *   failure         → shake + red
 *   waiting         → suspended, slow heartbeat (CSS, via data-state)
 *   wake / sleep    → LEDs and mechanisms powering up / down
 *   verify          → stepped progress
 *   approve         → crisp press, then the gate opens
 */
export interface Ctx {
  tl: gsap.core.Timeline;
  mode: 'cinema' | 'inview';
  sc: HTMLElement;
  world: SVGGElement;
  thread: SVGGElement;
  threadWorld: SVGGElement | null;
  /** fraction of the act (0..1) → timeline time */
  p: (f: number) => number;
  /** fraction of the act → duration */
  d: (f: number) => number;
  q: <T extends Element = SVGElement>(sel: string) => T[];
  one: <T extends Element = SVGElement>(sel: string) => T;
}

type Vars = gsap.TweenVars;
const arr = (t: gsap.TweenTarget) => gsap.utils.toArray(t) as Element[];

/** Authored markup is the finished composition; `from` says where it starts. */
export function enterFrom(c: Ctx, t: gsap.TweenTarget, from: Vars, f: number, df: number, ease: string = EASE.wake, stagger = 0) {
  const els = arr(t);
  if (!els.length) return;
  // Make sure GSAP has parsed each element's transform: for <text>/<rect> a bare getProperty('y') would
  // otherwise return the geometry attribute instead of the transform offset.
  // The origin goes in first, while the element is still untransformed: setting it on an already rotated/scaled
  // element makes GSAP compensate with a hidden x/y offset that the tween back never removes.
  const origin: Vars = {};
  for (const key of ['svgOrigin', 'transformOrigin'] as const) if (key in from) origin[key] = from[key];
  gsap.set(els, { x: '+=0' });
  if (Object.keys(origin).length) gsap.set(els, origin);
  // Neither the start state nor the tween repeats the origin, so GSAP never re-anchors mid-move.
  const start: Vars = {};
  const to: Vars = {};
  for (const key of Object.keys(from)) {
    if (key in origin) continue;
    const v = (from as Record<string, unknown>)[key];
    start[key] = v;
    to[key] = key === 'autoAlpha' || key === 'opacity' ? 1 : gsap.getProperty(els[0], key);
    if (typeof v === 'string' && /^[+-]=/.test(v)) to[key] = (v[0] === '+' ? '-=' : '+=') + v.slice(2);
  }
  gsap.set(els, start);
  c.tl.to(els, { ...to, duration: c.d(df), ease, stagger: stagger ? c.d(stagger) : 0 }, c.p(f));
}

/** Fade + rise in. Hidden before, shown after. */
export function appear(c: Ctx, t: gsap.TweenTarget, f: number, df = 0.04, y = 14, stagger = 0) {
  enterFrom(c, t, { autoAlpha: 0, y: `+=${y}` as unknown as number }, f, df, EASE.approve, stagger);
}
export function vanish(c: Ctx, t: gsap.TweenTarget, f: number, df = 0.04, y = 0) {
  c.tl.to(arr(t), { autoAlpha: 0, y: y ? `+=${y}` : '+=0', duration: c.d(df), ease: EASE.sleep }, c.p(f));
}
export function fade(c: Ctx, t: gsap.TweenTarget, to: number, f: number, df = 0.04) {
  c.tl.to(arr(t), { opacity: to, duration: c.d(df), ease: EASE.linear }, c.p(f));
}

/** Draw a conduit (wall + bore) on. */
export function draw(c: Ctx, conduits: gsap.TweenTarget, f: number, df = 0.08, ease: string = EASE.transfer) {
  const paths = arr(conduits).flatMap((g) => [...g.querySelectorAll('.conduit__wall, .conduit__bore')]);
  if (!paths.length) return;
  gsap.set(paths, { drawSVG: '0% 0%' });
  c.tl.to(paths, { drawSVG: '0% 100%', duration: c.d(df), ease }, c.p(f));
  const flows = arr(conduits).flatMap((g) => [...g.querySelectorAll('.conduit__flow')]);
  gsap.set(flows, { autoAlpha: 0 });
  c.tl.to(flows, { autoAlpha: 1, duration: c.d(0.02) }, c.p(f + df));
}

/** Flip a data-state (the single source of visual meaning). Reversible on scrub. */
export function state(c: Ctx, t: gsap.TweenTarget, st: string, f: number) {
  c.tl.set(arr(t), { attr: { 'data-state': st } }, c.p(f));
}

/** Gate: closed = lock held / route blocked. */
export function gate(c: Ctx, g: Element, closed: boolean, f: number, df = 0.03) {
  const h = Number(g.getAttribute('data-h') ?? 130);
  const half = h / 2;
  const stub = 12;
  const top = g.querySelector('.gate__jaw--t')!;
  const bot = g.querySelector('.gate__jaw--b')!;
  const lock = g.querySelector('.gate__lock use');
  const t = c.p(f);
  const d = c.d(df);
  if (closed) {
    c.tl.to(top, { attr: { height: half }, duration: d, ease: EASE.lock }, t);
    c.tl.to(bot, { attr: { y: 0, height: half }, duration: d, ease: EASE.lock }, t);
    if (lock) c.tl.to(lock, { opacity: 1, duration: d }, t);
  } else {
    c.tl.to(top, { attr: { height: stub }, duration: d, ease: EASE.approve }, t);
    c.tl.to(bot, { attr: { y: half - stub, height: stub }, duration: d, ease: EASE.approve }, t);
    if (lock) c.tl.to(lock, { opacity: 0, duration: d }, t);
  }
}

/** Position of a path at fraction u (0..1). */
export function pt(path: SVGGeometryElement, u: number) {
  const L = path.getTotalLength();
  const p = path.getPointAtLength(Math.max(0, Math.min(1, u)) * L);
  return { x: p.x, y: p.y };
}
/** Fraction along `path` nearest to a point. */
export function uAt(path: SVGGeometryElement, x: number, y: number) {
  const L = path.getTotalLength();
  let best = 0;
  let bd = Infinity;
  const N = 400;
  for (let i = 0; i <= N; i++) {
    const p = path.getPointAtLength((i / N) * L);
    const d = (p.x - x) ** 2 + (p.y - y) ** 2;
    if (d < bd) {
      bd = d;
      best = i / N;
    }
  }
  return best;
}

/**
 * Transfer: move `target` along a path between two fractions.
 * The path is sampled once, so the tween is an ordinary x/y tween: fully reversible when scrubbing.
 * It always starts on the path (fromCurrent: false), never from wherever the target happened to be.
 */
export function along(
  c: Ctx,
  path: SVGGeometryElement,
  target: Element,
  f: number,
  df: number,
  o: { from?: number; to?: number; rotate?: boolean; ease?: string; offset?: [number, number] } = {},
) {
  const { from = 0, to = 1, rotate = false, ease = EASE.transfer, offset = [0, 0] } = o;
  const L = path.getTotalLength();
  const n = Math.max(10, Math.round((Math.abs(to - from) * L) / 12));
  const pts: Array<{ x: number; y: number }> = [];
  for (let i = 0; i <= n; i++) {
    const p = path.getPointAtLength((from + ((to - from) * i) / n) * L);
    pts.push({ x: p.x + offset[0], y: p.y + offset[1] });
  }
  c.tl.to(target, { motionPath: { path: pts, curviness: 0, autoRotate: rotate, fromCurrent: false }, duration: c.d(df), ease }, c.p(f));
}

/** Free move of an element (x/y/rotation/scale/alpha). */
export function move(c: Ctx, t: gsap.TweenTarget, vars: Vars, f: number, df: number, ease: string = EASE.transfer) {
  c.tl.to(arr(t), { ...vars, duration: c.d(df), ease }, c.p(f));
}

/** success = pulse + open + advance (callers add the "advance"). */
export function success(c: Ctx, t: Element, f: number) {
  const tl = c.tl;
  state(c, t, 'ok', f);
  tl.to(t, { scale: '*=1.25', duration: c.d(0.015), ease: EASE.approve }, c.p(f));
  tl.to(t, { scale: '/=1.25', duration: c.d(0.03), ease: 'power2.out' }, c.p(f + 0.015));
}
/** failure = shake + red. */
export function failure(c: Ctx, t: Element, f: number) {
  state(c, t, 'fail', f);
  const x = Number(gsap.getProperty(t, 'x'));
  const tl = c.tl;
  const d = c.d(0.008);
  [-6, 6, -4, 4, 0].forEach((dx, i) => tl.to(t, { x: `+=${dx}`, duration: d, ease: 'none' }, c.p(f) + d * i));
  void x;
}

/** wake: LED + mechanism power up, in sequence if several. */
export function wake(c: Ctx, t: gsap.TweenTarget, f: number, df = 0.04, stagger = 0, st = 'active') {
  const els = arr(t);
  els.forEach((el, i) => {
    const at = f + i * stagger;
    c.tl.set(el, { attr: { 'data-state': st } }, c.p(at));
    c.tl.to(el, { opacity: 1, duration: c.d(df), ease: EASE.wake }, c.p(at));
  });
}
export function sleep(c: Ctx, t: gsap.TweenTarget, f: number, df = 0.05, to = 0.35) {
  const els = arr(t);
  els.forEach((el) => {
    c.tl.set(el, { attr: { 'data-state': 'idle' } }, c.p(f));
    c.tl.to(el, { opacity: to, duration: c.d(df), ease: EASE.sleep }, c.p(f));
  });
}

/** verify: stepped progress on an attr width. */
export function verify(c: Ctx, bar: Element, w: number, f: number, df: number) {
  c.tl.fromTo(bar, { attr: { width: 0 } }, { attr: { width: w }, duration: c.d(df), ease: EASE.verify, immediateRender: false }, c.p(f));
}

/*
 * Camera layers (each scene's .world and the shared #thread-world) are driven through a plain {x, y, s} proxy
 * written straight to the transform attribute, not through GSAP's SVG transforms: those bake the pivot into
 * the matrix and re-derive it from the current (zoomed) matrix whenever the layer is re-parsed, so the scene
 * and the story capsule slowly drift off their marks. Here a world point P lands on screen at (x + s·Px, y + s·Py).
 */
type Cam = { x: number; y: number; s: number };
const cams = new WeakMap<Element, Cam>();
const cam = (el: Element) => {
  let k = cams.get(el);
  if (!k) cams.set(el, (k = { x: 0, y: 0, s: 1 }));
  return k;
};
const paint = (el: Element, k: Cam) => el.setAttribute('transform', `matrix(${k.s} 0 0 ${k.s} ${k.x} ${k.y})`);
/** Back to the untouched composition (on build and on teardown). */
export function resetCamera(layers: Element[]) {
  for (const el of layers) {
    cams.set(el, { x: 0, y: 0, s: 1 });
    el.removeAttribute('transform');
  }
}
function shot(el: Element, x: number, y: number, s: number, at: number, duration: number, c: Ctx) {
  const k = cam(el);
  c.tl.to(k, { x, y, s, duration, ease: EASE.camera, onUpdate: () => paint(el, k) }, at);
}

/** Scene camera: world and thread share the same transform so the capsule stays glued to the artwork. */
export function camera(c: Ctx, cx: number, cy: number, s: number, f: number, df: number, target: [number, number] = [800, 450]) {
  if (c.mode !== 'cinema') return;
  const x = target[0] - s * cx;
  const y = target[1] - s * cy;
  shot(c.world, x, y, s, c.p(f), c.d(df), c);
  if (c.threadWorld) shot(c.threadWorld, x, y, s, c.p(f), c.d(df), c);
}
/** Start the act already zoomed: the scene is set at once, the shared thread layer eases in during the transition. */
export function cameraStart(c: Ctx, cx: number, cy: number, s: number, target: [number, number] = [800, 450]) {
  if (c.mode !== 'cinema') return;
  const x = target[0] - s * cx;
  const y = target[1] - s * cy;
  const k = cam(c.world);
  Object.assign(k, { x, y, s });
  paint(c.world, k);
  if (c.threadWorld) shot(c.threadWorld, x, y, s, c.p(0) - TR * 2, TR * 2, c);
}
/** A quick press on a control: dip and recover. */
export function press(c: Ctx, t: Element, f: number) {
  c.tl.to(t, { opacity: 0.35, duration: c.d(0.008), ease: 'none' }, c.p(f));
  c.tl.to(t, { opacity: 1, duration: c.d(0.02), ease: 'power2.out' }, c.p(f) + c.d(0.008));
}
export function cameraHome(c: Ctx, f: number, df: number) {
  camera(c, 800, 450, 1, f, df);
}
/** Hold the act's last framing and only pull back home while the next act takes over (its cross-fade window). */
export function cameraHomeOnExit(c: Ctx) {
  if (c.mode !== 'cinema') return;
  shot(c.world, 0, 0, 1, c.p(1), TR * 2, c);
  if (c.threadWorld) shot(c.threadWorld, 0, 0, 1, c.p(1), TR * 2, c);
}

/** type text into a <text> element, character by character (stepped). */
export function typeText(c: Ctx, el: SVGTextElement, f: number, df: number) {
  const full = el.textContent ?? '';
  const o = { n: 0 };
  el.textContent = '';
  c.tl.fromTo(o, { n: 0 }, { n: full.length, duration: c.d(df), ease: 'none', immediateRender: false, onUpdate: () => { el.textContent = full.slice(0, Math.round(o.n)); } }, c.p(f));
}

export { gsap };
