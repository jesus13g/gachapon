//! Modelos que `ea instalar` puede ofrecer descargar (spec 023, RF-011).
//! Viven aquí, junto a [`Ajustes::por_defecto`](crate::Ajustes::por_defecto),
//! porque es el único sitio del código donde se nombran modelos (art. VII).

/// Un GGUF descargable para un perfil de modelo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DescargaModelo {
    /// Id del perfil (`modelos.<id>`) al que sirve.
    pub perfil: &'static str,
    /// Nombre con el que lo sirve `llama-server` (`--alias`); el mismo
    /// `nombre` del perfil por defecto.
    pub nombre: &'static str,
    pub url: &'static str,
    /// Tamaño aproximado, solo para avisar antes de descargar.
    pub bytes: u64,
    /// VRAM (o memoria unificada) mínima en GB para ofrecerlo.
    pub vram_min_gb: u32,
    /// Orden para servirlo, con los marcadores `{gguf}` y `{nombre}`.
    pub orden_servidor: &'static str,
}

impl DescargaModelo {
    /// El nombre del fichero (lo último de la URL).
    pub fn fichero(&self) -> &'static str {
        self.url.rsplit('/').next().unwrap_or(self.url)
    }

    /// La orden para servirlo con la ruta real del GGUF.
    pub fn orden_con(&self, gguf: &str) -> String {
        self.orden_servidor.replace("{gguf}", gguf).replace("{nombre}", self.nombre)
    }

    /// El tamaño en GB, redondeado a una cifra decimal.
    pub fn gb(&self) -> f64 {
        (self.bytes as f64 / 1e8).round() / 10.0
    }
}

pub const DESCARGAS: &[DescargaModelo] = &[DescargaModelo {
    perfil: "local",
    nombre: "qwen3-coder-30b",
    url: "https://huggingface.co/unsloth/Qwen3-Coder-30B-A3B-Instruct-GGUF/resolve/main/Qwen3-Coder-30B-A3B-Instruct-Q4_K_M.gguf",
    bytes: 18_600_000_000,
    vram_min_gb: 20,
    orden_servidor: "llama-server -m {gguf} --alias {nombre} --host 127.0.0.1 --port 8080 --jinja -c 65536",
}];

/// La descarga de un perfil, si la hay.
pub fn descarga_para(perfil: &str) -> Option<&'static DescargaModelo> {
    DESCARGAS.iter().find(|d| d.perfil == perfil)
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use crate::{Ajustes, Fuentes, Rol};

    #[test]
    fn el_perfil_del_ejecutor_por_defecto_tiene_descarga_con_su_nombre() {
        let d = tempfile::tempdir().unwrap();
        let a = Ajustes::por_defecto(&Fuentes::aislada(d.path()));
        let (id, config) = a.agente_de(Rol::Ejecutor);
        let perfil = &config.modelo;
        let desc = descarga_para(perfil).unwrap_or_else(|| panic!("el perfil {perfil} de {id} no tiene descarga"));
        assert_eq!(desc.nombre, a.modelos[perfil.as_str()].nombre);
    }

    #[test]
    fn orden_y_fichero() {
        let d = descarga_para("local").unwrap();
        assert!(d.fichero().ends_with(".gguf"));
        let o = d.orden_con("/m/x.gguf");
        assert!(o.contains("-m /m/x.gguf") && o.contains("--alias qwen3-coder-30b") && !o.contains('{'));
        assert!(d.gb() > 10.0);
        assert!(d.url.starts_with("https://"));
    }
}
