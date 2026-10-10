//! Pruebas de extremo a extremo de los modelos de voz reales, sin micrófono
//! ni altavoces: se sintetiza una frase, se transcribe de vuelta y se busca la
//! palabra de activación en audio sintetizado. Requieren los modelos
//! descargados (ver README), así que van con `#[ignore]`:
//!
//! ```text
//! cargo test -p ea-orchestrator --release --test voz_modelos -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Imprimen las latencias medidas. Ojo: la voz sintética no es una persona
//! hablando a un micrófono; estas pruebas no sustituyen a la prueba real.

use std::time::Instant;

use ea_orchestrator::voz::config::{self, VozConfig};
use ea_orchestrator::voz::palabra::DetectorPalabra;
use ea_orchestrator::voz::reconocimiento::{SherpaReconocedor, SAMPLE_RATE};
use ea_orchestrator::voz::sintesis::SherpaSintetizador;
use ea_orchestrator::voz::{captura, Reconocedor};
use sherpa_onnx::LinearResampler;

fn a_16k(muestras: &[f32], frecuencia: i32) -> Vec<f32> {
    if frecuencia == SAMPLE_RATE {
        return muestras.to_vec();
    }
    LinearResampler::create(frecuencia, SAMPLE_RATE)
        .expect("remuestreador")
        .resample(muestras, true)
}

fn normalizar(t: &str) -> String {
    ea_orchestrator::voz::palabra::normalizar(t)
}

#[test]
#[ignore]
fn sintesis_y_reconocimiento_de_ida_y_vuelta() {
    let cfg = VozConfig::default();
    cfg.verificar().expect("modelos de voz presentes");

    let t = Instant::now();
    let tts = SherpaSintetizador::cargar(&cfg).unwrap();
    println!("carga TTS: {:?}", t.elapsed());
    let t = Instant::now();
    let asr = SherpaReconocedor::cargar(&cfg).unwrap();
    println!("carga ASR: {:?}", t.elapsed());

    let frase = "Hola, ¿qué tareas tengo pendientes para hoy?";
    let t = Instant::now();
    let (audio, frecuencia) = tts.sintetizar(frase).unwrap();
    let sintesis = t.elapsed();
    let duracion = audio.len() as f32 / frecuencia as f32;
    println!("síntesis: {sintesis:?} para {duracion:.2} s de audio (factor tiempo real {:.2})", sintesis.as_secs_f32() / duracion);
    assert!(!audio.is_empty(), "el audio sintetizado está vacío");
    assert!(audio.iter().any(|m| m.abs() > 0.01), "el audio sintetizado es silencio");

    let audio16 = a_16k(&audio, frecuencia);
    let t = Instant::now();
    let texto = asr.transcribir(&audio16).unwrap().texto;
    println!("ASR: {:?} para {duracion:.2} s de audio -> {texto:?}", t.elapsed());
    let (esperado, obtenido) = (normalizar(frase), normalizar(&texto));
    assert!(
        obtenido.contains("tareas") && obtenido.contains("pendientes"),
        "transcripción inesperada: '{obtenido}' (esperado ~ '{esperado}')"
    );
}

/// El camino «no se detectó voz»: el VAD no produce ningún segmento sobre
/// silencio ni sobre ruido débil, y sí sobre voz.
#[test]
#[ignore]
fn vad_descarta_silencio_y_acepta_voz() {
    let cfg = VozConfig::default();
    let vad = captura::crear_vad(&cfg).unwrap();

    let silencio = vec![0.0f32; SAMPLE_RATE as usize * 3];
    for trozo in silencio.chunks(512) {
        vad.accept_waveform(trozo);
    }
    vad.flush();
    assert!(vad.is_empty(), "el VAD vio voz en silencio");

    vad.reset();
    let ruido: Vec<f32> = (0..SAMPLE_RATE as usize * 3)
        .map(|i| (((i as u64).wrapping_mul(2654435761) % 1000) as f32 / 1000.0 - 0.5) * 0.01)
        .collect();
    for trozo in ruido.chunks(512) {
        vad.accept_waveform(trozo);
    }
    vad.flush();
    assert!(vad.is_empty(), "el VAD vio voz en ruido débil");

    vad.reset();
    let tts = SherpaSintetizador::cargar(&cfg).unwrap();
    let (audio, f) = tts.sintetizar("Necesito que me recuerdes la reunión de mañana.").unwrap();
    let mut audio = a_16k(&audio, f);
    audio.extend(vec![0.0; SAMPLE_RATE as usize]);
    for trozo in audio.chunks(512) {
        vad.accept_waveform(trozo);
    }
    vad.flush();
    assert!(!vad.is_empty(), "el VAD no vio voz en voz sintética");
}

#[test]
#[ignore]
fn palabra_de_activacion_sobre_audio_sintetico() {
    let cfg = VozConfig::default();
    let mut pa = cfg.palabra_activacion.clone();
    pa.activada = true;
    let t = Instant::now();
    let detector = DetectorPalabra::cargar(&pa).expect("modelo de palabra de activación");
    println!("carga KWS: {:?}", t.elapsed());
    let tts = SherpaSintetizador::cargar(&cfg).unwrap();

    let sintetizar = |texto: &str| {
        let (a, f) = tts.sintetizar(texto).unwrap();
        a_16k(&a, f)
    };
    let positivos = [
        "hola asistente",
        "Hola, asistente.",
        "hola asistente dime algo",
        "Hola asistente, apunta una tarea",
    ];
    let negativos = [
        "Mañana tengo que ir al dentista a las cinco.",
        "El asistente de dirección llegó tarde.",
        "Hola, ¿has visto mi cartera?",
        "La asistencia técnica es muy lenta.",
        "Hay que revisar la lista de tareas pendientes de la semana.",
    ];
    let mut aciertos = 0;
    for p in positivos {
        let audio = sintetizar(p);
        let t = Instant::now();
        let hallada = detector.contiene_palabra(&audio);
        println!("KWS positivo {p:?}: {hallada} ({:?} para {:.1} s de audio)", t.elapsed(), audio.len() as f32 / 16000.0);
        aciertos += hallada as usize;
    }
    let mut falsos = 0;
    for n in negativos {
        let audio = sintetizar(n);
        let hallada = detector.contiene_palabra(&audio);
        println!("KWS negativo {n:?}: {hallada}");
        falsos += hallada as usize;
    }
    println!("aciertos {aciertos}/{} falsos positivos {falsos}/{}", positivos.len(), negativos.len());
    assert!(aciertos * 2 >= positivos.len(), "el detector no encontró la palabra en al menos la mitad de las frases sintetizadas");
    assert_eq!(falsos, 0, "falsos positivos en frases sin la palabra");
    let _ = config::voz_dir();
}
