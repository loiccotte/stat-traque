//! Onglets « Historique » (mes parties, stats perso, LP, récap) et « Tier list ».
//! Données préparées par historique.rs / stats.rs ; ici on ne fait que dessiner.

use std::sync::Arc;

use eframe::egui::{self, RichText, Stroke, Ui};

use super::ecrans::couleur_winrate;
use super::{format, theme};
use crate::affichage::{EtatAffichage, VueTierList};
use crate::ddragon::DonneesJeu;
use crate::historique::{lp_absolus, nom_file, FileClassee, ProgressionLp, Recap, StatsGroupe, StatsPerso};

pub fn dessiner(ui: &mut Ui, etat: &EtatAffichage, donnees: Option<&DonneesJeu>) {
    let rien = etat.historique.is_empty() && etat.progression_lp.is_none() && etat.recap.is_none();
    if rien {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new(
                    "Pas encore d'historique. Il se remplit tout seul après tes parties \
                     (clé API nécessaire), et reste consultable hors ligne.",
                )
                .color(theme::TEXTE_DOUX)
                .size(15.0),
            )
        });
        return;
    }
    if let Some(r) = &etat.recap {
        dessiner_recap(ui, r, donnees);
        ui.add_space(8.0);
    }
    if let Some(p) = &etat.progression_lp {
        dessiner_lp(ui, p);
        ui.add_space(8.0);
    }
    if let Some(s) = &etat.stats_perso {
        dessiner_stats(ui, s, donnees);
        ui.add_space(8.0);
    }
    if !etat.historique.is_empty() {
        dessiner_parties(ui, etat, donnees);
    }
}

fn dessiner_recap(ui: &mut Ui, r: &Recap, donnees: Option<&DonneesJeu>) {
    let p = &r.partie;
    theme::carte().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            theme::titre(ui, "Récap de ta dernière partie");
            let (texte, couleur) = if p.remake {
                ("Remake", theme::TEXTE_DOUX)
            } else if p.victoire {
                ("Victoire", theme::VERT)
            } else {
                ("Défaite", theme::EQUIPE_ROUGE)
            };
            ui.label(RichText::new(texte).color(couleur).strong());
            ui.label(RichText::new(format::nom_champion(donnees, p.champion)).strong());
            ui.label(RichText::new(nom_file(p.file)).color(theme::TEXTE_DOUX));
            ui.label(format!("{} en {} min", format::kda(p.kills, p.morts, p.assists), p.duree / 60));
            if let Some(lp) = p.lp {
                ui.label(RichText::new(format::variation_lp(lp)).color(if lp >= 0 { theme::VERT } else { theme::EQUIPE_ROUGE }));
            }
        });
        egui::Grid::new("recap").num_columns(4).spacing([16.0, 3.0]).show(ui, |ui| {
            for entete in ["", "Cette partie", "Ta moyenne", "Écart"] {
                ui.label(RichText::new(entete).color(theme::TEXTE_DOUX).small());
            }
            ui.end_row();
            for l in &r.lignes {
                let d = l.decimales as usize;
                ui.label(RichText::new(l.libelle).color(theme::TEXTE_DOUX));
                ui.label(RichText::new(format::nombre(l.valeur, d)).strong());
                ui.label(l.moyenne.map_or("—".into(), |m| format::nombre(m, d)));
                let couleur = match l.ecart_pct {
                    Some(e) if e >= 5.0 => theme::VERT,
                    Some(e) if e <= -5.0 => theme::EQUIPE_ROUGE,
                    _ => theme::TEXTE,
                };
                ui.label(RichText::new(format::ecart(l.ecart_pct)).color(couleur));
                ui.end_row();
            }
        });
        if r.lignes.iter().all(|l| l.moyenne.is_none()) {
            ui.label(
                RichText::new(format!(
                    "Pas encore de moyenne sur ce champion ({} autre(s) partie(s) connue(s)).",
                    r.parties_champion
                ))
                .color(theme::TEXTE_DOUX)
                .small(),
            );
        }
    });
}

