//! Lo mínimo de XML que necesitan los LaunchAgents de launchd (property list)
//! y las tareas del Programador de tareas de Windows (XML de tarea). Puro:
//! solo construye texto.

/// Escapa un texto para ir dentro de un elemento XML.
pub(crate) fn escapar(t: &str) -> String {
    let mut s = String::with_capacity(t.len());
    for c in t.chars() {
        match c {
            '&' => s.push_str("&amp;"),
            '<' => s.push_str("&lt;"),
            '>' => s.push_str("&gt;"),
            '"' => s.push_str("&quot;"),
            '\'' => s.push_str("&apos;"),
            c => s.push(c),
        }
    }
    s
}

/// `<key>k</key><string>v</string>` con la sangría dada.
pub(crate) fn cadena(sangria: &str, clave: &str, valor: &str) -> String {
    format!("{sangria}<key>{}</key>\n{sangria}<string>{}</string>\n", escapar(clave), escapar(valor))
}

pub(crate) fn entero(sangria: &str, clave: &str, valor: i64) -> String {
    format!("{sangria}<key>{}</key>\n{sangria}<integer>{valor}</integer>\n", escapar(clave))
}

pub(crate) fn booleano(sangria: &str, clave: &str, valor: bool) -> String {
    format!("{sangria}<key>{}</key>\n{sangria}<{valor}/>\n", escapar(clave))
}

/// `<key>ProgramArguments</key><array>…</array>`.
pub(crate) fn lista_de_cadenas(sangria: &str, clave: &str, valores: &[String]) -> String {
    let mut s = format!("{sangria}<key>{}</key>\n{sangria}<array>\n", escapar(clave));
    for v in valores {
        s.push_str(&format!("{sangria}  <string>{}</string>\n", escapar(v)));
    }
    s.push_str(&format!("{sangria}</array>\n"));
    s
}

/// El documento entero alrededor del cuerpo de la `<dict>` raíz.
pub(crate) fn documento(cuerpo: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n{cuerpo}</dict>\n</plist>\n"
    )
}

/// Lo que cambia entre una tarea de servicio y la de una rutina.
pub(crate) struct TareaXml<'a> {
    pub descripcion: &'a str,
    /// `DOMINIO\\usuario`; sin él, el disparador y el principal valen para
    /// quien inicie sesión.
    pub usuario: Option<&'a str>,
    /// Los elementos hijos de `<Triggers>` ya escritos.
    pub disparadores: &'a str,
    pub comando: &'a str,
    pub argumentos: Option<&'a str>,
    pub directorio: Option<&'a str>,
    /// Servicio: sin tope de tiempo y reiniciado si falla. Rutina: tope de
    /// una hora, se lanza si se perdió por estar apagado el equipo y no se
    /// reintenta (reintentar encolaría la rutina dos veces).
    pub servicio: bool,
}

/// El XML de una tarea de Windows (`schtasks /Create /XML`): privilegio
/// mínimo, token interactivo y, a diferencia de las tareas creadas con
/// `/SC`, sin «detener a las 72 h» ni «no arrancar con batería».
pub(crate) fn tarea_windows(t: &TareaXml) -> String {
    let usuario = t.usuario.map(escapar);
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-16\"?>\n");
    s.push_str("<Task version=\"1.2\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\n");
    s.push_str(&format!("  <RegistrationInfo>\n    <Description>{}</Description>\n  </RegistrationInfo>\n", escapar(t.descripcion)));
    s.push_str(&format!("  <Triggers>\n{}  </Triggers>\n", t.disparadores));
    s.push_str("  <Principals>\n    <Principal id=\"Author\">\n");
    if let Some(u) = &usuario {
        s.push_str(&format!("      <UserId>{u}</UserId>\n"));
    }
    s.push_str("      <LogonType>InteractiveToken</LogonType>\n      <RunLevel>LeastPrivilege</RunLevel>\n    </Principal>\n  </Principals>\n");
    s.push_str("  <Settings>\n    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>\n");
    s.push_str("    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>\n    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>\n");
    s.push_str("    <AllowHardTerminate>true</AllowHardTerminate>\n");
    if !t.servicio {
        s.push_str("    <StartWhenAvailable>true</StartWhenAvailable>\n");
    }
    s.push_str("    <AllowStartOnDemand>true</AllowStartOnDemand>\n    <Enabled>true</Enabled>\n    <Hidden>true</Hidden>\n");
    s.push_str(&format!("    <ExecutionTimeLimit>{}</ExecutionTimeLimit>\n", if t.servicio { "PT0S" } else { "PT1H" }));
    if t.servicio {
        s.push_str("    <RestartOnFailure>\n      <Interval>PT1M</Interval>\n      <Count>999</Count>\n    </RestartOnFailure>\n");
    }
    s.push_str("  </Settings>\n  <Actions Context=\"Author\">\n    <Exec>\n");
    s.push_str(&format!("      <Command>{}</Command>\n", escapar(t.comando)));
    if let Some(a) = t.argumentos {
        s.push_str(&format!("      <Arguments>{}</Arguments>\n", escapar(a)));
    }
    if let Some(d) = t.directorio {
        s.push_str(&format!("      <WorkingDirectory>{}</WorkingDirectory>\n", escapar(d)));
    }
    s.push_str("    </Exec>\n  </Actions>\n</Task>\n");
    s
}

