import { gsap } from 'gsap';
import { appear, vanish, enterFrom, draw, state, along, move, fade, uAt, camera, cameraHome, cameraStart } from '../lib/motion';
import type { ActBuilder } from './acts';

type G = SVGGeometryElement;

/* ─────────────── 12 · OBSERVABILITY ─────────────── */
export const journal: ActBuilder = (c) => {
  const th = c.thread;
  const line = c.one<G>('#jr-line');
  const evs = c.q('[data-k=ev]');
  const spokes = c.one('[data-k=spokes]');
  const X = (i: number) => 160 + i * 145;

  gsap.set([spokes, c.one('[data-k=corr-arc]'), c.one('[data-k=corr-t]'), c.one('[data-k=append]')], { autoAlpha: 0 });
  draw(c, line.parentElement!, 0, 0.5, 'none');
  appear(c, c.one('[data-k=ticks]'), 0.02, 0.2);
  appear(c, c.one('[data-k=emit]'), 0.1, 0.1);
  evs.slice(0, 8).forEach((e, i) => appear(c, e, 0.04 + i * 0.055, 0.04, 12));
  appear(c, c.one('[data-k=append]'), 0.45, 0.04);

  // the story capsule rides the journal as events are appended
  state(c, th, 'active', 0);
  along(c, line, th, 0.03, 0.42, { from: uAt(line, 160, 560), to: uAt(line, X(7), 560), ease: 'none' });

  // one event, everything attached
  const others = evs.filter((_, i) => i !== 3 && i < 8);
  fade(c, others, 0.15, 0.5, 0.03);
  appear(c, spokes, 0.52, 0.12, 0, 0);
  vanish(c, spokes, 0.7, 0.04);
  fade(c, others, 1, 0.7, 0.04);

  // a correction is a new event; nothing is erased
  appear(c, evs[8], 0.76, 0.04, 12);
  c.tl.to(c.one('[data-k=corr-arc]'), { autoAlpha: 1, duration: c.d(0.02) }, c.p(0.8));
  draw(c, [], 0.8, 0.05);
  appear(c, c.one('[data-k=corr-t]'), 0.82, 0.04);
  appear(c, evs[9], 0.88, 0.04, 12);
  along(c, line, th, 0.88, 0.08, { from: uAt(line, X(7), 560), to: uAt(line, X(9), 560) });
  void enterFrom; void camera; void cameraHome; void cameraStart; void move;
};

/* ─────────────── 13 · INTERFACES ─────────────── */
export const interfaces: ActBuilder = (c) => {
  const th = c.thread;
  const panel = c.one('[data-k=ui-panel]');
  const cli = c.one('[data-k=ui-cli]');
  const tui = c.one('[data-k=ui-tui]');
  const cards = c.q('[data-k=card]');
  const bar = c.one('[data-k=tab-bar]');
  const tabs = c.q('[data-k=tab] text');
  // each screen flips about its own centre; the origin is set once, before any scale, so GSAP never re-anchors it
  const o = '50% 50%';
  gsap.set(tabs, { fill: '#a3adb8' });
  gsap.set(tabs[0], { fill: '#ff9a3c' });
  gsap.set([cli, tui], { transformOrigin: o });
  gsap.set([cli, tui], { scaleX: 0, autoAlpha: 0 });

  // the machine becomes the product: the capsule turns into a card on the board
  move(c, th, { x: 598, y: 456, scale: 0.4 }, 0.0, 0.1);
  vanish(c, th, 0.11, 0.02);
  enterFrom(c, panel, { autoAlpha: 0, y: '+=30', scale: 0.96, transformOrigin: o } as gsap.TweenVars, 0.02, 0.1, 'power3.out');
  appear(c, cards, 0.12, 0.03, 10, 0.02);
  appear(c, c.one('[data-k=tabs]'), 0.12, 0.04);
  // jobs move across the board
  c.tl.to(cards[0], { x: '+=156', duration: c.d(0.1), ease: 'power2.inOut' }, c.p(0.3));
  c.tl.to(cards[2], { x: '+=156', duration: c.d(0.1), ease: 'power2.inOut' }, c.p(0.38));

  // flip: web panel → CLI → TUI
  const flip = (from: Element, to: Element, f: number) => {
    c.tl.to(from, { scaleX: 0, autoAlpha: 0, duration: c.d(0.05), ease: 'power2.in' }, c.p(f));
    c.tl.to(to, { scaleX: 1, autoAlpha: 1, duration: c.d(0.06), ease: 'power2.out' }, c.p(f + 0.05));
  };
  flip(panel, cli, 0.5);
  c.tl.to(bar, { x: '+=190', duration: c.d(0.05) }, c.p(0.5));
  c.tl.to(tabs[0], { fill: '#a3adb8', duration: c.d(0.03) }, c.p(0.5));
  c.tl.to(tabs[1], { fill: '#ff9a3c', duration: c.d(0.03) }, c.p(0.52));
  flip(cli, tui, 0.76);
  c.tl.to(bar, { x: '+=190', duration: c.d(0.05) }, c.p(0.76));
  c.tl.to(tabs[1], { fill: '#a3adb8', duration: c.d(0.03) }, c.p(0.76));
  c.tl.to(tabs[2], { fill: '#ff9a3c', duration: c.d(0.03) }, c.p(0.78));
  void along; void uAt; void fade; void state;
};

