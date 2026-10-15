---
descripcion: Escribir o cambiar código Rust con cargo (crates, workspaces, tests, clippy).
---
- En un workspace, compila y prueba solo el crate tocado: `cargo build -p <crate>`, `cargo test -p <crate>`.
- Lee el error completo del compilador: suele decir la solución (`help:`). Arregla el primer error y recompila; los siguientes suelen ser consecuencia.
- No añadas dependencias nuevas a `Cargo.toml` salvo que la tarea lo pida (no hay red para descargarlas).
- Imita el estilo del fichero: nombres, manejo de errores (`?`, `Result` propio, `anyhow`) y comentarios en el mismo idioma.
- En código de biblioteca evita `unwrap()`/`expect()`; en tests están bien.
- Tests unitarios en `#[cfg(test)] mod tests` del mismo fichero; de integración en `tests/`.
- Si existe, pasa `cargo clippy -p <crate>` antes de terminar y corrige los avisos de lo que hayas escrito.
