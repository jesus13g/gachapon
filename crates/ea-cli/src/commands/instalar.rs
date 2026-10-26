//! `ea instalar` y `ea desinstalar` (spec 023): la instalación en un paso.
//! Detecta, ofrece las dependencias externas una a una, enseña el plan
//! completo y lo aplica tras **una** confirmación escrita en una terminal
//! real; al final comprueba que los servicios responden y dice lo que queda
//! en manos del usuario. Los planes son de `ea_acciones::instalacion`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use ea_acciones::instalacion::dependencias::{self, Dependencia, Estado};
use ea_acciones::instalacion::origen::{self, nombre_binario, BINARIO_ESCRITORIO};
use ea_acciones::instalacion::wsl::{self, EstadoWsl};
use ea_acciones::instalacion::{self, Contexto, Origen, Sonda, SondaReal};
use ea_acciones::servicios::{aplicar, correr_orden, Entorno, Paso};
use ea_acciones::Canal;
use ea_ajustes::Fuentes;
use ea_plataforma::Sistema;
use serde_json::Value;

use super::{preguntar, respuesta_confirma};

/// La vía de Windows (spec 023, D-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Wsl,
    Nativa,
}

impl Via {
    pub fn desde(texto: &str) -> Result<Via> {
        match texto.trim().to_lowercase().as_str() {
            "wsl" | "wsl2" => Ok(Via::Wsl),
            "nativa" => Ok(Via::Nativa),
            otro => bail!("vía desconocida {otro:?}: `wsl` o `nativa`"),
        }
    }
}

/// Opciones de `ea instalar`.
#[derive(Debug, Clone, Default)]
pub struct Opciones {
    pub via: Option<Via>,
    pub distro: String,
    pub sin_escritorio: bool,
    pub sin_dependencias: bool,
    /// Lo pasa la entrada `RunOnce` tras el reinicio de WSL.
    pub continuar: bool,
}

/// La frase que hay que escribir para la vía nativa de Windows (art. VIII).
const FRASE_SIN_SANDBOX: &str = "sin sandbox";

/// ¿Corremos dentro de WSL? (`/proc/version` lo dice.)
fn dentro_de_wsl() -> bool {
    std::fs::read_to_string("/proc/version").is_ok_and(|v| v.to_lowercase().contains("microsoft"))
}

/// Lo primero: una persona delante de un teclado y no `root` (los servicios
/// son del usuario).
fn comprobar_terminal_y_usuario() -> Result<()> {
    Canal::terminal_interactiva()?;
    if ea_plataforma::usuario::uid() == Some(0) {
        bail!("no ejecutes `ea instalar` como root: los servicios, los ajustes y la app son de tu usuario (lo que necesite sudo lo pedirá él)");
    }
    Ok(())
}

/// El contexto de esta máquina.
fn contexto(sin_escritorio: bool) -> Result<Contexto> {
    let sistema = Sistema::actual();
    let exe = std::env::current_exe().context("no se pudo saber dónde está este binario")?;
    let exes_origen = exe.parent().context("el binario no tiene carpeta")?.to_path_buf();
    let home = ea_plataforma::usuario::home().context("no se pudo saber la carpeta de tu usuario")?;
    let origen = origen::origen_de(&exes_origen);
    let mut destinos = origen::destinos(sistema, &origen, &home, |n| std::env::var(n).ok());
    if matches!(origen, Origen::Clon { .. }) {
        // `target/debug` o `target/release`: el que corre.
        destinos.bin = exes_origen.clone();
    }
    let ajustes = ea_ajustes::cargar();
    let hay_escritorio = exes_origen.join(nombre_binario(sistema, BINARIO_ESCRITORIO)).is_file()
        || matches!(&origen, Origen::Clon { repo } if repo.join("escritorio/target/release").join(nombre_binario(sistema, BINARIO_ESCRITORIO)).is_file());
    Ok(Contexto {
        sistema,
        origen,
        exes_origen,
        destinos,
        skills_dir: ea_ajustes::expandir_tilde(&ajustes.agente.skills.dir),
        puerto_mcp: ajustes.red.puerto_mcp,
        con_escritorio: !sin_escritorio && !dentro_de_wsl() && hay_escritorio,
        home,
    })
}

