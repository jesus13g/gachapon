'use strict';

const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];

const SECCIONES = ['inicio', 'trabajos', 'diario', 'rutinas', 'propuestas', 'acciones', 'sistema', 'boveda'];
// Qué filas de cada sección tienen un detalle que pedir al servidor (por prefijo de clave).
const DETALLE = { inicio: 'j', trabajos: 'j', rutinas: 'r', boveda: 'b' };
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
  boveda: ['Bóveda', 'Las notas de los agentes y las compartidas. El buscador busca dentro de su contenido.'],
};
const VACIO = {
  trabajos: 'No hay trabajos. Se lanzan desde Claude Code con /mision, /obra o /rutina.',
  diario: 'El diario está vacío.',
  rutinas: 'No hay rutinas registradas (ea rutina registrar).',
  propuestas: 'Nada espera tu decisión.',
  boveda: 'No hay notas.',
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
    html: `<article class="f${abierto ? ' o' : ''}${st.sel === k ? ' s' : ''}${extra}" data-k="${esc(k)}">` +
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
          `Lanzar ahora la rutina ${r.slug}: crea un trabajo; el orquestador lo recoge en segundos`, { slug: r.slug }, enCola ? 'coin ya' : 'coin'));
    const d = st.detalle[k];
    const det = d ? `<p class="m">versión ${d.version} · ${esc(r.worktree ?? '')}${r.criterio_exito ? ' · criterio: ' + esc(r.criterio_exito) : ''}</p><pre class="toml">${esc(d.contenido ?? '')}</pre>` : '';
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

function filasBoveda(d) {
  return d.notas.map(n => {
    const k = 'b' + n.ruta;
    const promovida = n.promocion === 'promovida';
    const row = `<i class="m" title="${esc(n.region)}">${n.region === 'compartida' ? '◆' : '◇'}</i>` +
      `<span class="x">${esc(n.titulo)}</span><span class="m w">${esc(n.ruta)}</span>` +
      `<span class="m">${esc(n.autor ?? '')}</span><span class="m">${reloj(n.actualizada)}</span>`;
    const x = st.detalle[k];
    let det = '';
    if (x) {
      const c = x.cabecera;
      det = (c ? `<p class="m">${esc(c.autor)} · ${esc(c.origen)} · ${promovida ? 'promovida' : 'sin promover'}` +
        `${c.tags.length ? ' · ' + esc(c.tags.join(', ')) : ''}${c.promovida_de ? ' · de ' + esc(c.promovida_de) : ''}` +
        `${c.promovida_a ? ' · a ' + esc(c.promovida_a) : ''}</p>` : `<p class="m">${esc(x.aviso_cabecera ?? '')}</p>`) +
        `<pre>${esc(x.contenido)}${x.truncado ? '\n…' : ''}</pre>`;
    }
    // Promover es decisión del usuario: segundo clic para confirmar.
    if (n.region !== 'compartida') {
      const armada = st.armado && st.armado.n === k && st.armado.hasta > Date.now();
      det += `<div class="acc">${btn('promover', armada ? '⇧ confirmar' : '⇧ promover a compartida', armada ? 'confirmar: copia a la región compartida' : 'promover a la región compartida', { ruta: n.ruta }, armada ? 'hot' : '')}</div>`;
    }
    return fila(k, 'b', row, `${n.titulo} ${n.ruta}`, det, '');
  });
}
function filasSistema(e, plano = false) {
  const out = [];
  e.servicios.forEach(s => {
    const activo = s.activo === 'active';
    const g = s.activo === 'desconocido' ? '◌' : activo || s.activo === 'failed' ? '●' : '○';
    const armado = st.armado && st.armado.n === s.nombre && st.armado.hasta > Date.now();
    const row = `<i class="${s.activo === 'failed' ? 'hot' : activo ? 'm' : 'n'}" title="${esc(s.activo)}">${g}</i>` +
      `<span class="x">${s.nombre}</span><span class="${s.activo === 'failed' ? 'hot' : 'm'}">${activo ? '' : esc(s.activo)}</span>` +
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
  return `<article class="card ${t.estado}${st.abiertos.has(k) ? ' o' : ''}${st.sel === k ? ' s' : ''}" data-k="${k}" title="pulsa para ver la ficha">` +
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
    case 'boveda': return d ? con(columnas('b', '', 'nota', 'ruta', 'autor', 'hace'), filasBoveda(d)) : [];
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
      `<button class="c${st.filtroEstado === e ? ' on' : ''}" data-f="${e}" title="${EST[e][1]}" aria-label="${EST[e][1]}">` +
      `${capsula(FILTRO[e], IC[e] ?? '', 16)}${EST[e][1]} ${st.trabajos.filter(t => t.estado === e).length}</button>`).join('');
  } else if (st.sec === 'diario') {
    h = `<button class="c${st.latidos ? ' on' : ''}" data-a="latidos" title="latidos" aria-label="Latidos">${st.latidos ? '♥ con latidos' : '♡ sin latidos'}</button>`;
  } else if (st.sec === 'propuestas') {
    h = `<button class="c${st.propTodas ? '' : ' on'}" data-a="prop" data-v="0" title="solo las que esperan tu decisión" aria-label="Pendientes">◇ pendientes</button>` +
        `<button class="c${st.propTodas ? ' on' : ''}" data-a="prop" data-v="1" title="también las ya decididas" aria-label="Todas">∗ todas</button>`;
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
    st.html = html;
    $('#filas').innerHTML = html; // la escena, encima, no se repinta
    lista.scrollTop = y;
  }
  relojes();
  barra();
}

