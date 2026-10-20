'use strict';

const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];

const SECCIONES = ['inicio', 'trabajos', 'diario', 'rutinas', 'propuestas', 'acciones', 'sistema', 'boveda', 'memoria', 'ajustes'];
// Qué filas de cada sección tienen un detalle que pedir al servidor (por prefijo de clave).
const DETALLE = { inicio: 'j', trabajos: 'j', rutinas: 'r', boveda: 'b', memoria: 'm' };
const RIESGO = { lectura: '○', reversible: '◐', irreversible: '●' };
// Estados de un trabajo: glifo y palabra. El orden es el del recorrido.
const EST = {
  en_cola: ['○', 'en cola'], ejecutando: ['◐', 'ejecutando'], en_revision: ['◇', 'en revisión'],
  en_espera: ['!', 'en espera'], cerrado: ['●', 'cerrado'], cancelado: ['⊘', 'cancelado'],
};
const FINAL = ['cerrado', 'cancelado'];
const MODO = { mision: 'misión', obra: 'obra', rutina: 'rutina' };
// La mitad de arriba de la cápsula es el modo (los colores de la escena).
const COLOR = { mision: '#e5484d', obra: '#3e8ed0', rutina: '#f2c94c', propuesta: '#f28cb1' };
// Cómo luce la cápsula según el estado: gira, lleva halo, se apaga.
const IC = { ejecutando: 'run', en_espera: 'hot', cerrado: 'cerrada', cancelado: 'cancelada' };
const CLAUDE = '#d97757'; // el color de Claude Code (placa de la escena)
const ACTIVOS = ['en_cola', 'ejecutando', 'en_revision', 'en_espera'];
const NOMBRE = { 'modelo-local': 'modelo local', 'claude-code': 'Claude Code', orquestador: 'orquestador', usuario: 'tú' };
const TITULOS = {
  inicio: ['Orquestador', 'Las colas, quién tiene el worktree y por dónde va cada cápsula: en cola → ejecutando → en revisión → en espera → cerrado.'],
  trabajos: ['Trabajos', 'Cada trabajo es una cápsula: misión, obra o rutina. Ábrela para ver su premio (el informe), el diff y lo que admite.'],
  diario: ['Diario', 'El registro de todo lo que ocurre en el ecosistema, lo más reciente arriba.'],
  rutinas: ['Rutinas', 'Máquinas de cápsulas amarillas, versionadas y registradas. Echa una ficha para lanzar una ahora sin esperar a su hora.'],
  propuestas: ['Propuestas', 'Cambios que un agente quiere hacer y que necesitan tu visto bueno.'],
  acciones: ['Acciones', 'Las herramientas del ecosistema. Simula primero para ver qué pasaría; después ejecuta.'],
  sistema: ['Sistema', 'Los servicios que mantienen el ecosistema en marcha y los agentes que trabajan en él.'],
  boveda: ['Bóveda', 'Las notas de los agentes y las tuyas, como en Obsidian. Elige una en el árbol; ✎ (Ctrl+E) la edita y ⋯ tiene el resto. El buscador busca también dentro del contenido.'],
  memoria: ['Memoria', 'Lo que Claude Code recuerda de cada proyecto. Elige una nota en el árbol; ✎ (Ctrl+E) la edita y ⋯ tiene el resto.'],
  ajustes: ['Ajustes', 'Cómo se comporta el ecosistema. Lo marcado ⌨ se cambia en una terminal con `ea ajustes`.'],
};
const VACIO = {
  trabajos: 'No hay trabajos. Se lanzan desde Claude Code con /mision, /obra o /rutina.',
  diario: 'El diario está vacío.',
  rutinas: 'No hay rutinas registradas (ea rutina registrar).',
  propuestas: 'Nada espera tu decisión.',
  boveda: 'No hay notas.',
  memoria: 'Ningún proyecto tiene memoria (~/.claude/projects/*/memory).',
};

const st = {
  sec: 'inicio',
  estado: null,        // ea.estado()
  todos: [],           // ea.trabajos(), siempre al día
  trabajos: [],        // los de todos que se ven: los del proyecto elegido, o todos
  proyecto: null,      // cwd del proyecto elegido en la barra (filtra trabajos, diario y escena)
  sesiones: [],        // ea.sesiones(): sesiones vivas de Claude Code
  proyectos: null,     // claves (cwd) de los proyectos de la barra, para detectar los nuevos
  nuevos: {},          // cwd -> instante hasta el que su tarjeta luce la clase nueva
  datos: null,         // datos de la sección visible
  detalle: {},         // detalle cargado de las filas abiertas
  diffs: {},           // diff de los trabajos con el diff desplegado (id -> respuesta)
  abiertos: new Set(), // claves abiertas (j<id>, r<slug>, …)
  sel: null,           // clave seleccionada
  enlace: null,        // clave de la fila que va en el hash (#seccion/<fila>)
  irA: null,           // clave de la fila que hay que enseñar al pintarla
  vis: [],             // claves visibles, en orden
  html: '',
  q: '',
  filtroEstado: null,
  propTodas: false,
  latidos: false,
  armado: null,        // { n, hasta }: reinicio de servicio pendiente de confirmar
  ocupado: false,
  otra: false,         // hay que repetir el sondeo al terminar el actual
  consola: {},         // por herramienta: { v: valores, sim, res, err }
  rutEnt: {},          // por rutina: { v: entradas escritas, previa, err }
  ed: {},              // estado de cada editor (memoria, bóveda): ver ed()
  ajBorrador: {},      // clave -> lo escrito en el control de un ajuste, sin guardar
  sondeoMs: null,      // web.sondeo_ms de los ajustes (null: 3000)
  diarioLimite: null,  // web.diario_limite de los ajustes (null: 300)
  relojTick: null,     // el intervalo del sondeo
};

// ------------------------------------------------------------------ utilidades

const esc = s => String(s ?? '').replace(/[&<>"']/g,
  c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));

const ms = t => (typeof t === 'number' ? t * 1000 : Date.parse(t));
const p2 = n => String(n).padStart(2, '0');

function unidades(s) {
  if (s < 60) return s + 's';
  if (s < 3600) return Math.floor(s / 60) + 'm';
  if (s < 86400) return Math.floor(s / 3600) + 'h';
  return Math.floor(s / 86400) + 'd';
}
const hace = t => (t == null ? '' : unidades(Math.max(0, Math.floor((Date.now() - ms(t)) / 1000))));
const en = t => (t == null ? '' : '→ ' + unidades(Math.max(0, Math.floor((ms(t) - Date.now()) / 1000))));

// Un tiempo transcurrido que se actualiza solo (ver relojes()): va vacío en
// el HTML para que el paso de los segundos no obligue a repintar la lista.
const reloj = t => (t == null ? '' : `<time data-t="${ms(t)}"></time>`);
function relojes() {
  $$('time[data-t]').forEach(e => {
    const txt = unidades(Math.max(0, Math.floor((Date.now() - Number(e.dataset.t)) / 1000)));
    if (e.textContent !== txt) e.textContent = txt;
  });
}

function hora(t) {
  const d = new Date(ms(t));
  const h = `${p2(d.getHours())}:${p2(d.getMinutes())}:${p2(d.getSeconds())}`;
  return d.toDateString() === new Date().toDateString() ? h : `${p2(d.getDate())}/${p2(d.getMonth() + 1)} ${h.slice(0, 5)}`;
}

const dur = n => (n < 1000 ? n + 'ms' : n < 60000 ? (n / 1000).toFixed(1) + 's' : Math.round(n / 60000) + 'm');

let avisoTimer;
function avisa(msg, ok = false) {
  const a = $('#aviso');
  a.textContent = (ok ? '' : '✕ ') + msg;
  a.className = 'v' + (ok ? ' ok' : '');
  clearTimeout(avisoTimer);
  avisoTimer = setTimeout(() => { a.className = ''; }, 5000);
}

// La cápsula de la escena, en pequeño. `cls`: run, hot, cerrada, cancelada,
// averiada. Con halo (hot) necesita más margen alrededor.
const capsula = (color, cls = '', tam = 18, rombo = false) =>
  `<svg class="ic ${cls}" width="${tam}" height="${tam}" viewBox="${cls === 'hot' ? '-15 -15 30 30' : '-12 -12 24 24'}" aria-hidden="true"><g class="giro">` +
  '<circle class="halo" r="13"/><path class="baja" d="M-10 0 A10 10 0 0 0 10 0 Z"/>' +
  `<path class="alta" fill="${color}" d="M-10 0 A10 10 0 0 1 10 0 Z"/>` +
  '<rect class="junta" x="-10.4" y="-1" width="20.8" height="2" rx="1"/><circle class="aguj" cx="-5" cy="-3.2" r="1"/>' +
  '<circle class="aguj" cx="0" cy="-4.6" r="1"/><circle class="aguj" cx="5" cy="-3.2" r="1"/>' +
  '<ellipse class="brillo" cx="-4" cy="-6" rx="3" ry="1.4" transform="rotate(-25 -4 -6)"/>' +
  (rombo ? '<path class="rombo" d="M0 -8.5 L3.5 -5 L0 -1.5 L-3.5 -5 Z"/>' : '') + '</g></svg>';
const capsulaJ = (t, tam) => capsula(COLOR[t.modo] ?? '#8f949c', IC[t.estado] ?? '', tam);

// ---------------------------------------------------------------------- filas

function fila(k, cls, row, txt, det = '', extra = '') {
  const abierto = st.abiertos.has(k);
  return {
    k,
    txt: txt.toLowerCase(),
    html: `<article class="f${abierto ? ' o' : ''}${st.sel === k ? ' s' : ''}${extra}" data-k="${esc(k)}" tabindex="0">` +
      `<div class="r ${cls}">${row}</div>${abierto && det ? `<div class="det">${det}</div>` : ''}</article>`,
  };
}

const btn = (a, glifo, titulo, datos = {}, cls = '') =>
  `<button class="b ${cls}" data-a="${a}" title="${esc(titulo)}" aria-label="${esc(titulo)}"` +
  Object.entries(datos).map(([k, v]) => ` data-${k}="${esc(v)}"`).join('') + `>${glifo}</button>`;

// Lo que un evento cuenta de sí mismo en una línea: su error o su texto.
function extracto(e) {
  if (!e.detalle) return '';
  try {
    const d = JSON.parse(e.detalle);
    const x = d && (d.error ?? d.texto);
    return x == null ? '' : String(x);
  } catch { return e.detalle; }
}

function eventos(lista) {
  return lista.map(e => {
    const err = e.resultado === 'error';
    const ex = extracto(e);
    return `<div class="ev"><span>${hora(e.ts)}</span>` +
      `<i class="${err ? 'hot' : ''}">${e.resultado === 'ok' ? '✓' : err ? '✕' : '·'}</i>` +
      `<span>${esc(e.actor)}</span><span>${esc(e.accion)}</span><span>${e.duracion_ms != null ? dur(e.duracion_ms) : ''}</span>` +
      (ex ? `<span class="ex${err ? ' hot' : ''}">${esc(ex)}</span>` : '') + `</div>`;
  }).join('');
}

// ------------------------------------------------------------------- trabajos

const estadoJ = t => `<span class="est ${t.estado}">${capsulaJ(t)}${EST[t.estado][1]}</span>`;

// «fase 2/5» o «iteración 3», lo que diga más del avance.
function avance(t) {
  if (t.fases_total) return `fase ${Math.min(t.fase_actual ?? 1, t.fases_total)}/${t.fases_total}`;
  return t.iteracion ? `iteración ${t.iteracion}` : '';
}

// El informe es el premio del trabajo: [texto, clase]. En una tarjeta
// (`largo`) dice además por qué falló.
function premio(t, largo = false) {
  const av = avance(t);
  const con = (...xs) => xs.filter(Boolean).join(' · ');
  if (t.estado === 'cancelado') return ['⊘ sin premio', ''];
  if (t.resultado === 'bloqueado' || t.estado === 'en_espera') {
    const roto = t.estado === 'cerrado' ? '✕ premio roto' : '✕ bloqueado';
    return [largo && t.detalle ? '✕ ' + t.detalle.slice(0, 120) : roto, 'hot'];
  }
  if (t.resultado && t.estado === 'en_revision') return [con(largo ? '✓ trae premio' : '★ terminado', av), largo ? '' : 'pz'];
  if (t.resultado) return [t.obra_completa && t.fases_total ? `★ obra completa · ${t.fases_total}/${t.fases_total}` : con('★ terminado', av), 'pz'];
  return [av, ''];
}

// Los botones que admite un trabajo según su estado. `i` es el informe, si hay.
function accionesTrabajo(t, i, mas = '') {
  const id = esc(t.id);
  const b = (a, g, tt, cls = '') => btn(a, g, tt, { id: t.id }, cls);
  let h = '';
  if (t.estado === 'en_revision') {
    h += b('cerrar', '★ canjear <span>cerrar</span>', 'cerrar: darlo por bueno', 'coin');
    if (i && !i.obra_completa) h += b('continuar', '▶ seguir girando <span class="dd">continuar</span>', 'la obra sigue con las fases que quedan');
    h += `<input class="nota" placeholder="nota para volver a girar" aria-label="Nota para reformular" maxlength="10000">` +
      b('reformular', '↺ volver a girar <span class="dd">reformular</span>', 'devolverlo al modelo local con esta nota (obligatoria)');
  } else if (t.estado === 'en_espera') {
    h += `<textarea class="nota" rows="2" placeholder="contexto que se añade al paquete (opcional)" aria-label="Contexto" maxlength="10000"></textarea>` +
      b('confirmar', '✓ desatascar', 'pasa el paquete de corrección a Claude Code', 'coin hot');
  }
  h += mas;
  if (!FINAL.includes(t.estado)) h += b('cancelar', '⊘ devolver', 'cancelar el trabajo (segundo clic para confirmar)', 'hot');
  return h ? `<div class="acc" data-j="${id}">${h}</div>` : '';
}

function informeHtml(d) {
  const i = d.informe;
  if (!i) return '<p class="m">Todavía no hay informe.</p>';
  const bloq = i.resultado === 'bloqueado';
  let h = '';
  if (i.motivo) h += `<p class="et">motivo</p><pre class="tx">${esc(i.motivo)}</pre>`;
  if (i.evidencia) h += `<p class="et">evidencia</p><pre class="tx">${esc(i.evidencia)}</pre>`;
  if (i.diff_stat) h += `<p class="et">diff stat</p><pre class="tx">${esc(i.diff_stat)}</pre>`;
  if (i.fases?.length) {
    h += '<p class="et">fases</p>' + i.fases.map(f =>
      `<p><i class="${f.resultado === 'hecha' ? '' : 'hot'}">${f.resultado === 'hecha' ? '✓' : '✕'}</i> ${f.numero}. ${esc(f.titulo)} <span class="dd">${esc(f.resultado)}${f.diff_stat ? ' · ' + esc(f.diff_stat.trim().split('\n').pop()) : ''}</span></p>`).join('');
  }
  return `<div class="caja-premio"><div class="cab-p"><span class="premio">★ PREMIO</span>` +
    `<span class="${bloq ? 'hot' : 'x'}">${bloq ? '✕ bloqueado' : '✓ terminado'}</span>` +
    `<span class="m">· obra_completa: ${i.obra_completa ? 'sí' : 'no'} · ${i.fases_hechas} fases hechas · ${i.duracion_s}s</span></div>` +
    `<div class="cuerpo-p">${h}</div></div>`;
}

function diffHtml(id) {
  const abierto = st.diffs[id] !== undefined;
  let h = `<p>${btn('diff', (abierto ? '▾' : '▸') + ' diff', 'mostrar u ocultar el diff del trabajo', { id }, 'l')}</p>`;
  const x = st.diffs[id];
  if (x) h += x.error ? `<p class="hot">${esc(x.error)}</p>` : `<pre class="diff">${colorDiff(x.diff || '(sin cambios)')}${x.truncado ? '\n… (recortado)' : ''}</pre>`;
  return h;
}

// Cada línea del diff con su color: cabeceras y stat apagados, hunks en el
// acento, añadidos y borrados en verde y rojo.
function colorDiff(txt) {
  return esc(txt).split('\n').map(l => {
    const c = /^(diff |index |\+\+\+ |--- | \S.*\|)/.test(l) ? 'd-st'
      : l.startsWith('@@') ? 'd-h' : l[0] === '+' ? 'd-mas' : l[0] === '-' ? 'd-menos' : '';
    return c ? `<span class="${c}">${l}</span>` : l;
  }).join('\n');
}

