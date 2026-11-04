// Pruebas del Markdown del panel (static/markdown.js), que pinta lo que
// escriben los agentes: nada de lo que llega puede acabar siendo HTML activo.
// Las lanza tests/enlaces_js.rs con `node --test`; también a mano:
//   node --test crates/ea-web/tests/js/*.test.js
'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const { esc, md } = require('../../static/markdown.js');

// Lo único que puede salir: estas etiquetas, con estos atributos y valores
// entre comillas dobles; un enlace solo a http(s) o a `#`.
const ETIQUETAS = new Set(['p', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'strong', 'em', 'code', 'pre', 'a', 'span',
  'ul', 'ol', 'li', 'blockquote', 'table', 'thead', 'tbody', 'tr', 'th', 'td', 'hr']);
const ATRIBUTOS = new Set(['href', 'target', 'rel', 'class', 'data-ml', 'title']);

function soloLoPermitido(html) {
  const resto = html.replace(/<(\/?)([a-z0-9]+)((?:\s[a-z-]+="[^"<>]*")*)\s*>/g, (_, cierre, etiqueta, attrs) => {
    assert.ok(ETIQUETAS.has(etiqueta), `etiqueta <${etiqueta}> en ${html}`);
    for (const [, nombre, valor] of attrs.matchAll(/\s([a-z-]+)="([^"]*)"/g)) {
      assert.ok(ATRIBUTOS.has(nombre), `atributo ${nombre} en ${html}`);
      if (nombre === 'href') assert.match(valor, /^(https?:\/\/|#$)/i, `href=${valor} en ${html}`);
    }
    assert.ok(!cierre || !attrs, html);
    return '';
  });
  // Lo que no es una etiqueta permitida no lleva ni `<` ni `>` sin escapar.
  assert.doesNotMatch(resto, /[<>]/, `HTML sin escapar en ${html}`);
}

// En todos los sitios donde va texto: párrafo, título, lista, cita, tabla,
// enlace y wiki-enlace con alias.
const SITIOS = [
  x => x,
  x => `# ${x}`,
  x => `- ${x}`,
  x => `> ${x}`,
  x => `| a |\n|---|\n| ${x} |`,
  x => `[${x}](https://a.ejemplo)`,
  x => `[[nota|${x}]]`,
  x => `**${x}** y _${x}_`,
];
const CTX = { wiki: d => (d === 'nota' ? 'boveda/nota.md' : null), md: u => (u === 'otra.md' ? 'boveda/otra.md' : null) };

function pintar(texto, ctx = CTX) {
  const html = md(texto, ctx);
  soloLoPermitido(html);
  return html;
}

test('el detector de lo permitido ve lo que tiene que ver', () => {
  for (const malo of ['<img src=x onerror=alert(1)>', '<a href="javascript:x">x</a>', '<p onclick="x">a</p>',
    '<a href="https://a.ejemplo" onmouseover="x">a</a>', '<p>a > b</p>', '<script>', '<a href="data:text/html,x">a</a>']) {
    assert.throws(() => soloLoPermitido(malo), undefined, malo);
  }
  soloLoPermitido('<p><a href="https://a.ejemplo" target="_blank" rel="noopener noreferrer">x</a> &lt;b&gt; <span class="roto" title="t">y</span></p>');
});

test('esc escapa todo lo que el HTML interpreta', () => {
  assert.equal(esc(`<a href="x" title='y'>&</a>`), '&lt;a href=&quot;x&quot; title=&#39;y&#39;&gt;&amp;&lt;/a&gt;');
  assert.equal(esc(null), '');
  assert.equal(esc(undefined), '');
  assert.equal(esc(3), '3');
});

test('<script> sale como texto', () => {
  for (const sitio of SITIOS) {
    const html = pintar(sitio('<script>alert(1)</script>'));
    assert.ok(html.includes('&lt;script&gt;alert(1)&lt;/script&gt;'), html);
  }
});

test('<img onerror> sale como texto', () => {
  for (const sitio of SITIOS) {
    const html = pintar(sitio('<img src=x onerror="alert(1)">'));
    assert.ok(html.includes('&lt;img src=x onerror=&quot;alert(1)&quot;&gt;'), html);
  }
  pintar('<svg/onload=alert(1)> <iframe srcdoc="<script>">');
});

test('los enlaces javascript: y data: no son enlaces', () => {
  for (const url of ['javascript:alert(1)', 'JaVaScRiPt:alert(1)', 'javascript&#58;alert(1)', '&#106;avascript:alert(1)',
    'data:text/html,<script>alert(1)</script>', 'data:text/html;base64,PHNjcmlwdD4=', 'vbscript:x', '//evil.ejemplo/x']) {
    const html = pintar(`[pulsa](${url})`);
    assert.ok(!html.includes('<a'), `${url}: ${html}`);
    assert.ok(html.startsWith('<p><span class="roto" title="'), html);
  }
  // Ni dentro de un enlace con texto que parece http.
  assert.ok(!pintar('[https://ok.ejemplo](javascript:alert(1))').includes('<a'));
  // Un wiki-enlace o un enlace relativo a una nota que no existe tampoco.
  assert.ok(!pintar('[[javascript:alert(1)]]').includes('<a'));
  assert.ok(!pintar('[x](no-existe.md)').includes('<a'));
});

test('los enlaces que sí lo son', () => {
  assert.equal(pintar('[web](https://a.ejemplo/b?c=1&d=2)'),
    '<p><a href="https://a.ejemplo/b?c=1&amp;d=2" target="_blank" rel="noopener noreferrer">web</a></p>');
  assert.equal(pintar('[otra](otra.md)'), '<p><a href="#" class="ml" data-ml="boveda/otra.md">otra</a></p>');
  assert.equal(pintar('[[nota]]'), '<p><a href="#" class="ml" data-ml="boveda/nota.md">[[nota]]</a></p>');
});

test('las comillas no cierran el atributo: ni en la URL ni en el título', () => {
  // (`pintar` ya falla si sale un atributo que no es de la lista, como `onmouseover`.)
  assert.equal(pintar('[x](https://a.ejemplo/"onmouseover="alert(1))'),
    '<p><a href="https://a.ejemplo/&quot;onmouseover=&quot;alert(1" target="_blank" rel="noopener noreferrer">x</a>)</p>');
  assert.equal(pintar("[x](https://a.ejemplo/'onmouseover='alert(1))"),
    '<p><a href="https://a.ejemplo/&#39;onmouseover=&#39;alert(1" target="_blank" rel="noopener noreferrer">x</a>)</p>');
  pintar('[x](https://a.ejemplo/"><script>alert(1)</script>)');
  // El título de un enlace roto (la URL) y el de un wiki-enlace sin nota (`roto`).
  const roto = pintar('[x](no"existe\'.md)');
  assert.ok(roto.includes('title="no&quot;existe&#39;.md"'), roto);
  const wiki = pintar('[[falta]]', { ...CTX, roto: 'no "hay" <nota>' });
  assert.ok(wiki.includes('title="no &quot;hay&quot; &lt;nota&gt;"'), wiki);
  // La clave de la nota que da el contexto también se escapa.
  const clave = pintar('[[nota]]', { wiki: () => 'a"b<c>' });
  assert.ok(clave.includes('data-ml="a&quot;b&lt;c&gt;"'), clave);
});

test('wiki-enlaces con alias: solo el alias, escapado', () => {
  assert.equal(pintar('[[nota|la receta]]'), '<p><a href="#" class="ml" data-ml="boveda/nota.md">la receta</a></p>');
  assert.equal(pintar('[[nota#seccion|ver]]'), '<p><a href="#" class="ml" data-ml="boveda/nota.md">ver</a></p>');
  assert.equal(pintar('[[nota|<b>"x"</b>]]'), '<p><a href="#" class="ml" data-ml="boveda/nota.md">&lt;b&gt;&quot;x&quot;&lt;/b&gt;</a></p>');
  assert.equal(pintar('[[falta|alias]]'), '<p><span class="roto" title="no hay esa nota">alias</span></p>');
});

test('los bloques de código se escapan enteros y no se formatean', () => {
  const html = pintar('```html\n</code></pre><script>alert(1)</script>\n**no** [x](https://a.ejemplo)\n```');
  assert.equal(html, '<pre><code>&lt;/code&gt;&lt;/pre&gt;&lt;script&gt;alert(1)&lt;/script&gt;\n**no** [x](https://a.ejemplo)</code></pre>');
  assert.equal(pintar('`<img onerror=x>`'), '<p><code>&lt;img onerror=x&gt;</code></p>');
  // Sin cerrar: hasta el final, igual de escapado.
  assert.equal(pintar('```\n<b>'), '<pre><code>&lt;b&gt;</code></pre>');
});

test('las secuencias de control no rescatan fragmentos guardados', () => {
  // \u0001N\u0002 es la marca interna de un fragmento ya pintado: si llega en
  // el texto, se quita y no trae el enlace de otro sitio.
  assert.equal(pintar('[[nota]] \u00010\u0002'), '<p><a href="#" class="ml" data-ml="boveda/nota.md">[[nota]]</a> 0</p>');
  assert.equal(pintar('\u00010\u0002 [x](https://a.ejemplo)'),
    '<p>0 <a href="https://a.ejemplo" target="_blank" rel="noopener noreferrer">x</a></p>');
  // El resto de controles (ANSI, NUL, campana) son texto.
  const html = pintar('\u001b[31mrojo\u001b[0m \u0000 \u0007 <b>');
  assert.ok(html.includes('\u001b[31mrojo') && html.includes('&lt;b&gt;'), html);
});

test('nada de lo que llega se cuela, ni mezclado', () => {
  const piezas = ['<script>', '</p>', '<img onerror=x>', '"', "'", '&', '[[nota|', ']]', '[x](', 'javascript:', ')', '```', '\n', '**',
    '_', '`', '|', '# ', '> ', '- ', '\u0001', '\u0002', '0', 'https://a.ejemplo/', 'data:', '<a href="javascript:x">'];
  let semilla = 11;
  const azar = k => { semilla = (semilla * 1103515245 + 12345) % 2147483648; return semilla % k; };
  for (let i = 0; i < 3000; i++) {
    const texto = Array.from({ length: 1 + azar(12) }, () => piezas[azar(piezas.length)]).join('');
    pintar(texto);
    pintar(texto, {});
  }
});
