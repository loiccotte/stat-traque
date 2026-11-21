//! Commande `stat-traque essai-fenetre` : ouvre la fenêtre et l'overlay avec des
//! données de démonstration et MESURE la RAM et le CPU du processus à chaque étape.
//! Sert à vérifier les budgets sur le vrai PC (Windows) sans lancer LoL.

use std::time::{Duration, Instant};

use super::{basculer_overlay, demarrer, demo, fermer, ouvrir};
use crate::affichage;
use crate::gameflow::Phase;

/// RAM réellement occupée par le processus (« working set » / RSS), en Mo.
pub fn memoire_mo() -> f64 {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        // SAFETY : structure C simple remplie par Windows ; on lui donne sa taille.
        unsafe {
            let mut c: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
            c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
            if K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) != 0 {
                return c.WorkingSetSize as f64 / 1_048_576.0;
            }
        }
        0.0
    }
    #[cfg(not(windows))]
    {
        // Linux : ligne « VmRSS:  12345 kB » de /proc/self/status.
        let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
        status
            .lines()
            .find_map(|l| l.strip_prefix("VmRSS:"))
            .and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<f64>().ok())
            .map_or(0.0, |kb| kb / 1024.0)
    }
}

/// Temps CPU consommé par le processus depuis son lancement (tous threads).
pub fn temps_cpu() -> Duration {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::FILETIME;
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
        let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let (mut a, mut b, mut noyau, mut utilisateur) = (zero, zero, zero, zero);
        // SAFETY : Windows remplit les 4 structures qu'on lui passe.
        unsafe { GetProcessTimes(GetCurrentProcess(), &mut a, &mut b, &mut noyau, &mut utilisateur) };
        let cent_ns = |f: FILETIME| ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64;
        Duration::from_nanos((cent_ns(noyau) + cent_ns(utilisateur)) * 100)
    }
    #[cfg(not(windows))]
    {
        // Champs 14 et 15 de /proc/self/stat : temps utilisateur et noyau, en « ticks » (1/100 s).
        let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
        let apres_nom = stat.rsplit_once(')').map_or("", |(_, r)| r);
        let champs: Vec<u64> = apres_nom.split_whitespace().filter_map(|c| c.parse().ok()).collect();
        let ticks = champs.get(11).copied().unwrap_or(0) + champs.get(12).copied().unwrap_or(0);
        Duration::from_millis(ticks * 10)
    }
}

/// Attend `secondes` sans rien changer, puis affiche RAM + CPU consommé pendant l'attente.
fn etape(nom: &str, secondes: u64) {
    let cpu = temps_cpu();
    let debut = Instant::now();
    std::thread::sleep(Duration::from_secs(secondes));
    let pourcent = (temps_cpu() - cpu).as_secs_f64() * 100.0 / debut.elapsed().as_secs_f64();
    println!("{nom:<42} RAM {:>6.1} Mo   CPU {pourcent:>5.2} %", memoire_mo());
}

/// Déroulé de l'essai (environ 40 s). Fermer la fenêtre à la main n'est pas nécessaire.
pub fn essai() {
    println!("stat-traque — essai de la fenêtre (données de démonstration)\n");
    etape("1. Au repos, aucune fenêtre", 3);
    demarrer();
    affichage::modifier(|e| *e = demo::etat(Phase::ChampSelect));
    etape("2. Thread fenêtre prêt, aucune fenêtre", 3);
    ouvrir();
    etape("3. Fenêtre ouverte (champ select)", 6);
    etape("4. Fenêtre ouverte, rien ne change", 5);
    affichage::modifier(|e| *e = demo::etat(Phase::InProgress));
    etape("5. Fenêtre ouverte (onglet partie)", 4);
    fermer();
    etape("6. Fenêtre fermée", 4);
    basculer_overlay();
    etape("7. Overlay affiché", 6);
    etape("8. Overlay, rien ne change", 5);
    fermer();
    etape("9. Tout fermé", 4);
    println!("\nBudgets : < 15 Mo fenêtre fermée, < 40 Mo en partie (overlay), CPU ~0 % quand rien ne change.");
}
