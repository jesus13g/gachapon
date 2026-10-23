import { gsap } from 'gsap';
import { appear, vanish, enterFrom, draw, state, along, move, fade, uAt, success } from '../lib/motion';
import type { ActBuilder } from './acts';
import { EASE } from '../lib/tokens';

/* ─────────────── 00 · BOOT ─────────────── */
export const boot: ActBuilder = (c, { pose, gl }) => {
  const $ = (s: string) => c.sc.querySelector(s);
  const thread = c.thread;

  // time-based wake-up on load (not scrubbed): the machine comes alive, a capsule appears
  const machine = $('[data-k=machine]');
  const lines = [...c.sc.querySelectorAll('.boot-line')];
  gsap.set(lines, { autoAlpha: 0 });
  gsap.set($('[data-k=machine-conduits]'), { opacity: 0.35 });
  gsap.set(c.sc.querySelectorAll('.machine__led'), { opacity: 0.25 });
  const intro = gsap.timeline({ delay: 0.35 });
  intro.to(lines, { autoAlpha: 1, duration: 0.01, stagger: 0.28 });
  intro.to(c.sc.querySelectorAll('.machine__led'), { opacity: 1, duration: 0.2, stagger: 0.16 }, 0.2);
  intro.add(() => {
    machine?.classList.remove('is-dormant');
    machine?.querySelectorAll('.conduit').forEach((e) => e.setAttribute('data-state', 'active'));
    machine?.querySelectorAll('.machine__led').forEach((e, i) => e.setAttribute('data-state', i === 0 ? 'planner' : i === 2 ? 'exec' : 'active'));
  }, 1.1);
  intro.to($('[data-k=machine-conduits]'), { opacity: 1, duration: 0.6 }, 1.1);
  if (gl) {
    pose.a = 1;
    intro.fromTo(pose, { boot: 0, pop: 0.6 }, { boot: 1, pop: 1, duration: 1.1, ease: 'power3.out' }, 1.0);
  }

  // scrubbed: the capsule leaves the dome and settles at the centre, where the story begins
  if (c.mode !== 'cinema') return;
  if (gl) {
    gsap.set(thread, { autoAlpha: 0, x: 1130, y: 330 });
    c.tl.to(pose, { vx: 800, vy: 450, size: 120, rx: 0, ry: 0, rz: 0, idle: 0, duration: c.d(0.75), ease: EASE.transfer }, c.p(0.12));
    // the hidden 2D capsule travels with the 3D one, so the hand-off happens in place
    c.tl.to(thread, { x: 800, y: 450, duration: c.d(0.75), ease: EASE.transfer }, c.p(0.12));
    c.tl.set(thread, { autoAlpha: 1 }, c.p(0.88));
    c.tl.set(pose, { a: 0 }, c.p(0.88));
  } else {
    gsap.set(thread, { autoAlpha: 1, x: 1130, y: 330, scale: 1.35, rotation: -24 });
    c.tl.to(thread, { x: 800, y: 450, scale: 1, rotation: 0, duration: c.d(0.8), ease: EASE.transfer }, c.p(0.1));
  }
  c.tl.to(c.sc.querySelector('.boot-term'), { autoAlpha: 0, duration: c.d(0.2) }, c.p(0.3));
};

/* ─────────────── 01 · PROBLEM ─────────────── */
const CHAOS = [
  { x: 120, y: -260, r: -7 },
  { x: -90, y: -250, r: 9 },
  { x: -230, y: -190, r: -11 },
  { x: -190, y: 150, r: 8 },
  { x: 200, y: 330, r: -6 },
  { x: 380, y: 360, r: 12 },
];
export const problem: ActBuilder = (c) => {
  const terms = c.q('[data-k=term]');
  const centers = [[295, 590], [565, 770], [1275, 770], [1325, 550], [1105, 260], [285, 420]];
  terms.forEach((t, i) => {
    const o = CHAOS[i];
    const [cx, cy] = centers[i];
    enterFrom(c, t, { x: o.x, y: o.y, rotation: o.r, svgOrigin: `${cx} ${cy}` } as gsap.TweenVars, 0, 0.5, 'power3.inOut');
  });
  // jobs flying in at random
  const caps = c.q('[data-k=chaos-caps] .capsule');
  caps.forEach((cap, i) => {
    const tx = [300, 1180, 520, 1250, 900][i];
    const ty = [620, 340, 700, 600, 250][i];
    c.tl.to(cap, { x: tx, y: ty, duration: c.d(0.3), ease: 'power1.inOut' }, c.p(0.05 + i * 0.07));
  });
  vanish(c, c.q('[data-k=chaos-caps]'), 0.52, 0.06);
  // premium tags → order tags
  fade(c, c.q('.term__tag'), 0, 0.5, 0.06);
  fade(c, c.q('[data-k=tangle]'), 0, 0.5, 0.1);
  appear(c, c.q('[data-k=okTag]'), 0.68, 0.06, 8, 0.02);
  // hub assembles around the capsule
  enterFrom(c, c.q('.hub'), { scale: 0, autoAlpha: 0, svgOrigin: '800 450' } as gsap.TweenVars, 0.42, 0.2, EASE.lock);
  draw(c, c.q('[data-k=order] .conduit'), 0.58, 0.14);
  state(c, c.q('.term'), 'active', 0.62);
};

