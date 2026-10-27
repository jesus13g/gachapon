//! «⌨ abrir en la terminal»: lo que la web no puede confirmar (lo
//! irreversible, art. IV.2) se abre en una terminal real del escritorio con
//! el comando de `ea` ya escrito. La web **solo abre la ventana**: la
//! simulación y la frase `confirmo <referencia>` las pide `ea` en esa TTY,
//! igual que si el usuario lo hubiera tecleado. Nunca se pasa la frase.
//!
//! El comando sale de una lista cerrada (decidir una propuesta pendiente,
//! instalar o desinstalar los servicios, o ejecutar una herramienta supervisada del catálogo con argumentos que
//! validan contra sus campos) y va como argv, sin shell, como hace
//! `explorador` con el gestor de ficheros.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ea_acciones::{Canal, Riesgo};
use ea_store::{EstadoPropuesta, EventOutcome, NewEvent};
use serde::Serialize;
use serde_json::{json, Map, Value};

use super::{ErrorWeb, Resultado, ACTOR};
use crate::AppState;

/// Cómo se abre la terminal. `WebConfig::load` usa `Sistema`; las pruebas,
/// `Simulada` (nunca se abre una ventana de verdad desde una prueba).
#[derive(Clone)]
pub enum Terminal {
    /// Un lanzador del sistema (`ea_plataforma::escritorio::lanzadores_terminal`),
    /// con el binario `ea` que hay junto a `ea-web`.
    Sistema { ea: PathBuf },
    /// No se lanza nada: cada argv que se habría lanzado se apunta aquí.
    Simulada { ea: PathBuf, lanzadas: Arc<Mutex<Vec<Vec<String>>>> },
    /// Sin terminal (503).
    Ninguna,
}

impl Terminal {
    /// La del sistema, con el `ea` de la misma carpeta que este binario
    /// (`target/release/`, donde los ejecutan los servicios), nunca del PATH.
    pub fn del_sistema() -> Self {
        match ea_plataforma::rutas::ruta_hermana("ea") {
            Some(ea) => Self::Sistema { ea },
            None => Self::Ninguna,
        }
    }

    /// Una simulada y el registro donde apunta lo que habría lanzado.
    pub fn simulada(ea: impl Into<PathBuf>) -> (Self, Arc<Mutex<Vec<Vec<String>>>>) {
        let lanzadas = Arc::new(Mutex::new(Vec::new()));
        (Self::Simulada { ea: ea.into(), lanzadas: lanzadas.clone() }, lanzadas)
    }
}

/// Lo que se ha abierto, para que la página lo diga.
#[derive(Debug, Serialize)]
pub struct TerminalAbierta {
    pub ok: bool,
    /// El comando tal como se vería escrito (solo para mostrarlo).
    pub comando: String,
}

