//! Métriques détaillées d'une partie (match-v5) et RÉFÉRENCES par palier et par rôle.
//!
//! Deux étages :
//!  1. `Metriques` : quelques nombres BRUTS lus dans le JSON match-v5 d'un joueur
//!     (champs du participant + bloc `challenges`, sans la timeline). Ce sont des
//!     entiers (les pourcentages sont stockés ×1000) : ils tiennent dans une ligne
//!     TSV de l'historique et se comparent exactement (tests).
//!  2. `Mesure` : des valeurs COMPARABLES entre parties, calculées à partir du résumé
//!     (par minute, en %...). Ce sont elles qu'on compare à la référence du palier.
//!
//! Les références (`References`) : pour chaque (palier, rôle, mesure), le nombre de
//! parties, la somme et la somme des carrés. De quoi calculer moyenne et écart-type
//! sans garder les parties, et cumuler collecte après collecte.
//! Lignes du fichier de stats (voir stats.rs) :
//!   R<TAB>palier<TAB>rôle<TAB>mesure<TAB>parties<TAB>somme<TAB>somme des carrés
//! Un ancien stat-traque les ignore (la 2e colonne n'est pas un numéro de champion).
//!
//! Noms des champs match-v5 utilisés (participant) : damageDealtToObjectives,
//! damageDealtToBuildings, turretTakedowns, wardsPlaced, wardsKilled,
//! detectorWardsPlaced, totalDamageTaken, damageSelfMitigated.
//! (challenges) : killParticipation, teamDamagePercentage, damageTakenOnTeamPercentage,
//! laneMinionsFirst10Minutes, jungleCsBefore10Minutes, maxCsAdvantageOnLaneOpponent,
//! maxLevelLeadLaneOpponent, earlyLaningPhaseGoldExpAdvantage,
//! laningPhaseGoldExpAdvantage, soloKills, turretPlatesTaken, dragonTakedowns,
//! baronTakedowns, riftHeraldTakedowns, controlWardsPlaced, wardTakedowns,
//! effectiveHealAndShielding, enemyChampionImmobilizations.
//! Un champ absent compte 0 (Riot omet parfois un challenge, ex. sans adversaire de lane).

use std::collections::HashMap;

use serde_json::Value;

use crate::historique::ResumePartie;

// =====================================================================
// 1. Métriques brutes d'une partie
// =====================================================================

/// D'où viennent les métriques d'une partie.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Source {
    /// Partie introuvable chez Riot (rattrapage impossible) : aucune métrique.
    #[default]
    Absentes,
    /// Champs du participant seulement (pas de bloc `challenges`).
    SansChallenges,
    /// Participant + challenges.
    Completes,
}

/// Métriques brutes d'un joueur dans une partie (voir l'en-tête du fichier).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Metriques {
    pub source: Source,
    /// killParticipation × 1000 (0,62 -> 620).
    pub kp: u16,
    /// teamDamagePercentage × 1000.
    pub part_degats: u16,
    /// damageTakenOnTeamPercentage × 1000.
    pub part_encaisses: u16,
    /// Sbires de lane + camps de jungle avant 10 minutes.
    pub cs_10: u16,
    /// maxCsAdvantageOnLaneOpponent × 10 (peut être négatif).
    pub avance_cs: i32,
    /// maxLevelLeadLaneOpponent.
    pub avance_niveau: i16,
    /// earlyLaningPhaseGoldExpAdvantage : 1 = +20 % d'or et d'XP sur l'adversaire à 7 min.
    pub avance_early: u8,
    /// laningPhaseGoldExpAdvantage : même chose à 14 min.
    pub avance_lane: u8,
    pub solo_kills: u16,
    /// turretPlatesTaken.
    pub plaques: u16,
    pub degats_objectifs: u32,
    pub degats_batiments: u32,
    /// turretTakedowns (tours détruites ou aidées).
    pub tours: u16,
    /// Dragons + barons + hérauts (takedowns).
    pub epiques: u16,
    /// wardsPlaced.
    pub balises: u16,
    /// controlWardsPlaced (repli : detectorWardsPlaced).
    pub pinks: u16,
    /// wardTakedowns (repli : wardsKilled).
    pub balises_detruites: u16,
    pub degats_subis: u32,
    pub degats_attenues: u32,
    /// effectiveHealAndShielding (arrondi).
    pub soins_boucliers: u32,
    /// enemyChampionImmobilizations.
    pub immobilisations: u16,
}

