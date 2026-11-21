//! Historique et stats perso : MES parties, mes moyennes, mes LP, le récap d'après-partie.
//!
//! Base locale (dans `%APPDATA%\stat-traque\historique\`), deux petits fichiers texte
//! par compte (TSV = colonnes séparées par des tabulations, lisible dans un tableur) :
//!   parties-<puuid>.tsv  une ligne par partie (résumé de ~200 octets, PAS le JSON
//!                        match-v5 complet qui pèse 30 à 60 Ko). Format v2 : les
//!                        métriques détaillées de la fiche joueur (metriques.rs) en
//!                        fin de ligne ; une ligne v1 (sans elles) reste lisible.
//!   Rattrapage : une partie v1 parmi les 20 plus récentes de la Faille est
//!   re-téléchargée UNE fois pour ses métriques (≤ 20 requêtes, une seule fois).
//!   lp-<puuid>.tsv       une ligne à chaque fois que mon rang solo/flex a changé
//!
//! Synchronisation en arrière-plan (thread à part, jamais sur le cerveau) :
//!  - quand le client donne mon puuid (démarrage) ;
//!  - après chaque partie (phase EndOfGame), avec récap comparé à MA moyenne.
//!
//! On ne télécharge QUE les parties nouvelles (`startTime` = date de la dernière
//! partie connue), 100 au plus par synchro. Coût d'une synchro :
//!   1 requête (liste des ids) + 1 par partie nouvelle + 1 (rang league-v4).
//! Après une partie : 3 requêtes. Premier lancement : jusqu'à 102 requêtes, étalées
//! (une partie toutes les 2,5 s, soit ~48 requêtes / 2 min) pour laisser à la recherche
//! des rangs en partie (~31 requêtes) sa place dans la limite de 100 requêtes / 2 min.
//!
//! LIMITE CONNUE (pas de pagination) : une synchro ne lit qu'UNE page de match-v5,
//! soit les 100 parties les plus récentes. Au premier lancement, les parties plus
//! anciennes ne sont donc jamais récupérées ; et si plus de 100 parties ont été jouées
//! sans lancer l'app, les plus anciennes de ce lot manquent (trou définitif, car la
//! synchro suivante repart de la plus récente). Choix volontaire : remonter plus loin
//! coûterait 1 requête par partie, au débit de la clé dev (~45 parties / min).
//!
//! Familles de files : les moyennes ne mélangent pas la Faille, l'ARAM et l'Arena
//! (un CS/min d'ARAM n'a rien à voir avec un CS/min de Faille). Voir `famille`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::serde_statique::Texte;
use crate::evenement::Evenement;
use crate::metriques::Metriques;
use crate::rangs::Rang;
use crate::riot_api::{Compteurs, ErreurApi, RiotApi};
use crate::{affichage, log};

/// Parties lues au plus par synchro (1 page match-v5, maximum autorisé par Riot) :
/// au premier lancement, on ne remonte pas plus loin (voir LIMITE CONNUE en tête).
const PREMIERE_SYNCHRO: u8 = 100;
/// Pause entre deux parties téléchargées (voir l'en-tête : partage du débit de la clé).
const PAUSE_ENTRE_PARTIES: Duration = Duration::from_millis(2500);
/// Parties gardées pour l'affichage (le fichier, lui, garde tout).
const PARTIES_AFFICHEES: usize = 20;
/// En dessous, « ta moyenne » ne veut pas dire grand-chose : on ne compare pas.
const MIN_PARTIES_MOYENNE: u32 = 3;
/// Points de LP gardés pour le graphique, par file.
const POINTS_PAR_FILE: usize = 30;
/// v2 : métriques détaillées en fin de ligne (une ligne v1 reste lisible).
const ENTETE_PARTIES: &str = "#stat-traque historique v2";
/// Parties anciennes (sans métriques) re-téléchargées au plus par synchro, pour que
/// la fiche joueur ait de quoi calculer le radar sans attendre 20 nouvelles parties.
const RATTRAPAGE_MAX: usize = 20;
const ENTETE_LP: &str = "#stat-traque lp v1";

fn maintenant() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Nom lisible d'une file (queueId de match-v5).
pub fn nom_file(file: u16) -> &'static str {
    match file {
        420 => "Classée solo/duo",
        440 => "Classée flex",
        400 => "Normale draft",
        430 => "Normale aveugle",
        490 => "Partie rapide",
        450 => "ARAM",
        700 | 720 => "Clash",
        900 | 1900 => "URF",
        1700 | 1710 => "Arena",
        0 => "Personnalisée",
        _ => "Autre mode",
    }
}

/// Famille de jeu : on ne compare que des parties de la même famille.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum Famille {
    /// Faille de l'invocateur : normales, classées, partie rapide, Clash.
    Faille,
    /// ARAM (et Clash ARAM).
    Aram,
    Arena,
    /// URF, modes temporaires, personnalisées...
    Autre,
}

impl Famille {
    pub fn libelle(self) -> &'static str {
        match self {
            Famille::Faille => "Faille de l'invocateur",
            Famille::Aram => "ARAM",
            Famille::Arena => "Arena",
            Famille::Autre => "Autres modes",
        }
    }
}

/// Famille d'une file (queueId).
pub fn famille(file: u16) -> Famille {
    match file {
        400 | 420 | 430 | 440 | 490 | 700 => Famille::Faille,
        450 | 720 => Famille::Aram,
        1700 | 1710 => Famille::Arena,
        _ => Famille::Autre,
    }
}

// =====================================================================
// 1. Résumé d'une partie
// =====================================================================

/// Ce qu'on garde d'une partie : quelques nombres, pas les 60 Ko de JSON.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ResumePartie {
    /// Identifiant match-v5 (ex : "EUW1_7234567890").
    pub id: Box<str>,
    /// Début de la partie, en secondes Unix.
    pub date: u64,
    /// File (queueId) : 420 solo, 440 flex, 450 ARAM... voir `nom_file`.
    pub file: u16,
    pub champion: u16,
    /// "top", "jungle", "middle", "bottom", "utility" ; None en ARAM / Arena.
    #[serde(deserialize_with = "crate::serde_statique::option")]
    pub role: Option<Texte>,
    pub victoire: bool,
    /// Remake : partie de moins de 5 min arrêtée par un vote (`gameEndedInEarlySurrender`).
    /// Ignorée dans les stats, les moyennes, les séries et les LP.
    pub remake: bool,
    pub kills: u16,
    pub morts: u16,
    pub assists: u16,
    /// Sbires + monstres neutres.
    pub cs: u16,
    /// Durée en secondes.
    pub duree: u32,
    /// Dégâts infligés aux champions.
    pub degats: u32,
    pub vision: u16,
    pub or: u32,
    /// "16.23"
    pub patch: Box<str>,
    /// LP gagnés (+) ou perdus (-) sur cette partie, si on a pu le calculer.
    pub lp: Option<i32>,
    /// Métriques détaillées (challenges match-v5, voir metriques.rs). None : ligne
    /// d'un ancien fichier (v1), pas encore rattrapée par la synchro.
    pub metriques: Option<Metriques>,
}

impl ResumePartie {
    /// Résume une partie match-v5 du point de vue du joueur `puuid`.
    /// None si le joueur n'y est pas ou si le JSON est incomplet.
    pub fn depuis_match(partie: &Value, puuid: &str) -> Option<Self> {
        let moi = partie["info"]["participants"].as_array()?.iter().find(|p| p["puuid"] == puuid)?;
        Self::depuis_participant(partie, moi)
    }

    /// Résume une partie du point de vue d'un participant (`moi` = un élément de
    /// `info.participants`). Sert aussi à la collecte des références (10 par partie).
    pub fn depuis_participant(partie: &Value, moi: &Value) -> Option<Self> {
        let info = &partie["info"];
        // Petites fonctions locales (« closures ») pour lire un nombre sans répéter le code.
        let n = |k: &str| moi[k].as_u64().unwrap_or(0);
        let n16 = |k: &str| n(k).min(u16::MAX as u64) as u16;
        let n32 = |k: &str| n(k).min(u32::MAX as u64) as u32;
        let debut_ms = info["gameStartTimestamp"].as_u64().or_else(|| info["gameCreation"].as_u64())?;
        let position = moi["teamPosition"].as_str().unwrap_or("");
        Some(ResumePartie {
            id: partie["metadata"]["matchId"].as_str()?.into(),
            date: debut_ms / 1000,
            file: info["queueId"].as_u64().unwrap_or(0) as u16,
            champion: n16("championId"),
            role: crate::stats::ROLES.iter().copied().find(|r| r.eq_ignore_ascii_case(position)),
            victoire: moi["win"] == true,
            remake: moi["gameEndedInEarlySurrender"] == true,
            kills: n16("kills"),
            morts: n16("deaths"),
            assists: n16("assists"),
            cs: (n("totalMinionsKilled") + n("neutralMinionsKilled")).min(u16::MAX as u64) as u16,
            duree: info["gameDuration"].as_u64().unwrap_or(0) as u32,
            degats: n32("totalDamageDealtToChampions"),
            vision: n16("visionScore"),
            or: n32("goldEarned"),
            patch: crate::stats::patch_court(info["gameVersion"].as_str().unwrap_or("")).into(),
            lp: None,
            metriques: Some(Metriques::depuis_participant(moi)),
        })
    }

