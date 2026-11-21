//! Commande `stat-traque essai-fenetre` : ouvre la fenêtre et l'overlay avec des
//! données de démonstration et MESURE la RAM et le CPU des DEUX processus
//! (principal = celui de l'icône, et celui de la fenêtre) à chaque étape.
//! Sert à vérifier les budgets sur le vrai PC (Windows) sans lancer LoL.

use std::time::{Duration, Instant};

use super::{basculer_overlay, demarrer, demo, fermer, mode_demo, ouvrir, pid_enfant};
use crate::affichage;
use crate::gameflow::Phase;

/// RAM d'un processus en Mo : (totale en RAM, privée). `None` = ce processus-ci.
///  - totale = « working set » Windows / RSS Linux : inclut le code des DLL
///    partagées avec les autres programmes (pilote graphique...) ;
///  - privée = mémoire propre au processus : sous Windows la mémoire « validée »
///    (colonne « Taille de validation » du Gestionnaire des tâches, onglet Détails ;
///    la colonne « Mémoire » par défaut montre le jeu de travail privé, souvent un
///    peu plus petit) ; sous Linux la RSS anonyme (RssAnon).
pub fn memoire_mo(pid: Option<u32>) -> Option<(f64, f64)> {
    const MO: f64 = 1_048_576.0;
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        // SAFETY : appels Windows simples ; la structure est remplie par Windows.
        unsafe {
            let processus = match pid {
                None => GetCurrentProcess(),
                Some(p) => OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, p),
            };
            if processus.is_null() {
                return None;
            }
            let mut c: PROCESS_MEMORY_COUNTERS_EX = std::mem::zeroed();
            c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
            let ok = K32GetProcessMemoryInfo(processus, &mut c as *mut _ as *mut _, c.cb) != 0;
            if pid.is_some() {
                CloseHandle(processus);
            }
            ok.then(|| (c.WorkingSetSize as f64 / MO, c.PrivateUsage as f64 / MO))
        }
    }
    #[cfg(not(windows))]
    {
        // Linux : lignes « VmRSS:  12345 kB » et « RssAnon: ... » de /proc/<pid>/status.
        let chemin = pid.map_or("/proc/self/status".to_string(), |p| format!("/proc/{p}/status"));
        let status = std::fs::read_to_string(chemin).ok()?;
        let ligne = |nom: &str| {
            status
                .lines()
                .find_map(|l| l.strip_prefix(nom))
                .and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<f64>().ok())
                .map_or(0.0, |kb| kb * 1024.0 / MO)
        };
        Some((ligne("VmRSS:"), ligne("RssAnon:")))
    }
}

/// Temps CPU consommé par un processus depuis son lancement (tous threads).
pub fn temps_cpu(pid: Option<u32>) -> Option<Duration> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
        use windows_sys::Win32::System::Threading::{
            GetCurrentProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let (mut a, mut b, mut noyau, mut utilisateur) = (zero, zero, zero, zero);
        // SAFETY : Windows remplit les 4 structures qu'on lui passe.
        unsafe {
            let processus = match pid {
                None => GetCurrentProcess(),
                Some(p) => OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, p),
            };
            if processus.is_null() {
                return None;
            }
            let ok = GetProcessTimes(processus, &mut a, &mut b, &mut noyau, &mut utilisateur) != 0;
            if pid.is_some() {
                CloseHandle(processus);
            }
            let cent_ns = |f: FILETIME| ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64;
            ok.then(|| Duration::from_nanos((cent_ns(noyau) + cent_ns(utilisateur)) * 100))
        }
    }
    #[cfg(not(windows))]
    {
        let chemin = pid.map_or("/proc/self/stat".to_string(), |p| format!("/proc/{p}/stat"));
        temps_depuis_stat(&std::fs::read_to_string(chemin).ok()?)
    }
}

/// Linux : temps utilisateur + noyau depuis le contenu de /proc/<pid>/stat.
/// Après le nom (entre parenthèses) viennent les champs à partir du n° 3 (l'état,
/// une LETTRE) ; utime et stime sont les champs 14 et 15, donc les n° 11 et 12
/// en comptant depuis 0. Unité : « ticks » de 1/100 s.
fn temps_depuis_stat(stat: &str) -> Option<Duration> {
    let apres_nom = stat.rsplit_once(')')?.1;
    let mut champs = apres_nom.split_whitespace();
    let utime: u64 = champs.nth(11)?.parse().ok()?;
    let stime: u64 = champs.next()?.parse().ok()?;
    Some(Duration::from_millis((utime + stime) * 10))
}

