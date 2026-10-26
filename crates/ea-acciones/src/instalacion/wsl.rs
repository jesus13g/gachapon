//! La vía WSL2 de Windows (spec 023, decisión 9): instalar WSL si falta,
//! preparar la distribución (`systemd` y `linger`), instalar la parte de
//! Linux dentro y la app de escritorio nativa en Windows.
//!
//! Solo planes puros: se prueban en cualquier sistema. Varios pasos son
//! interactivos (UAC, `sudo`, la confirmación de `ea instalar` dentro de
//! WSL), así que quien los aplique tiene que ejecutarlos **con la consola
//! heredada**, no con la salida capturada.

use std::path::Path;

use super::{Contexto, Origen, Sonda};
use crate::servicios::Paso;

/// El `instalar.sh` publicado en la web (GitHub Pages).
pub const URL_INSTALAR_SH: &str = "https://jesus13g.github.io/gachapon/instalar.sh";

/// La distribución que se instala si el usuario no elige otra.
pub const DISTRO_POR_DEFECTO: &str = "Ubuntu";

/// Cómo está WSL en esta máquina.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EstadoWsl {
    /// No hay WSL (o `wsl.exe --status` falla).
    SinWsl,
    /// Hay WSL, pero no la distribución pedida.
    SinDistro,
    /// La distribución existe; `systemd` dice si arranca con él.
    Distro { nombre: String, systemd: bool },
}

