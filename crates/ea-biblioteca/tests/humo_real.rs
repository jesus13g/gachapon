//! Humo manual, solo lectura, sobre el HOME REAL del usuario. Ignorado por
//! defecto (las pruebas normales no tocan el HOME):
//! `cargo test -p ea-biblioteca --test humo_real -- --ignored --nocapture`.
//! Imprime solo recuentos y avisos, nunca definiciones de MCP.

use std::collections::BTreeMap;

use ea_ajustes::{cargar, Ajustes};
use ea_biblioteca::{Biblioteca, Raices};

#[test]
#[ignore = "lee el HOME real; ejecutar a mano"]
fn humo_sobre_el_home_real() {
    let a: Ajustes = cargar();
    let inv = Biblioteca::cargar(&Raices::desde_entorno(), &a);
    let mut c: BTreeMap<String, usize> = BTreeMap::new();
    for e in &inv.elementos {
        *c.entry(format!("{} {}", e.agente, e.tipo)).or_default() += 1;
    }
    println!("{} elementos", inv.elementos.len());
    for (k, n) in c {
        println!("  {k}: {n}");
    }
    for av in &inv.avisos {
        println!("aviso: {av}");
    }
    assert!(!serde_json::to_string(&inv).unwrap().is_empty());
}
