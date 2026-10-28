//! Crear un trabajo (validar la orden, escribirla y encolarla) y leer lo
//! que hay en `trabajos/<id>/`.

use std::fs;
use std::path::Path;

use ea_store::{FilaFase, FilaTrabajo};

use super::{ahora_rfc3339, espejo, nuevo_id, Bus, Cola};
use crate::equipo::RolEquipo;
use crate::error::{ErrorBus, Resultado};
use crate::fs_atomico::escribir_json;
use crate::skills::{pide_skills, ConfigSkills};
use crate::tipos::{Estado, EstadoFase, EstadoTrabajo, Informe, Modo, Orden, Rol};

impl Bus {
    /// Escribe `orden.json` y `estado.json` y encola en `modelo-local`. Si la
    /// orden trae el id vacío se le asigna uno nuevo. Antes de escribir nada
    /// valida la estructura (`validar_estructura`: worktree absoluto, fases
    /// 1..N), los criterios y las skills (llave, existencia y topes), de modo
    /// que cualquier punto de entrada (CLI, ea-web, rutinas) lo hereda. Que el
    /// worktree exista y sea un repo con commits lo comprueban la CLI y la web
    /// con [`crate::validar_worktree`] (aquí no se ejecuta git).
    ///
    /// Con `equipo` (spec 019, RF-004) comprueba además que el equipo existe y
    /// está despierto, que `asignado_a` es un ejecutor suyo, que `revisor` es
    /// un revisor o planificador suyo y distinto de `asignado_a`. Con
    /// `asignado_a` la orden entra en la cola de ese agente; si no, en la del
    /// rol ejecutor. Los planificadores del equipo tienen la llave de las
    /// skills; los revisores no. Esa validación, el encolado y la anotación
    /// en el equipo van bajo el bloqueo de los equipos: un `dormir_equipo` a
    /// la vez no deja la orden en la cola de un agente que ya nadie atiende.
    ///
    /// El espejo en SQLite (filas de `trabajos` y `trabajo_fases`) se escribe
    /// después de encolar y es de mejor esfuerzo, como el diario: si falla,
    /// el trabajo queda creado y en su cola.
    pub fn crear_trabajo(&self, mut orden: Orden) -> Resultado<String> {
        validar_estructura(&orden)?;
        validar_criterios(&orden)?;
        let bloqueo_equipos = if orden.equipo.is_some() { Some(self.bloquear_equipos()?) } else { None };
        let equipo = self.validar_equipo_de_orden(&orden)?;
        if pide_skills(&orden) {
            let mut cfg = match &self.skills {
                Some(cfg) => cfg.clone(),
                None => ConfigSkills::desde_ajustes(&ea_ajustes::cargar()),
            };
            if equipo.as_ref().is_some_and(|e| e.con_rol(&orden.creada_por, RolEquipo::Planificador)) {
                cfg.pueden_elegir.push(orden.creada_por.clone());
            }
            cfg.validar_orden(&orden)?;
        }
        if orden.id.is_empty() {
            orden.id = nuevo_id();
        }
        if orden.creada_en.is_empty() {
            orden.creada_en = ahora_rfc3339();
        }
        let id = orden.id.clone();
        let dir = self.dir_trabajo(&id)?;
        if dir.join("orden.json").exists() {
            return Err(ErrorBus::Argumentos(format!("el trabajo {id} ya existe")));
        }
        escribir_json(&dir.join("orden.json"), &orden)?;
        let estado = Estado {
            estado: EstadoTrabajo::EnCola,
            fase_actual: orden.fases.first().map(|f| f.numero),
            iteracion: 0,
            actualizado_en: ahora_rfc3339(),
            detalle: None,
            ejecutado_por: None,
        };
        escribir_json(&dir.join("estado.json"), &estado)?;
        self.historial(&id, "crear", &estado)?;
        if let Some(s) = &self.store {
            self.diario(s, &id, "bus_crear", true, serde_json::json!({ "modo": orden.modo.as_str(), "titulo": orden.titulo }));
        }
        match &orden.asignado_a {
            Some(agente) => self.encolar(Cola::Agente(agente), &id, "orden nueva")?,
            None => self.encolar(Rol::Ejecutor, &id, "orden nueva")?,
        };
        if let Some(e) = &orden.equipo {
            self.anotar_trabajo_en_equipo_bloqueado(e, &id)?;
        }
        drop(bloqueo_equipos);
        if let Some(s) = &self.store {
            espejo(
                &id,
                "insertar el trabajo",
                s.insertar_trabajo(&FilaTrabajo {
                    id: id.clone(),
                    modo: orden.modo.as_str().into(),
                    titulo: orden.titulo.clone(),
                    worktree: orden.worktree.clone(),
                    estado: estado.estado.as_str().into(),
                    creado_en: orden.creada_en.clone(),
                    actualizado_en: estado.actualizado_en.clone(),
                    creado_por: orden.creada_por.clone(),
                    rutina_slug: orden.rutina.as_ref().map(|r| r.slug.clone()),
                    rutina_version: orden.rutina.as_ref().map(|r| i64::from(r.version)),
                }),
            );
            for f in &orden.fases {
                let fila = FilaFase {
                    trabajo_id: id.clone(),
                    numero: i64::from(f.numero),
                    titulo: f.titulo.clone(),
                    estado: EstadoFase::Pendiente.as_str().into(),
                    informe: None,
                    cerrado_en: None,
                };
                espejo(&id, "guardar la fase", s.guardar_fase(&fila));
            }
        }
        Ok(id)
    }