/// Lanza una orden heredando la terminal (para `sudo`, instaladores que
/// preguntan, UAC…).
fn correr_interactiva(argv: &[String]) -> Result<(), String> {
    let (prog, args) = argv.split_first().ok_or("orden vacía")?;
    let estado = std::process::Command::new(prog).args(args).status().map_err(|e| format!("no se pudo ejecutar {prog}: {e}"))?;
    if estado.success() { Ok(()) } else { Err(format!("terminó con {estado}")) }
}

fn mostrar_pasos(pasos: &[Paso]) {
    for p in pasos {
        let nota = match p {
            Paso::Escribir { ruta, .. } | Paso::EscribirUtf16 { ruta, .. } | Paso::EscribirBytes { ruta, .. } | Paso::Copiar { destino: ruta, .. }
                if ruta.exists() =>
            {
                " (sustituye el que hay)"
            }
            _ => "",
        };
        println!("  - {}{nota}", p.describir());
    }
}

fn describir_origen(ctx: &Contexto) -> String {
    match &ctx.origen {
        Origen::Paquete => format!("paquete en {} → binarios en {}", ctx.exes_origen.display(), ctx.destinos.bin.display()),
        Origen::Clon { repo } => format!("clon del repo {} (modo desarrollo: los binarios se quedan en {})", repo.display(), ctx.destinos.bin.display()),
    }
}

/// Las dependencias externas: se enseñan y se ofrece cada una con su
/// propia pregunta. Devuelve las notas que quedan para el final.
fn ofrecer_dependencias(ctx: &Contexto, sonda: &dyn Sonda, solo: Option<&[&str]>) -> Result<Vec<String>> {
    let deps: Vec<Dependencia> =
        dependencias::detectar(ctx, sonda).into_iter().filter(|d| solo.is_none_or(|ids| ids.contains(&d.id))).collect();
    if deps.is_empty() {
        return Ok(vec![]);
    }
    println!("Dependencias:");
    for d in &deps {
        let estado = match &d.estado {
            Estado::Presente(v) if v.is_empty() => "sí".to_string(),
            Estado::Presente(v) => format!("sí ({v})"),
            Estado::Falta if d.obligatoria => "FALTA".to_string(),
            Estado::Falta => "falta".to_string(),
        };
        println!("  {:<22} {estado:<28} {}", d.id, d.descripcion);
    }
    let mut notas = Vec::new();
    for d in &deps {
        if let (Estado::Falta, Some(oferta)) = (&d.estado, &d.oferta) {
            println!("\n{}: {}", d.id, oferta.explicacion);
            mostrar_pasos(&oferta.pasos);
            if oferta.admin {
                println!("  (pedirá tu contraseña de administrador o la confirmación de UAC)");
            }
            let r = preguntar("  ¿Instalarlo ahora? [s/N]: ")?;
            if respuesta_confirma(false, d.id, &r) {
                match aplicar(&oferta.pasos, &mut |argv| correr_interactiva(argv)) {
                    Ok(_) => println!("  Hecho."),
                    Err(e) => {
                        println!("  No se pudo: {e}");
                        notas.push(format!("{}: no se pudo instalar ({e})", d.id));
                    }
                }
            } else if d.obligatoria {
                notas.push(format!("{}: falta y es necesario ({})", d.id, d.descripcion));
            }
        }
        if let Some(n) = &d.nota {
            notas.push(format!("{}: {n}", d.id));
        }
    }
    println!();
    Ok(notas)
}

