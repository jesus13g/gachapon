'use strict';

// La escena de la portada: cada agente es una máquina de gashapon y cada
// trabajo una cápsula que viaja entre ellas. Dónde está una cápsula sale del
// estado de su trabajo; lo que se anima es la diferencia entre dos sondeos.
// Las colas son la pila de cápsulas que se ve tras la ventana de cada máquina.

const gasha = (() => {
  const NS = 'http://www.w3.org/2000/svg';
  const quieto = matchMedia('(prefers-reduced-motion: reduce)');

  const COLOR = { mision: '#e5484d', obra: '#3e8ed0', rutina: '#f2c94c', propuesta: '#f28cb1' };
  const MODOS = { mision: 'misión', obra: 'obra', rutina: 'rutina' };
  const ESTADOS = {
    en_cola: 'en cola', ejecutando: 'ejecutando', en_revision: 'en revisión',
    en_espera: 'en espera', cerrado: 'cerrado', cancelado: 'cancelado',
  };
  const AGENTE = { 'claude-code': 'cc', 'modelo-local': 'ml', usuario: 'tu' };
  const NOM = { cc: 'Claude Code', ml: 'modelo local', tu: 'tú' };
  // Dónde vive una cápsula según el estado de su trabajo.
  const LUGAR = {
    en_cola: 'ml', ejecutando: 'wt', en_revision: 'cc',
    en_espera: 'tu', cerrado: 'bandeja', cancelado: 'papelera',
  };

  // Escena de 1000×320. Las máquinas se dibujan en coordenadas propias de
  // 150×255 (el modelo de caja de sobremesa) y se escalan.
  const SUELO = 284;
  const RIEL = 24;
  const HUB = { x: 500, y: RIEL };
  const MAQ = {
    cc: { x: 40, s: 0.95, nombre: 'CLAUDE CODE', rol: 'formaliza · revisa', color: '#d97757' },
    tu: { x: 262, s: 0.75, nombre: 'TÚ', rol: 'decides lo que falla', color: null },
    ml: { x: 807, s: 0.95, nombre: 'MODELO LOCAL', rol: 'ejecuta (Qwen)', color: '#6f5bd3' },
  };
  for (const m of Object.values(MAQ)) m.y = SUELO - 255 * m.s;
  const WT = { x: 690, y: 245 };
  const BANDEJA = { x: 434, y: 266, max: 6 };
  const PAPELERA = { x: 584, y: 268 };

  // Huecos de la pila sobre la bandeja en V de la ventana, de abajo arriba.
  // Cápsulas grandes, como en una máquina de verdad: radio 14 de 150.
  const R_VENTANA = 14;
  const fondoV = x => (x < 60 ? 104 + (x - 8) * 20 / 52 : x > 90 ? 124 - (x - 90) * 20 / 52 : 124);
  const FILAS = [[75, 47, 103, 22, 128], [61, 89, 33, 117], [75, 47, 103, 22, 128], [61, 89, 33, 117]];
  const HUECOS = FILAS.flatMap((xs, f) => xs.map(x => [x, fondoV(x) - R_VENTANA + 1 - f * 24]));

  let raiz, svg, plegada = false, alAbrir = () => {};
  const el = {};               // nodos fijos de la escena
  const caps = new Map();      // k -> cápsula
  const conocidos = new Map(); // k -> último estado visto
  const pilas = { cc: [], ml: [], tu: [], wt: [], bandeja: [] };
  let real = null;             // últimos datos del servidor
  let primera = true;
  let maxDiario = null, ultimoLatido = null, semOcupado = null;
  let avisosPrevios = new Set();
  let corriendo = false;
  const demo = { paso: -1, trabajos: [], propuestas: [], timer: null };

  // ------------------------------------------------------------ utilidades

  const corto = (s, n = 34) => {
    const t = String(s ?? '');
    return t.length > n ? t.slice(0, n - 1) + '…' : t;
  };
  const p2 = n => String(n).padStart(2, '0');

  function nodo(tipo, attrs = {}) {
    const n = document.createElementNS(NS, tipo);
    for (const [k, v] of Object.entries(attrs)) n.setAttribute(k, v);
    return n;
  }

  const visible = () => raiz && !plegada && raiz.offsetParent !== null;
  const instantaneo = () => quieto.matches || document.hidden || !visible();

  function anima(n, frames, opts) {
    if (!n || !n.animate || instantaneo()) return null;
    return n.animate(frames, opts);
  }

  function colorAg(id) {
    return MAQ[id]?.color ?? (getComputedStyle(document.documentElement).getPropertyValue('--accent').trim() || '#888');
  }

  // Curvas de velocidad.
  const suave = p => (p < 0.5 ? 4 * p * p * p : 1 - (-2 * p + 2) ** 3 / 2);
  const acelera = p => p * p;
  const frena = p => 1 - (1 - p) ** 2;
  function rebote(p) {
    const n = 7.5625, d = 2.75;
    if (p < 1 / d) return n * p * p;
    if (p < 2 / d) return n * (p -= 1.5 / d) * p + 0.75;
    if (p < 2.5 / d) return n * (p -= 2.25 / d) * p + 0.9375;
    return n * (p -= 2.625 / d) * p + 0.984375;
  }

  // ------------------------------------------------------------- dibujo fijo

  // Engranaje de dientes cuadrados.
  function engranaje(cx, cy, r1, r2, n) {
    const pts = [];
    for (let k = 0; k < n * 4; k++) {
      const a = (k + 0.5) * Math.PI * 2 / (n * 4);
      const r = k % 4 < 2 ? r1 : r2;
      pts.push(`${(cx + r * Math.cos(a)).toFixed(1)},${(cy + r * Math.sin(a)).toFixed(1)}`);
    }
    return `M${pts.join('L')}Z`;
  }

  const estiloAg = m => `--ag:${m.color ?? 'var(--accent)'}`;
  const sitio = m => `translate(${m.x} ${m.y.toFixed(2)}) scale(${m.s})`;

  // La caja: lo que queda detrás de las cápsulas.
  function maquinaAtras(id, m) {
    return `<g transform="${sitio(m)}"><g class="maq" data-m="${id}" style="${estiloAg(m)}">` +
      '<ellipse class="sombra" cx="80" cy="255" rx="86" ry="6"/>' +
      '<path class="lado" d="M150 6 L162 12 L162 249 L150 255 Z"/>' +
      '<rect class="cuerpo" width="150" height="255" rx="8"/>' +
      '<rect class="interior" x="8" y="28" width="134" height="112" rx="3"/>' +
      '<path class="bandeja-v" d="M8 104 L60 124 L90 124 L142 104 L142 140 L8 140 Z"/>' +
      '<path class="tabique" d="M55 109 V121 M95 109 V121"/>' +
      '<rect class="adh-azul" x="12" y="148" width="32" height="38" rx="2"/>' +
      '<text class="adh-t" x="28" y="158">COLA</text><text class="adh-n" x="28" y="179">0</text>' +
      '<rect class="coin-in" x="54" y="150" width="46" height="14" rx="7"/>' +
      '<text class="coin-t" x="77" y="159.5">COIN IN</text>' +
      '<rect class="ranura" x="116" y="146" width="8" height="28" rx="4"/>' +
      '<g class="manivela"><circle class="aro" cx="78" cy="200" r="24"/><circle class="aro2" cx="78" cy="200" r="19"/>' +
      '<rect class="asa" x="72.5" y="180" width="11" height="40" rx="5"/></g>' +
      '<circle class="boton" cx="124" cy="192" r="5"/>' +
      '<text class="logo" x="12" y="206">ガチャ</text>' +
      '<rect class="boca" x="12" y="212" width="40" height="36" rx="3"/>' +
      '<rect class="boca-in" x="15" y="215" width="34" height="30" rx="2"/>' +
      '<rect class="devol" x="110" y="205" width="16" height="10" rx="2"/>' +
      '<g class="adh"><rect x="104" y="222" width="36" height="24" rx="1"/>' +
      '<path d="M108 228 H136 M108 233 H131 M108 238 H134"/></g>' +
      '</g></g>';
  }

  // Lo que queda delante de las cápsulas: la cabecera (tapa por donde
  // entran), el cristal con su reflejo, el labio de la bandeja y la trampilla.
  function maquinaDelante(id, m) {
    const ancho = m.nombre.length * 5.4 + 12;
    return `<g transform="${sitio(m)}"><g class="maq-f" data-m="${id}" style="${estiloAg(m)}">` +
      '<path class="tapa" d="M0 8 Q0 0 8 0 H142 Q150 0 150 8 V28 H0 Z"/>' +
      '<path class="lado" d="M150 6 L162 12 L162 31 L150 28 Z"/>' +
      `<rect class="placa" x="8" y="6" width="${ancho}" height="16" rx="4"/>` +
      `<text class="placa-t" x="14" y="17.5">${m.nombre}</text>` +
      '<rect class="vidrio" x="8" y="28" width="134" height="112" rx="3"/>' +
      '<path class="reflejo" d="M14 32 L54 32 L24 104 L14 104 Z"/>' +
      '<path class="labio" d="M8 132 H142 V140 H8 Z"/>' +
      '<rect class="trampilla" x="15" y="215" width="34" height="14" rx="1"/>' +
      '<text class="mas" x="138" y="42"></text>' +
      '</g></g>';
  }

  function escenaSVG() {
    const cima = id => `${(MAQ[id].x + 75 * MAQ[id].s).toFixed(1)}`;
    const riel = `M${cima('cc')} ${MAQ.cc.y.toFixed(1)} V${RIEL} H${cima('ml')} V${MAQ.ml.y.toFixed(1)} ` +
      `M${cima('tu')} ${MAQ.tu.y.toFixed(1)} V${RIEL}`;
    const rol = id => `<text class="rot" x="${cima(id)}" y="299">${MAQ[id].rol}</text>`;
    return `<svg class="esc" viewBox="0 0 1000 320" role="img" aria-label="Escena de los agentes">` +
      '<g class="fondo">' +
      `<line class="suelo" x1="10" y1="${SUELO}" x2="990" y2="${SUELO}"/>` +
      `<path class="riel" d="${riel}"/><path class="riel-in" d="${riel}"/>` +
      maquinaAtras('cc', MAQ.cc) + maquinaAtras('tu', MAQ.tu) + maquinaAtras('ml', MAQ.ml) +
      rol('cc') + rol('tu') + rol('ml') +
      // el worktree: una mesa de trabajo con su semáforo
      '<rect class="mesa" x="625" y="258" width="130" height="7" rx="2"/>' +
      '<rect class="pata" x="633" y="265" width="5" height="19"/><rect class="pata" x="742" y="265" width="5" height="19"/>' +
      `<text class="rot" id="g-wt" x="${WT.x}" y="299">worktree · libre</text>` +
      '<g class="sem verde"><rect class="sem-palo" x="781" y="214" width="4" height="70"/>' +
      '<rect class="sem-caja" x="772" y="146" width="22" height="70" rx="5"/>' +
      '<circle class="l-rojo" cx="783" cy="160" r="7"/><circle class="l-ambar" cx="783" cy="181" r="7"/>' +
      '<circle class="l-verde" cx="783" cy="202" r="7"/></g>' +
      // bandeja de salida y papelera
      '<path class="bandeja-s" d="M418 264 L425 283 H535 L542 264"/>' +
      `<text class="rot" x="480" y="299">cerrados</text>` +
      // el orquestador: caja de engranajes en el raíl
      `<g class="hub"><rect x="${HUB.x - 34}" y="${HUB.y - 14}" width="68" height="28" rx="6"/>` +
      `<path class="engranaje" d="${engranaje(HUB.x - 14, HUB.y, 9.5, 6.5, 8)}"/>` +
      `<circle class="eje" cx="${HUB.x - 14}" cy="${HUB.y}" r="2.5"/>` +
      `<circle class="led" cx="${HUB.x + 16}" cy="${HUB.y}" r="4.5"/></g>` +
      `<text class="rot" x="${HUB.x}" y="${HUB.y + 28}">orquestador</text>` +
      '</g>' +
      '<g class="caps"></g>' +
      '<g class="frente">' +
      maquinaDelante('cc', MAQ.cc) + maquinaDelante('tu', MAQ.tu) + maquinaDelante('ml', MAQ.ml) +
      '<rect class="bandeja-l" x="421" y="275" width="118" height="8" rx="2"/>' +
      '<path class="papelera" d="M572 254 L576 284 H592 L596 254 Z"/>' +
      '<rect class="papelera-b" x="570" y="251" width="28" height="4" rx="1"/>' +
      '</g>' +
      '<g class="efectos"></g>' +
      '<g class="avisos"></g>' +
      '<text class="demo-et" x="984" y="315" visibility="hidden">DEMO</text>' +
      '<text class="ticker" x="16" y="315"></text>' +
      '</svg>';
  }

  function bocadillo() {
    const g = nodo('g', { class: 'boc', visibility: 'hidden' });
    g.append(nodo('path', { d: 'M-5 9 L0 16 L5 9 Z' }), nodo('rect', { y: -11, height: 21, rx: 5 }), nodo('text', { y: 4 }));
    el.avisos.appendChild(g);
    return g;
  }

  const LEYENDA = Object.entries({ misión: COLOR.mision, obra: COLOR.obra, rutina: COLOR.rutina, propuesta: COLOR.propuesta })
    .map(([n, c]) => `<span><i style="color:${c}">●</i>${n}</span>`).join('') + '<span><i class="hot">!</i>aviso</span>';

  // ---------------------------------------------------------------- cápsulas

  function crearCap(k, modo) {
    const g = nodo('g', { class: 'cap' + (k[0] === 'j' && !k.includes('demo') ? ' real' : '') });
    g.dataset.cap = k;
    g.innerHTML = '<g class="giro"><circle class="halo" r="14"/>' +
      '<path class="baja" d="M-10 0 A10 10 0 0 0 10 0 Z"/>' +
      `<path class="alta" fill="${COLOR[modo] ?? COLOR.mision}" d="M-10 0 A10 10 0 0 1 10 0 Z"/>` +
      '<rect class="junta" x="-10.4" y="-1" width="20.8" height="2" rx="1"/>' +
      '<circle class="aguj" cx="-5" cy="-3.2" r="1"/><circle class="aguj" cx="0" cy="-4.6" r="1"/><circle class="aguj" cx="5" cy="-3.2" r="1"/>' +
      '<ellipse class="brillo" cx="-4" cy="-6" rx="3" ry="1.4" transform="rotate(-25 -4 -6)"/>' +
      '<ellipse class="brillo" cx="3.5" cy="5.5" rx="3" ry="1.2"/>' +
      (modo === 'propuesta' ? '<path class="rombo" d="M0 -8.5 L3.5 -5 L0 -1.5 L-3.5 -5 Z"/>' : '') +
      '<text class="excl" y="-2.4">!</text></g><title></title>';
    el.caps.appendChild(g);
    const c = { k, el: g, x: 0, y: 0, s: 1, rot: 0, op: 1, segs: [], lugar: null };
    caps.set(k, c);
    return c;
  }

  function pinta(c) {
    c.el.setAttribute('transform', `translate(${c.x.toFixed(1)} ${c.y.toFixed(1)}) rotate(${Math.round(c.rot % 360)}) scale(${c.s.toFixed(3)})`);
    c.el.style.opacity = c.op;
  }

  function ponEn(c, h) {
    Object.assign(c, { op: 1, ...h });
    c.rot = 0;
    pinta(c);
  }

  // Lo que dice la cápsula de su trabajo: título, color de aviso y giro.
  function vestir(c, t) {
    c.el.querySelector('title').textContent = `${MODOS[t.modo] ?? t.modo} · ${t.titulo} · ${ESTADOS[t.estado] ?? t.estado}`;
    c.el.classList.toggle('hot', t.estado === 'en_espera' || (t.estado === 'en_revision' && t.resultado === 'bloqueado'));
    c.el.classList.toggle('run', t.estado === 'ejecutando');
    c.el.classList.toggle('cerrada', t.estado === 'cerrado');
  }

  // Dónde se queda la i-ésima cápsula de un lugar con n.
  function hueco(lugar, i, n) {
    const m = MAQ[lugar];
    if (m) {
      const [lx, ly] = HUECOS[Math.min(i, HUECOS.length - 1)];
      return { x: m.x + lx * m.s, y: m.y + ly * m.s, s: R_VENTANA / 10 * m.s, op: i < HUECOS.length ? 1 : 0 };
    }
    if (lugar === 'wt') return { x: WT.x + (i - (n - 1) / 2) * 28, y: WT.y, s: 1.3, op: 1 };
    return { x: BANDEJA.x + i * 19, y: BANDEJA.y, s: 0.85, op: 1 };
  }

  function huecoDe(c) {
    const pila = pilas[c.lugar];
    const i = pila ? pila.indexOf(c) : -1;
    return i < 0 ? { x: c.x, y: c.y, s: c.s } : hueco(c.lugar, i, pila.length);
  }

  const boca = id => ({ x: MAQ[id].x + 32 * MAQ[id].s, y: MAQ[id].y + 226 * MAQ[id].s });

  function sacaDePila(c) {
    const pila = pilas[c.lugar];
    if (pila) {
      const i = pila.indexOf(c);
      if (i >= 0) pila.splice(i, 1);
    }
  }

  function quita(c) {
    sacaDePila(c);
    c.el.remove();
    caps.delete(c.k);
  }

  // Cada cápsula quieta, a su hueco (o se recoloca si la pila cambió).
  function recolocar(forzar = false) {
    const ya = forzar || instantaneo();
    for (const [lugar, pila] of Object.entries(pilas)) {
      pila.forEach((c, i) => {
        if (c.segs.length) return;
        const h = hueco(lugar, i, pila.length);
        if (Math.abs(h.x - c.x) + Math.abs(h.y - c.y) < 0.5 && Math.abs(h.s - c.s) < 0.01 && h.op === c.op) return;
        if (ya) ponEn(c, h);
        else c.segs.push({ a: h, dur: 280, ease: frena });
      });
    }
    animar();
  }

  // ----------------------------------------------------------------- viajes

  // Un viaje es una lista de tramos: { a: destino o función que lo da, dur,
  // ease, arco, al (al empezar), fin (al acabar) } o { espera: ms }.
  function viaje(c, de, a, retraso) {
    sacaDePila(c);
    c.lugar = a;
    if (pilas[a]) pilas[a].push(c);
    if (instantaneo()) {
      c.segs = [];
      if (a === 'papelera' || a === 'fuera') quita(c);
      else ponEn(c, huecoDe(c));
      return;
    }
    const s = [];
    if (retraso) s.push({ espera: retraso });
    const origen = de.startsWith('nuevo:') ? de.slice(6) : MAQ[de] ? de : null;
    // De la ventana cae por el embudo…
    if (MAQ[de]) {
      const m = MAQ[de];
      s.push({ a: { x: m.x + 75 * m.s, y: m.y + 126 * m.s, op: 0 }, dur: 220, ease: acelera });
    }
    // …y sale por la boca al girar la manivela, con un rebote.
    if (origen) {
      const b = boca(origen);
      s.push({ a: { x: b.x, y: b.y - 8, s: 1.2 * MAQ[origen].s, op: 1 }, dur: 0, al: () => { manivela(origen); trampilla(origen); } });
      s.push({ a: { x: b.x, y: b.y + 4 }, dur: 380, ease: rebote });
    }
    if (a === 'fuera') {
      s.push({ a: { s: 0, op: 0 }, dur: 420, ease: acelera, fin: () => quita(c) });
    } else if (MAQ[a]) {
      // Sube por el raíl y entra por arriba (la tapa la tapa) hasta su hueco.
      const m = MAQ[a];
      s.push({ a: { x: m.x + 75 * m.s, y: m.y - 14, s: 1.25 * m.s }, arco: true, ease: suave });
      s.push({ a: { x: m.x + 75 * m.s, y: m.y + 40 * m.s, s: R_VENTANA / 10 * m.s }, dur: 220, ease: acelera });
      s.push({ a: () => huecoDe(c), dur: 260, ease: frena, fin: () => { ranura(a); recolocar(); } });
    } else if (a === 'wt') {
      s.push({ a: { x: WT.x, y: WT.y - 45, s: 1.3 }, arco: true, ease: suave });
      s.push({ a: () => huecoDe(c), dur: 340, ease: rebote, fin: () => recolocar() });
    } else if (a === 'bandeja') {
      s.push({ a: () => ({ ...huecoDe(c), y: BANDEJA.y - 34 }), arco: true, ease: suave });
      s.push({ a: () => huecoDe(c), dur: 340, ease: rebote, fin: () => recolocar() });
    } else if (a === 'papelera') {
      s.push({ a: { x: PAPELERA.x, y: PAPELERA.y - 36, s: 1 }, arco: true, ease: suave });
      s.push({ a: { x: PAPELERA.x, y: PAPELERA.y, op: 0.2 }, dur: 300, ease: acelera, fin: () => { puf(PAPELERA.x, PAPELERA.y - 12); quita(c); } });
    }
    c.segs = s;
    animar();
  }

  // Que se desvanezca donde está (la bandeja, al pasar de seis).
  function desvanecer(c) {
    sacaDePila(c);
    c.lugar = null;
    if (instantaneo()) return quita(c);
    c.segs = [{ a: { op: 0, s: c.s * 0.6 }, dur: 400, ease: acelera, fin: () => quita(c) }];
    animar();
  }

  function avanzar(c, ahora) {
    while (c.segs.length) {
      const sg = c.segs[0];
      if (sg.t0 == null) {
        sg.t0 = ahora;
        if (sg.al) sg.al();
        if (sg.espera == null) {
          sg.de = { x: c.x, y: c.y, s: c.s, op: c.op, rot: c.rot };
          sg.hasta = { x: c.x, y: c.y, s: c.s, op: 1, ...(typeof sg.a === 'function' ? sg.a() : sg.a) };
          if (sg.arco) {
            // Arco por encima del raíl: cuanto más lejos, más alto.
            const { de, hasta } = sg;
            const dist = Math.hypot(hasta.x - de.x, hasta.y - de.y);
            const cima = Math.max(RIEL - 10, Math.min(de.y, hasta.y) - 40 - Math.abs(hasta.x - de.x) * 0.25);
            sg.ctrl = { x: (de.x + hasta.x) / 2, y: 2 * cima - (de.y + hasta.y) / 2 };
            sg.dur = Math.min(1400, Math.max(600, 450 + dist * 0.9));
            sg.giro = 720;
          }
        }
      }
      const dur = sg.espera ?? sg.dur;
      const p = dur > 0 ? Math.min(1, (ahora - sg.t0) / dur) : 1;
      if (sg.espera == null) {
        const e = (sg.ease || suave)(p);
        const { de, hasta, ctrl } = sg;
        if (ctrl) {
          const q = 1 - e;
          c.x = q * q * de.x + 2 * q * e * ctrl.x + e * e * hasta.x;
          c.y = q * q * de.y + 2 * q * e * ctrl.y + e * e * hasta.y;
        } else {
          c.x = de.x + (hasta.x - de.x) * e;
          c.y = de.y + (hasta.y - de.y) * e;
        }
        c.s = de.s + (hasta.s - de.s) * e;
        c.op = de.op + (hasta.op - de.op) * Math.min(1, p * 1.5);
        c.rot = de.rot + (sg.giro || 0) * e;
      }
      if (p < 1) return;
      c.rot %= 360;
      c.segs.shift();
      if (sg.fin) sg.fin();
    }
  }

  function animar() {
    if (corriendo || ![...caps.values()].some(c => c.segs.length)) return;
    corriendo = true;
    requestAnimationFrame(paso);
  }

  function paso(ahora) {
    for (const c of [...caps.values()]) {
      if (!c.segs.length) continue;
      avanzar(c, ahora);
      pinta(c);
    }
    corriendo = [...caps.values()].some(c => c.segs.length);
    if (corriendo) requestAnimationFrame(paso);
  }

  // ------------------------------------------------- piezas de las máquinas

  const pieza = (id, sel) => svg.querySelector(`[data-m="${id}"] ${sel}`);

  function manivela(id) {
    anima(pieza(id, '.manivela'), [{ transform: 'rotate(0deg)' }, { transform: 'rotate(360deg)' }], { duration: 600, easing: 'ease-in-out' });
  }

  function trampilla(id) {
    anima(pieza(id, '.trampilla'), [{ transform: 'scaleY(1)' }, { transform: 'scaleY(.15)', offset: 0.3 }, { transform: 'scaleY(1)' }], { duration: 520 });
  }

  // La ranura «COIN IN» se ilumina al recibir.
  function ranura(id) {
    anima(pieza(id, '.ranura'), [{ fill: '#3a3d42' }, { fill: '#f2c94c', offset: 0.2 }, { fill: '#3a3d42' }], { duration: 900 });
  }

  // El botón redondo se enciende con la actividad del agente.
  function boton(id) {
    anima(pieza(id, '.boton'), [{ fill: '#c8c8c4' }, { fill: colorAg(id), offset: 0.15 }, { fill: '#c8c8c4' }], { duration: 1200 });
  }

  function sacude(id) {
    const frames = [
      { transform: 'translate(0,0)' }, { transform: 'translate(-3px,1px)' }, { transform: 'translate(3px,-1px)' },
      { transform: 'translate(-2px,0)' }, { transform: 'translate(0,0)' },
    ];
    const ns = id === 'hub' ? [el.hub] : svg.querySelectorAll(`[data-m="${id}"]`);
    ns.forEach(n => anima(n, frames, { duration: 180, iterations: 3 }));
  }

  function puf(x, y) {
    for (let i = 0; i < 4; i++) {
      const p = nodo('circle', { class: 'puf', cx: x, cy: y, r: 2.5 });
      el.efectos.appendChild(p);
      const a = (i / 4) * Math.PI * 2 + 0.6;
      const an = anima(p, [{ transform: 'translate(0,0)', opacity: 0.9 }, { transform: `translate(${Math.cos(a) * 14}px,${Math.sin(a) * 10 - 6}px)`, opacity: 0 }], { duration: 450, easing: 'ease-out' });
      if (an) an.onfinish = () => p.remove();
      else p.remove();
    }
  }

  // Un evento del diario sin cápsula: una chispa del agente al orquestador por el raíl.
  function chispa(id) {
    boton(id);
    if (instantaneo()) return;
    const m = MAQ[id];
    const x0 = m.x + 75 * m.s, y0 = m.y;
    const p = nodo('circle', { class: 'chispa', cx: x0, cy: y0, r: 3.5, fill: colorAg(id) });
    el.efectos.appendChild(p);
    const an = p.animate([
      { transform: 'translate(0,0)', opacity: 1 },
      { transform: `translate(0px,${RIEL - y0}px)`, opacity: 1, offset: 0.35 },
      { transform: `translate(${HUB.x - 14 - x0}px,${RIEL - y0}px)`, opacity: 0.2 },
    ], { duration: 800, easing: 'ease-in-out' });
    an.onfinish = () => p.remove();
    anima(el.led, [{ opacity: 1 }, { opacity: 0.2, offset: 0.5 }, { opacity: 1 }], { duration: 300, delay: 760 });
  }

  // ------------------------------------------------------------- el ticker

  function cuenta(txt, hot = false, retraso = 0) {
    const poner = () => {
      const d = new Date();
      el.ticker.textContent = `${p2(d.getHours())}:${p2(d.getMinutes())}:${p2(d.getSeconds())}  ${txt}`;
      el.ticker.classList.toggle('hot', hot);
    };
    if (retraso && !instantaneo()) setTimeout(poner, retraso);
    else poner();
  }

  function mensaje(t, antes) {
    const q = `«${corto(t.titulo)}»`;
    switch (t.estado) {
      case 'en_cola':
        return antes === undefined
          ? [`${t.modo === 'rutina' ? 'tú' : 'Claude Code'} → modelo local · ${MODOS[t.modo] ?? t.modo} ${q}`]
          : [`Claude Code → modelo local · reformula ${q}`];
      case 'ejecutando': return [`modelo local toma ${q} · worktree ocupado`];
      case 'en_revision':
        if (antes === 'en_espera') return [`tú → Claude Code · confirmas ${q}`];
        return t.resultado === 'bloqueado'
          ? [`✕ modelo local → Claude Code · ${q} bloqueado`, true]
          : [`modelo local → Claude Code · entrega ${q}`];
      case 'en_espera': return [`! ${q} falló: el paquete de corrección te espera`, true];
      case 'cerrado': return [`Claude Code cierra ${q}`];
      case 'cancelado': return [`⊘ ${q} cancelado`];
      default: return [`${q} · ${t.estado}`];
    }
  }

  // ------------------------------------------------------------- los avisos

  function avisos(d) {
    const av = { cc: [], ml: [], tu: [], hub: [] };
    for (const t of d.trabajos) {
      if (t.estado === 'en_espera') {
        av.tu.push({ k: 'esp' + t.id, txt: `! paquete de corrección: ${corto(t.titulo, 26)}`, hot: true });
      } else if (t.estado === 'en_revision' && t.resultado === 'bloqueado') {
        av.cc.push({ k: 'bloq' + t.id, txt: `✕ bloqueado: ${corto(t.titulo, 28)}`, hot: true });
      }
    }
    const e = d.estado;
    if (e && !e.orquestador.vivo) av.hub.push({ k: 'latido', txt: '♥ sin latido: no se ejecuta nada', hot: true });
    const caidos = e ? e.servicios.filter(s => s.activo !== 'active' && s.activo !== 'desconocido').map(s => s.nombre) : [];
    if (caidos.length) av.hub.push({ k: 'srv' + caidos.join(','), txt: `▦ parado: ${caidos.join(', ')}`, hot: true });
    if (d.propuestas.length) {
      const n = d.propuestas.length;
      av.tu.push({ k: 'prop', txt: `◇ ${n} propuesta${n > 1 ? 's' : ''} esperan tu decisión`, hot: false });
    }

    const ahora = new Set();
    for (const [zona, lista] of Object.entries(av)) {
      const hot = lista.some(a => a.hot);
      const grupo = zona === 'hub' ? [el.hub] : svg.querySelectorAll(`[data-m="${zona}"]`);
      grupo.forEach(n => { n.classList.toggle('alerta', hot); n.classList.toggle('suave', !hot && lista.length > 0); });
      // Lo nuevo y grave sacude la máquina.
      lista.forEach(a => ahora.add(zona + a.k));
      if (lista.some(a => a.hot && !avisosPrevios.has(zona + a.k))) sacude(zona);
      pintaBocadillo(zona, lista);
    }
    avisosPrevios = ahora;
    return av;
  }

  function pintaBocadillo(zona, lista) {
    const g = el.boc[zona];
    if (!lista.length) { g.setAttribute('visibility', 'hidden'); return; }
    const a = lista.find(x => x.hot) ?? lista[0];
    const txt = a.txt + (lista.length > 1 ? ` (+${lista.length - 1})` : '');
    const ancho = txt.length * 6.6 + 18;
    let x, y;
    // El del orquestador, a su derecha sobre el raíl; los demás, encima de su máquina.
    if (zona === 'hub') { x = HUB.x + 44 + ancho / 2; y = HUB.y; } else {
      const m = MAQ[zona];
      x = m.x + 75 * m.s;
      y = Math.max(m.y - 16, 52);
    }
    x = Math.min(994 - ancho / 2, Math.max(6 + ancho / 2, x));
    g.setAttribute('transform', `translate(${x.toFixed(1)} ${y.toFixed(1)})`);
    g.setAttribute('class', 'boc ' + (a.hot ? 'hot' : 'suave'));
    g.querySelector('rect').setAttribute('x', -ancho / 2);
    g.querySelector('rect').setAttribute('width', ancho);
    g.querySelector('text').textContent = txt;
    // La cola del bocadillo apunta a la máquina, aunque el bocadillo se haya movido.
    const cx = zona === 'hub' ? HUB.x : MAQ[zona].x + 75 * MAQ[zona].s;
    const dx = Math.max(-ancho / 2 + 10, Math.min(ancho / 2 - 10, cx - x));
    g.querySelector('path').setAttribute('d', zona === 'hub'
      ? `M${-ancho / 2} -5 L${-ancho / 2 - 8} 0 L${-ancho / 2} 5 Z`
      : `M${dx - 5} 9 L${dx} 16 L${dx + 5} 9 Z`);
    g.setAttribute('visibility', 'visible');
  }

  // --------------------------------------------------------------- el modelo

  // Los datos reales más la demo, si está en marcha.
  function componer() {
    const d = {
      ...real,
      trabajos: [...demo.trabajos, ...real.trabajos],
      propuestas: [...demo.propuestas, ...real.propuestas],
    };
    const run = demo.trabajos.find(t => t.estado === 'ejecutando');
    if (run && !real.semaforos.length) d.semaforos = [{ agente: 'modelo-local', trabajo: run.id }];
    return d;
  }

  function aplicar() {
    if (!real || !svg) return;
    const d = componer();
    // Muchos cambios juntos (volver a la portada tras un rato) se aplican sin viajes.
    const cambios = d.trabajos.filter(t => conocidos.get('j' + t.id) !== t.estado).length;
    const sinViaje = primera || cambios > 10;
    let retraso = 0;
    const turno = () => { const r = retraso; retraso += 280; return r; };
    const vivos = new Set();
    const recientes = new Set(d.trabajos.filter(t => t.estado === 'cerrado').slice(0, BANDEJA.max).map(t => 'j' + t.id));

    // Del más antiguo al más reciente: la lista llega al revés.
    for (const t of [...d.trabajos].reverse()) {
      const k = 'j' + t.id;
      vivos.add(k);
      const antes = conocidos.get(k);
      conocidos.set(k, t.estado);
      const a = LUGAR[t.estado];
      let c = caps.get(k);
      if (sinViaje) {
        if (!c && (a === 'papelera' || (a === 'bandeja' && !recientes.has(k)))) continue;
        if (!c) c = crearCap(k, t.modo);
        vestir(c, t);
        if (c.lugar !== a) {
          sacaDePila(c);
          if (a === 'papelera') { quita(c); continue; }
          c.lugar = a;
          pilas[a].push(c);
          c.segs = [];
        }
        continue;
      }
      if (antes === t.estado) { if (c) vestir(c, t); continue; }
      if (!c && antes === undefined && a === 'papelera') continue;
      let de;
      if (c) de = c.lugar;
      else {
        const o = antes === undefined ? (t.modo === 'rutina' ? 'tu' : 'cc') : MAQ[LUGAR[antes]] ? LUGAR[antes] : 'cc';
        c = crearCap(k, t.modo);
        ponEn(c, { ...boca(o), op: 0 });
        de = 'nuevo:' + o;
      }
      vestir(c, t);
      const r = turno();
      viaje(c, de, a, r);
      const [txt, hot] = mensaje(t, antes);
      cuenta(txt, hot, r + 400);
    }

    for (const p of d.propuestas) {
      const k = 'p' + p.id;
      vivos.add(k);
      if (caps.has(k)) continue;
      conocidos.set(k, 'pendiente');
      const c = crearCap(k, 'propuesta');
      c.el.querySelector('title').textContent = `propuesta #${p.id} · ${p.descripcion}`;
      if (sinViaje) { c.lugar = 'tu'; pilas.tu.push(c); continue; }
      let o = AGENTE[p.proponente] ?? 'cc';
      if (o === 'tu') o = 'cc';
      ponEn(c, { ...boca(o), op: 0 });
      const r = turno();
      viaje(c, 'nuevo:' + o, 'tu', r);
      cuenta(`${NOM[o]} → tú · propuesta «${corto(p.descripcion)}»`, false, r + 400);
    }

    // Lo que ya no está: propuestas decididas, trabajos borrados, el fin de la demo.
    for (const c of [...caps.values()]) {
      if (vivos.has(c.k) || c.lugar === null || c.lugar === 'fuera' || c.lugar === 'papelera') continue;
      if (sinViaje) { quita(c); continue; }
      if (c.k[0] === 'p') cuenta(`decides la propuesta ${c.k.includes('demo') ? '(demo)' : '#' + c.k.slice(1)}`, false, retraso + 400);
      if (MAQ[c.lugar]) viaje(c, c.lugar, 'fuera', turno());
      else desvanecer(c);
    }

    while (pilas.bandeja.length > BANDEJA.max) desvanecer(pilas.bandeja[0]);

    recolocar(sinViaje);
    primera = false;
    contadores();
    semaforo(d.semaforos);
    latido(d.estado);
    chispas(d.diario || []);
    const av = avisos(d);
    resumen(d, av);
    el.demoEt.setAttribute('visibility', demo.paso >= 0 ? 'visible' : 'hidden');
  }

  function contadores() {
    for (const id of Object.keys(MAQ)) {
      const n = pilas[id].length;
      pieza(id, '.adh-n').textContent = n;
      pieza(id, '.mas').textContent = n > HUECOS.length ? `+${n - HUECOS.length}` : '';
    }
  }

  function semaforo(sem) {
    const s = sem && sem[0];
    const ocupado = !!s;
    el.sem.classList.toggle('rojo', ocupado);
    el.sem.classList.toggle('verde', !ocupado);
    if (semOcupado !== null && semOcupado !== ocupado) {
      anima(el.sem.querySelector('.l-ambar'), [{ opacity: 0.18 }, { opacity: 1, offset: 0.2 }, { opacity: 1, offset: 0.7 }, { opacity: 0.18 }], { duration: 700 });
    }
    semOcupado = ocupado;
    el.wt.textContent = ocupado ? `worktree · ${NOM[AGENTE[s.agente]] ?? s.agente}` : 'worktree · libre';
  }

  function latido(e) {
    if (!e) return;
    svg.classList.toggle('sin-latido', !e.orquestador.vivo);
    const l = e.orquestador.ultimo_latido;
    if (ultimoLatido !== null && l !== ultimoLatido) {
      anima(el.led, [{ transform: 'scale(1)', opacity: 1 }, { transform: 'scale(1.8)', opacity: 0.5, offset: 0.3 }, { transform: 'scale(1)', opacity: 1 }], { duration: 600 });
    }
    ultimoLatido = l ?? null;
  }

  function chispas(diario) {
    const ids = diario.map(e => e.id);
    if (maxDiario !== null) {
      diario
        .filter(e => e.id > maxDiario && AGENTE[e.actor] && !e.accion.startsWith('bus_') && e.accion !== 'heartbeat')
        .slice(0, 4).reverse()
        .forEach((e, i) => {
          setTimeout(() => chispa(AGENTE[e.actor]), i * 300);
          cuenta(`${NOM[AGENTE[e.actor]]} · ${e.accion}`, e.resultado === 'error', i * 300);
        });
    }
    maxDiario = Math.max(maxDiario ?? 0, ...ids);
  }

  function resumen(d, av) {
    const s = d.semaforos && d.semaforos[0];
    const graves = Object.values(av).flat().filter(a => a.hot).map(a => a.txt);
    svg.setAttribute('aria-label',
      `Cola del modelo local: ${pilas.ml.length}. En revisión de Claude Code: ${pilas.cc.length}. ` +
      `Esperan tu decisión: ${pilas.tu.length}. Worktree ${s ? 'ocupado por ' + (NOM[AGENTE[s.agente]] ?? s.agente) : 'libre'}.` +
      (graves.length ? ' Avisos: ' + graves.join('; ') + '.' : ''));
  }

  // ------------------------------------------------------------------ demo

  // Un recorrido de ejemplo que no toca el servidor: una misión que se
  // reformula y se cierra, una obra, una rutina que falla y una propuesta.
  function nuevoDemo(id, modo, titulo) {
    demo.trabajos.unshift({ id, modo, titulo, estado: 'en_cola', resultado: null });
  }
  function estadoDemo(id, estado, resultado = null) {
    const t = demo.trabajos.find(x => x.id === id);
    if (t) Object.assign(t, { estado, resultado });
  }
  const GUION = [
    () => nuevoDemo('demo-1', 'mision', 'arreglar los tests de ea-bus'),
    () => nuevoDemo('demo-2', 'obra', 'portada con escena gashapon'),
    () => estadoDemo('demo-1', 'ejecutando'),
    () => estadoDemo('demo-1', 'en_revision', 'terminado'),
    () => { estadoDemo('demo-2', 'ejecutando'); nuevoDemo('demo-3', 'rutina', 'copia nocturna de la bóveda'); },
    () => estadoDemo('demo-1', 'en_cola'),
    () => {
      estadoDemo('demo-2', 'en_revision', 'terminado');
      demo.propuestas.unshift({ id: 'demo', proponente: 'claude-code', descripcion: 'reiniciar ea-mcp' });
    },
    () => { estadoDemo('demo-2', 'cerrado'); estadoDemo('demo-1', 'ejecutando'); },
    () => estadoDemo('demo-1', 'en_revision', 'bloqueado'),
    () => { estadoDemo('demo-1', 'cerrado'); estadoDemo('demo-3', 'ejecutando'); },
    () => estadoDemo('demo-3', 'en_espera', 'bloqueado'),
    () => {},
    () => { demo.propuestas = []; },
    () => estadoDemo('demo-3', 'en_revision', 'terminado'),
    () => estadoDemo('demo-3', 'cerrado'),
    () => {},
    () => { demo.trabajos = []; demo.propuestas = []; },
  ];

  function pasoDemo() {
    demo.paso++;
    if (demo.paso >= GUION.length) {
      demo.paso = -1;
      el.botonDemo.disabled = false;
      cuenta('fin de la demo');
      aplicar();
      return;
    }
    GUION[demo.paso]();
    aplicar();
    demo.timer = setTimeout(pasoDemo, 2000);
  }

  function lanzarDemo() {
    if (demo.paso >= 0 || !real) return;
    if (plegada) plegar(false);
    el.botonDemo.disabled = true;
    cuenta('demo: un recorrido de ejemplo, sin tocar nada');
    pasoDemo();
  }

  // ------------------------------------------------------------ montaje

  function plegar(v) {
    plegada = v;
    raiz.classList.toggle('plegada', v);
    el.botonPlegar.textContent = v ? '▸' : '▾';
    el.botonPlegar.title = v ? 'mostrar la escena' : 'plegar la escena';
    try { localStorage.setItem('ea-escena-plegada', v ? '1' : '0'); } catch { /* sin almacenamiento */ }
  }

  function montar(cont, opts = {}) {
    raiz = cont;
    if (opts.alAbrir) alAbrir = opts.alAbrir;
    cont.innerHTML = '<div class="esc-barra"><span class="esc-tit">⇄ Cómo se hablan los agentes</span>' +
      `<span class="esc-ley">${LEYENDA}</span><span class="grow"></span>` +
      '<button data-g="demo" title="un recorrido de ejemplo, sin tocar nada">▶ demo</button>' +
      '<button data-g="plegar"></button></div>' + escenaSVG();
    svg = cont.querySelector('svg');
    Object.assign(el, {
      caps: svg.querySelector('.caps'),
      efectos: svg.querySelector('.efectos'),
      avisos: svg.querySelector('.avisos'),
      hub: svg.querySelector('.hub'),
      led: svg.querySelector('.led'),
      sem: svg.querySelector('.sem'),
      wt: svg.querySelector('#g-wt'),
      ticker: svg.querySelector('.ticker'),
      demoEt: svg.querySelector('.demo-et'),
      botonDemo: cont.querySelector('[data-g=demo]'),
      botonPlegar: cont.querySelector('[data-g=plegar]'),
    });
    el.boc = { cc: bocadillo(), ml: bocadillo(), tu: bocadillo(), hub: bocadillo() };
    cuenta('esperando noticias de los agentes…');
    let v = false;
    try { v = localStorage.getItem('ea-escena-plegada') === '1'; } catch { /* sin almacenamiento */ }
    plegar(v);

    cont.addEventListener('click', ev => {
      const b = ev.target.closest('button[data-g]');
      if (b) {
        if (b.dataset.g === 'demo') lanzarDemo();
        else plegar(!plegada);
        return;
      }
      const c = ev.target.closest('.cap.real');
      if (c) alAbrir(c.dataset.cap);
    });
    // Al volver a la pestaña lo que estaba a medio camino termina donde debe.
    document.addEventListener('visibilitychange', () => { if (!document.hidden) recolocar(); });
  }

  function actualizar(datos) {
    real = {
      trabajos: datos.trabajos || [],
      propuestas: datos.propuestas || [],
      semaforos: datos.semaforos || [],
      estado: datos.estado || null,
      diario: datos.diario || [],
    };
    aplicar();
  }

  return { montar, actualizar };
})();
