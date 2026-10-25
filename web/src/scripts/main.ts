import { gsap } from 'gsap';
import { ScrollTrigger } from 'gsap/ScrollTrigger';
import { applyCrops, cinema, inview, type Director } from './director';

gsap.registerPlugin(ScrollTrigger);
ScrollTrigger.config({ ignoreMobileResize: true });

const html = document.documentElement;
const mm = gsap.matchMedia();

mm.add(
  {
    wide: '(min-width: 900px) and (min-height: 540px)',
    narrow: '(max-width: 899px), (max-height: 539px)',
    reduce: '(prefers-reduced-motion: reduce)',
  },
  (ctx) => {
    const { wide, reduce } = ctx.conditions as { wide: boolean; narrow: boolean; reduce: boolean };
    // Reduced motion: no timelines at all. The static composition tells the whole story.
    if (reduce) {
      html.classList.remove('is-cinema');
      applyCrops(!wide);
      return;
    }
    let d: Director | null = null;
    let cancelled = false;
    (async () => {
      try {
        if (wide) {
          const glOk = innerWidth >= 1100 && !matchMedia('(pointer: coarse)').matches;
          const mod = glOk ? await import('../lib/capsule3d').catch(() => null) : null;
          if (cancelled) return;
          d = cinema(mod ? mod.createCapsule3D : null);
        } else {
          html.classList.remove('is-cinema');
          d = inview();
        }
        html.classList.add('is-ready');
      } catch (err) {
        console.error('[gachapon] experience failed, falling back to the static story', err);
        html.classList.remove('is-cinema');
        applyCrops(!wide);
      }
    })();
    return () => {
      cancelled = true;
      d?.destroy();
    };
  },
);

// The logo always returns to the very top (the pinned stage hides #boot as an anchor target).
document.querySelector('.brand')?.addEventListener('click', (e) => {
  e.preventDefault();
  const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;
  history.replaceState(null, '', location.pathname + location.search);
  window.scrollTo({ top: 0, behavior: reduce ? 'auto' : 'smooth' });
});

// The language button keeps the reader on the same chapter: the other page opens at #<act id>.
document.querySelector<HTMLAnchorElement>('[data-lang-switch]')?.addEventListener('click', (e) => {
  const scenes = [...document.querySelectorAll<HTMLElement>('.scene')];
  let i = -1;
  if (html.classList.contains('is-cinema')) {
    i = [...document.querySelectorAll('.rail button')].findIndex((b) => b.getAttribute('aria-current') === 'true');
  } else {
    const mid = innerHeight / 2;
    i = scenes.findIndex((s) => {
      const r = s.getBoundingClientRect();
      return r.top <= mid && r.bottom >= mid;
    });
  }
  const id = scenes[i]?.id;
  if (i > 0 && id) {
    e.preventDefault();
    location.href = `${(e.currentTarget as HTMLAnchorElement).pathname}#${id}`;
  }
});