/* ─────────────── 14 · THE WHOLE SYSTEM ─────────────── */
export const system: ActBuilder = (c) => {
  const th = c.thread;
  const q = (k: string) => c.one(`[data-k=${k}]`);
  const benefits = c.q('[data-k=benefit]');
  const pathR = c.one<G>('#sy-job-r');

  cameraStart(c, 800, 545, 1.25);
  c.tl.set(th, { autoAlpha: 1, x: 800, y: 545 }, c.p(0));
  enterFrom(c, c.one('.hub'), { scale: 0, autoAlpha: 0, transformOrigin: '100px 100px' } as gsap.TweenVars, 0.0, 0.06, 'back.out(2)');
  appear(c, q('hub-label'), 0.04, 0.03);
  appear(c, c.q('.agent--planner, .agent--executor'), 0.06, 0.05, 0, 0.02);
  draw(c, c.q('.conduit'), 0.1, 0.08);
  cameraHome(c, 0.14, 0.2);
  const seq: Array<[string, number]> = [['mcp', 0.2], ['locks', 0.26], ['teams', 0.31], ['journal', 0.36], ['routines', 0.41], ['vault', 0.46], ['tools', 0.5], ['user', 0.54]];
  seq.forEach(([k, f]) => appear(c, q(k), f, 0.04, 10));
  appear(c, q('wires'), 0.38, 0.2, 0);
  // the system keeps working while its virtues are named
  state(c, th, 'exec', 0.5);
  along(c, pathR, th, 0.52, 0.14, { rotate: true });
  state(c, th, 'ok', 0.67);
  along(c, pathR, th, 0.68, 0.14, { from: 1, to: 0, rotate: true });
  move(c, th, { x: 800, y: 545, rotation: 0 }, 0.82, 0.04);
  state(c, th, 'active', 0.86);
  benefits.forEach((b, i) => appear(c, b, 0.6 + i * 0.06, 0.04, 14));
  void camera; void fade; void vanish; void uAt;
};

/* ─────────────── 15 · DONE ─────────────── */
export const final: ActBuilder = (c, { pose, gl }) => {
  const th = c.thread;
  const done = c.sc.ownerDocument.querySelector<SVGTextElement>('.done-text')!;
  const machine = c.one('[data-k=machine2]');
  const copy = c.sc.querySelector('.final-copy')!;
  // DONE shows up under the opened capsule, never printed across it
  gsap.set(done, { x: 800, y: 770, scale: 1, autoAlpha: 0 });
  gsap.set(machine, { autoAlpha: 0 });
  gsap.set(copy, { autoAlpha: 0, y: 26 });
  if (c.mode !== 'cinema') {
    appear(c, machine, 0.05, 0.1);
    appear(c, copy, 0.3, 0.1);
    return;
  }
  if (gl) {
    c.tl.set(pose, { vx: 800, vy: 545, size: 120, rx: 0, ry: 0, rz: 0, open: 0, idle: 0, a: 0 }, c.p(0) - 0.01);
    c.tl.to(pose, { a: 1, duration: 0.4 }, c.p(0.02));
    c.tl.to(th, { autoAlpha: 0, duration: 0.4 }, c.p(0.02));
    c.tl.to(pose, { size: 560, ry: 0.9, rx: 0.15, duration: c.d(0.34), ease: 'power2.inOut' }, c.p(0.06));
    c.tl.to(pose, { open: 1, duration: c.d(0.14), ease: 'power2.out' }, c.p(0.4));
    c.tl.to(done, { autoAlpha: 1, duration: c.d(0.05) }, c.p(0.52));
    c.tl.to(pose, { size: 330, vx: 1130, vy: 400, ry: 0.35, rx: 0.2, rz: -0.3, open: 0.6, idle: 1, duration: c.d(0.3), ease: 'power3.inOut' }, c.p(0.6));
  } else {
    c.tl.to(th, { scale: 3.6, duration: c.d(0.34), ease: 'power2.inOut' }, c.p(0.06));
    state(c, th, 'ok', 0.4);
    c.tl.to(done, { autoAlpha: 1, duration: c.d(0.05) }, c.p(0.48));
    c.tl.to(th, { x: 1130, y: 400, scale: 2.6, duration: c.d(0.3), ease: 'power3.inOut' }, c.p(0.6));
  }
  c.tl.to(done, { x: 1130, y: 575, scale: 0.9, duration: c.d(0.3), ease: 'power3.inOut' }, c.p(0.6));
  c.tl.to(machine, { autoAlpha: 1, duration: c.d(0.2) }, c.p(0.6));
  c.tl.to(copy, { autoAlpha: 1, y: 0, duration: c.d(0.14), ease: 'power3.out' }, c.p(0.8));
  void along; void move; void fade; void vanish; void enterFrom; void draw; void camera; void cameraHome; void cameraStart; void uAt;
};
