//! El subconjunto portable de `OnCalendar` (spec 020, decisión 6): lo que
//! launchd (`StartCalendarInterval`) y el Programador de tareas (`schtasks`)
//! saben expresar. En Linux manda systemd con toda su sintaxis; fuera de
//! Linux el propio sistema valida, calcula las próximas ejecuciones y
//! traduce, todo con funciones puras (la fecha de «ahora» llega de fuera).
//!
//! Admitido (sin distinguir mayúsculas):
//! - `minutely`, `hourly`, `daily`, `weekly` (lunes 00:00), `monthly` (día 1
//!   a las 00:00);
//! - `HH:MM[:SS]` y `*-*-* HH:MM[:SS]`, con segundos a 0;
//! - un prefijo opcional de días con las horas anteriores: `Mon..Fri`,
//!   `Mon,Wed,Fri`, `Sat`, `Mon..Wed,Fri` (también con el nombre entero).
//!
//! No se admite: varias horas, fechas concretas, listas de horas, segundos
//! distintos de 0, rangos que dan la vuelta (`Fri..Mon`), ni días con una
//! palabra clave (`Mon daily`).
//!
//! Las horas son las locales, sin ajustar los cambios de horario de verano.

use time::macros::format_description;
use time::{Date, Duration, PrimitiveDateTime, Time, Weekday};

/// Lo que se le dice al usuario cuando el calendario queda fuera.
pub const ADMITIDO: &str = "minutely, hourly, daily, weekly, monthly, HH:MM[:SS] o *-*-* HH:MM[:SS], \
     con un prefijo opcional de días (Mon..Fri, Mon,Wed,Fri, Sat)";

/// Con qué frecuencia dispara un [`Horario`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cuando {
    CadaMinuto,
    CadaHora,
    /// A esa hora y minuto, los días de [`Horario::dias`] (todos si no hay).
    Diario {
        hora: u8,
        minuto: u8,
    },
    /// El día 1 de cada mes a las 00:00.
    Mensual,
}

/// Un calendario del subconjunto portable, ya analizado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Horario {
    pub cuando: Cuando,
    /// Días de la semana en que dispara, de lunes a domingo y sin repetir;
    /// vacío = todos.
    pub dias: Vec<Weekday>,
}

/// La traducción a `StartCalendarInterval`/`StartInterval` de launchd.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntervaloLaunchd {
    /// `StartInterval`: cada tantos segundos.
    Cada(u32),
    /// `StartCalendarInterval`: un diccionario por cada disparo (claves
    /// `Weekday`, `Day`, `Hour`, `Minute`; las que faltan valen «cualquiera»).
    Calendario(Vec<Vec<(&'static str, u8)>>),
}

fn error(texto: &str, motivo: &str) -> String {
    format!("fuera de Linux solo se admite {ADMITIDO}; «{texto}» no vale: {motivo}")
}

fn dia_de_nombre(n: &str) -> Option<Weekday> {
    Some(match n.to_ascii_lowercase().as_str() {
        "mon" | "monday" => Weekday::Monday,
        "tue" | "tuesday" => Weekday::Tuesday,
        "wed" | "wednesday" => Weekday::Wednesday,
        "thu" | "thursday" => Weekday::Thursday,
        "fri" | "friday" => Weekday::Friday,
        "sat" | "saturday" => Weekday::Saturday,
        "sun" | "sunday" => Weekday::Sunday,
        _ => return None,
    })
}

fn dia_de_indice(i: u8) -> Weekday {
    [Weekday::Monday, Weekday::Tuesday, Weekday::Wednesday, Weekday::Thursday, Weekday::Friday, Weekday::Saturday, Weekday::Sunday][usize::from(i)]
}

fn abreviatura(d: Weekday) -> &'static str {
    ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][usize::from(d.number_days_from_monday())]
}

/// `MON`, `TUE`… como los quiere `schtasks /D`.
fn abreviatura_schtasks(d: Weekday) -> String {
    abreviatura(d).to_ascii_uppercase()
}

/// El día de la semana de launchd: domingo = 0, lunes = 1… sábado = 6.
fn numero_launchd(d: Weekday) -> u8 {
    d.number_days_from_sunday()
}

