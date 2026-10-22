//! `ea-escritorio`: shell fino de Tauri 2 que carga el panel de `ea-web`
//! (spec 021). La página no recibe ningún comando de Tauri; todo lo propio del
//! escritorio lo hace este proceso. Fuera del workspace principal.

// En Windows no abre una consola.
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod api;
mod avisos;
mod bandeja;
mod clave;
mod espera;
mod fragmento;
mod origen;
mod vigia;

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use tauri::http::Response;
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use url::Url;

use ea_ajustes::Entorno;
use vigia::{Estado, VigiaClave, VigiaTrabajos};

/// Estado compartido del shell (gestionado por Tauri).
struct Compartido {
    puerto: u16,
    fichero_clave: Option<PathBuf>,
    /// Contador para dar una etiqueta nueva a cada ventana que se recrea.
    generacion: AtomicUsize,
    /// Hay bandeja: cerrar oculta. Si no, cerrar termina.
    bandeja: AtomicBool,
    /// ¿Mostrando el panel (true) o la página de espera (false)?
    panel: AtomicBool,
    /// Vista pedida mientras se espera a `ea-web`.
    fragmento_pendiente: Mutex<Option<String>>,
}

fn compartido(app: &AppHandle) -> tauri::State<'_, Compartido> {
    app.state::<Compartido>()
}

fn ventana(app: &AppHandle) -> Option<WebviewWindow> {
    let etiqueta = format!("principal-{}", compartido(app).generacion.load(Ordering::SeqCst));
    app.get_webview_window(&etiqueta)
}

/// Crea la ventana: el panel con la clave en el guion de inicio, o la página
/// de espera (sin clave).
fn crear_ventana(app: &AppHandle, panel: bool, fragmento: Option<&str>, visible: bool) -> tauri::Result<WebviewWindow> {
    let c = compartido(app);
    let puerto = c.puerto;
    let n = c.generacion.load(Ordering::SeqCst) + 1;
    let url = if panel { origen::url_panel(puerto, fragmento) } else { espera::url_espera() };
    let url = Url::parse(&url).expect("la URL del panel es válida");
    let mut b = WebviewWindowBuilder::new(app, format!("principal-{n}"), WebviewUrl::External(url))
        .title("Ecosistema")
        .inner_size(1280.0, 800.0)
        .min_inner_size(640.0, 480.0)
        .visible(visible)
        .on_navigation(move |u| {
            if origen::navegacion_permitida(u, puerto) {
                return true;
            }
            if matches!(u.scheme(), "http" | "https" | "mailto") {
                let _ = ea_plataforma::escritorio::abrir(u.as_str().as_ref());
            }
            false
        });
    if panel {
        let clave = c.fichero_clave.as_deref().and_then(clave::leer);
        let guion = clave::guion_inicio(clave.as_deref());
        if !guion.is_empty() {
            b = b.initialization_script(guion);
        }
    }
    let w = b.build()?;
    // Solo con la ventana construida pasa a ser la vigente.
    c.generacion.store(n, Ordering::SeqCst);
    let w2 = w.clone();
    let app2 = app.clone();
    w.on_window_event(move |ev| {
        if let WindowEvent::CloseRequested { api, .. } = ev
            && compartido(&app2).bandeja.load(Ordering::SeqCst)
        {
            api.prevent_close();
            let _ = w2.hide();
        }
    });
    Ok(w)
}

/// Muestra la ventana, la trae al frente y, si hay fragmento, cambia la vista.
fn mostrar(app: &AppHandle, fragmento: Option<&str>) {
    if let Some(f) = fragmento {
        if compartido(app).panel.load(Ordering::SeqCst) {
            if let Some(w) = ventana(app) {
                let _ = w.eval(fragmento::guion_ir_a(f));
            }
        } else {
            *compartido(app).fragmento_pendiente.lock().unwrap() = Some(f.to_string());
        }
    }
    if let Some(w) = ventana(app) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Elección del menú de la bandeja.
pub fn al_elegir_menu(app: &AppHandle, id: &str) {
    match id {
        "alternar" => {
            if let Some(w) = ventana(app) {
                if w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false) {
                    let _ = w.hide();
                } else {
                    mostrar(app, None);
                }
            }
        }
        "salir" => {
            avisos::borrar_presencia();
            app.exit(0);
        }
        otro => {
            if let Some((_, _, frag)) = bandeja::SECCIONES.iter().find(|(i, _, _)| *i == otro) {
                mostrar(app, Some(frag));
            }
        }
    }
}

