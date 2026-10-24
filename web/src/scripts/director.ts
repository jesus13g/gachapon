import { gsap } from 'gsap';
import { ScrollTrigger } from 'gsap/ScrollTrigger';
import { SCROLL, STORY, TR, VB, type Transition } from '../lib/tokens';
import { resetCamera, type Ctx } from '../lib/motion';
import { makePose, type Pose } from '../lib/pose';
import type { Capsule3D, createCapsule3D } from '../lib/capsule3d';
import { BUILDERS, type ActBuilder } from './acts';

gsap.registerPlugin(ScrollTrigger);

const $ = <T extends Element = HTMLElement>(s: string, r: ParentNode = document) => r.querySelector<T>(s);
const $$ = <T extends Element = HTMLElement>(s: string, r: ParentNode = document) => [...r.querySelectorAll<T>(s)];

/** Mobile/tablet: crop every illustration to its action so labels stay legible. */
/** Capsules are drawn around their own centre; make GSAP rotate/scale them about it (its SVG default is the bbox corner). */
function centreCapsules() {
  gsap.set($$('.capsule'), { transformOrigin: '60px 26px' });
}

export function applyCrops(narrow: boolean) {
  for (const svg of $$<SVGSVGElement>('.scene__svg')) {
    const crop = svg.dataset.crop;
    svg.setAttribute('viewBox', narrow && crop ? crop : `0 0 ${VB.w} ${VB.h}`);
  }
}

function makeCtx(tl: gsap.core.Timeline, sc: HTMLElement, mode: Ctx['mode'], start: number, len: number, margin: number, thread: SVGGElement, threadWorld: SVGGElement | null): Ctx {
  const a = start + margin;
  const span = len - margin * 2;
  return {
    tl,
    mode,
    sc,
    world: $<SVGGElement>('.world', sc)!,
    thread,
    threadWorld,
    p: (f) => a + f * span,
    d: (f) => f * span,
    q: <T extends Element = SVGElement>(sel: string) => $$<T>(sel, sc),
    one: <T extends Element = SVGElement>(sel: string) => $<T>(sel, sc) as T,
  };
}

/* ───────────── hover metadata ───────────── */
function tooltips(root: HTMLElement) {
  const tip = document.createElement('div');
  tip.className = 'tip';
  tip.setAttribute('role', 'tooltip');
  document.body.appendChild(tip);
  const over = (e: Event) => {
    const t = (e.target as Element).closest?.('[data-hint]') as HTMLElement | null;
    if (!t) return;
    tip.textContent = t.getAttribute('data-hint');
    tip.classList.add('on');
  };
  const move = (e: PointerEvent) => {
    tip.style.left = Math.min(e.clientX + 16, innerWidth - tip.offsetWidth - 12) + 'px';
    tip.style.top = Math.min(e.clientY + 16, innerHeight - tip.offsetHeight - 12) + 'px';
  };
  const out = (e: Event) => {
    if ((e.target as Element).closest?.('[data-hint]')) tip.classList.remove('on');
  };
  root.addEventListener('pointerover', over);
  root.addEventListener('pointermove', move);
  root.addEventListener('pointerout', out);
  return () => {
    root.removeEventListener('pointerover', over);
    root.removeEventListener('pointermove', move);
    root.removeEventListener('pointerout', out);
    tip.remove();
  };
}

/* ───────────── transitions between acts ───────────── */
function enterExit(kind: Transition, o: string) {
  const origin = { transformOrigin: o };
  switch (kind) {
    case 'zoom':
      return { out: { scale: 1.7, ...origin }, in: { scale: 0.62, ...origin } };
    case 'zoomOut':
      return { out: { scale: 0.62, ...origin }, in: { scale: 1.7, ...origin } };
    case 'panL':
      return { out: { xPercent: -30 }, in: { xPercent: 30 } };
    case 'drop':
      return { out: { yPercent: -26 }, in: { yPercent: 26 } };
    case 'rotate':
      return { out: { rotation: -16, scale: 1.2, transformOrigin: '50% 50%' }, in: { rotation: 16, scale: 0.8, transformOrigin: '50% 50%' } };
    case 'flip':
      return { out: { rotationY: -70, transformOrigin: '50% 50%' }, in: { rotationY: 70, transformOrigin: '50% 50%' } };
    default:
      return { out: {}, in: {} };
  }
}

export interface Director {
  destroy(): void;
}

