// Cliente de la API de ea-web: el único sitio de la página que conoce las
// rutas HTTP. Cada función devuelve el JSON de la respuesta o lanza un
// Error con el mensaje del servidor.

const ea = (() => {
  async function api(ruta, opts = {}) {
    const r = await fetch(ruta, { headers: { 'Content-Type': 'application/json' }, ...opts });
    const txt = await r.text();
    let data = null;
    try { data = JSON.parse(txt); } catch { /* cuerpo no JSON */ }
    if (!r.ok) throw Object.assign(new Error((data && data.error) || txt || r.statusText), { status: r.status });
    return data;
  }
  const post = (ruta, cuerpo = {}) => api(ruta, { method: 'POST', body: JSON.stringify(cuerpo) });
  const c = encodeURIComponent;

  return {
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

    memoria: () => api('/api/memoria'),
    memoriaFichero: (proyecto, fichero) => api(`/api/memoria/${c(proyecto)}/${c(fichero)}`),
    guardarMemoria: (proyecto, fichero, contenido, version) =>
      post(`/api/memoria/${c(proyecto)}/${c(fichero)}`, { contenido, version }),

    reiniciar: servicio => post(`/api/servicios/${c(servicio)}/reiniciar`),
  };
})();
