//! Palabra de activación (§6.5 "nivel permanente", fase 5 "después con
//! palabra de activación"). Corre sobre el mismo runtime `sherpa-onnx` que
//! el resto de la voz (decisión 2 de la especificación), en CPU (decisión 5).
//!
//! # Elección del modelo
//!
//! `sherpa-onnx` solo publica modelos de detección de palabra clave
//! preentrenados para chino e inglés (`sherpa-onnx-kws-zipformer-*`). Para
//! español no existe ninguno. Pero el detector de `sherpa-onnx`
//! (`KeywordSpotter`) es de vocabulario abierto: acepta cualquier
//! transductor *streaming* y una lista de palabras clave expresada como
//! secuencias de los tokens del propio modelo. Así que se usa un transductor
//! streaming entrenado en español (`sherpa-onnx-streaming-zipformer-es-kroko`,
//! 150 MB, BPE de 652 piezas) y se tokeniza la palabra elegida por el
//! usuario contra su vocabulario (`tokenizar`). Sirve cualquier palabra o
//! frase en español sin reentrenar nada, y el modelo entiende los fonemas del
//! español, que es justo lo que no daría un modelo inglés forzado.
//!
//! Alternativa descartada dentro del mismo runtime: VAD + Whisper sobre cada
//! frase que se oiga en la habitación para buscar la palabra en el texto.
//! Obliga a transcribir de forma continua todo lo que se hable cerca (coste
//! de CPU y, sobre todo, texto inventado en silencios, el riesgo de §6.6).
//!
//! # Mitigaciones de falsos positivos
//!
//! - Umbral y refuerzo configurables del detector (`umbral`, `puntuacion`).
//! - [`FiltroActivacion`]: si `exigir_voz`, la detección solo vale si el VAD
//!   (Silero) vio voz justo antes; un ruido que el modelo confunde con la
//!   palabra no activa nada.
//! - [`FiltroActivacion`]: periodo de enfriamiento tras cada activación.
//! - El bucle de escucha se detiene mientras hay un turno de voz en curso,
//!   así que el asistente no se activa con su propia voz.
//! - Tras la activación, el turno sigue el mismo camino que el manual, con
//!   su puerta de VAD y su descarte de transcripciones vacías: una activación
//!   falsa sin nadie hablando termina en `voz_transcripcion_descartada`.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use sherpa_onnx::{KeywordSpotter, KeywordSpotterConfig, VoiceActivityDetector};

use super::config::PalabraActivacionConfig;
use super::reconocimiento::SAMPLE_RATE;

/// Número máximo de segmentaciones alternativas de la palabra que se pasan
/// al detector: el modelo puede emitir la palabra troceada de varias formas.
const MAX_VARIANTES: usize = 4;
/// Cuántas piezas más que la segmentación mínima se aceptan como variante.
const HOLGURA_PIEZAS: usize = 1;

// ------------------------------------------------------------ tokenización

