//! Onglets « Historique » (mes parties, stats perso, LP, récap) et « Tier list ».
//! Données préparées par historique.rs / stats.rs ; ici on ne fait que dessiner.
//!
//! Règles : aucun redessin demandé (coût nul au repos) ; les portraits ne sont demandés
//! qu'aux lignes visibles (`composants::portrait` saute les lignes hors écran) ; le verrou
//! des textures n'est tenu que le temps d'un portrait, jamais d'une trame à l'autre.

use std::sync::Arc;

use eframe::egui::{self, vec2, Align, Layout, RichText, Stroke, Ui};

use super::composants::{self, TaillePortrait};
use super::ecrans::couleur_winrate;
use super::fiche::RetourCopie;
use super::{format, textures, theme};
use crate::affichage::{EtatAffichage, VueTierList};
use crate::ddragon::DonneesJeu;
use crate::historique::{lp_absolus, nom_file, FileClassee, ProgressionLp, Recap, StatsGroupe, StatsPerso};

// ---------------------------------------------------------------- Fonctions pures (testées)

/// Résultat d'une partie : (texte, victoire ?). `None` pour un remake (ni V ni D).
fn resultat(remake: bool, victoire: bool) -> Option<(&'static str, bool)> {
    if remake {
        None
    } else if victoire {
        Some(("Victoire", true))
    } else {
        Some(("Défaite", false))
    }
}

/// Couleur d'une variation de LP : vert si >= 0, rouge sinon.
fn couleur_lp(lp: i32) -> egui::Color32 {
    if lp >= 0 {
        theme::VICTOIRE
    } else {
        theme::DEFAITE
    }
}

/// CS total et par minute : `"187 · 6,4/min"`.
fn texte_cs(cs: u16, par_min: f32) -> String {
    format!("{cs} · {}/min", format::decimal(par_min, 1))
}

/// Phrase quand le récap n'a pas de moyenne : accords corrects (« 1 autre partie connue »).
fn texte_sans_moyenne(autres: u32) -> String {
    if autres == 0 {
        return "Pas encore de moyenne sur ce champion.".to_string();
    }
    let (adjectif, nom, participe) = if autres >= 2 { ("autres", "parties", "connues") } else { ("autre", "partie", "connue") };
    format!("Pas encore de moyenne sur ce champion ({autres}\u{a0}{adjectif} {nom} {participe}).")
}

/// Identifiant Data Dragon d'un champion à partir de son id numérique (pour l'icône).
fn id_dd(donnees: Option<&DonneesJeu>, id: u16) -> Option<&str> {
    donnees?.champions.get(&id).map(|c| &*c.id)
}

/// Devine l'id Data Dragon depuis le nom affiché quand on n'a pas les données de jeu :
/// on retire espaces, points et apostrophes, la lettre après une apostrophe passe en minuscule
/// (« Kai'Sa » -> « Kaisa », « Vel'Koz » -> « Velkoz »), plus quelques exceptions connues.
fn id_dd_approx(nom: &str) -> Option<String> {
    match nom {
        "Wukong" => return Some("MonkeyKing".into()),
        "Nunu & Willump" => return Some("Nunu".into()),
        "Renata Glasc" => return Some("Renata".into()),
        _ => {}
    }
    let mut sortie = String::new();
    let mut apres_apostrophe = false;
    for c in nom.chars() {
        if c == '\'' || c == '\u{2019}' {
            apres_apostrophe = true;
        } else if c.is_alphanumeric() {
            if apres_apostrophe {
                sortie.extend(c.to_lowercase());
            } else {
                sortie.push(c);
            }
            apres_apostrophe = false;
        }
    }
    (!sortie.is_empty()).then_some(sortie)
}

// ---------------------------------------------------------------- Petits outils de dessin

