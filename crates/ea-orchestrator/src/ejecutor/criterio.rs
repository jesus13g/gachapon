//! El criterio de éxito: lo ejecuta el SISTEMA, no el agente, con tope de
//! tiempo y cancelable; su salida, recortada, es la evidencia del informe.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::avisar_sin_arbol;
use crate::sandbox::{Ocultar, Sandbox};

const MAX_EVIDENCIA: usize = 2048;

/// Últimos `MAX_EVIDENCIA` bytes (en límite de carácter): la cola de la salida
/// es donde suele estar el fallo.
fn recortar(s: &str) -> String {
    let s = s.trim();
    if s.len() <= MAX_EVIDENCIA {
        return s.to_string();
    }
    let mut i = s.len() - MAX_EVIDENCIA;
    while !s.is_char_boundary(i) {
        i += 1;
    }
    format!("…{}", &s[i..])
}

/// Hasta `max` bytes de `s`, en límite de carácter.
pub(super) fn corte(s: &str, max: usize) -> usize {
    let mut i = max.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// La evidencia lleva el criterio (≤2 KB) y el resumen del agente (≤1 KB más).
pub(super) fn recortar_resumen(s: &str) -> String {
    let max = MAX_EVIDENCIA + 1024;
    if s.len() <= max {
        return s.to_string();
    }
    format!("{}…", &s[..corte(s, max)])
}

/// Corre el criterio de éxito con `sh -c` en `wt`, con un tope de `tope`
/// (`topes.tiempo_criterio_s` de la orden, ya recortado al techo) y cancelable: mientras corre se pregunta a `cancelado`
/// y, si dice que sí, se mata su árbol y devuelve `None` (sin evidencia).
/// Si no, `Some((pasa, evidencia))`.
///
/// Con el agente aislado (`sandbox` es bwrap o Seatbelt), el criterio corre
/// en el mismo sandbox ([`Sandbox::envolver_criterio`]): ejecuta tests y
/// guiones que ha podido escribir el agente, así que solo escribe en el
/// worktree (no en su `.git`), en su `/tmp` y en `escribibles`
/// (`agente.criterio.escribibles`, canónicas), y no ve lo de `ocultar`: el
/// directorio de ejecución (la clave del panel, D-Bus, `systemd --user`) y
/// las rutas de `agente.tapar`; la red, como el usuario. Una ruta de `escribibles` que ya no está o que ahora lleva a
/// otro sitio (un enlace cambiado) no se monta: el criterio no se corre y no
/// pasa. Sin sandbox, tal cual; con uno que no está disponible no se corre y
/// no pasa.
pub fn correr_criterio(
    sandbox: &Sandbox,
    wt: &Path,
    criterio: &str,
    escribibles: &[PathBuf],
    ocultar: Ocultar,
    tope: Duration,
    cancelado: &dyn Fn() -> bool,
) -> Option<(bool, String)> {
    let Some(mut c) = ea_plataforma::shell::orden_sh(&format!("exec 2>&1; {criterio}")) else {
        // Sin `sh` el criterio no se puede comprobar: nunca cuenta como éxito.
        return Some((false, "no hay `sh`: instala Git for Windows (el criterio se ejecuta con sh)".into()));
    };
    c.current_dir(wt);
    if *sandbox != Sandbox::Ninguno
        && let Some(p) = escribibles.iter().find(|p| ea_plataforma::rutas::canonica(p).ok().as_deref() != Some(p.as_path()))
    {
        return Some((false, format!("la ruta escribible del criterio {} ya no existe o ha cambiado (agente.criterio.escribibles): no se corre", p.display())));
    }
    let envoltorio = match sandbox.envolver_criterio(c, wt, escribibles, ocultar) {
        Ok(e) => e,
        Err(motivo) => return Some((false, motivo)),
    };
    // La orden sale del envoltorio; su temporal (macOS) vive hasta el final.
    let r = ea_plataforma::proceso::correr(envoltorio.orden, tope, cancelado);
    avisar_sin_arbol(&r);
    if r.cancelado {
        return None;
    }
    let mut texto = format!("{}{}", r.salida, r.errores);
    if r.codigo.is_none() {
        texto.push_str(&format!("\n(criterio interrumpido por tiempo: {} s, topes.tiempo_criterio_s)", tope.as_secs()));
    }
    Some((r.codigo == Some(0), recortar(&texto)))
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crate::ejecutor::ejecutar_uno;
    use crate::ejecutor::pruebas::*;
    use ea_bus::{Bus, Evento};

    #[test]
    fn cancelar_durante_el_criterio_no_da_error_al_entregar() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = con_store(e.path());
        let mut o = orden(wt.path(), "touch marca; sleep 1; true");
        o.topes.iteraciones = 1;
        let id = bus.crear_trabajo(o).unwrap();
        let hilo = cancelar_al_ver(bus.raiz(), &id, wt.path().join("marca"));
        ejecutar_uno(&bus, &config(), &doble("true", 0));
        assert!(hilo.join().unwrap(), "no se pudo cancelar");
        // La entrega llega tarde: el trabajo se queda cancelado, sin informe.
        queda_cancelado_y_suelto(&bus, &id, wt.path());
        assert_eq!(cancelaciones_en_curso(e.path()).len(), 1);
    }

    /// B-10: cancelar mientras corre un criterio largo lo corta en el
    /// siguiente sondeo (2 s), sin esperar a que acabe ni a su tope (120 s),
    /// y sigue el camino de la cancelación: sin informe y anotada una vez.
    #[test]
    fn cancelar_durante_un_criterio_largo_lo_corta() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = con_store(e.path());
        let mut o = orden(wt.path(), "touch marca; sleep 30; touch acabo");
        o.topes.iteraciones = 1;
        let id = bus.crear_trabajo(o).unwrap();
        let (raiz, id2, marca) = (bus.raiz().to_path_buf(), id.clone(), wt.path().join("marca"));
        let hilo = std::thread::spawn(move || {
            assert!(esperar_fichero(&marca, Duration::from_secs(20)), "el criterio no empezó");
            std::thread::sleep(Duration::from_secs(1));
            Bus::abrir_en(&raiz).unwrap().aplicar(&id2, Evento::Cancelar).unwrap();
            Instant::now()
        });
        ejecutar_uno(&bus, &config(), &doble("true", 0));
        let cancelado_en = hilo.join().unwrap();
        assert!(cancelado_en.elapsed() < Duration::from_secs(5), "tardó {:?} desde la cancelación", cancelado_en.elapsed());
        assert!(!wt.path().join("acabo").exists());
        queda_cancelado_y_suelto(&bus, &id, wt.path());
        assert_eq!(cancelaciones_en_curso(e.path()).len(), 1);
    }

    /// U-09: el criterio se corta al vencer `topes.tiempo_criterio_s`, ya
    /// recortado al techo de los ajustes (aquí la orden pide 600 s y el techo
    /// deja 1), y la evidencia lo dice.
    #[test]
    fn un_criterio_que_supera_su_tope_se_corta() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = con_store(e.path());
        let mut o = orden(wt.path(), "sleep 30; touch acabo");
        (o.topes.iteraciones, o.topes.tiempo_criterio_s) = (1, 600);
        let id = bus.crear_trabajo(o).unwrap();
        let mut cfg = config();
        cfg.techo_topes.tiempo_criterio_s = 1;
        let inicio = Instant::now();
        ejecutar_uno(&bus, &cfg, &doble("true", 0));
        assert!(inicio.elapsed() < Duration::from_secs(15), "tardó {:?}", inicio.elapsed());
        let inf = bus.leer_informe(&id).unwrap().unwrap();
        assert_eq!(inf.resultado, ea_bus::ResultadoInforme::Bloqueado);
        assert!(inf.evidencia.contains("(criterio interrumpido por tiempo: 1 s, topes.tiempo_criterio_s)"), "{}", inf.evidencia);
        assert!(!wt.path().join("acabo").exists());
    }
}