/// Windows : l'overlay natif vit dans le thread de l'icône -> on la lance ici
/// (une icône apparaît le temps de l'essai). Ailleurs : rien à faire.
pub fn avec_icone() {
    #[cfg(windows)]
    {
        static UNE_FOIS: std::sync::Once = std::sync::Once::new();
        UNE_FOIS.call_once(|| {
            let (tx, _rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _garder = _rx; // le canal reste ouvert pendant l'essai
                crate::tray::lancer(tx);
            });
            std::thread::sleep(Duration::from_millis(300));
        });
    }
}

/// Attend `secondes` sans rien changer, puis affiche RAM + CPU des deux processus.
fn etape(nom: &str, secondes: u64) {
    let enfant = pid_enfant();
    let cpu = (temps_cpu(None), enfant.and_then(|p| temps_cpu(Some(p))));
    let debut = Instant::now();
    std::thread::sleep(Duration::from_secs(secondes));
    let duree = debut.elapsed().as_secs_f64();
    let pourcent = |avant: Option<Duration>, apres: Option<Duration>| match (avant, apres) {
        (Some(a), Some(b)) => format!("{:>5.2} %", b.saturating_sub(a).as_secs_f64() * 100.0 / duree),
        _ => "    —  ".into(),
    };
    let (totale, privee) = memoire_mo(None).unwrap_or_default();
    let principal = format!("privée {privee:>5.1} Mo (totale {totale:>5.1})  CPU {}", pourcent(cpu.0, temps_cpu(None)));
    let fenetre = match enfant.filter(|p| pid_enfant() == Some(*p)).and_then(|p| memoire_mo(Some(p)).map(|m| (p, m))) {
        Some((p, (t, pr))) => {
            format!("privée {pr:>5.1} Mo (totale {t:>5.1})  CPU {}", pourcent(cpu.1, temps_cpu(Some(p))))
        }
        None => "pas de processus".into(),
    };
    println!("{nom:<36} | {principal} | {fenetre}");
}

/// Déroulé de l'essai (environ 50 s). Rien à faire : tout s'ouvre et se ferme seul.
pub fn essai() {
    println!("stat-traque — essai de la fenêtre (données de démonstration)\n");
    println!("{:<36} | {:<46} | processus de la fenêtre", "étape", "processus principal (icône)");
    etape("1. Au repos, rien d'ouvert", 3);
    demarrer(None);
    mode_demo();
    avec_icone();
    affichage::modifier(|e| *e = demo::etat(Phase::Lobby));
    etape("2. Interface prête, rien d'ouvert", 3);
    // Passage en champ select : la fenêtre s'ouvre seule (sauf fenetre_auto=non).
    affichage::modifier(|e| e.phase = Some(Phase::ChampSelect));
    ouvrir();
    etape("3. Fenêtre ouverte (champ select)", 6);
    etape("4. Fenêtre ouverte, rien ne change", 5);
    affichage::modifier(|e| *e = demo::etat(Phase::InProgress));
    etape("5. Fenêtre ouverte (onglet partie)", 4);
    fermer();
    etape("6. Fenêtre fermée", 4);
    basculer_overlay();
    etape("7. Overlay affiché (en partie)", 5);
    etape("8. Overlay, rien ne change", 5);
    fermer();
    etape("9. Tout fermé", 4);
    println!("\nBudgets du processus principal : < 15 Mo au repos, < 40 Mo en partie avec l'overlay ;");
    println!("CPU ~0 % quand rien ne change. Le processus de la fenêtre disparaît à la fermeture.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lit_utime_et_stime() {
        // Extrait réel de /proc/<pid>/stat (nom avec espace et parenthèse).
        let stat = "1234 (stat (tra) que) S 1 1234 1234 0 -1 4194560 500 0 0 0 250 75 3 4 20 0 5 0 100 0 0";
        assert_eq!(temps_depuis_stat(stat), Some(Duration::from_millis((250 + 75) * 10)));
        assert_eq!(temps_depuis_stat("abîmé"), None);
    }
}
