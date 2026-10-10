//! El bucle de una ejecución de rutina: recorre el grafo de pasos (§9.2),
//! aplicando los presupuestos duros y el guard anti-ciclos exigidos por
//! §11.2 ("el bucle infinito debe ser imposible, no improbable"). No sabe
//! nada de `ea-store` ni de HTTP/MCP concretos — recibe las dos acciones
//! de efecto (invocar herramienta, elegir rama) como implementaciones de
//! trait, para poder probarse con dobles de prueba sin red real.

use std::collections::HashSet;
use std::future::Future;
use std::time::{Duration, Instant};

use ea_store::RunOutcome;

use crate::plantilla::{self, Contexto};
use crate::rutina::{Paso, Rutina};

/// Ejecuta un paso `accion`: la única forma en que una rutina toca el
/// mundo exterior en esta fase, siempre de solo lectura (validado al
/// cargar la rutina, ver `rutina::HERRAMIENTAS_PERMITIDAS`).
pub trait EjecutorHerramientas {
    fn llamar(
        &self,
        herramienta: &str,
        argumentos: serde_json::Value,
    ) -> impl Future<Output = anyhow::Result<serde_json::Value>> + Send;
}

/// Resuelve un paso `decision`: elige una rama de un conjunto cerrado,
/// acotada por gramática en `ea-proxy` — nunca por instrucción en el
/// prompt (§9.2).
pub trait EjecutorDecisiones {
    fn elegir_rama(
        &self,
        contexto: &str,
        opciones: &[String],
    ) -> impl Future<Output = anyhow::Result<String>> + Send;
}

#[derive(Debug, Clone)]
pub struct PasoEjecutado {
    pub id: String,
    pub tipo: &'static str,
    pub detalle: serde_json::Value,
}

#[derive(Debug)]
pub struct ResultadoEjecucion {
    pub outcome: RunOutcome,
    pub motivo_fallo: Option<String>,
    pub pasos: Vec<PasoEjecutado>,
    pub propuestas: Vec<String>,
    pub llamadas_modelo: i64,
}

fn fallo(
    pasos: Vec<PasoEjecutado>,
    propuestas: Vec<String>,
    llamadas_modelo: i64,
    motivo: String,
) -> ResultadoEjecucion {
    ResultadoEjecucion {
        outcome: RunOutcome::Fallida,
        motivo_fallo: Some(motivo),
        pasos,
        propuestas,
        llamadas_modelo,
    }
}

