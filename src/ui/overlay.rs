//! Overlay en jeu : petit cadre semi-transparent avec les rangs des 10 joueurs.
//!
//! C'est une fenêtre Windows NORMALE posée au-dessus du jeu (toujours au-dessus,
//! sans bordure, transparente, qui laisse passer les clics) : AUCUNE injection
//! dans le processus du jeu. Conformité Riot : uniquement les rangs officiels
//! (déjà visibles sur les sites et le client), aucun timer, aucune automatisation.
//! Limite : invisible en « Plein écran » exclusif -> mode « Sans bordure » requis.

use eframe::egui::{self, Color32, RichText, Ui};

use super::format;
use super::palette::{self, Rgb};
use crate::affichage::EtatAffichage;
use crate::rangs::InfoJoueur;

/// Taille de la fenêtre de l'overlay (points logiques).
pub const TAILLE: [f32; 2] = [520.0, 150.0];

/// Couleur de la palette (rouge, vert, bleu) -> couleur egui.
fn c(rgb: Rgb) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// Fond semi-transparent (le reste de la fenêtre est totalement transparent).
/// `from_rgba_unmultiplied` : on donne la couleur de la palette et l'opacité (205/255).
fn fond() -> Color32 {
    let [r, g, b] = palette::FOND;
    Color32::from_rgba_unmultiplied(r, g, b, 205)
}

pub fn dessiner(ui: &mut Ui, etat: &EtatAffichage) {
    let cadre = egui::Frame::NONE
        .fill(fond())
        // Angles droits et filet or de 1 px, comme l'overlay natif.
        .stroke(egui::Stroke::new(1.0, c(palette::OR)))
        .corner_radius(egui::CornerRadius::ZERO)
        .inner_margin(egui::Margin::symmetric(8, 5));
    // Fond central totalement transparent : seul le cadre est visible.
    egui::CentralPanel::no_frame().show(ui, |ui| {
        cadre.show(ui, |ui| {
            if etat.rangs.is_empty() {
                let texte = if etat.cle_api { "stat-traque : rangs en cours de chargement…" } else { "stat-traque : pas de clé API, rangs indisponibles" };
                ui.label(RichText::new(texte).color(c(palette::TEXTE_SECONDAIRE)).size(13.0));
                return;
            }
            let donnees = etat.donnees.as_deref();
            ui.horizontal_top(|ui| {
                if etat.sans_equipes {
                    // Arena : deux colonnes dans l'ordre reçu, sans couleur d'équipe.
                    let (a, b) = format::colonnes_arena(&etat.rangs);
                    equipe(ui, "joueurs-a", c(palette::TEXTE), &a, donnees);
                    ui.add_space(10.0);
                    equipe(ui, "joueurs-b", c(palette::TEXTE), &b, donnees);
                } else {
                    let (bleus, rouges): (Vec<&InfoJoueur>, Vec<&InfoJoueur>) = etat.rangs.iter().partition(|j| j.bleu);
                    equipe(ui, "bleus", c(palette::EQUIPE_BLEUE), &bleus, donnees);
                    ui.add_space(10.0);
                    equipe(ui, "rouges", c(palette::EQUIPE_ROUGE), &rouges, donnees);
                }
            });
        });
    });
}

fn equipe(ui: &mut Ui, id: &str, couleur: Color32, joueurs: &[&InfoJoueur], donnees: Option<&crate::ddragon::DonneesJeu>) {
    egui::Grid::new(("overlay", id)).num_columns(3).spacing([8.0, 1.0]).show(ui, |ui| {
        for j in joueurs {
            let nom = RichText::new(format::nom_champion(donnees, j.champion)).size(13.0).strong();
            ui.label(if j.moi { nom.color(c(palette::OR)) } else { nom.color(couleur) });
            let rang = RichText::new(format::rang_court(j.solo.as_ref())).size(13.0);
            ui.label(match &j.solo {
                Some(r) => rang.color(super::theme::couleur_palier(&r.palier)),
                None => rang.color(c(palette::TEXTE_SECONDAIRE)),
            });
            let bilan = j.solo.as_ref().map_or(String::new(), |r| {
                format!("{} ({})", format::pourcent_entier(r.winrate()), r.victoires as u32 + r.defaites as u32)
            });
            ui.label(RichText::new(bilan).size(12.0).color(c(palette::TEXTE_SECONDAIRE)));
            ui.end_row();
        }
    });
}
