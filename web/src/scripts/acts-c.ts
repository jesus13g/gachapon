import { gsap } from 'gsap';
import { appear, vanish, enterFrom, draw, state, along, move, fade, uAt, success, failure, gate, verify, camera, cameraHome, cameraStart, press, wake, sleep } from '../lib/motion';
import type { ActBuilder } from './acts';
import { TR } from '../lib/tokens';

type G = SVGGeometryElement;

/* ─────────────── 06 · HUMAN CONTROL ─────────────── */
export const control: ActBuilder = (c) => {
  const th = c.thread;
  const lane = c.one<G>('#cl-lane');
  const irrev = c.one<G>('#cl-irrev');
  const barrier = c.one('.barrier');
  const wall = c.one('.barrier__wall');
  const capX = c.one('.capX');
  const cursor = c.one('.cursor');
  const card = c.one('[data-k=card]');
  const u = (x: number) => uAt(lane, x, 470);

  draw(c, [lane.parentElement!], 0, 0.08);
  appear(c, [c.one('.agent'), barrier, c.one('[data-k=barrier-label]')], 0.02, 0.05);
  appear(c, [c.one('[data-k=user]'), c.one('[data-k=user-link]')], 0.08, 0.05);
  gsap.set([card, c.one('[data-k=card-link]'), c.one('[data-k=sim]'), c.one('[data-k=irrev]'), cursor, capX], { autoAlpha: 0 });

  // 1 · the change travels and stops at the barrier
  state(c, th, 'active', 0.04);
  // the change leaves from beside the agent, never drawn on top of it
  along(c, lane, th, 0.06, 0.2, { from: u(360), to: u(836), rotate: true });
  failure(c, th, 0.27);
  appear(c, c.one('[data-k=stn-applied]'), 0.28, 0.04);
  appear(c, [card, c.one('[data-k=card-link]')], 0.29, 0.05, 16);
  state(c, th, 'wait', 0.34);
  c.tl.set(th, { rotation: 0 }, c.p(0.34));

  // 2 · you simulate, then accept
  const sim = c.one('[data-k=btn-sim] rect');
  const acc = c.one('[data-k=btn-acc] rect');
  const [sx, sy] = [920 + 74, 120 + 143];
  const [ax, ay] = [920 + 204, 120 + 143];
  c.tl.set(cursor, { x: 1180, y: 380, autoAlpha: 1 }, c.p(0.34));
  move(c, cursor, { x: sx, y: sy }, 0.34, 0.05);
  press(c, c.one('[data-k=btn-sim]'), 0.4);
  appear(c, c.one('[data-k=sim]'), 0.42, 0.03);
  move(c, cursor, { x: ax, y: ay }, 0.46, 0.05);
  press(c, c.one('[data-k=btn-acc]'), 0.52);
  c.tl.to(acc, { attr: { fill: '#3ddc84', 'fill-opacity': 0.25 }, duration: c.d(0.01) }, c.p(0.52));
  void sim;
  vanish(c, cursor, 0.56, 0.03);
  state(c, barrier, 'ok', 0.55);
  c.tl.to(wall, { opacity: 0.12, duration: c.d(0.03) }, c.p(0.55));
  c.tl.set(th, { x: 836, y: 470 }, c.p(0.55));
  success(c, th, 0.56);
  along(c, lane, th, 0.57, 0.14, { from: u(836), to: u(1250), rotate: true });
  state(c, c.one('[data-k=stn-applied] .station'), 'ok', 0.71);
  move(c, th, { rotation: 0 }, 0.72, 0.02);

  // 3 · an irreversible one is diverted to a real terminal
  c.tl.set(capX, { autoAlpha: 1, x: 360, y: 470 }, c.p(0.68));
  along(c, lane, capX, 0.68, 0.08, { from: u(360), to: u(600), rotate: true });
  along(c, irrev, capX, 0.77, 0.1, { rotate: true });
  appear(c, c.one('[data-k=irrev]'), 0.8, 0.06, 12);
  vanish(c, c.one('[data-k=card]'), 0.82, 0.04);
  vanish(c, c.one('[data-k=card-link]'), 0.82, 0.04);
  failure(c, capX, 0.88);
};

