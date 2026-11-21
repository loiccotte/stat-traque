# Envoie tes sessions enregistrées (anonymisées) pour que Claude puisse les rejouer.
# Utilisation, depuis le dossier du projet :  powershell -File scripts\envoyer.ps1
# Copie les enregistrements + le journal dans enregistrements\, puis commit + push.
$ErrorActionPreference = "Stop"
$source = Join-Path $env:APPDATA "stat-traque\enregistrements"
$cible = Join-Path $PSScriptRoot "..\enregistrements"
New-Item -ItemType Directory -Force -Path $cible | Out-Null

$fichiers = Get-ChildItem -Path $source -Filter *.jsonl -ErrorAction SilentlyContinue |
    Where-Object { $_.Length -gt 0 }
if (-not $fichiers) { "Aucun enregistrement : lance stat-traque et joue au moins une partie."; exit 1 }
$fichiers | Copy-Item -Destination $cible -Force
$date = Get-Date -Format "yyyy-MM-dd_HHmm"
$journal = Join-Path $env:APPDATA "stat-traque\journal.txt"
if (Test-Path $journal) { Copy-Item $journal (Join-Path $cible "journal-$date.txt") -Force }

"$($fichiers.Count) session(s) copiée(s). Envoi..."
git add $cible
git commit -m "Enregistrements de test ($date)" | Out-Null
git push
"Envoyé. Tu peux continuer à jouer normalement."
