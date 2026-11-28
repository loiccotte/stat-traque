# Prépare un zip PORTABLE à envoyer à un testeur (il n'a pas besoin d'installer Rust).
# Utilisation :  powershell -File scripts\paquet.ps1   ->  paquet\stat-traque.zip
#   -SansCompiler : réutilise l'exe déjà compilé par ce script (target\paquet).
#
# Ce que le script garantit (et vérifie avant de zipper) :
#  - les chemins de CE PC (dont ton nom d'utilisateur Windows) ne se retrouvent pas dans l'exe ;
#  - aucune clé API n'est dans l'exe ;
#  - le zip ne contient que : l'exe, le lanceur console, LISEZMOI.txt, VERSION.txt, SHA256.txt
#    (et le fichier de stats de runes s'il existe). Jamais ta clé, ton journal ni tes enregistrements.
param([switch]$SansCompiler)
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
$racine = (Get-Location).Path

# cargo n'est pas toujours dans le PATH (installation par rustup).
$cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
if (-not (Get-Command cargo -ErrorAction SilentlyContinue) -and (Test-Path $cargoBin)) { $env:PATH += ";$cargoBin" }

# 1) Compilation « portable » : on remplace le dossier personnel par un faux chemin dans l'exe
#    (sinon les messages d'erreur du programme contiennent C:\Users\<ton nom>\...).
#    Dossier de compilation séparé : ça ne dérange pas les autres compilations.
$env:RUSTFLAGS = "--remap-path-prefix=$($env:USERPROFILE)=C:\build"
$env:CARGO_TARGET_DIR = Join-Path $racine "target\paquet"
if (-not $SansCompiler) {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "La compilation a échoué." }
}
$exe = Join-Path $env:CARGO_TARGET_DIR "release\stat-traque.exe"
if (-not (Test-Path $exe)) { throw "Exe introuvable : $exe (lance sans -SansCompiler)." }

# 2) Vérification de confidentialité : on cherche dans les octets de l'exe, en texte simple
#    ET en UTF-16 (Windows), les chemins de ce PC et une éventuelle clé API.
# ISO-8859-1 (1 octet = 1 caractère) : on cherche des motifs dans les octets bruts.
# (GetEncoding(28591) marche aussi bien sous Windows PowerShell 5.1 que sous PowerShell 7.)
$latin1 = [Text.Encoding]::GetEncoding(28591)
$contenu = $latin1.GetString([IO.File]::ReadAllBytes($exe))
$motifs = @(
    $env:USERPROFILE,
    ($env:USERPROFILE -replace '\\', '/'),
    "\Users\$($env:USERNAME)",
    "/Users/$($env:USERNAME)"
) | Select-Object -Unique
$fuites = @()
foreach ($m in $motifs) {
    if ($contenu.Contains($m)) { $fuites += "texte : $m" }
    $m16 = $latin1.GetString([Text.Encoding]::Unicode.GetBytes($m))
    if ($contenu.Contains($m16)) { $fuites += "UTF-16 : $m" }
}
if ($contenu -match 'RGAPI-[0-9a-fA-F]{8}') { $fuites += "une clé API Riot" }
if ($fuites.Count -gt 0) {
    throw "FUITE : l'exe contient des données personnelles ($($fuites -join ' ; ')). Paquet NON créé."
}
"Vérification de confidentialité : OK (aucun chemin personnel, aucune clé)."

# 3) Contenu du paquet
$dossier = "paquet\stat-traque"
Remove-Item -Recurse -Force paquet -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $dossier | Out-Null
Copy-Item $exe $dossier

# Raccourci pour le mode console (dépannage : on voit tous les messages).
"@echo off`r`nstart `"`" `"%~dp0stat-traque.exe`" --console" | Set-Content -Encoding ASCII "$dossier\Mode console (depannage).bat"

# Stats de runes collectées sur ce PC (stat-traque collecter) : copiées à côté de l'exe.
$stats = Get-ChildItem (Join-Path $env:APPDATA "stat-traque\cache\stats") -Filter "stats-*.tsv" -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
if ($stats) { Copy-Item $stats.FullName $dossier; "Stats incluses : $($stats.Name)" }
else { "Pas de fichier de stats de runes : le testeur n'aura pas l'import auto des runes (rangs et tableau fonctionnent)." }

