//! Salud del ecosistema, de solo lectura: los servicios residentes
//! ([`servicios`]) y la disponibilidad de los agentes del catálogo
//! ([`disponibilidad`], spec 017). La usan `ea-web` (`GET /api/estado` y
//! `GET /api/agentes`), `ea agentes` y la TUI.
//!
//! Los servicios se preguntan al gestor de servicios del sistema
//! (`systemd --user`, launchd o el Programador de tareas). Todo es de mejor
//! esfuerzo: si el gestor no está o no responde, se devuelve "desconocido" en
//! vez de fallar (§12.4: degradación limpia).
//!
//! De los agentes, qué rol ocupa cada uno, si su binario está y si responde el
//! endpoint de su modelo. Nunca se lanza al agente: el binario solo se busca en
//! el `PATH` y al modelo solo se le pide `GET /models`.

use std::collections::HashMap;
use std::time::Duration;

use ea_ajustes::{Ajustes, VistaAgente};
use serde::Serialize;

use crate::GestorServicios;

/// Los únicos servicios que la interfaz puede consultar y reiniciar (de la
/// tabla de `ea-ajustes`).
pub use ea_ajustes::SERVICIOS_MONITORIZADOS as SERVICIOS;

pub struct Servicio {
    pub nombre: &'static str,
    /// `active`, `inactive`, `failed`… o `desconocido`.
    pub activo: String,
    /// Segundos que lleva activo. Se calcula con el reloj monotónico y no
    /// con `ActiveEnterTimestamp`: esta máquina alterna con Windows y arranca
    /// con la hora de pared desajustada hasta que NTP la corrige, así que las
    /// marcas de pared de los servicios pueden quedar en el futuro. Solo con
    /// systemd; launchd y el Programador de tareas no dan el dato (`None`).
    pub activo_s: Option<i64>,
}

pub async fn servicios(consultar: bool) -> Vec<Servicio> {
    let desconocido = || {
        SERVICIOS
            .iter()
            .map(|&nombre| Servicio { nombre, activo: "desconocido".into(), activo_s: None })
            .collect::<Vec<_>>()
    };
    if !consultar {
        return desconocido();
    }
    let Some(info) = GestorServicios::usuario().panel(&SERVICIOS).await else {
        return desconocido();
    };
    SERVICIOS
        .iter()
        .map(|&nombre| {
            let i = info.iter().find(|i| i.nombre == nombre);
            Servicio {
                nombre,
                activo: i.map_or_else(|| "desconocido".into(), |i| i.activo.clone()),
                activo_s: i.and_then(|i| i.activo_s),
            }
        })
        .collect()
}

const LIMITE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Serialize)]
pub struct AgenteDisponible {
    #[serde(flatten)]
    pub vista: VistaAgente,
    /// Si el endpoint local del modelo responde; `None` si el modelo es
    /// remoto o el propio del agente (no se le pregunta a nadie fuera).
    pub modelo_accesible: Option<bool>,
}

/// El catálogo con los ajustes `a`, buscando los binarios en el `PATH` de
/// este proceso. Cada endpoint local se pregunta una sola vez.
pub async fn disponibilidad(a: &Ajustes) -> Vec<AgenteDisponible> {
    let vistas = a.vista_agentes(std::env::var_os("PATH").as_deref());
    let mut respuestas: HashMap<String, bool> = HashMap::new();
    let mut salida = Vec::with_capacity(vistas.len());
    for vista in vistas {
        let modelo_accesible = match &vista.modelo {
            Some(m) if !m.remoto && !m.base_url.is_empty() => {
                if !respuestas.contains_key(&m.base_url) {
                    let ok = responde(&m.base_url).await;
                    respuestas.insert(m.base_url.clone(), ok);
                }
                Some(respuestas[&m.base_url])
            }
            _ => None,
        };
        salida.push(AgenteDisponible { vista, modelo_accesible });
    }
    salida
}

/// `GET <base_url>/models` responde 2xx antes del límite. Solo se llama con
/// endpoints locales (`ea-ajustes` exige loopback a un perfil no remoto). Con
/// `reqwest` y no con el cliente mínimo de la instalación: un endpoint local
/// puede ser `https://`.
async fn responde(base_url: &str) -> bool {
    let Ok(cliente) = reqwest::Client::builder().timeout(LIMITE).no_proxy().build() else { return false };
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    cliente.get(url).send().await.is_ok_and(|r| r.status().is_success())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ea_ajustes::{Fuentes, PerfilModelo};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Un endpoint que contesta 200 a cualquier petición.
    async fn endpoint() -> String {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let dir = l.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = l.accept().await {
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf).await;
                let _ = s.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 11\r\n\r\n{\"data\":[]}").await;
            }
        });
        format!("http://{dir}/v1")
    }

    #[tokio::test]
    async fn el_modelo_local_se_pregunta_y_el_remoto_no() {
        let d = tempfile::tempdir().unwrap();
        let mut a = Ajustes::por_defecto(&Fuentes::aislada(d.path()));
        a.modelos.get_mut("local").unwrap().base_url = endpoint().await;
        a.modelos.insert("caido".into(), PerfilModelo { nombre: "x".into(), base_url: "http://127.0.0.1:9/v1".into(), remoto: false });
        a.modelos.insert("nube".into(), PerfilModelo { nombre: "gpt".into(), base_url: String::new(), remoto: true });
        a.agentes.get_mut("codex").unwrap().modelo = "caido".into();
        a.agentes.get_mut("claude-code").unwrap().modelo = "nube".into();

        let r = disponibilidad(&a).await;
        let de = |id: &str| r.iter().find(|x| x.vista.id == id).unwrap();
        assert_eq!(de("qwen-code").modelo_accesible, Some(true));
        assert_eq!(de("codex").modelo_accesible, Some(false));
        assert_eq!(de("claude-code").modelo_accesible, None);
        assert_eq!(de("antigravity").modelo_accesible, None);
        assert_eq!(de("qwen-code").vista.rol, Some("ejecutor"));
        assert_eq!(de("claude-code").vista.rol, Some("planificador"));
        assert_eq!(de("codex").vista.rol, None);
    }

    /// Sin consultar al gestor, cada servicio sale «desconocido» y en el orden de la lista.
    #[tokio::test]
    async fn sin_consultar_todo_es_desconocido() {
        let s = servicios(false).await;
        assert_eq!(s.iter().map(|s| s.nombre).collect::<Vec<_>>(), SERVICIOS);
        assert!(s.iter().all(|s| s.activo == "desconocido" && s.activo_s.is_none()));
    }
}