/* ─────────────── 02 · PLANNER + EXECUTOR ─────────────── */
export const roles: ActBuilder = (c) => {
  const th = c.thread;
  const planner = c.one('.agent--planner');
  const exec = c.one('.agent--executor');
  const path = (id: string) => c.one<SVGGeometryElement>(`#${id}`);
  const hub = [800, 450];

  // focus device: planner first, then executor
  gsap.set(exec, { opacity: 0.35 });
  fade(c, exec, 1, 0.3, 0.08);
  fade(c, planner, 0.45, 0.44, 0.06);
  fade(c, planner, 1, 0.62, 0.06);

  // objective → planner
  enterFrom(c, c.one('[data-k=obj]'), { x: -230, autoAlpha: 0 } as gsap.TweenVars, 0.0, 0.1, EASE.transfer);
  move(c, c.one('[data-k=obj]'), { x: '+=170', y: '+=68', scale: 0.2, autoAlpha: 0, svgOrigin: '195 390' } as gsap.TweenVars, 0.11, 0.05);
  appear(c, c.one('[data-k=plan]'), 0.15, 0.04, -10);
  vanish(c, c.one('[data-k=plan]'), 0.2, 0.03);

  // the job capsule is born at the planner
  c.tl.set(th, { autoAlpha: 0, x: 330, y: 450, scale: 0 }, c.p(0));
  c.tl.to(th, { autoAlpha: 1, scale: 1, duration: c.d(0.04), ease: EASE.lock }, c.p(0.18));
  state(c, th, 'planner', 0.18);

  // planner → hub
  appear(c, c.one('[data-k=tag-job]'), 0.2, 0.03);
  move(c, th, { x: 440, y: 430 }, 0.2, 0.03);
  along(c, path('r-job-l'), th, 0.23, 0.08);
  move(c, th, { x: hub[0], y: hub[1] }, 0.31, 0.04);
  state(c, th, 'active', 0.31);

  // hub → executor
  appear(c, c.one('[data-k=tag-run]'), 0.35, 0.03);
  move(c, th, { x: 900, y: 430 }, 0.35, 0.03);
  along(c, path('r-job-r'), th, 0.38, 0.08);
  move(c, th, { x: 1270, y: 450, scale: 0.7 }, 0.46, 0.04);
  state(c, th, 'exec', 0.46);

  // working (local model)
  c.tl.to(th, { scale: 0.85, duration: c.d(0.03), yoyo: true, repeat: 3, ease: 'sine.inOut' }, c.p(0.5));

  // evidence returns
  success(c, th, 0.64);
  appear(c, c.one('[data-k=tag-ev]'), 0.66, 0.03);
  move(c, th, { x: 1150, y: 500, scale: 1 }, 0.66, 0.03);
  along(c, path('r-ev-r'), th, 0.69, 0.08);
  move(c, th, { x: 800, y: 450 }, 0.77, 0.03);
  appear(c, c.one('[data-k=tag-review]'), 0.8, 0.03);
  move(c, th, { x: 700, y: 500 }, 0.8, 0.02);
  along(c, path('r-ev-l'), th, 0.82, 0.07);
  fade(c, c.one('[data-k=cost]'), 1, 0.7, 0.05);
  appear(c, c.one('[data-k=cost]'), 0.7, 0.06, 8);
  // back to the core for the next act
  move(c, th, { x: 800, y: 450 }, 0.91, 0.07);
  state(c, th, 'active', 0.95);
  void uAt;
};
