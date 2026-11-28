//! Interface graphique : fenêtre principale (onglets) et overlay en jeu.
//!
//! L'interface ne parle JAMAIS au client LoL ni à l'API Riot : elle LIT l'état
//! préparé par le cerveau (`affichage::lire()`), et se redessine seulement quand
//! il change (`affichage::quand_ca_change`).
//!
//! DEUX PROCESSUS (pour la RAM, voir parent.rs) :
//!   - le processus principal (icône) n'a AUCUNE bibliothèque graphique chargée ;
//!   - la fenêtre egui vit dans un processus enfant qui meurt à la fermeture ;
//!   - l'overlay en jeu est une petite fenêtre Windows native (GDI), sans OpenGL.
//!
//!   parent.rs        côté principal : lance / nourrit / ferme l'enfant, phases, overlay
//!   protocole.rs     messages JSON principal -> enfant, et demandes enfant -> principal (pur, testé)
//!   enfant.rs        côté enfant : la fenêtre egui (`stat-traque --fenetre`)
//!   overlay_natif.rs overlay Windows natif (GDI), dans le thread de l'icône
//!   ecrans.rs        cadre commun de la fenêtre (état, titre, onglets, statut)
//!   ecran_*.rs       contenu des onglets : accueil, champselect, partie, divers (journal, réglages)
//!   composants.rs    composants du thème (panneau, étoiles, portrait...) ; textures.rs : portraits
//!   palette.rs       jetons de couleur du thème (pur, testé)
//!   historique.rs    onglets Historique (parties, stats, LP, récap) et Tier list
//!   fiche.rs         onglet Fiche (radar, MMR estimé...) et boutons « Copier pour Discord »
//!   radar.rs         géométrie du radar de la fiche (pur, testé)
//!   overlay.rs       overlay egui (Linux / développement seulement)
//!   theme.rs         couleurs (bleu nuit / or)
//!   onglets.rs       liste des onglets (pur, testé)
//!   format.rs        mise en forme des textes (pur, testé)
//!   reglages.rs      formulaire des réglages (pur, testé)
//!   demo.rs          fausses données pour essayer sans LoL
//!   mesure.rs        commande `essai-fenetre` : RAM / CPU des deux processus
//!
//! Windows : la vraie interface. Linux : la même (X11), pour le développement.

// Certaines fonctions ne servent qu'à une partie des plateformes / commandes.
#![allow(dead_code)]

pub mod demo;
pub mod format;
pub mod onglets;
pub mod palette;
pub mod protocole;
pub mod radar;
pub mod reglages;

#[cfg(any(windows, target_os = "linux"))]
mod composants;
#[cfg(any(windows, target_os = "linux"))]
mod ecran_accueil;
#[cfg(any(windows, target_os = "linux"))]
mod ecran_champselect;
#[cfg(any(windows, target_os = "linux"))]
mod ecran_divers;
#[cfg(any(windows, target_os = "linux"))]
mod ecran_partie;
#[cfg(any(windows, target_os = "linux"))]
mod ecrans;
#[cfg(any(windows, target_os = "linux"))]
mod enfant;
#[cfg(any(windows, target_os = "linux"))]
mod fiche;
#[cfg(any(windows, target_os = "linux"))]
mod historique;
#[cfg(any(windows, target_os = "linux"))]
mod mesure;
#[cfg(any(windows, target_os = "linux"))]
mod overlay;
#[cfg(windows)]
pub mod overlay_natif;
#[cfg(any(windows, target_os = "linux"))]
mod parent;
#[cfg(any(windows, target_os = "linux"))]
mod textures;
#[cfg(any(windows, target_os = "linux"))]
mod theme;

#[cfg(any(windows, target_os = "linux"))]
pub use enfant::lancer as processus_fenetre;
#[cfg(any(windows, target_os = "linux"))]
pub use mesure::essai;
#[cfg(any(windows, target_os = "linux"))]
pub use parent::{
    attendre_fermeture, basculer_overlay, demarrer, fenetre_ouverte, fermer, fermer_et_attendre, mode_demo, ouvrir,
    pid_enfant,
};

/// Ouvre la fenêtre (ou l'overlay) avec les données de démonstration et attend
/// qu'on la ferme (commandes `stat-traque essai-fenetre voir` / `overlay`).
/// L'overlay n'a pas de croix : on l'arrête avec Ctrl+C dans la console.
#[cfg(any(windows, target_os = "linux"))]
pub fn apercu(overlay: bool) {
    demarrer(None);
    mode_demo();
    crate::affichage::modifier(|e| *e = demo::etat(crate::gameflow::Phase::InProgress));
    if overlay {
        mesure::avec_icone(); // Windows : l'overlay natif vit dans le thread de l'icône
        basculer_overlay();
        if cfg!(windows) {
            // Ctrl+C arrête le processus (l'overlay natif n'est pas un enfant à attendre).
            loop {
                std::thread::park();
            }
        }
    } else {
        ouvrir();
    }
    attendre_fermeture();
}

// Autres systèmes : pas d'interface, fonctions vides (comme tray.rs).
#[cfg(not(any(windows, target_os = "linux")))]
mod sans_interface {
    pub fn demarrer(_cerveau: Option<std::sync::mpsc::Sender<crate::evenement::Evenement>>) {}
    pub fn ouvrir() {}
    pub fn fermer() {}
    pub fn fermer_et_attendre(_delai: std::time::Duration) {}
    pub fn basculer_overlay() {}
    pub fn attendre_fermeture() {}
    pub fn fenetre_ouverte() -> bool {
        false
    }
    pub fn apercu(_overlay: bool) {}
    pub fn essai() {}
    pub fn processus_fenetre(_overlay: bool) {}
}
#[cfg(not(any(windows, target_os = "linux")))]
pub use sans_interface::*;
