//! Côté processus PRINCIPAL (celui de l'icône) : ouvre / ferme la fenêtre et
//! l'overlay, sans jamais charger de bibliothèque graphique lui-même.
//!
//! ARCHITECTURE (à lire avant de modifier) :
//!  - La fenêtre principale vit dans un PROCESSUS ENFANT (`stat-traque --fenetre`).
//!    Le pilote OpenGL reste chargé jusqu'à la fin du processus qui l'utilise :
//!    en le confinant dans l'enfant, qui meurt à la fermeture, toute la RAM revient.
//!  - Un thread « fenetre » (créé une fois) dort tant que rien n'est demandé
//!    (Condvar : 0 % CPU). Quand la fenêtre est demandée, il lance l'enfant et lui
//!    écrit une ligne JSON à chaque changement de l'état d'affichage (voir
//!    protocole.rs). Fermer l'entrée de l'enfant = lui demander de se fermer.
//!  - Si l'enfant plante ou est fermé par la croix, le thread le voit (un petit
//!    thread attend sa fin) et se rendort. Le processus principal continue.
//!  - L'overlay en jeu, lui, est une fenêtre Windows native (overlay_natif.rs, dans le
//!    thread de l'icône) : texte GDI, pas d'OpenGL, < 1 Mo. Sous Linux (développement)
//!    il passe par l'enfant en mode overlay (egui).

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Condvar, Mutex, Once, OnceLock};
use std::time::{Duration, Instant};

use super::protocole::{ligne, Expediteur, Message};
use crate::affichage;
use crate::evenement::Evenement;
use crate::gameflow::Phase;

/// Ce que l'enfant affiche.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Principale,
    /// Linux seulement (sous Windows l'overlay est natif).
    Overlay,
}

/// Ce qui est DEMANDÉ (icône, raccourci, cerveau) et ce qui tourne vraiment.
struct Demandes {
    fenetre: bool,
    overlay: bool,
    premier_plan: bool,
    /// Dernière phase vue (pour réagir aux CHANGEMENTS de phase seulement).
    phase: Option<Phase>,
    /// Numéro du processus enfant vivant (mesures), None = pas d'enfant.
    enfant: Option<u32>,
    /// L'enfant vient de se terminer (posé par le thread qui attend sa fin).
    enfant_fini: bool,
}

static DEMANDES: Mutex<Demandes> = Mutex::new(Demandes {
    fenetre: false,
    overlay: false,
    premier_plan: false,
    phase: None,
    enfant: None,
    enfant_fini: false,
});
/// Réveille le thread « fenetre » (et ceux qui attendent la fermeture).
static SIGNAL: Condvar = Condvar::new();
static DEMARRAGE: Once = Once::new();
/// Pour prévenir le cerveau quand la fenêtre s'ouvre / se ferme (suivi de partie).
static CERVEAU: OnceLock<Mutex<Sender<Evenement>>> = OnceLock::new();
/// Mode démonstration (`essai-fenetre`) : l'enfant prend les noms de la démo.
static DEMO: AtomicBool = AtomicBool::new(false);

impl Demandes {
    fn mode_voulu(&self) -> Option<Mode> {
        if self.fenetre {
            Some(Mode::Principale)
        } else if self.overlay && cfg!(not(windows)) {
            Some(Mode::Overlay)
        } else {
            None
        }
    }
}

/// Lance le thread « fenetre » (une seule fois). Aucune fenêtre n'est créée ici.
/// `cerveau` : pour qu'il sache quand la fenêtre est ouverte (None : commandes d'essai).
pub fn demarrer(cerveau: Option<Sender<Evenement>>) {
    DEMARRAGE.call_once(|| {
        if let Some(tx) = cerveau {
            let _ = CERVEAU.set(Mutex::new(tx));
        }
        if let Err(e) = std::thread::Builder::new().name("fenetre".into()).spawn(boucle) {
            crate::log!("Interface indisponible : {e}");
            return;
        }
        // Appelée par le cerveau après CHAQUE changement d'état.
        affichage::quand_ca_change(reveil);
    });
}

/// Ouvre la fenêtre principale (ou la ramène au premier plan).
pub fn ouvrir() {
    modifier(|d| {
        d.fenetre = true;
        d.premier_plan = true;
        if cfg!(not(windows)) {
            d.overlay = false; // Linux : un seul enfant à la fois
        }
    });
}

