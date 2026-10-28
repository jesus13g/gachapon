//! Las colas del bus: `colas/<rol>/`, `colas/agentes/<agente>/` y
//! `colas/revision/<agente>/`, con una entrada JSON por trabajo encolado.
//! Se toma la más antigua reclamándola con un `rename` atómico.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{Bus, ahora_rfc3339, nuevo_id};
use crate::error::{ErrorBus, Resultado};
use crate::fs_atomico::escribir_json;
use crate::tipos::{Orden, Rol};

/// Una cola del bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cola<'a> {
    /// La de un rol: `colas/claude-code/` (planificador) o
    /// `colas/modelo-local/` (ejecutor).
    Rol(Rol),
    /// Las órdenes asignadas a un agente: `colas/agentes/<agente>/` (spec
    /// 019, RF-005).
    Agente(&'a str),
    /// Las entregas que un revisor tiene que mirar: `colas/revision/<agente>/`.
    /// Va aparte de las órdenes asignadas para que un ejecutor que también sea
    /// revisor no tome una revisión como si fuera trabajo (spec 019, RF-006).
    Revision(&'a str),
}

impl From<Rol> for Cola<'_> {
    fn from(r: Rol) -> Self {
        Cola::Rol(r)
    }
}

impl Cola<'_> {
    /// Su carpeta. El agente tiene que ser del catálogo: así no se abre
    /// ninguna ruta arbitraria.
    pub(super) fn dir(self, raiz: &Path) -> Resultado<PathBuf> {
        let colas = raiz.join("colas");
        match self {
            Cola::Rol(r) => Ok(colas.join(r.as_str())),
            Cola::Agente(a) | Cola::Revision(a) if ea_ajustes::ficha(a).is_none() => Err(ErrorBus::Argumentos(format!("{a:?} no es un agente del catálogo"))),
            Cola::Agente(a) => Ok(colas.join("agentes").join(a)),
            Cola::Revision(a) => Ok(colas.join("revision").join(a)),
        }
    }
}

/// Entrada de una cola.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntradaCola {
    pub id: String,
    pub trabajo: String,
    pub motivo: String,
    pub encolado_en: String,
}

impl Bus {
    pub fn encolar<'a>(&self, cola: impl Into<Cola<'a>>, trabajo: &str, motivo: &str) -> Resultado<String> {
        encolar_en(&cola.into().dir(&self.raiz)?, trabajo, motivo)
    }

    /// Entradas visibles de la cola, de la más antigua a la más nueva; vacía
    /// si nunca se le encoló nada.
    pub fn listar_cola<'a>(&self, cola: impl Into<Cola<'a>>) -> Resultado<Vec<EntradaCola>> {
        listar_en(&cola.into().dir(&self.raiz)?)
    }

    /// Toma el trabajo más antiguo de la cola. Reclama la entrada
    /// renombrándola (atómico): de dos procesos, solo uno lo consigue. No
    /// cambia el estado del trabajo; para eso, [`Bus::aplicar`] con `Tomar`.
    pub fn tomar<'a>(&self, cola: impl Into<Cola<'a>>) -> Resultado<Option<String>> {
        tomar_en(&cola.into().dir(&self.raiz)?, &|_| true)
    }

    /// Lo que toma un trabajador de un agente ejecutor: primero la cola de su
    /// agente y, si está vacía, la del rol `modelo-local`. Mismo reclamo por
    /// `rename` que [`Bus::tomar`] (spec 019, RF-005).
    pub fn tomar_para(&self, agente: &str) -> Resultado<Option<String>> {
        self.tomar_para_con(agente, &|_| true)
    }

    /// Como [`Bus::tomar_para`], pero de la cola de rol solo toma las órdenes
    /// que pasan `filtro`; las demás se quedan donde estaban (se lee la orden
    /// antes de reclamar la entrada). La cola propia del agente se toma
    /// entera. Una orden ilegible pasa el filtro, para que se descarte.
    pub fn tomar_para_con(&self, agente: &str, filtro: &dyn Fn(&Orden) -> bool) -> Resultado<Option<String>> {
        if let Some(t) = self.tomar(Cola::Agente(agente))? {
            return Ok(Some(t));
        }
        tomar_en(&Cola::Rol(Rol::Ejecutor).dir(&self.raiz)?, &|id| self.leer_orden(id).map(|o| filtro(&o)).unwrap_or(true))
    }

    /// Todas las colas: la de cada rol, la de cada agente y la de revisión
    /// de cada agente del catálogo que tenga carpeta.
    pub(super) fn colas(&self) -> Vec<Cola<'static>> {
        let mut v: Vec<Cola<'static>> = Rol::TODOS.iter().map(|&r| Cola::Rol(r)).collect();
        for sub in ["agentes", "revision"] {
            let Ok(rd) = fs::read_dir(self.raiz.join("colas").join(sub)) else { continue };
            let mut agentes: Vec<&'static str> =
                rd.flatten().filter(|e| e.path().is_dir()).filter_map(|e| ea_ajustes::ficha(e.file_name().to_str()?).map(|f| f.id.as_str())).collect();
            agentes.sort();
            v.extend(agentes.into_iter().map(|a| if sub == "agentes" { Cola::Agente(a) } else { Cola::Revision(a) }));
        }
        v
    }

    /// Quita de TODAS las colas las entradas de un trabajo.
    pub fn retirar_de_colas(&self, id: &str) -> Resultado<usize> {
        let mut n = 0;
        for cola in self.colas() {
            for e in fs::read_dir(cola.dir(&self.raiz)?)?.flatten() {
                let p = e.path();
                let nombre = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if nombre.starts_with('.') || !nombre.ends_with(".json") {
                    continue;
                }
                let de_este = fs::read(&p).ok().and_then(|b| serde_json::from_slice::<EntradaCola>(&b).ok()).is_some_and(|en| en.trabajo == id);
                if de_este && fs::remove_file(&p).is_ok() {
                    n += 1;
                }
            }
        }
        Ok(n)
    }

    /// Devuelve a su cola un trabajo `EnCola` cuya entrada se reclamó y no se
    /// llegó a tomar (el worktree estaba ocupado, se reformuló, se perdió el
    /// reclamo): la del agente asignado si su equipo sigue despierto y él en
    /// él; si no, la del rol ejecutor. Elegir y encolar van bajo el bloqueo de
    /// los equipos, para que no entre en la cola de un agente cuyo equipo se
    /// está durmiendo. Devuelve el id de la entrada.
    pub fn devolver(&self, id: &str, motivo: &str) -> Resultado<String> {
        let _bloqueo = self.bloquear_equipos()?;
        self.devolver_bloqueado(id, motivo)
    }

    /// [`Bus::devolver`] para quien ya tiene el bloqueo de los equipos. Una
    /// orden ilegible va a la cola del rol, donde el ejecutor la descarta.
    pub(crate) fn devolver_bloqueado(&self, id: &str, motivo: &str) -> Resultado<String> {
        match self.leer_orden(id).ok().and_then(|o| self.asignado_vigente(&o)) {
            Some(agente) => self.encolar(Cola::Agente(&agente), id, motivo),
            None => self.encolar(Rol::Ejecutor, id, motivo),
        }
    }
}