/// Portrait 38 px d'un champion. Le verrou n'est tenu que pendant cet appel.
fn portrait(ui: &mut Ui, id_dd: Option<&str>) {
    let cote = TaillePortrait::Ligne.cote();
    if let Some(partage) = textures::portraits(ui.ctx()) {
        if let Ok(mut p) = partage.lock() {
            composants::portrait(ui, &mut p, id_dd, None, TaillePortrait::Ligne);
            return;
        }
    }
    // Pas de cache de textures (tests, démarrage) : on réserve la place pour ne rien décaler.
    ui.allocate_space(vec2(cote, cote));
}

/// Case de largeur fixe (les colonnes restent alignées d'une ligne à l'autre).
/// `droite` : nombres alignés à droite.
fn case(ui: &mut Ui, largeur: f32, contenu: RichText, droite: bool) {
    let sens = if droite { Layout::right_to_left(Align::Center) } else { Layout::left_to_right(Align::Center) };
    ui.allocate_ui_with_layout(vec2(largeur, 20.0), sens, |ui| {
        ui.add(egui::Label::new(contenu).truncate());
    });
}

fn doux(t: &str) -> RichText {
    RichText::new(t).color(theme::TEXTE_DOUX).small()
}

/// Texte lisible (on évite `.strong()`, invisible sur ce fond).
fn texte(t: impl Into<String>) -> RichText {
    RichText::new(t.into()).color(theme::TEXTE)
}

/// Texte de victoire / défaite / remake, coloré.
fn texte_resultat(remake: bool, victoire: bool) -> RichText {
    match resultat(remake, victoire) {
        None => RichText::new("Remake").color(theme::TEXTE_DOUX),
        Some((t, v)) => RichText::new(t).color(if v { theme::VICTOIRE } else { theme::DEFAITE }),
    }
}

// ---------------------------------------------------------------- Historique

pub fn dessiner(ui: &mut Ui, etat: &EtatAffichage, donnees: Option<&DonneesJeu>, copie: &mut RetourCopie) {
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
        dessiner_recap(ui, r, etat.recap_partage.as_deref(), donnees, copie);
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

/// `partage` : le même récap en texte Discord (bouton « Copier »), s'il existe.
fn dessiner_recap(ui: &mut Ui, r: &Recap, partage: Option<&str>, donnees: Option<&DonneesJeu>, copie: &mut RetourCopie) {
    let p = &r.partie;
    composants::panneau(ui, Some("Récap de ta dernière partie"), |ui| {
        ui.horizontal(|ui| {
            portrait(ui, id_dd(donnees, p.champion));
            ui.vertical(|ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(texte_resultat(p.remake, p.victoire));
                    ui.label(texte(format::nom_champion(donnees, p.champion)));
                    ui.label(RichText::new(nom_file(p.file)).color(theme::TEXTE_DOUX));
                    if let Some(lp) = p.lp {
                        ui.label(RichText::new(format::variation_lp(lp)).color(couleur_lp(lp)));
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label(texte(format!("{} en {}", format::kda(p.kills, p.morts, p.assists), format::duree(p.duree))));
                    if let Some(t) = partage {
                        ui.add_space(12.0);
                        copie.bouton(ui, "recap", t);
                    }
                });
            });
        });
        ui.add_space(4.0);
        egui::Grid::new("recap").num_columns(4).spacing([16.0, 3.0]).show(ui, |ui| {
            for entete in ["", "Cette partie", "Ta moyenne", "Écart"] {
                ui.label(doux(entete));
            }
            ui.end_row();
            for l in &r.lignes {
                let d = l.decimales as usize;
                ui.label(RichText::new(l.libelle).color(theme::TEXTE_DOUX));
                ui.label(texte(format::nombre(l.valeur, d)));
                ui.label(texte(l.moyenne.map_or("—".into(), |m| format::nombre(m, d))));
                let couleur = match l.ecart_pct {
                    Some(e) if e >= 5.0 => theme::VICTOIRE,
                    Some(e) if e <= -5.0 => theme::DEFAITE,
                    _ => theme::TEXTE,
                };
                ui.label(RichText::new(format::ecart(l.ecart_pct)).color(couleur));
                ui.end_row();
            }
        });
        if r.lignes.iter().all(|l| l.moyenne.is_none()) {
            ui.label(RichText::new(texte_sans_moyenne(r.parties_champion)).color(theme::TEXTE_DOUX).small());
        }
    });
}