/// Fija ajustes como `ea ajustes establecer` (canal terminal) y lo apunta
/// en el diario.
fn fijar_ajustes(cambios: &[(&str, Value)]) -> Result<()> {
    let f = Fuentes::sistema();
    let lote: Vec<(String, Option<Value>)> = cambios.iter().map(|(k, v)| ((*k).to_string(), Some(v.clone()))).collect();
    let aplicados = ea_ajustes::establecer(&f, ea_ajustes::Canal::Terminal, None, &lote)?;
    super::ajustes::apuntar(&f, &aplicados);
    Ok(())
}

/// Espera a que responda una URL, hasta un tope.
fn esperar(url: &str, cualquier_codigo: bool, tope: Duration) -> bool {
    let inicio = Instant::now();
    loop {
        let ok = if cualquier_codigo {
            instalacion::http_responde(url, Duration::from_secs(2))
        } else {
            instalacion::http_ok(url, Duration::from_secs(2))
        };
        if ok {
            return true;
        }
        if inicio.elapsed() >= tope {
            return false;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Abre la app sin atarla a esta terminal.
fn abrir_app(binario: &Path) -> Result<()> {
    let mut c = std::process::Command::new(binario);
    c.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    ea_plataforma::proceso::preparar_grupo(&mut c);
    c.spawn().with_context(|| format!("no se pudo abrir {}", binario.display()))?;
    Ok(())
}

/// `ea instalar`.
pub fn instalar(op: Opciones) -> Result<()> {
    comprobar_terminal_y_usuario()?;
    let sistema = Sistema::actual();
    let mut sin_sandbox = false;
    if sistema == Sistema::Windows {
        let via = match op.via {
            Some(v) => v,
            None => {
                println!("En Windows hay dos vías:");
                println!("  1) WSL2 (recomendada): la parte de Linux dentro de WSL, con sandbox, y la app nativa en Windows.");
                println!("  2) Nativa: todo en Windows, pero el ejecutor trabaja SIN sandbox (sin aislamiento).");
                match preguntar("  ¿Cuál? [1/2]: ")?.as_str() {
                    "1" => Via::Wsl,
                    "2" => Via::Nativa,
                    _ => {
                        println!("Cancelado: no se ha hecho nada.");
                        return Ok(());
                    }
                }
            }
        };
        match via {
            Via::Wsl => return instalar_wsl(&op),
            Via::Nativa => {
                println!("\nEn Windows nativo no hay sandbox: el agente ejecutor podrá escribir en cualquier sitio que tu usuario pueda.");
                let r = preguntar(&format!("  Para seguir así escribe «{FRASE_SIN_SANDBOX}» (otra cosa cancela): "))?;
                if r.trim().to_lowercase() != FRASE_SIN_SANDBOX {
                    println!("Cancelado: no se ha hecho nada.");
                    return Ok(());
                }
                sin_sandbox = true;
            }
        }
    }

    let ctx = contexto(op.sin_escritorio)?;
    let sonda = SondaReal;
    let entorno = Entorno::usuario().map_err(anyhow::Error::msg)?;
    let faltan: Vec<PathBuf> = origen::BINARIOS
        .iter()
        .map(|b| ctx.exes_origen.join(nombre_binario(ctx.sistema, b)))
        .filter(|p| !p.is_file())
        .collect();
    if !faltan.is_empty() {
        let lista = faltan.iter().map(|f| f.display().to_string()).collect::<Vec<_>>().join(", ");
        bail!("faltan binarios junto a este `ea` ({lista}); compílalos con `cargo build --release` o usa un paquete completo");
    }

    println!("Origen:    {}", describir_origen(&ctx));
    println!("Sistema:   {}\n", ctx.sistema.nombre());
    let notas = if op.sin_dependencias { vec![] } else { ofrecer_dependencias(&ctx, &sonda, None)? };

    let ajustes_toml = Fuentes::sistema().fichero();
    let pasos = instalacion::plan_instalar_todo(&ctx, &entorno, &ajustes_toml, &sonda).map_err(anyhow::Error::msg)?;
    println!("ea instalar hará esto:");
    mostrar_pasos(&pasos);
    if ctx.con_escritorio {
        println!("  - poner escritorio.al_iniciar = true (la app arranca con la sesión, en la bandeja)");
    }
    if sin_sandbox {
        println!("  - poner agente.sandbox = false");
    }
    let r = preguntar("\n  ¿Hacerlo ahora? [s/N]: ")?;
    if !respuesta_confirma(false, "instalar", &r) {
        println!("Cancelado: no se ha hecho nada.");
        return Ok(());
    }
    let hecho = aplicar(&pasos, &mut |argv| correr_orden(argv)).map_err(anyhow::Error::msg)?;
    println!("Hecho ({} pasos).", hecho.len());

    let escritorio = ctx.destinos.bin.join(nombre_binario(ctx.sistema, BINARIO_ESCRITORIO));
    let mut cambios: Vec<(&str, Value)> = Vec::new();
    if sin_sandbox {
        cambios.push(("agente.sandbox", Value::Bool(false)));
    }
    if ctx.con_escritorio {
        cambios.push((ea_ajustes::CLAVE_AL_INICIAR, Value::Bool(true)));
    }
    if !cambios.is_empty() {
        fijar_ajustes(&cambios)?;
    }
    if ctx.con_escritorio {
        // Con el binario instalado, no con el del paquete temporal.
        if let Err(e) = ea_ajustes::aplicar_al_iniciar(true, Some(&escritorio)) {
            eprintln!("aviso: {e}");
        }
    }

    verificar(&ctx, notas, true)?;
    if ctx.con_escritorio && escritorio.is_file() {
        let r = preguntar("\n  ¿Abrir la app ahora? [S/n]: ")?;
        if r.is_empty() || respuesta_confirma(false, "abrir", &r) {
            abrir_app(&escritorio)?;
        }
    }
    Ok(())
}

/// Enseña los pasos, pide la confirmación y los aplica con la consola
/// heredada (UAC, `sudo` y la confirmación de `ea instalar` dentro de WSL
/// necesitan la terminal). `false`: el usuario canceló.
fn confirmar_y_aplicar(pasos: &[Paso], extra: &[String]) -> Result<bool> {
    mostrar_pasos(pasos);
    for e in extra {
        println!("  - {e}");
    }
    let r = preguntar("\n  ¿Hacerlo ahora? [s/N]: ")?;
    if !respuesta_confirma(false, "instalar", &r) {
        println!("Cancelado: no se ha hecho nada.");
        return Ok(false);
    }
    let hecho = aplicar(pasos, &mut |argv| correr_interactiva(argv)).map_err(anyhow::Error::msg)?;
    println!("Hecho ({} pasos).", hecho.len());
    Ok(true)
}

/// La vía WSL2 de Windows (spec 023, decisión 9): WSL (con reinicio si
/// falta), la distribución con `systemd` y `linger`, la parte de Linux
/// dentro con `instalar.sh --sin-escritorio` y, en Windows, solo la app.
fn instalar_wsl(op: &Opciones) -> Result<()> {
    let distro = if op.distro.trim().is_empty() { wsl::DISTRO_POR_DEFECTO.to_string() } else { op.distro.trim().to_string() };
    if !wsl::distro_valida(&distro) {
        bail!("nombre de distribución no válido: {distro:?} (letras, cifras, «.», «_» o «-»)");
    }
    let sonda = SondaReal;
    let ctx = contexto(op.sin_escritorio)?;
    if !ctx.con_escritorio {
        bail!("falta {} junto a este ea.exe: en la vía WSL la app es lo único que se instala en Windows", nombre_binario(ctx.sistema, BINARIO_ESCRITORIO));
    }
    let wslconfig = sonda.leer(&ctx.home.join(".wslconfig"));
    if wsl::reenvio_localhost_desactivado(wslconfig.as_deref()) {
        println!("Aviso: %USERPROFILE%\\.wslconfig tiene localhostForwarding=false; la app de Windows no verá el panel de WSL hasta que lo quites.\n");
    }
    println!("Vía WSL2, distribución {distro}.\n");

    let estado = wsl::estado_wsl(&sonda, &distro);
    let continuar = ctx.destinos.bin.join(nombre_binario(ctx.sistema, "ea"));
    if matches!(estado, EstadoWsl::SinWsl | EstadoWsl::SinDistro) {
        if op.continuar {
            bail!("tras el reinicio sigue sin estar la distribución {distro} (`wsl.exe -l -q`); instálala con `wsl --install -d {distro}` y vuelve a ejecutar el instalador");
        }
        println!("Falta {}: se instala con permiso de administrador (UAC) y Windows pedirá reiniciar.", if estado == EstadoWsl::SinWsl { "WSL2" } else { "la distribución" });
        println!("Tras el reinicio, al iniciar sesión, el instalador sigue solo.\n");
        println!("ea instalar hará esto:");
        let mut pasos = wsl::plan_escritorio_windows(&ctx, &distro);
        pasos.extend(wsl::plan_instalar_wsl(&distro, &continuar));
        if confirmar_y_aplicar(&pasos, &[])? {
            println!("\nReinicia Windows para terminar de instalar WSL. Al volver a entrar, se abrirá el instalador para seguir.");
        }
        return Ok(());
    }

    // Una distribución recién instalada no tiene usuario: hay que crearlo
    // antes de `linger` y de instalar (si no, todo quedaría como root).
    let uid = sonda.salida(&["wsl.exe", "-d", &distro, "--exec", "id", "-u"]).map(|t| wsl::limpiar_salida_wsl(&t).trim().to_string());
    if uid.as_deref() == Some("0") || uid.is_none() {
        println!("Hay que crear tu usuario de Linux en {distro}. Se abre ahora: cuando veas el símbolo del sistema, escribe `exit`.");
        let r = preguntar("  ¿Abrirla? [S/n]: ")?;
        if !(r.is_empty() || respuesta_confirma(false, "abrir", &r)) {
            println!("Cancelado: no se ha hecho nada.");
            return Ok(());
        }
        correr_interactiva(&["wsl.exe".into(), "-d".into(), distro.clone()]).map_err(anyhow::Error::msg)?;
    }

    // En Windows solo hace falta el WebView de la app; lo demás está en WSL.
    let notas = if op.sin_dependencias { vec![] } else { ofrecer_dependencias(&ctx, &sonda, Some(&["webview2"]))? };

    println!("ea instalar hará esto:");
    let mut pasos = wsl::plan_preparar_distro(&distro, &estado);
    pasos.extend(wsl::plan_linux_dentro(&distro, wsl::URL_INSTALAR_SH));
    pasos.extend(wsl::plan_escritorio_windows(&ctx, &distro));
    pasos.extend(instalacion::menu::plan_menu(&ctx));
    let extra = [
        format!("poner escritorio.wsl = {distro} (la app lee la clave del panel de WSL y mantiene viva la distribución)"),
        "poner escritorio.al_iniciar = true (la app arranca con la sesión, en la bandeja)".to_string(),
    ];
    if !confirmar_y_aplicar(&pasos, &extra)? {
        return Ok(());
    }
    fijar_ajustes(&[("escritorio.wsl", Value::String(distro.clone())), (ea_ajustes::CLAVE_AL_INICIAR, Value::Bool(true))])?;
    let escritorio = ctx.destinos.bin.join(nombre_binario(ctx.sistema, BINARIO_ESCRITORIO));
    if let Err(e) = ea_ajustes::aplicar_al_iniciar(true, Some(&escritorio)) {
        eprintln!("aviso: {e}");
    }
    // Los agentes de la vía WSL están dentro: su resumen ya lo dio el
    // `ea instalar` de Linux.
    verificar(&ctx, notas, false)?;
    let r = preguntar("\n  ¿Abrir la app ahora? [S/n]: ")?;
    if r.is_empty() || respuesta_confirma(false, "abrir", &r) {
        abrir_app(&escritorio)?;
    }
    Ok(())
}

/// Comprueba que los servicios responden, enseña los agentes y lo que queda
/// en manos del usuario.
fn verificar(ctx: &Contexto, notas: Vec<String>, con_agentes: bool) -> Result<()> {
    let ajustes = ea_ajustes::cargar();
    let tope = Duration::from_secs(30);
    println!("\nComprobando los servicios (hasta {} s)…", tope.as_secs());
    let web = esperar(&format!("http://127.0.0.1:{}/api/estado", ajustes.red.puerto_web), false, tope);
    let mcp = esperar(&format!("http://127.0.0.1:{}/", ctx.puerto_mcp), true, tope);
    let si = |b: bool| if b { "responde" } else { "NO responde (mira `ea servicios ver`)" };
    println!("  ea-web  127.0.0.1:{:<6} {}", ajustes.red.puerto_web, si(web));
    println!("  ea-mcp  127.0.0.1:{:<6} {}", ctx.puerto_mcp, si(mcp));
    println!();
    if con_agentes && let Err(e) = super::agentes::listar(false) {
        eprintln!("aviso: no se pudo enseñar los agentes: {e:#}");
    }
    if !notas.is_empty() {
        println!("\nQueda en tus manos:");
        for n in &notas {
            println!("  - {n}");
        }
    }
    if ctx.sistema != Sistema::Windows && matches!(ctx.origen, Origen::Paquete) {
        println!("\n`ea` queda en {}; abre una terminal nueva si aún no lo encuentra.", ctx.home.join(".local/bin/ea").display());
    }
    Ok(())
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn la_via_se_lee_y_rechaza_lo_demas() {
        assert_eq!(Via::desde("WSL").unwrap(), Via::Wsl);
        assert_eq!(Via::desde("wsl2").unwrap(), Via::Wsl);
        assert_eq!(Via::desde(" nativa ").unwrap(), Via::Nativa);
        assert!(Via::desde("linux").is_err());
    }
}

/// `ea desinstalar`.
pub fn desinstalar() -> Result<()> {
    comprobar_terminal_y_usuario()?;
    let ctx = contexto(false)?;
    let ctx = Contexto { con_escritorio: true, ..ctx };
    let sonda = SondaReal;
    let entorno = Entorno::usuario().map_err(anyhow::Error::msg)?;
    let pasos = instalacion::plan_desinstalar_todo(&ctx, &entorno, &sonda).map_err(anyhow::Error::msg)?;
    println!("ea desinstalar hará esto en {}:", ctx.sistema.nombre());
    mostrar_pasos(&pasos);
    println!("  - poner escritorio.al_iniciar = false (quita el autoarranque de la app)");
    let r = preguntar("\n  ¿Hacerlo ahora? [s/N]: ")?;
    if !respuesta_confirma(false, "desinstalar", &r) {
        println!("Cancelado: no se ha hecho nada.");
        return Ok(());
    }
    // Primero el autoarranque: después ya no estaría el binario.
    if let Err(e) = fijar_ajustes(&[(ea_ajustes::CLAVE_AL_INICIAR, Value::Bool(false))]) {
        eprintln!("aviso: {e:#}");
    }
    if let Err(e) = ea_ajustes::aplicar_al_iniciar(false, None) {
        eprintln!("aviso: {e}");
    }
    let hecho = aplicar(&pasos, &mut |argv| correr_orden(argv)).map_err(anyhow::Error::msg)?;
    println!("Hecho ({} pasos).", hecho.len());
    let a = ea_ajustes::cargar();
    let f = Fuentes::sistema();
    println!("\nTus datos se quedan (no se ha borrado nada de esto):");
    println!("  ajustes        {}", f.fichero().display());
    println!("  base de datos  {}", a.rutas.db.display());
    println!("  bóveda         {}", a.rutas.boveda.display());
    println!("  bus y estado   {}", a.rutas.estado.display());
    if ctx.sistema == Sistema::Windows && matches!(ctx.origen, Origen::Paquete) {
        println!("\nWindows no deja borrar el `ea.exe` que está corriendo: bórralo a mano ({}).", ctx.destinos.bin.display());
    }
    Ok(())
}
