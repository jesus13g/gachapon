//! `ea hook subagentes` con el binario real y un estado temporal.

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn hook(estado: &std::path::Path, accion: &str, entrada: &str) -> Output {
    let mut hijo = Command::new(env!("CARGO_BIN_EXE_ea"))
        .args(["hook", "subagentes", accion])
        .env("EA_ESTADO_DIR", estado)
        .env("EA_CONFIG_DIR", estado.join("config"))
        .env("EA_DB_PATH", estado.join("ea.db"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("lanzar ea");
    hijo.stdin.take().unwrap().write_all(entrada.as_bytes()).unwrap();
    hijo.wait_with_output().unwrap()
}

#[test]
fn start_y_stop_escriben_donde_ea_web_lee() {
    let d = tempfile::tempdir().unwrap();
    let e = r#"{"session_id":"sesion1","agent_id":"ag1","agent_type":"Explore","transcript_path":"/t/x.jsonl"}"#;
    let o = hook(d.path(), "start", e);
    assert!(o.status.success() && o.stdout.is_empty() && o.stderr.is_empty());
    let f = d.path().join("subagentes/sesion1/ag1.json");
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&f).unwrap()).unwrap();
    assert_eq!(v["agente"], "ag1");
    assert_eq!(v["tipo"], "Explore");
    assert!(v["desde"].as_u64().unwrap() > 0 && v.get("hasta").is_none());
    assert!(hook(d.path(), "stop", e).status.success());
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&f).unwrap()).unwrap();
    assert!(v["hasta"].as_u64().unwrap() >= v["desde"].as_u64().unwrap());
}

#[test]
fn la_basura_sale_en_silencio_con_cero() {
    let d = tempfile::tempdir().unwrap();
    for (accion, entrada) in [("start", "no es json"), ("start", ""), ("stop", r#"{"session_id":"../x","agent_id":"a"}"#), ("raro", "{}")] {
        let o = hook(d.path(), accion, entrada);
        assert!(o.status.success(), "{accion} {entrada}");
        assert!(o.stdout.is_empty() && o.stderr.is_empty());
    }
    assert!(!d.path().join("subagentes").exists());
}