    pub fn famille(&self) -> Famille {
        famille(self.file)
    }

    pub fn minutes(&self) -> f32 {
        self.duree as f32 / 60.0
    }

    pub fn cs_par_min(&self) -> f32 {
        if self.duree == 0 { 0.0 } else { self.cs as f32 / self.minutes() }
    }

    /// (kills + assists) / morts ; morts = 0 compte comme 1 (convention habituelle).
    pub fn kda(&self) -> f32 {
        (self.kills + self.assists) as f32 / self.morts.max(1) as f32
    }

    /// Une ligne du fichier TSV. Format v2 : les 16 colonnes de la v1, puis (si on
    /// les a) les `metriques::NB_COLONNES` colonnes des métriques. Un ancien
    /// stat-traque (v1) lit les 16 premières et ignore le reste.
    fn vers_ligne(&self) -> String {
        let resultat = if self.remake { "R" } else if self.victoire { "V" } else { "D" };
        let lp = self.lp.map_or("-".to_string(), |l| l.to_string());
        let metriques = self.metriques.map(|m| format!("\t{}", m.vers_colonnes())).unwrap_or_default();
        format!(
            "{}\t{}\t{}\t{}\t{}\t{resultat}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{lp}{metriques}\n",
            self.id,
            self.date,
            self.file,
            self.champion,
            self.role.unwrap_or("-"),
            self.kills,
            self.morts,
            self.assists,
            self.cs,
            self.duree,
            self.degats,
            self.vision,
            self.or,
            self.patch,
        )
    }

    /// Relit une ligne écrite par `vers_ligne` (None si elle est abîmée).
    fn depuis_ligne(ligne: &str) -> Option<Self> {
        let c: Vec<&str> = ligne.split('\t').collect();
        if c.len() < 16 {
            return None;
        }
        Some(ResumePartie {
            id: c[0].into(),
            date: c[1].parse().ok()?,
            file: c[2].parse().ok()?,
            champion: c[3].parse().ok()?,
            role: crate::stats::ROLES.iter().copied().find(|r| *r == c[4]),
            victoire: c[5] == "V",
            remake: c[5] == "R",
            kills: c[6].parse().ok()?,
            morts: c[7].parse().ok()?,
            assists: c[8].parse().ok()?,
            cs: c[9].parse().ok()?,
            duree: c[10].parse().ok()?,
            degats: c[11].parse().ok()?,
            vision: c[12].parse().ok()?,
            or: c[13].parse().ok()?,
            patch: c[14].into(),
            lp: c[15].trim().parse().ok(),
            // Colonnes 16 et suivantes : métriques (v2). Absentes en v1.
            metriques: c.get(16..).and_then(Metriques::depuis_colonnes),
        })
    }
}

// =====================================================================
// 2. Points de rang (suivi des LP)
// =====================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FileClassee {
    Solo,
    Flex,
}

impl FileClassee {
    fn texte(self) -> &'static str {
        match self {
            FileClassee::Solo => "solo",
            FileClassee::Flex => "flex",
        }
    }
    /// queueId match-v5 correspondant.
    pub fn queue(self) -> u16 {
        match self {
            FileClassee::Solo => 420,
            FileClassee::Flex => 440,
        }
    }
}

/// Mon rang dans une file, à une date donnée.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PointLp {
    pub date: u64,
    pub file: FileClassee,
    pub rang: Rang,
}

const PALIERS: [&str; 10] = [
    "IRON", "BRONZE", "SILVER", "GOLD", "PLATINUM", "EMERALD", "DIAMOND", "MASTER", "GRANDMASTER", "CHALLENGER",
];

/// Rang -> « LP absolus » sur une échelle continue, pour calculer des différences
/// même quand on change de division ou de palier.
/// Chaque division vaut 100 LP, chaque palier 400 (4 divisions) jusqu'à Diamant.
/// Master, Grandmaster et Challenger partagent la même échelle de LP (pas de division).
///   Gold I 90 LP = 3*400 + 3*100 + 90 = 1590 ; Platine IV 10 LP = 1610 -> +20.
pub fn lp_absolus(r: &Rang) -> i32 {
    let palier = PALIERS.iter().position(|p| **p == *r.palier).unwrap_or(0) as i32;
    if palier >= 7 {
        return 7 * 400 + r.lp as i32;
    }
    let division = match &*r.division {
        "IV" => 0,
        "III" => 1,
        "II" => 2,
        "I" => 3,
        _ => 0,
    };
    palier * 400 + division * 100 + r.lp as i32
}

// =====================================================================
// 3. Moyennes (stats perso)
// =====================================================================

/// Totaux d'un groupe de parties (un champion, un rôle, un patch...).
/// On garde des SOMMES : les moyennes se calculent à la demande, sans arrondi cumulé.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StatsGroupe {
    pub parties: u32,
    pub victoires: u32,
    pub kills: u32,
    pub morts: u32,
    pub assists: u32,
    pub cs: u32,
    pub secondes: u32,
    pub degats: u64,
    pub vision: u32,
    pub or: u64,
}

impl StatsGroupe {
    fn ajouter(&mut self, p: &ResumePartie) {
        self.parties += 1;
        self.victoires += p.victoire as u32;
        self.kills += p.kills as u32;
        self.morts += p.morts as u32;
        self.assists += p.assists as u32;
        self.cs += p.cs as u32;
        self.secondes += p.duree;
        self.degats += p.degats as u64;
        self.vision += p.vision as u32;
        self.or += p.or as u64;
    }

    /// Divise sans planter sur 0 partie.
    fn moyenne(&self, total: f64) -> f32 {
        if self.parties == 0 { 0.0 } else { (total / self.parties as f64) as f32 }
    }

    pub fn winrate(&self) -> f32 {
        self.moyenne(self.victoires as f64 * 100.0)
    }
    pub fn kda(&self) -> f32 {
        (self.kills + self.assists) as f32 / self.morts.max(1) as f32
    }
    /// CS/min sur l'ensemble du temps joué (une longue partie pèse plus qu'une courte).
    pub fn cs_par_min(&self) -> f32 {
        if self.secondes == 0 { 0.0 } else { self.cs as f32 * 60.0 / self.secondes as f32 }
    }
    pub fn degats_moyens(&self) -> f32 {
        self.moyenne(self.degats as f64)
    }
    pub fn vision_moyenne(&self) -> f32 {
        self.moyenne(self.vision as f64)
    }
    pub fn or_moyen(&self) -> f32 {
        self.moyenne(self.or as f64)
    }
}

/// Mes stats (les plus jouées d'abord). `total`, `par_champion`, `par_role` et
/// `par_patch` ne comptent QUE la Faille de l'invocateur ; l'ARAM, l'Arena et les
/// autres modes ont leur ligne dans `par_famille` (et `par_file`).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct StatsPerso {
    pub total: StatsGroupe,
    pub par_champion: Vec<(u16, StatsGroupe)>,
    #[serde(deserialize_with = "crate::serde_statique::paires")]
    pub par_role: Vec<(Texte, StatsGroupe)>,
    pub par_patch: Vec<(Box<str>, StatsGroupe)>,
    pub par_famille: Vec<(Famille, StatsGroupe)>,
    pub par_file: Vec<(u16, StatsGroupe)>,
}

/// Regroupe les parties selon `cle` (None = partie ignorée), tri par nombre de parties.
fn grouper<K: std::hash::Hash + Eq + Ord + Clone>(
    parties: &[ResumePartie],
    cle: impl Fn(&ResumePartie) -> Option<K>,
) -> Vec<(K, StatsGroupe)> {
    let mut table: HashMap<K, StatsGroupe> = HashMap::new();
    for p in parties.iter().filter(|p| !p.remake) {
        if let Some(k) = cle(p) {
            table.entry(k).or_default().ajouter(p);
        }
    }
    let mut v: Vec<_> = table.into_iter().collect();
    // Plus jouées d'abord ; à égalité, ordre de la clé (résultat stable pour les tests).
    v.sort_by(|a, b| b.1.parties.cmp(&a.1.parties).then_with(|| a.0.cmp(&b.0)));
    v
}

// =====================================================================
// 4. Récap d'après-partie
// =====================================================================