/// Nombre de colonnes TSV écrites par `vers_colonnes`.
pub const NB_COLONNES: usize = 22;

impl Metriques {
    /// Lit les métriques d'un participant match-v5 (`p` = un élément de `participants`).
    pub fn depuis_participant(p: &Value) -> Self {
        let c = &p["challenges"];
        // Petites fonctions de lecture : un champ absent ou bizarre vaut 0.
        let f = |v: &Value, k: &str| v[k].as_f64().unwrap_or(0.0);
        let u16_ = |x: f64| x.round().clamp(0.0, u16::MAX as f64) as u16;
        let u32_ = |x: f64| x.round().clamp(0.0, u32::MAX as f64) as u32;
        let mille = |k: &str| u16_(f(c, k) * 1000.0);
        let defi_ou = |defi: &str, repli: &str| if c[defi].is_number() { f(c, defi) } else { f(p, repli) };
        Metriques {
            source: if c.is_object() { Source::Completes } else { Source::SansChallenges },
            kp: mille("killParticipation"),
            part_degats: mille("teamDamagePercentage"),
            part_encaisses: mille("damageTakenOnTeamPercentage"),
            cs_10: u16_(f(c, "laneMinionsFirst10Minutes") + f(c, "jungleCsBefore10Minutes")),
            avance_cs: (f(c, "maxCsAdvantageOnLaneOpponent") * 10.0).round() as i32,
            avance_niveau: f(c, "maxLevelLeadLaneOpponent").round().clamp(-30.0, 30.0) as i16,
            avance_early: (f(c, "earlyLaningPhaseGoldExpAdvantage") > 0.0) as u8,
            avance_lane: (f(c, "laningPhaseGoldExpAdvantage") > 0.0) as u8,
            solo_kills: u16_(f(c, "soloKills")),
            plaques: u16_(f(c, "turretPlatesTaken")),
            degats_objectifs: u32_(f(p, "damageDealtToObjectives")),
            degats_batiments: u32_(f(p, "damageDealtToBuildings")),
            tours: u16_(f(p, "turretTakedowns")),
            epiques: u16_(f(c, "dragonTakedowns") + f(c, "baronTakedowns") + f(c, "riftHeraldTakedowns")),
            balises: u16_(f(p, "wardsPlaced")),
            pinks: u16_(defi_ou("controlWardsPlaced", "detectorWardsPlaced")),
            balises_detruites: u16_(defi_ou("wardTakedowns", "wardsKilled")),
            degats_subis: u32_(f(p, "totalDamageTaken")),
            degats_attenues: u32_(f(p, "damageSelfMitigated")),
            soins_boucliers: u32_(f(c, "effectiveHealAndShielding")),
            immobilisations: u16_(f(c, "enemyChampionImmobilizations")),
        }
    }

    /// Métriques « introuvables » (la partie n'existe plus chez Riot).
    pub fn absentes() -> Self {
        Metriques::default()
    }

    /// Les colonnes TSV (toujours `NB_COLONNES`, dans cet ordre).
    pub fn vers_colonnes(&self) -> String {
        let source = match self.source {
            Source::Absentes => 0,
            Source::SansChallenges => 1,
            Source::Completes => 2,
        };
        let v: [String; NB_COLONNES] = [
            source.to_string(),
            self.kp.to_string(),
            self.part_degats.to_string(),
            self.part_encaisses.to_string(),
            self.cs_10.to_string(),
            self.avance_cs.to_string(),
            self.avance_niveau.to_string(),
            self.avance_early.to_string(),
            self.avance_lane.to_string(),
            self.solo_kills.to_string(),
            self.plaques.to_string(),
            self.degats_objectifs.to_string(),
            self.degats_batiments.to_string(),
            self.tours.to_string(),
            self.epiques.to_string(),
            self.balises.to_string(),
            self.pinks.to_string(),
            self.balises_detruites.to_string(),
            self.degats_subis.to_string(),
            self.degats_attenues.to_string(),
            self.soins_boucliers.to_string(),
            self.immobilisations.to_string(),
        ];
        v.join("\t")
    }

