//! Overlay en jeu : petit cadre semi-transparent avec les rangs des 10 joueurs.
//!
//! C'est une fenêtre Windows NORMALE posée au-dessus du jeu (toujours au-dessus,
//! sans bordure, transparente, qui laisse passer les clics) : AUCUNE injection
//! dans le processus du jeu. Conformité Riot : uniquement les rangs officiels
//! (déjà visibles sur les sites et le client), aucun timer, aucune automatisation.
//! Limite : invisible en « Plein écran » exclusif -> mode « Sans bordure » requis.

use eframe::egui::{self, Color32, RichText, Ui};

use super::format;
use super::theme;
use crate::affichage::EtatAffichage;
use crate::rangs::InfoJoueur;

/// Taille de la fenêtre de l'overlay (points logiques).
pub const TAILLE: [f32; 2] = [520.0, 150.0];

/// Fond semi-transparent (le reste de la fenêtre est totalement transparent).
const FOND: Color32 = Color32::from_rgba_premultiplied(1, 8, 16, 205);

pub fn dessiner(ui: &mut Ui, etat: &EtatAffichage) {
    let cadre = egui::Frame::NONE
        .fill(FOND)
        .stroke(egui::Stroke::new(1.0, theme::OR_SOMBRE))
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(8, 5));
    // Fond central totalement transparent : seul le cadre est visible.
    egui::CentralPanel::no_frame().show(ui, |ui| {
        cadre.show(ui, |ui| {
            if etat.rangs.is_empty() {
                let texte = if etat.cle_api { "stat-traque : rangs en cours de chargement…" } else { "stat-traque : pas de clé API, rangs indisponibles" };
                ui.label(RichText::new(texte).color(theme::TEXTE_DOUX).size(13.0));
                return;
            }
            let donnees = etat.donnees.as_deref();
            let (bleus, rouges) = format::par_equipe(&etat.rangs);
            ui.horizontal_top(|ui| {
                equipe(ui, "bleus", theme::EQUIPE_BLEUE, &bleus, donnees);
                ui.add_space(10.0);
                equipe(ui, "rouges", theme::EQUIPE_ROUGE, &rouges, donnees);
            });
        });
    });
}

fn equipe(ui: &mut Ui, id: &str, couleur: Color32, joueurs: &[&InfoJoueur], donnees: Option<&crate::ddragon::DonneesJeu>) {
    egui::Grid::new(("overlay", id)).num_columns(3).spacing([8.0, 1.0]).show(ui, |ui| {
        for j in joueurs {
            let nom = RichText::new(format::nom_champion(donnees, j.champion)).size(13.0).strong();
            ui.label(if j.moi { nom.color(theme::OR) } else { nom.color(couleur) });
            let rang = RichText::new(format::rang_court(j.solo.as_ref())).size(13.0);
            ui.label(match &j.solo {
                Some(r) => rang.color(theme::couleur_palier(&r.palier)),
                None => rang.color(theme::TEXTE_DOUX),
            });
            let bilan = j.solo.as_ref().map_or(String::new(), |r| {
                format!("{} ({})", format::pourcent_entier(r.winrate()), r.victoires as u32 + r.defaites as u32)
            });
            ui.label(RichText::new(bilan).size(12.0).color(theme::TEXTE_DOUX));
            ui.end_row();
        }
    });
}