/* ═════════════ CINEMA (desktop / tablet landscape) ═════════════ */
export function cinema(create3D: typeof createCapsule3D | null): Director {
  const html = document.documentElement;
  html.classList.add('is-cinema');
  applyCrops(false);
  centreCapsules();

  const stage = $('.stage')!;
  const scenes = $$('.scene');
  const threadLayer = $<SVGSVGElement>('.thread-layer')!;
  const threadWorld = $<SVGGElement>('#thread-world', threadLayer)!;
  const thread = $<SVGGElement>('.thread', threadLayer)!;
  const cleanups: Array<() => void> = [];
  resetCamera([threadWorld, ...$$('.world')]);

  // geometry
  const starts: number[] = [];
  let total = 0;
  for (const a of STORY) {
    starts.push(total);
    total += a.len;
  }

  // lazy: false — a tween must read its start values only after the previous ones have rendered, or a jump
  // (fast scroll, chapter rail, deep link) records stale positions and capsules land in the wrong place.
  const master = gsap.timeline({ paused: false, defaults: { ease: 'none', lazy: false } });
  // anchor so the timeline's total duration equals `total` units
  master.set({}, {}, total);

  // WebGL capsule (hero + final) — optional, lazy
  const pose: Pose = makePose();
  let gl: Capsule3D | null = null;
  const canvas = document.createElement('canvas');
  canvas.id = 'gl';
  canvas.setAttribute('aria-hidden', 'true');
  stage.insertBefore(canvas, stage.querySelector('.scene'));
  if (create3D) {
    gl = create3D(canvas, stage, pose, (fn) => {
      gsap.ticker.add(fn);
      return () => gsap.ticker.remove(fn);
    });
    if (gl) {
      html.classList.add('has-gl');
      const pm = (e: PointerEvent) => gl?.setPointer((e.clientX / innerWidth) * 2 - 1, (e.clientY / innerHeight) * 2 - 1);
      addEventListener('pointermove', pm, { passive: true });
      cleanups.push(() => removeEventListener('pointermove', pm));
    }
  }
  if (!gl) canvas.remove();

  /* build every act */
  const arts = scenes.map((s) => $<HTMLElement>('.scene__art', s)!);
  const copies = scenes.map((s) => $<HTMLElement>('.scene__copy, .hero-copy, .final-copy', s));
  gsap.set(thread, { x: STORY[0].start[0], y: STORY[0].start[1], autoAlpha: 0 });

  scenes.forEach((sc, i) => {
    const cfg = STORY[i];
    const S = starts[i];
    const E = S + cfg.len;
    const ctx = makeCtx(master, sc, 'cinema', S, cfg.len, TR, thread as unknown as SVGGElement, threadWorld);
    const builder: ActBuilder | undefined = BUILDERS[cfg.id];
    const art = arts[i];

    /* camera transition into this act */
    if (i > 0) {
      const prev = STORY[i - 1];
      const o = `${(prev.end[0] / VB.w) * 100}% ${(prev.end[1] / VB.h) * 100}%`;
      const { out, in: inn } = enterExit(cfg.in, o);
      const t0 = S - TR;
      const W = TR * 2;
      // incoming
      gsap.set(art, { autoAlpha: 0, ...inn });
      master.to(art, { autoAlpha: 1, scale: 1, rotation: 0, rotationY: 0, xPercent: 0, yPercent: 0, duration: W, ease: 'power3.inOut' }, t0);
      // outgoing
      master.to(arts[i - 1], { autoAlpha: 0, ...out, duration: W, ease: 'power3.inOut' }, t0);
      // the capsule glides to where this act needs it
      master.to(thread, { x: cfg.start[0], y: cfg.start[1], duration: W, ease: 'power3.inOut' }, t0);
    }

    /* copy + beats */
    const copy = copies[i];
    if (copy) {
      if (cfg.id === 'final') gsap.set(copy, { autoAlpha: 0, y: 26 });
      else if (i > 0) {
        gsap.set(copy, { autoAlpha: 0, y: 26 });
        // the copy comes in once the previous artwork has mostly faded, so text never sits on the old scene
        master.to(copy, { autoAlpha: 1, y: 0, duration: TR * 0.9, ease: 'power3.out' }, S + TR * 0.2);
      }
      if (i < STORY.length - 1) master.to(copy, { autoAlpha: 0, y: -18, duration: TR * 0.8, ease: 'power2.in' }, E - TR * 1.05);
      const beats = $$('.beat', copy);
      const fr = BEAT_FRACS[cfg.id] ?? beats.map((_, n) => n / beats.length);
      beats.forEach((b, n) => {
        if (n > 0) gsap.set(b, { autoAlpha: 0, y: 18 });
        if (n > 0) master.to(b, { autoAlpha: 1, y: 0, duration: 4, ease: 'power3.out' }, ctx.p(fr[n]) - 1);
        // the outgoing beat is gone before the next one starts, so two headlines never sit on top of each other
        if (n < beats.length - 1) master.to(b, { autoAlpha: 0, y: -14, duration: 3, ease: 'power2.in' }, ctx.p(fr[n + 1]) - 4);
      });
    }

    builder?.(ctx, { pose, gl: !!gl });
  });

  /* live scenes (CSS animations + visibility) */
  const setLive = (t: number) => {
    scenes.forEach((s, i) => {
      const live = t >= starts[i] - TR - 0.5 && t <= starts[i] + STORY[i].len + TR + 0.5;
      if (live !== s.classList.contains('is-live')) {
        s.classList.toggle('is-live', live);
        s.style.visibility = live ? 'visible' : 'hidden';
      }
    });
  };
  scenes.forEach((s, i) => {
    s.style.visibility = i === 0 ? 'visible' : 'hidden';
    if (i === 0) s.classList.add('is-live');
  });

  /* pin + scrub */
  const st = ScrollTrigger.create({
    trigger: stage,
    start: 'top top',
    end: () => `+=${(total / 100) * SCROLL.stretch * innerHeight}`,
    pin: true,
    pinSpacing: true,
    scrub: SCROLL.scrub,
    anticipatePin: 1,
    invalidateOnRefresh: true,
    animation: master,
    onRefresh: () => {
      setLive(master.time());
      updateRail(master.time());
    },
  });
  master.pause();
  // the scrub eases behind the scroll, so follow the timeline itself, not the scroll events
  master.eventCallback('onUpdate', () => {
    setLive(master.time());
    updateRail(master.time());
  });

  /* chapter rail */
  const rail = $('.rail')!;
  const railBtns = $$<HTMLButtonElement>('button', rail);
  let cur = -1;
  function updateRail(t: number) {
    let idx = 0;
    for (let i = 0; i < starts.length; i++) if (t >= starts[i] - 1) idx = i;
    if (idx === cur) return;
    cur = idx;
    railBtns.forEach((b, i) => b.setAttribute('aria-current', String(i === idx)));
  }
  const goto = (i: number) => {
    const y = st.start + ((starts[i] + TR + 1) / total) * (st.end - st.start);
    window.scrollTo({ top: y, behavior: matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth' });
  };
  railBtns.forEach((b, i) => {
    const h = () => goto(i);
    b.addEventListener('click', h);
    cleanups.push(() => b.removeEventListener('click', h));
  });
  cleanups.push(tooltips(stage));

  // deep link: #bus etc.
  const hash = location.hash.slice(1);
  const hi = STORY.findIndex((a) => a.id === hash);
  if (hi > 0) requestAnimationFrame(() => {
    ScrollTrigger.refresh();
    goto(hi);
  });

  const ro = new ResizeObserver(() => gl?.resize());
  ro.observe(stage);
  cleanups.push(() => ro.disconnect());

  return {
    destroy() {
      cleanups.forEach((f) => f());
      st.kill();
      master.kill();
      gl?.dispose();
      canvas.remove();
      html.classList.remove('is-cinema', 'has-gl');
      scenes.forEach((s) => {
        s.style.visibility = '';
        s.classList.remove('is-live');
      });
      gsap.set([...arts, ...copies.filter(Boolean), thread, ...$$('.beat')], { clearProps: 'all' });
      resetCamera([threadWorld, ...$$('.world')]);
    },
  };
}

/** Beat boundaries (fractions of the usable act) when equal division isn't right. */
const BEAT_FRACS: Record<string, number[]> = {
  modes: [0, 0.34, 0.68],
  roles: [0, 0.34],
  control: [0, 0.6],
  teams: [0, 0.5],
  sandbox: [0, 0.6],
  locks: [0, 0.52],
  routines: [0, 0.55],
  journal: [0, 0.68],
  knowledge: [0, 0.34, 0.68],
};

/* ═════════════ IN-VIEW (phones, short viewports): same scenes, played once when they enter ═════════════ */
export function inview(): Director {
  applyCrops(true);
  centreCapsules();
  const scenes = $$('.scene');
  const cleanups: Array<() => void> = [];
  const tls: gsap.core.Timeline[] = [];

  scenes.forEach((sc, i) => {
    const cfg = STORY[i];
    const builder = BUILDERS[cfg.id];
    if (!builder) return;
    const local = $<SVGGElement>('.local-thread', sc);
    const tl = gsap.timeline({ paused: true, defaults: { ease: 'none', lazy: false } });
    tl.set({}, {}, 100);
    const ctx = makeCtx(tl, sc, 'inview', 0, 100, 0, (local ?? document.createElementNS('http://www.w3.org/2000/svg', 'g')) as SVGGElement, null);
    if (local) gsap.set(local, { x: cfg.start[0], y: cfg.start[1] });
    builder(ctx, { pose: makePose(), gl: false });
    tl.duration(8);
    tls.push(tl);
    sc.classList.add('is-live');
    const trig = ScrollTrigger.create({
      trigger: sc,
      start: 'top 65%',
      once: true,
      onEnter: () => tl.play(),
    });
    cleanups.push(() => trig.kill());
  });
  cleanups.push(tooltips(document.body));
  return {
    destroy() {
      cleanups.forEach((f) => f());
      tls.forEach((t) => t.kill());
    },
  };
}
