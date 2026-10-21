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
  trabajos: 'No hay trabajos. Encarga una misión con ＋ misión, o desde Claude Code con /mision, /obra o /rutina.',
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
  faseDiff: {},        // diff de las fases desplegadas ('<id>:<n>' -> respuesta)
  fasePlan: new Set(), // fases con el plan desplegado ('<id>:<n>')
  abiertos: new Set(), // claves abiertas (j<id>, r<slug>, …)
  sel: null,           // clave seleccionada
  enlace: null,        // clave de la fila que va en el hash (#seccion/<fila>)
  irA: null,           // clave de la fila que hay que enseñar al pintarla
  vis: [],             // claves visibles, en orden
  html: '',
  q: '',
  filtroEstado: null,
  nueva: null,         // formulario «＋ misión» abierto: { v: valores, form: ea.nuevoTrabajo(), err }
  propTodas: false,
  latidos: false,
  // Filtros del diario (van al servidor y al hash, ver PARAMS.diario).
  dActor: null,
  dResultado: null,    // 'ok' | 'error'
  dAccion: null,       // prefijo de la familia: 'bus_', 'mcp_'…
  dDesde: null,        // 'hoy' | 'ayer' | '24h' | '7d' | AAAA-MM-DD
  dHasta: null,        // AAAA-MM-DD (incluido)
  dTrabajo: null,
  dFechas: false,      // se eligió «fechas…» en el rango (aún sin fecha)
  // «↓ más antiguos»: ver fundeDiario().
  dLista: null,        // lo cargado del diario, sin el filtro de proyecto
  dMas: false,         // hay páginas antiguas cargadas además de la primera
  dFin: false,         // no quedan eventos más antiguos
  dFirma: '',          // los filtros con los que se cargó dLista
  dCargando: false,
  anota: null,         // formulario «✎ anotar» del diario: { v: { texto, trabajo }, corrige: id | null, err }
  armado: null,       // { n, hasta }: reinicio de servicio pendiente de confirmar
  ocupado: false,
  otra: false,         // hay que repetir el sondeo al terminar el actual
  consola: {},         // por herramienta: { v: valores, sim, res, err }
  rutEnt: {},          // por rutina: { v: entradas escritas, previa, err }
  rutEd: null,         // editor de TOML abierto: ver editorRutina()
  rutVer: {},          // por rutina: { abierto, lista, err, txt: { n: TOML } }: ver versionesHtml()
  rutVerSel: null,     // versión elegida: { slug (null: la fila del enlace), n, diff }
  rutHor: {},          // por rutina: { v: OnCalendar escrito, sim, quitar, err }: ver horarioRutina()
  ed: {},              // estado de cada editor (memoria, bóveda): ver ed()
  ajBorrador: {},      // clave -> lo escrito en el control de un ajuste, sin guardar
  rfBorrador: {},      // trabajo -> { nota, fase }: lo escrito para reformularlo, sin enviar
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

// «cancelando…»: cancelado, pero el ejecutor aún tiene su worktree (parando al agente).
const estadoJ = t => `<span class="est ${t.estado}">${capsulaJ(t)}${t.cancelando ? 'cancelando…' : EST[t.estado][1]}</span>`;
const NO_SE_DESHACE = 'lo que el agente ya escribió en el worktree no se deshace';

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
  if (t.cancelando) return ['⊘ cancelando…', 'hot'];
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
    h += reformularHtml(t);
  } else if (t.estado === 'en_espera') {
    h += `<textarea class="nota" rows="2" placeholder="contexto que se añade al paquete (opcional)" aria-label="Contexto" maxlength="10000"></textarea>` +
      b('confirmar', '✓ desatascar', 'pasa el paquete de corrección a Claude Code', 'coin hot');
  }
  h += mas;
  if (!FINAL.includes(t.estado)) h += b('cancelar', '⊘ devolver', `cancelar el trabajo (segundo clic para confirmar)${t.estado === 'ejecutando' ? '; ' + NO_SE_DESHACE : ''}`, 'hot');
  return h ? `<div class="acc" data-j="${id}">${h}</div>` : '';
}

// Reformular: encima, las notas de las revisiones anteriores (solo lectura);
// debajo, la nota nueva y, en una obra, desde qué fase se rehace: la
// pendiente por defecto (en una obra completa, la última). Esa fase y las
// siguientes vuelven a quedar pendientes. Lo escrito vive en st.rfBorrador.
function reformularHtml(t) {
  const o = st.detalle['j' + t.id]?.orden;
  const bo = st.rfBorrador[t.id] ?? {};
  const id = esc(t.id);
  let h = '<div class="rf">';
  if (o?.notas_revision?.length) {
    h += '<p class="et">notas anteriores</p>' +
      o.notas_revision.map((n, i) => `<p class="tx"><span class="dd">${i + 1}.</span> ${esc(n)}</p>`).join('');
  }
  h += `<textarea class="nota" data-rf="${id}" rows="3" placeholder="nota para volver a girar: qué falla y qué cambiar" aria-label="Nota para reformular" maxlength="10000">${esc(bo.nota ?? '')}</textarea></div>`;
  if (t.modo === 'obra' && t.fases_total) {
    const fases = o?.fases?.length ? o.fases : Array.from({ length: t.fases_total }, (_, i) => ({ numero: i + 1 }));
    const nums = fases.map(f => f.numero);
    const pend = nums.includes(t.fase_actual) ? t.fase_actual : Math.max(...nums);
    const elegida = bo.fase ?? pend;
    h += `<select data-rff="${id}" aria-label="Fase desde la que se rehace" title="esa fase y las siguientes vuelven a quedar pendientes">` +
      fases.map(f => `<option value="${f.numero}"${f.numero === elegida ? ' selected' : ''}>rehacer desde la fase ${f.numero}` +
        `${f.titulo ? ': ' + esc(f.titulo) : ''}${f.numero === pend ? ' · pendiente' : ''}</option>`).join('') + '</select>';
  }
  return h + btn('reformular', '↺ volver a girar <span class="dd">reformular</span>', 'devolverlo al modelo local con esta nota (obligatoria)', { id: t.id });
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

// Ficha completa (H16): metadatos de la orden, el plan de la obra con el
// estado y el diff de cada fase, y el resumen del agente. Solo lectura.
function metaTrabajo(o) {
  const t = o.topes || {};
  const sk = o.skills?.length ? 'skills: ' + o.skills.map(esc).join(', ') : 'sin skills';
  const topes = `topes: ${t.iteraciones} it. · ${Math.round((t.tiempo_s || 0) / 60)} min · ${t.turnos} turnos · ${t.llamadas_herramienta} llamadas`;
  const rut = o.rutina ? ` · rutina ${esc(o.rutina.slug)} v${o.rutina.version}` : '';
  return `<p class="m">${sk} · ${topes} · creada por ${esc(o.creada_por || '—')}${rut}</p>`;
}

const FASE_IC = { hecha: ['✓', ''], pendiente: ['·', 'm'], bloqueada: ['✕', 'hot'], cancelada: ['⊘', 'm'], 'en curso': ['▶', ''] };

function planObra(id, d) {
  const fs = d.orden.fases || [];
  if (!fs.length) return '';
  const est = Object.fromEntries((d.fases || []).map(f => [f.numero, f]));
  const actual = d.estado?.estado === 'ejecutando' ? d.estado.fase_actual : null;
  return '<p class="et">plan de la obra</p><div class="fo">' + fs.map(f => {
    const e = { ...est[f.numero] };
    if (f.numero === actual && e.estado === 'pendiente') e.estado = 'en curso';
    const k = id + ':' + f.numero;
    const [ic, cls] = FASE_IC[e.estado] ?? ['◌', 'hot'];
    const plan = st.fasePlan.has(k), dif = st.faseDiff[k];
    let h = `<i class="${cls}">${ic}</i><span class="x">${f.numero}. ${esc(f.titulo)}</span>` +
      `<span class="m">${esc(e.estado || '—')}${e.cerrado_en ? ' · ' + hora(e.cerrado_en) : ''}${e.diff_stat ? ' · ' + esc(e.diff_stat.trim().split('\n').pop()) : ''}</span>` +
      `<span>${btn('plan-fase', (plan ? '▾' : '▸') + ' plan', 'instrucciones, entregable y criterio de la fase', { id, v: f.numero }, 'l')}` +
      (e.instantanea ? ' ' + btn('diff-fase', (dif !== undefined ? '▾' : '▸') + ' diff', 'lo que cambió en esta fase (instantáneas de árbol)', { id, v: f.numero }, 'l') : '') + '</span>';
    if (plan) h += `<div class="ex"><p class="et">instrucciones</p><pre class="tx">${esc(f.instrucciones)}</pre>` +
      `<p class="et">entregable</p><p class="tx">${esc(f.entregable)}</p><p class="et">criterio</p><pre class="tx">${esc(f.criterio)}</pre>` +
      (f.skills?.length ? `<p class="m">skills: ${f.skills.map(esc).join(', ')}</p>` : '') + '</div>';
    if (dif) h += '<div class="ex">' + (dif.error ? `<p class="hot">${esc(dif.error)}</p>`
      : `<pre class="diff">${colorDiff(dif.diff || '(sin cambios)')}${dif.truncado ? '\n… (recortado)' : ''}</pre>`) + '</div>';
    return h;
  }).join('') + '</div>';
}

function resumenAgente(d) {
  return d.resumen_agente ? `<p class="et">resumen del agente</p><pre class="tx">${esc(d.resumen_agente)}</pre>` : '';
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
    if (t.cancelando) det += `<p class="m hot">cancelando…: el ejecutor está parando al agente y soltará el worktree; ${NO_SE_DESHACE}.</p>`;
    if (d) {
      const o = d.orden;
      det += `<p class="et">objetivo</p><p class="tx">${esc(o.objetivo)}</p>` +
        `<p class="m">${esc(o.worktree)}${o.criterio_exito ? ' · criterio: ' + esc(o.criterio_exito) : ''}${o.revisar_cada ? ' · revisa cada ' + o.revisar_cada : ''}</p>` +
        metaTrabajo(o) +
        (o.notas_revision?.length && t.estado !== 'en_revision' ? '<p class="et">notas de revisión</p>' + o.notas_revision.map(n => `<p class="tx">${esc(n)}</p>`).join('') : '') +
        informeHtml(d) +
        resumenAgente(d) +
        planObra(t.id, d) +
        (d.paquete ? `<p class="et hot">paquete de corrección</p><pre class="tx">${esc(d.paquete)}</pre>` : '') +
        (d.informe?.commit_inicio ? diffHtml(t.id) : '') +
        `<p class="et">historial ${btn('ir-diario', '→ diario', 'los eventos de este trabajo en el diario', { id: t.id }, 'l')}</p><div class="evs">` + d.historial.map(h =>
          `<div class="ev"><span>${hora(h.ts)}</span><i></i><span>${esc(h.evento)}</span><span>${esc(EST[h.estado]?.[1] ?? h.estado)}</span><span>${h.iteracion ? 'it. ' + h.iteracion : ''}</span></div>`).join('') + '</div>' +
        accionesTrabajo(t, d.informe);
    } else {
      det += accionesTrabajo(t, null);
    }
    if (d && FINAL.includes(t.estado)) det += botonClonar(t);
    return fila(k, 'j', row, `${t.titulo} ${t.modo} ${t.id} ${EST[t.estado][1]} ${t.resultado ?? ''}`, det, FINAL.includes(t.estado) ? ' fin' : '');
  });
}


