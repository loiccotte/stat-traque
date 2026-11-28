//! Fiche joueur (F41-F44) et textes à partager sur Discord (F24).
//!
//! Une fiche = pour un joueur :
//!  - un RADAR de jeu à 7 axes (0 à 100), noté par rapport aux joueurs du même
//!    palier ET du même rôle (références collectées, voir metriques.rs) ;
//!  - un MMR ESTIMÉ (Riot ne publie pas le MMR : c'est une estimation, avec un niveau
//!    de confiance) ;
//!  - ses spécificités : champions, rôles, étiquettes de style, forme récente.
//!
//! Coût en requêtes Riot :
//!  - MA fiche : 0 (base locale de l'historique, voir historique.rs) ;
//!  - un AUTRE joueur, à la demande seulement (jamais automatiquement pour les 10) :
//!    1 (rang league-v4) + 1 (ids de ses parties classées) + 9 parties au plus =
//!    11 requêtes, toutes via le limiteur et le cache (parties : cache permanent,
//!    rang : 10 min). +1 si on part d'un Riot ID tapé (account-v1, cache 24 h).
//!    Une seule fiche à la fois.
//!
//! Conformité Riot : uniquement des données officielles (match-v5, league-v4). La
//! fiche d'un autre joueur n'est possible que s'il est CONNU : présent dans ma partie
//! en cours (spectator-v5, voir `autoriser`) ou Riot ID tapé par l'utilisateur. Jamais
//! en champ select classé (pseudos cachés par Riot : pas de dé-anonymisation).
//!
//! ---------- Formules ----------
//! Radar : pour chaque partie (rôle connu), chaque mesure x devient un « z-score »
//!   z = (x - moyenne du palier au rôle) / écart-type, borné à [-3, 3].
//!   Un axe = moyenne pondérée des z de ses mesures (poids négatif = moins c'est mieux),
//!   puis moyenne sur les parties. Score = 50 + 20 × z, borné à [0, 100] :
//!   50 = joueur moyen du palier, 70 = un écart-type au-dessus, 90 = deux.
//!   Axes (mesures, poids) : voir `Axe::composantes`.
//! MMR (moi) : sur mes 20 dernières classées solo/duo,
//!   écart = 12 × (gain moyen - perte moyenne) [borné ±300 LP]
//!         + 3 × (winrate - 50) × n / (n + 10)  [n = parties]
//!   (un compte dont le MMR est au-dessus du rang gagne plus qu'il ne perd).
//! MMR (autre) : rang et bilan de la saison (league-v4), + tendance des parties lues :
//!   écart = 6 × (winrate saison - 50) × n / (n + 50) [borné ±250]
//!         + 2 × (winrate récent - 50) × m / (m + 10).
//!   MMR estimé = LP « absolus » du rang (historique::lp_absolus) + écart.
//!   L'écart est arrondi à 25 LP, et la confiance ne dépasse jamais « moyenne ».

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::affichage::{self, VueAxeRadar, VueChampionFiche, VueCibleFiche, VueFiche, VueMmr, VueRoleFiche};
use crate::evenement::Evenement;
use crate::historique::{self, lp_absolus, Famille, Historique, Recap, ResumePartie};
use crate::metriques::{self, nom_palier, Mesure, References, NB_MESURES};
use crate::rangs::Rang;
use crate::riot_api::{Compteurs, ErreurApi, RiotApi};
use crate::log;

/// Parties lues pour un autre joueur (voir le coût en tête : 2 + 9 = 11 requêtes).
pub const PARTIES_AUTRE: u8 = 9;
/// Requêtes en parallèle pour les parties d'un autre joueur (le limiteur reste le juge).
const PARALLELE: usize = 3;
/// Parties de MA base utilisées pour le radar et la forme.
const PARTIES_RADAR: usize = 20;
/// Parties de MA base utilisées pour le champion pool et les rôles.
const PARTIES_POOL: usize = 50;
/// Écart (en z moyen) à partir duquel une étiquette de style est donnée.
const SEUIL_ETIQUETTE: f32 = 0.75;
/// Parties avec métriques nécessaires pour donner des étiquettes de style.
const MIN_PARTIES_STYLE: u32 = 3;
/// Palier de référence d'un joueur non classé (Or) : la référence est alors approximative.
const PALIER_NON_CLASSE: usize = 3;

// =====================================================================
// 1. Radar
// =====================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axe {
    Combat,
    Farm,
    Vision,
    Objectifs,
    Lane,
    Survie,
    Agressivite,
}

pub const AXES: [Axe; 7] = [Axe::Combat, Axe::Farm, Axe::Vision, Axe::Objectifs, Axe::Lane, Axe::Survie, Axe::Agressivite];

impl Axe {
    pub fn nom(self) -> &'static str {
        match self {
            Axe::Combat => "Combat",
            Axe::Farm => "Farm",
            Axe::Vision => "Vision",
            Axe::Objectifs => "Objectifs",
            Axe::Lane => "Phase de lane",
            Axe::Survie => "Survie",
            Axe::Agressivite => "Agressivité",
        }
    }

    /// Les mesures d'un axe et leur poids (négatif : moins c'est mieux).
    pub fn composantes(self) -> &'static [(Mesure, f32)] {
        match self {
            // Impact en combat : dégâts, part des dégâts de l'équipe, participation, contrôle.
            Axe::Combat => &[(Mesure::DegatsMin, 1.0), (Mesure::PartDegats, 1.0), (Mesure::Kp, 1.0), (Mesure::Immobilisations, 0.5)],
            // Farm : CS/min surtout, CS à 10 min, or/min.
            Axe::Farm => &[(Mesure::CsMin, 1.5), (Mesure::Cs10, 1.0), (Mesure::OrMin, 1.0)],
            Axe::Vision => &[
                (Mesure::VisionMin, 1.5),
                (Mesure::BalisesMin, 1.0),
                (Mesure::Pinks, 1.0),
                (Mesure::BalisesDetruites, 1.0),
            ],
            // Objectifs : dégâts aux objectifs et bâtiments, dragons/barons/hérauts, tours.
            Axe::Objectifs => &[
                (Mesure::ObjectifsMin, 1.0),
                (Mesure::BatimentsMin, 1.0),
                (Mesure::Epiques, 1.0),
                (Mesure::Tours, 1.0),
            ],
            // Phase de lane : avance de CS, avance or+XP à 7 min, avance de niveaux, plaques.
            Axe::Lane => &[
                (Mesure::AvanceCs, 1.0),
                (Mesure::AvanceEarly, 1.0),
                (Mesure::AvanceNiveau, 1.0),
                (Mesure::Plaques, 0.5),
            ],
            // Survie : peu de morts (compte double), beaucoup encaissé par vie.
            Axe::Survie => &[(Mesure::Morts10, -2.0), (Mesure::EncaissesParVie, 1.0)],
            // Agressivité : solo kills, kills par 10 min, dégâts encaissés (aller au contact).
            Axe::Agressivite => &[(Mesure::SoloKills, 1.0), (Mesure::Kills10, 1.0), (Mesure::EncaissesMin, 0.5)],
        }
    }
}

/// z-score borné. L'écart-type a un plancher : une référence très serrée (ou nulle)
/// ne doit pas donner des z énormes.
fn z_score(x: f32, r: &metriques::Reference) -> f32 {
    let plancher = (r.moyenne.abs() * 0.05).max(0.01);
    ((x - r.moyenne) / r.ecart_type.max(plancher)).clamp(-3.0, 3.0)
}

