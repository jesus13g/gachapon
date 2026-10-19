//! Lo que el usuario hace a mano con la bóveda desde sus interfaces (el
//! editor de `ea-web`): leer una nota entera, editarla, crear notas y
//! carpetas, renombrar o mover (con los enlaces al día) y mandar a la
//! papelera y restaurar. Nada de esto está en `ea-mcp` ni en `ea-acciones`:
//! ningún agente llega aquí, y por si acaso `quien` se comprueba en cada
//! operación.
//!
//! Reglas:
//! - **La firma no se toca.** El usuario puede editar notas de las tres
//!   regiones, pero `autor`, `region`, `creada`, `promocion` y `promovida_*`
//!   de una cabecera válida no cambian; y a una nota sin cabecera no se le
//!   puede poner una firmada por un agente.
//! - **Crear notas, solo en `compartida/`** (con `autor: usuario`). Carpetas,
//!   en cualquier región.
//! - **Renombrar o mover, dentro de la misma región.** A otra región se llega
//!   promoviendo.
//! - **Borrar es reversible**: la nota va a `<raíz>/.papelera/<id>/<ruta>` y
//!   se puede restaurar.
//!
//! Como el resto del escritor: todo bajo el candado, con escritura atómica,
//! el índice al día y un evento en el diario dentro del candado.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use ea_store::{ResultadoBusqueda, Store};
use serde_json::json;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::boveda::{entrada_de, es_enlace, escribir_atomico, evento, exigir_usuario, existe, huella};
use crate::cabecera::{self, ahora, Cabecera, Cabeceras};
use crate::enlaces;
use crate::error::{ErrorBoveda, Resultado};
use crate::limites::MAX_FICHERO_BYTES;
use crate::ruta::{self, region_de, validar_ruta, PAPELERA, REGIONES, REGION_COMPARTIDA};
use crate::Boveda;

/// Una nota entera, cabecera incluida, con la versión (huella) que hay que
/// devolver al guardarla.
#[derive(Debug, Clone)]
pub struct NotaCompleta {
    pub ruta: String,
    pub region: String,
    /// El fichero tal cual, con la cabecera.
    pub contenido: String,
    /// Huella del contenido: lo que hay que devolver al guardar.
    pub version: String,
    pub cabecera: Option<Cabecera>,
    pub aviso_cabecera: Option<String>,
    pub bytes: u64,
    /// Última modificación del fichero (RFC 3339).
    pub modificada: Option<String>,
}

/// Las notas indexadas y las carpetas que hay en disco (también vacías).
#[derive(Debug, Clone)]
pub struct Arbol {
    pub notas: Vec<ResultadoBusqueda>,
    /// Carpetas relativas a la raíz, regiones incluidas, ordenadas.
    pub carpetas: Vec<String>,
    /// Cuántas notas hay en la papelera.
    pub en_papelera: usize,
}

#[derive(Debug, Clone)]
pub struct Renombrada {
    pub nota: NotaCompleta,
    /// Otras notas cuyos enlaces se reescribieron.
    pub actualizadas: Vec<String>,
    /// Notas con `[[enlaces]]` ambiguos al nombre viejo, que se dejaron igual.
    pub ambiguas: Vec<String>,
}

/// Una nota en la papelera.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnPapelera {
    /// Con qué se restaura.
    pub id: String,
    /// Dónde estaba (y adónde vuelve).
    pub ruta: String,
    /// Cuándo se borró (RFC 3339).
    pub borrada: String,
    pub bytes: u64,
}

fn rfc3339(meta: &fs::Metadata) -> Option<String> {
    meta.modified().ok().and_then(|t| OffsetDateTime::from(t).format(&Rfc3339).ok())
}

/// Contenido que se acepta al escribir a mano: tamaño y sin nulos.
fn contenido_valido(texto: &str) -> Resultado<()> {
    if texto.len() as u64 > MAX_FICHERO_BYTES {
        return Err(ErrorBoveda::Argumentos(format!(
            "la nota pesa {} bytes y el máximo es {MAX_FICHERO_BYTES}",
            texto.len()
        )));
    }
    if texto.contains('\0') {
        return Err(ErrorBoveda::Argumentos("el contenido no admite caracteres nulos".into()));
    }
    Ok(())
}

/// Añade `.md` si falta y valida.
fn ruta_de_nota(ruta: &str) -> Resultado<String> {
    let mut r = ruta.trim().to_string();
    if !r.ends_with(".md") {
        r.push_str(".md");
    }
    validar_ruta(&r)?;
    Ok(r)
}