// El formulario «＋ misión», en línea encima de la lista. Lo escrito vive en
// st.nueva.v (el sondeo repinta). El criterio solo se envía: lo ejecuta el
// sistema al final de cada iteración, nunca esta página.
function formMision() {
  const n = st.nueva;
  if (!n) return [];
  const v = n.v, t = n.form.techo, obra = n.modo === 'obra';
  const min = Math.max(1, Math.floor(t.tiempo_s / 60));
  const campo = (c, ph, extra = '') =>
    `<input data-nm="${c}" placeholder="${esc(ph)}" aria-label="${esc(ph)}" value="${esc(v[c] ?? '')}"${extra}>`;
  const tope = (c, et, max) =>
    `<label>${et} <input data-nm="${c}" type="number" inputmode="numeric" min="1" max="${max}" value="${esc(v[c] ?? '')}" aria-label="${et} (como mucho ${max})"></label>`;
  const det = '<div class="nm">' + modosNueva(n) +
    '<div class="fm">' + campo('titulo', 'título', ' maxlength="200"') +
    campo('worktree', 'worktree: ruta absoluta del repo', ' class="wt" list="nm-wt" spellcheck="false"') +
    `<datalist id="nm-wt">${n.form.worktrees.map(w => `<option value="${esc(w)}"></option>`).join('')}</datalist></div>` +
    (obra
      ? `<textarea data-nm="objetivo" rows="3" maxlength="20000" placeholder="objetivo general: el contexto que reciben todas las fases" aria-label="Objetivo general">${esc(v.objetivo ?? '')}</textarea>` +
        fasesObra(n) + '<div class="fm">' + revisarCada(n) + tope('iteraciones', 'iteraciones', t.iteraciones) + tope('minutos', 'minutos', min) + '</div>' +
        `<p class="m">Cada fase pasa cuando su criterio pasa; lo ejecuta el sistema, nunca esta página. Cada cuántas fases revisa Claude Code lo eliges tú. Topes como mucho: ${t.iteraciones} iteraciones y ${min} min (techo de ajustes.toml).</p>`
      : `<textarea data-nm="objetivo" rows="4" maxlength="20000" placeholder="objetivo: qué tiene que conseguir el modelo local" aria-label="Objetivo">${esc(v.objetivo ?? '')}</textarea>` +
        '<div class="fm">' + campo('criterio', 'criterio: comando que decide si está hecho (obligatorio)', ' class="cr" maxlength="2000" spellcheck="false"') +
        tope('iteraciones', 'iteraciones', t.iteraciones) + tope('minutos', 'minutos', min) + '</div>' +
        `<p class="m">El criterio lo ejecuta el sistema al final de cada iteración, nunca esta página. Topes como mucho: ${t.iteraciones} iteraciones y ${min} min (techo de ajustes.toml).</p>`) +
    (n.aviso ? `<p class="m">⧉ ${esc(n.aviso)}</p>` : '') +
    (n.err ? `<p class="hot">✕ ${esc(n.err)}</p>` : '') +
    '<div class="acc">' + (obra
      ? btn('crear-mision', '▶ encolar obra', 'crear la obra y ponerla en la cola del modelo local', {}, 'coin')
      : btn('crear-mision', '▶ encolar misión', 'crear la misión y ponerla en la cola del modelo local', {}, 'coin')) +
    btn('nueva-mision', '✕ descartar', 'cerrar el formulario sin crear nada') + '</div></div>';
  const nombre = obra ? 'obra' : 'misión';
  const row = `<span class="est">${capsula(obra ? COLOR.obra : COLOR.mision)}nueva</span><span class="n">—</span>` +
    `<span class="x">${esc(v.titulo || nombre + ' sin título')}</span><span class="m w">${nombre}</span>` +
    `<span class="m">${obra ? `${n.fases.length} ${n.fases.length === 1 ? 'fase' : 'fases'}` : 'para el modelo local'}</span><span class="m"></span>`;
  return [suelto(`<article class="f o nueva"><div class="r j">${row}</div><div class="det">${det}</div></article>`, true)];
}

async function creaMision() {
  const n = st.nueva, v = n.v;
  const num = c => (String(v[c] ?? '').trim() ? Number(v[c]) : undefined);
  const iteraciones = num('iteraciones'), minutos = num('minutos');
  for (const [x, nombre] of [[iteraciones, 'iteraciones'], [minutos, 'minutos']]) {
    if (x !== undefined && !(Number.isInteger(x) && x >= 1)) throw new Error(`'${nombre}' debe ser un número entero mayor que 0`);
  }
  const topes = { iteraciones, tiempo_s: minutos === undefined ? undefined : minutos * 60 };
  const obra = n.modo === 'obra';
  if (obra && !v.revisar) throw new Error('elige cada cuántas fases revisa Claude Code');
  const t = await ea.crearTrabajo(obra
    ? {
      modo: 'obra', titulo: v.titulo ?? '', worktree: v.worktree ?? '', objetivo: v.objetivo ?? '', topes,
      fases: n.fases.map(f => ({ titulo: f.titulo ?? '', instrucciones: f.instrucciones ?? '', entregable: f.entregable ?? '', criterio: f.criterio ?? '' })),
      revisar_cada: v.revisar === 'fin' ? null : Number(v.revisar),
    }
    : { modo: 'mision', titulo: v.titulo ?? '', worktree: v.worktree ?? '', objetivo: v.objetivo ?? '', criterio: v.criterio ?? '', topes });
  st.nueva = null;
  st.filtroEstado = null;
  avisa(`✓ ${obra ? 'obra' : 'misión'} ${t.id.slice(9, 15)} en la cola del modelo local`, true);
  abre('trabajos', 'j' + t.id);
}

// ----------------------------------------------- «＋ misión» en modo obra
// Lo escrito en las fases vive en st.nueva.fases (el sondeo repinta); el
// número de cada fase es su posición. Las fases se añaden, quitan y
// reordenan con botones; cada una lleva su criterio, que solo se envía.

const FASES_MAX = 20;
const faseVacia = () => ({ titulo: '', instrucciones: '', entregable: '', criterio: '' });

// Misión u obra, si el servidor anuncia las obras en `modos`.
function modosNueva(n) {
  if (!n.form.modos.includes('obra')) return '';
  const m = (modo, et, titulo) =>
    `<button class="b${n.modo === modo ? ' on' : ''}" data-a="nm-modo" data-v="${modo}" title="${esc(titulo)}" aria-pressed="${n.modo === modo}">${et}</button>`;
  return '<div class="seg" role="group" aria-label="Modo">' +
    m('mision', '● misión', 'un objetivo y un criterio') + m('obra', '◎ obra', 'un plan por fases, cada una con su criterio') + '</div>';
}

