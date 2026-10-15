---
descripcion: Arreglar un fallo o un test roto: reproducir, localizar la causa y corregirla con el cambio mínimo.
---
1. Reproduce: ejecuta el comando que falla y lee la salida entera, sobre todo la ÚLTIMA traza o el primer error.
2. Localiza: ve al fichero y la línea que señala el error; lee unas 40 líneas alrededor.
3. Formula la causa en una frase antes de editar. Si no la sabes, añade un `print`/`eprintln!` temporal y vuelve a ejecutar.
4. Corrige la causa, no el síntoma: no captures la excepción para ocultarla ni cambies el valor esperado del test.
5. Cambio mínimo: no reformatees ni refactorices nada que no esté relacionado.
6. Quita las trazas temporales y vuelve a ejecutar hasta que pase.
Si tras 3 intentos sigue fallando igual, para y explica la causa probable en tu resumen.