pub(super) fn encolar_en(dir: &Path, trabajo: &str, motivo: &str) -> Resultado<String> {
    let entrada = EntradaCola { id: nuevo_id(), trabajo: trabajo.into(), motivo: motivo.into(), encolado_en: ahora_rfc3339() };
    escribir_json(&dir.join(format!("{}.json", entrada.id)), &entrada)?;
    Ok(entrada.id)
}

pub(super) fn listar_en(dir: &Path) -> Resultado<Vec<EntradaCola>> {
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.into()),
    };
    let mut nombres: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            !n.starts_with('.') && n.ends_with(".json")
        })
        .collect();
    nombres.sort();
    Ok(nombres.iter().filter_map(|p| fs::read(p).ok()).filter_map(|b| serde_json::from_slice(&b).ok()).collect())
}

fn tomar_en(dir: &Path, admite: &dyn Fn(&str) -> bool) -> Resultado<Option<String>> {
    fn listar(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
        let mut v: Vec<PathBuf> = fs::read_dir(dir)?
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                !n.starts_with('.') && n.ends_with(".json")
            })
            .collect();
        v.sort();
        Ok(v)
    }
    // Sondeo barato: con la cola vacía (o sin directorio) no se crea ni se
    // abre el cerrojo.
    match listar(dir) {
        Ok(v) if v.is_empty() => return Ok(None),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    // Un solo reclamante a la vez en esta cola (entre hilos y procesos): el
    // `rename` basta en Unix, pero en Windows dos reclamos simultáneos del
    // mismo fichero no son fiables.
    let _cerrojo = {
        let f = fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(".tomar.lock"))?;
        f.lock()?;
        f
    };
    // Dentro del cerrojo se vuelve a listar: otro pudo llevarse las candidatas.
    let candidatas = match listar(dir) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    for p in candidatas {
        // Con filtro, se mira la entrada antes de reclamarla; si no pasa, se queda.
        if let Some(e) = fs::read(&p).ok().and_then(|b| serde_json::from_slice::<EntradaCola>(&b).ok())
            && !admite(&e.trabajo)
        {
            continue;
        }
        let nombre = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        // Único por reclamo: dos hilos del mismo proceso no pueden pisarse.
        static RECLAMOS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = RECLAMOS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let reclamada = dir.join(format!(".{nombre}.tomada-{}-{n}", std::process::id()));
        if fs::rename(&p, &reclamada).is_err() {
            continue; // otro proceso se adelantó
        }
        let entrada: Option<EntradaCola> = fs::read(&reclamada).ok().and_then(|b| serde_json::from_slice(&b).ok());
        let _ = fs::remove_file(&reclamada);
        if let Some(e) = entrada {
            return Ok(Some(e.trabajo));
        }
    }
    Ok(None)
}
