//! El escritor único y el lector acotado de la bóveda.
//!
//! **Por qué un candado de fichero y no un servicio.** Escriben en la bóveda
//! procesos distintos: `ea-mcp` (las notas de los agentes) y las interfaces
//! del usuario (`ea`, `ea-web`) al promover. Un servicio dueño de la cola
//! obligaría a la CLI a depender de que ese servicio esté levantado, y haría
//! pasar la promoción del usuario por el proceso que sirve a los agentes. En
//! su lugar, este módulo es el único código que escribe ficheros de la
//! bóveda y toda operación de escritura toma antes un `flock` exclusivo sobre
//! `<raíz>/.ea-escritor.lock`: dos escritores, sean hilos o procesos, nunca
//! se solapan, y el evento del diario se emite dentro del candado, así que el
//! orden de los eventos es el orden real de las escrituras. El candado
//! garantiza exclusión mutua, no un orden FIFO estricto entre esperantes.

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

use ea_plataforma::fs::Permisos;
use ea_store::{EntradaIndice, EventOutcome, NewEvent, ResultadoBusqueda, Store};
use serde_json::{json, Value};

use crate::cabecera::{self, ahora, Cabecera, Cabeceras};
use crate::error::{ErrorBoveda, Resultado};
use crate::limites::{Limites, MAX_FICHEROS, MAX_FICHERO_BYTES};
use crate::ruta::{self, region_de, region_de_agente, validar_ruta, REGIONES, REGION_COMPARTIDA};

const FICHERO_CANDADO: &str = ".ea-escritor.lock";
const ESPERA_POR_DEFECTO: Duration = Duration::from_secs(10);

/// Huella FNV-1a (la de `ea-plataforma`) del contenido, para comprobar que la
/// nota no cambió entre la simulación de una promoción y su ejecución.
pub fn huella(texto: &str) -> String {
    ea_plataforma::fs::huella(texto.as_bytes())
}

/// Convierte el texto de búsqueda de una persona o un modelo en una consulta
/// FTS5 segura: cada término entre comillas (sin sintaxis de operadores, que
/// un texto libre podría romper) y unidos por AND implícito.
pub fn consulta_fts(texto: &str) -> Resultado<String> {
    let terminos: Vec<String> = texto
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .take(12)
        .map(|t| format!("\"{}\"", t.chars().take(64).collect::<String>()))
        .collect();
    if terminos.is_empty() {
        return Err(ErrorBoveda::Argumentos("la consulta no contiene ningún término buscable".into()));
    }
    Ok(terminos.join(" "))
}

pub struct NuevaNota<'a> {
    pub titulo: &'a str,
    pub cuerpo: &'a str,
    /// Ruta relativa a la región del autor (`ideas/rust.md`); sin ella se
    /// deriva del título.
    pub ruta: Option<&'a str>,
    pub tags: &'a [String],
    /// `trabajo:<id>`, `rutina:<slug>` o `manual`.
    pub origen: &'a str,
    /// Permite reemplazar una nota existente de la propia región.
    pub sobrescribir: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotaEscrita {
    pub ruta: String,
    pub region: String,
    pub bytes: usize,
    /// `true` si la nota no existía.
    pub creada: bool,
    pub huella: String,
}

#[derive(Debug, Clone)]
pub struct NotaLeida {
    pub ruta: String,
    pub region: String,
    pub cabecera: Option<Cabecera>,
    /// Por qué no hay cabecera válida (nota sin cabecera o con otra).
    pub aviso_cabecera: Option<String>,
    pub contenido: String,
    pub desde: usize,
    pub total_caracteres: usize,
    pub truncado: bool,
    pub siguiente_desde: Option<usize>,
}

/// Lo que haría una promoción, y la huella del contenido que se vio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanPromocion {
    pub origen: String,
    pub destino: String,
    pub titulo: String,
    pub autor: String,
    pub bytes: usize,
    pub huella: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResumenSincronizacion {
    pub indexadas: usize,
    pub actualizadas: usize,
    pub eliminadas: usize,
    /// Ficheros ignorados por exceder el tamaño máximo.
    pub omitidas: usize,
}

