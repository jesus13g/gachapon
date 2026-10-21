//! De dónde salen los ajustes (directorios y entorno) y su forma tipada.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::agentes::{Rol, CATALOGO};
use crate::validar::expandir_tilde;

/// Cambia el directorio de configuración (`ajustes.toml`, los TOML heredados,
/// las rutinas y las skills de Qwen). Sirve sobre todo para aislar pruebas.
pub const ENV_CONFIG: &str = "EA_CONFIG_DIR";

/// Las variables de entorno que se consultan: las del proceso o un mapa fijo
/// (las pruebas nunca tocan el entorno del proceso).
#[derive(Debug, Clone)]
pub enum Entorno {
    Proceso,
    Fijo(BTreeMap<String, String>),
}

impl Entorno {
    /// Valor no vacío (sin espacios en los extremos) de una variable.
    pub fn var(&self, nombre: &str) -> Option<String> {
        let v = match self {
            Entorno::Proceso => std::env::var(nombre).ok(),
            Entorno::Fijo(m) => m.get(nombre).cloned(),
        }?;
        let v = v.trim();
        (!v.is_empty()).then(|| v.to_string())
    }
}

/// Directorios base y entorno con los que se resuelven los ajustes.
#[derive(Debug, Clone)]
pub struct Fuentes {
    pub dir_config: PathBuf,
    pub dir_datos: PathBuf,
    pub dir_estado: PathBuf,
    pub entorno: Entorno,
}

impl Fuentes {
    /// Los directorios de siempre (`ProjectDirs` de «ecosistema-asistente»):
    /// configuración en `config_dir` (o `$EA_CONFIG_DIR`), datos en
    /// `data_dir` y estado en `state_dir` (o `data_local_dir` si el sistema
    /// no tiene). Mismos valores que antes de existir `ajustes.toml`.
    pub fn sistema() -> Self {
        let dirs = directories::ProjectDirs::from("", "", "ecosistema-asistente");
        let entorno = Entorno::Proceso;
        let dir_config = match entorno.var(ENV_CONFIG) {
            Some(d) => expandir_tilde(&d),
            None => dirs.as_ref().map(|d| d.config_dir().to_path_buf()).unwrap_or_else(|| PathBuf::from("config")),
        };
        let dir_datos = dirs.as_ref().map(|d| d.data_dir().to_path_buf()).unwrap_or_default();
        let dir_estado = dirs
            .as_ref()
            .map(|d| d.state_dir().unwrap_or_else(|| d.data_local_dir()).to_path_buf())
            .unwrap_or_else(|| PathBuf::from("estado"));
        Self { dir_config, dir_datos, dir_estado, entorno }
    }

    /// Todo bajo `raiz` (`config/`, `datos/`, `estado/`) y sin variables de
    /// entorno: para pruebas.
    pub fn aislada(raiz: &Path) -> Self {
        Self {
            dir_config: raiz.join("config"),
            dir_datos: raiz.join("datos"),
            dir_estado: raiz.join("estado"),
            entorno: Entorno::Fijo(BTreeMap::new()),
        }
    }

    /// Añade una variable al entorno fijo (en el del proceso no hace nada:
    /// ese no se toca desde aquí).
    pub fn con_var(mut self, nombre: &str, valor: &str) -> Self {
        if let Entorno::Fijo(m) = &mut self.entorno {
            m.insert(nombre.to_string(), valor.to_string());
        }
        self
    }

    /// `ajustes.toml`, el único fichero que el sistema escribe.
    pub fn fichero(&self) -> PathBuf {
        self.dir_config.join("ajustes.toml")
    }

