'use strict';

const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];

const SECCIONES = ['tareas', 'diario', 'rutinas', 'propuestas', 'sistema'];
const SIM = { pendiente: '○', en_progreso: '◐', completada: '●', fallida: '✕', cancelada: '⊘' };
const ORIGEN = { manual: '✎', boton: '▪', voz: '◉', programado: '↻', agente: '◈' };
const PASO = { accion: '▸', decision: '¿', propuesta: '◇', fin: '■' };
const ACCIONES = {
  pendiente: [['en_progreso', '▶', 'iniciar'], ['cancelada', '⊘', 'cancelar']],
  en_progreso: [['completada', '✓', 'completar'], ['fallida', '✕', 'fallar'], ['cancelada', '⊘', 'cancelar']],
};

const st = {
  sec: 'tareas',
  estado: null,        // /api/estado
  datos: null,         // datos de la sección visible
  detalle: {},         // detalle cargado de las filas abiertas
  abiertos: new Set(), // claves abiertas (t41, r<slug>, …)
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
  voz: false,
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

function hora(t) {
  const d = new Date(ms(t));
  const h = `${p2(d.getHours())}:${p2(d.getMinutes())}:${p2(d.getSeconds())}`;
  return d.toDateString() === new Date().toDateString() ? h : `${p2(d.getDate())}/${p2(d.getMonth() + 1)} ${h.slice(0, 5)}`;
}

const dur = n => (n < 1000 ? n + 'ms' : n < 60000 ? (n / 1000).toFixed(1) + 's' : Math.round(n / 60000) + 'm');

async function api(ruta, opts = {}) {
  const r = await fetch(ruta, { headers: { 'Content-Type': 'application/json' }, ...opts });
  const txt = await r.text();
  let data = null;
  try { data = JSON.parse(txt); } catch { /* cuerpo no JSON */ }
  if (!r.ok) throw new Error((data && data.error) || txt || r.statusText);
  return data;
}
const post = (ruta, cuerpo = {}) => api(ruta, { method: 'POST', body: JSON.stringify(cuerpo) });

let avisoTimer;
function avisa(msg, ok = false) {
  const a = $('#aviso');
  a.textContent = (ok ? '' : '✕ ') + msg;
  a.className = 'v' + (ok ? ' ok' : '');
  clearTimeout(avisoTimer);
  avisoTimer = setTimeout(() => { a.className = ''; }, 5000);
}

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

function eventos(lista) {
  return lista.map(e => `<div class="ev"><span>${hora(e.ts)}</span>` +
    `<i class="${e.resultado === 'error' ? 'hot' : ''}">${e.resultado === 'ok' ? '✓' : e.resultado === 'error' ? '✕' : '·'}</i>` +
    `<span>${esc(e.actor)}</span><span>${esc(e.accion)}</span><span>${e.duracion_ms != null ? dur(e.duracion_ms) : ''}</span></div>`).join('');
}

function filasTareas(tareas) {
  return tareas.map(t => {
    const k = 't' + t.id;
    const row = `<i class="e ${t.estado}" title="${t.estado}">${SIM[t.estado]}</i>` +
      `<i class="m" title="${t.origen}">${ORIGEN[t.origen] ?? ''}</i><span class="n">#${t.id}</span>` +
      `<span class="x">${esc(t.titulo)}</span><span class="m w">${esc(t.asignado_a)}</span><span class="m">${hace(t.actualizada)}</span>`;
    const d = st.detalle[k];
    const acc = (ACCIONES[t.estado] || []).map(([v, g, tt]) => btn('estado', g, tt, { id: t.id, v }, v === 'fallida' ? 'hot' : '')).join('');
    const det = `<p class="m">${esc(t.creado_por)} → ${esc(t.asignado_a)} · ${hora(t.creada)}</p>` +
      `<p class="tx">» ${esc(t.peticion)}</p>` +
      (t.resultado ? `<p class="tx">« ${esc(t.resultado)}</p>` : '') +
      (t.padre ? `<p>↳ ${btn('ir-tarea', '#' + t.padre, 'tarea padre', { id: t.padre }, 'l')}</p>` : '') +
      `<div class="evs">${d ? eventos(d.eventos) : ''}</div>` +
      (acc ? `<div class="acc">${t.estado === 'en_progreso' ? '<input class="res" placeholder="«" aria-label="Resultado">' : ''}${acc}</div>` : '');
    return fila(k, 't', row, `#${t.id} ${t.titulo} ${t.asignado_a} ${t.estado} ${t.origen}`, det,
      t.estado === 'completada' || t.estado === 'cancelada' ? ' fin' : '');
  });
}

function pretty(txt) {
  if (!txt) return '';
  try { return JSON.stringify(JSON.parse(txt), null, 1); } catch { return txt; }
}

function filasDiario(evs) {
  return evs.map(e => {
    const k = 'e' + e.id;
    const row = `<span class="m">${hora(e.ts)}</span>` +
      `<i class="${e.resultado === 'error' ? 'hot' : 'm'}">${e.resultado === 'ok' ? '✓' : e.resultado === 'error' ? '✕' : '·'}</i>` +
      `<span class="m w">${esc(e.actor)}</span><span class="x">${esc(e.accion)}</span>` +
      `<span class="n">${e.tarea != null ? '#' + e.tarea : ''}</span><span class="m">${e.duracion_ms != null ? dur(e.duracion_ms) : ''}</span>`;
    const det = `<p class="m">#${e.id} · ${esc(e.actor)} · ${new Date(e.ts).toLocaleString()}</p>` +
      (e.detalle ? `<pre>${esc(pretty(e.detalle))}</pre>` : '') +
      (e.tarea != null ? `<p>${btn('ir-tarea', '→ #' + e.tarea, 'ver tarea', { id: e.tarea }, 'l')}</p>` : '');
    return fila(k, 'd', row, `${e.actor} ${e.accion} ${e.tarea != null ? '#' + e.tarea : ''} ${e.detalle ?? ''}`, det);
  });
}

function filasRutinas(rs) {
  return rs.map(r => {
    const k = 'r' + r.slug;
    const u = r.ultima;
    const ultima = u ? `<i class="${u.resultado === 'fallida' ? 'hot' : ''}">${u.resultado === 'ok' ? '●' : u.resultado === 'fallida' ? '✕' : '◐'}</i> ${hace(u.inicio)}` : '—';
    const row = `<i>↻</i><span class="x">${esc(r.slug)}</span>` +
      `<span class="m ${r.error ? 'hot' : ''}">${esc(r.descripcion ?? r.error ?? '')}</span>` +
      `<span class="m">${ultima}</span><span class="m">${r.proxima ? en(r.proxima) : ''}</span>`;
    const d = st.detalle[k];
    let det = '';
    if (d) {
      det = `<p class="m" title="tiempo · pasos · llamadas al modelo">◷ ${d.max_duracion_s}s · ▸ ${d.max_pasos} · ¿ ${d.max_llamadas_modelo}</p>` +
        d.pasos.map(p => {
          let info = '';
          if (p.tipo === 'accion') info = `${p.herramienta} ${JSON.stringify(p.argumentos ?? {})}`;
          else if (p.tipo === 'decision') info = `${p.contexto.split('\n')[0]} [${Object.keys(p.ramas).join(' | ')}]`;
          else if (p.tipo === 'propuesta') info = p.descripcion;
          return `<div class="pa"><i>${PASO[p.tipo]}</i><span class="x">${esc(p.id)}</span><span class="m">${esc(info)}</span>` +
            `<span class="m">${p.destinos.length ? '→ ' + esc(p.destinos.join(', ')) : ''}</span></div>`;
        }).join('') +
        (d.ejecuciones.length ? `<div class="sep">↻</div>` + d.ejecuciones.map(x =>
          `<div class="ev"><span>${hora(x.inicio)}</span><i class="${x.resultado === 'fallida' ? 'hot' : ''}">${x.resultado === 'ok' ? '✓' : x.resultado === 'fallida' ? '✕' : '◐'}</i>` +
          `<span>▸ ${x.pasos} · ¿ ${x.llamadas_modelo}</span><span>${esc(x.detalle ?? '')}</span><span></span></div>`).join('') : '');
    }
    det += `<div class="acc">${btn('encolar', '▶', 'encolar', { slug: r.slug })}</div>`;
    return fila(k, 'u', row, `${r.slug} ${r.descripcion ?? ''}`, det);
  });
}

function filasPropuestas(ps) {
  return ps.map(p => {
    const k = 'p' + p.id;
    const pend = p.estado === 'pendiente';
    const glifo = pend ? '◇' : p.estado === 'aceptada' ? '✓' : '✕';
    const acc = pend
      ? btn('decidir', '✓', 'aceptar', { id: p.id, v: 'aceptar' }) + btn('decidir', '✕', 'rechazar', { id: p.id, v: 'rechazar' }, 'hot')
      : '';
    const row = `<i class="${pend ? 'hot' : 'm'}" title="${p.estado}">${glifo}</i><span class="n">#${p.id}</span>` +
      `<span class="x">${esc(p.descripcion)}</span><span class="m w">${esc(p.rutina ?? '')}</span><span class="m">${hace(p.creada)}</span><span>${acc}</span>`;
    const det = `<p class="tx">${esc(p.descripcion)}</p>` +
      `<p class="m">${esc(p.rutina ?? '')} · ejecución #${p.ejecucion}${p.decidida ? ' · ' + hora(p.decidida) : ''}</p>`;
    return fila(k, 'p', row, `${p.descripcion} ${p.rutina ?? ''} ${p.estado}`, det, pend ? '' : ' fin');
  });
}

function filasSistema(e) {
  const out = [];
  e.servicios.forEach(s => {
    const activo = s.activo === 'active';
    const g = s.activo === 'desconocido' ? '◌' : activo || s.activo === 'failed' ? '●' : '○';
    const armado = st.armado && st.armado.n === s.nombre && st.armado.hasta > Date.now();
    const row = `<i class="${s.activo === 'failed' ? 'hot' : activo ? 'm' : 'n'}" title="${esc(s.activo)}">${g}</i>` +
      `<span class="x">${s.nombre}</span><span class="${s.activo === 'failed' ? 'hot' : 'm'}">${activo ? '' : esc(s.activo)}</span>` +
      `<span class="m">${s.activo_s != null ? unidades(s.activo_s) : ''}</span>` +
      btn('reiniciar', armado ? '↻?' : '↻', armado ? 'confirmar reinicio' : 'reiniciar', { n: s.nombre }, armado ? 'hot' : '');
    out.push(fila('s' + s.nombre, 's', row, s.nombre + ' ' + s.activo));
  });
  const o = e.orquestador;
  out.push(fila('sorq', 's', `<i class="${o.vivo ? 'm' : 'hot'}">♥</i><span class="x">orquestador</span><span class="m"></span><span class="m">${hace(o.ultimo_latido)}</span><span></span>`, 'orquestador latido', '', ' g'));
  e.agentes.forEach((a, i) => {
    const tarea = a.tarea ? `<span class="x" title="${esc(a.tarea.titulo)}">#${a.tarea.id} ${esc(a.tarea.titulo)}</span>` : '<span class="m">—</span>';
    const row = `<i class="${a.tarea ? '' : 'n'}">◈</i><span class="x">${a.nombre}</span>${tarea}` +
      `<span class="m" title="${esc(a.ultimo?.accion ?? '')}">${a.ultimo ? hace(a.ultimo.ts) : ''}</span><span></span>`;
    out.push(fila('s' + a.nombre, 's', row, a.nombre + ' ' + (a.tarea?.titulo ?? ''), '', i === 0 ? ' g' : ''));
  });
  return out;
}

function construir() {
  const d = st.datos;
  switch (st.sec) {
    case 'tareas': return d ? filasTareas(d) : [];
    case 'diario': return d ? filasDiario(d) : [];
    case 'rutinas': return d ? filasRutinas(d) : [];
    case 'propuestas': return d ? filasPropuestas(d) : [];
    default: return st.estado ? filasSistema(st.estado) : [];
  }
}

// -------------------------------------------------------------------- pintado

function barra() {
  const fb = $('#fb');
  let h = '';
  if (st.sec === 'tareas' && st.estado) {
    h = Object.keys(SIM).map(e =>
      `<button class="c${st.filtroEstado === e ? ' on' : ''}" data-f="${e}" title="${e}" aria-label="${e}">` +
      `<i class="e ${e}">${SIM[e]}</i>${st.estado.tareas[e] || ''}</button>`).join('');
  } else if (st.sec === 'diario') {
    h = `<button class="c${st.latidos ? ' on' : ''}" data-a="latidos" title="latidos" aria-label="Latidos">${st.latidos ? '♥' : '♡'}</button>`;
  } else if (st.sec === 'propuestas') {
    h = `<button class="c${st.propTodas ? '' : ' on'}" data-a="prop" data-v="0" title="pendientes" aria-label="Pendientes">◇</button>` +
        `<button class="c${st.propTodas ? ' on' : ''}" data-a="prop" data-v="1" title="todas" aria-label="Todas">∗</button>`;
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
  const o = $('#orq');
  o.className = e.orquestador.vivo ? '' : 'hot';
  o.title = e.orquestador.ultimo_latido ? 'latido hace ' + hace(e.orquestador.ultimo_latido) : 'sin latido';

  const activas = (e.tareas.pendiente || 0) + (e.tareas.en_progreso || 0);
  const bt = $('[data-s=tareas] b'), bp = $('[data-s=propuestas] b');
  bt.textContent = activas || '';
  bp.textContent = e.propuestas_pendientes || '';
  bp.className = 'hot';
  document.title = e.propuestas_pendientes ? `◇ ${e.propuestas_pendientes}` : 'ea';
}

function pintar() {
  // No pisar lo que se está escribiendo dentro de un detalle.
  const foco = document.activeElement;
  if (foco && foco.matches('input') && foco.closest('#lista')) return;

  if (!st.datos && st.sec !== 'sistema') {
    st.vis = [];
    st.html = '';
    $('#lista').innerHTML = '';
    barra();
    return;
  }

  const q = st.q.trim().toLowerCase();
  const todas = construir();
  const vis = q ? todas.filter(f => f.txt.includes(q)) : todas;
  st.vis = vis.map(f => f.k);
  if (st.sel && !st.vis.includes(st.sel)) st.sel = null;

  const html = vis.length ? vis.map(f => f.html).join('') : '<p class="vacio">∅</p>';
  if (html !== st.html) {
    const lista = $('#lista');
    const y = lista.scrollTop;
    st.html = html;
    lista.innerHTML = html;
    lista.scrollTop = y;
  }
  barra();
}

// ---------------------------------------------------------------------- datos

async function cargaDetalle(k) {
  const prefijo = { tareas: 't', rutinas: 'r' }[st.sec];
  if (!prefijo || k[0] !== prefijo) return;
  const ruta = prefijo === 't' ? '/api/tareas/' : '/api/rutinas/';
  try { st.detalle[k] = await api(ruta + encodeURIComponent(k.slice(1))); } catch { delete st.detalle[k]; }
}

async function cargaSeccion() {
  const sec = st.sec;
  let datos = null;
  if (sec === 'tareas') datos = await api('/api/tareas?limite=200' + (st.filtroEstado ? '&estado=' + st.filtroEstado : ''));
  else if (sec === 'diario') datos = await api('/api/diario?limite=300' + (st.latidos ? '&latidos=1' : ''));
  else if (sec === 'rutinas') datos = await api('/api/rutinas');
  else if (sec === 'propuestas') datos = await api('/api/propuestas' + (st.propTodas ? '' : '?estado=pendiente'));
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
    st.estado = await api('/api/estado');
    $('#off').hidden = true;
    cabecera();
    if (await cargaSeccion()) pintar();
  } catch {
    $('#off').hidden = false;
  } finally {
    st.ocupado = false;
    if (st.otra) { st.otra = false; tick(); }
  }
}

// ------------------------------------------------------------------ acciones

function ir(sec) {
  st.sec = sec;
  st.datos = null;
  st.html = '';
  st.sel = null;
  st.q = '';
  $('#q').value = '';
  location.hash = sec;
  $$('#rail button').forEach(b => b.classList.toggle('on', b.dataset.s === sec));
  $('#lista').scrollTop = 0;
  pintar();
  tick();
}

async function alterna(k) {
  if (st.abiertos.has(k)) st.abiertos.delete(k);
  else { st.abiertos.add(k); await cargaDetalle(k); }
  pintar();
}

function mover(d) {
  if (!st.vis.length) return;
  const i = st.vis.indexOf(st.sel);
  st.sel = st.vis[Math.min(st.vis.length - 1, Math.max(0, i < 0 ? (d > 0 ? 0 : st.vis.length - 1) : i + d))];
  pintar();
  $(`.f[data-k="${CSS.escape(st.sel)}"]`)?.scrollIntoView({ block: 'nearest' });
}

async function accion(b) {
  const { a, id, v, slug, n } = b.dataset;
  try {
    if (a === 'estado') {
      const res = $('.res', b.closest('.f'))?.value.trim();
      await post(`/api/tareas/${id}/estado`, { estado: v, resultado: res || null });
    } else if (a === 'decidir') {
      await post(`/api/propuestas/${id}/decidir`, { decision: v });
    } else if (a === 'encolar') {
      const t = await post(`/api/rutinas/${encodeURIComponent(slug)}/encolar`);
      avisa('▶ #' + t.id, true);
    } else if (a === 'reiniciar') {
      if (st.armado && st.armado.n === n && st.armado.hasta > Date.now()) {
        st.armado = null;
        await post(`/api/servicios/${n}/reiniciar`);
        avisa('↻ ' + n, true);
      } else {
        st.armado = { n, hasta: Date.now() + 3000 };
        pintar();
        setTimeout(pintar, 3100);
        return;
      }
    } else if (a === 'ir-tarea') {
      st.filtroEstado = null;
      ir('tareas');
      st.abiertos.add('t' + id);
      st.sel = 't' + id;
      return;
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

async function voz() {
  if (st.voz) return;
  st.voz = true;
  const bv = $('#voz'), eco = $('#eco');
  bv.classList.add('escuchando');
  eco.hidden = true;
  try {
    const r = await post('/api/voz/hablar');
    const lineas = r.aviso ? [r.aviso] : [`» ${r.transcripcion ?? ''}`, `« ${r.respuesta ?? ''}`];
    eco.innerHTML = lineas.map(l => `<p>${esc(l)}</p>`).join('');
    eco.hidden = false;
    setTimeout(() => { eco.hidden = true; }, 15000);
  } catch (e) {
    avisa(e.message);
  } finally {
    st.voz = false;
    bv.classList.remove('escuchando');
    tick();
  }
}

function formulario(mostrar) {
  const f = $('#form');
  f.hidden = !mostrar;
  if (mostrar) f.elements.titulo.focus();
}

// -------------------------------------------------------------------- eventos

document.addEventListener('click', ev => {
  const t = ev.target;
  const rail = t.closest('#rail button');
  if (rail) return ir(rail.dataset.s);
  if (t.closest('#voz')) return voz();
  if (t.closest('#nueva')) return formulario($('#form').hidden);
  const f = t.closest('button[data-f]');
  if (f) {
    st.filtroEstado = st.filtroEstado === f.dataset.f ? null : f.dataset.f;
    st.html = '';
    return tick();
  }
  const b = t.closest('button[data-a]');
  if (b) return accion(b);
  if (t.closest('input')) return;
  const fila = t.closest('#lista .f');
  if (fila) { st.sel = fila.dataset.k; alterna(fila.dataset.k); }
});

$('#form').addEventListener('submit', async ev => {
  ev.preventDefault();
  const f = ev.target.elements;
  try {
    const t = await post('/api/tareas', { titulo: f.titulo.value, asignado_a: f.asignado_a.value, solicita: f.solicita.value });
    f.titulo.value = '';
    f.solicita.value = '';
    formulario(false);
    avisa('+ #' + t.id, true);
    st.filtroEstado = null;
    if (st.sec !== 'tareas') ir('tareas'); else { st.html = ''; tick(); }
    st.sel = 't' + t.id;
  } catch (e) {
    avisa(e.message);
  }
});

$('#q').addEventListener('input', ev => { st.q = ev.target.value; st.html = ''; pintar(); });

document.addEventListener('keydown', ev => {
  const t = ev.target;
  const enCampo = t.matches('input, textarea, select');
  if (ev.key === 'Escape') {
    if (t.id === 'q') { t.value = ''; st.q = ''; }
    if (enCampo) { t.blur(); if (t.closest('#form')) formulario(false); }
    else { st.abiertos.clear(); st.sel = null; }
    st.html = '';
    pintar();
    return;
  }
  if (enCampo || ev.ctrlKey || ev.metaKey || ev.altKey) return;
  const n = Number(ev.key);
  if (n >= 1 && n <= SECCIONES.length) { ir(SECCIONES[n - 1]); return; }
  if (ev.key === 'n') { ev.preventDefault(); formulario(true); }
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
  $$('#rail button').forEach(b => b.classList.toggle('on', b.dataset.s === st.sec));
  tick();
  setInterval(tick, 3000);
}