    pub fn leer_orden(&self, id: &str) -> Resultado<Orden> {
        self.leer(id, "orden.json")
    }

    pub fn leer_estado(&self, id: &str) -> Resultado<Estado> {
        self.leer(id, "estado.json")
    }

    pub fn leer_informe(&self, id: &str) -> Resultado<Option<Informe>> {
        let p = self.dir_trabajo(id)?.join("informe.json");
        match fs::read(&p) {
            Ok(b) => Ok(Some(serde_json::from_slice(&b)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn escribir_informe(&self, id: &str, informe: &Informe) -> Resultado<()> {
        let dir = self.dir_trabajo(id)?;
        if !dir.exists() {
            return Err(ErrorBus::NoExiste(id.into()));
        }
        escribir_json(&dir.join("informe.json"), informe)
    }

    /// Ids de todos los trabajos, del más antiguo al más reciente.
    pub fn listar_trabajos(&self) -> Resultado<Vec<String>> {
        let mut v: Vec<String> = fs::read_dir(self.raiz.join("trabajos"))?
            .flatten()
            .filter(|e| e.path().join("orden.json").exists())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        v.sort();
        Ok(v)
    }

    fn leer<T: serde::de::DeserializeOwned>(&self, id: &str, fichero: &str) -> Resultado<T> {
        let p = self.dir_trabajo(id)?.join(fichero);
        match fs::read(&p) {
            Ok(b) => Ok(serde_json::from_slice(&b)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(ErrorBus::NoExiste(id.into())),
            Err(e) => Err(e.into()),
        }
    }
}

/// Lo que toda orden cumple, la cree quien la cree, sin mirar el disco ni
/// ejecutar git: el worktree es una ruta absoluta (una relativa se
/// resolvería contra el directorio del proceso que la ejecuta, el `$HOME`
/// del servicio) y las fases de una obra se numeran 1, 2, …, N en el orden en
/// que van (el ejecutor reanuda por número, y con 10/20/30 se saltaba fases).
fn validar_estructura(o: &Orden) -> Resultado<()> {
    if !Path::new(&o.worktree).is_absolute() {
        return Err(ErrorBus::Argumentos("'worktree' debe ser una ruta absoluta".into()));
    }
    if o.modo == Modo::Obra && o.fases.iter().enumerate().any(|(i, f)| f.numero as usize != i + 1) {
        let llegan: Vec<String> = o.fases.iter().map(|f| f.numero.to_string()).collect();
        return Err(ErrorBus::Argumentos(format!(
            "las fases deben numerarse 1, 2, …, N, en el orden en que van (esta obra trae {})",
            llegan.join(", ")
        )));
    }
    Ok(())
}

/// El criterio de éxito es obligatorio: sin él, nadie puede decir que algo
/// está hecho. Misión y rutina lo llevan en la orden; la obra, en cada fase
/// (o en la orden, si no tiene fases).
fn validar_criterios(o: &Orden) -> Resultado<()> {
    let vacio = |c: &Option<String>| c.as_deref().is_none_or(|c| c.trim().is_empty());
    match o.modo {
        Modo::Obra if !o.fases.is_empty() => {
            if let Some(f) = o.fases.iter().find(|f| f.criterio.trim().is_empty()) {
                return Err(ErrorBus::Argumentos(format!("la fase {} no tiene criterio", f.numero)));
            }
        }
        _ if vacio(&o.criterio_exito) => {
            return Err(ErrorBus::Argumentos("falta criterio_exito: es obligatorio".into()));
        }
        _ => {}
    }
    Ok(())
}