    /// Ficheros antiguos que se siguen leyendo (nunca se escriben).
    pub fn heredados(&self) -> [(&'static str, PathBuf); 3] {
        [
            ("orquestador.toml", self.dir_config.join("orquestador.toml")),
            ("orchestrator.toml", self.dir_config.join("orchestrator.toml")),
            ("boveda.toml", self.dir_config.join("boveda.toml")),
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Ajustes {
    /// Perfiles de modelo por id (`modelos.<id>`); siempre está `local`.
    pub modelos: BTreeMap<String, PerfilModelo>,
    /// Configuración de cada agente del catálogo, por id (todos están).
    pub agentes: BTreeMap<String, ConfigAgente>,
    pub roles: Roles,
    /// Contención, topes y skills del ejecutor.
    pub agente: Agente,
    pub orquestador: Orquestador,
    pub red: Red,
    pub rutas: Rutas,
    pub web: Web,
    pub biblioteca: AjustesBiblioteca,
}

/// Un perfil de modelo. `base_url` vacío: el endpoint propio del agente (solo
/// vale si es remoto). Un perfil no remoto tiene un `base_url` de loopback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PerfilModelo {
    pub nombre: String,
    pub base_url: String,
    pub remoto: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfigAgente {
    /// Programa que se lanza (o cuya disponibilidad se mira).
    pub bin: String,
    /// Id del perfil de modelo; vacío: el que tenga configurado el propio
    /// agente (solo para un planificador).
    pub modelo: String,
}

/// Qué agente del catálogo ocupa cada rol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Roles {
    pub planificador: String,
    pub ejecutor: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Agente {
    pub sandbox: bool,
    pub avisos_escritorio: bool,
    pub techo_topes: TechoTopes,
    pub skills: Skills,
}

/// Techo de los topes de cualquier orden: el ejecutor recorta cada tope de la
/// orden a este valor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TechoTopes {
    pub turnos: u32,
    pub tiempo_s: u64,
    pub llamadas_herramienta: u32,
    pub iteraciones: u32,
}

/// Lo máximo que puede valer el techo, también desde la terminal.
pub const TECHO_DURO: TechoTopes = TechoTopes { turnos: 100, tiempo_s: 3600, llamadas_herramienta: 200, iteraciones: 10 };

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Skills {
    /// Catálogo de skills de Qwen (ya resuelto: nunca vacío).
    pub dir: String,
    pub habilitadas: Vec<String>,
    pub max_por_trabajo: usize,
    pub max_caracteres: usize,
    pub pueden_elegir: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Orquestador {
    pub intervalo_sondeo_s: u64,
    pub intervalo_heartbeat_s: u64,
    pub rutinas_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Red {
    pub puerto_web: u16,
    pub puerto_mcp: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rutas {
    pub db: PathBuf,
    pub estado: PathBuf,
    pub boveda: PathBuf,
}

/// Ajustes de la biblioteca de herramientas (spec 018).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct AjustesBiblioteca {
    /// Categoría propia por id de elemento (`<agente>/<tipo>/<ámbito>/<nombre>`).
    pub categorias: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Web {
    pub sondeo_ms: u64,
    pub diario_limite: u32,
}

impl Ajustes {
    /// Los valores por defecto. Es el único sitio del código donde se nombran
    /// el modelo y su endpoint (art. VII).
    pub fn por_defecto(f: &Fuentes) -> Self {
        let local = PerfilModelo {
            nombre: "qwen3-coder-30b".into(),
            base_url: "http://127.0.0.1:8080/v1".into(),
            remoto: false,
        };
        let agentes = CATALOGO
            .iter()
            .map(|a| {
                let (bin, modelo) = match a.id {
                    "claude-code" => ("claude", ""),
                    "qwen-code" => ("qwen", "local"),
                    "codex" => ("codex", "local"),
                    "opencode" => ("opencode", "local"),
                    "antigravity" => ("agy", ""),
                    otro => (otro, ""),
                };
                (a.id.to_string(), ConfigAgente { bin: bin.into(), modelo: modelo.into() })
            })
            .collect();
        Self {
            modelos: BTreeMap::from([("local".to_string(), local)]),
            agentes,
            roles: Roles { planificador: "claude-code".into(), ejecutor: "qwen-code".into() },
            agente: Agente {
                sandbox: true,
                avisos_escritorio: true,
                techo_topes: TECHO_DURO,
                skills: Skills {
                    dir: f.dir_config.join("qwen-skills").to_string_lossy().into_owned(),
                    habilitadas: vec![],
                    max_por_trabajo: 3,
                    max_caracteres: 6000,
                    pueden_elegir: vec!["claude-code".into()],
                },
            },
            orquestador: Orquestador {
                intervalo_sondeo_s: 5,
                intervalo_heartbeat_s: 30,
                rutinas_dir: f.dir_config.join("rutinas"),
            },
            red: Red { puerto_web: 8092, puerto_mcp: 8765 },
            rutas: Rutas {
                db: f.dir_datos.join("ecosistema.db"),
                estado: f.dir_estado.clone(),
                boveda: f.dir_datos.join("boveda"),
            },
            web: Web { sondeo_ms: 3000, diario_limite: 300 },
            biblioteca: AjustesBiblioteca::default(),
        }
    }

    /// El agente que ocupa un rol y su configuración.
    pub fn agente_de(&self, rol: Rol) -> (&str, &ConfigAgente) {
        let id = match rol {
            Rol::Planificador => &self.roles.planificador,
            Rol::Ejecutor => &self.roles.ejecutor,
        };
        (id, &self.agentes[id.as_str()])
    }

    /// El rol que ocupa ahora un agente del catálogo, si ocupa alguno.
    pub fn rol_de(&self, agente: &str) -> Option<Rol> {
        Rol::TODOS.into_iter().find(|r| self.agente_de(*r).0 == agente)
    }

    /// El perfil de modelo de un agente (`None`: el suyo propio).
    pub fn modelo_de(&self, agente: &str) -> Option<&PerfilModelo> {
        self.agentes.get(agente).and_then(|a| self.modelos.get(&a.modelo))
    }

    /// El perfil del ejecutor. Tras cargar siempre existe y es local.
    pub fn modelo_ejecutor(&self) -> &PerfilModelo {
        let (id, _) = self.agente_de(Rol::Ejecutor);
        self.modelo_de(id).or_else(|| self.modelos.get("local")).expect("el perfil local siempre existe")
    }

    /// Antigüedad máxima del último latido para dar por vivo al orquestador:
    /// el doble del intervalo (mismo criterio en `ea status`, ea-web y ea-mcp).
    pub fn umbral_latido_s(&self) -> i64 {
        (self.orquestador.intervalo_heartbeat_s * 2) as i64
    }
}
