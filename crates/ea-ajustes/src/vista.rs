//! Forma serializable de los ajustes efectivos: la devuelve `GET
//! /api/ajustes` y `ea ajustes ver --json`.

use serde::Serialize;
use serde_json::Value;

use crate::cargar::{Efectivos, Origen};
use crate::claves::{titulo, Clave, Tipo, SECCIONES};
use crate::fuentes::Fuentes;

#[derive(Debug, Clone, Serialize)]
pub struct FilaAjuste {
    pub clave: String,
    pub titulo: String,
    pub descripcion: &'static str,
    pub tipo: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permitidos: Option<&'static [&'static str]>,
    pub valor: Value,
    pub defecto: Value,
    pub origen: Origen,
    /// `web` o `terminal`.
    pub edicion: &'static str,
    pub endurecible: bool,
    /// Variable de entorno que la fija ahora mismo, si la hay.
    pub fijada_por: Option<&'static str>,
    /// `caliente` o `reinicio`.
    pub aplica: &'static str,
    pub reinicio: &'static [&'static str],
}

#[derive(Debug, Clone, Serialize)]
pub struct SeccionAjustes {
    pub id: &'static str,
    pub titulo: &'static str,
    pub claves: Vec<FilaAjuste>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Heredado {
    pub fichero: &'static str,
    pub ruta: String,
    pub existe: bool,
}

/// Lo que necesita el cliente web para sí mismo.
#[derive(Debug, Clone, Serialize)]
pub struct Cliente {
    pub sondeo_ms: u64,
    pub diario_limite: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Resumen {
    pub fichero: String,
    pub existe: bool,
    pub version: String,
    pub heredados: Vec<Heredado>,
    pub avisos: Vec<String>,
    pub cliente: Cliente,
    pub secciones: Vec<SeccionAjustes>,
}

impl Efectivos {
    pub fn resumen(&self, f: &Fuentes) -> Resumen {
        let secciones = SECCIONES
            .iter()
            .map(|(id, titulo)| SeccionAjustes {
                id,
                titulo,
                claves: self.claves.iter().filter(|(_, c)| c.seccion == *id).map(|(n, c)| self.fila(n, c)).collect(),
            })
            .collect();
        Resumen {
            fichero: self.fichero.display().to_string(),
            existe: self.existe,
            version: self.version.clone(),
            heredados: f
                .heredados()
                .into_iter()
                .map(|(fichero, ruta)| Heredado { fichero, existe: ruta.exists(), ruta: ruta.display().to_string() })
                .collect(),
            avisos: self.avisos.clone(),
            cliente: Cliente { sondeo_ms: self.ajustes.web.sondeo_ms, diario_limite: self.ajustes.web.diario_limite },
            secciones,
        }
    }

    fn fila(&self, nombre: &str, c: &'static Clave) -> FilaAjuste {
        let (min, max) = match c.tipo {
            Tipo::Entero { min, max } => (Some(min), Some(max)),
            _ => (None, None),
        };
        let origen = self.origenes[nombre].clone();
        let fijada_por = match origen {
            Origen::Entorno { variable } => Some(variable),
            _ => None,
        };
        FilaAjuste {
            clave: nombre.to_string(),
            titulo: titulo(nombre, c),
            descripcion: c.descripcion,
            tipo: c.tipo.nombre(),
            min,
            max,
            permitidos: match c.tipo {
                Tipo::Lista { permitidos } => permitidos,
                Tipo::Eleccion { permitidos } => Some(permitidos),
                _ => None,
            },
            valor: self.valores[nombre].clone(),
            defecto: self.defectos[nombre].clone(),
            origen,
            edicion: c.edicion.nombre(),
            endurecible: c.endurecible,
            fijada_por,
            aplica: c.aplica.nombre(),
            reinicio: c.aplica.servicios(),
        }
    }
}