fn dessiner_lp(ui: &mut Ui, p: &ProgressionLp) {
    theme::carte().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        theme::titre(ui, "Progression");
        egui::Grid::new("lp").num_columns(4).spacing([16.0, 3.0]).show(ui, |ui| {
            for (nom, actuel, meilleur, semaine) in [
                ("Solo/duo", &p.solo, &p.meilleur_solo, p.solo_7_jours),
                ("Flex", &p.flex, &p.meilleur_flex, p.flex_7_jours),
            ] {
                if actuel.is_none() && meilleur.is_none() {
                    continue;
                }
                ui.label(RichText::new(nom).color(theme::TEXTE_DOUX));
                match actuel {
                    Some(r) => ui.label(RichText::new(r.to_string()).color(theme::couleur_palier(&r.palier)).strong()),
                    None => ui.label("Non classé"),
                };
                ui.label(meilleur.as_ref().map_or(String::new(), |m| format!("meilleur : {m}")));
                match semaine {
                    Some(lp) => ui.label(
                        RichText::new(format!("{} sur 7 jours", format::variation_lp(lp)))
                            .color(if lp >= 0 { theme::VERT } else { theme::EQUIPE_ROUGE }),
                    ),
                    None => ui.label(""),
                };
                ui.end_row();
            }
        });
        // Petit graphique des LP en solo/duo (points dans l'ordre chronologique).
        let valeurs: Vec<i32> = p.points.iter().filter(|x| x.file == FileClassee::Solo).map(|x| lp_absolus(&x.rang)).collect();
        if valeurs.len() >= 2 {
            graphique(ui, &valeurs);
        }
    });
}

/// Courbe simple : un point par relevé de LP, mis à l'échelle entre le min et le max.
fn graphique(ui: &mut Ui, valeurs: &[i32]) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 70.0), egui::Sense::hover());
    let min = *valeurs.iter().min().unwrap_or(&0) as f32;
    let max = (*valeurs.iter().max().unwrap_or(&0) as f32).max(min + 1.0);
    let pas = rect.width() / (valeurs.len() - 1) as f32;
    let points: Vec<egui::Pos2> = valeurs
        .iter()
        .enumerate()
        .map(|(i, &v)| egui::pos2(rect.left() + i as f32 * pas, rect.bottom() - (v as f32 - min) / (max - min) * rect.height()))
        .collect();
    let p = ui.painter();
    p.rect_stroke(rect, 2.0, Stroke::new(1.0, theme::OR_SOMBRE), egui::StrokeKind::Inside);
    p.add(egui::Shape::line(points, Stroke::new(2.0, theme::TURQUOISE)));
}

fn ligne_stats(ui: &mut Ui, nom: &str, s: &StatsGroupe) {
    ui.label(RichText::new(nom).strong());
    ui.label(s.parties.to_string());
    ui.label(RichText::new(format::pourcent_entier(s.winrate())).color(couleur_winrate(s.winrate())));
    ui.label(format::decimal(s.kda(), 1));
    ui.label(format::decimal(s.cs_par_min(), 1));
    ui.end_row();
}

fn entetes(ui: &mut Ui, premiere: &str) {
    for e in [premiere, "Parties", "Victoires", "KDA", "CS/min"] {
        ui.label(RichText::new(e).color(theme::TEXTE_DOUX).small());
    }
    ui.end_row();
}

fn dessiner_stats(ui: &mut Ui, s: &StatsPerso, donnees: Option<&DonneesJeu>) {
    theme::carte().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        theme::titre(ui, "Mes stats (Faille de l'invocateur)");
        ui.columns(2, |c| {
            egui::Grid::new("par_champion").num_columns(5).striped(true).spacing([12.0, 3.0]).show(&mut c[0], |ui| {
                entetes(ui, "Champion");
                ligne_stats(ui, "Total", &s.total);
                for (id, g) in s.par_champion.iter().take(8) {
                    ligne_stats(ui, &format::nom_champion(donnees, *id), g);
                }
            });
            egui::Grid::new("par_role").num_columns(5).striped(true).spacing([12.0, 3.0]).show(&mut c[1], |ui| {
                entetes(ui, "Rôle");
                for (role, g) in &s.par_role {
                    ligne_stats(ui, format::libelle_role(role), g);
                }
                for (famille, g) in &s.par_famille {
                    ligne_stats(ui, famille.libelle(), g);
                }
            });
        });
    });
}

