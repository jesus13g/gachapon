//! Enlaces entre notas, al estilo de Obsidian: `[[destino]]` (con alias `|`,
//! ancla `#` y con o sin `.md`) y los enlaces markdown `[texto](ruta.md)`.
//! Todo es puro (texto y listas de rutas, nada de disco): lo usan el escritor
//! de la bóveda al renombrar o mover una nota y `ea-web` para la memoria de
//! Claude Code.
//!
//! Resolución de un `[[destino]]` (como Obsidian): la nota cuya ruta es
//! exactamente `destino` (añadiendo `.md` si falta) o acaba en `/destino`. Si
//! hay varias, gana la de la misma carpeta que la nota que enlaza; si tampoco,
//! el enlace es **ambiguo** y no se toca.

/// Forma de un enlace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forma {
    /// `[[destino|alias]]`: `destino` es lo que va antes de `|` o `#`, sin espacios en los extremos.
    Wiki,
    /// `[texto](url)`: `url` es lo que va antes de `#`, `)` o un espacio.
    Markdown,
}

/// Recorre los enlaces de `texto` y deja que `f` cambie su destino: `f`
/// recibe la forma y el destino y devuelve el destino nuevo (o `None` para
/// dejarlo como está). Conserva alias, anclas y el resto del texto. Devuelve
/// `None` si no cambió ningún enlace.
pub fn reescribir(texto: &str, mut f: impl FnMut(Forma, &str) -> Option<String>) -> Option<String> {
    let mut out = String::with_capacity(texto.len());
    let mut resto = texto;
    let mut cambios = false;
    while !resto.is_empty() {
        if let Some(tras) = resto.strip_prefix("[[")
            && let Some(fin) = tras.find("]]").filter(|&f| !tras[..f].contains('\n'))
        {
            let dentro = &tras[..fin];
            let corte = dentro.find(['|', '#']).unwrap_or(dentro.len());
            let (destino, cola) = dentro.split_at(corte);
            match f(Forma::Wiki, destino.trim()).filter(|n| n != destino.trim()) {
                Some(nuevo) => {
                    out.push_str(&format!("[[{nuevo}{cola}]]"));
                    cambios = true;
                }
                None => {
                    out.push_str("[[");
                    out.push_str(dentro);
                    out.push_str("]]");
                }
            }
            resto = &tras[fin + 2..];
            continue;
        }
        if let Some(tras) = resto.strip_prefix("](") {
            let fin = tras.find([')', '#', ' ', '\n']);
            if let Some(fin) = fin.filter(|&i| i > 0 && !tras[i..].starts_with('\n')) {
                let url = &tras[..fin];
                if let Some(nuevo) = f(Forma::Markdown, url).filter(|n| n != url) {
                    out.push_str("](");
                    out.push_str(&nuevo);
                    cambios = true;
                    resto = &tras[fin..];
                    continue;
                }
            }
        }
        let c = resto.chars().next().unwrap_or_default();
        out.push(c);
        resto = &resto[c.len_utf8()..];
    }
    cambios.then_some(out)
}

/// Cambia los enlaces a la nota `viejo` (nombre sin `.md`, sin carpeta) por
/// `nuevo`: `[[viejo]]` (con o sin `.md`, alias o ancla) y `](viejo.md)`,
/// `](./viejo.md)` o con ancla. Es lo que usa la memoria de Claude Code, donde
/// todas las notas están en la misma carpeta.
pub fn reescribir_por_nombre(texto: &str, viejo: &str, nuevo: &str) -> Option<String> {
    reescribir(texto, |forma, destino| match forma {
        Forma::Wiki => {
            let (base, ext) = match destino.strip_suffix(".md") {
                Some(b) => (b, ".md"),
                None => (destino, ""),
            };
            (base == viejo).then(|| format!("{nuevo}{ext}"))
        }
        Forma::Markdown => {
            let (prefijo, sin) = match destino.strip_prefix("./") {
                Some(s) => ("./", s),
                None => ("", destino),
            };
            (sin == format!("{viejo}.md")).then(|| format!("{prefijo}{nuevo}.md"))
        }
    })
}

/// Carpeta de una ruta (`a/b/c.md` → `a/b`; `c.md` → ``).
pub fn carpeta(ruta: &str) -> &str {
    ruta.rsplit_once('/').map(|(c, _)| c).unwrap_or("")
}