fn dessiner_lp(ui: &mut Ui, p: &ProgressionLp) {
    composants::panneau(ui, Some("Progression"), |ui| {
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
                    Some(r) => ui.label(RichText::new(r.to_string()).color(theme::couleur_palier(&r.palier))),
                    None => ui.label(RichText::new("Non classé").color(theme::TEXTE)),
                };
                ui.label(RichText::new(meilleur.as_ref().map_or(String::new(), |m| format!("meilleur : {m}"))).color(theme::TEXTE));
                match semaine {
                    Some(lp) => ui.label(RichText::new(format!("{} sur 7 jours", format::variation_lp(lp))).color(couleur_lp(lp))),
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
    if !ui.is_rect_visible(rect) {
        return;
    }
    let min = *valeurs.iter().min().unwrap_or(&0) as f32;
    let max = (*valeurs.iter().max().unwrap_or(&0) as f32).max(min + 1.0);
    let pas = rect.width() / (valeurs.len() - 1) as f32;
    let points: Vec<egui::Pos2> = valeurs
        .iter()
        .enumerate()
        .map(|(i, &v)| egui::pos2(rect.left() + i as f32 * pas, rect.bottom() - (v as f32 - min) / (max - min) * rect.height()))
        .collect();
    let p = ui.painter();
    p.rect_stroke(rect, 0.0, Stroke::new(1.0, theme::LIGNE), egui::StrokeKind::Inside);
    p.add(egui::Shape::line(points, Stroke::new(2.0, theme::OR)));
}

/// Ligne de stats : `portrait` = id Data Dragon à afficher (None : pas de portrait, case vide).
fn ligne_stats(ui: &mut Ui, nom: &str, portrait_dd: Option<Option<&str>>, s: &StatsGroupe) {
    ui.horizontal(|ui| {
        match portrait_dd {
            Some(id) => portrait(ui, id),
            None => {
                ui.allocate_space(vec2(TaillePortrait::Ligne.cote(), 20.0));
            }
        }
        ui.label(texte(nom));
    });
    ui.label(texte(s.parties.to_string()));
    ui.label(RichText::new(format::pourcent_entier(s.winrate())).color(couleur_winrate(s.winrate())));
    ui.label(texte(format::decimal(s.kda(), 1)));
    ui.label(texte(format::decimal(s.cs_par_min(), 1)));
    ui.end_row();
}

fn entetes(ui: &mut Ui, premiere: &str) {
    for e in [premiere, "Parties", "Victoires", "KDA", "CS/min"] {
        ui.label(doux(e));
    }
    ui.end_row();
}

fn dessiner_stats(ui: &mut Ui, s: &StatsPerso, donnees: Option<&DonneesJeu>) {
    composants::panneau(ui, Some("Mes stats (Faille de l'invocateur)"), |ui| {
        ui.columns(2, |c| {
            egui::Grid::new("par_champion").num_columns(5).spacing([12.0, 3.0]).show(&mut c[0], |ui| {
                entetes(ui, "Champion");
                ligne_stats(ui, "Total", None, &s.total);
                for (id, g) in s.par_champion.iter().take(8) {
                    ligne_stats(ui, &format::nom_champion(donnees, *id), Some(id_dd(donnees, *id)), g);
                }
            });
            egui::Grid::new("par_role").num_columns(5).spacing([12.0, 3.0]).show(&mut c[1], |ui| {
                entetes(ui, "Rôle");
                for (role, g) in &s.par_role {
                    ligne_stats(ui, format::libelle_role(role), None, g);
                }
                for (famille, g) in &s.par_famille {
                    ligne_stats(ui, famille.libelle(), None, g);
                }
            });
        });
    });
}

/// Largeurs des colonnes de « Dernières parties » : quand, champion (portrait + nom), file,
/// rôle, V/D, KDA, CS, durée, LP.
const L_PARTIES: [f32; 9] = [78.0, 150.0, 90.0, 64.0, 74.0, 60.0, 86.0, 46.0, 44.0];

fn dessiner_parties(ui: &mut Ui, etat: &EtatAffichage, donnees: Option<&DonneesJeu>) {
    let maintenant = format::maintenant();
    composants::panneau(ui, Some("Dernières parties"), |ui| {
        ui.horizontal(|ui| {
            for (e, l) in ["Quand", "Champion", "File", "Rôle", "Résultat", "KDA", "CS", "Durée", "LP"].into_iter().zip(L_PARTIES) {
                case(ui, l, doux(e), false);
            }
        });
        for p in etat.historique.iter() {
            ui.horizontal(|ui| {
                let l = L_PARTIES;
                case(ui, l[0], RichText::new(format::il_y_a(maintenant, p.date)).color(theme::TEXTE_DOUX), false);
                // Champion : portrait (demandé seulement si la ligne est visible) + nom.
                ui.allocate_ui_with_layout(vec2(l[1], 38.0), Layout::left_to_right(Align::Center), |ui| {
                    portrait(ui, id_dd(donnees, p.champion));
                    ui.add(egui::Label::new(texte(format::nom_champion(donnees, p.champion))).truncate());
                });
                case(ui, l[2], texte(nom_file(p.file)), false);
                case(ui, l[3], texte(p.role.map_or("—", format::libelle_role)), false);
                // Résultat : pastille V/D + mot ; un remake n'a pas de pastille.
                ui.allocate_ui_with_layout(vec2(l[4], 38.0), Layout::left_to_right(Align::Center), |ui| {
                    if resultat(p.remake, p.victoire).is_some() {
                        composants::pastille_victoire(ui, p.victoire);
                    }
                    ui.label(texte_resultat(p.remake, p.victoire).small());
                });
                case(ui, l[5], texte(format::kda(p.kills, p.morts, p.assists)), false);
                case(ui, l[6], texte(texte_cs(p.cs, p.cs_par_min())), false);
                case(ui, l[7], texte(format::duree(p.duree)), true);
                case(
                    ui,
                    l[8],
                    p.lp.map_or_else(|| RichText::new(""), |lp| RichText::new(format::variation_lp(lp)).color(couleur_lp(lp))),
                    true,
                );
            });
        }
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

/// Largeurs des colonnes de la tier list : rang, champion (portrait + nom), victoires, choisi, banni, parties.
const L_TIER: [f32; 6] = [32.0, 190.0, 80.0, 80.0, 80.0, 80.0];

/// Point d'entrée appelé par `ecrans.rs` : sans données de jeu, l'id des icônes est deviné
/// depuis le nom (voir `id_dd_approx`). Passer par `dessiner_tier_list_avec` pour l'id exact.
pub fn dessiner_tier_list(ui: &mut Ui, t: &Arc<VueTierList>, filtre: &mut FiltreTier) {
    dessiner_tier_list_avec(ui, t, filtre, None);
}

pub fn dessiner_tier_list_avec(ui: &mut Ui, t: &Arc<VueTierList>, filtre: &mut FiltreTier, donnees: Option<&DonneesJeu>) {
    ui.horizontal(|ui| {
        for role in crate::stats::ROLES {
            let libelle = if filtre.role == role {
                RichText::new(format::libelle_role(role)).color(theme::OR)
            } else {
                RichText::new(format::libelle_role(role)).color(theme::TEXTE_SECONDAIRE)
            };
            if ui.selectable_label(filtre.role == role, libelle).clicked() {
                filtre.role = role;
            }
        }
        ui.add_space(12.0);
        ui.label(
            RichText::new(format!("Patch {} · {} analysées", t.patch, format::pluriel(t.parties, "partie")))
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
    ui.horizontal(|ui| {
        for (e, l) in ["#", "Champion", "Victoires", "Choisi", "Banni", "Parties"].into_iter().zip(L_TIER) {
            case(ui, l, doux(e), false);
        }
    });
    let hauteur = TaillePortrait::Ligne.cote() + ui.spacing().item_spacing.y;
    // `show_rows` : seules les lignes visibles sont construites (donc seuls leurs portraits sont demandés).
    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, hauteur, indices.len(), |ui, visibles| {
        for rang in visibles {
            let l = &t.lignes[indices[rang]];
            ui.horizontal(|ui| {
                case(ui, L_TIER[0], RichText::new((rang + 1).to_string()).color(theme::TEXTE_DOUX), false);
                ui.allocate_ui_with_layout(vec2(L_TIER[1], 38.0), Layout::left_to_right(Align::Center), |ui| {
                    let dd = format::id_dd_depuis_nom(donnees, &l.champion).or_else(|| id_dd_approx(&l.champion));
                    portrait(ui, dd.as_deref());
                    ui.add(egui::Label::new(texte(&l.champion)).truncate());
                });
                case(ui, L_TIER[2], RichText::new(format::pourcent(l.winrate)).color(couleur_winrate(l.winrate)), false);
                case(ui, L_TIER[3], texte(format::pourcent(l.pick_rate)), false);
                case(ui, L_TIER[4], texte(format::pourcent(l.ban_rate)), false);
                case(ui, L_TIER[5], texte(format::milliers(l.parties)), false);
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gameflow::Phase;

    #[test]
    fn resultat_remake_victoire_defaite() {
        assert_eq!(resultat(true, true), None);
        assert_eq!(resultat(false, true), Some(("Victoire", true)));
        assert_eq!(resultat(false, false), Some(("Défaite", false)));
    }

    #[test]
    fn lp_couleur_selon_signe() {
        assert_eq!(couleur_lp(0), theme::VICTOIRE);
        assert_eq!(couleur_lp(18), theme::VICTOIRE);
        assert_eq!(couleur_lp(-15), theme::DEFAITE);
    }

    #[test]
    fn cs_en_francais() {
        assert_eq!(texte_cs(187, 6.43), "187 · 6,4/min");
    }

    #[test]
    fn pluriel_du_recap() {
        assert_eq!(texte_sans_moyenne(0), "Pas encore de moyenne sur ce champion.");
        assert!(texte_sans_moyenne(1).contains("1\u{a0}autre partie connue"));
        assert!(texte_sans_moyenne(3).contains("3\u{a0}autres parties connues"));
    }

    #[test]
    fn id_dd_sans_donnees() {
        assert_eq!(id_dd(None, 1), None);
        let d = super::super::demo::donnees();
        assert_eq!(id_dd(Some(&d), 117), Some("Lulu"));
        assert_eq!(id_dd(Some(&d), 65000), None);
    }

    #[test]
    fn id_dd_devine() {
        assert_eq!(id_dd_approx("Kai'Sa").as_deref(), Some("Kaisa"));
        assert_eq!(id_dd_approx("Vel'Koz").as_deref(), Some("Velkoz"));
        assert_eq!(id_dd_approx("Dr. Mundo").as_deref(), Some("DrMundo"));
        assert_eq!(id_dd_approx("Wukong").as_deref(), Some("MonkeyKing"));
        assert_eq!(id_dd_approx("Nunu & Willump").as_deref(), Some("Nunu"));
        assert_eq!(id_dd_approx(""), None);
    }

    /// Dessine l'historique et la tier list de démo sans fenêtre : pas de panique (2 trames).
    #[test]
    fn dessin_sans_panique() {
        let ctx = egui::Context::default();
        theme::appliquer(&ctx);
        let etat = super::super::demo::etat(Phase::InProgress);
        let donnees = etat.donnees.clone();
        let mut copie = RetourCopie::default();
        let mut filtre = FiltreTier::default();
        for _ in 0..2 {
            let mut sortie = ctx.run_ui(Default::default(), |ui| {
                dessiner(ui, &etat, donnees.as_deref(), &mut copie);
                if let Some(t) = &etat.tier_list {
                    dessiner_tier_list_avec(ui, t, &mut filtre, donnees.as_deref());
                }
            });
            sortie.textures_delta.clear();
        }
    }
}
