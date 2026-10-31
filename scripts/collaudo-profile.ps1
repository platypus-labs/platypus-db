<#
.SYNOPSIS
  Copia, ripristina ed elenca i dati di PlatypusDB di questo utente (per il collaudo dell'aggiornamento).
.DESCRIPTION
  Le cartelle sono %APPDATA%\dev.platypusdb.desktop (connessioni, impostazioni, script, cronologia, log)
  e %LOCALAPPDATA%\dev.platypusdb.desktop (archivio della web view). Le password sono nel Credential
  Manager di Windows, servizio "platypusdb": questo script non le copia, le elenca soltanto.
  Senza opzioni elenca i file e i profili salvati. -Backup copia; -Restore ripristina e, prima di
  sostituire qualcosa, salva lo stato attuale in un profilo "prima-del-ripristino-<data>".
  Nessuna cartella dell'utente viene mai cancellata senza una copia fatta prima.
#>
[CmdletBinding()]
param(
    [switch]$Backup,
    [switch]$Restore,
    [switch]$Compare,
    [string]$Name = (Get-Date -Format 'yyyyMMdd-HHmmss'),
    [string]$Root = (Join-Path $env:USERPROFILE 'PlatypusDB-collaudo')
)
$ErrorActionPreference = 'Stop'
$id = 'dev.platypusdb.desktop'
$sources = @{ roaming = (Join-Path $env:APPDATA $id); local = (Join-Path $env:LOCALAPPDATA $id) }

function Copy-Profile($to) {
    New-Item -ItemType Directory -Force -Path $to | Out-Null
    foreach ($k in $sources.Keys) {
        if (Test-Path $sources[$k]) { Copy-Item -Recurse -Force $sources[$k] (Join-Path $to $k) }
    }
}
function Get-Hashes($base) {
    Get-ChildItem -Recurse -File $base -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -notmatch '\(logs|EBWebView)\' } |
        ForEach-Object { [pscustomobject]@{ File = $_.FullName.Substring($base.Length); Bytes = $_.Length; Sha = (Get-FileHash $_.FullName -Algorithm SHA256).Hash } }
}

if (Get-Process -Name 'platypus-db' -ErrorAction SilentlyContinue) {
    if ($Backup -or $Restore -or $Compare) { throw 'PlatypusDB e aperto: chiudilo prima (anche dal vassoio).' }
}

if ($Backup) {
    $to = Join-Path $Root $Name
    if (Test-Path $to) { throw "Il profilo '$Name' esiste gia: scegli un altro -Name." }
    Copy-Profile $to
    Write-Host "Copia fatta in $to"
    return
}
if ($Restore) {
    $from = Join-Path $Root $Name
    if (-not (Test-Path $from)) { throw "Il profilo '$Name' non esiste in $Root." }
    $safe = Join-Path $Root ("prima-del-ripristino-" + (Get-Date -Format 'yyyyMMdd-HHmmss'))
    Copy-Profile $safe
    Write-Host "Stato attuale salvato in $safe"
    foreach ($k in $sources.Keys) {
        $src = Join-Path $from $k
        if (Test-Path $src) {
            if (Test-Path $sources[$k]) { Remove-Item -Recurse -Force $sources[$k] }
            Copy-Item -Recurse -Force $src $sources[$k]
        }
    }
    Write-Host "Ripristinato il profilo '$Name'."
    return
}
if ($Compare) {
    $from = Join-Path $Root $Name
    if (-not (Test-Path $from)) { throw "Il profilo '$Name' non esiste in $Root." }
    foreach ($k in $sources.Keys) {
        $a = Get-Hashes (Join-Path $from $k); $b = Get-Hashes $sources[$k]
        Write-Host "== $k (< solo nella copia, > solo ora, = diverso)"
        $mapA = @{}; $a | ForEach-Object { $mapA[$_.File] = $_ }
        $mapB = @{}; $b | ForEach-Object { $mapB[$_.File] = $_ }
        $mapA.Keys | Where-Object { -not $mapB.ContainsKey($_) } | ForEach-Object { "< $_" }
        $mapB.Keys | Where-Object { -not $mapA.ContainsKey($_) } | ForEach-Object { "> $_" }
        $mapA.Keys | Where-Object { $mapB.ContainsKey($_) -and $mapA[$_].Sha -ne $mapB[$_].Sha } | ForEach-Object { "= $_" }
    }
    return
}

foreach ($k in $sources.Keys) {
    Write-Host "== $k : $($sources[$k])"
    if (Test-Path $sources[$k]) {
        Get-ChildItem -Recurse -File $sources[$k] | Where-Object { $_.FullName -notmatch '\EBWebView\' } |
            ForEach-Object { '{0,10}  {1}' -f $_.Length, $_.FullName.Substring($sources[$k].Length + 1) }
    } else { '(non esiste)' }
}
Write-Host '== Credential Manager (servizio platypusdb)'
cmdkey /list | Select-String 'platypusdb'
Write-Host "== Profili salvati in $Root"
if (Test-Path $Root) { Get-ChildItem -Directory $Root | ForEach-Object Name }