/// Une y normaliza (`.` y `..`) una ruta relativa a una carpeta. `None` si
/// sube por encima de la raíz.
fn unir(base: &str, rel: &str) -> Option<String> {
    let mut partes: Vec<&str> = base.split('/').filter(|p| !p.is_empty()).collect();
    for p in rel.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                partes.pop()?;
            }
            p => partes.push(p),
        }
    }
    (!partes.is_empty()).then(|| partes.join("/"))
}

fn con_md(d: &str) -> String {
    if d.ends_with(".md") { d.to_string() } else { format!("{d}.md") }
}

/// Las notas que podría nombrar un `[[destino]]`, sin desempatar.
pub fn candidatos_wiki<'a>(destino: &str, desde: &str, todas: &'a [String]) -> Vec<&'a String> {
    let d = destino.trim();
    if d.is_empty() {
        return Vec::new();
    }
    if d.starts_with("./") || d.starts_with("../") {
        let Some(r) = unir(carpeta(desde), &con_md(d)) else { return Vec::new() };
        return todas.iter().filter(|t| **t == r).collect();
    }
    let d = con_md(d.trim_start_matches('/'));
    let sufijo = format!("/{d}");
    todas.iter().filter(|t| **t == d || t.ends_with(&sufijo)).collect()
}

/// La nota a la que lleva un `[[destino]]` escrito en la nota `desde`, o
/// `None` si no hay ninguna o el enlace es ambiguo.
pub fn resolver_wiki(destino: &str, desde: &str, todas: &[String]) -> Option<String> {
    let c = candidatos_wiki(destino, desde, todas);
    match c.as_slice() {
        [] => None,
        [una] => Some((*una).clone()),
        varias => {
            let aqui = carpeta(desde);
            let mismas: Vec<_> = varias.iter().filter(|t| carpeta(t) == aqui).collect();
            match mismas.as_slice() {
                [una] => Some((**una).clone()),
                _ => None,
            }
        }
    }
}

fn decodificar(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let h = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(h, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn codificar(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '(' => out.push_str("%28"),
            ')' => out.push_str("%29"),
            '#' => out.push_str("%23"),
            '%' => out.push_str("%25"),
            c => out.push(c),
        }
    }
    out
}

/// ¿Es un enlace a otra cosa que una nota (`https:`, `mailto:`…)?
fn es_externo(url: &str) -> bool {
    match url.find(':') {
        Some(i) => !url[..i].contains('/'),
        None => false,
    }
}

/// La ruta (relativa a la raíz de la bóveda) a la que lleva un enlace
/// markdown escrito en la nota `desde`: relativo a su carpeta, o a la raíz si
/// empieza por `/`. Decodifica `%XX`. `None` si es externo, sube por encima de
/// la raíz o no es una nota `.md`.
pub fn resolver_md(url: &str, desde: &str) -> Option<String> {
    if url.is_empty() || es_externo(url) {
        return None;
    }
    let url = decodificar(url)?;
    let r = match url.strip_prefix('/') {
        Some(abs) => unir("", abs)?,
        None => unir(carpeta(desde), &url)?,
    };
    r.ends_with(".md").then_some(r)
}

/// Ruta relativa de `objetivo` vista desde la carpeta de la nota `desde`.
fn relativa(desde: &str, objetivo: &str) -> String {
    let base: Vec<&str> = carpeta(desde).split('/').filter(|p| !p.is_empty()).collect();
    let obj: Vec<&str> = objetivo.split('/').collect();
    let comun = base.iter().zip(&obj).take_while(|(a, b)| a == b).count();
    let mut partes: Vec<&str> = vec![".."; base.len() - comun];
    partes.extend(&obj[comun..]);
    partes.join("/")
}

/// El sufijo de ruta más corto (en carpetas, sin `.md`) que nombra solo a
/// `ruta` entre `todas`, con al menos `minimo` componentes.
pub fn sufijo_minimo(ruta: &str, todas: &[String], minimo: usize) -> String {
    let sin = ruta.strip_suffix(".md").unwrap_or(ruta);
    let partes: Vec<&str> = sin.split('/').collect();
    for k in minimo.max(1)..=partes.len() {
        let suf = partes[partes.len() - k..].join("/");
        let d = format!("{suf}.md");
        let barra = format!("/{d}");
        if todas.iter().filter(|t| **t == d || t.ends_with(&barra)).count() <= 1 {
            return suf;
        }
    }
    sin.to_string()
}

/// Lo que dejó una reescritura tras mover una nota.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Reescrita {
    /// El texto nuevo, si algún enlace cambió.
    pub texto: Option<String>,
    /// Hay `[[enlaces]]` con el nombre de la nota movida que no se pueden
    /// resolver sin ambigüedad: se dejan como estaban.
    pub ambiguos: bool,
}