# Version : commit + date, pour savoir quel exe le testeur utilise.
$commit = (git rev-parse --short HEAD 2>$null)
$branche = (git branch --show-current 2>$null)
"stat-traque (test)`r`ncommit : $commit ($branche)`r`ncompilé le : $(Get-Date -Format 'yyyy-MM-dd HH:mm')" |
    Set-Content -Encoding UTF8 "$dossier\VERSION.txt"

@"
stat-traque - alternative légère à Blitz (projet perso, gratuit, sans pub ni compte)
VERSION DE TEST : merci d'aider !

CE QU'IL FAUT SAVOIR
- Tout reste sur ton PC : l'app ne s'installe pas, n'envoie rien à personne (pas de
  télémétrie) et parle seulement au client LoL (en local), à Riot et à Data Dragon.
- Rien n'est automatisé en jeu. Elle n'affiche que ce que le client et les API officielles donnent.

1. LANCER
   Double-clic sur stat-traque.exe : une icône apparaît près de l'horloge (peut-être sous la
   flèche ^). Clic gauche = ouvrir la fenêtre ; clic droit = menu (overlay, quitter).
   Si Windows affiche "Windows a protégé votre ordinateur" : Informations complémentaires,
   puis Exécuter quand même (l'exe n'est pas signé).
   Si Windows BLOQUE sans proposer d'exécuter (Smart App Control activé) : dis-le-moi,
   il faut soit le désactiver (Sécurité Windows > Contrôle des applications), soit que je
   signe l'exe.

2. CLÉ API RIOT (pour les rangs, l'historique et les fiches)
   Sans clé, l'app marche mais sans rangs. Crée une clé de développement sur
   https://developer.riotgames.com (elle dure 24 h), puis dans PowerShell, dans le dossier
   de l'exe :   .\stat-traque.exe cle RGAPI-xxxxxxxx
   La clé reste dans %APPDATA%\stat-traque\ et n'est jamais envoyée ailleurs qu'à Riot.
   Serveur : EUW par défaut ; sinon   .\stat-traque.exe region na1   (eun1, kr, na1...)

3. À TESTER (dis-moi ce qui marche ou pas)
   - L'icône, la fenêtre (clic gauche, croix, rouvrir).
   - En partie : onglet Partie (tableau des 10 joueurs avec rangs).
   - Overlay : dans LoL, options vidéo, mode d'affichage "Sans bordure" (pas "Plein écran"),
     puis Ctrl+Maj+Tab en jeu pour l'afficher / le masquer.
   - Pas de lag en jeu, pas de plantage. Note la RAM dans le Gestionnaire des tâches si tu peux
     (stat-traque.exe : environ 5 à 10 Mo au repos).

4. ME RENVOYER LES TRACES (facultatif, anonymisé)
   L'app note chaque partie dans %APPDATA%\stat-traque\enregistrements\ (pseudos remplacés
   par J-xxxx). Envoie-moi seulement le dernier fichier .jsonl de ce dossier, pas le reste.
   Mode dépannage : "Mode console (depannage).bat" affiche tous les messages.

5. DÉSINSTALLER
   Clic droit sur l'icône > Quitter, supprime stat-traque.exe, puis le dossier
   %APPDATA%\stat-traque\ . Si tu as activé le démarrage avec Windows :
   .\stat-traque.exe demarrage non   (avant de supprimer).
"@ | Set-Content -Encoding UTF8 "$dossier\LISEZMOI.txt"

# Empreinte du fichier : le testeur peut vérifier que l'exe n'a pas été modifié en route.
$hash = (Get-FileHash "$dossier\stat-traque.exe" -Algorithm SHA256).Hash
"$hash  stat-traque.exe" | Set-Content -Encoding ASCII "$dossier\SHA256.txt"

Compress-Archive -Path $dossier -DestinationPath paquet\stat-traque.zip -Force
$mo = [math]::Round((Get-Item paquet\stat-traque.zip).Length / 1MB, 2)
"Prêt : paquet\stat-traque.zip ($mo Mo) - SHA-256 de l'exe : $hash"