    /// Relit les colonnes écrites par `vers_colonnes` (None si abîmées).
    pub fn depuis_colonnes(c: &[&str]) -> Option<Self> {
        if c.len() < NB_COLONNES {
            return None;
        }
        // `p(i)` : la colonne i convertie dans le type attendu (déduit par Rust).
        fn p<T: std::str::FromStr>(c: &[&str], i: usize) -> Option<T> {
            c[i].trim().parse().ok()
        }
        Some(Metriques {
            source: match p::<u8>(c, 0)? {
                0 => Source::Absentes,
                1 => Source::SansChallenges,
                _ => Source::Completes,
            },
            kp: p(c, 1)?,
            part_degats: p(c, 2)?,
            part_encaisses: p(c, 3)?,
            cs_10: p(c, 4)?,
            avance_cs: p(c, 5)?,
            avance_niveau: p(c, 6)?,
            avance_early: p(c, 7)?,
            avance_lane: p(c, 8)?,
            solo_kills: p(c, 9)?,
            plaques: p(c, 10)?,
            degats_objectifs: p(c, 11)?,
            degats_batiments: p(c, 12)?,
            tours: p(c, 13)?,
            epiques: p(c, 14)?,
            balises: p(c, 15)?,
            pinks: p(c, 16)?,
            balises_detruites: p(c, 17)?,
            degats_subis: p(c, 18)?,
            degats_attenues: p(c, 19)?,
            soins_boucliers: p(c, 20)?,
            immobilisations: p(c, 21)?,
        })
    }
}

// =====================================================================
// 2. Mesures comparables
// =====================================================================

/// Une mesure comparable entre parties (voir `mesures`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Mesure {
    /// Participation aux kills, en %.
    Kp,
    /// Part des dégâts de l'équipe aux champions, en %.
    PartDegats,
    DegatsMin,
    OrMin,
    CsMin,
    /// CS à 10 minutes (sbires + camps).
    Cs10,
    VisionMin,
    BalisesMin,
    /// Balises de contrôle posées (par partie).
    Pinks,
    BalisesDetruites,
    ObjectifsMin,
    BatimentsMin,
    /// Dragons, barons, hérauts (participations).
    Epiques,
    Tours,
    Plaques,
    /// Plus grande avance de CS sur l'adversaire de lane.
    AvanceCs,
    /// % de parties avec +20 % d'or et d'XP sur l'adversaire à 7 min.
    AvanceEarly,
    AvanceNiveau,
    SoloKills,
    Kills10,
    Morts10,
    /// Dégâts subis + atténués par minute.
    EncaissesMin,
    /// Dégâts subis + atténués par mort (survivre en encaissant).
    EncaissesParMort,
    SoinsMin,
    Immobilisations,
    /// Part des dégâts subis par l'équipe, en %.
    PartEncaisses,
}

pub const NB_MESURES: usize = 26;

/// Toutes les mesures, dans l'ordre (l'indice = `Mesure as usize`).
pub const MESURES: [Mesure; NB_MESURES] = [
    Mesure::Kp,
    Mesure::PartDegats,
    Mesure::DegatsMin,
    Mesure::OrMin,
    Mesure::CsMin,
    Mesure::Cs10,
    Mesure::VisionMin,
    Mesure::BalisesMin,
    Mesure::Pinks,
    Mesure::BalisesDetruites,
    Mesure::ObjectifsMin,
    Mesure::BatimentsMin,
    Mesure::Epiques,
    Mesure::Tours,
    Mesure::Plaques,
    Mesure::AvanceCs,
    Mesure::AvanceEarly,
    Mesure::AvanceNiveau,
    Mesure::SoloKills,
    Mesure::Kills10,
    Mesure::Morts10,
    Mesure::EncaissesMin,
    Mesure::EncaissesParMort,
    Mesure::SoinsMin,
    Mesure::Immobilisations,
    Mesure::PartEncaisses,
];