/// Ferme la fenêtre et l'overlay.
pub fn fermer() {
    modifier(|d| {
        d.fenetre = false;
        d.overlay = false;
    });
    overlay_natif(false);
}

/// Affiche / masque l'overlay.
pub fn basculer_overlay() {
    let mut affiche = false;
    modifier(|d| {
        d.overlay = !d.overlay;
        affiche = d.overlay;
        if affiche && cfg!(not(windows)) {
            d.fenetre = false;
        }
    });
    overlay_natif(affiche);
}

/// La fenêtre principale est-elle ouverte (enfant vivant en mode fenêtre) ?
pub fn fenetre_ouverte() -> bool {
    let d = DEMANDES.lock().unwrap();
    d.enfant.is_some() && d.fenetre
}

/// Numéro du processus de la fenêtre (pour mesurer sa RAM).
pub fn pid_enfant() -> Option<u32> {
    DEMANDES.lock().unwrap().enfant
}

/// Mode démonstration : l'enfant utilise les noms de champions de la démo.
pub fn mode_demo() {
    DEMO.store(true, Ordering::Relaxed);
}

/// Bloque jusqu'à ce qu'aucune fenêtre ne soit ni demandée ni ouverte.
pub fn attendre_fermeture() {
    let mut d = DEMANDES.lock().unwrap();
    while d.mode_voulu().is_some() || d.enfant.is_some() {
        d = SIGNAL.wait(d).unwrap();
    }
}

/// Ferme tout et attend la fin de l'enfant, au plus `delai` (menu « Quitter »).
pub fn fermer_et_attendre(delai: Duration) {
    fermer();
    let limite = Instant::now() + delai;
    let mut d = DEMANDES.lock().unwrap();
    while d.enfant.is_some() {
        let reste = limite.saturating_duration_since(Instant::now());
        if reste.is_zero() {
            break;
        }
        d = SIGNAL.wait_timeout(d, reste).unwrap().0;
    }
}

/// Modifie les demandes, puis réveille le thread « fenetre ».
fn modifier(f: impl FnOnce(&mut Demandes)) {
    f(&mut DEMANDES.lock().unwrap());
    SIGNAL.notify_all();
}

/// Overlay natif (Windows, thread de l'icône). Ne fait rien ailleurs.
fn overlay_natif(affiche: bool) {
    #[cfg(windows)]
    crate::tray::afficher_overlay(affiche);
    #[cfg(not(windows))]
    let _ = affiche;
}

fn prevenir_cerveau() {
    if let Some(tx) = CERVEAU.get() {
        let _ = tx.lock().unwrap().send(Evenement::Fenetre);
    }
}

/// Le cerveau a changé l'état : on regarde seulement la PHASE (pas de copie de
/// tout l'état), on réveille le thread (envoi à l'enfant), on redessine l'overlay.
/// ATTENTION : appelée PENDANT `affichage::modifier` -> ne jamais écrire dans le
/// journal (`log!`) d'ici, ce serait un interblocage.
fn reveil() {
    let phase = affichage::phase();
    // Comparer et noter la nouvelle phase d'un seul coup (sous le verrou) : si deux
    // threads appellent `reveil` en même temps, un seul traite le changement.
    let (ancienne, overlay) = {
        let mut d = DEMANDES.lock().unwrap();
        (std::mem::replace(&mut d.phase, phase), d.overlay)
    };
    if phase != ancienne {
        changement_de_phase(ancienne, phase);
    }
    SIGNAL.notify_all();
    #[cfg(windows)]
    if overlay {
        crate::tray::redessiner_overlay();
    }
    #[cfg(not(windows))]
    let _ = overlay;
}

fn changement_de_phase(ancienne: Option<Phase>, phase: Option<Phase>) {
    let en_partie = |p: Option<Phase>| p.is_some_and(Phase::en_partie);
    // Lecture du fichier de réglages AVANT de prendre le verrou (accès disque).
    let ouvrir_auto =
        phase == Some(Phase::ChampSelect) && crate::stockage::Reglages::charger().get("fenetre_auto") != Some("non");
    // Le raccourci n'est actif QUE pendant la partie : en dehors, Ctrl+Maj+Tab
    // reste aux autres applications (navigateur...).
    if en_partie(phase) != en_partie(ancienne) {
        crate::tray::raccourci_overlay(en_partie(phase));
    }
    let fin_de_partie = en_partie(ancienne) && !en_partie(phase);
    modifier(|d| {
        if ouvrir_auto {
            d.fenetre = true;
        }
        if fin_de_partie {
            d.overlay = false; // l'overlay disparaît tout seul après la partie
        }
    });
    if fin_de_partie {
        overlay_natif(false);
    }
}