/// Uno de los tres comandos permitidos, ya validado.
#[derive(Debug, PartialEq)]
enum Orden {
    Propuesta(i64),
    /// `ea servicios instalar|desinstalar` (spec 020, H4): el gestor del
    /// sistema se cambia en la TTY, con su confirmación escrita.
    Servicios(&'static str),
    Herramienta { nombre: &'static str, argumentos: Value },
    /// `ea boveda vaciar-papelera`: borrar para siempre la papelera de la
    /// bóveda (spec 015, RF-012). No es una herramienta del catálogo.
    VaciarPapeleraBoveda,
}

impl Orden {
    /// Los argumentos de `ea` (sin el binario). `--esperar` deja la ventana
    /// abierta al final, para que se lea el resultado.
    fn argv(&self) -> Vec<String> {
        match self {
            Orden::Propuesta(id) => {
                vec!["propuestas".into(), "decidir".into(), id.to_string(), "--aceptar".into(), "--esperar".into()]
            }
            Orden::Servicios(accion) => vec!["servicios".into(), (*accion).into(), "--esperar".into()],
            Orden::VaciarPapeleraBoveda => vec!["boveda".into(), "vaciar-papelera".into(), "--esperar".into()],
            Orden::Herramienta { nombre, argumentos } => vec![
                "acciones".into(),
                "ejecutar".into(),
                (*nombre).into(),
                "--args".into(),
                argumentos.to_string(),
                "--esperar".into(),
            ],
        }
    }

    fn texto(&self) -> String {
        let mut partes = vec!["ea".to_string()];
        for a in self.argv() {
            if a == "--esperar" {
                continue;
            }
            partes.push(if a.starts_with('{') { format!("'{a}'") } else { a });
        }
        partes.join(" ")
    }

    fn detalle(&self) -> Value {
        match self {
            Orden::Propuesta(id) => json!({ "propuesta": id }),
            Orden::Servicios(accion) => json!({ "servicios": accion }),
            Orden::VaciarPapeleraBoveda => json!({ "boveda": "vaciar-papelera" }),
            Orden::Herramienta { nombre, argumentos } => json!({ "herramienta": nombre, "argumentos": argumentos }),
        }
    }
}

fn invalido(m: impl Into<String>) -> ErrorWeb {
    ErrorWeb::Invalido(m.into())
}

/// Lo que pide el cuerpo, sin validar todavía contra el estado.
#[derive(Debug, PartialEq)]
enum Pedido {
    Propuesta(i64),
    Servicios(&'static str),
    Herramienta(String, Value),
    VaciarPapeleraBoveda,
}

/// El cuerpo es `{propuesta: <id>}`, `{servicios: "instalar"|"desinstalar"}`,
/// `{herramienta, argumentos}` o `{boveda: "vaciar-papelera"}`, nada más: ni
/// comandos ni texto libre.
fn leer_cuerpo(cuerpo: &Value) -> Resultado<Pedido> {
    let obj = cuerpo
        .as_object()
        .ok_or_else(|| invalido("se espera {propuesta}, {servicios}, {herramienta, argumentos} o {boveda}"))?;
    let claves: Vec<&str> = obj.keys().map(String::as_str).collect();
    match claves.as_slice() {
        ["boveda"] if obj["boveda"] == "vaciar-papelera" => Ok(Pedido::VaciarPapeleraBoveda),
        ["boveda"] => Err(invalido("de la bóveda solo se abre en la terminal {\"boveda\": \"vaciar-papelera\"}")),
        ["propuesta"] => obj["propuesta"]
            .as_i64()
            .filter(|&id| id > 0)
            .map(Pedido::Propuesta)
            .ok_or_else(|| invalido("'propuesta' debe ser el número de una propuesta")),
        ["servicios"] => match obj["servicios"].as_str() {
            Some("instalar") => Ok(Pedido::Servicios("instalar")),
            Some("desinstalar") => Ok(Pedido::Servicios("desinstalar")),
            _ => Err(invalido("'servicios' debe ser «instalar» o «desinstalar»")),
        },
        ["argumentos", "herramienta"] | ["herramienta", "argumentos"] => {
            let h = obj["herramienta"].as_str().ok_or_else(|| invalido("'herramienta' debe ser un nombre del catálogo"))?;
            Ok(Pedido::Herramienta(h.to_string(), obj["argumentos"].clone()))
        }
        _ => Err(invalido("solo se abre en la terminal {propuesta}, {servicios}, {herramienta, argumentos} o {boveda}")),
    }
}

/// Una herramienta supervisada que la web no puede confirmar, con
/// argumentos que son exactamente sus campos y pasan la validación.
fn validar_herramienta(nombre: &str, argumentos: Value) -> Resultado<Orden> {
    let h = ea_acciones::buscar(nombre)
        .filter(|h| h.ejecucion == ea_acciones::Ejecucion::Supervisada)
        .ok_or_else(|| invalido(format!("'{nombre}' no es una herramienta supervisada del catálogo")))?;
    if Canal::Web.puede_confirmar(h.riesgo).is_ok() {
        return Err(invalido(format!("'{nombre}' se confirma desde la web; la terminal es para lo irreversible")));
    }
    let args: &Map<String, Value> =
        argumentos.as_object().ok_or_else(|| invalido("'argumentos' debe ser un objeto"))?;
    let campos = ea_acciones::campos(nombre);
    if let Some(k) = args.keys().find(|k| !campos.iter().any(|c| c.nombre == k.as_str())) {
        return Err(invalido(format!("'{k}' no es un argumento de {nombre}")));
    }
    if args.values().any(|v| !(v.is_string() || v.is_i64() || v.is_u64())) {
        return Err(invalido("los argumentos solo pueden ser textos o enteros"));
    }
    ea_acciones::validar_argumentos(nombre, &argumentos)?;
    Ok(Orden::Herramienta { nombre: h.nombre, argumentos })
}

/// Una propuesta pendiente con una acción irreversible (las demás se
/// deciden desde la web).
async fn validar_propuesta(s: &AppState, id: i64) -> Resultado<Orden> {
    let p = s.store.lock().await.get_propuesta(id)?;
    if p.estado != EstadoPropuesta::Pendiente {
        return Err(ErrorWeb::Conflicto(format!("la propuesta #{id} ya está decidida")));
    }
    let riesgo = p.accion.as_ref().and_then(|a| Riesgo::parse(&a.riesgo));
    match riesgo {
        Some(r) if Canal::Web.puede_confirmar(r).is_err() => Ok(Orden::Propuesta(id)),
        _ => Err(invalido(format!("la propuesta #{id} se decide desde la web; la terminal es para lo irreversible"))),
    }
}

/// Abre una terminal con `ea <args>` según la configuración: la del sistema,
/// la simulada de las pruebas o ninguna (503). Quien llama ya ha validado los
/// argumentos contra una lista cerrada.
pub fn lanzar_argv(s: &AppState, args: &[String]) -> Resultado<()> {
    match &s.config.terminal {
        Terminal::Ninguna => return Err(ErrorWeb::NoDisponible("no hay terminal configurada".into())),
        Terminal::Sistema { ea } => {
            if !ea.is_file() {
                return Err(ErrorWeb::NoDisponible(format!("no está el binario {}", ea.display())));
            }
            let mut argv = vec![ea.display().to_string()];
            argv.extend(args.iter().cloned());
            ea_acciones::lanzar_en_terminal(&argv).map_err(ErrorWeb::NoDisponible)?;
        }
        Terminal::Simulada { ea, lanzadas } => {
            let mut argv = vec![ea_plataforma::escritorio::lanzadores_terminal()[0].to_string(), ea.display().to_string()];
            argv.extend(args.iter().cloned());
            lanzadas.lock().map_err(|_| ErrorWeb::Interno("registro envenenado".into()))?.push(argv);
        }
    }
    Ok(())
}

/// Abre una terminal con el comando que confirma en ella. Queda en el
/// diario como `accion_terminal_abierta` (abrir no decide nada).
pub async fn abrir(s: &AppState, cuerpo: &Value) -> Resultado<TerminalAbierta> {
    let orden = match leer_cuerpo(cuerpo)? {
        Pedido::Propuesta(id) => validar_propuesta(s, id).await?,
        Pedido::Servicios(accion) => Orden::Servicios(accion),
        Pedido::Herramienta(h, args) => validar_herramienta(&h, args)?,
        Pedido::VaciarPapeleraBoveda => Orden::VaciarPapeleraBoveda,
    };
    lanzar_argv(s, &orden.argv())?;
    let evento = NewEvent {
        actor: ACTOR.into(),
        action: "accion_terminal_abierta".into(),
        trabajo: None,
        outcome: Some(EventOutcome::Ok),
        duration_ms: None,
        detail: Some(orden.detalle().to_string()),
    };
    if let Err(e) = s.store.lock().await.append_event(evento) {
        tracing::warn!(error = %e, "no se pudo apuntar la terminal abierta en el diario");
    }
    Ok(TerminalAbierta { ok: true, comando: orden.texto() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_argv_es_cerrado_y_sin_frase() {
        let o = Orden::Herramienta { nombre: "ea_eliminar_rutina", argumentos: json!({ "slug": "x" }) };
        assert_eq!(o.argv(), ["acciones", "ejecutar", "ea_eliminar_rutina", "--args", r#"{"slug":"x"}"#, "--esperar"]);
        assert_eq!(o.texto(), r#"ea acciones ejecutar ea_eliminar_rutina --args '{"slug":"x"}'"#);
        assert_eq!(Orden::Propuesta(7).argv(), ["propuestas", "decidir", "7", "--aceptar", "--esperar"]);
        assert_eq!(Orden::Servicios("desinstalar").argv(), ["servicios", "desinstalar", "--esperar"]);
        assert_eq!(Orden::Servicios("instalar").texto(), "ea servicios instalar");
        assert!(!Orden::Propuesta(7).argv().iter().any(|a| a.contains("confirmo")));
    }

    #[test]
    fn solo_acepta_las_formas_cerradas() {
        assert!(leer_cuerpo(&json!({ "comando": "rm -rf /" })).is_err());
        assert!(leer_cuerpo(&json!("ea propuestas decidir 1")).is_err());
        assert!(leer_cuerpo(&json!({ "propuesta": "1; rm" })).is_err());
        assert!(leer_cuerpo(&json!({ "propuesta": 1, "confirmo": "1" })).is_err());
        assert_eq!(leer_cuerpo(&json!({ "propuesta": 3 })).unwrap(), Pedido::Propuesta(3));
        assert_eq!(leer_cuerpo(&json!({ "servicios": "instalar" })).unwrap(), Pedido::Servicios("instalar"));
        for malo in [json!({ "servicios": "reiniciar" }), json!({ "servicios": 1 }), json!({ "servicios": "instalar", "x": 1 }), json!({ "servicios": "instalar; rm" })] {
            assert!(leer_cuerpo(&malo).is_err(), "{malo}");
        }
        assert_eq!(leer_cuerpo(&json!({ "boveda": "vaciar-papelera" })).unwrap(), Pedido::VaciarPapeleraBoveda);
        assert!(leer_cuerpo(&json!({ "boveda": "rm -rf" })).is_err());
        assert!(leer_cuerpo(&json!({ "boveda": "vaciar-papelera", "confirmo": "x" })).is_err());
        let v = Orden::VaciarPapeleraBoveda;
        assert_eq!(v.argv(), ["boveda", "vaciar-papelera", "--esperar"]);
        assert_eq!(v.texto(), "ea boveda vaciar-papelera");
        assert!(validar_herramienta("ea_eliminar_rutina", json!({ "slug": "a b" })).is_err());
        assert!(validar_herramienta("ea_eliminar_rutina", json!({ "slug": "x", "extra": 1 })).is_err());
        assert!(validar_herramienta("ea_encolar_rutina", json!({ "slug": "x" })).is_err(), "reversible: desde la web");
        assert!(validar_herramienta("ea_leer_nota", json!({})).is_err());
        assert!(validar_herramienta("ea_eliminar_rutina", json!({ "slug": "x" })).is_ok());
    }
}