function fasesObra(n) {
  const total = n.fases.length;
  return '<ol class="fases">' + n.fases.map((f, i) => {
    const c = (campo, ph, extra = '') =>
      `<input data-nf="${i}" data-nc="${campo}" placeholder="${esc(ph)}" aria-label="Fase ${i + 1}: ${esc(ph)}" value="${esc(f[campo] ?? '')}"${extra}>`;
    return `<li class="fase"><div class="fm"><b class="nf">${i + 1}</b>` + c('titulo', 'título de la fase', ' maxlength="200"') +
      (i > 0 ? btn('nf-subir', '↑', `subir la fase ${i + 1}`, { n: i }) : '') +
      (i < total - 1 ? btn('nf-bajar', '↓', `bajar la fase ${i + 1}`, { n: i }) : '') +
      (total > 1 ? btn('nf-quitar', '✕', `quitar la fase ${i + 1}`, { n: i }, 'hot') : '') + '</div>' +
      `<textarea data-nf="${i}" data-nc="instrucciones" rows="2" maxlength="20000" placeholder="instrucciones: qué hacer en esta fase" aria-label="Fase ${i + 1}: instrucciones">${esc(f.instrucciones ?? '')}</textarea>` +
      '<div class="fm">' + c('entregable', 'entregable: qué queda hecho', ' maxlength="2000"') +
      c('criterio', 'criterio: comando que decide si la fase está hecha (obligatorio)', ' class="cr" maxlength="2000" spellcheck="false"') + '</div></li>';
  }).join('') + '</ol>' +
    (total < FASES_MAX ? '<div class="acc">' + btn('nf-mas', '＋ fase', 'añadir una fase al final') + '</div>' : '');
}

// Cada cuántas fases revisa Claude Code (art. IV.4): sin valor por defecto;
// hasta que se elige, el formulario no se envía.
function revisarCada(n) {
  const total = n.fases.length, v = n.v.revisar ?? '';
  const op = (valor, et) => `<option value="${valor}"${v === valor ? ' selected' : ''}>${et}</option>`;
  return `<label>revisión <select data-nm="revisar" aria-label="Cada cuántas fases revisa Claude Code"${v ? '' : ' class="falta"'}>` +
    op('', '— elige —') + op('fin', 'solo al final') +
    Array.from({ length: total }, (_, i) => op(String(i + 1), i ? `cada ${i + 1} fases` : 'tras cada fase')).join('') + '</select></label>';
}

// Las acciones de los botones del modo obra; true si era una de ellas.
function accionFase(a, i) {
  const n = st.nueva;
  if (!n || !['nm-modo', 'nf-mas', 'nf-subir', 'nf-bajar', 'nf-quitar'].includes(a)) return false;
  const f = n.fases;
  if (a === 'nm-modo') n.modo = i;
  else if (a === 'nf-mas' && f.length < FASES_MAX) f.push(faseVacia());
  else if (a === 'nf-quitar' && f.length > 1) f.splice(Number(i), 1);
  else if (a === 'nf-subir' || a === 'nf-bajar') {
    const j = Number(i), k = a === 'nf-subir' ? j - 1 : j + 1;
    if (k >= 0 && k < f.length) [f[j], f[k]] = [f[k], f[j]];
  }
  // Una revisión que ya no cabe en el número de fases se vuelve a elegir.
  if (Number(n.v.revisar) > f.length) n.v.revisar = '';
  n.err = null;
  st.html = '';
  document.activeElement?.blur(); // con un campo enfocado, pintar() no repinta
  pintar();
  if (a === 'nf-mas') $(`[data-nf="${f.length - 1}"][data-nc="titulo"]`)?.focus();
  return true;
}

// ------------------------------------- «⧉ lanzar de nuevo» (clonar, T7)
// Un trabajo cerrado o cancelado se vuelve a encargar desde «＋ misión»,
// relleno con su orden: título, worktree, objetivo, criterio, topes y, en
// una obra, fases y revisar_cada. No se copian el id, las notas de revisión,
// el informe ni las skills (desde la web no se eligen); una rutina vuelve
// como misión normal. Nada se envía solo: el usuario lo revisa y lo encola,
// y POST /api/trabajos le da un id nuevo.

function botonClonar(t) {
  const armado = st.armado?.n === 'k' + t.id && st.armado.hasta > Date.now();
  return '<div class="acc">' + (armado
    ? btn('clonar', '⧉ reemplazar el formulario abierto', 'segundo clic: lo escrito en «＋ misión» se pierde', { id: t.id }, 'hot')
    : btn('clonar', '⧉ lanzar de nuevo', 'abrir «＋ misión» relleno con esta orden para revisarla y encolarla', { id: t.id })) + '</div>';
}

async function clonaTrabajo(id) {
  const o = st.detalle['j' + id]?.orden;
  if (!o) throw new Error('la ficha del trabajo aún no ha cargado');
  // Con el formulario ya abierto, un segundo clic confirma que se reemplaza.
  if (st.nueva && !(st.armado && st.armado.n === 'k' + id && st.armado.hasta > Date.now())) {
    st.armado = { n: 'k' + id, hasta: Date.now() + 3000 };
    st.html = '';
    pintar();
    setTimeout(() => { st.armado = null; st.html = ''; pintar(); }, 3100);
    return;
  }
  st.armado = null;
  const form = await ea.nuevoTrabajo();
  const techo = form.techo, minTecho = Math.max(1, Math.floor(techo.tiempo_s / 60));
  const it = o.topes?.iteraciones ?? form.defecto.iteraciones;
  const min = Math.max(1, Math.round((o.topes?.tiempo_s ?? form.defecto.tiempo_s) / 60));
  const obra = o.modo === 'obra' && o.fases?.length > 0;
  const fases = obra
    ? o.fases.map(f => ({ titulo: f.titulo ?? '', instrucciones: f.instrucciones ?? '', entregable: f.entregable ?? '', criterio: f.criterio ?? '' }))
    : [faseVacia()];
  const revisar = !obra ? '' : o.revisar_cada == null ? 'fin' : o.revisar_cada <= fases.length ? String(o.revisar_cada) : '';
  const avisos = [`copia de ${id.slice(9, 15)}: revísala antes de encolar; tendrá un id nuevo`];
  if (o.modo === 'rutina') avisos.push(`viene de la rutina ${o.rutina?.slug ?? ''}: se lanza como misión normal, no como rutina`);
  if (o.skills?.length || o.fases?.some(f => f.skills?.length)) avisos.push('las skills de la orden original no se copian (desde la web no se eligen)');
  if (it > techo.iteraciones || min > minTecho) avisos.push('los topes se han recortado al techo de ajustes.toml');
  if (st.sec !== 'trabajos' && ir('trabajos') === false) return;
  st.nueva = {
    form, err: null, modo: obra ? 'obra' : 'mision', fases, aviso: avisos.join(' · '),
    v: {
      titulo: o.titulo ?? '', worktree: o.worktree ?? '', objetivo: o.objetivo ?? '', criterio: obra ? '' : o.criterio_exito ?? '',
      iteraciones: Math.min(it, techo.iteraciones), minutos: Math.min(min, minTecho), revisar,
    },
  };
  st.html = '';
  document.activeElement?.blur();
  pintar();
  $('#lista').scrollTop = 0;
  $('[data-nm="titulo"]')?.focus();
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

// Adónde lleva un evento del diario, además de a su trabajo: lo que traiga
// su `detail` (también dentro de `argumentos`, `args` y `resultado`). Cada
// destino es { sec, fila, glifo, titulo }; la fila es la clave sin prefijo
// y el glifo va ya escapado (btn no lo escapa).
// Las claves salen de quien emite cada familia: `propuesta` y `herramienta`
// (ea-acciones), `ruta` (ea-boveda; al renombrar, `a`; al promover,
// `destino`), `rutina` (rutina_registrada) o `slug` (argumentos de las
// herramientas de rutinas), `cambios[].clave` (ajustes_cambiados),
// `proyecto` y `fichero` (memoria_*). Un `mcp_<x>` es la herramienta `ea_<x>`.
function destinosEvento(e) {
  let d = {};
  try { d = JSON.parse(e.detalle) ?? {}; } catch { /* sin detalle o no es JSON */ }
  const obj = x => x && typeof x === 'object' && !Array.isArray(x);
  const capas = [d, d.argumentos, d.args, d.resultado].filter(obj);
  const de = (n, vale) => {
    for (const c of capas) {
      const v = c[n];
      if ((typeof v === 'string' || typeof v === 'number') && vale.test(String(v))) return String(v);
    }
    return null;
  };
  const out = [];
  const pon = (sec, fila, glifo, titulo) => {
    if (fila && !out.some(o => o.sec === sec && o.fila === fila)) out.push({ sec, fila, glifo, titulo });
  };
  const ac = e.accion ?? '';
  const prop = de('propuesta', /^\d+$/);
  pon('propuestas', prop, '→ propuesta #' + prop, 'ver la propuesta en ◇ Propuestas');
  const mcp = /^mcp_([a-z_]+?)(_simulada|_denegada)?$/.exec(ac);
  const h = de('herramienta', /^ea_[a-z_]+$/) ?? (mcp ? 'ea_' + mcp[1] : null);
  pon('acciones', h, '→ ' + esc(h), 'ver la herramienta en ▶ Acciones');
  const rut = de('rutina', /^[\w.-]{1,128}$/) ?? de('slug', /^[\w.-]{1,128}$/);
  pon('rutinas', rut, '→ rutina ' + esc(rut), 'ver la rutina en ↻ Rutinas');
  // La ruta de una nota que sigue en su sitio: no la que se fue a la
  // papelera, ni una carpeta, ni la ruta relativa a su región que lleva
  // ea_escribir_nota (la de verdad viene en su boveda_escritura).
  if (!/^(boveda_papelera|boveda_carpeta|mcp_escribir_nota)/.test(ac)) {
    const md = /^[^/]+\/.+\.md$/;
    const ruta = ac === 'boveda_renombrado' ? de('a', md) : ac === 'boveda_promocion' ? de('destino', md) : de('ruta', md);
    pon('boveda', ruta, '→ nota', 'abrir la nota en ▥ Bóveda');
  }
  if (ac.startsWith('memoria_') && ac !== 'memoria_eliminada') {
    const p = de('proyecto', /^[\w.-]+$/), f = ac === 'memoria_renombrada' ? de('a', /^[^/]+\.md$/) : de('fichero', /^[^/]+\.md$/);
    if (p && f) pon('memoria', p + '/' + f, '→ memoria', 'abrir la memoria en ✎ Memoria');
  }
  const claves = Array.isArray(d.cambios) ? d.cambios.map(c => c?.clave) : [d.clave];
  for (const c of claves) {
    if (typeof c === 'string' && /^[\w.-]{1,128}$/.test(c)) pon('ajustes', c, '→ ' + esc(c), 'ver el ajuste en ⚙ Ajustes');
  }
  return out;
}

// Los botones de los destinos de un evento (ver destinosEvento). Los datos
// van en data-dsec y data-dfila: data-f ya es el filtro de estado de Trabajos.
function enlacesEvento(e) {
  const ds = destinosEvento(e);
  if (!ds.length) return '';
  return '<p>' + ds.map(x => x.sec === 'propuestas'
    ? btn('ir-prop', x.glifo, x.titulo, { id: x.fila }, 'l')
    : btn('ir-a', x.glifo, x.titulo, { dsec: x.sec, dfila: x.fila }, 'l')).join(' ') + '</p>';
}

// Abre la fila `f` (clave sin prefijo) de la sección `sec`. En Bóveda y
// Memoria, la fila es la nota abierta del editor; si ya no existe, el
// editor la suelta al pintar.
function irAFila(sec, f) {
  if (EDITOR[sec]) {
    if (!cierraBorrador(sec)) return;
    const E = ed(sec), k = (sec === 'boveda' ? 'b' : 'm') + f;
    Object.assign(E, { sel: k, menu: false, renombrando: null, creando: null, papelera: false });
    EDITOR[sec].desplegar?.(E, k);
    guardaEditor(sec);
    ir(sec);
    return;
  }
  if (FILA[sec]) abre(sec, FILA[sec] + f);
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
      (trabajoDe(e) ? `<p>${btn('ir-trabajo', '→ trabajo', 'ver el trabajo', { id: trabajoDe(e) }, 'l')}</p>` : '') +
      correccionesDiario(e) +
      enlacesEvento(e);
    return fila(k, 'd', row, `${e.actor} ${e.accion} ${e.trabajo ?? ''} ${e.detalle ?? ''}`, det);
  });
}

