import { gsap } from 'gsap';
import { appear, vanish, enterFrom, draw, state, along, move, fade, uAt, success, failure, gate, verify, camera, cameraStart } from '../lib/motion';
import type { ActBuilder } from './acts';

type G = SVGGeometryElement;

/* ─────────────── 03 · THE JOB BUS ─────────────── */
export const bus: ActBuilder = (c) => {
  const th = c.thread;
  const loop = c.one<G>('#bus-loop');
  const intake = c.one<G>('#bus-intake');
  const stn = (s: string) => c.one(`[data-s=${s}] .station`);
  const uQ = uAt(loop, 360, 410);
  const uR = uAt(loop, 800, 410);
  const uV = uAt(loop, 1100, 650);
  const uC = uAt(loop, 360, 650);

  enterFrom(c, [c.one('[data-k=shell]'), c.one('[data-k=bus-labels]')], { autoAlpha: 0 }, 0, 0.08);
  draw(c, c.q('.conduit'), 0, 0.14);
  appear(c, c.q('[data-k=stn]'), 0.06, 0.05, 12, 0.025);
  appear(c, c.one('[data-k=queue-line]'), 0.1, 0.04);
  appear(c, [...c.q('[data-k=bay]'), ...c.q('.ghost'), c.one('[data-k=spurs]')], 0.44, 0.06, 10, 0.02);
  appear(c, c.one('[data-k=bus-note]'), 0.8, 0.05);

  state(c, th, 'idle', 0.08);
  along(c, intake, th, 0.1, 0.12);
  state(c, stn('queued'), 'active', 0.22);
  state(c, stn('queued'), 'idle', 0.3);
  along(c, loop, th, 0.3, 0.1, { from: uQ, to: uR, rotate: true });
  state(c, th, 'exec', 0.4);
  state(c, stn('running'), 'active', 0.4);
  state(c, stn('running'), 'idle', 0.52);
  along(c, loop, th, 0.52, 0.17, { from: uR, to: uV, rotate: true });
  state(c, th, 'planner', 0.69);
  state(c, stn('review'), 'active', 0.7);
  state(c, stn('review'), 'idle', 0.78);
  along(c, loop, th, 0.78, 0.14, { from: uV, to: uC, rotate: true });
  success(c, th, 0.92);
  state(c, stn('closed'), 'ok', 0.92);
  move(c, th, { rotation: 0 }, 0.94, 0.04);
};

/* ─────────────── 04 · LOCKS + WORKTREES ─────────────── */
export const locks: ActBuilder = (c) => {
  const th = c.thread;
  const lane = c.one<G>('#lk-lane');
  const fork = c.one<G>('#lk-fork');
  const capB = c.one('.capB');
  const gA = c.one('[data-k=gateA] .gate');
  const gB = c.one('[data-k=gateB] .gate');
  const bars = c.q('.wt__bar');
  const u = (x: number) => uAt(lane, x, 450);

  draw(c, c.one('#lk-lane').parentElement!, 0, 0.1);
  appear(c, [c.one('[data-k=wtA]'), c.one('[data-k=gateA]')], 0.04, 0.06);
  c.tl.set(c.one('[data-k=lock-text]'), { autoAlpha: 0 }, c.p(0));
  gsap.set([c.one('[data-k=lock-text]'), c.one('[data-k=blocked-text]'), c.one('[data-k=branch-label]'), c.one('[data-k=wtB]'), c.one('[data-k=gateB]')], { autoAlpha: 0 });
  gsap.set(fork.parentElement, { autoAlpha: 0 });

  // A takes the lock
  state(c, th, 'exec', 0.05);
  along(c, lane, th, 0.07, 0.2, { from: u(200), to: u(770), rotate: true });
  gate(c, gA, true, 0.29);
  state(c, gA, 'blocked', 0.29);
  appear(c, c.one('[data-k=lock-text]'), 0.3, 0.04);
  move(c, th, { x: 1010, y: 450, rotation: 0 }, 0.3, 0.06);
  verify(c, bars[0], 472, 0.36, 0.6);

  // B is turned away. It rolls in only once A has left the start of the lane, so the two never overlap.
  gsap.set(capB, { x: 120, y: 450 });
  enterFrom(c, capB, { x: '-=220', autoAlpha: 0 } as unknown as gsap.TweenVars, 0.12, 0.08, 'power2.out');
  along(c, lane, capB, 0.32, 0.12, { from: u(130), to: u(630), rotate: true });
  failure(c, capB, 0.45);
  appear(c, c.one('[data-k=blocked-text]'), 0.47, 0.04);

  // fork: own worktree
  c.tl.to(fork.parentElement, { autoAlpha: 1, duration: 0.01 }, c.p(0.55));
  draw(c, fork.parentElement!, 0.55, 0.1);
  appear(c, c.one('[data-k=branch-label]'), 0.56, 0.04);
  appear(c, [c.one('[data-k=wtB]'), c.one('[data-k=gateB]')], 0.62, 0.06);
  vanish(c, c.one('[data-k=blocked-text]'), 0.64, 0.03);
  state(c, capB, 'exec', 0.66);
  along(c, fork, capB, 0.68, 0.12, { rotate: true });
  move(c, capB, { x: 1010, y: 712, rotation: 0, scale: 0.9 }, 0.8, 0.05);
  // inside its worktree the «job B» label would sit on top of «executor B»
  vanish(c, capB.querySelector('.cap-label')!, 0.8, 0.03);
  gate(c, gB, true, 0.83);
  verify(c, bars[1], 472, 0.84, 0.13);
};