/* ─────────────── 07 · TEAMS ─────────────── */
export const teams: ActBuilder = (c) => {
  const th = c.thread;
  const P = (id: string) => c.one<G>('#' + id);
  const planner = c.one('.agent--planner');
  const execs = c.q('[data-k=exec]');
  const wts = c.q('[data-k=wt]');
  const reviewer = c.one('[data-k=reviewer]');
  const capB = c.one('.capB');
  const capC = c.one('.capC');
  const cursor = c.one('.cursor');
  const wake_ = c.one('[data-k=wake]');
  const sleep_ = c.one('[data-k=sleep]');
  const bars = c.q('.wt__bar');
  const outs = c.one('[data-k=conduits-out]');
  const ins = c.one('[data-k=conduits-in]');

  // the scene starts as "one planner, one executor"
  cameraStart(c, 520, 445, 1.3, [900, 470]);
  gsap.set([execs[1], execs[2], wts[0], wts[1], wts[2], reviewer, outs, ins, capB, capC, sleep_, cursor], { autoAlpha: 0 });
    const exAg = execs.map((e) => e.querySelector('.agent')!);
  gsap.set([planner, ...exAg], { opacity: 0.35 });
  c.q('.agent').forEach((a) => a.setAttribute('data-state', 'idle'));
  // the story capsule glides into the planner during the transition and is absorbed there (it fades over the
  // second half of the glide instead of popping out on top of the planner); it comes back as job A, an
  // executor's job like its companions, not still carrying 06's green
  if (c.mode === 'cinema') c.tl.to(th, { autoAlpha: 0, duration: TR, ease: 'power2.in' }, c.p(0) - TR);
  c.tl.set(th, { autoAlpha: 0 }, c.p(0));
  state(c, th, 'exec', 0);

  // wake team (your decision)
  c.tl.set(cursor, { x: 520, y: 420, autoAlpha: 1 }, c.p(0.02));
  move(c, cursor, { x: 235, y: 360 }, 0.02, 0.05);
  press(c, wake_, 0.08);
  wake(c, planner, 0.1, 0.03, 0, 'planner');
  wake(c, exAg[0], 0.14, 0.03, 0, 'exec');
  vanish(c, cursor, 0.12, 0.02);
  appear(c, wts[0], 0.15, 0.04);
  appear(c, outs, 0.15, 0.03);

  // the camera pulls back: it was only part of a bigger machine
  cameraHome(c, 0.18, 0.14);
  appear(c, [execs[1], wts[1]], 0.24, 0.04);
  wake(c, exAg[1], 0.25, 0.03, 0, 'exec');
  appear(c, [execs[2], wts[2]], 0.29, 0.04);
  wake(c, exAg[2], 0.3, 0.03, 0, 'exec');
  appear(c, reviewer, 0.34, 0.04);
  wake(c, reviewer.querySelector('.agent')!, 0.35, 0.03, 0, 'planner');
  appear(c, ins, 0.35, 0.03);

  // distribute: three jobs, three routes, three worktrees
  // each job shows up only as it leaves the planner, far enough behind the previous one not to overlap it
  const tg: Array<[Element, string]> = [[th, 'tc-a'], [capB, 'tc-b'], [capC, 'tc-c']];
  const exY = [370, 520, 670];
  tg.forEach(([el, id], i) => {
    const go = 0.4 + i * 0.04;
    c.tl.set(el, { autoAlpha: 1, x: 370, y: 520, rotation: 0 }, c.p(go));
    along(c, P(id), el, go, 0.08, { rotate: true });
    // across the executor to the mouth of its outgoing conduit, where the next transfer starts (no jump)
    move(c, el, { x: 836, y: exY[i], rotation: 0 }, go + 0.08, 0.02);
    along(c, P(['ex-a', 'ex-b', 'ex-c'][i]), el, go + 0.1, 0.05, { rotate: true });
    // inside the worktree the job rides its commit line, smaller, clear of the tag and the progress bar
    move(c, el, { x: 1110, y: exY[i] + 8, rotation: 0, scale: 0.7 }, go + 0.15, 0.02);
    verify(c, bars[i], 252, 0.58 + i * 0.03, 0.16);
  });

  // results converge on the reviewer
  const wr = ['wr-a', 'wr-b', 'wr-c'];
  // one after another, and each companion is absorbed as it reaches the reviewer, so they never pile up
  [th, capB, capC].forEach((el, i) => {
    success(c, el, 0.8 + i * 0.03);
    // off the commit line to the worktree's exit, where its conduit to the reviewer starts: without it the
    // capsule jumps 150 units there (and scrolling back would leave it stuck at that exit)
    move(c, el, { x: 1260, y: exY[i], rotation: 0 }, 0.8 + i * 0.03, 0.01);
    along(c, P(wr[i]), el, 0.81 + i * 0.03, 0.05, { rotate: true });
    if (i > 0) vanish(c, el, 0.84 + i * 0.03, 0.02);
  });
  move(c, th, { rotation: 0, scale: 1, x: 1480, y: 520 }, 0.86, 0.03);

  // and everyone goes back to sleep (your decision)
  vanish(c, wake_, 0.9, 0.02);
  appear(c, sleep_, 0.9, 0.03, 0);
  c.tl.set(cursor, { x: 520, y: 420, autoAlpha: 1 }, c.p(0.91));
  move(c, cursor, { x: 235, y: 360 }, 0.91, 0.03);
  press(c, sleep_, 0.945);
  vanish(c, cursor, 0.95, 0.02);
  sleep(c, [planner, ...exAg, reviewer.querySelector('.agent')!], 0.955, 0.04, 0.4);
  void camera;
  void fade;
  void enterFrom;
};