/// Pone al día los enlaces de una nota después de mover `vieja` a `nueva`.
/// `desde_antes`/`desde_ahora` son la ruta de la nota que se reescribe antes
/// y después del movimiento (distintas solo si es la propia nota movida, cuyos
/// enlaces markdown relativos hay que recalcular). `antes` y `despues` son las
/// rutas de todas las notas en cada momento.
pub fn reescribir_tras_mover(texto: &str, desde_antes: &str, desde_ahora: &str, vieja: &str, nueva: &str, antes: &[String], despues: &[String]) -> Reescrita {
    let mut ambiguos = false;
    let texto = reescribir(texto, |forma, destino| match forma {
        Forma::Wiki => match resolver_wiki(destino, desde_antes, antes) {
            Some(r) if r == vieja => {
                if destino.starts_with("./") || destino.starts_with("../") {
                    let mut rel = relativa(desde_ahora, nueva);
                    if !rel.starts_with("../") {
                        rel = format!("./{rel}");
                    }
                    return Some(if destino.ends_with(".md") { rel } else { rel.trim_end_matches(".md").to_string() });
                }
                let componentes = destino.trim_start_matches('/').split('/').count();
                let s = sufijo_minimo(nueva, despues, componentes);
                Some(if destino.ends_with(".md") { format!("{s}.md") } else { s })
            }
            Some(_) => None,
            None => {
                let c = candidatos_wiki(destino, desde_antes, antes);
                if c.len() > 1 && c.iter().any(|t| *t == vieja) {
                    ambiguos = true;
                }
                None
            }
        },
        Forma::Markdown => {
            let r = resolver_md(destino, desde_antes)?;
            let movida = r == vieja;
            if !movida && desde_antes == desde_ahora {
                return None;
            }
            let objetivo = if movida { nueva.to_string() } else { r };
            // Si desde donde está ahora ya lleva al mismo sitio, no se toca.
            if resolver_md(destino, desde_ahora).as_deref() == Some(objetivo.as_str()) {
                return None;
            }
            Some(if destino.starts_with('/') {
                format!("/{}", codificar(&objetivo))
            } else {
                let rel = codificar(&relativa(desde_ahora, &objetivo));
                if destino.starts_with("./") && !rel.starts_with("../") { format!("./{rel}") } else { rel }
            })
        }
    });
    Reescrita { texto, ambiguos }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn reescribe_todas_las_formas_de_enlace() {
        let t = "[[refactor]] [[refactor.md]] [[refactor|el refactor]] [[refactor#Fases]] \
                 [Ver](refactor.md) [Ver](./refactor.md) [Ver](refactor.md#fases)";
        assert_eq!(
            reescribir_por_nombre(t, "refactor", "rehecho").unwrap(),
            "[[rehecho]] [[rehecho.md]] [[rehecho|el refactor]] [[rehecho#Fases]] \
                 [Ver](rehecho.md) [Ver](./rehecho.md) [Ver](rehecho.md#fases)"
        );
    }

    #[test]
    fn no_toca_nombres_parecidos_ni_texto_suelto() {
        let t = "[[refactorizar]] [[otro|refactor]] [x](refactorizar.md) [x](otro/refactor.md) refactor.md ñandú";
        assert_eq!(reescribir_por_nombre(t, "refactor", "rehecho"), None);
    }

    #[test]
    fn un_corchete_sin_cerrar_no_rompe_nada() {
        let t = "[[refactor\n]] y luego [[refactor]]";
        assert_eq!(reescribir_por_nombre(t, "refactor", "r").unwrap(), "[[refactor\n]] y luego [[r]]");
    }

    #[test]
    fn resuelve_como_obsidian() {
        let todas = v(&["compartida/a/x.md", "compartida/b/x.md", "claude-code/y.md", "compartida/a/z.md"]);
        assert_eq!(resolver_wiki("y", "compartida/a/z.md", &todas).as_deref(), Some("claude-code/y.md"));
        assert_eq!(resolver_wiki("y.md", "compartida/a/z.md", &todas).as_deref(), Some("claude-code/y.md"));
        assert_eq!(resolver_wiki("a/x", "claude-code/y.md", &todas).as_deref(), Some("compartida/a/x.md"));
        // Ambiguo, salvo desde la misma carpeta.
        assert_eq!(resolver_wiki("x", "claude-code/y.md", &todas), None);
        assert_eq!(resolver_wiki("x", "compartida/a/z.md", &todas).as_deref(), Some("compartida/a/x.md"));
        assert_eq!(resolver_wiki("../b/x", "compartida/a/z.md", &todas).as_deref(), Some("compartida/b/x.md"));
        assert_eq!(resolver_wiki("nada", "compartida/a/z.md", &todas), None);
    }

    #[test]
    fn resuelve_enlaces_markdown() {
        assert_eq!(resolver_md("x.md", "compartida/a/z.md").as_deref(), Some("compartida/a/x.md"));
        assert_eq!(resolver_md("../b/x%20y.md", "compartida/a/z.md").as_deref(), Some("compartida/b/x y.md"));
        assert_eq!(resolver_md("/claude-code/y.md", "compartida/a/z.md").as_deref(), Some("claude-code/y.md"));
        assert_eq!(resolver_md("https://x.org/a.md", "compartida/a/z.md"), None);
        assert_eq!(resolver_md("../../../x.md", "compartida/a/z.md"), None);
        assert_eq!(resolver_md("imagen.png", "compartida/a/z.md"), None);
    }

    #[test]
    fn el_sufijo_minimo_desambigua() {
        let todas = v(&["compartida/a/x.md", "compartida/b/x.md", "claude-code/y.md"]);
        assert_eq!(sufijo_minimo("claude-code/y.md", &todas, 1), "y");
        assert_eq!(sufijo_minimo("compartida/a/x.md", &todas, 1), "a/x");
        assert_eq!(sufijo_minimo("claude-code/y.md", &todas, 2), "claude-code/y");
    }

    #[test]
    fn mover_reescribe_los_enlaces_de_las_demas() {
        let antes = v(&["compartida/x.md", "compartida/otra.md", "claude-code/n.md"]);
        let despues = v(&["compartida/sub/x2.md", "compartida/otra.md", "claude-code/n.md"]);
        let t = "[[x]] [[x|alias]] [[x#ancla]] [[compartida/x.md]] [v](x.md) [v](./x.md#a) [o](otra.md) [[otra]]";
        let r = reescribir_tras_mover(t, "compartida/otra.md", "compartida/otra.md", "compartida/x.md", "compartida/sub/x2.md", &antes, &despues);
        assert_eq!(r.texto.as_deref(), Some("[[x2]] [[x2|alias]] [[x2#ancla]] [[sub/x2.md]] [v](sub/x2.md) [v](./sub/x2.md#a) [o](otra.md) [[otra]]"));
        assert!(!r.ambiguos);
        let r = reescribir_tras_mover(
            "[v](../compartida/x.md)",
            "claude-code/n.md",
            "claude-code/n.md",
            "compartida/x.md",
            "compartida/sub/x2.md",
            &antes,
            &despues,
        );
        assert_eq!(r.texto.as_deref(), Some("[v](../compartida/sub/x2.md)"));
    }

    #[test]
    fn la_nota_movida_recalcula_sus_enlaces_relativos() {
        let antes = v(&["compartida/x.md", "compartida/otra.md"]);
        let despues = v(&["compartida/sub/x.md", "compartida/otra.md"]);
        let r = reescribir_tras_mover(
            "[o](otra.md) [[otra]] [yo](x.md)",
            "compartida/x.md",
            "compartida/sub/x.md",
            "compartida/x.md",
            "compartida/sub/x.md",
            &antes,
            &despues,
        );
        assert_eq!(r.texto.as_deref(), Some("[o](../otra.md) [[otra]] [yo](x.md)"));
    }

    #[test]
    fn los_ambiguos_no_se_tocan_y_se_avisan() {
        let antes = v(&["compartida/a/x.md", "compartida/b/x.md", "claude-code/n.md"]);
        let despues = v(&["compartida/a/y.md", "compartida/b/x.md", "claude-code/n.md"]);
        let r = reescribir_tras_mover("[[x]]", "claude-code/n.md", "claude-code/n.md", "compartida/a/x.md", "compartida/a/y.md", &antes, &despues);
        assert_eq!(r, Reescrita { texto: None, ambiguos: true });
        // Desde la misma carpeta no es ambiguo.
        let r = reescribir_tras_mover(
            "[[x]]",
            "compartida/a/n.md",
            "compartida/a/n.md",
            "compartida/a/x.md",
            "compartida/a/y.md",
            &v(&["compartida/a/x.md", "compartida/b/x.md", "compartida/a/n.md"]),
            &v(&["compartida/a/y.md", "compartida/b/x.md", "compartida/a/n.md"]),
        );
        assert_eq!(r.texto.as_deref(), Some("[[y]]"));
    }
}