/// Score 0-100 à partir d'un z moyen.
pub fn score(z: f32) -> u8 {
    (50.0 + 20.0 * z).round().clamp(0.0, 100.0) as u8
}

#[derive(Debug, Clone, PartialEq)]
pub struct Radar {
    /// Score de chaque axe (ordre de `AXES`), None faute de données.
    pub scores: [Option<u8>; 7],
    /// z moyen de chaque mesure (sert aux étiquettes), None faute de données.
    pub z_mesures: [Option<f32>; NB_MESURES],
    /// Parties utilisées (Faille, rôle connu, pas de remake).
    pub parties: u32,
    /// Parties qui avaient les métriques détaillées.
    pub parties_detaillees: u32,
    /// Au moins une référence venait d'un palier voisin ou des valeurs par défaut.
    pub approximatif: bool,
}

/// Radar d'un ensemble de parties, par rapport au palier `palier` (indice de
/// `metriques::PALIERS_REFERENCE`). Chaque partie est comparée à la référence de SON rôle.
pub fn radar(parties: &[ResumePartie], palier: usize, refs: &References) -> Radar {
    let mut somme_axes = [(0.0f32, 0u32); 7];
    let mut somme_z = [(0.0f32, 0u32); NB_MESURES];
    let mut approximatif = false;
    let mut n = 0;
    let mut detaillees = 0;
    for p in parties.iter().filter(|p| !p.remake && p.famille() == Famille::Faille) {
        let Some(role) = p.role else { continue };
        n += 1;
        if p.metriques.is_some_and(|m| m.source == metriques::Source::Completes) {
            detaillees += 1;
        }
        // z de chaque mesure connue pour cette partie.
        let mut z = [None; NB_MESURES];
        for (i, x) in metriques::mesures(p).iter().enumerate() {
            if let Some(x) = x {
                let r = refs.reference(palier, role, metriques::MESURES[i]);
                approximatif |= r.approximative;
                z[i] = Some(z_score(*x, &r));
                somme_z[i].0 += z[i].unwrap_or(0.0);
                somme_z[i].1 += 1;
            }
        }
        for (a, axe) in AXES.iter().enumerate() {
            let (mut total, mut poids) = (0.0, 0.0);
            for (m, w) in axe.composantes() {
                if let Some(zm) = z[*m as usize] {
                    total += w * zm;
                    poids += w.abs();
                }
            }
            if poids > 0.0 {
                somme_axes[a].0 += total / poids;
                somme_axes[a].1 += 1;
            }
        }
    }
    let moyenne = |(s, k): (f32, u32)| (k > 0).then(|| s / k as f32);
    Radar {
        scores: somme_axes.map(|x| moyenne(x).map(score)),
        z_mesures: somme_z.map(moyenne),
        parties: n,
        parties_detaillees: detaillees,
        approximatif,
    }
}

// =====================================================================
// 2. MMR estimé
// =====================================================================

/// Confiance dans l'estimation. Volontairement PLAFONNÉE à « moyenne » : sans le
/// vrai MMR (Riot ne le publie pas), on ne peut jamais en être sûr.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Confiance {
    Faible,
    Moyenne,
}

impl Confiance {
    pub fn libelle(self) -> &'static str {
        match self {
            Confiance::Faible => "faible",
            Confiance::Moyenne => "moyenne",
        }
    }
}

/// L'écart est arrondi à 25 LP près : plus de précision serait trompeuse.
const ARRONDI_MMR: f32 = 25.0;

#[derive(Debug, Clone, PartialEq)]
pub struct EstimationMmr {
    /// MMR estimé, sur l'échelle des LP « absolus » (voir historique::lp_absolus).
    pub lp_absolus: i32,
    /// Écart avec le rang affiché, en LP (+ = au-dessus).
    pub ecart: i32,
    pub confiance: Confiance,
    pub raisons: Vec<String>,
}

impl EstimationMmr {
    fn nouvelle(rang: &Rang, ecart: f32, confiance: Confiance, raisons: Vec<String>) -> Self {
        let ecart = ((ecart / ARRONDI_MMR).round() * ARRONDI_MMR) as i32;
        EstimationMmr { lp_absolus: (lp_absolus(rang) + ecart).max(0), ecart, confiance, raisons }
    }
}

/// Nom français d'un palier league-v4 ("EMERALD" -> "Émeraude").
pub fn palier_fr(palier: &str) -> &'static str {
    match palier {
        "IRON" => "Fer",
        "BRONZE" => "Bronze",
        "SILVER" => "Argent",
        "GOLD" => "Or",
        "PLATINUM" => "Platine",
        "EMERALD" => "Émeraude",
        "DIAMOND" => "Diamant",
        "MASTER" => "Master",
        "GRANDMASTER" => "Grand Master",
        "CHALLENGER" => "Challenger",
        _ => "?",
    }
}

/// « Émeraude II 45 LP ».
pub fn rang_fr(r: &Rang) -> String {
    if r.division.is_empty() || metriques::indice_palier(&r.palier) == Some(7) {
        format!("{} {} LP", palier_fr(&r.palier), r.lp)
    } else {
        format!("{} {} {} LP", palier_fr(&r.palier), r.division, r.lp)
    }
}

/// LP absolus -> rang équivalent : 1590 -> « ~Or I », 2950 -> « ~Master 150 LP ».
pub fn rang_equivalent(lp: i32) -> String {
    let lp = lp.max(0);
    if lp >= 2800 {
        return format!("~Master+ {} LP", lp - 2800);
    }
    let palier = (lp / 400) as usize;
    let division = ["IV", "III", "II", "I"][((lp % 400) / 100) as usize];
    format!("~{} {division}", nom_palier(palier))
}

/// Moyenne d'une liste (0 si vide).
fn moyenne(v: &[f32]) -> f32 {
    if v.is_empty() { 0.0 } else { v.iter().sum::<f32>() / v.len() as f32 }
}

/// Mon MMR d'après MES parties classées solo/duo (base locale, plus récentes d'abord).
/// None si je ne suis pas classé.
pub fn mmr_moi(rang: Option<&Rang>, parties: &[ResumePartie]) -> Option<EstimationMmr> {
    let rang = rang?;
    let solo: Vec<&ResumePartie> = parties.iter().filter(|p| p.file == 420 && !p.remake).take(PARTIES_RADAR).collect();
    let gains: Vec<f32> = solo.iter().filter_map(|p| p.lp).filter(|l| *l > 0).map(|l| l as f32).collect();
    let pertes: Vec<f32> = solo.iter().filter_map(|p| p.lp).filter(|l| *l < 0).map(|l| -l as f32).collect();
    let mut raisons = Vec::new();
    let mut ecart = 0.0;
    let avec_lp = gains.len() + pertes.len();
    if !gains.is_empty() && !pertes.is_empty() {
        let (g, p) = (moyenne(&gains), moyenne(&pertes));
        ecart += (12.0 * (g - p)).clamp(-300.0, 300.0);
        raisons.push(format!("gain moyen +{g:.0} LP, perte moyenne -{p:.0} LP ({avec_lp} parties)"));
    }
    let n = solo.len() as f32;
    if n > 0.0 {
        let wr = solo.iter().filter(|p| p.victoire).count() as f32 * 100.0 / n;
        ecart += 3.0 * (wr - 50.0) * n / (n + 10.0);
        raisons.push(format!("{wr:.0} % de victoires sur les {} dernières classées", solo.len()));
    }
    let confiance = match avec_lp {
        4.. => Confiance::Moyenne,
        _ => Confiance::Faible,
    };
    if raisons.is_empty() {
        raisons.push("aucune partie classée récente connue : MMR supposé égal au rang".into());
    }
    Some(EstimationMmr::nouvelle(rang, ecart, confiance, raisons))
}

