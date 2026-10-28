//! Prueba de arquitectura: el grafo de crates del workspace sigue el mapa de
//! capas (`docs/manual.md`, «Estructura»). Lee los `Cargo.toml` y el código de
//! cada crate; no compila nada ni toca el estado del usuario.

use std::path::{Path, PathBuf};

/// Lo que puede usar cualquier presentación: las capas de abajo, nunca otra
/// presentación.
const BAJO_LAS_PRESENTACIONES: &[&str] = &["ea-plataforma", "ea-ajustes", "ea-store", "ea-boveda", "ea-biblioteca", "ea-bus", "ea-acciones"];

/// Las aristas `ea-*` permitidas en `[dependencies]` (también las de
/// `[target.*.dependencies]`) de cada crate del workspace. Un crate nuevo tiene
/// que entrar aquí, con su sitio en el mapa.
const PERMITIDAS: &[(&str, &[&str])] = &[
    ("ea-plataforma", &[]),
    ("ea-ajustes", &["ea-plataforma"]),
    ("ea-store", &["ea-ajustes"]),
    ("ea-boveda", &["ea-store", "ea-ajustes", "ea-plataforma"]),
    ("ea-biblioteca", &["ea-ajustes", "ea-plataforma", "ea-store"]),
    ("ea-bus", &["ea-store", "ea-ajustes", "ea-plataforma"]),
    ("ea-acciones", &["ea-store", "ea-boveda", "ea-bus", "ea-ajustes", "ea-biblioteca", "ea-plataforma"]),
    ("ea-cli", BAJO_LAS_PRESENTACIONES),
    ("ea-web", BAJO_LAS_PRESENTACIONES),
    ("ea-mcp", BAJO_LAS_PRESENTACIONES),
    // Nunca `ea-acciones`: el ejecutor no muta nada a través de ellas.
    ("ea-orchestrator", &["ea-store", "ea-bus", "ea-ajustes", "ea-plataforma"]),
    // Utilidades de prueba: fuera del mapa, solo en `[dev-dependencies]`
    // (ver `lo_de_prueba_solo_entra_por_dev_dependencies`).
    ("ea-pruebas", BAJO_LAS_PRESENTACIONES),
];

/// Las presentaciones no dependen entre sí, ni siquiera en las pruebas.
const PRESENTACIONES: [&str; 4] = ["ea-cli", "ea-web", "ea-mcp", "ea-orchestrator"];

fn raiz() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn leer_toml(ruta: &Path) -> toml::Table {
    let texto = std::fs::read_to_string(ruta).unwrap_or_else(|e| panic!("{}: {e}", ruta.display()));
    texto.parse().unwrap_or_else(|e| panic!("{}: {e}", ruta.display()))
}

/// Los miembros del workspace (`crates/ea-x`), según el `Cargo.toml` raíz.
fn miembros() -> Vec<String> {
    let t = leer_toml(&raiz().join("Cargo.toml"));
    t["workspace"]["members"].as_array().unwrap().iter().map(|m| m.as_str().unwrap().to_string()).collect()
}

/// Las dependencias (nombre y valor) de las secciones `seccion` de un
/// manifiesto, también las de `[target.<cfg>.<seccion>]`.
fn entradas<'a>(manifiesto: &'a toml::Table, seccion: &str) -> Vec<(&'a String, &'a toml::Value)> {
    let mut tablas: Vec<&toml::Table> = manifiesto.get(seccion).and_then(|v| v.as_table()).into_iter().collect();
    if let Some(objetivos) = manifiesto.get("target").and_then(|v| v.as_table()) {
        tablas.extend(objetivos.values().filter_map(|o| o.get(seccion).and_then(|v| v.as_table())));
    }
    tablas.into_iter().flat_map(|t| t.iter()).collect()
}

/// Los nombres de las dependencias de las secciones `seccion` de un manifiesto.
fn dependencias(manifiesto: &toml::Table, seccion: &str) -> Vec<String> {
    entradas(manifiesto, seccion).into_iter().map(|(d, _)| d.clone()).collect()
}