/// Espacios simples y sin signos de puntuación; conserva las mayúsculas.
pub fn limpiar(palabra: &str) -> String {
    palabra
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Forma canónica en minúsculas (para la etiqueta de la palabra).
pub fn normalizar(palabra: &str) -> String {
    limpiar(palabra).to_lowercase()
}

/// El vocabulario del modelo distingue mayúsculas (escribe «Hola» al
/// principio de frase y «hola» en medio), así que la palabra se busca en
/// minúsculas y con la primera letra en mayúscula.
pub fn variantes_de_mayusculas(palabra: &str) -> Vec<String> {
    let minusculas = normalizar(palabra);
    let mut letras = minusculas.chars();
    let capitalizada = match letras.next() {
        Some(c) => c.to_uppercase().chain(letras).collect::<String>(),
        None => String::new(),
    };
    let mut v = vec![minusculas];
    if capitalizada != v[0] {
        v.push(capitalizada);
    }
    v
}

/// Segmentaciones del texto (tal cual, con sus mayúsculas) en piezas del vocabulario (estilo
/// SentencePiece: cada palabra lleva el prefijo `▁`), de menos a más piezas,
/// con un máximo de [`MAX_VARIANTES`] y como mucho [`HOLGURA_PIEZAS`] piezas
/// más que la mínima. Error si algún carácter no se puede representar.
pub fn tokenizar(palabra: &str, vocabulario: &HashSet<String>) -> anyhow::Result<Vec<Vec<String>>> {
    let normalizada = limpiar(palabra);
    anyhow::ensure!(!normalizada.is_empty(), "la palabra de activación está vacía");

    let texto: Vec<char> = format!("▁{}", normalizada.replace(' ', "▁")).chars().collect();
    let n = texto.len();
    let longitud_max = vocabulario.iter().map(|p| p.chars().count()).max().unwrap_or(1);

    // minimas[i]: mínimo de piezas para cubrir texto[i..]; None si imposible.
    let mut minimas: Vec<Option<usize>> = vec![None; n + 1];
    minimas[n] = Some(0);
    for i in (0..n).rev() {
        for fin in (i + 1..=n.min(i + longitud_max)).rev() {
            let pieza: String = texto[i..fin].iter().collect();
            if vocabulario.contains(&pieza) {
                if let Some(resto) = minimas[fin] {
                    if minimas[i].map_or(true, |m| resto + 1 < m) {
                        minimas[i] = Some(resto + 1);
                    }
                }
            }
        }
    }
    let minimo = minimas[0].ok_or_else(|| {
        anyhow::anyhow!("la palabra '{palabra}' contiene caracteres que el modelo no sabe representar")
    })?;

    // Enumeración acotada por profundidad de presupuesto de piezas.
    fn recorrer(
        texto: &[char],
        i: usize,
        presupuesto: usize,
        longitud_max: usize,
        vocabulario: &HashSet<String>,
        minimas: &[Option<usize>],
        actual: &mut Vec<String>,
        salida: &mut Vec<Vec<String>>,
    ) {
        if salida.len() >= 64 {
            return;
        }
        if i == texto.len() {
            salida.push(actual.clone());
            return;
        }
        if presupuesto == 0 {
            return;
        }
        for fin in (i + 1..=texto.len().min(i + longitud_max)).rev() {
            let pieza: String = texto[i..fin].iter().collect();
            if !vocabulario.contains(&pieza) {
                continue;
            }
            // Poda: lo que queda debe caber en el presupuesto.
            match minimas[fin] {
                Some(m) if m < presupuesto => {}
                _ => continue,
            }
            actual.push(pieza);
            recorrer(texto, fin, presupuesto - 1, longitud_max, vocabulario, minimas, actual, salida);
            actual.pop();
        }
    }

    let mut todas = Vec::new();
    recorrer(
        &texto,
        0,
        minimo + HOLGURA_PIEZAS,
        longitud_max,
        vocabulario,
        &minimas,
        &mut Vec::new(),
        &mut todas,
    );
    todas.sort_by_key(|s| s.len());
    todas.dedup();
    todas.truncate(MAX_VARIANTES);
    Ok(todas)
}

/// Lee las piezas de un `tokens.txt` de sherpa-onnx (`pieza id` por línea).
pub fn leer_vocabulario(contenido: &str) -> HashSet<String> {
    contenido
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .map(str::to_string)
        .collect()
}

/// Contenido del fichero de palabras clave de `sherpa-onnx`: una línea por
/// variante, `pieza pieza … :refuerzo #umbral @etiqueta`. Todas las variantes
/// comparten etiqueta, así que cuentan como la misma palabra.
pub fn formatear_palabras_clave(variantes: &[Vec<String>], palabra: &str, refuerzo: f32, umbral: f32) -> String {
    let etiqueta = normalizar(palabra).replace(' ', "_");
    variantes
        .iter()
        .map(|v| format!("{} :{refuerzo} #{umbral} @{etiqueta}\n", v.join(" ")))
        .collect()
}

// ------------------------------------------------------------------ filtro

/// Por qué se descarta una detección que el modelo dio por buena.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MotivoDescarte {
    /// El VAD no vio voz justo antes: probable ruido.
    SinVoz,
    /// Demasiado pronto tras la activación anterior.
    Enfriamiento,
}

