// Markdown mínimo, propio y sin dependencias, para lo que escriben los
// agentes (notas de la bóveda y memorias): todo el texto se escapa antes de
// formatear y solo los enlaces http(s) y los internos llegan a ser enlaces.
// Sin DOM, para poder probarlo con node (ea-web/tests/js). Se carga antes de
// app.js, que toma de aquí `esc` y `md`.
'use strict';

const markdown = (() => {
  // Escapa lo que el HTML interpreta, también las comillas de los atributos.
  const esc = s => String(s ?? '').replace(/[&<>"']/g,
    c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));

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

  return { esc, md };
})();

if (typeof module !== 'undefined') module.exports = markdown;
