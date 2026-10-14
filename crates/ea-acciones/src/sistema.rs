//! La capa de sistema operativo (§7.3): lo único que toca `systemctl` y el
//! sistema de ficheros. Dos traits para que el tipo diga lo que se puede
//! hacer: [`Consulta`] solo mira (basta para simular y para proponer; es lo
//! único que reciben `ea-mcp` y el orquestador) y [`Sistema`] además muta
//! (solo lo construyen las interfaces del usuario). Sin shell: argumentos
//! como vector, y nombres de una lista cerrada o validados.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use ea_boveda::{Boveda, ErrorBoveda, PlanPromocion};
use ea_store::Store;
use tokio::process::Command;
use tokio::time::timeout;

/// Servicios que se pueden reiniciar: lista cerrada.
pub const SERVICIOS_REINICIABLES: [&str; 4] =
    ["llama-server", "ea-mcp", "ea-orchestrator", "ea-web"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EstadoServicio {
    /// La unidad existe en `systemd --user`.
    pub cargado: bool,
    /// `active`, `inactive`, `failed`…
    pub activo: String,
}

pub trait Consulta: Send + Sync {
    fn servicio_estado(
        &self,
        nombre: &str,
    ) -> impl Future<Output = Result<EstadoServicio, String>> + Send;

    /// Contenido del fichero de la rutina, o `None` si no existe.
    fn rutina_contenido(&self, slug: &str) -> Result<Option<String>, String>;

    /// Lo que haría promover una nota a la región compartida, sin hacerlo.
    fn plan_promocion(&self, ruta: &str, destino: Option<&str>) -> Result<PlanPromocion, ErrorBoveda>;
}

pub trait Sistema: Consulta {
    fn servicio_reiniciar(&self, nombre: &str) -> impl Future<Output = Result<(), String>> + Send;
    fn rutina_eliminar(&self, slug: &str) -> Result<(), String>;

    /// Copia una nota a la región compartida (la promoción que acepta el
    /// usuario). `huella` es la del contenido que se simuló.
    fn nota_promover(
        &self,
        store: &mut Store,
        ruta: &str,
        destino: Option<&str>,
        huella: &str,
        actor: &str,
        canal: &str,
    ) -> Result<PlanPromocion, ErrorBoveda>;
}

/// Los slugs acaban en una ruta de fichero: nada que no sea `[A-Za-z0-9_-]`.
pub fn slug_valido(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= 64
        && slug.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn ruta_rutina(dir: &std::path::Path, slug: &str) -> Result<PathBuf, String> {
    if !slug_valido(slug) {
        return Err(format!("slug de rutina inválido: '{slug}'"));
    }
    Ok(dir.join(format!("{slug}.toml")))
}

fn leer_rutina(dir: &std::path::Path, slug: &str) -> Result<Option<String>, String> {
    let ruta = ruta_rutina(dir, slug)?;
    match std::fs::read_to_string(&ruta) {
        Ok(c) => Ok(Some(c)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("no se pudo leer {}: {e}", ruta.display())),
    }
}

async fn estado_systemd(nombre: &str) -> Result<EstadoServicio, String> {
    let mut cmd = Command::new("systemctl");
    cmd.args(["--user", "show", "-p", "LoadState", "-p", "ActiveState"]);
    cmd.arg(format!("{nombre}.service"));
    cmd.kill_on_drop(true);
    let salida = timeout(Duration::from_secs(5), cmd.output())
        .await
        .map_err(|_| "systemctl tardó demasiado".to_string())?
        .map_err(|e| format!("no se pudo ejecutar systemctl: {e}"))?;
    if !salida.status.success() {
        return Err(String::from_utf8_lossy(&salida.stderr).trim().to_string());
    }
    let texto = String::from_utf8_lossy(&salida.stdout);
    let valor = |clave: &str| {
        texto
            .lines()
            .find_map(|l| l.strip_prefix(clave).and_then(|r| r.strip_prefix('=')))
            .unwrap_or("")
            .trim()
            .to_string()
    };
    Ok(EstadoServicio {
        cargado: valor("LoadState") == "loaded",
        activo: valor("ActiveState"),
    })
}

/// Solo mira: lo que reciben `ea-mcp` y el orquestador. No implementa
/// [`Sistema`], así que con él es imposible mutar el sistema.
pub struct SoloConsulta {
    pub rutinas_dir: PathBuf,
    /// Solo se usa para mirar ([`Boveda::planear_promocion`]); promover
    /// exige un [`Sistema`], que este tipo no implementa.
    pub boveda: Boveda,
}

impl Consulta for SoloConsulta {
    async fn servicio_estado(&self, nombre: &str) -> Result<EstadoServicio, String> {
        estado_systemd(nombre).await
    }
    fn rutina_contenido(&self, slug: &str) -> Result<Option<String>, String> {
        leer_rutina(&self.rutinas_dir, slug)
    }
    fn plan_promocion(&self, ruta: &str, destino: Option<&str>) -> Result<PlanPromocion, ErrorBoveda> {
        self.boveda.planear_promocion(ruta, destino)
    }
}

/// El sistema real, para las interfaces del usuario (`ea`, `ea-web`).
pub struct SistemaReal {
    pub rutinas_dir: PathBuf,
    pub boveda: Boveda,
    /// Servicio al que pertenece el proceso actual: no puede reiniciarse a
    /// sí mismo a mitad de la petición (`ea-web` pasa `Some("ea-web")`).
    pub propio: Option<&'static str>,
}

impl Consulta for SistemaReal {
    async fn servicio_estado(&self, nombre: &str) -> Result<EstadoServicio, String> {
        estado_systemd(nombre).await
    }
    fn rutina_contenido(&self, slug: &str) -> Result<Option<String>, String> {
        leer_rutina(&self.rutinas_dir, slug)
    }
    fn plan_promocion(&self, ruta: &str, destino: Option<&str>) -> Result<PlanPromocion, ErrorBoveda> {
        self.boveda.planear_promocion(ruta, destino)
    }
}

impl Sistema for SistemaReal {
    async fn servicio_reiniciar(&self, nombre: &str) -> Result<(), String> {
        if !SERVICIOS_REINICIABLES.contains(&nombre) {
            return Err(format!("servicio fuera de la lista cerrada: '{nombre}'"));
        }
        if self.propio == Some(nombre) {
            return Err(format!(
                "{nombre} no puede reiniciarse a sí mismo desde su propia petición; \
                 hazlo desde la terminal con `ea`"
            ));
        }
        let mut cmd = Command::new("systemctl");
        cmd.args(["--user", "restart", &format!("{nombre}.service")]);
        cmd.kill_on_drop(true);
        let salida = timeout(Duration::from_secs(30), cmd.output())
            .await
            .map_err(|_| "systemctl tardó demasiado".to_string())?
            .map_err(|e| format!("no se pudo ejecutar systemctl: {e}"))?;
        if salida.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&salida.stderr).trim().to_string())
        }
    }

    fn rutina_eliminar(&self, slug: &str) -> Result<(), String> {
        let ruta = ruta_rutina(&self.rutinas_dir, slug)?;
        std::fs::remove_file(&ruta).map_err(|e| format!("no se pudo borrar {}: {e}", ruta.display()))
    }

    fn nota_promover(
        &self,
        store: &mut Store,
        ruta: &str,
        destino: Option<&str>,
        huella: &str,
        actor: &str,
        canal: &str,
    ) -> Result<PlanPromocion, ErrorBoveda> {
        self.boveda.promover(store, ruta, destino, huella, actor, canal)
    }
}

