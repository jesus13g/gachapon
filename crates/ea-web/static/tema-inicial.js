// El tema claro antes de pintar nada: va en <head> sin defer ni async, así que
// bloquea como bloqueaba el guion en línea que había. Fuera de index.html para
// que la CSP no tenga que admitir guiones en línea.
try { if (localStorage.getItem('ea-tema') === 'claro') document.documentElement.classList.add('claro'); } catch { /* sin almacenamiento */ }