// Las correcciones de un evento: «corrige #m» en una nota del usuario,
// «corregido por #n» en el evento corregido y «✎ corregir» para añadir otra.
// El diario es append-only: corregir no toca el evento, añade una nota.
function correccionesDiario(e) {
  let corrige = null;
  if (e.accion === 'usuario_nota') try { corrige = JSON.parse(e.detalle)?.corrige ?? null; } catch { /* detalle no JSON */ }
  const a = n => btn('ir-evento', '#' + n, 'ir al evento #' + n, { id: n }, 'l');
  return (corrige != null ? `<p class="m">corrige ${a(corrige)}</p>` : '') +
    (e.corregido_por?.length ? `<p class="hot">corregido por ${e.corregido_por.map(a).join(' ')}</p>` : '') +
    `<p>${btn('d-corregir', '✎ corregir', 'añadir una nota que corrige este evento; el evento queda tal cual', { id: e.id, v: trabajoDe(e) ?? '' }, 'l')}</p>`;
}

// El formulario «✎ anotar» del diario, en línea encima de la lista: una nota
// del usuario (usuario_nota) que deja constancia de algo o corrige un evento.
// Lo escrito vive en st.anota.v (el sondeo repinta).
function formAnotar() {
  const n = st.anota;
  if (!n) return [];
  const v = n.v, corrige = n.corrige != null;
  const det = '<div class="nm">' +
    (corrige ? `<p class="m">corrige el evento ${btn('ir-evento', '#' + n.corrige, 'ver el evento que se corrige', { id: n.corrige }, 'l')}, que queda tal cual: la nota lo enlaza.</p>` : '') +
    `<textarea data-an="texto" rows="3" maxlength="10000" placeholder="${corrige ? 'qué estaba mal y qué pasó de verdad' : 'qué quieres que conste (p. ej. «he reiniciado llama-server a mano»)'}" aria-label="Texto de la nota">${esc(v.texto ?? '')}</textarea>` +
    `<div class="fm"><input data-an="trabajo" class="wt" maxlength="128" spellcheck="false" placeholder="trabajo (opcional): id del bus" aria-label="Trabajo (opcional)" value="${esc(v.trabajo ?? '')}"></div>` +
    '<p class="m">Queda en el diario como usuario_nota, con actor usuario, y no se puede editar ni borrar: un error se corrige con otra nota.</p>' +
    (n.err ? `<p class="hot">✕ ${esc(n.err)}</p>` : '') +
    '<div class="acc">' + btn('d-anotar-enviar', '✎ anotar', 'añadir la nota al diario', {}, 'coin') +
    btn(corrige ? 'd-corregir' : 'd-anotar', '✕ descartar', 'cerrar el formulario sin anotar nada', corrige ? { id: n.corrige } : {}) + '</div></div>';
  const row = '<span class="m">ahora</span><i class="m">✎</i><span class="m w">usuario</span>' +
    `<span class="x">${corrige ? 'corrección de #' + esc(n.corrige) : 'nota'}</span><span class="n"></span><span class="m"></span>`;
  return [suelto(`<article class="f o nueva"><div class="r d">${row}</div><div class="det">${det}</div></article>`, true)];
}

async function anotaDiario() {
  const n = st.anota, texto = (n.v.texto ?? '').trim(), trabajo = (n.v.trabajo ?? '').trim();
  if (!texto) throw new Error('escribe el texto de la nota');
  const cuerpo = { texto };
  if (n.corrige != null) cuerpo.corrige = n.corrige;
  if (trabajo) cuerpo.trabajo = trabajo;
  const e = await ea.anotarDiario(cuerpo);
  st.anota = null;
  avisa(`✓ anotado en el diario: #${e.id}`, true);
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
    const atascada = suyos.some(t => t.estado === 'en_espera') || r.ultima?.estado === 'en_espera';
    const enCola = suyos.some(t => t.estado === 'en_cola');
    const desc = r.error ?? (r.descripcion ?? '') + (atascada ? ' · la última se atascó' : '');
    const row = capsula(COLOR.rutina, r.error ? 'averiada' : atascada ? 'hot' : '') +
      `<span class="x">${esc(r.slug)}</span><span class="m">v${r.version}</span>` +
      `<span class="m ${r.error || atascada ? 'hot' : ''}">${esc(desc)}</span>` +
      `<span class="m"${r.calendario ? ` title="${esc('OnCalendar=' + r.calendario)}"` : ''}>${r.proxima && !r.error ? en(r.proxima) : ''}</span>` +
      ultimaRutina(r.ultima) +
      (r.error ? '<span class="hot fuera">fuera de servicio</span>'
        : btn('lanzar', enCola ? '✓ en la cúpula' : '<span class="ranura" aria-hidden="true"><i></i></span>echar ficha',
          `Lanzar ahora la rutina ${r.slug} con las entradas de su ficha: crea un trabajo; el orquestador lo recoge en segundos`, { slug: r.slug }, enCola ? 'coin ya' : 'coin'));
    const d = st.detalle[k];
    const editando = st.rutEd?.slug === r.slug;
    const det = d ? `<p class="m">versión ${d.version} · ${esc(r.worktree ?? '')}${r.criterio_exito ? ' · criterio: ' + esc(r.criterio_exito) : ''}</p>` +
      ejecucionesRutina(d.ejecuciones) +
      formEntradas(r) +
      horarioRutina(r) +
      (editando ? editorRutina()
        : `<div class="acc">${btn('red', '✎ nueva versión', 'edita el TOML y registra una versión nueva (la versión ya va subida en uno)', { slug: r.slug })}</div>` +
          `<pre class="toml">${esc(d.contenido ?? '')}</pre>`) +
      versionesHtml(r.slug) : '';
    return fila(k, 'u', row, `${r.slug} ${r.descripcion ?? ''}`, det);
  });
}