/// ¿Es un nombre de distribución aceptable (`[A-Za-z0-9._-]{1,64}`)? Los
/// planes lo dan por hecho: quien los llama lo comprueba antes.
pub fn distro_valida(d: &str) -> bool {
    !d.is_empty() && d.len() <= 64 && d.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// `wsl.exe` escribe en UTF-16LE; la sonda lo lee como UTF-8 con pérdida,
/// así que quedan NUL (y a veces un BOM) entre los caracteres.
pub fn limpiar_salida_wsl(texto: &str) -> String {
    texto.chars().filter(|c| *c != '\0' && *c != '\u{feff}').collect()
}

/// Las distribuciones de `wsl.exe -l -q`, una por línea.
pub fn distros(salida: &str) -> Vec<String> {
    limpiar_salida_wsl(salida).lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect()
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn ejecutar(argv: Vec<String>, tolerar: bool) -> Paso {
    Paso::Ejecutar { argv, tolerar, reintentos: 0 }
}

/// Mira WSL y la distribución `distro`.
pub fn estado_wsl(sonda: &dyn Sonda, distro: &str) -> EstadoWsl {
    if sonda.binario("wsl.exe").is_none() || sonda.salida(&["wsl.exe", "--status"]).is_none() {
        return EstadoWsl::SinWsl;
    }
    let lista = sonda.salida(&["wsl.exe", "-l", "-q"]).map(|t| distros(&t)).unwrap_or_default();
    let Some(nombre) = lista.into_iter().find(|n| n.eq_ignore_ascii_case(distro)) else {
        return EstadoWsl::SinDistro;
    };
    let systemd = sonda
        .salida(&["wsl.exe", "-d", &nombre, "--exec", "sh", "-c", "test -d /run/systemd/system && echo si"])
        .is_some_and(|t| limpiar_salida_wsl(&t).trim() == "si");
    EstadoWsl::Distro { nombre, systemd }
}

/// Instalar WSL con la distribución (con elevación: UAC) y dejar una
/// entrada `RunOnce` del usuario para seguir tras el reinicio que pide
/// Windows. `continuar` es el `ea.exe` ya copiado a su carpeta estable.
pub fn plan_instalar_wsl(distro: &str, continuar: &Path) -> Vec<Paso> {
    debug_assert!(distro_valida(distro));
    let d = distro.replace('\'', "''");
    vec![
        ejecutar(
            vec![
                "powershell.exe".into(),
                "-NoProfile".into(),
                "-Command".into(),
                format!("Start-Process -FilePath wsl.exe -ArgumentList '--install','-d','{d}','--no-launch' -Verb RunAs -Wait"),
            ],
            false,
        ),
        ejecutar(
            vec![
                "reg.exe".into(),
                "add".into(),
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\RunOnce".into(),
                "/v".into(),
                "gachapon-instalar".into(),
                "/t".into(),
                "REG_SZ".into(),
                "/d".into(),
                format!("\"{}\" instalar --via wsl --distro {distro} --continuar", continuar.display()),
                "/f".into(),
            ],
            false,
        ),
    ]
}

/// Guion (como root) que deja `systemd=true` en `/etc/wsl.conf` sin
/// duplicar nada: cambia una línea `systemd=` que ya exista, añade la clave
/// bajo un `[boot]` que ya exista o, si no hay ninguno, añade la sección.
pub const GUION_SYSTEMD: &str = "f=/etc/wsl.conf; \
if grep -qs '^[[:space:]]*systemd[[:space:]]*=' \"$f\"; then sed -i 's/^[[:space:]]*systemd[[:space:]]*=.*/systemd=true/' \"$f\"; \
elif grep -qs '^[[:space:]]*\\[boot\\]' \"$f\"; then sed -i '/^[[:space:]]*\\[boot\\]/a systemd=true' \"$f\"; \
else printf '\\n[boot]\\nsystemd=true\\n' >> \"$f\"; fi";

/// Guion (del usuario por defecto) que deja sus servicios `systemd --user`
/// en marcha aunque no haya sesión abierta (`linger`); pide la contraseña
/// de `sudo` en la consola.
pub const GUION_LINGER: &str = "sudo loginctl enable-linger \"$(id -un)\"";

/// Preparar la distribución: `systemd` (y reiniciar WSL para que arranque
/// con él) si no lo tiene, y `linger` para el usuario (idempotente).
pub fn plan_preparar_distro(distro: &str, estado: &EstadoWsl) -> Vec<Paso> {
    let mut p = Vec::new();
    let systemd = matches!(estado, EstadoWsl::Distro { systemd: true, .. });
    if !systemd {
        p.push(ejecutar(s(&["wsl.exe", "-d", distro, "-u", "root", "--exec", "sh", "-c", GUION_SYSTEMD]), false));
        p.push(ejecutar(s(&["wsl.exe", "--shutdown"]), false));
    }
    p.push(ejecutar(s(&["wsl.exe", "-d", distro, "--exec", "sh", "-c", GUION_LINGER]), false));
    p
}

/// Instalar la parte de Linux dentro de la distribución con el
/// `instalar.sh` publicado, sin la app de escritorio (va en Windows).
pub fn plan_linux_dentro(distro: &str, url_script: &str) -> Vec<Paso> {
    let url = url_script.replace('\'', "");
    vec![ejecutar(
        vec![
            "wsl.exe".into(),
            "-d".into(),
            distro.into(),
            "--exec".into(),
            "bash".into(),
            "-lc".into(),
            format!("curl -fsSL '{url}' | sh -s -- --sin-escritorio"),
        ],
        false,
    )]
}

/// La app nativa de Windows: con origen paquete, `ea-escritorio.exe` y
/// `ea.exe` a la carpeta estable. La entrada del menú Inicio, el
/// autoarranque y `escritorio.wsl` los pone el flujo común (`menu.rs` y
/// los ajustes), no este plan.
pub fn plan_escritorio_windows(ctx: &Contexto, distro: &str) -> Vec<Paso> {
    debug_assert!(distro_valida(distro));
    match ctx.origen {
        Origen::Paquete => ["ea-escritorio.exe", "ea.exe"]
            .iter()
            .map(|b| Paso::Copiar { origen: ctx.exes_origen.join(b), destino: ctx.destinos.bin.join(b) })
            .collect(),
        Origen::Clon { .. } => Vec::new(),
    }
}

/// ¿Desactiva `%USERPROFILE%\.wslconfig` el reenvío de `localhost`? Sin él
/// la app de Windows no ve el panel que corre en WSL.
pub fn reenvio_localhost_desactivado(wslconfig: Option<&str>) -> bool {
    let Some(texto) = wslconfig else { return false };
    let mut en_wsl2 = false;
    let mut desactivado = false;
    for linea in texto.lines() {
        let l = linea.split(['#', ';']).next().unwrap_or("").trim();
        if l.starts_with('[') {
            en_wsl2 = l.eq_ignore_ascii_case("[wsl2]");
            continue;
        }
        if !en_wsl2 {
            continue;
        }
        if let Some((k, v)) = l.split_once('=')
            && k.trim().eq_ignore_ascii_case("localhostForwarding")
        {
            desactivado = v.trim().eq_ignore_ascii_case("false");
        }
    }
    desactivado
}

#[cfg(test)]
mod pruebas {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use super::super::Destinos;
    use super::*;

    #[derive(Default)]
    struct Falsa {
        bins: Vec<&'static str>,
        salidas: HashMap<String, String>,
    }

    impl Sonda for Falsa {
        fn binario(&self, nombre: &str) -> Option<PathBuf> {
            self.bins.contains(&nombre).then(|| PathBuf::from(format!("C:/Windows/System32/{nombre}")))
        }
        fn existe(&self, _: &Path) -> bool {
            false
        }
        fn leer(&self, _: &Path) -> Option<String> {
            None
        }
        fn salida(&self, argv: &[&str]) -> Option<String> {
            self.salidas.get(&argv.join(" ")).cloned()
        }
        fn http_ok(&self, _: &str) -> bool {
            false
        }
    }

    /// Como la escribe `wsl.exe`: UTF-16LE leído como UTF-8.
    fn utf16(t: &str) -> String {
        let mut s = String::from('\u{feff}');
        for c in t.chars() {
            s.push(c);
            s.push('\0');
        }
        s
    }

    const SYSTEMD: &str = "wsl.exe -d Ubuntu --exec sh -c test -d /run/systemd/system && echo si";

    #[test]
    fn sin_wsl_exe_o_con_status_fallido_no_hay_wsl() {
        assert_eq!(estado_wsl(&Falsa::default(), "Ubuntu"), EstadoWsl::SinWsl);
        let f = Falsa { bins: vec!["wsl.exe"], ..Default::default() };
        assert_eq!(estado_wsl(&f, "Ubuntu"), EstadoWsl::SinWsl);
    }

    #[test]
    fn distingue_sin_distro_y_distro_con_o_sin_systemd() {
        let mut f = Falsa { bins: vec!["wsl.exe"], ..Default::default() };
        f.salidas.insert("wsl.exe --status".into(), utf16("Versión predeterminada: 2"));
        f.salidas.insert("wsl.exe -l -q".into(), utf16("Debian\r\n"));
        assert_eq!(estado_wsl(&f, "Ubuntu"), EstadoWsl::SinDistro);
        f.salidas.insert("wsl.exe -l -q".into(), utf16("Debian\r\nUbuntu\r\n"));
        assert_eq!(estado_wsl(&f, "ubuntu"), EstadoWsl::Distro { nombre: "Ubuntu".into(), systemd: false });
        f.salidas.insert(SYSTEMD.into(), utf16("si\n"));
        assert_eq!(estado_wsl(&f, "Ubuntu"), EstadoWsl::Distro { nombre: "Ubuntu".into(), systemd: true });
    }

    #[test]
    fn limpia_la_salida_utf16_de_wsl() {
        assert_eq!(distros(&utf16("Ubuntu\r\nkali-linux\r\n\r\n")), vec!["Ubuntu", "kali-linux"]);
        assert_eq!(limpiar_salida_wsl(&utf16("si")), "si");
    }

    #[test]
    fn nombres_de_distro() {
        assert!(distro_valida("Ubuntu-24.04"));
        assert!(!distro_valida(""));
        assert!(!distro_valida("a b"));
        assert!(!distro_valida("x';calc;'"));
        assert!(!distro_valida(&"a".repeat(65)));
    }

    #[test]
    fn instalar_wsl_eleva_y_deja_runonce_para_seguir() {
        let p = plan_instalar_wsl("Ubuntu", Path::new(r"C:\Users\ana\AppData\Local\Programs\gachapon\ea.exe"));
        assert_eq!(p.len(), 2);
        let Paso::Ejecutar { argv, tolerar, .. } = &p[0] else { panic!() };
        assert!(!tolerar);
        assert_eq!(argv[0], "powershell.exe");
        assert_eq!(argv[3], "Start-Process -FilePath wsl.exe -ArgumentList '--install','-d','Ubuntu','--no-launch' -Verb RunAs -Wait");
        let Paso::Ejecutar { argv, .. } = &p[1] else { panic!() };
        assert_eq!(argv[0], "reg.exe");
        assert!(argv[2].ends_with(r"\RunOnce"));
        assert_eq!(argv[8], r#""C:\Users\ana\AppData\Local\Programs\gachapon\ea.exe" instalar --via wsl --distro Ubuntu --continuar"#);
        assert_eq!(argv.last().unwrap(), "/f");
    }

    #[test]
    fn preparar_la_distro_activa_systemd_solo_si_falta_y_siempre_linger() {
        let sin = plan_preparar_distro("Ubuntu", &EstadoWsl::Distro { nombre: "Ubuntu".into(), systemd: false });
        assert_eq!(sin.len(), 3);
        let Paso::Ejecutar { argv, .. } = &sin[0] else { panic!() };
        assert_eq!(&argv[..6], &s(&["wsl.exe", "-d", "Ubuntu", "-u", "root", "--exec"]));
        assert_eq!(argv.last().unwrap(), GUION_SYSTEMD);
        let Paso::Ejecutar { argv, .. } = &sin[1] else { panic!() };
        assert_eq!(argv, &s(&["wsl.exe", "--shutdown"]));
        let con = plan_preparar_distro("Ubuntu", &EstadoWsl::Distro { nombre: "Ubuntu".into(), systemd: true });
        assert_eq!(con.len(), 1);
        let Paso::Ejecutar { argv, .. } = &con[0] else { panic!() };
        assert_eq!(argv.last().unwrap(), GUION_LINGER);
        assert!(!argv.contains(&"root".to_string()));
    }

    /// El guion de `systemd` en un `sh` de verdad, sobre un fichero
    /// temporal: idempotente y sin duplicar claves.
    #[test]
    fn el_guion_de_systemd_es_idempotente() {
        let Some(sh) = ea_plataforma::rutas::buscar_en_path("sh") else { return };
        if ea_plataforma::rutas::buscar_en_path("sed").is_none() || ea_plataforma::Sistema::actual() == ea_plataforma::Sistema::Windows {
            return;
        }
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("wsl.conf");
        let guion = GUION_SYSTEMD.replace("/etc/wsl.conf", &f.display().to_string());
        let correr = || {
            let ok = std::process::Command::new(&sh).args(["-c", &guion]).status().unwrap().success();
            assert!(ok);
            std::fs::read_to_string(&f).unwrap()
        };
        let contar = |t: &str| t.matches("systemd=true").count();
        // sin fichero
        let t = correr();
        assert_eq!(contar(&t), 1, "{t}");
        assert_eq!(contar(&correr()), 1);
        // con [boot] sin systemd
        std::fs::write(&f, "[boot]\ncommand=echo hola\n[user]\ndefault=ana\n").unwrap();
        let t = correr();
        assert_eq!(contar(&t), 1, "{t}");
        assert!(t.starts_with("[boot]\nsystemd=true\n"), "{t}");
        assert!(t.contains("default=ana"));
        // con systemd=false
        std::fs::write(&f, "[boot]\nsystemd = false\n").unwrap();
        let t = correr();
        assert_eq!(t, "[boot]\nsystemd=true\n");
    }

    #[test]
    fn la_parte_de_linux_va_por_instalar_sh_sin_escritorio() {
        let p = plan_linux_dentro("Ubuntu", URL_INSTALAR_SH);
        let Paso::Ejecutar { argv, .. } = &p[0] else { panic!() };
        assert_eq!(&argv[..6], &s(&["wsl.exe", "-d", "Ubuntu", "--exec", "bash", "-lc"]));
        assert_eq!(argv[6], format!("curl -fsSL '{URL_INSTALAR_SH}' | sh -s -- --sin-escritorio"));
        // una comilla en la URL no rompe el citado
        let p = plan_linux_dentro("Ubuntu", "https://x/'; rm -rf ~; '");
        let Paso::Ejecutar { argv, .. } = &p[0] else { panic!() };
        assert_eq!(argv[6].matches('\'').count(), 2);
    }

    fn contexto(origen: Origen) -> Contexto {
        Contexto {
            sistema: ea_plataforma::Sistema::Windows,
            origen,
            exes_origen: PathBuf::from(r"C:\Temp\gachapon-instalar-1"),
            destinos: Destinos {
                bin: PathBuf::from(r"C:\Users\ana\AppData\Local\Programs\gachapon"),
                flujo: PathBuf::from(r"C:\Users\ana\AppData\Local\Programs\gachapon\flujo"),
                datos: PathBuf::from(r"C:\Users\ana\AppData\Local\Programs\gachapon"),
            },
            home: PathBuf::from(r"C:\Users\ana"),
            skills_dir: PathBuf::from(r"C:\Users\ana\AppData\Roaming\ecosistema-asistente\config\qwen-skills"),
            puerto_mcp: 8765,
            con_escritorio: true,
        }
    }

    #[test]
    fn la_app_de_windows_se_copia_solo_desde_un_paquete() {
        let ctx = contexto(Origen::Paquete);
        let p = plan_escritorio_windows(&ctx, "Ubuntu");
        assert_eq!(
            p,
            vec![
                Paso::Copiar { origen: ctx.exes_origen.join("ea-escritorio.exe"), destino: ctx.destinos.bin.join("ea-escritorio.exe") },
                Paso::Copiar { origen: ctx.exes_origen.join("ea.exe"), destino: ctx.destinos.bin.join("ea.exe") },
            ]
        );
        assert!(plan_escritorio_windows(&contexto(Origen::Clon { repo: PathBuf::from(r"C:\src\gachapon") }), "Ubuntu").is_empty());
    }

    #[test]
    fn detecta_el_reenvio_de_localhost_desactivado() {
        assert!(!reenvio_localhost_desactivado(None));
        assert!(!reenvio_localhost_desactivado(Some("[wsl2]\nmemory=8GB\n")));
        assert!(reenvio_localhost_desactivado(Some("[wsl2]\nlocalhostForwarding = false # sin reenvío\n")));
        assert!(reenvio_localhost_desactivado(Some("[WSL2]\r\nLocalhostForwarding=False\r\n")));
        assert!(!reenvio_localhost_desactivado(Some("[otra]\nlocalhostForwarding=false\n")));
        assert!(!reenvio_localhost_desactivado(Some("[wsl2]\n# localhostForwarding=false\n")));
        assert!(!reenvio_localhost_desactivado(Some("[wsl2]\nlocalhostForwarding=false\nlocalhostForwarding=true\n")));
    }
}
