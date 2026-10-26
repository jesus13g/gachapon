//! `ea` en el `PATH` del usuario (spec 023). Unix: un enlace
//! `~/.local/bin/ea` y, si esa carpeta no está en el `PATH`, un bloque con
//! marcas en `~/.zprofile` (macOS) o `~/.profile` (Linux). Windows: la
//! carpeta de los binarios en el `Path` del usuario, sin pisar lo que hay.

use std::path::{Path, PathBuf};

use ea_plataforma::Sistema;

use super::origen::nombre_binario;
use super::{leer_para_reescribir, Contexto, Sonda};
use crate::servicios::Paso;

pub const MARCA_INICIO: &str = "# gachapon:inicio";
pub const MARCA_FIN: &str = "# gachapon:fin";

/// La línea que se añade al perfil del shell.
const LINEA_PATH: &str = "export PATH=\"$HOME/.local/bin:$PATH\"";

fn dir_local_bin(home: &Path) -> PathBuf {
    home.join(".local").join("bin")
}

/// El perfil del shell de inicio de sesión que se toca. En Linux, si existe
/// `~/.bash_profile` (o `~/.bash_login`), bash no lee `~/.profile`: se usa
/// ese.
pub fn perfil_shell(sistema: Sistema, home: &Path, sonda: &dyn Sonda) -> PathBuf {
    if sistema == Sistema::MacOs {
        return home.join(".zprofile");
    }
    [".bash_profile", ".bash_login"].iter().map(|n| home.join(n)).find(|p| sonda.existe(p)).unwrap_or_else(|| home.join(".profile"))
}

/// ¿Está `dir` en un `PATH` (separador `:` o `;`)? Compara también con `~`
/// y `$HOME` sin expandir, que es como suele quedar en los perfiles.
pub fn en_path(path: &str, dir: &Path, home: &Path, sistema: Sistema) -> bool {
    let sep = if sistema == Sistema::Windows { ';' } else { ':' };
    let objetivo = dir.to_string_lossy().trim_end_matches(['/', '\\']).to_string();
    path.split(sep).any(|e| {
        let e = e.trim().trim_end_matches(['/', '\\']);
        if sistema == Sistema::Windows {
            return e.eq_ignore_ascii_case(&objetivo);
        }
        let expandido = e.strip_prefix("~/").or_else(|| e.strip_prefix("$HOME/")).map(|r| home.join(r).to_string_lossy().into_owned());
        e == objetivo || expandido.as_deref() == Some(objetivo.as_str())
    })
}

/// Añade el bloque con marcas al final del perfil (o lo deja si ya está).
pub fn con_bloque(actual: Option<&str>) -> Option<String> {
    let actual = actual.unwrap_or_default();
    if actual.contains(MARCA_INICIO) {
        return None;
    }
    let mut s = actual.to_string();
    if !s.is_empty() && !s.ends_with('\n') {
        s.push('\n');
    }
    s.push_str(&format!("{MARCA_INICIO}\n{LINEA_PATH}\n{MARCA_FIN}\n"));
    Some(s)
}

/// Quita el bloque con marcas; `None` si no estaba.
pub fn sin_bloque(actual: &str) -> Option<String> {
    let ini = actual.find(MARCA_INICIO)?;
    let fin = actual[ini..].find(MARCA_FIN)? + ini + MARCA_FIN.len();
    let fin = if actual[fin..].starts_with('\n') { fin + 1 } else { fin };
    Some(format!("{}{}", &actual[..ini], &actual[fin..]))
}