// ------------------------------------------- horario de una rutina (R4)
//
// El timer de systemd de la rutina (`ea-rutina@<slug>.timer`): su OnCalendar,
// si está activo y un campo para cambiarlo. «◷ guardar» simula primero
// (systemd-analyze valida y da las próximas 3 ejecuciones) y el segundo clic
// ejecuta lo simulado con su precondición: si el timer cambió entre medias,
// la revalidación lo para. «✕ desprogramar», igual. Lo de .sal caduca al
// cambiar el campo.
function horarioRutina(r) {
  if (r.error) return '';
  const H = st.rutHor[r.slug] || {};
  const act = r.timer === 'active' ? 'activo' : r.timer ? esc(r.timer) : '';
  const ahora = r.calendario ? `<b>${esc(r.calendario)}</b>${act ? ' · ' + act : ''}${r.proxima ? ' · próxima ' + en(r.proxima) : ''}`
    : r.timer === 'active' ? 'activo con el horario de la plantilla' : 'sin programar';
  const listo = q => H.sim && !!H.quitar === q;
  let h = `<div class="hor"><p class="et">horario</p><p class="m">${ahora}</p><div class="fm">` +
    `<label class="m">OnCalendar <input data-rhor="${esc(r.slug)}" value="${esc(H.v ?? r.calendario ?? '')}" placeholder="*-*-* 08:00:00" spellcheck="false" ` +
    `title="expresión OnCalendar de systemd: *-*-* 08:00:00 (cada día a las 8), Mon *-*-* 09:00 (los lunes), daily…" aria-label="OnCalendar de ${esc(r.slug)}"></label>` +
    btn('rhguardar', listo(false) ? '✓ confirmar' : '◷ guardar',
      listo(false) ? 'segundo clic: programa el timer como dice la simulación' : 'simula el horario (systemd lo valida y da las próximas 3 ejecuciones); el segundo clic lo guarda', { slug: r.slug }) +
    (r.calendario || r.timer === 'active'
      ? btn('rhquitar', listo(true) ? '✓ confirmar' : '✕ desprogramar',
        listo(true) ? 'segundo clic: desactiva y borra el timer' : 'simula quitar el timer (la rutina sigue registrada); el segundo clic lo quita', { slug: r.slug })
      : '') + '</div>';
  if (H.sim) h += `<p class="tx sal">${esc(H.sim.texto)}</p>`;
  if (H.err) h += `<p class="hot sal">✕ ${esc(H.err)}</p>`;
  return h + '</div>';
}

async function accionHorario(a, slug, b) {
  const H = (st.rutHor[slug] ??= {});
  const quitar = a === 'rhquitar';
  const herr = quitar ? 'ea_desprogramar_rutina' : 'ea_programar_rutina';
  if (H.sim && !!H.quitar === quitar) {
    let r;
    try {
      r = await ea.ejecutar(herr, H.sim.argumentos, H.sim.precondicion);
    } catch (e) {
      if (e.status === 403) throw e;
      H.sim = null;
      H.err = e.message;
    }
    if (r) {
      delete st.rutHor[slug];
      avisa('✓ ' + r.texto, true);
    }
    return;
  }
  const campo = b.closest('.hor')?.querySelector('[data-rhor]');
  if (campo) H.v = campo.value;
  H.sim = H.err = null;
  H.quitar = quitar;
  try {
    H.sim = await ea.simular(herr, quitar ? { slug } : { slug, calendario: H.v ?? '' });
  } catch (e) {
    if (e.status === 403) throw e;
    H.err = e.message;
  }
}

// --------------------------------------- ejecuciones de una rutina (R2)
//
// La columna «última» de la fila (estado y hace cuánto) y, en la ficha, las
// últimas ejecuciones que da el servidor (`ultima` y `ejecuciones`, del bus),
// cada una con su botón a la ficha del trabajo. Solo lectura.

// Una ejecución «pinta mal» si espera la confirmación de su paquete o se
// bloqueó.
const ejMal = e => e.estado === 'en_espera' || e.resultado === 'bloqueado';

function ultimaRutina(e) {
  if (!e) return '<span class="m">—</span>';
  return `<span class="m${ejMal(e) ? ' hot' : ''}" title="${esc(`trabajo ${e.trabajo} · v${e.version}${e.detalle ? ' · ' + e.detalle : ''}`)}">` +
    `${EST[e.estado]?.[0] ?? ''} ${esc(EST[e.estado]?.[1] ?? e.estado)} · ${reloj(e.actualizado)}</span>`;
}

function ejecucionesRutina(es) {
  if (!es) return '';
  if (!es.length) return '<p class="et">últimas ejecuciones</p><p class="m">sin ejecuciones todavía</p>';
  return `<p class="et">últimas ${es.length} ejecuciones</p><div class="evs">` + es.map(e =>
    `<div class="ev"><span title="${esc(e.creado)}">${hora(e.creado)}</span><span class="${ejMal(e) ? 'hot' : ''}">${EST[e.estado]?.[0] ?? ''}</span>` +
    `<span class="${ejMal(e) ? 'hot' : ''}">${btn('ir-trabajo', esc(e.trabajo.slice(9, 15)), `abrir la ficha del trabajo ${e.trabajo}`, { id: e.trabajo }, 'l')} ` +
    `${esc(EST[e.estado]?.[1] ?? e.estado)}${e.resultado ? ' · ' + e.resultado : ''}${e.detalle ? ' · ' + esc(e.detalle) : ''}</span>` +
    `<span class="m">v${e.version}</span><span class="m">${e.duracion_s != null ? dur(e.duracion_s * 1000) : ''}</span></div>`).join('') + '</div>';
}

// ------------------------------------------------ editor de rutinas (TOML)
//
// «✎ nueva versión» en la ficha y «＋ rutina» en la barra abren el mismo
// editor (st.rutEd: { slug, texto, base, val, err, armado }; slug '' es una
// rutina nueva). «◌ validar» pregunta al servidor sin escribir nada;
// «✓ registrar» pide un segundo clic. Lo de .sal caduca al teclear.

const PLANTILLA_RUTINA = `# Una rutina: el agente local la ejecuta; el sistema corre el criterio.
[rutina]
slug = "mi-rutina"                 # [A-Za-z0-9_-], 64 como mucho
version = 1                        # solo puede subir
descripcion = "Qué hace, en una línea"
worktree = "/ruta/absoluta/al/repo" # repo git donde trabaja
criterio_exito = "test -s {salida}" # shell; lo ejecuta el sistema, nunca el agente
salida_esperada = "Qué debe quedar hecho y dónde"

[entradas]                         # clave = valor por defecto; {clave} se sustituye
salida = "resumen.md"

[[pasos]]
texto = "Primer paso, concreto, con rutas y comandos; escribe {salida}."

[[pasos]]
texto = "Comprueba que {salida} no esté vacío."
`;

// El TOML con la versión de [rutina] subida en uno (la primera línea
// «version = N» del fichero, que es la de la cabecera).
function subeVersion(toml) {
  return toml.replace(/^(\s*version\s*=\s*)(\d+)/m, (_, pre, n) => pre + (Number(n) + 1));
}

// Diff de líneas mínimo (subsecuencia común más larga), con el mismo formato
// que el de las simulaciones: solo lo que cambia, dos líneas de contexto y
// @@ entre trozos. Vacío si son iguales.
function diffLineas(antes, despues) {
  const a = antes.replace(/\n$/, '').split('\n'), b = despues.replace(/\n$/, '').split('\n');
  if (!antes) a.length = 0;
  const n = a.length, m = b.length;
  let ops;
  if (n * m > 4e6) ops = [...a.map(l => ['-', l]), ...b.map(l => ['+', l])];
  else {
    const L = Array.from({ length: n + 1 }, () => new Uint32Array(m + 1));
    for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--)
      L[i][j] = a[i] === b[j] ? L[i + 1][j + 1] + 1 : Math.max(L[i + 1][j], L[i][j + 1]);
    ops = [];
    let i = 0, j = 0;
    while (i < n || j < m) {
      if (i < n && j < m && a[i] === b[j]) { ops.push([' ', a[i]]); i++; j++; }
      else if (i < n && (j === m || L[i + 1][j] >= L[i][j + 1])) ops.push(['-', a[i++]]);
      else ops.push(['+', b[j++]]);
    }
  }
  const cambia = ops.map((o, k) => (o[0] === ' ' ? -1 : k)).filter(k => k >= 0);
  if (!cambia.length) return '';
  let out = '', prev = null;
  ops.forEach(([s, l], k) => {
    if (!cambia.some(c => Math.abs(c - k) <= 2)) return;
    if ((prev !== null && prev + 1 !== k) || (prev === null && k > 0)) out += '@@\n';
    prev = k;
    out += s + l + '\n';
  });
  return out;
}