impl Mesure {
    /// Nom court dans le fichier de stats (ne JAMAIS changer : fichiers publiés).
    pub fn cle(self) -> &'static str {
        match self {
            Mesure::Kp => "kp",
            Mesure::PartDegats => "part_degats",
            Mesure::DegatsMin => "degats_min",
            Mesure::OrMin => "or_min",
            Mesure::CsMin => "cs_min",
            Mesure::Cs10 => "cs10",
            Mesure::VisionMin => "vision_min",
            Mesure::BalisesMin => "balises_min",
            Mesure::Pinks => "pinks",
            Mesure::BalisesDetruites => "balises_detruites",
            Mesure::ObjectifsMin => "objectifs_min",
            Mesure::BatimentsMin => "batiments_min",
            Mesure::Epiques => "epiques",
            Mesure::Tours => "tours",
            Mesure::Plaques => "plaques",
            Mesure::AvanceCs => "avance_cs",
            Mesure::AvanceEarly => "avance_early",
            Mesure::AvanceNiveau => "avance_niveau",
            Mesure::SoloKills => "solo_kills",
            Mesure::Kills10 => "kills10",
            Mesure::Morts10 => "morts10",
            Mesure::EncaissesMin => "encaisses_min",
            Mesure::EncaissesParMort => "encaisses_par_mort",
            Mesure::SoinsMin => "soins_min",
            Mesure::Immobilisations => "immobilisations",
            Mesure::PartEncaisses => "part_encaisses",
        }
    }

    pub fn depuis_cle(cle: &str) -> Option<Mesure> {
        MESURES.iter().copied().find(|m| m.cle() == cle)
    }
}

/// Les mesures d'une partie (None = inconnue pour cette partie).
/// Les mesures « de base » (dégâts, or, CS, vision, kills, morts par minute) existent
/// pour toute partie ; les autres demandent les métriques détaillées.
pub fn mesures(p: &ResumePartie) -> [Option<f32>; NB_MESURES] {
    let mut v = [None; NB_MESURES];
    let minutes = p.minutes();
    if minutes < 1.0 {
        return v;
    }
    let par_min = |x: f64| Some((x / minutes as f64) as f32);
    v[Mesure::DegatsMin as usize] = par_min(p.degats as f64);
    v[Mesure::OrMin as usize] = par_min(p.or as f64);
    v[Mesure::CsMin as usize] = par_min(p.cs as f64);
    v[Mesure::VisionMin as usize] = par_min(p.vision as f64);
    v[Mesure::Kills10 as usize] = par_min(p.kills as f64 * 10.0);
    v[Mesure::Morts10 as usize] = par_min(p.morts as f64 * 10.0);
    let Some(m) = p.metriques.filter(|m| m.source != Source::Absentes) else { return v };
    let encaisses = m.degats_subis as f64 + m.degats_attenues as f64;
    v[Mesure::BalisesMin as usize] = par_min(m.balises as f64);
    v[Mesure::Pinks as usize] = Some(m.pinks as f32);
    v[Mesure::BalisesDetruites as usize] = Some(m.balises_detruites as f32);
    v[Mesure::ObjectifsMin as usize] = par_min(m.degats_objectifs as f64);
    v[Mesure::BatimentsMin as usize] = par_min(m.degats_batiments as f64);
    v[Mesure::Tours as usize] = Some(m.tours as f32);
    v[Mesure::EncaissesMin as usize] = par_min(encaisses);
    v[Mesure::EncaissesParMort as usize] = Some((encaisses / p.morts.max(1) as f64) as f32);
    if m.source == Source::Completes {
        v[Mesure::Kp as usize] = Some(m.kp as f32 / 10.0);
        v[Mesure::PartDegats as usize] = Some(m.part_degats as f32 / 10.0);
        v[Mesure::PartEncaisses as usize] = Some(m.part_encaisses as f32 / 10.0);
        v[Mesure::Cs10 as usize] = Some(m.cs_10 as f32);
        v[Mesure::Epiques as usize] = Some(m.epiques as f32);
        v[Mesure::Plaques as usize] = Some(m.plaques as f32);
        v[Mesure::AvanceCs as usize] = Some(m.avance_cs as f32 / 10.0);
        v[Mesure::AvanceEarly as usize] = Some(m.avance_early as f32 * 100.0);
        v[Mesure::AvanceNiveau as usize] = Some(m.avance_niveau as f32);
        v[Mesure::SoloKills as usize] = Some(m.solo_kills as f32);
        v[Mesure::SoinsMin as usize] = par_min(m.soins_boucliers as f64);
        v[Mesure::Immobilisations as usize] = Some(m.immobilisations as f32);
    }
    v
}