fn parsear_dias(spec: &str) -> Result<Vec<Weekday>, String> {
    let mut marcados = [false; 7];
    for parte in spec.split(',') {
        let (a, b) = match parte.split_once("..") {
            Some((a, b)) => (a, b),
            None => (parte, parte),
        };
        let a = dia_de_nombre(a).ok_or_else(|| format!("«{a}» no es un día de la semana"))?;
        let b = dia_de_nombre(b).ok_or_else(|| format!("«{b}» no es un día de la semana"))?;
        let (i, j) = (a.number_days_from_monday(), b.number_days_from_monday());
        if i > j {
            return Err(format!("el rango «{parte}» da la vuelta a la semana; escríbelo como lista"));
        }
        for k in i..=j {
            marcados[usize::from(k)] = true;
        }
    }
    Ok((0..7u8).filter(|&k| marcados[usize::from(k)]).map(dia_de_indice).collect())
}

fn parte_numerica(p: &str, max: u8) -> Option<u8> {
    (!p.is_empty() && p.len() <= 2 && p.bytes().all(|b| b.is_ascii_digit())).then(|| p.parse::<u8>().ok()).flatten().filter(|&n| n < max)
}

fn parsear_hora(t: &str) -> Result<(u8, u8), String> {
    let partes: Vec<&str> = t.split(':').collect();
    if !(2..=3).contains(&partes.len()) {
        return Err(format!("«{t}» no es una hora HH:MM[:SS]"));
    }
    let hora = parte_numerica(partes[0], 24).ok_or_else(|| format!("la hora de «{t}» no está entre 0 y 23"))?;
    let minuto = parte_numerica(partes[1], 60).ok_or_else(|| format!("los minutos de «{t}» no están entre 0 y 59"))?;
    if let Some(s) = partes.get(2)
        && parte_numerica(s, 60) != Some(0)
    {
        return Err(format!("los segundos de «{t}» deben ser 00 (la resolución es de un minuto)"));
    }
    Ok((hora, minuto))
}

/// Analiza un calendario del subconjunto portable.
pub fn parsear(texto: &str) -> Result<Horario, String> {
    let fichas: Vec<&str> = texto.split_whitespace().collect();
    if fichas.is_empty() {
        return Err("el calendario está vacío (p. ej. «*-*-* 08:00:00»)".into());
    }
    if fichas.len() == 1 {
        let palabra = match fichas[0].to_ascii_lowercase().as_str() {
            "minutely" => Some((Cuando::CadaMinuto, vec![])),
            "hourly" => Some((Cuando::CadaHora, vec![])),
            "daily" => Some((Cuando::Diario { hora: 0, minuto: 0 }, vec![])),
            "weekly" => Some((Cuando::Diario { hora: 0, minuto: 0 }, vec![Weekday::Monday])),
            "monthly" => Some((Cuando::Mensual, vec![])),
            _ => None,
        };
        if let Some((cuando, dias)) = palabra {
            return Ok(Horario { cuando, dias });
        }
    }
    let (dias, resto) = if fichas[0].chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
        let dias = parsear_dias(fichas[0]).map_err(|m| error(texto, &m))?;
        (dias, &fichas[1..])
    } else {
        (vec![], &fichas[..])
    };
    let hora = match resto {
        [h] => h,
        ["*-*-*", h] => h,
        [] => return Err(error(texto, "faltan la hora (HH:MM)")),
        _ => return Err(error(texto, "solo cabe una hora, con la fecha «*-*-*» opcional")),
    };
    let (hora, minuto) = parsear_hora(hora).map_err(|m| error(texto, &m))?;
    Ok(normalizar(Horario { cuando: Cuando::Diario { hora, minuto }, dias }))
}

/// Si están los siete días, es lo mismo que ninguno.
fn normalizar(mut h: Horario) -> Horario {
    if h.dias.len() == 7 {
        h.dias.clear();
    }
    h
}

impl Horario {
    fn dias_texto(&self) -> String {
        let idx: Vec<u8> = self.dias.iter().map(|d| d.number_days_from_monday()).collect();
        let mut trozos = Vec::new();
        let mut i = 0;
        while i < idx.len() {
            let mut j = i;
            while j + 1 < idx.len() && idx[j + 1] == idx[j] + 1 {
                j += 1;
            }
            let (a, b) = (abreviatura(dia_de_indice(idx[i])), abreviatura(dia_de_indice(idx[j])));
            if j - i >= 2 {
                trozos.push(format!("{a}..{b}"));
            } else {
                trozos.extend((i..=j).map(|k| abreviatura(dia_de_indice(idx[k])).to_string()));
            }
            i = j + 1;
        }
        trozos.join(",")
    }