/// Une ligne du récap : ma valeur, et ma moyenne sur ce champion.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LigneRecap {
    #[serde(deserialize_with = "crate::serde_statique::texte")]
    pub libelle: Texte,
    pub valeur: f32,
    pub moyenne: Option<f32>,
    /// Écart en % par rapport à la moyenne (+13.0 = 13 % de mieux).
    pub ecart_pct: Option<f32>,
    /// Chiffres après la virgule à l'affichage.
    pub decimales: u8,
}

impl LigneRecap {
    fn nouvelle(libelle: &'static str, valeur: f32, moyenne: Option<f32>, decimales: u8) -> Self {
        let ecart_pct = moyenne.filter(|m| *m > 0.0).map(|m| (valeur - m) * 100.0 / m);
        Self { libelle, valeur, moyenne, ecart_pct, decimales }
    }

    /// « CS/min 7.8 (ta moyenne 6.9, +13 %) »
    pub fn texte(&self) -> String {
        let d = self.decimales as usize;
        let mut t = format!("{} {:.d$}", self.libelle, self.valeur);
        if let Some(m) = self.moyenne {
            t += &format!(" (ta moyenne {m:.d$}");
            if let Some(e) = self.ecart_pct {
                t += &format!(", {e:+.0} %");
            }
            t += ")";
        }
        t
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Recap {
    pub partie: ResumePartie,
    /// Nombre de mes AUTRES parties sur ce champion, même famille (base de la moyenne).
    pub parties_champion: u32,
    pub lignes: Vec<LigneRecap>,
}

impl Recap {
    /// Compare `partie` à mes autres parties sur le même champion ET la même famille
    /// de file (une ARAM n'est pas comparée à la Faille), remakes exclus.
    pub fn calculer(h: &Historique, partie: &ResumePartie) -> Self {
        let mut moy = StatsGroupe::default();
        let comparable = |p: &&ResumePartie| {
            p.champion == partie.champion && p.famille() == partie.famille() && p.id != partie.id && !p.remake
        };
        for p in h.parties.iter().filter(comparable) {
            moy.ajouter(p);
        }
        let assez = moy.parties >= MIN_PARTIES_MOYENNE;
        let m = |valeur: f32| assez.then_some(valeur);
        let lignes = vec![
            LigneRecap::nouvelle("KDA", partie.kda(), m(moy.kda()), 1),
            LigneRecap::nouvelle("CS/min", partie.cs_par_min(), m(moy.cs_par_min()), 1),
            LigneRecap::nouvelle("Dégâts", partie.degats as f32, m(moy.degats_moyens()), 0),
            LigneRecap::nouvelle("Vision", partie.vision as f32, m(moy.vision_moyenne()), 0),
            LigneRecap::nouvelle("Or", partie.or as f32, m(moy.or_moyen()), 0),
        ];
        Recap { partie: partie.clone(), parties_champion: moy.parties, lignes }
    }

    /// Texte du récap (pour le journal), avec le nom du champion déjà traduit.
    pub fn texte(&self, nom_champion: &str) -> Vec<String> {
        let p = &self.partie;
        let resultat = if p.remake { "Remake" } else if p.victoire { "Victoire" } else { "Défaite" };
        let role = p.role.map(|r| format!(" ({r})")).unwrap_or_default();
        let lp = p.lp.map(|l| format!(" — LP {l:+}")).unwrap_or_default();
        let mut t = vec![format!(
            "--- Récap : {resultat} — {nom_champion}{role} — {} — {}/{}/{} en {} min{lp} ---",
            nom_file(p.file),
            p.kills,
            p.morts,
            p.assists,
            p.duree / 60
        )];
        t.extend(self.lignes.iter().map(|l| format!("  {}", l.texte())));
        if self.parties_champion < MIN_PARTIES_MOYENNE {
            t.push(format!(
                "  (pas encore de moyenne sur ce champion : {} autre(s) partie(s) connue(s))",
                self.parties_champion
            ));
        }
        t
    }
}

// =====================================================================
// 5. Progression des LP
// =====================================================================

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProgressionLp {
    pub solo: Option<Rang>,
    pub flex: Option<Rang>,
    /// Meilleur rang que j'ai atteint depuis que stat-traque me suit.
    pub meilleur_solo: Option<Rang>,
    pub meilleur_flex: Option<Rang>,
    /// LP gagnés (+) / perdus (-) sur les 7 derniers jours.
    pub solo_7_jours: Option<i32>,
    pub flex_7_jours: Option<i32>,
    /// Derniers points (30 au plus PAR file), du plus ancien au plus récent : pour un graphique.
    pub points: Vec<PointLp>,
}

// =====================================================================
// 6. La base locale
// =====================================================================

/// Toute la base d'un compte. Chargée le temps d'une synchro ou d'une commande,
/// puis lâchée : rien ne reste en RAM au repos (seulement les 20 parties affichées).
#[derive(Debug, Clone, Default)]
pub struct Historique {
    /// Plus récentes d'abord.
    pub parties: Vec<ResumePartie>,
    /// Ordre chronologique (plus ancien d'abord).
    pub lp: Vec<PointLp>,
}

/// Le puuid ne contient normalement que lettres, chiffres, '-' et '_' ; on filtre
/// quand même pour ne jamais fabriquer un nom de fichier dangereux.
fn nom_compte(puuid: &str) -> String {
    puuid.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect()
}

/// Dossier de la base locale : `%APPDATA%\stat-traque\historique\`.
pub fn dossier() -> PathBuf {
    let d = crate::stockage::dossier_app().join("historique");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Écriture « atomique » : fichier temporaire puis renommage (jamais de fichier à moitié écrit).
fn ecrire_atomique(chemin: &Path, texte: &str) -> std::io::Result<()> {
    let temporaire = chemin.with_extension("tmp");
    std::fs::write(&temporaire, texte)?;
    std::fs::rename(&temporaire, chemin)
}

impl Historique {
    fn chemins(dossier: &Path, puuid: &str) -> (PathBuf, PathBuf) {
        let c = nom_compte(puuid);
        (dossier.join(format!("parties-{c}.tsv")), dossier.join(format!("lp-{c}.tsv")))
    }

    /// Lit la base ; fichiers absents = base vide. Les lignes abîmées sont ignorées.
    pub fn charger(dossier: &Path, puuid: &str) -> Self {
        let (f_parties, f_lp) = Self::chemins(dossier, puuid);
        let texte = std::fs::read_to_string(f_parties).unwrap_or_default();
        let mut parties: Vec<ResumePartie> =
            texte.lines().filter(|l| !l.starts_with('#')).filter_map(ResumePartie::depuis_ligne).collect();
        parties.sort_by_key(|p| std::cmp::Reverse(p.date));
        let texte = std::fs::read_to_string(f_lp).unwrap_or_default();
        let lp = texte
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| {
                let c: Vec<&str> = l.split('\t').collect();
                let file = match *c.get(1)? {
                    "solo" => FileClassee::Solo,
                    "flex" => FileClassee::Flex,
                    _ => return None,
                };
                let rang = Rang {
                    palier: (*c.get(2)?).into(),
                    division: (*c.get(3)?).into(),
                    lp: c.get(4)?.parse().ok()?,
                    victoires: c.get(5)?.parse().ok()?,
                    defaites: c.get(6)?.parse().ok()?,
                    serie: false,
                };
                Some(PointLp { date: c[0].parse().ok()?, file, rang })
            })
            .collect();
        Historique { parties, lp }
    }

    pub fn sauver(&self, dossier: &Path, puuid: &str) -> std::io::Result<()> {
        let (f_parties, f_lp) = Self::chemins(dossier, puuid);
        let mut texte = format!("{ENTETE_PARTIES}\n");
        texte.extend(self.parties.iter().map(ResumePartie::vers_ligne));
        ecrire_atomique(&f_parties, &texte)?;
        let mut texte = format!("{ENTETE_LP}\n");
        for p in &self.lp {
            let r = &p.rang;
            texte += &format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                p.date,
                p.file.texte(),
                r.palier,
                r.division,
                r.lp,
                r.victoires,
                r.defaites
            );
        }
        ecrire_atomique(&f_lp, &texte)
    }

    pub fn contient(&self, id: &str) -> bool {
        self.parties.iter().any(|p| &*p.id == id)
    }

    /// Ajoute une partie à sa place (tri par date), sans doublon.
    pub fn ajouter(&mut self, p: ResumePartie) {
        if self.contient(&p.id) {
            return;
        }
        let place = self.parties.iter().position(|x| x.date < p.date).unwrap_or(self.parties.len());
        self.parties.insert(place, p);
    }

    fn dernier_point(&self, file: FileClassee) -> Option<&PointLp> {
        self.lp.iter().rev().find(|p| p.file == file)
    }

    /// Note mon rang actuel dans une file (seulement s'il a changé).
    /// Si UNE seule partie a été jouée depuis le point précédent, on connaît ses LP :
    /// on les inscrit sur la partie correspondante et on les renvoie.
    pub fn noter_rang(&mut self, file: FileClassee, rang: Rang, date: u64) -> Option<i32> {
        let precedent = self.dernier_point(file).cloned();
        if let Some(prec) = &precedent {
            let meme = prec.rang.palier == rang.palier
                && prec.rang.division == rang.division
                && prec.rang.lp == rang.lp
                && prec.rang.victoires == rang.victoires
                && prec.rang.defaites == rang.defaites;
            if meme {
                return None;
            }
        }
        let delta = precedent.as_ref().and_then(|prec| {
            let avant = prec.rang.victoires as u32 + prec.rang.defaites as u32;
            let apres = rang.victoires as u32 + rang.defaites as u32;
            (apres == avant + 1).then(|| lp_absolus(&rang) - lp_absolus(&prec.rang))
        });
        if let (Some(d), Some(prec)) = (delta, &precedent) {
            // Les parties candidates : même file, pas un remake, LP pas encore notés,
            // terminées APRÈS le point précédent. On n'écrit les LP que s'il y en a
            // EXACTEMENT une : sinon on ne sait pas laquelle, et un LP faux est pire
            // qu'un LP absent. (Si Riot a publié le rang avant la partie : zéro candidate.)
            let mut candidates: Vec<&mut ResumePartie> = self
                .parties
                .iter_mut()
                .filter(|p| {
                    p.file == file.queue() && !p.remake && p.lp.is_none() && p.date + p.duree as u64 >= prec.date
                })
                .collect();
            if let [p] = candidates.as_mut_slice() {
                p.lp = Some(d);
            }
        }
        self.lp.push(PointLp { date, file, rang });
        delta
    }

    /// Série en cours dans ces files (remakes ignorés) : +3 = 3 victoires d'affilée,
    /// -2 = 2 défaites. Une série d'ARAM ne dit rien de ta forme en classée.
    pub fn serie(&self, files: &[u16]) -> i32 {
        let mut jouees = self.parties.iter().filter(|p| !p.remake && files.contains(&p.file));
        let Some(premiere) = jouees.next() else { return 0 };
        let n = 1 + jouees.take_while(|p| p.victoire == premiere.victoire).count() as i32;
        if premiere.victoire { n } else { -n }
    }

    pub fn stats(&self) -> StatsPerso {
        let p = &self.parties;
        // `then_some` : Some(valeur) seulement si la condition est vraie -> les parties
        // hors Faille sont ignorées dans ces regroupements.
        let faille = |x: &ResumePartie| x.famille() == Famille::Faille;
        StatsPerso {
            total: grouper(p, |x| faille(x).then_some(())).first().map(|x| x.1).unwrap_or_default(),
            par_champion: grouper(p, |x| faille(x).then_some(x.champion)),
            par_role: grouper(p, |x| x.role.filter(|_| faille(x))),
            par_patch: grouper(p, |x| faille(x).then(|| x.patch.clone())),
            par_famille: grouper(p, |x| Some(x.famille())),
            par_file: grouper(p, |x| Some(x.file)),
        }
    }

    pub fn progression(&self, maintenant: u64) -> ProgressionLp {
        let meilleur = |f: FileClassee| {
            self.lp.iter().filter(|p| p.file == f).map(|p| &p.rang).max_by_key(|r| lp_absolus(r)).cloned()
        };
        // LP actuels - LP au début de la fenêtre de 7 jours (dernier point AVANT la fenêtre,
        // sinon le premier point dedans).
        let sept_jours = |f: FileClassee| {
            let debut = maintenant.saturating_sub(7 * 24 * 3600);
            let points: Vec<&PointLp> = self.lp.iter().filter(|p| p.file == f).collect();
            let actuel = points.last()?;
            let reference = points.iter().rev().find(|p| p.date < debut).or_else(|| points.first())?;
            Some(lp_absolus(&actuel.rang) - lp_absolus(&reference.rang))
        };
        // 30 derniers points de chaque file, puis remis dans l'ordre des dates.
        let derniers = |f: FileClassee| {
            let points: Vec<&PointLp> = self.lp.iter().filter(|p| p.file == f).collect();
            points[points.len().saturating_sub(POINTS_PAR_FILE)..].to_vec()
        };
        let mut points: Vec<PointLp> =
            derniers(FileClassee::Solo).into_iter().chain(derniers(FileClassee::Flex)).cloned().collect();
        points.sort_by_key(|p| p.date); // tri stable : l'ordre d'arrivée est gardé à égalité
        ProgressionLp {
            solo: self.dernier_point(FileClassee::Solo).map(|p| p.rang.clone()),
            flex: self.dernier_point(FileClassee::Flex).map(|p| p.rang.clone()),
            meilleur_solo: meilleur(FileClassee::Solo),
            meilleur_flex: meilleur(FileClassee::Flex),
            solo_7_jours: sept_jours(FileClassee::Solo),
            flex_7_jours: sept_jours(FileClassee::Flex),
            points,
        }
    }
}

// =====================================================================
// 7. Synchronisation avec l'API Riot
// =====================================================================

/// Ce dont la synchro a besoin chez Riot. Un « trait » = une liste de fonctions
/// qu'un type promet de fournir : `RiotApi` en vrai, un faux en test (sans réseau).
pub trait SourceRiot {
    /// Ids des parties commencées après `depuis` (0 = toutes), plus récentes d'abord.
    fn ids_parties(&self, puuid: &str, depuis: u64, nombre: u8) -> Result<Vec<String>, ErreurApi>;
    /// JSON match-v5 d'une partie (NON gardé dans le cache : on n'en garde que le résumé).
    fn partie(&self, id: &str) -> Result<Value, ErreurApi>;
    /// Entrées league-v4 de ce joueur, fraîches (pas le cache de 10 min).
    fn rangs(&self, puuid: &str) -> Result<Value, ErreurApi>;
}

impl SourceRiot for RiotApi {
    fn ids_parties(&self, puuid: &str, depuis: u64, nombre: u8) -> Result<Vec<String>, ErreurApi> {
        let v = self.ids_parties_depuis(puuid, depuis, nombre)?;
        Ok(v.as_array().into_iter().flatten().filter_map(|i| i.as_str().map(String::from)).collect())
    }
    fn partie(&self, id: &str) -> Result<Value, ErreurApi> {
        self.partie_sans_cache(id)
    }
    fn rangs(&self, puuid: &str) -> Result<Value, ErreurApi> {
        self.rangs_frais(puuid)
    }
}

#[derive(Debug)]
pub struct ResultatSynchro {
    pub historique: Historique,
    /// Ids des parties ajoutées pendant cette synchro.
    pub nouvelles: Vec<Box<str>>,
    /// Erreur qui a interrompu la synchro (ce qui était fait est sauvé quand même).
    pub erreur: Option<ErreurApi>,
}

/// Télécharge les parties nouvelles et note le rang. Sauve après CHAQUE partie :
/// si l'app est fermée au milieu, on reprend là où on en était.
/// `toujours_le_rang` = false : on ne demande le rang que si une partie est arrivée
/// (juste après une partie, Riot peut publier les LP AVANT la partie : on les
/// attribuerait alors à rien, et ça économise 1 requête par essai).
pub fn synchroniser(
    source: &impl SourceRiot,
    dossier: &Path,
    puuid: &str,
    pause: Duration,
    maintenant: u64,
    toujours_le_rang: bool,
) -> ResultatSynchro {
    let mut h = Historique::charger(dossier, puuid);
    let mut nouvelles = Vec::new();
    // On repart de la dernière partie connue (Riot filtre par date de début).
    let depuis = h.parties.first().map_or(0, |p| p.date);
    let ids = match source.ids_parties(puuid, depuis, PREMIERE_SYNCHRO) {
        Ok(ids) => ids,
        Err(e) => return ResultatSynchro { historique: h, nouvelles, erreur: Some(e) },
    };
    let mut erreur = None;
    // Des plus anciennes aux plus récentes : une synchro interrompue ne laisse pas de « trou ».
    // (On fait la liste d'abord : `h` sera modifié dans la boucle.)
    let a_telecharger: Vec<&String> = ids.iter().rev().filter(|id| !h.contient(id)).collect();
    for (i, id) in a_telecharger.into_iter().enumerate() {
        if i > 0 {
            std::thread::sleep(pause);
        }
        match source.partie(id) {
            Ok(json) => {
                if let Some(r) = ResumePartie::depuis_match(&json, puuid) {
                    nouvelles.push(r.id.clone());
                    h.ajouter(r);
                    let _ = h.sauver(dossier, puuid);
                }
            }
            Err(ErreurApi::Introuvable) => {} // partie effacée chez Riot : on passe
            Err(e) => {
                erreur = Some(e);
                break;
            }
        }
    }
    // Rattrapage des métriques des parties d'avant la v2 (Faille seulement, les plus
    // récentes d'abord, `RATTRAPAGE_MAX` au plus) : 1 requête par partie.
    if erreur.is_none() {
        erreur = rattraper_metriques(source, &mut h, dossier, puuid, pause, !nouvelles.is_empty()).err();
    }
    // Le rang APRÈS les parties : les LP de la dernière partie s'inscrivent dessus.
    if erreur.is_none() && (toujours_le_rang || !nouvelles.is_empty()) {
        erreur = noter_rangs(source, &mut h, puuid, maintenant).err();
    }
    let _ = h.sauver(dossier, puuid);
    ResultatSynchro { historique: h, nouvelles, erreur }
}

/// Re-télécharge les parties sans métriques (voir `RATTRAPAGE_MAX`). Une partie
/// effacée chez Riot (404) est marquée « absentes » : on ne la redemandera plus.
fn rattraper_metriques(
    source: &impl SourceRiot,
    h: &mut Historique,
    dossier: &Path,
    puuid: &str,
    pause: Duration,
    deja_telecharge: bool,
) -> Result<(), ErreurApi> {
    let ids: Vec<Box<str>> = h
        .parties
        .iter()
        // Seulement parmi les `RATTRAPAGE_MAX` parties de Faille les plus récentes : ce
        // sont celles du radar ; remonter plus loin coûterait des requêtes pour rien.
        .filter(|p| p.famille() == Famille::Faille && !p.remake)
        .take(RATTRAPAGE_MAX)
        .filter(|p| p.metriques.is_none())
        .map(|p| p.id.clone())
        .collect();
    for (i, id) in ids.iter().enumerate() {
        if i > 0 || deja_telecharge {
            std::thread::sleep(pause);
        }
        let metriques = match source.partie(id) {
            Ok(json) => ResumePartie::depuis_match(&json, puuid).and_then(|r| r.metriques),
            Err(ErreurApi::Introuvable) => None,
            Err(e) => return Err(e),
        };
        if let Some(p) = h.parties.iter_mut().find(|p| p.id == *id) {
            p.metriques = Some(metriques.unwrap_or_else(Metriques::absentes));
        }
        let _ = h.sauver(dossier, puuid);
    }
    Ok(())
}

/// Demande mon rang (1 requête) et le note dans la base (sans la sauver).
fn noter_rangs(source: &impl SourceRiot, h: &mut Historique, puuid: &str, maintenant: u64) -> Result<(), ErreurApi> {
    let (solo, flex) = crate::rangs::lire_rangs(&source.rangs(puuid)?);
    if let Some(s) = solo {
        h.noter_rang(FileClassee::Solo, s, maintenant);
    }
    if let Some(f) = flex {
        h.noter_rang(FileClassee::Flex, f, maintenant);
    }
    Ok(())
}

// =====================================================================
// 8. Orchestration : thread de fond, publication à l'affichage
// =====================================================================

/// Ce que la synchro envoie au cerveau (qui l'affiche avec les noms des champions).
#[derive(Debug, Clone, Default)]
pub struct MajHistorique {
    pub dernieres: Vec<ResumePartie>,
    pub stats: StatsPerso,
    pub progression: ProgressionLp,
    pub recap: Option<Recap>,
    pub nouvelles: usize,
    /// Coût réseau mesuré de la synchro.
    pub requetes: Compteurs,
}

/// Une seule synchro à la fois (elles écrivent les mêmes fichiers).
static EN_COURS: AtomicBool = AtomicBool::new(false);
/// Demandée pendant qu'une autre tournait : on la refait à la fin.
static A_REFAIRE: AtomicBool = AtomicBool::new(false);
/// Une des demandes en attente venait d'une fin de partie (récap voulu).
static RECAP_VOULU: AtomicBool = AtomicBool::new(false);
/// Compte de la DERNIÈRE demande : relu à chaque tour de la boucle de synchro, pour
/// qu'une synchro refaite après un changement de compte vise le bon compte.
static PUUID_DEMANDE: Mutex<Option<Box<str>>> = Mutex::new(None);

/// Lance une synchro en arrière-plan. `apres_partie` : on attend que Riot publie la
/// partie (quelques dizaines de secondes) puis on prépare le récap.
pub fn lancer_synchro(api: Arc<RiotApi>, puuid: Box<str>, apres_partie: bool, tx: Sender<Evenement>) {
    *PUUID_DEMANDE.lock().unwrap() = Some(puuid);
    if apres_partie {
        RECAP_VOULU.store(true, Ordering::SeqCst);
    }
    if EN_COURS.swap(true, Ordering::SeqCst) {
        A_REFAIRE.store(true, Ordering::SeqCst);
        return;
    }
    let _ = std::thread::Builder::new().name("historique".into()).spawn(move || {
        let dossier = dossier();
        loop {
            A_REFAIRE.store(false, Ordering::SeqCst);
            let recap = RECAP_VOULU.swap(false, Ordering::SeqCst);
            // `clone()` : on relâche le verrou tout de suite (la synchro dure longtemps).
            let Some(puuid) = PUUID_DEMANDE.lock().unwrap().clone() else { break };
            let _ = std::fs::write(dossier.join("dernier-compte.txt"), &*puuid);
            if let Some(maj) = une_synchro(&api, &dossier, &puuid, recap) {
                let _ = tx.send(Evenement::Historique(Box::new(maj)));
            }
            if A_REFAIRE.load(Ordering::SeqCst) {
                continue;
            }
            EN_COURS.store(false, Ordering::SeqCst);
            // Une demande a pu arriver entre les deux lignes ci-dessus : on la reprend,
            // sauf si un autre thread l'a déjà fait (`swap` renvoie l'ancienne valeur).
            if !A_REFAIRE.load(Ordering::SeqCst) || EN_COURS.swap(true, Ordering::SeqCst) {
                return;
            }
        }
        // Sortie par `break` (aucun compte demandé) : on libère la place.
        EN_COURS.store(false, Ordering::SeqCst);
    });
}

/// Après une partie, Riot met de quelques secondes à quelques minutes à publier le
/// résultat : on réessaie à ces moments-là (secondes d'attente avant chaque essai).
/// Coût : 1 requête par essai sans résultat, 3 pour l'essai qui trouve la partie.
const ESSAIS_APRES_PARTIE: [u64; 4] = [20, 40, 60, 180];
/// Si la partie est arrivée mais pas encore ses LP : un dernier essai (1 requête).
const ATTENTE_LP: u64 = 60;

fn une_synchro(api: &RiotApi, dossier: &Path, puuid: &str, recap: bool) -> Option<MajHistorique> {
    let avant = Compteurs::maintenant();
    let attentes: &[u64] = if recap { &ESSAIS_APRES_PARTIE } else { &[0] };
    let mut resultat = None;
    for (i, &attente) in attentes.iter().enumerate() {
        std::thread::sleep(Duration::from_secs(attente));
        // Au dernier essai, on note le rang même sans partie nouvelle.
        let dernier = i + 1 == attentes.len();
        let r = synchroniser(api, dossier, puuid, PAUSE_ENTRE_PARTIES, maintenant(), !recap || dernier);
        let trouve = !r.nouvelles.is_empty();
        let bloque = matches!(r.erreur, Some(ErreurApi::CleInvalide | ErreurApi::PasDeCle));
        resultat = Some(r);
        if trouve || bloque {
            break;
        }
    }
    let mut r = resultat?;
    // Partie classée arrivée, mais Riot n'a pas encore compté ses LP : on redemande le rang.
    let lp_manquants = r.historique.parties.first().is_some_and(|p| {
        recap && r.nouvelles.contains(&p.id) && [420, 440].contains(&p.file) && !p.remake && p.lp.is_none()
    });
    if lp_manquants && r.erreur.is_none() {
        std::thread::sleep(Duration::from_secs(ATTENTE_LP));
        r.erreur = noter_rangs(api, &mut r.historique, puuid, maintenant()).err();
        let _ = r.historique.sauver(dossier, puuid);
    }
    if let Some(e) = &r.erreur {
        log!("Historique : synchro incomplète ({e}).");
    }
    let h = &r.historique;
    // Récap : la partie la plus récente, si elle vient d'arriver et a fini il y a < 1 h.
    let recap = h
        .parties
        .first()
        .filter(|p| recap && r.nouvelles.contains(&p.id))
        .filter(|p| p.date + p.duree as u64 + 3600 >= maintenant())
        .map(|p| Recap::calculer(h, p));
    Some(MajHistorique {
        dernieres: h.parties.iter().take(PARTIES_AFFICHEES).cloned().collect(),
        stats: h.stats(),
        progression: h.progression(maintenant()),
        recap,
        nouvelles: r.nouvelles.len(),
        requetes: Compteurs::depuis(avant),
    })
}

/// Appelé par le cerveau : journal + état d'affichage.
pub fn publier(maj: MajHistorique, donnees: Option<&crate::ddragon::DonneesJeu>) {
    let nom = |c: u16| donnees.map_or(c.to_string(), |d| d.nom_champion(c).to_string());
    if maj.nouvelles > 0 {
        log!("Historique : {} partie(s) ajoutée(s) ({}).", maj.nouvelles, maj.requetes);
    }
    if let Some(r) = &maj.recap {
        for ligne in r.texte(&nom(r.partie.champion)) {
            log!("{ligne}");
        }
    }
    // Texte prêt à coller sur Discord (F24), pour le bouton « Copier » de l'interface.
    let partage = maj.recap.as_ref().map(|r| crate::fiche::recap_discord(r, &nom(r.partie.champion), None));
    affichage::modifier(move |e| {
        e.historique = std::sync::Arc::new(maj.dernieres);
        e.stats_perso = Some(std::sync::Arc::new(maj.stats));
        e.progression_lp = Some(std::sync::Arc::new(maj.progression));
        if partage.is_some() {
            e.recap_partage = partage;
        }
        if maj.recap.is_some() {
            e.recap = maj.recap;
        }
    });
}

/// Série en cours d'après la base locale (0 requête) : sert aux badges de série de
/// MON joueur. `file` : la file de la partie en cours si on la connaît (spectator),
/// sinon on regarde les classées (solo + flex).
pub fn serie_locale(puuid: &str, file: Option<u16>) -> i32 {
    let files: Vec<u16> = file.map_or(vec![420, 440], |f| vec![f]);
    Historique::charger(&dossier(), puuid).serie(&files)
}

// =====================================================================
// 9. Commande console : `stat-traque historique`
// =====================================================================

/// Affiche la base locale (aucune requête réseau). La base est remplie par l'app.
pub fn commande() {
    let dossier = dossier();
    let Ok(puuid) = std::fs::read_to_string(dossier.join("dernier-compte.txt")) else {
        println!("Pas encore d'historique : lance l'app avec le client LoL ouvert (et une clé API).");
        return;
    };
    let h = Historique::charger(&dossier, puuid.trim());
    let donnees = crate::ddragon::charger();
    let nom = |c: u16| donnees.as_ref().map_or(c.to_string(), |d| d.nom_champion(c).to_string());
    for ligne in texte_historique(&h, &nom, maintenant()) {
        println!("{ligne}");
    }
}

/// Le texte de la commande (séparé pour être testé).
fn texte_historique(h: &Historique, nom: &dyn Fn(u16) -> String, maintenant: u64) -> Vec<String> {
    let mut t = Vec::new();
    let s = h.stats();
    let ligne_stats = |libelle: &str, g: &StatsGroupe| {
        format!(
            "  {libelle:<16} {:>4} parties  {:>5.1} % V  KDA {:>4.1}  {:>4.1} cs/min  {:>6.0} dégâts  {:>3.0} vision",
            g.parties,
            g.winrate(),
            g.kda(),
            g.cs_par_min(),
            g.degats_moyens(),
            g.vision_moyenne()
        )
    };
    t.push(format!("=== Historique : {} parties (remakes exclus des stats) ===", h.parties.len()));
    t.push(ligne_stats("Total Faille", &s.total));
    let p = h.progression(maintenant);
    for (libelle, actuel, meilleur, semaine) in [
        ("Solo/duo", &p.solo, &p.meilleur_solo, p.solo_7_jours),
        ("Flex", &p.flex, &p.meilleur_flex, p.flex_7_jours),
    ] {
        if let Some(a) = actuel {
            let m = meilleur.as_ref().map(|m| format!(" · meilleur {m}")).unwrap_or_default();
            let s7 = semaine.map(|d| format!(" · {d:+} LP sur 7 jours")).unwrap_or_default();
            t.push(format!("  {libelle} : {a}{m}{s7}"));
        }
    }
    t.push("--- Par champion (Faille) ---".into());
    t.extend(s.par_champion.iter().take(15).map(|(c, g)| ligne_stats(&nom(*c), g)));
    t.push("--- Par rôle ---".into());
    t.extend(s.par_role.iter().map(|(r, g)| ligne_stats(r, g)));
    t.push("--- Par patch ---".into());
    t.extend(s.par_patch.iter().take(5).map(|(pa, g)| ligne_stats(pa, g)));
    t.push("--- Par famille ---".into());
    t.extend(s.par_famille.iter().map(|(f, g)| ligne_stats(f.libelle(), g)));
    t.push("--- Par file ---".into());
    t.extend(s.par_file.iter().map(|(f, g)| ligne_stats(nom_file(*f), g)));
    t.push(format!("--- {} dernières parties ---", PARTIES_AFFICHEES.min(h.parties.len())));
    for p in h.parties.iter().take(PARTIES_AFFICHEES) {
        let resultat = if p.remake { "R" } else if p.victoire { "V" } else { "D" };
        let lp = p.lp.map(|l| format!(" {l:+} LP")).unwrap_or_default();
        t.push(format!(
            "  {resultat} {:<14} {:<17} {:>2}/{:>2}/{:>2}  {:>4.1} cs/min  {:>2} min  {}{lp}",
            nom(p.champion),
            nom_file(p.file),
            p.kills,
            p.morts,
            p.assists,
            p.cs_par_min(),
            p.duree / 60,
            p.patch
        ));
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const MOI: &str = "puuid-moi";

    /// Construit un JSON match-v5 minimal, comme Riot le renvoie.
    fn match_json(id: &str, debut: u64, file: u16, champion: u16, victoire: bool, (k, d, a): (u16, u16, u16)) -> Value {
        serde_json::json!({
            "metadata": {"matchId": id},
            "info": {
                "gameStartTimestamp": debut * 1000,
                "gameDuration": 1800,
                "queueId": file,
                "gameVersion": "16.23.725.1234",
                "participants": [
                    {"puuid": "autre", "championId": 1, "win": !victoire},
                    {"puuid": MOI, "championId": champion, "teamPosition": "MIDDLE", "win": victoire,
                     "kills": k, "deaths": d, "assists": a, "totalMinionsKilled": 200, "neutralMinionsKilled": 10,
                     "totalDamageDealtToChampions": 25000, "visionScore": 30, "goldEarned": 12000,
                     "gameEndedInEarlySurrender": false}
                ]
            }
        })
    }

    fn rang(palier: &str, division: &str, lp: u16, v: u16, d: u16) -> Rang {
        Rang { palier: palier.into(), division: division.into(), lp, victoires: v, defaites: d, serie: false }
    }

    fn dossier_test(nom: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("stat-traque-histo-{nom}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn resume_une_partie_match_v5() {
        let r = ResumePartie::depuis_match(&match_json("EUW1_1", 1_000, 420, 103, true, (8, 2, 10)), MOI).unwrap();
        assert_eq!(&*r.id, "EUW1_1");
        assert_eq!((r.date, r.file, r.champion), (1_000, 420, 103));
        assert_eq!(r.role, Some("middle"));
        assert!(r.victoire && !r.remake);
        assert_eq!(r.cs, 210);
        assert_eq!(r.cs_par_min(), 7.0);
        assert_eq!(r.kda(), 9.0);
        assert_eq!(&*r.patch, "16.23");
        // Joueur absent de la partie : pas de résumé.
        assert!(ResumePartie::depuis_match(&match_json("EUW1_1", 1, 420, 1, true, (0, 0, 0)), "inconnu").is_none());
    }

    #[test]
    fn ligne_tsv_aller_retour() {
        let mut r = ResumePartie::depuis_match(&match_json("EUW1_2", 5, 450, 7, false, (1, 9, 3)), MOI).unwrap();
        r.lp = Some(-17);
        r.role = None;
        let relu = ResumePartie::depuis_ligne(r.vers_ligne().trim_end()).unwrap();
        assert_eq!(relu, r);
        assert!(ResumePartie::depuis_ligne("ligne\tabîmée").is_none());
    }

    #[test]
    fn lp_absolus_et_changements_de_division() {
        // Promotion Gold I 90 -> Platine IV 10 : +20.
        assert_eq!(lp_absolus(&rang("PLATINUM", "IV", 10, 0, 0)) - lp_absolus(&rang("GOLD", "I", 90, 0, 0)), 20);
        // Rétrogradation Emerald IV 0 -> Platine I 75 : -25.
        assert_eq!(lp_absolus(&rang("PLATINUM", "I", 75, 0, 0)) - lp_absolus(&rang("EMERALD", "IV", 0, 0, 0)), -25);
        // Diamant I 90 -> Master 12 : +22 ; Master et Grandmaster sur la même échelle.
        assert_eq!(lp_absolus(&rang("MASTER", "I", 12, 0, 0)) - lp_absolus(&rang("DIAMOND", "I", 90, 0, 0)), 22);
        assert_eq!(lp_absolus(&rang("GRANDMASTER", "I", 500, 0, 0)), lp_absolus(&rang("MASTER", "I", 500, 0, 0)));
    }

    #[test]
    fn noter_rang_inscrit_les_lp_sur_la_partie() {
        let mut h = Historique::default();
        assert_eq!(h.noter_rang(FileClassee::Solo, rang("GOLD", "I", 90, 10, 10), 1_000), None);
        // Même rang : pas de nouveau point.
        assert_eq!(h.noter_rang(FileClassee::Solo, rang("GOLD", "I", 90, 10, 10), 1_500), None);
        assert_eq!(h.lp.len(), 1);
        let mut p = ResumePartie::depuis_match(&match_json("EUW1_3", 2_000, 420, 1, true, (1, 1, 1)), MOI).unwrap();
        h.ajouter(p.clone());
        assert_eq!(h.noter_rang(FileClassee::Solo, rang("PLATINUM", "IV", 10, 11, 10), 5_000), Some(20));
        p.lp = Some(20);
        assert_eq!(h.parties[0], p);
        // Deux parties d'un coup : LP inconnus par partie.
        assert_eq!(h.noter_rang(FileClassee::Solo, rang("PLATINUM", "IV", 40, 13, 10), 9_000), None);
        let prog = h.progression(9_000);
        assert_eq!(prog.solo.unwrap().lp, 40);
        assert_eq!(prog.meilleur_solo.unwrap().lp, 40);
        assert_eq!(prog.solo_7_jours, Some(50));
        assert_eq!(prog.flex, None);
    }

    #[test]
    fn stats_par_champion_role_patch_sans_les_remakes() {
        let mut h = Historique::default();
        h.ajouter(ResumePartie::depuis_match(&match_json("A", 1, 420, 103, true, (10, 0, 5)), MOI).unwrap());
        h.ajouter(ResumePartie::depuis_match(&match_json("B", 2, 420, 103, false, (2, 4, 2)), MOI).unwrap());
        h.ajouter(ResumePartie::depuis_match(&match_json("C", 3, 450, 7, true, (1, 1, 1)), MOI).unwrap());
        let mut remake = ResumePartie::depuis_match(&match_json("D", 4, 420, 7, false, (0, 0, 0)), MOI).unwrap();
        remake.remake = true;
        h.ajouter(remake);
        assert_eq!(h.parties.iter().map(|p| &*p.id).collect::<Vec<_>>(), ["D", "C", "B", "A"]);
        let s = h.stats();
        // Le total et les champions ne comptent que la Faille (A et B), pas l'ARAM (C).
        assert_eq!(s.total.parties, 2);
        assert!(s.par_champion.iter().all(|(c, _)| *c != 7));
        assert_eq!(s.par_famille.iter().map(|(f, g)| (*f, g.parties)).collect::<Vec<_>>(), [(Famille::Faille, 2), (Famille::Aram, 1)]);
        assert_eq!(s.par_champion[0].0, 103);
        let ahri = s.par_champion[0].1;
        assert_eq!((ahri.parties, ahri.victoires), (2, 1));
        assert_eq!(ahri.winrate(), 50.0);
        assert_eq!(ahri.kda(), 19.0 / 4.0);
        assert_eq!(ahri.cs_par_min(), 7.0);
        assert_eq!(s.par_role, vec![("middle", s.total)]);
        assert_eq!(&*s.par_patch[0].0, "16.23");
        assert_eq!(s.par_file[0].0, 420);
        // Séries par file : en classée, le remake D est ignoré -> la dernière est B (défaite).
        assert_eq!(h.serie(&[420, 440]), -1);
        assert_eq!(h.serie(&[450]), 1);
        assert_eq!(h.serie(&[1700]), 0);
    }

    #[test]
    fn recap_compare_a_ma_moyenne() {
        let mut h = Historique::default();
        for (i, k) in [(1, 4), (2, 6), (3, 8)] {
            h.ajouter(ResumePartie::depuis_match(&match_json(&format!("V{i}"), i, 420, 103, true, (k, 2, 0)), MOI).unwrap());
        }
        let mut nouvelle = ResumePartie::depuis_match(&match_json("N", 10, 420, 103, true, (12, 2, 0)), MOI).unwrap();
        nouvelle.cs = 252; // 8.4 cs/min contre 7.0 de moyenne
        nouvelle.lp = Some(21);
        h.ajouter(nouvelle.clone());
        let r = Recap::calculer(&h, &nouvelle);
        assert_eq!(r.parties_champion, 3);
        let cs = r.lignes.iter().find(|l| l.libelle == "CS/min").unwrap();
        assert_eq!(cs.texte(), "CS/min 8.4 (ta moyenne 7.0, +20 %)");
        let kda = &r.lignes[0];
        assert_eq!(kda.texte(), "KDA 6.0 (ta moyenne 3.0, +100 %)");
        let texte = r.texte("Ahri");
        assert!(texte[0].contains("Victoire — Ahri (middle)"), "{}", texte[0]);
        assert!(texte[0].contains("LP +21"));
        // Champion jamais joué : pas de moyenne.
        let autre = ResumePartie { champion: 999, ..nouvelle };
        let r = Recap::calculer(&h, &autre);
        assert_eq!(r.lignes[1].texte(), "CS/min 8.4");
        assert!(r.texte("X").last().unwrap().contains("pas encore de moyenne"));
    }

    #[test]
    fn recap_ne_compare_pas_l_aram_a_la_faille() {
        let mut h = Historique::default();
        for i in 1..=3 {
            h.ajouter(ResumePartie::depuis_match(&match_json(&format!("F{i}"), i, 420, 103, true, (4, 2, 0)), MOI).unwrap());
        }
        let aram = ResumePartie::depuis_match(&match_json("A", 10, 450, 103, true, (12, 2, 30)), MOI).unwrap();
        h.ajouter(aram.clone());
        let r = Recap::calculer(&h, &aram);
        assert_eq!(r.parties_champion, 0);
        assert!(r.lignes.iter().all(|l| l.moyenne.is_none()));
        // L'Arena non plus, et une normale draft est comparée aux classées (Faille).
        assert_eq!(famille(1700), Famille::Arena);
        let draft = ResumePartie::depuis_match(&match_json("D", 11, 400, 103, true, (4, 2, 0)), MOI).unwrap();
        assert_eq!(Recap::calculer(&h, &draft).parties_champion, 3);
    }

    #[test]
    fn promotion_diamant_master_et_lp_seulement_si_une_seule_candidate() {
        let mut h = Historique::default();
        h.noter_rang(FileClassee::Solo, rang("DIAMOND", "I", 85, 50, 40), 1_000);
        h.ajouter(ResumePartie::depuis_match(&match_json("P", 2_000, 420, 1, true, (1, 1, 1)), MOI).unwrap());
        // Diamant I 85 -> Master 0 LP après une victoire : +15.
        assert_eq!(h.noter_rang(FileClassee::Solo, rang("MASTER", "I", 0, 51, 40), 5_000), Some(15));
        assert_eq!(h.parties[0].lp, Some(15));
        assert_eq!(h.progression(5_000).meilleur_solo.unwrap().palier.as_ref(), "MASTER");
        // Deux parties classées non notées depuis le point (ex : une arrivée en retard) :
        // le rang n'a compté qu'une partie, mais on ne sait pas laquelle -> rien d'écrit.
        h.ajouter(ResumePartie::depuis_match(&match_json("Q", 6_000, 420, 1, false, (1, 1, 1)), MOI).unwrap());
        h.ajouter(ResumePartie::depuis_match(&match_json("R", 9_000, 420, 1, false, (1, 1, 1)), MOI).unwrap());
        assert_eq!(h.noter_rang(FileClassee::Solo, rang("DIAMOND", "I", 80, 51, 41), 12_000), Some(-20));
        assert!(h.parties.iter().filter(|p| &*p.id != "P").all(|p| p.lp.is_none()));
    }

    #[test]
    fn trente_points_lp_par_file() {
        let mut h = Historique::default();
        for i in 0..40u16 {
            h.noter_rang(FileClassee::Solo, rang("GOLD", "II", i, i, 0), i as u64 * 10);
        }
        h.noter_rang(FileClassee::Flex, rang("SILVER", "I", 5, 1, 0), 15);
        let points = h.progression(1_000).points;
        assert_eq!(points.iter().filter(|p| p.file == FileClassee::Solo).count(), 30);
        // Le point flex, ancien, est gardé quand même (30 PAR file), et tout est trié par date.
        assert_eq!(points.iter().filter(|p| p.file == FileClassee::Flex).count(), 1);
        assert!(points.windows(2).all(|w| w[0].date <= w[1].date));
    }

    /// Faux Riot : des parties en mémoire, et compte les appels.
    #[derive(Default)]
    struct FausseSource {
        parties: Vec<Value>,
        rangs: Value,
        appels: RefCell<u32>,
        /// Ces parties répondent 404 (effacées chez Riot).
        effacees: Vec<&'static str>,
        /// Ces parties échouent (réseau coupé) tant qu'elles sont dans la liste.
        en_panne: RefCell<Vec<&'static str>>,
    }

    impl SourceRiot for FausseSource {
        fn ids_parties(&self, _: &str, depuis: u64, nombre: u8) -> Result<Vec<String>, ErreurApi> {
            *self.appels.borrow_mut() += 1;
            let mut v: Vec<&Value> = self
                .parties
                .iter()
                .filter(|p| p["info"]["gameStartTimestamp"].as_u64().unwrap() / 1000 >= depuis)
                .collect();
            v.sort_by_key(|p| std::cmp::Reverse(p["info"]["gameStartTimestamp"].as_u64()));
            Ok(v.iter().take(nombre as usize).map(|p| p["metadata"]["matchId"].as_str().unwrap().to_string()).collect())
        }
        fn partie(&self, id: &str) -> Result<Value, ErreurApi> {
            *self.appels.borrow_mut() += 1;
            if self.effacees.contains(&id) {
                return Err(ErreurApi::Introuvable);
            }
            if self.en_panne.borrow().contains(&id) {
                return Err(ErreurApi::Reseau);
            }
            self.parties.iter().find(|p| p["metadata"]["matchId"] == id).cloned().ok_or(ErreurApi::Introuvable)
        }
        fn rangs(&self, _: &str) -> Result<Value, ErreurApi> {
            *self.appels.borrow_mut() += 1;
            Ok(self.rangs.clone())
        }
    }

    fn rangs_json(lp: u16, v: u16, d: u16) -> Value {
        serde_json::json!([{"queueType":"RANKED_SOLO_5x5","tier":"GOLD","rank":"II","leaguePoints":lp,"wins":v,"losses":d}])
    }

    #[test]
    fn synchro_seulement_les_parties_nouvelles() {
        let dossier = dossier_test("synchro");
        let mut source = FausseSource {
            parties: vec![
                match_json("EUW1_1", 1_000, 420, 103, true, (5, 2, 3)),
                match_json("EUW1_2", 5_000, 450, 7, false, (3, 8, 20)),
            ],
            rangs: rangs_json(50, 10, 10),
            ..Default::default()
        };
        let r = synchroniser(&source, &dossier, MOI, Duration::ZERO, 10_000, true);
        assert!(r.erreur.is_none());
        assert_eq!(r.nouvelles.len(), 2);
        assert_eq!(*source.appels.borrow(), 4); // ids + 2 parties + rang
        // Une partie classée de plus, gagnée +18 LP.
        source.parties.push(match_json("EUW1_3", 20_000, 420, 103, true, (9, 1, 4)));
        source.rangs = rangs_json(68, 11, 10);
        *source.appels.borrow_mut() = 0;
        let r = synchroniser(&source, &dossier, MOI, Duration::ZERO, 30_000, true);
        assert_eq!(r.nouvelles, vec![Box::from("EUW1_3")]);
        assert_eq!(*source.appels.borrow(), 3); // ids + 1 partie + rang
        // Relu depuis le disque : tout est là, LP compris.
        let h = Historique::charger(&dossier, MOI);
        assert_eq!(h.parties.len(), 3);
        assert_eq!(h.parties[0].lp, Some(18));
        assert_eq!(h.lp.len(), 2);
        // Rien de neuf : 2 requêtes seulement, aucune partie retéléchargée.
        *source.appels.borrow_mut() = 0;
        let r = synchroniser(&source, &dossier, MOI, Duration::ZERO, 40_000, true);
        assert!(r.nouvelles.is_empty());
        assert_eq!(*source.appels.borrow(), 2);
        // Après une partie, sans partie nouvelle : pas même la requête de rang.
        *source.appels.borrow_mut() = 0;
        synchroniser(&source, &dossier, MOI, Duration::ZERO, 50_000, false);
        assert_eq!(*source.appels.borrow(), 1);
        let texte = texte_historique(&h, &|c| format!("champ{c}"), 40_000);
        assert!(texte.iter().any(|l| l.contains("champ103") && l.contains("+18 LP")));
        let _ = std::fs::remove_dir_all(dossier);
    }

    #[test]
    fn fichier_v1_relu_puis_metriques_rattrapees() {
        let dossier = dossier_test("v2");
        // Fichier écrit par l'ancienne version (v1, 16 colonnes, sans métriques).
        let v1 = "#stat-traque historique v1\n\
            EUW1_0\t500\t420\t103\tmiddle\tD\t1\t5\t3\t150\t1800\t9000\t20\t9000\t16.23\t-\n\
            EUW1_1\t1000\t420\t103\tmiddle\tV\t5\t2\t3\t210\t1800\t25000\t30\t12000\t16.23\t18\n";
        std::fs::write(dossier.join(format!("parties-{MOI}.tsv")), v1).unwrap();
        let h = Historique::charger(&dossier, MOI);
        assert_eq!(h.parties.len(), 2);
        assert!(h.parties.iter().all(|p| p.metriques.is_none()));
        assert_eq!(h.parties[0].lp, Some(18));
        // EUW1_0 n'existe plus chez Riot (404) ; EUW1_1 est re-téléchargée pour ses métriques.
        let source = FausseSource {
            parties: vec![match_json("EUW1_1", 1_000, 420, 103, true, (5, 2, 3))],
            rangs: rangs_json(50, 10, 10),
            effacees: vec!["EUW1_0"],
            ..Default::default()
        };
        let r = synchroniser(&source, &dossier, MOI, Duration::ZERO, 10_000, false);
        assert!(r.erreur.is_none());
        assert!(r.nouvelles.is_empty());
        assert_eq!(*source.appels.borrow(), 3); // ids + 2 parties à rattraper
        let h = Historique::charger(&dossier, MOI);
        let m1 = h.parties[0].metriques.unwrap();
        assert_eq!(m1.source, crate::metriques::Source::SansChallenges);
        assert_eq!(h.parties[1].metriques.unwrap().source, crate::metriques::Source::Absentes);
        // Rien à rattraper la fois suivante (la 404 n'est pas redemandée).
        *source.appels.borrow_mut() = 0;
        synchroniser(&source, &dossier, MOI, Duration::ZERO, 20_000, false);
        assert_eq!(*source.appels.borrow(), 1);
        // Le fichier est passé en v2, et ses 16 premières colonnes restent celles de la v1.
        let texte = std::fs::read_to_string(dossier.join(format!("parties-{MOI}.tsv"))).unwrap();
        assert!(texte.starts_with("#stat-traque historique v2\n"));
        let ligne = texte.lines().find(|l| l.starts_with("EUW1_1")).unwrap();
        assert_eq!(ligne.split('\t').count(), 16 + crate::metriques::NB_COLONNES);
        assert!(ligne.starts_with("EUW1_1\t1000\t420\t103\tmiddle\tV\t5\t2\t3\t210\t1800\t25000\t30\t12000\t16.23\t18\t"));
        let _ = std::fs::remove_dir_all(dossier);
    }

    #[test]
    fn coupure_reseau_puis_reprise_sans_doublon_et_partie_404() {

        let dossier = dossier_test("reprise");
        let source = FausseSource {
            parties: (1..=4u64).map(|i| match_json(&format!("EUW1_{i}"), i * 1_000, 420, 103, true, (1, 1, 1))).collect(),
            rangs: rangs_json(10, 4, 0),
            effacees: vec!["EUW1_2"],
            en_panne: RefCell::new(vec!["EUW1_3"]),
            ..Default::default()
        };
        // 1 OK, 2 en 404 (ignorée), 3 en panne -> arrêt ; 1 est sauvée, le rang n'est pas noté.
        let r = synchroniser(&source, &dossier, MOI, Duration::ZERO, 10_000, true);
        assert_eq!(r.erreur, Some(ErreurApi::Reseau));
        assert_eq!(r.nouvelles, vec![Box::from("EUW1_1")]);
        assert!(r.historique.lp.is_empty());
        assert_eq!(Historique::charger(&dossier, MOI).parties.len(), 1);
        // Le réseau revient : on reprend après la 1, sans la retélécharger.
        source.en_panne.borrow_mut().clear();
        *source.appels.borrow_mut() = 0;
        let r = synchroniser(&source, &dossier, MOI, Duration::ZERO, 20_000, true);
        assert!(r.erreur.is_none());
        assert_eq!(r.nouvelles, vec![Box::from("EUW1_3"), Box::from("EUW1_4")]);
        assert_eq!(*source.appels.borrow(), 5); // ids + 2 (404) + 3 + 4 + rang
        let h = Historique::charger(&dossier, MOI);
        let ids: Vec<&str> = h.parties.iter().map(|p| &*p.id).collect();
        assert_eq!(ids, ["EUW1_4", "EUW1_3", "EUW1_1"]);
        assert_eq!(h.lp.len(), 1);
        let _ = std::fs::remove_dir_all(dossier);
    }
}