// =====================================================================
// 3. Paliers de référence
// =====================================================================

/// Paliers des références : Master, Grandmaster et Challenger sont regroupés en
/// « MASTER » (peu de joueurs, et ils jouent entre eux).
pub const PALIERS_REFERENCE: [&str; 8] = ["IRON", "BRONZE", "SILVER", "GOLD", "PLATINUM", "EMERALD", "DIAMOND", "MASTER"];

/// Indice du palier de référence d'un palier league-v4 ("GOLD" -> 3, "CHALLENGER" -> 7).
pub fn indice_palier(palier: &str) -> Option<usize> {
    match palier.to_ascii_uppercase().as_str() {
        "GRANDMASTER" | "CHALLENGER" => Some(7),
        p => PALIERS_REFERENCE.iter().position(|x| *x == p),
    }
}

/// Nom français d'un palier de référence.
pub fn nom_palier(indice: usize) -> &'static str {
    ["Fer", "Bronze", "Argent", "Or", "Platine", "Émeraude", "Diamant", "Master+"].get(indice).copied().unwrap_or("?")
}

// =====================================================================
// 4. Références : moyenne et écart-type par (palier, rôle, mesure)
// =====================================================================

/// Sommes d'une mesure : de quoi calculer moyenne et écart-type, et cumuler.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Somme {
    pub parties: u32,
    pub somme: f64,
    pub carres: f64,
}

impl Somme {
    pub fn ajouter(&mut self, x: f32) {
        let x = x as f64;
        self.parties += 1;
        self.somme += x;
        self.carres += x * x;
    }
    fn fusionner(&mut self, autre: &Somme) {
        self.parties += autre.parties;
        self.somme += autre.somme;
        self.carres += autre.carres;
    }
    pub fn moyenne(&self) -> f32 {
        if self.parties == 0 { 0.0 } else { (self.somme / self.parties as f64) as f32 }
    }
    /// Écart-type (de la population) : racine de (moyenne des carrés - carré de la moyenne).
    pub fn ecart_type(&self) -> f32 {
        if self.parties == 0 {
            return 0.0;
        }
        let n = self.parties as f64;
        let m = self.somme / n;
        ((self.carres / n - m * m).max(0.0)).sqrt() as f32
    }
}

/// En dessous, une référence collectée n'est pas fiable : on passe au repli.
pub const MIN_PARTIES_REFERENCE: u32 = 30;

/// Clé : (indice de palier, rôle, mesure). Le rôle pointe dans `stats::ROLES`.
type CleRef = (u8, &'static str, Mesure);

#[derive(Debug, Clone, Default, PartialEq)]
pub struct References {
    table: HashMap<CleRef, Somme>,
}

/// Une référence utilisable : moyenne, écart-type, et si elle est approximative.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reference {
    pub moyenne: f32,
    pub ecart_type: f32,
    /// true = palier voisin ou valeur par défaut codée en dur.
    pub approximative: bool,
}

impl References {
    pub fn est_vide(&self) -> bool {
        self.table.is_empty()
    }

    /// Ajoute les mesures d'un joueur dans une partie (rôle connu seulement).
    pub fn ajouter(&mut self, palier: usize, resume: &ResumePartie) {
        let Some(role) = resume.role else { return };
        for (m, x) in MESURES.iter().zip(mesures(resume)) {
            if let Some(x) = x.filter(|x| x.is_finite()) {
                self.table.entry((palier as u8, role, *m)).or_default().ajouter(x);
            }
        }
    }

