//! Interface graphique : fenêtre principale (onglets) et overlay en jeu.
//!
//! L'interface ne parle JAMAIS au client LoL ni à l'API Riot : elle LIT l'état
//! préparé par le cerveau (`affichage::lire()`), et se redessine seulement quand
//! il change (`affichage::quand_ca_change`).
//!
//!   fenetre.rs   thread de la fenêtre, ouverture / fermeture, overlay (egui)
//!   ecrans.rs    dessin des onglets de la fenêtre principale
//!   overlay.rs   dessin de l'overlay en jeu
//!   theme.rs     couleurs (bleu nuit / or)
//!   onglets.rs   liste des onglets (pur, testé)
//!   format.rs    mise en forme des textes (pur, testé)
//!   reglages.rs  formulaire des réglages (pur, testé)
//!   demo.rs      fausses données pour essayer sans LoL
//!   mesure.rs    commande `essai-fenetre` : RAM / CPU fenêtre ouverte, fermée, overlay
//!
//! Windows : la vraie interface. Linux : la même (X11), pour le développement.

// Certaines fonctions ne servent qu'à une partie des plateformes / commandes.
#![allow(dead_code)]

pub mod demo;
pub mod format;
pub mod onglets;
pub mod reglages;

#[cfg(any(windows, target_os = "linux"))]
mod ecrans;
#[cfg(any(windows, target_os = "linux"))]
mod fenetre;
#[cfg(any(windows, target_os = "linux"))]
mod mesure;
#[cfg(any(windows, target_os = "linux"))]
mod overlay;
#[cfg(any(windows, target_os = "linux"))]
mod theme;

#[cfg(any(windows, target_os = "linux"))]
pub use fenetre::{attendre_fermeture, basculer_overlay, demarrer, fermer, ouvrir};
#[cfg(any(windows, target_os = "linux"))]
pub use mesure::essai;

/// Ouvre la fenêtre avec les données de démonstration et attend qu'on la ferme
/// (commande `stat-traque essai-fenetre voir`).
#[cfg(any(windows, target_os = "linux"))]
pub fn apercu() {
    demarrer();
    crate::affichage::modifier(|e| *e = demo::etat(crate::gameflow::Phase::InProgress));
    ouvrir();
    attendre_fermeture();
}

// Autres systèmes : pas d'interface, fonctions vides (comme tray.rs).
#[cfg(not(any(windows, target_os = "linux")))]
mod sans_interface {
    pub fn demarrer() {}
    pub fn ouvrir() {}
    pub fn fermer() {}
    pub fn basculer_overlay() {}
    pub fn attendre_fermeture() {}
    pub fn apercu() {}
    pub fn essai() {}
}
#[cfg(not(any(windows, target_os = "linux")))]
pub use sans_interface::*;
