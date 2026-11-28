//! Dessin de la fenêtre principale : barre d'état, onglets, contenu.
//!
//! egui est une interface « immédiate » : à chaque image on redécrit TOUTE la
//! fenêtre à partir de l'état (pas d'objets « bouton » à garder en mémoire).
//! Ici on ne redessine qu'à chaque changement de l'état ou action de la souris,
//! jamais en boucle (voir fenetre.rs).
//!
//! Chaque onglet = une fonction `dessiner_xxx`. Pour en ajouter un : voir onglets.rs.

//! Le contenu de chaque onglet vit dans son propre fichier :
//! ecran_accueil.rs, ecran_champselect.rs, ecran_partie.rs, ecran_divers.rs (journal, réglages).

use eframe::egui::{self, RichText, Ui};

use super::ecran_accueil::dessiner_accueil;
use super::ecran_champselect::dessiner_champ_select;
use super::ecran_divers::{dessiner_journal, dessiner_reglages};
use super::ecran_partie::dessiner_partie;
use super::composants;
use super::format;
use super::onglets::{onglet_conseille, Onglet};
use super::reglages::FormReglages;
use super::theme;
use crate::affichage::EtatAffichage;
use crate::gameflow::Phase;
use crate::stockage::Reglages;

// Ré-export : fiche.rs et historique.rs importent `ecrans::couleur_winrate`.
pub use super::ecran_champselect::couleur_winrate;

/// Ce que la fenêtre retient entre deux images (en plus de l'état du cerveau).
/// Les champs sont `pub(super)` : les fichiers ecran_*.rs voisins en ont besoin.
pub struct Ecran {
    pub onglet: Onglet,
    /// Dernière phase vue : on change d'onglet seulement quand elle change.
    pub(super) phase_vue: Option<Phase>,
    /// Formulaire des réglages (rechargé à chaque ouverture de la fenêtre).
    pub(super) form: FormReglages,
    /// Résultat du dernier « Enregistrer ».
    pub(super) retour_reglages: Option<(bool, String)>,
    /// Rôle choisi dans la tier list (+ lignes déjà filtrées).
    pub(super) filtre_tier: super::historique::FiltreTier,
    /// Onglet Fiche : joueur choisi dans la liste.
    pub(super) fiche: super::fiche::EtatOnglet,
    /// « Copié » affiché à côté du bouton utilisé (fiche ou récap).
    pub(super) copie: super::fiche::RetourCopie,
}

impl Ecran {
    pub fn nouveau(etat: &EtatAffichage) -> Self {
        Self {
            onglet: onglet_conseille(etat.phase).unwrap_or(Onglet::Accueil),
            phase_vue: etat.phase,
            form: FormReglages::depuis(&Reglages::charger()),
            retour_reglages: None,
            filtre_tier: super::historique::FiltreTier::default(),
            fiche: Default::default(),
            copie: Default::default(),
        }
    }

    /// Appelée quand l'état du cerveau a changé.
    pub fn etat_change(&mut self, etat: &EtatAffichage) {
        if etat.phase != self.phase_vue {
            self.phase_vue = etat.phase;
            if let Some(o) = onglet_conseille(etat.phase) {
                self.onglet = o;
            }
        }
    }
}

/// Point d'entrée : toute la fenêtre.
pub fn dessiner(ui: &mut Ui, ecran: &mut Ecran, etat: &EtatAffichage) {
    egui::Panel::top("statut")
        .frame(egui::Frame::NONE.fill(theme::CADRE).inner_margin(egui::Margin::symmetric(12, 8)))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                titre_application(ui);
                ui.add_space(12.0);
                let couleur = if etat.client_connecte { theme::VERT } else { theme::TEXTE_DOUX };
                // Pastille de couleur (dessinée : la police par défaut n'a pas le caractère ●).
                let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 4.0, couleur);
                ui.label(RichText::new(format::statut(etat.client_connecte, etat.phase, etat.cle_api)).color(theme::TEXTE_DOUX));
            });
            ui.add_space(4.0);
            // Barre d'onglets du design : texte or + soulignement or sur l'actif.
            let visibles: Vec<Onglet> = Onglet::TOUS.into_iter().filter(|o| o.visible(etat)).collect();
            composants::onglets(ui, &mut ecran.onglet, &visibles);
        });
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(theme::FOND).inner_margin(egui::Margin::same(12)))
        .show(ui, |ui| {
            // Tier list et journal ont leur propre défilement « virtualisé » : seules
            // les lignes visibles sont dessinées (pas des centaines à chaque image).
            if matches!(ecran.onglet, Onglet::TierList | Onglet::Journal) {
                dessiner_onglet(ui, ecran, etat);
            } else {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| dessiner_onglet(ui, ecran, etat));
            }
        });
}

fn dessiner_onglet(ui: &mut Ui, ecran: &mut Ecran, etat: &EtatAffichage) {
    let donnees = etat.donnees.as_deref();
    match ecran.onglet {
        Onglet::Accueil => dessiner_accueil(ui, etat),
        Onglet::ChampSelect => match &etat.champ_select {
            Some(cs) => dessiner_champ_select(ui, cs, donnees),
            None => vide(ui, "Pas de champ select en cours. Lance une partie : l'analyse s'affichera ici."),
        },
        Onglet::Partie => {
            // Clic sur « Fiche » à côté d'un joueur : on montre l'onglet Fiche
            // (avec « Chargement... » le temps que la fiche arrive).
            if dessiner_partie(ui, etat, donnees) {
                ecran.onglet = Onglet::Fiche;
            }
        }
        Onglet::Fiche => super::fiche::dessiner(ui, etat, &mut ecran.fiche, &mut ecran.copie, donnees),
        Onglet::Historique => super::historique::dessiner(ui, etat, donnees, &mut ecran.copie),
        Onglet::TierList => match &etat.tier_list {
            Some(t) => super::historique::dessiner_tier_list_avec(ui, t, &mut ecran.filtre_tier, donnees),
            None => vide(ui, "Pas encore de stats pour ce patch (stat-traque collecter 30)."),
        },
        Onglet::Journal => dessiner_journal(ui, &etat.messages),
        Onglet::Reglages => dessiner_reglages(ui, ecran),
    }
}

/// Titre « STAT-TRAQUE » : Cinzel or, capitales espacées, précédé d'un petit losange or.
fn titre_application(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    let pts = composants::sommets_losange(rect.center(), 5.0, 5.0).to_vec();
    ui.painter().add(egui::Shape::convex_polygon(pts, theme::OR, egui::Stroke::NONE));
    ui.label(
        RichText::new("STAT-TRAQUE")
            .font(theme::police_titre(18.0))
            .extra_letter_spacing(2.0)
            .color(theme::OR),
    );
}

/// Message centré quand un onglet n'a rien à montrer.
pub(super) fn vide(ui: &mut Ui, texte: &str) {
    ui.add_space(40.0);
    ui.vertical_centered(|ui| ui.label(RichText::new(texte).color(theme::TEXTE_DOUX).size(15.0)));
}

