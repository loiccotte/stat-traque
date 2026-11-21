# Prépare un zip à envoyer aux amis (pas besoin d'installer Rust chez eux).
# Utilisation : powershell -File scripts\paquet.ps1   ->  paquet\stat-traque.zip
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
cargo build --release
$dossier = "paquet\stat-traque"
Remove-Item -Recurse -Force paquet -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $dossier | Out-Null
Copy-Item target\release\stat-traque.exe $dossier

# Raccourci pour le mode console (dépannage : on voit tous les messages).
"@echo off`r`nstart `"`" `"%~dp0stat-traque.exe`" --console" | Set-Content -Encoding ASCII "$dossier\Mode console (depannage).bat"

# Stats de runes collectées sur ce PC (stat-traque collecter) : copiées à côté de l'exe.
$stats = Get-ChildItem (Join-Path $env:APPDATA "stat-traque\cache\stats") -Filter "stats-*.tsv" -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
if ($stats) { Copy-Item $stats.FullName $dossier; "Stats incluses : $($stats.Name)" } else { "ATTENTION : pas de stats de runes (lance d'abord : stat-traque collecter 30)" }

@"
stat-traque - alternative légère à Blitz (projet perso, gratuit)

1. Double-clic sur stat-traque.exe : une icône apparaît près de l'horloge.
   Clic gauche sur l'icône = ouvrir la fenêtre ; clic droit = menu (overlay, quitter).
   Si Windows affiche "Windows a protégé votre ordinateur" : Informations complémentaires
   puis Exécuter quand même (l'exe n'est pas signé).
2. En champ select la fenêtre s'ouvre toute seule : analyse des deux compos, picks
   conseillés, matchups, et au verrouillage ta page de runes "stat-traque ..." et tes
   sorts sont choisis automatiquement. Garde une place libre dans tes pages de runes.
   Fermer la fenêtre libère sa mémoire ; stat-traque reste dans l'icône.
3. OVERLAY EN JEU : Ctrl+Maj+Tab pendant la partie (ou clic droit sur l'icône) affiche
   les rangs des 10 joueurs par-dessus le jeu ; les clics passent au travers.
   IMPORTANT : dans les options vidéo de LoL, choisis le mode "Sans bordure"
   (en "Plein écran", Windows ne peut rien afficher par-dessus le jeu).
   Position : overlay_position=x,y dans les réglages (par défaut 12,12 : en haut à gauche).
4. Réglages : onglet "Réglages" de la fenêtre, ou le fichier
   %APPDATA%\stat-traque\reglages.txt (une ligne par réglage) :
     flash=f        Flash sur F (par défaut : D)
     runes=non      ne pas changer mes runes
     sorts=non      ne pas changer mes sorts
     accepter=oui   accepter la partie automatiquement (désactivé par défaut)
     maj=non        pas de mise à jour automatique
     fenetre_auto=non  ne pas ouvrir la fenêtre au début du champ select
   Lancement avec Windows : stat-traque demarrage oui   (ou non)
5. Mise à jour automatique : l'ancienne version reste à côté (stat-traque.exe.ancien)
   jusqu'au 2e lancement de la nouvelle. Pour revenir en arrière (ou si stat-traque.exe
   a disparu après une coupure) : quitter stat-traque, supprimer stat-traque.exe s'il
   existe, renommer stat-traque.exe.ancien en stat-traque.exe, mettre maj=non dans les réglages.
Pour arrêter : clic droit sur l'icône, Quitter.
"@ | Set-Content -Encoding UTF8 "$dossier\LISEZMOI.txt"

Compress-Archive -Path $dossier -DestinationPath paquet\stat-traque.zip -Force
"Prêt : paquet\stat-traque.zip (à envoyer sur Discord)"