/// Sistema en memoria para pruebas: no toca `systemd` ni el disco, y apunta
/// qué mutaciones se le pidieron.
#[derive(Default)]
pub struct SistemaSimulado {
    pub servicios: Mutex<HashMap<String, EstadoServicio>>,
    pub rutinas: Mutex<HashMap<String, String>>,
    /// Notas de la bóveda en memoria, por ruta relativa.
    pub notas: Mutex<HashMap<String, String>>,
    /// Si es `Some`, los reinicios fallan con este mensaje.
    pub fallo_reinicio: Mutex<Option<String>>,
    /// Registro de mutaciones aplicadas, p. ej. `reiniciar:ea-mcp`.
    pub mutaciones: Mutex<Vec<String>>,
}

impl SistemaSimulado {
    pub fn con_servicio(self, nombre: &str, activo: &str) -> Self {
        self.servicios.lock().unwrap().insert(
            nombre.to_string(),
            EstadoServicio { cargado: true, activo: activo.to_string() },
        );
        self
    }

    pub fn con_rutina(self, slug: &str, contenido: &str) -> Self {
        self.rutinas.lock().unwrap().insert(slug.to_string(), contenido.to_string());
        self
    }

    pub fn con_nota(self, ruta: &str, contenido: &str) -> Self {
        self.notas.lock().unwrap().insert(ruta.to_string(), contenido.to_string());
        self
    }