fn dessiner_parties(ui: &mut Ui, etat: &EtatAffichage, donnees: Option<&DonneesJeu>) {
    let maintenant = format::maintenant();
    theme::carte().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        theme::titre(ui, "Dernières parties");
        egui::Grid::new("parties").num_columns(8).striped(true).spacing([12.0, 3.0]).show(ui, |ui| {
            for e in ["Quand", "File", "Champion", "Rôle", "Résultat", "KDA", "CS/min", "LP"] {
                ui.label(RichText::new(e).color(theme::TEXTE_DOUX).small());
            }
            ui.end_row();
            for p in etat.historique.iter() {
                ui.label(RichText::new(format::il_y_a(maintenant, p.date)).color(theme::TEXTE_DOUX));
                ui.label(nom_file(p.file));
                ui.label(RichText::new(format::nom_champion(donnees, p.champion)).strong());
                ui.label(p.role.map_or("—", format::libelle_role));
                let (texte, couleur) = if p.remake {
                    ("Remake", theme::TEXTE_DOUX)
                } else if p.victoire {
                    ("Victoire", theme::VERT)
                } else {
                    ("Défaite", theme::EQUIPE_ROUGE)
                };
                ui.label(RichText::new(texte).color(couleur));
                ui.label(format::kda(p.kills, p.morts, p.assists));
                ui.label(format::decimal(p.cs_par_min(), 1));
                ui.label(p.lp.map_or(String::new(), format::variation_lp));
                ui.end_row();
            }
        });
    });
}

// ---------------------------------------------------------------- Tier list

/// Rôle affiché dans la tier list, et les lignes de ce rôle (indices dans
/// `VueTierList::lignes`), recalculées SEULEMENT quand le rôle ou la liste change.
pub struct FiltreTier {
    role: &'static str,
    /// (liste pour laquelle le filtre a été fait, rôle, indices des lignes)
    cache: Option<(Arc<VueTierList>, &'static str, Vec<usize>)>,
}

impl Default for FiltreTier {
    fn default() -> Self {
        Self { role: "middle", cache: None }
    }
}

pub fn dessiner_tier_list(ui: &mut Ui, t: &Arc<VueTierList>, filtre: &mut FiltreTier) {
    ui.horizontal(|ui| {
        for role in crate::stats::ROLES {
            let texte = RichText::new(format::libelle_role(role));
            if ui.selectable_label(filtre.role == role, texte).clicked() {
                filtre.role = role;
            }
        }
        ui.add_space(12.0);
        ui.label(
            RichText::new(format!("Patch {} · {} parties analysées", t.patch, format::milliers(t.parties)))
                .color(theme::TEXTE_DOUX),
        );
    });
    let a_jour = filtre.cache.as_ref().is_some_and(|(liste, role, _)| Arc::ptr_eq(liste, t) && *role == filtre.role);
    if !a_jour {
        let indices = t.lignes.iter().enumerate().filter(|(_, l)| l.role == filtre.role).map(|(i, _)| i).collect();
        filtre.cache = Some((t.clone(), filtre.role, indices));
    }
    let Some((_, _, indices)) = &filtre.cache else { return };
    ui.add_space(6.0);
    // Largeurs fixes : les colonnes restent alignées quelle que soit la partie visible.
    const LARGEURS: [f32; 6] = [36.0, 150.0, 90.0, 80.0, 80.0, 80.0];
    let ligne = |ui: &mut Ui, cases: [RichText; 6]| {
        ui.horizontal(|ui| {
            for (case, largeur) in cases.into_iter().zip(LARGEURS) {
                ui.add_sized([largeur, 18.0], egui::Label::new(case));
            }
        });
    };
    let doux = |t: &str| RichText::new(t).color(theme::TEXTE_DOUX).small();
    ligne(ui, [doux("#"), doux("Champion"), doux("Victoires"), doux("Choisi"), doux("Banni"), doux("Parties")]);
    let hauteur = 18.0 + ui.spacing().item_spacing.y;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, hauteur, indices.len(), |ui, visibles| {
        for rang in visibles {
            let l = &t.lignes[indices[rang]];
            ligne(
                ui,
                [
                    RichText::new((rang + 1).to_string()).color(theme::TEXTE_DOUX),
                    RichText::new(&l.champion).strong(),
                    RichText::new(format::pourcent(l.winrate)).color(couleur_winrate(l.winrate)),
                    RichText::new(format::pourcent(l.pick_rate)),
                    RichText::new(format::pourcent(l.ban_rate)),
                    RichText::new(format::milliers(l.parties)),
                ],
            );
        }
    });
}
