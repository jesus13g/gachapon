//! Vigilancia periódica (spec 021, decisiones 5, 7 y 9). Sin `notify`: cada
//! 2 s se compara el contenido del fichero de la clave y se sondea la API. La
//! lógica de decisión es pura y se prueba sin ventana; el hilo que la mueve
//! está en `main`.

use std::collections::HashMap;
use std::time::Duration;

use serde_json::Value;


/// Cada cuánto mira el vigía.
pub const PERIODO: Duration = Duration::from_secs(2);

/// El semáforo de la cabecera y del icono de la bandeja.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Estado {
    Bien,
    Aviso,
    Caido,
}

impl Estado {
    pub fn texto(self) -> &'static str {
        match self {
            Estado::Bien => "todo en orden",
            Estado::Aviso => "con avisos",
            Estado::Caido => "ea-web no responde",
        }
    }
}

/// `desconocido` (la API no consultó systemd) no degrada el semáforo.
fn servicio_sano(activo: Option<&str>) -> bool {
    matches!(activo, Some("active" | "desconocido"))
}

/// Si la ventana inicial se crea visible: con `--oculto` solo si hay bandeja
/// desde la que recuperarla.
pub fn visible_al_crear(oculto: bool, hay_bandeja: bool) -> bool {
    !oculto || !hay_bandeja
}

/// Estado a partir de `GET /api/estado`: el orquestador tiene que estar vivo y
/// todos los servicios activos; si no, aviso.
pub fn estado_de(estado_general: &Value) -> Estado {
    let vivo = estado_general["orquestador"]["vivo"].as_bool().unwrap_or(false);
    let servicios_activos = estado_general["servicios"]
        .as_array()
        .is_none_or(|s| s.iter().all(|sv| servicio_sano(sv["activo"].as_str())));
    if vivo && servicios_activos { Estado::Bien } else { Estado::Aviso }
}

/// Detecta cambios en la clave del fichero.
pub struct VigiaClave {
    ultima: Option<String>,
}

impl VigiaClave {
    /// `inicial` es la clave que ya se entregó a la página.
    pub fn nuevo(inicial: Option<String>) -> Self {
        Self { ultima: inicial }
    }

    /// `Some(nueva)` si la clave del fichero cambió desde la última vez
    /// (`nueva` es `None` si ya no hay clave válida); `None` si sigue igual.
    #[cfg(test)]
    pub fn comprobar(&mut self, fichero: &std::path::Path) -> Option<Option<String>> {
        self.comprobar_valor(crate::clave::leer(fichero))
    }

    /// Como [`comprobar`](Self::comprobar), con la clave ya leída (en modo
    /// WSL se lee por `wsl.exe`, no de un fichero).
    pub fn comprobar_valor(&mut self, actual: Option<String>) -> Option<Option<String>> {
        if actual == self.ultima {
            return None;
        }
        self.ultima = actual.clone();
        Some(actual)
    }
}

/// Un trabajo que acaba de llegar a un estado final.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvisoTrabajo {
    pub id: String,
    pub titulo: String,
    pub estado: String,
}

fn es_final(estado: &str) -> bool {
    matches!(estado, "en_revision" | "en_espera" | "cerrado")
}

/// Detecta los trabajos que pasan a un estado final entre dos sondeos de
/// `GET /api/trabajos`. La primera observación solo toma nota: no avisa de lo
/// antiguo.
#[derive(Default)]
pub struct VigiaTrabajos {
    vistos: Option<HashMap<String, String>>,
}

impl VigiaTrabajos {
    /// Tras una caída de `ea-web` (o un sondeo fallido) la lista previa ya no
    /// es fiable: la siguiente lectura solo recalibra, sin avisar de lo
    /// ocurrido mientras no se veía.
    pub fn olvidar(&mut self) {
        self.vistos = None;
    }

    pub fn observar(&mut self, lista: &Value) -> Vec<AvisoTrabajo> {
        let Some(filas) = lista.as_array() else { return Vec::new() };
        let ahora: HashMap<String, String> = filas
            .iter()
            .filter_map(|t| Some((t["id"].as_str()?.to_string(), t["estado"].as_str()?.to_string())))
            .collect();
        let mut avisos = Vec::new();
        if let Some(antes) = &self.vistos {
            for t in filas {
                let (Some(id), Some(estado)) = (t["id"].as_str(), t["estado"].as_str()) else { continue };
                let antes_final = antes.get(id).is_some_and(|e| es_final(e));
                if es_final(estado) && !antes_final {
                    let titulo = t["titulo"].as_str().filter(|s| !s.is_empty()).unwrap_or(id);
                    avisos.push(AvisoTrabajo { id: id.to_string(), titulo: titulo.to_string(), estado: estado.to_string() });
                }
            }
        }
        self.vistos = Some(ahora);
        avisos
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn servicios_desconocidos_no_degradan_pero_inactivos_si() {
        let ok = json!({"orquestador":{"vivo":true},"servicios":[{"activo":"desconocido"},{"activo":"active"}]});
        assert_eq!(estado_de(&ok), Estado::Bien);
        let mal = json!({"orquestador":{"vivo":true},"servicios":[{"activo":"failed"},{"activo":"active"}]});
        assert_eq!(estado_de(&mal), Estado::Aviso);
        let muerto = json!({"orquestador":{"vivo":false},"servicios":[{"activo":"desconocido"}]});
        assert_eq!(estado_de(&muerto), Estado::Aviso);
    }

    #[test]
    fn oculto_sin_bandeja_se_muestra() {
        assert!(!visible_al_crear(true, true));
        assert!(visible_al_crear(true, false));
        assert!(visible_al_crear(false, true));
    }

    #[test]
    fn el_vigia_ve_la_clave_nueva_y_su_desaparicion() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("clave");
        std::fs::write(&f, A).unwrap();
        let mut v = VigiaClave::nuevo(Some(A.into()));
        assert_eq!(v.comprobar(&f), None, "sin cambios");
        std::fs::write(&f, format!("{B}\n")).unwrap();
        assert_eq!(v.comprobar(&f), Some(Some(B.into())));
        assert_eq!(v.comprobar(&f), None, "ya entregada");
        std::fs::write(&f, "corrupta").unwrap();
        assert_eq!(v.comprobar(&f), Some(None), "clave inválida = sin clave");
        std::fs::remove_file(&f).unwrap();
        assert_eq!(v.comprobar(&f), None, "sigue sin clave");
        std::fs::write(&f, A).unwrap();
        assert_eq!(v.comprobar(&f), Some(Some(A.into())));
    }