/// Lo de prueba que un manifiesto mete fuera de `[dev-dependencies]`:
/// `ea-pruebas` como dependencia normal o de compilación, los dobles (la
/// *feature* `dobles`) activados desde ellas, o por defecto.
fn de_prueba_fuera_de_las_pruebas(nombre: &str, manifiesto: &toml::Table) -> Vec<String> {
    let es_dobles = |f: &toml::Value| f.as_str().is_some_and(|f| f == "dobles" || f.ends_with("/dobles"));
    let mut fallos = Vec::new();
    for seccion in ["dependencies", "build-dependencies"] {
        for (dep, valor) in entradas(manifiesto, seccion) {
            if dep == "ea-pruebas" {
                fallos.push(format!("{nombre} → ea-pruebas ({seccion}): solo en [dev-dependencies]"));
            }
            if valor.get("features").and_then(|f| f.as_array()).is_some_and(|f| f.iter().any(es_dobles)) {
                fallos.push(format!("{nombre} → {dep} ({seccion}): los dobles solo se activan desde [dev-dependencies]"));
            }
        }
    }
    let defecto = manifiesto.get("features").and_then(|f| f.get("default")).and_then(|d| d.as_array());
    if defecto.is_some_and(|d| d.iter().any(es_dobles)) {
        fallos.push(format!("{nombre}: los dobles no van en las features por defecto"));
    }
    fallos
}

/// Las utilidades de prueba y los dobles no llegan a ningún binario: solo
/// entran por `[dev-dependencies]`.
#[test]
fn lo_de_prueba_solo_entra_por_dev_dependencies() {
    let mut fallos = Vec::new();
    for miembro in miembros() {
        let manifiesto = leer_toml(&raiz().join(&miembro).join("Cargo.toml"));
        let nombre = manifiesto["package"]["name"].as_str().unwrap().to_string();
        fallos.extend(de_prueba_fuera_de_las_pruebas(&nombre, &manifiesto));
    }
    assert!(fallos.is_empty(), "algo de prueba fuera de las pruebas:\n{}", fallos.join("\n"));
}

#[test]
fn las_aristas_entre_crates_siguen_el_mapa_de_capas() {
    let mut fallos = Vec::new();
    for miembro in miembros() {
        let manifiesto = leer_toml(&raiz().join(&miembro).join("Cargo.toml"));
        let nombre = manifiesto["package"]["name"].as_str().unwrap().to_string();
        let Some((_, permitidas)) = PERMITIDAS.iter().find(|(c, _)| *c == nombre) else {
            fallos.push(format!("{nombre}: crate nuevo sin sitio en el mapa de capas (PERMITIDAS)"));
            continue;
        };
        for dep in dependencias(&manifiesto, "dependencies") {
            if dep.starts_with("ea-") && !permitidas.contains(&dep.as_str()) {
                fallos.push(format!("{nombre} → {dep}: arista que el mapa de capas no permite"));
            }
            if dep == "libc" && nombre != "ea-plataforma" {
                fallos.push(format!("{nombre} → libc: solo ea-plataforma usa libc"));
            }
        }
        if PRESENTACIONES.contains(&nombre.as_str()) {
            for seccion in ["dependencies", "dev-dependencies", "build-dependencies"] {
                for dep in dependencias(&manifiesto, seccion) {
                    if dep != nombre && PRESENTACIONES.contains(&dep.as_str()) {
                        fallos.push(format!("{nombre} → {dep} ({seccion}): una presentación no depende de otra"));
                    }
                }
            }
        }
    }
    assert!(fallos.is_empty(), "el grafo se sale del mapa de capas:\n{}", fallos.join("\n"));
}

/// Los `.rs` bajo `dir`, recorridos en profundidad.
fn ficheros_rs(dir: &Path, salida: &mut Vec<PathBuf>) {
    let Ok(entradas) = std::fs::read_dir(dir) else { return };
    for e in entradas.flatten() {
        let ruta = e.path();
        if ruta.is_dir() {
            ficheros_rs(&ruta, salida);
        } else if ruta.extension().is_some_and(|x| x == "rs") {
            salida.push(ruta);
        }
    }
}