function editorRutina() {
  const E = st.rutEd;
  const nueva = E.slug === '';
  const lineas = Math.min(32, Math.max(10, E.texto.split('\n').length + 1));
  const armada = E.armado && E.armado > Date.now();
  let h = `<p class="et">${nueva ? '＋ rutina nueva' : `✎ nueva versión de ${esc(E.slug)}`} · TOML</p>` +
    `<textarea class="toml-ed" data-red rows="${lineas}" spellcheck="false" aria-label="definición TOML de la rutina">${esc(E.texto)}</textarea>` +
    '<div class="acc">' +
    btn('rvalidar', '◌ validar', 'comprueba la definición como al registrar y muestra la orden y los cambios, sin escribir nada') +
    btn('rregistrar', armada ? '✓ confirmar registro' : '✓ registrar', armada ? 'confirmar: registra esta versión' : 'registra esta versión (pide confirmar)', {}, armada ? 'coin hot' : 'coin') +
    btn('rdescartar', '✕ descartar', 'cierra el editor sin registrar nada') + '</div>';
  if (E.val) {
    const v = E.val;
    const base = v.anterior ? v.anterior.definicion : '';
    const d = diffLineas(base, E.texto);
    h += `<p class="tx sal">✓ válida: <b>${esc(v.rutina)}</b> v${v.version}${v.anterior ? ` (registrada: v${v.anterior.version})` : ' · rutina nueva'} · criterio: <b>${esc(v.previa.criterio)}</b></p>` +
      `<p class="et sal">${v.anterior ? `cambios frente a la v${v.anterior.version}` : 'definición completa'}</p>` +
      `<pre class="diff sal">${colorDiff(d || '(sin cambios)')}</pre>` +
      `<p class="et sal">orden con las entradas por defecto</p><pre class="toml sal">${esc(v.previa.objetivo)}</pre>`;
  }
  if (E.err) h += `<p class="hot sal">✕ ${esc(E.err)}</p>`;
  return h;
}

// Abre el editor: con la versión registrada (y la versión subida) o con la plantilla.
function abreEditorRutina(slug) {
  if (st.rutEd && st.rutEd.slug === slug) { st.rutEd = null; return; }
  const base = slug ? st.detalle['r' + slug]?.contenido ?? '' : '';
  st.rutEd = { slug, base, texto: slug ? subeVersion(base) : PLANTILLA_RUTINA, val: null, err: null, armado: null };
}

// ------------------------------------------- versiones de una rutina (R3)
//
// «▸ versiones» en la ficha lista las registradas (la más reciente primero)
// con «ver» (su TOML) y «diff» (frente a la versión anterior de la lista, con
// diffLineas y colorDiff, como el editor). Solo lectura. La versión elegida
// va al hash: #rutinas/<slug>?version=N[&diff=1].

// La versión elegida de `slug`, si la hay ({ n, diff }).
function selVersion(slug) {
  const s = st.rutVerSel;
  if (!s) return null;
  // La de un enlace se queda con la fila del enlace en cuanto la hay.
  if (s.slug == null && st.enlace?.startsWith('r')) s.slug = st.enlace.slice(1);
  return s.slug === slug ? s : null;
}

// La lista de versiones y los TOML que hacen falta para lo elegido. Los TOML
// no cambian: se piden una vez.
async function cargaVersiones(slug) {
  const V = (st.rutVer[slug] ??= { abierto: false, lista: null, err: null, txt: {} });
  const sel = selVersion(slug);
  if (sel) V.abierto = true;
  if (!V.abierto) return;
  try {
    V.lista = await ea.versionesRutina(slug);
    V.err = null;
    if (!sel) return;
    const i = V.lista.findIndex(v => v.version === sel.n);
    if (i < 0) { V.err = `la rutina '${slug}' no tiene versión ${sel.n}`; return; }
    const hacen = [sel.n, ...(sel.diff && V.lista[i + 1] ? [V.lista[i + 1].version] : [])];
    for (const n of hacen) V.txt[n] ??= (await ea.versionRutina(slug, n)).contenido;
  } catch (e) {
    if (e.status === 403) throw e;
    V.err = e.message;
  }
}

function versionesHtml(slug) {
  const V = st.rutVer[slug], abierto = !!V?.abierto;
  let h = `<p class="et">${btn('rversiones', (abierto ? '▾' : '▸') + ' versiones', 'las versiones registradas: su TOML y qué cambió frente a la anterior', { slug }, 'l')}</p>`;
  if (!abierto) return h;
  if (V.err) h += `<p class="hot">✕ ${esc(V.err)}</p>`;
  if (!V.lista) return h + (V.err ? '' : '<p class="m">cargando…</p>');
  const sel = selVersion(slug);
  h += '<div class="evs">' + V.lista.map((v, i) => {
    const ant = V.lista[i + 1];
    const es = m => sel?.n === v.version && !!sel.diff === m;
    return `<div class="ev rv"><span class="${sel?.n === v.version ? '' : 'm'}">v${v.version}</span>` +
      `<span title="${esc(v.creada_en)}">${hora(v.creada_en)}</span><span class="m">${esc(v.creada_por)}</span><span>` +
      btn('rver', es(false) ? '▾ ver' : 'ver', `el TOML de la versión ${v.version}`, { slug, n: v.version }, 'l') +
      (ant ? ' ' + btn('rvdiff', es(true) ? '▾ diff' : 'diff', `qué cambió de la v${ant.version} a la v${v.version}`, { slug, n: v.version }, 'l') : '') +
      '</span></div>';
  }).join('') + '</div>';
  const i = sel ? V.lista.findIndex(v => v.version === sel.n) : -1;
  if (i < 0) return h;
  const txt = V.txt[sel.n], ant = V.lista[i + 1];
  if (txt === undefined) return h + '<p class="m">cargando…</p>';
  if (!sel.diff) return h + `<p class="et">v${sel.n} · TOML</p><pre class="toml">${esc(txt)}</pre>`;
  if (!ant) return h + '<p class="m">es la primera versión: no hay anterior con la que comparar</p>';
  if (V.txt[ant.version] === undefined) return h + '<p class="m">cargando…</p>';
  return h + `<p class="et">cambios de la v${ant.version} a la v${sel.n}</p>` +
    `<pre class="diff">${colorDiff(diffLineas(V.txt[ant.version], txt) || '(sin cambios)')}</pre>`;
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
    // Registrar una rutina lleva el TOML entero: su simulación ya trae el diff.
    const conToml = typeof ac?.argumentos?.toml === 'string';
    let det = conToml && p.descripcion === ac.simulacion ? '' : `<p class="tx">${esc(p.descripcion)}</p>`;
    if (ac) {
      det += `<p class="m">${esc(ac.herramienta)} · riesgo ${esc(ac.riesgo)} · propuesta por ${esc(p.proponente ?? '?')}</p>` +
        (conToml ? `<p class="m">toml: ${ac.argumentos.toml.length} caracteres</p><p class="et">simulación</p><pre class="diff">${colorDiff(ac.simulacion)}</pre>`
          : `<p class="m">${esc(JSON.stringify(ac.argumentos))}</p><p class="tx">Simulación: ${esc(ac.simulacion)}</p>`);
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
        if (f.largo) return `<textarea ${attrs(f)} placeholder="${esc(f.ayuda)}" spellcheck="false" rows="8">${esc(val)}</textarea>`;
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
    case 'trabajos': return d ? [...formMision(), ...con(columnas('j', 'estado', 'id', 'trabajo', 'modo', 'premio · avance', 'hace'),
      filasTrabajos(st.filtroEstado ? d.filter(t => t.estado === st.filtroEstado) : d))] : [];
    case 'diario': return d ? [...formAnotar(), ...con(columnas('d', 'hora', '', 'quién', 'qué hizo', 'trabajo', 'duró'), filasDiario(d)), ...pieDiario(d)] : [];
    case 'rutinas': return d ? [...(st.rutEd?.slug === '' ? [suelto(`<div class="det rnueva">${editorRutina()}</div>`, true)] : []),
      ...con(columnas('u', '', 'rutina', 'versión', 'qué hace', 'próxima', 'última', ''), filasRutinas(d))] : [];
    case 'propuestas': return d ? con(columnas('p', '', 'nº', 'qué propone', 'con qué', 'hace', 'tu decisión'), filasPropuestas(d)) : [];
    case 'ajustes': return d ? filasAjustes(d) : [];
    default: return st.estado ? [columnas('s', '', 'servicio o agente', 'qué hace', 'desde', ''), ...filasSistema(st.estado)] : [];
  }
}

// -------------------------------------------------------------------- pintado

// La cápsula de cada filtro de estado: el color solo la distingue de sus vecinas.
const FILTRO = { en_cola: COLOR.mision, ejecutando: COLOR.obra, en_revision: COLOR.obra, en_espera: COLOR.rutina, cerrado: COLOR.obra, cancelado: COLOR.mision };

// Los filtros del diario, que se aplican en el servidor. Los actores y las
// familias son los que escribe el código (spec 009, «Familias de eventos»).
const ACTORES = ['usuario', 'claude-code', 'modelo-local', 'orquestador', 'ea-cli'];
const FAMILIAS = { bus_: 'bus', mcp_: 'mcp', propuesta_: 'propuestas', accion_: 'acciones', boveda_: 'bóveda', memoria_: 'memoria', ajustes_: 'ajustes', usuario_: 'notas' };
const RANGOS = { hoy: 'hoy', ayer: 'ayer', '24h': '24 h', '7d': '7 días' };
const FECHA = /^\d{4}-\d{2}-\d{2}$/;

