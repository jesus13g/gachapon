# Instalación de gachapon en un paso (spec 023) para Windows.
#
#   irm https://jesus13g.github.io/gachapon/instalar.ps1 | iex
#
# Descarga gachapon-instalar.exe de la última Release, comprueba su suma
# SHA-256 y lo ejecuta en esta misma consola: allí `ea instalar` pregunta la
# vía (WSL2 o nativa), enseña el plan y pide la confirmación.
#
# Variables de entorno:
#   GACHAPON_VERSION   una versión concreta (p. ej. v0.2.0); por defecto, la última
#   GACHAPON_URL_BASE  otra URL de descarga (pruebas)
#
# Con `iex` no hay argumentos; para pasarlos a `ea instalar`, guarda el
# script y ejecútalo: .\instalar.ps1 --via wsl

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$repo = 'https://github.com/jesus13g/gachapon'
if ($env:GACHAPON_URL_BASE) {
    $base = $env:GACHAPON_URL_BASE
} elseif ($env:GACHAPON_VERSION) {
    $base = "$repo/releases/download/$($env:GACHAPON_VERSION)"
} else {
    $base = "$repo/releases/latest/download"
}

$nombre = 'gachapon-instalar.exe'
$dir = Join-Path ([System.IO.Path]::GetTempPath()) ("gachapon-" + [System.Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $dir | Out-Null

try {
    $exe = Join-Path $dir $nombre
    $sumas = Join-Path $dir 'SHA256SUMS'
    Write-Host "gachapon: descargando $nombre..."
    Invoke-WebRequest -UseBasicParsing -Uri "$base/$nombre" -OutFile $exe
    Invoke-WebRequest -UseBasicParsing -Uri "$base/SHA256SUMS" -OutFile $sumas

    $esperada = $null
    foreach ($linea in Get-Content $sumas) {
        $partes = $linea -split '\s+', 2
        if ($partes.Count -eq 2 -and $partes[1].TrimStart('*').Trim() -eq $nombre) {
            $esperada = $partes[0].ToLowerInvariant()
        }
    }
    if (-not $esperada) {
        throw "SHA256SUMS no trae la suma de $nombre"
    }
    $obtenida = (Get-FileHash -Algorithm SHA256 -Path $exe).Hash.ToLowerInvariant()
    if ($esperada -ne $obtenida) {
        throw "la suma SHA-256 de $nombre no coincide (esperada $esperada, obtenida $obtenida); no se instala nada"
    }

    # En la misma consola; sin pausa final porque la consola ya es la tuya.
    & $exe --sin-pausa @args
    $codigo = $LASTEXITCODE
} catch {
    Write-Host "gachapon: $($_.Exception.Message)" -ForegroundColor Red
    $codigo = 1
} finally {
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $dir
}

if ($codigo -ne 0) {
    Write-Host "gachapon: la instalación terminó con el código $codigo"
}
