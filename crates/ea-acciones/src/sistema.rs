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

use ea_biblioteca::{Cambio, Contexto, Hecho, Ordenes, OrdenesDoble, OrdenesReales};
use ea_boveda::{Boveda, ErrorBoveda, PlanPromocion};
use ea_store::Store;
use tokio::process::Command;
use tokio::time::timeout;

use crate::temporizador::{comprobar_calendario, Calendario, EstadoTimer, Temporizadores};

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

    /// Contenido de la copia retirada de la rutina (en la papelera de
    /// `rutinas_dir`, [`ea_bus::ruta_retirada`]), o `None` si no la hay.
    fn rutina_retirada(&self, slug: &str) -> Result<Option<String>, String>;

    /// Lo que haría promover una nota a la región compartida, sin hacerlo.
    fn plan_promocion(&self, ruta: &str, destino: Option<&str>) -> Result<PlanPromocion, ErrorBoveda>;

    /// Cómo está el timer de systemd de una rutina (fichero, `OnCalendar`,
    /// activo). No cambia nada.
    fn timer_estado(&self, slug: &str) -> impl Future<Output = Result<EstadoTimer, String>> + Send;

    /// Valida un `OnCalendar` con `systemd-analyze calendar` (que no cambia
    /// nada). `Ok(Err)`: systemd no lo acepta.
    fn calendario_validar(
        &self,
        calendario: &str,
    ) -> impl Future<Output = Result<Result<Calendario, String>, String>> + Send;

    /// Lo que haría activar o desactivar un elemento de la biblioteca (spec
    /// 018): el [`Cambio`] con su mecanismo, objetivo y valor antes y
    /// después. Solo lee; falla si no se puede cambiar o falta el binario
    /// del comando oficial.
    fn biblioteca_cambio(&self, elemento: &str, activar: bool) -> Result<Cambio, String>;
}

pub trait Sistema: Consulta {
    fn servicio_reiniciar(&self, nombre: &str) -> impl Future<Output = Result<(), String>> + Send;

    /// Borra para siempre la copia retirada de una rutina (la de la papelera).
    fn rutina_eliminar(&self, slug: &str) -> Result<(), String>;

    /// Mueve el TOML de la rutina a la papelera ([`ea_bus::retirar_rutina`]).
    fn rutina_retirar(&self, slug: &str) -> Result<(), String>;

    /// Devuelve el TOML de la papelera a su sitio ([`ea_bus::restaurar_rutina`]).
    fn rutina_restaurar(&self, slug: &str) -> Result<(), String>;

    /// Registra la definición `texto` (lo mismo que `ea rutina registrar`:
    /// la versión en SQLite y la copia en `rutinas_dir`). Devuelve el slug y
    /// la versión registrados.
    fn rutina_registrar(&self, store: &Store, texto: &str, actor: &str) -> Result<(String, u32), String>;

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

    /// Escribe `ea-rutina@<slug>.timer` con ese `OnCalendar`, `daemon-reload`
    /// y lo activa ([`Temporizadores::programar`]).
    fn timer_programar(&self, slug: &str, calendario: &str) -> impl Future<Output = Result<(), String>> + Send;

    /// Desactiva y borra el timer de una rutina; `Ok(false)` si no lo había
    /// ([`Temporizadores::desprogramar`]). Retirar una rutina lo reutiliza.
    fn timer_desprogramar(&self, slug: &str) -> impl Future<Output = Result<bool, String>> + Send;

    /// Aplica un [`Cambio`] de la biblioteca: revalida contra el estado de
    /// ahora y lo ejecuta con su mecanismo (comando oficial, campo con copia
    /// previa, mover o ajustes).
    fn biblioteca_aplicar(&self, cambio: &Cambio) -> impl Future<Output = Result<Hecho, String>> + Send;
}

/// Planifica con el entorno real y comprueba que el binario del comando
/// oficial existe.
fn planear_real(elemento: &str, activar: bool) -> Result<Cambio, String> {
    let c = ea_biblioteca::planear(elemento, activar, &Contexto::sistema()).map_err(|e| e.to_string())?;
    ea_biblioteca::cambio::comprobar_disponible(&c, &OrdenesReales).map_err(|e| e.to_string())?;
    Ok(c)
}

