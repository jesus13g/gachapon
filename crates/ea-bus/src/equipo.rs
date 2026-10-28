//! Equipos de agentes (spec 019): `equipos/<id>/equipo.json`, escrito con
//! `fs_atomico` y con un bloqueo para toda lectura-modificación-escritura.
//!
//! Un equipo es un grupo de agentes del catálogo con un rol cada uno dentro
//! del equipo. La regla de sus miembros ([`validar_equipo`]) es única para la
//! CLI, la web y el orquestador (RF-002).

use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;

use ea_ajustes::{Ajustes, IDS_AGENTES, Rol, buscar_binario, ficha};
use ea_store::{EventOutcome, NewEvent};
use serde::{Deserialize, Serialize};

use crate::bus::{Bus, Cola, ahora_rfc3339, nuevo_id};
use crate::error::{ErrorBus, Resultado};
use crate::fs_atomico::escribir_json;
use crate::tipos::{Equipo, EstadoEquipo, Orden};

/// El papel de un agente dentro de un equipo. El revisor es un planificador
/// que solo revisa: no crea trabajo ni cierra ni reformula.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RolEquipo {
    Planificador,
    Ejecutor,
    Revisor,
}

impl RolEquipo {
    pub fn as_str(self) -> &'static str {
        match self {
            RolEquipo::Planificador => "planificador",
            RolEquipo::Ejecutor => "ejecutor",
            RolEquipo::Revisor => "revisor",
        }
    }

    pub fn desde(texto: &str) -> Option<RolEquipo> {
        match texto.trim() {
            "planificador" => Some(RolEquipo::Planificador),
            "ejecutor" => Some(RolEquipo::Ejecutor),
            "revisor" => Some(RolEquipo::Revisor),
            _ => None,
        }
    }

    /// El rol del catálogo que el agente tiene que admitir para tener este.
    pub fn rol_base(self) -> Rol {
        match self {
            RolEquipo::Ejecutor => Rol::Ejecutor,
            RolEquipo::Planificador | RolEquipo::Revisor => Rol::Planificador,
        }
    }
}

/// Un miembro de un equipo: el id del agente del catálogo y su rol en él.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Miembro {
    pub agente: String,
    pub rol: RolEquipo,
}

/// Comprueba los miembros de un equipo contra el catálogo y los ajustes:
/// - cada agente es del catálogo y aparece una sola vez;
/// - cada uno admite el rol que se le pide (`ejecutor` → Ejecutor;
///   `planificador` y `revisor` → Planificador);
/// - el ejecutor tiene un perfil de modelo que existe y es local (art. VII);
/// - hay al menos un ejecutor y un planificador.
///
/// Devuelve el motivo del primer problema ([`ErrorBus::Argumentos`]). La
/// disponibilidad del binario es aparte ([`comprobar_disponibles`]).
pub fn validar_equipo(miembros: &[Miembro], a: &Ajustes) -> Resultado<()> {
    let mut vistos: Vec<&str> = Vec::new();
    for m in miembros {
        let f = ficha(&m.agente).ok_or_else(|| ErrorBus::Argumentos(format!("{:?} no es un agente del catálogo", m.agente)))?;
        if vistos.contains(&m.agente.as_str()) {
            return Err(ErrorBus::Argumentos(format!("{} aparece más de una vez: un agente ocupa un solo rol en el equipo", f.nombre)));
        }
        vistos.push(&m.agente);
        if !f.admite(m.rol.rol_base()) {
            return Err(ErrorBus::Argumentos(format!("{} no puede ser {}", f.nombre, m.rol.as_str())));
        }
        if m.rol == RolEquipo::Ejecutor {
            let perfil = a.agentes.get(&m.agente).map(|g| g.modelo.as_str()).unwrap_or_default();
            if perfil.is_empty() {
                return Err(ErrorBus::Argumentos(format!("{} es ejecutor y necesita un perfil de modelo local (agentes.{}.modelo)", f.nombre, m.agente)));
            }
            match a.modelos.get(perfil) {
                None => return Err(ErrorBus::Argumentos(format!("{}: no existe el perfil de modelo {perfil:?}", f.nombre))),
                Some(p) if p.remoto => {
                    return Err(ErrorBus::Argumentos(format!(
                        "{} es ejecutor y su perfil {perfil:?} es remoto: el ejecutor solo usa modelos locales",
                        f.nombre
                    )));
                }
                Some(_) => {}
            }
        }
    }
    if !miembros.iter().any(|m| m.rol == RolEquipo::Ejecutor) {
        return Err(ErrorBus::Argumentos("el equipo necesita al menos un ejecutor".into()));
    }
    if !miembros.iter().any(|m| m.rol == RolEquipo::Planificador) {
        return Err(ErrorBus::Argumentos("el equipo necesita al menos un planificador".into()));
    }
    Ok(())
}

