//! Una obra por fases: el bucle de [`super::mision`] en cada fase, una
//! instantánea del árbol al cerrarla y una parada de revisión cada
//! `revisar_cada` fases (o al terminar).

use std::path::Path;

use ea_bus::git::{self, instantanea};
use ea_bus::{EstadoFase, FaseInforme, Informe, InformeFase, ResultadoInforme};

use super::criterio::corte;
use super::mision::bucle;
use super::{Cancelado, Contexto, bloqueado};

const MAX_DIFF: usize = 100_000;

/// El diff (o su `--stat`) entre dos árboles, sin espacios a los lados;
/// vacío si git falla.
fn diff_entre(wt: &Path, a: &str, b: &str, stat: bool) -> String {
    git::diff_entre(wt, a, b, stat, git::TOPE_LECTURA).map(|s| s.trim().to_string()).unwrap_or_default()
}

/// La obra desde `fase_actual`, con parada de revisión cada `revisar_cada`
/// fases hechas (o al terminar todas). Antes de cada fase se mira
/// [`Contexto::cortar`]: si se ha cancelado (o el orquestador se para), se
/// corta sin informe.
pub(super) fn trabajar(ctx: &Contexto, fase_actual: Option<u32>, commit_inicio: String) -> Result<Informe, Cancelado> {
    let Contexto { bus, config, id, orden, artefactos, wt, .. } = *ctx;
    let dir = artefactos.dir();
    let mut fases = orden.fases.clone();
    fases.sort_by_key(|f| f.numero);
    let total = fases.len();
    // `Bus::reformular` deja `fase_actual` en la fase a rehacer (la pedida o
    // la pendiente; en una obra completa, la última). Sin ella, la última.
    let inicio_idx = fase_actual.and_then(|n| fases.iter().position(|f| f.numero == n)).unwrap_or(total - 1);

    let mut previo = match artefactos.arbol_ultimo() {
        Some(arbol) => arbol,
        None => {
            let Some(arbol) = instantanea(wt, dir) else {
                return Ok(bloqueado("no se pudo guardar la instantánea del árbol", String::new()));
            };
            let _ = artefactos.guardar_arbol_ultimo(&arbol);
            arbol
        }
    };
    let base_revision = artefactos.arbol_revision().unwrap_or_else(|| {
        let _ = artefactos.guardar_arbol_revision(&previo);
        previo.clone()
    });
    // Fases ya informadas de ejecuciones anteriores (las que sigan vigentes).
    let mut informadas = artefactos.fases_informe().unwrap_or_default();
    informadas.retain(|f| f.numero < fases[inicio_idx].numero);

    let mut fallo: Option<(String, String)> = None;
    let mut evidencia = String::new();
    let mut completa = false;
    let mut idx = inicio_idx;
    while idx < total {
        let f = &fases[idx];
        if ctx.cortar() {
            return Err(Cancelado { fase: Some(f.numero) });
        }
        let titulo = format!("{} · fase {} de {}: {}", orden.titulo, idx + 1, total, f.titulo);
        let cuerpo =
            format!("## Objetivo general (contexto)\n{}\n\n## Esta fase: {}\n{}\n\nEntregable: {}", orden.objetivo, f.titulo, f.instrucciones, f.entregable);
        let skills = if f.skills.is_empty() { &orden.skills } else { &f.skills };
        let r = bucle(ctx, &titulo, &cuerpo, &f.criterio, skills).ok_or(Cancelado { fase: Some(f.numero) })?;
        evidencia = r.evidencia.clone();
        if !r.pasa {
            let stat = instantanea(wt, dir).map(|t| diff_entre(wt, &previo, &t, true)).unwrap_or_default();
            let informe = InformeFase { motivo: r.motivo.clone(), diff_stat: Some(stat.clone()), arbol: None };
            let _ = bus.registrar_fase(id, f.numero, EstadoFase::Bloqueada, &informe);
            informadas.push(FaseInforme {
                numero: f.numero,
                titulo: f.titulo.clone(),
                resultado: EstadoFase::Bloqueada,
                diff_stat: stat,
                ejecutado_por: Some(config.ejecutor.ejecutado_por()),
            });
            fallo = Some((format!("fase {}: {}", f.numero, r.motivo.unwrap_or_default()), r.evidencia));
            break;
        }
        let Some(arbol) = instantanea(wt, dir) else {
            fallo = Some((format!("fase {}: no se pudo guardar la instantánea", f.numero), r.evidencia));
            break;
        };
        let stat = diff_entre(wt, &previo, &arbol, true);
        let informe = InformeFase { diff_stat: Some(stat.clone()), arbol: Some(arbol.clone()), motivo: None };
        let _ = bus.registrar_fase(id, f.numero, EstadoFase::Hecha, &informe);
        informadas.push(FaseInforme {
            numero: f.numero,
            titulo: f.titulo.clone(),
            resultado: EstadoFase::Hecha,
            diff_stat: stat,
            ejecutado_por: Some(config.ejecutor.ejecutado_por()),
        });
        previo = arbol;
        let _ = artefactos.guardar_arbol_ultimo(&previo);
        idx += 1;
        let hechas = idx as u32;
        if idx == total {
            completa = true;
            break;
        }
        if orden.revisar_cada.is_some_and(|k| k > 0 && hechas.is_multiple_of(k)) {
            break;
        }
    }
    let _ = artefactos.guardar_fases_informe(&informadas);

    let actual = instantanea(wt, dir).unwrap_or_else(|| previo.clone());
    let stat = diff_entre(wt, &base_revision, &actual, true);
    let parche = diff_entre(wt, &base_revision, &actual, false);
    let parche = if parche.len() > MAX_DIFF { format!("{}\n…(recortado)", &parche[..corte(&parche, MAX_DIFF)]) } else { parche };
    let hechas = informadas.iter().filter(|f| f.resultado == EstadoFase::Hecha).count() as u32;
    if fallo.is_none() {
        // Parada de revisión (o fin): la próxima revisión parte de aquí.
        let _ = artefactos.guardar_arbol_revision(&actual);
    }
    let (resultado, motivo) = match &fallo {
        Some((m, _)) => (ResultadoInforme::Bloqueado, Some(m.clone())),
        None if completa => (ResultadoInforme::Terminado, None),
        None => (ResultadoInforme::Terminado, Some(format!("revisión intermedia tras {hechas} fases; quedan {}", total as u32 - hechas))),
    };
    if let Some((_, ev)) = fallo {
        evidencia = ev;
    }
    Ok(Informe {
        evidencia,
        commit_inicio: Some(commit_inicio),
        diff_stat: Some(stat),
        motivo,
        fases_hechas: hechas,
        obra_completa: completa,
        fases: informadas,
        diff: Some(parche),
        ..Informe::nuevo(resultado)
    })
}