impl MotivoDescarte {
    pub fn como_str(self) -> &'static str {
        match self {
            Self::SinVoz => "sin_voz",
            Self::Enfriamiento => "enfriamiento",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Veredicto {
    Aceptar,
    Descartar(MotivoDescarte),
}

/// Lógica pura (sin audio ni reloj propio) que decide si una detección del
/// modelo se convierte en activación. El tiempo se inyecta para poder
/// probarla.
pub struct FiltroActivacion {
    enfriamiento: Duration,
    exigir_voz: bool,
    ultima_activacion: Option<Instant>,
}

impl FiltroActivacion {
    pub fn new(enfriamiento: Duration, exigir_voz: bool) -> Self {
        Self { enfriamiento, exigir_voz, ultima_activacion: None }
    }

    /// `voz_reciente`: el VAD vio voz dentro de la ventana previa a la
    /// detección.
    pub fn evaluar(&mut self, ahora: Instant, voz_reciente: bool) -> Veredicto {
        if let Some(ultima) = self.ultima_activacion {
            if ahora.saturating_duration_since(ultima) < self.enfriamiento {
                return Veredicto::Descartar(MotivoDescarte::Enfriamiento);
            }
        }
        if self.exigir_voz && !voz_reciente {
            return Veredicto::Descartar(MotivoDescarte::SinVoz);
        }
        self.ultima_activacion = Some(ahora);
        Veredicto::Aceptar
    }

    /// Reinicia el enfriamiento desde `ahora` (al terminar un turno, para que
    /// el eco o las últimas palabras del turno no cuenten como activación).
    pub fn marcar(&mut self, ahora: Instant) {
        self.ultima_activacion = Some(ahora);
    }
}

/// Guarda de turno único: solo un turno de voz (manual o por palabra) puede
/// tener el micrófono a la vez. Se libera al soltarla.
pub struct GuardaTurno<'a>(&'a AtomicBool);

impl<'a> GuardaTurno<'a> {
    pub fn intentar(bandera: &'a AtomicBool) -> Option<Self> {
        bandera
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Self(bandera))
    }
}

impl Drop for GuardaTurno<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

// ----------------------------------------------------------------- detector

pub struct DetectorPalabra {
    spotter: KeywordSpotter,
    pub palabra: String,
    exigir_voz: bool,
    enfriamiento: Duration,
}

/// Resultado de una sesión de escucha de [`DetectorPalabra::esperar`].
pub enum Escucha {
    /// Palabra detectada y aceptada por el filtro.
    Activada,
    /// El modelo la detectó pero el filtro la rechazó.
    Descartada(MotivoDescarte),
    /// Se pidió parar (turno manual en curso o cierre).
    Interrumpida,
}

/// Ventana previa a la detección en la que el VAD debe haber visto voz.
const VENTANA_VOZ: Duration = Duration::from_millis(1500);

impl DetectorPalabra {
    pub fn cargar(cfg: &PalabraActivacionConfig) -> anyhow::Result<Self> {
        cfg.verificar()?;
        let p = |x: &std::path::Path| x.to_string_lossy().into_owned();

        let vocabulario = leer_vocabulario(&std::fs::read_to_string(&cfg.tokens)?);
        let mut palabras_clave = String::new();
        let mut variantes_total = 0;
        // La palabra y sus alias comparten etiqueta: cualquiera activa.
        for forma in std::iter::once(&cfg.palabra).chain(cfg.alias.iter()) {
            let mut variantes = Vec::new();
            for texto in variantes_de_mayusculas(forma) {
                variantes.extend(tokenizar(&texto, &vocabulario)?);
            }
            variantes_total += variantes.len();
            palabras_clave.push_str(&formatear_palabras_clave(&variantes, &cfg.palabra, cfg.puntuacion, cfg.umbral));
        }
        tracing::info!(palabra = %cfg.palabra, alias = ?cfg.alias, variantes = variantes_total, "palabra de activación tokenizada");

        let mut config = KeywordSpotterConfig::default();
        config.model_config.transducer.encoder = Some(p(&cfg.encoder));
        config.model_config.transducer.decoder = Some(p(&cfg.decoder));
        config.model_config.transducer.joiner = Some(p(&cfg.joiner));
        config.model_config.tokens = Some(p(&cfg.tokens));
        config.model_config.provider = Some("cpu".to_string());
        config.model_config.num_threads = 1;
        config.keywords_buf = Some(palabras_clave);
        config.keywords_score = cfg.puntuacion;
        config.keywords_threshold = cfg.umbral;

        let spotter = KeywordSpotter::create(&config)
            .ok_or_else(|| anyhow::anyhow!("sherpa-onnx no pudo cargar el detector de palabra de activación"))?;
        Ok(Self {
            spotter,
            palabra: cfg.palabra.clone(),
            exigir_voz: cfg.exigir_voz,
            enfriamiento: Duration::from_secs_f32(cfg.enfriamiento_s),
        })
    }

