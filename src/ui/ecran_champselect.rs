//! Écran Champ select : compos, plan de jeu, runes, matchups, objets.
//! (Code déplacé tel quel depuis ecrans.rs.)

use eframe::egui::{self, Color32, RichText, Ui};

use super::composants::{self, TaillePortrait};
use super::format;
use super::textures::{self, PortraitsPartage};
use super::theme;
use crate::affichage::{VueChampSelect, VueObjets, VueRunes};
use crate::compo::AnalyseEquipe;
use crate::ddragon::DonneesJeu;

pub(super) fn dessiner_champ_select(ui: &mut Ui, cs: &VueChampSelect, donnees: Option<&DonneesJeu>) {
    // En-tête : mon champion, mon rôle, verrouillé ?
    let portraits = textures::portraits(ui.ctx());
    ui.horizontal(|ui| {
        if let Some(nom) = &cs.mon_champion {
            portrait(ui, portraits.as_ref(), donnees, nom);
        }
        let champion = cs.mon_champion.as_deref().unwrap_or("pas encore choisi");
        ui.label(RichText::new(champion).color(theme::OR).strong().size(22.0));
        if let Some(r) = &cs.role {
            ui.label(RichText::new(format::libelle_role(r)).size(16.0));
        }
        if cs.verrouille {
            ui.label(RichText::new("verrouillé").color(theme::VERT));
        } else if cs.mon_champion.is_some() {
            ui.label(RichText::new("pas verrouillé").color(theme::TEXTE_DOUX));
        }
    });
    ui.add_space(6.0);

    // Les deux compos côte à côte.
    ui.columns(2, |colonnes| {
        dessiner_equipe(&mut colonnes[0], "Ton équipe", theme::EQUIPE_BLEUE, &cs.allies, cs.analyse_allies.as_ref(), donnees, portraits.as_ref());
        dessiner_equipe(&mut colonnes[1], "Équipe ennemie", theme::EQUIPE_ROUGE, &cs.ennemis, cs.analyse_ennemis.as_ref(), donnees, portraits.as_ref());
    });

    if !cs.plan_de_jeu.is_empty() {
        ui.add_space(8.0);
        composants::panneau(ui, Some("Plan de jeu"), |ui| {
            for conseil in &cs.plan_de_jeu {
                ui.label(format!("• {conseil}"));
            }
        });
    }
    if !cs.picks_conseilles.is_empty() {
        ui.add_space(8.0);
        composants::panneau(ui, Some("Picks qui complètent la compo"), |ui| {
            for (nom, raison) in &cs.picks_conseilles {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(nom).strong());
                    ui.label(RichText::new(raison).color(theme::TEXTE_DOUX));
                });
            }
        });
    }
    if let Some(r) = &cs.runes {
        ui.add_space(8.0);
        dessiner_runes(ui, r);
    }
    if cs.adversaire_lane.is_some() || !cs.meilleurs_matchups.is_empty() || !cs.pires_matchups.is_empty() {
        ui.add_space(8.0);
        dessiner_matchups(ui, cs);
    }
    if let Some(o) = &cs.objets {
        ui.add_space(8.0);
        dessiner_objets(ui, o);
    }
    // Les futures sections (fiche matchup, tier list...) se placent ici, chacune
    // dans un `if let Some(..)` : rien ne s'affiche tant que la donnée n'existe pas.
}

