// Pruebas del parseo del hash (static/enlaces.js). Las lanza
// tests/enlaces_js.rs con `node --test`; también a mano:
//   node --test crates/ea-web/tests/js/
'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const enlaces = require('../../static/enlaces.js');

test('solo la sección', () => {
  assert.deepEqual(enlaces.leer('#diario'), { sec: 'diario', fila: null, params: {} });
  assert.deepEqual(enlaces.leer('trabajos'), { sec: 'trabajos', fila: null, params: {} });
  assert.deepEqual(enlaces.leer(''), { sec: '', fila: null, params: {} });
  assert.deepEqual(enlaces.leer('#'), { sec: '', fila: null, params: {} });
});

test('sección y fila', () => {
  assert.deepEqual(enlaces.leer('#trabajos/20261020-101500-ab12'),
    { sec: 'trabajos', fila: '20261020-101500-ab12', params: {} });
  assert.deepEqual(enlaces.leer('#rutinas/copia%20diaria'), { sec: 'rutinas', fila: 'copia diaria', params: {} });
  // La fila puede llevar barras (lo que va tras la primera).
  assert.equal(enlaces.leer('#acciones/a/b').fila, 'a/b');
  assert.equal(enlaces.leer('#trabajos/').fila, null);
});

test('parámetros con ? o con &', () => {
  const esperado = { sec: 'diario', fila: null, params: { latidos: '1', q: 'rutina fallida' } };
  assert.deepEqual(enlaces.leer('#diario?latidos=1&q=rutina%20fallida'), esperado);
  assert.deepEqual(enlaces.leer('#diario&latidos=1&q=rutina%20fallida'), esperado);
  assert.deepEqual(enlaces.leer('#trabajos/x?estado=en_revision'),
    { sec: 'trabajos', fila: 'x', params: { estado: 'en_revision' } });
  // Un valor con '=' o '?' codificados vuelve tal cual.
  assert.equal(enlaces.leer('#diario?q=a%3Db%3Fc').params.q, 'a=b?c');
  // El primero gana; un parámetro sin nombre se ignora.
  assert.deepEqual(enlaces.leer('#diario?q=uno&q=dos&=x').params, { q: 'uno' });
});

test('la clave nunca es un parámetro de sección', () => {
  // cliente.js la saca antes, pero por si acaso.
  assert.deepEqual(enlaces.leer('#trabajos/x&clave=secreta'), { sec: 'trabajos', fila: 'x', params: {} });
  assert.deepEqual(enlaces.leer('#clave=secreta&diario?latidos=1'), { sec: 'diario', fila: null, params: { latidos: '1' } });
});

test('lo que deja cliente.js tras quitar la clave se lee igual', () => {
  // ea-web-open '#diario?latidos=1' abre '#diario?latidos=1&clave=K'; cliente.js quita la parte de la clave.
  const partes = 'diario?latidos=1&clave=K'.split('&').filter(p => !p.startsWith('clave='));
  assert.deepEqual(enlaces.leer('#' + partes.join('&')), { sec: 'diario', fila: null, params: { latidos: '1' } });
});

test('codificación rota no rompe', () => {
  assert.deepEqual(enlaces.leer('#diario?q=%E0%A4'), { sec: 'diario', fila: null, params: { q: '%E0%A4' } });
});

test('escribir', () => {
  assert.equal(enlaces.escribir({ sec: 'inicio' }), 'inicio');
  assert.equal(enlaces.escribir({ sec: 'trabajos', fila: '20261020-101500-ab12' }), 'trabajos/20261020-101500-ab12');
  assert.equal(enlaces.escribir({ sec: 'diario', params: { latidos: '1', q: null, actor: '' } }), 'diario?latidos=1');
  assert.equal(enlaces.escribir({ sec: 'diario', params: { q: 'a&b=c?d #e' } }), 'diario?q=a%26b%3Dc%3Fd%20%23e');
  assert.equal(enlaces.escribir({ sec: 'rutinas', fila: 'a/b', params: { q: 'ñ' } }), 'rutinas/a%2Fb?q=%C3%B1');
});

test('ida y vuelta', () => {
  for (const v of [
    { sec: 'trabajos', fila: 'x-1', params: { estado: 'cerrado', q: 'obra' } },
    { sec: 'diario', fila: null, params: { latidos: '1', q: 'a&b=c?d/e' } },
    { sec: 'acciones', fila: 'ea_escribir_nota', params: {} },
    { sec: 'rutinas', fila: 'con/barra', params: { q: '¿qué?' } },
  ]) assert.deepEqual(enlaces.leer('#' + enlaces.escribir(v)), v);
});
