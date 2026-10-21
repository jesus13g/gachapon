//! Edición de un campo de un JSON SOBRE EL TEXTO, sin reformatear nada más.
//!
//! `serde_json` reescribiría el fichero entero (otro orden de claves u otra
//! sangría), y activar tras desactivar no dejaría los bytes como estaban. Aquí
//! se localiza el objeto por su ruta con un analizador mínimo (admite comas
//! finales; los comentarios los rechaza quien llama) y solo se toca el campo:
//!
//! - [`poner`]: si el campo existe, sustituye su valor; si no, lo añade como
//!   último miembro copiando la sangría del anterior;
//! - [`quitar`]: borra exactamente lo que `poner` añadió, así que
//!   `poner` + `quitar` sobre un objeto sin el campo devuelve el texto original.


#[cfg(test)]
use serde_json::Value;

use crate::error::ErrorBiblioteca;

#[derive(Debug, Clone)]
struct Miembro {
    clave: String,
    ini_clave: usize,
    valor: Nodo,
}

#[derive(Debug, Clone)]
struct Nodo {
    /// Posición del primer byte del valor.
    ini: usize,
    /// Posición siguiente al último byte del valor.
    fin: usize,
    /// Los miembros, si es un objeto.
    miembros: Option<Vec<Miembro>>,
}

fn mal(msg: impl Into<String>) -> ErrorBiblioteca {
    ErrorBiblioteca::Io(format!("JSON no interpretable: {}", msg.into()))
}

struct Lector<'a> {
    t: &'a [u8],
    texto: &'a str,
    i: usize,
}

impl<'a> Lector<'a> {
    fn blancos(&mut self) {
        while self.i < self.t.len() && self.t[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn cadena(&mut self) -> Result<(usize, usize), ErrorBiblioteca> {
        let ini = self.i;
        if self.t.get(self.i) != Some(&b'"') {
            return Err(mal(format!("se esperaba «\"» en el byte {}", self.i)));
        }
        self.i += 1;
        while self.i < self.t.len() {
            match self.t[self.i] {
                b'\\' => self.i += 2,
                b'"' => {
                    self.i += 1;
                    return Ok((ini, self.i));
                }
                _ => self.i += 1,
            }
        }
        Err(mal("cadena sin cerrar"))
    }

    fn valor(&mut self) -> Result<Nodo, ErrorBiblioteca> {
        self.blancos();
        let ini = self.i;
        match self.t.get(self.i) {
            None => Err(mal("fin inesperado")),
            Some(b'{') => {
                self.i += 1;
                let mut miembros = Vec::new();
                loop {
                    self.blancos();
                    match self.t.get(self.i) {
                        Some(b'}') => {
                            self.i += 1;
                            break;
                        }
                        Some(b',') => self.i += 1,
                        Some(b'"') => {
                            let (ci, cf) = self.cadena()?;
                            let clave: String = serde_json::from_str(&self.texto[ci..cf]).map_err(|e| mal(e.to_string()))?;
                            self.blancos();
                            if self.t.get(self.i) != Some(&b':') {
                                return Err(mal(format!("se esperaba «:» en el byte {}", self.i)));
                            }
                            self.i += 1;
                            let valor = self.valor()?;
                            miembros.push(Miembro { clave, ini_clave: ci, valor });
                        }
                        _ => return Err(mal(format!("objeto mal formado en el byte {}", self.i))),
                    }
                }
                Ok(Nodo { ini, fin: self.i, miembros: Some(miembros) })
            }
            Some(b'[') => {
                self.i += 1;
                loop {
                    self.blancos();
                    match self.t.get(self.i) {
                        Some(b']') => {
                            self.i += 1;
                            break;
                        }
                        Some(b',') => self.i += 1,
                        Some(_) => {
                            self.valor()?;
                        }
                        None => return Err(mal("lista sin cerrar")),
                    }
                }
                Ok(Nodo { ini, fin: self.i, miembros: None })
            }
            Some(b'"') => {
                let (ini, fin) = self.cadena()?;
                Ok(Nodo { ini, fin, miembros: None })
            }
            Some(_) => {
                while self.i < self.t.len() && !matches!(self.t[self.i], b',' | b'}' | b']') && !self.t[self.i].is_ascii_whitespace() {
                    self.i += 1;
                }
                if self.i == ini {
                    return Err(mal(format!("valor vacío en el byte {ini}")));
                }
                Ok(Nodo { ini, fin: self.i, miembros: None })
            }
        }
    }
}

fn analizar(texto: &str) -> Result<Nodo, ErrorBiblioteca> {
    let mut l = Lector { t: texto.as_bytes(), texto, i: 0 };
    let raiz = l.valor()?;
    l.blancos();
    if l.i != texto.len() {
        return Err(mal(format!("texto sobrante en el byte {}", l.i)));
    }
    if raiz.miembros.is_none() {
        return Err(mal("la raíz no es un objeto"));
    }
    Ok(raiz)
}

/// El objeto que cuelga de `ruta` (vacía: la raíz).
fn objeto<'a>(raiz: &'a Nodo, ruta: &[&str]) -> Result<&'a Nodo, ErrorBiblioteca> {
    let mut actual = raiz;
    for paso in ruta {
        actual = actual
            .miembros
            .as_ref()
            .and_then(|m| m.iter().find(|m| m.clave == *paso))
            .map(|m| &m.valor)
            .ok_or_else(|| mal(format!("no existe «{paso}»")))?;
        if actual.miembros.is_none() {
            return Err(mal(format!("«{paso}» no es un objeto")));
        }
    }
    Ok(actual)
}