    /// La forma normalizada, como la escribe `systemd-analyze calendar`
    /// (`Mon..Fri *-*-* 09:00:00`).
    pub fn normalizado(&self) -> String {
        match self.cuando {
            Cuando::CadaMinuto => "*-*-* *:*:00".into(),
            Cuando::CadaHora => "*-*-* *:00:00".into(),
            Cuando::Mensual => "*-*-01 00:00:00".into(),
            Cuando::Diario { hora, minuto } => {
                let dias = if self.dias.is_empty() { String::new() } else { format!("{} ", self.dias_texto()) };
                format!("{dias}*-*-* {hora:02}:{minuto:02}:00")
            }
        }
    }

    fn dispara_el(&self, fecha: Date) -> bool {
        match self.cuando {
            Cuando::Mensual => fecha.day() == 1,
            Cuando::Diario { .. } => self.dias.is_empty() || self.dias.contains(&fecha.weekday()),
            _ => true,
        }
    }

    /// Las `n` próximas ejecuciones estrictamente posteriores a `desde`
    /// (hora local, sin zona).
    pub fn proximas(&self, desde: PrimitiveDateTime, n: usize) -> Vec<PrimitiveDateTime> {
        let mut salida = Vec::with_capacity(n);
        if n == 0 {
            return salida;
        }
        match self.cuando {
            Cuando::CadaMinuto | Cuando::CadaHora => {
                let t = desde.time();
                let (paso, base) = if self.cuando == Cuando::CadaMinuto {
                    (Duration::minutes(1), Time::from_hms(t.hour(), t.minute(), 0))
                } else {
                    (Duration::hours(1), Time::from_hms(t.hour(), 0, 0))
                };
                let mut c = PrimitiveDateTime::new(desde.date(), base.expect("hora válida")) + paso;
                while salida.len() < n {
                    salida.push(c);
                    c += paso;
                }
            }
            Cuando::Diario { .. } | Cuando::Mensual => {
                let (hora, minuto) = match self.cuando {
                    Cuando::Diario { hora, minuto } => (hora, minuto),
                    _ => (0, 0),
                };
                let hora = Time::from_hms(hora, minuto, 0).expect("hora validada al analizar");
                let mut dia = Some(desde.date());
                // Cota: un mes de 31 días por ejecución, más un margen.
                for _ in 0..(32 * n + 32) {
                    let Some(d) = dia else { break };
                    let c = PrimitiveDateTime::new(d, hora);
                    if self.dispara_el(d) && c > desde {
                        salida.push(c);
                        if salida.len() == n {
                            break;
                        }
                    }
                    dia = d.next_day();
                }
            }
        }
        salida
    }

    /// `StartCalendarInterval` / `StartInterval` de launchd.
    pub fn launchd(&self) -> IntervaloLaunchd {
        match self.cuando {
            Cuando::CadaMinuto => IntervaloLaunchd::Cada(60),
            Cuando::CadaHora => IntervaloLaunchd::Calendario(vec![vec![("Minute", 0)]]),
            Cuando::Mensual => IntervaloLaunchd::Calendario(vec![vec![("Day", 1), ("Hour", 0), ("Minute", 0)]]),
            Cuando::Diario { hora, minuto } => {
                let hm = [("Hour", hora), ("Minute", minuto)];
                if self.dias.is_empty() {
                    return IntervaloLaunchd::Calendario(vec![hm.to_vec()]);
                }
                IntervaloLaunchd::Calendario(self.dias.iter().map(|&d| std::iter::once(("Weekday", numero_launchd(d))).chain(hm).collect()).collect())
            }
        }
    }