/// Todos los agentes del equipo tienen su binario en `path` (o en la ruta
/// indicada en `agentes.<id>.bin`). Aparte de [`validar_equipo`] para que las
/// pruebas no dependan del `PATH` real.
pub fn comprobar_disponibles(miembros: &[Miembro], a: &Ajustes, path: Option<&OsStr>) -> Resultado<()> {
    for m in miembros {
        let bin = a.agentes.get(&m.agente).map(|g| g.bin.as_str()).unwrap_or_default();
        if buscar_binario(bin, path).is_none() {
            let nombre = ficha(&m.agente).map_or(m.agente.as_str(), |f| f.nombre);
            return Err(ErrorBus::Argumentos(format!("{nombre} no está disponible: no se encuentra el binario {bin:?}")));
        }
    }
    Ok(())
}

/// `agente:rol` → miembro. El rol es `planificador`, `ejecutor` o `revisor`.
pub fn parse_miembro(texto: &str) -> Resultado<Miembro> {
    let (agente, rol) = texto.split_once(':').ok_or_else(|| ErrorBus::Argumentos(format!("{texto:?}: se espera agente:rol (p. ej. codex:ejecutor)")))?;
    let (agente, rol) = (agente.trim(), rol.trim());
    if ficha(agente).is_none() {
        return Err(ErrorBus::Argumentos(format!("{agente:?} no es un agente del catálogo ({})", IDS_AGENTES.join(", "))));
    }
    let rol = RolEquipo::desde(rol).ok_or_else(|| ErrorBus::Argumentos(format!("{rol:?} no es un rol de equipo (planificador, ejecutor o revisor)")))?;
    Ok(Miembro { agente: agente.into(), rol })
}

/// «Despertar a todos»: los agentes del catálogo cuyo binario está en `path`.
/// Los que admiten ejecutor van a ejecutor, salvo el que ocupa
/// `roles.planificador`, que va a planificador; los que solo admiten
/// planificador (Claude Code, Antigravity) van a planificador. Devuelve los
/// miembros y los agentes omitidos con el motivo; es un error que alguien
/// quede en un papel imposible (p. ej. un ejecutor con modelo remoto) o que
/// falte el planificador de `roles.planificador`.
pub fn repartir_todos(a: &Ajustes, path: Option<&OsStr>) -> Resultado<(Vec<Miembro>, Vec<String>)> {
    let mut miembros = Vec::new();
    let mut omitidos = Vec::new();
    for id in IDS_AGENTES {
        let f = ficha(id).expect("del catálogo");
        let bin = a.agentes.get(*id).map(|g| g.bin.as_str()).unwrap_or_default();
        if buscar_binario(bin, path).is_none() {
            omitidos.push(format!("{} (no está disponible: no se encuentra {bin:?})", f.nombre));
            continue;
        }
        let rol = if *id == a.roles.planificador || !f.admite(Rol::Ejecutor) { RolEquipo::Planificador } else { RolEquipo::Ejecutor };
        miembros.push(Miembro { agente: (*id).into(), rol });
    }
    if !miembros.iter().any(|m| m.agente == a.roles.planificador) {
        return Err(ErrorBus::Argumentos(format!(
            "el planificador de roles.planificador ({}) no está disponible: no se puede despertar a todos",
            a.roles.planificador
        )));
    }
    let planificador = miembros.iter().find(|m| m.rol == RolEquipo::Planificador).cloned().expect("hay uno");
    let imposibles: Vec<String> = miembros
        .iter()
        .filter(|m| m.rol == RolEquipo::Ejecutor)
        .filter_map(|m| validar_equipo(&[m.clone(), planificador.clone()], a).err().map(|e| format!("  - {}: {e}", m.agente)))
        .collect();
    if !imposibles.is_empty() {
        return Err(ErrorBus::Argumentos(format!(
            "no se puede despertar a todos:\n{}\nElige a mano con --miembro agente:rol (o cambia su modelo en `ea ajustes`).",
            imposibles.join("\n")
        )));
    }
    Ok((miembros, omitidos))
}