/// Lo que no puede cambiar a mano en una cabecera.
fn firma(c: &Cabecera) -> [Option<&str>; 8] {
    [
        Some(&c.autor),
        Some(&c.region),
        Some(&c.creada),
        Some(&c.promocion),
        c.promovida_de.as_deref(),
        c.promovida_a.as_deref(),
        c.promovida_por.as_deref(),
        c.promovida_en.as_deref(),
    ]
}

/// El texto que se escribe al guardar `nuevo` sobre `actual`, aplicando las
/// reglas de la firma.
fn texto_al_guardar(actual: &str, nuevo: &str) -> Resultado<String> {
    let (previa, _) = cabecera::interpretar(actual);
    let (pedida, cuerpo) = cabecera::interpretar(nuevo);
    match (previa, pedida) {
        (Cabeceras::Valida(p), Cabeceras::Valida(mut n)) => {
            if firma(&p) != firma(&n) {
                return Err(ErrorBoveda::Cabecera(
                    "la firma de la nota (autor, region, creada, promocion y promovida_*) no se cambia a mano".into(),
                ));
            }
            n.actualizada = ahora();
            n.validar()?;
            Ok(n.con_cuerpo(cuerpo))
        }
        (Cabeceras::Valida(_), Cabeceras::Invalida(e)) => {
            Err(ErrorBoveda::Cabecera(format!("la cabecera ya no es válida: {e}")))
        }
        (Cabeceras::Valida(_), Cabeceras::Ausente) => Err(ErrorBoveda::Cabecera(
            "la nota lleva cabecera y no se puede quitar (la firma no se cambia a mano)".into(),
        )),
        // Una nota sin cabecera válida solo puede ganar una firmada por el usuario.
        (_, Cabeceras::Valida(n)) if n.autor != "usuario" => Err(ErrorBoveda::Prohibida(format!(
            "no se puede firmar a mano una nota como '{}'",
            n.autor
        ))),
        _ => Ok(nuevo.to_string()),
    }
}

/// `20261019T120000Z`: la marca de tiempo de un id de la papelera.
fn marca_ahora() -> String {
    let d = OffsetDateTime::now_utc();
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        d.year(),
        u8::from(d.month()),
        d.day(),
        d.hour(),
        d.minute(),
        d.second()
    )
}

/// `20261019T120000Z-1` → `2026-10-19T12:00:00Z`.
fn borrada_de(id: &str) -> String {
    let m = id.as_bytes();
    if m.len() >= 16 && m[8] == b'T' && m[15] == b'Z' {
        let s = |a: usize, b: usize| &id[a..b];
        return format!("{}-{}-{}T{}:{}:{}Z", s(0, 4), s(4, 6), s(6, 8), s(9, 11), s(11, 13), s(13, 15));
    }
    String::new()
}