    /// Lit une ligne `R` (colonnes déjà séparées). Les lignes inconnues sont ignorées.
    pub fn lire_ligne(&mut self, c: &[&str]) {
        if c.len() != 7 || c[0] != "R" {
            return;
        }
        let palier = indice_palier(c[1]);
        let role = crate::stats::ROLES.iter().copied().find(|r| *r == c[2]);
        let mesure = Mesure::depuis_cle(c[3]);
        let (Some(palier), Some(role), Some(mesure)) = (palier, role, mesure) else { return };
        let (Ok(parties), Ok(somme), Ok(carres)) = (c[4].parse(), c[5].parse(), c[6].parse()) else { return };
        self.table.entry((palier as u8, role, mesure)).or_default().fusionner(&Somme { parties, somme, carres });
    }

    /// Lignes `R`, triées (fichier stable). `min_parties` : on n'écrit pas les plus rares.
    pub fn vers_lignes(&self, min_parties: u32) -> Vec<String> {
        let mut v: Vec<(&CleRef, &Somme)> = self.table.iter().filter(|(_, s)| s.parties >= min_parties).collect();
        v.sort_by(|a, b| a.0.cmp(b.0));
        v.into_iter()
            .map(|((p, r, m), s)| {
                format!("R\t{}\t{r}\t{}\t{}\t{:.3}\t{:.3}", PALIERS_REFERENCE[*p as usize], m.cle(), s.parties, s.somme, s.carres)
            })
            .collect()
    }

    /// Somme brute (tests, diagnostics).
    pub fn somme(&self, palier: usize, role: &str, mesure: Mesure) -> Option<&Somme> {
        let role = crate::stats::ROLES.iter().copied().find(|r| *r == role)?;
        self.table.get(&(palier as u8, role, mesure))
    }

    /// La référence d'une mesure : le palier demandé s'il a assez de parties, sinon
    /// le palier voisin le plus proche (en dessous puis au-dessus), sinon la valeur
    /// par défaut du rôle (approximative).
    pub fn reference(&self, palier: usize, role: &str, mesure: Mesure) -> Reference {
        let essais = [palier as i32, palier as i32 - 1, palier as i32 + 1];
        for (i, p) in essais.iter().enumerate() {
            if *p < 0 || *p as usize >= PALIERS_REFERENCE.len() {
                continue;
            }
            if let Some(s) = self.somme(*p as usize, role, mesure).filter(|s| s.parties >= MIN_PARTIES_REFERENCE) {
                return Reference { moyenne: s.moyenne(), ecart_type: s.ecart_type(), approximative: i > 0 };
            }
        }
        let (moyenne, ecart_type) = reference_par_defaut(role, mesure);
        Reference { moyenne, ecart_type, approximative: true }
    }
}

