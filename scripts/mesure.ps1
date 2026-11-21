# Banc de mesure perf de stat-traque (Windows). Vérifie les budgets de CLAUDE.md.
# Utilisation :  cargo build --release ; powershell -File scripts\mesure.ps1
# Code de sortie 1 si un budget est dépassé (utilisé par la CI).
param(
    [string]$Exe = "target\release\stat-traque.exe",
    [int]$Secondes = 10
)
$ErrorActionPreference = "Stop"
$budgets = @{ TailleMo = 8; RamReposMo = 15; CpuReposPct = 0.5; DemarrageMs = 300 }
$horsBudget = @()

# 1) Taille de l'exe
$tailleMo = (Get-Item $Exe).Length / 1MB
"Taille exe      : {0:N2} Mo  (budget < {1} Mo)" -f $tailleMo, $budgets.TailleMo
if ($tailleMo -ge $budgets.TailleMo) { $horsBudget += "taille" }

# 2) RAM et CPU au repos (client LoL fermé : c'est l'état le plus fréquent)
$p = Start-Process -FilePath $Exe -PassThru
Start-Sleep -Seconds 3
$p.Refresh(); $cpu0 = $p.TotalProcessorTime
Start-Sleep -Seconds $Secondes
$p.Refresh()
$cpuPct = ($p.TotalProcessorTime - $cpu0).TotalMilliseconds / ($Secondes * 1000 * [Environment]::ProcessorCount) * 100
$ramMo = $p.WorkingSet64 / 1MB
$priveeMo = $p.PrivateMemorySize64 / 1MB
$pic = $p.PeakWorkingSet64 / 1MB
Stop-Process -Id $p.Id -Force
"RAM au repos    : {0:N1} Mo (privée {1:N1} Mo, pic {2:N1} Mo)  (budget < {3} Mo)" -f $ramMo, $priveeMo, $pic, $budgets.RamReposMo
"CPU au repos    : {0:N3} %  (budget < {1} %)" -f $cpuPct, $budgets.CpuReposPct
if ($ramMo -ge $budgets.RamReposMo) { $horsBudget += "ram" }
if ($cpuPct -ge $budgets.CpuReposPct) { $horsBudget += "cpu" }

# 3) Démarrage : l'app écrit "Prêt en X ms." dans son journal
$journal = Join-Path $env:APPDATA "stat-traque\journal.txt"
$ligne = Select-String -Path $journal -Pattern "Prêt en (\d+) ms" | Select-Object -First 1
if ($ligne) {
    $ms = [int]$ligne.Matches[0].Groups[1].Value
    "Démarrage       : $ms ms  (budget < $($budgets.DemarrageMs) ms)"
    if ($ms -ge $budgets.DemarrageMs) { $horsBudget += "demarrage" }
}

if ($horsBudget.Count -gt 0) { "HORS BUDGET : $($horsBudget -join ', ')"; exit 1 }
"OK : tous les budgets sont respectés."