/// Lo que hace falta para despertar un equipo; el id, la fecha y el estado
/// los pone el sistema.
#[derive(Debug, Clone)]
pub struct NuevoEquipo {
    pub titulo: String,
    pub objetivo: String,
    pub worktree_base: String,
    pub miembros: Vec<Miembro>,
    pub creado_por: String,
}

impl Bus {
    fn dir_equipo(&self, id: &str) -> Resultado<PathBuf> {
        if !crate::bus::id_valido(id) {
            return Err(ErrorBus::Argumentos(format!("id de equipo inválido: {id:?}")));
        }
        Ok(self.raiz.join("equipos").join(id))
    }

    /// `flock` exclusivo sobre `equipos/.equipos.lock`: ordena entre procesos
    /// toda escritura de equipos (crear, dormir, anotar un trabajo) y la
    /// comprobación de «un agente, un equipo despierto», y también crear un
    /// trabajo de un equipo y devolver uno a su cola ([`Bus::devolver`]).
    /// Quien solo lee no lo necesita. No es reentrante. Se toma después del
    /// de un trabajo, nunca antes (ver `Bus::bloquear_estado`).
    pub(crate) fn bloquear_equipos(&self) -> Resultado<fs::File> {
        let dir = self.raiz.join("equipos");
        fs::create_dir_all(&dir)?;
        let f = fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(".equipos.lock"))?;
        f.lock()?;
        Ok(f)
    }

    /// Despierta un equipo: valida los miembros con [`validar_equipo`], rechaza
    /// a un agente que ya esté en otro equipo despierto, lo escribe y apunta
    /// `equipo_despertado` en el diario. La disponibilidad de los binarios la
    /// comprueba quien llama ([`comprobar_disponibles`]).
    pub fn crear_equipo(&self, nuevo: NuevoEquipo, ajustes: &Ajustes) -> Resultado<Equipo> {
        let texto = |v: &str, que: &str| -> Resultado<String> {
            let v = v.trim();
            if v.is_empty() || v.chars().any(char::is_control) {
                return Err(ErrorBus::Argumentos(format!("falta {que} (o lleva caracteres de control)")));
            }
            Ok(v.to_string())
        };
        let titulo = texto(&nuevo.titulo, "el título del equipo")?;
        let objetivo = texto(&nuevo.objetivo, "el objetivo del equipo")?;
        let worktree_base = texto(&nuevo.worktree_base, "el worktree base del equipo")?;
        validar_equipo(&nuevo.miembros, ajustes)?;
        let _bloqueo = self.bloquear_equipos()?;
        // Todo ejecutor despierto necesita un trabajador: los ejecutores
        // distintos de los equipos despiertos y del nuevo caben en `paralelo`.
        // También el agente de `roles.ejecutor`, que siempre tiene trabajador.
        let mut ejecutores: Vec<String> = vec![ajustes.agente_de(Rol::Ejecutor).0.to_string()];
        ejecutores.extend(nuevo.miembros.iter().filter(|m| m.rol == RolEquipo::Ejecutor).map(|m| m.agente.clone()));
        for otro in self.listar_equipos()?.iter().filter(|e| e.estado == EstadoEquipo::Despierto) {
            ejecutores.extend(otro.miembros.iter().filter(|m| m.rol == RolEquipo::Ejecutor).map(|m| m.agente.clone()));
        }
        ejecutores.sort();
        ejecutores.dedup();
        let tope = ajustes.orquestador.paralelo as usize;
        if ejecutores.len() > tope {
            return Err(ErrorBus::Argumentos(format!(
                "habría {} ejecutores despiertos ({}; el agente de roles.ejecutor siempre cuenta) y orquestador.paralelo es {tope}: \
                 sube orquestador.paralelo (`ea ajustes establecer orquestador.paralelo {}`), duerme otro equipo o, con \
                 paralelo 1, usa como único ejecutor al de roles.ejecutor",
                ejecutores.len(),
                ejecutores.join(", "),
                ejecutores.len()
            )));
        }
        for otro in self.listar_equipos()?.iter().filter(|e| e.estado == EstadoEquipo::Despierto) {
            if let Some(m) = nuevo.miembros.iter().find(|m| otro.miembro(&m.agente).is_some()) {
                return Err(ErrorBus::Argumentos(format!(
                    "{} ya está en el equipo despierto {} ({}): un agente solo está en un equipo a la vez",
                    m.agente, otro.id, otro.titulo
                )));
            }
        }
        let equipo = Equipo {
            id: nuevo_id(),
            titulo,
            objetivo,
            worktree_base,
            miembros: nuevo.miembros,
            estado: EstadoEquipo::Despierto,
            creado_en: ahora_rfc3339(),
            creado_por: nuevo.creado_por,
            trabajos: vec![],
        };
        escribir_json(&self.dir_equipo(&equipo.id)?.join("equipo.json"), &equipo)?;
        self.diario_equipo("equipo_despertado", &equipo);
        Ok(equipo)
    }

    pub fn leer_equipo(&self, id: &str) -> Resultado<Equipo> {
        match fs::read(self.dir_equipo(id)?.join("equipo.json")) {
            Ok(b) => Ok(serde_json::from_slice(&b)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(ErrorBus::NoExiste(format!("equipo {id}"))),
            Err(e) => Err(e.into()),
        }
    }

    /// Todos los equipos, del más antiguo al más reciente. Salta los ficheros
    /// ilegibles.
    pub fn listar_equipos(&self) -> Resultado<Vec<Equipo>> {
        let mut ids: Vec<String> = fs::read_dir(self.raiz.join("equipos"))?
            .flatten()
            .filter(|e| e.path().join("equipo.json").exists())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        ids.sort();
        Ok(ids.iter().filter_map(|id| self.leer_equipo(id).ok()).collect())
    }

    /// Duerme el equipo. Lo pendiente en la cola de cada uno de sus
    /// ejecutores vuelve a su cola vigente, que con el equipo ya dormido es la
    /// del rol `modelo-local` (motivo «equipo dormido»), para que no quede
    /// huérfano: lo toma cualquier ejecutor. Dormir uno dormido no hace nada.
    pub fn dormir_equipo(&self, id: &str) -> Resultado<Equipo> {
        let _bloqueo = self.bloquear_equipos()?;
        let mut equipo = self.leer_equipo(id)?;
        if equipo.estado == EstadoEquipo::Dormido {
            return Ok(equipo);
        }
        equipo.estado = EstadoEquipo::Dormido;
        escribir_json(&self.dir_equipo(id)?.join("equipo.json"), &equipo)?;
        for m in equipo.miembros.iter().filter(|m| m.rol == RolEquipo::Ejecutor) {
            while let Some(trabajo) = self.tomar(Cola::Agente(&m.agente))? {
                self.devolver_bloqueado(&trabajo, "equipo dormido")?;
            }
        }
        // Las sesiones de planificadores y revisores siguen abiertas en su
        // terminal, pero ya no están en el equipo: se les quita la presencia.
        // La de los ejecutores es del trabajador del orquestador, que la quita
        // al reconciliar.
        for m in equipo.miembros.iter().filter(|m| m.rol != RolEquipo::Ejecutor) {
            if self.presencia_de(&m.agente)?.is_some_and(|p| p.equipo.as_deref() == Some(id)) {
                self.quitar_presencia(&m.agente)?;
            }
        }
        self.diario_equipo("equipo_dormido", &equipo);
        Ok(equipo)
    }

    /// Añade el trabajo a la lista del equipo (sin repetir).
    pub fn anotar_trabajo_en_equipo(&self, equipo: &str, trabajo: &str) -> Resultado<()> {
        let _bloqueo = self.bloquear_equipos()?;
        self.anotar_trabajo_en_equipo_bloqueado(equipo, trabajo)
    }

    /// [`Bus::anotar_trabajo_en_equipo`] para quien ya tiene el bloqueo de
    /// los equipos (`crear_trabajo`).
    pub(crate) fn anotar_trabajo_en_equipo_bloqueado(&self, equipo: &str, trabajo: &str) -> Resultado<()> {
        let mut e = self.leer_equipo(equipo)?;
        if !e.trabajos.iter().any(|t| t == trabajo) {
            e.trabajos.push(trabajo.into());
            escribir_json(&self.dir_equipo(equipo)?.join("equipo.json"), &e)?;
        }
        Ok(())
    }

    /// El agente al que va la cola de un trabajo reasignable: `asignado_a`
    /// solo si su equipo sigue despierto y el agente sigue en él.
    pub(crate) fn asignado_vigente(&self, o: &Orden) -> Option<String> {
        let (a, e) = (o.asignado_a.as_ref()?, o.equipo.as_ref()?);
        let equipo = self.leer_equipo(e).ok()?;
        (equipo.estado == EstadoEquipo::Despierto && equipo.con_rol(a, RolEquipo::Ejecutor)).then(|| a.clone())
    }

    /// Las comprobaciones de `crear_trabajo` que dependen del equipo (RF-004).
    /// Devuelve el equipo, si la orden trae uno.
    pub(crate) fn validar_equipo_de_orden(&self, o: &Orden) -> Resultado<Option<Equipo>> {
        let Some(id) = &o.equipo else {
            if o.asignado_a.is_some() || o.revisor.is_some() {
                return Err(ErrorBus::Argumentos("asignado_a y revisor necesitan un equipo".into()));
            }
            return Ok(None);
        };
        let equipo = self.leer_equipo(id)?;
        if equipo.estado != EstadoEquipo::Despierto {
            return Err(ErrorBus::Argumentos(format!("el equipo {id} está dormido")));
        }
        if let Some(a) = &o.asignado_a
            && !equipo.con_rol(a, RolEquipo::Ejecutor)
        {
            return Err(ErrorBus::Argumentos(format!("{a} no es un ejecutor del equipo {id}")));
        }
        if let Some(r) = &o.revisor {
            if !(equipo.con_rol(r, RolEquipo::Revisor) || equipo.con_rol(r, RolEquipo::Planificador)) {
                return Err(ErrorBus::Argumentos(format!("{r} no es un revisor ni un planificador del equipo {id}")));
            }
            if o.asignado_a.as_deref() == Some(r.as_str()) {
                return Err(ErrorBus::Argumentos(format!("{r} no puede revisar lo que ejecuta él mismo")));
            }
        }
        Ok(Some(equipo))
    }

    /// El diario es secundario: un fallo de SQLite no deshace el equipo.
    fn diario_equipo(&self, accion: &str, e: &Equipo) {
        let Some(s) = &self.store else { return };
        let miembros: Vec<String> = e.miembros.iter().map(|m| format!("{}:{}", m.agente, m.rol.as_str())).collect();
        let _ = s.append_event(NewEvent {
            actor: self.actor.clone(),
            action: accion.into(),
            outcome: Some(EventOutcome::Ok),
            detail: Some(serde_json::json!({ "equipo": e.id, "titulo": e.titulo, "miembros": miembros }).to_string()),
            ..Default::default()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ea_ajustes::Fuentes;

    fn ajustes() -> (tempfile::TempDir, Ajustes) {
        let d = tempfile::tempdir().unwrap();
        let a = Ajustes::por_defecto(&Fuentes::aislada(d.path()));
        (d, a)
    }

    fn m(agente: &str, rol: RolEquipo) -> Miembro {
        Miembro { agente: agente.into(), rol }
    }

    fn bien() -> Vec<Miembro> {
        vec![m("claude-code", RolEquipo::Planificador), m("qwen-code", RolEquipo::Ejecutor), m("codex", RolEquipo::Ejecutor)]
    }

    #[test]
    fn un_equipo_correcto_pasa_y_el_revisor_es_opcional() {
        let (_d, a) = ajustes();
        validar_equipo(&bien(), &a).unwrap();
        let mut con_revisor = bien();
        con_revisor.push(m("antigravity", RolEquipo::Revisor));
        validar_equipo(&con_revisor, &a).unwrap();
    }

    #[test]
    fn rechaza_agente_repetido_desconocido_y_rol_no_admitido() {
        let (_d, a) = ajustes();
        let mut v = bien();
        v.push(m("codex", RolEquipo::Revisor));
        assert!(validar_equipo(&v, &a).unwrap_err().to_string().contains("más de una vez"));
        let mut v = bien();
        v.push(m("otro", RolEquipo::Revisor));
        assert!(validar_equipo(&v, &a).unwrap_err().to_string().contains("catálogo"));
        // Antigravity solo admite Planificador: no puede ejecutar
        let mut v = bien();
        v.push(m("antigravity", RolEquipo::Ejecutor));
        assert!(validar_equipo(&v, &a).unwrap_err().to_string().contains("no puede ser ejecutor"));
        // Claude Code solo admite Planificador
        let v = vec![m("claude-code", RolEquipo::Ejecutor), m("qwen-code", RolEquipo::Planificador)];
        assert!(validar_equipo(&v, &a).is_err());
    }

    #[test]
    fn el_ejecutor_necesita_modelo_local() {
        let (_d, mut a) = ajustes();
        a.modelos.get_mut("local").unwrap().remoto = true;
        assert!(validar_equipo(&bien(), &a).unwrap_err().to_string().contains("remoto"));
        a.modelos.get_mut("local").unwrap().remoto = false;
        a.agentes.get_mut("codex").unwrap().modelo = String::new();
        assert!(validar_equipo(&bien(), &a).unwrap_err().to_string().contains("perfil de modelo local"));
        a.agentes.get_mut("codex").unwrap().modelo = "fantasma".into();
        assert!(validar_equipo(&bien(), &a).unwrap_err().to_string().contains("no existe el perfil"));
    }

    #[test]
    fn exige_un_ejecutor_y_un_planificador() {
        let (_d, a) = ajustes();
        let sin_ej = vec![m("claude-code", RolEquipo::Planificador), m("antigravity", RolEquipo::Revisor)];
        assert!(validar_equipo(&sin_ej, &a).unwrap_err().to_string().contains("ejecutor"));
        let sin_pl = vec![m("qwen-code", RolEquipo::Ejecutor), m("antigravity", RolEquipo::Revisor)];
        assert!(validar_equipo(&sin_pl, &a).unwrap_err().to_string().contains("planificador"));
        assert!(validar_equipo(&[], &a).is_err());
    }

    #[test]
    fn la_disponibilidad_se_comprueba_aparte() {
        let (d, mut a) = ajustes();
        let bin = d.path().join(ea_plataforma::rutas::nombre_exe("falso"));
        std::fs::write(&bin, "#!/bin/sh\n").unwrap();
        ea_plataforma::fs::poner_permisos_unix(&bin, 0o755).unwrap();
        for id in ["claude-code", "qwen-code", "codex"] {
            a.agentes.get_mut(id).unwrap().bin = bin.to_string_lossy().into_owned();
        }
        comprobar_disponibles(&bien(), &a, None).unwrap();
        a.agentes.get_mut("codex").unwrap().bin = "no-existe-nunca".into();
        assert!(comprobar_disponibles(&bien(), &a, None).unwrap_err().to_string().contains("Codex"));
    }

    #[test]
    fn el_rol_se_escribe_en_minusculas() {
        assert_eq!(serde_json::to_string(&m("codex", RolEquipo::Revisor)).unwrap(), r#"{"agente":"codex","rol":"revisor"}"#);
        assert_eq!(RolEquipo::desde("ejecutor"), Some(RolEquipo::Ejecutor));
        assert_eq!(RolEquipo::desde("modelo-local"), None);
    }

    /// Un directorio con binarios de mentira para los ids dados.
    fn bin(ids: &[&str], a: &Ajustes) -> (tempfile::TempDir, std::ffi::OsString) {
        let d = tempfile::tempdir().unwrap();
        for id in ids {
            let p = d.path().join(ea_plataforma::rutas::nombre_exe(&a.agentes[*id].bin));
            std::fs::write(&p, "#!/bin/sh\n").unwrap();
            ea_plataforma::fs::poner_permisos_unix(&p, 0o755).unwrap();
        }
        let path = d.path().as_os_str().to_owned();
        (d, path)
    }

    #[test]
    fn parsea_agente_y_rol() {
        assert_eq!(parse_miembro("codex:ejecutor").unwrap(), Miembro { agente: "codex".into(), rol: RolEquipo::Ejecutor });
        assert_eq!(parse_miembro(" antigravity : revisor ").unwrap().rol, RolEquipo::Revisor);
        assert!(parse_miembro("codex").unwrap_err().to_string().contains("agente:rol"));
        assert!(parse_miembro("nadie:ejecutor").unwrap_err().to_string().contains("catálogo"));
        assert!(parse_miembro("codex:jefe").unwrap_err().to_string().contains("rol de equipo"));
    }

    #[test]
    fn todos_reparte_segun_lo_que_admite_cada_agente() {
        let (_d, a) = ajustes();
        let (_b, path) = bin(IDS_AGENTES, &a);
        let (m, omitidos) = repartir_todos(&a, Some(&path)).unwrap();
        let rol = |id: &str| m.iter().find(|x| x.agente == id).map(|x| x.rol);
        assert_eq!(rol("claude-code"), Some(RolEquipo::Planificador));
        assert_eq!(rol("antigravity"), Some(RolEquipo::Planificador));
        assert_eq!(rol("qwen-code"), Some(RolEquipo::Ejecutor));
        assert_eq!(rol("codex"), Some(RolEquipo::Ejecutor));
        assert_eq!(rol("opencode"), Some(RolEquipo::Ejecutor));
        assert!(omitidos.is_empty());
        // El que ocupa roles.planificador va a planificador aunque admita ejecutor.
        let mut b = a.clone();
        b.roles.planificador = "codex".into();
        let (m, _) = repartir_todos(&b, Some(&path)).unwrap();
        assert_eq!(m.iter().find(|x| x.agente == "codex").unwrap().rol, RolEquipo::Planificador);
    }

    #[test]
    fn todos_omite_lo_que_no_esta_y_dice_lo_imposible() {
        let (_d, a) = ajustes();
        let (_b, path) = bin(&["claude-code", "qwen-code"], &a);
        let (m, omitidos) = repartir_todos(&a, Some(&path)).unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(omitidos.len(), 3);
        assert!(omitidos.iter().any(|o| o.contains("Codex") && o.contains("no está disponible")));
        // sin el planificador de los roles
        let (_b2, path) = bin(&["qwen-code"], &a);
        assert!(repartir_todos(&a, Some(&path)).unwrap_err().to_string().contains("planificador"));
        // un ejecutor con modelo remoto es imposible
        let (_b3, path) = bin(&["claude-code", "qwen-code", "codex"], &a);
        let mut r = a.clone();
        let mut remoto = r.modelos[&r.agentes["codex"].modelo].clone();
        remoto.remoto = true;
        r.modelos.insert("remoto".into(), remoto);
        r.agentes.get_mut("codex").unwrap().modelo = "remoto".into();
        let e = repartir_todos(&r, Some(&path)).unwrap_err().to_string();
        assert!(e.contains("codex") && e.contains("remoto") && e.contains("--miembro"), "{e}");
    }
}
