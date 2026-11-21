//! Dessin de la fenêtre principale : barre d'état, onglets, contenu.
//!
//! egui est une interface « immédiate » : à chaque image on redécrit TOUTE la
//! fenêtre à partir de l'état (pas d'objets « bouton » à garder en mémoire).
//! Ici on ne redessine qu'à chaque changement de l'état ou action de la souris,
//! jamais en boucle (voir fenetre.rs).
//!
//! Chaque onglet = une fonction `dessiner_xxx`. Pour en ajouter un : voir onglets.rs.

use eframe::egui::{self, Color32, RichText, Ui};

use super::format;
use super::onglets::{onglet_conseille, Onglet};
use super::reglages::{libelle_region, FormReglages, REGIONS};
use super::theme;
use super::protocole::Demande;
use crate::affichage::{EtatAffichage, VueChampSelect, VueCibleFiche, VueObjets, VueRunes};
use crate::compo::AnalyseEquipe;
use crate::ddragon::DonneesJeu;
use crate::gameflow::Phase;
use crate::live::{InstantanePartie, JoueurLive};
use crate::rangs::InfoJoueur;
use crate::stockage::Reglages;

/// Ce que la fenêtre retient entre deux images (en plus de l'état du cerveau).
pub struct Ecran {
    pub onglet: Onglet,
    /// Dernière phase vue : on change d'onglet seulement quand elle change.
    phase_vue: Option<Phase>,
    /// Formulaire des réglages (rechargé à chaque ouverture de la fenêtre).
    form: FormReglages,
    /// Résultat du dernier « Enregistrer ».
    retour_reglages: Option<(bool, String)>,
    /// Rôle choisi dans la tier list (+ lignes déjà filtrées).
    filtre_tier: super::historique::FiltreTier,
    /// Onglet Fiche : joueur choisi dans la liste.
    fiche: super::fiche::EtatOnglet,
    /// « Copié » affiché à côté du bouton utilisé (fiche ou récap).
    copie: super::fiche::RetourCopie,
}

impl Ecran {
    pub fn nouveau(etat: &EtatAffichage) -> Self {
        Self {
            onglet: onglet_conseille(etat.phase).unwrap_or(Onglet::Journal),
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
                ui.label(RichText::new("stat-traque").color(theme::OR).strong().size(18.0));
                ui.add_space(12.0);
                let couleur = if etat.client_connecte { theme::VERT } else { theme::TEXTE_DOUX };
                // Pastille de couleur (dessinée : la police par défaut n'a pas le caractère ●).
                let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 4.0, couleur);
                ui.label(RichText::new(format::statut(etat.client_connecte, etat.phase, etat.cle_api)).color(theme::TEXTE_DOUX));
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                for o in Onglet::TOUS.into_iter().filter(|o| o.visible(etat)) {
                    let texte = RichText::new(o.libelle()).size(14.0);
                    let texte = if ecran.onglet == o { texte.color(theme::OR).strong() } else { texte };
                    if ui.selectable_label(ecran.onglet == o, texte).clicked() {
                        ecran.onglet = o;
                    }
                }
            });
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
            Some(t) => super::historique::dessiner_tier_list(ui, t, &mut ecran.filtre_tier),
            None => vide(ui, "Pas encore de stats pour ce patch (stat-traque collecter 30)."),
        },
        Onglet::Journal => dessiner_journal(ui, &etat.messages),
        Onglet::Reglages => dessiner_reglages(ui, ecran),
    }
}

/// Message centré quand un onglet n'a rien à montrer.
fn vide(ui: &mut Ui, texte: &str) {
    ui.add_space(40.0);
    ui.vertical_centered(|ui| ui.label(RichText::new(texte).color(theme::TEXTE_DOUX).size(15.0)));
}

// ---------------------------------------------------------------- Champ select

fn dessiner_champ_select(ui: &mut Ui, cs: &VueChampSelect, donnees: Option<&DonneesJeu>) {
    // En-tête : mon champion, mon rôle, verrouillé ?
    ui.horizontal(|ui| {
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
        dessiner_equipe(&mut colonnes[0], "Ton équipe", theme::EQUIPE_BLEUE, &cs.allies, cs.analyse_allies.as_ref(), donnees);
        dessiner_equipe(&mut colonnes[1], "Équipe ennemie", theme::EQUIPE_ROUGE, &cs.ennemis, cs.analyse_ennemis.as_ref(), donnees);
    });

    if !cs.plan_de_jeu.is_empty() {
        ui.add_space(8.0);
        theme::carte().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            theme::titre(ui, "Plan de jeu");
            for conseil in &cs.plan_de_jeu {
                ui.label(format!("• {conseil}"));
            }
        });
    }
    if !cs.picks_conseilles.is_empty() {
        ui.add_space(8.0);
        theme::carte().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            theme::titre(ui, "Picks qui complètent la compo");
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
) {
    theme::carte().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.label(RichText::new(titre).color(couleur).strong().size(16.0));
        if champions.is_empty() {
            ui.label(RichText::new("pas encore de champions").color(theme::TEXTE_DOUX));
        } else {
            ui.label(champions.join(" · "));
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
        ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(rect.left(), haut), rect.max), 1.0, theme::TURQUOISE);
        reponse.on_hover_text(format!("{nom} : {}/3", format::decimal(v, 1)));
    }
}

