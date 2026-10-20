// Enlaces profundos: el hash de la página, en los dos sentidos. Funciones
// puras (sin DOM) para poder probarlas con node (ea-web/tests/js).
//
// Gramática: #<seccion>[/<fila>][?<param>=<valor>[&<param>=<valor>…]]
//   #trabajos/20261020-101500-ab12      la ficha de un trabajo, desplegada
//   #diario?latidos=1&q=rutina          el diario con filtros
// El hash son partes separadas por `&` (cliente.js ya sacó `clave=`): la
// primera parte sin `=` es la ruta y las demás, parámetros de la sección. El
// `?` delante del primer parámetro es opcional al leer y se escribe siempre.
// La fila y los valores van con encodeURIComponent.

const enlaces = (() => {
  const dec = s => { try { return decodeURIComponent(s); } catch { return s; } };

  // '#trabajos/x?q=a' -> { sec: 'trabajos', fila: 'x', params: { q: 'a' } }.
  // Sin ruta, sec es ''; sin fila, null.
  function leer(hash) {
    const h = String(hash ?? '').replace(/^#/, '');
    const i = h.indexOf('?');
    const partes = (i < 0 ? h : h.slice(0, i) + '&' + h.slice(i + 1)).split('&').filter(Boolean);
    let ruta = null;
    const params = {};
    for (const p of partes) {
      const j = p.indexOf('=');
      if (j < 0) { ruta ??= p; continue; }
      const nombre = dec(p.slice(0, j));
      if (nombre && nombre !== 'clave' && !(nombre in params)) params[nombre] = dec(p.slice(j + 1));
    }
    ruta ??= '';
    const b = ruta.indexOf('/');
    const sec = dec(b < 0 ? ruta : ruta.slice(0, b));
    const fila = b < 0 || b === ruta.length - 1 ? null : dec(ruta.slice(b + 1));
    return { sec, fila, params };
  }

  // La inversa de leer, sin '#'. Los parámetros nulos, vacíos o undefined no
  // se escriben; el orden es el del objeto.
  function escribir({ sec, fila = null, params = {} }) {
    const e = encodeURIComponent;
    let h = e(sec ?? '') + (fila != null && fila !== '' ? '/' + e(fila) : '');
    const ps = Object.entries(params).filter(([, v]) => v != null && v !== '').map(([k, v]) => e(k) + '=' + e(v));
    if (ps.length) h += '?' + ps.join('&');
    return h;
  }

  return { leer, escribir };
})();

if (typeof module !== 'undefined') module.exports = enlaces;