fn dessiner_equipe(
    ui: &mut Ui,
    titre: &str,
    couleur: Color32,
    champions: &[String],
    analyse: Option<&AnalyseEquipe>,
    donnees: Option<&DonneesJeu>,
    portraits: Option<&PortraitsPartage>,
) {
    composants::panneau(ui, None, |ui| {
        ui.label(RichText::new(titre).color(couleur).strong().size(16.0));
        if champions.is_empty() {
            ui.label(RichText::new("pas encore de champions").color(theme::TEXTE_DOUX));
        } else {
            // Un portrait par champion déjà choisi (noms conservés en infobulle).
            ui.horizontal_wrapped(|ui| {
                for nom in champions {
                    portrait(ui, portraits, donnees, nom).on_hover_text(nom);
                }
            });
        }
        let Some(a) = analyse.filter(|a| a.nombre > 0) else { return };
        ui.separator();
        // Style principal + secondaires.
        if let Some((style, _)) = a.styles.first() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Style :").color(theme::TEXTE_DOUX));
                ui.label(RichText::new(style.nom()).color(theme::OR).strong());
                let autres: Vec<&str> = a.styles.iter().skip(1).take(2).map(|s| s.0.nom()).collect();
                if !autres.is_empty() {
                    ui.label(RichText::new(format!("(aussi : {})", autres.join(", "))).color(theme::TEXTE_DOUX));
                }
            });
        }
        barre_ap_ad(ui, a.ap_pct);
        jauge(ui, "Frontline", a.frontline as f32, 15.0);
        jauge(ui, "Contrôle", a.cc as f32, 15.0);
        // Courbe de puissance : 3 petites barres verticales.
        ui.horizontal(|ui| {
            ui.label(RichText::new("Puissance").color(theme::TEXTE_DOUX));
            courbe(ui, a.courbe);
            ui.label(RichText::new(format::moment_fort(a.courbe)).color(theme::OR));
        });
        for alerte in &a.alertes {
            ui.label(RichText::new(format!("! {alerte}")).color(theme::ALERTE));
        }
        if !a.roles.is_empty() {
            ui.add_space(2.0);
            egui::Grid::new(("roles", titre)).num_columns(2).spacing([12.0, 2.0]).show(ui, |ui| {
                for (id, role) in &a.roles {
                    ui.label(format::nom_depuis_id_dd(donnees, id));
                    ui.label(RichText::new(*role).color(theme::TEXTE_DOUX));
                    ui.end_row();
                }
            });
        }
    });
}

/// Portrait 38 px d'un champion affiché par son nom. Le verrou des textures n'est
/// pris que le temps du dessin (jamais gardé d'une trame à l'autre).
fn portrait(ui: &mut Ui, partage: Option<&PortraitsPartage>, donnees: Option<&DonneesJeu>, nom: &str) -> egui::Response {
    let id = format::id_dd_depuis_nom(donnees, nom);
    match partage.and_then(|p| p.lock().ok()) {
        Some(mut p) => composants::portrait(ui, &mut p, id.as_deref(), None, TaillePortrait::Ligne),
        None => ui.allocate_response(egui::vec2(38.0, 38.0), egui::Sense::hover()),
    }
}

/// Barre horizontale AP (bleu) / AD (orange).
fn barre_ap_ad(ui: &mut Ui, ap_pct: u8) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Dégâts").color(theme::TEXTE_DOUX));
        let largeur = (ui.available_width() - 120.0).max(60.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(largeur, 12.0), egui::Sense::hover());
        let separation = rect.left() + rect.width() * format::fraction(ap_pct as f32, 100.0);
        let p = ui.painter();
        p.rect_filled(egui::Rect::from_min_max(rect.min, egui::pos2(separation, rect.bottom())), 2.0, theme::AP);
        p.rect_filled(egui::Rect::from_min_max(egui::pos2(separation, rect.top()), rect.max), 2.0, theme::AD);
        ui.label(format!("{} % AP / {} % AD", ap_pct, 100 - ap_pct.min(100)));
    });
}

/// Jauge « nom  [#####-----]  7/15 ».
fn jauge(ui: &mut Ui, nom: &str, valeur: f32, max: f32) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(nom).color(theme::TEXTE_DOUX));
        let largeur = (ui.available_width() - 60.0).max(60.0);
        ui.add(
            egui::ProgressBar::new(format::fraction(valeur, max))
                .desired_width(largeur)
                .desired_height(10.0)
                .fill(theme::OR_SOMBRE),
        );
        ui.label(format!("{valeur:.0}/{max:.0}"));
    });
}

/// Trois barres verticales early / mid / late (0 à 3).
fn courbe(ui: &mut Ui, valeurs: [f32; 3]) {
    for (nom, v) in ["E", "M", "L"].iter().zip(valeurs) {
        let (rect, reponse) = ui.allocate_exact_size(egui::vec2(10.0, 22.0), egui::Sense::hover());
        let haut = rect.bottom() - rect.height() * format::fraction(v, 3.0);
        ui.painter().rect_filled(rect, 1.0, theme::CADRE.gamma_multiply(2.0));
        ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(rect.left(), haut), rect.max), 1.0, theme::OR);
        reponse.on_hover_text(format!("{nom} : {}/3", format::decimal(v, 1)));
    }
}

