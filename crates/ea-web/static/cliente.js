// Cliente de la API de ea-web: el único sitio de la página que conoce las
// rutas HTTP. Cada función devuelve el JSON de la respuesta o lanza un
// Error con el mensaje del servidor.

//
// Clave de sesión: `ea-web-open` abre la página con `#clave=<clave>`. El hash
// son partes separadas por `&` (`clave=…` y el enlace profundo: sección,
// fila y filtros, ver enlaces.js): aquí se saca la clave, se guarda en sessionStorage y se
// deja el resto del hash tal cual con history.replaceState, antes de que
// app.js lo lea. Toda petición que no sea GET lleva la clave en X-EA-Clave;
// sin ella (o si el servidor la rechaza) el panel queda en solo lectura y las
// escrituras fallan con el aviso, sin llegar al servidor.

const ea = (() => {
  const AVISO_LECTURA = 'Panel en solo lectura: ábrelo con ea-web-open para poder cambiar nada.';
  let clave = null;
  try { clave = sessionStorage.getItem('ea-clave'); } catch { /* sin almacenamiento */ }
  const oyentes = [];
  function recogeClave() {
    const partes = location.hash.slice(1).split('&').filter(Boolean);
    const resto = partes.filter(p => !p.startsWith('clave='));
    if (resto.length !== partes.length) {
      const nueva = decodeURIComponent(partes.find(p => p.startsWith('clave=')).slice(6));
      if (nueva) {
        clave = nueva;
        try { sessionStorage.setItem('ea-clave', clave); } catch { /* sin almacenamiento: vale para esta carga */ }
        oyentes.forEach(f => f(true));
      }
      history.replaceState(null, '', location.pathname + location.search + (resto.length ? '#' + resto.join('&') : ''));
    }
  }
  recogeClave();
  // También si la clave llega a una página ya abierta (se pega en la barra de
  // direcciones): estos oyentes van antes que los de app.js, que ya ven el
  // hash sin ella.
  addEventListener('popstate', recogeClave);
  addEventListener('hashchange', recogeClave);
  function soloLectura() {
    clave = null;
    try { sessionStorage.removeItem('ea-clave'); } catch { /* sin almacenamiento */ }
    oyentes.forEach(f => f(false));
  }

  async function api(ruta, opts = {}) {
    const escribe = opts.method && opts.method !== 'GET';
    if (escribe && !clave) throw Object.assign(new Error(AVISO_LECTURA), { status: 403 });
    const headers = { 'Content-Type': 'application/json' };
    if (clave) headers['X-EA-Clave'] = clave;
    const r = await fetch(ruta, { ...opts, headers });
    const txt = await r.text();
    let data = null;
    try { data = JSON.parse(txt); } catch { /* cuerpo no JSON */ }
    if (r.status === 403 && r.headers.get('X-EA-Clave') === 'requerida') soloLectura();
    if (!r.ok) throw Object.assign(new Error((data && data.error) || txt || r.statusText), { status: r.status });
    return data;
  }
  const post = (ruta, cuerpo = {}) => api(ruta, { method: 'POST', body: JSON.stringify(cuerpo) });
  const c = encodeURIComponent;

  return {
    // Sesión: `escritura()` dice si hay clave (sin preguntar); `sesion()`
    // pregunta al servidor si vale y, si no, pasa a solo lectura;
    // `alCambiarSesion(f)` avisa con f(false) al perder la clave y con
    // f(true) al recibir una nueva en una página ya abierta.
    escritura: () => !!clave,
    sesion: async () => {
      if (!clave) return false;
      const { escritura } = await api('/api/sesion');
      if (!escritura) soloLectura();
      return escritura;
    },
    alCambiarSesion: f => oyentes.push(f),
    avisoLectura: AVISO_LECTURA,

    estado: () => api('/api/estado'),
    // Catálogo de agentes: rol, binario, modelo y si responde (spec 017).
    agentes: () => api('/api/agentes'),
    sesiones: () => api('/api/sesiones'),
    // Los agentes vivos y dónde está cada trabajo activo (la escena de Inicio).
    ecosistema: () => api('/api/ecosistema'),

    trabajos: () => api('/api/trabajos'),
    trabajo: id => api('/api/trabajos/' + c(id)),
    // Con `fase`, el diff de esa fase de una obra (entre sus instantáneas).
    diff: (id, fase) => api(`/api/trabajos/${c(id)}/diff${fase ? '?fase=' + Number(fase) : ''}`),
    // accion: cerrar, cancelar, continuar, reformular ({ nota, fase? }: fase,
    // solo en obras, desde la que se rehace) o confirmar ({ contexto }).
    accionTrabajo: (id, accion, cuerpo = {}) => post(`/api/trabajos/${c(id)}/${accion}`, cuerpo),
    // Crear una misión: lo que pide el formulario (techo, topes por defecto y
    // worktrees sugeridos) y el alta ({ modo, titulo, worktree, objetivo, criterio, topes }).
    nuevoTrabajo: () => api('/api/trabajos/nuevo'),
    crearTrabajo: cuerpo => post('/api/trabajos', cuerpo),
    colas: () => api('/api/colas'),
    semaforos: () => api('/api/semaforos'),

    // Filtros del servidor: actor, resultado (ok | error), accion (prefijo,
    // [a-z_]+), desde y hasta (RFC 3339 o AAAA-MM-DD) y trabajo; los vacíos
    // no se envían.
    diario: ({ limite, latidos = false, ...filtros } = {}) => {
      const q = new URLSearchParams({ limite });
      if (latidos) q.set('latidos', '1');
      for (const [k, v] of Object.entries(filtros)) if (v) q.set(k, v);
      return api('/api/diario?' + q);
    },
    // Una nota del usuario ({ texto, corrige?, trabajo? }): deja constancia de
    // algo o corrige un evento sin tocarlo (el diario es append-only).
    anotarDiario: cuerpo => post('/api/diario/anotar', cuerpo),

    // Con `retiradas`, las de la papelera (o sin fichero) en vez de las activas.
    rutinas: (retiradas = false) => api('/api/rutinas' + (retiradas ? '?retiradas=1' : '')),
    rutina: slug => api('/api/rutinas/' + c(slug)),
    lanzarRutina: (slug, entradas = {}) => post(`/api/rutinas/${c(slug)}/lanzar`, { entradas }),
    previaRutina: (slug, entradas = {}) => post(`/api/rutinas/${c(slug)}/previa`, { entradas }),
    // Las versiones registradas (sin definición) y una con su TOML.
    versionesRutina: slug => api(`/api/rutinas/${c(slug)}/versiones`),
    versionRutina: (slug, n) => api(`/api/rutinas/${c(slug)}/versiones/${c(n)}`),
    // Definir o corregir una rutina: validar no escribe nada; registrar, sí.
    validarRutina: toml => post('/api/rutinas/validar', { toml }),
    registrarRutina: toml => post('/api/rutinas/registrar', { toml }),

    propuestas: ({ pendientes = false } = {}) => api('/api/propuestas' + (pendientes ? '?estado=pendiente' : '')),
    decidir: (id, decision) => post(`/api/propuestas/${id}/decidir`, { decision }),

    acciones: () => api('/api/acciones'),
    simular: (herramienta, argumentos) => post('/api/acciones/simular', { herramienta, argumentos }),
    ejecutar: (herramienta, argumentos, precondicion) =>
      post('/api/acciones/ejecutar', { herramienta, argumentos, precondicion }),
    // Los últimos eventos de una herramienta en el diario (solo lectura).
    historialAccion: nombre => api(`/api/acciones/${c(nombre)}/historial`),

    // Abre una terminal real con el comando que confirma lo irreversible
    // ({ propuesta }, { servicios: 'instalar'|'desinstalar' }, { herramienta,
    // argumentos } o { boveda: 'vaciar-papelera' }); la web no confirma nada.
    abrirTerminal: cuerpo => post('/api/terminal/abrir', cuerpo),

    // Lo que `ea servicios ver` cuenta (solo lectura). Instalar y desinstalar
    // solo se abren en la terminal: abrirTerminal({ servicios }).
    serviciosPlan: () => api('/api/servicios/plan'),

    boveda: q => api('/api/boveda?q=' + c(q)),
    nota: ruta => api('/api/boveda/nota?ruta=' + c(ruta)),
    promover: ruta => post('/api/boveda/promover', { ruta }),
    // El editor de la bóveda (gestos del usuario; ningún agente llega aquí).
    bovedaArbol: () => api('/api/boveda/arbol'),
    // El índice de búsqueda frente al disco, y su reconstrucción entera.
    bovedaIndice: () => api('/api/boveda/indice'),
    reindexarBoveda: () => post('/api/boveda/reindexar'),
    bovedaFichero: ruta => api('/api/boveda/fichero?ruta=' + c(ruta)),
    guardarNota: (ruta, contenido, version) => post('/api/boveda/guardar', { ruta, contenido, version }),
    crearNota: (ruta, contenido = '') => post('/api/boveda/crear', { ruta, contenido }),
    renombrarNota: (ruta, nuevo, version) => post('/api/boveda/renombrar', { ruta, nuevo, version }),
    eliminarNota: (ruta, version) => post('/api/boveda/eliminar', { ruta, version }),
    crearCarpeta: ruta => post('/api/boveda/carpeta', { ruta }),
    renombrarCarpeta: (ruta, nuevo) => post('/api/boveda/carpeta/renombrar', { ruta, nuevo }),
    eliminarCarpeta: ruta => post('/api/boveda/carpeta/eliminar', { ruta }),
    mostrarNota: ruta => post('/api/boveda/mostrar', { ruta }),
    papelera: () => api('/api/boveda/papelera'),
    restaurarNota: id => post('/api/boveda/papelera/restaurar', { id }),

    memoria: () => api('/api/memoria'),
    memoriaFichero: (proyecto, fichero) => api(`/api/memoria/${c(proyecto)}/${c(fichero)}`),
    guardarMemoria: (proyecto, fichero, contenido, version) =>
      post(`/api/memoria/${c(proyecto)}/${c(fichero)}`, { contenido, version }),
    crearMemoria: (proyecto, datos) => post(`/api/memoria/${c(proyecto)}/crear`, datos),
    eliminarMemoria: (proyecto, fichero, version) => post(`/api/memoria/${c(proyecto)}/${c(fichero)}/eliminar`, { version }),
    renombrarMemoria: (proyecto, fichero, nuevo, version) =>
      post(`/api/memoria/${c(proyecto)}/${c(fichero)}/renombrar`, { nuevo, version }),
    mostrarMemoria: (proyecto, fichero) => post(`/api/memoria/${c(proyecto)}/${c(fichero)}/mostrar`),

    reiniciar: servicio => post(`/api/servicios/${c(servicio)}/reiniciar`),

    // La biblioteca de herramientas (spec 018): el inventario ya redactado, su
    // uso, y simular y ejecutar activar, desactivar y abrir una sesión.
    biblioteca: () => api('/api/biblioteca'),
    bibliotecaFicha: id => api('/api/biblioteca/' + c(id)),
    bibliotecaUso: () => api('/api/biblioteca/uso'),
    simularBiblioteca: (herramienta, argumentos) => post('/api/biblioteca/simular', { herramienta, argumentos }),
    ejecutarBiblioteca: (herramienta, argumentos, precondicion) =>
      post('/api/biblioteca/ejecutar', { herramienta, argumentos, precondicion }),

    // Equipos de agentes (spec 019). Despertar y dormir son gestos del usuario:
    // despertar ({ titulo, objetivo, worktree, miembros: [{ agente, rol }] | todos,
    // terminal }) abre una terminal por cada planificador y revisor si se pide.
    worktrees: () => api('/api/worktrees'),
    equipos: () => api('/api/equipos'),
    equipo: id => api('/api/equipos/' + c(id)),
    despertarEquipo: cuerpo => post('/api/equipos', cuerpo),
    dormirEquipo: id => post(`/api/equipos/${c(id)}/dormir`),

    // Los ajustes (ajustes.toml). `cambios`: clave -> valor; null la quita.
    ajustes: () => api('/api/ajustes'),
    cambiarAjustes: (version, cambios) => post('/api/ajustes', { version, cambios }),
  };
})();
