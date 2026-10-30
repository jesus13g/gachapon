//! El bucle de iteraciones, el de una misión, una rutina y cada fase de una
//! obra: llamar al agente hasta que pase el criterio, que corre el sistema.

use ea_bus::git;
use ea_bus::{Informe, Orden, ResultadoInforme};

use super::criterio::{correr_criterio, recortar_resumen};
use super::{Cancelado, Contexto};
use crate::sandbox::{Ocultar, Sandbox};

/// Resultado de un bucle de iteraciones.
pub(super) struct Bucle {
    pub(super) pasa: bool,
    pub(super) evidencia: String,
    pub(super) motivo: Option<String>,
}

/// El prompt no nombra al agente ni al modelo; `regla` es la de las
/// herramientas propias del agente (vacía si no tiene).
fn construir_prompt(orden: &Orden, titulo: &str, cuerpo: &str, criterio: &str, regla: &str) -> String {
    let mut p = format!("# {titulo}\n\n{cuerpo}\n");
    if !orden.notas_revision.is_empty() {
        p.push_str("\n## Notas de la revisión anterior (tenlas en cuenta)\n");
        for n in &orden.notas_revision {
            p.push_str(&format!("- {n}\n"));
        }
    }
    p.push_str("\n## Reglas\n- Trabaja SOLO dentro del directorio actual (el worktree); no toques nada fuera.\n- No hagas commits.\n");
    if !regla.is_empty() {
        p.push_str(&format!("- {regla}\n"));
    }
    p.push_str(&format!("- El criterio de éxito es el comando `{criterio}` y debe pasar.\n- Cuando termines, explica en 3 líneas qué hiciste.\n"));
    p
}

/// Llama al agente hasta que el criterio pase, salgan los topes (55) o se
/// agoten las iteraciones. El criterio lo ejecuta el SISTEMA, no el agente, y
/// su ausencia nunca cuenta como éxito. Las `skills` se vuelven a validar
/// contra la configuración del momento: si ya no valen, no se llama a Qwen.
/// `None` si hay que dejar el trabajo ([`Contexto::cortar`]: se canceló o el
/// orquestador se para): antes de cada iteración se relee el estado, el
/// agente y el criterio lo sondean mientras corren, y tras un corte no se
/// corre el criterio.
pub(super) fn bucle(ctx: &Contexto, titulo: &str, cuerpo: &str, criterio: &str, skills: &[String]) -> Option<Bucle> {
    let Contexto { bus, config, agente, id, orden, artefactos, wt, .. } = *ctx;
    let cortar = || ctx.cortar();
    if criterio.trim().is_empty() {
        return Some(Bucle { pasa: false, evidencia: "la orden no trae criterio de éxito".into(), motivo: Some("sin criterio de éxito".into()) });
    }
    let sistema = match config.skills.resolver(&orden.creada_por, skills) {
        Ok(s) => ea_bus::bloque_prompt(&s),
        Err(e) => return Some(Bucle { pasa: false, evidencia: e.to_string(), motivo: Some(e.to_string()) }),
    };
    if !sistema.is_empty() {
        let _ = artefactos.guardar_skills_agente(&sistema);
    }
    let base = construir_prompt(orden, titulo, cuerpo, criterio, agente.regla_herramientas());
    let max = orden.topes.iteraciones.max(1);
    let mut previa: Option<String> = None;
    let sandbox = Sandbox::actual(config.sandbox);
    let ejecucion = crate::sandbox::dir_ejecucion();
    for it in 1..=max {
        // Solo se anota (y se llama al agente) si sigue `ejecutando`; la
        // comprobación y la escritura van bajo el bloqueo del trabajo.
        if cortar() || matches!(bus.anotar_iteracion(id), Ok(None)) {
            return None;
        }
        let prompt = match &previa {
            None => base.clone(),
            Some(ev) => {
                format!("{base}\n## Intento anterior (iteración {})\nEl criterio sigue fallando con:\n\n```\n{ev}\n```\n\nCorrige lo que falta.\n", it - 1)
            }
        };
        let salida = agente.ejecutar(&prompt, &sistema, wt, &orden.topes, config, &cortar);
        let _ = artefactos.guardar_salida_agente(&salida.bruta);
        if salida.cancelado || cortar() {
            return None;
        }
        let tope = std::time::Duration::from_secs(orden.topes.tiempo_criterio_s);
        let (pasa, ev) = correr_criterio(
            &sandbox,
            wt,
            criterio,
            &config.criterio_escribibles,
            Ocultar { ejecucion: ejecucion.as_deref(), tapar: &config.tapar },
            tope,
            &cortar,
        )?;
        let mut evidencia = ev.clone();
        if !salida.resumen.is_empty() {
            evidencia = recortar_resumen(&format!("{ev}\n--- resumen del agente ---\n{}", salida.resumen.trim()));
        }
        if pasa {
            return Some(Bucle { pasa: true, evidencia, motivo: None });
        }
        if let Some(c) = agente.codigo_topes().filter(|c| salida.codigo == Some(*c)) {
            return Some(Bucle { pasa: false, evidencia, motivo: Some(format!("topes agotados ({c})")) });
        }
        if it == max {
            if salida.codigo != Some(0) {
                evidencia.push_str(&format!("\n(el agente salió con código {:?})", salida.codigo));
            }
            return Some(Bucle { pasa: false, evidencia, motivo: Some("iteraciones agotadas".into()) });
        }
        previa = Some(ev);
    }
    unreachable!("el bucle siempre devuelve en la última iteración")
}

