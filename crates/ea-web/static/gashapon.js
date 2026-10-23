'use strict';

// La escena de la portada: un pequeño salón de máquinas de gashapon. Cada
// agente activo que detecta el servidor (GET /api/ecosistema) es una máquina;
// sus subagentes, máquinas pequeñas a su lado, unidas por un cable. Cada
// trabajo es una cápsula, y dónde está sale del servidor (`tareas[].lugar`).
// Las máquinas principales cuelgan del raíl del bus: por él viajan las
// cápsulas de una máquina a otra y las señales del diario hasta el
// orquestador. Nada se anima porque sí: lo que se mueve es la diferencia
// entre dos sondeos (una cápsula que cambia de lugar, una máquina que
// aparece o se apaga, un evento nuevo del diario) o un estado real que dura
// (una máquina trabajando).

const gasha = (() => {
  const NS = 'http://www.w3.org/2000/svg';
  const quieto = matchMedia('(prefers-reduced-motion: reduce)');

  // ---------------------------------------------------- variantes visuales
  // La familia la decide el servidor (ecosistema/familia.rs); aquí solo se
  // viste. Para un proveedor nuevo basta una entrada. Todas comparten la caja
  // blanca: cambian la tapa (c1), la placa (c2), el emblema y la faja.
  const VARIANTES = {
    claude: { nombre: 'Claude', c1: '#d97757', c2: '#8f3f22', emblema: '✳', faja: 'rayas' },
    qwen: { nombre: 'Qwen', c1: '#6f5bd3', c2: '#3c2e8f', emblema: '◆', faja: 'puntos' },
    codex: { nombre: 'Codex', c1: '#2f9e72', c2: '#17583f', emblema: '›_', faja: 'rejilla' },
    opencode: { nombre: 'OpenCode', c1: '#3e8ed0', c2: '#1d4f7e', emblema: '{ }', faja: 'rayas' },
    antigravity: { nombre: 'Antigravity', c1: '#d4a017', c2: '#7a5a00', emblema: '▲', faja: 'puntos' },
    local: { nombre: 'modelo local', c1: '#5b8a3a', c2: '#2f4f1a', emblema: '⌂', faja: 'rejilla' },
    desconocida: { nombre: 'desconocido', c1: '#8a8f98', c2: '#4a4e55', emblema: '?', faja: 'lisa' },
  };
  const variante = f => VARIANTES[f] ?? VARIANTES.desconocida;

  const COLOR = { mision: '#e5484d', obra: '#3e8ed0', rutina: '#f2c94c', propuesta: '#f28cb1' };
  const MODOS = { mision: 'misión', obra: 'obra', rutina: 'rutina' };
  const ESTADOS = {
    en_cola: 'en cola', ejecutando: 'ejecutando', en_revision: 'en revisión',
    en_espera: 'en espera', cerrado: 'cerrado', cancelado: 'cancelado',
  };
  const ESTADO_AG = { trabajando: 'trabajando', esperando: 'esperando', terminado: 'terminado' };

  // ------------------------------------------------------------- geometría
  // Una máquina se dibuja en coordenadas propias de 162×255 (caja de 150 más
  // el lateral) y se escala. El ancho de la escena sigue al del contenedor.
  const RIEL = 26;          // el raíl principal del bus
  const MARGEN = 16;
  const ESPINA = MARGEN + 6; // bajante que une los raíles de cada fila
  const R_VENTANA = 14;
  const fondoV = x => (x < 60 ? 104 + (x - 8) * 20 / 52 : x > 90 ? 124 - (x - 90) * 20 / 52 : 124);
  const FILAS = [[75, 47, 103, 22, 128], [61, 89, 33, 117], [75, 47, 103, 22, 128], [61, 89, 33, 117]];
  const HUECOS = FILAS.flatMap((xs, f) => xs.map(x => [x, fondoV(x) - R_VENTANA + 1 - f * 24]));
  const BANDEJA_MAX = 6;
  const MOSTRADOR_MAX = 5;

  let raiz, svg, plegada = false, alAbrir = () => {};
  const el = {};
  let W = 1000, H = 320;
  let HUB = { x: 80, y: RIEL }, VENT = { x: 920, y: RIEL };
  let BANDEJA = { x: 860, y: 280 }, PAPELERA = { x: 970, y: 282 };

  // Estado de la escena.
  const maqs = new Map();      // id -> máquina { a, atras, frente, rot, x, y, s, fila, ... }
  const caps = new Map();      // k -> cápsula
  const conocidos = new Map(); // k -> 'estado|lugar' del sondeo anterior
  const pilas = new Map();     // lugar -> [cápsulas]
  const filas = [];            // [{ riel, base }] de la última composición
  let real = null;
  let primera = true;
  let maxSenal = null, ultimoLatido = null;
  let avisosPrevios = new Set();
  let corriendo = false;
  let claveConductos = '';
  let pendienteLayout = false;

  // ------------------------------------------------------------ utilidades

  const corto = (s, n = 34) => {
    const t = String(s ?? '');
    return t.length > n ? t.slice(0, n - 1) + '…' : t;
  };
  const p2 = n => String(n).padStart(2, '0');
  const f1 = n => n.toFixed(1);

  function nodo(tipo, attrs = {}) {
    const n = document.createElementNS(NS, tipo);
    for (const [k, v] of Object.entries(attrs)) n.setAttribute(k, v);
    return n;
  }
  const escTxt = s => String(s ?? '').replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));

  const visible = () => raiz && !plegada && raiz.offsetParent !== null;
  const instantaneo = () => quieto.matches || document.hidden || !visible();

  function anima(n, frames, opts) {
    if (!n || !n.animate || instantaneo()) return null;
    return n.animate(frames, opts);
  }

  const suave = p => (p < 0.5 ? 4 * p * p * p : 1 - (-2 * p + 2) ** 3 / 2);
  const acelera = p => p * p;
  const frena = p => 1 - (1 - p) ** 2;
  const lineal = p => p;
  function rebote(p) {
    const n = 7.5625, d = 2.75;
    if (p < 1 / d) return n * p * p;
    if (p < 2 / d) return n * (p -= 1.5 / d) * p + 0.75;
    if (p < 2.5 / d) return n * (p -= 2.25 / d) * p + 0.9375;
    return n * (p -= 2.625 / d) * p + 0.984375;
  }

  function engranaje(cx, cy, r1, r2, n) {
    const pts = [];
    for (let k = 0; k < n * 4; k++) {
      const a = (k + 0.5) * Math.PI * 2 / (n * 4);
      const r = k % 4 < 2 ? r1 : r2;
      pts.push(`${f1(cx + r * Math.cos(a))},${f1(cy + r * Math.sin(a))}`);
    }
    return `M${pts.join('L')}Z`;
  }

  // ---------------------------------------------------------- las máquinas

  // La caja: lo que queda detrás de las cápsulas. Las luces (LED de la tapa,
  // luz interior, piloto) son las que dicen el estado.
  function maquinaAtras(v) {
    return '<ellipse class="sombra" cx="80" cy="255" rx="86" ry="6"/>' +
      '<path class="lado" d="M150 6 L162 12 L162 249 L150 255 Z"/>' +
      '<rect class="cuerpo" width="150" height="255" rx="8"/>' +
      '<rect class="interior" x="8" y="28" width="134" height="112" rx="3"/>' +
      '<rect class="luz" x="8" y="28" width="134" height="112" rx="3"/>' +
      '<path class="bandeja-v" d="M8 104 L60 124 L90 124 L142 104 L142 140 L8 140 Z"/>' +
      '<path class="tabique" d="M55 109 V121 M95 109 V121"/>' +
      '<rect class="adh-azul" x="12" y="148" width="32" height="38" rx="2"/>' +
      '<text class="adh-t" x="28" y="158">COLA</text><text class="adh-n" x="28" y="179">0</text>' +
      '<rect class="coin-in" x="54" y="150" width="46" height="14" rx="7"/>' +
      '<text class="coin-t" x="77" y="159.5">COIN IN</text>' +
      '<rect class="ranura" x="116" y="146" width="8" height="28" rx="4"/>' +
      '<g class="manivela"><circle class="aro" cx="78" cy="200" r="24"/><circle class="aro2" cx="78" cy="200" r="19"/>' +
      '<rect class="asa" x="72.5" y="180" width="11" height="40" rx="5"/></g>' +
      '<circle class="piloto" cx="124" cy="192" r="5"/>' +
      '<text class="logo" x="12" y="206">ガチャ</text>' +
      '<rect class="boca" x="12" y="212" width="40" height="36" rx="3"/>' +
      '<rect class="boca-in" x="15" y="215" width="34" height="30" rx="2"/>' +
      '<rect class="devol" x="110" y="205" width="16" height="10" rx="2"/>' +
      `<g class="emb"><rect x="102" y="220" width="40" height="24" rx="2"/><text x="122" y="236.5">${escTxt(v.emblema)}</text></g>` +
      `<rect class="faja" x="0" y="249" width="150" height="6" fill="url(#faja-${v.faja})"/>`;
  }

  // Lo que queda delante de las cápsulas: la tapa con su fila de LED y la
  // placa, el cristal con su reflejo, el labio de la bandeja y la trampilla.
  function maquinaDelante(nombre) {
    const leds = [0, 1, 2, 3, 4, 5].map(i => `<circle class="led" style="--i:${i}" cx="${24 + i * 20.4}" cy="5" r="2.6"/>`).join('');
    const txt = corto(nombre.toUpperCase(), 22);
    const ancho = Math.min(134, txt.length * 5.4 + 12);
    return '<path class="tapa" d="M0 8 Q0 0 8 0 H142 Q150 0 150 8 V28 H0 Z"/>' +
      '<path class="lado" d="M150 6 L162 12 L162 31 L150 28 Z"/>' + leds +
      `<rect class="placa" x="8" y="10" width="${ancho}" height="15" rx="4"/>` +
      `<text class="placa-t" x="14" y="21">${escTxt(txt)}</text>` +
      '<rect class="vidrio" x="8" y="28" width="134" height="112" rx="3"/>' +
      '<path class="reflejo" d="M14 32 L54 32 L24 104 L14 104 Z"/>' +
      '<path class="labio" d="M8 132 H142 V140 H8 Z"/>' +
      '<rect class="trampilla" x="15" y="215" width="34" height="14" rx="1"/>' +
      '<text class="mas" x="138" y="42"></text>';
  }

  function estiloVar(v) {
    return `--ag:${v.c1};--ag2:${v.c2}`;
  }

  function crearMaquina(a) {
    const v = variante(a.familia);
    const atras = nodo('g', { class: 'maq', 'data-m': a.id, style: estiloVar(v) });
    atras.innerHTML = maquinaAtras(v);
    const frente = nodo('g', { class: 'maq-f', 'data-m': a.id, style: estiloVar(v) });
    frente.innerHTML = maquinaDelante(a.nombre) + '<title></title>';
    const rot = nodo('g', { class: 'rot-m' });
    rot.innerHTML = '<text class="r1"></text><text class="r2"></text><text class="r3"></text>';
    el.atras.appendChild(atras);
    el.frente.appendChild(frente);
    el.rotulos.appendChild(rot);
    const m = { id: a.id, a, atras, frente, rot, familia: a.familia, nombre: a.nombre, x: 0, y: 0, s: 1, colocada: false };
    maqs.set(a.id, m);
    return m;
  }

  // Cambiar de familia o de nombre redibuja la máquina (raro: un modelo que
  // se conoce tarde).
  function revestir(m, a) {
    if (m.familia === a.familia && m.nombre === a.nombre) return;
    const v = variante(a.familia);
    m.atras.innerHTML = maquinaAtras(v);
    m.frente.innerHTML = maquinaDelante(a.nombre) + '<title></title>';
    m.atras.setAttribute('style', estiloVar(v));
    m.frente.setAttribute('style', estiloVar(v));
    m.familia = a.familia;
    m.nombre = a.nombre;
  }

  // Lo que cambia con el estado: luces, piloto, rótulos y la ficha del title.
  function vestirMaquina(m, a) {
    m.a = a;
    revestir(m, a);
    for (const g of [m.atras, m.frente]) {
      g.classList.toggle('trabajando', a.estado === 'trabajando');
      g.classList.toggle('esperando', a.estado === 'esperando');
      g.classList.toggle('apagada', a.estado === 'terminado');
      g.classList.toggle('sub', !!a.padre);
    }
    const v = variante(a.familia);
    const [r1, r2, r3] = m.rot.children;
    const grande = !a.padre;
    r1.textContent = corto(a.nombre, grande ? 26 : 14);
    if (grande) {
      r2.textContent = corto([a.modelo ?? v.nombre, a.rol, a.local && 'local'].filter(Boolean).join(' · '), 30);
      r3.textContent = a.actividad ? corto(a.actividad, 30) : ESTADO_AG[a.estado];
    } else {
      r2.textContent = a.estado === 'terminado' ? 'terminado' : corto(a.actividad || a.modelo || '', 15);
      r3.textContent = '';
    }
    m.rot.classList.toggle('sub', !grande);
    m.rot.classList.toggle('apagada', a.estado === 'terminado');
    const desde = a.desde ? new Date(a.desde).toLocaleTimeString() : null;
    m.frente.querySelector('title').textContent = [
      `${a.nombre} (${v.nombre})`,
      a.modelo && `modelo: ${a.modelo}${a.local ? ' (local)' : ''}`,
      a.rol && `rol: ${a.rol}`,
      `estado: ${ESTADO_AG[a.estado] ?? a.estado}`,
      a.actividad && `ahora: ${a.actividad}`,
      a.padre && `lanzado por: ${maqs.get(a.padre)?.a.nombre ?? a.padre}`,
      a.equipo && `equipo: ${a.equipo}`,
      a.proyecto && `en: ${a.proyecto}`,
      desde && `desde las ${desde}`,
    ].filter(Boolean).join('\n');
  }

  function colocarMaquina(m, x, y, s, fila) {
    const cambia = !m.colocada || Math.abs(m.x - x) + Math.abs(m.y - y) > 0.5 || Math.abs(m.s - s) > 0.001;
    Object.assign(m, { x, y, s, fila });
    if (!cambia) return;
    const cx = x + 75 * s, base = y + 255 * s;
    const grande = !m.a.padre;
    // Máquina y rótulo se deslizan juntos (o se colocan de golpe la primera vez).
    const destinos = [
      [m.atras, `translate(${f1(x)}px, ${f1(y)}px) scale(${s.toFixed(3)})`],
      [m.frente, `translate(${f1(x)}px, ${f1(y)}px) scale(${s.toFixed(3)})`],
      [m.rot, `translate(${f1(cx)}px, ${f1(base + (grande ? 15 : 11))}px)`],
    ];
    for (const [g, t] of destinos) {
      g.style.transition = !m.colocada || instantaneo() ? 'none' : '';
      g.style.transform = t;
    }
    const [r1, r2, r3] = m.rot.children;
    r2.setAttribute('y', grande ? 13 : 10);
    r3.setAttribute('y', 26);
    r1.removeAttribute('y');
    if (!m.colocada) {
      m.colocada = true;
      entra(m);
    }
  }

  // Una máquina nueva se enciende: sube un poco y las luces prenden una vez.
  function entra(m) {
    if (instantaneo() || primera) return;
    for (const g of [m.atras, m.frente, m.rot]) {
      g.classList.add('entra');
      g.addEventListener('animationend', () => g.classList.remove('entra'), { once: true });
    }
  }

  // Una máquina que ya no está: se apaga y se va. Lo que tuviera dentro ya
  // tiene su lugar nuevo (el servidor no lo deja en una máquina que no existe).
  function sale(m) {
    maqs.delete(m.id);
    const nodos = [m.atras, m.frente, m.rot];
    if (instantaneo()) { nodos.forEach(n => n.remove()); return; }
    for (const g of nodos) g.classList.add('sale');
    let hecho = false;
    const quitar = () => { if (!hecho) { hecho = true; nodos.forEach(n => n.remove()); } };
    m.atras.addEventListener('animationend', quitar, { once: true });
    setTimeout(quitar, 1200); // por si la animación no llega a terminar (pestaña oculta)
  }

  // ----------------------------------------------------------- composición

  // Cada máquina principal con sus subagentes es un bloque: la máquina a la
  // izquierda y los hijos en columnas de dos a su derecha, recursivamente y
  // cada nivel a la mitad de escala.
  function medir(n, s) {
    const mw = 162 * s, mh = 255 * s;
    const etq = n.a.padre ? 24 : 44;
    // La máquina va centrada en una columna tan ancha como su rótulo.
    const col = Math.max(mw, n.a.padre ? 80 : 176);
    const hs = n.hijos.map(h => medir(h, Math.max(0.2, s * 0.52)));
    const cols = [];
    for (let i = 0; i < hs.length; i += 2) {
      const col = hs.slice(i, i + 2);
      cols.push({ col, w: Math.max(...col.map(c => c.w)), h: col.reduce((t, c) => t + c.h, 0) + (col.length - 1) * 6 });
    }
    const hw = cols.reduce((t, c) => t + c.w, 0) + Math.max(0, cols.length - 1) * 4;
    return {
      n, s, mw, mh, etq, cols, col,
      w: col + (cols.length ? 8 + hw : 0),
      h: Math.max(mh + etq, ...cols.map(c => c.h)),
    };
  }

  function colocarBloque(b, x, base, fila, sitios) {
    sitios.push({ id: b.n.a.id, x: x + (b.col - b.mw) / 2, y: base - b.etq - b.mh, s: b.s, fila });
    let cx = x + b.col + 8;
    for (const c of b.cols) {
      let bb = base;
      for (const h of c.col) { colocarBloque(h, cx, bb, fila, sitios); bb -= h.h + 6; }
      cx += c.w + 4;
    }
  }

  // Dónde va cada máquina. Determinista: el orden es el del servidor.
  function componer(agentes) {
    const porId = new Map(agentes.map(a => [a.id, { a, hijos: [] }]));
    const raices = [];
    for (const n of porId.values()) {
      const p = n.a.padre && porId.get(n.a.padre);
      if (p) p.hijos.push(n); else raices.push(n);
    }
    const escala = W < 640 ? 0.42 : W < 900 ? 0.52 : 0.6;
    const bloques = raices.map(n => medir(n, escala));
    const ancho = W - 2 * MARGEN - 34;
    const lineas = [];
    let actual = null;
    for (const b of bloques) {
      if (!actual || (actual.w + 16 + b.w > ancho && actual.bs.length)) { actual = { bs: [], w: -16, h: 0 }; lineas.push(actual); }
      actual.bs.push(b);
      actual.w += 16 + b.w;
      actual.h = Math.max(actual.h, b.h);
    }
    const sitios = [];
    filas.length = 0;
    let arriba = RIEL + 34;
    lineas.forEach((l, i) => {
      const base = arriba + l.h;
      const riel = i === 0 ? RIEL : arriba - 16;
      filas.push({ riel, base });
      let x = ESPINA + 34 + Math.max(0, (ancho - l.w) / 2);
      for (const b of l.bs) { colocarBloque(b, x, base, i, sitios); x += b.w + 16; }
      arriba = base + 42;
    });
    const fin = filas.length ? filas[filas.length - 1].base : RIEL + 34 + 110;
    return { sitios, fin };
  }

  // -------------------------------------------------------------- caminos

  const cima = m => ({ x: m.x + 75 * m.s, y: m.y });
  const bocaDe = m => ({ x: m.x + 32 * m.s, y: m.y + 226 * m.s });
  const rielDe = fila => filas[fila]?.riel ?? RIEL;

  // De un lugar hasta el raíl principal, por el raíl de su fila y la espina.
  function subida(lugar) {
    const m = maqs.get(lugar);
    if (m && !m.a.padre) {
      const c = cima(m), r = rielDe(m.fila);
      const pts = [c, { x: c.x, y: r }];
      if (m.fila > 0) pts.push({ x: ESPINA, y: r }, { x: ESPINA, y: RIEL });
      return { pts, fila: m.fila };
    }
    if (lugar === 'usuario') return { pts: [{ x: VENT.x, y: RIEL }], fila: 0 };
    return { pts: [{ x: HUB.x, y: RIEL }], fila: 0 };
  }

  // El camino por los conductos entre dos lugares (máquinas, bus o usuario).
  function camino(de, a) {
    const s1 = subida(de), s2 = subida(a);
    if (s1.fila === s2.fila && s1.fila > 0) { s1.pts.length = 2; s2.pts.length = 2; }
    return [...s1.pts, ...s2.pts.reverse()];
  }

  // ------------------------------------------------------ fondo y conductos

  function defs() {
    return '<defs>' +
      '<pattern id="faja-rayas" width="6" height="6" patternUnits="userSpaceOnUse" patternTransform="rotate(45)"><rect width="6" height="6" fill="var(--ag)"/><rect width="3" height="6" fill="var(--ag2)"/></pattern>' +
      '<pattern id="faja-puntos" width="6" height="6" patternUnits="userSpaceOnUse"><rect width="6" height="6" fill="var(--ag)"/><circle cx="3" cy="3" r="1.3" fill="var(--ag2)"/></pattern>' +
      '<pattern id="faja-rejilla" width="5" height="5" patternUnits="userSpaceOnUse"><rect width="5" height="5" fill="var(--ag)"/><path d="M0 0 H5 M0 0 V5" stroke="var(--ag2)" stroke-width="1"/></pattern>' +
      '<pattern id="faja-lisa" width="4" height="4" patternUnits="userSpaceOnUse"><rect width="4" height="4" fill="var(--ag)"/></pattern>' +
      '</defs>';
  }

  // Raíles, bajantes, cables de los subagentes y los conductos con trabajo
  // en curso. Solo se rehace cuando cambia algo de eso.
  function conductos(tareas) {
    const tops = [...maqs.values()].filter(m => !m.a.padre);
    const subs = [...maqs.values()].filter(m => m.a.padre && maqs.has(m.a.padre));
    // Pares planificador–ejecutor con un trabajo entre ellos, y hacia dónde va.
    const flujos = new Map();
    for (const t of tareas) {
      if (!t.planificador || !t.ejecutor || !maqs.has(t.planificador) || !maqs.has(t.ejecutor)) continue;
      if (!['en_cola', 'ejecutando', 'en_revision'].includes(t.estado)) continue;
      const vuelve = t.estado === 'en_revision';
      const k = `${t.planificador}>${t.ejecutor}>${vuelve ? 'v' : 'i'}`;
      flujos.set(k, { de: vuelve ? t.ejecutor : t.planificador, a: vuelve ? t.planificador : t.ejecutor });
    }
    const clave = [W, ...filas.map(f => f.riel + ':' + f.base), ...tops.map(m => `${m.id}@${f1(m.x)},${f1(m.y)}`),
      ...subs.map(m => `${m.id}@${f1(m.x)},${f1(m.y)}:${m.a.estado}`), ...flujos.keys()].join('|');
    if (clave === claveConductos) return;
    claveConductos = clave;

    const pl = pts => 'M' + pts.map(p => `${f1(p.x)} ${f1(p.y)}`).join(' L');
    let d = `M${ESPINA} ${RIEL} H${VENT.x}`;
    filas.forEach((f, i) => {
      const xs = tops.filter(m => m.fila === i).map(m => cima(m).x);
      if (i > 0 && xs.length) d += ` M${ESPINA} ${RIEL} V${f.riel} H${f1(Math.max(...xs))}`;
    });
    for (const m of tops) { const c = cima(m); d += ` M${f1(c.x)} ${rielDe(m.fila)} V${f1(c.y + 2)}`; }
    let html = `<path class="riel" d="${d}"/><path class="riel-in" d="${d}"/>`;
    // Jerarquía: un cable fino del costado del padre a la peana del hijo.
    for (const m of subs) {
      const p = maqs.get(m.a.padre);
      const x0 = p.x + 156 * p.s, y0 = p.y + 196 * p.s;
      const x1 = m.x + 2, y1 = m.y + 236 * m.s;
      const dx = Math.max(10, (x1 - x0) / 2);
      html += `<path class="cable${m.a.estado === 'terminado' ? ' off' : ''}" style="--ag:${variante(p.a.familia).c1}" ` +
        `d="M${f1(x0)} ${f1(y0)} C${f1(x0 + dx)} ${f1(y0)} ${f1(x1 - dx)} ${f1(y1)} ${f1(x1)} ${f1(y1)}"/>` +
        `<rect class="peana" x="${f1(m.x - 2)}" y="${f1(m.y + 255 * m.s)}" width="${f1(156 * m.s)}" height="2" rx="1"/>`;
    }
    // Orquestación: el tramo de raíl por el que va y viene el trabajo.
    for (const { de, a } of flujos.values()) {
      html += `<path class="flujo" style="--ag:${variante(maqs.get(a).a.familia).c1}" d="${pl(camino(de, a))}"/>`;
    }
    el.conductos.innerHTML = html;
  }

  // Lo fijo que se recoloca con el ancho: orquestador, mostrador, bandeja.
  function infra(fin) {
    HUB = { x: ESPINA + 44, y: RIEL };
    VENT = { x: W - MARGEN - 46, y: RIEL };
    const suelo = fin + 26;
    BANDEJA = { x: W - MARGEN - 150, y: suelo - 12 };
    PAPELERA = { x: W - MARGEN - 18, y: suelo - 10 };
    H = suelo + 34;
    svg.setAttribute('viewBox', `0 0 ${W} ${f1(H)}`);
    el.hub.setAttribute('transform', `translate(${HUB.x} ${HUB.y})`);
    el.hubRot.setAttribute('x', HUB.x);
    el.vent.setAttribute('transform', `translate(${VENT.x} ${VENT.y})`);
    el.salida.setAttribute('transform', `translate(${BANDEJA.x - 16} ${suelo - 26})`);
    el.papelera.setAttribute('transform', `translate(${PAPELERA.x - 12} ${suelo - 30})`);
    el.ticker.setAttribute('y', f1(H - 10));
  }

  function escenaSVG() {
    return `<svg class="esc" viewBox="0 0 ${W} ${H}" role="img" aria-label="Escena de los agentes">` + defs() +
      '<g class="conductos"></g>' +
      // el orquestador: caja de engranajes en el raíl
      '<g class="hub"><g class="hub-in"><rect x="-34" y="-14" width="68" height="28" rx="6"/>' +
      `<path class="engranaje" d="${engranaje(-14, 0, 9.5, 6.5, 8)}"/>` +
      '<circle class="eje" cx="-14" cy="0" r="2.5"/><circle class="led-hub" cx="16" cy="0" r="4.5"/></g><title>orquestador (el bus)</title></g>' +
      `<text class="rot" id="g-hub" x="0" y="${RIEL + 28}">orquestador</text>` +
      // el mostrador del usuario: lo que espera tu decisión
      '<g class="vent"><rect class="vent-caja" x="-30" y="-13" width="60" height="26" rx="6"/>' +
      '<text class="vent-t" y="4">TÚ</text><text class="rot vent-n" x="30" y="40"></text><title>lo que espera tu decisión</title></g>' +
      // bandeja de salida y papelera
      '<g class="salida"><path class="bandeja-s" d="M0 4 L7 23 H117 L124 4"/><text class="rot" x="62" y="38">cerrados</text></g>' +
      '<g class="papelera-g"><path class="papelera" d="M2 3 L6 33 H22 L26 3 Z"/><rect class="papelera-b" x="0" y="0" width="28" height="4" rx="1"/></g>' +
      '<g class="maqs-atras"></g>' +
      '<g class="caps"></g>' +
      '<g class="maqs-frente"></g>' +
      '<g class="rotulos"></g>' +
      '<g class="esc-vacio" visibility="hidden"><text class="vacio-t" y="0">No hay agentes activos</text>' +
      '<text class="vacio-s" y="18">Las máquinas se encienden solas al abrir una sesión de Claude Code o al despertar el orquestador.</text></g>' +
      '<g class="efectos"></g>' +
      '<g class="avisos"></g>' +
      '<text class="ticker" x="16" y="0"></text>' +
      '</svg>';
  }

  // --------------------------------------------------------------- cápsulas

  function crearCap(k, modo) {
    const g = nodo('g', { class: 'cap' + (k[0] === 'j' ? ' real' : '') });
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
    c.el.setAttribute('transform', `translate(${f1(c.x)} ${f1(c.y)}) rotate(${Math.round(c.rot % 360)}) scale(${c.s.toFixed(3)})`);
    c.el.style.opacity = c.op;
  }

  function ponEn(c, h) {
    Object.assign(c, { op: 1, ...h });
    c.rot = 0;
    pinta(c);
  }

  function vestir(c, t) {
    c.el.querySelector('title').textContent = `${MODOS[t.modo] ?? t.modo} · ${t.titulo} · ${ESTADOS[t.estado] ?? t.estado}`;
    c.el.classList.toggle('hot', t.estado === 'en_espera' || (t.estado === 'en_revision' && t.resultado === 'bloqueado'));
    c.el.classList.toggle('run', t.estado === 'ejecutando');
    c.el.classList.toggle('cerrada', t.estado === 'cerrado');
  }

  const pila = l => { if (!pilas.has(l)) pilas.set(l, []); return pilas.get(l); };

  // Dónde se queda la i-ésima cápsula de un lugar con n.
  function hueco(lugar, i, n, c) {
    const m = maqs.get(lugar);
    if (m) {
      // Lo que se ejecuta está en la boca de la máquina; lo demás, tras el cristal.
      if (c?.run) {
        const b = bocaDe(m);
        return { x: b.x + (i - (n - 1) / 2) * 6 * m.s, y: b.y - 4 * m.s, s: 1.25 * m.s, op: 1 };
      }
      const [lx, ly] = HUECOS[Math.min(i, HUECOS.length - 1)];
      return { x: m.x + lx * m.s, y: m.y + ly * m.s, s: R_VENTANA / 10 * m.s, op: i < HUECOS.length ? 1 : 0 };
    }
    if (lugar === 'usuario') return { x: VENT.x - 28 + (i % MOSTRADOR_MAX) * 14, y: RIEL + 25, s: 0.62, op: i < MOSTRADOR_MAX ? 1 : 0 };
    if (lugar === 'bandeja') return { x: BANDEJA.x + i * 19, y: BANDEJA.y, s: 0.85, op: 1 };
    // bus: en el raíl, a la derecha del orquestador
    return { x: HUB.x + 50 + i * 17, y: RIEL, s: 0.7, op: 1 };
  }

  // Las cápsulas de una máquina: las que se ejecutan aparte de la cola.
  function huecoDe(c) {
    const p = pilas.get(c.lugar);
    if (!p) return { x: c.x, y: c.y, s: c.s };
    const mismos = p.filter(x => !!x.run === !!c.run);
    return hueco(c.lugar, mismos.indexOf(c), mismos.length, c);
  }

  function sacaDePila(c) {
    const p = pilas.get(c.lugar);
    if (p) { const i = p.indexOf(c); if (i >= 0) p.splice(i, 1); }
  }

  function quita(c) {
    sacaDePila(c);
    c.el.remove();
    caps.delete(c.k);
  }

  function recolocar(forzar = false) {
    const ya = forzar || instantaneo();
    for (const p of pilas.values()) {
      for (const c of p) {
        if (c.segs.length) continue;
        const h = huecoDe(c);
        if (Math.abs(h.x - c.x) + Math.abs(h.y - c.y) < 0.5 && Math.abs(h.s - c.s) < 0.01 && h.op === c.op) continue;
        if (ya) ponEn(c, h);
        else c.segs.push({ a: h, dur: 320, ease: frena });
      }
    }
    animar();
  }

  // ----------------------------------------------------------------- viajes

  // Un viaje: sale de donde esté (una máquina lo suelta por arriba girando la
  // manivela), recorre los conductos y entra en su lugar.
  function viaje(c, de, a, retraso) {
    sacaDePila(c);
    c.lugar = a;
    if (a !== 'papelera' && a !== 'fuera') pila(a).push(c);
    if (instantaneo()) {
      c.segs = [];
      if (a === 'papelera' || a === 'fuera') quita(c);
      else ponEn(c, huecoDe(c));
      return;
    }
    const s = [];
    if (retraso) s.push({ espera: retraso });
    const mDe = maqs.get(de);
    if (mDe) {
      const top = cima(mDe);
      s.push({ a: { x: top.x, y: top.y + 6, s: 0.9 * Math.max(mDe.s, 0.5) }, dur: 260, ease: acelera, al: () => { manivela(de); trampilla(de); } });
    }
    const destinoConducto = a === 'bandeja' || a === 'papelera' || a === 'fuera' ? null : a;
    const pts = camino(de, destinoConducto ?? 'bus');
    // Si no hay conducto (bandeja, papelera), basta llegar al raíl más cercano.
    const ruta = destinoConducto ? pts : pts.slice(0, Math.max(1, mDe ? (mDe.fila > 0 ? 4 : 2) : 1));
    let prev = mDe ? cima(mDe) : { x: c.x, y: c.y };
    for (const p of ruta) {
      const dist = Math.hypot(p.x - prev.x, p.y - prev.y);
      if (dist < 1) continue;
      s.push({ a: { x: p.x, y: p.y, s: 0.75 }, dur: Math.max(70, dist / 0.5), ease: lineal });
      prev = p;
    }
    const mA = maqs.get(a);
    if (mA) {
      s.push({ a: { ...cima(mA), y: mA.y + 34 * mA.s, s: R_VENTANA / 10 * mA.s }, dur: 220, ease: acelera });
      s.push({ a: () => huecoDe(c), dur: 260, ease: frena, fin: () => { ranura(a); recolocar(); } });
    } else if (a === 'bandeja') {
      s.push({ a: () => ({ ...huecoDe(c), y: BANDEJA.y - 34 }), arco: true, ease: suave });
      s.push({ a: () => huecoDe(c), dur: 340, ease: rebote, fin: () => recolocar() });
    } else if (a === 'papelera') {
      s.push({ a: { x: PAPELERA.x, y: PAPELERA.y - 36, s: 1 }, arco: true, ease: suave });
      s.push({ a: { x: PAPELERA.x, y: PAPELERA.y, op: 0.2 }, dur: 300, ease: acelera, fin: () => { puf(PAPELERA.x, PAPELERA.y - 12); quita(c); } });
    } else if (a === 'fuera') {
      s.push({ a: { s: 0, op: 0 }, dur: 380, ease: acelera, fin: () => quita(c) });
    } else {
      s.push({ a: () => huecoDe(c), dur: 300, ease: rebote, fin: () => recolocar() });
    }
    c.segs = s;
    animar();
  }

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
            const { de, hasta } = sg;
            const dist = Math.hypot(hasta.x - de.x, hasta.y - de.y);
            const alto = Math.max(RIEL, Math.min(de.y, hasta.y) - 40 - Math.abs(hasta.x - de.x) * 0.2);
            sg.ctrl = { x: (de.x + hasta.x) / 2, y: 2 * alto - (de.y + hasta.y) / 2 };
            sg.dur = Math.min(1300, Math.max(500, 400 + dist * 0.8));
            sg.giro = 540;
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

  // Un solo bucle de animación para todas las cápsulas, y solo mientras
  // alguna viaja.
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

  // ------------------------------------------------- gestos de las máquinas

  const pieza = (id, sel) => svg.querySelector(`[data-m="${CSS.escape(id)}"] ${sel}`);

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
  function sacude(n) {
    const frames = [
      { transform: 'translate(0,0)' }, { transform: 'translate(-3px,1px)' }, { transform: 'translate(3px,-1px)' },
      { transform: 'translate(-2px,0)' }, { transform: 'translate(0,0)' },
    ];
    anima(n, frames, { duration: 180, iterations: 3 });
  }

  // Un nodo de efecto se va al acabar su animación; si la animación no llega
  // a terminar (pestaña en segundo plano), lo quita un temporizador.
  function efimero(n, an, dur) {
    const fuera = () => n.remove();
    an.onfinish = an.oncancel = fuera;
    setTimeout(fuera, dur + 400);
  }

  function puf(x, y) {
    if (instantaneo()) return;
    for (let i = 0; i < 4; i++) {
      const p = nodo('circle', { class: 'puf', cx: x, cy: y, r: 2.5 });
      el.efectos.appendChild(p);
      const a = (i / 4) * Math.PI * 2 + 0.6;
      const an = p.animate([{ transform: 'translate(0,0)', opacity: 0.9 }, { transform: `translate(${Math.cos(a) * 14}px,${Math.sin(a) * 10 - 6}px)`, opacity: 0 }], { duration: 450, easing: 'ease-out' });
      efimero(p, an, 450);
    }
  }

  // Una señal: un pulso que recorre los conductos de una máquina al
  // orquestador (un evento del diario firmado por ese agente).
  function pulso(de, error) {
    const pts = camino(de, 'bus');
    if (instantaneo() || pts.length < 2) return;
    const m = maqs.get(de);
    const color = error ? 'var(--hot)' : m ? variante(m.a.familia).c1 : 'var(--accent)';
    const p = nodo('circle', { class: 'chispa', cx: 0, cy: 0, r: 3.2 });
    p.style.fill = color;
    el.efectos.appendChild(p);
    let total = 0;
    const largos = pts.map((q, i) => (i ? (total += Math.hypot(q.x - pts[i - 1].x, q.y - pts[i - 1].y)) : 0));
    const frames = pts.map((q, i) => ({ transform: `translate(${f1(q.x)}px,${f1(q.y)}px)`, offset: total ? largos[i] / total : i / (pts.length - 1), opacity: i === pts.length - 1 ? 0.2 : 1 }));
    const dur = Math.min(1600, Math.max(500, total / 0.6));
    efimero(p, p.animate(frames, { duration: dur, easing: 'ease-in-out' }), dur);
    if (m) anima(pieza(de, '.piloto'), [{ opacity: 1 }, { opacity: 0.3, offset: 0.4 }, { opacity: 1 }], { duration: 500 });
    anima(el.ledHub, [{ opacity: 1 }, { opacity: 0.2, offset: 0.5 }, { opacity: 1 }], { duration: 300, delay: 700 });
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

  const nombreLugar = l => (l === 'usuario' ? 'tú' : l === 'bus' ? 'el bus' : maqs.get(l)?.a.nombre ?? 'el bus');

  function mensaje(t, lugar, antes) {
    const q = `«${corto(t.titulo)}»`;
    const en = nombreLugar(lugar);
    switch (t.estado) {
      case 'en_cola': return [lugar === 'bus' ? `${q} espera en el bus: no hay ejecutor encendido` : `${q} entra en la cola de ${en}`];
      case 'ejecutando': return [`${en} toma ${q}`];
      case 'en_revision':
        if (antes === 'en_espera') return [`confirmas ${q}: vuelve a revisión`];
        return t.resultado === 'bloqueado' ? [`✕ ${q} bloqueado: lo revisa ${en}`, true] : [`${q} entregado: lo revisa ${en}`];
      case 'en_espera': return [`! ${q} falló: el paquete de corrección te espera`, true];
      case 'cerrado': return [`${q} cerrado`];
      case 'cancelado': return [`⊘ ${q} cancelado`];
      default: return [`${q} · ${t.estado}`];
    }
  }

  // ------------------------------------------------------------- los avisos

  function bocadillo() {
    const g = nodo('g', { class: 'boc', visibility: 'hidden' });
    g.append(nodo('path', {}), nodo('rect', { y: -11, height: 21, rx: 5 }), nodo('text', { y: 4 }));
    el.avisos.appendChild(g);
    return g;
  }

  // Avisos graves del sistema, junto al orquestador; las máquinas con algo
  // bloqueado se marcan con el piloto rojo.
  function avisos(d) {
    const lista = [];
    const e = d.estado;
    if (e && !e.orquestador.vivo) lista.push({ k: 'latido', txt: '♥ sin latido: no se ejecuta nada' });
    const caidos = e ? e.servicios.filter(s => s.activo !== 'active' && s.activo !== 'desconocido').map(s => s.nombre) : [];
    if (caidos.length) lista.push({ k: 'srv' + caidos.join(','), txt: `▦ parado: ${caidos.join(', ')}` });
    const g = el.boc;
    el.hub.classList.toggle('alerta', lista.length > 0);
    if (lista.some(a => !avisosPrevios.has(a.k))) sacude(el.hubIn);
    avisosPrevios = new Set(lista.map(a => a.k));
    if (!lista.length) { g.setAttribute('visibility', 'hidden'); return lista; }
    const txt = lista[0].txt + (lista.length > 1 ? ` (+${lista.length - 1})` : '');
    const ancho = txt.length * 6.4 + 18;
    const x = Math.min(W - 6 - ancho / 2, HUB.x + 44 + ancho / 2);
    g.setAttribute('transform', `translate(${f1(x)} ${HUB.y})`);
    g.setAttribute('class', 'boc hot');
    g.querySelector('rect').setAttribute('x', -ancho / 2);
    g.querySelector('rect').setAttribute('width', ancho);
    g.querySelector('text').textContent = txt;
    g.querySelector('path').setAttribute('d', `M${-ancho / 2} -5 L${-ancho / 2 - 8} 0 L${-ancho / 2} 5 Z`);
    g.setAttribute('visibility', 'visible');
    return lista;
  }

  // --------------------------------------------------------------- el modelo

  function maquinas(eco) {
    const vivos = new Set(eco.agentes.map(a => a.id));
    for (const m of [...maqs.values()]) if (!vivos.has(m.id)) sale(m);
    for (const a of eco.agentes) vestirMaquina(maqs.get(a.id) ?? crearMaquina(a), a);
    const { sitios, fin } = componer(eco.agentes);
    infra(fin);
    for (const s of sitios) colocarMaquina(maqs.get(s.id), s.x, s.y, s.s, s.fila);
    el.vacio.setAttribute('visibility', eco.agentes.length ? 'hidden' : 'visible');
    el.vacio.setAttribute('transform', `translate(${W / 2} ${RIEL + 34 + 50})`);
    raiz.classList.toggle('sin-agentes', !eco.agentes.length);
  }

  // Cada trabajo a su lugar: el que da el servidor para los activos; la
  // bandeja para los últimos cerrados y la papelera para los cancelados.
  function capsulas(d) {
    const tareas = new Map((d.ecosistema.tareas || []).map(t => [t.trabajo, t]));
    const lugarDe = t => tareas.get(t.id)?.lugar ?? (t.estado === 'cerrado' ? 'bandeja' : t.estado === 'cancelado' ? 'papelera' : 'bus');
    const cambios = d.trabajos.filter(t => conocidos.get('j' + t.id) !== `${t.estado}|${lugarDe(t)}`).length;
    const sinViaje = primera || cambios > 10;
    let retraso = 0;
    const turno = () => { const r = retraso; retraso += 280; return r; };
    const vivos = new Set();
    const recientes = new Set(d.trabajos.filter(t => t.estado === 'cerrado').slice(0, BANDEJA_MAX).map(t => 'j' + t.id));

    for (const t of [...d.trabajos].reverse()) {
      const k = 'j' + t.id;
      vivos.add(k);
      const a = lugarDe(t);
      const antes = conocidos.get(k);
      const ahora = `${t.estado}|${a}`;
      conocidos.set(k, ahora);
      let c = caps.get(k);
      if (sinViaje) {
        if (!c && (a === 'papelera' || (a === 'bandeja' && !recientes.has(k)))) continue;
        if (!c) c = crearCap(k, t.modo);
        c.run = t.estado === 'ejecutando';
        vestir(c, t);
        if (c.lugar !== a) {
          sacaDePila(c);
          if (a === 'papelera') { quita(c); continue; }
          c.lugar = a;
          pila(a).push(c);
          c.segs = [];
        }
        continue;
      }
      if (antes === ahora) { if (c) vestir(c, t); continue; }
      if (!c && antes === undefined && a === 'papelera') continue;
      const estadoAntes = antes?.split('|')[0];
      let de;
      if (c) de = c.lugar;
      else {
        // Nueva: sale de su planificador (o de ti, si es una rutina).
        const tarea = tareas.get(t.id);
        de = tarea?.del_usuario ? 'usuario' : tarea?.planificador ?? 'bus';
        c = crearCap(k, t.modo);
        const m = maqs.get(de);
        ponEn(c, { ...(m ? cima(m) : hueco(de, 0, 1)), op: 0, s: 0.6 });
      }
      c.run = t.estado === 'ejecutando';
      vestir(c, t);
      const r = turno();
      if (de === a) { sacaDePila(c); pila(a).push(c); recolocar(); }
      else viaje(c, de, a, r);
      if (estadoAntes !== t.estado) {
        const [txt, hot] = mensaje(t, a, estadoAntes);
        cuenta(txt, hot, r + 400);
      }
    }

    // Las propuestas pendientes esperan en el mostrador.
    for (const p of d.propuestas) {
      const k = 'p' + p.id;
      vivos.add(k);
      if (caps.has(k)) continue;
      const c = crearCap(k, 'propuesta');
      c.el.querySelector('title').textContent = `propuesta #${p.id} · ${p.descripcion}`;
      if (sinViaje) { c.lugar = 'usuario'; pila('usuario').push(c); continue; }
      // De la máquina del agente que la propone, si hay una sola encendida.
      const suyas = [...maqs.values()].filter(m => !m.a.padre && m.a.agente === p.proponente);
      const de = suyas.length === 1 ? suyas[0].id : 'bus';
      ponEn(c, { ...(suyas.length === 1 ? cima(suyas[0]) : hueco('bus', 0, 1)), op: 0, s: 0.6 });
      const r = turno();
      viaje(c, de, 'usuario', r);
      cuenta(`${nombreLugar(de)} → tú · propuesta «${corto(p.descripcion)}»`, false, r + 400);
    }

    // Lo que ya no está: propuestas decididas, trabajos borrados.
    for (const c of [...caps.values()]) {
      if (vivos.has(c.k) || c.lugar === null || c.lugar === 'fuera' || c.lugar === 'papelera') continue;
      if (sinViaje) { quita(c); continue; }
      if (c.k[0] === 'p') cuenta(`decides la propuesta #${c.k.slice(1)}`, false, retraso + 400);
      viaje(c, c.lugar, 'fuera', turno());
    }

    const bandeja = pilas.get('bandeja') ?? [];
    while (bandeja.length > BANDEJA_MAX) desvanecer(bandeja[0]);

    recolocar(sinViaje);
  }

  // Contadores de cada máquina y del mostrador; piloto rojo en la que tiene
  // algo bloqueado.
  function contadores() {
    for (const m of maqs.values()) {
      const p = (pilas.get(m.id) ?? []).filter(c => !c.run);
      m.atras.querySelector('.adh-n').textContent = p.length;
      m.frente.querySelector('.mas').textContent = p.length > HUECOS.length ? `+${p.length - HUECOS.length}` : '';
      const alerta = (pilas.get(m.id) ?? []).some(c => c.el.classList.contains('hot'));
      m.atras.classList.toggle('alerta', alerta);
      m.frente.classList.toggle('alerta', alerta);
    }
    const u = pilas.get('usuario') ?? [];
    el.ventN.textContent = u.length ? `${u.length} espera${u.length > 1 ? 'n' : ''} tu decisión` : '';
    el.vent.classList.toggle('alerta', u.some(c => c.el.classList.contains('hot')));
    el.vent.classList.toggle('suave', u.length > 0);
  }

  function senales(lista) {
    const nuevas = maxSenal === null ? [] : lista.filter(s => s.id > maxSenal);
    nuevas.slice(0, 4).reverse().forEach((s, i) => {
      const de = s.de && (maqs.has(s.de) || s.de === 'usuario') ? s.de : null;
      if (de) {
        if (instantaneo()) pulso(de, s.error);
        else setTimeout(() => pulso(de, s.error), i * 300);
      }
      cuenta(`${de ? nombreLugar(de) : s.actor} · ${s.accion.replace(/_/g, ' ')}`, s.error, i * 300);
    });
    maxSenal = Math.max(maxSenal ?? 0, ...lista.map(s => s.id), 0);
  }

  function latido(e) {
    if (!e) return;
    svg.classList.toggle('sin-latido', !e.orquestador.vivo);
    const l = e.orquestador.ultimo_latido;
    if (ultimoLatido !== null && l !== ultimoLatido) {
      anima(el.ledHub, [{ transform: 'scale(1)', opacity: 1 }, { transform: 'scale(1.8)', opacity: 0.5, offset: 0.3 }, { transform: 'scale(1)', opacity: 1 }], { duration: 600 });
    }
    ultimoLatido = l ?? null;
  }

  function resumen(d, av) {
    const ags = d.ecosistema.agentes;
    const principales = ags.filter(a => !a.padre);
    const desc = principales.map(a => {
      const hijos = ags.filter(h => h.padre === a.id && h.estado !== 'terminado').length;
      return `${a.nombre}${a.modelo ? ' (' + a.modelo + ')' : ''} ${ESTADO_AG[a.estado]}` +
        (a.actividad ? ` en ${a.actividad}` : '') + (hijos ? `, con ${hijos} subagente${hijos > 1 ? 's' : ''}` : '');
    });
    const u = (pilas.get('usuario') ?? []).length;
    svg.setAttribute('aria-label',
      (principales.length ? `${principales.length} agente${principales.length > 1 ? 's' : ''} activo${principales.length > 1 ? 's' : ''}: ${desc.join('; ')}.` : 'No hay agentes activos.') +
      (u ? ` Esperan tu decisión: ${u}.` : '') +
      (av.length ? ' Avisos: ' + av.map(a => a.txt).join('; ') + '.' : ''));
  }

  function aplicar() {
    if (!real || !svg) return;
    const d = real;
    maquinas(d.ecosistema);
    capsulas(d);
    conductos(d.ecosistema.tareas || []);
    contadores();
    latido(d.estado);
    senales(d.ecosistema.senales || []);
    resumen(d, avisos(d));
    primera = false;
  }

  // Al cambiar el ancho, la escena se recompone (una vez por fotograma).
  function alCambiarAncho() {
    const w = Math.round(Math.min(1200, Math.max(360, raiz.clientWidth)));
    if (!w || w === W || pendienteLayout) return;
    pendienteLayout = true;
    requestAnimationFrame(() => {
      pendienteLayout = false;
      W = w;
      claveConductos = '';
      if (real) { aplicar(); recolocar(true); }
    });
  }

  // ------------------------------------------------------------ montaje

  const LEYENDA = Object.entries({ misión: COLOR.mision, obra: COLOR.obra, rutina: COLOR.rutina, propuesta: COLOR.propuesta })
    .map(([n, c]) => `<span><i style="color:${c}">●</i>${n}</span>`).join('') +
    '<span title="cada máquina es un agente activo; las pequeñas, sus subagentes">▣ agente · ▪ subagente</span>';

  function plegar(v) {
    plegada = v;
    raiz.classList.toggle('plegada', v);
    el.botonPlegar.textContent = v ? '▸' : '▾';
    el.botonPlegar.title = v ? 'mostrar la escena' : 'plegar la escena';
    try { localStorage.setItem('ea-escena-plegada', v ? '1' : '0'); } catch { /* sin almacenamiento */ }
    if (!v) alCambiarAncho();
  }

  function montar(cont, opts = {}) {
    raiz = cont;
    if (opts.alAbrir) alAbrir = opts.alAbrir;
    W = Math.round(Math.min(1200, Math.max(360, cont.clientWidth || 1000)));
    cont.innerHTML = '<div class="esc-barra"><span class="esc-tit">⇄ Los agentes, ahora</span>' +
      `<span class="esc-ley">${LEYENDA}</span><span class="grow"></span>` +
      '<button data-g="plegar"></button></div>' + escenaSVG();
    svg = cont.querySelector('svg');
    Object.assign(el, {
      conductos: svg.querySelector('.conductos'),
      atras: svg.querySelector('.maqs-atras'),
      frente: svg.querySelector('.maqs-frente'),
      rotulos: svg.querySelector('.rotulos'),
      caps: svg.querySelector('.caps'),
      efectos: svg.querySelector('.efectos'),
      avisos: svg.querySelector('.avisos'),
      hub: svg.querySelector('.hub'),
      hubRot: svg.querySelector('#g-hub'),
      ledHub: svg.querySelector('.led-hub'),
      vent: svg.querySelector('.vent'),
      ventN: svg.querySelector('.vent-n'),
      salida: svg.querySelector('.salida'),
      papelera: svg.querySelector('.papelera-g'),
      hubIn: svg.querySelector('.hub-in'),
      vacio: svg.querySelector('.esc-vacio'),
      ticker: svg.querySelector('.ticker'),
      botonPlegar: cont.querySelector('[data-g=plegar]'),
    });
    el.boc = bocadillo();
    infra(RIEL + 34 + 110);
    cuenta('esperando noticias de los agentes…');
    let v = false;
    try { v = localStorage.getItem('ea-escena-plegada') === '1'; } catch { /* sin almacenamiento */ }
    plegar(v);

    cont.addEventListener('click', ev => {
      if (ev.target.closest('button[data-g=plegar]')) { plegar(!plegada); return; }
      const c = ev.target.closest('.cap.real');
      if (c) alAbrir(c.dataset.cap);
    });
    // Al volver a la pestaña lo que estaba a medio camino termina donde debe.
    document.addEventListener('visibilitychange', () => { if (!document.hidden) recolocar(true); });
    if (window.ResizeObserver) new ResizeObserver(alCambiarAncho).observe(cont);
  }

  function actualizar(datos) {
    real = {
      trabajos: datos.trabajos || [],
      propuestas: datos.propuestas || [],
      estado: datos.estado || null,
      ecosistema: datos.ecosistema || { agentes: [], tareas: [], senales: [] },
    };
    aplicar();
  }

  return { montar, actualizar, VARIANTES };
})();
