// Cliente de la API de ea-web: el único sitio de la página que conoce las
// rutas HTTP. Cada función devuelve el JSON de la respuesta o lanza un
// Error con el mensaje del servidor.

//
// Clave de sesión: `ea-web-open` abre la página con `#clave=<clave>`. El hash
// son partes separadas por `&` (la sección, `clave=…` y, más adelante,
// enlaces profundos): aquí se saca la clave, se guarda en sessionStorage y se
// deja el resto del hash tal cual con history.replaceState, antes de que
// app.js lo lea. Toda petición que no sea GET lleva la clave en X-EA-Clave;
// sin ella (o si el servidor la rechaza) el panel queda en solo lectura y las
// escrituras fallan con el aviso, sin llegar al servidor.

const ea = (() => {
  const AVISO_LECTURA = 'Panel en solo lectura: ábrelo con ea-web-open para poder cambiar nada.';
  let clave = null;
  try { clave = sessionStorage.getItem('ea-clave'); } catch { /* sin almacenamiento */ }
  {
    const partes = location.hash.slice(1).split('&').filter(Boolean);
    const resto = partes.filter(p => !p.startsWith('clave='));
    if (resto.length !== partes.length) {
      const nueva = decodeURIComponent(partes.find(p => p.startsWith('clave=')).slice(6));
      if (nueva) {
        clave = nueva;
        try { sessionStorage.setItem('ea-clave', clave); } catch { /* sin almacenamiento: vale para esta carga */ }
      }
      history.replaceState(null, '', location.pathname + location.search + (resto.length ? '#' + resto.join('&') : ''));
    }
  }
  const oyentes = [];
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
    // `alCambiarSesion(f)` avisa con f(false) al perder la clave.
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
    sesiones: () => api('/api/sesiones'),

    trabajos: () => api('/api/trabajos'),
    trabajo: id => api('/api/trabajos/' + c(id)),
    diff: id => api(`/api/trabajos/${c(id)}/diff`),
    // accion: cerrar, cancelar, continuar, reformular ({ nota }) o confirmar ({ contexto }).
    accionTrabajo: (id, accion, cuerpo = {}) => post(`/api/trabajos/${c(id)}/${accion}`, cuerpo),
    colas: () => api('/api/colas'),
    semaforos: () => api('/api/semaforos'),

    diario: ({ limite, latidos = false } = {}) =>
      api('/api/diario?limite=' + limite + (latidos ? '&latidos=1' : '')),

    rutinas: () => api('/api/rutinas'),
    rutina: slug => api('/api/rutinas/' + c(slug)),
    lanzarRutina: slug => post(`/api/rutinas/${c(slug)}/lanzar`),

    propuestas: ({ pendientes = false } = {}) => api('/api/propuestas' + (pendientes ? '?estado=pendiente' : '')),
    decidir: (id, decision) => post(`/api/propuestas/${id}/decidir`, { decision }),

    acciones: () => api('/api/acciones'),
    simular: (herramienta, argumentos) => post('/api/acciones/simular', { herramienta, argumentos }),
    ejecutar: (herramienta, argumentos, precondicion) =>
      post('/api/acciones/ejecutar', { herramienta, argumentos, precondicion }),

    boveda: q => api('/api/boveda?q=' + c(q)),
    nota: ruta => api('/api/boveda/nota?ruta=' + c(ruta)),
    promover: ruta => post('/api/boveda/promover', { ruta }),
    // El editor de la bóveda (gestos del usuario; ningún agente llega aquí).
    bovedaArbol: () => api('/api/boveda/arbol'),
    bovedaFichero: ruta => api('/api/boveda/fichero?ruta=' + c(ruta)),
    guardarNota: (ruta, contenido, version) => post('/api/boveda/guardar', { ruta, contenido, version }),
    crearNota: (ruta, contenido = '') => post('/api/boveda/crear', { ruta, contenido }),
    renombrarNota: (ruta, nuevo, version) => post('/api/boveda/renombrar', { ruta, nuevo, version }),
    eliminarNota: (ruta, version) => post('/api/boveda/eliminar', { ruta, version }),
    crearCarpeta: ruta => post('/api/boveda/carpeta', { ruta }),
    mostrarNota: ruta => post('/api/boveda/mostrar', { ruta }),
    papelera: () => api('/api/boveda/papelera'),
    restaurarNota: id => post('/api/boveda/papelera/restaurar', { id }),

    memoria: () => api('/api/memoria'),
    memoriaFichero: (proyecto, fichero) => api(`/api/memoria/${c(proyecto)}/${c(fichero)}`),
    guardarMemoria: (proyecto, fichero, contenido, version) =>
      post(`/api/memoria/${c(proyecto)}/${c(fichero)}`, { contenido, version }),
    eliminarMemoria: (proyecto, fichero, version) => post(`/api/memoria/${c(proyecto)}/${c(fichero)}/eliminar`, { version }),
    renombrarMemoria: (proyecto, fichero, nuevo, version) =>
      post(`/api/memoria/${c(proyecto)}/${c(fichero)}/renombrar`, { nuevo, version }),
    mostrarMemoria: (proyecto, fichero) => post(`/api/memoria/${c(proyecto)}/${c(fichero)}/mostrar`),

    reiniciar: servicio => post(`/api/servicios/${c(servicio)}/reiniciar`),

    // Los ajustes (ajustes.toml). `cambios`: clave -> valor; null la quita.
    ajustes: () => api('/api/ajustes'),
    cambiarAjustes: (version, cambios) => post('/api/ajustes', { version, cambios }),
  };
})();