/// ¿Es una línea con código propio de un sistema? `libc`, `std::os::unix` o un
/// atributo `cfg` que mira el sistema (`target_os`, `unix`, `windows`…).
fn es_de_sistema(linea: &str) -> bool {
    let atributo_cfg = linea.contains("#[cfg") || linea.contains("#![cfg");
    linea.contains("libc::")
        || linea.contains("std::os::unix")
        || (atributo_cfg && ["target_os", "target_family", "unix", "windows"].iter().any(|w| linea.contains(w)))
}

/// La excepción documentada: el aviso con acción del orquestador, solo en Linux.
fn excepcion(ruta: &str, linea: &str) -> bool {
    ruta.starts_with("crates/ea-orchestrator/src/ejecutor")
        && linea.contains("target_os = \"linux\"")
        && !linea.contains("libc::")
        && !linea.contains("std::os::unix")
}

#[test]
fn solo_ea_plataforma_tiene_codigo_de_cada_sistema() {
    let raiz = raiz();
    let mut fallos = Vec::new();
    for miembro in miembros().into_iter().filter(|m| !m.ends_with("/ea-plataforma")) {
        let mut ficheros = Vec::new();
        ficheros_rs(&raiz.join(&miembro).join("src"), &mut ficheros);
        assert!(!ficheros.is_empty(), "{miembro}: sin código en src/");
        for fichero in ficheros {
            let texto = std::fs::read_to_string(&fichero).unwrap();
            let ruta = fichero.strip_prefix(&raiz).unwrap().to_string_lossy().replace('\\', "/");
            // Las pruebas pueden mirar el sistema: solo cuenta lo anterior al primer `#[cfg(test)]`.
            for (n, linea) in texto.lines().enumerate().take_while(|(_, l)| !l.contains("#[cfg(test)]")) {
                let codigo = linea.trim_start();
                if !codigo.starts_with("//") && es_de_sistema(codigo) && !excepcion(&ruta, codigo) {
                    fallos.push(format!("{ruta}:{}: {codigo}", n + 1));
                }
            }
        }
    }
    assert!(fallos.is_empty(), "código propio de un sistema fuera de ea-plataforma:\n{}", fallos.join("\n"));
}

/// Las comprobaciones no dejan pasar lo que vigilan.
#[test]
fn las_comprobaciones_detectan_lo_prohibido() {
    let m: toml::Table = "[dependencies]\nea-web = { path = \"../ea-web\" }\n[target.'cfg(unix)'.dependencies]\nlibc = \"0.2\"\n".parse().unwrap();
    assert_eq!(dependencias(&m, "dependencies"), ["ea-web", "libc"]);
    let m: toml::Table = "[features]\ndefault = [\"x/dobles\"]\n[dependencies]\nea-pruebas = { path = \"../ea-pruebas\" }\n\
        [target.'cfg(unix)'.dependencies]\nea-acciones = { path = \"../ea-acciones\", features = [\"dobles\"] }\n\
        [dev-dependencies]\nea-biblioteca = { path = \"../ea-biblioteca\", features = [\"dobles\"] }\n"
        .parse()
        .unwrap();
    assert_eq!(de_prueba_fuera_de_las_pruebas("x", &m).len(), 3, "{:?}", de_prueba_fuera_de_las_pruebas("x", &m));
    for linea in
        ["#[cfg(unix)]", "#[cfg(not(windows))]", "#![cfg(target_os = \"macos\")]", "use std::os::unix::fs::PermissionsExt;", "unsafe { libc::kill(1, 0) };"]
    {
        assert!(es_de_sistema(linea), "{linea}");
    }
    assert!(!es_de_sistema("#[cfg(test)]"));
    assert!(excepcion("crates/ea-orchestrator/src/ejecutor.rs", "#[cfg(target_os = \"linux\")]"));
    assert!(!excepcion("crates/ea-orchestrator/src/ejecutor.rs", "#[cfg(target_os = \"macos\")]"));
    assert!(!excepcion("crates/ea-orchestrator/src/main.rs", "#[cfg(target_os = \"linux\")]"));
}