/// Valeurs PAR DÉFAUT (moyenne, écart-type) par rôle, quand aucune référence
/// collectée n'existe. Ordres de grandeur d'une partie classée Or-Émeraude (~30 min),
/// estimés à la main d'après les statistiques publiques habituelles : c'est
/// APPROXIMATIF, et l'affichage le dit (« référence approximative »).
/// Ordre des colonnes : top, jungle, middle, bottom, utility.
fn reference_par_defaut(role: &str, mesure: Mesure) -> (f32, f32) {
    let i = crate::stats::ROLES.iter().position(|r| *r == role).unwrap_or(2);
    let t: [(f32, f32); 5] = match mesure {
        Mesure::Kp => [(45.0, 12.0), (60.0, 12.0), (55.0, 12.0), (55.0, 12.0), (62.0, 12.0)],
        Mesure::PartDegats => [(23.0, 7.0), (18.0, 6.0), (26.0, 7.0), (27.0, 7.0), (10.0, 5.0)],
        Mesure::DegatsMin => [(750.0, 250.0), (600.0, 200.0), (850.0, 280.0), (850.0, 280.0), (350.0, 150.0)],
        Mesure::OrMin => [(400.0, 60.0), (380.0, 60.0), (420.0, 60.0), (430.0, 60.0), (250.0, 40.0)],
        Mesure::CsMin => [(7.0, 1.2), (6.0, 1.0), (7.5, 1.2), (7.8, 1.2), (1.0, 0.6)],
        Mesure::Cs10 => [(65.0, 12.0), (55.0, 10.0), (70.0, 12.0), (70.0, 12.0), (8.0, 6.0)],
        Mesure::VisionMin => [(0.7, 0.25), (1.1, 0.3), (0.8, 0.25), (0.8, 0.25), (2.2, 0.5)],
        Mesure::BalisesMin => [(0.35, 0.15), (0.45, 0.15), (0.35, 0.15), (0.4, 0.15), (0.9, 0.25)],
        Mesure::Pinks => [(2.0, 1.5), (4.0, 2.0), (2.5, 1.5), (2.0, 1.5), (6.0, 3.0)],
        Mesure::BalisesDetruites => [(3.0, 2.0), (6.0, 3.0), (3.0, 2.0), (3.0, 2.0), (7.0, 4.0)],
        Mesure::ObjectifsMin => [(450.0, 250.0), (1100.0, 350.0), (250.0, 150.0), (400.0, 200.0), (120.0, 80.0)],
        Mesure::BatimentsMin => [(180.0, 130.0), (80.0, 60.0), (120.0, 90.0), (180.0, 120.0), (50.0, 40.0)],
        Mesure::Epiques => [(1.0, 1.0), (4.0, 2.0), (1.5, 1.2), (2.0, 1.4), (2.0, 1.4)],
        Mesure::Tours => [(2.0, 1.5), (1.5, 1.3), (1.8, 1.4), (2.5, 1.6), (1.8, 1.4)],
        Mesure::Plaques => [(1.5, 1.5), (0.4, 0.8), (1.0, 1.2), (1.2, 1.3), (0.6, 0.9)],
        Mesure::AvanceCs => [(15.0, 15.0), (15.0, 15.0), (15.0, 15.0), (15.0, 15.0), (5.0, 8.0)],
        Mesure::AvanceEarly => [(20.0, 40.0); 5],
        Mesure::AvanceNiveau => [(1.0, 1.0); 5],
        Mesure::SoloKills => [(1.2, 1.3), (0.6, 0.9), (1.0, 1.2), (0.7, 1.0), (0.2, 0.5)],
        Mesure::Kills10 => [(1.8, 1.1), (2.0, 1.1), (2.2, 1.2), (2.5, 1.3), (0.6, 0.5)],
        Mesure::Morts10 => [(1.8, 0.8), (1.8, 0.8), (1.8, 0.8), (1.8, 0.8), (2.0, 0.8)],
        Mesure::EncaissesMin => [(1400.0, 450.0), (1300.0, 400.0), (800.0, 300.0), (700.0, 250.0), (750.0, 350.0)],
        Mesure::EncaissesParMort => [(8000.0, 5000.0), (7000.0, 4000.0), (5000.0, 3000.0), (4500.0, 2500.0), (4500.0, 3000.0)],
        Mesure::SoinsMin => [(30.0, 40.0), (30.0, 40.0), (30.0, 40.0), (40.0, 40.0), (300.0, 250.0)],
        Mesure::Immobilisations => [(15.0, 10.0), (20.0, 12.0), (20.0, 12.0), (12.0, 8.0), (35.0, 18.0)],
        Mesure::PartEncaisses => [(24.0, 7.0), (24.0, 7.0), (16.0, 6.0), (14.0, 5.0), (16.0, 6.0)],
    };
    t[i]
}