/// Una cadena entre comillas simples de PowerShell.
pub fn literal_ps(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn powershell(guion: String) -> Paso {
    Paso::Ejecutar {
        argv: vec!["powershell".into(), "-NoProfile".into(), "-NonInteractive".into(), "-Command".into(), guion],
        tolerar: false,
        reintentos: 0,
    }
}

/// Leer el `Path` del usuario del registro **sin expandir** (`%USERPROFILE%`
/// se queda como está) y escribirlo como `REG_EXPAND_SZ`.
/// `[Environment]::Get/SetEnvironmentVariable` los expandiría y los dejaría
/// fijos. Después se avisa al sistema del cambio (el truco de poner y quitar
/// una variable con `SetEnvironmentVariable`, que difunde `WM_SETTINGCHANGE`).
const PS_LEER_PATH: &str = "$k = Get-Item -Path 'HKCU:\\Environment'; $p = $k.GetValue('Path', '', 'DoNotExpandEnvironmentNames'); if (-not $p) { $p = '' }";
const PS_ESCRIBIR_PATH: &str = "Set-ItemProperty -Path 'HKCU:\\Environment' -Name Path -Value $n -Type ExpandString; \
     [Environment]::SetEnvironmentVariable('GACHAPON_AVISO', '1', 'User'); [Environment]::SetEnvironmentVariable('GACHAPON_AVISO', $null, 'User')";

/// Guion que añade `dir` al `Path` del usuario si falta.
pub fn guion_poner_path(dir: &Path) -> String {
    format!(
        "$d = {}; {PS_LEER_PATH}; \
         if (-not (($p -split ';') | Where-Object {{ $_.TrimEnd('\\') -ieq $d.TrimEnd('\\') }})) {{ \
         $n = if ($p) {{ $p.TrimEnd(';') + ';' + $d }} else {{ $d }}; {PS_ESCRIBIR_PATH} }}",
        literal_ps(&dir.display().to_string())
    )
}

/// Guion que quita `dir` del `Path` del usuario (deja lo demás).
pub fn guion_quitar_path(dir: &Path) -> String {
    format!(
        "$d = {}; {PS_LEER_PATH}; \
         if ($p) {{ $n = (($p -split ';') | Where-Object {{ $_ -and ($_.TrimEnd('\\') -ine $d.TrimEnd('\\')) }}) -join ';'; \
         if ($n -ne $p) {{ {PS_ESCRIBIR_PATH} }} }}",
        literal_ps(&dir.display().to_string())
    )
}

/// Los pasos para que `ea` quede en el `PATH`.
pub fn plan_ruta(ctx: &Contexto, sonda: &dyn Sonda) -> Vec<Paso> {
    let path = sonda.var("PATH").unwrap_or_default();
    if ctx.sistema == Sistema::Windows {
        if en_path(&path, &ctx.destinos.bin, &ctx.home, ctx.sistema) {
            return vec![];
        }
        return vec![powershell(guion_poner_path(&ctx.destinos.bin))];
    }
    let local = dir_local_bin(&ctx.home);
    let mut p = Vec::new();
    if enlace_sustituible(ctx, sonda) {
        p.push(Paso::Enlazar { origen: ctx.destinos.bin.join(nombre_binario(ctx.sistema, "ea")), destino: local.join("ea") });
    }
    if !en_path(&path, &local, &ctx.home, ctx.sistema) {
        let perfil = perfil_shell(ctx.sistema, &ctx.home, sonda);
        if let Ok(actual) = leer_para_reescribir(sonda, &perfil)
            && let Some(contenido) = con_bloque(actual.as_deref())
        {
            p.push(Paso::EditarPersonal { ruta: perfil, contenido });
        }
    }
    p
}

/// ¿Se puede poner (o rehacer) `~/.local/bin/ea`? Sí si no hay nada o si es
/// un enlace a un `ea` de gachapon (una carpeta de binarios propia o el
/// `target/` de un clon); un `ea` ajeno no se pisa.
fn enlace_sustituible(ctx: &Contexto, sonda: &dyn Sonda) -> bool {
    let enlace = dir_local_bin(&ctx.home).join("ea");
    if !sonda.existe(&enlace) {
        return true;
    }
    let Some(destino) = sonda.destino_enlace(&enlace) else { return false };
    let texto = destino.to_string_lossy().replace('\\', "/");
    destino.file_name().is_some_and(|n| n == "ea")
        && (destino.starts_with(&ctx.destinos.bin)
            || texto.contains("/ecosistema-asistente/bin/")
            || texto.contains("/target/release/")
            || texto.contains("/target/debug/"))
}

/// Lo que `plan_ruta` deja sin hacer y el usuario debe saber.
pub fn avisos_ruta(ctx: &Contexto, sonda: &dyn Sonda) -> Vec<String> {
    let mut v = Vec::new();
    if ctx.sistema == Sistema::Windows {
        return v;
    }
    if !enlace_sustituible(ctx, sonda) {
        v.push(format!(
            "{} ya existe y no es de gachapon: no se toca; usa {} o pon un alias",
            dir_local_bin(&ctx.home).join("ea").display(),
            ctx.destinos.bin.join("ea").display()
        ));
    }
    let perfil = perfil_shell(ctx.sistema, &ctx.home, sonda);
    if let Err(e) = leer_para_reescribir(sonda, &perfil) {
        v.push(format!("{e}; añade tú «{LINEA_PATH}»"));
    }
    v
}

/// Los pasos para quitarlo: el enlace (solo si apunta a nuestro `ea`) y el
/// bloque con marcas; en Windows, la carpeta del `Path` del usuario.
pub fn plan_quitar_ruta(ctx: &Contexto, sonda: &dyn Sonda) -> Vec<Paso> {
    if ctx.sistema == Sistema::Windows {
        return vec![powershell(guion_quitar_path(&ctx.destinos.bin))];
    }
    let mut p = Vec::new();
    let enlace = dir_local_bin(&ctx.home).join("ea");
    let nuestro = ctx.destinos.bin.join(nombre_binario(ctx.sistema, "ea"));
    if sonda.destino_enlace(&enlace).is_some_and(|d| d == nuestro) {
        p.push(Paso::Borrar(enlace));
    }
    let perfil = perfil_shell(ctx.sistema, &ctx.home, sonda);
    if let Some(contenido) = leer_para_reescribir(sonda, &perfil).ok().flatten().as_deref().and_then(sin_bloque) {
        p.push(Paso::EditarPersonal { ruta: perfil, contenido });
    }
    p
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn reconoce_la_carpeta_en_el_path() {
        let home = Path::new("/home/ana");
        let d = Path::new("/home/ana/.local/bin");
        assert!(en_path("/usr/bin:/home/ana/.local/bin", d, home, Sistema::Linux));
        assert!(en_path("/usr/bin:/home/ana/.local/bin/", d, home, Sistema::Linux));
        assert!(en_path("~/.local/bin:/usr/bin", d, home, Sistema::Linux));
        assert!(!en_path("/usr/bin:/home/ana/.local/binario", d, home, Sistema::Linux));
        assert!(en_path("C:\\x;c:\\Users\\Ana\\gachapon\\", Path::new("C:\\Users\\ana\\gachapon"), home, Sistema::Windows));
    }

    #[test]
    fn el_bloque_se_pone_una_vez_y_se_quita_entero() {
        let uno = con_bloque(Some("alias l=ls")).unwrap();
        assert!(uno.starts_with("alias l=ls\n# gachapon:inicio\n"));
        assert_eq!(con_bloque(Some(&uno)), None, "no se duplica");
        assert_eq!(sin_bloque(&uno).unwrap(), "alias l=ls\n");
        assert_eq!(sin_bloque("nada"), None);
        assert!(con_bloque(None).unwrap().contains(LINEA_PATH));
    }

    #[test]
    fn el_literal_de_powershell_escapa_comillas() {
        assert_eq!(literal_ps("C:\\O'Brien\\x"), "'C:\\O''Brien\\x'");
        assert!(guion_poner_path(Path::new("C:\\a b")).contains("$d = 'C:\\a b'"));
    }
}
