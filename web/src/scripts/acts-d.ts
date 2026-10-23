import { gsap } from 'gsap';
import { appear, vanish, enterFrom, draw, state, along, move, fade, uAt, success, failure, gate, verify, camera, cameraHome, cameraStart, press, typeText } from '../lib/motion';
import type { ActBuilder } from './acts';

type G = SVGGeometryElement;
const ctr = (el: Element) => {
  const b = (el as unknown as SVGGraphicsElement).getBBox();
  return [b.x + b.width / 2, b.y + b.height / 2] as const;
};

/* ─────────────── 09 · VAULT + SKILLS + LIBRARY ─────────────── */
export const knowledge: ActBuilder = (c) => {
  const th = c.thread;
  const note = c.one('[data-k=note-promo]');
  const key = c.one('[data-k=vault-key]');
  const cursor = c.one('.cursor');
  const skills = c.q('[data-k=skill]');
  const tools = c.q('.tool');
  const scan = c.one('[data-k=scan]');

  // start on the vault
  cameraStart(c, 400, 590, 1.05, [1010, 560]);
  gsap.set(cursor, { autoAlpha: 0 });
  c.tl.set(th, { autoAlpha: 0 }, c.p(0));

  appear(c, c.q('[data-k=region]'), 0.02, 0.05, 12, 0.02);
  appear(c, key, 0.08, 0.04);
  // a note crosses to SHARED only when you promote it
  c.tl.set(cursor, { x: 640, y: 520, autoAlpha: 1 }, c.p(0.1));
  move(c, cursor, { x: 380, y: 440 }, 0.1, 0.04);
  press(c, key, 0.15);
  c.tl.to(key, { rotation: 90, svgOrigin: '370 430', duration: c.d(0.03), ease: 'back.out(2)' }, c.p(0.15));
  vanish(c, cursor, 0.18, 0.02);
  move(c, note, { x: 470, y: 560 }, 0.18, 0.1);
  state(c, note, 'ok', 0.28);

  // skills reach the executor for one job, then vanish
  camera(c, 1050, 560, 1.05, 0.3, 0.07, [1000, 560]);
  c.tl.set(th, { autoAlpha: 1, scale: 0 }, c.p(0.34));
  c.tl.to(th, { scale: 1, duration: c.d(0.03), ease: 'back.out(2)' }, c.p(0.34));
  skills.forEach((s, i) => {
    const f = 0.37 + i * 0.07;
    enterFrom(c, s, { autoAlpha: 0, y: '-=50' } as gsap.TweenVars, f, 0.03, 'power3.out');
    move(c, s, { y: 452 + i * 0, x: 965, scale: 0.8 }, f + 0.03, 0.035, 'power2.in');
    c.tl.to(th, { scale: 1.2, duration: c.d(0.012), yoyo: true, repeat: 1 }, c.p(f + 0.065));
    vanish(c, s, f + 0.065, 0.015);
  });
  appear(c, c.one('[data-k=ctx-note]'), 0.58, 0.03);

  // the tool library: one inventory
  camera(c, 1800, 580, 1.05, 0.64, 0.07, [960, 560]);
  appear(c, tools, 0.66, 0.03, 10, 0.01);
  c.tl.fromTo(scan, { x: 0 }, { x: 626, duration: c.d(0.18), ease: 'none', immediateRender: false }, c.p(0.72));
  for (let j = 0; j < 6; j++) {
    const col = tools.filter((_, i) => i % 6 === j);
    state(c, col, 'exec', 0.72 + j * 0.03);
  }
  vanish(c, scan, 0.9, 0.02);
  cameraHome(c, 0.93, 0.07);
};