fn dessiner_runes(ui: &mut Ui, r: &VueRunes) {
    composants::panneau(ui, Some("Runes conseillées"), |ui| {
        ui.label(RichText::new(&r.page).strong());
        ui.horizontal_wrapped(|ui| {
            stat(ui, "victoires", &format::pourcent(r.winrate));
            stat(ui, "parties", &format::milliers(r.parties));
            stat(ui, "choisie dans", &format::pourcent(r.pick_rate));
            stat(ui, "parties analysées", &format::milliers(r.parties_champion));
            let portee = r.role.as_deref().map_or("tous rôles".to_string(), |x| format::libelle_role(&x.to_lowercase()).to_string());
            stat(ui, "rôle", &portee);
        });
        if let Some(s) = &r.sorts {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Sorts :").color(theme::TEXTE_DOUX));
                ui.label(s);
            });
        }
    });
}

/// Adversaire de lane, contre-picks, meilleurs / pires duels.
fn dessiner_matchups(ui: &mut Ui, cs: &VueChampSelect) {
    composants::panneau(ui, Some("Matchups"), |ui| {
        if let Some(adv) = &cs.adversaire_lane {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Adversaire probable :").color(theme::TEXTE_DOUX));
                ui.label(RichText::new(adv).strong());
                match &cs.matchup {
                    Some(m) => {
                        ui.label(RichText::new(format::pourcent(m.winrate)).color(couleur_winrate(m.winrate)).strong());
                        ui.label(RichText::new(format!("pour toi sur {} parties", format::milliers(m.parties))).color(theme::TEXTE_DOUX));
                    }
                    None => {
                        ui.label(RichText::new("(pas assez de parties sur ce duel)").color(theme::TEXTE_DOUX));
                    }
                }
            });
        }
        let listes = [
            ("Contre-picks", &cs.contre_picks),
            ("Tes meilleurs duels", &cs.meilleurs_matchups),
            ("Tes pires duels", &cs.pires_matchups),
        ];
        for (titre, liste) in listes.into_iter().filter(|(_, l)| !l.is_empty()) {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("{titre} :")).color(theme::TEXTE_DOUX));
                for m in liste.iter() {
                    ui.label(RichText::new(&m.champion).strong());
                    ui.label(RichText::new(format::pourcent(m.winrate)).color(couleur_winrate(m.winrate)))
                        .on_hover_text(format!("{} parties", format::milliers(m.parties)));
                    ui.add_space(6.0);
                }
            });
        }
    });
}

/// Objets conseillés : core, bottes, situationnels.
fn dessiner_objets(ui: &mut Ui, o: &VueObjets) {
    composants::panneau(ui, None, |ui| {
        ui.horizontal(|ui| {
            theme::titre(ui, "Objets conseillés");
            ui.label(RichText::new(format!("{} parties analysées", format::milliers(o.parties))).color(theme::TEXTE_DOUX).small());
            if o.item_set_applique {
                ui.label(RichText::new("item set ajouté à la boutique").color(theme::VERT).small());
            }
        });
        egui::Grid::new("objets").num_columns(4).spacing([14.0, 3.0]).show(ui, |ui| {
            for (groupe, objets) in [("Core", &o.core), ("Bottes", &o.bottes), ("Situationnels", &o.situationnels)] {
                for (i, obj) in objets.iter().enumerate() {
                    ui.label(RichText::new(if i == 0 { groupe } else { "" }).color(theme::TEXTE_DOUX));
                    ui.label(RichText::new(&obj.nom).strong());
                    ui.label(format!("dans {} des parties", format::pourcent_entier(obj.frequence)));
                    ui.label(RichText::new(format!("{} de victoires", format::pourcent(obj.winrate))).color(couleur_winrate(obj.winrate)));
                    ui.end_row();
                }
            }
        });
    });
}

/// Vert au-dessus de 51 %, rouge sous 49 %, neutre entre les deux.
pub fn couleur_winrate(winrate: f32) -> Color32 {
    if winrate >= 51.0 {
        theme::VERT
    } else if winrate <= 49.0 {
        theme::EQUIPE_ROUGE
    } else {
        theme::TEXTE
    }
}

/// « valeur libellé » : la valeur en évidence, le libellé en discret.
fn stat(ui: &mut Ui, libelle: &str, valeur: &str) {
    ui.label(RichText::new(valeur).color(theme::OR).strong());
    ui.label(RichText::new(libelle).color(theme::TEXTE_DOUX));
    ui.add_space(8.0);
}