pub async fn ejecutar<H, D>(
    rutina: &Rutina,
    ultima_ejecucion: &str,
    herramientas: &H,
    decisiones: &D,
) -> ResultadoEjecucion
where
    H: EjecutorHerramientas,
    D: EjecutorDecisiones,
{
    let inicio = Instant::now();
    let presupuesto_tiempo = Duration::from_secs(rutina.max_duracion_s);

    let mut pasos_log: Vec<PasoEjecutado> = Vec::new();
    let mut propuestas: Vec<String> = Vec::new();
    let mut visitados: HashSet<String> = HashSet::new();
    let mut llamadas_modelo: i64 = 0;
    let mut salida_previa = String::new();
    let mut actual = rutina.inicio.clone();

    loop {
        if inicio.elapsed() > presupuesto_tiempo {
            return fallo(pasos_log, propuestas, llamadas_modelo, "presupuesto de tiempo agotado".into());
        }
        if pasos_log.len() as u32 >= rutina.max_pasos {
            return fallo(pasos_log, propuestas, llamadas_modelo, "presupuesto de pasos agotado".into());
        }
        if !visitados.insert(actual.clone()) {
            return fallo(
                pasos_log,
                propuestas,
                llamadas_modelo,
                format!("bucle detectado: el paso '{actual}' se repite en la misma ejecución"),
            );
        }

        let Some(paso) = rutina.pasos.get(&actual) else {
            return fallo(pasos_log, propuestas, llamadas_modelo, format!("paso '{actual}' no existe"));
        };

        match paso {
            Paso::Fin => {
                pasos_log.push(PasoEjecutado {
                    id: actual.clone(),
                    tipo: "fin",
                    detalle: serde_json::Value::Null,
                });
                return ResultadoEjecucion {
                    outcome: RunOutcome::Ok,
                    motivo_fallo: None,
                    pasos: pasos_log,
                    propuestas,
                    llamadas_modelo,
                };
            }

            Paso::Accion { herramienta, argumentos, siguiente } => {
                let ctx = Contexto { ultima_ejecucion, salida_previa: &salida_previa };
                let args_render = plantilla::renderizar_valor(argumentos, &ctx);
                match herramientas.llamar(herramienta, args_render.clone()).await {
                    Ok(resultado) => {
                        salida_previa = resultado.to_string();
                        pasos_log.push(PasoEjecutado {
                            id: actual.clone(),
                            tipo: "accion",
                            detalle: serde_json::json!({
                                "herramienta": herramienta,
                                "argumentos": args_render,
                                "resultado": resultado,
                            }),
                        });
                        actual = siguiente.clone();
                    }
                    Err(e) => {
                        pasos_log.push(PasoEjecutado {
                            id: actual.clone(),
                            tipo: "accion",
                            detalle: serde_json::json!({ "herramienta": herramienta, "error": e.to_string() }),
                        });
                        return fallo(
                            pasos_log,
                            propuestas,
                            llamadas_modelo,
                            format!("fallo al invocar '{herramienta}': {e}"),
                        );
                    }
                }
            }

            Paso::Decision { contexto, ramas } => {
                if llamadas_modelo >= rutina.max_llamadas_modelo as i64 {
                    return fallo(
                        pasos_log,
                        propuestas,
                        llamadas_modelo,
                        "presupuesto de llamadas al modelo agotado".into(),
                    );
                }
                let ctx = Contexto { ultima_ejecucion, salida_previa: &salida_previa };
                let contexto_render = plantilla::renderizar(contexto, &ctx);
                let opciones: Vec<String> = ramas.keys().cloned().collect();
                llamadas_modelo += 1;

                match decisiones.elegir_rama(&contexto_render, &opciones).await {
                    Ok(rama) => match ramas.get(&rama) {
                        Some(siguiente) => {
                            pasos_log.push(PasoEjecutado {
                                id: actual.clone(),
                                tipo: "decision",
                                detalle: serde_json::json!({ "rama_elegida": rama }),
                            });
                            actual = siguiente.clone();
                        }
                        None => {
                            pasos_log.push(PasoEjecutado {
                                id: actual.clone(),
                                tipo: "decision",
                                detalle: serde_json::json!({
                                    "rama_elegida": rama,
                                    "error": "fuera del conjunto cerrado",
                                }),
                            });
                            return fallo(
                                pasos_log,
                                propuestas,
                                llamadas_modelo,
                                format!("el modelo eligió una rama fuera del conjunto cerrado: '{rama}'"),
                            );
                        }
                    },
                    Err(e) => {
                        pasos_log.push(PasoEjecutado {
                            id: actual.clone(),
                            tipo: "decision",
                            detalle: serde_json::json!({ "error": e.to_string() }),
                        });
                        return fallo(pasos_log, propuestas, llamadas_modelo, format!("fallo al decidir rama: {e}"));
                    }
                }
            }

            Paso::Propuesta { descripcion, siguiente } => {
                let ctx = Contexto { ultima_ejecucion, salida_previa: &salida_previa };
                let descripcion_render = plantilla::renderizar(descripcion, &ctx);
                pasos_log.push(PasoEjecutado {
                    id: actual.clone(),
                    tipo: "propuesta",
                    detalle: serde_json::json!({ "descripcion": descripcion_render }),
                });
                propuestas.push(descripcion_render);
                match siguiente {
                    Some(s) => actual = s.clone(),
                    None => {
                        return ResultadoEjecucion {
                            outcome: RunOutcome::Ok,
                            motivo_fallo: None,
                            pasos: pasos_log,
                            propuestas,
                            llamadas_modelo,
                        };
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicI64, Ordering};

    use super::*;

    struct HerramientasFalsas;
    impl EjecutorHerramientas for HerramientasFalsas {
        async fn llamar(&self, _herramienta: &str, _argumentos: serde_json::Value) -> anyhow::Result<serde_json::Value> {
            Ok(serde_json::json!({ "eventos": [{"outcome": "error"}] }))
        }
    }

    struct DecisionesFijas(&'static str);
    impl EjecutorDecisiones for DecisionesFijas {
        async fn elegir_rama(&self, _contexto: &str, _opciones: &[String]) -> anyhow::Result<String> {
            Ok(self.0.to_string())
        }
    }

    struct DecisionesContadas(AtomicI64);
    impl EjecutorDecisiones for DecisionesContadas {
        async fn elegir_rama(&self, _contexto: &str, opciones: &[String]) -> anyhow::Result<String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(opciones[0].clone())
        }
    }

    fn rutina_ejemplo() -> Rutina {
        let mut pasos = HashMap::new();
        pasos.insert(
            "consultar".to_string(),
            Paso::Accion {
                herramienta: "ea_listar_diario".to_string(),
                argumentos: serde_json::json!({ "desde": "{ultima_ejecucion}" }),
                siguiente: "clasificar".to_string(),
            },
        );
        pasos.insert(
            "clasificar".to_string(),
            Paso::Decision {
                contexto: "¿hay errores? {salida_previa}".to_string(),
                ramas: HashMap::from([
                    ("hay_errores".to_string(), "proponer".to_string()),
                    ("sin_errores".to_string(), "fin".to_string()),
                ]),
            },
        );
        pasos.insert(
            "proponer".to_string(),
            Paso::Propuesta {
                descripcion: "revisar desde {ultima_ejecucion}".to_string(),
                siguiente: None,
            },
        );
        pasos.insert("fin".to_string(), Paso::Fin);

        Rutina {
            slug: "ejemplo".to_string(),
            descripcion: "de prueba".to_string(),
            inicio: "consultar".to_string(),
            max_duracion_s: 60,
            max_pasos: 10,
            max_llamadas_modelo: 5,
            ventana_inicial_dias: 1,
            pasos,
        }
    }

    #[tokio::test]
    async fn sigue_la_rama_hacia_propuesta_y_termina() {
        let rutina = rutina_ejemplo();
        let resultado = ejecutar(
            &rutina,
            "2026-09-13T00:00:00Z",
            &HerramientasFalsas,
            &DecisionesFijas("hay_errores"),
        )
        .await;

        assert!(matches!(resultado.outcome, RunOutcome::Ok));
        assert_eq!(resultado.propuestas.len(), 1);
        assert!(resultado.propuestas[0].contains("2026-09-13T00:00:00Z"));
        assert_eq!(resultado.llamadas_modelo, 1);
        assert_eq!(resultado.pasos.len(), 3);
    }

    #[tokio::test]
    async fn rama_sin_errores_termina_sin_propuesta() {
        let rutina = rutina_ejemplo();
        let resultado = ejecutar(
            &rutina,
            "2026-09-13T00:00:00Z",
            &HerramientasFalsas,
            &DecisionesFijas("sin_errores"),
        )
        .await;

        assert!(matches!(resultado.outcome, RunOutcome::Ok));
        assert!(resultado.propuestas.is_empty());
    }

    #[tokio::test]
    async fn rama_fuera_del_conjunto_cerrado_falla_la_ejecucion() {
        let rutina = rutina_ejemplo();
        let resultado = ejecutar(
            &rutina,
            "2026-09-13T00:00:00Z",
            &HerramientasFalsas,
            &DecisionesFijas("rama_inventada"),
        )
        .await;

        assert!(matches!(resultado.outcome, RunOutcome::Fallida));
        assert!(resultado.motivo_fallo.unwrap().contains("conjunto cerrado"));
    }

    #[tokio::test]
    async fn presupuesto_de_pasos_agotado_aborta_la_ejecucion() {
        let mut rutina = rutina_ejemplo();
        rutina.max_pasos = 1;
        let resultado = ejecutar(
            &rutina,
            "2026-09-13T00:00:00Z",
            &HerramientasFalsas,
            &DecisionesFijas("hay_errores"),
        )
        .await;

        assert!(matches!(resultado.outcome, RunOutcome::Fallida));
        assert_eq!(resultado.pasos.len(), 1);
    }

    #[tokio::test]
    async fn presupuesto_de_llamadas_al_modelo_agotado_aborta_antes_de_decidir() {
        let mut rutina = rutina_ejemplo();
        rutina.max_llamadas_modelo = 0;
        let contador = DecisionesContadas(AtomicI64::new(0));
        let resultado = ejecutar(&rutina, "2026-09-13T00:00:00Z", &HerramientasFalsas, &contador).await;

        assert!(matches!(resultado.outcome, RunOutcome::Fallida));
        assert_eq!(contador.0.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn un_ciclo_declarado_se_detecta_y_aborta() {
        let mut rutina = rutina_ejemplo();
        // Fuerza un ciclo: la decisión, en su rama "sin_errores", vuelve a
        // apuntar al paso inicial en vez de a "fin".
        rutina.pasos.insert(
            "clasificar".to_string(),
            Paso::Decision {
                contexto: "ctx".to_string(),
                ramas: HashMap::from([
                    ("hay_errores".to_string(), "proponer".to_string()),
                    ("sin_errores".to_string(), "consultar".to_string()),
                ]),
            },
        );
        let resultado = ejecutar(
            &rutina,
            "2026-09-13T00:00:00Z",
            &HerramientasFalsas,
            &DecisionesFijas("sin_errores"),
        )
        .await;

        assert!(matches!(resultado.outcome, RunOutcome::Fallida));
        assert!(resultado.motivo_fallo.unwrap().contains("bucle detectado"));
    }
}
