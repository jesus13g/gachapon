//! Bandeja del sistema (spec 021, decisión 7, RF-007): icono con el semáforo
//! y menú. Si no hay bandeja (algunos Wayland sin StatusNotifierItem), el
//! shell lo dice y cerrar la ventana lo termina.

use tauri::image::Image;
use tauri::menu::{MenuBuilder, MenuEvent};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::AppHandle;

use crate::vigia::Estado;

const ID: &str = "principal";

/// (id del elemento del menú, rótulo, fragmento al que lleva).
pub const SECCIONES: [(&str, &str, &str); 4] =
    [("inicio", "Inicio", "inicio"), ("trabajos", "Trabajos", "trabajos"), ("diario", "Diario", "diario"), ("sistema", "Sistema", "sistema")];

fn icono(e: Estado) -> Image<'static> {
    let bytes: &[u8] = match e {
        Estado::Bien => include_bytes!("../iconos/bien.png"),
        Estado::Aviso => include_bytes!("../iconos/aviso.png"),
        Estado::Caido => include_bytes!("../iconos/caido.png"),
    };
    Image::from_bytes(bytes).expect("los iconos embebidos son PNG válidos")
}

/// Crea la bandeja. `Err` si el sistema no ofrece ninguna.
pub fn crear(app: &AppHandle) -> tauri::Result<TrayIcon> {
    let mut menu = MenuBuilder::new(app).text("alternar", "Mostrar u ocultar").separator();
    for (id, rotulo, _) in SECCIONES {
        menu = menu.text(id, rotulo);
    }
    let menu = menu.separator().text("salir", "Salir").build()?;
    TrayIconBuilder::with_id(ID)
        .icon(icono(Estado::Caido))
        .tooltip("Ecosistema: esperando a ea-web")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, ev: MenuEvent| crate::al_elegir_menu(app, ev.id().as_ref()))
        .build(app)
}

/// Cambia el icono y la ayuda según el estado.
pub fn actualizar(app: &AppHandle, e: Estado) {
    if let Some(t) = app.tray_by_id(ID) {
        let _ = t.set_icon(Some(icono(e)));
        let _ = t.set_tooltip(Some(format!("Ecosistema: {}", e.texto())));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn los_iconos_embebidos_son_png_validos() {
        for e in [Estado::Bien, Estado::Aviso, Estado::Caido] {
            let _ = icono(e);
        }
    }

    #[test]
    fn las_secciones_existen_en_el_panel() {
        let app_js = include_str!("../../crates/ea-web/static/app.js");
        for (_, _, frag) in SECCIONES {
            assert!(app_js.contains(&format!("'{frag}'")), "{frag} no es una sección del panel");
        }
    }
}