/// MMR d'un autre joueur : bilan de la saison (league-v4) + tendance de ses parties lues.
pub fn mmr_autre(rang: Option<&Rang>, recentes: &[ResumePartie]) -> Option<EstimationMmr> {
    let rang = rang?;
    let mut raisons = Vec::new();
    let n = (rang.victoires + rang.defaites) as f32;
    let mut ecart = 0.0;
    if n > 0.0 {
        ecart += (6.0 * (rang.winrate() - 50.0) * n / (n + 50.0)).clamp(-250.0, 250.0);
        raisons.push(format!("{:.0} % de victoires sur {} parties cette saison", rang.winrate(), n as u32));
    }
    let solo: Vec<&ResumePartie> = recentes.iter().filter(|p| p.file == 420 && !p.remake).collect();
    let m = solo.len() as f32;
    if m > 0.0 {
        let wr = solo.iter().filter(|p| p.victoire).count() as f32 * 100.0 / m;
        ecart += 2.0 * (wr - 50.0) * m / (m + 10.0);
        raisons.push(format!("tendance : {wr:.0} % sur ses {} dernières classées", solo.len()));
    }
    let confiance = match n as u32 {
        30.. => Confiance::Moyenne,
        _ => Confiance::Faible,
    };
    Some(EstimationMmr::nouvelle(rang, ecart, confiance, raisons))
}

// =====================================================================
// 3. Spécificités : champions, rôles, style, forme
// =====================================================================

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Bilan {
    pub parties: u32,
    pub victoires: u32,
    pub kills: u32,
    pub morts: u32,
    pub assists: u32,
}