fn id_valido(id: &str) -> bool {
    !id.is_empty() && id.len() <= 40 && !id.starts_with('.') && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Los `.md` regulares bajo `dir` (sin seguir enlaces ni entrar en ocultos),
/// con su ruta relativa a `dir`.
fn notas_bajo(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let mut pendientes = vec![(dir.to_path_buf(), String::new())];
    while let Some((d, prefijo)) = pendientes.pop() {
        let Ok(entradas) = fs::read_dir(&d) else { continue };
        for e in entradas.flatten() {
            let nombre = e.file_name().to_string_lossy().into_owned();
            let Ok(tipo) = e.file_type() else { continue };
            if nombre.starts_with('.') || tipo.is_symlink() {
                continue;
            }
            let rel = if prefijo.is_empty() { nombre.clone() } else { format!("{prefijo}/{nombre}") };
            if tipo.is_dir() {
                pendientes.push((e.path(), rel));
            } else if tipo.is_file() && nombre.ends_with(".md") {
                out.push((rel, e.path()));
            }
        }
    }
    out.sort();
    out
}

/// Borra las carpetas vacías desde `desde` hacia arriba, hasta `tope` incluida.
fn podar(desde: &Path, tope: &Path) {
    let mut d = Some(desde);
    while let Some(dir) = d {
        if !dir.starts_with(tope) || fs::remove_dir(dir).is_err() {
            break;
        }
        if dir == tope {
            break;
        }
        d = dir.parent();
    }
}

impl Boveda {
    /// La nota entera, con su versión.
    pub fn leer_completa(&self, ruta: &str) -> Resultado<NotaCompleta> {
        let texto = self.leer_texto(ruta)?;
        let meta = fs::metadata(self.resolver(ruta)?)?;
        let (cab, _) = cabecera::interpretar(&texto);
        let (cabecera, aviso) = match cab {
            Cabeceras::Valida(c) => (Some(c), None),
            Cabeceras::Invalida(e) => (None, Some(e)),
            Cabeceras::Ausente => (None, Some("la nota no tiene cabecera".to_string())),
        };
        Ok(NotaCompleta {
            ruta: ruta.to_string(),
            region: region_de(ruta).to_string(),
            version: huella(&texto),
            contenido: texto,
            cabecera,
            aviso_cabecera: aviso,
            bytes: meta.len(),
            modificada: rfc3339(&meta),
        })
    }

    /// Ruta absoluta de una nota que existe (para mostrarla en el explorador).
    pub fn ruta_absoluta(&self, ruta: &str) -> Resultado<PathBuf> {
        let p = self.resolver(ruta)?;
        if !fs::metadata(&p).is_ok_and(|m| m.is_file()) {
            return Err(ErrorBoveda::NoExiste(ruta.to_string()));
        }
        Ok(p)
    }

    /// El árbol: las notas del índice (puesto al día antes) y las carpetas de
    /// las regiones, también las vacías. Ignora lo oculto y los enlaces.
    pub fn arbol(&self, store: &mut Store) -> Resultado<Arbol> {
        self.sincronizar(store, false)?;
        let notas = store.boveda_todas()?;
        let mut carpetas = Vec::new();
        if let Ok(raiz) = fs::canonicalize(self.raiz()) {
            let mut pendientes: Vec<(PathBuf, String)> = REGIONES
                .iter()
                .map(|r| (raiz.join(r), r.to_string()))
                .filter(|(d, _)| d.is_dir() && !es_enlace(d))
                .collect();
            while let Some((dir, rel)) = pendientes.pop() {
                carpetas.push(rel.clone());
                if carpetas.len() > crate::limites::MAX_FICHEROS {
                    break;
                }
                let Ok(entradas) = fs::read_dir(&dir) else { continue };
                for e in entradas.flatten() {
                    let nombre = e.file_name().to_string_lossy().into_owned();
                    if nombre.starts_with('.') || !e.file_type().is_ok_and(|t| t.is_dir()) {
                        continue;
                    }
                    pendientes.push((e.path(), format!("{rel}/{nombre}")));
                }
            }
        }
        carpetas.sort();
        Ok(Arbol { notas, carpetas, en_papelera: self.papelera()?.len() })
    }

    /// Guarda a mano una nota que ya existe. `version` es la huella con la que
    /// se abrió: si en disco ya es otra, [`ErrorBoveda::Cambio`]. La firma de
    /// la cabecera no cambia (ver el módulo); `actualizada` se pone al día.
    pub fn guardar_usuario(
        &self,
        store: &mut Store,
        ruta: &str,
        texto: &str,
        version: &str,
        quien: &str,
    ) -> Resultado<NotaCompleta> {
        let inicio = Instant::now();
        exigir_usuario(quien, "editar la bóveda a mano")?;
        validar_ruta(ruta)?;
        contenido_valido(texto)?;
        let _candado = self.candado()?;
        let actual = self.leer_texto(ruta)?;
        let antes = huella(&actual);
        if antes != version {
            return Err(ErrorBoveda::Cambio(format!("{ruta} cambió en disco desde que la abriste; recárgala")));
        }
        if actual == texto {
            return self.leer_completa(ruta);
        }
        let final_ = texto_al_guardar(&actual, texto)?;
        contenido_valido(&final_)?;
        let p = self.resolver(ruta)?;
        escribir_atomico(&p, &final_)?;
        let meta = fs::metadata(&p)?;
        let (entrada, cuerpo) = entrada_de(ruta, &final_, &meta);
        let indice = store.boveda_indexar(&entrada, &cuerpo);
        let nueva = huella(&final_);
        evento(
            store,
            quien,
            "boveda_edicion",
            None,
            true,
            json!({
                "ruta": ruta, "region": region_de(ruta), "bytes": final_.len(),
                "huella_antes": antes, "huella": nueva, "indice": indice.is_ok(),
            }),
            inicio,
        )
        .map_err(|e| ErrorBoveda::EventoPerdido(ruta.to_string(), e))?;
        self.leer_completa(ruta)
    }

    /// Crea una nota del usuario, solo en la región compartida, con su
    /// cabecera (`autor: usuario`, título = nombre del fichero). `cuerpo` va
    /// sin cabecera; vacío, se pone el título como encabezado.
    pub fn crear_usuario(&self, store: &mut Store, ruta: &str, cuerpo: &str, quien: &str) -> Resultado<NotaCompleta> {
        let inicio = Instant::now();
        exigir_usuario(quien, "crear notas a mano")?;
        let ruta = ruta_de_nota(ruta)?;
        if region_de(&ruta) != REGION_COMPARTIDA {
            return Err(ErrorBoveda::Prohibida(format!(
                "{ruta}: a mano solo se crean notas en {REGION_COMPARTIDA}/ (las demás regiones son de sus agentes)"
            )));
        }
        contenido_valido(cuerpo)?;
        if !matches!(cabecera::interpretar(cuerpo).0, Cabeceras::Ausente) {
            return Err(ErrorBoveda::Argumentos("el contenido va sin cabecera: la pone el sistema".into()));
        }
        let titulo = Path::new(&ruta).file_stem().and_then(|s| s.to_str()).unwrap_or("nota").trim().to_string();
        cabecera::titulo_valido(&titulo)?;
        let ahora = ahora();
        let cab = Cabecera {
            titulo: titulo.clone(),
            autor: "usuario".into(),
            region: REGION_COMPARTIDA.into(),
            creada: ahora.clone(),
            actualizada: ahora,
            origen: "manual".into(),
            promocion: "ninguna".into(),
            tags: Vec::new(),
            promovida_de: None,
            promovida_a: None,
            promovida_por: None,
            promovida_en: None,
            extras: Vec::new(),
        };
        cab.validar()?;
        let cuerpo = if cuerpo.trim().is_empty() { format!("# {titulo}\n") } else { cuerpo.to_string() };
        let texto = cab.con_cuerpo(&cuerpo);

        let _candado = self.candado()?;
        let destino = self.resolver(&ruta)?;
        if existe(&destino) {
            return Err(ErrorBoveda::YaExiste(ruta));
        }
        if let Some(dir) = destino.parent() {
            fs::create_dir_all(dir)?;
        }
        self.resolver(&ruta)?;
        escribir_atomico(&destino, &texto)?;
        let meta = fs::metadata(&destino)?;
        let (entrada, c) = entrada_de(&ruta, &texto, &meta);
        let indice = store.boveda_indexar(&entrada, &c);
        evento(
            store,
            quien,
            "boveda_creacion",
            None,
            true,
            json!({ "ruta": ruta, "region": REGION_COMPARTIDA, "bytes": texto.len(), "huella": huella(&texto), "indice": indice.is_ok() }),
            inicio,
        )
        .map_err(|e| ErrorBoveda::EventoPerdido(ruta.clone(), e))?;
        self.leer_completa(&ruta)
    }

    /// Crea una carpeta (vacía) en cualquier región.
    pub fn crear_carpeta(&self, store: &mut Store, ruta: &str, quien: &str) -> Resultado<String> {
        let inicio = Instant::now();
        exigir_usuario(quien, "crear carpetas a mano")?;
        let ruta = ruta.trim().trim_end_matches('/').to_string();
        if ruta::validar_carpeta(&ruta)?.len() < 2 {
            return Err(ErrorBoveda::Argumentos("las regiones ya existen: crea una carpeta dentro de una".into()));
        }
        let _candado = self.candado()?;
        let p = self.resolver_carpeta(&ruta)?;
        if existe(&p) {
            return Err(ErrorBoveda::YaExiste(ruta));
        }
        fs::create_dir_all(&p)?;
        self.resolver_carpeta(&ruta)?;
        evento(store, quien, "boveda_carpeta", None, true, json!({ "ruta": ruta, "region": region_de(&ruta) }), inicio)
            .map_err(|e| ErrorBoveda::EventoPerdido(ruta.clone(), e))?;
        Ok(ruta)
    }

    /// Renombra o mueve una nota dentro de su región, sin pisar ninguna, y
    /// pone al día los enlaces (`[[…]]` y markdown) de todas las notas
    /// indexadas. Los `[[enlaces]]` ambiguos no se tocan: se devuelven.
    pub fn renombrar(
        &self,
        store: &mut Store,
        de: &str,
        a: &str,
        version: &str,
        quien: &str,
    ) -> Resultado<Renombrada> {
        let inicio = Instant::now();
        exigir_usuario(quien, "renombrar notas a mano")?;
        validar_ruta(de)?;
        let a = ruta_de_nota(a)?;
        if !ruta::misma_region(de, &a) {
            return Err(ErrorBoveda::Prohibida(format!(
                "{de} → {a}: solo se mueve dentro de la misma región (a la compartida se llega promoviendo)"
            )));
        }
        if a == de {
            return Err(ErrorBoveda::Argumentos("la ruta nueva es la misma".into()));
        }
        self.sincronizar(store, false)?;
        let _candado = self.candado()?;
        let texto = self.leer_texto(de)?;
        if huella(&texto) != version {
            return Err(ErrorBoveda::Cambio(format!("{de} cambió en disco desde que la abriste; recárgala")));
        }
        let origen = self.resolver(de)?;
        let destino = self.resolver(&a)?;
        if existe(&destino) {
            return Err(ErrorBoveda::YaExiste(a));
        }
        if let Some(dir) = destino.parent() {
            fs::create_dir_all(dir)?;
        }
        let destino = self.resolver(&a)?;
        // `hard_link` falla si el destino existe: así nunca se pisa otra nota.
        match fs::hard_link(&origen, &destino) {
            Ok(()) => {
                if let Err(e) = fs::remove_file(&origen) {
                    let _ = fs::remove_file(&destino);
                    return Err(e.into());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Err(ErrorBoveda::YaExiste(a)),
            // Sin enlaces duros (otro sistema de ficheros): renombrar, ya
            // comprobado bajo el candado que el destino no existe.
            Err(_) => fs::rename(&origen, &destino)?,
        }

        let mut antes: Vec<String> = store.boveda_todas()?.into_iter().map(|r| r.ruta).collect();
        if !antes.iter().any(|r| r == de) {
            antes.push(de.to_string());
        }
        let despues: Vec<String> = antes.iter().map(|r| if r == de { a.clone() } else { r.clone() }).collect();
        let (mut actualizadas, mut ambiguas) = (Vec::new(), Vec::new());
        let mut texto_movida = texto.clone();
        for r in &despues {
            let (desde_antes, actual) = if *r == a {
                (de, texto.clone())
            } else {
                match self.leer_texto(r) {
                    Ok(t) => (r.as_str(), t),
                    Err(_) => continue,
                }
            };
            let re = enlaces::reescribir_tras_mover(&actual, desde_antes, r, de, &a, &antes, &despues);
            if re.ambiguos {
                ambiguas.push(r.clone());
            }
            let Some(nuevo) = re.texto else { continue };
            if *r == a {
                texto_movida = nuevo;
                continue;
            }
            let p = self.resolver(r)?;
            escribir_atomico(&p, &nuevo)?;
            if let Ok(meta) = fs::metadata(&p) {
                let (e, c) = entrada_de(r, &nuevo, &meta);
                let _ = store.boveda_indexar(&e, &c);
            }
            actualizadas.push(r.clone());
        }
        if texto_movida != texto {
            escribir_atomico(&destino, &texto_movida)?;
        }
        store.boveda_olvidar(de)?;
        let meta = fs::metadata(&destino)?;
        let (entrada, c) = entrada_de(&a, &texto_movida, &meta);
        let indice = store.boveda_indexar(&entrada, &c);
        evento(
            store,
            quien,
            "boveda_renombrado",
            None,
            true,
            json!({
                "de": de, "a": a, "region": region_de(&a), "actualizadas": actualizadas,
                "ambiguas": ambiguas, "indice": indice.is_ok(),
            }),
            inicio,
        )
        .map_err(|e| ErrorBoveda::EventoPerdido(a.clone(), e))?;
        Ok(Renombrada { nota: self.leer_completa(&a)?, actualizadas, ambiguas })
    }

    /// Carpeta de la papelera, comprobando que no es un enlace simbólico.
    fn dir_papelera(&self) -> Resultado<PathBuf> {
        let raiz = fs::canonicalize(self.raiz())
            .map_err(|e| ErrorBoveda::Io(format!("raíz de la bóveda {}: {e}", self.raiz().display())))?;
        let p = raiz.join(PAPELERA);
        if es_enlace(&p) {
            return Err(ErrorBoveda::FueraDeLaBoveda(format!("{PAPELERA} es un enlace simbólico")));
        }
        Ok(p)
    }

    /// Manda una nota a la papelera (`<raíz>/.papelera/<id>/<ruta>`), si sigue
    /// en la versión que vio el usuario. Se restaura con [`Self::restaurar`].
    pub fn a_papelera(&self, store: &mut Store, ruta: &str, version: &str, quien: &str) -> Resultado<EnPapelera> {
        let inicio = Instant::now();
        exigir_usuario(quien, "borrar notas a mano")?;
        validar_ruta(ruta)?;
        let _candado = self.candado()?;
        let texto = self.leer_texto(ruta)?;
        if huella(&texto) != version {
            return Err(ErrorBoveda::Cambio(format!("{ruta} cambió en disco desde que la abriste; recárgala")));
        }
        let origen = self.resolver(ruta)?;
        let papelera = self.dir_papelera()?;
        let marca = marca_ahora();
        let mut n = 1;
        let id = loop {
            let id = format!("{marca}-{n}");
            if !existe(&papelera.join(&id)) {
                break id;
            }
            n += 1;
            if n > 10_000 {
                return Err(ErrorBoveda::Io("la papelera no admite más entradas en este segundo".into()));
            }
        };
        let destino = papelera.join(&id).join(ruta);
        if let Some(dir) = destino.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::rename(&origen, &destino)?;
        store.boveda_olvidar(ruta)?;
        let bytes = texto.len() as u64;
        evento(
            store,
            quien,
            "boveda_papelera",
            None,
            true,
            json!({ "ruta": ruta, "region": region_de(ruta), "id": id, "bytes": bytes, "huella": version }),
            inicio,
        )
        .map_err(|e| ErrorBoveda::EventoPerdido(ruta.to_string(), e))?;
        Ok(EnPapelera { borrada: borrada_de(&id), id, ruta: ruta.to_string(), bytes })
    }

    /// Lo que hay en la papelera, lo más reciente primero.
    pub fn papelera(&self) -> Resultado<Vec<EnPapelera>> {
        let Ok(dir) = self.dir_papelera() else { return Ok(Vec::new()) };
        let Ok(entradas) = fs::read_dir(&dir) else { return Ok(Vec::new()) };
        let mut out = Vec::new();
        for e in entradas.flatten() {
            let id = e.file_name().to_string_lossy().into_owned();
            if !id_valido(&id) || !e.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            for (ruta, p) in notas_bajo(&e.path()) {
                if validar_ruta(&ruta).is_err() {
                    continue;
                }
                let bytes = fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                out.push(EnPapelera { id: id.clone(), ruta, borrada: borrada_de(&id), bytes });
            }
        }
        out.sort_by(|a, b| b.id.cmp(&a.id));
        Ok(out)
    }

    /// Devuelve a su sitio la nota `id` de la papelera; si allí ya hay otra,
    /// [`ErrorBoveda::YaExiste`] y no se toca nada.
    pub fn restaurar(&self, store: &mut Store, id: &str, quien: &str) -> Resultado<NotaCompleta> {
        let inicio = Instant::now();
        exigir_usuario(quien, "restaurar notas a mano")?;
        if !id_valido(id) {
            return Err(ErrorBoveda::Argumentos(format!("'{id}' no es una entrada de la papelera")));
        }
        let _candado = self.candado()?;
        let base = self.dir_papelera()?.join(id);
        if !base.is_dir() || es_enlace(&base) {
            return Err(ErrorBoveda::NoExiste(format!("{id} (en la papelera)")));
        }
        let Some((ruta, guardada)) = notas_bajo(&base).into_iter().find(|(r, _)| validar_ruta(r).is_ok()) else {
            return Err(ErrorBoveda::NoExiste(format!("{id} (en la papelera)")));
        };
        let destino = self.resolver(&ruta)?;
        if existe(&destino) {
            return Err(ErrorBoveda::YaExiste(format!("{ruta} (ya hay otra nota ahí: renómbrala antes de restaurar)")));
        }
        if let Some(dir) = destino.parent() {
            fs::create_dir_all(dir)?;
        }
        let destino = self.resolver(&ruta)?;
        fs::rename(&guardada, &destino)?;
        if let Some(dir) = guardada.parent() {
            podar(dir, &base);
        }
        let texto = self.leer_texto(&ruta)?;
        let meta = fs::metadata(&destino)?;
        let (entrada, c) = entrada_de(&ruta, &texto, &meta);
        let indice = store.boveda_indexar(&entrada, &c);
        evento(
            store,
            quien,
            "boveda_restauracion",
            None,
            true,
            json!({ "ruta": ruta, "region": region_de(&ruta), "id": id, "bytes": texto.len(), "indice": indice.is_ok() }),
            inicio,
        )
        .map_err(|e| ErrorBoveda::EventoPerdido(ruta.clone(), e))?;
        self.leer_completa(&ruta)
    }
}
