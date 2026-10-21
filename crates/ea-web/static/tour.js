'use strict';

// Recorrido guiado: va de vista en vista, ilumina una zona y la explica.
// Solo mira: cambia de sección con la función que le da app.js y nunca
// pulsa nada por el usuario (el velo capta los clics y las teclas).

const tour = (() => {
  const NOMBRES = {
    inicio: 'Inicio', trabajos: 'Trabajos', diario: 'Diario', rutinas: 'Rutinas',
    propuestas: 'Propuestas', acciones: 'Acciones', sistema: 'Sistema', boveda: 'Bóveda', memoria: 'Memoria', biblioteca: 'Biblioteca', equipos: 'Equipos', ajustes: 'Ajustes',
  };

  // Cada paso: sección (opcional), anclas candidatas (la primera visible gana),
  // `si` (se salta si no hay ancla), título y texto (HTML propio, constante).
  const PASOS = [
    { titulo: 'Bienvenido a ea',
      texto: 'Este recorrido enseña cada vista en un par de minutos. <b>→</b> avanza, <b>←</b> vuelve, <b>Esc</b> sale. Solo mira: no lanza, cierra ni decide nada.' },
    { sec: 'inicio', ancla: ['#rail'], titulo: 'Las secciones',
      texto: 'Cada símbolo es una vista; las teclas <b>1 – 9</b> y <b>0</b> también llevan a ellas. Las pegatinas cuentan lo que espera: azul si espera al planificador, roja si te espera a ti.' },
    { sec: 'inicio', ancla: ['#cabecera .cab-izq'], titulo: 'La salud del sistema',
      texto: 'Un punto por servicio: verde en marcha, rojo caído. El engranaje es el orquestador: gira mientras late y se pone rojo si deja de latir.' },
    { sec: 'inicio', ancla: ['#btema, #bayuda'], titulo: 'Tema y ayuda',
      texto: '<b>☾/☀</b> cambia entre modo oscuro y claro. <b>?</b> abre la chuleta con la leyenda completa; desde ella, o con la tecla <b>t</b>, vuelves a este recorrido.' },
    { sec: 'inicio', ancla: ['#escena'], titulo: 'La escena',
      texto: 'Cada agente es una máquina de gashapon y cada trabajo una cápsula: la pila tras el cristal es su cola y el raíl de arriba, el orquestador. Pulsa una cápsula para abrir su ficha; <b>▶ demo</b> enseña un recorrido de ejemplo sin tocar nada.' },
    { sec: 'inicio', ancla: ['.actores'], titulo: 'Colas y semáforo',
      texto: 'Lo que espera al ejecutor, lo que espera al planificador y quién tiene el worktree. Solo uno escribe a la vez: el semáforo en rojo dice que está ocupado.' },
    { sec: 'inicio', ancla: ['.paq'], si: true, titulo: 'Cápsulas atascadas',
      texto: 'Una rutina falló y dejó un paquete de corrección. Añade contexto si quieres y pulsa <b>✓ desatascar</b>: solo entonces pasa al planificador.' },
    { sec: 'inicio', ancla: ['.tablero.cc'], si: true, titulo: 'Claude Code',
      texto: 'Una tarjeta por sesión abierta de Claude Code, con sus subagentes debajo: los que siguen en marcha llevan barra; los terminados salen atenuados.' },
    { sec: 'inicio', ancla: ['.tablero.t5'], titulo: 'El tablero',
      texto: 'Las cápsulas avanzan de izquierda a derecha: <i>en cola → ejecutando → en revisión → en espera → cerrado</i>. Pulsa una tarjeta para ver su ficha debajo.' },
    { sec: 'trabajos', ancla: ['#barra'], titulo: 'Filtrar trabajos',
      texto: 'Cada cápsula filtra por su estado, con el número de trabajos en cada uno; pulsa otra vez para quitar el filtro. <b>/</b> salta al cuadro de texto.' },
    { sec: 'trabajos', ancla: ['#lista .f', '#lista'], titulo: 'La lista de trabajos',
      texto: 'Una fila por misión, obra o rutina, con su premio (el informe) y su avance. Pulsa una fila, o <b>Enter</b>, para abrir la ficha: informe, fases, diff e historial.' },
    { sec: 'trabajos', ancla: ['#lista .acc', '#lista .f', '#lista'], titulo: 'Decidir desde la ficha',
      texto: '<b>★ canjear</b> cierra, <b>▶ seguir girando</b> continúa una obra, <b>↺ volver a girar</b> reformula con una nota y <b>⊘ devolver</b> cancela (pide un segundo clic). El azul redondo es la acción principal.' },
    { sec: 'diario', ancla: ['#lista'], titulo: 'El diario',
      texto: 'Todo lo que ocurre en el ecosistema, lo más reciente arriba: quién hizo qué, en qué trabajo y cuánto duró.' },
    { sec: 'diario', ancla: ['#fb'], titulo: 'Filtros del diario',
      texto: 'Quién, <b>✓</b>/<b>✕</b>, qué familia de eventos y cuándo: se piden al servidor, así que miran todo el diario y no solo lo cargado. Quedan en la dirección (<b>#diario?resultado=error</b>) para enlazarlos; el cuadro de filtrar busca texto en lo que llega.' },
    { sec: 'diario', ancla: ['[data-a=latidos]'], si: true, titulo: 'Latidos',
      texto: 'El orquestador late cada pocos segundos. Por defecto se ocultan para no tapar el resto; <b>♡</b> los muestra.' },
    { sec: 'rutinas', ancla: ['#lista'], titulo: 'Rutinas',
      texto: 'Los procesos repetibles, versionados, con su próxima ejecución. <b>▶ echar ficha</b> lanza una ahora: es lo único que crea trabajo desde esta página.' },
    { sec: 'propuestas', ancla: ['#barra'], titulo: 'Pendientes o todas',
      texto: 'Por defecto solo ves lo que espera tu decisión; <b>∗ todas</b> enseña también las ya decididas.' },
    { sec: 'propuestas', ancla: ['#lista .f', '#lista'], titulo: 'Tu visto bueno',
      texto: 'Cambios que un agente quiere hacer. <b>✓ aceptar</b> ejecuta la acción (pide confirmar) y <b>✕</b> la rechaza. Las irreversibles llevan <b>⌨</b>: solo se confirman en la terminal.' },
    { sec: 'acciones', ancla: ['#lista .f', '#lista'], titulo: 'Acciones a mano',
      texto: 'Las herramientas del ecosistema. Abre una, rellena sus campos y <b>simula</b> para ver qué pasaría; solo después se puede <b>ejecutar</b>.' },
    { sec: 'sistema', ancla: ['#lista'], titulo: 'Sistema',
      texto: 'Los servicios que mantienen el ecosistema en marcha y los agentes que trabajan en él. <b>↻</b> reinicia un servicio; pide un segundo clic.' },
    { sec: 'boveda', ancla: ['#filas .arbol-col', '#lista'], titulo: 'La bóveda, como en Obsidian',
      texto: 'Las tres regiones son las carpetas raíz: <b>modelo-local</b> y <b>claude-code</b> (de cada agente) y <b>compartida</b> (la tuya). <b>＋ nota</b> crea una nota en la compartida, <b>＋ carpeta</b> una carpeta en cualquier región y la <b>papelera</b> guarda lo borrado para restaurarlo. El cuadro de arriba filtra por nombre y busca también dentro del contenido.' },
    { sec: 'boveda', ancla: ['#filas .nota-cab', '#filas .nota', '#lista'], titulo: 'Leer, editar, mover y promover',
      texto: '<b>✎</b> (o <b>Ctrl+E</b>) edita cualquier nota y <b>Ctrl+S</b> guarda; la firma de la cabecera (autor, región, fechas, promoción) no se cambia. <b>⋯</b> renombra o mueve dentro de la región (con los enlaces al día), promueve a la compartida, copia la ruta, la muestra en el explorador o la manda a la papelera.' },
    { sec: 'memoria', ancla: ['#filas .arbol', '#lista'], titulo: 'La memoria de Claude Code',
      texto: 'Lo que Claude Code recuerda de cada proyecto: una carpeta por proyecto y, dentro, una nota por memoria; <b>≡</b> es el índice. Pulsa una para abrirla (o <b>j / k</b>); pulsa la carpeta para plegarla.' },
    { sec: 'memoria', ancla: ['#filas .nota-cab', '#lista'], titulo: 'Leer, editar y más',
      texto: '<b>✎</b> (o <b>Ctrl+E</b>) alterna lectura y edición sin perder el borrador; <b>Ctrl+S</b> guarda, y si la memoria cambió en disco avisa antes de pisarla. <b>⋯</b> renombra (y pone al día los enlaces), copia la ruta, la muestra en el explorador o la elimina para siempre.' },
    { sec: 'biblioteca', ancla: ['#lista .f', '#lista'], titulo: 'La biblioteca de herramientas',
      texto: 'Lo que cada agente tiene instalado: skills, plugins, subagentes, MCP, comandos y extensiones, por categoría y tipo (<b>●</b> activado, <b>○</b> no) y con cuánto se usan. Abre una para ver su origen y su uso; <b>⏻</b> activa o desactiva (simula y pide aceptar), <b>▶ sesión con…</b> abre una terminal con solo lo que elijas y <b>✎</b> le da una categoría propia. Nunca se enseñan los secretos de un MCP.' },
    { sec: 'equipos', ancla: ['#lista .f', '#lista'], titulo: 'Equipos de agentes',
      texto: 'Un equipo es un grupo de agentes despertado para una tarea: planificadores y revisores en una terminal, ejecutores en paralelo. Cada fila enseña quién está despierto (<b>●</b>) y cuántos trabajos lleva; ábrela para ver la presencia, la cola y las entregas de cada uno. <b>＋ despertar</b> lo crea y <b>◌ dormir</b> lo apaga (pide confirmar).' },
    { sec: 'ajustes', ancla: ['#lista .f', '#lista'], titulo: 'Ajustes',
      texto: 'Cómo se comporta el ecosistema y de dónde sale cada valor. <b>◐</b> se cambia aquí (segundo clic para confirmar; los topes solo bajan), <b>⌨</b> solo en una terminal con <b>ea ajustes</b> y <b>⊘</b> lo fija una variable de entorno.' },
    { ancla: ['#proyectos:not(.plegada)', '#proyectos', '#bproy'], titulo: 'Proyectos',
      texto: 'Las sesiones de Claude Code abiertas, agrupadas por proyecto, con sus trabajos activos. Pulsa un proyecto para ver solo lo suyo en toda la web; <b>p</b> pliega o despliega la barra.' },
    { titulo: 'Eso es todo',
      texto: 'Atajos: <b>1 – 9, 0</b> secciones, <b>/</b> filtrar, <b>j / k</b> moverse, <b>Enter</b> abrir, <b>Esc</b> cerrar, <b>?</b> la chuleta y <b>t</b> este recorrido.' },
  ];

  const M = 12;          // margen del globo con la ventana y con el ancla
  let ops = {};          // { ir(sec), seccion() }: los pone montar()
  let pasos = [];        // los del recorrido en marcha
  let i = -1;            // paso visible; -1 = parado
  let origen = null;     // sección en la que estaba el usuario
  let dir = 1;           // sentido del último movimiento (para saltar pasos `si`)
  let ancla = null;      // elementos iluminados
  let espera = null;     // temporizador de la espera del ancla
  let reloj = null;      // temporizador de recolocación
  let ultimo = '';       // último recuadro escrito, para no reiniciar la transición
  const el = {};

  const visible = e => {
    const r = e.getBoundingClientRect();
    return e.getClientRects().length > 0 && r.width > 0 && r.height > 0;
  };

  // Los elementos visibles del primer selector del paso que case alguno.
  function hallar(p) {
    for (const sel of p.ancla ?? []) {
      const xs = [...document.querySelectorAll(sel)].filter(visible);
      if (xs.length) return xs;
    }
    return null;
  }

  function montar(o) {
    ops = o;
    const raiz = document.createElement('div');
    raiz.id = 'tour';
    raiz.hidden = true;
    raiz.innerHTML =
      '<div class="velo"></div><div class="foco" hidden></div>' +
      '<section class="globo" role="dialog" aria-modal="true" aria-labelledby="tour-tit">' +
      '<p class="paso"></p><h2 id="tour-tit"></h2><p class="txt" aria-live="polite"></p>' +
      '<div class="nav"><button data-t="salir">saltar</button><span class="grow"></span>' +
      '<button data-t="atras">← anterior</button><button data-t="sig" class="coin">siguiente →</button></div>' +
      '</section>';
    document.body.appendChild(raiz);
    el.raiz = raiz;
    el.foco = raiz.querySelector('.foco');
    el.globo = raiz.querySelector('.globo');
    el.paso = raiz.querySelector('.paso');
    el.tit = raiz.querySelector('h2');
    el.txt = raiz.querySelector('.txt');
    el.salir = raiz.querySelector('[data-t=salir]');
    el.atras = raiz.querySelector('[data-t=atras]');
    el.sig = raiz.querySelector('[data-t=sig]');
    // Ningún clic sale de aquí: ni el velo ni el globo llegan a app.js.
    raiz.addEventListener('click', ev => {
      ev.stopPropagation();
      const b = ev.target.closest('[data-t]');
      if (!b) return;
      if (b.dataset.t === 'salir') terminar();
      else if (b.dataset.t === 'atras') ir(i - 1);
      else ir(i + 1);
    });
  }

  function iniciar(sec = null) {
    if (i >= 0) return;
    pasos = sec == null ? PASOS : PASOS.filter(p => p.sec === sec);
    if (!pasos.length) return;
    origen = ops.seccion();
    const ayuda = document.querySelector('#ayuda');
    if (ayuda) ayuda.hidden = true;
    el.raiz.hidden = false;
    document.addEventListener('keydown', teclas, true);
    addEventListener('resize', colocar);
    document.querySelector('#lista').addEventListener('scroll', colocar, { passive: true });
    reloj = setInterval(recolocar, 250);
    i = -2; // parado, pero ya en marcha: ir() fija el sentido
    ir(0);
  }

  function ir(n) {
    clearTimeout(espera);
    espera = null;
    if (n < 0) n = 0;
    if (n >= pasos.length) return terminar();
    dir = Math.sign(n - i) || 1;
    i = n;
    const p = pasos[i];
    if (p.sec && p.sec !== ops.seccion()) ops.ir(p.sec);
    el.paso.textContent = `${i + 1} / ${pasos.length}` + (p.sec ? ` · ${NOMBRES[p.sec]}` : '');
    el.tit.textContent = p.titulo;
    el.txt.innerHTML = p.texto;
    el.atras.disabled = i === 0;
    el.sig.textContent = i === pasos.length - 1 ? '✓ terminar' : 'siguiente →';
    el.sig.focus({ preventScroll: true });
    ancla = null;
    if (!p.ancla) return colocar();
    // Los datos de la sección llegan con el tick() siguiente: se espera el ancla.
    const limite = Date.now() + 2000;
    const buscar = () => {
      const xs = hallar(p);
      if (xs) {
        espera = null;
        ancla = xs;
        ancla[0].scrollIntoView({ block: 'nearest' });
        return colocar();
      }
      if (Date.now() < limite) { espera = setTimeout(buscar, 100); return; }
      espera = null;
      if (p.si) return ir(i + dir);
      colocar();
    };
    buscar();
  }

  function colocar() {
    if (i < 0) return;
    const g = el.globo;
    el.raiz.classList.toggle('sin-foco', !ancla);
    el.foco.hidden = !ancla;
    // La ventana visible (en un móvil con la página más ancha, innerWidth miente).
    const vv = visualViewport;
    const W = vv ? vv.width : innerWidth, H = vv ? vv.height : innerHeight;
    let r = null;
    if (ancla) {
      const rs = ancla.map(e => e.getBoundingClientRect());
      const izq = Math.max(0, Math.min(...rs.map(x => x.left)) - 6);
      const arr = Math.max(0, Math.min(...rs.map(x => x.top)) - 6);
      const der = Math.min(W, Math.max(...rs.map(x => x.right)) + 6);
      const aba = Math.min(H, Math.max(...rs.map(x => x.bottom)) + 6);
      r = { left: izq, top: arr, right: der, bottom: aba, width: Math.max(0, der - izq), height: Math.max(0, aba - arr) };
      const clave = [r.top, r.left, r.width, r.height].join();
      if (clave !== ultimo) {
        ultimo = clave;
        Object.assign(el.foco.style, { top: r.top + 'px', left: r.left + 'px', width: r.width + 'px', height: r.height + 'px' });
      }
    } else {
      ultimo = '';
    }
    const abajo = W <= 760;
    g.classList.toggle('abajo', abajo);
    g.style.width = abajo ? W - 2 * M + 'px' : '';
    const { width: gw, height: gh } = g.getBoundingClientRect();
    if (abajo) { // fijo encima del carril, que mide 61 px
      g.style.left = M + 'px';
      g.style.top = Math.round(H - 72 - gh) + 'px';
      return;
    }
    const entre = (x, a, b) => Math.max(a, Math.min(b, x));
    let top = (H - gh) / 2, left = (W - gw) / 2;
    if (r) {
      if (r.bottom + M + gh <= H - M) { top = r.bottom + M; left = entre(r.left, M, W - gw - M); }
      else if (r.top - M - gh >= M) { top = r.top - M - gh; left = entre(r.left, M, W - gw - M); }
      else if (r.right + M + gw <= W - M) { left = r.right + M; top = entre(r.top + (r.height - gh) / 2, M, H - gh - M); }
    }
    g.style.top = Math.round(top) + 'px';
    g.style.left = Math.round(left) + 'px';
  }

  // Cada 250 ms: si el repintado de la lista sustituyó el ancla, se busca de nuevo.
  function recolocar() {
    if (i < 0 || espera !== null) return;
    const p = pasos[i];
    if (p.ancla && (!ancla || ancla.some(e => !e.isConnected))) {
      const xs = hallar(p);
      if (xs) ancla = xs;
      else if (ancla) ancla = null;
    }
    colocar();
  }

  function teclas(ev) {
    const t = ev.target;
    const boton = t.closest?.('[data-t]');
    const k = ev.key;
    if (k === 'Tab') {
      const bs = [el.salir, el.atras, el.sig].filter(b => !b.disabled);
      const n = bs.indexOf(t);
      bs[(n + (ev.shiftKey ? -1 : 1) + bs.length) % bs.length].focus();
    } else if (k === 'ArrowRight' || (k === 'Enter' && (!boton || boton === el.sig))) {
      ir(i + 1);
    } else if (k === 'ArrowLeft') {
      ir(i - 1);
    } else if (k === 'Escape') {
      terminar();
    } else if ((k === 'Enter' || k === ' ') && boton) {
      return; // el botón enfocado hace su gesto nativo
    }
    ev.preventDefault();
    ev.stopPropagation();
  }

  function terminar() {
    clearTimeout(espera);
    clearInterval(reloj);
    espera = reloj = null;
    document.removeEventListener('keydown', teclas, true);
    removeEventListener('resize', colocar);
    document.querySelector('#lista').removeEventListener('scroll', colocar);
    el.raiz.hidden = true;
    i = -1;
    ancla = null;
    ultimo = '';
    if (origen !== ops.seccion()) ops.ir(origen);
    try { localStorage.setItem('ea-tour-visto', '1'); } catch { /* sin almacenamiento */ }
    document.querySelector('#bayuda')?.focus();
  }

  return { montar, iniciar, activo: () => i >= 0 };
})();
