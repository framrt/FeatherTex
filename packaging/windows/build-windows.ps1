param(
    [string]$Version = "0.1.0"
)

$ErrorActionPreference = "Stop"
$RootDir = (Resolve-Path "$PSScriptRoot\..\..").Path

Push-Location $RootDir
try {
    cargo build --release --locked
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build --release failed with exit code $LASTEXITCODE"
    }

    $StageDir = Join-Path $env:TEMP ("feathertex_win_" + [guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Force -Path $StageDir | Out-Null
    $BundleDir = Join-Path $StageDir "FeatherTex-$Version-windows-x86_64"
    New-Item -ItemType Directory -Force -Path $BundleDir | Out-Null

    Copy-Item "$RootDir\target\release\feathertex.exe" "$BundleDir\feathertex.exe"

    $DemoDest = Join-Path $BundleDir "demo"
    New-Item -ItemType Directory -Force -Path (Join-Path $DemoDest "chapters") | Out-Null
    Copy-Item "$RootDir\demo\main.tex" "$DemoDest\main.tex"
    Copy-Item "$RootDir\demo\references.bib" "$DemoDest\references.bib"
    Copy-Item "$RootDir\demo\chapters\intro.tex" "$DemoDest\chapters\intro.tex"
    Copy-Item "$RootDir\demo\chapters\results.tex" "$DemoDest\chapters\results.tex"

    @"
FeatherTex v$Version (Windows x86_64)
===================================

Requisiti consigliati nel PATH di sistema:
1. Distribuzione LaTeX: MiKTeX o TeX Live (pdflatex, bibtex, biber, chktex, texcount)
2. Anteprima PDF integrata: Poppler per Windows (pdfinfo.exe, pdftoppm.exe / pdftocairo.exe)

Progetto dimostrativo:
Apri il file 'demo\main.tex' con il pulsante "Apri" dentro FeatherTex.
"@ | Set-Content -Encoding UTF8 "$BundleDir\README-Windows.txt"

    $DistDir = Join-Path $RootDir "dist"
    New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
    $OutZip = Join-Path $DistDir "feathertex-$Version-windows-x86_64.zip"
    if (Test-Path $OutZip) { Remove-Item $OutZip -Force }
    Compress-Archive -Path "$BundleDir\*" -DestinationPath $OutZip

    Remove-Item $StageDir -Recurse -Force
    Write-Host "Pacchetto Windows creato: $OutZip"
}
finally {
    Pop-Location
}