    #[test]
    fn la_clave_que_aparece_despues_se_entrega() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("clave");
        let mut v = VigiaClave::nuevo(None);
        assert_eq!(v.comprobar(&f), None);
        std::fs::write(&f, A).unwrap();
        assert_eq!(v.comprobar(&f), Some(Some(A.into())));
    }

    #[test]
    fn el_semaforo_sale_de_la_api() {
        let bien = json!({"orquestador": {"vivo": true}, "servicios": [{"activo": "active"}, {"activo": "active"}]});
        assert_eq!(estado_de(&bien), Estado::Bien);
        let parado = json!({"orquestador": {"vivo": false}, "servicios": [{"activo": "active"}]});
        assert_eq!(estado_de(&parado), Estado::Aviso);
        let servicio = json!({"orquestador": {"vivo": true}, "servicios": [{"activo": "inactive"}]});
        assert_eq!(estado_de(&servicio), Estado::Aviso);
        assert_eq!(estado_de(&json!({})), Estado::Aviso);
    }

    #[test]
    fn los_trabajos_avisan_al_llegar_a_un_estado_final() {
        let mut v = VigiaTrabajos::default();
        let t = |id: &str, estado: &str| json!({"id": id, "titulo": format!("t {id}"), "estado": estado});
        // La primera vez solo toma nota, aunque haya trabajos terminados.
        assert!(v.observar(&json!([t("a", "cerrado"), t("b", "ejecutando")])).is_empty());
        // b termina: aviso. a sigue cerrado: nada.
        let av = v.observar(&json!([t("a", "cerrado"), t("b", "en_revision")]));
        assert_eq!(av, vec![AvisoTrabajo { id: "b".into(), titulo: "t b".into(), estado: "en_revision".into() }]);
        // b pasa de en_revision a cerrado: ya se avisó.
        assert!(v.observar(&json!([t("a", "cerrado"), t("b", "cerrado")])).is_empty());
        // Un trabajo nuevo que ya llega terminado entre dos sondeos avisa.
        assert_eq!(v.observar(&json!([t("a", "cerrado"), t("b", "cerrado"), t("c", "en_espera")])).len(), 1);
        // Cancelado no avisa; un valor que no es lista, tampoco.
        assert!(v.observar(&json!([t("d", "cancelado")])).is_empty());
        assert!(v.observar(&json!({"error": "x"})).is_empty());
    }

    #[test]
    fn tras_una_caida_se_recalibra_sin_avisar_de_lo_antiguo() {
        let mut v = VigiaTrabajos::default();
        let t = |id: &str, estado: &str| json!({"id": id, "titulo": format!("t {id}"), "estado": estado});
        assert!(v.observar(&json!([t("a", "ejecutando")])).is_empty());
        v.olvidar();
        // Al volver, a ya terminó y llegó b terminado: no se avisa de golpe.
        assert!(v.observar(&json!([t("a", "cerrado"), t("b", "en_revision")])).is_empty());
        // Lo que termina después sí avisa.
        assert_eq!(v.observar(&json!([t("a", "cerrado"), t("b", "en_revision"), t("c", "cerrado")])).len(), 1);
    }

    #[test]
    fn avisa_por_el_estado_final_aunque_salte_estados_intermedios() {
        let mut v = VigiaTrabajos::default();
        let t = |estado: &str| json!([{"id": "a", "titulo": "t a", "estado": estado}]);
        assert!(v.observar(&t("pendiente")).is_empty());
        // Entre dos sondeos pasó por ejecutando y en_revision: se ve cerrado.
        let av = v.observar(&t("cerrado"));
        assert_eq!(av.len(), 1);
        assert_eq!(av[0].estado, "cerrado");
        // Un trabajo en revisión que se cierra después no repite el aviso.
        let mut w = VigiaTrabajos::default();
        w.observar(&t("ejecutando"));
        assert_eq!(w.observar(&t("en_revision")).len(), 1);
        assert!(w.observar(&t("cerrado")).is_empty());
    }
}