/// Le thread « fenetre » : dort, lance l'enfant demandé, le nourrit, se rendort.
fn boucle() {
    loop {
        let mode = {
            let mut d = DEMANDES.lock().unwrap();
            loop {
                if let Some(m) = d.mode_voulu() {
                    break m;
                }
                d = SIGNAL.wait(d).unwrap(); // 0 % CPU ici
            }
        };
        match lancer_enfant(mode) {
            Ok(()) => {}
            Err(e) => {
                crate::log!("Fenêtre impossible à ouvrir : {e}");
                // Pas de nouvel essai en boucle : on oublie la demande.
                modifier(|d| match mode {
                    Mode::Principale => d.fenetre = false,
                    Mode::Overlay => d.overlay = false,
                });
            }
        }
    }
}

/// Lance l'enfant, lui envoie l'état à chaque changement, revient à sa fin.
fn lancer_enfant(mode: Mode) -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    let mut commande = Command::new(exe);
    commande.arg("--fenetre");
    if mode == Mode::Overlay {
        commande.arg("overlay");
    }
    let mut enfant = commande.stdin(Stdio::piped()).stdout(Stdio::null()).spawn()?;
    let Some(entree) = enfant.stdin.take() else { return Ok(()) };
    let pid = enfant.id();
    modifier(|d| {
        d.enfant = Some(pid);
        d.enfant_fini = false;
    });
    prevenir_cerveau();
    // Un petit thread attend la fin de l'enfant (croix, plantage ou fin normale).
    std::thread::spawn(move || {
        let _ = enfant.wait();
        modifier(|d| d.enfant_fini = true);
    });

    let mut expediteur = Expediteur::default();
    let mut version_envoyee = None;
    // L'entrée de l'enfant ; `None` = fermée (on lui a demandé de se fermer).
    let mut entree = Some(entree);
    if DEMO.load(Ordering::Relaxed) {
        envoyer(&mut entree, &[ligne(&Message::Demo)]);
    }
    loop {
        // Attendre : un changement d'état, une demande, ou la fin de l'enfant.
        let premier_plan = {
            let mut d = DEMANDES.lock().unwrap();
            loop {
                if d.enfant_fini {
                    break;
                }
                let garder = d.mode_voulu() == Some(mode);
                if entree.is_some() && (!garder || d.premier_plan || version_envoyee != Some(affichage::version())) {
                    break;
                }
                d = SIGNAL.wait(d).unwrap(); // 0 % CPU ici
            }
            if d.enfant_fini {
                break;
            }
            if d.mode_voulu() != Some(mode) {
                // Fermer l'entrée = « ferme-toi ». On attend ensuite sa fin (boucle).
                entree = None;
                continue;
            }
            std::mem::take(&mut d.premier_plan)
        };
        let mut lignes = Vec::new();
        let version = affichage::version();
        if version_envoyee != Some(version) {
            // On lit la version AVANT l'état : si un changement arrive entre les deux,
            // on renverra au prochain tour (jamais d'état en retard). Les changements
            // arrivés pendant un envoi sont regroupés : seul le dernier état part.
            lignes = expediteur.lignes(affichage::lire());
            version_envoyee = Some(version);
        }
        if premier_plan {
            lignes.push(ligne(&Message::PremierPlan));
        }
        envoyer(&mut entree, &lignes);
    }
    drop(entree);
    // L'enfant est fini. S'il s'est fermé TOUT SEUL (croix, plantage), on oublie
    // la demande, sinon le thread le relancerait aussitôt.
    modifier(|d| {
        d.enfant = None;
        d.enfant_fini = false;
        if d.mode_voulu() == Some(mode) {
            match mode {
                Mode::Principale => d.fenetre = false,
                Mode::Overlay => d.overlay = false,
            }
        }
    });
    prevenir_cerveau();
    Ok(())
}


/// Écrit les lignes à l'enfant. S'il ne lit plus (fermé, planté), on ferme notre
/// côté : le thread d'attente signalera sa fin.
fn envoyer(entree: &mut Option<std::process::ChildStdin>, lignes: &[String]) {
    let Some(e) = entree.as_mut() else { return };
    let mut ok = true;
    for l in lignes {
        ok &= writeln!(e, "{l}").is_ok();
    }
    if !ok || e.flush().is_err() {
        *entree = None;
    }
}