#[cfg(test)]
mod tests {
    use crate::ejecutor::ejecutar_uno;
    use crate::ejecutor::pruebas::*;
    use ea_bus::git;
    use ea_bus::{Bus, EstadoFase, EstadoTrabajo, Orden, ResultadoInforme};

    #[test]
    fn cada_fase_de_obra_puede_llevar_sus_skills() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let sk = tempfile::tempdir().unwrap();
        let cfg = config_con_skills(sk.path());
        let bus = Bus::abrir_en(e.path()).unwrap().con_skills(cfg.skills.clone());
        let mut o = Orden { skills: vec!["rust".into()], creada_por: "claude-code".into(), ..fases3(wt.path(), None) };
        o.fases[1].skills = vec!["tests".into()];
        bus.crear_trabajo(o).unwrap();
        let ag = doble(SIGUIENTE, 0);
        ejecutar_uno(&bus, &cfg, &ag);
        let s = ag.sistemas.lock().unwrap();
        assert_eq!(s.len(), 3);
        assert!(s[0].contains("cargo") && !s[0].contains("Prueba"));
        assert!(s[1].contains("Prueba") && !s[1].contains("cargo"));
        assert!(s[2].contains("cargo"));
    }

    #[test]
    fn obra_de_tres_fases_para_tras_la_segunda_y_continuar_termina() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = con_store(e.path());
        let id = bus.crear_trabajo(fases3(wt.path(), Some(2))).unwrap();
        let ag = doble(SIGUIENTE, 0);
        ejecutar_uno(&bus, &config(), &ag);
        let inf = bus.leer_informe(&id).unwrap().unwrap();
        assert_eq!((inf.resultado, inf.fases_hechas, inf.obra_completa), (ResultadoInforme::Terminado, 2, false));
        assert_eq!(inf.fases.len(), 2);
        assert!(inf.diff.as_deref().unwrap().contains("f1") && inf.diff.as_deref().unwrap().contains("f2"));
        assert!(!inf.diff.as_deref().unwrap().contains("+++ b/f3"));
        let st = bus.leer_estado(&id).unwrap();
        assert_eq!((st.estado, st.fase_actual), (EstadoTrabajo::EnRevision, Some(3)));
        assert_eq!(ag.visto.lock().unwrap().len(), 2);
        assert!(ag.visto.lock().unwrap()[1].contains("fase 2 de 3"));
        let fases = ea_store::Store::open_at(&e.path().join("ea.db")).unwrap().listar_fases(&id).unwrap();
        assert_eq!(fases.iter().map(|f| f.estado.as_str()).collect::<Vec<_>>(), ["hecha", "hecha", "pendiente"]);
        assert!(fases[1].informe.as_deref().unwrap().contains("f2"));
        // El índice del usuario sigue intacto.
        let staged = git::git(wt.path(), ["diff", "--cached", "--name-only"], git::TOPE_LECTURA).unwrap();
        assert!(staged.is_empty(), "índice tocado: {staged}");

        bus.reformular(&id, "", None).unwrap(); // = ea trabajo continuar
        // Entre medias cambia el ejecutor: la fase 3 la hace el nuevo.
        let mut otro = config();
        otro.ejecutor.id = "codex".into();
        ejecutar_uno(&bus, &otro, &ag);
        let inf = bus.leer_informe(&id).unwrap().unwrap();
        assert_eq!((inf.fases_hechas, inf.obra_completa, inf.motivo), (3, true, None));
        assert_eq!(inf.fases.len(), 3);
        let quien: Vec<_> = inf.fases.iter().map(|f| f.ejecutado_por.as_ref().unwrap().agente.as_str()).collect();
        assert_eq!(quien, ["qwen-code", "qwen-code", "codex"]);
        let d = inf.diff.unwrap();
        assert!(d.contains("+++ b/f3") && !d.contains("+++ b/f1"));
        assert_eq!(bus.leer_estado(&id).unwrap().fase_actual, Some(4));

        // Un ejecutor que no se puede lanzar no gasta iteraciones: el trabajo
        // queda bloqueado con el motivo.
        let id = bus.crear_trabajo(fases3(wt.path(), None)).unwrap();
        otro.ejecutor.bin = "/no/existe/codex".into();
        // Windows rechaza `sandbox = true` antes de buscar el binario (CE-003).
        if cfg!(windows) {
            otro.sandbox = false;
        }
        ejecutar_uno(&bus, &otro, &crate::agentes::Codex);
        let inf = bus.leer_informe(&id).unwrap().unwrap();
        assert_eq!((inf.resultado, inf.fases_hechas), (ResultadoInforme::Bloqueado, 0));
        let motivo = inf.motivo.unwrap();
        assert!(motivo.contains("codex no está disponible") && motivo.contains("/no/existe/codex"), "{motivo}");
        assert_eq!(bus.leer_estado(&id).unwrap().iteracion, 0);
    }

    #[test]
    fn reformular_una_obra_completa_desde_la_fase_2_rehace_la_2_y_la_3() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = con_store(e.path());
        let id = bus.crear_trabajo(fases3(wt.path(), None)).unwrap();
        let ag = doble(SIGUIENTE, 0);
        ejecutar_uno(&bus, &config(), &ag);
        assert!(inf_obra_completa(&bus, &id));
        assert_eq!(ag.visto.lock().unwrap().len(), 3);

        bus.reformular(&id, "rehaz desde la 2", Some(2)).unwrap();
        ejecutar_uno(&bus, &config(), &ag);
        let vistos = ag.visto.lock().unwrap();
        assert_eq!(vistos.len(), 5, "solo las fases 2 y 3 vuelven al agente");
        assert!(vistos[3].contains("fase 2 de 3") && vistos[4].contains("fase 3 de 3"));
        let inf = bus.leer_informe(&id).unwrap().unwrap();
        assert_eq!((inf.fases_hechas, inf.obra_completa), (3, true));
        assert_eq!(inf.fases.iter().map(|f| f.numero).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(bus.leer_estado(&id).unwrap().fase_actual, Some(4));
        let fases = ea_store::Store::open_at(&e.path().join("ea.db")).unwrap().listar_fases(&id).unwrap();
        assert!(fases.iter().all(|f| f.estado == "hecha"));
    }

    #[test]
    fn obra_con_una_fase_fallida_se_entrega_bloqueada() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(e.path()).unwrap();
        let id = bus.crear_trabajo(fases3(wt.path(), None)).unwrap();
        let ag = doble("[ -f f1 ] || echo x > f1", 0);
        ejecutar_uno(&bus, &config(), &ag);
        let inf = bus.leer_informe(&id).unwrap().unwrap();
        assert_eq!(inf.resultado, ResultadoInforme::Bloqueado);
        assert!(inf.motivo.unwrap().starts_with("fase 2: iteraciones agotadas"));
        assert_eq!((inf.fases_hechas, inf.obra_completa), (1, false));
        assert_eq!(inf.fases.iter().map(|f| f.resultado).collect::<Vec<_>>(), [EstadoFase::Hecha, EstadoFase::Bloqueada]);
        assert_eq!(bus.leer_estado(&id).unwrap().fase_actual, Some(2));
    }

    #[test]
    fn cancelar_una_obra_no_empieza_la_fase_siguiente() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = con_store(e.path());
        let id = bus.crear_trabajo(fases3(wt.path(), None)).unwrap();
        let ag = cancelador(&bus);
        ejecutar_uno(&bus, &config(), &ag);
        assert_eq!(ag.llamadas.load(std::sync::atomic::Ordering::SeqCst), 1);
        queda_cancelado_y_suelto(&bus, &id, wt.path());
        assert_eq!(cancelaciones_en_curso(e.path())[0]["fase"].as_u64(), Some(1));
        // La fase cortada no se registra como hecha ni bloqueada.
        let fases = ea_store::Store::open_at(&e.path().join("ea.db")).unwrap().listar_fases(&id).unwrap();
        assert!(fases.iter().all(|f| f.informe.is_none()), "{fases:?}");
    }
}