function filasTrabajos(ts) {
  return ts.map(t => {
    const k = 'j' + t.id;
    const [pz, pzCls] = premio(t);
    const row = estadoJ(t) + `<span class="n">${esc(t.id.slice(9, 15))}</span>` +
      `<span class="x">${esc(t.titulo)}</span><span class="m w">${MODO[t.modo] ?? t.modo}${t.rutina ? ' · ' + esc(t.rutina.slug) + ' v' + t.rutina.version : ''}</span>` +
      `<span class="m ${pzCls}">${esc(pz || '—')}</span><span class="m">${reloj(t.actualizado)}</span>`;
    const d = st.detalle[k];
    let det = `<p class="m">${esc(t.id)} · creado ${hora(t.creado)}${t.detalle ? ' · ' + esc(t.detalle) : ''}</p>`;
    if (d) {
      const o = d.orden;
      det += `<p class="et">objetivo</p><p class="tx">${esc(o.objetivo)}</p>` +
        `<p class="m">${esc(o.worktree)}${o.criterio_exito ? ' · criterio: ' + esc(o.criterio_exito) : ''}${o.revisar_cada ? ' · revisa cada ' + o.revisar_cada : ''}</p>` +
        (o.notas_revision?.length ? '<p class="et">notas de revisión</p>' + o.notas_revision.map(n => `<p class="tx">${esc(n)}</p>`).join('') : '') +
        informeHtml(d) +
        (d.paquete ? `<p class="et hot">paquete de corrección</p><pre class="tx">${esc(d.paquete)}</pre>` : '') +
        (d.informe?.commit_inicio ? diffHtml(t.id) : '') +
        `<p class="et">historial</p><div class="evs">` + d.historial.map(h =>
          `<div class="ev"><span>${hora(h.ts)}</span><i></i><span>${esc(h.evento)}</span><span>${esc(EST[h.estado]?.[1] ?? h.estado)}</span><span>${h.iteracion ? 'it. ' + h.iteracion : ''}</span></div>`).join('') + '</div>' +
        accionesTrabajo(t, d.informe);
    } else {
      det += accionesTrabajo(t, null);
    }
    return fila(k, 'j', row, `${t.titulo} ${t.modo} ${t.id} ${EST[t.estado][1]} ${t.resultado ?? ''}`, det, FINAL.includes(t.estado) ? ' fin' : '');
  });
}


function pretty(txt) {
  if (!txt) return '';
  try { return JSON.stringify(JSON.parse(txt), null, 1); } catch { return txt; }
}

// El trabajo del bus al que se refiere un evento del diario, si lo hay.
function trabajoDe(e) {
  if (e.trabajo) return e.trabajo;
  try { return JSON.parse(e.detalle)?.trabajo || null; } catch { return null; }
}

function filasDiario(evs) {
  return evs.map(e => {
    const k = 'e' + e.id;
    const row = `<span class="m">${hora(e.ts)}</span>` +
      `<i class="${e.resultado === 'error' ? 'hot' : 'm'}">${e.resultado === 'ok' ? '✓' : e.resultado === 'error' ? '✕' : '·'}</i>` +
      `<span class="m w">${esc(e.actor)}</span><span class="x">${esc(e.accion)}</span>` +
      `<span class="n" title="${esc(e.trabajo ?? '')}">${e.trabajo ? esc(e.trabajo.slice(-8)) : ''}</span><span class="m">${e.duracion_ms != null ? dur(e.duracion_ms) : ''}</span>`;
    const det = `<p class="m">#${e.id} · ${esc(e.actor)} · ${new Date(e.ts).toLocaleString()}</p>` +
      (e.detalle ? `<pre>${esc(pretty(e.detalle))}</pre>` : '') +
      (trabajoDe(e) ? `<p>${btn('ir-trabajo', '→ trabajo', 'ver el trabajo', { id: trabajoDe(e) }, 'l')}</p>` : '');
    return fila(k, 'd', row, `${e.actor} ${e.accion} ${e.trabajo ?? ''} ${e.detalle ?? ''}`, det);
  });
}

// Las entradas de una rutina (con su valor por defecto) y la vista previa de
// la orden que recibirá el agente. Lo de .sal caduca al cambiar un campo.
function formEntradas(r) {
  if (r.error) return '';
  const re = st.rutEnt[r.slug] || {};
  const ents = Object.entries(r.entradas ?? {});
  let h = '<div class="fm">' + (ents.length ? ents.map(([k, def]) =>
    `<label class="m">${esc(k)} <input data-rent="${esc(r.slug)}" data-ent="${esc(k)}" value="${esc(re.v?.[k] ?? def)}" ` +
    `title="entrada ${esc(k)} (por defecto: ${esc(def)})" aria-label="entrada ${esc(k)}"></label>`).join('') : '<span class="m">sin entradas</span>') +
    btn('rprevia', '◌ vista previa', 'muestra el objetivo y el criterio que recibirá el agente, sin lanzar nada', { slug: r.slug }) + '</div>';
  if (re.previa) {
    h += `<p class="tx sal">criterio: <b>${esc(re.previa.criterio)}</b></p><pre class="toml sal">${esc(re.previa.objetivo)}</pre>` +
      `<div class="acc sal">${btn('lanzar', '<span class="ranura" aria-hidden="true"><i></i></span>echar ficha', 'lanzar con estas entradas', { slug: r.slug }, 'coin')}</div>`;
  }
  if (re.err) h += `<p class="hot sal">✕ ${esc(re.err)}</p>`;
  return h;
}

function filasRutinas(rs) {
  return rs.map(r => {
    const k = 'r' + r.slug;
    // Una máquina de cápsulas amarillas: con halo si la última se atascó,
    // apagada si su rutina.toml no se puede leer.
    const suyos = st.trabajos.filter(t => t.rutina?.slug === r.slug);
    const atascada = suyos.some(t => t.estado === 'en_espera');
    const enCola = suyos.some(t => t.estado === 'en_cola');
    const desc = r.error ?? (r.descripcion ?? '') + (atascada ? ' · la última se atascó' : '');
    const row = capsula(COLOR.rutina, r.error ? 'averiada' : atascada ? 'hot' : '') +
      `<span class="x">${esc(r.slug)}</span><span class="m">v${r.version}</span>` +
      `<span class="m ${r.error || atascada ? 'hot' : ''}">${esc(desc)}</span>` +
      `<span class="m">${r.proxima && !r.error ? en(r.proxima) : ''}</span>` +
      (r.error ? '<span class="hot fuera">fuera de servicio</span>'
        : btn('lanzar', enCola ? '✓ en la cúpula' : '<span class="ranura" aria-hidden="true"><i></i></span>echar ficha',
          `Lanzar ahora la rutina ${r.slug} con las entradas de su ficha: crea un trabajo; el orquestador lo recoge en segundos`, { slug: r.slug }, enCola ? 'coin ya' : 'coin'));
    const d = st.detalle[k];
    const det = d ? `<p class="m">versión ${d.version} · ${esc(r.worktree ?? '')}${r.criterio_exito ? ' · criterio: ' + esc(r.criterio_exito) : ''}</p>` +
      formEntradas(r) + `<pre class="toml">${esc(d.contenido ?? '')}</pre>` : '';
    return fila(k, 'u', row, `${r.slug} ${r.descripcion ?? ''}`, det);
  });
}

function filasPropuestas(ps) {
  return ps.map(p => {
    const k = 'p' + p.id;
    const pend = p.estado === 'pendiente';
    const ac = p.accion;
    const irrev = ac && ac.riesgo === 'irreversible';
    const falla = p.ejecutada && p.ejecutada.estado === 'fallida';
    const glifo = pend ? '◇' : falla ? '!' : p.estado === 'aceptada' ? '✓' : '✕';
    // Aceptar una propuesta con acción la ejecuta: pide confirmar (segundo
    // clic). Una irreversible no se confirma desde aquí, solo en terminal.
    const armada = st.armado && st.armado.n === k && st.armado.hasta > Date.now();
    let acc = '';
    if (pend) {
      if (irrev) acc += `<span class="m" title="irreversible: solo desde la terminal (ea propuestas decidir ${p.id})">⌨ terminal</span>`;
      else if (ac) acc += btn('decidir', armada ? '✓ confirmar' : '✓ aceptar', armada ? 'confirmar: ejecuta ' + ac.herramienta : 'aceptar (ejecuta la acción; pide confirmar)', { id: p.id, v: 'aceptar', ac: 1 }, armada ? 'coin hot' : 'coin');
      else acc += btn('decidir', '✓ aceptar', 'aceptar', { id: p.id, v: 'aceptar' }, 'coin');
      acc += btn('decidir', '✕ rechazar', 'rechazar', { id: p.id, v: 'rechazar' }, 'hot');
    }
    const quien = ac ? `${ac.herramienta} [${ac.riesgo}]` : '';
    const row = `<span title="${glifo} ${p.estado}${p.ejecutada ? ' · ' + p.ejecutada.estado : ''}">${capsula(COLOR.propuesta, falla ? 'hot' : pend ? '' : 'cerrada', 18, true)}</span><span class="n">#${p.id}</span>` +
      `<span class="x">${esc(p.descripcion)}</span><span class="m w">${esc(quien)}</span><span class="m">${reloj(p.creada)}</span><span class="bs">${acc}</span>`;
    let det = `<p class="tx">${esc(p.descripcion)}</p>`;
    if (ac) {
      det += `<p class="m">${esc(ac.herramienta)} · riesgo ${esc(ac.riesgo)} · propuesta por ${esc(p.proponente ?? '?')}</p>` +
        `<p class="m">${esc(JSON.stringify(ac.argumentos))}</p><p class="tx">Simulación: ${esc(ac.simulacion)}</p>`;
      if (irrev && pend) det += `<p class="hot">Irreversible: confírmala desde la terminal con <b>ea propuestas decidir ${p.id} --aceptar</b>.</p>`;
    }
    if (p.ejecutada) det += `<p class="${falla ? 'hot' : 'tx'}">Ejecución ${esc(p.ejecutada.estado)}: ${esc(p.ejecutada.detalle ?? '')}</p>`;
    if (p.decidida) det += `<p class="m">${hora(p.decidida)}</p>`;
    return fila(k, 'p', row, `${p.descripcion} ${quien} ${p.estado}`, det, pend ? '' : ' fin');
  });
}

// ------------------------------------------------------------------- markdown

// Markdown mínimo, propio y sin dependencias: todo el texto se escapa antes
// de formatear y solo los enlaces http(s) y los internos llegan a ser enlaces.
// `ctx`: { wiki(destino), md(url) } devuelven la clave de la nota enlazada (o
// null si no hay ninguna), y `roto` es el título de un enlace sin destino.
function mdEnLinea(txt, ctx) {
  const guardados = [];
  const guarda = html => `\u0001${guardados.push(html) - 1}\u0002`;
  let s = esc(txt.replace(/[\u0001\u0002]/g, ''));
  s = s.replace(/`([^`]+)`/g, (_, c) => guarda(`<code>${c}</code>`));
  s = s.replace(/\[\[([^\[\]\n]+)\]\]/g, (_, n) => {
    const [destino, alias] = n.split('|');
    const k = ctx.wiki?.(destino.split('#')[0].trim());
    const txt = alias?.trim() || `[[${n}]]`; // con alias, como en Obsidian, solo el alias
    return guarda(k ? `<a href="#" class="ml" data-ml="${esc(k)}">${txt}</a>` : `<span class="roto" title="${esc(ctx.roto ?? 'no hay esa nota')}">${txt}</span>`);
  });
  const enfasis = t => t
    .replace(/\*\*(?=\S)(.+?)(?<=\S)\*\*/g, '<strong>$1</strong>')
    .replace(/(^|[^*\w])\*(?=[^*\s])(.+?)(?<=[^*\s])\*(?!\*)/g, '$1<em>$2</em>')
    .replace(/(^|[^\w])_(?=[^_\s])(.+?)(?<=[^_\s])_(?!\w)/g, '$1<em>$2</em>');
  s = s.replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, (_, etiqueta, url) => {
    const t = enfasis(etiqueta);
    if (/^https?:\/\//i.test(url)) return guarda(`<a href="${url}" target="_blank" rel="noopener noreferrer">${t}</a>`);
    const k = ctx.md?.(url);
    if (k) return guarda(`<a href="#" class="ml" data-ml="${esc(k)}">${t}</a>`);
    return guarda(`<span class="roto" title="${url}">${t}</span>`); // relativo a otra cosa, javascript:… → sin enlace
  });
  s = enfasis(s);
  for (let i = 0; i < 4 && s.includes('\u0001'); i++) s = s.replace(/\u0001(\d+)\u0002/g, (_, n) => guardados[n]);
  return s;
}

function md(texto, ctx = {}) {
  const lineas = texto.replace(/\r\n?/g, '\n').replace(/^---\n[\s\S]*?\n---[ \t]*(\n|$)/, '').split('\n');
  const enLinea = t => mdEnLinea(t, ctx);
  const ITEM = /^(\s*)([-*+]|\d+[.)])\s+(.*)$/;
  const FENCE = /^\s*```/;
  const HR = /^\s*([-*_])(\s*\1){2,}\s*$/;
  const SEP = /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$/;
  const celdas = l => l.trim().replace(/^\||\|$/g, '').split('|').map(c => c.trim());
  const inicio = l => FENCE.test(l) || /^#{1,6}\s/.test(l) || HR.test(l) || /^\s*>/.test(l) || ITEM.test(l);
  let out = '';
  let i = 0;
  while (i < lineas.length) {
    const l = lineas[i];
    if (!l.trim()) { i++; continue; }
    if (FENCE.test(l)) {
      const cod = [];
      for (i++; i < lineas.length && !FENCE.test(lineas[i]); i++) cod.push(lineas[i]);
      i++;
      out += `<pre><code>${esc(cod.join('\n'))}</code></pre>`;
    } else if (/^#{1,6}\s/.test(l)) {
      const [, almohadillas, t] = /^(#+)\s+(.*?)\s*#*\s*$/.exec(l);
      out += `<h${almohadillas.length}>${enLinea(t)}</h${almohadillas.length}>`;
      i++;
    } else if (HR.test(l)) {
      out += '<hr>';
      i++;
    } else if (/^\s*>/.test(l)) {
      const q = [];
      for (; i < lineas.length && /^\s*>/.test(lineas[i]); i++) q.push(lineas[i].replace(/^\s*> ?/, ''));
      out += `<blockquote>${md(q.join('\n'), ctx)}</blockquote>`;
    } else if (l.includes('|') && i + 1 < lineas.length && SEP.test(lineas[i + 1])) {
      const cab = celdas(l);
      i += 2;
      let cuerpo = '';
      for (; i < lineas.length && lineas[i].trim() && lineas[i].includes('|'); i++) {
        cuerpo += '<tr>' + celdas(lineas[i]).map(c => `<td>${enLinea(c)}</td>`).join('') + '</tr>';
      }
      out += `<table><thead><tr>${cab.map(c => `<th>${enLinea(c)}</th>`).join('')}</tr></thead><tbody>${cuerpo}</tbody></table>`;
    } else if (ITEM.test(l)) {
      // Lista: un nivel de anidamiento por sangría (lo más hondo se aplana).
      const items = [];
      const base = ITEM.exec(l)[1].length;
      while (i < lineas.length) {
        const m = ITEM.exec(lineas[i]);
        if (m) {
          const hijo = m[1].length >= base + 2 && items.length > 0;
          const it = { ol: /^\d/.test(m[2]), txt: m[3] };
          if (!hijo && items.length && it.ol !== items[0].ol) break; // otro tipo de lista: empieza una nueva
          if (hijo) (items.at(-1).hijos ??= []).push(it); else items.push(it);
        } else if (!lineas[i].trim()) {
          // una línea en blanco no corta la lista si sigue otro elemento
          if (!ITEM.test(lineas[i + 1] ?? '')) break;
        } else if (/^\s+\S/.test(lineas[i]) && items.length && !inicio(lineas[i])) {
          const it = items.at(-1).hijos?.at(-1) ?? items.at(-1);
          it.txt += ' ' + lineas[i].trim();
        } else break;
        i++;
      }
      const lista = (xs, cuerpo) => `<${xs[0].ol ? 'ol' : 'ul'}>${xs.map(cuerpo).join('')}</${xs[0].ol ? 'ol' : 'ul'}>`;
      out += lista(items, it => `<li>${enLinea(it.txt)}${it.hijos ? lista(it.hijos, h => `<li>${enLinea(h.txt)}</li>`) : ''}</li>`);
    } else {
      const p = [];
      for (; i < lineas.length && lineas[i].trim() && (p.length === 0 || !inicio(lineas[i])); i++) p.push(lineas[i].trim());
      out += `<p>${enLinea(p.join(' '))}</p>`;
    }
  }
  return out;
}