/* ─────────────── 08 · ROUTINES ─────────────── */
export const routines: ActBuilder = (c) => {
  const th = c.thread;
  const P = (id: string) => c.one<G>('#' + id);
  const plates = c.q('[data-k=plate]');
  const capF = c.one('.capF');
  const capRun = c.one('.capRun');
  const cursor = c.one('.cursor');
  const okCells = c.q('[data-k=cell-ok]');
  const failCell = c.one('[data-k=cell-fail]');
  const pkg = c.one('[data-k=pkg]');
  const planner = c.one('.agent--planner');
  const barrier = c.one('.barrier');

  gsap.set([plates[1], plates[2], c.one('[data-k=calendar]'), pkg, c.one('[data-k=confirm]'), c.one('[data-k=user]'), capF, capRun, cursor, ...okCells, failCell, c.one('[data-k=failed-t]'), c.one('[data-k=planner-t]'), planner], { autoAlpha: 0 });
  gsap.set(c.one('#rt-cal').parentElement, { autoAlpha: 0 });
  gsap.set([P('rt-fail').parentElement, P('rt-pkg').parentElement, P('rt-back').parentElement], { autoAlpha: 0 });
  c.tl.set(th, { autoAlpha: 1 }, c.p(0));

  // a good run is folded into routine v1
  move(c, th, { x: 450, y: 540, scale: 0.4 }, 0.02, 0.1);
  vanish(c, th, 0.11, 0.02);
  enterFrom(c, plates[0], { scale: 0.6, autoAlpha: 0, transformOrigin: '50% 50%' } as gsap.TweenVars, 0.1, 0.04, 'back.out(2.2)');

  // then it runs by itself, on a calendar
  c.tl.to(c.one('#rt-cal').parentElement, { autoAlpha: 1, duration: 0.01 }, c.p(0.14));
  draw(c, c.one('#rt-cal').parentElement!, 0.14, 0.05);
  appear(c, c.one('[data-k=calendar]'), 0.18, 0.04);
  okCells.slice(0, 4).forEach((cell, i) => {
    const f = 0.24 + i * 0.07;
    c.tl.set(capRun, { autoAlpha: 1, x: 640, y: 520 }, c.p(f));
    along(c, P('rt-cal'), capRun, f, 0.05, { rotate: true });
    enterFrom(c, cell, { scale: 0, autoAlpha: 0, transformOrigin: '50% 50%' } as gsap.TweenVars, f + 0.05, 0.02, 'back.out(3)');
    vanish(c, capRun, f + 0.05, 0.01);
  });

  // v2: a new, explicit version stacks on top
  enterFrom(c, plates[1], { y: '-=90', autoAlpha: 0 } as gsap.TweenVars, 0.52, 0.05, 'power3.out');
  okCells.slice(4).forEach((cell, i) => enterFrom(c, cell, { scale: 0, autoAlpha: 0, transformOrigin: '50% 50%' } as gsap.TweenVars, 0.58 + i * 0.015, 0.02, 'back.out(3)'));

  // a run fails → correction package → waits for you
  c.tl.set(capF, { autoAlpha: 1, x: 640, y: 520 }, c.p(0.62));
  along(c, P('rt-cal'), capF, 0.62, 0.05, { rotate: true });
  enterFrom(c, failCell, { scale: 0, autoAlpha: 0, transformOrigin: '50% 50%' } as gsap.TweenVars, 0.67, 0.02, 'back.out(3)');
  appear(c, c.one('[data-k=failed-t]'), 0.68, 0.03);
  c.tl.to(P('rt-fail').parentElement, { autoAlpha: 1, duration: 0.01 }, c.p(0.7));
  move(c, capF, { x: 1265, y: 540, rotation: 0 }, 0.67, 0.03);
  draw(c, P('rt-fail').parentElement!, 0.7, 0.03);
  c.tl.set(capF, { rotation: 90 }, c.p(0.7));
  move(c, capF, { y: 700, rotation: 90 }, 0.7, 0.04);
  appear(c, pkg, 0.73, 0.04);
  state(c, capF, 'wait', 0.75);
  appear(c, [c.one('[data-k=confirm]'), c.one('[data-k=user]'), planner, c.one('[data-k=planner-t]')], 0.76, 0.04);
  // it waits at the package's door, where its conduit starts, not on top of the package's text
  move(c, capF, { x: 1050, y: 750, rotation: 0 }, 0.75, 0.03);

  // you confirm → the planner corrects → v3
  c.tl.set(cursor, { x: 1180, y: 880, autoAlpha: 1 }, c.p(0.8));
  move(c, cursor, { x: 975, y: 850 }, 0.8, 0.04);
  press(c, c.one('[data-k=user]'), 0.85);
  vanish(c, cursor, 0.87, 0.02);
  state(c, barrier, 'ok', 0.86);
  c.tl.to(c.one('.barrier__wall'), { opacity: 0.12, duration: c.d(0.02) }, c.p(0.86));
  c.tl.to(P('rt-pkg').parentElement, { autoAlpha: 1, duration: 0.01 }, c.p(0.86));
  state(c, planner, 'planner', 0.88);
  state(c, capF, 'planner', 0.88);
  along(c, P('rt-pkg'), capF, 0.87, 0.05, { rotate: true });
  c.tl.to(P('rt-back').parentElement, { autoAlpha: 1, duration: 0.01 }, c.p(0.91));
  along(c, P('rt-back'), capF, 0.92, 0.04, { rotate: true });
  vanish(c, capF, 0.96, 0.01);
  enterFrom(c, plates[2], { y: '-=90', autoAlpha: 0 } as gsap.TweenVars, 0.95, 0.04, 'power3.out');
  // the story capsule comes back out at the mouth of the calendar conduit, beside v3 rather than on top of it
  c.tl.set(th, { x: 660, y: 520, scale: 1 }, c.p(0.97));
  c.tl.to(th, { autoAlpha: 1, duration: 0.01 }, c.p(0.97));
  void success; void sleep; void gate; void uAt;
};
