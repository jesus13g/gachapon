//! Diff de líneas mínimo (subsecuencia común más larga), para enseñar en una
//! simulación qué cambia entre dos definiciones. No pretende ser `diff -u`:
//! solo las líneas que cambian, con un poco de contexto.

/// Por encima de tantas celdas (líneas × líneas) no se busca el diff mínimo:
/// se da todo lo viejo como borrado y todo lo nuevo como añadido.
const MAX_CELDAS: usize = 4_000_000;
/// Líneas iguales que se enseñan alrededor de cada cambio.
const CONTEXTO: usize = 2;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    Igual,
    Fuera,
    Dentro,
}

fn operaciones<'a>(a: &[&'a str], b: &[&'a str]) -> Vec<(Op, &'a str)> {
    let (n, m) = (a.len(), b.len());
    if n.saturating_mul(m) > MAX_CELDAS {
        return a.iter().map(|l| (Op::Fuera, *l)).chain(b.iter().map(|l| (Op::Dentro, *l))).collect();
    }
    // lcs[i][j]: longitud de la subsecuencia común de a[i..] y b[j..].
    let mut lcs = vec![0u32; (n + 1) * (m + 1)];
    let ix = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[ix(i, j)] = if a[i] == b[j] { lcs[ix(i + 1, j + 1)] + 1 } else { lcs[ix(i + 1, j)].max(lcs[ix(i, j + 1)]) };
        }
    }
    let (mut i, mut j, mut ops) = (0, 0, Vec::with_capacity(n + m));
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            ops.push((Op::Igual, a[i]));
            i += 1;
            j += 1;
        } else if i < n && (j == m || lcs[ix(i + 1, j)] >= lcs[ix(i, j + 1)]) {
            ops.push((Op::Fuera, a[i]));
            i += 1;
        } else {
            ops.push((Op::Dentro, b[j]));
            j += 1;
        }
    }
    ops
}

/// Las líneas que cambian de `antes` a `despues`, con `-`, `+` o un espacio
/// delante y `@@` entre trozos separados. Vacío si son iguales.
pub fn diff_lineas(antes: &str, despues: &str) -> String {
    let a: Vec<&str> = antes.lines().collect();
    let b: Vec<&str> = despues.lines().collect();
    let ops = operaciones(&a, &b);
    let cambia: Vec<usize> = ops.iter().enumerate().filter(|(_, (o, _))| *o != Op::Igual).map(|(k, _)| k).collect();
    if cambia.is_empty() {
        return String::new();
    }
    let visible = |k: usize| cambia.iter().any(|&c| c.abs_diff(k) <= CONTEXTO);
    let mut out = String::new();
    let mut anterior: Option<usize> = None;
    for (k, (op, linea)) in ops.iter().enumerate() {
        if !visible(k) {
            continue;
        }
        if anterior.is_some_and(|p| p + 1 != k) || (anterior.is_none() && k > 0) {
            out.push_str("@@\n");
        }
        anterior = Some(k);
        let signo = match op {
            Op::Igual => ' ',
            Op::Fuera => '-',
            Op::Dentro => '+',
        };
        out.push(signo);
        out.push_str(linea);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod pruebas {
    use super::diff_lineas;

    #[test]
    fn solo_lo_que_cambia_con_contexto() {
        let a = "a\nb\nc\nd\ne\nf\ng\n";
        let b = "a\nb\nc\nD\ne\nf\ng\n";
        assert_eq!(diff_lineas(a, b), "@@\n b\n c\n-d\n+D\n e\n f\n");
        assert_eq!(diff_lineas(a, a), "");
        assert_eq!(diff_lineas("", "x\n"), "+x\n");
    }
}
