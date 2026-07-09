# fetch-fonts.ps1
# Downloads IBM Plex Sans (Regular / Medium / SemiBold) into this folder so it
# can be embedded as the UI font. See README.md for how to enable it in theme.rs.
#
# Run from the gui/fonts directory:
#   powershell -ExecutionPolicy Bypass -File .\fetch-fonts.ps1

$ErrorActionPreference = 'Stop'
$base = 'https://raw.githubusercontent.com/IBM/plex/master/packages/plex-sans/fonts/complete/ttf'
$files = @(
    'IBMPlexSans-Regular.ttf',
    'IBMPlexSans-Medium.ttf',
    'IBMPlexSans-SemiBold.ttf'
)

$dest = $PSScriptRoot
foreach ($f in $files) {
    $url = "$base/$f"
    $out = Join-Path $dest $f
    Write-Host "Downloading $f ..."
    try {
        Invoke-WebRequest -Uri $url -OutFile $out -UseBasicParsing
    } catch {
        Write-Warning "Failed: $url"
        Write-Warning "If the repo layout changed, grab the TTFs manually from https://github.com/IBM/plex"
    }
}
Write-Host "Done. Now enable Plex Sans in src/theme.rs (see README.md)."