// ---------------------------------------------------------------------- datos

async function cargaDetalle(k) {
  if (!(DETALLE[st.sec] || '').includes(k[0])) return;
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
  else if (sec === 'diario') datos = await ea.diario({ limite: 300, latidos: st.latidos });
  else if (sec === 'rutinas') datos = await ea.rutinas();
  else if (sec === 'propuestas') datos = await ea.propuestas({ pendientes: !st.propTodas });
  else if (sec === 'boveda') datos = await ea.boveda(st.q.trim());
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

// Lo que cambia con la sección fuera de la lista: título, guía y cuadros.
function marco() {
  const [nombre, guia] = TITULOS[st.sec];
  $('#titulo h1').textContent = nombre;
  $('#titulo p').textContent = guia;
  $('#q').placeholder = st.sec === 'boveda' ? 'buscar en las notas ( / )' : 'filtrar ( / )';
  $$('#rail button[data-s]').forEach(b => b.classList.toggle('on', b.dataset.s === st.sec));
  $('#barra').hidden = st.sec === 'inicio'; // la portada no es una lista que filtrar
  $('#escena').hidden = st.sec !== 'inicio';
}

function ir(sec) {
  st.sec = sec;
  st.datos = null;
  st.html = '';
  st.sel = null;
  st.q = '';
  $('#q').value = '';
  location.hash = sec;
  marco();
  $('#lista').scrollTop = 0;
  pintar();
  tick();
}

async function alterna(k) {
  if (st.abiertos.has(k)) st.abiertos.delete(k);
  else { st.abiertos.add(k); await cargaDetalle(k); }
  pintar();
  // Desde una tarjeta del tablero la ficha se abre más abajo: que se vea.
  if (st.abiertos.has(k)) $(`.f[data-k="${CSS.escape(k)}"]`)?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
}

function mover(d) {
  if (!st.vis.length) return;
  const i = st.vis.indexOf(st.sel);
  st.sel = st.vis[Math.min(st.vis.length - 1, Math.max(0, i < 0 ? (d > 0 ? 0 : st.vis.length - 1) : i + d))];
  pintar();
  $(`.f[data-k="${CSS.escape(st.sel)}"]`)?.scrollIntoView({ block: 'nearest' });
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
  ir(sec);
  st.abiertos.add(k);
  st.sel = k;
}


async function accion(b) {
  const { a, id, v, slug, n, ruta, h } = b.dataset;
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
    } else if (a === 'promover') {
      const k = 'b' + ruta;
      if (!(st.armado && st.armado.n === k && st.armado.hasta > Date.now())) {
        st.armado = { n: k, hasta: Date.now() + 4000 };
        pintar();
        setTimeout(pintar, 4100);
        return;
      }
      st.armado = null;
      const r = await ea.promover(ruta);
      avisa(r.texto, true);
    } else if (a === 'lanzar') {
      const t = await ea.lanzarRutina(slug);
      avisa(`✓ ${slug}: cápsula ${t.trabajo.slice(9, 15)} en la cola del modelo local`, true);
      st.abiertos.add('j' + t.trabajo);
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
  b.innerHTML = (claro ? SOL : LUNA) + (claro ? 'claro' : 'oscuro');
  b.title = claro ? 'Modo claro (pulsa para oscuro)' : 'Modo oscuro (pulsa para claro)';
  b.setAttribute('aria-label', b.title);
}

function ayuda(mostrar) {
  $('#ayuda').hidden = !mostrar;
  try { localStorage.setItem('ea-ayuda-vista', '1'); } catch { /* sin almacenamiento */ }
}

// -------------------------------------------------------------------- eventos

document.addEventListener('click', ev => {
  const t = ev.target;
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
  if (t.closest('input, select, textarea, .acc')) return;
  const fila = t.closest('#lista [data-k]');
  if (fila) { st.sel = fila.dataset.k; alterna(fila.dataset.k); }
});

// La consola de acciones guarda lo escrito en el estado (la lista se repinta
// entera) y da por caducada la simulación en cuanto cambia un argumento.
$('#lista').addEventListener('input', ev => {
  const el = ev.target.closest('[data-campo]');
  if (!el) return;
  const c = (st.consola[el.dataset.h] ??= {});
  (c.v ??= {})[el.dataset.campo] = el.value;
  c.sim = c.res = c.err = null;
  st.html = '';
  $$('.sal', el.closest('.det')).forEach(n => n.remove());
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
  if (ev.key === 'Escape') {
    if (t.id === 'q') { t.value = ''; st.q = ''; }
    if (enCampo) t.blur();
    else { st.abiertos.clear(); st.sel = null; }
    st.html = '';
    pintar();
    return;
  }
  if (ev.key === 'Enter' && t.matches('input[data-campo]')) {
    ev.preventDefault();
    accion({ dataset: { a: 'simular', h: t.dataset.h } });
    return;
  }
  if (enCampo || ev.ctrlKey || ev.metaKey || ev.altKey) return;
  if ((ev.key === 'Enter' || ev.key === ' ') && t.matches('[data-pt]')) { ev.preventDefault(); abre('trabajos', 'j' + t.dataset.pt); return; }
  const n = Number(ev.key);
  if (n >= 1 && n <= SECCIONES.length) { ir(SECCIONES[n - 1]); return; }
  if (ev.key === 'p') proyectos(true);
  else if (ev.key === '?') { ev.preventDefault(); ayuda($('#ayuda').hidden); }
  else if (ev.key === 't') tour.iniciar();
  else if (ev.key === '/') { ev.preventDefault(); $('#q').focus(); }
  else if (ev.key === 'j') mover(1);
  else if (ev.key === 'k') mover(-1);
  else if ((ev.key === 'Enter' || ev.key === ' ') && st.sel && !t.closest('button')) { ev.preventDefault(); alterna(st.sel); }
});

document.addEventListener('visibilitychange', tick);
addEventListener('hashchange', () => {
  const h = location.hash.slice(1);
  if (SECCIONES.includes(h) && h !== st.sec) ir(h);
});

// -------------------------------------------------------------------- arranque

{
  const h = location.hash.slice(1);
  if (SECCIONES.includes(h)) st.sec = h;
  gasha.montar($('#escena'), { alAbrir: k => { st.sel = k; alterna(k); } });
  tour.montar({ ir, seccion: () => st.sec });
  marco();
  tema();
  proyectos();
  // La primera vez, el recorrido guiado.
  try { if (!localStorage.getItem('ea-tour-visto')) tour.iniciar(); } catch { /* sin almacenamiento */ }
  tick();
  setInterval(tick, 3000);
  setInterval(relojes, 1000);
}