    /// Los argumentos de `schtasks /Create` que fijan el disparador
    /// (`/SC`, `/MO`, `/D`, `/ST`).
    pub fn schtasks(&self) -> Vec<String> {
        let v = |s: &[&str]| s.iter().map(|x| (*x).to_string()).collect::<Vec<_>>();
        match self.cuando {
            Cuando::CadaMinuto => v(&["/SC", "MINUTE", "/MO", "1"]),
            Cuando::CadaHora => v(&["/SC", "HOURLY", "/MO", "1", "/ST", "00:00"]),
            Cuando::Mensual => v(&["/SC", "MONTHLY", "/D", "1", "/ST", "00:00"]),
            Cuando::Diario { hora, minuto } => {
                let st = format!("{hora:02}:{minuto:02}");
                if self.dias.is_empty() {
                    vec!["/SC".into(), "DAILY".into(), "/ST".into(), st]
                } else {
                    let dias = self.dias.iter().map(|&d| abreviatura_schtasks(d)).collect::<Vec<_>>().join(",");
                    vec!["/SC".into(), "WEEKLY".into(), "/D".into(), dias, "/ST".into(), st]
                }
            }
        }
    }
}

/// Una ejecución como la escribe systemd: `Thu 2026-10-22 08:00:00`.
pub fn formatear(dt: PrimitiveDateTime) -> String {
    dt.format(format_description!("[weekday repr:short] [year]-[month]-[day] [hour]:[minute]:[second]")).unwrap_or_else(|_| dt.to_string())
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use time::macros::datetime;

    fn h(t: &str) -> Horario {
        parsear(t).unwrap_or_else(|e| panic!("{t}: {e}"))
    }

    #[test]
    fn las_palabras_clave() {
        assert_eq!(h("minutely").cuando, Cuando::CadaMinuto);
        assert_eq!(h("Hourly").cuando, Cuando::CadaHora);
        assert_eq!(h("daily"), Horario { cuando: Cuando::Diario { hora: 0, minuto: 0 }, dias: vec![] });
        assert_eq!(h("weekly").dias, vec![Weekday::Monday]);
        assert_eq!(h("weekly").normalizado(), "Mon *-*-* 00:00:00");
        assert_eq!(h("monthly").cuando, Cuando::Mensual);
        assert_eq!(h("  daily  ").normalizado(), "*-*-* 00:00:00");
    }

    #[test]
    fn las_horas_con_y_sin_fecha() {
        for t in ["08:30", "8:30", "08:30:00", "*-*-* 08:30", "*-*-* 08:30:00"] {
            assert_eq!(h(t), Horario { cuando: Cuando::Diario { hora: 8, minuto: 30 }, dias: vec![] }, "{t}");
        }
        assert_eq!(h("23:59").normalizado(), "*-*-* 23:59:00");
    }

    #[test]
    fn los_dias() {
        assert_eq!(h("Mon..Fri 09:00").normalizado(), "Mon..Fri *-*-* 09:00:00");
        assert_eq!(h("Mon,Wed,Fri *-*-* 09:00").normalizado(), "Mon,Wed,Fri *-*-* 09:00:00");
        assert_eq!(h("sat 10:00").dias, vec![Weekday::Saturday]);
        assert_eq!(h("Monday..Friday 07:15").dias.len(), 5);
        assert_eq!(h("Mon..Wed,Fri 07:15").normalizado(), "Mon..Wed,Fri *-*-* 07:15:00");
        assert_eq!(h("Mon,Tue 07:15").normalizado(), "Mon,Tue *-*-* 07:15:00");
        assert_eq!(h("Sun,Mon..Sat 07:15"), h("07:15"), "los siete días son todos");
    }

    #[test]
    fn lo_que_no_cabe_se_rechaza_con_motivo() {
        for malo in [
            "",
            "   ",
            "yearly",
            "quarterly",
            "*:0/15",
            "Mon..Fri",
            "Mon daily",
            "08:00 09:00",
            "*-*-* 08:00 09:00",
            "2026-10-22 08:00",
            "08:00:30",
            "24:00",
            "08:60",
            "8",
            "08:",
            "Fri..Mon 08:00",
            "Foo 08:00",
            "08:00,09:00",
            "*-*-01 08:00",
            "Mon..Fri,Foo 08:00",
            "123:00",
            "*-*-* *:00:00",
        ] {
            let e = parsear(malo).unwrap_err();
            assert!(e.contains("fuera de Linux solo se admite") || e.contains("vacío"), "{malo:?}: {e}");
        }
        assert!(parsear("08:00:30").unwrap_err().contains("segundos"));
        assert!(parsear("Fri..Mon 08:00").unwrap_err().contains("da la vuelta"));
    }

    #[test]
    fn proximas_diarias() {
        let ahora = datetime!(2026-10-22 08:00:00); // jueves
        let p = h("08:00").proximas(ahora, 3);
        assert_eq!(p, [datetime!(2026-10-23 08:00), datetime!(2026-10-24 08:00), datetime!(2026-10-25 08:00)]);
        let p = h("08:30").proximas(ahora, 2);
        assert_eq!(p, [datetime!(2026-10-22 08:30), datetime!(2026-10-23 08:30)]);
        assert_eq!(formatear(p[0]), "Thu 2026-10-22 08:30:00");
    }

    #[test]
    fn proximas_con_dias() {
        let viernes_tarde = datetime!(2026-10-23 18:00:00);
        let p = h("Mon..Fri 09:00").proximas(viernes_tarde, 3);
        assert_eq!(p, [datetime!(2026-10-26 09:00), datetime!(2026-10-27 09:00), datetime!(2026-10-28 09:00)]);
        let p = h("Sat 10:00").proximas(viernes_tarde, 2);
        assert_eq!(p, [datetime!(2026-10-24 10:00), datetime!(2026-10-31 10:00)]);
        let p = h("weekly").proximas(datetime!(2026-10-26 00:00:00), 2);
        assert_eq!(p, [datetime!(2026-11-02 00:00), datetime!(2026-11-09 00:00)]);
    }

    #[test]
    fn proximas_de_minuto_hora_y_mes() {
        let ahora = datetime!(2026-12-31 23:59:30);
        assert_eq!(h("minutely").proximas(ahora, 2), [datetime!(2027-01-01 00:00), datetime!(2027-01-01 00:01)]);
        assert_eq!(h("hourly").proximas(datetime!(2026-10-22 08:00:00), 2), [datetime!(2026-10-22 09:00), datetime!(2026-10-22 10:00)]);
        assert_eq!(h("monthly").proximas(ahora, 3), [datetime!(2027-01-01 00:00), datetime!(2027-02-01 00:00), datetime!(2027-03-01 00:00)]);
        assert_eq!(h("monthly").proximas(datetime!(2026-10-01 00:00:00), 1), [datetime!(2026-11-01 00:00)]);
        assert!(h("daily").proximas(ahora, 0).is_empty());
    }

    #[test]
    fn traduccion_a_launchd() {
        assert_eq!(h("minutely").launchd(), IntervaloLaunchd::Cada(60));
        assert_eq!(h("hourly").launchd(), IntervaloLaunchd::Calendario(vec![vec![("Minute", 0)]]));
        assert_eq!(h("monthly").launchd(), IntervaloLaunchd::Calendario(vec![vec![("Day", 1), ("Hour", 0), ("Minute", 0)]]));
        assert_eq!(h("08:30").launchd(), IntervaloLaunchd::Calendario(vec![vec![("Hour", 8), ("Minute", 30)]]));
        assert_eq!(
            h("Sun,Mon 09:00").launchd(),
            IntervaloLaunchd::Calendario(vec![vec![("Weekday", 1), ("Hour", 9), ("Minute", 0)], vec![("Weekday", 0), ("Hour", 9), ("Minute", 0)],])
        );
    }

    #[test]
    fn traduccion_a_schtasks() {
        assert_eq!(h("minutely").schtasks(), ["/SC", "MINUTE", "/MO", "1"]);
        assert_eq!(h("hourly").schtasks(), ["/SC", "HOURLY", "/MO", "1", "/ST", "00:00"]);
        assert_eq!(h("daily").schtasks(), ["/SC", "DAILY", "/ST", "00:00"]);
        assert_eq!(h("*-*-* 07:05").schtasks(), ["/SC", "DAILY", "/ST", "07:05"]);
        assert_eq!(h("Mon..Fri 09:00").schtasks(), ["/SC", "WEEKLY", "/D", "MON,TUE,WED,THU,FRI", "/ST", "09:00"]);
        assert_eq!(h("weekly").schtasks(), ["/SC", "WEEKLY", "/D", "MON", "/ST", "00:00"]);
        assert_eq!(h("monthly").schtasks(), ["/SC", "MONTHLY", "/D", "1", "/ST", "00:00"]);
    }
}
