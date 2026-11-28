//! Écrans Journal et Réglages.
//! (Code déplacé tel quel depuis ecrans.rs.)

use eframe::egui::{self, RichText, Ui};

use super::ecrans::{vide, Ecran};
use super::reglages::{libelle_region, FormReglages, REGIONS};
use super::theme;
use crate::stockage::Reglages;

pub(super) fn dessiner_journal(ui: &mut Ui, messages: &[String]) {
    if messages.is_empty() {
        return vide(ui, "Aucun message pour l'instant.");
    }
    let hauteur = ui.text_style_height(&egui::TextStyle::Monospace) + ui.spacing().item_spacing.y;
    egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).show_rows(
        ui,
        hauteur,
        messages.len(),
        |ui, lignes| {
            for m in &messages[lignes] {
                ui.monospace(m);
            }
        },
    );
}

// ---------------------------------------------------------------- Réglages

pub(super) fn dessiner_reglages(ui: &mut Ui, ecran: &mut Ecran) {
    let f = &mut ecran.form;
    theme::carte().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        theme::titre(ui, "Champ select");
        ui.horizontal(|ui| {
            ui.label("Flash sur la touche");
            ui.radio_value(&mut f.flash_sur_f, false, "D");
            ui.radio_value(&mut f.flash_sur_f, true, "F");
        });
        ui.checkbox(&mut f.runes_auto, "Appliquer les runes conseillées au verrouillage");
        ui.checkbox(&mut f.sorts_auto, "Appliquer les sorts conseillés au verrouillage");
        ui.checkbox(&mut f.objets_auto, "Ajouter l'item set conseillé dans la boutique");
        ui.checkbox(&mut f.fenetre_auto, "Ouvrir cette fenêtre au début du champ select");
        ui.label(
            RichText::new("Runes, sorts, item set : pris en compte au prochain verrouillage.")
                .color(theme::TEXTE_DOUX)
                .small(),
        );
    });
    ui.add_space(8.0);
    theme::carte().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        theme::titre(ui, "Compte et données");
        ui.horizontal(|ui| {
            ui.label("Serveur");
            egui::ComboBox::from_id_salt("region").selected_text(libelle_region(&f.region).to_string()).show_ui(ui, |ui| {
                for (code, libelle) in REGIONS {
                    ui.selectable_value(&mut f.region, code.to_string(), libelle);
                }
            });
        });
        ui.checkbox(&mut f.enregistrer, "Enregistrer mes sessions (anonymisées) pour aider à corriger les bugs");
        ui.label(RichText::new("Serveur et enregistrement : pris en compte au prochain démarrage.").color(theme::TEXTE_DOUX).small());
    });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui.button(RichText::new("Enregistrer").strong()).clicked() {
            // On relit le fichier juste avant : on ne perd pas les autres réglages.
            let mut r = Reglages::charger();
            ecran.form.appliquer(&mut r);
            ecran.retour_reglages = Some(match r.sauver() {
                Ok(()) => (true, "Réglages enregistrés.".into()),
                Err(e) => (false, format!("Impossible d'enregistrer : {e}")),
            });
        }
        if ui.button("Annuler les changements").clicked() {
            ecran.form = FormReglages::depuis(&Reglages::charger());
            ecran.retour_reglages = None;
        }
        if let Some((ok, texte)) = &ecran.retour_reglages {
            ui.colored_label(if *ok { theme::VERT } else { theme::ALERTE }, texte);
        }
    });
    ui.add_space(12.0);
    ui.label(
        RichText::new(
            "Overlay en jeu : Ctrl+Maj+Tab pendant la partie (jeu en mode « Sans bordure »), \
             ou menu de l'icône. Position : ligne overlay_position=x,y dans reglages.txt.",
        )
        .color(theme::TEXTE_DOUX)
        .small(),
    );
}