/* ─────────────── 10 · MCP + PERMISSIONS ─────────────── */
export const mcp: ActBuilder = (c) => {
  const th = c.thread;
  const P = (id: string) => c.one<G>('#' + id);
  const capM = c.one('.capM');
  const capI = c.one('.capI');
  const read = P('mc-read');
  const mut = P('mc-mut');
  const irr = P('mc-irr');
  const u = (p: G, x: number, y: number) => uAt(p, x, y);

  gsap.set([c.one('[data-k=role-executor]'), c.one('[data-k=role-unknown]'), c.one('[data-k=seal-mut]'), c.one('[data-k=seal-irr]'), c.one('[data-k=tag-prop]'), c.one('[data-k=prompt]'), c.one('[data-k=unchanged]'), capM, capI], { autoAlpha: 0 });
  draw(c, read.parentElement!, 0, 0.1);
  draw(c, mut.parentElement!, 0.02, 0.1);
  draw(c, irr.parentElement!, 0.04, 0.1);
  appear(c, [c.one('.agent'), c.one('[data-k=role-planner]')], 0.0, 0.05);
  appear(c, c.one('[data-k=door]'), 0.04, 0.05);
  appear(c, [c.one('[data-k=s-read]'), c.one('[data-k=s-mut]'), c.one('[data-k=s-irr]')], 0.08, 0.04, 10, 0.02);

  // read: passes
  state(c, th, 'ok', 0.06);
  move(c, th, { x: 300, y: 430 }, 0.04, 0.03);
  along(c, read, th, 0.07, 0.15, { from: u(read, 300, 430), rotate: true });
  state(c, c.one('[data-k=s-read] .station'), 'ok', 0.22);
  move(c, th, { rotation: 0 }, 0.23, 0.02);

  // reversible mutation: becomes a proposal
  c.tl.set(capM, { autoAlpha: 1, x: 300, y: 550 }, c.p(0.2));
  along(c, mut, capM, 0.2, 0.09, { to: u(mut, 800, 550), rotate: true });
  appear(c, c.one('[data-k=tag-prop]'), 0.3, 0.03);
  state(c, capM, 'wait', 0.3);
  along(c, mut, capM, 0.31, 0.09, { from: u(mut, 800, 550), to: u(mut, 1290, 550), rotate: true });

  // irreversible: stopped, needs a real terminal
  c.tl.set(capI, { autoAlpha: 1, x: 300, y: 670 }, c.p(0.36));
  along(c, irr, capI, 0.36, 0.09, { to: u(irr, 788, 670), rotate: true });
  failure(c, capI, 0.46);
  state(c, c.one('[data-k=s-irr] .station'), 'blocked', 0.47);

  // permissions follow the role
  vanish(c, c.one('[data-k=role-planner]'), 0.5, 0.02);
  appear(c, c.one('[data-k=role-executor]'), 0.52, 0.02, 0);
  c.tl.to(c.one('[data-k=seal-irr]'), { autoAlpha: 1, duration: c.d(0.03) }, c.p(0.53));
  vanish(c, c.one('[data-k=role-executor]'), 0.6, 0.02);
  appear(c, c.one('[data-k=role-unknown]'), 0.62, 0.02, 0);
  c.tl.to(c.one('[data-k=seal-mut]'), { autoAlpha: 1, duration: c.d(0.03) }, c.p(0.63));

  // …but rewriting the prompt moves nothing
  appear(c, c.one('[data-k=prompt]'), 0.68, 0.04, 10);
  c.q<SVGTextElement>('[data-k=prompt] text.t-nocase:not(.t-sm)').forEach((t, i) => typeText(c, t, 0.72 + i * 0.045, 0.04));
  c.tl.set(capI, { x: 300 }, c.p(0.82));
  along(c, irr, capI, 0.83, 0.07, { to: u(irr, 788, 670), rotate: true });
  failure(c, capI, 0.91);
  appear(c, c.one('[data-k=unchanged]'), 0.9, 0.04);
  void success; void gate; void verify; void camera; void cameraHome; void press; void fade;
};

/* ─────────────── 11 · SANDBOX + LOCAL FIRST ─────────────── */
export const sandbox: ActBuilder = (c) => {
  const th = c.thread;
  const P = (id: string) => c.one<G>('#' + id);
  const exec = c.one('.agent--executor');
  const fills = c.q('.gauge__fill');
  const gauges = c.q('[data-k=gauge]');

  state(c, th, 'exec', 0.0);
  enterFrom(c, [c.one('[data-k=box]'), exec], { autoAlpha: 0, scale: 0.9, svgOrigin: '800 535' } as gsap.TweenVars, 0.0, 0.08, 'power3.out');
  appear(c, c.one('[data-k=fs]'), 0.08, 0.06);
  draw(c, [P('fs-w1').parentElement!, P('fs-w2').parentElement!], 0.12, 0.06);
  appear(c, c.one('[data-k=net]'), 0.22, 0.06);
  draw(c, [P('net-x').parentElement!, P('net-ok').parentElement!], 0.26, 0.06);
  appear(c, c.one('[data-k=tools]'), 0.34, 0.05);
  appear(c, c.one('[data-k=gauges]'), 0.44, 0.04);
  const w = [96, 70, 120, 170];
  fills.forEach((f, i) => verify(c, f, w[i], 0.46 + i * 0.02, i === 3 ? 0.3 : 0.2));
  gauges.forEach((g) => g.querySelectorAll('.gauge__fill'));
  c.tl.set(gauges[3], { attr: { 'data-state': 'exec' } }, c.p(0.0));
  gsap.set(c.one('[data-k=limit-t]'), { autoAlpha: 0 });
  state(c, gauges[3], 'fail', 0.76);
  state(c, exec, 'blocked', 0.77);
  state(c, th, 'fail', 0.77);
  appear(c, c.one('[data-k=limit-t]'), 0.78, 0.04);
  appear(c, c.one('[data-k=platforms]'), 0.86, 0.05);
  void success; void failure; void gate; void along; void move; void fade; void uAt; void ctr;
};
