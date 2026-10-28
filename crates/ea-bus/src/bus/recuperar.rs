//! Lo que se hace al arrancar el orquestador: dejar el bus como si nadie
//! se hubiera caído a mitad (reclamos huérfanos, entradas perdidas o rancias
//! y trabajos que se quedaron ejecutando).

use std::collections::HashSet;
use std::fs;

use super::colas::{encolar_en, listar_en};
use super::{Bus, Cola, EntradaCola};
use crate::error::Resultado;
use crate::tipos::{EstadoTrabajo, Informe, ResultadoInforme, Rol};

impl Bus {
    /// Para llamar al arrancar (es idempotente). Devuelve a la cola los
    /// reclamos `.tomada-*` huérfanos cuyo trabajo sigue `EnCola`; reconcilia
    /// por estado lo que sigue `EnCola` sin entrada en ninguna cola de las
    /// que se toman órdenes (la del rol ejecutor y las de los agentes: un
    /// reclamo que se borró y no se llegó a tomar porque el proceso cayó
    /// entre medias), que vuelve a la suya con [`Bus::devolver`] (motivo
    /// «recuperado»); y entrega con [`Bus::entregar`], como Bloqueado
    /// («interrumpido»), lo que se quedó en `Ejecutando` (una rutina queda en
    /// espera con su paquete). Devuelve los ids recuperados.
    pub fn recuperar(&self) -> Resultado<Vec<String>> {
        let mut recuperados = Vec::new();
        for cola in self.colas() {
            let dir = cola.dir(&self.raiz)?;
            for e in fs::read_dir(&dir)?.flatten() {
                let p = e.path();
                let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                if !n.starts_with('.') || !n.contains(".tomada-") {
                    continue;
                }
                let entrada: Option<EntradaCola> = fs::read(&p).ok().and_then(|b| serde_json::from_slice(&b).ok());
                if let Some(en) = entrada {
                    // En colas/revision lo pendiente es una entrega, no una orden.
                    let esperado = if matches!(cola, Cola::Revision(_)) { EstadoTrabajo::EnRevision } else { EstadoTrabajo::EnCola };
                    let en_cola = self.leer_estado(&en.trabajo).is_ok_and(|s| s.estado == esperado);
                    let ya_en_cola = listar_en(&dir)?.iter().any(|x| x.trabajo == en.trabajo);
                    if en_cola && !ya_en_cola {
                        encolar_en(&dir, &en.trabajo, "recuperado")?;
                        recuperados.push(en.trabajo);
                    }
                }
                let _ = fs::remove_file(&p);
            }
        }
        // Entradas de trabajos ya cerrados o cancelados (restos de antes de
        // que Cerrar y Cancelar limpiaran las colas).
        for id in self.listar_trabajos()? {
            if self.leer_estado(&id).is_ok_and(|s| matches!(s.estado, EstadoTrabajo::Cerrado | EstadoTrabajo::Cancelado)) {
                self.retirar_de_colas(&id)?;
            }
        }
        let en_cola = |id: &str| self.leer_estado(id).is_ok_and(|s| s.estado == EstadoTrabajo::EnCola);
        let encolados = self.encolados_para_ejecutar()?;
        for id in self.listar_trabajos()? {
            if !en_cola(&id) || encolados.contains(&id) {
                continue;
            }
            // Se vuelve a mirar bajo el bloqueo del trabajo: un `Reformular`
            // a la vez lo deja en cola y lo encola sin soltarlo.
            let _bloqueo = self.bloquear_estado(&id)?;
            if en_cola(&id) && !self.encolados_para_ejecutar()?.contains(&id) {
                self.devolver(&id, "recuperado")?;
                recuperados.push(id);
            }
        }
        for id in self.listar_trabajos()? {
            if self.leer_estado(&id).is_ok_and(|s| s.estado == EstadoTrabajo::Ejecutando) {
                let inf = Informe { motivo: Some("interrumpido".into()), ..Informe::nuevo(ResultadoInforme::Bloqueado) };
                self.entregar(&id, inf)?;
                recuperados.push(id);
            }
        }
        Ok(recuperados)
    }

    /// Los trabajos con alguna entrada en una cola de las que se toman
    /// órdenes: la del rol ejecutor y las de los agentes.
    fn encolados_para_ejecutar(&self) -> Resultado<HashSet<String>> {
        let mut v = HashSet::new();
        for cola in self.colas().into_iter().filter(|c| matches!(c, Cola::Rol(Rol::Ejecutor) | Cola::Agente(_))) {
            v.extend(self.listar_cola(cola)?.into_iter().map(|e| e.trabajo));
        }
        Ok(v)
    }
}