/// Pasa de la página de espera al panel: recrea la ventana (para que el guion
/// de inicio lleve la clave actual) conservando posición, tamaño y visibilidad.
fn pasar_al_panel(app: &AppHandle) {
    let c = compartido(app);
    let pendiente = c.fragmento_pendiente.lock().unwrap().take();
    let vieja = ventana(app);
    let visible = vieja.as_ref().is_none_or(|w| w.is_visible().unwrap_or(true));
    let geometria = vieja.as_ref().and_then(|w| Some((w.outer_position().ok()?, w.inner_size().ok()?)));
    match crear_ventana(app, true, pendiente.as_deref(), visible) {
        Ok(nueva) => {
            if let Some((pos, tam)) = geometria {
                let _ = nueva.set_size(tam);
                let _ = nueva.set_position(pos);
            }
            c.panel.store(true, Ordering::SeqCst);
            if let Some(v) = vieja {
                let _ = v.destroy();
            }
        }
        Err(e) => eprintln!("ea-escritorio: no se pudo abrir el panel: {e}"),
    }
}

/// Aviso nativo de un trabajo terminado. En Linux, `notify-send` con acción
/// para poder abrir el trabajo al pulsarlo; si no se puede, la notificación
/// del plugin (que en escritorio no avisa de la pulsación).
fn avisar(app: &AppHandle, a: vigia::AvisoTrabajo) {
    if !avisos::activados() {
        return;
    }
    let (titulo, cuerpo) = avisos::texto(&a);
    let app = app.clone();
    std::thread::spawn(move || {
        #[cfg(target_os = "linux")]
        {
            match avisar_notify_send(&titulo, &cuerpo) {
                Some(true) => {
                    let app2 = app.clone();
                    let _ = app.run_on_main_thread(move || mostrar(&app2, Some(&format!("trabajos/{}", a.id))));
                    return;
                }
                Some(false) => return,
                None => {} // notify-send no sirvió: cae al plugin
            }
        }
        use tauri_plugin_notification::NotificationExt;
        let _ = app.notification().builder().title(titulo).body(cuerpo).show();
    });
}

/// Avisos de `notify-send` en curso a la vez.
#[cfg(target_os = "linux")]
static AVISOS_EN_CURSO: AtomicUsize = AtomicUsize::new(0);
#[cfg(target_os = "linux")]
const MAX_AVISOS_SIMULTANEOS: usize = 4;
#[cfg(target_os = "linux")]
const TOPE_AVISO: std::time::Duration = std::time::Duration::from_secs(60);

/// `Some(true)` si el usuario pulsó «Abrir»; `Some(false)` si se cerró sin
/// pulsar (o venció el tope); `None` si `notify-send` no existe, falló o hay
/// demasiados avisos abiertos (entonces se usa el plugin).
#[cfg(target_os = "linux")]
fn avisar_notify_send(titulo: &str, cuerpo: &str) -> Option<bool> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    if AVISOS_EN_CURSO.fetch_add(1, Ordering::SeqCst) >= MAX_AVISOS_SIMULTANEOS {
        AVISOS_EN_CURSO.fetch_sub(1, Ordering::SeqCst);
        return None;
    }
    let resultado = (|| {
        let mut hijo = Command::new("notify-send")
            .args(["--app-name=ea-escritorio", "--action=abrir=Abrir", "--wait"])
            .arg(titulo)
            .arg(cuerpo)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let inicio = std::time::Instant::now();
        let estado = loop {
            match hijo.try_wait() {
                Ok(Some(e)) => break e,
                Ok(None) if inicio.elapsed() >= TOPE_AVISO => {
                    let _ = hijo.kill();
                    let _ = hijo.wait();
                    return Some(false);
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(200)),
                Err(_) => return None,
            }
        };
        if !estado.success() {
            return None;
        }
        let mut s = String::new();
        if let Some(mut o) = hijo.stdout.take() {
            let _ = o.read_to_string(&mut s);
        }
        Some(s.trim() == "abrir")
    })();
    AVISOS_EN_CURSO.fetch_sub(1, Ordering::SeqCst);
    resultado
}

/// El hilo del vigía: estado, clave, página de espera y avisos.
fn vigilar(app: AppHandle) {
    let (puerto, fichero) = {
        let c = compartido(&app);
        (c.puerto, c.fichero_clave.clone())
    };
    let inicial = fichero.as_deref().and_then(clave::leer);
    let mut clave = VigiaClave::nuevo(inicial);
    let mut trabajos = VigiaTrabajos::default();
    let mut ultimo: Option<Estado> = None;
    let mut motivo_enviado = String::new();
    loop {
        std::thread::sleep(vigia::PERIODO);
        let respuesta = api::obtener(puerto, "estado");
        let estado = match &respuesta {
            Ok(v) => vigia::estado_de(v),
            Err(_) => Estado::Caido,
        };
        if ultimo != Some(estado) {
            ultimo = Some(estado);
            let a = app.clone();
            let _ = app.run_on_main_thread(move || bandeja::actualizar(&a, estado));
        }
        if respuesta.is_err() {
            trabajos.olvidar();
        }
        let en_panel = compartido(&app).panel.load(Ordering::SeqCst);
        match respuesta {
            Err(motivo) if !en_panel => {
                if motivo != motivo_enviado {
                    motivo_enviado = motivo.clone();
                    let a = app.clone();
                    let _ = app.run_on_main_thread(move || {
                        if let Some(w) = ventana(&a) {
                            let _ = w.eval(espera::guion_motivo(&motivo));
                        }
                    });
                }
            }
            Err(_) => {}
            Ok(_) if !en_panel => {
                motivo_enviado.clear();
                let a = app.clone();
                let _ = app.run_on_main_thread(move || pasar_al_panel(&a));
                // La ventana nueva ya lleva la clave de este momento.
                clave = VigiaClave::nuevo(fichero.as_deref().and_then(clave::leer));
            }
            Ok(_) => {
                if let Some(f) = fichero.as_deref()
                    && let Some(nueva) = clave.comprobar(f)
                {
                    let a = app.clone();
                    let _ = app.run_on_main_thread(move || {
                        if let Some(w) = ventana(&a) {
                            let _ = w.eval(clave::guion_renovar(nueva.as_deref()));
                        }
                    });
                }
                match api::obtener(puerto, "trabajos") {
                    Ok(lista) => {
                        for a in trabajos.observar(&lista) {
                            avisar(&app, a);
                        }
                    }
                    Err(_) => trabajos.olvidar(),
                }
            }
        }
    }
}

