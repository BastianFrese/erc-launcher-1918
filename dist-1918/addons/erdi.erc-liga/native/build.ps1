# Builds liga_plugin.dll with MSVC (the SDK template uses zig; MSVC is what this machine has).
# Output: native/liga_plugin.dll

$ErrorActionPreference = 'Stop'

$hier = $PSScriptRoot

# vcvars64.bat suchen: erst vswhere, dann feste Pfade (PowerShell 5.1 und der (x86)-Pfad
# vertragen sich nicht immer, deshalb beides).
$vcvars = $null
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (Test-Path $vswhere) {
    $vs = & $vswhere -latest -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($vs) { $vcvars = Join-Path $vs.Trim() 'VC\Auxiliary\Build\vcvars64.bat' }
}
if (-not $vcvars -or -not (Test-Path $vcvars)) {
    $kandidaten = Get-ChildItem 'C:\Program Files*\Microsoft Visual Studio' -Directory -ErrorAction SilentlyContinue |
        ForEach-Object { Join-Path $_.FullName '*\VC\Auxiliary\Build\vcvars64.bat' } |
        ForEach-Object { Get-ChildItem $_ -ErrorAction SilentlyContinue } |
        Sort-Object FullName -Descending
    if ($kandidaten) { $vcvars = $kandidaten[0].FullName }
}
if (-not $vcvars -or -not (Test-Path $vcvars)) { throw 'vcvars64.bat nicht gefunden - MSVC fehlt' }

$quelle = Join-Path $hier 'src\liga_plugin.cpp'
if (-not (Test-Path $quelle)) { throw "Quelldatei fehlt: $quelle" }

$bau = Join-Path $hier 'build'
New-Item -ItemType Directory -Force -Path $bau | Out-Null

$zeilen = @(
    "call `"$vcvars`"",
    "cd /d `"$bau`"",
    "cl /nologo /std:c++latest /Zc:preprocessor /O2 /EHsc /W4 /LD /MD /I`"$hier\include`" `"$quelle`" /Fe:`"$hier\liga_plugin.dll`""
)

Push-Location $bau
try {
    $zeilen | Set-Content -Path (Join-Path $bau 'bau.cmd') -Encoding ascii
    cmd /c (Join-Path $bau 'bau.cmd')
    if ($LASTEXITCODE -ne 0) { throw "cl.exe meldet Fehler $LASTEXITCODE" }
} finally {
    Pop-Location
}

$dll = Join-Path $hier 'liga_plugin.dll'
Write-Output "fertig: $dll ($([math]::Round((Get-Item $dll).Length / 1KB, 1)) KB)"