// --------------------------------------------------------------------- editor
//
// Memoria y Bóveda comparten el editor de dos columnas, árbol y nota. Cada
// una aporta un adaptador (`EDITOR[sec]`) que sabe leer sus datos, pintar su
// árbol y hablar con su API; el estado de cada editor vive en `st.ed[sec]`.

const sucio = e => !!e && e.texto !== e.original;
const EDITOR = {};

// El estado del editor de una sección.
const ed = (sec = st.sec) => (st.ed[sec] ??= {
  sel: null,           // clave de la nota abierta
  modo: 'lectura',     // 'lectura' | 'edicion'
  borrador: null,      // { k, texto, original, version, error, conflicto } de la nota abierta
  plegados: new Set(), // carpetas plegadas en el árbol
  menu: false,         // el menú ⋯ de la nota, abierto
  renombrando: null,   // clave de la nota que se está renombrando
  creando: null,       // 'nota' | 'carpeta': el campo de crear, abierto
  papelera: false,     // la papelera, abierta en lugar de la nota
});

// Descarta el borrador de la nota abierta; con cambios, antes pregunta.
function cierraBorrador(sec = st.sec) {
  const A = EDITOR[sec];
  if (!A) return true;
  const E = ed(sec);
  if (sucio(E.borrador) && !confirm(`Esta ${A.cosa} tiene cambios sin guardar. ¿Descartarlos?`)) return false;
  E.borrador = null;
  return true;
}
const sueltaBorradores = () => cierraBorrador(); // al salir de la sección

// El borrador de la nota abierta, si se está editando. Uno sin cambios sigue
// a la versión del disco.
function asegurarBorrador(E) {
  const k = E.sel, x = k && st.detalle[k];
  if (E.borrador && E.borrador.k !== k) E.borrador = null;
  if (!x || E.modo !== 'edicion') return;
  if (!E.borrador || (!sucio(E.borrador) && E.borrador.version !== x.version && !E.borrador.error))
    E.borrador = { k, texto: x.contenido, original: x.contenido, version: x.version, error: null, conflicto: false };
}

function eligeNota(k) {
  const A = EDITOR[st.sec], E = ed();
  if (!A || k === E.sel) return;
  if (!cierraBorrador()) return;
  E.sel = k;
  E.menu = false;
  E.renombrando = null;
  E.papelera = false;
  A.desplegar?.(E, k);
  guardaEditor();
  pintar();
  return tick();
}

// La nota abierta y las carpetas plegadas se recuerdan en el navegador.
function guardaEditor(sec = st.sec) {
  const E = ed(sec);
  try {
    localStorage.setItem(`ea-${sec}-plegados`, JSON.stringify([...E.plegados]));
    if (E.sel) localStorage.setItem(`ea-${sec}-sel`, E.sel); else localStorage.removeItem(`ea-${sec}-sel`);
  } catch { /* sin almacenamiento */ }
}

function plegar(id) {
  const E = ed();
  if (!E.plegados.delete(id)) E.plegados.add(id);
  guardaEditor();
  pintar();
}

// Nodos del árbol: carpetas { d, etiqueta, titulo, cuenta, hijos } y notas
// { k, nombre, icono, iconoTitulo, titulo }. Con filtro, todo desplegado.
const plegada = (E, n) => !st.q.trim() && E.plegados.has(n.d);

// Las claves de las notas que se ven, en orden (para j/k).
const clavesVisibles = (E, nodos) =>
  nodos.flatMap(n => (n.hijos ? (plegada(E, n) ? [] : clavesVisibles(E, n.hijos)) : [n.k]));

function arbolNotas(A, E, nodos, prof = 0) {
  return nodos.map(n => {
    if (n.hijos) {
      const pl = plegada(E, n);
      return `<div class="ad" data-ad="${esc(n.d)}" role="treeitem" tabindex="0" aria-expanded="${!pl}" title="${esc(n.titulo)}" style="--n:${prof}">` +
        `<i>${pl ? '▸' : '▾'}</i><span class="n">${esc(n.etiqueta)}</span><span class="m">${n.cuenta ?? ''}</span></div>` +
        (pl ? '' : `<div role="group">${arbolNotas(A, E, n.hijos, prof + 1)}</div>`);
    }
    const sel = n.k === E.sel;
    const nombre = E.renombrando === n.k
      ? `<input class="ren" data-ren="${esc(n.k)}" value="${esc(A.nombreEditable(n.k))}" spellcheck="false" aria-label="Nombre nuevo de ${esc(n.nombre)}">`
      : `<span class="n">${esc(n.nombre)}</span>`;
    const punto = sel && sucio(E.borrador) ? '<b class="sin-guardar" title="sin guardar">●</b>' : '';
    return `<div class="af${sel ? ' sel' : ''}" data-af="${esc(n.k)}" role="treeitem" tabindex="0" aria-selected="${sel}" style="--n:${prof}"` +
      ` title="${esc(n.titulo)}"><i title="${esc(n.iconoTitulo)}">${n.icono}</i>${nombre}${punto}</div>`;
  }).join('');
}

function notaEditor(A, E, d) {
  const panel = A.panel?.(d, E);
  if (panel) return panel;
  const k = E.sel;
  if (!k) return `<p class="vacio">Elige una ${A.cosa} del árbol.</p>`;
  const x = st.detalle[k], e = E.borrador?.k === k ? E.borrador : null;
  if (!x && !e) return '<p class="vacio">Cargando…</p>';
  const edita = E.modo === 'edicion' && e;
  const modo = edita ? 'pasar a lectura (Ctrl+E)' : 'pasar a edición (Ctrl+E)';
  const opcion = o =>
    `<button role="menuitem" class="${o.cls ?? ''}" data-a="${o.a}"${o.off ? ` disabled title="${esc(o.off)}"` : ''}>${o.txt}</button>`;
  let h = '<header class="nota-cab">' +
    `<div class="miga">${A.miga(d, k)}` +
    `<span class="sin-guardar"${sucio(e) ? '' : ' hidden'}>● sin guardar</span></div><span class="grow"></span>` +
    `<button class="ico${edita ? ' on' : ''}" data-a="modo-nota" title="${modo}" aria-label="${modo}" aria-pressed="${!!edita}">${edita ? '◉' : '✎'}</button>` +
    `<span class="menu-anc"><button class="ico" data-a="menu-nota" title="Más acciones" aria-label="Más acciones" aria-haspopup="menu" aria-expanded="${E.menu}">⋯</button>` +
    (E.menu ? `<div class="menu" role="menu">${A.menu(d, k, x).map(opcion).join('')}</div>` : '') +
    '</span></header>';
  h += A.meta(d, k, x);
  if (e?.error) h += `<p class="hot">✕ ${esc(e.error)}</p>`;
  else if (e && x && x.version !== e.version) h += `<p class="hot">La ${A.cosa} cambió en disco mientras la editabas: si guardas, se te pedirá recargarla.</p>`;
  if (e) {
    const recargar = e.conflicto || (x && x.version !== e.version);
    h += `<div class="acc"${sucio(e) || recargar ? '' : ' hidden'} data-si-sucio>` +
      btn('guardar-nota', '✓ guardar', `escribir la ${A.cosa} en disco (Ctrl+S)`, {}, 'coin') +
      btn('descartar-nota', '✕ descartar', 'tirar el borrador', {}, 'hot') +
      (recargar ? btn('recargar-nota', '↺ recargar', 'descartar el borrador y leer la versión del disco') : '') + '</div>';
  }
  h += edita
    ? `<textarea class="editor" data-ed="${esc(k)}" spellcheck="false" aria-label="Contenido de ${esc(A.nombre(k))}">\n${esc(e.texto)}</textarea>`
    : `<div class="md">${md(e ? e.texto : x.contenido, A.ctx(d, k))}</div>`;
  return h;
}

// El editor no son filas: el árbol y la nota se pintan por separado para que
// el sondeo no les reinicie el scroll.
function pintaEditor() {
  const A = EDITOR[st.sec], E = ed(), d = st.datos;
  if (E.sel && !A.existe(d, E.sel) && !sucio(E.borrador)) {
    E.sel = null; // la borraron o la movieron fuera
    E.borrador = null;
  }
  asegurarBorrador(E);
  const nodos = A.arbol(d, st.q.trim().toLowerCase());
  st.vis = clavesVisibles(E, nodos);
  if (A.vacio(d)) {
    st.html = '';
    $('#filas').innerHTML = `<p class="vacio">${VACIO[st.sec]}</p>`;
    return;
  }
  if ($('#filas > .mem-vista')?.dataset.sec !== st.sec)
    $('#filas').innerHTML = `<div class="mem-vista" data-sec="${st.sec}"><div class="arbol-col"><div class="arbol-acc"></div>` +
      `<nav class="arbol" role="tree" aria-label="${A.aria}"></nav></div><article class="nota"></article></div>`;
  // st.html vacío pide repintar: se rehace el contenido, no las columnas (ni su scroll).
  if (st.html !== 'ed') $$('#filas .mem-vista [data-h]').forEach(el => { el.dataset.h = ''; });
  st.html = 'ed';
  const pon = (el, html) => { if (el.dataset.h !== html) { el.dataset.h = html; el.innerHTML = html; } };
  pon($('#filas .arbol-acc'), A.barraArbol?.(d, E) ?? '');
  pon($('#filas .arbol'), nodos.length ? arbolNotas(A, E, nodos) : `<p class="vacio">Nada coincide con «${esc(st.q.trim())}».</p>`);
  const nota = $('#filas .nota');
  pon(nota, notaEditor(A, E, d));
  nota.classList.toggle('ed', !!$('textarea', nota));
}

// -------------------------------------------------------------------- memoria

const INDICE = 'MEMORY.md';
const claveMem = (proyecto, fichero) => 'm' + proyecto + '/' + fichero;
// m<proyecto>/<fichero> → [proyecto, fichero]
const partesMem = k => { const i = k.indexOf('/'); return [k.slice(1, i), k.slice(i + 1)]; };

// Etiqueta corta de cada proyecto: el último tramo de su cwd (dos si se repite).
function etiquetasMem(ps) {
  const partes = p => (p.cwd ?? '').split('/').filter(Boolean);
  const e = new Map(ps.map(p => [p.id, partes(p).pop() || p.id]));
  for (const p of ps) {
    if (ps.some(o => o.id !== p.id && e.get(o.id) === e.get(p.id)) && p.cwd) e.set(p.id, partes(p).slice(-2).join('/'));
  }
  return e;
}