impl Bilan {
    fn ajouter(&mut self, p: &ResumePartie) {
        self.parties += 1;
        self.victoires += p.victoire as u32;
        self.kills += p.kills as u32;
        self.morts += p.morts as u32;
        self.assists += p.assists as u32;
    }
    pub fn winrate(&self) -> f32 {
        if self.parties == 0 { 0.0 } else { self.victoires as f32 * 100.0 / self.parties as f32 }
    }
    pub fn kda(&self) -> f32 {
        (self.kills + self.assists) as f32 / self.morts.max(1) as f32
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Specificites {
    /// Parties comptées (Faille, sans remake).
    pub parties: u32,
    /// Champions les plus joués d'abord (5 au plus).
    pub champions: Vec<(u16, Bilan)>,
    pub roles: Vec<(&'static str, u32)>,
    pub etiquettes: Vec<String>,
    /// "V" / "D", plus récente d'abord (10 au plus).
    pub forme: String,
    pub serie: i32,
}

/// Regroupe par clé, les plus jouées d'abord (à égalité : ordre de la clé, stable).
fn compter_par<K: Ord + Copy>(parties: &[&ResumePartie], cle: impl Fn(&ResumePartie) -> Option<K>) -> Vec<(K, Bilan)> {
    let mut v: Vec<(K, Bilan)> = Vec::new();
    for p in parties {
        let Some(k) = cle(p) else { continue };
        match v.iter_mut().find(|(x, _)| *x == k) {
            Some((_, b)) => b.ajouter(p),
            None => {
                let mut b = Bilan::default();
                b.ajouter(p);
                v.push((k, b));
            }
        }
    }
    v.sort_by(|a, b| b.1.parties.cmp(&a.1.parties).then(a.0.cmp(&b.0)));
    v
}

/// Étiquettes de style d'après le radar (z moyen des mesures) et le champion pool.
pub fn etiquettes(radar: &Radar, champions: &[(u16, Bilan)], parties: u32) -> Vec<String> {
    let mut e = Vec::new();
    // One-trick : plus de 60 % des parties sur un seul champion (et assez de parties).
    if let Some((_, b)) = champions.first() {
        if parties >= 5 && b.parties * 100 > parties * 60 {
            e.push("One-trick".to_string());
        }
    }
    if radar.parties < MIN_PARTIES_STYLE {
        return e;
    }
    let z = |m: Mesure| radar.z_mesures[m as usize];
    let haut = |m: Mesure| z(m).is_some_and(|x| x >= SEUIL_ETIQUETTE);
    let bas = |m: Mesure| z(m).is_some_and(|x| x <= -SEUIL_ETIQUETTE);
    let axe = |a: Axe| radar.scores[AXES.iter().position(|x| *x == a).unwrap_or(0)];
    let regles: [(bool, &str); 11] = [
        (haut(Mesure::CsMin), "Farmer"),
        (haut(Mesure::DegatsMin) || haut(Mesure::PartDegats), "Gros dégâts"),
        (axe(Axe::Vision).is_some_and(|s| s >= 65), "Vision"),
        (haut(Mesure::Kp), "Roamer / KP élevé"),
        (axe(Axe::Lane).is_some_and(|s| s >= 65), "Joueur d'early"),
        // Scaling : perd souvent la phase de lane mais fait ses dégâts plus tard.
        (z(Mesure::AvanceEarly).is_some_and(|x| x <= -0.5) && z(Mesure::DegatsMin).is_some_and(|x| x >= 0.25), "Scaling"),
        // Split push : beaucoup de bâtiments, peu de participation aux combats.
        (haut(Mesure::BatimentsMin) && z(Mesure::Kp).is_some_and(|x| x < 0.0), "Split push"),
        (haut(Mesure::Morts10), "Prend des risques"),
        (bas(Mesure::Morts10), "Solide"),
        (haut(Mesure::PartEncaisses), "Frontline"),
        (haut(Mesure::SoinsMin), "Soins / boucliers"),
    ];
    e.extend(regles.iter().filter(|(ok, _)| *ok).map(|(_, nom)| nom.to_string()));
    e
}

/// Spécificités d'un ensemble de parties (plus récentes d'abord).
pub fn specificites(parties: &[ResumePartie], radar: &Radar) -> Specificites {
    let jouees: Vec<&ResumePartie> =
        parties.iter().filter(|p| !p.remake && p.famille() == Famille::Faille).take(PARTIES_POOL).collect();
    let mut champions = compter_par(&jouees, |p| Some(p.champion));
    let roles = compter_par(&jouees, |p| p.role).into_iter().map(|(r, b)| (r, b.parties)).collect();
    let n = jouees.len() as u32;
    let etiquettes = etiquettes(radar, &champions, n);
    champions.truncate(5);
    // Forme : classées d'abord ; sans classée, toute la Faille.
    let classees: Vec<&&ResumePartie> = jouees.iter().filter(|p| [420, 440].contains(&p.file)).collect();
    let forme_de: Vec<&ResumePartie> =
        if classees.is_empty() { jouees.clone() } else { classees.into_iter().copied().collect() };
    let forme: String = forme_de.iter().take(10).map(|p| if p.victoire { 'V' } else { 'D' }).collect();
    let serie = match forme_de.first() {
        None => 0,
        Some(premiere) => {
            let k = forme_de.iter().take_while(|p| p.victoire == premiere.victoire).count() as i32;
            if premiere.victoire { k } else { -k }
        }
    };
    Specificites { parties: n, champions, roles, etiquettes, forme, serie }
}

// =====================================================================
// 4. La fiche complète
// =====================================================================

#[derive(Debug, Clone, PartialEq)]
pub struct Fiche {
    pub puuid: String,
    pub nom: String,
    pub moi: bool,
    pub rang: Option<Rang>,
    /// Indice du palier de référence (metriques::PALIERS_REFERENCE).
    pub palier: usize,
    pub radar: Radar,
    pub mmr: Option<EstimationMmr>,
    pub specificites: Specificites,
    /// Requêtes Riot mesurées pour cette fiche.
    pub requetes: Compteurs,
    pub erreur: Option<String>,
}

/// Calcule une fiche à partir de parties déjà connues (plus récentes d'abord).
pub fn calculer(puuid: &str, nom: &str, moi: bool, rang: Option<Rang>, parties: &[ResumePartie], refs: &References) -> Fiche {
    let palier = rang.as_ref().and_then(|r| metriques::indice_palier(&r.palier)).unwrap_or(PALIER_NON_CLASSE);
    let recentes: Vec<ResumePartie> =
        parties.iter().filter(|p| !p.remake && p.famille() == Famille::Faille).take(PARTIES_RADAR).cloned().collect();
    let mut r = radar(&recentes, palier, refs);
    // Non classé : référence « Or » par défaut, donc forcément approximative.
    r.approximatif |= rang.is_none();
    let specificites = specificites(parties, &r);
    let mmr = if moi { mmr_moi(rang.as_ref(), parties) } else { mmr_autre(rang.as_ref(), parties) };
    Fiche {
        puuid: puuid.to_string(),
        nom: nom.to_string(),
        moi,
        rang,
        palier,
        radar: r,
        mmr,
        specificites,
        requetes: Compteurs::default(),
        erreur: None,
    }
}

/// Version affichable (noms des champions traduits), avec le texte Discord.
pub fn vers_vue(f: &Fiche, nom_champion: &dyn Fn(u16) -> String) -> VueFiche {
    let s = &f.specificites;
    let total = s.parties.max(1) as f32;
    let mut vue = VueFiche {
        puuid: f.puuid.clone(),
        nom: f.nom.clone(),
        moi: f.moi,
        rang: f.rang.as_ref().map(rang_fr),
        palier_reference: nom_palier(f.palier).to_string(),
        reference_approximative: f.radar.approximatif,
        radar: AXES
            .iter()
            .zip(f.radar.scores)
            .map(|(a, score)| VueAxeRadar { nom: a.nom().to_string(), score })
            .collect(),
        mmr: f.mmr.as_ref().map(|m| VueMmr {
            rang_equivalent: rang_equivalent(m.lp_absolus),
            ecart_lp: m.ecart,
            confiance: m.confiance.libelle().to_string(),
            raisons: m.raisons.clone(),
        }),
        champions: s
            .champions
            .iter()
            .map(|(c, b)| VueChampionFiche {
                champion: nom_champion(*c),
                parties: b.parties,
                victoires: b.victoires,
                winrate: b.winrate(),
                part: b.parties as f32 * 100.0 / total,
                kda: b.kda(),
            })
            .collect(),
        roles: s
            .roles
            .iter()
            .map(|(r, n)| VueRoleFiche { role: r.to_string(), parties: *n, part: *n as f32 * 100.0 / total })
            .collect(),
        etiquettes: s.etiquettes.clone(),
        forme: s.forme.clone(),
        serie: s.serie,
        parties_analysees: f.radar.parties,
        requetes: f.requetes.reseau as u32,
        erreur: f.erreur.clone(),
        texte_partage: String::new(),
    };
    vue.texte_partage = fiche_discord(&vue);
    vue
}

// =====================================================================
// 5. Chargement : moi (base locale) ou un autre joueur (API Riot)
// =====================================================================

/// Ce dont la fiche d'un autre joueur a besoin chez Riot (faux en test).
pub trait SourceFiche: Sync {
    fn rangs(&self, puuid: &str) -> Result<Value, ErreurApi>;
    fn ids_classees(&self, puuid: &str, nombre: u8) -> Result<Vec<String>, ErreurApi>;
    fn partie(&self, id: &str) -> Result<Value, ErreurApi>;
}

impl SourceFiche for RiotApi {
    fn rangs(&self, puuid: &str) -> Result<Value, ErreurApi> {
        RiotApi::rangs(self, puuid) // cache 10 min
    }
    fn ids_classees(&self, puuid: &str, nombre: u8) -> Result<Vec<String>, ErreurApi> {
        let v = self.ids_parties_classees(puuid, nombre)?; // cache 10 min
        Ok(v.as_array().into_iter().flatten().filter_map(|i| i.as_str().map(String::from)).collect())
    }
    fn partie(&self, id: &str) -> Result<Value, ErreurApi> {
        RiotApi::partie(self, id) // partie terminée : cache permanent
    }
}

/// Résultat de la lecture d'une partie : son résumé (None si le joueur n'y est pas),
/// ou l'erreur Riot.
type LecturePartie = Result<Option<ResumePartie>, ErreurApi>;

/// Rang solo et dernières parties classées d'un autre joueur : 2 + `nombre` requêtes
/// au plus. Les parties sont lues EN PARALLÈLE (`PARALLELE` à la fois, le limiteur
/// global fait respecter les limites de la clé).
/// Ce que `charger_autre` a obtenu de Riot.
#[derive(Debug, Default)]
pub struct Chargement {
    pub rang: Option<Rang>,
    /// Plus récentes d'abord.
    pub parties: Vec<ResumePartie>,
    pub erreur: Option<ErreurApi>,
    /// Requêtes de CETTE fiche seulement (compteurs par fil, voir riot_api).
    pub requetes: Compteurs,
}

pub fn charger_autre(source: &impl SourceFiche, puuid: &str, nombre: u8) -> Chargement {
    let avant = Compteurs::du_fil();
    let mut c = Chargement::default();
    match source.rangs(puuid) {
        Ok(v) => c.rang = crate::rangs::lire_rangs(&v).0,
        Err(ErreurApi::Introuvable) => {}
        // Clé expirée / absente, réseau... : inutile d'insister.
        Err(e) => {
            c.erreur = Some(e);
            c.requetes = Compteurs::du_fil_depuis(avant);
            return c;
        }
    }
    let ids = match source.ids_classees(puuid, nombre) {
        Ok(ids) => ids,
        Err(e) => {
            c.erreur = Some(e);
            c.requetes = Compteurs::du_fil_depuis(avant);
            return c;
        }
    };
    c.requetes = Compteurs::du_fil_depuis(avant);
    let ids: Vec<String> = ids.into_iter().take(nombre as usize).collect();
    // Chaque fil prend l'indice suivant (`fetch_add`) jusqu'à épuisement, et mesure
    // ses propres requêtes (rendues à la fin, quand le fil se termine).
    let suivant = AtomicUsize::new(0);
    let resultats: Mutex<Vec<(usize, LecturePartie)>> = Mutex::new(Vec::new());
    let couts: Vec<Compteurs> = std::thread::scope(|s| {
        let fils: Vec<_> = (0..PARALLELE.min(ids.len()))
            .map(|_| {
                s.spawn(|| {
                    let debut = Compteurs::du_fil();
                    loop {
                        let i = suivant.fetch_add(1, Ordering::SeqCst);
                        let Some(id) = ids.get(i) else { break };
                        let r = source.partie(id).map(|json| ResumePartie::depuis_match(&json, puuid));
                        let bloque = matches!(r, Err(ErreurApi::CleInvalide | ErreurApi::PasDeCle));
                        resultats.lock().unwrap().push((i, r));
                        if bloque {
                            suivant.store(usize::MAX / 2, Ordering::SeqCst); // les autres fils s'arrêtent
                            break;
                        }
                    }
                    Compteurs::du_fil_depuis(debut)
                })
            })
            .collect();
        fils.into_iter().filter_map(|f| f.join().ok()).collect()
    });
    c.requetes = couts.into_iter().fold(c.requetes, Compteurs::plus);
    let mut resultats = resultats.into_inner().unwrap_or_default();
    resultats.sort_by_key(|(i, _)| *i); // ordre des ids : plus récentes d'abord
    for (_, r) in resultats {
        match r {
            Ok(Some(p)) => c.parties.push(p),
            Ok(None) | Err(ErreurApi::Introuvable) => {}
            Err(e) => c.erreur = c.erreur.or(Some(e)),
        }
    }
    c
}

/// Références du patch (lignes `R` du fichier de stats, lues en flux). Vides si le
/// fichier manque : le radar se rabat alors sur les valeurs par défaut.
pub fn references(patch: Option<&str>) -> References {
    patch
        .and_then(crate::stats::trouver_fichier)
        .and_then(|f| metriques::charger_references(&f))
        .unwrap_or_default()
}

/// MA fiche : base locale seulement, 0 requête.
pub fn fiche_moi(dossier: &Path, puuid: &str, refs: &References) -> Fiche {
    let h = Historique::charger(dossier, puuid);
    let rang = h.progression(0).solo;
    // Mon Riot ID (lu dans mes parties, voir historique::riot_id_local), sinon « Moi ».
    let nom = historique::riot_id_local(dossier, puuid).unwrap_or_else(|| "Moi".to_string());
    calculer(puuid, &nom, true, rang, &h.parties, refs)
}

/// Fiche d'un autre joueur (≤ 11 requêtes).
pub fn fiche_autre(source: &impl SourceFiche, puuid: &str, nom: &str, refs: &References) -> Fiche {
    let c = charger_autre(source, puuid, PARTIES_AUTRE);
    let mut f = calculer(puuid, nom, false, c.rang, &c.parties, refs);
    f.erreur = c.erreur.map(|e| e.to_string());
    f.requetes = c.requetes;
    f
}

// =====================================================================
// 6. À la demande, depuis l'app (une fiche à la fois)
// =====================================================================

/// Joueurs dont la fiche peut être demandée : (puuid, Riot ID, champion).
static AUTORISES: Mutex<Vec<(String, String, u16)>> = Mutex::new(Vec::new());
static EN_COURS: AtomicBool = AtomicBool::new(false);
/// Dernière demande arrivée pendant un calcul : (puuid, nom, moi).
static DEMANDE: Mutex<Option<(String, String, bool)>> = Mutex::new(None);

/// Appelé quand spectator-v5 donne les 10 joueurs de MA partie : seuls eux (et moi)
/// peuvent avoir une fiche depuis l'app. Remplace la liste précédente.
pub fn autoriser(joueurs: Vec<(String, String, u16)>) {
    let cibles: Vec<VueCibleFiche> =
        joueurs.iter().map(|(p, n, c)| VueCibleFiche { puuid: p.clone(), nom: n.clone(), champion: *c }).collect();
    *AUTORISES.lock().unwrap() = joueurs;
    affichage::modifier(|e| e.cibles_fiche = cibles);
}

/// Riot ID d'un joueur autorisé, None s'il n'est pas connu (=> fiche refusée).
pub fn nom_autorise(puuid: &str) -> Option<String> {
    AUTORISES.lock().unwrap().iter().find(|(p, _, _)| p == puuid).map(|(_, n, _)| n.clone())
}

/// Lance le calcul d'une fiche en arrière-plan ; le résultat arrive en
/// `Evenement::Fiche`. Si une fiche est déjà en cours, la demande attend la fin
/// (seule la DERNIÈRE demande est gardée).
/// `api` : None sans clé API ; ma propre fiche marche quand même (0 requête).
pub fn lancer(api: Option<Arc<RiotApi>>, puuid: String, nom: String, moi: bool, patch: Option<String>, tx: Sender<Evenement>) {
    *DEMANDE.lock().unwrap() = Some((puuid, nom, moi));
    if EN_COURS.swap(true, Ordering::SeqCst) {
        return;
    }
    affichage::modifier(|e| e.fiche_en_cours = true);
    let lance = std::thread::Builder::new().name("fiche".into()).spawn(move || {
        let refs = references(patch.as_deref());
        loop {
            let Some((puuid, nom, moi)) = DEMANDE.lock().unwrap().take() else {
                // Indicateur éteint AVANT de libérer la place : sinon une nouvelle
                // demande pourrait l'allumer puis le voir éteint par nous.
                affichage::modifier(|e| e.fiche_en_cours = false);
                EN_COURS.store(false, Ordering::SeqCst);
                // Une demande a pu arriver juste avant `store(false)` : on la reprend
                // (sauf si un autre fil l'a déjà fait : `swap` renvoie l'ancienne valeur).
                if DEMANDE.lock().unwrap().is_some() && !EN_COURS.swap(true, Ordering::SeqCst) {
                    affichage::modifier(|e| e.fiche_en_cours = true);
                    continue;
                }
                break;
            };
            let f = match (&api, moi) {
                (_, true) => fiche_moi(&historique::dossier(), &puuid, &refs),
                (Some(api), false) => fiche_autre(&**api, &puuid, &nom, &refs),
                (None, false) => continue, // refusé en amont (cerveau) : pas de clé
            };
            let _ = tx.send(Evenement::Fiche(Box::new(f)));
        }
    });
    if lance.is_err() {
        // Fil impossible à créer : on libère la place, sinon plus aucune fiche ne partirait.
        EN_COURS.store(false, Ordering::SeqCst);
        affichage::modifier(|e| e.fiche_en_cours = false);
    }
}

/// Appelé par le cerveau : journal + état d'affichage.
pub fn publier(f: Fiche, donnees: Option<&crate::ddragon::DonneesJeu>) {
    let nom = |c: u16| donnees.map_or(c.to_string(), |d| d.nom_champion(c).to_string());
    let vue = vers_vue(&f, &nom);
    for ligne in texte_console(&vue) {
        log!("{ligne}");
    }
    log!("Fiche : {}.", f.requetes);
    affichage::modifier(move |e| e.fiche = Some(Arc::new(vue)));
}

// =====================================================================
// 7. Textes : console et Discord
// =====================================================================

/// Nom affiché d'un rôle ("bottom" -> "ADC").
pub fn role_fr(role: &str) -> &str {
    match role {
        "top" => "Top",
        "jungle" => "Jungle",
        "middle" => "Mid",
        "bottom" => "ADC",
        "utility" => "Support",
        autre => autre,
    }
}

/// Barre de texte : 72 -> « ▰▰▰▰▰▰▰▱▱▱ ».
pub fn barre(score: u8) -> String {
    let pleins = ((score as usize) + 5) / 10;
    "▰".repeat(pleins.min(10)) + &"▱".repeat(10 - pleins.min(10))
}

fn ligne_radar(a: &VueAxeRadar) -> String {
    match a.score {
        Some(s) => format!("{:<14} {} {s:>3}", a.nom, barre(s)),
        None => format!("{:<14} {} {:>3}", a.nom, "·".repeat(10), "?"),
    }
}

fn texte_mmr(m: &VueMmr) -> String {
    format!("{} ({:+} LP par rapport au rang affiché, confiance {})", m.rang_equivalent, m.ecart_lp, m.confiance)
}

fn texte_champions(v: &VueFiche) -> String {
    v.champions
        .iter()
        .take(3)
        .map(|c| format!("{} ({} p., {:.0} %)", c.champion, c.parties, c.winrate))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Texte de la fiche pour la console (`log!`).
pub fn texte_console(v: &VueFiche) -> Vec<String> {
    let mut t = vec![format!(
        "=== Fiche : {} — {} ===",
        v.nom,
        v.rang.as_deref().unwrap_or("non classé")
    )];
    if let Some(e) = &v.erreur {
        t.push(format!("  (fiche incomplète : {e})"));
    }
    let approx = if v.reference_approximative { ", référence approximative" } else { "" };
    t.push(format!(
        "  Radar ({} parties, comparé aux {} du même rôle{approx}) :",
        v.parties_analysees, v.palier_reference
    ));
    t.extend(v.radar.iter().map(|a| format!("    {}", ligne_radar(a))));
    match &v.mmr {
        Some(m) => {
            t.push(format!("  MMR estimé : {}", texte_mmr(m)));
            t.extend(m.raisons.iter().map(|r| format!("    - {r}")));
        }
        None => t.push("  MMR estimé : non classé en solo/duo".into()),
    }
    if !v.champions.is_empty() {
        t.push("  Champions :".into());
        for c in &v.champions {
            t.push(format!(
                "    {:<14} {:>3} parties  {:>5.1} % V  KDA {:>4.1}  ({:.0} % des parties)",
                c.champion, c.parties, c.winrate, c.kda, c.part
            ));
        }
    }
    if !v.roles.is_empty() {
        let r: Vec<String> = v.roles.iter().map(|r| format!("{} {:.0} %", role_fr(&r.role), r.part)).collect();
        t.push(format!("  Rôles : {}", r.join(" · ")));
    }
    if !v.etiquettes.is_empty() {
        t.push(format!("  Style : {}", v.etiquettes.join(" · ")));
    }
    if !v.forme.is_empty() {
        t.push(format!("  Forme : {} (série {:+})", v.forme, v.serie));
    }
    t
}

/// Fiche en texte Discord (markdown), prête à coller.
pub fn fiche_discord(v: &VueFiche) -> String {
    let mut t = format!("**Fiche stat-traque — {}**", v.nom);
    if let Some(r) = &v.rang {
        t += &format!(" · {r}");
    }
    t += "\n";
    if let Some(m) = &v.mmr {
        t += &format!("MMR estimé : **{}** ({:+} LP, confiance {})\n", m.rang_equivalent, m.ecart_lp, m.confiance);
    }
    let approx = if v.reference_approximative { ", approx." } else { "" };
    t += &format!("Radar vs {} même rôle ({} parties{approx}) :\n```\n", v.palier_reference, v.parties_analysees);
    for a in &v.radar {
        t += &ligne_radar(a);
        t += "\n";
    }
    t += "```\n";
    if !v.champions.is_empty() {
        t += &format!("Champions : {}\n", texte_champions(v));
    }
    if !v.roles.is_empty() {
        let r: Vec<String> = v.roles.iter().take(2).map(|r| format!("{} {:.0} %", role_fr(&r.role), r.part)).collect();
        t += &format!("Rôles : {}\n", r.join(" · "));
    }
    if !v.etiquettes.is_empty() {
        t += &format!("Style : {}\n", v.etiquettes.join(" · "));
    }
    if !v.forme.is_empty() {
        t += &format!("Forme : `{}` (série {:+})\n", v.forme, v.serie);
    }
    t += "_MMR et radar = estimations à partir des données officielles Riot._";
    t
}

/// Récap d'après-partie en texte Discord (markdown). `radar` : radar de CETTE partie
/// (comparée à mon palier), si on l'a.
pub fn recap_discord(r: &Recap, nom_champion: &str, radar: Option<&Radar>) -> String {
    let p = &r.partie;
    let resultat = if p.remake { "Remake" } else if p.victoire { "Victoire" } else { "Défaite" };
    let role = p.role.map(|r| format!(" ({})", role_fr(r))).unwrap_or_default();
    let lp = p.lp.map(|l| format!(" · **{l:+} LP**")).unwrap_or_default();
    let mut t = format!(
        "**{resultat}** — {nom_champion}{role} · {} · {} min{lp}\n**{}/{}/{}** · {:.1} CS/min · {} dégâts · {} vision\n",
        historique::nom_file(p.file),
        p.duree / 60,
        p.kills,
        p.morts,
        p.assists,
        p.cs_par_min(),
        p.degats,
        p.vision
    );
    let comparaisons: Vec<String> = r
        .lignes
        .iter()
        .filter_map(|l| l.ecart_pct.map(|e| format!("{} {e:+.0} %", l.libelle)))
        .collect();
    if !comparaisons.is_empty() {
        t += &format!("vs ma moyenne sur {nom_champion} : {}\n", comparaisons.join(" · "));
    }
    if let Some(radar) = radar.filter(|r| r.scores.iter().any(Option::is_some)) {
        t += "```\n";
        for (a, s) in AXES.iter().zip(radar.scores) {
            t += &ligne_radar(&VueAxeRadar { nom: a.nom().to_string(), score: s });
            t += "\n";
        }
        t += "```";
    }
    t.trim_end().to_string()
}

// =====================================================================
// 8. Commandes console : `stat-traque fiche [Pseudo#TAG]` et `stat-traque partager`
// =====================================================================

fn mon_compte() -> Option<String> {
    std::fs::read_to_string(historique::dossier().join("dernier-compte.txt")).ok().map(|s| s.trim().to_string())
}

fn patch_actuel() -> (Option<String>, Option<crate::ddragon::DonneesJeu>) {
    let d = crate::ddragon::charger();
    (d.as_ref().map(|d| crate::stats::patch_court(&d.version)), d)
}

/// `stat-traque fiche` (moi, 0 requête) ou `stat-traque fiche Pseudo#TAG`.
/// Taper un Riot ID = joueur connu de l'utilisateur (conformité : pas de recherche
/// automatique de pseudos cachés).
pub fn commande(riot_id: Option<&str>) {
    let (patch, donnees) = patch_actuel();
    let nom_champion = |c: u16| donnees.as_ref().map_or(c.to_string(), |d| d.nom_champion(c).to_string());
    let refs = references(patch.as_deref());
    if refs.est_vide() {
        println!("(Pas de références collectées pour ce patch : radar sur des valeurs par défaut.)");
    }
    let fiche = match riot_id {
        None => {
            let Some(puuid) = mon_compte() else {
                println!("Pas encore d'historique : lance l'app avec le client LoL ouvert (et une clé API).");
                return;
            };
            fiche_moi(&historique::dossier(), &puuid, &refs)
        }
        Some(id) => {
            let Some((pseudo, tag)) = id.rsplit_once('#') else {
                println!("Riot ID attendu sous la forme Pseudo#TAG.");
                return;
            };
            let api = RiotApi::depuis_reglages(&crate::stockage::Reglages::charger());
            let avant = Compteurs::du_fil();
            let puuid = match api.compte(pseudo, tag) {
                Ok(c) => c["puuid"].as_str().unwrap_or("").to_string(),
                Err(e) => {
                    println!("Joueur {id} : {e}");
                    return;
                }
            };
            let cout_compte = Compteurs::du_fil_depuis(avant);
            let mut f = fiche_autre(&api, &puuid, id, &refs);
            f.requetes = f.requetes.plus(cout_compte); // + la requête du Riot ID
            f
        }
    };
    let vue = vers_vue(&fiche, &nom_champion);
    for ligne in texte_console(&vue) {
        println!("{ligne}");
    }
    println!("Coût : {}.", fiche.requetes);
    println!("\n--- À coller sur Discord ---\n{}", vue.texte_partage);
}

/// `stat-traque partager` : récap de ma dernière partie en texte Discord (0 requête).
pub fn commande_partager() {
    let Some(puuid) = mon_compte() else {
        println!("Pas encore d'historique : lance l'app avec le client LoL ouvert (et une clé API).");
        return;
    };
    let h = Historique::charger(&historique::dossier(), &puuid);
    let Some(derniere) = h.parties.first() else {
        println!("Aucune partie dans l'historique.");
        return;
    };
    let (patch, donnees) = patch_actuel();
    let nom = donnees.as_ref().map_or(derniere.champion.to_string(), |d| d.nom_champion(derniere.champion).to_string());
    println!("{}", texte_partage_derniere(&h, &nom, &references(patch.as_deref())));
}

/// Texte Discord de ma dernière partie, avec son radar (séparé pour les tests).
pub fn texte_partage_derniere(h: &Historique, nom_champion: &str, refs: &References) -> String {
    let Some(p) = h.parties.first() else { return String::new() };
    let rang = h.progression(0).solo;
    let palier = rang.as_ref().and_then(|r| metriques::indice_palier(&r.palier)).unwrap_or(PALIER_NON_CLASSE);
    let r = radar(std::slice::from_ref(p), palier, refs);
    recap_discord(&Recap::calculer(h, p), nom_champion, Some(&r))
}

/// Patch court des données du jeu (pour trouver le fichier de stats).
pub fn patch_de(d: Option<&crate::ddragon::DonneesJeu>) -> Option<String> {
    d.map(|d| crate::stats::patch_court(&d.version))
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::metriques::tests::participant_complet;
    use serde_json::json;

    fn rang(palier: &str, division: &str, lp: u16, v: u16, d: u16) -> Rang {
        Rang { palier: palier.into(), division: division.into(), lp, victoires: v, defaites: d, serie: false }
    }

    /// Partie match-v5 construite à la main : moi (`p`) avec challenges.
    fn match_json(id: &str, debut: u64, champion: u16, victoire: bool, cs: u64, morts: u64) -> Value {
        let mut moi = participant_complet();
        moi["puuid"] = json!("moi");
        moi["championId"] = json!(champion);
        moi["win"] = json!(victoire);
        moi["totalMinionsKilled"] = json!(cs);
        moi["neutralMinionsKilled"] = json!(0);
        moi["deaths"] = json!(morts);
        json!({
            "metadata": {"matchId": id},
            "info": {"gameStartTimestamp": debut * 1000, "gameDuration": 1800, "queueId": 420,
                     "gameVersion": "16.23.1.1", "participants": [moi]}
        })
    }

    fn resume(id: &str, debut: u64, champion: u16, victoire: bool, cs: u64, morts: u64) -> ResumePartie {
        ResumePartie::depuis_match(&match_json(id, debut, champion, victoire, cs, morts), "moi").unwrap()
    }

    /// Références fictives : en Or (3), mid, chaque mesure a pour moyenne la valeur de
    /// la partie « moyenne » (cs 210, 2 morts) et un écart-type de 10 % de la moyenne.
    fn refs_or_mid() -> References {
        let p = resume("ref", 1, 103, true, 210, 2);
        let mut r = References::default();
        for (m, x) in metriques::MESURES.iter().zip(metriques::mesures(&p)) {
            let Some(x) = x else { continue };
            let sd = (x.abs() * 0.1).max(0.1) as f64;
            // 40 parties : 20 à x - sd, 20 à x + sd -> moyenne x, écart-type sd.
            let (a, b) = (x as f64 - sd, x as f64 + sd);
            let somme = 20.0 * (a + b);
            let carres = 20.0 * (a * a + b * b);
            let ligne = format!("R\tGOLD\tmiddle\t{}\t40\t{somme}\t{carres}", m.cle());
            r.lire_ligne(&ligne.split('\t').collect::<Vec<_>>());
        }
        r
    }

    #[test]
    fn radar_a_50_pour_un_joueur_moyen_et_monte_avec_le_farm() {
        let refs = refs_or_mid();
        let moyen: Vec<ResumePartie> = (0..5).map(|i| resume(&format!("A{i}"), i, 103, true, 210, 2)).collect();
        let r = radar(&moyen, 3, &refs);
        assert_eq!(r.parties, 5);
        assert_eq!(r.parties_detaillees, 5);
        assert!(!r.approximatif);
        assert!(r.scores.iter().all(|s| *s == Some(50)), "{:?}", r.scores);
        // Plus de CS (+2 écarts-types de CS/min) et 0 mort : Farm et Survie montent.
        let fort: Vec<ResumePartie> = (0..5).map(|i| resume(&format!("B{i}"), i, 103, true, 252, 0)).collect();
        let r = radar(&fort, 3, &refs);
        let farm = r.scores[1].unwrap();
        let survie = r.scores[5].unwrap();
        assert!(farm > 60 && farm <= 100, "farm {farm}");
        assert!(survie > 70, "survie {survie}");
        assert!(r.z_mesures[Mesure::CsMin as usize].unwrap() > 1.5);
        // Autre palier sans données (Diamant = 6, voisins 5 et 7 vides) : approximatif.
        assert!(radar(&fort, 6, &refs).approximatif);
        assert_eq!(barre(72), "▰▰▰▰▰▰▰▱▱▱");
        assert_eq!(barre(0), "▱▱▱▱▱▱▱▱▱▱");
        assert_eq!(score(5.0), 100);
    }

    #[test]
    fn etiquettes_de_style_et_one_trick() {
        let refs = refs_or_mid();
        let parties: Vec<ResumePartie> = (0..6).map(|i| resume(&format!("C{i}"), i, 103, i.is_multiple_of(2), 260, 5)).collect();
        let f = calculer("moi", "Moi", true, Some(rang("GOLD", "II", 50, 10, 10)), &parties, &refs);
        let e = &f.specificites.etiquettes;
        assert!(e.contains(&"One-trick".to_string()), "{e:?}");
        assert!(e.contains(&"Farmer".to_string()), "{e:?}");
        assert!(e.contains(&"Prend des risques".to_string()), "{e:?}");
        assert!(!e.contains(&"Solide".to_string()));
        assert_eq!(f.specificites.champions[0].1.parties, 6);
        assert_eq!(f.specificites.roles, vec![("middle", 6)]);
        assert_eq!(f.specificites.forme, "VDVDVD"); // la 1re de la liste = la plus récente
        assert_eq!(f.specificites.serie, 1);
    }

    #[test]
    fn mmr_moi_gains_superieurs_aux_pertes() {
        let mut parties = Vec::new();
        for i in 0..12u64 {
            let mut p = resume(&format!("L{i}"), 100 - i, 103, !i.is_multiple_of(3), 210, 2);
            p.lp = Some(if p.victoire { 25 } else { -15 });
            parties.push(p);
        }
        let r = rang("EMERALD", "III", 50, 30, 20);
        let m = mmr_moi(Some(&r), &parties).unwrap();
        // 12 × (25 - 15) = 120, + winrate 66,7 % : 3 × 16,7 × 12/22 ≈ 27 -> ~147, arrondi à 150.
        assert_eq!(m.ecart, 150);
        // Confiance plafonnée à « moyenne », même avec 12 parties aux LP connus.
        assert_eq!(m.confiance, Confiance::Moyenne);
        // Émeraude III 50 LP = 2150 LP absolus ; +150 -> 2300 = Émeraude I.
        assert_eq!(rang_equivalent(m.lp_absolus), "~Émeraude I");
        // Pertes > gains : en dessous.
        for p in &mut parties {
            p.lp = p.lp.map(|l| if l > 0 { 14 } else { -26 });
            p.victoire = false;
        }
        let m = mmr_moi(Some(&r), &parties).unwrap();
        assert!(m.ecart < -150, "{}", m.ecart);
        // Peu de parties avec LP : confiance faible ; non classé : pas d'estimation.
        assert_eq!(mmr_moi(Some(&r), &parties[..2]).unwrap().confiance, Confiance::Faible);
        assert!(mmr_moi(None, &parties).is_none());
    }

    #[test]
    fn mmr_autre_et_rangs_equivalents() {
        let r = rang("GOLD", "I", 90, 120, 80); // 60 % sur 200 parties
        let m = mmr_autre(Some(&r), &[]).unwrap();
        // 6 × 10 × 200/250 = 48 LP, arrondi à 50.
        assert_eq!(m.ecart, 50);
        assert_eq!(m.confiance, Confiance::Moyenne);
        assert_eq!(rang_equivalent(m.lp_absolus), "~Platine IV");
        assert_eq!(mmr_autre(Some(&rang("GOLD", "I", 0, 5, 5)), &[]).unwrap().confiance, Confiance::Faible);
        assert_eq!(rang_equivalent(2950), "~Master+ 150 LP");
        assert_eq!(rang_equivalent(-10), "~Fer IV");
        assert_eq!(rang_fr(&rang("EMERALD", "II", 45, 0, 0)), "Émeraude II 45 LP");
        assert_eq!(rang_fr(&rang("CHALLENGER", "I", 900, 0, 0)), "Challenger 900 LP");
    }

    /// Faux Riot pour la fiche d'un autre joueur (compte les appels, thread-safe).
    struct FausseSource {
        appels: AtomicUsize,
        cle_expiree: bool,
    }
    // Les parties sont lues par plusieurs fils : on compte avec un atomique.
    impl SourceFiche for FausseSource {
        fn rangs(&self, _: &str) -> Result<Value, ErreurApi> {
            self.appels.fetch_add(1, Ordering::SeqCst);
            if self.cle_expiree {
                return Err(ErreurApi::CleInvalide);
            }
            Ok(json!([{"queueType":"RANKED_SOLO_5x5","tier":"GOLD","rank":"II","leaguePoints":40,"wins":30,"losses":20}]))
        }
        fn ids_classees(&self, _: &str, nombre: u8) -> Result<Vec<String>, ErreurApi> {
            self.appels.fetch_add(1, Ordering::SeqCst);
            Ok((0..20).take(nombre as usize).map(|i| format!("EUW1_{i}")).collect())
        }
        fn partie(&self, id: &str) -> Result<Value, ErreurApi> {
            self.appels.fetch_add(1, Ordering::SeqCst);
            if id == "EUW1_3" {
                return Err(ErreurApi::Introuvable);
            }
            let i: u64 = id[5..].parse().unwrap();
            Ok(match_json(id, 1_000 - i, 103, i.is_multiple_of(2), 210, 2))
        }
    }

    #[test]
    fn fiche_d_un_autre_joueur_onze_requetes_au_plus() {
        let source = FausseSource { appels: AtomicUsize::new(0), cle_expiree: false };
        let c = charger_autre(&source, "moi", PARTIES_AUTRE);
        let (rang, parties) = (c.rang, c.parties);
        assert_eq!(source.appels.load(Ordering::SeqCst), 11);
        assert!(c.erreur.is_none());
        // Faux Riot : aucune VRAIE requête comptée pour cette fiche.
        assert_eq!(c.requetes, Compteurs::default());
        assert_eq!(rang.as_ref().unwrap().lp, 40);
        // 9 parties lues, une en 404 : 8, dans l'ordre (plus récentes d'abord).
        assert_eq!(parties.len(), 8);
        assert!(parties.windows(2).all(|w| w[0].date > w[1].date));
        let f = calculer("moi", "Joueur#EUW", false, rang, &parties, &refs_or_mid());
        let vue = vers_vue(&f, &|c| format!("champ{c}"));
        assert_eq!(vue.radar.len(), 7);
        assert_eq!(vue.rang.as_deref(), Some("Or II 40 LP"));
        assert_eq!(vue.champions[0].champion, "champ103");
        assert!(vue.mmr.is_some());
        assert!(vue.texte_partage.contains("**Fiche stat-traque — Joueur#EUW**"));
        assert!(vue.texte_partage.contains("▰"));
        assert!(texte_console(&vue).iter().any(|l| l.contains("MMR estimé")));
        // Clé expirée : on s'arrête dès la 1re requête, avec l'erreur.
        let source = FausseSource { appels: AtomicUsize::new(0), cle_expiree: true };
        let c = charger_autre(&source, "moi", PARTIES_AUTRE);
        assert_eq!(source.appels.load(Ordering::SeqCst), 1);
        assert!(c.parties.is_empty());
        assert_eq!(c.erreur, Some(ErreurApi::CleInvalide));
    }

    #[test]
    fn ma_fiche_porte_mon_riot_id() {
        let d = std::env::temp_dir().join(format!("stat-traque-fiche-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        assert_eq!(fiche_moi(&d, "moi", &References::default()).nom, "Moi");
        std::fs::write(d.join("riot-id-moi.txt"), "Zed King#EUW").unwrap();
        let f = fiche_moi(&d, "moi", &References::default());
        assert_eq!(f.nom, "Zed King#EUW");
        assert_eq!(f.requetes, Compteurs::default());
        assert!(vers_vue(&f, &|c| c.to_string()).texte_partage.contains("Zed King#EUW"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn recap_et_partage_discord() {
        let mut h = Historique::default();
        for i in 1..=3u64 {
            h.ajouter(resume(&format!("H{i}"), i, 103, true, 210, 2));
        }
        let mut derniere = resume("N", 10, 103, true, 252, 1);
        derniere.lp = Some(21);
        h.ajouter(derniere);
        let texte = texte_partage_derniere(&h, "Ahri", &refs_or_mid());
        assert!(texte.starts_with("**Victoire** — Ahri (Mid) · Classée solo/duo · 30 min · **+21 LP**"), "{texte}");
        assert!(texte.contains("vs ma moyenne sur Ahri : "), "{texte}");
        assert!(texte.contains("CS/min +20 %"), "{texte}");
        assert!(texte.contains("```\nCombat"), "{texte}");
        assert!(texte_partage_derniere(&Historique::default(), "x", &References::default()).is_empty());
    }

    #[test]
    fn autorisation_des_fiches() {
        autoriser(vec![("p1".into(), "Ami#EUW".into(), 103)]);
        assert_eq!(nom_autorise("p1").as_deref(), Some("Ami#EUW"));
        assert_eq!(nom_autorise("inconnu"), None);
        assert_eq!(crate::affichage::lire().cibles_fiche.len(), 1);
    }
}