// desde/hasta en RFC 3339 para la API. Los rangos se resuelven en cada
// petición (la ventana avanza con el sondeo); las fechas son días locales y
// `hasta` incluye el suyo entero.
function ventanaDiario() {
  const dia = (s, mas = 0) => { const [a, m, d] = s.split('-').map(Number); return new Date(a, m - 1, d + mas); };
  const hoy = new Date(); hoy.setHours(0, 0, 0, 0);
  const ayer = new Date(hoy); ayer.setDate(ayer.getDate() - 1);
  const v = st.dDesde;
  let desde = null, hasta = null;
  if (v === 'hoy') desde = hoy;
  else if (v === 'ayer') { desde = ayer; hasta = new Date(hoy - 1); }
  else if (v === '24h') desde = new Date(Date.now() - 864e5);
  else if (v === '7d') desde = new Date(Date.now() - 7 * 864e5);
  else if (v && FECHA.test(v)) desde = dia(v);
  if (st.dHasta) hasta = new Date(dia(st.dHasta, 1) - 1);
  return { desde: desde?.toISOString() ?? null, hasta: hasta?.toISOString() ?? null };
}

// Paginación «↓ más antiguos» (spec 010). El sondeo solo pide la primera
// página; si ya se cargaron otras, la funde con ellas por id (sin duplicar).
// Unos filtros nuevos, o un hueco entre la primera página y lo cargado (más
// eventos nuevos que el límite, p. ej. con la pestaña oculta), vuelven a
// empezar por la primera página.
const ordenDiario = (a, b) => (ms(b.ts) - ms(a.ts)) || (b.id - a.id);
const firmaDiario = () => JSON.stringify([st.dActor, st.dResultado, st.dAccion, st.dTrabajo, st.dDesde, st.dHasta, st.latidos]);
const limiteDiario = () => st.diarioLimite ?? 300;
const filtrosDiario = () => ({
  limite: limiteDiario(), latidos: st.latidos,
  actor: st.dActor, resultado: st.dResultado, accion: st.dAccion, trabajo: st.dTrabajo, ...ventanaDiario(),
});

// `L`: el límite con el que se pidió `primera` (los ajustes pueden llegar a medias).
function fundeDiario(primera, L) {
  const firma = firmaDiario(), prev = st.dLista, ult = primera[primera.length - 1];
  const enlaza = st.dMas && prev?.length && (primera.length < L || ordenDiario(ult, prev[0]) >= 0);
  if (firma !== st.dFirma || !enlaza) {
    st.dFirma = firma;
    st.dMas = false;
    st.dFin = primera.length < L;
    st.dLista = primera;
  } else {
    const ids = new Set(primera.map(e => e.id));
    st.dLista = [...primera, ...prev.filter(e => !ids.has(e.id))].sort(ordenDiario);
  }
  return st.dLista;
}

// Pide la página anterior al evento más antiguo cargado, con los mismos filtros.
async function masDiario() {
  const lista = st.dLista ?? [], ult = lista[lista.length - 1];
  if (!ult || st.dFin || st.dCargando) return;
  const firma = st.dFirma, filtros = filtrosDiario();
  st.dCargando = true;
  st.html = '';
  pintar();
  try {
    const mas = await ea.diario({ ...filtros, antes_de: ult.id });
    if (firma !== st.dFirma) return; // cambiaron los filtros mientras tanto
    const ids = new Set();
    st.dLista = [...(st.dLista ?? []), ...lista, ...mas].filter(e => !ids.has(e.id) && ids.add(e.id)).sort(ordenDiario);
    st.dMas = true;
    st.dFin = mas.length < filtros.limite;
  } finally {
    st.dCargando = false;
  }
}

// El pie de la lista: el botón de la página siguiente, mientras quede algo.
function pieDiario(d) {
  if (st.dFin || !st.dLista?.length) return [];
  const nada = d.length ? '' : '<span class="m">nada de este proyecto en lo cargado</span>';
  return [suelto(`<div class="mas">${nada}${btn('d-mas', st.dCargando ? '… cargando' : '↓ más antiguos',
    'cargar los eventos anteriores, con los mismos filtros', {}, 'c')}</div>`, true)];
}

function barraDiario() {
  const opcion = (v, t, actual) => `<option value="${esc(v)}"${v === (actual ?? '') ? ' selected' : ''}>${esc(t)}</option>`;
  const sel = (campo, titulo, ops, actual) =>
    `<select class="c${actual ? ' on' : ''}" data-df="${campo}" title="${titulo}" aria-label="${titulo}">${ops.map(([v, t]) => opcion(v, t, actual)).join('')}</select>`;
  const actores = st.dActor && !ACTORES.includes(st.dActor) ? [...ACTORES, st.dActor] : ACTORES;
  const fechas = st.dFechas || FECHA.test(st.dDesde ?? '') || !!st.dHasta;
  const rango = fechas ? 'fechas' : st.dDesde;
  const res = (v, g, t) => `<button class="c${st.dResultado === v ? ' on' : ''}" data-a="d-resultado" data-v="${v}" title="${t}" aria-pressed="${st.dResultado === v}">${g}</button>`;
  let h = sel('actor', 'quién', [['', 'quién: todos'], ...actores.map(a => [a, a])], st.dActor) +
    res('ok', '✓', 'solo lo que salió bien') + res('error', '✕', 'solo los errores') +
    sel('accion', 'familia de eventos', [['', 'qué: todo'], ...Object.entries(FAMILIAS)], st.dAccion) +
    sel('rango', 'cuándo', [['', 'cuándo: siempre'], ...Object.entries(RANGOS), ['fechas', 'fechas…']], rango);
  if (fechas) {
    const d = FECHA.test(st.dDesde ?? '') ? st.dDesde : '';
    h += `<input type="date" class="c${d ? ' on' : ''}" data-df="desde" value="${esc(d)}" title="desde (incluido)" aria-label="Desde">` +
      `<input type="date" class="c${st.dHasta ? ' on' : ''}" data-df="hasta" value="${esc(st.dHasta ?? '')}" title="hasta (incluido)" aria-label="Hasta">`;
  }
  if (st.dTrabajo) h += `<button class="c on" data-a="d-trabajo" title="solo los eventos del trabajo ${esc(st.dTrabajo)}; pulsa para quitarlo" aria-pressed="true">▣ ${esc(st.dTrabajo.slice(-8))} ✕</button>`;
  h += `<button class="c${st.latidos ? ' on' : ''}" data-a="latidos" title="mostrar u ocultar los latidos del orquestador" aria-pressed="${st.latidos}">${st.latidos ? '♥ con latidos' : '♡ sin latidos'}</button>`;
  return h;
}