/// El valor de `campo` dentro del objeto `ruta` (`None` si no está).
#[cfg(test)]
pub fn leer(texto: &str, ruta: &[&str], campo: &str) -> Result<Option<Value>, ErrorBiblioteca> {
    let raiz = analizar(texto)?;
    let obj = objeto(&raiz, ruta)?;
    match obj.miembros.as_ref().and_then(|m| m.iter().find(|m| m.clave == campo)) {
        None => Ok(None),
        Some(m) => serde_json::from_str(&texto[m.valor.ini..m.valor.fin]).map(Some).map_err(|e| mal(e.to_string())),
    }
}

/// Sangría (espacios o tabuladores) de la línea en que empieza `pos`, si en
/// esa línea solo hay blancos antes de `pos`.
fn sangria(texto: &str, pos: usize) -> Option<&str> {
    let inicio = texto[..pos].rfind('\n').map_or(0, |n| n + 1);
    let previa = &texto[inicio..pos];
    (inicio > 0 && previa.chars().all(|c| c == ' ' || c == '\t')).then_some(previa)
}

/// Pone `campo = literal` (el literal JSON ya escrito, p. ej. `false`) en el
/// objeto `ruta`.
pub fn poner(texto: &str, ruta: &[&str], campo: &str, literal: &str) -> Result<String, ErrorBiblioteca> {
    let raiz = analizar(texto)?;
    let obj = objeto(&raiz, ruta)?;
    let miembros = obj.miembros.as_ref().expect("es un objeto");
    if let Some(m) = miembros.iter().find(|m| m.clave == campo) {
        return Ok(format!("{}{literal}{}", &texto[..m.valor.ini], &texto[m.valor.fin..]));
    }
    let clave = serde_json::to_string(campo).map_err(|e| mal(e.to_string()))?;
    let Some(ultimo) = miembros.last() else {
        // `{}` (con o sin blancos dentro): se escribe en línea.
        return Ok(format!("{}{{{clave}: {literal}}}{}", &texto[..obj.ini], &texto[obj.fin..]));
    };
    let fin = ultimo.valor.fin;
    let nuevo = match sangria(texto, ultimo.ini_clave) {
        Some(s) => format!(",\n{s}{clave}: {literal}"),
        None => format!(", {clave}: {literal}"),
    };
    Ok(format!("{}{nuevo}{}", &texto[..fin], &texto[fin..]))
}

/// Quita `campo` del objeto `ruta` (si no estaba, devuelve el texto igual).
pub fn quitar(texto: &str, ruta: &[&str], campo: &str) -> Result<String, ErrorBiblioteca> {
    let raiz = analizar(texto)?;
    let obj = objeto(&raiz, ruta)?;
    let miembros = obj.miembros.as_ref().expect("es un objeto");
    let Some(pos) = miembros.iter().position(|m| m.clave == campo) else { return Ok(texto.to_string()) };
    let (desde, hasta) = if pos > 0 {
        // Desde el final del miembro anterior: se lleva la coma y la sangría.
        (miembros[pos - 1].valor.fin, miembros[pos].valor.fin)
    } else if miembros.len() > 1 {
        (miembros[0].ini_clave, miembros[1].ini_clave)
    } else {
        (obj.ini + 1, obj.fin - 1)
    };
    Ok(format!("{}{}", &texto[..desde], &texto[hasta..]))
}

#[cfg(test)]
mod pruebas {
    use super::*;

    const BASE: &str = "{\n  \"mcp\": {\n    \"x\": {\n      \"type\": \"local\",\n      \"command\": [\"a\", \"b\"]\n    },\n    \"y\": { \"type\": \"remote\", \"url\": \"https://h\" }\n  },\n  \"otro\": 1\n}\n";

    #[test]
    fn poner_y_quitar_son_inversas() {
        for ruta in [&["mcp", "x"][..], &["mcp", "y"][..], &["mcp"][..]] {
            let con = poner(BASE, ruta, "enabled", "false").unwrap();
            assert_ne!(con, BASE);
            assert_eq!(leer(&con, ruta, "enabled").unwrap(), Some(Value::Bool(false)));
            serde_json::from_str::<Value>(&con).unwrap();
            assert_eq!(quitar(&con, ruta, "enabled").unwrap(), BASE, "{ruta:?}");
        }
    }

    #[test]
    fn sustituye_un_valor_existente_y_quita_el_primero_o_unico() {
        let t = "{\"a\": {\"enabled\": true, \"b\": 1}}";
        assert_eq!(poner(t, &["a"], "enabled", "false").unwrap(), "{\"a\": {\"enabled\": false, \"b\": 1}}");
        assert_eq!(quitar(t, &["a"], "enabled").unwrap(), "{\"a\": {\"b\": 1}}");
        assert_eq!(quitar("{\"a\": {\"enabled\": true}}", &["a"], "enabled").unwrap(), "{\"a\": {}}");
        assert_eq!(poner("{\"a\": {}}", &["a"], "enabled", "false").unwrap(), "{\"a\": {\"enabled\": false}}");
    }

    #[test]
    fn admite_comas_finales_y_cadenas_con_escapes() {
        let t = "{\n  \"a\": {\n    \"k\": \"con \\\" comilla, y }\",\n  },\n}\n";
        let con = poner(t, &["a"], "enabled", "false").unwrap();
        assert_eq!(leer(&con, &["a"], "enabled").unwrap(), Some(Value::Bool(false)));
        assert_eq!(quitar(&con, &["a"], "enabled").unwrap(), t);
    }

    #[test]
    fn rutas_que_no_existen_son_error() {
        assert!(poner(BASE, &["mcp", "z"], "enabled", "false").is_err());
        assert!(leer("[1]", &[], "a").is_err());
        assert!(leer("{\"a\": ", &[], "a").is_err());
    }
}
