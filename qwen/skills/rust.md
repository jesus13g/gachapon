---
descripcion: Escribir o cambiar código Rust con cargo (crates, workspaces, tests, clippy).
---
- En un workspace, compila y prueba solo el crate tocado: `cargo test -p <crate>`.
- Arregla el PRIMER error del compilador y recompila; lee su `help:`.
- No añadas dependencias: no hay red.
- En código de biblioteca evita `unwrap()`; en tests vale. Tests en `#[cfg(test)] mod tests` del mismo fichero.
