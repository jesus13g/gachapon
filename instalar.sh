#!/bin/sh
# Instalación de gachapon en un paso (spec 023) para Linux y macOS.
#
#   curl -fsSL https://jesus13g.github.io/gachapon/instalar.sh | sh
#   ./instalar.sh            # desde un clon del repo: compila e instala
#
# Este script solo consigue los binarios y le pasa el control a
# `ea instalar`, que enseña el plan y pide la confirmación en la terminal.
# Las opciones que reciba (p. ej. --sin-escritorio) se pasan a `ea instalar`.
#
# Variables:
#   GACHAPON_VERSION   una versión concreta (p. ej. v0.2.0); por defecto, la última
#   GACHAPON_URL_BASE  otra URL de descarga (pruebas); por defecto, las Releases de GitHub

set -eu

REPO_URL="https://github.com/jesus13g/gachapon"

fallo() {
	printf 'gachapon: %s\n' "$1" >&2
	exit 1
}

aviso() {
	printf 'gachapon: %s\n' "$1" >&2
}

hay() {
	command -v "$1" >/dev/null 2>&1
}

# La confirmación se da en una terminal real: con `curl | sh` la entrada
# estándar es el propio script, así que `ea instalar` lee de /dev/tty.
comprobar_tty() {
	if ! (: </dev/tty) 2>/dev/null; then
		fallo "hace falta una terminal interactiva (/dev/tty) para confirmar la instalación; ejecuta el comando en una terminal"
	fi
}

if [ "$(id -u)" = "0" ]; then
	fallo "no lo ejecutes como root: gachapon se instala para tu usuario (los servicios son de usuario); si hace falta sudo, se pedirá"
fi

# ¿Dentro de WSL? Allí la app de escritorio va en Windows, no en Linux.
en_wsl() {
	grep -qi microsoft /proc/version 2>/dev/null
}

con_sin_escritorio() {
	for a in "$@"; do
		[ "$a" = "--sin-escritorio" ] && return 0
	done
	return 1
}

# ------------------------------------------------------------ desde un clon

dir_script=""
case "$0" in
	*instalar.sh) dir_script=$(cd "$(dirname "$0")" && pwd) ;;
esac

es_clon() {
	[ -n "$dir_script" ] &&
		[ -f "$dir_script/Cargo.toml" ] &&
		grep -q '^\[workspace\]' "$dir_script/Cargo.toml" &&
		[ -d "$dir_script/crates/ea-cli" ]
}

if es_clon; then
	comprobar_tty
	hay cargo || fallo "para instalar desde el repo hace falta Rust (cargo): https://rustup.rs; o instala la última versión publicada con: curl -fsSL https://jesus13g.github.io/gachapon/instalar.sh | sh"
	cd "$dir_script"
	printf 'gachapon: compilando desde %s…\n' "$dir_script"
	cargo build --release
	if con_sin_escritorio "$@" || en_wsl; then
		:
	elif ! cargo build --release --manifest-path escritorio/Cargo.toml; then
		aviso "no se pudo compilar la app de escritorio (en Linux necesita WebKitGTK 4.1 y libayatana-appindicator3); sigo sin ella: el panel se abre en el navegador"
	fi
	if en_wsl && ! con_sin_escritorio "$@"; then
		set -- "$@" --sin-escritorio
	fi
	exec "$dir_script/target/release/ea" instalar "$@" </dev/tty
fi

# ------------------------------------------------------- desde una Release

comprobar_tty

sistema=$(uname -s)
arquitectura=$(uname -m)
case "$sistema" in
	Linux)
		case "$arquitectura" in
			x86_64 | amd64) objetivo="x86_64-unknown-linux-gnu" ;;
			aarch64 | arm64) objetivo="aarch64-unknown-linux-gnu" ;;
			*) fallo "no hay binarios publicados para Linux $arquitectura; compila desde el repo: git clone $REPO_URL && ./gachapon/instalar.sh" ;;
		esac
		;;
	Darwin) objetivo="universal-apple-darwin" ;;
	*) fallo "sistema no soportado por este script: $sistema (en Windows usa instalar.ps1 o gachapon-instalar.exe)" ;;
esac

if [ -n "${GACHAPON_URL_BASE:-}" ]; then
	base="$GACHAPON_URL_BASE"
elif [ -n "${GACHAPON_VERSION:-}" ]; then
	base="$REPO_URL/releases/download/$GACHAPON_VERSION"
else
	base="$REPO_URL/releases/latest/download"
fi

paquete="gachapon-$objetivo.tar.gz"

descargar() {
	if hay curl; then
		curl -fsSL --retry 3 -o "$2" "$1"
	elif hay wget; then
		wget -q -O "$2" "$1"
	else
		fallo "hace falta curl o wget para descargar"
	fi
}

suma() {
	if hay sha256sum; then
		sha256sum "$1" | cut -d ' ' -f 1
	elif hay shasum; then
		shasum -a 256 "$1" | cut -d ' ' -f 1
	else
		fallo "hace falta sha256sum o shasum para comprobar la descarga"
	fi
}

tmp=$(mktemp -d 2>/dev/null || mktemp -d -t gachapon)
trap 'rm -rf "$tmp"' EXIT
trap 'exit 130' INT TERM

printf 'gachapon: descargando %s…\n' "$paquete"
descargar "$base/$paquete" "$tmp/$paquete" || fallo "no se pudo descargar $base/$paquete"
descargar "$base/SHA256SUMS" "$tmp/SHA256SUMS" || fallo "no se pudo descargar $base/SHA256SUMS"

esperada=$(awk -v f="$paquete" '{ n = $2; sub(/^\*/, "", n); if (n == f) print $1 }' "$tmp/SHA256SUMS")
[ -n "$esperada" ] || fallo "SHA256SUMS no trae la suma de $paquete"
obtenida=$(suma "$tmp/$paquete")
if [ "$esperada" != "$obtenida" ]; then
	fallo "la suma SHA-256 de $paquete no coincide (esperada $esperada, obtenida $obtenida); no se instala nada"
fi

mkdir "$tmp/paquete"
tar -xzf "$tmp/$paquete" -C "$tmp/paquete"
[ -x "$tmp/paquete/ea" ] || fallo "el paquete no trae el binario ea"

if en_wsl && ! con_sin_escritorio "$@"; then
	set -- "$@" --sin-escritorio
fi

# Sin exec: al terminar se borra lo temporal (ea instalar ya copió los
# binarios a su carpeta estable).
"$tmp/paquete/ea" instalar "$@" </dev/tty
