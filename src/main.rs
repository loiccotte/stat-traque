// stat-traque : alternative légère à Blitz pour League of Legends.
//
// En "release" sous Windows : pas de fenêtre console, l'app vit dans la barre
// des tâches (icône près de l'horloge). `stat-traque --console` pour voir les messages.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//
// Organisation (un fichier = un rôle) :
//   main.rs        démarrage : lance les threads
//   evenement.rs   les messages échangés entre threads
//   cerveau.rs     reçoit les événements, décide quoi faire
//   lcu/           client LoL local (lockfile + WebSocket)
//   live.rs        partie en cours (port 2999), seulement pendant la partie
//   gameflow.rs    les phases du client (salon, champ select, partie...)
//   ddragon.rs     données du patch, en cache sur le disque
//   riot_api/      API Riot : limiteur de débit + cache
//   historique.rs  mes parties, stats perso, LP, récap (base locale TSV)
//   stockage.rs    dossier %APPDATA%\stat-traque (clé, réglages, cache)
//   http.rs        clients HTTP partagés
//   tray.rs        icône Windows
//   demarrage.rs   lancement avec Windows (registre)
//   maj.rs         mise à jour automatique (releases GitHub)
//   journal.rs     messages (console + fichier)
//   enregistreur.rs  enregistre les sessions (anonymisées) pour les rejouer
//   rejeu.rs       rejoue un enregistrement sans LoL
//
// Rappel Rust : `Option<T>` = une valeur OU rien ; `Result<T, E>` = réussite OU erreur ;
// `?` = "si ça a raté, on s'arrête et on renvoie l'erreur".

mod affichage;
mod cerveau;
mod champselect;
mod collecte;
mod compo;
mod ddragon;
mod demarrage;
mod enregistreur;
mod evenement;
mod gameflow;
mod historique;
mod http;
mod journal;
mod lcu;
mod live;
mod maj;
mod rangs;
mod rejeu;
mod stats;
mod riot_api;
mod stockage;
mod tray;

use std::sync::mpsc;
use std::time::Instant;

const AIDE: &str = "stat-traque — alternative légère à Blitz

  stat-traque                    lance l'app (icône dans la barre des tâches)
  stat-traque --console          lance l'app dans une console à part (Ctrl+C pour arrêter)
  stat-traque cle RGAPI-xxxx     enregistre ta clé API Riot (stockée en local)
  stat-traque region euw1        change de serveur (euw1, eun1, na1, kr...)
  stat-traque --rejouer f.jsonl  rejoue une session enregistrée (sans LoL)
  stat-traque collecter 30       collecte 30 min de parties Master+ (stats de runes)
  stat-traque historique         mes parties, stats perso et LP (base locale, hors ligne)
  stat-traque demarrage oui      lance stat-traque avec Windows (non = arrêter)
  stat-traque aide               affiche cette aide";

fn main() {
    let debut = Instant::now();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();

    // Commandes ponctuelles : on fait la chose et on s'arrête.
    match args.as_slice() {
        ["cle", cle] => return commande_cle(cle),
        ["region", r] => return commande_region(r),
        ["historique"] => {
            tray::ouvrir_console();
            return historique::commande();
        }
        ["demarrage", choix] => return commande_demarrage(choix),
        ["--rejouer", fichier] => return commande_rejouer(fichier),
        ["collecter", minutes] => {
            tray::ouvrir_console();
            journal::initialiser();
            return collecte::collecter(minutes.parse().unwrap_or(30));
        }
        ["aide" | "--help" | "-h", ..] => {
            tray::ouvrir_console();
            println!("{AIDE}");
            return;
        }
        _ => {}
    }

    // Sous Linux (développement) il n'y a pas d'icône : toujours en mode console.
    let console = args.contains(&"--console") || !cfg!(windows);
    if console {
        tray::nouvelle_console();
    }
    // Deux instances = deux icônes, deux connexions, un journal écrasé : on refuse.
    if !tray::instance_unique() {
        println!("stat-traque est déjà lancé (icône près de l'horloge). Pour l'arrêter : taskkill /IM stat-traque.exe /F");
        if console {
            std::thread::sleep(std::time::Duration::from_secs(5)); // le temps de lire
        }
        return;
    }
    tray::sans_boites_erreur_disque();
    journal::initialiser();
    log!("stat-traque {} — démarrage", env!("CARGO_PKG_VERSION"));

    // Un canal = une file de messages. Les threads envoient (`tx`), le cerveau reçoit (`rx`).
    // Réglages lus UNE fois ici, puis prêtés (`&`) à ceux qui en ont besoin au démarrage.
    let reglages = stockage::Reglages::charger();
    enregistreur::demarrer(&reglages);
    let (tx, rx) = mpsc::channel();
    ddragon::charger_en_fond(tx.clone());
    lcu::evenements::lancer(tx.clone());
    verifier_cle_en_fond(&reglages);
    // Lancement avec Windows : registre remis à jour si l'exe a bougé.
    demarrage::synchroniser(&reglages);

    log!("Prêt en {} ms.", debut.elapsed().as_millis());
    // Mise à jour auto APRÈS « Prêt » : ce lancement a réussi, l'ancien exe peut être rangé.
    maj::lancer_en_fond(&reglages);
    if console {
        cerveau::boucle(rx, tx, cerveau::Mode::Console);
    } else {
        // Windows : la fenêtre de l'icône doit tourner sur le thread principal.
        let tx_cerveau = tx.clone();
        std::thread::Builder::new()
            .name("cerveau".into())
            .spawn(move || cerveau::boucle(rx, tx_cerveau, cerveau::Mode::Tray))
            .expect("impossible de lancer le cerveau");
        tray::lancer(tx); // revient quand on clique sur "Quitter"
    }
}