function barra() {
  const fb = $('#fb');
  let h = '';
  if (st.sec === 'trabajos') {
    // «＋ misión» va primero: los filtros pueden desbordar la barra.
    h = `<button class="c${st.nueva ? ' on' : ''}" data-a="nueva-mision" title="encargar una misión al modelo local" aria-pressed="${!!st.nueva}">＋ misión</button>` +
      Object.keys(EST).map(e =>
        `<button class="c${st.filtroEstado === e ? ' on' : ''}" data-f="${e}" title="${EST[e][1]}" aria-pressed="${st.filtroEstado === e}">` +
        `${capsula(FILTRO[e], IC[e] ?? '', 16)}${EST[e][1]} ${st.trabajos.filter(t => t.estado === e).length}</button>`).join('');
  } else if (st.sec === 'diario') {
    // «✎ anotar» va primero, como «＋ misión»: los filtros pueden desbordar.
    const on = !!st.anota && st.anota.corrige == null;
    h = `<button class="c${on ? ' on' : ''}" data-a="d-anotar" title="anotar en el diario: dejar constancia de algo con actor usuario" aria-pressed="${on}">✎ anotar</button>` +
      barraDiario();
  } else if (st.sec === 'rutinas') {
    const on = st.rutEd?.slug === '';
    h = `<button class="c${on ? ' on' : ''}" data-a="rnueva" title="rutina nueva: abre el editor con una plantilla TOML" aria-pressed="${on}">＋ rutina</button>`;
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
  const html = st.vis.length || st.sec === 'inicio' || vis.some(f => f.fijo) ? vis.map(f => f.html).join('') : `<p class="vacio">${vacio}</p>`;
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
  if (k[0] === 'r' && (st.rutVer[k.slice(1)]?.abierto || selVersion(k.slice(1)))) await cargaVersiones(k.slice(1)).catch(() => {});
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
  else if (sec === 'diario') {
    const filtros = filtrosDiario();
    datos = fundeDiario(await ea.diario(filtros), filtros.limite);
  }
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
  diario: {
    actor: texto('dActor', v => /^[a-z0-9_-]{1,64}$/.test(v)),
    resultado: texto('dResultado', v => v === 'ok' || v === 'error'),
    accion: texto('dAccion', v => /^[a-z_]{1,64}$/.test(v)),
    // Un rango (hoy, ayer, 24h, 7d) o un día; al llegar de un enlace, el
    // «fechas…» sin fecha que hubiera elegido se olvida.
    desde: { ...texto('dDesde'), lee: v => { st.dDesde = v && (Object.hasOwn(RANGOS, v) || FECHA.test(v)) ? v : null; st.dFechas = false; } },
    hasta: texto('dHasta', v => FECHA.test(v)),
    trabajo: texto('dTrabajo', v => /^[\w.-]{1,128}$/.test(v)),
    latidos: bandera('latidos'),
  },
  propuestas: { todas: bandera('propTodas') },
  // La versión elegida en la ficha enlazada (ver versionesHtml); diff=1, su
  // diff con la anterior. Solo se escribe si es de la fila del enlace.
  rutinas: {
    version: {
      lee: v => { st.rutVerSel = /^[1-9]\d{0,8}$/.test(v ?? '') ? { slug: null, n: Number(v), diff: false } : null; },
      escribe: () => { const s = st.rutVerSel, f = st.enlace?.startsWith('r') ? st.enlace.slice(1) : null; return s && f && selVersion(f) === s ? String(s.n) : null; },
    },
    diff: {
      lee: v => { if (st.rutVerSel) st.rutVerSel.diff = v === '1'; },
      escribe: () => { const s = st.rutVerSel, f = st.enlace?.startsWith('r') ? st.enlace.slice(1) : null; return s?.diff && f && selVersion(f) === s ? '1' : null; },
    },
  },
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
      const fase = a === 'reformular' ? $('[data-rff]', caja) : null;
      if (fase) cuerpo.fase = Number(fase.value);
      await ea.accionTrabajo(id, a, cuerpo);
      if (a === 'reformular') delete st.rfBorrador[id];
      const enCurso = a === 'cancelar' && st.trabajos.some(t => t.id === id && t.estado === 'ejecutando');
      avisa(`${id.slice(9, 15)}: ${a}${enCurso ? ' · ' + NO_SE_DESHACE : ''}`, true);
    } else if (a === 'clonar') {
      await clonaTrabajo(id);
      return;
    } else if (a === 'nueva-mision') {
      if (st.nueva) st.nueva = null;
      else {
        const form = await ea.nuevoTrabajo();
        st.nueva = { form, err: null, modo: 'mision', fases: [faseVacia()], v: { worktree: st.proyecto ?? '', iteraciones: form.defecto.iteraciones, minutos: Math.max(1, Math.round(form.defecto.tiempo_s / 60)) } };
      }
      st.html = '';
      pintar();
      $('[data-nm="titulo"]')?.focus();
      return;
    } else if (accionFase(a, a === 'nm-modo' ? v : n)) {
      return;
    } else if (a === 'crear-mision') {
      try { await creaMision(); } catch (e) { if (st.nueva) st.nueva.err = e.message; }
    } else if (a === 'diff') {
      if (st.diffs[id] !== undefined) delete st.diffs[id];
      else st.diffs[id] = await ea.diff(id).catch(e => ({ error: e.message }));
    } else if (a === 'diff-fase') {
      const k = id + ':' + v;
      if (st.faseDiff[k] !== undefined) delete st.faseDiff[k];
      else st.faseDiff[k] = await ea.diff(id, Number(v)).catch(e => ({ error: e.message }));
    } else if (a === 'plan-fase') {
      const k = id + ':' + v;
      if (st.fasePlan.has(k)) st.fasePlan.delete(k); else st.fasePlan.add(k);
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
    } else if (a === 'red' || a === 'rnueva') {
      abreEditorRutina(a === 'red' ? slug : '');
    } else if (a === 'rversiones') {
      const V = (st.rutVer[slug] ??= { abierto: false, lista: null, err: null, txt: {} });
      V.abierto = !V.abierto;
      if (!V.abierto && selVersion(slug)) { st.rutVerSel = null; escribeHash(false); }
      if (V.abierto) await cargaVersiones(slug);
    } else if (a === 'rver' || a === 'rvdiff') {
      // Otra vez sobre lo mismo lo pliega. Va al hash si es la fila del enlace.
      const diff = a === 'rvdiff', s = selVersion(slug);
      st.rutVerSel = s && s.n === Number(n) && s.diff === diff ? null : { slug, n: Number(n), diff };
      escribeHash(false);
      await cargaVersiones(slug);
    } else if (a === 'rdescartar') {
      st.rutEd = null;
    } else if (a === 'rvalidar' && st.rutEd) {
      const E = st.rutEd;
      E.val = E.err = E.armado = null;
      try {
        E.val = await ea.validarRutina(E.texto);
      } catch (e) {
        if (e.status === 403) throw e;
        E.err = e.message;
      }
    } else if (a === 'rregistrar' && st.rutEd) {
      // Segundo clic para confirmar, como aceptar una propuesta.
      const E = st.rutEd;
      if (!(E.armado && E.armado > Date.now())) {
        E.armado = Date.now() + 4000;
        st.html = '';
        pintar();
        setTimeout(() => { st.html = ''; pintar(); }, 4100);
        return;
      }
      E.armado = null;
      let r;
      try {
        r = await ea.registrarRutina(E.texto);
      } catch (e) {
        if (e.status === 403) throw e;
        E.val = null;
        E.err = e.message;
      }
      if (r) {
        st.rutEd = null;
        delete st.rutEnt[r.rutina];
        avisa(`✓ ${r.rutina}: registrada la versión ${r.version}`, true);
        st.abiertos.add('r' + r.rutina);
        st.sel = 'r' + r.rutina;
      }
    } else if (a === 'rhguardar' || a === 'rhquitar') {
      await accionHorario(a, slug, b);
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
    } else if (a === 'ir-diario') {
      ir('diario', { fila: null, params: { trabajo: id } });
      return;
    } else if (a === 'ir-prop') {
      st.propTodas = true;
      abre('propuestas', 'p' + id);
      return;
    } else if (a === 'ir-a') {
      irAFila(b.dataset.dsec, b.dataset.dfila);
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
    } else if (a === 'd-resultado') {
      st.dResultado = st.dResultado === v ? null : v;
    } else if (a === 'd-trabajo') {
      st.dTrabajo = null;
    } else if (a === 'd-mas') {
      await masDiario();
    } else if (a === 'd-anotar' || a === 'd-corregir') {
      // Anotar o corregir un evento: el mismo formulario, con `corrige` fijado.
      const corrige = a === 'd-corregir' ? Number(id) : null;
      if (st.anota && st.anota.corrige === corrige) st.anota = null;
      else st.anota = { corrige, err: null, v: { texto: '', trabajo: (a === 'd-corregir' ? v : st.dTrabajo) || '' } };
      st.html = '';
      pintar();
      $('[data-an="texto"]')?.focus();
      return;
    } else if (a === 'd-anotar-enviar') {
      try { await anotaDiario(); } catch (e) { if (st.anota) st.anota.err = e.message; }
    } else if (a === 'ir-evento') {
      ir('diario', { fila: String(id), params: {} });
      return;
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
  // El editor de una rutina: lo validado deja de valer al cambiar el texto.
  const rt = ev.target.closest('textarea[data-red]');
  if (rt && st.rutEd) {
    st.rutEd.texto = rt.value;
    st.rutEd.val = st.rutEd.err = st.rutEd.armado = null;
    st.html = '';
    $$('.sal', rt.closest('.det')).forEach(n => n.remove());
    return;
  }
  // El horario de una rutina: lo simulado deja de valer al cambiar el campo.
  const rh = ev.target.closest('[data-rhor]');
  if (rh) {
    const H = (st.rutHor[rh.dataset.rhor] ??= {});
    H.v = rh.value;
    H.sim = H.err = null;
    st.html = '';
    const caja = rh.closest('.hor');
    $$('.sal', caja).forEach(n => n.remove());
    const g = $('[data-a="rhguardar"]', caja);
    if (g) g.textContent = '◷ guardar';
    const q = $('[data-a="rhquitar"]', caja);
    if (q) q.textContent = '✕ desprogramar';
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
  // La nota y la fase de reformular un trabajo sobreviven al sondeo.
  const rf = ev.target.closest('[data-rf], [data-rff]');
  if (rf) {
    const bo = (st.rfBorrador[rf.dataset.rf ?? rf.dataset.rff] ??= {});
    if (rf.dataset.rf !== undefined) bo.nota = rf.value; else bo.fase = Number(rf.value);
    return;
  }
  // Lo escrito en el formulario «✎ anotar» del diario.
  const an = ev.target.closest('[data-an]');
  if (an && st.anota) { st.anota.v[an.dataset.an] = an.value; st.anota.err = null; return; }
  // Lo escrito en el formulario «＋ misión».
  const nm = ev.target.closest('[data-nm]');
  if (nm && st.nueva) { st.nueva.v[nm.dataset.nm] = nm.value; st.nueva.err = null; return; }
  // Lo escrito en una fase de la obra.
  const nf = ev.target.closest('[data-nf]');
  if (nf && st.nueva?.fases[nf.dataset.nf]) { st.nueva.fases[nf.dataset.nf][nf.dataset.nc] = nf.value; st.nueva.err = null; return; }
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

// Los desplegables y las fechas de la barra del diario: filtros del servidor.
$('#fb').addEventListener('change', ev => {
  const c = ev.target.closest('[data-df]');
  if (!c || st.sec !== 'diario') return;
  const v = c.value || null;
  if (c.dataset.df === 'actor') st.dActor = v;
  else if (c.dataset.df === 'accion') st.dAccion = v;
  else if (c.dataset.df === 'rango') {
    st.dFechas = v === 'fechas';
    st.dDesde = st.dFechas ? null : v;
    st.dHasta = null;
  } else if (c.dataset.df === 'desde') { st.dDesde = v; st.dFechas = true; }
  else if (c.dataset.df === 'hasta') { st.dHasta = v; st.dFechas = true; }
  c.blur();
  st.html = '';
  pintar();
  tick();
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