    pub fn nuevo_filtro(&self) -> FiltroActivacion {
        FiltroActivacion::new(self.enfriamiento, self.exigir_voz)
    }

    /// Procesa audio ya capturado y dice si contiene la palabra. Es el
    /// núcleo de [`esperar`](Self::esperar) y lo que usan las pruebas con
    /// audio sintetizado (sin micrófono).
    pub fn contiene_palabra(&self, audio: &[f32]) -> bool {
        let stream = self.spotter.create_stream();
        let mut hallada = false;
        // Medio segundo de silencio al final para que el detector cierre.
        let cola = vec![0.0f32; SAMPLE_RATE as usize / 2];
        stream.accept_waveform(SAMPLE_RATE, audio);
        stream.accept_waveform(SAMPLE_RATE, &cola);
        stream.input_finished();
        while self.spotter.is_ready(&stream) {
            self.spotter.decode(&stream);
            if self.resultado_positivo(&stream) {
                hallada = true;
                self.spotter.reset(&stream);
            }
        }
        hallada
    }

    fn resultado_positivo(&self, stream: &sherpa_onnx::OnlineStream) -> bool {
        self.spotter
            .get_result(stream)
            .map(|r| !r.keyword.is_empty())
            .unwrap_or(false)
    }

    /// Escucha el micrófono hasta que el detector da una palabra y el filtro
    /// la acepta, la rechaza, o `parar` se activa. Bloqueante: se llama desde
    /// `spawn_blocking`. Libera el micrófono al volver.
    pub fn esperar(
        &self,
        vad: &VoiceActivityDetector,
        filtro: &mut FiltroActivacion,
        parar: &AtomicBool,
    ) -> anyhow::Result<Escucha> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| anyhow::anyhow!("no hay dispositivo de audio de entrada"))?;
        let stream_config = cpal::StreamConfig {
            channels: 1,
            sample_rate: cpal::SampleRate(SAMPLE_RATE as u32),
            buffer_size: cpal::BufferSize::Default,
        };
        let (tx, rx) = mpsc::channel::<Vec<f32>>();
        let entrada = device.build_input_stream(
            &stream_config,
            move |data: &[f32], _| {
                let _ = tx.send(data.to_vec());
            },
            |err| tracing::warn!(error = %err, "error en la captura de audio (palabra de activación)"),
            None,
        )?;
        entrada.play()?;

        vad.reset();
        let stream = self.spotter.create_stream();
        let mut ultima_voz: Option<Instant> = None;

        loop {
            if parar.load(Ordering::Acquire) {
                return Ok(Escucha::Interrumpida);
            }
            let chunk = match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(c) => c,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    anyhow::bail!("la captura de audio se interrumpió")
                }
            };

            vad.accept_waveform(&chunk);
            if vad.detected() {
                ultima_voz = Some(Instant::now());
            }
            while !vad.is_empty() {
                ultima_voz = Some(Instant::now());
                vad.pop();
            }

            stream.accept_waveform(SAMPLE_RATE, &chunk);
            while self.spotter.is_ready(&stream) {
                self.spotter.decode(&stream);
                if self.resultado_positivo(&stream) {
                    self.spotter.reset(&stream);
                    let ahora = Instant::now();
                    let voz_reciente = ultima_voz.is_some_and(|t| ahora.duration_since(t) <= VENTANA_VOZ);
                    return Ok(match filtro.evaluar(ahora, voz_reciente) {
                        Veredicto::Aceptar => Escucha::Activada,
                        Veredicto::Descartar(m) => Escucha::Descartada(m),
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vocab(piezas: &[&str]) -> HashSet<String> {
        piezas.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn normaliza_mayusculas_y_signos() {
        assert_eq!(normalizar("  ¡Oye,   Asistente! "), "oye asistente");
        assert_eq!(normalizar("Ñandú"), "ñandú");
    }

    #[test]
    fn variantes_de_mayusculas_cubren_inicio_de_frase() {
        assert_eq!(variantes_de_mayusculas("Hola Asistente!"), vec!["hola asistente", "Hola asistente"]);
        assert_eq!(variantes_de_mayusculas("ñu"), vec!["ñu", "Ñu"]);
    }

    #[test]
    fn tokenizar_conserva_mayusculas_del_vocabulario() {
        let v = vocab(&["▁Hola", "▁hola", "▁a", "si"]);
        assert_eq!(tokenizar("Hola", &v).unwrap()[0], vec!["▁Hola"]);
        assert!(tokenizar("HOLA", &v).is_err());
    }

    #[test]
    fn tokeniza_con_prefijo_de_palabra() {
        let v = vocab(&["▁oye", "▁asis", "tente", "▁", "o", "y", "e"]);
        let r = tokenizar("oye asistente", &v).unwrap();
        assert_eq!(r[0], vec!["▁oye", "▁asis", "tente"]);
    }

    #[test]
    fn devuelve_varias_segmentaciones_ordenadas_por_longitud() {
        let v = vocab(&["▁ho", "▁hola", "la", "l", "a"]);
        let r = tokenizar("hola", &v).unwrap();
        assert_eq!(r[0], vec!["▁hola"]);
        assert!(r.contains(&vec!["▁ho".to_string(), "la".to_string()]));
        assert!(r.windows(2).all(|w| w[0].len() <= w[1].len()));
    }

    #[test]
    fn rechaza_caracteres_fuera_del_vocabulario() {
        let v = vocab(&["▁a", "b"]);
        assert!(tokenizar("ac", &v).is_err());
        assert!(tokenizar("   ", &v).is_err());
    }

    #[test]
    fn lee_vocabulario_de_tokens_txt() {
        let v = leer_vocabulario("<blk> 0\n▁de 4\nr 5\n");
        assert!(v.contains("▁de") && v.contains("r") && v.len() == 3);
    }

    #[test]
    fn formatea_palabras_clave_con_etiqueta_comun() {
        let t = formatear_palabras_clave(
            &[vec!["▁oye".into(), "▁asis".into()], vec!["▁o".into(), "ye".into()]],
            "Oye Asistente",
            1.5,
            0.3,
        );
        assert_eq!(t, "▁oye ▁asis :1.5 #0.3 @oye_asistente\n▁o ye :1.5 #0.3 @oye_asistente\n");
    }

    #[test]
    fn filtro_acepta_con_voz_y_respeta_enfriamiento() {
        let t0 = Instant::now();
        let mut f = FiltroActivacion::new(Duration::from_secs(3), true);
        assert_eq!(f.evaluar(t0, true), Veredicto::Aceptar);
        assert_eq!(
            f.evaluar(t0 + Duration::from_secs(1), true),
            Veredicto::Descartar(MotivoDescarte::Enfriamiento)
        );
        assert_eq!(f.evaluar(t0 + Duration::from_secs(4), true), Veredicto::Aceptar);
    }

    #[test]
    fn filtro_descarta_sin_voz_y_no_gasta_enfriamiento() {
        let t0 = Instant::now();
        let mut f = FiltroActivacion::new(Duration::from_secs(3), true);
        assert_eq!(f.evaluar(t0, false), Veredicto::Descartar(MotivoDescarte::SinVoz));
        // Un descarte por falta de voz no inicia el enfriamiento.
        assert_eq!(f.evaluar(t0 + Duration::from_millis(100), true), Veredicto::Aceptar);
    }

    #[test]
    fn filtro_sin_exigir_voz_acepta_sin_vad() {
        let mut f = FiltroActivacion::new(Duration::from_secs(0), false);
        assert_eq!(f.evaluar(Instant::now(), false), Veredicto::Aceptar);
    }

    #[test]
    fn marcar_reinicia_el_enfriamiento() {
        let t0 = Instant::now();
        let mut f = FiltroActivacion::new(Duration::from_secs(3), false);
        f.marcar(t0);
        assert_eq!(
            f.evaluar(t0 + Duration::from_secs(1), true),
            Veredicto::Descartar(MotivoDescarte::Enfriamiento)
        );
    }

    #[test]
    fn guarda_de_turno_es_exclusiva_y_se_libera() {
        let b = AtomicBool::new(false);
        let g = GuardaTurno::intentar(&b).expect("primera guarda");
        assert!(GuardaTurno::intentar(&b).is_none());
        assert!(b.load(Ordering::Acquire));
        drop(g);
        assert!(GuardaTurno::intentar(&b).is_some());
    }
}
