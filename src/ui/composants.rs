//! Composants graphiques du thème (panneau, étoiles, portrait, onglets...).
//!
//! Stub : signatures figées (contrat entre les tâches), corps provisoires.
//! Rempli par T-01 (panneau, titre, onglets, barre, pastille), T-03 (portrait),
//! T-04 (étoiles, emblème de rang).

// Rempli et branché par T-01 à T-04 : en attendant, rien n'appelle ces fonctions.
#![allow(dead_code)]

use eframe::egui::{Color32, Response, Sense, Ui, Vec2};

use super::onglets::Onglet;
use super::textures::Portraits;
use crate::rangs::Rang;

/// Taille d'un portrait de champion : 38 px (ligne) ou 66 px (fiche).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaillePortrait {
    Ligne,
    Fiche,
}

/// Panneau à angles droits avec titre optionnel.
pub fn panneau<R>(ui: &mut Ui, _titre: Option<&str>, contenu: impl FnOnce(&mut Ui) -> R) -> R {
    contenu(ui)
}

/// Titre de section (or, capitales espacées).
pub fn titre_section(ui: &mut Ui, texte: &str) {
    ui.label(texte);
}

/// Cinq étoiles + nombre à droite (`None` : pas de note).
pub fn etoiles(_ui: &mut Ui, _note: Option<u8>) {}

/// Or clair à partir de `note::SEUIL_S`, or normal sinon.
pub fn couleur_etoiles(_note: u8) -> Color32 {
    Color32::PLACEHOLDER
}

/// Portrait de champion avec bordure or et niveau.
pub fn portrait(
    ui: &mut Ui,
    _p: &mut Portraits,
    _id_dd: Option<&str>,
    _niveau: Option<u8>,
    _t: TaillePortrait,
) -> Response {
    ui.allocate_response(Vec2::ZERO, Sense::hover())
}

/// Emblème de rang (losange à double contour or).
pub fn embleme_rang(_ui: &mut Ui, _rang: Option<&Rang>, _cote: f32) {}

/// Barre d'onglets ; renvoie true si l'onglet actif a changé.
pub fn onglets(_ui: &mut Ui, _actif: &mut Onglet, _visibles: &[Onglet]) -> bool {
    false
}

/// Barre de progression (dégradé or, piste sombre), `fraction` de 0 à 1.
pub fn barre(_ui: &mut Ui, _fraction: f32, _largeur: f32) {}

/// Pastille V / D d'une partie.
pub fn pastille_victoire(_ui: &mut Ui, _victoire: bool) {}
