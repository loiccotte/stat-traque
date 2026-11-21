# Prépare un zip à envoyer aux amis (pas besoin d'installer Rust chez eux).
# Utilisation : powershell -File scripts\paquet.ps1   ->  paquet\stat-traque.zip
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
cargo build --release
$dossier = "paquet\stat-traque"
Remove-Item -Recurse -Force paquet -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $dossier | Out-Null
Copy-Item target\release\stat-traque.exe $dossier

# Raccourci : lance l'app avec la console (pas encore de fenêtre graphique).
"@echo off`r`nstart `"`" `"%~dp0stat-traque.exe`" --console" | Set-Content -Encoding ASCII "$dossier\Lancer stat-traque.bat"

# Stats de runes collectées sur ce PC (stat-traque collecter) : copiées à côté de l'exe.
$stats = Get-ChildItem (Join-Path $env:APPDATA "stat-traque\cache\stats") -Filter "stats-*.tsv" -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
if ($stats) { Copy-Item $stats.FullName $dossier; "Stats incluses : $($stats.Name)" } else { "ATTENTION : pas de stats de runes (lance d'abord : stat-traque collecter 30)" }

@"
stat-traque - alternative légère à Blitz (projet perso, gratuit)

1. Double-clic sur "Lancer stat-traque.bat" (garde la fenêtre ouverte pendant que tu joues).
   Si Windows affiche "Windows a protégé votre ordinateur" : Informations complémentaires
   puis Exécuter quand même (l'exe n'est pas signé).
2. En champ select : analyse des deux compos, picks conseillés, et au verrouillage
   ta page de runes "stat-traque ..." et tes sorts sont choisis automatiquement.
   Garde au moins une place libre dans tes pages de runes.
3. Réglages (fichier %APPDATA%\stat-traque\reglages.txt, une ligne par réglage) :
     flash=f        Flash sur F (par défaut : D)
     runes=non      ne pas changer mes runes
     sorts=non      ne pas changer mes sorts
     accepter=oui   accepter la partie automatiquement (désactivé par défaut)
     maj=non        pas de mise à jour automatique
   Lancement avec Windows : stat-traque demarrage oui   (ou non)
4. Mise à jour automatique : l'ancienne version reste à côté (stat-traque.exe.ancien)
   jusqu'au 2e lancement de la nouvelle. Pour revenir en arrière (ou si stat-traque.exe
   a disparu après une coupure) : quitter stat-traque, supprimer stat-traque.exe s'il
   existe, renommer stat-traque.exe.ancien en stat-traque.exe, mettre maj=non dans les réglages.
Pour arrêter : ferme la fenêtre noire.
"@ | Set-Content -Encoding UTF8 "$dossier\LISEZMOI.txt"

Compress-Archive -Path $dossier -DestinationPath paquet\stat-traque.zip -Force
"Prêt : paquet\stat-traque.zip (à envoyer sur Discord)"