    pub fn mutaciones(&self) -> Vec<String> {
        self.mutaciones.lock().unwrap().clone()
    }
}

impl Consulta for SistemaSimulado {
    async fn servicio_estado(&self, nombre: &str) -> Result<EstadoServicio, String> {
        Ok(self
            .servicios
            .lock()
            .unwrap()
            .get(nombre)
            .cloned()
            .unwrap_or(EstadoServicio { cargado: false, activo: "inactive".into() }))
    }
    fn rutina_contenido(&self, slug: &str) -> Result<Option<String>, String> {
        if !slug_valido(slug) {
            return Err(format!("slug de rutina inválido: '{slug}'"));
        }
        Ok(self.rutinas.lock().unwrap().get(slug).cloned())
    }
    fn plan_promocion(&self, ruta: &str, destino: Option<&str>) -> Result<PlanPromocion, ErrorBoveda> {
        let partes = ea_boveda::ruta::validar_ruta(ruta)?;
        if partes[0] == ea_boveda::ruta::REGION_COMPARTIDA {
            return Err(ErrorBoveda::Argumentos(format!("{ruta} ya está en la región compartida")));
        }
        let destino = match destino {
            Some(d) => ea_boveda::ruta::en_region(ea_boveda::ruta::REGION_COMPARTIDA, d)?,
            None => format!("compartida/{}", partes[1..].join("/")),
        };
        let notas = self.notas.lock().unwrap();
        let contenido = notas.get(ruta).ok_or_else(|| ErrorBoveda::NoExiste(ruta.to_string()))?;
        if notas.contains_key(&destino) {
            return Err(ErrorBoveda::YaExiste(destino));
        }
        Ok(PlanPromocion {
            origen: ruta.to_string(),
            destino,
            titulo: ruta.to_string(),
            autor: partes[0].to_string(),
            bytes: contenido.len(),
            huella: ea_boveda::huella(contenido),
        })
    }
}

impl Sistema for SistemaSimulado {
    async fn servicio_reiniciar(&self, nombre: &str) -> Result<(), String> {
        if let Some(e) = self.fallo_reinicio.lock().unwrap().clone() {
            return Err(e);
        }
        self.mutaciones.lock().unwrap().push(format!("reiniciar:{nombre}"));
        Ok(())
    }
    fn rutina_eliminar(&self, slug: &str) -> Result<(), String> {
        self.mutaciones.lock().unwrap().push(format!("eliminar_rutina:{slug}"));
        self.rutinas.lock().unwrap().remove(slug);
        Ok(())
    }
    fn nota_promover(
        &self,
        _store: &mut Store,
        ruta: &str,
        destino: Option<&str>,
        huella: &str,
        _actor: &str,
        _canal: &str,
    ) -> Result<PlanPromocion, ErrorBoveda> {
        let plan = self.plan_promocion(ruta, destino)?;
        if plan.huella != huella {
            return Err(ErrorBoveda::Cambio(ruta.to_string()));
        }
        let mut notas = self.notas.lock().unwrap();
        let c = notas[ruta].clone();
        notas.insert(plan.destino.clone(), c);
        self.mutaciones.lock().unwrap().push(format!("promover:{ruta}->{}", plan.destino));
        Ok(plan)
    }
}