pub(crate) struct Candado {
    _fichero: File,
}

#[derive(Debug, Clone)]
pub struct Boveda {
    raiz: PathBuf,
    espera: Duration,
}

pub(crate) fn existe(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok()
}

pub(crate) fn es_enlace(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink())
}

pub(crate) fn mtime_ns(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}

/// Escritura atómica (la de `ea-plataforma`): se escribe un temporal oculto
/// en el mismo directorio, se sincroniza y se renombra sobre el destino. Un
/// lector ve la nota anterior o la nueva entera, nunca una mitad. No crea el
/// directorio.
pub(crate) fn escribir_atomico(destino: &Path, contenido: &str) -> Resultado<()> {
    Ok(ea_plataforma::fs::escribir_atomico(destino, contenido.as_bytes(), Permisos::Heredados)?)
}

pub(crate) fn evento(store: &Store, actor: &str, accion: &str, trabajo: Option<&str>, ok: bool, detalle: Value, inicio: Instant) -> Result<(), String> {
    store
        .append_event(NewEvent {
            actor: actor.to_string(),
            action: accion.to_string(),
            trabajo: trabajo.map(str::to_string),
            outcome: Some(if ok { EventOutcome::Ok } else { EventOutcome::Error }),
            duration_ms: Some(inicio.elapsed().as_millis() as i64),
            detail: Some(detalle.to_string()),
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub(crate) fn entrada_de(ruta: &str, texto: &str, meta: &fs::Metadata) -> (EntradaIndice, String) {
    let (cab, cuerpo) = cabecera::interpretar(texto);
    let region = region_de(ruta).to_string();
    let titulo_ajeno = || {
        cabecera::primer_encabezado(cuerpo).unwrap_or_else(|| {
            Path::new(ruta).file_stem().and_then(|s| s.to_str()).unwrap_or(ruta).to_string()
        })
    };
    let entrada = match cab {
        Cabeceras::Valida(c) => EntradaIndice {
            ruta: ruta.to_string(),
            region,
            autor: Some(c.autor),
            titulo: c.titulo,
            origen: Some(c.origen),
            promocion: Some(c.promocion),
            creada: Some(c.creada),
            actualizada: Some(c.actualizada),
            tags: c.tags.join(" "),
            mtime_ns: mtime_ns(meta),
            bytes: meta.len() as i64,
        },
        _ => EntradaIndice {
            ruta: ruta.to_string(),
            region,
            autor: None,
            titulo: titulo_ajeno(),
            origen: None,
            promocion: None,
            creada: None,
            actualizada: None,
            tags: String::new(),
            mtime_ns: mtime_ns(meta),
            bytes: meta.len() as i64,
        },
    };
    (entrada, cuerpo.to_string())
}

/// Solo el usuario (sus interfaces, que firman como
/// [`ea_ajustes::ACTOR_USUARIO`]) hace ciertas operaciones: promover y editar
/// la bóveda a mano. Es una lista blanca: un agente, el orquestador, un actor
/// que no se conoce o nadie, no.
pub(crate) fn exigir_usuario(quien: &str, que: &str) -> Resultado<()> {
    if quien != ea_ajustes::ACTOR_USUARIO {
        return Err(ErrorBoveda::Prohibida(format!("'{quien}' no puede {que}")));
    }
    Ok(())
}

impl Boveda {
    /// Sobre una raíz explícita (el directorio no tiene por qué existir aún).
    pub fn en(raiz: impl Into<PathBuf>) -> Self {
        Self { raiz: raiz.into(), espera: ESPERA_POR_DEFECTO }
    }

    /// Sobre la raíz configurada: `rutas.boveda` de los ajustes, que resuelve,
    /// por este orden, `EA_BOVEDA_DIR`, `ajustes.toml`, el heredado
    /// `boveda.toml` (`raiz = "..."`) y un directorio propio del ecosistema. El
    /// valor por defecto NO es la bóveda personal de Obsidian del usuario:
    /// apuntarla allí es una decisión suya, explícita (ver README).
    pub fn configurada() -> Self {
        Self::en(ea_ajustes::cargar().rutas.boveda)
    }

    /// Cuánto espera el escritor al candado antes de rendirse.
    pub fn con_espera(mut self, espera: Duration) -> Self {
        self.espera = espera;
        self
    }

    pub fn raiz(&self) -> &Path {
        &self.raiz
    }

    // --- rutas -------------------------------------------------------------

    /// Ruta absoluta de una nota ya validada léxicamente, comprobando contra
    /// el disco que, resueltos los enlaces simbólicos, sigue dentro de la
    /// carpeta de su región (un enlace hacia fuera de la bóveda, o hacia otra
    /// región, se rechaza).
    pub(crate) fn resolver(&self, ruta: &str) -> Resultado<PathBuf> {
        let partes = validar_ruta(ruta)?;
        self.resolver_partes(ruta, &partes)
    }

    /// Como [`Self::resolver`], para una carpeta (`region` o `region/…`).
    pub(crate) fn resolver_carpeta(&self, ruta: &str) -> Resultado<PathBuf> {
        let partes = ruta::validar_carpeta(ruta)?;
        self.resolver_partes(ruta, &partes)
    }

    fn resolver_partes(&self, ruta: &str, partes: &[&str]) -> Resultado<PathBuf> {
        let raiz = fs::canonicalize(&self.raiz)
            .map_err(|e| ErrorBoveda::Io(format!("raíz de la bóveda {}: {e}", self.raiz.display())))?;
        let region = raiz.join(partes[0]);
        let completa = partes.iter().fold(raiz.clone(), |p, c| p.join(c));

        let mut ancestro: &Path = &completa;
        while !existe(ancestro) {
            ancestro = ancestro.parent().ok_or_else(|| ErrorBoveda::Ruta(ruta.to_string()))?;
        }
        let canonico = fs::canonicalize(ancestro)
            .map_err(|_| ErrorBoveda::FueraDeLaBoveda(format!("{ruta} (enlace simbólico roto)")))?;
        if !(canonico.starts_with(&region) || ancestro == raiz) {
            return Err(ErrorBoveda::FueraDeLaBoveda(ruta.to_string()));
        }
        if es_enlace(&completa) {
            return Err(ErrorBoveda::FueraDeLaBoveda(format!("{ruta} (es un enlace simbólico)")));
        }
        Ok(completa)
    }

    pub(crate) fn candado(&self) -> Resultado<Candado> {
        fs::create_dir_all(&self.raiz)?;
        let f = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.raiz.join(FICHERO_CANDADO))?;
        let limite = Instant::now() + self.espera;
        loop {
            match f.try_lock() {
                Ok(()) => return Ok(Candado { _fichero: f }),
                Err(std::fs::TryLockError::WouldBlock) => {
                    if Instant::now() >= limite {
                        return Err(ErrorBoveda::Ocupada);
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(std::fs::TryLockError::Error(e)) => return Err(e.into()),
            }
        }
    }

    pub(crate) fn leer_texto(&self, ruta: &str) -> Resultado<String> {
        let p = self.resolver(ruta)?;
        let meta = fs::metadata(&p).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => ErrorBoveda::NoExiste(ruta.to_string()),
            _ => e.into(),
        })?;
        if !meta.is_file() {
            return Err(ErrorBoveda::NoExiste(format!("{ruta} (no es una nota)")));
        }
        if meta.len() > MAX_FICHERO_BYTES {
            return Err(ErrorBoveda::Argumentos(format!(
                "{ruta} pesa {} bytes; el máximo legible es {MAX_FICHERO_BYTES}",
                meta.len()
            )));
        }
        String::from_utf8(fs::read(&p)?).map_err(|_| ErrorBoveda::Argumentos(format!("{ruta} no es texto UTF-8")))
    }

    // --- escritura ---------------------------------------------------------

    /// Validaciones que no necesitan el disco: autor, cabecera, tamaños y
    /// forma de la ruta. Devuelve la región del autor y la ruta pedida, ya
    /// con su región delante.
    fn validar_nota(autor: &str, n: &NuevaNota<'_>, lim: &Limites) -> Resultado<(&'static str, Option<String>)> {
        let region = region_de_agente(autor).ok_or_else(|| {
            ErrorBoveda::Prohibida(format!("'{autor}' no escribe en la bóveda (solo los agentes, en su región)"))
        })?;
        cabecera::titulo_valido(n.titulo)?;
        cabecera::tags_validos(n.tags)?;
        if !cabecera::origen_valido(n.origen) {
            return Err(ErrorBoveda::Cabecera(format!(
                "origen '{}' no válido (trabajo:<id>, rutina:<slug> o manual)",
                n.origen
            )));
        }
        if n.cuerpo.trim().is_empty() {
            return Err(ErrorBoveda::Argumentos("el contenido de la nota está vacío".into()));
        }
        if n.cuerpo.len() > lim.max_escritura {
            return Err(ErrorBoveda::Argumentos(format!(
                "el contenido pesa {} bytes y el máximo es {}",
                n.cuerpo.len(),
                lim.max_escritura
            )));
        }
        if n.cuerpo.contains('\0') {
            return Err(ErrorBoveda::Argumentos("el contenido no admite caracteres nulos".into()));
        }
        let pedida = match n.ruta {
            Some(r) => Some(ruta::en_region(region, r)?),
            None => None,
        };
        Ok((region, pedida))
    }

    /// Simulación de una escritura: valida todo y dice dónde caería la nota
    /// y si ya existe, sin escribir ni tomar el candado.
    pub fn simular_escritura(&self, autor: &str, n: &NuevaNota<'_>, lim: &Limites) -> Resultado<(String, bool)> {
        let (region, pedida) = Self::validar_nota(autor, n, lim)?;
        let ruta = pedida.unwrap_or_else(|| format!("{region}/{}.md", ruta::slug(n.titulo)));
        let existe = fs::canonicalize(&self.raiz).is_ok() && existe(&self.resolver(&ruta)?);
        if existe && n.ruta.is_some() && !n.sobrescribir {
            return Err(ErrorBoveda::YaExiste(format!("{ruta} (usa sobrescribir para reemplazarla)")));
        }
        Ok((ruta, existe))
    }

    /// Escribe una nota en la región del propio autor, con libertad total
    /// (decisión 8) pero nunca fuera de ella: no hay forma de nombrar otra
    /// región. Valida la cabecera, escribe de forma atómica bajo el candado,
    /// actualiza el índice y deja el evento `boveda_escritura`.
    pub fn escribir_nota(
        &self,
        store: &mut Store,
        autor: &str,
        n: &NuevaNota<'_>,
        lim: &Limites,
    ) -> Resultado<NotaEscrita> {
        let inicio = Instant::now();
        let (region, pedida) = Self::validar_nota(autor, n, lim)?;

        let _candado = self.candado()?;
        let (ruta_final, destino, previo) = match pedida {
            Some(r) => {
                let d = self.resolver(&r)?;
                let previo = if existe(&d) { Some(self.leer_texto(&r)?) } else { None };
                if previo.is_some() && !n.sobrescribir {
                    return Err(ErrorBoveda::YaExiste(format!("{r} (usa sobrescribir para reemplazarla)")));
                }
                (r, d, previo)
            }
            None => {
                let base = ruta::slug(n.titulo);
                let mut k = 1;
                loop {
                    let r = if k == 1 { format!("{region}/{base}.md") } else { format!("{region}/{base}-{k}.md") };
                    let d = self.resolver(&r)?;
                    if !existe(&d) {
                        break (r, d, None);
                    }
                    k += 1;
                    if k > 1000 {
                        return Err(ErrorBoveda::YaExiste(format!("demasiadas notas llamadas '{base}'")));
                    }
                }
            }
        };

        let ahora = ahora();
        let mut cab = Cabecera {
            titulo: n.titulo.to_string(),
            autor: autor.to_string(),
            region: region.to_string(),
            creada: ahora.clone(),
            actualizada: ahora,
            origen: n.origen.to_string(),
            promocion: "ninguna".into(),
            tags: n.tags.to_vec(),
            promovida_de: None,
            promovida_a: None,
            promovida_por: None,
            promovida_en: None,
            extras: Vec::new(),
        };
        // Al reemplazar se conserva lo que no es del contenido: cuándo nació
        // y si ya fue promovida.
        if let Some(Cabeceras::Valida(p)) = previo.as_deref().map(|t| cabecera::interpretar(t).0) {
            cab.creada = p.creada;
            cab.promocion = p.promocion;
            cab.promovida_a = p.promovida_a;
            cab.promovida_por = p.promovida_por;
            cab.promovida_en = p.promovida_en;
            cab.extras = p.extras;
        }
        cab.validar()?;
        let texto = cab.con_cuerpo(n.cuerpo);

        if let Some(dir) = destino.parent() {
            fs::create_dir_all(dir)?;
        }
        // Los directorios recién creados no pueden haberse desviado por un
        // enlace creado entre medias: se vuelve a comprobar antes de escribir.
        self.resolver(&ruta_final)?;
        escribir_atomico(&destino, &texto)?;
        let meta = fs::metadata(&destino)?;
        let (entrada, cuerpo_indexado) = entrada_de(&ruta_final, &texto, &meta);
        let indice = store.boveda_indexar(&entrada, &cuerpo_indexado);

        let creada = previo.is_none();
        let escrita = NotaEscrita {
            ruta: ruta_final.clone(),
            region: region.to_string(),
            bytes: texto.len(),
            creada,
            huella: huella(&texto),
        };
        let trabajo = n.origen.strip_prefix("trabajo:");
        evento(
            store,
            autor,
            "boveda_escritura",
            trabajo,
            true,
            json!({
                "ruta": ruta_final, "region": region, "bytes": texto.len(),
                "modo": if creada { "creada" } else { "reemplazada" },
                "titulo": n.titulo, "origen": n.origen, "huella": escrita.huella,
                "indice": indice.is_ok(),
            }),
            inicio,
        )
        .map_err(|e| ErrorBoveda::EventoPerdido(ruta_final.clone(), e))?;
        Ok(escrita)
    }

    // --- promoción ---------------------------------------------------------

    /// Qué haría promover una nota, sin hacerlo (para simular y proponer).
    /// Solo mira: no toma el candado ni escribe.
    pub fn planear_promocion(&self, origen: &str, destino: Option<&str>) -> Resultado<PlanPromocion> {
        let partes = validar_ruta(origen)?;
        if partes[0] == REGION_COMPARTIDA {
            return Err(ErrorBoveda::Argumentos(format!("{origen} ya está en la región compartida")));
        }
        let destino = match destino {
            Some(d) => ruta::en_region(REGION_COMPARTIDA, d)?,
            None => format!("{REGION_COMPARTIDA}/{}", partes[1..].join("/")),
        };
        validar_ruta(&destino)?;
        let texto = self.leer_texto(origen)?;
        let d = self.resolver(&destino)?;
        if existe(&d) {
            return Err(ErrorBoveda::YaExiste(format!("{destino} (la región compartida no se sobrescribe)")));
        }
        let (cab, cuerpo) = cabecera::interpretar(&texto);
        let (titulo, autor) = match cab {
            Cabeceras::Valida(c) => (c.titulo, c.autor),
            _ => (
                cabecera::primer_encabezado(cuerpo).unwrap_or_else(|| ruta::slug(origen)),
                partes[0].to_string(),
            ),
        };
        Ok(PlanPromocion {
            origen: origen.to_string(),
            destino,
            titulo,
            autor,
            bytes: texto.len(),
            huella: huella(&texto),
        })
    }

    /// Promueve una nota a la región compartida **copiándola**: la nota
    /// original se conserva (solo su cabecera pasa a `promocion: promovida`,
    /// con `promovida_a`). Es la única vía por la que algo llega a la región
    /// compartida. Quien llama es la interfaz del usuario tras su aceptación;
    /// por si acaso, un agente (o el orquestador) como `quien` se rechaza.
    /// `huella_esperada` es la del contenido que el usuario vio al aceptar.
    pub fn promover(
        &self,
        store: &mut Store,
        origen: &str,
        destino: Option<&str>,
        huella_esperada: &str,
        quien: &str,
        canal: &str,
    ) -> Resultado<PlanPromocion> {
        let inicio = Instant::now();
        exigir_usuario(quien, "promover: la promoción la decide el usuario (decisión 9)")?;
        let _candado = self.candado()?;
        let plan = self.planear_promocion(origen, destino)?;
        if plan.huella != huella_esperada {
            return Err(ErrorBoveda::Cambio(format!(
                "{origen} es distinta de la que se simuló (huella {} y ahora {})",
                huella_esperada, plan.huella
            )));
        }
        let texto = self.leer_texto(origen)?;
        let (cab, cuerpo) = cabecera::interpretar(&texto);
        let ahora = ahora();
        let (copia, original_marcado) = match cab {
            Cabeceras::Valida(c) => {
                let mut copia = c.clone();
                copia.region = REGION_COMPARTIDA.into();
                copia.promocion = "promovida".into();
                copia.promovida_de = Some(origen.to_string());
                copia.promovida_a = None;
                copia.promovida_por = Some(quien.to_string());
                copia.promovida_en = Some(ahora.clone());
                copia.actualizada = ahora.clone();
                let mut orig = c;
                orig.promocion = "promovida".into();
                orig.promovida_a = Some(plan.destino.clone());
                orig.promovida_por = Some(quien.to_string());
                orig.promovida_en = Some(ahora.clone());
                (copia, Some(orig))
            }
            _ => (
                Cabecera {
                    titulo: plan.titulo.clone(),
                    autor: plan.autor.clone(),
                    region: REGION_COMPARTIDA.into(),
                    creada: ahora.clone(),
                    actualizada: ahora.clone(),
                    origen: "manual".into(),
                    promocion: "promovida".into(),
                    tags: Vec::new(),
                    promovida_de: Some(origen.to_string()),
                    promovida_a: None,
                    promovida_por: Some(quien.to_string()),
                    promovida_en: Some(ahora.clone()),
                    extras: Vec::new(),
                },
                // Una nota sin cabecera del contrato se deja intacta.
                None,
            ),
        };
        copia.validar()?;
        let texto_copia = copia.con_cuerpo(cuerpo);
        let ruta_destino = self.resolver(&plan.destino)?;
        if let Some(dir) = ruta_destino.parent() {
            fs::create_dir_all(dir)?;
        }
        self.resolver(&plan.destino)?;
        escribir_atomico(&ruta_destino, &texto_copia)?;
        let meta = fs::metadata(&ruta_destino)?;
        let (entrada, cuerpo_ind) = entrada_de(&plan.destino, &texto_copia, &meta);
        let indice_copia = store.boveda_indexar(&entrada, &cuerpo_ind);

        let mut original_ok = false;
        if let Some(orig) = original_marcado {
            let ruta_orig = self.resolver(origen)?;
            let texto_orig = orig.con_cuerpo(cuerpo);
            if escribir_atomico(&ruta_orig, &texto_orig).is_ok() {
                original_ok = true;
                if let Ok(meta) = fs::metadata(&ruta_orig) {
                    let (e, c) = entrada_de(origen, &texto_orig, &meta);
                    let _ = store.boveda_indexar(&e, &c);
                }
            }
        }
        evento(
            store,
            quien,
            "boveda_promocion",
            None,
            true,
            json!({
                "origen": origen, "destino": plan.destino, "autor": plan.autor,
                "titulo": plan.titulo, "bytes": texto_copia.len(), "huella": plan.huella,
                "canal": canal, "original_marcado": original_ok, "indice": indice_copia.is_ok(),
            }),
            inicio,
        )
        .map_err(|e| ErrorBoveda::EventoPerdido(plan.destino.clone(), e))?;
        Ok(plan)
    }

    // --- lectura acotada ---------------------------------------------------

    /// Lee una nota por trozos de como mucho `lim.max_lectura` caracteres de
    /// cuerpo, desde el carácter `desde`.
    pub fn leer(&self, ruta: &str, desde: usize, lim: &Limites) -> Resultado<NotaLeida> {
        let texto = self.leer_texto(ruta)?;
        let (cab, cuerpo) = cabecera::interpretar(&texto);
        let total = cuerpo.chars().count();
        let contenido: String = cuerpo.chars().skip(desde).take(lim.max_lectura).collect();
        let fin = desde + contenido.chars().count();
        let (cabecera, aviso) = match cab {
            Cabeceras::Valida(c) => (Some(c), None),
            Cabeceras::Invalida(e) => (None, Some(e)),
            Cabeceras::Ausente => (None, Some("la nota no tiene cabecera".to_string())),
        };
        Ok(NotaLeida {
            ruta: ruta.to_string(),
            region: region_de(ruta).to_string(),
            cabecera,
            aviso_cabecera: aviso,
            contenido,
            desde,
            total_caracteres: total,
            truncado: fin < total,
            siguiente_desde: (fin < total).then_some(fin),
        })
    }

    // --- índice y búsqueda -------------------------------------------------

    /// Pone al día el índice con lo que hay en las regiones: notas nuevas o
    /// editadas por fuera (en Obsidian, a mano) y notas borradas. No escribe
    /// en la bóveda, solo en la caché de la base de datos. Ignora ficheros y
    /// carpetas ocultos, enlaces simbólicos y lo que no sea `.md`.
    pub fn sincronizar(&self, store: &mut Store, forzar: bool) -> Resultado<ResumenSincronizacion> {
        self.recorrer(store, forzar, true)
    }

    /// Lo que `sincronizar` cambiaría ahora mismo, sin tocar el índice: notas
    /// nuevas (`indexadas`), cambiadas por fecha o tamaño (`actualizadas`) y
    /// borradas del disco (`eliminadas`). Todo a cero = el índice coincide.
    pub fn diferencia_indice(&self, store: &mut Store) -> Resultado<ResumenSincronizacion> {
        self.recorrer(store, false, false)
    }

    /// Reconstruye el índice entero desde los ficheros (`ea boveda reindexar`)
    /// y deja el evento `boveda_reindexado` en el diario. No escribe en la
    /// bóveda: el índice es una caché.
    pub fn reindexar(&self, store: &mut Store, quien: &str) -> Resultado<ResumenSincronizacion> {
        let inicio = Instant::now();
        let r = self.recorrer(store, true, true)?;
        let _ = evento(
            store,
            quien,
            "boveda_reindexado",
            None,
            true,
            serde_json::json!({
                "indexadas": r.indexadas, "actualizadas": r.actualizadas,
                "eliminadas": r.eliminadas, "omitidas": r.omitidas,
            }),
            inicio,
        );
        Ok(r)
    }

    fn recorrer(&self, store: &mut Store, forzar: bool, aplicar: bool) -> Resultado<ResumenSincronizacion> {
        let mut resumen = ResumenSincronizacion::default();
        let Ok(raiz) = fs::canonicalize(&self.raiz) else {
            return Ok(resumen);
        };
        let conocidas: std::collections::HashMap<String, (i64, i64)> = store
            .boveda_rutas_indexadas()?
            .into_iter()
            .map(|r| (r.ruta, (r.mtime_ns, r.bytes)))
            .collect();
        let mut vistas = std::collections::HashSet::new();
        let mut pendientes: Vec<(PathBuf, String)> = Vec::new();
        for region in REGIONES {
            let dir = raiz.join(region);
            if dir.is_dir() && !es_enlace(&dir) {
                pendientes.push((dir, region.to_string()));
            }
        }
        let mut recorridos = 0;
        while let Some((dir, prefijo)) = pendientes.pop() {
            let Ok(entradas) = fs::read_dir(&dir) else { continue };
            for e in entradas.flatten() {
                let nombre = e.file_name().to_string_lossy().into_owned();
                if nombre.starts_with('.') {
                    continue;
                }
                let Ok(tipo) = e.file_type() else { continue };
                if tipo.is_symlink() {
                    continue;
                }
                let ruta_rel = format!("{prefijo}/{nombre}");
                if tipo.is_dir() {
                    pendientes.push((e.path(), ruta_rel));
                } else if tipo.is_file() && nombre.ends_with(".md") {
                    recorridos += 1;
                    if recorridos > MAX_FICHEROS {
                        return Ok(resumen);
                    }
                    let Ok(meta) = e.metadata() else { continue };
                    vistas.insert(ruta_rel.clone());
                    if meta.len() > MAX_FICHERO_BYTES {
                        resumen.omitidas += 1;
                        continue;
                    }
                    let igual = conocidas.get(&ruta_rel) == Some(&(mtime_ns(&meta), meta.len() as i64));
                    if igual && !forzar {
                        continue;
                    }
                    if !aplicar {
                        if conocidas.contains_key(&ruta_rel) {
                            resumen.actualizadas += 1;
                        } else {
                            resumen.indexadas += 1;
                        }
                        continue;
                    }
                    let Ok(bytes) = fs::read(e.path()) else { continue };
                    let Ok(texto) = String::from_utf8(bytes) else { continue };
                    let (entrada, cuerpo) = entrada_de(&ruta_rel, &texto, &meta);
                    store.boveda_indexar(&entrada, &cuerpo)?;
                    if conocidas.contains_key(&ruta_rel) {
                        resumen.actualizadas += 1;
                    } else {
                        resumen.indexadas += 1;
                    }
                }
            }
        }
        for ruta_vieja in conocidas.keys().filter(|r| !vistas.contains(*r)) {
            if aplicar {
                store.boveda_olvidar(ruta_vieja)?;
            }
            resumen.eliminadas += 1;
        }
        Ok(resumen)
    }

    fn comprobar_region(region: Option<&str>) -> Resultado<()> {
        match region {
            Some(r) if !ruta::es_region(r) => Err(ErrorBoveda::Argumentos(format!(
                "región '{r}' desconocida ({})",
                REGIONES.join(", ")
            ))),
            _ => Ok(()),
        }
    }

    /// Búsqueda de texto completo (FTS5) con resultados acotados: como mucho
    /// `lim.max_resultados` (o `pedido`, si es menor) y fragmentos de como
    /// mucho `lim.max_fragmento` caracteres.
    pub fn buscar(
        &self,
        store: &mut Store,
        consulta: &str,
        region: Option<&str>,
        pedido: Option<usize>,
        lim: &Limites,
    ) -> Resultado<Vec<ResultadoBusqueda>> {
        Self::comprobar_region(region)?;
        let fts = consulta_fts(consulta)?;
        self.sincronizar(store, false)?;
        let n = pedido.unwrap_or(lim.max_resultados).clamp(1, lim.max_resultados);
        let mut r = store.boveda_buscar(&fts, region, n as i64, (lim.max_fragmento / 6).clamp(8, 64) as i64)?;
        for x in &mut r {
            if x.fragmento.chars().count() > lim.max_fragmento {
                x.fragmento = x.fragmento.chars().take(lim.max_fragmento).collect::<String>() + "…";
            }
        }
        Ok(r)
    }

    /// Notas más recientes, sin consulta.
    pub fn listar(
        &self,
        store: &mut Store,
        region: Option<&str>,
        pedido: Option<usize>,
        lim: &Limites,
    ) -> Resultado<Vec<ResultadoBusqueda>> {
        Self::comprobar_region(region)?;
        self.sincronizar(store, false)?;
        let n = pedido.unwrap_or(lim.max_resultados).clamp(1, lim.max_resultados);
        Ok(store.boveda_recientes(region, n as i64)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Solo `usuario`, el actor con que firman la CLI y la web, hace lo que
    /// es del usuario. Ningún otro actor del sistema pasa: los de los roles,
    /// el orquestador, los del MCP sin rol o desconocido, el bus de `ea
    /// trabajo` (`ea-cli`) ni la cadena vacía.
    #[test]
    fn exigir_usuario_es_una_lista_blanca() {
        assert!(exigir_usuario("usuario", "x").is_ok());
        for quien in
            ["modelo-local", "claude-code", "orquestador", "sin-rol", "desconocido", "ea-cli", "", " ", "Usuario", " usuario", "planificador", "ejecutor"]
        {
            assert!(matches!(exigir_usuario(quien, "x"), Err(ErrorBoveda::Prohibida(_))), "{quien:?}");
        }
    }
}