/// Charge les références depuis un fichier de stats EN FLUX : seules les lignes `R`
/// sont gardées (au plus 8 paliers × 5 rôles × 26 mesures ≈ 1 000 entrées, ~50 Ko).
pub fn charger_references(chemin: &std::path::Path) -> Option<References> {
    let mut r = References::default();
    crate::stats::lire_lignes(chemin, |l| {
        if l.starts_with("R\t") {
            let c: Vec<&str> = l.split('\t').collect();
            r.lire_ligne(&c);
        }
    })?;
    Some(r)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use serde_json::json;

    /// Participant match-v5 construit à la main, avec un bloc challenges.
    pub fn participant_complet() -> Value {
        json!({
            "puuid": "p", "championId": 103, "teamPosition": "MIDDLE", "win": true,
            "kills": 8, "deaths": 2, "assists": 10, "totalMinionsKilled": 200, "neutralMinionsKilled": 10,
            "totalDamageDealtToChampions": 27000, "visionScore": 30, "goldEarned": 12600,
            "damageDealtToObjectives": 9000, "damageDealtToBuildings": 4500, "turretTakedowns": 3,
            "wardsPlaced": 12, "wardsKilled": 4, "detectorWardsPlaced": 3,
            "totalDamageTaken": 18000, "damageSelfMitigated": 9000,
            "challenges": {
                "killParticipation": 0.6234, "teamDamagePercentage": 0.31, "damageTakenOnTeamPercentage": 0.15,
                "laneMinionsFirst10Minutes": 78, "jungleCsBefore10Minutes": 4.0,
                "maxCsAdvantageOnLaneOpponent": 22.5, "maxLevelLeadLaneOpponent": 2,
                "earlyLaningPhaseGoldExpAdvantage": 1, "laningPhaseGoldExpAdvantage": 0,
                "soloKills": 3, "turretPlatesTaken": 2, "dragonTakedowns": 2, "baronTakedowns": 1,
                "riftHeraldTakedowns": 0, "controlWardsPlaced": 5, "wardTakedowns": 6,
                "effectiveHealAndShielding": 1234.6, "enemyChampionImmobilizations": 25
            }
        })
    }

    #[test]
    fn lit_les_challenges_et_tolere_les_absents() {
        let m = Metriques::depuis_participant(&participant_complet());
        assert_eq!(m.source, Source::Completes);
        assert_eq!((m.kp, m.part_degats, m.part_encaisses), (623, 310, 150));
        assert_eq!((m.cs_10, m.avance_cs, m.avance_niveau), (82, 225, 2));
        assert_eq!((m.avance_early, m.avance_lane, m.solo_kills, m.plaques), (1, 0, 3, 2));
        assert_eq!((m.epiques, m.pinks, m.balises_detruites, m.soins_boucliers), (3, 5, 6, 1235));
        // Sans challenges : replis sur les champs du participant, le reste à 0.
        let mut p = participant_complet();
        p.as_object_mut().unwrap().remove("challenges");
        let m = Metriques::depuis_participant(&p);
        assert_eq!(m.source, Source::SansChallenges);
        assert_eq!((m.pinks, m.balises_detruites, m.kp, m.tours), (3, 4, 0, 3));
    }

    #[test]
    fn colonnes_aller_retour() {
        let m = Metriques::depuis_participant(&participant_complet());
        let texte = m.vers_colonnes();
        let c: Vec<&str> = texte.split('\t').collect();
        assert_eq!(c.len(), NB_COLONNES);
        assert_eq!(Metriques::depuis_colonnes(&c), Some(m));
        assert_eq!(Metriques::depuis_colonnes(&c[..5]), None);
    }

    #[test]
    fn references_somme_ecart_type_et_repli() {
        let mut s = Somme::default();
        for x in [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0] {
            s.ajouter(x);
        }
        assert_eq!(s.moyenne(), 5.0);
        assert!((s.ecart_type() - 2.0).abs() < 1e-5);
        let mut r = References::default();
        // 30 parties en Or (3), mid : assez pour l'Or, et sert de repli au Platine (4).
        for _ in 0..30 {
            r.lire_ligne(&["R", "GOLD", "middle", "cs_min", "1", "7", "49"]);
        }
        let or = r.reference(3, "middle", Mesure::CsMin);
        assert_eq!((or.moyenne, or.ecart_type, or.approximative), (7.0, 0.0, false));
        assert!(r.reference(4, "middle", Mesure::CsMin).approximative);
        assert_eq!(r.reference(4, "middle", Mesure::CsMin).moyenne, 7.0);
        // Diamant (6) : ni lui ni ses voisins -> valeur par défaut.
        let d = r.reference(6, "utility", Mesure::CsMin);
        assert_eq!((d.moyenne, d.approximative), (1.0, true));
        // Aller-retour texte.
        let lignes = r.vers_lignes(1);
        assert_eq!(lignes, vec!["R\tGOLD\tmiddle\tcs_min\t30\t210.000\t1470.000"]);
        let mut relu = References::default();
        let c: Vec<&str> = lignes[0].split('\t').collect();
        relu.lire_ligne(&c);
        assert_eq!(relu, r);
        assert_eq!(indice_palier("CHALLENGER"), Some(7));
        assert_eq!(indice_palier("bronze"), Some(1));
    }
}