/// Una misión (o una rutina): el bucle con el criterio de la orden y el
/// informe con lo cambiado desde `commit_inicio`.
pub(super) fn trabajar(ctx: &Contexto, commit_inicio: String) -> Result<Informe, Cancelado> {
    let orden = ctx.orden;
    let criterio = orden.criterio_exito.clone().unwrap_or_default();
    let r = bucle(ctx, &orden.titulo, &orden.objetivo, &criterio, &orden.skills).ok_or(Cancelado { fase: None })?;
    let diff_stat = git::diff_stat_desde(ctx.wt, &commit_inicio);
    Ok(Informe {
        evidencia: r.evidencia,
        commit_inicio: Some(commit_inicio),
        diff_stat: Some(diff_stat),
        motivo: r.motivo,
        obra_completa: r.pasa,
        ..Informe::nuevo(if r.pasa { ResultadoInforme::Terminado } else { ResultadoInforme::Bloqueado })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ejecutor::pruebas::*;
    use crate::ejecutor::{Ejecutor, QwenCode, ejecutar_uno};
    use ea_bus::{Bus, EstadoTrabajo, Orden, Rol};
    use std::path::Path;
    use std::process::Command;

    #[test]
    fn el_prompt_no_nombra_al_agente_y_lleva_la_regla_del_adaptador() {
        let o = orden(Path::new("/w"), "test -f ok.txt");
        let neutro = construir_prompt(&o, "t", "c", "test -f ok.txt", "");
        assert!(!neutro.to_lowercase().contains("qwen") && !neutro.contains("run_shell_command"), "{neutro}");
        assert!(neutro.contains("`test -f ok.txt`") && neutro.contains("## Reglas"));
        let qwen = construir_prompt(&o, "t", "c", "x", QwenCode.regla_herramientas());
        assert!(qwen.contains("- Para crear un fichero nuevo usa run_shell_command"));
    }

    #[test]
    fn criterio_que_pasa_da_terminado_y_encola_en_claude_code() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(e.path()).unwrap();
        let id = bus.crear_trabajo(orden(wt.path(), "test -f ok.txt")).unwrap();
        let ag = doble("echo hola > ok.txt", 0);
        assert_eq!(ejecutar_uno(&bus, &config(), &ag), Some(id.clone()));
        assert!(ag.visto.lock().unwrap()[0].contains("usa mayúsculas"));
        assert_eq!(ag.visto.lock().unwrap().len(), 1);
        assert!(inf_obra_completa(&bus, &id));
        assert_eq!(bus.leer_estado(&id).unwrap().estado, EstadoTrabajo::EnRevision);
        let inf = bus.leer_informe(&id).unwrap().unwrap();
        assert_eq!(inf.resultado, ResultadoInforme::Terminado);
        assert!(inf.diff_stat.unwrap().contains("ok.txt"));
        assert!(inf.commit_inicio.is_some());
        assert_eq!(bus.listar_cola(Rol::Planificador).unwrap()[0].trabajo, id);
        assert!(e.path().join("trabajos").join(&id).join(ea_bus::SALIDA_AGENTE).exists());
        let por = config().ejecutor.ejecutado_por();
        assert_eq!(bus.leer_estado(&id).unwrap().ejecutado_por.as_ref(), Some(&por));
        assert_eq!(inf.ejecutado_por.as_ref(), Some(&por), "el informe dice quién lo hizo");
        assert_eq!(ejecutar_uno(&bus, &config(), &ag), None);
    }

    #[test]
    fn el_diff_stat_del_informe_es_el_de_git_mas_los_nuevos() {
        let wt = repo();
        for a in [["config", "core.autocrlf", "false"], ["config", "core.quotePath", "true"]] {
            Command::new("git").args(a).current_dir(wt.path()).status().unwrap();
        }
        let e = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(e.path()).unwrap();
        let id = bus.crear_trabajo(orden(wt.path(), "test -f ok.txt")).unwrap();
        let ag = doble(
            "printf 'a\\nb\\n' > a.txt; echo hola > ok.txt; printf 'ñ\\n' > canción.txt; mkdir -p 'sub dir' __pycache__; \
             echo e > 'sub dir/espacio.txt'; echo p > __pycache__/m.pyc",
            0,
        );
        ejecutar_uno(&bus, &config(), &ag);
        let inf = bus.leer_informe(&id).unwrap().unwrap();
        let inicio = inf.commit_inicio.unwrap();
        let o = Command::new("git").args(["diff", "--stat", &inicio]).current_dir(wt.path()).output().unwrap();
        let stat = String::from_utf8_lossy(&o.stdout).trim().to_string();
        assert!(stat.contains("a.txt"), "{stat}");
        let esperado = format!("{stat}\nnuevos: \"canci\\303\\263n.txt\", ok.txt, sub dir/espacio.txt");
        assert_eq!(inf.diff_stat.as_deref(), Some(esperado.as_str()));
    }

    #[test]
    fn las_skills_elegidas_van_al_system_prompt_y_solo_esas() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let sk = tempfile::tempdir().unwrap();
        let cfg = config_con_skills(sk.path());
        let bus = Bus::abrir_en(e.path()).unwrap().con_skills(cfg.skills.clone());
        let o = Orden { skills: vec!["rust".into()], creada_por: "claude-code".into(), ..orden(wt.path(), "test -f ok.txt") };
        let id = bus.crear_trabajo(o).unwrap();
        let ag = doble("echo hola > ok.txt", 0);
        ejecutar_uno(&bus, &cfg, &ag);
        let s = &ag.sistemas.lock().unwrap()[0];
        assert!(s.contains("Usa cargo test."));
        assert!(!s.contains("Prueba primero."));
        assert!(!ag.visto.lock().unwrap()[0].contains("Usa cargo test."));
        assert!(e.path().join("trabajos").join(&id).join(ea_bus::SKILLS_AGENTE).exists());
        assert_eq!(bus.leer_informe(&id).unwrap().unwrap().resultado, ResultadoInforme::Terminado);
    }

    #[test]
    fn sin_llave_las_skills_bloquean_sin_llamar_a_qwen() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let sk = tempfile::tempdir().unwrap();
        let cfg = config_con_skills(sk.path());
        // Al crear, "test" aún tenía la llave; luego los ajustes cambian y el
        // ejecutor vuelve a validar con los de ahora.
        let mut al_crear = cfg.skills.clone();
        al_crear.pueden_elegir.push("test".into());
        let bus = Bus::abrir_en(e.path()).unwrap().con_skills(al_crear);
        let o = Orden { skills: vec!["rust".into()], ..orden(wt.path(), "test -f ok.txt") };
        let id = bus.crear_trabajo(o).unwrap();
        let ag = doble("echo hola > ok.txt", 0);
        ejecutar_uno(&bus, &cfg, &ag);
        assert!(ag.visto.lock().unwrap().is_empty());
        let inf = bus.leer_informe(&id).unwrap().unwrap();
        assert_eq!(inf.resultado, ResultadoInforme::Bloqueado);
        assert!(inf.motivo.unwrap().contains("llave"));
    }

    #[test]
    fn criterio_que_falla_da_bloqueado_aunque_el_agente_diga_exito() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(e.path()).unwrap();
        let id = bus.crear_trabajo(orden(wt.path(), "echo roto; exit 1")).unwrap();
        ejecutar_uno(&bus, &config(), &doble("true", 0));
        let inf = bus.leer_informe(&id).unwrap().unwrap();
        assert_eq!(inf.resultado, ResultadoInforme::Bloqueado);
        assert_eq!(inf.motivo.as_deref(), Some("iteraciones agotadas"));
        assert!(inf.evidencia.contains("roto"));
        assert_eq!(bus.listar_cola(Rol::Planificador).unwrap()[0].motivo, "bloqueado");
    }

    #[test]
    fn topes_y_error_de_qwen_dan_su_motivo() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(e.path()).unwrap();
        let a = bus.crear_trabajo(orden(wt.path(), "false")).unwrap();
        ejecutar_uno(&bus, &config(), &doble("true", 55));
        assert_eq!(bus.leer_informe(&a).unwrap().unwrap().motivo.as_deref(), Some("topes agotados (55)"));
        let b = bus.crear_trabajo(orden(wt.path(), "false")).unwrap();
        ejecutar_uno(&bus, &config(), &doble("true", 1));
        assert_eq!(bus.leer_informe(&b).unwrap().unwrap().motivo.as_deref(), Some("iteraciones agotadas"));
        assert!(bus.leer_informe(&b).unwrap().unwrap().evidencia.contains("código"));
    }

    #[test]
    fn mision_pasa_en_la_segunda_iteracion_con_la_evidencia_del_fallo() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(e.path()).unwrap();
        let id = bus.crear_trabajo(orden(wt.path(), "test -f ok.txt || { echo FALTA-OK; exit 1; }")).unwrap();
        let ag = doble("if [ -f marca ]; then echo hola > ok.txt; else touch marca; fi", 0);
        ejecutar_uno(&bus, &config(), &ag);
        let vistos = ag.visto.lock().unwrap();
        assert_eq!(vistos.len(), 2);
        assert!(!vistos[0].contains("sigue fallando"));
        assert!(vistos[1].contains("El criterio sigue fallando con") && vistos[1].contains("FALTA-OK"));
        let inf = bus.leer_informe(&id).unwrap().unwrap();
        assert_eq!(inf.resultado, ResultadoInforme::Terminado);
        assert_eq!(bus.leer_estado(&id).unwrap().iteracion, 2);
        assert_eq!(bus.listar_cola(Rol::Planificador).unwrap().len(), 1);
    }

    #[test]
    fn mision_que_agota_iteraciones_se_entrega_bloqueada() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(e.path()).unwrap();
        let mut o = orden(wt.path(), "false");
        o.topes.iteraciones = 3;
        let id = bus.crear_trabajo(o).unwrap();
        let ag = doble("true", 0);
        ejecutar_uno(&bus, &config(), &ag);
        assert_eq!(ag.visto.lock().unwrap().len(), 3);
        let inf = bus.leer_informe(&id).unwrap().unwrap();
        assert_eq!((inf.resultado, inf.motivo.as_deref()), (ResultadoInforme::Bloqueado, Some("iteraciones agotadas")));
        assert_eq!(bus.leer_estado(&id).unwrap().iteracion, 3);
    }

    #[test]
    fn codigo_55_entrega_sin_reintentar() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(e.path()).unwrap();
        bus.crear_trabajo(orden(wt.path(), "false")).unwrap();
        let ag = doble("true", 55);
        ejecutar_uno(&bus, &config(), &ag);
        assert_eq!(ag.visto.lock().unwrap().len(), 1);
    }

    #[test]
    fn cancelar_entre_iteraciones_no_vuelve_a_llamar_al_agente() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = con_store(e.path());
        let mut o = orden(wt.path(), "touch criterio-corrio; false");
        o.topes.iteraciones = 3;
        let id = bus.crear_trabajo(o).unwrap();
        let ag = cancelador(&bus);
        assert_eq!(ejecutar_uno(&bus, &config(), &ag), Some(id.clone()));
        assert_eq!(ag.llamadas.load(std::sync::atomic::Ordering::SeqCst), 1);
        // Tras la cancelación no se corre el criterio.
        assert!(!wt.path().join("criterio-corrio").exists());
        queda_cancelado_y_suelto(&bus, &id, wt.path());
        assert_eq!(bus.leer_estado(&id).unwrap().iteracion, 1);
        let c = cancelaciones_en_curso(e.path());
        assert_eq!(c.len(), 1);
        assert_eq!((c[0]["trabajo"].as_str(), c[0]["iteracion"].as_u64(), c[0]["fase"].is_null()), (Some(id.as_str()), Some(1), true));
        let historial = std::fs::read_to_string(bus.raiz().join("trabajos").join(&id).join("historial.jsonl")).unwrap();
        assert!(historial.lines().last().unwrap().contains("cancelado_en_curso"));
    }
}