fn dessiner_runes(ui: &mut Ui, r: &VueRunes) {
    theme::carte().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        theme::titre(ui, "Runes conseillées");
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
    theme::carte().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        theme::titre(ui, "Matchups");
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
    theme::carte().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
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

// ---------------------------------------------------------------- Partie

/// Onglet Partie. Renvoie true si le joueur a demandé une fiche (bouton « Fiche »).
fn dessiner_partie(ui: &mut Ui, etat: &EtatAffichage, donnees: Option<&DonneesJeu>) -> bool {
    let mut demandee = false;
    if etat.rangs.is_empty() && etat.partie.is_none() {
        let texte = if etat.cle_api {
            "Pas de partie en cours. Les rangs des 10 joueurs s'afficheront ici au lancement de la partie."
        } else {
            "Pas de partie en cours. Pour voir les rangs, enregistre ta clé API : stat-traque cle RGAPI-..."
        };
        vide(ui, texte);
        return false;
    }
    if !etat.rangs.is_empty() {
        let (bleus, rouges) = format::par_equipe(&etat.rangs);
        for (titre, couleur, joueurs) in [("Équipe bleue", theme::EQUIPE_BLEUE, bleus), ("Équipe rouge", theme::EQUIPE_ROUGE, rouges)] {
            if joueurs.is_empty() {
                continue;
            }
            theme::carte().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(RichText::new(titre).color(couleur).strong().size(16.0));
                demandee |= tableau_rangs(ui, titre, &joueurs, &etat.cibles_fiche, donnees);
            });
            ui.add_space(8.0);
        }
    }
    if let Some(p) = &etat.partie {
        theme::carte().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            theme::titre(ui, &format!("En direct · {} · {}", p.mode, format::duree(p.secondes)));
            tableau_live(ui, p, donnees);
        });
    }
    demandee
}

/// Tableau des rangs d'une équipe. Renvoie true si une fiche a été demandée.
fn tableau_rangs(ui: &mut Ui, id: &str, joueurs: &[&InfoJoueur], cibles: &[VueCibleFiche], donnees: Option<&DonneesJeu>) -> bool {
    let mut demandee = false;
    egui::Grid::new(("rangs", id)).num_columns(9).striped(true).spacing([14.0, 4.0]).show(ui, |ui| {
        for entete in ["Champion", "Joueur", "Solo", "Bilan solo", "Flex", "Maîtrise", "Meilleur vu", "Badges", ""] {
            ui.label(RichText::new(entete).color(theme::TEXTE_DOUX).small());
        }
        ui.end_row();
        for j in joueurs {
            let champion = RichText::new(format::nom_champion(donnees, j.champion)).strong();
            ui.label(if j.moi { champion.color(theme::OR) } else { champion });
            ui.label(if j.moi { format!("{} (toi)", j.nom) } else { j.nom.to_string() });
            match &j.solo {
                Some(r) => {
                    let texte = if r.serie { format!("{r}  série") } else { r.to_string() };
                    ui.label(RichText::new(texte).color(theme::couleur_palier(&r.palier)));
                    ui.label(format::bilan(r));
                }
                None => {
                    ui.label(RichText::new("Non classé").color(theme::TEXTE_DOUX));
                    ui.label("");
                }
            }
            ui.label(j.flex.as_ref().map_or(String::new(), |r| r.to_string()));
            ui.label(format::maitrise(j.maitrise));
            ui.label(format::meilleur_vu(j).unwrap_or_default());
            let badges: Vec<String> = j.badges.iter().map(|b| b.to_string()).collect();
            ui.label(RichText::new(badges.join(" · ")).color(theme::TURQUOISE).small());
            // Bouton « Fiche » : moi (base locale), ou un joueur connu par spectator.
            // Pas de bouton pour un joueur inconnu (le parent refuserait de toute façon).
            let demande = if j.moi {
                Some(Demande::MaFiche)
            } else {
                format::cible_du_joueur(cibles, &j.nom, j.champion).map(|c| Demande::Fiche { puuid: c.puuid.clone() })
            };
            match demande {
                Some(d) => {
                    let aide = if j.moi { "Ta fiche : 0 requête Riot" } else { "Radar, MMR estimé, champions : 11 requêtes au plus" };
                    if ui.small_button("Fiche").on_hover_text(aide).clicked() {
                        super::enfant::demander(&d);
                        demandee = true;
                    }
                }
                None => {
                    ui.label("");
                }
            }
            ui.end_row();
        }
    });
    demandee
}

fn tableau_live(ui: &mut Ui, p: &InstantanePartie, donnees: Option<&DonneesJeu>) {
    let (bleus, rouges): (Vec<&JoueurLive>, Vec<&JoueurLive>) = p.joueurs.iter().partition(|j| j.bleu);
    egui::Grid::new("live").num_columns(6).striped(true).spacing([14.0, 4.0]).show(ui, |ui| {
        for entete in ["Champion", "Joueur", "Niv.", "KDA", "CS", "Objets"] {
            ui.label(RichText::new(entete).color(theme::TEXTE_DOUX).small());
        }
        ui.end_row();
        for (couleur, joueurs) in [(theme::EQUIPE_BLEUE, bleus), (theme::EQUIPE_ROUGE, rouges)] {
            for j in joueurs {
                ui.label(RichText::new(&*j.champion).color(couleur).strong());
                ui.label(&*j.pseudo);
                ui.label(j.niveau.to_string());
                ui.label(format!("{}  ({})", format::kda(j.kills, j.morts, j.assists), format::ratio_kda(j.kills, j.morts, j.assists)));
                ui.label(j.cs.to_string());
                ui.label(RichText::new(format::noms_objets(donnees, &j.items).join(", ")).small());
                ui.end_row();
            }
        }
    });
}

// ---------------------------------------------------------------- Journal

fn dessiner_journal(ui: &mut Ui, messages: &[String]) {
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

fn dessiner_reglages(ui: &mut Ui, ecran: &mut Ecran) {
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