/// El disparador «al iniciar sesión» (de ese usuario, si se sabe).
pub(crate) fn disparador_inicio_sesion(usuario: Option<&str>) -> String {
    let id = usuario.map(|u| format!("      <UserId>{}</UserId>\n", escapar(u))).unwrap_or_default();
    format!("    <LogonTrigger>\n      <Enabled>true</Enabled>\n{id}    </LogonTrigger>\n")
}

/// El texto como lo espera `schtasks /XML`: UTF-16 LE con marca de orden de bytes.
pub(crate) fn utf16_le_con_bom(texto: &str) -> Vec<u8> {
    let mut v = vec![0xFF, 0xFE];
    for u in texto.encode_utf16() {
        v.extend(u.to_le_bytes());
    }
    v
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn escapa_lo_que_rompe_el_xml() {
        assert_eq!(escapar("a&b<c>\"d'"), "a&amp;b&lt;c&gt;&quot;d&apos;");
        assert!(cadena("  ", "Label", "x<y").contains("<string>x&lt;y</string>"));
        assert!(booleano("", "RunAtLoad", true).contains("<true/>"));
    }

    fn servicio<'a>(usuario: Option<&'a str>, disp: &'a str) -> TareaXml<'a> {
        TareaXml { descripcion: "ea <web>", usuario, disparadores: disp, comando: "C:\\ea & co\\ea-web.exe", argumentos: None, directorio: Some("C:\\ea & co"), servicio: true }
    }

    #[test]
    fn la_tarea_de_un_servicio_no_muere_ni_pide_administrador() {
        let disp = disparador_inicio_sesion(Some("PC\\ana"));
        let x = tarea_windows(&servicio(Some("PC\\ana"), &disp));
        assert!(x.starts_with("<?xml version=\"1.0\" encoding=\"UTF-16\"?>"));
        assert!(x.contains("<LogonTrigger>") && x.contains("<UserId>PC\\ana</UserId>"));
        assert!(x.contains("<LogonType>InteractiveToken</LogonType>") && x.contains("<RunLevel>LeastPrivilege</RunLevel>"));
        assert!(x.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(x.contains("<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>"));
        assert!(x.contains("<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>"));
        assert!(x.contains("<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>"));
        assert!(x.contains("<RestartOnFailure>\n      <Interval>PT1M</Interval>\n      <Count>999</Count>"));
        assert!(x.contains("<Command>C:\\ea &amp; co\\ea-web.exe</Command>") && x.contains("<WorkingDirectory>C:\\ea &amp; co</WorkingDirectory>"));
        assert!(x.contains("<Description>ea &lt;web&gt;</Description>"));
        assert!(!x.contains("<Arguments>"));
    }

    #[test]
    fn la_tarea_de_una_rutina_es_corta_y_no_se_reintenta() {
        let t = TareaXml { servicio: false, argumentos: Some("rutina lanzar \"x\""), ..servicio(None, "") };
        let x = tarea_windows(&t);
        assert!(x.contains("<ExecutionTimeLimit>PT1H</ExecutionTimeLimit>") && !x.contains("RestartOnFailure"));
        assert!(x.contains("<StartWhenAvailable>true</StartWhenAvailable>"));
        assert!(x.contains("<Arguments>rutina lanzar &quot;x&quot;</Arguments>"));
        assert!(!x.contains("<UserId>"), "sin usuario conocido no se inventa");
    }

    #[test]
    fn utf16_con_bom() {
        assert_eq!(utf16_le_con_bom("a\u{e9}"), [0xFF, 0xFE, b'a', 0, 0xE9, 0]);
    }
}