/* ─────────────── 05 · THREE WAYS TO WORK ─────────────── */
export const modes: ActBuilder = (c) => {
  const th = c.thread;
  const P = (id: string) => c.one<G>('#' + id);
  const mLane = P('m-lane');
  const bLane = P('b-lane');
  const rLane = P('r-lane');
  const ring = P('m-ring');
  const capB = c.one('.capBuild');
  const capR = c.one('.capRoutine');
  const gateM = c.one('[data-k=gateM] .gate');
  const stnS = c.one('[data-k=stn-success] .station');
  const fills = c.q('.phase__fill');
  const phaseN = c.q('[data-k=phase]');

  const par = (id: string) => P(id).parentElement!;
  draw(c, par('m-lane'), 0, 0.08);
  draw(c, par('b-lane'), 0.02, 0.08);
  draw(c, par('r-lane'), 0.04, 0.08);
  appear(c, c.one('[data-k=lane-mission]'), 0.0, 0.05);
  appear(c, c.one('[data-k=lane-build]'), 0.3, 0.05);
  appear(c, c.one('[data-k=lane-routine]'), 0.62, 0.05);
  gsap.set([capB, capR], { autoAlpha: 0 });

  // already framed on the mission lane when the act arrives (the home framing would put merge under the rail)
  cameraStart(c, 1050, 345, 1.2, [960, 470]);
  camera(c, 1150, 540, 1.18, 0.31, 0.06, [930, 470]);
  // the routine lane sits low enough that split and the dimmed BUILD label stay clear of the copy
  camera(c, 1180, 735, 1.2, 0.64, 0.06, [960, 600]);
  // the full composition runs from split (520) to merge (1574): frame it on its own centre, clear of the chapter rail
  camera(c, 1000, 450, 1, 0.9, 0.08);
  // lanes that are not in focus step back so the copy stays legible
  const dimM = [c.one('[data-k=lane-mission]'), par('m-lane')];
  const dimB = [c.one('[data-k=lane-build]'), par('b-lane')];
  fade(c, dimM, 0.12, 0.31, 0.04);
  fade(c, dimB, 0.12, 0.63, 0.04);
  fade(c, [...dimM, ...dimB], 1, 0.9, 0.07);

  // MISSION: try → verify → fail → evidence → retry → success
  const uIn = uAt(mLane, 952, 345);
  const uOut = uAt(mLane, 1068, 345);
  const uGate = uAt(mLane, 1232, 345);
  const uEnd = uAt(mLane, 1410, 345);
  state(c, th, 'exec', 0.03);
  along(c, mLane, th, 0.04, 0.06, { from: 0, to: uIn });
  move(c, th, { x: 1010, y: 287 }, 0.1, 0.02);
  along(c, ring, th, 0.12, 0.1, { ease: 'none' });
  state(c, th, 'wait', 0.14);
  failure(c, th, 0.16);
  state(c, th, 'active', 0.18);
  state(c, th, 'exec', 0.2);
  along(c, ring, th, 0.22, 0.03, { to: 0.25, ease: 'none' });
  const crit = c.one('[data-k=crit-v]');
  gsap.set(crit, { fill: '#ff5d5d' });
  gsap.set(c.one('[data-k=crit-ok]'), { autoAlpha: 0 });
  success(c, th, 0.26);
  c.tl.to(crit, { fill: '#3ddc84', duration: c.d(0.01) }, c.p(0.26));
  c.tl.to(c.one('[data-k=crit-ok]'), { autoAlpha: 1, duration: c.d(0.01) }, c.p(0.26));
  gate(c, gateM, false, 0.27);
  state(c, gateM, 'ok', 0.27);
  along(c, mLane, th, 0.26, 0.05, { from: uOut, to: uGate });
  along(c, mLane, th, 0.31, 0.02, { from: uGate, to: uEnd });
  state(c, stnS, 'ok', 0.33);

  // BUILD: 01 → 02 → 03 → review → 04 → 05
  const bu = (x: number) => uAt(bLane, x, 540);
  c.tl.set(capB, { autoAlpha: 1 }, c.p(0.32));
  const stops = [868, 988, 1108, 1208, 1310, 1430];
  along(c, bLane, capB, 0.34, 0.04, { from: 0, to: bu(stops[0]) });
  const seq: Array<[number, number]> = [[0.38, 0.03], [0.44, 0.03], [0.5, 0.03]];
  seq.forEach(([f, df], i) => {
    verify(c, fills[i], 84, f, df);
    along(c, bLane, capB, f + df, 0.03, { from: bu(stops[i]), to: bu(stops[i + 1]) });
  });
  appear(c, c.one('[data-k=reform]'), 0.54, 0.02);
  appear(c, c.one('[data-k=reform-t]'), 0.54, 0.02);
  vanish(c, [c.one('[data-k=reform]'), c.one('[data-k=reform-t]')], 0.58, 0.02);
  appear(c, c.one('[data-k=cont-t]'), 0.58, 0.02);
  state(c, capB, 'ok', 0.58);
  along(c, bLane, capB, 0.58, 0.03, { from: bu(stops[3]), to: bu(stops[4]) });
  verify(c, fills[3], 84, 0.61, 0.02);
  along(c, bLane, capB, 0.63, 0.02, { from: bu(stops[4]), to: bu(stops[5]) });
  verify(c, fills[4], 84, 0.65, 0.02);
  void phaseN;

  // ROUTINE: define once, run whenever
  const ru = (x: number) => uAt(rLane, x, 735);
  c.tl.set(capR, { autoAlpha: 1 }, c.p(0.62));
  along(c, rLane, capR, 0.64, 0.05, { from: 0, to: ru(900) });
  vanish(c, capR, 0.7, 0.01);
  enterFrom(c, c.one('[data-k=rt-card]'), { scale: 0, autoAlpha: 0, transformOrigin: '50% 50%' } as gsap.TweenVars, 0.69, 0.04, 'back.out(2)');
  appear(c, [c.one('[data-k=calendar]'), c.one('[data-k=sched-t]')], 0.74, 0.04);
  c.q('[data-k=run]').forEach((r, i) => {
    enterFrom(c, r, { scale: 0, autoAlpha: 0, transformOrigin: '50% 50%' } as gsap.TweenVars, 0.78 + i * 0.04, 0.03, 'back.out(3)');
  });

  // converge
  const mEnd = par('m-end');
  void mEnd;
  along(c, P('m-end'), th, 0.91, 0.06, { rotate: true });
  along(c, P('b-end'), capB, 0.92, 0.04);
  vanish(c, capB, 0.97, 0.02);
  move(c, th, { rotation: 0 }, 0.97, 0.02);
  fade(c, th, 1, 0.97, 0.01);
};