fn main() {
    let ajustes = ea_ajustes::cargar();
    let puerto = ajustes.red.puerto_web;
    let fichero_clave = ea_ajustes::dir_ejecucion(&Entorno::Proceso).map(|d| ea_ajustes::fichero_clave_web(&d));
    let argv: Vec<String> = std::env::args().collect();
    let oculto = argv.iter().skip(1).any(|a| a == "--oculto");
    let inicial = fragmento::de_argumentos(&argv);

    let construido = tauri::Builder::default()
        // El primero: un segundo arranque reenvía su fragmento a esta ventana.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            let f = fragmento::de_argumentos(&argv);
            let app2 = app.clone();
            let _ = app.run_on_main_thread(move || mostrar(&app2, f.as_deref()));
        }))
        .plugin(tauri_plugin_notification::init())
        .register_uri_scheme_protocol(espera::PROTOCOLO, |_ctx, _peticion| {
            Response::builder()
                .header("Content-Type", "text/html; charset=utf-8")
                .body(espera::HTML.as_bytes().to_vec())
                .expect("la respuesta de la página de espera es válida")
        })
        .setup(move |app| {
            let handle = app.handle().clone();
            app.manage(Compartido {
                puerto,
                fichero_clave,
                generacion: AtomicUsize::new(0),
                bandeja: AtomicBool::new(false),
                panel: AtomicBool::new(false),
                fragmento_pendiente: Mutex::new(None),
            });
            match bandeja::crear(&handle) {
                Ok(t) => {
                    // La bandeja vive mientras viva la aplicación.
                    std::mem::forget(t);
                    compartido(&handle).bandeja.store(true, Ordering::SeqCst);
                }
                Err(e) => eprintln!("ea-escritorio: este sistema no ofrece bandeja ({e}); cerrar la ventana terminará el shell"),
            }
            // Si ea-web ya responde, abre directo el panel; si no, la espera.
            let listo = api::obtener(puerto, "estado").is_ok();
            // Sin bandeja una ventana oculta sería inalcanzable: se muestra.
            let visible = vigia::visible_al_crear(oculto, compartido(&handle).bandeja.load(Ordering::SeqCst));
            crear_ventana(&handle, listo, inicial.as_deref(), visible)?;
            if listo {
                compartido(&handle).panel.store(true, Ordering::SeqCst);
            } else {
                *compartido(&handle).fragmento_pendiente.lock().unwrap() = inicial.clone();
            }
            avisos::crear_presencia();
            std::thread::spawn(move || vigilar(handle));
            Ok(())
        })
        .build(tauri::generate_context!());

    match construido {
        Ok(app) => app.run(|_app, ev| {
            if let RunEvent::Exit = ev {
                avisos::borrar_presencia();
            }
        }),
        Err(e) => {
            eprintln!("ea-escritorio: no se pudo iniciar: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    //! RF-003: la página no recibe ningún comando de Tauri.
    use serde_json::Value;

    #[test]
    fn la_configuracion_no_expone_tauri_a_la_pagina() {
        let conf: Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(conf["app"]["withGlobalTauri"], Value::Bool(false));
        assert_eq!(conf["app"]["windows"].as_array().map(Vec::len), Some(0), "las ventanas se crean en código");
    }

    #[test]
    fn las_capabilities_no_dan_permisos_ni_remotos() {
        let cap: Value = serde_json::from_str(include_str!("../capabilities/predeterminada.json")).unwrap();
        assert!(cap.get("remote").is_none(), "una capability remota daría IPC a la página");
        assert_eq!(cap["permissions"].as_array().map(Vec::len), Some(0));
        assert_eq!(cap["windows"].as_array().map(Vec::len), Some(0));
        let dir = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/capabilities")).unwrap();
        assert_eq!(dir.count(), 1, "solo la capability vacía");
    }
}