/// Rejoue un enregistrement dans la console, avec les données du patch en cache.
fn commande_rejouer(fichier: &str) {
    tray::ouvrir_console();
    let (tx, rx) = mpsc::channel();
    if let Some(d) = ddragon::charger() {
        let _ = tx.send(evenement::Evenement::DonneesPretes(std::sync::Arc::new(d)));
    }
    rejeu::lancer(fichier, tx.clone());
    cerveau::boucle(rx, tx, cerveau::Mode::Rejeu);
}

fn commande_cle(cle: &str) {
    tray::ouvrir_console();
    if !cle.starts_with("RGAPI-") {
        println!("Ça ne ressemble pas à une clé Riot (elle commence par RGAPI-).");
        return;
    }
    match stockage::sauver_cle(cle) {
        Ok(()) => println!("Clé enregistrée dans {}", stockage::chemin_cle().display()),
        Err(e) => println!("Impossible d'enregistrer la clé : {e}"),
    }
}

/// `stat-traque demarrage oui|non` : écrit le registre ET le réglage `demarrage`.
fn commande_demarrage(choix: &str) {
    tray::ouvrir_console();
    let oui = match choix.to_lowercase().as_str() {
        "oui" | "on" | "1" => true,
        "non" | "off" | "0" => false,
        _ => {
            println!("Utilisation : stat-traque demarrage oui   (ou non)");
            return;
        }
    };
    if let Err(e) = demarrage::activer(oui) {
        println!("Impossible de modifier le lancement avec Windows : {e}");
        return;
    }
    let mut r = stockage::Reglages::charger();
    r.set("demarrage", if oui { "oui" } else { "non" });
    let _ = r.sauver();
    if oui {
        println!("stat-traque se lancera avec Windows.");
    } else {
        println!("stat-traque ne se lancera plus avec Windows.");
    }
}

fn commande_region(region: &str) {
    tray::ouvrir_console();
    let mut r = stockage::Reglages::charger();
    r.set("region", &region.to_lowercase());
    match r.sauver() {
        Ok(()) => println!("Serveur : {} (routage {})", r.region(), riot_api::region_routage(r.region())),
        Err(e) => println!("Impossible d'enregistrer : {e}"),
    }
}

/// Ménage du cache et vérification de la clé, sans retarder le démarrage.
fn verifier_cle_en_fond(reglages: &stockage::Reglages) {
    // Le thread a besoin de SA copie (il vit plus longtemps que `main` ne prête les réglages).
    let reglages = reglages.clone();
    std::thread::spawn(move || {
        let api = riot_api::RiotApi::depuis_reglages(&reglages);
        let supprimes = api.nettoyer_cache();
        if supprimes > 0 {
            log!("Cache Riot : {supprimes} fichiers périmés supprimés.");
        }
        match api.verifier_cle() {
            Ok(()) => log!("Clé API : valide."),
            Err(e) => log!("Clé API : {e}"),
        }
    });
}