EDITOR.memoria = {
  cosa: 'memoria',
  aria: 'Memorias por proyecto',
  async datos() {
    const datos = await ea.memoria();
    if (st.proyecto) {
      const id = st.proyecto.replace(/[^A-Za-z0-9]/g, '-');
      datos.proyectos = datos.proyectos.filter(p => p.cwd === st.proyecto || p.id === id);
    }
    return datos;
  },
  carga: k => ea.memoriaFichero(...partesMem(k)),
  vacio: d => !d.proyectos.length,
  existe: (d, k) => { const [pid, f] = partesMem(k); return d.proyectos.some(p => p.id === pid && p.ficheros.some(x => x.nombre === f)); },
  desplegar: (E, k) => E.plegados.delete(partesMem(k)[0]),
  // Un directorio por proyecto y, dentro, sus memorias; el filtro mira el
  // proyecto y el nombre, título y descripción de cada memoria.
  arbol(d, q) {
    const et = etiquetasMem(d.proyectos);
    return d.proyectos.map(p => {
      const deProy = q && `${et.get(p.id)} ${p.cwd ?? ''} ${p.id}`.toLowerCase().includes(q);
      const fs = p.ficheros.filter(f => !q || deProy || `${f.nombre} ${f.titulo ?? ''} ${f.descripcion ?? ''}`.toLowerCase().includes(q));
      return {
        d: p.id, etiqueta: et.get(p.id), titulo: p.cwd ?? p.id, cuenta: p.ficheros.length,
        hijos: fs.map(f => ({
          k: claveMem(p.id, f.nombre), nombre: f.nombre.replace(/\.md$/, ''), titulo: f.descripcion ?? f.titulo ?? f.nombre,
          icono: f.nombre === INDICE ? '≡' : '◇', iconoTitulo: f.nombre === INDICE ? 'índice de las memorias' : 'memoria',
        })),
      };
    }).filter(g => g.hijos.length);
  },
  nombre: k => partesMem(k)[1],
  nombreEditable: k => partesMem(k)[1].replace(/\.md$/, ''),
  miga(d, k) {
    const [pid, nombre] = partesMem(k);
    const p = d.proyectos.find(x => x.id === pid);
    return `<span class="m" title="${esc(p?.cwd ?? pid)}">${esc(etiquetasMem(d.proyectos).get(pid) ?? pid)}</span><span class="m">/</span><b>${esc(nombre)}</b>`;
  },
  meta(d, k) {
    const [pid, nombre] = partesMem(k);
    const f = d.proyectos.find(x => x.id === pid)?.ficheros.find(x => x.nombre === nombre);
    const meta = [f?.titulo, f?.tipo, f?.descripcion].filter(Boolean).join(' · ');
    return `<p class="m">${f ? f.bytes + ' bytes · modificada hace ' + reloj(f.modificada) : 'ya no está en disco'}${meta ? ' · ' + esc(meta) : ''}</p>`;
  },
  menu(d, k) {
    const off = partesMem(k)[1] === INDICE && 'el índice de Claude Code no se renombra ni se borra';
    return [
      { a: 'renombrar-nota', txt: '✎ Renombrar', off },
      { a: 'copiar-ruta-nota', txt: '⧉ Copiar la ruta' },
      { a: 'mostrar-nota', txt: '▤ Mostrar en el explorador de archivos' },
      { a: 'eliminar-nota', txt: '✕ Eliminar archivo', cls: 'hot', off },
    ];
  },
  // Enlaces entre memorias del mismo proyecto: por nombre de fichero.
  ctx(d, k) {
    const [pid] = partesMem(k);
    const p = d.proyectos.find(x => x.id === pid);
    const clave = f => (p?.ficheros.some(y => y.nombre === f) ? claveMem(pid, f) : null);
    return {
      wiki: destino => clave(destino.trim().replace(/\.md$/, '') + '.md'),
      md: url => { const m = /^(?:\.\/)?([\w.-]+\.md)(?:#.*)?$/.exec(url); return m ? clave(m[1]) : null; },
      roto: 'no hay esa memoria',
    };
  },
  rutaAbs: x => x.ruta,
  guardar: (k, texto, version) => ea.guardarMemoria(...partesMem(k), texto, version),
  mostrar: k => ea.mostrarMemoria(...partesMem(k)),
  confirmaEliminar: (k, aviso) => `¿Eliminar definitivamente ${partesMem(k)[1]}? No se puede deshacer.${aviso}`,
  async eliminar(k, x) {
    await ea.eliminarMemoria(...partesMem(k), x.version);
    return '✓ ' + partesMem(k)[1] + ' eliminada';
  },
  async renombrar(k, nuevo, x) {
    const [pid, nombre] = partesMem(k);
    const n = nuevo.trim().replace(/\.md$/, '') + '.md';
    if (n === '.md' || n === nombre) return null;
    const r = await ea.renombrarMemoria(pid, nombre, n, x.version);
    const m = r.actualizadas.length;
    return {
      k: claveMem(pid, n), detalle: r.memoria,
      aviso: `✓ renombrada a ${n}` + (m ? ` · enlaces al día en ${m} ${m === 1 ? 'memoria' : 'memorias'}` : ''),
    };
  },
};

// --------------------------------------------------------------------- bóveda
//
// La bóveda como un editor tipo Obsidian: las tres regiones son las raíces del
// árbol. El usuario edita notas de cualquier región (la firma de la cabecera
// no cambia), crea notas solo en compartida/ y carpetas en cualquiera, mueve
// dentro de la región y borra a la papelera (se restaura).

const carpetaDe = r => (r.includes('/') ? r.slice(0, r.lastIndexOf('/')) : '');
const conMd = d => (d.endsWith('.md') ? d : d + '.md');
const regionDe = r => r.split('/')[0];

// Une y normaliza (`.` y `..`); null si sube por encima de la raíz.
function unirRuta(base, rel) {
  const p = base.split('/').filter(Boolean);
  for (const x of rel.split('/')) {
    if (!x || x === '.') continue;
    if (x === '..') { if (!p.length) return null; p.pop(); } else p.push(x);
  }
  return p.length ? p.join('/') : null;
}

// Como ea-boveda (enlaces.rs): ruta exacta o que acaba en /destino; si hay
// varias, la de la misma carpeta; si no, ambiguo (null).
function resolverWiki(destino, desde, todas) {
  const d = destino.trim();
  if (!d) return null;
  if (d.startsWith('./') || d.startsWith('../')) {
    const r = unirRuta(carpetaDe(desde), conMd(d));
    return r && todas.includes(r) ? r : null;
  }
  const x = conMd(d.replace(/^\/+/, '')), suf = '/' + x;
  const c = todas.filter(t => t === x || t.endsWith(suf));
  if (c.length === 1) return c[0];
  const aqui = c.filter(t => carpetaDe(t) === carpetaDe(desde));
  return aqui.length === 1 ? aqui[0] : null;
}

// Un enlace markdown: relativo a la carpeta de la nota, o a la raíz con `/`.
function resolverMd(url, desde) {
  if (!url || /^[^/]*:/.test(url)) return null;
  let u;
  try { u = decodeURIComponent(url); } catch { return null; }
  const r = u.startsWith('/') ? unirRuta('', u.slice(1)) : unirRuta(carpetaDe(desde), u);
  return r && r.endsWith('.md') ? r : null;
}

const rutaB = k => k.slice(1);
const nombreB = k => rutaB(k).split('/').pop().replace(/\.md$/, '');

// La carpeta en la que se crea: la de la nota abierta (si vale) o compartida/.
function baseCreacion(E, tipo) {
  const r = E.sel && rutaB(E.sel);
  if (r && (tipo === 'carpeta' || regionDe(r) === 'compartida')) return carpetaDe(r) + '/';
  return 'compartida/';
}

async function creaEnBoveda(valor) {
  const E = ed('boveda');
  const tipo = E.creando;
  E.creando = null;
  document.activeElement?.blur(); // con el campo enfocado, pintar() no repinta
  const v = valor.trim().replace(/\/+$/, '');
  if (!tipo || !v || v + '/' === baseCreacion(E, tipo)) { pintar(); return; }
  try {
    if (tipo === 'nota') {
      if (!cierraBorrador('boveda')) { pintar(); return; }
      const n = await ea.crearNota(v);
      const k = 'b' + n.ruta;
      st.detalle[k] = n;
      Object.assign(E, { sel: k, modo: 'edicion', borrador: null, papelera: false });
      EDITOR.boveda.desplegar(E, k);
      guardaEditor('boveda');
      avisa('✓ creada ' + n.ruta, true);
    } else {
      await ea.crearCarpeta(v);
      EDITOR.boveda.desplegar(E, 'b' + v + '/x');
      E.plegados.delete(v);
      guardaEditor('boveda');
      avisa('✓ carpeta ' + v, true);
    }
  } catch (err) {
    avisa(err.message);
  }
  st.html = '';
  await tick();
}

EDITOR.boveda = {
  cosa: 'nota',
  aria: 'Notas de la bóveda por región',
  async datos() {
    const q = st.q.trim(), E = ed('boveda');
    const [d, busca, papelera] = await Promise.all([
      ea.bovedaArbol(),
      q ? ea.boveda(q).catch(() => null) : null,
      E.papelera ? ea.papelera() : null,
    ]);
    d.rutas = d.notas.map(n => n.ruta);
    d.rutasSet = new Set(d.rutas);
    d.fts = busca ? new Set(busca.notas.map(n => n.ruta)) : null;
    d.papelera = papelera;
    return d;
  },
  carga: k => ea.bovedaFichero(rutaB(k)),
  vacio: () => false, // las regiones siempre están, para poder crear
  existe: (d, k) => d.rutasSet.has(rutaB(k)),
  desplegar(E, k) {
    let c = carpetaDe(rutaB(k));
    while (c) { E.plegados.delete(c); c = carpetaDe(c); }
  },
  // Regiones, carpetas (también vacías) y notas; primero las carpetas. El
  // filtro mira nombre, ruta y título y, con el índice, también el contenido.
  arbol(d, q) {
    const dirs = new Map();
    const dir = ruta => {
      if (dirs.has(ruta)) return dirs.get(ruta);
      const n = { d: ruta, etiqueta: ruta.split('/').pop(), titulo: ruta, hijos: [], cuenta: 0 };
      dirs.set(ruta, n);
      const padre = carpetaDe(ruta);
      if (padre) dir(padre).hijos.push(n);
      return n;
    };
    d.regiones.forEach(dir);
    if (!q) d.carpetas.forEach(dir);
    for (const n of d.notas) {
      if (q && !`${n.ruta} ${n.titulo}`.toLowerCase().includes(q) && !d.fts?.has(n.ruta)) continue;
      const promovida = n.promocion === 'promovida';
      dir(carpetaDe(n.ruta)).hijos.push({
        k: 'b' + n.ruta, nombre: n.ruta.split('/').pop().replace(/\.md$/, ''),
        titulo: n.titulo + (n.autor ? ' · ' + (NOMBRE[n.autor] ?? n.autor) : ''),
        icono: n.region === 'compartida' ? '◆' : promovida ? '◈' : '◇',
        iconoTitulo: n.region + (promovida ? ' · promovida' : ''),
      });
    }
    const ordena = n => {
      n.hijos.sort((a, b) => (!!b.hijos - !!a.hijos) || (a.etiqueta ?? a.nombre).localeCompare(b.etiqueta ?? b.nombre, 'es'));
      n.hijos.forEach(h => h.hijos && ordena(h));
      n.cuenta = n.hijos.reduce((s, h) => s + (h.hijos ? h.cuenta : 1), 0);
    };
    const raices = d.regiones.map(r => dirs.get(r));
    raices.forEach(ordena);
    const poda = ns => ns.filter(n => !n.hijos || n.cuenta > 0).map(n => (n.hijos ? { ...n, hijos: poda(n.hijos) } : n));
    return q ? poda(raices) : raices;
  },
  nombre: k => rutaB(k).split('/').pop(),
  // Lo que se escribe al renombrar: la ruta dentro de la región, sin .md.
  nombreEditable: k => rutaB(k).split('/').slice(1).join('/').replace(/\.md$/, ''),
  miga(d, k) {
    const partes = rutaB(k).split('/');
    const ultima = partes.pop();
    return partes.map(p => `<span class="m">${esc(p)}</span><span class="m">/</span>`).join('') + `<b>${esc(ultima)}</b>`;
  },
  meta(d, k, x) {
    if (!x) return '<p class="m">ya no está en disco</p>';
    const c = x.cabecera;
    const partes = [`${x.bytes} bytes · modificada hace ${reloj(x.modificada)}`];
    if (c) {
      partes.push(
        'de ' + esc(NOMBRE[c.autor] ?? c.autor), esc(c.origen),
        c.promocion === 'promovida' ? 'promovida' : 'sin promover',
        c.tags.length ? esc(c.tags.map(t => '#' + t).join(' ')) : '',
        c.promovida_de ? 'copia de ' + esc(c.promovida_de) : '',
        c.promovida_a ? 'promovida a ' + esc(c.promovida_a) : '');
    } else partes.push(esc(x.aviso_cabecera ?? 'sin cabecera'));
    return `<p class="m">${partes.filter(Boolean).join(' · ')}</p>`;
  },
  menu(d, k, x) {
    const comp = regionDe(rutaB(k)) === 'compartida';
    return [
      { a: 'renombrar-nota', txt: '✎ Renombrar o mover' },
      ...(comp ? [] : [{ a: 'promover-nota', txt: '⇧ Promover a compartida', off: x?.cabecera?.promocion === 'promovida' && 'ya está promovida' }]),
      { a: 'copiar-ruta-nota', txt: '⧉ Copiar la ruta' },
      { a: 'mostrar-nota', txt: '▤ Mostrar en el explorador de archivos' },
      { a: 'eliminar-nota', txt: '✕ Mover a la papelera', cls: 'hot' },
    ];
  },
  ctx(d, k) {
    const desde = rutaB(k);
    return {
      wiki: destino => { const r = resolverWiki(destino, desde, d.rutas); return r ? 'b' + r : null; },
      md: url => { const r = resolverMd(url.split('#')[0], desde); return r && d.rutasSet.has(r) ? 'b' + r : null; },
      roto: 'no hay esa nota (o el enlace es ambiguo)',
    };
  },
  // La barra de la columna del árbol: crear, carpeta y papelera.
  barraArbol(d, E) {
    const n = d.en_papelera;
    let h = '<div class="arbol-bar">' +
      btn('crear-nota', '＋ nota', 'nota nueva (solo en la región compartida)', {}, E.creando === 'nota' ? 'on' : '') +
      btn('carpeta-nota', '＋ carpeta', 'carpeta nueva, en cualquier región', {}, E.creando === 'carpeta' ? 'on' : '') +
      btn('papelera-nota', 'papelera' + (n ? ` (${n})` : ''), 'las notas borradas: se pueden restaurar', {}, E.papelera ? 'on' : '') +
      '</div>';
    if (E.creando) {
      const que = E.creando === 'nota' ? 'Ruta de la nota nueva (compartida/…)' : 'Ruta de la carpeta nueva (región/…)';
      h += `<input class="ren nueva" data-crear="${E.creando}" value="${esc(baseCreacion(E, E.creando))}" spellcheck="false" aria-label="${que}" title="${que}: Enter crea, Esc cancela">`;
    }
    return h;
  },
  // La papelera ocupa la columna de la nota mientras está abierta.
  panel(d, E) {
    if (!E.papelera) return null;
    const l = d.papelera;
    let h = '<header class="nota-cab"><div class="miga"><b>Papelera</b></div><span class="grow"></span>' +
      '<button class="ico" data-a="papelera-nota" title="Cerrar la papelera" aria-label="Cerrar la papelera">✕</button></header>' +
      '<p class="m">Las notas borradas desde aquí. Restaurar una la devuelve a su sitio, si no hay otra con la misma ruta.</p>';
    if (!l) return h + '<p class="vacio">Cargando…</p>';
    if (!l.length) return h + '<p class="vacio">La papelera está vacía.</p>';
    return h + '<div class="pap">' + l.map(p =>
      `<div class="pap-f"><span class="n" title="${esc(p.ruta)}">${esc(p.ruta)}</span><span class="m">${p.bytes} bytes · hace ${reloj(p.borrada)}</span>` +
      btn('restaurar-nota', '↺ restaurar', 'devolver ' + p.ruta + ' a su sitio', { id: p.id }) + '</div>').join('') + '</div>';
  },
  // Acciones propias; null = no es suya, false = ya repintó, true = sondear.
  async accion(a, E, datos) {
    if (a === 'crear-nota' || a === 'carpeta-nota') {
      const tipo = a === 'crear-nota' ? 'nota' : 'carpeta';
      E.creando = E.creando === tipo ? null : tipo;
      pintar();
      const i = $('input[data-crear]');
      if (i) { i.focus(); i.setSelectionRange(i.value.length, i.value.length); }
      return false;
    }
    if (a === 'papelera-nota') {
      E.papelera = !E.papelera;
      return true;
    }
    if (a === 'restaurar-nota') {
      const n = await ea.restaurarNota(datos.id);
      avisa('✓ restaurada en ' + n.ruta, true);
      if (cierraBorrador('boveda')) {
        const k = 'b' + n.ruta;
        st.detalle[k] = n;
        Object.assign(E, { sel: k, papelera: false });
        EDITOR.boveda.desplegar(E, k);
        guardaEditor('boveda');
      }
      return true;
    }
    if (a === 'promover-nota') {
      if (!E.sel) return false;
      const r = rutaB(E.sel);
      if (!confirm(`¿Promover ${r} a la región compartida?\n\nSe copia a compartida/; el original se queda, marcado como promovido.`)) { pintar(); return false; }
      const p = await ea.promover(r);
      avisa(p.texto, true);
      return true;
    }
    return null;
  },
  rutaAbs: x => x.absoluta,
  guardar: (k, texto, version) => ea.guardarNota(rutaB(k), texto, version),
  mostrar: k => ea.mostrarNota(rutaB(k)),
  confirmaEliminar: (k, aviso) => `¿Mover ${rutaB(k)} a la papelera? Se puede restaurar.${aviso}`,
  async eliminar(k, x) {
    await ea.eliminarNota(rutaB(k), x.version);
    return `✓ ${nombreB(k)} en la papelera`;
  },
  async renombrar(k, nuevo, x) {
    const ruta = rutaB(k), region = regionDe(ruta);
    let v = nuevo.trim().replace(/^\/+/, '').replace(/\.md$/, '');
    if (!v) return null;
    const primera = v.split('/')[0];
    if (primera === region) v = v.slice(region.length + 1);
    else if (['modelo-local', 'claude-code', 'compartida'].includes(primera))
      throw new Error('solo se mueve dentro de la misma región (a la compartida se llega con ⋯ → Promover)');
    const n = `${region}/${v}.md`;
    if (n === ruta) return null;
    const r = await ea.renombrarNota(ruta, n, x.version);
    const m = r.actualizadas.length, a = r.ambiguas.length;
    return {
      k: 'b' + r.nota.ruta, detalle: r.nota,
      aviso: `✓ ${carpetaDe(ruta) === carpetaDe(r.nota.ruta) ? 'renombrada' : 'movida'} a ${r.nota.ruta}` +
        (m ? ` · enlaces al día en ${m} ${m === 1 ? 'nota' : 'notas'}` : '') +
        (a ? ` · enlaces ambiguos sin tocar en: ${r.ambiguas.join(', ')}` : ''),
    };
  },
};

function filasSistema(e, plano = false) {
  const out = [];
  e.servicios.forEach(s => {
    const activo = s.activo === 'active';
    const g = s.activo === 'desconocido' ? '◌' : activo || s.activo === 'failed' ? '●' : '○';
    const armado = st.armado && st.armado.n === s.nombre && st.armado.hasta > Date.now();
    const row = `<i class="${s.activo === 'failed' ? 'hot' : activo ? 'ok' : 'n'}" title="${esc(s.activo)}">${g}</i>` +
      `<span class="x">${esc(s.nombre)}</span><span class="${s.activo === 'failed' ? 'hot' : 'm'}">${activo ? 'en marcha' : esc(s.activo)}</span>` +
      `<span class="m">${s.activo_s != null ? unidades(s.activo_s) : ''}</span>` +
      btn('reiniciar', armado ? '↻ confirmar' : '↻ reiniciar', armado ? 'confirmar reinicio' : 'reiniciar (pide confirmar)', { n: s.nombre }, armado ? 'hot' : '');
    out.push(fila('s' + s.nombre, 's', row, s.nombre + ' ' + s.activo));
  });
  const o = e.orquestador;
  out.push(fila('sorq', 's', `<i class="${o.vivo ? 'm' : 'hot'}">♥</i><span class="x">orquestador</span><span class="${o.vivo ? 'm' : 'hot'}">${o.vivo ? 'con latido' : 'sin latido: no se ejecutará nada'}</span><span class="m">${reloj(o.ultimo_latido)}</span><span></span>`, 'orquestador latido', '', plano ? '' : ' g'));
  e.agentes.forEach((a, i) => {
    const row = `<i class="n">◈</i><span class="x">${esc(NOMBRE[a.nombre] ?? a.nombre)}</span><span class="m">${a.ultimo ? esc(a.ultimo.accion) : 'sin actividad'}</span>` +
      `<span class="m" title="${esc(a.ultimo?.accion ?? '')}">${reloj(a.ultimo?.ts)}</span><span></span>`;
    out.push(fila('s' + a.nombre, 's', row, a.nombre + ' ' + (a.ultimo?.accion ?? ''), '', i === 0 && !plano ? ' g' : ''));
  });
  return out;
}

function filasAcciones(hs) {
  return hs.map(h => {
    const c = st.consola[h.nombre] || {};
    const row = `<i class="${h.riesgo === 'irreversible' ? 'hot' : 'm'}" title="${h.riesgo}">${RIESGO[h.riesgo]}</i>` +
      `<span class="x">${esc(h.nombre)}</span><span class="m">${esc(h.resumen)}</span><span class="m">${h.riesgo}</span>`;
    let det;
    if (!h.supervisada) {
      det = '<p class="m">Herramienta de los agentes: se ejecuta al invocarla, sin confirmación. No se lanza desde aquí.</p>';
    } else {
      const v = c.v || {};
      const attrs = f => `data-campo="${esc(f.nombre)}" data-h="${esc(h.nombre)}" title="${esc(f.ayuda)}" aria-label="${esc(f.ayuda)}"`;
      det = '<div class="fm">' + h.campos.map(f => {
        const val = v[f.nombre] ?? '';
        if (f.opciones.length) {
          return `<select ${attrs(f)}><option value="">${esc(f.nombre)}</option>` +
            f.opciones.map(o => `<option${o === val ? ' selected' : ''}>${esc(o)}</option>`).join('') + '</select>';
        }
        return `<input ${attrs(f)} placeholder="${esc(f.ayuda)}" value="${esc(val)}"${f.entero ? ' inputmode="numeric" size="8"' : ''}>`;
      }).join('') + btn('simular', '◌ simular', 'describe lo que haría, sin hacer nada', { h: h.nombre }) + '</div>';
      // Lo que sale de simular o ejecutar (.sal) caduca en cuanto cambia un argumento.
      if (c.sim) {
        det += `<p class="tx sal">Simulación: ${esc(c.sim.texto)}</p>` + (c.sim.confirmable
          ? `<div class="acc sal">${btn('ejecutar', '▶ ejecutar', 'ejecutar lo simulado', { h: h.nombre }, 'hot')}</div>`
          : `<p class="hot sal">Irreversible: solo desde la terminal, con <b>ea acciones ejecutar ${esc(h.nombre)} --args '${esc(JSON.stringify(c.sim.argumentos))}'</b>.</p>`);
      }
      if (c.res) {
        det += `<p class="tx sal">✓ ${esc(c.res.texto)}` +
          '' + '</p>';
      }
      if (c.err) det += `<p class="hot sal">✕ ${esc(c.err)}</p>`;
    }
    return fila('a' + h.nombre, 'a', row, `${h.nombre} ${h.resumen} ${h.riesgo}`, det, h.supervisada ? '' : ' fin');
  });
}
// ------------------------------------------------------------------- ajustes

const ORIGEN_AJ = { defecto: () => 'por defecto', ajustes: () => 'ajustes.toml', heredado: o => 'heredado: ' + o.fichero, entorno: o => 'entorno: ' + o.variable };
const origenAj = o => (ORIGEN_AJ[o.tipo] ?? (() => o.tipo))(o);
// Un valor para leerlo y tal como se escribiría en la terminal.
const valorAj = v => (Array.isArray(v) ? (v.length ? v.join(', ') : '(ninguna)') : typeof v === 'boolean' ? (v ? 'sí' : 'no') : String(v ?? ''));
const crudoAj = v => (Array.isArray(v) ? v.join(',') : String(v ?? ''));
const armadoAj = n => !!(st.armado && st.armado.n === n && st.armado.hasta > Date.now());

function filaAjuste(c) {
  const fijada = !!c.fijada_por;
  const web = c.edicion === 'web' && !fijada;
  const [glifo, tit] = fijada ? ['⊘', `la fija la variable ${c.fijada_por}`] : web ? ['◐', 'se cambia desde aquí'] : ['⌨', 'solo desde una terminal (ea ajustes)'];
  const reinicia = c.aplica === 'reinicio';
  const row = `<i class="${fijada ? 'n' : 'm'}" title="${esc(tit)}">${glifo}</i><span class="x" title="${esc(c.clave)}">${esc(c.titulo)}</span>` +
    `<span class="${c.origen.tipo === 'defecto' ? 'm' : 'x'}" title="${esc(valorAj(c.valor))}">${esc(valorAj(c.valor))}</span>` +
    `<span class="m">${esc(origenAj(c.origen))}</span>` +
    `<span class="${reinicia ? 'hot' : 'm'}" title="${reinicia ? esc('reiniciar ' + c.reinicio.join(', ')) : 'sin reiniciar nada'}">${reinicia ? 'requiere reinicio' : 'en caliente'}</span>`;
  const rango = c.min != null ? ` · de ${c.min} a ${c.max}` : c.permitidos ? ` · solo ${c.permitidos.join(', ') || 'nada'}` : '';
  let det = `<p class="tx">${esc(c.descripcion)}</p>` +
    `<p class="m">${esc(c.clave)} · por defecto ${esc(valorAj(c.defecto))}${esc(rango)}${reinicia ? esc(' · al cambiarlo, reinicia ' + c.reinicio.join(', ')) : ''}</p>`;
  if (fijada) {
    det += `<p class="hot">La fija la variable ${esc(c.fijada_por)}: no se cambia desde ningún sitio mientras esté definida.</p>`;
  } else if (web) {
    const actual = st.ajBorrador[c.clave] ?? crudoAj(c.valor);
    const attrs = `data-aj="${esc(c.clave)}" title="${esc(c.titulo)}" aria-label="${esc(c.titulo)}"`;
    const control = c.tipo === 'booleano'
      ? `<select ${attrs}>${[['true', 'sí'], ['false', 'no']].map(([v, t]) => `<option value="${v}"${v === actual ? ' selected' : ''}>${t}</option>`).join('')}</select>`
      : `<input ${attrs} value="${esc(actual)}"${c.tipo === 'entero' ? ' inputmode="numeric" size="8"' : ''} spellcheck="false">`;
    const g = armadoAj('g' + c.clave), d = armadoAj('d' + c.clave);
    det += '<div class="fm">' + control +
      btn('ajuste-guardar', g ? '✓ confirmar' : '✓ guardar', g ? 'confirmar el cambio' : 'guardar en ajustes.toml (pide confirmar)', { c: c.clave }, g ? 'hot' : '') +
      (c.origen.tipo === 'ajustes'
        ? btn('ajuste-defecto', d ? '↺ confirmar' : '↺ por defecto', d ? 'confirmar: quitarlo de ajustes.toml' : 'quitarlo de ajustes.toml: vuelve al heredado o al valor por defecto (pide confirmar)', { c: c.clave }, d ? 'hot' : '')
        : '') + '</div>';
    if (c.endurecible) det += `<p class="m">Desde aquí solo puede bajar; para subirlo: <b>ea ajustes establecer ${esc(c.clave)}=…</b> en una terminal.</p>`;
  } else {
    det += `<p class="m">Solo desde una terminal: <b>ea ajustes establecer ${esc(c.clave)}=${esc(crudoAj(c.valor))}</b>` +
      (c.origen.tipo === 'ajustes' ? ` (o <b>ea ajustes establecer --quitar ${esc(c.clave)}</b> para volver al heredado o al valor por defecto)` : '') + '.</p>';
  }
  return fila('g' + c.clave, 'aj', row, `${c.clave} ${c.titulo} ${valorAj(c.valor)} ${origenAj(c.origen)}`, det);
}

function filasAjustes(d) {
  const out = [suelto(`<p class="aj-cab m">${esc(d.fichero)}${d.existe ? '' : ' · aún no existe: todo sale de otras capas'}</p>` +
    d.avisos.map(a => `<p class="aj-cab hot">! ${esc(a)}</p>`).join(''), true)];
  out.push(columnas('aj', '', 'ajuste', 'valor', 'origen', 'aplica'));
  d.secciones.forEach(s => {
    if (!s.claves.length) return;
    out.push(grupo('⚙', esc(s.titulo), 0), ...s.claves.map(filaAjuste));
  });
  return out;
}

// Lo que la página toma de los ajustes para sí misma.
function aplicaCliente(cl) {
  if (!cl) return;
  const antes = st.sondeoMs;
  st.sondeoMs = cl.sondeo_ms;
  st.diarioLimite = cl.diario_limite;
  if (st.relojTick && antes !== st.sondeoMs) programaSondeo();
}

function programaSondeo() {
  clearInterval(st.relojTick);
  st.relojTick = setInterval(tick, st.sondeoMs ?? 3000);
}

// Guarda (o quita, con `defecto`) un ajuste con la versión que se leyó.
async function guardaAjuste(clave, defecto) {
  const d = st.datos;
  const c = d?.secciones?.flatMap(s => s.claves).find(x => x.clave === clave);
  if (!c) throw new Error('ajuste desconocido');
  let valor = null;
  if (!defecto) {
    const crudo = ($(`[data-aj="${CSS.escape(clave)}"]`)?.value ?? st.ajBorrador[clave] ?? crudoAj(c.valor)).trim();
    if (c.tipo === 'booleano') valor = crudo === 'true';
    else if (c.tipo === 'entero') {
      if (!/^\d+$/.test(crudo)) throw new Error(`'${c.titulo}' debe ser un número`);
      valor = Number(crudo);
    } else if (c.tipo === 'lista') valor = crudo.split(',').map(x => x.trim()).filter(Boolean);
    else valor = crudo;
  }
  const r = await ea.cambiarAjustes(d.version, { [clave]: valor });
  delete st.ajBorrador[clave];
  st.datos = r.ajustes;
  aplicaCliente(r.ajustes.cliente);
  for (const ap of r.aplicados) {
    avisa(`✓ ${ap.clave}: ${valorAj(ap.antes)} → ${valorAj(ap.despues)}` + (ap.reinicio.length ? ` · requiere reiniciar ${ap.reinicio.join(', ')}` : ''), true);
  }
}

const suelto = (html, fijo = false) => ({ k: null, txt: '', html, fijo });
const grupo = (glifo, titulo, n, ayuda = '') =>
  suelto(`<h2 class="gr"><i>${glifo}</i>${titulo}${n ? `<b>${n}</b>` : ''}${ayuda ? `<span>${ayuda}</span>` : ''}</h2>`);
// Los nombres de las columnas de una lista.
const columnas = (cls, ...nombres) =>
  suelto(`<div class="r hd ${cls}">${nombres.map(n => `<span>${n}</span>`).join('')}</div>`, true);

const barraViva = '<div class="bar" role="progressbar" aria-label="en marcha"><i></i></div>';

// ------------------------------------------------------------------- portada

// Una columna de la portada: la máquina de un agente, con su placa y la
// pegatina COLA, o el semáforo del worktree.
function cajaCola(placa, cls, rol, entradas) {
  return `<div class="actor"><h3><span class="placa ${cls}">${placa}</span><span class="grow"></span>` +
    `<span class="pegatina">COLA <b>${entradas.length}</b></span></h3><p class="rol">${rol}</p>` +
    (entradas.length ? entradas.map(e => {
      const t = st.trabajos.find(x => x.id === e.trabajo);
      return `<p class="que" data-k="j${esc(e.trabajo)}" title="pulsa para ver la ficha">${capsula(COLOR[t?.modo] ?? '#8f949c')}<span class="n">${esc(e.titulo ?? e.trabajo)}</span></p>` +
        `<p class="pie">${esc(e.motivo)} · hace ${reloj(e.encolado_en)}</p>`;
    }).join('') : '<p class="que">vacía</p>') + '</div>';
}

function cajaSemaforo(sem) {
  const dueños = sem.length ? sem.map(o => {
    const t = st.trabajos.find(x => x.id === o.trabajo);
    return `<p class="que" data-k="j${esc(o.trabajo)}" title="pulsa para ver la ficha">${capsula(COLOR[t?.modo] ?? '#8f949c', 'run')}<span class="n">${esc(NOMBRE[o.agente] ?? o.agente)} · ${esc(t?.titulo ?? o.trabajo)}</span></p>` +
      `<p class="pie">desde hace ${reloj(o.desde)} · pid ${o.pid}</p>${barraViva}`;
  }).join('') : '<p class="que">libre</p>';
  const luz = sem.length ? 'rojo: ocupado' : 'verde: libre';
  return `<div class="actor${sem.length ? ' run' : ''}"><h3><span class="placa wt">WORKTREE</span><span class="grow"></span>` +
    `<span class="semm ${sem.length ? 'ocupado' : 'libre'}" role="img" aria-label="semáforo en ${luz}"><span class="rojo"></span><span class="ambar"></span><span class="verde"></span></span></h3>` +
    `<p class="rol">quien lo tiene escribe; el otro espera</p>${dueños}</div>`;
}

function tarjetaJ(t) {
  const k = 'j' + t.id;
  const run = t.estado === 'ejecutando';
  const [res, resCls] = premio(t, true);
  return `<article class="card ${t.estado}${st.abiertos.has(k) ? ' o' : ''}${st.sel === k ? ' s' : ''}" data-k="${k}" tabindex="0" title="pulsa para ver la ficha">` +
    `<header><span>${capsulaJ(t, 16)}${MODO[t.modo] ?? t.modo}</span><span>${reloj(t.actualizado)}</span></header>` +
    `<p class="tit">${esc(t.titulo)}</p>` + (run ? barraViva : '') +
    (res ? `<p class="res ${resCls}">${esc(res)}</p>` : '') + '</article>';
}

// El recorrido de un trabajo, de izquierda a derecha.
function tableroJ() {
  const por = e => st.trabajos.filter(t => t.estado === e);
  const cerrados = st.trabajos.filter(t => FINAL.includes(t.estado)).slice(0, 6);
  const col = (titulo, ayuda, ts) =>
    `<section class="col"><h3>${titulo}<b>${ts.length || ''}</b></h3><p class="ay">${ayuda}</p>` +
    (ts.length ? ts.map(tarjetaJ).join('') : '<p class="nada">nada</p>') + '</section>';
  return suelto('<div class="tablero t5">' +
    col('○ En cola', 'esperan al modelo local', por('en_cola')) +
    col('◐ Ejecutando', 'el modelo local trabaja', por('ejecutando')) +
    col('◇ En revisión', 'esperan a Claude Code', por('en_revision')) +
    col('! En espera', 'esperan tu confirmación', por('en_espera')) +
    col('● Cerrados', 'los últimos, con su premio', cerrados) + '</div>');
}

// Las sesiones vivas de Claude Code (del proyecto elegido, o todas), cada una
// con sus subagentes: los que siguen en marcha y los últimos terminados.
function tableroCC(ss) {
  const sub = a => `<p class="sa${a.hasta ? ' fin' : ''}">${capsula(CLAUDE, a.hasta ? '' : 'run', 12, true)}` +
    `<span class="n">${esc(a.descripcion)}</span></p>` +
    `<p class="pie">${esc([a.tipo, a.modelo].filter(Boolean).join(' · '))} · ${a.hasta ? 'acabó hace ' + reloj(a.hasta) : 'desde hace ' + reloj(a.desde)}</p>` +
    (a.hasta ? '' : barraViva);
  const tarjeta = s => `<article class="card sesion${s.ocupada ? ' ejecutando' : ''}">` +
    `<header><span><i${s.ocupada ? ` style="color:${CLAUDE}"` : ''}>${s.ocupada ? '●' : '○'}</i>${s.ocupada ? 'trabajando' : 'libre'}</span><span>${reloj(s.actualizada)}</span></header>` +
    `<p class="tit">${esc(s.nombre)}</p><p class="res">${esc(s.cwd.split('/').filter(Boolean).pop() ?? s.cwd)}</p>` +
    (s.ocupada ? barraViva : '') + s.subagentes.map(sub).join('') + '</article>';
  return suelto(`<div class="tablero cc">${ss.map(tarjeta).join('')}</div>`);
}

// Aviso destacado de un paquete de corrección: añadir contexto y confirmar.
function avisoPaquete(t) {
  const motivo = (t.detalle ?? 'La rutina falló').replace(/[.\s]*$/, '.');
  return suelto(`<div class="paq"><h3>${capsulaJ(t)}${esc(t.rutina ? t.rutina.slug : t.titulo)} <span class="m">${t.rutina ? 'v' + t.rutina.version + ' ' : ''}falló · hace ${reloj(t.actualizado)}</span></h3>` +
    `<p class="m">${esc(motivo)} Revisa el paquete en su ficha, añade contexto si hace falta y confirma: solo entonces pasa a Claude Code.</p>` +
    accionesTrabajo(t, null, btn('ir-trabajo', '▸ ver paquete', 'abrir la ficha', { id: t.id })) + '</div>');
}

// La portada: colas y semáforo, lo que espera una decisión, el tablero de
// trabajos (con la ficha de los abiertos debajo).
function filasInicio(d) {
  const out = [
    grupo('⇄', 'Colas', 0, 'modelo local → Claude Code'),
    suelto('<div class="actores">' +
      cajaCola('MODELO LOCAL', 'ml', 'cápsulas que esperan para ejecutarse', d.colas['modelo-local']) +
      cajaCola('CLAUDE CODE', 'cc', 'cápsulas que esperan a que alguien las abra', d.colas['claude-code']) +
      cajaSemaforo(d.semaforos) + '</div>'),
  ];
  const paq = st.trabajos.filter(t => t.estado === 'en_espera' && t.modo === 'rutina');
  if (paq.length) out.push(grupo('!', 'Cápsulas atascadas', paq.length, 'una rutina falló; nada pasa a Claude Code sin tu confirmación'), ...paq.map(avisoPaquete));
  if (d.propuestas.length) {
    out.push(grupo('◇', 'Esperan tu decisión', d.propuestas.length, 'acepta o rechaza; aceptar ejecuta la acción'), ...filasPropuestas(d.propuestas));
  }
  const e = st.estado;
  if (e) {
    const mal = new Set(e.servicios.filter(s => s.activo !== 'active' && s.activo !== 'desconocido').map(s => 's' + s.nombre));
    if (mal.size) out.push(grupo('▦', 'Servicios parados'), ...filasSistema(e, true).filter(f => mal.has(f.k)));
  }
  const ss = st.sesiones.filter(s => !st.proyecto || s.cwd === st.proyecto);
  if (ss.length) out.push(grupo('◆', 'Claude Code', ss.length, ss.length === 1 ? 'sesión abierta y sus subagentes' : 'sesiones abiertas y sus subagentes'), tableroCC(ss));
  out.push(grupo('▤', 'Trabajos', st.trabajos.length, 'las cápsulas pasan de izquierda a derecha'), tableroJ());
  const abiertos = st.trabajos.filter(t => st.abiertos.has('j' + t.id));
  if (abiertos.length) out.push(grupo('▸', 'Ficha', 0, 'pulsa la fila para cerrarla'), ...filasTrabajos(abiertos));
  return out;
}

function construir() {
  const d = st.datos;
  const con = (cab, filas) => (filas.length ? [cab, ...filas] : []);
  switch (st.sec) {
    case 'inicio': return d ? filasInicio(d) : [];
    case 'acciones': return d ? con(columnas('a', '', 'herramienta', 'qué hace', 'riesgo'), filasAcciones(d)) : [];
    case 'trabajos': return d ? con(columnas('j', 'estado', 'id', 'trabajo', 'modo', 'premio · avance', 'hace'),
      filasTrabajos(st.filtroEstado ? d.filter(t => t.estado === st.filtroEstado) : d)) : [];
    case 'diario': return d ? con(columnas('d', 'hora', '', 'quién', 'qué hizo', 'trabajo', 'duró'), filasDiario(d)) : [];
    case 'rutinas': return d ? con(columnas('u', '', 'rutina', 'versión', 'qué hace', 'próxima', ''), filasRutinas(d)) : [];
    case 'propuestas': return d ? con(columnas('p', '', 'nº', 'qué propone', 'con qué', 'hace', 'tu decisión'), filasPropuestas(d)) : [];
    case 'ajustes': return d ? filasAjustes(d) : [];
    default: return st.estado ? [columnas('s', '', 'servicio o agente', 'qué hace', 'desde', ''), ...filasSistema(st.estado)] : [];
  }
}

// -------------------------------------------------------------------- pintado

// La cápsula de cada filtro de estado: el color solo la distingue de sus vecinas.
const FILTRO = { en_cola: COLOR.mision, ejecutando: COLOR.obra, en_revision: COLOR.obra, en_espera: COLOR.rutina, cerrado: COLOR.obra, cancelado: COLOR.mision };

function barra() {
  const fb = $('#fb');
  let h = '';
  if (st.sec === 'trabajos') {
    h = Object.keys(EST).map(e =>
      `<button class="c${st.filtroEstado === e ? ' on' : ''}" data-f="${e}" title="${EST[e][1]}" aria-pressed="${st.filtroEstado === e}">` +
      `${capsula(FILTRO[e], IC[e] ?? '', 16)}${EST[e][1]} ${st.trabajos.filter(t => t.estado === e).length}</button>`).join('');
  } else if (st.sec === 'diario') {
    h = `<button class="c${st.latidos ? ' on' : ''}" data-a="latidos" title="mostrar u ocultar los latidos del orquestador" aria-pressed="${st.latidos}">${st.latidos ? '♥ con latidos' : '♡ sin latidos'}</button>`;
  } else if (st.sec === 'propuestas') {
    h = `<button class="c${st.propTodas ? '' : ' on'}" data-a="prop" data-v="0" title="solo las que esperan tu decisión" aria-pressed="${!st.propTodas}">◇ pendientes</button>` +
        `<button class="c${st.propTodas ? ' on' : ''}" data-a="prop" data-v="1" title="también las ya decididas" aria-pressed="${st.propTodas}">∗ todas</button>`;
  }
  if (fb.dataset.h !== h) { fb.dataset.h = h; fb.innerHTML = h; }
}

function cabecera() {
  const e = st.estado;
  if (!e) return;
  $('#servicios').innerHTML = e.servicios.map(s => {
    const cls = s.activo === 'active' ? 'ok' : s.activo === 'failed' ? 'bad' : 'off';
    const g = s.activo === 'desconocido' ? '◌' : s.activo === 'inactive' ? '○' : '●';
    return `<i class="dot ${cls}" title="${s.nombre} · ${s.activo}">${g}</i>`;
  }).join('');
  const caidos = e.servicios.filter(s => s.activo !== 'active' && s.activo !== 'desconocido').map(s => s.nombre);
  const salud = $('#salud');
  salud.textContent = caidos.length ? 'parado: ' + caidos.join(', ')
    : !e.orquestador.vivo ? 'orquestador sin latido'
      : e.servicios.every(s => s.activo === 'active') ? 'todo en marcha' : '';
  salud.className = caidos.length || !e.orquestador.vivo ? 'hot' : '';
  const o = $('#orq');
  o.classList.toggle('hot', !e.orquestador.vivo);
  const latido = e.orquestador.ultimo_latido ? 'orquestador: latido hace ' + hace(e.orquestador.ultimo_latido) : 'orquestador sin latido';
  $('title', o).textContent = latido;
  o.setAttribute('aria-label', latido);

  // Trabajos que esperan a alguien: revisión de Claude Code o tu confirmación.
  const atentos = st.trabajos.filter(t => t.estado === 'en_revision' || t.estado === 'en_espera').length;
  const esperan = st.trabajos.filter(t => t.estado === 'en_espera').length;
  const bt = $('[data-s=trabajos] b'), bp = $('[data-s=propuestas] b');
  bt.textContent = atentos || '';
  bt.className = esperan ? 'hot' : '';
  bp.textContent = e.propuestas_pendientes || '';
  bp.className = '';
  document.title = esperan ? `! ${esperan}` : e.propuestas_pendientes ? `◇ ${e.propuestas_pendientes}` : 'ea';
}

// Barra de proyectos: un proyecto por cada cwd con sesiones vivas de Claude Code,
// con sus sesiones y los trabajos activos del bus cuyo worktree cuelga de él.
function barraProyectos() {
  const grupos = new Map();
  for (const s of st.sesiones) {
    if (!grupos.has(s.cwd)) grupos.set(s.cwd, []);
    grupos.get(s.cwd).push(s);
  }
  const partes = c => c.split('/').filter(Boolean);
  const etiqueta = new Map([...grupos.keys()].map(c => [c, partes(c).pop() || c]));
  for (const c of grupos.keys()) {
    const igual = [...grupos.keys()].some(o => o !== c && etiqueta.get(o) === etiqueta.get(c));
    if (igual) etiqueta.set(c, partes(c).slice(-2).join('/'));
  }
  if (st.proyecto && !grupos.has(st.proyecto)) elige(null); // se cerraron sus sesiones
  const trabaja = ss => ss.some(s => s.ocupada);
  const lista = [...grupos.entries()].sort(([, a], [, b]) =>
    trabaja(b) - trabaja(a) || Math.min(...a.map(s => s.desde)) - Math.min(...b.map(s => s.desde)));

  // Los proyectos que aparecen después de la primera carga destacan un momento.
  const claves = lista.map(([c]) => c);
  if (st.proyectos) claves.filter(c => !st.proyectos.includes(c)).forEach(c => { st.nuevos[c] = Date.now() + 3000; });
  st.proyectos = claves;

  const html = lista.map(([cwd, ss]) => {
    const ts = st.todos.filter(t => ACTIVOS.includes(t.estado) && deProyecto(t, cwd));
    const hot = ts.some(t => t.estado === 'en_espera');
    const ocupada = trabaja(ss);
    const resumen = `${ss.length} ${ss.length === 1 ? 'sesión' : 'sesiones'}` + (ts.length ? ` · ${ts.length} ${ts.length === 1 ? 'trabajo' : 'trabajos'}` : '');
    const filas = ss.map(s =>
      `<div class="ps"><i${s.ocupada ? ` style="color:${CLAUDE}"` : ''}>${s.ocupada ? '●' : '○'}</i>` +
      `<span class="n">${esc(s.nombre)}</span><span class="m">${s.ocupada ? 'trabajando' : 'libre'} · ${reloj(s.actualizada)}</span></div>`).join('') +
      ts.map(t => {
        const fase = t.modo === 'obra' && t.fases_total ? ` ${t.fase_actual}/${t.fases_total}` : '';
        return `<div class="pt${t.estado === 'en_espera' ? ' hot' : ''}" data-pt="${esc(t.id)}" role="button" tabindex="0" title="${esc(t.titulo)} · ${EST[t.estado][1]}">` +
          `<i>${EST[t.estado][0]}</i><span class="n">${esc(t.titulo)}</span><span class="m">${fase.trim()}</span></div>`;
      }).join('');
    const titulo = `${cwd} · ${st.proyecto === cwd ? 'pulsa para ver todos los proyectos' : 'pulsa para ver solo este proyecto'}`;
    return `<section class="pry${hot ? ' hot' : ''}${ocupada ? ' trab' : ''}${st.proyecto === cwd ? ' sel' : ''}${st.nuevos[cwd] > Date.now() ? ' nueva' : ''}" data-pry="${esc(cwd)}" title="${esc(titulo)}">` +
      `<h3><i${ocupada ? ` style="color:${CLAUDE}"` : ''}>${ocupada ? '●' : '○'}</i><span class="n">${esc(etiqueta.get(cwd))}</span><span class="m">${resumen}</span></h3>${filas}</section>`;
  }).join('') || '<p class="vacio">Sin sesiones de Claude Code abiertas.</p>';

  const l = $('#proy-lista');
  if (l.dataset.h !== html) { l.dataset.h = html; l.innerHTML = html; }
  relojes();
  const n = lista.length;
  $('#proy-n').textContent = n || '';
  $('#bproy b').textContent = n || '';
  const f = $('#fproy');
  f.hidden = !st.proyecto;
  if (st.proyecto) {
    $('span', f).textContent = etiqueta.get(st.proyecto);
    f.title = `Solo se ve ${st.proyecto}: pulsa para ver todos los proyectos`;
  }
  $('#bproy').classList.toggle('trab', lista.some(([, ss]) => trabaja(ss)));
}

// ¿El trabajo es del proyecto? Su worktree es la carpeta del proyecto o cuelga de ella.
const deProyecto = (t, cwd) => !!t.worktree && (t.worktree === cwd || t.worktree.startsWith(cwd + '/'));

// Los trabajos que se ven: con un proyecto elegido, solo los suyos.
function filtra() {
  st.trabajos = st.proyecto ? st.todos.filter(t => deProyecto(t, st.proyecto)) : st.todos;
}

// Elegir un proyecto (o null para verlos todos) filtra toda la web.
function elige(cwd) {
  st.proyecto = st.proyecto === cwd ? null : cwd;
  filtra();
  st.html = '';
  $('#proy-lista').dataset.h = '';
}

// Desplegada o plegada a una tira con el punto de cada proyecto (en el móvil,
// plegada no se ve); se recuerda en el navegador. Sin elección guardada:
// desplegada en pantallas anchas y plegada en estrechas.
function proyectos(cambiar = false) {
  const a = $('#proyectos');
  let abierta;
  try { const g = localStorage.getItem('ea-proyectos'); if (g) abierta = g === 'abierta'; } catch { /* sin almacenamiento */ }
  if (st.proyAbierta === undefined) st.proyAbierta = abierta ?? matchMedia('(min-width: 1100px)').matches;
  if (cambiar) {
    st.proyAbierta = !st.proyAbierta;
    try { localStorage.setItem('ea-proyectos', st.proyAbierta ? 'abierta' : 'plegada'); } catch { /* sin almacenamiento */ }
  }
  a.classList.toggle('plegada', !st.proyAbierta);
  const titulo = st.proyAbierta ? 'Plegar la barra de proyectos (tecla p)' : 'Desplegar la barra de proyectos: sesiones de Claude Code y sus trabajos (tecla p)';
  const b = $('#bproy');
  b.classList.toggle('abierta', st.proyAbierta);
  b.title = titulo;
  b.setAttribute('aria-expanded', String(st.proyAbierta));
  const p = $('[data-plegar-proy]');
  p.textContent = st.proyAbierta ? '▸' : '◂';
  p.title = titulo;
  p.setAttribute('aria-label', titulo);
}

function pintar() {
  escribeHash(false); // el hash sigue a la vista (filtros, fila abierta)
  // No pisar lo que se está escribiendo dentro de un detalle.
  const foco = document.activeElement;
  if (foco && foco.matches('input, select, textarea') && foco.closest('#lista')) return relojes();

  if (!st.datos && st.sec !== 'sistema') {
    st.vis = [];
    st.html = '';
    $('#filas').innerHTML = '';
    barra();
    return;
  }

  if (EDITOR[st.sec]) {
    pintaEditor();
    relojes();
    barra();
    return;
  }

  const q = st.q.trim().toLowerCase();
  const todas = construir();
  // En la bóveda el filtro es una búsqueda de texto completo en el servidor.
  const vis = q && st.sec !== 'boveda' ? todas.filter(f => f.fijo || f.txt.includes(q)) : todas;
  st.vis = vis.map(f => f.k).filter(Boolean);
  if (st.sel && !st.vis.includes(st.sel)) st.sel = null;

  const vacio = q ? `Nada coincide con «${esc(st.q.trim())}».`
    : st.sec === 'propuestas' && st.propTodas ? 'No hay propuestas.' : VACIO[st.sec] ?? 'Nada que mostrar.';
  const html = st.vis.length || st.sec === 'inicio' ? vis.map(f => f.html).join('') : `<p class="vacio">${vacio}</p>`;
  if (html !== st.html) {
    const lista = $('#lista');
    const y = lista.scrollTop;
    const foco = document.activeElement?.matches('#filas [data-k]') ? document.activeElement.dataset.k : null;
    st.html = html;
    $('#filas').innerHTML = html; // la escena, encima, no se repinta
    lista.scrollTop = y;
    if (foco) $(`#filas [data-k="${CSS.escape(foco)}"]`)?.focus({ preventScroll: true });
  }
  // La fila de un enlace profundo: que se vea en cuanto llega.
  if (st.irA) {
    const f = $(`#filas .f[data-k="${CSS.escape(st.irA)}"]`);
    if (f) { st.irA = null; f.scrollIntoView({ block: 'nearest' }); }
  }
  relojes();
  barra();
}

// ---------------------------------------------------------------------- datos

async function cargaDetalle(k) {
  if (!(DETALLE[st.sec] || '').includes(k[0])) return;
  if (EDITOR[st.sec]) {
    try { st.detalle[k] = await EDITOR[st.sec].carga(k); } catch { delete st.detalle[k]; }
    return;
  }
  const carga = { j: ea.trabajo, r: ea.rutina, b: ea.nota }[k[0]];
  try { st.detalle[k] = await carga(k.slice(1)); } catch { delete st.detalle[k]; }
}

async function cargaSeccion() {
  const sec = st.sec;
  let datos = null;
  if (sec === 'inicio') {
    const [propuestas, colas, semaforos, diario] = await Promise.all([
      ea.propuestas({ pendientes: true }), ea.colas(), ea.semaforos(), ea.diario({ limite: 30 })]);
    datos = { propuestas, colas, semaforos, diario, sesiones: st.proyecto ? st.sesiones.filter(s => s.cwd === st.proyecto) : st.sesiones };
  } else if (sec === 'trabajos') datos = st.trabajos;
  else if (sec === 'acciones') datos = await ea.acciones();
  else if (sec === 'diario') datos = await ea.diario({ limite: st.diarioLimite ?? 300, latidos: st.latidos });
  else if (sec === 'ajustes') {
    datos = await ea.ajustes();
    aplicaCliente(datos.cliente);
  }
  else if (sec === 'rutinas') datos = await ea.rutinas();
  else if (sec === 'propuestas') datos = await ea.propuestas({ pendientes: !st.propTodas });
  else if (EDITOR[sec]) {
    datos = await EDITOR[sec].datos();
    if (ed(sec).sel) await cargaDetalle(ed(sec).sel);
  }
  if (st.proyecto) {
    const ids = new Set(st.trabajos.map(t => t.id));
    const suyo = e => ids.has(trabajoDe(e));
    if (sec === 'diario') datos = datos.filter(suyo);
    else if (sec === 'inicio') datos.diario = datos.diario.filter(suyo);
  }
  await Promise.all([...st.abiertos].map(cargaDetalle));
  if (sec !== st.sec) return false; // el usuario cambió de sección mientras tanto
  st.datos = datos;
  return true;
}

async function tick() {
  if (document.hidden) return;
  if (st.ocupado) { st.otra = true; return; }
  st.ocupado = true;
  try {
    let ses;
    [st.estado, st.todos, ses] = await Promise.all([ea.estado(), ea.trabajos(), ea.sesiones()]);
    st.sesiones = ses.sesiones;
    filtra();
    $('#off').hidden = true;
    barraProyectos();
    cabecera();
    if (await cargaSeccion()) pintar();
    if (st.sec === 'inicio' && st.datos) gasha.actualizar({ trabajos: st.trabajos, estado: st.estado, ...st.datos });
  } catch {
    $('#off').hidden = false;
  } finally {
    st.ocupado = false;
    if (st.otra) { st.otra = false; tick(); }
  }
}

// ------------------------------------------------------------------ acciones

// Lo que cambia con la sección fuera de la lista: título y cuadros.
function marco() {
  const [nombre] = TITULOS[st.sec];
  $('#seccion').textContent = nombre;
  const q = $('#q'), busca = st.sec === 'boveda';
  q.placeholder = (busca ? 'buscar en las notas' : 'filtrar') + (matchMedia('(pointer: coarse)').matches ? '' : ' ( / )');
  q.setAttribute('aria-label', busca ? 'Buscar en las notas' : 'Filtrar la lista');
  $$('#rail button[data-s]').forEach(b => {
    b.classList.toggle('on', b.dataset.s === st.sec);
    if (b.dataset.s === st.sec) b.setAttribute('aria-current', 'page'); else b.removeAttribute('aria-current');
  });
  $('#barra').hidden = st.sec === 'inicio'; // la portada no es una lista que filtrar
  $('#escena').hidden = st.sec !== 'inicio';
  $('#lista').classList.toggle('vista-mem', !!EDITOR[st.sec]);
  for (const E of Object.values(st.ed)) {
    E.menu = false;
    E.renombrando = null;
    E.creando = null;
  }
}

// ------------------------------------------------------- enlaces profundos
//
// El hash dice la sección, la fila desplegada y los filtros (la gramática,
// en enlaces.js): #trabajos/<id>, #diario?latidos=1&q=texto. Al cargar y al
// cambiar el hash (atrás, adelante, un enlace) se aplica; al navegar se
// reescribe: cambiar de sección o abrir una fila crea una entrada del
// historial, lo demás la reemplaza (pintar() lo mantiene al día).

// Prefijo de la clave de las filas que se pueden enlazar, por sección. Bóveda
// y Memoria no llevan fila: su nota abierta la recuerda el navegador.
const FILA = { inicio: 'j', trabajos: 'j', diario: 'e', rutinas: 'r', propuestas: 'p', acciones: 'a', sistema: 's', ajustes: 'g' };

// Los parámetros de cada sección en el hash: nombre -> { lee(valor), escribe() }.
// `lee` recibe el texto del hash (undefined si no está: el valor por
// defecto) y lo deja en el estado; `escribe` da el texto del hash o null.
// Un filtro nuevo se añade aquí y, al cambiarlo, basta con repintar.
// `q` (el cuadro de filtrar) vale en todas las secciones.
const bandera = campo => ({ lee: v => { st[campo] = v === '1'; }, escribe: () => (st[campo] ? '1' : null) });
const texto = (campo, vale = () => true) => ({
  lee: v => { st[campo] = v && vale(v) ? v : null; },
  escribe: () => st[campo] || null,
});
const PARAMS = {
  trabajos: { estado: texto('filtroEstado', v => v in EST) },
  diario: { latidos: bandera('latidos') },
  propuestas: { todas: bandera('propTodas') },
};
const Q = { lee: v => { st.q = v ?? ''; }, escribe: () => st.q || null };
const paramsDe = sec => ({ ...PARAMS[sec], q: Q });

// El hash de la vista actual, sin '#'.
function hashActual() {
  const params = {};
  for (const [n, p] of Object.entries(paramsDe(st.sec))) params[n] = p.escribe();
  const pre = FILA[st.sec];
  const fila = pre && st.enlace?.startsWith(pre) && st.abiertos.has(st.enlace) ? st.enlace.slice(pre.length) : null;
  return enlaces.escribir({ sec: st.sec, fila, params });
}

// Lleva la vista al hash: push crea una entrada del historial.
function escribeHash(push) {
  const h = hashActual();
  if (location.hash.slice(1) === h) return;
  history[push ? 'pushState' : 'replaceState'](null, '', location.pathname + location.search + '#' + h);
}

// Pone en la sección actual los filtros y la fila de un enlace ({ fila, params }).
function aplicaEnlace({ fila, params }) {
  for (const [n, p] of Object.entries(paramsDe(st.sec))) p.lee(params[n]);
  $('#q').value = st.q;
  const pre = FILA[st.sec];
  const k = pre && fila != null ? pre + fila : null;
  if (st.enlace && st.enlace !== k) st.abiertos.delete(st.enlace);
  if (k) { st.abiertos.add(k); st.sel = k; st.irA = k; }
  st.enlace = k;
}

// El hash cambió desde fuera (atrás, adelante, un enlace, la barra de direcciones).
function alCambiarHash(hash) {
  if (hash.replace(/^#/, '') === hashActual()) return;
  const e = enlaces.leer(hash);
  const sec = SECCIONES.includes(e.sec) ? e.sec : st.sec;
  if (sec !== st.sec) { ir(sec, e); return; }
  aplicaEnlace(e);
  escribeHash(false);
  st.html = '';
  pintar();
  tick();
}

// Cambia de sección. Con `enlace` ({ fila, params }), la abre así y reemplaza
// la entrada del historial; sin él, conserva los filtros y crea una.
function ir(sec, enlace = null) {
  if (sec !== st.sec && !sueltaBorradores()) { escribeHash(false); return false; }
  const cambia = sec !== st.sec;
  st.sec = sec;
  st.datos = null;
  st.html = '';
  st.sel = null;
  st.q = '';
  $('#q').value = '';
  st.enlace = null;
  if (enlace) aplicaEnlace(enlace);
  escribeHash(cambia && !enlace);
  marco();
  $('#lista').scrollTop = 0;
  pintar();
  tick();
}

async function alterna(k) {
  // Abrir una fila enlazable crea una entrada del historial (atrás la cierra).
  const enlazable = FILA[st.sec] && k.startsWith(FILA[st.sec]);
  if (st.abiertos.has(k)) {
    st.abiertos.delete(k);
    if (st.enlace === k) st.enlace = null;
    escribeHash(false);
  } else {
    st.abiertos.add(k);
    if (enlazable) { st.enlace = k; escribeHash(true); }
    await cargaDetalle(k);
  }
  pintar();
  // Desde una tarjeta del tablero la ficha se abre más abajo: que se vea.
  if (st.abiertos.has(k)) $(`.f[data-k="${CSS.escape(k)}"]`)?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
}

function mover(d) {
  if (!st.vis.length) return;
  const i = st.vis.indexOf(st.sel);
  st.sel = st.vis[Math.min(st.vis.length - 1, Math.max(0, i < 0 ? (d > 0 ? 0 : st.vis.length - 1) : i + d))];
  const conFoco = document.activeElement?.matches('#filas [data-k]');
  pintar();
  const f = $(`.f[data-k="${CSS.escape(st.sel)}"]`);
  f?.scrollIntoView({ block: 'nearest' });
  if (conFoco) f?.focus({ preventScroll: true });
}

// Argumentos de la consola tal como los espera la herramienta: tipos y
// obligatorios según el catálogo.
function argumentos(h) {
  const def = (Array.isArray(st.datos) ? st.datos : []).find(x => x.nombre === h);
  if (!def) throw new Error('herramienta desconocida');
  const v = st.consola[h]?.v || {};
  const args = {};
  for (const f of def.campos) {
    const crudo = (v[f.nombre] ?? '').trim();
    if (!crudo) {
      if (f.obligatorio) throw new Error(`falta '${f.nombre}'`);
      continue;
    }
    if (f.entero && !/^\d+$/.test(crudo)) throw new Error(`'${f.nombre}' debe ser un número`);
    args[f.nombre] = f.entero ? Number(crudo) : crudo;
  }
  return args;
}

function abre(sec, k) {
  if (ir(sec) === false) return;
  st.abiertos.add(k);
  st.sel = k;
  if (FILA[sec] && k.startsWith(FILA[sec])) { st.enlace = k; st.irA = k; escribeHash(false); }
}


async function accion(b) {
  const { a, id, v, slug, n, h, k: clave } = b.dataset;
  try {
    if (['cerrar', 'cancelar', 'continuar', 'reformular', 'confirmar'].includes(a)) {
      const caja = b.closest('[data-j]');
      const texto = $('.nota', caja)?.value.trim() ?? '';
      if (a === 'reformular' && !texto) throw new Error('escribe la nota para reformular');
      if (a === 'cancelar' && !(st.armado && st.armado.n === 'c' + id && st.armado.hasta > Date.now())) {
        st.armado = { n: 'c' + id, hasta: Date.now() + 3000 };
        b.textContent = '⊘ confirmar';
        setTimeout(() => { st.armado = null; st.html = ''; pintar(); }, 3100);
        return;
      }
      st.armado = null;
      const cuerpo = a === 'reformular' ? { nota: texto } : a === 'confirmar' ? { contexto: texto } : {};
      await ea.accionTrabajo(id, a, cuerpo);
      avisa(`${id.slice(9, 15)}: ${a}`, true);
    } else if (a === 'diff') {
      if (st.diffs[id] !== undefined) delete st.diffs[id];
      else st.diffs[id] = await ea.diff(id).catch(e => ({ error: e.message }));
    } else if (a === 'decidir') {
      const k = 'p' + id;
      if (v === 'aceptar' && b.dataset.ac && !(st.armado && st.armado.n === k && st.armado.hasta > Date.now())) {
        st.armado = { n: k, hasta: Date.now() + 4000 };
        pintar();
        setTimeout(pintar, 4100);
        return;
      }
      st.armado = null;
      const p = await ea.decidir(id, v);
      if (p.ejecutada) avisa(`${p.ejecutada.estado}: ${p.ejecutada.detalle ?? ''}`, p.ejecutada.estado === 'ejecutada');
    } else if (a === 'lanzar') {
      // Con lo escrito en la ficha; los fallos se quedan junto al formulario.
      const re = st.rutEnt[slug];
      let t;
      try {
        t = await ea.lanzarRutina(slug, re?.v ?? {});
      } catch (e) {
        if (!re || e.status === 403) throw e;
        re.previa = null;
        re.err = e.message;
      }
      if (t) {
        delete st.rutEnt[slug];
        avisa(`✓ ${slug}: cápsula ${t.trabajo.slice(9, 15)} en la cola del modelo local`, true);
        st.abiertos.add('j' + t.trabajo);
      }
    } else if (a === 'rprevia') {
      const re = (st.rutEnt[slug] ??= {});
      re.previa = re.err = null;
      try {
        re.previa = await ea.previaRutina(slug, re.v ?? {});
      } catch (e) {
        if (e.status === 403) throw e;
        re.err = e.message;
      }
    } else if (a === 'reiniciar') {
      if (st.armado && st.armado.n === n && st.armado.hasta > Date.now()) {
        st.armado = null;
        await ea.reiniciar(n);
        avisa('↻ ' + n, true);
      } else {
        st.armado = { n, hasta: Date.now() + 3000 };
        pintar();
        setTimeout(pintar, 3100);
        return;
      }
    } else if (a === 'ajuste-guardar' || a === 'ajuste-defecto') {
      // Segundo clic para confirmar, como reiniciar un servicio.
      const n = (a === 'ajuste-guardar' ? 'g' : 'd') + b.dataset.c;
      if (!armadoAj(n)) {
        if (a === 'ajuste-guardar') {
          const el = $(`[data-aj="${CSS.escape(b.dataset.c)}"]`);
          if (el) st.ajBorrador[b.dataset.c] = el.value;
        }
        st.armado = { n, hasta: Date.now() + 3000 };
        st.html = '';
        pintar();
        setTimeout(() => { st.html = ''; pintar(); }, 3100);
        return;
      }
      st.armado = null;
      await guardaAjuste(b.dataset.c, a === 'ajuste-defecto');
    } else if (a === 'ir-trabajo') {
      st.filtroEstado = null;
      abre('trabajos', 'j' + id);
      return;
    } else if (a === 'ir-prop') {
      st.propTodas = true;
      abre('propuestas', 'p' + id);
      return;
    } else if (a === 'simular' || a === 'ejecutar') {
      // Los fallos se quedan junto al formulario, no en un aviso pasajero.
      const c = (st.consola[h] ??= {});
      const sim = c.sim;
      c.sim = c.res = c.err = null;
      try {
        if (a === 'simular') {
          c.sim = await ea.simular(h, argumentos(h));
        } else if (sim) {
          c.res = await ea.ejecutar(h, sim.argumentos, sim.precondicion);
        }
      } catch (e) {
        c.err = e.message;
      }
    } else if (a.endsWith('-nota')) {
      if (await accionNota(a, b.dataset) === false) return;
    } else if (a === 'latidos') {
      st.latidos = !st.latidos;
    } else if (a === 'prop') {
      st.propTodas = v === '1';
    }
  } catch (e) {
    avisa(e.message);
  }
  // La acción da por terminado lo que se escribía: sin soltar el campo, el
  // repintado seguiría en pausa y la lista se quedaría desactualizada.
  if (document.activeElement?.closest('#lista')) document.activeElement.blur();
  st.html = '';
  await tick();
}

// Las acciones del editor. `false`: ya repintó y no hace falta sondear.
async function accionNota(a, datos = {}) {
  const A = EDITOR[st.sec];
  if (!A) return false;
  const E = ed();
  const local = () => { pintar(); return false; };
  if (a !== 'menu-nota') E.menu = false;
  // Las acciones propias del adaptador: null = no es suya, false = ya
  // repintó, true = hecho (se sondea).
  const propia = await A.accion?.(a, E, datos) ?? null;
  if (propia === false) return false;
  if (propia === true) return;
  const k = E.sel;
  if (!k) return false;
  const x = st.detalle[k], e = E.borrador;
  if (a === 'menu-nota') {
    E.menu = !E.menu;
    return local();
  } else if (a === 'modo-nota') {
    E.modo = E.modo === 'edicion' ? 'lectura' : 'edicion';
    if (E.modo === 'lectura' && !sucio(e) && !e?.conflicto) E.borrador = null;
    pintar();
    if (E.modo === 'edicion') $('#filas .nota textarea')?.focus();
    return false;
  } else if (a === 'descartar-nota') {
    cierraBorrador();
  } else if (a === 'recargar-nota') {
    E.borrador = null;
  } else if (a === 'guardar-nota') {
    if (!e) return false;
    try {
      st.detalle[k] = await A.guardar(k, e.texto, e.version);
      E.borrador = null;
      avisa(`✓ ${A.cosa} guardada`, true);
    } catch (err) {
      // El borrador se queda intacto; si el disco cambió, se ofrece recargar.
      e.error = err.message;
      e.conflicto = err.status === 409;
    }
  } else if (a === 'copiar-ruta-nota') {
    if (!x) return local();
    const ruta = A.rutaAbs(x);
    try { await navigator.clipboard.writeText(ruta); } catch {
      const ta = Object.assign(document.createElement('textarea'), { value: ruta });
      document.body.append(ta);
      ta.select();
      document.execCommand('copy');
      ta.remove();
    }
    avisa('✓ ruta copiada: ' + ruta, true);
    return local();
  } else if (a === 'mostrar-nota') {
    await A.mostrar(k);
    avisa('✓ abierta en el explorador de archivos', true);
    return local();
  } else if (a === 'renombrar-nota') {
    if (sucio(e)) throw new Error('guarda o descarta los cambios antes de renombrar');
    E.renombrando = k;
    pintar();
    const i = $(`input[data-ren="${CSS.escape(k)}"]`);
    i?.focus();
    i?.select();
    return false;
  } else if (a === 'eliminar-nota') {
    if (!x) return local();
    const aviso = sucio(e) ? '\n\nLos cambios sin guardar también se perderán.' : '';
    if (!confirm(A.confirmaEliminar(k, aviso))) return local();
    const msg = await A.eliminar(k, x);
    delete st.detalle[k];
    E.sel = null;
    E.borrador = null;
    guardaEditor();
    avisa(msg, true);
  }
}

// Termina el renombrado en línea: con `nuevo` lo pide al servidor.
async function renombra(nuevo) {
  const A = EDITOR[st.sec], E = ed();
  const k = E.renombrando;
  E.renombrando = null;
  if (!k || !A) return;
  document.activeElement?.blur(); // con el campo enfocado, pintar() no repinta
  const x = st.detalle[k];
  if (!x) { pintar(); return; }
  try {
    const r = await A.renombrar(k, nuevo, x);
    if (!r) { pintar(); return; }
    delete st.detalle[k];
    st.detalle[r.k] = r.detalle;
    E.sel = r.k;
    E.borrador = null;
    A.desplegar?.(E, r.k);
    guardaEditor();
    avisa(r.aviso, true);
  } catch (err) {
    avisa(err.message);
  }
  st.html = '';
  await tick();
}

// Modo oscuro (el tema de Omarchy) o claro; se recuerda en el navegador.
const LUNA = '<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" aria-hidden="true"><path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z"/></svg>';
const SOL = '<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M2 12h2M20 12h2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/></svg>';
function tema(cambiar = false) {
  const raiz = document.documentElement;
  if (cambiar) {
    raiz.classList.toggle('claro');
    try { localStorage.setItem('ea-tema', raiz.classList.contains('claro') ? 'claro' : 'oscuro'); } catch { /* sin almacenamiento */ }
  }
  const claro = raiz.classList.contains('claro');
  const b = $('#btema');
  b.innerHTML = claro ? SOL : LUNA;
  b.title = claro ? 'Modo claro (pulsa para oscuro)' : 'Modo oscuro (pulsa para claro)';
  b.setAttribute('aria-label', b.title);
}

function ayuda(mostrar) {
  const a = $('#ayuda');
  const dentro = a.contains(document.activeElement);
  a.hidden = !mostrar;
  if (mostrar) $('[data-cerrar]', a).focus();
  else if (dentro) $('#bayuda').focus();
  try { localStorage.setItem('ea-ayuda-vista', '1'); } catch { /* sin almacenamiento */ }
}

// -------------------------------------------------------------------- eventos

document.addEventListener('click', ev => {
  const t = ev.target;
  const E = EDITOR[st.sec] && ed();
  if (E?.menu && !t.closest('.menu, [data-a="menu-nota"]')) { E.menu = false; pintar(); }
  const rail = t.closest('#rail button[data-s]');
  if (rail) return ir(rail.dataset.s);
  if (t.closest('#bayuda')) return ayuda($('#ayuda').hidden);
  if (t.closest('#btema')) return tema(true);
  if (t.closest('#bproy')) return proyectos(true);
  if (t.closest('[data-plegar-proy]')) return proyectos(true);
  const pt = t.closest('[data-pt]');
  if (pt) return abre('trabajos', 'j' + pt.dataset.pt);
  const pry = t.closest('[data-pry]');
  if (pry) { elige(pry.dataset.pry); barraProyectos(); pintar(); return tick(); }
  if (t.closest('#fproy')) { elige(null); barraProyectos(); pintar(); return tick(); }
  const tb = t.closest('[data-tour]');
  if (tb) return tour.iniciar(tb.dataset.tour === 'vista' ? st.sec : null);
  if (t.closest('#ayuda')) { if (!t.closest('.hoja') || t.closest('[data-cerrar]')) ayuda(false); return; }
  const f = t.closest('button[data-f]');
  if (f) {
    st.filtroEstado = st.filtroEstado === f.dataset.f ? null : f.dataset.f;
    st.html = '';
    return tick();
  }
  const b = t.closest('button[data-a]');
  if (b) return accion(b);
  const enlace = t.closest('a.ml');
  if (enlace) {
    ev.preventDefault();
    if (E) eligeNota(enlace.dataset.ml);
    return;
  }
  if (t.closest('input, select, textarea, .acc, .nota')) return;
  const ad = t.closest('[data-ad]');
  if (ad) return plegar(ad.dataset.ad);
  const af = t.closest('[data-af]');
  if (af) return eligeNota(af.dataset.af);
  const fila = t.closest('#lista [data-k]');
  if (fila) { st.sel = fila.dataset.k; alterna(fila.dataset.k); }
});

// La consola de acciones guarda lo escrito en el estado (la lista se repinta
// entera) y da por caducada la simulación en cuanto cambia un argumento.
$('#lista').addEventListener('input', ev => {
  // Lo escrito en un ajuste sobrevive al repintado del sondeo.
  const aj = ev.target.closest('[data-aj]');
  if (aj) { st.ajBorrador[aj.dataset.aj] = aj.value; return; }
  // El borrador de una nota vive en el estado: el sondeo no lo pierde. No se
  // repinta al teclear; solo se actualiza el indicador «sin guardar».
  const ta = ev.target.closest('textarea[data-ed]');
  if (ta) {
    const e = ed().borrador;
    if (e && e.k === ta.dataset.ed) {
      e.texto = ta.value;
      const nota = ta.closest('.nota');
      $('.sin-guardar', nota)?.toggleAttribute('hidden', !sucio(e));
      if (!e.conflicto) $('[data-si-sucio]', nota)?.toggleAttribute('hidden', !sucio(e));
    }
    return;
  }
  // Las entradas de una rutina: la vista previa deja de valer.
  const ri = ev.target.closest('[data-rent]');
  if (ri) {
    const re = (st.rutEnt[ri.dataset.rent] ??= {});
    (re.v ??= {})[ri.dataset.ent] = ri.value;
    re.previa = re.err = null;
    st.html = '';
    $$('.sal', ri.closest('.det')).forEach(n => n.remove());
    return;
  }
  const el = ev.target.closest('[data-campo]');
  if (!el) return;
  const c = (st.consola[el.dataset.h] ??= {});
  (c.v ??= {})[el.dataset.campo] = el.value;
  c.sim = c.res = c.err = null;
  st.html = '';
  $$('.sal', el.closest('.det')).forEach(n => n.remove());
});

$('#lista').addEventListener('focusout', ev => {
  const E = EDITOR[st.sec] && ed();
  if (ev.target.matches('input[data-ren]') && E?.renombrando) { E.renombrando = null; setTimeout(pintar); }
  // Al salir del campo de crear se cierra, salvo hacia la propia barra.
  if (ev.target.matches('input[data-crear]') && E?.creando && !ev.relatedTarget?.closest?.('.arbol-acc')) { E.creando = null; setTimeout(pintar); }
});

let busqueda;
$('#q').addEventListener('input', ev => {
  st.q = ev.target.value;
  st.html = '';
  pintar();
  if (st.sec === 'boveda') { clearTimeout(busqueda); busqueda = setTimeout(tick, 250); }
});

document.addEventListener('keydown', ev => {
  if (tour.activo()) return;
  const t = ev.target;
  const enCampo = t.matches('input, textarea, select');
  if (ev.key === 'Escape' && !$('#ayuda').hidden) return ayuda(false);
  if (EDITOR[st.sec]) {
    const E = ed();
    const tecla = ev.key.toLowerCase();
    if (t.matches('input[data-ren]')) {
      if (ev.key === 'Enter') { ev.preventDefault(); renombra(t.value); }
      else if (ev.key === 'Escape') { E.renombrando = null; pintar(); }
      return;
    }
    if (t.matches('input[data-crear]')) {
      if (ev.key === 'Enter') { ev.preventDefault(); creaEnBoveda(t.value); }
      else if (ev.key === 'Escape') { E.creando = null; t.blur(); pintar(); }
      return;
    }
    if (ev.key === 'Escape' && E.menu) { E.menu = false; pintar(); $('[data-a="menu-nota"]')?.focus(); return; }
    if ((ev.ctrlKey || ev.metaKey) && (tecla === 's' || tecla === 'e') && E.sel) {
      ev.preventDefault();
      if (tecla === 'e' && enCampo) t.blur(); // con el editor enfocado, pintar() no repinta
      accion({ dataset: { a: tecla === 's' ? 'guardar-nota' : 'modo-nota' } });
      return;
    }
    if ((ev.key === 'Enter' || ev.key === ' ') && t.matches('[data-af]')) { ev.preventDefault(); eligeNota(t.dataset.af); return; }
    if ((ev.key === 'Enter' || ev.key === ' ') && t.matches('[data-ad]')) { ev.preventDefault(); plegar(t.dataset.ad); return; }
    if (!enCampo && !ev.ctrlKey && !ev.metaKey && !ev.altKey && (ev.key === 'j' || ev.key === 'k') && st.vis.length) {
      const i = st.vis.indexOf(E.sel);
      const d = ev.key === 'j' ? 1 : -1;
      const k = st.vis[Math.min(st.vis.length - 1, Math.max(0, i < 0 ? (d > 0 ? 0 : st.vis.length - 1) : i + d))];
      eligeNota(k);
      $(`[data-af="${CSS.escape(k)}"]`)?.scrollIntoView({ block: 'nearest' });
      return;
    }
  }
  if (ev.key === 'Escape') {
    if (t.id === 'q') { t.value = ''; st.q = ''; }
    if (enCampo) t.blur();
    else {
      st.abiertos.clear();
      st.sel = null;
      st.enlace = null;
    }
    st.html = '';
    pintar();
    return;
  }
  if (t.matches('textarea[data-ed]')) {
    if (ev.key === 'Tab' && !ev.shiftKey && !ev.ctrlKey && !ev.altKey && !ev.metaKey) {
      ev.preventDefault();
      t.setRangeText('  ', t.selectionStart, t.selectionEnd, 'end');
      t.dispatchEvent(new Event('input', { bubbles: true }));
      return;
    }
  }
  if (ev.key === 'Enter' && t.matches('input[data-aj]')) {
    ev.preventDefault();
    const b = $(`button[data-a="ajuste-guardar"][data-c="${CSS.escape(t.dataset.aj)}"]`);
    t.blur();
    if (b) accion(b);
    return;
  }
  if (ev.key === 'Enter' && t.matches('input[data-campo]')) {
    ev.preventDefault();
    accion({ dataset: { a: 'simular', h: t.dataset.h } });
    return;
  }
  if (enCampo || ev.ctrlKey || ev.metaKey || ev.altKey) return;
  if ((ev.key === 'Enter' || ev.key === ' ') && t.matches('[data-pt]')) { ev.preventDefault(); abre('trabajos', 'j' + t.dataset.pt); return; }
  if ((ev.key === 'Enter' || ev.key === ' ') && t.matches('#lista [data-k]')) { ev.preventDefault(); st.sel = t.dataset.k; alterna(t.dataset.k); return; }
  if (ev.key === '0') { ir('ajustes'); return; }
  const n = Number(ev.key);
  if (n >= 1 && n <= 9 && n <= SECCIONES.length) { ir(SECCIONES[n - 1]); return; }
  if (ev.key === 'p') proyectos(true);
  else if (ev.key === '?') { ev.preventDefault(); ayuda($('#ayuda').hidden); }
  else if (ev.key === 't') tour.iniciar();
  else if (ev.key === '/') { ev.preventDefault(); $('#q').focus(); }
  else if (ev.key === 'j') mover(1);
  else if (ev.key === 'k') mover(-1);
  else if ((ev.key === 'Enter' || ev.key === ' ') && st.sel && !t.closest('button')) { ev.preventDefault(); alterna(st.sel); }
});

document.addEventListener('visibilitychange', tick);
addEventListener('beforeunload', ev => {
  if (Object.values(st.ed).some(E => sucio(E.borrador))) { ev.preventDefault(); ev.returnValue = ''; }
});
// popstate llega antes que un repintado pudiera pisar el hash nuevo; hashchange
// trae la URL nueva en el propio evento. Aplicar dos veces no hace nada.
addEventListener('popstate', () => alCambiarHash(location.hash));
addEventListener('hashchange', ev => alCambiarHash(new URL(ev.newURL).hash));

// -------------------------------------------------------------------- arranque

{
  const enlace = enlaces.leer(location.hash);
  if (SECCIONES.includes(enlace.sec)) st.sec = enlace.sec;
  try {
    // Las claves antiguas de la memoria (ea-mem-*) pasan a ea-memoria-*.
    for (const [viejo, nuevo] of [['ea-mem-plegados', 'ea-memoria-plegados'], ['ea-mem-sel', 'ea-memoria-sel']]) {
      const v = localStorage.getItem(viejo);
      if (v !== null && localStorage.getItem(nuevo) === null) localStorage.setItem(nuevo, v);
      localStorage.removeItem(viejo);
    }
    for (const sec of Object.keys(EDITOR)) {
      const E = ed(sec);
      E.plegados = new Set(JSON.parse(localStorage.getItem(`ea-${sec}-plegados`) ?? '[]'));
      E.sel = localStorage.getItem(`ea-${sec}-sel`);
    }
  } catch { /* sin almacenamiento */ }
  aplicaEnlace(enlace); // la fila y los filtros del hash (de otra sección, se ignoran por defecto)
  gasha.montar($('#escena'), { alAbrir: k => { st.sel = k; alterna(k); } });
  tour.montar({ ir, seccion: () => st.sec });
  marco();
  tema();
  proyectos();
  // La primera vez, el recorrido guiado.
  try { if (!localStorage.getItem('ea-tour-visto')) tour.iniciar(); } catch { /* sin almacenamiento */ }
  // Sin clave de sesión válida, la cabecera lo dice y las escrituras avisan.
  const lectura = () => {
    const sin = !ea.escritura();
    if (sin && $('#lectura').hidden) avisa(ea.avisoLectura);
    $('#lectura').hidden = !sin;
  };
  // Una clave nueva (pegada en una página ya abierta) se comprueba antes de fiarse.
  ea.alCambiarSesion(ok => (ok ? ea.sesion().catch(() => {}).finally(lectura) : lectura()));
  ea.sesion().catch(() => {}).finally(lectura);
  tick();
  // El refresco sale de los ajustes (web.sondeo_ms); sin ellos, cada 3 s.
  ea.ajustes().then(a => aplicaCliente(a.cliente)).catch(() => {}).finally(programaSondeo);
  setInterval(relojes, 1000);
}
