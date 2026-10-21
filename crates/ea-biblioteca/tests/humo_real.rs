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

/// Uso sobre el HOME real, con la caché en un estado TEMPORAL (nunca la real).
/// Imprime solo recuentos y tiempos: `-- --ignored --nocapture humo_uso`.
#[test]
#[ignore = "lee el HOME real; ejecutar a mano"]
fn humo_uso_sobre_el_home_real() {
    use ea_biblioteca::uso::{calcular_uso, FuentesUso};
    let a: Ajustes = cargar();
    let real = Raices::desde_entorno();
    let inv = Biblioteca::cargar(&real, &a);
    let temporal = tempfile::tempdir().unwrap();
    let mut r = real.clone();
    r.estado = temporal.path().to_path_buf();
    let mut fuentes = FuentesUso::desde_raices(&r); // la caché cae en el estado temporal
    fuentes.trabajos = real.estado.join("trabajos"); // los trabajos sí son los reales (solo lectura)
    for pasada in ["primera (sin caché)", "segunda (con caché)"] {
        let t = std::time::Instant::now();
        let inf = calcular_uso(&r, &inv, &fuentes);
        println!(
            "{pasada}: {:?}; {} elementos con uso, {} sin elemento, {} avisos; ficheros {} (releídos {}, incrementales {}), líneas {}",
            t.elapsed(),
            inf.usos.len(),
            inf.sin_elemento.len(),
            inf.avisos.len(),
            inf.estadisticas.ficheros,
            inf.estadisticas.ficheros_releidos,
            inf.estadisticas.ficheros_incrementales,
            inf.estadisticas.lineas_leidas,
        );
        let usos_total: u64 = inf.usos.iter().map(|u| u.total).sum();
        println!("  usos totales {usos_total}");
        for ac in &inf.actividad {
            println!("  actividad {}: {}", ac.agente, ac.ultima.as_deref().unwrap_or("-"));
        }
        for s in &inf.sin_elemento {
            println!("  sin elemento: {} {} {} ({})", s.agente, s.tipo, s.nombre, s.total);
        }
        for u in &inf.usos {
            println!("  uso: {} ({})", u.id, u.total);
        }
        for av in &inf.avisos {
            println!("  aviso: {av}");
        }
    }
}