/// Aplica en un hilo aparte: puede lanzar una orden de hasta 30 s.
async fn aplicar_real(cambio: &Cambio) -> Result<Hecho, String> {
    let c = cambio.clone();
    tokio::task::spawn_blocking(move || ea_biblioteca::aplicar_con(&c, &Contexto::sistema(), &OrdenesReales))
        .await
        .map_err(|e| format!("la tarea del cambio falló: {e}"))?
        .map_err(|e| e.to_string())
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
    leer_opcional(&ruta_rutina(dir, slug)?)
}

fn leer_retirada(dir: &std::path::Path, slug: &str) -> Result<Option<String>, String> {
    leer_opcional(&ea_bus::ruta_retirada(dir, slug).map_err(|e| e.to_string())?)
}

fn leer_opcional(ruta: &std::path::Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(ruta) {
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
    fn rutina_retirada(&self, slug: &str) -> Result<Option<String>, String> {
        leer_retirada(&self.rutinas_dir, slug)
    }
    fn plan_promocion(&self, ruta: &str, destino: Option<&str>) -> Result<PlanPromocion, ErrorBoveda> {
        self.boveda.planear_promocion(ruta, destino)
    }
    /// Los timers de `systemd --user` de esta sesión, solo para mirar.
    async fn timer_estado(&self, slug: &str) -> Result<EstadoTimer, String> {
        Temporizadores::usuario().estado(slug).await
    }
    async fn calendario_validar(&self, calendario: &str) -> Result<Result<Calendario, String>, String> {
        Temporizadores::usuario().validar_calendario(calendario).await
    }
    /// Solo planifica: aplicar exige un [`Sistema`], que este tipo no es.
    fn biblioteca_cambio(&self, elemento: &str, activar: bool) -> Result<Cambio, String> {
        planear_real(elemento, activar)
    }
}

/// El sistema real, para las interfaces del usuario (`ea`, `ea-web`).
pub struct SistemaReal {
    pub rutinas_dir: PathBuf,
    pub boveda: Boveda,
    /// Servicio al que pertenece el proceso actual: no puede reiniciarse a
    /// sí mismo a mitad de la petición (`ea-web` pasa `Some("ea-web")`).
    pub propio: Option<&'static str>,
    /// Dónde se escriben los timers de las rutinas y con qué órdenes
    /// ([`Temporizadores::usuario`] fuera de las pruebas).
    pub temporizadores: Temporizadores,
}

impl Consulta for SistemaReal {
    async fn servicio_estado(&self, nombre: &str) -> Result<EstadoServicio, String> {
        estado_systemd(nombre).await
    }
    fn rutina_contenido(&self, slug: &str) -> Result<Option<String>, String> {
        leer_rutina(&self.rutinas_dir, slug)
    }
    fn rutina_retirada(&self, slug: &str) -> Result<Option<String>, String> {
        leer_retirada(&self.rutinas_dir, slug)
    }
    fn plan_promocion(&self, ruta: &str, destino: Option<&str>) -> Result<PlanPromocion, ErrorBoveda> {
        self.boveda.planear_promocion(ruta, destino)
    }
    async fn timer_estado(&self, slug: &str) -> Result<EstadoTimer, String> {
        self.temporizadores.estado(slug).await
    }
    async fn calendario_validar(&self, calendario: &str) -> Result<Result<Calendario, String>, String> {
        self.temporizadores.validar_calendario(calendario).await
    }
    fn biblioteca_cambio(&self, elemento: &str, activar: bool) -> Result<Cambio, String> {
        planear_real(elemento, activar)
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
        let ruta = ea_bus::ruta_retirada(&self.rutinas_dir, slug).map_err(|e| e.to_string())?;
        std::fs::remove_file(&ruta).map_err(|e| format!("no se pudo borrar {}: {e}", ruta.display()))
    }

    fn rutina_retirar(&self, slug: &str) -> Result<(), String> {
        ea_bus::retirar_rutina(&self.rutinas_dir, slug).map_err(|e| e.to_string())
    }

    fn rutina_restaurar(&self, slug: &str) -> Result<(), String> {
        ea_bus::restaurar_rutina(&self.rutinas_dir, slug).map_err(|e| e.to_string())
    }

    fn rutina_registrar(&self, store: &Store, texto: &str, actor: &str) -> Result<(String, u32), String> {
        let bus = ea_bus::Bus::abrir().map_err(|e| e.to_string())?;
        let r = bus
            .con_store(store.duplicar().map_err(|e| e.to_string())?)
            .con_actor(actor)
            .registrar_rutina(texto, &self.rutinas_dir)
            .map_err(|e| e.to_string())?;
        Ok((r.rutina.slug, r.rutina.version))
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

    async fn timer_programar(&self, slug: &str, calendario: &str) -> Result<(), String> {
        self.temporizadores.programar(slug, calendario).await
    }

    async fn timer_desprogramar(&self, slug: &str) -> Result<bool, String> {
        self.temporizadores.desprogramar(slug).await
    }

    async fn biblioteca_aplicar(&self, cambio: &Cambio) -> Result<Hecho, String> {
        aplicar_real(cambio).await
    }
}

/// Sistema en memoria para pruebas: no toca `systemd` ni el disco, y apunta
/// qué mutaciones se le pidieron.
#[derive(Default)]
pub struct SistemaSimulado {
    pub servicios: Mutex<HashMap<String, EstadoServicio>>,
    pub rutinas: Mutex<HashMap<String, String>>,
    /// Rutinas retiradas (su copia en la papelera), por slug.
    pub retiradas: Mutex<HashMap<String, String>>,
    /// Notas de la bóveda en memoria, por ruta relativa.
    pub notas: Mutex<HashMap<String, String>>,
    /// Si es `Some`, los reinicios fallan con este mensaje.
    pub fallo_reinicio: Mutex<Option<String>>,
    /// Registro de mutaciones aplicadas, p. ej. `reiniciar:ea-mcp`.
    pub mutaciones: Mutex<Vec<String>>,
    /// Timers programados, por slug: su `OnCalendar` (activos todos).
    pub timers: Mutex<HashMap<String, String>>,
    /// Raíces y ajustes de prueba de la biblioteca (spec 018); sin ellos, los
    /// cambios de la biblioteca fallan. Los ficheros son los de esas raíces
    /// (temporales), y los comandos oficiales van al doble de `ordenes`.
    pub biblioteca: Mutex<Option<Contexto>>,
    pub ordenes: OrdenesDoble,
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

    pub fn con_retirada(self, slug: &str, contenido: &str) -> Self {
        self.retiradas.lock().unwrap().insert(slug.to_string(), contenido.to_string());
        self
    }

    pub fn con_nota(self, ruta: &str, contenido: &str) -> Self {
        self.notas.lock().unwrap().insert(ruta.to_string(), contenido.to_string());
        self
    }

    pub fn con_timer(self, slug: &str, calendario: &str) -> Self {
        self.timers.lock().unwrap().insert(slug.to_string(), calendario.to_string());
        self
    }

    pub fn con_biblioteca(self, ctx: Contexto) -> Self {
        *self.biblioteca.lock().unwrap() = Some(ctx);
        self
    }

    fn contexto_biblioteca(&self) -> Result<Contexto, String> {
        self.biblioteca.lock().unwrap().clone().ok_or_else(|| "la biblioteca no está configurada en el sistema simulado".to_string())
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
    fn rutina_retirada(&self, slug: &str) -> Result<Option<String>, String> {
        if !slug_valido(slug) {
            return Err(format!("slug de rutina inválido: '{slug}'"));
        }
        Ok(self.retiradas.lock().unwrap().get(slug).cloned())
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
    async fn timer_estado(&self, slug: &str) -> Result<EstadoTimer, String> {
        if !slug_valido(slug) {
            return Err(format!("slug de rutina inválido: '{slug}'"));
        }
        let cal = self.timers.lock().unwrap().get(slug).cloned();
        Ok(EstadoTimer {
            fichero: cal.is_some(),
            activo: Some(if cal.is_some() { "active" } else { "inactive" }.into()),
            calendario: cal,
        })
    }
    /// Una imitación de `systemd-analyze calendar`: acepta palabras de
    /// calendario conocidas y expresiones con cifras o `*`; el resto, no.
    async fn calendario_validar(&self, calendario: &str) -> Result<Result<Calendario, String>, String> {
        let c = match comprobar_calendario(calendario) {
            Ok(c) => c,
            Err(e) => return Ok(Err(e)),
        };
        const PALABRAS: [&str; 12] =
            ["daily", "weekly", "monthly", "hourly", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun", "Mon..Fri"];
        let vale = c.split_whitespace().all(|w| PALABRAS.contains(&w) || w.bytes().any(|b| b.is_ascii_digit() || b == b'*'));
        if !vale {
            return Ok(Err(format!("Failed to parse calendar specification '{c}': Invalid argument")));
        }
        Ok(Ok(Calendario {
            normalizado: c.to_string(),
            proximas: (1..=crate::temporizador::ITERACIONES).map(|i| format!("ejecución {i} de «{c}»")).collect(),
        }))
    }
    fn biblioteca_cambio(&self, elemento: &str, activar: bool) -> Result<Cambio, String> {
        let c = ea_biblioteca::planear(elemento, activar, &self.contexto_biblioteca()?).map_err(|e| e.to_string())?;
        ea_biblioteca::cambio::comprobar_disponible(&c, &self.ordenes as &dyn Ordenes).map_err(|e| e.to_string())?;
        Ok(c)
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
        self.retiradas.lock().unwrap().remove(slug).ok_or_else(|| format!("la rutina '{slug}' no está en la papelera"))?;
        self.mutaciones.lock().unwrap().push(format!("eliminar_rutina:{slug}"));
        Ok(())
    }
    fn rutina_retirar(&self, slug: &str) -> Result<(), String> {
        let c = self.rutinas.lock().unwrap().remove(slug).ok_or_else(|| format!("la rutina '{slug}' no está activa"))?;
        self.retiradas.lock().unwrap().insert(slug.to_string(), c);
        self.mutaciones.lock().unwrap().push(format!("retirar:{slug}"));
        Ok(())
    }
    fn rutina_restaurar(&self, slug: &str) -> Result<(), String> {
        if self.rutinas.lock().unwrap().contains_key(slug) {
            return Err(format!("la rutina '{slug}' ya está activa"));
        }
        let c = self.retiradas.lock().unwrap().remove(slug).ok_or_else(|| format!("la rutina '{slug}' no está en la papelera"))?;
        self.rutinas.lock().unwrap().insert(slug.to_string(), c);
        self.mutaciones.lock().unwrap().push(format!("restaurar:{slug}"));
        Ok(())
    }
    /// Con la misma política que el bus (`comprobar_registro`), pero el
    /// fichero va al mapa en memoria en vez de a `rutinas_dir`.
    fn rutina_registrar(&self, store: &Store, texto: &str, actor: &str) -> Result<(String, u32), String> {
        let (r, _) = ea_bus::comprobar_registro(store, texto).map_err(|e| e.to_string())?;
        let (slug, version) = (r.rutina.slug.clone(), r.rutina.version);
        store
            .guardar_rutina(&ea_store::FilaRutina {
                slug: slug.clone(),
                version: i64::from(version),
                definicion: texto.to_string(),
                creada_en: "2026-01-01T00:00:00Z".into(),
                creada_por: actor.to_string(),
            })
            .map_err(|e| e.to_string())?;
        self.rutinas.lock().unwrap().insert(slug.clone(), texto.to_string());
        self.retiradas.lock().unwrap().remove(&slug);
        self.mutaciones.lock().unwrap().push(format!("registrar_rutina:{slug}@{version}"));
        Ok((slug, version))
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
    async fn timer_programar(&self, slug: &str, calendario: &str) -> Result<(), String> {
        let c = comprobar_calendario(calendario)?;
        self.timers.lock().unwrap().insert(slug.to_string(), c.to_string());
        self.mutaciones.lock().unwrap().push(format!("programar:{slug}={c}"));
        Ok(())
    }
    async fn timer_desprogramar(&self, slug: &str) -> Result<bool, String> {
        let habia = self.timers.lock().unwrap().remove(slug).is_some();
        if habia {
            self.mutaciones.lock().unwrap().push(format!("desprogramar:{slug}"));
        }
        Ok(habia)
    }
    async fn biblioteca_aplicar(&self, cambio: &Cambio) -> Result<Hecho, String> {
        let h = ea_biblioteca::aplicar_con(cambio, &self.contexto_biblioteca()?, &self.ordenes).map_err(|e| e.to_string())?;
        self.mutaciones.lock().unwrap().push(format!("biblioteca:{}:{}", if h.activar { "activar" } else { "desactivar" }, h.id));
        Ok(h)
    }
}
